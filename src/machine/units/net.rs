use crate::core::ClickEvent;
use crate::machine::effects::{EffectReq, FsRead, ProcBatch, ProcKey};
use crate::machine::types::{UnitDecision, UnitMachine, View, loading_view, ready_view};
use crate::units::net::{Net, NetConfig};
use std::convert::Infallible;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct NetMachine {
    cfg: NetConfig,
}

impl NetMachine {
    pub fn new(cfg: NetConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Net,
}

impl UnitMachine for NetMachine {
    type PollOut = crate::render::markup::Markup;
    type State = State;
    type UnitError = Infallible;

    fn name(&self) -> &'static str {
        "Net"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        (
            State {
                unit: Net::from_cfg(self.cfg.clone()),
            },
            loading_view("net"),
            UnitDecision::PollNow,
        )
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(&self, state: &mut Self::State, click: ClickEvent) -> (Option<View>, UnitDecision) {
        state.unit.handle_click(click);

        // Mode transitions own background task lifetime.
        // In practice, `Net` already starts/stops ping in `handle_click`.
        (None, UnitDecision::PollNow)
    }

    async fn poll(
        &self,
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        if state.unit.mode == crate::units::net::DisplayMode::Ping {
            let key = ProcKey::new(format!(
                "ping:{}:{}",
                state.unit.cfg.interface, state.unit.cfg.ping_server
            ));
            let cmd = vec![
                "ping".to_string(),
                "-n".to_string(),
                "-O".to_string(),
                "-I".to_string(),
                state.unit.cfg.interface.clone(),
                state.unit.cfg.ping_server.clone(),
            ];
            let lines = effects
                .run(EffectReq::ProcBatch(ProcBatch {
                    key,
                    cmd,
                    max_lines: 64,
                    startup_grace: Duration::from_millis(250),
                }))
                .await
                .map_err(crate::machine::types::PollError::Transport)?
                .expect::<Vec<String>>()?;
            Ok(state.unit.read_formatted_ping(lines))
        } else {
            let carrier = effects
                .run(EffectReq::FsRead(FsRead {
                    key: crate::machine::effects::FsKey::new(format!(
                        "sys/class/net/{}/carrier",
                        state.unit.cfg.interface
                    )),
                    path: format!("/sys/class/net/{}/carrier", state.unit.cfg.interface).into(),
                    cache_fresh_for: Duration::from_millis(500),
                }))
                .await
                .ok()
                .and_then(|out| out.expect::<bytes::Bytes>().ok());
            Ok(state.unit.read_formatted_stats(carrier.as_deref()))
        }
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
