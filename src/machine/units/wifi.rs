use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::wifi::{Wifi, WifiConfig};

#[derive(Debug, Clone)]
pub struct WifiMachine {
    cfg: WifiConfig,
}

impl WifiMachine {
    pub fn new(cfg: WifiConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Wifi,
}

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl UnitMachine for WifiMachine {
    type PollOut = Markup;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Wifi"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Wifi::from_cfg(self.cfg.clone());
        Wifi::fix_up_and_validate();
        let view = View {
            body: Markup::text("wifi ") + Markup::text("loading").fg(crate::core::VIOLET),
            health: Health::Degraded,
        };

        (State { unit }, view, UnitDecision::PollNow)
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(
        &self,
        state: &mut Self::State,
        click: crate::core::ClickEvent,
    ) -> (Option<View>, UnitDecision) {
        state.unit.handle_click(click);
        (None, UnitDecision::PollNow)
    }

    async fn poll(
        &self,
        _effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        Ok(state.unit.read_markup())
    }

    fn on_poll_ok(
        &self,
        _state: &mut Self::State,
        body: Self::PollOut,
    ) -> (
        Availability<Markup, crate::machine::types::PollError<Self::UnitError>>,
        UnitDecision,
    ) {
        (Availability::Ready(body), UnitDecision::Idle)
    }
}
