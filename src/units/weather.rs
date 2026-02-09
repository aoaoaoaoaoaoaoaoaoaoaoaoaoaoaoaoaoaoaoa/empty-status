// TODO start splitting up into optionals, the dep tree is getting fat
// TODO phases of the moon!
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, TimeZone, Timelike, Utc};
use palette::FromColor;
use reqwest::Url;
use serde::{Deserialize, Deserializer};
use serde_inline_default::serde_inline_default;
use serde_repr::Deserialize_repr;
use serde_with::{serde_as, DeserializeAs};
use std::time::Instant;

use crate::machine::effects::{EffectReq, HttpCacheKey, HttpGet, HttpPolicy};
use crate::{
    core::{BROWN, VIOLET},
    mode_enum,
};

use crate::render::color::{Gradient, Srgb8, Stop};
use crate::render::markup::Markup;

mode_enum!(Now, Forecast);

impl std::fmt::Display for DisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayMode::Now => f.write_str("now"),
            DisplayMode::Forecast => f.write_str("forecast"),
        }
    }
}

const MIN_REFRESH_INTERVAL: f64 = 15.0;

/// All possible Open-Meteo weather codes, per WMO WW definitions.
/// use `serde_repr::Deserialize_repr`;
#[derive(Clone, Copy, Debug, Deserialize_repr)]
#[repr(u8)]
pub enum Wmo {
    ClearSky = 0,
    MainlyClear = 1,
    PartlyCloudy = 2,
    Overcast = 3,
    Fog = 45,
    DepositingRimeFog = 48,
    DrizzleLight = 51,
    DrizzleModerate = 53,
    DrizzleDense = 55,
    FreezingDrizzleLight = 56,
    FreezingDrizzleDense = 57,
    RainSlight = 61,
    RainModerate = 63,
    RainHeavy = 65,
    FreezingRainLight = 66,
    FreezingRainHeavy = 67,
    SnowfallSlight = 71,
    SnowfallModerate = 73,
    SnowfallHeavy = 75,
    SnowGrains = 77,
    RainShowersSlight = 80,
    RainShowersModerate = 81,
    RainShowersViolent = 82,
    SnowShowersSlight = 85,
    SnowShowersHeavy = 86,
    Thunderstorm = 95,
    ThunderstormWithHail = 96,
    ThunderstormWithHailDup = 99,
}

enum TimeDependent<T> {
    /// A type that can change over time, like a string that may depend on the current time.
    Fixed(T),
    DayNight(T, T),
}

impl<T> TimeDependent<T> {
    fn is_day_at(lat: f64, lon: f64, now_utc: DateTime<Utc>) -> bool {
        match spa::sunrise_and_set::<spa::StdFloatOps>(now_utc, lat, lon) {
            Ok(spa::SunriseAndSet::PolarDay) => true,
            Ok(spa::SunriseAndSet::PolarNight) => false,
            Ok(spa::SunriseAndSet::Daylight(sunrise, sunset)) => {
                now_utc >= sunrise && now_utc < sunset
            }
            // SAFETY: we avalidate our lat/lon in the config, so this should never happen.
            Err(_) => unreachable!(),
        }
    }
    /// Returns the value based on the current time.
    fn get_at(&self, lat: f64, lon: f64, now_utc: DateTime<Utc>) -> &T {
        match self {
            TimeDependent::Fixed(value) => value,
            TimeDependent::DayNight(day, night) => {
                if Self::is_day_at(lat, lon, now_utc) {
                    day
                } else {
                    night
                }
            }
        }
    }
}

impl Wmo {
    /// A day/night-aware emoji for each condition.
    fn get_emoji(self) -> TimeDependent<&'static str> {
        match self {
            Wmo::ClearSky => TimeDependent::DayNight("☀️", "🌙"),
            Wmo::MainlyClear => TimeDependent::DayNight("🌤️", "🌙☁️"),
            Wmo::PartlyCloudy => TimeDependent::DayNight("⛅", "🌙☁️"),
            Wmo::Overcast => TimeDependent::Fixed("☁️"),
            Wmo::Fog => TimeDependent::Fixed("🌫️"),
            Wmo::DepositingRimeFog => TimeDependent::Fixed("🌫️🧊"),
            Wmo::DrizzleLight | Wmo::RainSlight | Wmo::RainShowersSlight => {
                TimeDependent::DayNight("🌦️", "🌙🌧️")
            }
            Wmo::DrizzleModerate | Wmo::RainModerate | Wmo::RainShowersModerate => {
                TimeDependent::Fixed("🌧️")
            }
            Wmo::DrizzleDense | Wmo::RainHeavy | Wmo::RainShowersViolent => {
                TimeDependent::Fixed("🌧️🌧️")
            }
            Wmo::FreezingDrizzleLight
            | Wmo::FreezingDrizzleDense
            | Wmo::FreezingRainLight
            | Wmo::FreezingRainHeavy => TimeDependent::Fixed("🌧️🧊"),
            Wmo::SnowfallSlight | Wmo::SnowShowersSlight | Wmo::SnowGrains => {
                TimeDependent::Fixed("🌨️")
            }
            Wmo::SnowfallModerate => TimeDependent::Fixed("🌨️🌨️"),
            Wmo::SnowfallHeavy | Wmo::SnowShowersHeavy => TimeDependent::Fixed("🌨️🌨️🌨️"),
            Wmo::Thunderstorm | Wmo::ThunderstormWithHail | Wmo::ThunderstormWithHailDup => {
                TimeDependent::Fixed("⛈️")
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempUnits {
    Celsius,
    Fahrenheit,
}

impl TempUnits {
    pub fn suffix(&self) -> &str {
        match self {
            TempUnits::Celsius => "C",
            TempUnits::Fahrenheit => "F",
        }
    }

    pub fn convert_from_celcius(&self, temp_c: f64) -> f64 {
        match self {
            TempUnits::Celsius => temp_c,
            TempUnits::Fahrenheit => temp_c * 9.0 / 5.0 + 32.0,
        }
    }
}

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct WeatherConfig {
    pub lat: f64,
    pub lon: f64,
    #[serde_inline_default(60.0)]
    pub refresh_interval_sec: f64,
    #[serde_inline_default(TempUnits::Celsius)]
    pub units: TempUnits,
}

/// RFC3339‐ish format *without* seconds: “YYYY‐MM‐DDTHH:MM”
const FORMAT_NO_SECS: &str = "%Y-%m-%dT%H:%M";

struct Rfc3339NoSecs;

impl<'de> DeserializeAs<'de, DateTime<Utc>> for Rfc3339NoSecs {
    fn deserialize_as<D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveDateTime::parse_from_str(&s, FORMAT_NO_SECS)
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
            .map_err(serde::de::Error::custom)
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub(crate) struct OMCurrentWeather {
    #[serde(rename = "temperature_2m")]
    temp_c: f64,
    #[serde(rename = "weathercode")]
    wmo_code: Wmo,
    // this is what they reutrn by default, just going to assume that's not
    // going to change randomly...
    #[serde(rename = "time")]
    #[serde_as(as = "Rfc3339NoSecs")]
    time: DateTime<Utc>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub(crate) struct OMHourlyForecast {
    #[serde(rename = "time")]
    #[serde_as(as = "Vec<Rfc3339NoSecs>")]
    times_utc: Vec<DateTime<Utc>>,
    #[serde(rename = "temperature_2m")]
    temperatures_c: Vec<f64>,
    #[serde(rename = "weathercode")]
    wmo_codes: Vec<Wmo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OMResponseContainer {
    pub(crate) current: Option<OMCurrentWeather>,
    pub(crate) hourly: Option<OMHourlyForecast>,
}

#[derive(Debug)]
pub struct Weather {
    pub(crate) cfg: WeatherConfig,
    pub(crate) mode: DisplayMode,
    pub(crate) last_successful_poll: Option<Instant>,
    pub(crate) last_attempt_poll: Option<Instant>,
    pub(crate) res: Option<OMResponseContainer>,
}

/// Gets the next forecast times. These are always the next 4 "4-hour-round"
/// times, e.g. if now is 10:15, returns 12:00, 16:00, 20:00, 00:00, 04:00, 08:00.
fn get_wanted_forecast_datetimes() -> Vec<DateTime<Utc>> {
    const STEPS: u32 = 6;
    const STRIDE_HOURS: u32 = 4;

    let now_utc = Utc::now();
    let now_local = now_utc.with_timezone(&chrono::Local);

    // Forecasts are chosen by a 4-hour grid in local time: 00, 04, 08, 12, 16, 20.
    // The first slot is the first grid point strictly after `now_local`.
    let start_hour_local = (now_local.hour() / STRIDE_HOURS) * STRIDE_HOURS + STRIDE_HOURS;

    let mut out = Vec::with_capacity(STEPS as usize);

    for step in 0..STEPS {
        let hour_total = start_hour_local + step * STRIDE_HOURS;
        let hour = hour_total % 24;
        let day_offset = hour_total / 24;

        let date_local = now_local.date_naive() + chrono::Duration::days(i64::from(day_offset));
        let dt_local = date_local.and_hms_opt(hour, 0, 0);
        let Some(dt_local) =
            dt_local.and_then(|dt| chrono::Local.from_local_datetime(&dt).single())
        else {
            continue;
        };

        out.push(dt_local.with_timezone(&Utc));
    }

    out
}

impl Weather {
    pub fn from_cfg(cfg: WeatherConfig) -> Self {
        Self {
            cfg,
            mode: DisplayMode::Now,
            last_successful_poll: None,
            last_attempt_poll: None,
            res: None,
        }
    }

    const MIN_OPEN_METEO_INTERVAL: f64 = 60.0;

    pub(crate) async fn poll_weather(
        &mut self,
        effects: &crate::machine::effects::EffectEngine,
    ) -> Result<(), crate::machine::types::PollError<WeatherError>> {
        let mut url = Url::parse("https://api.open-meteo.com/v1/forecast")
            .map_err(|e| crate::machine::types::PollError::Unit(WeatherError(e.to_string())))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("latitude", &format!("{:.4}", self.cfg.lat));
            qp.append_pair("longitude", &format!("{:.4}", self.cfg.lon));
            match self.mode {
                DisplayMode::Now => {
                    qp.append_pair("current", "temperature_2m,weathercode");
                }
                DisplayMode::Forecast => {
                    qp.append_pair("hourly", "temperature_2m,weathercode");
                    qp.append_pair("forecast_days", "2");
                }
            }
        }

        let key = HttpCacheKey::new(format!(
            "open-meteo:{}:{:.4}:{:.4}",
            self.mode, self.cfg.lat, self.cfg.lon
        ));
        let min_interval = self
            .cfg
            .refresh_interval_sec
            .max(Self::MIN_OPEN_METEO_INTERVAL);
        let policy = HttpPolicy {
            // Conservative: stay well below free-tier caps.
            rate: crate::machine::http::RateLimitSpec {
                per: std::time::Duration::from_secs_f64(min_interval),
                burst: 1,
            },
            cache_fresh_for: std::time::Duration::from_secs_f64(min_interval),
        };

        let out = effects
            .run(EffectReq::HttpGet(HttpGet { key, url, policy }))
            .await
            .map_err(crate::machine::types::PollError::Transport)?;

        let body = out
            .expect::<crate::machine::effects::HttpResponse>()
            .map_err(|e| crate::machine::types::PollError::Unit(WeatherError(e.to_string())))?
            .body;

        let res: OMResponseContainer = serde_json::from_slice(&body)
            .map_err(|e| crate::machine::types::PollError::Unit(WeatherError(e.to_string())))?;
        self.res = Some(res);
        self.last_successful_poll = Some(Instant::now());
        Ok(())
    }

    async fn do_poll_if_needed(
        &mut self,
        effects: &crate::machine::effects::EffectEngine,
    ) -> Result<(), crate::machine::types::PollError<WeatherError>> {
        let now = Instant::now();
        let min_interval = self
            .cfg
            .refresh_interval_sec
            .max(Self::MIN_OPEN_METEO_INTERVAL);

        if self.last_successful_poll.is_some_and(|last| {
            now.duration_since(last).as_secs_f64() <= self.cfg.refresh_interval_sec
        }) {
            return Ok(());
        }

        if self
            .last_attempt_poll
            .is_some_and(|last| now.duration_since(last).as_secs_f64() < min_interval)
        {
            return Ok(());
        }

        self.last_attempt_poll = Some(now);
        if self.last_successful_poll.is_none_or(|last| {
            now.duration_since(last).as_secs_f64() > self.cfg.refresh_interval_sec
        }) {
            self.poll_weather(effects).await?;
        }
        Ok(())
    }

    pub(crate) fn format_res_now(&self, res: Option<&OMCurrentWeather>) -> Markup {
        let Some(res) = res else {
            return Markup::text("weather ") + Markup::text("current failed to load").fg(BROWN);
        };

        Markup::text("weather ").append(Markup::bracketed(self.format_single_code_and_tc(
            res.time,
            res.wmo_code,
            res.temp_c,
        )))
    }

    fn format_single_code_and_tc(&self, time: DateTime<Utc>, wmo_code: Wmo, temp_c: f64) -> Markup {
        let grad = Gradient::new(vec![
            Stop {
                t: 0.0,
                color: palette::Oklab::from_color(palette::Srgb::new(0.35, 0.55, 1.0)),
            },
            Stop {
                t: 0.45,
                color: palette::Oklab::from_color(palette::Srgb::new(0.2, 1.0, 0.5)),
            },
            Stop {
                t: 0.7,
                color: palette::Oklab::from_color(palette::Srgb::new(1.0, 0.8, 0.2)),
            },
            Stop {
                t: 1.0,
                color: palette::Oklab::from_color(palette::Srgb::new(1.0, 0.2, 0.2)),
            },
        ]);

        let emoji = *wmo_code
            .get_emoji()
            .get_at(self.cfg.lat, self.cfg.lon, time);
        let col: Srgb8 = grad.map_clamped(temp_c, -15.0, 40.0);
        let temp = Markup::text(format!(
            "{:2.0}",
            self.cfg.units.convert_from_celcius(temp_c)
        ))
        .fg(col);

        Markup::text(emoji)
            .append(temp)
            .append(Markup::text(format!("°{}", self.cfg.units.suffix())))
    }

    pub(crate) fn format_res_forecast(&self, res: Option<&OMHourlyForecast>) -> Markup {
        let Some(res) = res else {
            return Markup::text("weather ") + Markup::text("forecast failed to load").fg(BROWN);
        };
        let times = get_wanted_forecast_datetimes();
        // exact matching should work fine here, everything is rounded
        let mut out_parts = Vec::new();
        for (i, ft) in res.times_utc.iter().enumerate() {
            if times.contains(ft) {
                let part =
                    self.format_single_code_and_tc(*ft, res.wmo_codes[i], res.temperatures_c[i]);
                out_parts.push((ft, part));
            }
        }

        let mut out = Markup::text("weather ");
        for (ix, (time, part)) in out_parts.into_iter().enumerate() {
            if ix > 0 {
                out = out.append(Markup::text("-"));
            }

            let time_local = time.with_timezone(&chrono::Local);
            out = out.append(Markup::text(format!("{:02}", time_local.hour())));
            out = out.append(Markup::bracketed(part));
        }
        out
    }
}

impl Weather {
    pub fn fix_up_and_validate(&mut self) -> anyhow::Result<()> {
        let cfg = &mut self.cfg;
        if cfg.refresh_interval_sec < MIN_REFRESH_INTERVAL {
            tracing::warn!(
                "Weather refresh interval too low: {:.0}s, using minimum {:.0}s",
                cfg.refresh_interval_sec,
                MIN_REFRESH_INTERVAL
            );
            cfg.refresh_interval_sec = MIN_REFRESH_INTERVAL;
        }
        anyhow::ensure!(
            cfg.lat >= -90.0 && cfg.lat <= 90.0,
            "bad config: lat must be between -90 and 90 degrees"
        );
        anyhow::ensure!(
            cfg.lon >= -180.0 && cfg.lon <= 180.0,
            "bad config: lon must be between -180 and 180 degrees"
        );
        Ok(())
    }

    pub async fn read_markup(
        &mut self,
        effects: &crate::machine::effects::EffectEngine,
    ) -> Result<Markup, crate::machine::types::PollError<WeatherError>> {
        self.do_poll_if_needed(effects).await?;
        let Some(ref res) = self.res else {
            return Ok(Markup::text("weather ") + Markup::text("loading").fg(VIOLET));
        };

        Ok(match self.mode {
            DisplayMode::Now => self.format_res_now(res.current.as_ref()),
            DisplayMode::Forecast => self.format_res_forecast(res.hourly.as_ref()),
        })
    }

    pub fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        self.mode = DisplayMode::next(self.mode);
        self.last_successful_poll = None;
        self.last_attempt_poll = None;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WeatherError(pub String);

impl std::fmt::Display for WeatherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WeatherError {}
