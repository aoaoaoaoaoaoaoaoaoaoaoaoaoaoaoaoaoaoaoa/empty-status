use crate::core::{ClickEvent, VIOLET};
use crate::machine::effects::{EffectReq, FsRead, ProcBatch, ProcKey};
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

impl UnitMachine for NetMachine {
    type PollOut = Markup;
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

    async fn poll(
        &self,
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        let Some(unit) = state.unit.as_mut() else {
            return Err(crate::machine::types::PollError::Unit(UnitErr(
                "missing net unit".into(),
            )));
        };

        if unit.mode == crate::units::net::DisplayMode::Ping {
            let key = ProcKey::new(format!(
                "ping:{}:{}",
                unit.cfg.interface, unit.cfg.ping_server
            ));
            let cmd = vec![
                "ping".to_string(),
                "-n".to_string(),
                "-O".to_string(),
                "-I".to_string(),
                unit.cfg.interface.clone(),
                unit.cfg.ping_server.clone(),
            ];
            let lines = match effects
                .run(EffectReq::ProcBatch(ProcBatch {
                    key,
                    cmd,
                    max_lines: 64,
                }))
                .await
            {
                Ok(crate::machine::effects::EffectOut::ProcLines(lines)) => lines,
                Ok(_) => Vec::new(),
                Err(e) => {
                    return Err(crate::machine::types::PollError::Transport(e));
                }
            };
            Ok(unit.read_formatted_ping(lines))
        } else {
            let carrier = effects
                .run(EffectReq::FsRead(FsRead {
                    key: crate::machine::effects::FsKey::new(format!(
                        "sys/class/net/{}/carrier",
                        unit.cfg.interface
                    )),
                    path: format!("/sys/class/net/{}/carrier", unit.cfg.interface).into(),
                    cache_fresh_for: std::time::Duration::from_millis(500),
                }))
                .await
                .ok()
                .and_then(|out| out.expect::<bytes::Bytes>().ok());
            Ok(unit.read_formatted_stats(carrier.as_deref()))
        }
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
