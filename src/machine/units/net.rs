use crate::core::Unit;
use crate::core::{ClickEvent, VIOLET};
use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::net::{Net, NetConfig};

#[derive(Debug, Clone)]
pub struct NetMachine {
    cfg: NetConfig,
}

impl NetMachine {
    pub fn new(cfg: NetConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug, Default)]
pub struct State {
    unit: Option<Net>,
}

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

#[derive(Debug, Clone)]
pub struct PollOut {
    view: Markup,
}

impl UnitMachine for NetMachine {
    type PollOut = PollOut;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Net"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let view = View {
            body: Markup::text("net ") + Markup::text("loading").fg(VIOLET),
            health: Health::Degraded,
        };
        (
            State {
                unit: Some(Net::from_cfg(self.cfg.clone())),
            },
            view,
            UnitDecision::PollNow,
        )
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(&self, state: &mut Self::State, click: ClickEvent) -> (Option<View>, UnitDecision) {
        let Some(unit) = state.unit.as_mut() else {
            return (None, UnitDecision::PollNow);
        };
        unit.handle_click(click);

        // Mode transitions own background task lifetime.
        // In practice, `Net` already starts/stops ping in `handle_click`.
        (None, UnitDecision::PollNow)
    }

    async fn poll(&self, state: &mut Self::State) -> Result<Self::PollOut, Self::UnitError> {
        let Some(unit) = state.unit.as_mut() else {
            return Err(UnitErr("missing net unit".into()));
        };

        // Batch-only polling: view is computed here; any ping background work is drained
        // by `read_formatted_ping()`.
        let view = match unit.mode {
            crate::units::net::DisplayMode::Bandwidth => unit.read_formatted_stats(),
            crate::units::net::DisplayMode::Ping => unit.read_formatted_ping(),
        };

        Ok(PollOut { view })
    }

    fn on_poll_ok(
        &self,
        _state: &mut Self::State,
        out: Self::PollOut,
    ) -> (
        Availability<Markup, crate::machine::types::PollError<Self::UnitError>>,
        UnitDecision,
    ) {
        (Availability::Ready(out.view), UnitDecision::Idle)
    }
}
