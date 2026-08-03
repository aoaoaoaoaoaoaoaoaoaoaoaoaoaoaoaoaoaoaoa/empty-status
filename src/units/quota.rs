use std::{
    collections::HashMap,
    fmt, fs,
    future::Future,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, TimeZone, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize, de, de::DeserializeOwned};
use serde_inline_default::serde_inline_default;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

use crate::{
    core::{Button, Health, View},
    display::color_by_percent_remaining,
    probe_io::{ProbeIo, TransportError},
    render::{
        color::{GREY, VIOLET},
        markup::Markup,
    },
    units::{Cycle, FiniteCycle, MouseOrbit, ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_TIMEOUT: Duration = Duration::from_secs(8);
const CODEX_TIMEOUT: Duration = Duration::from_secs(6);
const CODEX_ARGS: [&str; 3] = ["app-server", "--listen", "stdio://"];
const OPENROUTER_CREDITS_ENDPOINT: &str = "https://openrouter.ai/api/v1/credits";
const TOKEN_LIMIT: usize = 16 * 1024;
const STATUSLINE_LIMIT: usize = 1 << 20;

cycle!(
    enum Facet {
        Remaining,
        Resets,
    }
);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde_inline_default("quota".to_owned())]
    label: String,
    #[serde(default)]
    providers: Providers,
    #[serde_inline_default(1800.0)]
    stale_after_sec: f64,
    #[serde_inline_default(86400.0)]
    error_after_sec: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase", deny_unknown_fields)]
enum ProviderConfig {
    Claude,
    Codex,
    OpenRouter { token_file: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Provider {
    Claude,
    Codex,
    OpenRouter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Providers(FiniteCycle<ProviderConfig>);

#[derive(Debug)]
pub struct Sample {
    feeds: HashMap<Provider, Feed>,
}

type Feed = Result<Snapshot, ProbeError>;

#[derive(Debug)]
struct Snapshot {
    captured_at: DateTime<Utc>,
    limits: Vec<Limit>,
}

#[derive(Debug)]
struct Limit {
    window: Option<Window>,
    remaining: Quantity,
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum Quantity {
    Percent(u8),
    Dollars { remaining: f64, capacity: f64 },
}

#[derive(Debug, Clone, Copy)]
enum Window {
    FiveHours,
    Week,
}

#[derive(Debug)]
pub struct Model {
    label: String,
    modes: MouseOrbit<Facet, Providers>,
    stale_after: Duration,
    error_after: Duration,
    latest: Option<Sample>,
}

#[derive(Debug, Clone)]
pub struct Request {
    providers: Providers,
}

pub type Reply = Result<Sample, ProbeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Fresh,
    Stale,
    Expired,
    Missing,
}

impl Default for Providers {
    fn default() -> Self {
        Self(FiniteCycle::new(
            ProviderConfig::Claude,
            [ProviderConfig::Codex],
        ))
    }
}

impl Providers {
    fn new(providers: Vec<ProviderConfig>) -> Result<Self, String> {
        let mut providers = providers.into_iter();
        let first = providers
            .next()
            .ok_or_else(|| "quota provider set cannot be empty".to_owned())?;
        let cycle = FiniteCycle::new(first, providers);
        for (index, provider) in cycle.points().iter().enumerate() {
            if cycle.points()[..index]
                .iter()
                .any(|other| other.provider() == provider.provider())
            {
                return Err(format!(
                    "duplicate quota provider `{}`",
                    provider.provider()
                ));
            }
            if let ProviderConfig::OpenRouter { token_file } = provider
                && !token_file.is_absolute()
            {
                return Err("OpenRouter token_file must be an absolute path".to_owned());
            }
        }
        Ok(Self(cycle))
    }

    fn selected(&self) -> Provider {
        self.0.focus().provider()
    }

    fn contains(&self, provider: Provider) -> bool {
        self.0
            .points()
            .iter()
            .any(|configured| configured.provider() == provider)
    }

    fn openrouter_token_file(&self) -> Option<&Path> {
        self.0.points().iter().find_map(|provider| match provider {
            ProviderConfig::OpenRouter { token_file } => Some(token_file.as_path()),
            ProviderConfig::Claude | ProviderConfig::Codex => None,
        })
    }

    fn providers(&self) -> impl Iterator<Item = Provider> + '_ {
        self.0.points().iter().map(ProviderConfig::provider)
    }
}

impl Cycle for Providers {
    fn advance(&mut self) {
        self.0.advance();
    }
}

impl<'de> Deserialize<'de> for Providers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        Self::new(Vec::<ProviderConfig>::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl ProviderConfig {
    const fn provider(&self) -> Provider {
        match self {
            Self::Claude => Provider::Claude,
            Self::Codex => Provider::Codex,
            Self::OpenRouter { .. } => Provider::OpenRouter,
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenRouter => "openrouter",
        })
    }
}

impl Provider {
    const fn sigil(self) -> &'static str {
        match self {
            Self::Claude => "cc",
            Self::Codex => "cx",
            Self::OpenRouter => "or",
        }
    }
}

impl Window {
    const fn label(self) -> &'static str {
        match self {
            Self::FiveHours => "5h",
            Self::Week => "1w",
        }
    }
}

impl Quantity {
    fn markup(self) -> Markup {
        let (text, percent) = match self {
            Self::Percent(remaining) => (format!("{remaining:>3}%"), f64::from(remaining)),
            Self::Dollars {
                remaining,
                capacity,
            } => {
                let text = if remaining < 0.0 {
                    format!("-${:.2}", -remaining)
                } else {
                    format!("${remaining:.2}")
                };
                let percent = if capacity > 0.0 {
                    remaining / capacity * 100.0
                } else {
                    0.0
                };
                (text, percent)
            }
        };
        Markup::text(text).fg(color_by_percent_remaining(percent))
    }
}

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        if config.label.is_empty() {
            return Err("quota label must not be empty".to_owned());
        }
        let stale_after = Duration::try_from_secs_f64(config.stale_after_sec)
            .map_err(|error| format!("stale_after_sec is not a duration: {error}"))?;
        let error_after = Duration::try_from_secs_f64(config.error_after_sec)
            .map_err(|error| format!("error_after_sec is not a duration: {error}"))?;
        if error_after <= stale_after {
            return Err(
                "quota ages must be finite and error_after_sec must exceed stale_after_sec"
                    .to_owned(),
            );
        }
        Ok(Self {
            label: config.label,
            modes: MouseOrbit::new(Facet::Remaining, config.providers),
            stale_after,
            error_after,
            latest: None,
        })
    }

    pub fn request(&self) -> Request {
        Request {
            providers: self.modes.right().clone(),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        match reply {
            Ok(mut sample) => {
                if let Some(previous) = self.latest.take() {
                    for (provider, feed) in previous.feeds {
                        if feed.is_ok() && !sample.feeds.get(&provider).is_some_and(Result::is_ok) {
                            let _ = sample.feeds.insert(provider, feed);
                        }
                    }
                }
                self.latest = Some(sample);
                self.render()
            }
            Err(error) => error_view(&self.label, error),
        }
    }

    pub fn click(&mut self, button: Button) -> Reaction {
        match button {
            Button::Middle => Reaction::refresh(),
            _ if self.modes.act(button) => Reaction::publish(self.render()),
            _ => Reaction::inert(),
        }
    }

    fn render(&self) -> View {
        let Some(sample) = self.latest.as_ref() else {
            return View::loading(&self.label);
        };
        let provider = self.modes.right().selected();
        let prefix = Markup::text(format!("{} ", self.label))
            + Markup::text(provider.sigil()).fg(GREY)
            + Markup::text(" ");
        let payload = match sample.feeds.get(&provider) {
            Some(Ok(snapshot)) => render_snapshot(snapshot, *self.modes.left()),
            Some(Err(_)) | None => Markup::bracketed(Markup::text("unavailable").fg(VIOLET)),
        };
        View::new(prefix + payload, self.health(sample))
    }

    fn health(&self, sample: &Sample) -> Health {
        let mut all_missing = true;
        let mut degraded = false;
        for provider in self.modes.right().providers() {
            match self.source_state(sample, provider) {
                SourceState::Expired => return Health::Error,
                SourceState::Fresh => all_missing = false,
                SourceState::Stale => {
                    all_missing = false;
                    degraded = true;
                }
                SourceState::Missing => degraded = true,
            }
        }
        if all_missing {
            Health::Error
        } else if degraded {
            Health::Degraded
        } else {
            Health::Ok
        }
    }

    fn source_state(&self, sample: &Sample, provider: Provider) -> SourceState {
        let Some(Ok(snapshot)) = sample.feeds.get(&provider) else {
            return SourceState::Missing;
        };
        let age = Utc::now()
            .signed_duration_since(snapshot.captured_at)
            .to_std()
            .unwrap_or(Duration::ZERO);
        if age > self.error_after {
            SourceState::Expired
        } else if age > self.stale_after {
            SourceState::Stale
        } else {
            SourceState::Fresh
        }
    }
}

fn render_snapshot(snapshot: &Snapshot, facet: Facet) -> Markup {
    match facet {
        Facet::Remaining => {
            if snapshot.limits.is_empty() {
                Markup::bracketed(Markup::text("no quota").fg(VIOLET))
            } else {
                Markup::join(" ", snapshot.limits.iter().map(render_remaining))
            }
        }
        Facet::Resets => {
            let resets = snapshot
                .limits
                .iter()
                .filter(|limit| limit.window.is_some())
                .map(render_reset)
                .collect::<Vec<_>>();
            if resets.is_empty() {
                Markup::bracketed(Markup::text("no reset").fg(GREY))
            } else {
                Markup::join(" ", resets)
            }
        }
    }
}

fn render_remaining(limit: &Limit) -> Markup {
    let window = limit.window.map_or_else(Markup::empty, |window| {
        Markup::text(window.label()).fg(GREY) + Markup::text(" ")
    });
    Markup::bracketed(window + limit.remaining.markup())
}

fn render_reset(limit: &Limit) -> Markup {
    let window = limit.window.map_or("", Window::label);
    let reset = limit
        .resets_at
        .and_then(|timestamp| Local.timestamp_opt(timestamp, 0).single())
        .map_or_else(
            || Markup::text("--").fg(VIOLET),
            |at| Markup::text(at.format("%a %m-%d %H:%M").to_string()),
        );
    Markup::bracketed(Markup::text(format!("{window}@")).fg(GREY) + reset)
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    let codex_enabled = request.providers.contains(Provider::Codex);
    let claude_enabled = request.providers.contains(Provider::Claude);
    let openrouter_token = request
        .providers
        .openrouter_token_file()
        .map(Path::to_path_buf);
    let codex = async {
        if codex_enabled {
            Some((
                Provider::Codex,
                sever(SOURCE_TIMEOUT, read_codex_quota()).await,
            ))
        } else {
            None
        }
    };
    let claude = async {
        if claude_enabled {
            Some((
                Provider::Claude,
                sever(SOURCE_TIMEOUT, load_claude_cache(io)).await,
            ))
        } else {
            None
        }
    };
    let openrouter = async {
        if let Some(token_file) = openrouter_token {
            Some((
                Provider::OpenRouter,
                sever(SOURCE_TIMEOUT, read_openrouter_quota(&token_file, io)).await,
            ))
        } else {
            None
        }
    };
    let (codex, claude, openrouter) = tokio::join!(codex, claude, openrouter);
    let mut feeds = HashMap::new();
    for (provider, feed) in [codex, claude, openrouter].into_iter().flatten() {
        if let Err(error) = &feed {
            tracing::warn!(%provider, %error, "quota source failed");
        }
        let _ = feeds.insert(provider, feed);
    }
    Ok(Sample { feeds })
}

async fn sever<T>(
    limit: Duration,
    future: impl Future<Output = Result<T, ProbeError>>,
) -> Result<T, ProbeError> {
    timeout(limit, future)
        .await
        .map_err(|_| ProbeError::Transport(TransportError::Timeout))?
}

async fn read_codex_quota() -> Feed {
    let mut command = Command::new("codex");
    let _ = command
        .args(CODEX_ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| ProbeError::Unit(format!("cannot start Codex app-server: {error}")))?;
    let Some(stdin) = child.stdin.take() else {
        reap(child).await;
        return Err(ProbeError::Unit("Codex app-server has no stdin".to_owned()));
    };
    let Some(stdout) = child.stdout.take() else {
        reap(child).await;
        return Err(ProbeError::Unit(
            "Codex app-server has no stdout".to_owned(),
        ));
    };
    let quota = read_codex_rpc(stdin, stdout).await;
    reap(child).await;
    quota
}

async fn reap(mut child: Child) {
    let _ = child.start_kill();
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
}

async fn read_codex_rpc(stdin: ChildStdin, stdout: ChildStdout) -> Feed {
    let mut rpc = Rpc::new(stdin, stdout);
    let _: serde_json::Value = rpc
        .call(
            1,
            "initialize",
            InitializeParams {
                client_info: ClientInfo {
                    name: "empty-status",
                    version: env!("CARGO_PKG_VERSION"),
                },
                capabilities: Capabilities {
                    experimental_api: true,
                },
            },
        )
        .await?;
    let envelope: CodexEnvelope = rpc
        .call(2, "account/rateLimits/read", EmptyParams {})
        .await?;
    envelope.snapshot(Utc::now())
}

#[derive(Debug)]
struct Rpc {
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Rpc {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            lines: BufReader::new(stdout).lines(),
        }
    }

    async fn call<P: Serialize, T: DeserializeOwned>(
        &mut self,
        id: u64,
        method: &str,
        params: P,
    ) -> Result<T, ProbeError> {
        let mut request = serde_json::to_vec(&RpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        })
        .map_err(|error| ProbeError::Unit(format!("cannot encode Codex RPC: {error}")))?;
        request.push(b'\n');
        self.stdin
            .write_all(&request)
            .await
            .map_err(TransportError::from)?;
        self.stdin.flush().await.map_err(TransportError::from)?;

        let deadline = Instant::now() + CODEX_TIMEOUT;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(ProbeError::Transport(TransportError::Timeout))?;
            let line = match timeout(remaining, self.lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => {
                    return Err(ProbeError::Unit(
                        "Codex app-server closed its output".to_owned(),
                    ));
                }
                Ok(Err(error)) => return Err(TransportError::from(error).into()),
                Err(_) => return Err(ProbeError::Transport(TransportError::Timeout)),
            };
            let value = serde_json::from_str::<serde_json::Value>(&line).map_err(|error| {
                ProbeError::Unit(format!("invalid Codex RPC response: {error}"))
            })?;
            if value.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
                continue;
            }
            let response = serde_json::from_value::<RpcResponse<T>>(value).map_err(|error| {
                ProbeError::Unit(format!("invalid Codex RPC envelope: {error}"))
            })?;
            return response.result.ok_or_else(|| {
                response.error.map_or_else(
                    || ProbeError::Unit("Codex RPC omitted its result".to_owned()),
                    |fault| {
                        ProbeError::Unit(format!("Codex RPC {}: {}", fault.code, fault.message))
                    },
                )
            });
        }
    }
}

#[derive(Debug, Serialize)]
struct RpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcFault>,
}

#[derive(Debug, Deserialize)]
struct RpcFault {
    code: i64,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams<'a> {
    client_info: ClientInfo<'a>,
    capabilities: Capabilities,
}

#[derive(Debug, Serialize)]
struct ClientInfo<'a> {
    name: &'a str,
    version: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    experimental_api: bool,
}

#[derive(Debug, Serialize)]
struct EmptyParams {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexEnvelope {
    rate_limits: Option<CodexSnapshot>,
    rate_limits_by_limit_id: Option<HashMap<String, CodexSnapshot>>,
}

impl CodexEnvelope {
    fn snapshot(&self, captured_at: DateTime<Utc>) -> Feed {
        let snapshot = self
            .rate_limits_by_limit_id
            .as_ref()
            .and_then(|snapshots| snapshots.get("codex"))
            .or(self.rate_limits.as_ref())
            .ok_or_else(|| ProbeError::Unit("Codex returned no rate-limit snapshot".to_owned()))?;
        let weekly = snapshot
            .window(7 * 24 * 60)
            .ok_or_else(|| ProbeError::Unit("Codex returned no weekly rate limit".to_owned()))?;
        let used = weekly.used_percent.and_then(percent).ok_or_else(|| {
            ProbeError::Unit("Codex returned an invalid weekly percentage".to_owned())
        })?;
        Ok(Snapshot {
            captured_at,
            limits: vec![Limit {
                window: Some(Window::Week),
                remaining: Quantity::Percent(100_u8.saturating_sub(used)),
                resets_at: weekly.resets_at.and_then(EpochSeconds::integer),
            }],
        })
    }
}

#[derive(Debug, Deserialize)]
struct CodexSnapshot {
    primary: Option<CodexWindow>,
    secondary: Option<CodexWindow>,
}

impl CodexSnapshot {
    fn window(&self, minutes: u64) -> Option<&CodexWindow> {
        self.primary
            .iter()
            .chain(self.secondary.iter())
            .find(|window| window.window_duration_mins == Some(minutes))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexWindow {
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
    fn integer(self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(value),
            Self::Float(value) if value.is_finite() => Some(value.round() as i64),
            Self::Float(_) => None,
        }
    }
}

async fn read_openrouter_quota(token_file: &Path, io: &ProbeIo) -> Feed {
    let raw = io.read(token_file).await?;
    if raw.len() > TOKEN_LIMIT {
        return Err(ProbeError::Unit(format!(
            "OpenRouter token exceeds {TOKEN_LIMIT} bytes"
        )));
    }
    let token = std::str::from_utf8(&raw)
        .map_err(|_| ProbeError::Unit("OpenRouter token is not UTF-8".to_owned()))?
        .trim();
    if token.is_empty() {
        return Err(ProbeError::Unit("OpenRouter token is empty".to_owned()));
    }
    let url = Url::parse(OPENROUTER_CREDITS_ENDPOINT)
        .map_err(|error| ProbeError::Unit(format!("invalid OpenRouter endpoint: {error}")))?;
    let body = io.get_bearer(url, token).await?;
    let envelope = serde_json::from_slice::<OpenRouterEnvelope>(&body)
        .map_err(|error| ProbeError::Unit(format!("invalid OpenRouter response: {error}")))?;
    envelope.snapshot(Utc::now())
}

#[derive(Debug, Deserialize)]
struct OpenRouterEnvelope {
    data: OpenRouterCredits,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCredits {
    total_credits: f64,
    total_usage: f64,
}

impl OpenRouterEnvelope {
    fn snapshot(self, captured_at: DateTime<Utc>) -> Feed {
        let credits = self.data;
        if !credits.total_credits.is_finite()
            || !credits.total_usage.is_finite()
            || credits.total_credits < 0.0
            || credits.total_usage < 0.0
        {
            return Err(ProbeError::Unit(
                "OpenRouter returned invalid credit totals".to_owned(),
            ));
        }
        Ok(Snapshot {
            captured_at,
            limits: vec![Limit {
                window: None,
                remaining: Quantity::Dollars {
                    remaining: credits.total_credits - credits.total_usage,
                    capacity: credits.total_credits,
                },
                resets_at: None,
            }],
        })
    }
}

pub fn run_claude_statusline() -> anyhow::Result<()> {
    let raw = read_statusline_object()?;
    let payload = serde_json::from_slice::<ClaudePayload>(&raw)?;
    let quota = payload.quota(Utc::now());
    if let Some(quota) = quota.as_ref() {
        write_claude_cache(quota)?;
    }
    write!(
        std::io::stdout().lock(),
        "{}",
        render_claude_statusline(quota.as_ref(), &payload)
    )?;
    Ok(())
}

fn read_statusline_object() -> anyhow::Result<Vec<u8>> {
    let mut input = std::io::stdin().lock();
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    let mut boundary = JsonBoundary::default();
    while raw.len() < STATUSLINE_LIMIT && input.read(&mut byte)? == 1 {
        raw.push(byte[0]);
        if boundary.push(byte[0]) {
            return Ok(raw);
        }
    }
    anyhow::bail!("Claude statusline input is incomplete or exceeds {STATUSLINE_LIMIT} bytes")
}

#[derive(Debug, Default)]
struct JsonBoundary {
    started: bool,
    string: bool,
    escape: bool,
    depth: usize,
}

impl JsonBoundary {
    fn push(&mut self, byte: u8) -> bool {
        if !self.started {
            if byte.is_ascii_whitespace() {
                return false;
            }
            if byte == b'{' {
                self.started = true;
                self.depth = 1;
            }
            return false;
        }
        if self.string {
            if self.escape {
                self.escape = false;
            } else if byte == b'\\' {
                self.escape = true;
            } else if byte == b'"' {
                self.string = false;
            }
            return false;
        }
        match byte {
            b'"' => self.string = true,
            b'{' => self.depth += 1,
            b'}' => {
                self.depth = self.depth.saturating_sub(1);
                return self.depth == 0;
            }
            _ => {}
        }
        false
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaudeQuota {
    captured_at: DateTime<Utc>,
    five_hour: Option<ClaudeLimit>,
    weekly: Option<ClaudeLimit>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaudeLimit {
    remaining_percent: u8,
    resets_at: Option<i64>,
}

impl ClaudeQuota {
    fn snapshot(&self) -> Snapshot {
        let mut limits = Vec::with_capacity(2);
        if let Some(limit) = self.five_hour.as_ref() {
            limits.push(Limit {
                window: Some(Window::FiveHours),
                remaining: Quantity::Percent(limit.remaining_percent),
                resets_at: limit.resets_at,
            });
        }
        if let Some(limit) = self.weekly.as_ref() {
            limits.push(Limit {
                window: Some(Window::Week),
                remaining: Quantity::Percent(limit.remaining_percent),
                resets_at: limit.resets_at,
            });
        }
        Snapshot {
            captured_at: self.captured_at,
            limits,
        }
    }
}

async fn load_claude_cache(io: &ProbeIo) -> Feed {
    let path = claude_cache_path()
        .ok_or_else(|| ProbeError::Unit("HOME and XDG_CACHE_HOME are both unset".to_owned()))?;
    let body = io.read(path).await?;
    let quota = serde_json::from_slice::<ClaudeQuota>(&body)
        .map_err(|error| ProbeError::Unit(format!("invalid Claude quota cache: {error}")))?;
    if quota.five_hour.is_none() && quota.weekly.is_none() {
        return Err(ProbeError::Unit(
            "Claude quota cache contains no limits".to_owned(),
        ));
    }
    Ok(quota.snapshot())
}

fn write_claude_cache(snapshot: &ClaudeQuota) -> anyhow::Result<()> {
    let target = claude_cache_path()
        .ok_or_else(|| anyhow::anyhow!("HOME and XDG_CACHE_HOME are both unset"))?;
    let parent = target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Claude cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = target.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(snapshot)?)?;
    fs::rename(temporary, target)?;
    Ok(())
}

fn claude_cache_path() -> Option<PathBuf> {
    let root = nonempty_env_path("XDG_CACHE_HOME")
        .or_else(|| nonempty_env_path("HOME").map(|home| home.join(".cache")))?;
    Some(root.join("empty-status/claude-rate-limits.json"))
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

#[derive(Debug, Default, Deserialize)]
struct ClaudePayload {
    rate_limits: Option<ClaudeRateLimits>,
    context_window: Option<ClaudeContextWindow>,
}

impl ClaudePayload {
    fn quota(&self, captured_at: DateTime<Utc>) -> Option<ClaudeQuota> {
        let limits = self.rate_limits.as_ref()?;
        let five_hour = limits.five_hour.as_ref().and_then(ClaudeWindow::limit);
        let weekly = limits.seven_day.as_ref().and_then(ClaudeWindow::limit);
        (five_hour.is_some() || weekly.is_some()).then_some(ClaudeQuota {
            captured_at,
            five_hour,
            weekly,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeRateLimits {
    five_hour: Option<ClaudeWindow>,
    seven_day: Option<ClaudeWindow>,
}

#[derive(Debug, Deserialize)]
struct ClaudeWindow {
    #[serde(alias = "usedPercentage")]
    used_percentage: Option<f64>,
    #[serde(alias = "resetsAt")]
    resets_at: Option<EpochSeconds>,
}

impl ClaudeWindow {
    fn limit(&self) -> Option<ClaudeLimit> {
        let used = percent(self.used_percentage?)?;
        Some(ClaudeLimit {
            remaining_percent: 100_u8.saturating_sub(used),
            resets_at: self.resets_at.and_then(EpochSeconds::integer),
        })
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeContextWindow {
    #[serde(alias = "remainingPercentage")]
    remaining_percentage: Option<f64>,
}

fn render_claude_statusline(quota: Option<&ClaudeQuota>, payload: &ClaudePayload) -> String {
    if let Some(quota) = quota {
        let windows = [
            quota
                .five_hour
                .as_ref()
                .map(|limit| format!("5h {:>3}%", limit.remaining_percent)),
            quota
                .weekly
                .as_ref()
                .map(|limit| format!("1w {:>3}%", limit.remaining_percent)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        return format!("cc {}", windows.join(" "));
    }
    payload
        .context_window
        .as_ref()
        .and_then(|context| percent(context.remaining_percentage?))
        .map_or_else(String::new, |remaining| format!("ctx {remaining:>3}%"))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, future, path::PathBuf, time::Duration};

    use chrono::Utc;

    use super::{
        ClaudePayload, CodexEnvelope, Config, Facet, JsonBoundary, Limit, Model,
        OpenRouterEnvelope, Provider, ProviderConfig, Providers, Quantity, Snapshot, Window,
        render_claude_statusline, sever,
    };
    use crate::{
        core::Button,
        probe_io::TransportError,
        units::{ProbeError, Reaction},
    };

    #[test]
    fn provider_set_rejects_duplicates_and_relative_secrets() {
        assert!(Providers::new(vec![ProviderConfig::Codex, ProviderConfig::Codex]).is_err());
        assert!(
            Providers::new(vec![ProviderConfig::OpenRouter {
                token_file: PathBuf::from("token"),
            }])
            .is_err()
        );
    }

    #[test]
    fn parses_current_codex_shape() {
        let raw = r#"{
            "rateLimits": {"primary": {"usedPercent": 20, "windowDurationMins": 300},
                           "secondary": {"resetsAt": 1776967090, "usedPercent": 64, "windowDurationMins": 10080}},
            "rateLimitsByLimitId": {}
        }"#;
        let snapshot = serde_json::from_str::<CodexEnvelope>(raw)
            .map_err(|error| error.to_string())
            .and_then(|envelope| {
                envelope
                    .snapshot(Utc::now())
                    .map_err(|error| error.to_string())
            });
        assert!(snapshot.is_ok());
        let Ok(snapshot) = snapshot else { return };
        assert!(matches!(
            snapshot.limits[0].remaining,
            Quantity::Percent(36)
        ));
    }

    #[test]
    fn parses_openrouter_dollars_without_fabricating_a_window() {
        let envelope = serde_json::from_str::<OpenRouterEnvelope>(
            r#"{"data":{"total_credits":100.5,"total_usage":25.75}}"#,
        );
        assert!(envelope.is_ok());
        let Ok(envelope) = envelope else { return };
        let snapshot = envelope.snapshot(Utc::now());
        assert!(snapshot.is_ok());
        let Ok(snapshot) = snapshot else { return };
        assert!(snapshot.limits[0].window.is_none());
        assert!(matches!(
            snapshot.limits[0].remaining,
            Quantity::Dollars {
                remaining: 74.75,
                capacity: 100.5,
            }
        ));
    }

    #[test]
    fn boundary_ignores_braces_inside_strings() {
        let mut boundary = JsonBoundary::default();
        let input = br#" {"outer":{"text":"not } done"}} trailing"#;
        let end = input
            .iter()
            .position(|byte| boundary.push(*byte))
            .map(|index| index + 1);
        assert_eq!(end, Some(32));
    }

    #[test]
    fn renders_partial_claude_windows() {
        let raw = r#"{
            "rate_limits": {
                "five_hour": {"used_percentage": 12, "resets_at": 1776733749},
                "seven_day": {"used_percentage": 37}
            }
        }"#;
        let payload = serde_json::from_str::<ClaudePayload>(raw);
        assert!(payload.is_ok());
        let Ok(payload) = payload else { return };
        let quota = payload.quota(Utc::now());
        assert_eq!(
            render_claude_statusline(quota.as_ref(), &payload),
            "cc 5h  88% 1w  63%"
        );
        let Some(quota) = quota else { return };
        assert_eq!(quota.snapshot().limits.len(), 2);
    }

    #[test]
    fn provider_and_facet_generators_traverse_the_product() {
        let providers = Providers::new(vec![
            ProviderConfig::Claude,
            ProviderConfig::Codex,
            ProviderConfig::OpenRouter {
                token_file: PathBuf::from("/run/secrets/openrouter"),
            },
        ]);
        assert!(providers.is_ok());
        let Ok(providers) = providers else { return };
        let model = Model::new(Config {
            label: "quota".to_owned(),
            providers,
            stale_after_sec: 60.0,
            error_after_sec: 120.0,
        });
        assert!(model.is_ok());
        let Ok(mut model) = model else { return };
        let now = Utc::now();
        let percent_limit = |window| Limit {
            window: Some(window),
            remaining: Quantity::Percent(50),
            resets_at: Some(now.timestamp() + 60),
        };
        model.latest = Some(super::Sample {
            feeds: HashMap::from([
                (
                    Provider::Claude,
                    Ok(Snapshot {
                        captured_at: now,
                        limits: vec![
                            percent_limit(Window::FiveHours),
                            percent_limit(Window::Week),
                        ],
                    }),
                ),
                (
                    Provider::Codex,
                    Ok(Snapshot {
                        captured_at: now,
                        limits: vec![percent_limit(Window::Week)],
                    }),
                ),
                (
                    Provider::OpenRouter,
                    Ok(Snapshot {
                        captured_at: now,
                        limits: vec![Limit {
                            window: None,
                            remaining: Quantity::Dollars {
                                remaining: 4.25,
                                capacity: 10.0,
                            },
                            resets_at: None,
                        }],
                    }),
                ),
            ]),
        });

        assert!(model.render().body.to_string().contains("cc"));
        assert!(model.render().body.to_string().contains("5h"));
        assert!(matches!(model.click(Button::Left), Reaction::Publish(_)));
        assert_eq!(*model.modes.left(), Facet::Resets);
        assert!(model.render().body.to_string().contains('@'));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("cx"));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("or"));
        assert!(model.render().body.to_string().contains("no reset"));
        assert!(matches!(model.click(Button::Left), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("$4.25"));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("cc"));
        let _ = model.apply(Ok(super::Sample {
            feeds: HashMap::from([
                (
                    Provider::Claude,
                    Err(ProbeError::Unit("severed".to_owned())),
                ),
                (Provider::Codex, Err(ProbeError::Unit("severed".to_owned()))),
                (
                    Provider::OpenRouter,
                    Err(ProbeError::Unit("severed".to_owned())),
                ),
            ]),
        }));
        assert!(model.render().body.to_string().contains("5h"));
        assert!(matches!(model.click(Button::Middle), Reaction::Refresh));
    }

    #[tokio::test]
    async fn one_stalled_source_cannot_suppress_its_peers() {
        let fast = sever(Duration::from_millis(5), async {
            Ok::<_, ProbeError>("alive")
        });
        let stalled = sever(
            Duration::from_millis(5),
            future::pending::<Result<&str, ProbeError>>(),
        );
        let (fast, stalled) = tokio::join!(fast, stalled);
        assert_eq!(fast.ok(), Some("alive"));
        assert!(matches!(
            stalled,
            Err(ProbeError::Transport(TransportError::Timeout))
        ));
    }
}
