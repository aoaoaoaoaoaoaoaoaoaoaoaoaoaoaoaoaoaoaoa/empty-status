use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
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
    #[serde_inline_default(1800.0)]
    pub stale_after_sec: f64,
    #[serde_inline_default(86400.0)]
    pub error_after_sec: f64,
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
    pub weekly_resets_at: String,
    pub five_hour_used_percent: u8,
    pub five_hour_resets_at: String,
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
enum ResetAt<'a> {
    Exact(i64),
    Label(&'a str),
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

        let claude_state = self.source_state(
            snapshot
                .claude
                .as_ref()
                .map(|quota| quota.captured_at.as_str()),
        );
        let codex_state = self.source_state(
            snapshot
                .codex
                .as_ref()
                .map(|quota| quota.captured_at.as_str()),
        );
        let health = Self::render_health(claude_state, codex_state);

        let parts = vec![
            render_window(
                "cc7",
                snapshot
                    .claude
                    .as_ref()
                    .map(ClaudeQuota::weekly_remaining_percent),
            ),
            render_window(
                "cc5",
                snapshot
                    .claude
                    .as_ref()
                    .map(ClaudeQuota::five_hour_remaining_percent),
            ),
            render_window(
                "cx7",
                snapshot
                    .codex
                    .as_ref()
                    .map(CodexQuota::weekly_remaining_percent),
            ),
        ];

        let mut body =
            Markup::text(format!("{} ", self.cfg.label)) + Markup::join(Markup::text(" "), parts);

        if self.expanded {
            let detail = vec![
                render_rollover(
                    "cc7",
                    snapshot
                        .claude
                        .as_ref()
                        .map(|quota| ResetAt::Label(quota.weekly_resets_at.as_str())),
                ),
                render_rollover(
                    "cc5",
                    snapshot
                        .claude
                        .as_ref()
                        .map(|quota| ResetAt::Label(quota.five_hour_resets_at.as_str())),
                ),
                render_rollover(
                    "cx7",
                    snapshot
                        .codex
                        .as_ref()
                        .and_then(|quota| quota.weekly_resets_at.map(ResetAt::Exact)),
                ),
            ];
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

    fn render_health(claude: SourceState, codex: SourceState) -> Health {
        if claude == SourceState::Expired
            || codex == SourceState::Expired
            || (claude == SourceState::Missing && codex == SourceState::Missing)
        {
            Health::Error
        } else if claude == SourceState::Stale || codex == SourceState::Stale {
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

fn render_rollover(label: &str, reset_at: Option<ResetAt<'_>>) -> Markup {
    let value = reset_at
        .and_then(|reset_at| match reset_at {
            ResetAt::Exact(timestamp) => format_rollover_timestamp(timestamp),
            ResetAt::Label(label) => Some(label.to_string()),
        })
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

    use super::{Health, Quota, QuotaClickAction, QuotaConfig};

    #[test]
    fn render_collapsed_snapshot() {
        let captured_at = Utc::now().to_rfc3339();
        let weekly_resets_at = Utc::now().timestamp() + 86400;
        let line = format!(
            r#"{{"sampled_at":"{captured_at}","codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":5,"five_hour_resets_at":null,"plan_type":"pro"}},"claude":{{"captured_at":"{captured_at}","weekly_used_percent":18,"weekly_resets_at":"Fri 11am (America/New_York)","five_hour_used_percent":9,"five_hour_resets_at":"Fri 9pm (America/New_York)"}}}}"#
        );

        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
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
        let line = format!(
            r#"{{"sampled_at":"{captured_at}","codex":{{"captured_at":"{captured_at}","weekly_used_percent":36,"weekly_resets_at":{weekly_resets_at},"five_hour_used_percent":5,"five_hour_resets_at":null,"plan_type":"pro"}},"claude":{{"captured_at":"{captured_at}","weekly_used_percent":18,"weekly_resets_at":"Fri 11am (America/New_York)","five_hour_used_percent":9,"five_hour_resets_at":"Fri 9pm (America/New_York)"}}}}"#
        );

        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
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
        assert!(body.contains("11am"));
        assert!(body.contains("9pm"));
    }

    #[test]
    fn right_click_requests_refresh_without_expanding() {
        let mut quota = Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
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
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        });
        assert!(quota.update_from_lines(vec![line]).is_ok());

        assert_eq!(quota.render().health, Health::Ok);
    }
}
