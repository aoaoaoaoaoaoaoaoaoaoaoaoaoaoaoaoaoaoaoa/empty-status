use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::cpu::{Cpu, CpuConfig};

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

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl UnitMachine for CpuMachine {
    type PollOut = Markup;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Cpu"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Cpu::from_cfg(self.cfg.clone());
        Cpu::fix_up_and_validate();
        let view = View {
            body: Markup::text("cpu ") + Markup::text("loading").fg(crate::core::VIOLET),
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
            .run(crate::machine::effects::EffectReq::FsRead(
                crate::machine::effects::FsRead {
                    key: crate::machine::effects::FsKey::new("proc/stat"),
                    path: "/proc/stat".into(),
                    cache_fresh_for: std::time::Duration::from_millis(150),
                },
            ))
            .await
            .map_err(crate::machine::types::PollError::Transport)?;

        let bytes = match out {
            crate::machine::effects::EffectOut::FsBytes(b) => b,
            _ => {
                return Err(crate::machine::types::PollError::Unit(UnitErr(
                    "unexpected effect output".into(),
                )));
            }
        };

        Ok(state.unit.read_markup_from_proc_stat(&bytes))
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
