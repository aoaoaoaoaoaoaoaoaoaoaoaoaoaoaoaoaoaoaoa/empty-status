use crate::machine::types::{UnitDecision, UnitMachine, View, loading_view, ready_view};
use crate::units::cpu::{Cpu, CpuConfig};
use std::convert::Infallible;

#[derive(Debug, Clone)]
pub struct CpuMachine {
    cfg: CpuConfig,
}

impl CpuMachine {
    pub fn new(cfg: CpuConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Cpu,
}

impl UnitMachine for CpuMachine {
    type PollOut = crate::render::markup::Markup;
    type State = State;
    type UnitError = Infallible;

    fn name(&self) -> &'static str {
        "Cpu"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Cpu::from_cfg(self.cfg.clone());
        let view = loading_view("cpu");

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
            .run(crate::machine::effects::EffectReq::FsRead(
                crate::machine::effects::FsRead {
                    key: crate::machine::effects::FsKey::new("proc/stat"),
                    path: "/proc/stat".into(),
                    cache_fresh_for: std::time::Duration::from_millis(150),
                },
            ))
            .await
            .map_err(crate::machine::types::PollError::Transport)?;
        let bytes = out.expect::<bytes::Bytes>()?;

        Ok(state.unit.read_markup_from_proc_stat(&bytes))
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
