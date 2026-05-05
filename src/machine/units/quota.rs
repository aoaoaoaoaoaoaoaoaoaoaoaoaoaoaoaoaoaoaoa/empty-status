use crate::core::ClickEvent;
use crate::machine::effects::{EffectReq, ProcExec};
use crate::machine::types::{Health, PollError, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::quota::{Quota, QuotaClickAction, QuotaConfig, QuotaParseError, QuotaProviders};
use std::time::{Duration, Instant};

pub const MIN_QUOTA_REFRESH_SECONDS: f64 = 15.0;

#[derive(Debug, Clone)]
pub struct QuotaMachine {
    cfg: QuotaConfig,
}

impl QuotaMachine {
    pub fn new(cfg: QuotaConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Quota,
    pending_force_refresh: bool,
    last_refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl From<QuotaParseError> for UnitErr {
    fn from(value: QuotaParseError) -> Self {
        Self(value.to_string())
    }
}

#[must_use]
pub fn canonical_poll_interval(seconds: f64) -> f64 {
    seconds.max(MIN_QUOTA_REFRESH_SECONDS)
}

impl State {
    fn can_force_refresh(&self, now: Instant) -> bool {
        match self.last_refresh_started_at {
            Some(last_refresh_started_at) => {
                now.duration_since(last_refresh_started_at).as_secs_f64()
                    >= MIN_QUOTA_REFRESH_SECONDS
            }
            None => true,
        }
    }

    fn begin_refresh(&mut self, now: Instant) -> bool {
        let force_refresh = self.pending_force_refresh;
        self.pending_force_refresh = false;
        self.last_refresh_started_at = Some(now);
        force_refresh
    }
}

impl UnitMachine for QuotaMachine {
    type PollOut = ();
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Quota"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Quota::from_cfg(self.cfg.clone());
        let view = View {
            body: Markup::text("quota ") + Markup::text("loading").fg(crate::core::VIOLET),
            health: Health::Degraded,
        };
        (
            State {
                unit,
                pending_force_refresh: false,
                last_refresh_started_at: None,
            },
            view,
            UnitDecision::PollNow,
        )
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(&self, state: &mut Self::State, click: ClickEvent) -> (Option<View>, UnitDecision) {
        match state.unit.handle_click(click) {
            QuotaClickAction::ToggleExpanded => {
                (Some(render_view(&state.unit)), UnitDecision::Idle)
            }
            QuotaClickAction::RefreshNow => {
                if state.can_force_refresh(Instant::now()) {
                    state.pending_force_refresh = true;
                    (None, UnitDecision::PollNow)
                } else {
                    (Some(render_view(&state.unit)), UnitDecision::Idle)
                }
            }
            QuotaClickAction::Ignore => (Some(render_view(&state.unit)), UnitDecision::Idle),
        }
    }

    async fn poll(
        &self,
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, PollError<Self::UnitError>> {
        let force_refresh = state.begin_refresh(Instant::now());
        let lines = effects
            .run(EffectReq::ProcExec(ProcExec {
                cmd: quota_probe_command(force_refresh, state.unit.providers()),
            }))
            .await
            .map_err(PollError::Transport)?
            .expect::<Vec<String>>()
            .map_err(|error| PollError::Unit(UnitErr(error.to_string())))?;

        state
            .unit
            .update_from_lines(lines)
            .map_err(UnitErr::from)
            .map_err(PollError::Unit)?;

        Ok(())
    }

    fn poll_timeout(&self) -> Duration {
        Duration::from_secs(45)
    }

    fn on_poll_ok(
        &self,
        state: &mut Self::State,
        _out: Self::PollOut,
    ) -> (
        crate::machine::types::Availability<View, PollError<Self::UnitError>>,
        UnitDecision,
    ) {
        (
            crate::machine::types::Availability::Ready(render_view(&state.unit)),
            UnitDecision::Idle,
        )
    }
}

fn render_view(unit: &Quota) -> View {
    let rendered = unit.render();
    View {
        body: rendered.body,
        health: rendered.health,
    }
}

fn quota_probe_command(force_refresh: bool, providers: &QuotaProviders) -> Vec<String> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .or_else(|| {
            std::env::args_os()
                .next()
                .map(|arg| arg.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "empty-status".to_string());

    let mut cmd = vec![exe, super::quota_probe::PROBE_ARG.to_string()];
    for provider in providers.iter() {
        cmd.push(super::quota_probe::PROVIDER_ARG.to_string());
        cmd.push(provider.as_arg().to_string());
    }
    if force_refresh {
        cmd.push("--force".to_string());
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::{MIN_QUOTA_REFRESH_SECONDS, State, canonical_poll_interval};
    use crate::units::quota::{Quota, QuotaConfig, QuotaProviders};
    use std::time::{Duration, Instant};

    fn quota() -> Quota {
        Quota::from_cfg(QuotaConfig {
            label: "quota".to_string(),
            providers: QuotaProviders::default(),
            stale_after_sec: 1800.0,
            error_after_sec: 86400.0,
        })
    }

    #[test]
    fn quota_poll_interval_has_hard_floor() {
        assert_eq!(canonical_poll_interval(5.0), MIN_QUOTA_REFRESH_SECONDS);
        assert_eq!(canonical_poll_interval(300.0), 300.0);
    }

    #[test]
    fn force_refresh_lockout_is_fifteen_seconds() {
        let mut state = State {
            unit: quota(),
            pending_force_refresh: false,
            last_refresh_started_at: None,
        };
        let now = Instant::now();
        assert!(state.can_force_refresh(now));
        state.last_refresh_started_at = Some(now);
        assert!(!state.can_force_refresh(now + Duration::from_secs(14)));
        assert!(state.can_force_refresh(now + Duration::from_secs(15)));
    }

    #[test]
    fn begin_refresh_consumes_pending_force_flag() {
        let now = Instant::now();
        let mut state = State {
            unit: quota(),
            pending_force_refresh: true,
            last_refresh_started_at: None,
        };
        assert!(state.begin_refresh(now));
        assert!(!state.pending_force_refresh);
        assert_eq!(state.last_refresh_started_at, Some(now));
    }
}
