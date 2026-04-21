use crate::units::quota::{ClaudeQuota, CodexQuota, ProbeSnapshot};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as SyncCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

pub(crate) const PROBE_ARG: &str = "--empty-status-quota-probe";

const FORCE_ARG: &str = "--force";
const CODEX_APP_SERVER_TIMEOUT: Duration = Duration::from_secs(6);
const CODEX_APP_SERVER_ARGS: &[&str] = &["app-server", "--listen", "stdio://"];
const CODEX_CLIENT_NAME: &str = "empty-status-probe";
const CLAUDE_REFRESH_SUCCESS: Duration = Duration::from_mins(15);
const CLAUDE_BOOT_TIMEOUT: Duration = Duration::from_secs(8);
const CLAUDE_VIEW_TIMEOUT: Duration = Duration::from_secs(8);
const CLAUDE_PANE_HEIGHT: &str = "40";
const CLAUDE_PANE_WIDTH: &str = "120";
const CLAUDE_PROMPT: &str = "\u{276f}";
const CLAUDE_STATUS_COMMAND: &[&str] = &[
    "claude", "--model", "haiku", "--effort", "low", "--tools", "",
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProbeArgs {
    force_refresh: bool,
}

impl ProbeArgs {
    pub(crate) fn parse<I, S>(args: I) -> Option<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut probe = false;
        let mut force_refresh = false;
        for arg in args {
            let arg = arg.as_ref();
            if arg == OsStr::new(PROBE_ARG) {
                probe = true;
            } else if arg == OsStr::new(FORCE_ARG) {
                force_refresh = true;
            }
        }

        probe.then_some(Self { force_refresh })
    }
}

pub(crate) async fn run(args: ProbeArgs) -> anyhow::Result<()> {
    let snapshot = probe_quota(args.force_refresh).await;
    let line = serde_json::to_string(&snapshot)?;
    writeln!(std::io::stdout().lock(), "{line}")?;
    Ok(())
}

async fn probe_quota(force_refresh: bool) -> ProbeSnapshot {
    let (codex, claude) = tokio::join!(read_codex_quota(), read_claude_quota(force_refresh));
    ProbeSnapshot { codex, claude }
}

async fn read_codex_quota() -> Option<CodexQuota> {
    let mut command = Command::new("codex");
    let _ = command
        .args(CODEX_APP_SERVER_ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = command.spawn().ok()?;
    let Some(stdin) = child.stdin.take() else {
        reap_codex_child(child).await;
        return None;
    };
    let Some(stdout) = child.stdout.take() else {
        reap_codex_child(child).await;
        return None;
    };

    let quota = read_codex_quota_rpc(stdin, stdout).await;
    reap_codex_child(child).await;
    quota
}

async fn reap_codex_child(mut child: tokio::process::Child) {
    let _ = child.start_kill();
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
}

async fn read_codex_quota_rpc(stdin: ChildStdin, stdout: ChildStdout) -> Option<CodexQuota> {
    let mut rpc = JsonRpcClient::new(stdin, stdout);
    let _: serde_json::Value = rpc
        .call(
            1,
            "initialize",
            InitializeParams {
                client_info: ClientInfo {
                    name: CODEX_CLIENT_NAME,
                    version: env!("CARGO_PKG_VERSION"),
                },
                capabilities: InitializeCapabilities {
                    experimental_api: true,
                },
            },
        )
        .await?;

    let envelope: CodexRateLimitsEnvelope = rpc
        .call(2, "account/rateLimits/read", EmptyParams {})
        .await?;
    envelope.quota(utc_now())
}

#[derive(Debug)]
struct JsonRpcClient {
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

impl JsonRpcClient {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            lines: BufReader::new(stdout).lines(),
        }
    }

    async fn call<P, T>(&mut self, id: u64, method: &str, params: P) -> Option<T>
    where
        P: Serialize,
        T: DeserializeOwned,
    {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let mut encoded = serde_json::to_vec(&request).ok()?;
        encoded.push(b'\n');
        self.stdin.write_all(&encoded).await.ok()?;
        self.stdin.flush().await.ok()?;

        let deadline = Instant::now() + CODEX_APP_SERVER_TIMEOUT;
        loop {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let line = match timeout(remaining, self.lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) | Ok(Err(_)) | Err(_) => return None,
            };
            let message = serde_json::from_str::<serde_json::Value>(&line).ok()?;
            if message.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
                continue;
            }
            return serde_json::from_value::<JsonRpcResponse<T>>(message)
                .ok()?
                .result;
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams<'a> {
    client_info: ClientInfo<'a>,
    capabilities: InitializeCapabilities,
}

#[derive(Debug, Serialize)]
struct ClientInfo<'a> {
    name: &'a str,
    version: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeCapabilities {
    experimental_api: bool,
}

#[derive(Debug, Serialize)]
struct EmptyParams {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateLimitsEnvelope {
    rate_limits: Option<CodexLimitSnapshot>,
    rate_limits_by_limit_id: Option<HashMap<String, CodexLimitSnapshot>>,
}

impl CodexRateLimitsEnvelope {
    fn quota(&self, captured_at: String) -> Option<CodexQuota> {
        let snapshot = self
            .rate_limits_by_limit_id
            .as_ref()
            .and_then(|snapshots| snapshots.get("codex"))
            .or(self.rate_limits.as_ref())?;
        let weekly = snapshot.window(7 * 24 * 60)?;
        Some(CodexQuota {
            captured_at,
            weekly_used_percent: percent(weekly.used_percent?)?,
            weekly_resets_at: weekly.resets_at.and_then(EpochSeconds::into_i64),
        })
    }
}

#[derive(Debug, Deserialize)]
struct CodexLimitSnapshot {
    primary: Option<CodexRateWindow>,
    secondary: Option<CodexRateWindow>,
}

impl CodexLimitSnapshot {
    fn window(&self, minutes: u64) -> Option<&CodexRateWindow> {
        self.primary
            .as_ref()
            .into_iter()
            .chain(self.secondary.as_ref())
            .find(|window| window.window_duration_mins == Some(minutes))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexRateWindow {
    #[serde(alias = "window_minutes")]
    window_duration_mins: Option<u64>,
    #[serde(alias = "used_percent")]
    used_percent: Option<f64>,
    #[serde(alias = "resets_at")]
    resets_at: Option<EpochSeconds>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
enum EpochSeconds {
    Integer(i64),
    Float(f64),
}

impl EpochSeconds {
    fn into_i64(self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(value),
            Self::Float(value) if value.is_finite() => Some(value.round() as i64),
            Self::Float(_) => None,
        }
    }
}

async fn read_claude_quota(force_refresh: bool) -> Option<ClaudeQuota> {
    let cached = load_claude_cache();
    if !force_refresh && cached.as_ref().is_some_and(cached_claude_is_fresh) {
        return cached;
    }

    tokio::task::spawn_blocking(probe_claude_quota)
        .await
        .ok()
        .flatten()
        .or(cached)
}

fn cached_claude_is_fresh(quota: &ClaudeQuota) -> bool {
    rfc3339_age_seconds(&quota.captured_at)
        .is_some_and(|age| age < CLAUDE_REFRESH_SUCCESS.as_secs_f64())
}

fn load_claude_cache() -> Option<ClaudeQuota> {
    let raw = fs::read_to_string(claude_cache_path()?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_claude_cache(snapshot: &ClaudeQuota) -> Option<()> {
    let target = claude_cache_path()?;
    let parent = target.parent()?;
    fs::create_dir_all(parent).ok()?;
    let temp = target.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec(snapshot).ok()?).ok()?;
    fs::rename(temp, target).ok()?;
    Some(())
}

fn claude_cache_path() -> Option<PathBuf> {
    let root = nonempty_env_path("XDG_CACHE_HOME")
        .or_else(|| home_dir().map(|home| home.join(".cache")))?;
    Some(root.join("empty-status").join("claude-rate-limits.json"))
}

fn home_dir() -> Option<PathBuf> {
    nonempty_env_path("HOME")
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn probe_claude_quota() -> Option<ClaudeQuota> {
    if !command_exists("claude") || !command_exists("tmux") {
        return None;
    }

    let cwd = read_recent_claude_cwd()?;
    let session = TmuxSession::new(cwd)?;
    let command = shell_join(CLAUDE_STATUS_COMMAND);
    let new_session_args = vec![
        OsString::from("new-session"),
        OsString::from("-d"),
        OsString::from("-x"),
        OsString::from(CLAUDE_PANE_WIDTH),
        OsString::from("-y"),
        OsString::from(CLAUDE_PANE_HEIGHT),
        OsString::from(command),
    ];
    let _ = session.run(new_session_args, false)?;

    let prompt_pane =
        session.wait_for_pane(|pane| pane.contains(CLAUDE_PROMPT), CLAUDE_BOOT_TIMEOUT);
    if !prompt_pane.contains(CLAUDE_PROMPT) {
        return None;
    }

    let _ = session.run(["send-keys", "/status", "Enter"], false)?;
    let _ = session.wait_for_pane(
        |pane| pane.contains("Version:") && pane.contains("Status"),
        CLAUDE_VIEW_TIMEOUT,
    );

    let _ = session.run(["send-keys", "Right"], false)?;
    let _ = session.wait_for_pane(
        |pane| pane.contains("Search settings..."),
        CLAUDE_VIEW_TIMEOUT,
    );

    let _ = session.run(["send-keys", "Right"], false)?;
    let pane = session.wait_for_pane(
        |pane| pane.contains("Current week (all models)"),
        CLAUDE_VIEW_TIMEOUT,
    );
    let snapshot = parse_claude_usage_pane(&pane)?;
    let _ = write_claude_cache(&snapshot);
    Some(snapshot)
}

#[derive(Debug)]
struct TmuxSession {
    cwd: PathBuf,
    socket_dir: PathBuf,
    socket_path: PathBuf,
}

impl TmuxSession {
    fn new(cwd: PathBuf) -> Option<Self> {
        let socket_dir = std::env::temp_dir().join(format!("empty-status-quota-{}", nonce()));
        fs::create_dir_all(&socket_dir).ok()?;
        Some(Self {
            cwd,
            socket_path: socket_dir.join("tmux.sock"),
            socket_dir,
        })
    }

    fn run<I, S>(&self, args: I, capture: bool) -> Option<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = SyncCommand::new("tmux");
        let _ = command.arg("-S");
        let _ = command.arg(&self.socket_path);
        let _ = command.args(args);
        let _ = command.current_dir(&self.cwd);
        let _ = command.env("TMUX_TMPDIR", std::env::temp_dir());
        let _ = command.stdin(Stdio::null());
        let _ = command.stderr(Stdio::null());
        if capture {
            let _ = command.stdout(Stdio::piped());
        } else {
            let _ = command.stdout(Stdio::null());
        }

        let output = command.output().ok()?;
        if !output.status.success() {
            return None;
        }

        Some(if capture {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            String::new()
        })
    }

    fn wait_for_pane(&self, predicate: impl Fn(&str) -> bool, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        let mut last = String::new();
        while Instant::now() < deadline {
            if let Some(pane) = self.run(["capture-pane", "-p", "-S", "-"], true) {
                if predicate(&pane) {
                    return pane;
                }
                last = pane;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        last
    }
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        let mut command = SyncCommand::new("tmux");
        let _ = command.arg("-S");
        let _ = command.arg(&self.socket_path);
        let _ = command.arg("kill-server");
        let _ = command.current_dir(&self.cwd);
        let _ = command.env("TMUX_TMPDIR", std::env::temp_dir());
        let _ = command.stdin(Stdio::null());
        let _ = command.stdout(Stdio::null());
        let _ = command.stderr(Stdio::null());
        let _ = command.status();
        let _ = fs::remove_dir_all(&self.socket_dir);
    }
}

fn read_recent_claude_cwd() -> Option<PathBuf> {
    let root = home_dir()?.join(".claude").join("projects");
    newest_project_files(&root)
        .into_iter()
        .take(96)
        .find_map(|file| find_recent_cwd_in_file(&file))
}

fn newest_project_files(root: &Path) -> Vec<PathBuf> {
    let Ok(projects) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut ranked = Vec::new();
    for project in projects.flatten() {
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension() != Some(OsStr::new("jsonl")) {
                continue;
            }
            let Ok(modified) = file.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            ranked.push((Reverse(modified), path));
        }
    }
    ranked.sort_by_key(|(modified, _)| *modified);
    ranked.into_iter().map(|(_, path)| path).collect()
}

fn find_recent_cwd_in_file(path: &Path) -> Option<PathBuf> {
    const BLOCK: usize = 1 << 16;

    let mut file = File::open(path).ok()?;
    let mut offset = file.seek(SeekFrom::End(0)).ok()?;
    let mut suffix = Vec::new();
    while offset != 0 {
        let len = usize::try_from(offset.min(BLOCK as u64)).ok()?;
        offset -= len as u64;
        let _ = file.seek(SeekFrom::Start(offset)).ok()?;
        let mut chunk = vec![0; len];
        file.read_exact(&mut chunk).ok()?;
        chunk.extend_from_slice(&suffix);

        let mut lines = chunk.split(|byte| *byte == b'\n');
        suffix = lines.next().map_or_else(Vec::new, |line| line.to_vec());
        let tail = lines.collect::<Vec<_>>();
        for line in tail.into_iter().rev() {
            if let Some(cwd) = cwd_from_jsonl_line(line) {
                return Some(cwd);
            }
        }
    }

    cwd_from_jsonl_line(&suffix)
}

fn cwd_from_jsonl_line(raw: &[u8]) -> Option<PathBuf> {
    let line = std::str::from_utf8(raw).ok()?;
    if !line.contains("\"cwd\"") {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let path = PathBuf::from(value.get("cwd")?.as_str()?);
    path.is_dir().then_some(path)
}

#[cfg(unix)]
fn command_exists(command: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths)
        .map(|path| path.join(command))
        .any(|path| {
            fs::metadata(path).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
}

#[cfg(not(unix))]
fn command_exists(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths)
        .map(|path| path.join(command))
        .any(|path| fs::metadata(path).is_ok_and(|metadata| metadata.is_file()))
}

fn parse_claude_usage_pane(pane: &str) -> Option<ClaudeQuota> {
    parse_claude_usage_pane_at(pane, utc_now())
}

fn parse_claude_usage_pane_at(pane: &str, captured_at: String) -> Option<ClaudeQuota> {
    let session = parse_usage_block(pane, "Current session")?;
    let weekly = parse_usage_block(pane, "Current week (all models)")?;
    Some(ClaudeQuota {
        captured_at,
        weekly_used_percent: weekly.used_percent,
        weekly_resets_at: weekly.resets_at,
        five_hour_used_percent: session.used_percent,
        five_hour_resets_at: session.resets_at,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UsageBlock {
    used_percent: u8,
    resets_at: String,
}

fn parse_usage_block(pane: &str, title: &str) -> Option<UsageBlock> {
    let (_, after) = pane.split_once(title)?;
    Some(UsageBlock {
        used_percent: parse_percent_used(after)?,
        resets_at: parse_resets_at(after)?,
    })
}

fn parse_percent_used(text: &str) -> Option<u8> {
    text.lines()
        .take(12)
        .find_map(|line| percent_before_marker(line, "% used"))
}

fn percent_before_marker(line: &str, marker: &str) -> Option<u8> {
    let marker_at = line.find(marker)?;
    let digits = line[..marker_at]
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    percent(digits.parse::<f64>().ok()?)
}

fn parse_resets_at(text: &str) -> Option<String> {
    text.lines().take(12).find_map(|line| {
        line.trim()
            .strip_prefix("Resets ")
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(ToString::to_string)
    })
}

fn percent(value: f64) -> Option<u8> {
    value
        .is_finite()
        .then(|| value.round().clamp(0.0, 100.0) as u8)
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn rfc3339_age_seconds(raw: &str) -> Option<f64> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?;
    Utc::now()
        .signed_duration_since(parsed.with_timezone(&Utc))
        .to_std()
        .ok()
        .map(|duration| duration.as_secs_f64())
}

fn shell_join(args: &[&str]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-' | b'_'
            )
    }) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\"'\"'"))
}

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{CodexRateLimitsEnvelope, parse_claude_usage_pane_at, shell_join};

    #[test]
    fn codex_rate_limit_parser_accepts_current_app_server_shape() {
        let raw = r#"{
            "rateLimits": {
                "limitId": "codex",
                "planType": "pro",
                "primary": {"resetsAt": 1776733749, "usedPercent": 20, "windowDurationMins": 300},
                "secondary": {"resetsAt": 1776967090, "usedPercent": 64, "windowDurationMins": 10080},
                "credits": null
            },
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "planType": "pro",
                    "primary": {"resetsAt": 1776733749, "usedPercent": 20, "windowDurationMins": 300},
                    "secondary": {"resetsAt": 1776967090, "usedPercent": 64, "windowDurationMins": 10080},
                    "credits": null
                }
            }
        }"#;
        let parsed = serde_json::from_str::<CodexRateLimitsEnvelope>(raw)
            .ok()
            .and_then(|envelope| envelope.quota("2026-04-20T00:00:00Z".to_string()));

        assert_eq!(
            parsed.as_ref().map(|quota| quota.weekly_used_percent),
            Some(64)
        );
        assert_eq!(
            parsed.and_then(|quota| quota.weekly_resets_at),
            Some(1_776_967_090)
        );
    }

    #[test]
    fn claude_pane_parser_reads_session_and_week_windows() {
        let pane = "
            Status

            Current session
            Sonnet 4.5
            12% used
            Resets 5:14 PM

            Current week (all models)
            Claude Code
            37% used
            Resets Tue 9:00 AM
        ";
        let parsed = parse_claude_usage_pane_at(pane, "2026-04-20T00:00:00Z".to_string());

        assert_eq!(
            parsed.as_ref().map(|quota| quota.five_hour_used_percent),
            Some(12)
        );
        assert_eq!(
            parsed.as_ref().map(|quota| quota.weekly_used_percent),
            Some(37)
        );
        assert_eq!(
            parsed
                .as_ref()
                .map(|quota| quota.five_hour_resets_at.as_str()),
            Some("5:14 PM")
        );
        assert_eq!(
            parsed.as_ref().map(|quota| quota.weekly_resets_at.as_str()),
            Some("Tue 9:00 AM")
        );
    }

    #[test]
    fn shell_join_quotes_empty_and_unsafe_arguments() {
        assert_eq!(
            shell_join(&["claude", "--tools", "", "has space"]),
            "claude --tools '' 'has space'"
        );
    }
}
