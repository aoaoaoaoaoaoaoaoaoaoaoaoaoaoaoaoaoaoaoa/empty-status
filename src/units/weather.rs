// TODO start splitting up into optionals, the dep tree is getting fat
// TODO port the `validation` machinery to `core` as a method on trait
// TODO tempt unids in config
// TODO wind stuff on click, maybe other modes
// TODO phases of the moon!
// TODO forecasts
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use serde_repr::Deserialize_repr;
use std::time::Instant;

use crate::{
    core::{ClickEvent, Unit, BROWN, RED, VIOLET},
    display::{color, color_by_pct_custom},
    register_unit,
};
use reqwest;

const MIN_REFRESH_INTERVAL: f64 = 15.0;

/// All possible Open-Meteo weather codes, per WMO WW definitions.
/// use serde_repr::Deserialize_repr;
#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum PhysicalWeather {
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

impl PhysicalWeather {
    /// A day/night-aware emoji for each condition.
    fn get_emoji(&self) -> TimeDependent<&'static str> {
        match self {
            // clear sky: sun by day, moon by night
            PhysicalWeather::ClearSky => TimeDependent::DayNight("☀️", "🌙"),

            // mostly clear: sun + small cloud by day, moon + cloud by night
            PhysicalWeather::MainlyClear => TimeDependent::DayNight("🌤️", "🌙☁️"),

            // partly cloudy: sun behind cloud vs cloud + moon
            PhysicalWeather::PartlyCloudy => TimeDependent::DayNight("⛅", "☁️🌙☁️"),

            // always overcast
            PhysicalWeather::Overcast => TimeDependent::Fixed("☁️"),

            // fog is the same day or night
            PhysicalWeather::Fog => TimeDependent::Fixed("🌫️"),
            PhysicalWeather::DepositingRimeFog => TimeDependent::Fixed("🌁"),

            // light drizzle: sun + rain vs cloud + rain
            PhysicalWeather::DrizzleLight => TimeDependent::DayNight("🌦️", "🌧️"),
            PhysicalWeather::DrizzleModerate => TimeDependent::Fixed("🌧️"),
            PhysicalWeather::DrizzleDense => TimeDependent::Fixed("🌧️"),

            // freezing drizzle
            PhysicalWeather::FreezingDrizzleLight | PhysicalWeather::FreezingDrizzleDense => {
                TimeDependent::Fixed("❄️🌧️")
            }

            // light rain: sun + rain vs cloud + rain
            PhysicalWeather::RainSlight => TimeDependent::DayNight("🌦️", "🌧️"),
            PhysicalWeather::RainModerate | PhysicalWeather::RainHeavy => {
                TimeDependent::Fixed("🌧️")
            }

            // freezing rain
            PhysicalWeather::FreezingRainLight | PhysicalWeather::FreezingRainHeavy => {
                TimeDependent::Fixed("❄️🌧️")
            }

            // snow
            PhysicalWeather::SnowfallSlight => TimeDependent::DayNight("🌨️", "❄️🌨️"),
            PhysicalWeather::SnowfallModerate
            | PhysicalWeather::SnowfallHeavy
            | PhysicalWeather::SnowGrains => TimeDependent::Fixed("❄️🌨️"),

            // showers
            PhysicalWeather::RainShowersSlight => TimeDependent::DayNight("🌦️", "🌧️"),
            PhysicalWeather::RainShowersModerate => TimeDependent::Fixed("🌧️"),
            PhysicalWeather::RainShowersViolent => TimeDependent::Fixed("⛈️"),

            PhysicalWeather::SnowShowersSlight | PhysicalWeather::SnowShowersHeavy => {
                TimeDependent::Fixed("❄️🌨️")
            }

            // thunder and hail all map to the same storm cloud emoji
            PhysicalWeather::Thunderstorm
            | PhysicalWeather::ThunderstormWithHail
            | PhysicalWeather::ThunderstormWithHailDup => TimeDependent::Fixed("⛈️"),
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
}

impl WeatherConfig {
    /// Our unit construction must be irrefutable -- it's on us to surface the errors
    /// into the unit's read_formatted method if we are invalidated.
    pub fn fix_up_and_validate(&mut self) -> anyhow::Result<()> {
        if self.refresh_interval_sec < MIN_REFRESH_INTERVAL {
            tracing::warn!(
                "Weather refresh interval too low: {:.0}s, using minimum {:.0}s",
                self.refresh_interval_sec,
                MIN_REFRESH_INTERVAL
            );
            self.refresh_interval_sec = MIN_REFRESH_INTERVAL;
        }
        anyhow::ensure!(
            self.lat >= -90.0 && self.lat <= 90.0,
            "bad config: lat must be between -90 and 90 degrees"
        );
        anyhow::ensure!(
            self.lon >= -180.0 && self.lon <= 180.0,
            "bad config: lon must be between -180 and 180 degrees"
        );
        Ok(())
    }
}

/// RFC3339‐ish format *without* seconds: “YYYY‐MM‐DDTHH:MM”
const FORMAT_NO_SECS: &str = "%Y-%m-%dT%H:%M";

pub mod rfc3339_no_seconds {
    use chrono::NaiveDateTime;
    use serde::Deserializer;

    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // parse as NaiveDateTime then assume UTC
        NaiveDateTime::parse_from_str(&s, FORMAT_NO_SECS)
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Deserialize)]
struct OMCurrentWeather {
    #[serde(rename = "temperature")]
    temperature_c: f64,
    #[serde(rename = "weathercode")]
    weather: PhysicalWeather,
    // this is what they reutrn by default, just going to assume that's not
    // going to change randomly...
    #[serde(rename = "time", with = "rfc3339_no_seconds")]
    dt_utc: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct OMResponse {
    current_weather: OMCurrentWeather,
}

#[derive(Debug)]
pub struct Weather {
    cfg: WeatherConfig,
    last_poll: Option<Instant>,
    res: Option<OMCurrentWeather>,
    // warning spam is evil
    validation: anyhow::Result<()>,
}

impl Weather {
    pub fn from_cfg(mut cfg: WeatherConfig) -> Self {
        let validation = cfg.fix_up_and_validate();
        Self {
            cfg,
            last_poll: None,
            res: None,
            validation,
        }
    }

    async fn poll_provider(&mut self) -> Result<()> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current_weather=true",
            self.cfg.lat, self.cfg.lon
        );
        self.res = None;
        let res: OMResponse = reqwest::get(&url).await?.json().await?;
        self.res = Some(res.current_weather);
        self.last_poll = Some(Instant::now());
        Ok(())
    }
}

#[async_trait]
impl Unit for Weather {
    async fn read_formatted(&mut self) -> String {
        if let Err(e) = &self.validation {
            return format!("weather {}", color(format!("{e}"), BROWN));
        }
        if self.last_poll.map_or(true, |last| {
            Instant::now().duration_since(last).as_secs_f64() > self.cfg.refresh_interval_sec
        }) {
            if let Err(e) = self.poll_provider().await {
                return format!("weather {}", color(format!("error: {e}"), RED));
            }
        }

        let res = match self.res {
            Some(ref weather) => weather,
            None => return format!("weather {}", color("loading", VIOLET)),
        };

        format!(
            "weather [{:^4}] {}°C",
            res.weather
                .get_emoji()
                .get_at(self.cfg.lat, self.cfg.lon, res.dt_utc),
            color(
                format!("{:3.1}", res.temperature_c),
                color_by_pct_custom(res.temperature_c, &[-10.0, 15.0, 25.0, 35.0]),
            )
        )
    }

    fn handle_click(&mut self, _click: ClickEvent) {
        self.last_poll = None;
    }
}

register_unit!(Weather, WeatherConfig);
