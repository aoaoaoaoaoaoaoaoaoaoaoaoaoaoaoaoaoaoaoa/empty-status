use crate::machine::types::{UnitDecision, UnitMachine, View, loading_view, ready_view};
use crate::units::time::{Time, TimeConfig};
use std::convert::Infallible;

#[derive(Debug, Clone)]
pub struct TimeMachine {
    cfg: TimeConfig,
}

impl TimeMachine {
    pub fn new(cfg: TimeConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Time,
}

impl UnitMachine for TimeMachine {
    type PollOut = crate::render::markup::Markup;
    type State = State;
    type UnitError = Infallible;

    fn name(&self) -> &'static str {
        "Time"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Time::from_cfg(self.cfg.clone());
        let view = loading_view("time");
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
        crate::machine::types::Availability<
            View,
            crate::machine::types::PollError<Self::UnitError>,
        >,
        UnitDecision,
    ) {
        ready_view(body)
    }
}
