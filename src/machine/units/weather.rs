use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::weather::{Weather, WeatherConfig, WeatherError};

#[derive(Debug, Clone)]
pub struct WeatherMachine {
    cfg: WeatherConfig,
}

impl WeatherMachine {
    pub fn new(cfg: WeatherConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Weather,
    last_view_now: Option<Markup>,
    last_view_forecast: Option<Markup>,
}

// Intentionally no `Default`: the runtime guarantees that timed-out polls
// return the original state.

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl WeatherMachine {
    fn fmt_err(err: &UnitErr) -> Markup {
        let mut s = err.to_string();
        s.truncate(80);
        Markup::text(s)
    }
}

impl UnitMachine for WeatherMachine {
    type PollOut = Markup;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Weather"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let mut unit = Weather::from_cfg(self.cfg.clone());
        // validate once at startup
        let view = match unit.fix_up_and_validate() {
            Ok(()) => View {
                body: Markup::text("weather ") + Markup::text("loading").fg(crate::core::VIOLET),
                health: Health::Degraded,
            },
            Err(e) => View {
                body: Markup::text("weather ") + Markup::text(e.to_string()).fg(crate::core::RED),
                health: Health::Error,
            },
        };
        let now = Markup::text("weather ") + Markup::text("loading").fg(crate::core::VIOLET);
        let forecast = Markup::text("weather ") + Markup::text("loading").fg(crate::core::VIOLET);
        (
            State {
                unit,
                last_view_now: Some(now),
                last_view_forecast: Some(forecast),
            },
            view,
            UnitDecision::PollNow, // poll immediately
        )
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        // No render-only ticking for weather.
        (None, UnitDecision::Idle)
    }

    fn on_click(
        &self,
        state: &mut Self::State,
        click: crate::core::ClickEvent,
    ) -> (Option<View>, UnitDecision) {
        state.unit.handle_click(click);
        let view = match state.unit.mode {
            crate::units::weather::DisplayMode::Now => match &state.last_view_now {
                Some(m) => View::ok(m.clone()),
                None => View {
                    body: Markup::text("weather ")
                        + Markup::text("loading").fg(crate::core::VIOLET),
                    health: Health::Degraded,
                },
            },
            crate::units::weather::DisplayMode::Forecast => match &state.last_view_forecast {
                Some(m) => View::ok(m.clone()),
                None => View {
                    body: Markup::text("weather ")
                        + Markup::text("loading").fg(crate::core::VIOLET),
                    health: Health::Degraded,
                },
            },
        };

        // Allow a pure UI toggle without waiting for the next poll.
        // Still request a poll to refresh the underlying data.
        (Some(view), UnitDecision::PollNow)
    }

    async fn poll(
        &self,
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        state.unit.read_markup(effects).await.map_err(|e| match e {
            crate::machine::types::PollError::Transport(t) => {
                crate::machine::types::PollError::Transport(t)
            }
            crate::machine::types::PollError::Unit(WeatherError(msg)) => {
                crate::machine::types::PollError::Unit(UnitErr(msg))
            }
        })
    }

    fn render_unit_error(&self, err: &Self::UnitError) -> Markup {
        Self::fmt_err(err)
    }

    fn on_poll_ok(
        &self,
        state: &mut Self::State,
        body: Self::PollOut,
    ) -> (
        Availability<Markup, crate::machine::types::PollError<Self::UnitError>>,
        UnitDecision,
    ) {
        // `read_markup()` already applied caching/poll-if-needed and rendered the correct mode.
        // We keep the per-mode last views only for click-time instantaneous toggling.
        match state.unit.mode {
            crate::units::weather::DisplayMode::Now => state.last_view_now = Some(body.clone()),
            crate::units::weather::DisplayMode::Forecast => {
                state.last_view_forecast = Some(body.clone());
            }
        }

        (Availability::Ready(body), UnitDecision::Idle)
    }
}
