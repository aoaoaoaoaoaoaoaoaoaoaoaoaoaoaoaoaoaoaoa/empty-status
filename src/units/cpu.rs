use std::time::Duration;

use serde::Deserialize;
use sysinfo::Components;

use crate::{
    core::{Button, View},
    display::{color_by_percent, color_by_thresholds},
    probe_io::ProbeIo,
    render::{color::VIOLET, markup::Markup},
    units::{ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(3);
const PROC_STAT: &str = "/proc/stat";
const KNOWN_CPU_HWMON_NAMES: &[&str] = &["coretemp", "k10temp"];

cycle!(
    enum Mode {
        Combined,
        Breakdown,
    }
);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {}

#[derive(Debug)]
pub struct Model {
    mode: Mode,
    previous: Option<CpuTimes>,
}

persist!(Model.mode);

#[derive(Debug, Clone, Copy)]
pub struct Request;

#[derive(Debug)]
pub struct Sample {
    times: CpuTimes,
    temperature: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    total: u64,
    busy: u64,
    user: u64,
    kernel: u64,
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub const fn new(_config: Config) -> Self {
        Self {
            mode: Mode::Combined,
            previous: None,
        }
    }

    pub const fn request() -> Request {
        Request
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        let sample = match reply {
            Ok(sample) => sample,
            Err(error) => return error_view("cpu", error),
        };
        let Some(previous) = self.previous.replace(sample.times) else {
            return View::loading("cpu");
        };

        let elapsed = sample.times.total.saturating_sub(previous.total) as f64;
        let percentage = |current: u64, old: u64| {
            if elapsed == 0.0 {
                0.0
            } else {
                current.saturating_sub(old) as f64 / elapsed * 100.0
            }
        };
        let busy = percentage(sample.times.busy, previous.busy);
        let user = percentage(sample.times.user, previous.user);
        let kernel = percentage(sample.times.kernel, previous.kernel);

        let load = match self.mode {
            Mode::Combined => {
                Markup::text("load ")
                    + Markup::text(format!("{busy:>3.0}%")).fg(color_by_percent(busy))
            }
            Mode::Breakdown => {
                Markup::text("u ")
                    + Markup::text(format!("{user:>3.0}%")).fg(color_by_percent(user))
                    + Markup::text(" k ")
                    + Markup::text(format!("{kernel:>3.0}%")).fg(color_by_percent(kernel))
            }
        };
        let temperature = sample.temperature.map_or_else(
            || Markup::text("unk").fg(VIOLET),
            |value| {
                Markup::text(format!("{value:>3.0}"))
                    .fg(color_by_thresholds(value, [40.0, 50.0, 70.0, 90.0]))
                    + Markup::text(" C")
            },
        );

        View::ok(
            Markup::text("cpu ")
                + Markup::bracketed(load)
                + Markup::text(" ")
                + Markup::bracketed(Markup::text("temp ") + temperature),
        )
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        Reaction::refresh()
    }
}

pub async fn probe(_request: Request, io: &ProbeIo) -> Reply {
    let stat = io.read(PROC_STAT).await?;
    let times = parse_times(&stat)?;
    let temperature = io.blocking(read_temperature).await?;
    Ok(Sample { times, temperature })
}

fn parse_times(stat: &[u8]) -> Result<CpuTimes, ProbeError> {
    let line = std::str::from_utf8(stat)
        .ok()
        .and_then(|text| text.lines().next())
        .ok_or_else(|| ProbeError::Unit("invalid /proc/stat encoding".to_owned()))?;
    let fields = line
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ProbeError::Unit(format!("invalid /proc/stat counter: {error}")))?;
    if fields.len() < 8 {
        return Err(ProbeError::Unit("short /proc/stat cpu row".to_owned()));
    }

    let total = fields[..8].iter().sum();
    let idle = fields[3].saturating_add(fields[4]);
    Ok(CpuTimes {
        total,
        busy: total.saturating_sub(idle),
        user: fields[0].saturating_add(fields[1]),
        kernel: fields[2]
            .saturating_add(fields[5])
            .saturating_add(fields[6]),
    })
}

fn read_temperature() -> Option<f64> {
    Components::new_with_refreshed_list()
        .iter()
        .filter(|component| {
            component
                .label()
                .split_whitespace()
                .next()
                .is_some_and(|name| KNOWN_CPU_HWMON_NAMES.contains(&name))
        })
        .filter_map(|component| component.temperature())
        .map(f64::from)
        .max_by(f64::total_cmp)
}

#[cfg(test)]
mod tests {
    use super::parse_times;

    #[test]
    fn excludes_guest_counters_from_total() {
        let times = parse_times(b"cpu  10 2 3 100 5 7 11 13 1000 1000\n");
        assert!(times.is_ok());
        let Ok(times) = times else { return };
        assert_eq!(times.total, 151);
        assert_eq!(times.busy, 46);
        assert_eq!(times.user, 12);
        assert_eq!(times.kernel, 21);
    }
}
