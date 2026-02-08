use crate::core::Unit;
use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};

#[derive(Debug)]
pub struct LegacyMachine {
    kind: &'static str,
    build: fn(toml::Value) -> anyhow::Result<Box<dyn Unit>>,
    raw_cfg: toml::Value,
}

impl LegacyMachine {
    pub fn new(
        kind: &'static str,
        build: fn(toml::Value) -> anyhow::Result<Box<dyn Unit>>,
        raw_cfg: toml::Value,
    ) -> Self {
        Self {
            kind,
            build,
            raw_cfg,
        }
    }

    fn build_unit(&self) -> anyhow::Result<Box<dyn Unit>> {
        (self.build)(self.raw_cfg.clone())
    }

    #[allow(dead_code)]
    fn build_broken_unit_state(msg: impl Into<String>) -> State {
        State {
            unit: Box::new(BrokenUnit { msg: msg.into() }),
            kind: "__placeholder__",
        }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Box<dyn Unit>,
    kind: &'static str,
}

impl State {
    fn is_placeholder(&self) -> bool {
        self.kind == "__placeholder__"
    }
}

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl UnitMachine for LegacyMachine {
    type PollOut = crate::core::Readout;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        self.kind
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let mut unit = self
            .build_unit()
            .unwrap_or_else(|e| Box::new(BrokenUnit { msg: e.to_string() }));

        let view = match unit.fix_up_and_validate() {
            Ok(()) => View {
                body: crate::render::markup::Markup::text(format!("{} ", self.kind))
                    + crate::render::markup::Markup::text("loading").fg(crate::core::VIOLET),
                health: Health::Degraded,
            },
            Err(e) => View {
                body: crate::render::markup::Markup::text(format!("{} ", self.kind))
                    + crate::render::markup::Markup::text(e.to_string()).fg(crate::core::RED),
                health: Health::Error,
            },
        };

        (
            State {
                unit,
                kind: self.kind,
            },
            view,
            UnitDecision::PollNow,
        )
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(
        &self,
        state: &mut Self::State,
        click: crate::core::ClickEvent,
    ) -> (Option<View>, UnitDecision) {
        if state.is_placeholder() {
            state.unit = self
                .build_unit()
                .unwrap_or_else(|e| Box::new(BrokenUnit { msg: e.to_string() }));
            state.kind = self.kind;
        }

        state.unit.handle_click(click);
        (None, UnitDecision::PollNow)
    }

    async fn poll(&self, state: &mut Self::State) -> Result<Self::PollOut, Self::UnitError> {
        Ok(state.unit.read_formatted().await)
    }

    fn on_poll_ok(
        &self,
        _state: &mut Self::State,
        ro: Self::PollOut,
    ) -> (
        Availability<
            crate::render::markup::Markup,
            crate::machine::types::PollError<Self::UnitError>,
        >,
        UnitDecision,
    ) {
        let health = match ro.health {
            crate::core::ChunkHealth::Ok => Health::Ok,
            crate::core::ChunkHealth::Warn => Health::Degraded,
            crate::core::ChunkHealth::Err => Health::Error,
        };

        let _ = health;
        (Availability::Ready(ro.markup), UnitDecision::Idle)
    }
}

#[derive(Debug)]
struct BrokenUnit {
    msg: String,
}

#[async_trait::async_trait]
impl Unit for BrokenUnit {
    async fn read_formatted(&mut self) -> crate::core::Readout {
        crate::core::Readout::err(
            crate::render::markup::Markup::text("unit error: ")
                + crate::render::markup::Markup::text(&self.msg).fg(crate::core::RED),
        )
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {}
}
