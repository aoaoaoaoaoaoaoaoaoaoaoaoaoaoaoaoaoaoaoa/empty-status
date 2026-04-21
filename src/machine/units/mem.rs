use crate::machine::types::{UnitDecision, UnitMachine, View, loading_view, ready_view};
use crate::units::mem::{Mem, MemConfig};
use std::convert::Infallible;

#[derive(Debug, Clone, Copy)]
pub struct MemMachine;

impl MemMachine {
    pub fn new(_cfg: MemConfig) -> Self {
        Self
    }
}

#[derive(Debug)]
pub struct State {
    unit: Mem,
}

impl UnitMachine for MemMachine {
    type PollOut = crate::render::markup::Markup;
    type State = State;
    type UnitError = Infallible;

    fn name(&self) -> &'static str {
        "Mem"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Mem::new();
        let view = loading_view("mem");

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
