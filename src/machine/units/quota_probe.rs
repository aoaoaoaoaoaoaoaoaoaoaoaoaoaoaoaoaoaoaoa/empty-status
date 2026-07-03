use crate::units::quota::{ClaudeQuota, CodexQuota, ProbeSnapshot, QuotaProvider, QuotaProviders};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

pub(crate) const PROBE_ARG: &str = "--empty-status-quota-probe";

pub(crate) const PROVIDER_ARG: &str = "--provider";
const CODEX_APP_SERVER_TIMEOUT: Duration = Duration::from_secs(6);
const CODEX_APP_SERVER_ARGS: &[&str] = &["app-server", "--listen", "stdio://"];
const CODEX_CLIENT_NAME: &str = "empty-status-probe";

#[derive(Debug, Clone)]
pub(crate) struct ProbeArgs {
    providers: QuotaProviders,
}

impl ProbeArgs {
    pub(crate) fn parse<I, S>(args: I) -> anyhow::Result<Option<Self>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut probe = false;
        let mut providers = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg.as_ref();
            if arg == OsStr::new(PROBE_ARG) {
                probe = true;
            } else if arg == OsStr::new(PROVIDER_ARG) {
                let Some(provider) = args.next() else {
                    anyhow::bail!("{PROVIDER_ARG} requires a provider");
                };
                let raw = provider.as_ref().to_string_lossy();
                let Some(provider) = QuotaProvider::from_arg(&raw) else {
                    anyhow::bail!("unknown quota provider `{raw}`");
                };
                providers.push(provider);
            }
        }

        if !probe {
            return Ok(None);
        }

        let providers = if providers.is_empty() {
            QuotaProviders::default()
        } else {
            QuotaProviders::new(providers)?
        };
        Ok(Some(Self { providers }))
    }
}

pub(crate) async fn run(args: ProbeArgs) -> anyhow::Result<()> {
    let snapshot = probe_quota(&args.providers).await;
    let line = serde_json::to_string(&snapshot)?;
    writeln!(std::io::stdout().lock(), "{line}")?;
    Ok(())
}

pub(crate) fn run_claude_statusline() -> anyhow::Result<()> {
    let raw = read_stdin_json_object()?;
    let payload = serde_json::from_str::<ClaudeStatuslinePayload>(&raw).unwrap_or_default();
    let quota = payload.quota(utc_now());
    if let Some(quota) = quota.as_ref() {
        let _ = write_claude_cache(quota);
    }
    print!("{}", render_claude_statusline(quota.as_ref(), &payload));
    Ok(())
}

fn read_stdin_json_object() -> anyhow::Result<String> {
    let mut stdin = std::io::stdin().lock();
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while stdin.read(&mut byte)? == 1 {
        raw.push(byte[0]);
        if json_object_end(&raw) == Some(raw.len()) {
            break;
        }
    }
    Ok(String::from_utf8(raw)?)
}

fn json_object_end(raw: &[u8]) -> Option<usize> {
    let mut started = false;
    let mut string = false;
    let mut escape = false;
    let mut depth = 0_usize;

    for (index, byte) in raw.iter().copied().enumerate() {
        if !started {
            if byte.is_ascii_whitespace() {
                continue;
            }
            if byte != b'{' {
                return None;
            }
            started = true;
            depth = 1;
            continue;
        }

        if string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                string = false;
            }
            continue;
        }

        match byte {
            b'"' => string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }

    None
}

async fn probe_quota(providers: &QuotaProviders) -> ProbeSnapshot {
    let read_codex = async {
        if providers.contains(QuotaProvider::Codex) {
            read_codex_quota().await
        } else {
            None
        }
    };
    let read_claude = async {
        if providers.contains(QuotaProvider::Claude) {
            read_claude_quota().await
        } else {
            None
        }
    };
    let (codex, claude) = tokio::join!(read_codex, read_claude);
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

async fn read_claude_quota() -> Option<ClaudeQuota> {
    load_claude_cache()
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

fn percent(value: f64) -> Option<u8> {
    value
        .is_finite()
        .then(|| value.round().clamp(0.0, 100.0) as u8)
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeStatuslinePayload {
    rate_limits: Option<ClaudeStatuslineRateLimits>,
    context_window: Option<ClaudeContextWindow>,
}

impl ClaudeStatuslinePayload {
    fn quota(&self, captured_at: String) -> Option<ClaudeQuota> {
        let rate_limits = self.rate_limits.as_ref()?;
        let five_hour = rate_limits.five_hour.as_ref()?;
        let weekly = rate_limits.seven_day.as_ref()?;
        Some(ClaudeQuota {
            captured_at,
            weekly_used_percent: percent(weekly.used_percentage?)?,
            weekly_resets_at: weekly.resets_at?.into_i64()?,
            five_hour_used_percent: percent(five_hour.used_percentage?)?,
            five_hour_resets_at: five_hour.resets_at?.into_i64()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeStatuslineRateLimits {
    five_hour: Option<ClaudeStatuslineWindow>,
    seven_day: Option<ClaudeStatuslineWindow>,
}

#[derive(Debug, Deserialize)]
struct ClaudeStatuslineWindow {
    #[serde(alias = "usedPercentage")]
    used_percentage: Option<f64>,
    #[serde(alias = "resetsAt")]
    resets_at: Option<EpochSeconds>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContextWindow {
    #[serde(alias = "remainingPercentage")]
    remaining_percentage: Option<f64>,
}

fn render_claude_statusline(
    quota: Option<&ClaudeQuota>,
    payload: &ClaudeStatuslinePayload,
) -> String {
    if let Some(quota) = quota {
        return format!(
            "cc 7d {:>3}% 5h {:>3}%",
            100_u8.saturating_sub(quota.weekly_used_percent),
            100_u8.saturating_sub(quota.five_hour_used_percent)
        );
    }

    payload
        .context_window
        .as_ref()
        .and_then(|context| percent(context.remaining_percentage?))
        .map_or_else(String::new, |remaining| format!("ctx {remaining:>3}%"))
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeStatuslinePayload, CodexRateLimitsEnvelope, json_object_end, render_claude_statusline,
    };

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
    fn claude_statusline_payload_reads_session_and_week_windows() {
        let raw = r#"{
            "rate_limits": {
                "five_hour": {"used_percentage": 12, "resets_at": 1776733749},
                "seven_day": {"used_percentage": 37, "resets_at": 1776967090}
            },
            "context_window": {"remaining_percentage": 88}
        }"#;
        let parsed = serde_json::from_str::<ClaudeStatuslinePayload>(raw)
            .ok()
            .and_then(|payload| payload.quota("2026-04-20T00:00:00Z".to_string()));

        assert_eq!(
            parsed.as_ref().map(|quota| quota.five_hour_used_percent),
            Some(12)
        );
        assert_eq!(
            parsed.as_ref().map(|quota| quota.weekly_used_percent),
            Some(37)
        );
        assert_eq!(
            parsed.as_ref().map(|quota| quota.five_hour_resets_at),
            Some(1_776_733_749)
        );
        assert_eq!(
            parsed.as_ref().map(|quota| quota.weekly_resets_at),
            Some(1_776_967_090)
        );
    }

    #[test]
    fn statusline_stdin_reader_stops_after_one_json_object() {
        let raw = br#"  {"outer":{"inner":"not } done","escape":"\\"}} trailing"#;
        assert_eq!(json_object_end(raw), Some(48));
    }

    #[test]
    fn claude_statusline_renders_quota_summary() {
        let raw = r#"{
            "rate_limits": {
                "five_hour": {"used_percentage": 12, "resets_at": 1776733749},
                "seven_day": {"used_percentage": 37, "resets_at": 1776967090}
            }
        }"#;
        let payload = serde_json::from_str::<ClaudeStatuslinePayload>(raw);
        assert!(payload.is_ok());
        let Ok(payload) = payload else {
            return;
        };
        let quota = payload.quota("2026-04-20T00:00:00Z".to_string());
        assert_eq!(
            render_claude_statusline(quota.as_ref(), &payload),
            "cc 7d  63% 5h  88%"
        );
    }
}
