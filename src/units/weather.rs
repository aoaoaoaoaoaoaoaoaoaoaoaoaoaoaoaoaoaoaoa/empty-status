use std::time::Duration;

use chrono::{DateTime, Local, TimeZone, Timelike, Utc};
use reqwest::Url;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use serde_repr::Deserialize_repr;

use crate::{
    core::{Button, Health, View},
    probe_io::{ProbeIo, TransportError},
    render::{
        color::{BLUE, CYAN, GREEN, Gradient, Knot, ORANGE, RED, Rgb8, VIOLET, YELLOW},
        markup::Markup,
    },
    units::{MouseOrbit, ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(15);
const FEED_TIMEOUT: Duration = Duration::from_secs(12);
const WEATHER_ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_ENDPOINT: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";
const CYAN_CELSIUS: f64 = 65.0 / 9.0;
const TEMPERATURE_COLORS: Gradient<6> = Gradient::new(
    Knot::new(-15.0, BLUE),
    [
        Knot::new(CYAN_CELSIUS, CYAN),
        Knot::new(10.0, GREEN),
        Knot::new(25.0, YELLOW),
        Knot::new(32.5, ORANGE),
        Knot::new(40.0, RED),
        Knot::new(50.0, VIOLET),
    ],
);
const AQI_COLORS: Gradient<5> = Gradient::new(
    Knot::new(0.0, CYAN),
    [
        Knot::new(40.0, GREEN),
        Knot::new(80.0, YELLOW),
        Knot::new(120.0, ORANGE),
        Knot::new(160.0, RED),
        Knot::new(200.0, VIOLET),
    ],
);
const SWAMP_GREEN: Rgb8 = Rgb8::new(0x7E, 0x9F, 0x54);
const BOG_GREEN: Rgb8 = Rgb8::new(0x66, 0x84, 0x4F);
const ROT_BROWN: Rgb8 = Rgb8::new(0x91, 0x65, 0x4F);
const CORPSE_PURPLE: Rgb8 = Rgb8::new(0x87, 0x5F, 0x70);
const FETID_VIOLET: Rgb8 = Rgb8::new(0x98, 0x71, 0x8F);
const HUMIDITY_COLORS: Gradient<7> = Gradient::new(
    Knot::new(0.0, YELLOW),
    [
        Knot::new(30.0, YELLOW),
        Knot::new(45.0, GREEN),
        Knot::new(62.0, SWAMP_GREEN),
        Knot::new(76.0, BOG_GREEN),
        Knot::new(86.0, ROT_BROWN),
        Knot::new(93.0, CORPSE_PURPLE),
        Knot::new(100.0, FETID_VIOLET),
    ],
);

cycle!(
    enum Horizon {
        Immediate,
        Forecast,
    }
);

cycle!(
    enum Metric {
        Temperature,
        RelativeHumidity,
        AirQuality,
    }
);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    lat: f64,
    lon: f64,
    #[serde_inline_default(TemperatureUnit::Celsius)]
    units: TemperatureUnit,
}

#[derive(Debug)]
pub struct Model {
    weather_url: Url,
    air_url: Url,
    units: TemperatureUnit,
    modes: MouseOrbit<Horizon, Metric>,
    latest: Option<Sample>,
}

#[derive(Debug, Clone)]
pub struct Request {
    weather_url: Url,
    air_url: Url,
}

#[derive(Debug)]
pub struct Sample {
    weather: Feed<WeatherPoint>,
    air: Feed<AirPoint>,
}

type Feed<P> = Result<Series<P>, ProbeError>;

#[derive(Debug)]
struct Series<P> {
    current: Option<P>,
    hourly: Vec<P>,
}

#[derive(Debug, Clone, Copy)]
struct WeatherPoint {
    at: DateTime<Utc>,
    temperature_c: f64,
    relative_humidity: Option<RelativeHumidity>,
    condition: Wmo,
    daylight: bool,
}

#[derive(Debug, Clone, Copy)]
struct AirPoint {
    at: DateTime<Utc>,
    aqi: Aqi,
}

#[derive(Debug, Clone, Copy)]
struct Aqi(u16);

#[derive(Debug, Clone, Copy)]
struct RelativeHumidity(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AqiBand {
    Good,
    Moderate,
    UnhealthyForSensitiveGroups,
    Unhealthy,
    VeryUnhealthy,
    Hazardous,
}

trait Timed: Copy {
    fn at(self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Deserialize_repr)]
#[repr(u8)]
enum Wmo {
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

#[derive(Debug, Deserialize)]
struct WeatherApiResponse {
    current: Option<WeatherApiCurrent>,
    hourly: Option<WeatherApiHourly>,
}

#[derive(Debug, Deserialize)]
struct WeatherApiCurrent {
    time: i64,
    temperature_2m: f64,
    relative_humidity_2m: Option<f64>,
    #[serde(alias = "weathercode")]
    weather_code: Wmo,
    is_day: u8,
}

#[derive(Debug, Deserialize)]
struct WeatherApiHourly {
    time: Vec<i64>,
    temperature_2m: Vec<f64>,
    relative_humidity_2m: Vec<Option<f64>>,
    #[serde(alias = "weathercode")]
    weather_code: Vec<Wmo>,
    is_day: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct AirApiResponse {
    current: Option<AirApiCurrent>,
    hourly: Option<AirApiHourly>,
}

#[derive(Debug, Deserialize)]
struct AirApiCurrent {
    time: i64,
    us_aqi: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct AirApiHourly {
    time: Vec<i64>,
    us_aqi: Vec<Option<f64>>,
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        if !config.lat.is_finite() || !(-90.0..=90.0).contains(&config.lat) {
            return Err("lat must be finite and between -90 and 90".to_owned());
        }
        if !config.lon.is_finite() || !(-180.0..=180.0).contains(&config.lon) {
            return Err("lon must be finite and between -180 and 180".to_owned());
        }
        Ok(Self {
            weather_url: open_meteo_url(
                WEATHER_ENDPOINT,
                config.lat,
                config.lon,
                "temperature_2m,relative_humidity_2m,weather_code,is_day",
            )?,
            air_url: open_meteo_url(AIR_ENDPOINT, config.lat, config.lon, "us_aqi")?,
            units: config.units,
            modes: MouseOrbit::new(Horizon::Immediate, Metric::Temperature),
            latest: None,
        })
    }

    pub fn request(&self) -> Request {
        Request {
            weather_url: self.weather_url.clone(),
            air_url: self.air_url.clone(),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        match reply {
            Ok(sample) => {
                self.latest = Some(sample);
                self.render()
            }
            Err(error) => error_view("weather", error),
        }
    }

    pub fn click(&mut self, button: Button) -> Reaction {
        if self.modes.act(button) {
            Reaction::publish(self.render())
        } else {
            Reaction::inert()
        }
    }

    fn render(&self) -> View {
        let Some(sample) = self.latest.as_ref() else {
            return View::loading("weather");
        };
        let horizon = *self.modes.left();
        match *self.modes.right() {
            Metric::Temperature => match sample.weather.as_ref() {
                Ok(series) => match horizon {
                    Horizon::Immediate => self.temperature_now(series),
                    Horizon::Forecast => self.temperature_forecast(series),
                },
                Err(error) => View::error("weather", format!("forecast: {error}")),
            },
            Metric::RelativeHumidity => match sample.weather.as_ref() {
                Ok(series) => match horizon {
                    Horizon::Immediate => Self::humidity_now(series),
                    Horizon::Forecast => Self::humidity_forecast(series),
                },
                Err(error) => View::error("weather", format!("forecast: {error}")),
            },
            Metric::AirQuality => match sample.air.as_ref() {
                Ok(series) => match horizon {
                    Horizon::Immediate => Self::aqi_now(series),
                    Horizon::Forecast => Self::aqi_forecast(series),
                },
                Err(error) => View::error("weather", format!("AQI: {error}")),
            },
        }
    }

    fn temperature_now(&self, series: &Series<WeatherPoint>) -> View {
        series.immediate().map_or_else(
            || View::error("weather", "current forecast missing"),
            |point| View::ok(Markup::text("weather ") + Markup::bracketed(self.weather(point))),
        )
    }

    fn temperature_forecast(&self, series: &Series<WeatherPoint>) -> View {
        let points = forecast_points(series);
        if points.is_empty() {
            return View::new(
                Markup::text("weather forecast unavailable"),
                Health::Degraded,
            );
        }
        View::ok(Markup::text("weather ") + forecast_markup(&points, |point| self.weather(point)))
    }

    fn humidity_now(series: &Series<WeatherPoint>) -> View {
        series
            .immediate()
            .and_then(|point| point.relative_humidity)
            .map_or_else(
                || View::error("weather", "current relative humidity missing"),
                |humidity| {
                    View::ok(
                        Markup::text("weather ")
                            + Markup::bracketed(Markup::text("RH ") + humidity.markup()),
                    )
                },
            )
    }

    fn humidity_forecast(series: &Series<WeatherPoint>) -> View {
        let points = forecast_points(series)
            .into_iter()
            .filter_map(|(hour, point)| point.relative_humidity.map(|humidity| (hour, humidity)))
            .collect::<Vec<_>>();
        if points.is_empty() {
            return View::new(
                Markup::text("weather RH forecast unavailable"),
                Health::Degraded,
            );
        }
        View::ok(Markup::text("weather RH ") + forecast_markup(&points, RelativeHumidity::markup))
    }

    fn aqi_now(series: &Series<AirPoint>) -> View {
        series.immediate().map_or_else(
            || View::error("weather", "current AQI missing"),
            |point| {
                View::new(
                    Markup::text("weather ")
                        + Markup::bracketed(Markup::text("AQI ") + point.aqi.markup()),
                    point.aqi.health(),
                )
            },
        )
    }

    fn aqi_forecast(series: &Series<AirPoint>) -> View {
        let points = forecast_points(series);
        if points.is_empty() {
            return View::new(
                Markup::text("weather AQI forecast unavailable"),
                Health::Degraded,
            );
        }
        let health = points
            .iter()
            .map(|(_, point)| point.aqi.health())
            .max()
            .unwrap_or(Health::Ok);
        View::new(
            Markup::text("weather AQI ") + forecast_markup(&points, |point| point.aqi.markup()),
            health,
        )
    }

    fn weather(&self, point: WeatherPoint) -> Markup {
        let temperature = self.units.convert(point.temperature_c);
        Markup::text(point.condition.emoji(point.daylight))
            + Markup::text(format!("{temperature:2.0}")).fg(temperature_color(point.temperature_c))
            + Markup::text(format!("°{}", self.units.suffix()))
    }
}

impl<P: Timed> Series<P> {
    fn immediate(&self) -> Option<P> {
        self.current.or_else(|| {
            let now = Utc::now();
            self.hourly
                .iter()
                .rev()
                .find(|point| point.at() <= now)
                .copied()
                .or_else(|| self.hourly.first().copied())
        })
    }

    fn nearest(&self, target: DateTime<Utc>) -> Option<P> {
        self.hourly
            .iter()
            .min_by_key(|point| (point.at() - target).num_seconds().unsigned_abs())
            .filter(|point| (point.at() - target).num_minutes().abs() <= 30)
            .copied()
    }
}

impl Timed for WeatherPoint {
    fn at(self) -> DateTime<Utc> {
        self.at
    }
}

impl Timed for AirPoint {
    fn at(self) -> DateTime<Utc> {
        self.at
    }
}

impl Aqi {
    fn parse(raw: f64) -> Result<Self, ProbeError> {
        if raw.is_finite() && (0.0..=f64::from(u16::MAX)).contains(&raw) {
            Ok(Self(raw.round() as u16))
        } else {
            Err(ProbeError::Unit(format!(
                "Open-Meteo returned invalid U.S. AQI {raw}"
            )))
        }
    }

    const fn band(self) -> AqiBand {
        match self.0 {
            0..=50 => AqiBand::Good,
            51..=100 => AqiBand::Moderate,
            101..=150 => AqiBand::UnhealthyForSensitiveGroups,
            151..=200 => AqiBand::Unhealthy,
            201..=300 => AqiBand::VeryUnhealthy,
            301.. => AqiBand::Hazardous,
        }
    }

    const fn health(self) -> Health {
        self.band().health()
    }

    fn markup(self) -> Markup {
        Markup::text(format!("{:>3}", self.0)).fg(AQI_COLORS.sample(f64::from(self.0)))
    }
}

impl RelativeHumidity {
    fn parse(raw: f64) -> Result<Self, ProbeError> {
        if raw.is_finite() && (0.0..=100.0).contains(&raw) {
            Ok(Self(raw.round() as u8))
        } else {
            Err(ProbeError::Unit(format!(
                "Open-Meteo returned invalid relative humidity {raw}"
            )))
        }
    }

    fn markup(self) -> Markup {
        Markup::text(format!("{:>3}%", self.0)).fg(HUMIDITY_COLORS.sample(f64::from(self.0)))
    }
}

impl AqiBand {
    const fn health(self) -> Health {
        match self {
            Self::Good | Self::Moderate => Health::Ok,
            Self::UnhealthyForSensitiveGroups => Health::Degraded,
            Self::Unhealthy | Self::VeryUnhealthy | Self::Hazardous => Health::Error,
        }
    }
}

impl TemperatureUnit {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Celsius => "C",
            Self::Fahrenheit => "F",
        }
    }

    const fn convert(self, celsius: f64) -> f64 {
        match self {
            Self::Celsius => celsius,
            Self::Fahrenheit => celsius * 9.0 / 5.0 + 32.0,
        }
    }
}

impl Wmo {
    const fn emoji(self, daylight: bool) -> &'static str {
        match self {
            Self::ClearSky if daylight => "☀️",
            Self::ClearSky => "🌙",
            Self::MainlyClear if daylight => "🌤️",
            Self::MainlyClear => "🌙☁️",
            Self::PartlyCloudy if !daylight => "🌙☁️",
            Self::PartlyCloudy => "⛅",
            Self::Overcast => "☁️",
            Self::Fog | Self::DepositingRimeFog => "🌫️",
            Self::DrizzleLight | Self::RainSlight | Self::RainShowersSlight if daylight => "🌦️",
            Self::DrizzleLight | Self::RainSlight | Self::RainShowersSlight => "🌙🌧️",
            Self::DrizzleModerate | Self::RainModerate | Self::RainShowersModerate => "🌧️",
            Self::DrizzleDense | Self::RainHeavy | Self::RainShowersViolent => "🌧️🌧️",
            Self::FreezingDrizzleLight
            | Self::FreezingDrizzleDense
            | Self::FreezingRainLight
            | Self::FreezingRainHeavy => "🌧️🧊",
            Self::SnowfallSlight | Self::SnowShowersSlight | Self::SnowGrains => "🌨️",
            Self::SnowfallModerate => "🌨️🌨️",
            Self::SnowfallHeavy | Self::SnowShowersHeavy => "🌨️🌨️🌨️",
            Self::Thunderstorm | Self::ThunderstormWithHail | Self::ThunderstormWithHailDup => "⛈️",
        }
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    let (weather, air) = tokio::join!(
        fetch_weather(request.weather_url, io),
        fetch_air(request.air_url, io),
    );
    Ok(Sample { weather, air })
}

async fn fetch_weather(url: Url, io: &ProbeIo) -> Feed<WeatherPoint> {
    let body = fetch(url, io).await?;
    parse_weather(&body)
}

async fn fetch_air(url: Url, io: &ProbeIo) -> Feed<AirPoint> {
    let body = fetch(url, io).await?;
    parse_air(&body)
}

async fn fetch(url: Url, io: &ProbeIo) -> Result<Vec<u8>, ProbeError> {
    tokio::time::timeout(FEED_TIMEOUT, io.get(url))
        .await
        .map_err(|_| ProbeError::Transport(TransportError::Timeout))?
        .map_err(Into::into)
}

fn open_meteo_url(endpoint: &str, lat: f64, lon: f64, variables: &str) -> Result<Url, String> {
    let mut url =
        Url::parse(endpoint).map_err(|error| format!("invalid Open-Meteo URL: {error}"))?;
    let _ = url
        .query_pairs_mut()
        .append_pair("latitude", &format!("{lat:.4}"))
        .append_pair("longitude", &format!("{lon:.4}"))
        .append_pair("current", variables)
        .append_pair("hourly", variables)
        .append_pair("forecast_days", "2")
        .append_pair("timeformat", "unixtime")
        .append_pair("timezone", "UTC");
    Ok(url)
}

fn parse_weather(body: &[u8]) -> Result<Series<WeatherPoint>, ProbeError> {
    let response: WeatherApiResponse = serde_json::from_slice(body).map_err(|error| {
        ProbeError::Unit(format!("invalid Open-Meteo weather response: {error}"))
    })?;
    Ok(Series {
        current: response.current.map(weather_from_current).transpose()?,
        hourly: response
            .hourly
            .map(weather_from_hourly)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn parse_air(body: &[u8]) -> Result<Series<AirPoint>, ProbeError> {
    let response: AirApiResponse = serde_json::from_slice(body)
        .map_err(|error| ProbeError::Unit(format!("invalid Open-Meteo AQI response: {error}")))?;
    Ok(Series {
        current: response
            .current
            .map(air_from_current)
            .transpose()?
            .flatten(),
        hourly: response
            .hourly
            .map(air_from_hourly)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn weather_from_current(current: WeatherApiCurrent) -> Result<WeatherPoint, ProbeError> {
    Ok(WeatherPoint {
        at: timestamp(current.time)?,
        temperature_c: current.temperature_2m,
        relative_humidity: current
            .relative_humidity_2m
            .map(RelativeHumidity::parse)
            .transpose()?,
        condition: current.weather_code,
        daylight: current.is_day != 0,
    })
}

fn weather_from_hourly(hourly: WeatherApiHourly) -> Result<Vec<WeatherPoint>, ProbeError> {
    let length = hourly.time.len();
    if hourly.temperature_2m.len() != length
        || hourly.relative_humidity_2m.len() != length
        || hourly.weather_code.len() != length
        || hourly.is_day.len() != length
    {
        return Err(ProbeError::Unit(
            "Open-Meteo weather arrays have unequal lengths".to_owned(),
        ));
    }
    hourly
        .time
        .into_iter()
        .zip(hourly.temperature_2m)
        .zip(hourly.relative_humidity_2m)
        .zip(hourly.weather_code)
        .zip(hourly.is_day)
        .map(
            |((((at, temperature_c), relative_humidity), condition), is_day)| {
                Ok(WeatherPoint {
                    at: timestamp(at)?,
                    temperature_c,
                    relative_humidity: relative_humidity
                        .map(RelativeHumidity::parse)
                        .transpose()?,
                    condition,
                    daylight: is_day != 0,
                })
            },
        )
        .collect()
}

fn air_from_current(current: AirApiCurrent) -> Result<Option<AirPoint>, ProbeError> {
    let at = timestamp(current.time)?;
    current
        .us_aqi
        .map(Aqi::parse)
        .transpose()
        .map(|aqi| aqi.map(|aqi| AirPoint { at, aqi }))
}

fn air_from_hourly(hourly: AirApiHourly) -> Result<Vec<AirPoint>, ProbeError> {
    if hourly.us_aqi.len() != hourly.time.len() {
        return Err(ProbeError::Unit(
            "Open-Meteo AQI arrays have unequal lengths".to_owned(),
        ));
    }
    hourly
        .time
        .into_iter()
        .zip(hourly.us_aqi)
        .filter_map(|(at, aqi)| {
            aqi.map(|aqi| {
                Ok(AirPoint {
                    at: timestamp(at)?,
                    aqi: Aqi::parse(aqi)?,
                })
            })
        })
        .collect()
}

fn timestamp(seconds: i64) -> Result<DateTime<Utc>, ProbeError> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or_else(|| ProbeError::Unit("Open-Meteo returned an invalid timestamp".to_owned()))
}

fn forecast_points<P: Timed>(series: &Series<P>) -> Vec<(u32, P)> {
    forecast_targets()
        .into_iter()
        .filter_map(|target| {
            series
                .nearest(target)
                .map(|point| (point.at().with_timezone(&Local).hour(), point))
        })
        .collect()
}

fn forecast_markup<P: Copy>(points: &[(u32, P)], render: impl Fn(P) -> Markup) -> Markup {
    Markup::join(
        "-",
        points.iter().map(|(hour, point)| {
            Markup::text(format!("{hour:02}")) + Markup::bracketed(render(*point))
        }),
    )
}

fn forecast_targets() -> Vec<DateTime<Utc>> {
    const COUNT: u32 = 6;
    const STRIDE: u32 = 4;
    let now = Local::now();
    let start = now.hour() / STRIDE * STRIDE + STRIDE;
    (0..COUNT)
        .filter_map(|step| {
            let total = start + step * STRIDE;
            let date = now.date_naive() + chrono::Duration::days(i64::from(total / 24));
            Local
                .from_local_datetime(&date.and_hms_opt(total % 24, 0, 0)?)
                .single()
                .map(|local| local.with_timezone(&Utc))
        })
        .collect()
}

fn temperature_color(celsius: f64) -> Rgb8 {
    TEMPERATURE_COLORS.sample(celsius)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        AQI_COLORS, AirPoint, Aqi, BOG_GREEN, CORPSE_PURPLE, CYAN_CELSIUS, Config, FETID_VIOLET,
        HUMIDITY_COLORS, Health, Model, ROT_BROWN, RelativeHumidity, SWAMP_GREEN, Series,
        WeatherPoint, Wmo, forecast_targets, parse_air, parse_weather, temperature_color,
    };
    use crate::{
        core::Button,
        render::color::{BLUE, CYAN, GREEN, ORANGE, RED, VIOLET, YELLOW},
        units::{ProbeError, Reaction, weather::TemperatureUnit},
    };

    #[test]
    fn temperature_gradient_clamps() {
        assert_eq!(temperature_color(-100.0), BLUE);
        assert_eq!(temperature_color(CYAN_CELSIUS), CYAN);
        assert_eq!(temperature_color(10.0), GREEN);
        assert_eq!(temperature_color(25.0), YELLOW);
        assert_eq!(temperature_color(32.5), ORANGE);
        assert_eq!(temperature_color(40.0), RED);
        assert_eq!(temperature_color(50.0), VIOLET);
        assert_eq!(temperature_color(100.0), VIOLET);
    }

    #[test]
    fn aqi_categories_are_exact() {
        let cases = [
            (50, Health::Ok),
            (51, Health::Ok),
            (100, Health::Ok),
            (101, Health::Degraded),
            (150, Health::Degraded),
            (151, Health::Error),
            (200, Health::Error),
            (201, Health::Error),
            (500, Health::Error),
        ];
        for (aqi, health) in cases {
            assert_eq!(Aqi(aqi).health(), health);
        }
        assert!(Aqi::parse(-1.0).is_err());
        assert!(Aqi::parse(501.0).is_ok());
        assert!(Aqi::parse(f64::INFINITY).is_err());
    }

    #[test]
    fn aqi_uses_the_cold_hot_scale_and_peaks_at_200() {
        let anchors = [
            (0, CYAN),
            (40, GREEN),
            (80, YELLOW),
            (120, ORANGE),
            (160, RED),
            (200, VIOLET),
            (300, VIOLET),
            (500, VIOLET),
        ];
        for (aqi, color) in anchors {
            assert_eq!(AQI_COLORS.sample(f64::from(aqi)), color);
        }
    }

    #[test]
    fn humidity_decays_from_comfort_green_into_corpse_purple() {
        let anchors = [
            (0, YELLOW),
            (30, YELLOW),
            (45, GREEN),
            (62, SWAMP_GREEN),
            (76, BOG_GREEN),
            (86, ROT_BROWN),
            (93, CORPSE_PURPLE),
            (100, FETID_VIOLET),
        ];
        for (humidity, color) in anchors {
            assert_eq!(HUMIDITY_COLORS.sample(f64::from(humidity)), color);
        }
    }

    #[test]
    fn parses_nullable_aqi_series() {
        let series = parse_air(
            br#"{
                "current": {"time": 1, "us_aqi": 151.4},
                "hourly": {
                    "time": [1, 2, 3],
                    "us_aqi": [50, null, 201]
                }
            }"#,
        );
        assert!(series.is_ok());
        let Ok(series) = series else { return };
        assert_eq!(series.current.map(|point| point.aqi.0), Some(151));
        assert_eq!(series.hourly.len(), 2);
        assert_eq!(series.hourly[0].aqi.0, 50);
        assert_eq!(series.hourly[1].aqi.0, 201);

        assert!(parse_air(br#"{"hourly":{"time":[1],"us_aqi":[1,2]}}"#).is_err());
    }

    #[test]
    fn parses_nullable_relative_humidity_series() {
        let series = parse_weather(
            br#"{
                "current": {
                    "time": 1,
                    "temperature_2m": 20,
                    "relative_humidity_2m": 55.4,
                    "weather_code": 0,
                    "is_day": 1
                },
                "hourly": {
                    "time": [1, 2, 3],
                    "temperature_2m": [20, 21, 22],
                    "relative_humidity_2m": [0, null, 100],
                    "weather_code": [0, 1, 2],
                    "is_day": [1, 1, 0]
                }
            }"#,
        );
        assert!(series.is_ok());
        let Ok(series) = series else { return };
        assert_eq!(
            series
                .current
                .and_then(|point| point.relative_humidity)
                .map(|humidity| humidity.0),
            Some(55)
        );
        assert_eq!(
            series.hourly[0].relative_humidity.map(|value| value.0),
            Some(0)
        );
        assert_eq!(
            series.hourly[1].relative_humidity.map(|value| value.0),
            None
        );
        assert_eq!(
            series.hourly[2].relative_humidity.map(|value| value.0),
            Some(100)
        );
        assert!(RelativeHumidity::parse(-1.0).is_err());
        assert!(RelativeHumidity::parse(101.0).is_err());
        assert!(RelativeHumidity::parse(f64::NAN).is_err());
    }

    #[test]
    fn feed_failures_are_confined_to_their_metric() {
        let model = Model::new(Config {
            lat: 0.0,
            lon: 0.0,
            units: TemperatureUnit::Celsius,
        });
        assert!(model.is_ok());
        let Ok(mut model) = model else { return };
        let now = Utc::now();
        model.latest = Some(super::Sample {
            weather: Ok(Series {
                current: Some(WeatherPoint {
                    at: now,
                    temperature_c: 20.0,
                    relative_humidity: Some(RelativeHumidity(55)),
                    condition: Wmo::ClearSky,
                    daylight: true,
                }),
                hourly: Vec::new(),
            }),
            air: Err(ProbeError::Unit("severed".to_owned())),
        });

        assert_eq!(model.render().health, Health::Ok);
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert_eq!(model.render().health, Health::Ok);
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert_eq!(model.render().health, Health::Error);

        let Some(sample) = model.latest.as_mut() else {
            return;
        };
        sample.weather = Err(ProbeError::Unit("severed".to_owned()));
        sample.air = Ok(Series {
            current: Some(AirPoint {
                at: now,
                aqi: Aqi(50),
            }),
            hourly: Vec::new(),
        });
        assert_eq!(model.render().health, Health::Ok);
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert_eq!(model.render().health, Health::Error);
    }

    #[test]
    fn mouse_generators_reach_the_six_weather_views() {
        let targets = forecast_targets();
        let weather = targets
            .iter()
            .copied()
            .map(|at| WeatherPoint {
                at,
                temperature_c: 20.0,
                relative_humidity: Some(RelativeHumidity(55)),
                condition: Wmo::ClearSky,
                daylight: true,
            })
            .collect();
        let air = targets
            .iter()
            .copied()
            .map(|at| AirPoint { at, aqi: Aqi(151) })
            .collect();
        let now = Utc::now();
        let model = Model::new(Config {
            lat: 0.0,
            lon: 0.0,
            units: TemperatureUnit::Celsius,
        });
        assert!(model.is_ok());
        let Ok(mut model) = model else { return };
        model.latest = Some(super::Sample {
            weather: Ok(Series {
                current: Some(WeatherPoint {
                    at: now,
                    temperature_c: 20.0,
                    relative_humidity: Some(RelativeHumidity(55)),
                    condition: Wmo::ClearSky,
                    daylight: true,
                }),
                hourly: weather,
            }),
            air: Ok(Series {
                current: Some(AirPoint {
                    at: now,
                    aqi: Aqi(151),
                }),
                hourly: air,
            }),
        });

        assert!(model.render().body.to_string().contains("20"));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("RH"));
        assert!(model.render().body.to_string().contains("55%"));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("AQI"));
        assert!(matches!(model.click(Button::Left), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("AQI"));
        assert!(model.render().body.to_string().contains('-'));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(!model.render().body.to_string().contains("AQI"));
        assert!(!model.render().body.to_string().contains("RH"));
        assert!(model.render().body.to_string().contains('-'));
        assert!(matches!(model.click(Button::Right), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("RH"));
        assert!(model.render().body.to_string().contains('-'));
        assert!(matches!(model.click(Button::Left), Reaction::Publish(_)));
        assert!(model.render().body.to_string().contains("RH"));
        assert!(!model.render().body.to_string().contains('-'));
        assert!(matches!(model.click(Button::Middle), Reaction::Inert));
    }
}
