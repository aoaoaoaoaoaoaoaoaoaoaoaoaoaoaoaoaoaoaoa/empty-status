use crate::machine::effects::{EffectReq, FsRead};
use crate::machine::types::{UnitDecision, UnitMachine, View, loading_view, ready_view};
use crate::units::bat::{Bat, BatConfig};
use std::convert::Infallible;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BatMachine {
    cfg: BatConfig,
}

impl BatMachine {
    pub fn new(cfg: BatConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Bat,
}

impl UnitMachine for BatMachine {
    type PollOut = crate::render::markup::Markup;
    type State = State;
    type UnitError = Infallible;

    fn name(&self) -> &'static str {
        "Bat"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Bat::from_cfg(self.cfg.clone());
        let view = loading_view("bat");
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
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        let out = match effects
            .run(EffectReq::FsRead(FsRead {
                key: crate::machine::effects::FsKey::new(format!(
                    "power/{}",
                    state.unit.uevent_path()
                )),
                path: state.unit.uevent_path().into(),
                cache_fresh_for: Duration::from_millis(200),
            }))
            .await
        {
            Ok(out) => out,
            Err(_) => return Ok(state.unit.missing_markup()),
        };
        let bytes = out.expect::<bytes::Bytes>()?;
        Ok(state.unit.read_markup_from_bytes(&bytes))
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
