use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_inline_default::serde_inline_default;

use crate::{
    core::{Button, View},
    display::color_by_percent_remaining,
    probe_io::ProbeIo,
    render::{
        color::{BLUE, CYAN, GREEN, ORANGE, VIOLET},
        markup::Markup,
    },
    units::{ProbeError, Reaction, error_view, positive_duration},
    util::Ema,
};

pub const TIMEOUT: Duration = Duration::from_secs(2);
const MICRO_AMP_HOUR_TO_COULOMB: f64 = 0.0036;
const MICRO_WATT_HOUR_TO_JOULE: f64 = 0.0036;

cycle!(
    enum Mode {
        CurrentCapacity,
        DesignCapacity,
    }
);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde_inline_default(0)]
    bat_id: usize,
    #[serde_inline_default(2.5)]
    power_smoothing_sec: f64,
}

#[derive(Debug)]
pub struct Model {
    id: usize,
    path: String,
    mode: Mode,
    status: Status,
    power: Ema,
}

#[derive(Debug)]
pub struct Request {
    path: String,
}

#[derive(Debug)]
pub struct Sample {
    captured_at: Instant,
    status: Status,
    charge: Charge,
}

#[derive(Debug)]
struct Charge {
    current_fraction: f64,
    design_fraction: f64,
    power_watts: f64,
    energy_joules: f64,
    full_energy_joules: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Charging,
    Discharging,
    Full,
    Balanced,
    Unknown,
}

#[derive(Debug, Default)]
struct Uevent {
    present: Option<bool>,
    status: Option<String>,
    charge_now: Option<f64>,
    charge_full: Option<f64>,
    charge_full_design: Option<f64>,
    voltage_now: Option<f64>,
    current_now: Option<f64>,
    energy_now: Option<f64>,
    energy_full: Option<f64>,
    energy_full_design: Option<f64>,
    power_now: Option<f64>,
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        let smoothing = positive_duration("power_smoothing_sec", config.power_smoothing_sec)?;
        Ok(Self {
            id: config.bat_id,
            path: format!("/sys/class/power_supply/BAT{}/uevent", config.bat_id),
            mode: Mode::CurrentCapacity,
            status: Status::Unknown,
            power: Ema::new(smoothing),
        })
    }

    pub fn request(&self) -> Request {
        Request {
            path: self.path.clone(),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        let sample = match reply {
            Ok(sample) => sample,
            Err(error) => return error_view(&format!("bat{}", self.id), error),
        };
        let status = if sample.status == Status::Unknown && sample.charge.power_watts < 0.01 {
            Status::Balanced
        } else {
            sample.status
        };
        if status != self.status {
            self.status = status;
            self.power.reset();
        }
        let power = self
            .power
            .push(sample.charge.power_watts, sample.captured_at);
        let fraction = match self.mode {
            Mode::CurrentCapacity => sample.charge.current_fraction,
            Mode::DesignCapacity => sample.charge.design_fraction,
        };
        let percent = (fraction * 100.0).clamp(0.0, 999.0);
        let remaining = match status {
            Status::Charging if power > 0.0 => {
                Some((sample.charge.full_energy_joules - sample.charge.energy_joules) / power)
            }
            Status::Discharging if power > 0.0 => Some(sample.charge.energy_joules / power),
            _ => None,
        }
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map_or_else(|| "--:--".to_owned(), clock_duration);
        let capacity = Markup::text(format!("{percent:3.0}"))
            .fg(color_by_percent_remaining(percent))
            + Markup::text("%");
        let capacity = match self.mode {
            Mode::CurrentCapacity => Markup::bracketed(capacity),
            Mode::DesignCapacity => Markup::delimited("<", capacity, ">"),
        };

        View::ok(
            Markup::text("bat ")
                + capacity
                + Markup::text(" ")
                + status.markup()
                + Markup::text(format!(" {power:2.2} W "))
                + Markup::bracketed(Markup::text(format!("{remaining} rem"))),
        )
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        Reaction::refresh()
    }
}

impl Status {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::to_ascii_lowercase).as_deref() {
            Some("charging") => Self::Charging,
            Some("discharging") => Self::Discharging,
            Some("full") => Self::Full,
            _ => Self::Unknown,
        }
    }

    fn markup(self) -> Markup {
        match self {
            Self::Discharging => Markup::text("DIS").fg(ORANGE),
            Self::Charging => Markup::text("CHR").fg(GREEN),
            Self::Full => Markup::text("FUL").fg(CYAN),
            Self::Balanced => Markup::text("BAL").fg(BLUE),
            Self::Unknown => Markup::text("UNK").fg(VIOLET),
        }
    }
}

impl Uevent {
    fn parse(bytes: &[u8]) -> Result<Self, ProbeError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| ProbeError::Unit(format!("invalid uevent encoding: {error}")))?;
        let mut event = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let number = || value.trim().parse::<f64>().ok();
            match key.strip_prefix("POWER_SUPPLY_").unwrap_or(key) {
                "PRESENT" => event.present = Some(value.trim() != "0"),
                "STATUS" => event.status = Some(value.trim().to_owned()),
                "CHARGE_NOW" => event.charge_now = number(),
                "CHARGE_FULL" => event.charge_full = number(),
                "CHARGE_FULL_DESIGN" => event.charge_full_design = number(),
                "VOLTAGE_NOW" => event.voltage_now = number(),
                "CURRENT_NOW" => event.current_now = number(),
                "ENERGY_NOW" => event.energy_now = number(),
                "ENERGY_FULL" => event.energy_full = number(),
                "ENERGY_FULL_DESIGN" => event.energy_full_design = number(),
                "POWER_NOW" => event.power_now = number(),
                _ => {}
            }
        }
        Ok(event)
    }

    fn charge(&self) -> Option<Charge> {
        self.charge_from_energy()
            .or_else(|| self.charge_from_current())
            .filter(Charge::valid)
    }

    fn charge_from_energy(&self) -> Option<Charge> {
        let energy = self.energy_now? * MICRO_WATT_HOUR_TO_JOULE;
        let full = self.energy_full? * MICRO_WATT_HOUR_TO_JOULE;
        let design = self.energy_full_design? * MICRO_WATT_HOUR_TO_JOULE;
        Some(Charge {
            current_fraction: energy / full,
            design_fraction: energy / design,
            power_watts: (self.power_now? / 1e6).abs(),
            energy_joules: energy,
            full_energy_joules: full,
        })
    }

    fn charge_from_current(&self) -> Option<Charge> {
        let charge = self.charge_now?;
        let full = self.charge_full?;
        let design = self.charge_full_design?;
        let voltage = self.voltage_now? / 1e6;
        Some(Charge {
            current_fraction: charge / full,
            design_fraction: charge / design,
            power_watts: (self.current_now? / 1e6 * voltage).abs(),
            energy_joules: charge * MICRO_AMP_HOUR_TO_COULOMB * voltage,
            full_energy_joules: full * MICRO_AMP_HOUR_TO_COULOMB * voltage,
        })
    }
}

impl Charge {
    fn valid(&self) -> bool {
        self.current_fraction.is_finite()
            && self.design_fraction.is_finite()
            && self.current_fraction >= 0.0
            && self.design_fraction >= 0.0
            && self.power_watts.is_finite()
            && self.energy_joules.is_finite()
            && self.full_energy_joules.is_finite()
            && self.energy_joules >= 0.0
            && self.full_energy_joules > 0.0
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    let bytes = io.read(request.path).await?;
    let event = Uevent::parse(&bytes)?;
    if event.present == Some(false) {
        return Err(ProbeError::Unit("absent".to_owned()));
    }
    let charge = event
        .charge()
        .ok_or_else(|| ProbeError::Unit("invalid battery data".to_owned()))?;
    Ok(Sample {
        captured_at: Instant::now(),
        status: Status::parse(event.status.as_deref()),
        charge,
    })
}

fn clock_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::Uevent;

    #[test]
    fn parses_energy_battery() {
        let event = Uevent::parse(
            b"POWER_SUPPLY_PRESENT=1\nPOWER_SUPPLY_STATUS=Discharging\nPOWER_SUPPLY_ENERGY_NOW=50000000\nPOWER_SUPPLY_ENERGY_FULL=100000000\nPOWER_SUPPLY_ENERGY_FULL_DESIGN=120000000\nPOWER_SUPPLY_POWER_NOW=10000000\n",
        );
        assert!(event.is_ok());
        let Ok(event) = event else { return };
        let charge = event.charge();
        assert!(charge.is_some());
        let Some(charge) = charge else { return };
        assert!((charge.current_fraction - 0.5).abs() < f64::EPSILON);
    }
}
