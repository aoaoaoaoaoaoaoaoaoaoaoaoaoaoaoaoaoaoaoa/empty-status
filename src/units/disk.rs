use std::{
    str,
    time::{Duration, Instant},
};

use serde::{Deserialize, Deserializer};
use serde_inline_default::serde_inline_default;

use crate::{
    core::{Button, View},
    probe_io::ProbeIo,
    render::{
        color::{BLUE, ORANGE},
        markup::Markup,
    },
    units::{ProbeError, Reaction, error_view, positive_duration},
    util::Ema,
};

pub const TIMEOUT: Duration = Duration::from_secs(3);
const BLOCK_ROOT: &str = "/sys/class/block";
const BARS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

#[derive(Debug, Clone)]
enum Selector {
    Name(String),
    PartLabel(String),
    PartUuid(String),
}

#[serde_inline_default]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    disk: Option<String>,
    #[serde(default)]
    partlabel: Option<String>,
    #[serde(default)]
    partuuid: Option<String>,
    #[serde_inline_default(0.5)]
    smoothing_sec: f64,
    #[serde_inline_default(3e8)]
    write_peak_ref: f64,
    #[serde_inline_default(1.5e9)]
    read_peak_ref: f64,
}

#[derive(Debug, Clone)]
pub struct Config {
    selector: Selector,
    smoothing_sec: f64,
    write_peak_ref: f64,
    read_peak_ref: f64,
}

#[derive(Debug)]
pub struct Model {
    selector: Selector,
    label: String,
    read_peak: f64,
    write_peak: f64,
    previous: Option<Counters>,
    read_rate: Ema,
    write_rate: Ema,
}

#[derive(Debug)]
pub struct Request {
    selector: Selector,
}

#[derive(Debug)]
pub struct Sample {
    captured_at: Instant,
    read_bytes: u64,
    written_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct Counters {
    captured_at: Instant,
    read_bytes: u64,
    written_bytes: u64,
}

pub type Reply = Result<Sample, ProbeError>;

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawConfig::deserialize(deserializer)?;
        let selector = match (raw.disk, raw.partlabel, raw.partuuid) {
            (Some(value), None, None) => Selector::Name(value),
            (None, Some(value), None) => Selector::PartLabel(value),
            (None, None, Some(value)) => Selector::PartUuid(value),
            (None, None, None) => {
                return Err(serde::de::Error::custom(
                    "set exactly one of disk, partlabel, or partuuid",
                ));
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "disk selectors are mutually exclusive",
                ));
            }
        };
        Ok(Self {
            selector,
            smoothing_sec: raw.smoothing_sec,
            write_peak_ref: raw.write_peak_ref,
            read_peak_ref: raw.read_peak_ref,
        })
    }
}

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        let label = selector_value(&config.selector);
        if label.is_empty() || label.contains('/') {
            return Err("disk selector must be one nonempty path component".to_owned());
        }
        let smoothing = positive_duration("smoothing_sec", config.smoothing_sec)?;
        for (name, value) in [
            ("write_peak_ref", config.write_peak_ref),
            ("read_peak_ref", config.read_peak_ref),
        ] {
            if !value.is_finite() || value <= 1.0 {
                return Err(format!("{name} must be finite and greater than one"));
            }
        }
        Ok(Self {
            selector: config.selector,
            label,
            read_peak: config.read_peak_ref,
            write_peak: config.write_peak_ref,
            previous: None,
            read_rate: Ema::new(smoothing),
            write_rate: Ema::new(smoothing),
        })
    }

    pub fn request(&self) -> Request {
        Request {
            selector: self.selector.clone(),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        let sample = match reply {
            Ok(sample) => sample,
            Err(error) => return error_view(&format!("disk {}", self.label), error),
        };
        let current = Counters {
            captured_at: sample.captured_at,
            read_bytes: sample.read_bytes,
            written_bytes: sample.written_bytes,
        };
        let Some(previous) = self.previous.replace(current) else {
            return View::loading(&format!("disk {}", self.label));
        };
        let elapsed = current
            .captured_at
            .saturating_duration_since(previous.captured_at)
            .as_secs_f64();
        let read = rate(current.read_bytes, previous.read_bytes, elapsed);
        let written = rate(current.written_bytes, previous.written_bytes, elapsed);
        let read = self.read_rate.push(read, current.captured_at);
        let written = self.write_rate.push(written, current.captured_at);

        View::ok(
            Markup::text(format!("disk {} ", self.label))
                + Markup::bracketed(
                    Markup::text(bar(read, self.read_peak)).fg(BLUE)
                        + Markup::text(bar(written, self.write_peak)).fg(ORANGE),
                ),
        )
    }

    pub const fn click(_button: Button) -> Reaction {
        Reaction::inert()
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    let name = resolve_name(&request.selector, io).await?;
    let stat_path = format!("{BLOCK_ROOT}/{name}/stat");
    let sector_size = async {
        let direct = format!("{BLOCK_ROOT}/{name}/queue/logical_block_size");
        match io.read(direct).await {
            Ok(size) => Ok(size),
            Err(_) => {
                io.read(format!("{BLOCK_ROOT}/{name}/../queue/logical_block_size"))
                    .await
            }
        }
    };
    let (stat, sector_size) = tokio::join!(io.read(stat_path), sector_size);
    let stat = stat?;
    let sector_size = parse_u64(&sector_size?)?;
    let (read_sectors, written_sectors) = parse_stat(&stat)?;
    Ok(Sample {
        captured_at: Instant::now(),
        read_bytes: read_sectors
            .checked_mul(sector_size)
            .ok_or_else(|| ProbeError::Unit("disk read counter overflow".to_owned()))?,
        written_bytes: written_sectors
            .checked_mul(sector_size)
            .ok_or_else(|| ProbeError::Unit("disk write counter overflow".to_owned()))?,
    })
}

async fn resolve_name(selector: &Selector, io: &ProbeIo) -> Result<String, ProbeError> {
    match selector {
        Selector::Name(name) => Ok(name.clone()),
        Selector::PartLabel(label) => resolve_link("by-partlabel", label, io).await,
        Selector::PartUuid(uuid) => resolve_link("by-partuuid", uuid, io).await,
    }
}

async fn resolve_link(kind: &str, value: &str, io: &ProbeIo) -> Result<String, ProbeError> {
    let target = io.read_link(format!("/dev/disk/{kind}/{value}")).await?;
    target
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| ProbeError::Unit("disk selector resolved to an invalid name".to_owned()))
}

fn selector_value(selector: &Selector) -> String {
    match selector {
        Selector::Name(value) | Selector::PartLabel(value) | Selector::PartUuid(value) => {
            value.clone()
        }
    }
}

fn parse_u64(bytes: &[u8]) -> Result<u64, ProbeError> {
    str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| ProbeError::Unit("invalid disk sector size".to_owned()))
}

fn parse_stat(bytes: &[u8]) -> Result<(u64, u64), ProbeError> {
    let mut fields = str::from_utf8(bytes)
        .map_err(|error| ProbeError::Unit(format!("invalid disk stat encoding: {error}")))?
        .split_whitespace();
    let read = fields
        .nth(2)
        .ok_or_else(|| ProbeError::Unit("short disk stat row".to_owned()))?
        .parse()
        .map_err(|error| ProbeError::Unit(format!("invalid disk read counter: {error}")))?;
    let written = fields
        .nth(3)
        .ok_or_else(|| ProbeError::Unit("short disk stat row".to_owned()))?
        .parse()
        .map_err(|error| ProbeError::Unit(format!("invalid disk write counter: {error}")))?;
    Ok((read, written))
}

fn rate(current: u64, previous: u64, elapsed: f64) -> f64 {
    if elapsed > 0.0 {
        current.saturating_sub(previous) as f64 / elapsed
    } else {
        0.0
    }
}

fn bar(value: f64, peak: f64) -> &'static str {
    if value <= 1.0 {
        return BARS[0];
    }
    let level = (value.ln() / peak.ln() * 8.0).clamp(1.0, 8.0) as usize;
    BARS[level]
}
