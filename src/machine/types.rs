use crate::render::markup::Markup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    Degraded,
    Error,
}

#[derive(Debug, Clone)]
pub struct View {
    pub body: Markup,
    pub health: Health,
}

#[derive(Debug, Clone)]
pub enum Availability<T, E> {
    Loading,
    Ready(T),
    Failed(E),
}

#[allow(dead_code)]
impl<T, E> Availability<T, E> {
    pub fn loading() -> Self {
        Self::Loading
    }
}

// Intentionally minimal; grow as use-sites demand.

impl View {
    #[must_use]
    pub fn ok(body: Markup) -> Self {
        Self {
            body,
            health: Health::Ok,
        }
    }

    // kept for symmetry with `ok`, but runtime owns error rendering now
    #[allow(dead_code)]
    #[must_use]
    pub fn error(body: Markup) -> Self {
        Self {
            body,
            health: Health::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitDecision {
    Idle,
    PollNow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Timeout,
    Transport(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError<E> {
    Transport(TransportError),
    Unit(E),
}

impl<E> From<String> for PollError<E> {
    fn from(value: String) -> Self {
        Self::Transport(TransportError::Transport(value))
    }
}

pub(crate) trait UnitMachine: Send + Sync + std::fmt::Debug + 'static {
    type State: Send + std::fmt::Debug + 'static;
    type PollOut: Send + std::fmt::Debug + 'static;
    type UnitError: Send + std::fmt::Debug + std::fmt::Display + 'static;

    fn name(&self) -> &'static str;

    fn init(&self) -> (Self::State, View, UnitDecision);

    fn on_tick(&self, state: &mut Self::State) -> (Option<View>, UnitDecision);

    fn on_click(
        &self,
        state: &mut Self::State,
        click: crate::core::ClickEvent,
    ) -> (Option<View>, UnitDecision);

    fn poll(
        &self,
        state: &mut Self::State,
    ) -> impl std::future::Future<Output = Result<Self::PollOut, Self::UnitError>> + Send;

    fn render_unit_error(&self, err: &Self::UnitError) -> Markup {
        Markup::text(err.to_string())
    }

    fn on_poll_ok(
        &self,
        state: &mut Self::State,
        out: Self::PollOut,
    ) -> (
        Availability<Markup, PollError<Self::UnitError>>,
        UnitDecision,
    );
}
