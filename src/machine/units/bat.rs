use crate::machine::effects::{EffectReq, FsRead};
use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::bat::{Bat, BatConfig};
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

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl UnitMachine for BatMachine {
    type PollOut = Markup;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Bat"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Bat::from_cfg(self.cfg.clone());
        Bat::fix_up_and_validate();
        let view = View {
            body: Markup::text("bat ") + Markup::text("loading").fg(crate::core::VIOLET),
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
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        let out = effects
            .run(EffectReq::FsRead(FsRead {
                key: crate::machine::effects::FsKey::new(format!(
                    "power/{}",
                    state.unit.uevent_path()
                )),
                path: state.unit.uevent_path().into(),
                cache_fresh_for: Duration::from_millis(200),
            }))
            .await?;
        let bytes = out.expect::<bytes::Bytes>()?;
        Ok(state.unit.read_markup_from_bytes(&bytes))
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
