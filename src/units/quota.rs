use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize, de};
use serde_inline_default::serde_inline_default;

use crate::core::{GREY, VIOLET};
use crate::display::color_by_pct_rev;
use crate::machine::types::Health;
use crate::render::markup::Markup;

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct QuotaConfig {
    #[serde_inline_default("quota".to_string())]
    pub label: String,
    #[serde(default)]
    pub providers: QuotaProviders,
    #[serde_inline_default(1800.0)]
    pub stale_after_sec: f64,
    #[serde_inline_default(86400.0)]
    pub error_after_sec: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaProvider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaProviders(Vec<QuotaProvider>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaProvidersError {
    Empty,
    Duplicate(QuotaProvider),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProbeSnapshot {
    pub codex: Option<CodexQuota>,
    pub claude: Option<ClaudeQuota>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexQuota {
    pub captured_at: String,
    pub weekly_used_percent: u8,
    pub weekly_resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeQuota {
    pub captured_at: String,
    pub weekly_used_percent: u8,
    pub weekly_resets_at: i64,
    pub five_hour_used_percent: u8,
    pub five_hour_resets_at: i64,
}

#[derive(Debug, Clone)]
pub struct QuotaRender {
    pub body: Markup,
    pub health: Health,
}

#[derive(Debug, Clone)]
pub struct Quota {
    cfg: QuotaConfig,
    expanded: bool,
    latest: Option<ProbeSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaClickAction {
    Ignore,
    ToggleExpanded,
    RefreshNow,
}

#[derive(Debug)]
pub enum QuotaParseError {
    InvalidSnapshot {
        line: String,
        source: serde_json::Error,
    },
}

impl std::fmt::Display for QuotaParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSnapshot { line, source } => {
                write!(f, "invalid quota snapshot `{line}`: {source}")
            }
        }
    }
}

impl std::error::Error for QuotaParseError {}

impl Default for QuotaProviders {
    fn default() -> Self {
        Self(vec![QuotaProvider::Claude, QuotaProvider::Codex])
    }
}

impl QuotaProvider {
    #[must_use]
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    #[must_use]
    pub fn from_arg(raw: &str) -> Option<Self> {
        match raw {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    fn windows(self) -> &'static [QuotaWindow] {
        match self {
            Self::Claude => &[QuotaWindow::ClaudeWeekly, QuotaWindow::ClaudeFiveHour],
            Self::Codex => &[QuotaWindow::CodexWeekly],
        }
    }
}

impl QuotaProviders {
    pub fn new(providers: Vec<QuotaProvider>) -> Result<Self, QuotaProvidersError> {
        if providers.is_empty() {
            return Err(QuotaProvidersError::Empty);
        }

        let mut seen = Vec::with_capacity(providers.len());
        for provider in &providers {
            if seen.contains(provider) {
                return Err(QuotaProvidersError::Duplicate(*provider));
            }
            seen.push(*provider);
        }

        Ok(Self(providers))
    }

    pub fn iter(&self) -> impl Iterator<Item = QuotaProvider> + '_ {
        self.0.iter().copied()
    }

    #[must_use]
    pub fn contains(&self, provider: QuotaProvider) -> bool {
        self.0.contains(&provider)
    }
}

impl<'de> Deserialize<'de> for QuotaProviders {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        Self::new(Vec::<QuotaProvider>::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl Serialize for QuotaProviders {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl std::fmt::Display for QuotaProvidersError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("quota provider set cannot be empty"),
            Self::Duplicate(provider) => write!(f, "duplicate quota provider `{provider}`"),
        }
    }
}

impl std::fmt::Display for QuotaProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_arg())
    }
}

impl std::error::Error for QuotaProvidersError {}

impl CodexQuota {
    fn weekly_remaining_percent(&self) -> u8 {
        100_u8.saturating_sub(self.weekly_used_percent)
    }
}

impl ClaudeQuota {
    fn weekly_remaining_percent(&self) -> u8 {
        100_u8.saturating_sub(self.weekly_used_percent)
    }

    fn five_hour_remaining_percent(&self) -> u8 {
        100_u8.saturating_sub(self.five_hour_used_percent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Fresh,
    Stale,
    Expired,
    Missing,
}

#[derive(Debug, Clone, Copy)]
enum QuotaWindow {
    ClaudeWeekly,
    ClaudeFiveHour,
    CodexWeekly,
}

impl QuotaWindow {
    fn label(self) -> &'static str {
        match self {
            Self::ClaudeWeekly => "cc7",
            Self::ClaudeFiveHour => "cc5",
            Self::CodexWeekly => "cx7",
        }
    }

    fn remaining_percent(self, snapshot: &ProbeSnapshot) -> Option<u8> {
        match self {
            Self::ClaudeWeekly => snapshot
                .claude
                .as_ref()
                .map(ClaudeQuota::weekly_remaining_percent),
            Self::ClaudeFiveHour => snapshot
                .claude
                .as_ref()
                .map(ClaudeQuota::five_hour_remaining_percent),
            Self::CodexWeekly => snapshot
                .codex
                .as_ref()
                .map(CodexQuota::weekly_remaining_percent),
        }
    }

    fn rollover(self, snapshot: &ProbeSnapshot) -> Option<i64> {
        match self {
            Self::ClaudeWeekly => snapshot.claude.as_ref().map(|quota| quota.weekly_resets_at),
            Self::ClaudeFiveHour => snapshot
                .claude
                .as_ref()
                .map(|quota| quota.five_hour_resets_at),
            Self::CodexWeekly => snapshot
                .codex
                .as_ref()
                .and_then(|quota| quota.weekly_resets_at),
        }
    }
}

impl Quota {
    pub fn from_cfg(cfg: QuotaConfig) -> Self {
        Self {
            cfg,
            expanded: false,
            latest: None,
        }
    }

    pub fn update_from_lines(&mut self, lines: Vec<String>) -> Result<(), QuotaParseError> {
        let mut latest = None;
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            latest = Some(
                serde_json::from_str::<ProbeSnapshot>(trimmed).map_err(|source| {
                    QuotaParseError::InvalidSnapshot {
                        line: trimmed.to_string(),
                        source,
                    }
                })?,
            );
        }
        if let Some(snapshot) = latest {
            self.update(snapshot);
        }
        Ok(())
    }

    pub fn providers(&self) -> &QuotaProviders {
        &self.cfg.providers
    }

    pub fn update(&mut self, snapshot: ProbeSnapshot) {
        self.latest = Some(snapshot);
    }

    pub fn render(&self) -> QuotaRender {
        let Some(snapshot) = self.latest.as_ref() else {
            return QuotaRender {
                body: Markup::text(format!("{} ", self.cfg.label))
                    + Markup::text("loading").fg(VIOLET),
                health: Health::Degraded,
            };
        };

        let health = self.render_health(snapshot);
        let parts = self
            .cfg
            .providers
            .iter()
            .flat_map(|provider| provider.windows().iter().copied())
            .map(|window| render_window(window.label(), window.remaining_percent(snapshot)))
            .collect::<Vec<_>>();

        let mut body =
            Markup::text(format!("{} ", self.cfg.label)) + Markup::join(Markup::text(" "), parts);

        if self.expanded {
            let detail = self
                .cfg
                .providers
                .iter()
                .flat_map(|provider| provider.windows().iter().copied())
                .map(|window| render_rollover(window.label(), window.rollover(snapshot)))
                .collect::<Vec<_>>();
            body = body + Markup::text(" ") + Markup::join(Markup::text(" "), detail);
        }

        QuotaRender { body, health }
    }

    pub fn handle_click(&mut self, click: crate::core::ClickEvent) -> QuotaClickAction {
        match click.button {
            1 => {
                self.expanded = !self.expanded;
                QuotaClickAction::ToggleExpanded
            }
            3 => QuotaClickAction::RefreshNow,
            _ => QuotaClickAction::Ignore,
        }
    }

    fn source_state(&self, timestamp: Option<&str>) -> SourceState {
        let Some(age) = timestamp.and_then(parse_rfc3339_age_seconds) else {
            return SourceState::Missing;
        };

        if age > self.cfg.error_after_sec {
            SourceState::Expired
        } else if age > self.cfg.stale_after_sec {
            SourceState::Stale
        } else {
            SourceState::Fresh
        }
    }

    fn provider_state(&self, snapshot: &ProbeSnapshot, provider: QuotaProvider) -> SourceState {
        match provider {
            QuotaProvider::Claude => self.source_state(
                snapshot
                    .claude
                    .as_ref()
                    .map(|quota| quota.captured_at.as_str()),
            ),
            QuotaProvider::Codex => self.source_state(
                snapshot
                    .codex
                    .as_ref()
                    .map(|quota| quota.captured_at.as_str()),
            ),
        }
    }

    fn render_health(&self, snapshot: &ProbeSnapshot) -> Health {
        let states = self
            .cfg
            .providers
            .iter()
            .map(|provider| self.provider_state(snapshot, provider))
            .collect::<Vec<_>>();

        if states.contains(&SourceState::Expired)
            || states.iter().all(|state| *state == SourceState::Missing)
        {
            Health::Error
        } else if states.contains(&SourceState::Stale) {
            Health::Degraded
        } else {
            Health::Ok
        }
    }
}

fn render_window(label: &str, remaining: Option<u8>) -> Markup {
    let value = remaining.map_or_else(
        || Markup::text("--").fg(VIOLET),
        |remaining| {
            Markup::text(format!("{remaining:>3}%")).fg(color_by_pct_rev(f64::from(remaining)))
        },
    );

    Markup::bracketed(Markup::text(label).fg(GREY) + Markup::text(" ") + value)
}

fn render_rollover(label: &str, reset_at: Option<i64>) -> Markup {
    let value = reset_at
        .and_then(format_rollover_timestamp)
        .map_or_else(|| Markup::text("--").fg(VIOLET), Markup::text);
    Markup::bracketed(Markup::text(format!("{label}@")).fg(GREY) + value)
}

fn parse_rfc3339_age_seconds(raw: &str) -> Option<f64> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?;
    let delta = Utc::now().signed_duration_since(parsed.with_timezone(&Utc));
    delta.to_std().ok().map(|duration| duration.as_secs_f64())
}

fn format_rollover_timestamp(timestamp: i64) -> Option<String> {
    let local = Local.timestamp_opt(timestamp, 0).single()?;
    Some(local.format("%a %m-%d %H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{Health, Quota, QuotaClickAction, QuotaConfig, QuotaProvider, QuotaProviders};

    #[test]
    fn render_collapsed_snapshot() {
        let captured_at = Utc::now().to_rfc3339();
        let weekly_resets_at = Utc::now().timestamp() + 86400;
        let line = format!(
            r#"{{"sampled_at":"{captured_at}","codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":5,"five_hour_resets_at":null,"plan_type":"pro"}},"claude":{{"captured_at":"{captured_at}","weekly_used_percent":18,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":9,"five_hour_resets_at":{weekly_resets_at}}}}}"#
        );

        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });
        assert!(quota.update_from_lines(vec![line]).is_ok());

        let rendered = quota.render();
        let body = rendered.body.to_string();
        assert!(body.contains("cc7"));
        assert!(body.contains("cc5"));
        assert!(body.contains("cx7"));
        assert_eq!(rendered.health, Health::Ok);
    }

    #[test]
    fn render_expanded_rollovers() {
        let captured_at = Utc::now().to_rfc3339();
        let weekly_resets_at = Utc::now().timestamp() + 86400;
        let rollover_label = super::format_rollover_timestamp(weekly_resets_at);
        assert!(rollover_label.is_some());
        let rollover_label = rollover_label.unwrap_or_default();
        let line = format!(
            r#"{{"sampled_at":"{captured_at}","codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":5,"five_hour_resets_at":null,"plan_type":"pro"}},"claude":{{"captured_at":"{captured_at}","weekly_used_percent":18,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":9,"five_hour_resets_at":{weekly_resets_at}}}}}"#
        );

        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });
        assert!(quota.update_from_lines(vec![line]).is_ok());
        assert_eq!(
            quota.handle_click(crate::core::ClickEvent {
                name: "quota".to_string(),
                instance: None,
                button: 1,
                modifiers: Vec::new(),
                x: 0,
                y: 0,
                relative_x: 0,
                relative_y: 0,
                width: 0,
                height: 0,
            }),
            QuotaClickAction::ToggleExpanded
        );

        let body = quota.render().body.to_string();
        assert!(body.contains("cc7@"));
        assert!(body.contains("cc5@"));
        assert!(body.contains("cx7@"));
        assert!(body.contains(&rollover_label));
    }

    #[test]
    fn right_click_requests_refresh_without_expanding() {
        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });

        assert_eq!(
            quota.handle_click(crate::core::ClickEvent {
                name: "quota".to_string(),
                instance: None,
                button: 3,
                modifiers: Vec::new(),
                x: 0,
                y: 0,
                relative_x: 0,
                relative_y: 0,
                width: 0,
                height: 0,
            }),
            QuotaClickAction::RefreshNow
        );
        assert!(!quota.expanded);
    }

    #[test]
    fn ignore_middle_click() {
        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });

        assert_eq!(
            quota.handle_click(crate::core::ClickEvent {
                name: "quota".to_string(),
                instance: None,
                button: 2,
                modifiers: Vec::new(),
                x: 0,
                y: 0,
                relative_x: 0,
                relative_y: 0,
                width: 0,
                height: 0,
            }),
            QuotaClickAction::Ignore
        );
        assert!(!quota.expanded);
    }

    #[test]
    fn fresh_single_source_stays_ok() {
        let captured_at = Utc::now().to_rfc3339();
        let weekly_resets_at = Utc::now().timestamp() + 86400;
        let line = format!(
            r#"{{"sampled_at":"{captured_at}","codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at},"plan_type":"pro"}},"claude":null}}"#
        );

        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });
        assert!(quota.update_from_lines(vec![line]).is_ok());

        assert_eq!(quota.render().health, Health::Ok);
    }

    #[test]
    fn codex_provider_renders_only_codex_window() {
        let captured_at = Utc::now().to_rfc3339();
        let weekly_resets_at = Utc::now().timestamp() + 86400;
        let line = format!(
            r#"{{"codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at}}},"claude":null}}"#
        );

        let providers = QuotaProviders::new(vec![QuotaProvider::Codex]);
        assert!(providers.is_ok());
        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: providers.unwrap_or_else(|_| QuotaProviders::default()),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });
        assert!(quota.update_from_lines(vec![line]).is_ok());

        let body = quota.render().body.to_string();
        assert!(!body.contains("cc7"));
        assert!(!body.contains("cc5"));
        assert!(body.contains("cx7"));
        assert_eq!(quota.render().health, Health::Ok);
    }
}
