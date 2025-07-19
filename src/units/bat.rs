use crate::core::{color, get_color, ClickEvent, Unit, BLUE, CYAN, GREEN, ORANGE, RED, VIOLET};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone, PartialEq, Eq)]
enum BatDisplayMode {
    CurCapacity,
    DesignCapacity,
    VoltageCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum BatStatus {
    Charging,
    Discharging,
    Full,
    Balanced,
    Unknown,
    Other,
}

impl BatStatus {
    pub fn from_uevent(u: &HashMap<String, String>) -> Self {
        match u.get("status") {
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "charging" => Self::Charging,
                "discharging" => Self::Discharging,
                "full" => Self::Full,
                "unknown" => Self::Unknown,
                _ => Self::Other,
            },
            None => Self::Other,
        }
    }
    pub fn state_string(&self) -> String {
        match self {
            Self::Discharging => color("dis", ORANGE),
            Self::Charging => color("chr", GREEN),
            Self::Full => color("ful", BLUE),
            Self::Balanced => color("bal", CYAN),
            _ => color("unk", VIOLET),
        }
    }
}

pub struct Bat {
    poll_interval: f64,
    min_rem_smooth: Option<f64>,
    mode: BatDisplayMode,
    cur_status: BatStatus,
    p_hist: VecDeque<f64>,
    p_hist_maxlen: usize,
    uevent_path: String,
}

impl Bat {
    pub fn new(poll_interval: f64, bat_id: usize) -> Self {
        let uevent_path = format!("/sys/class/power_supply/BAT{bat_id}/uevent");
        let p_hist_maxlen = (10.0 / poll_interval).ceil() as usize;
        Self {
            poll_interval,
            min_rem_smooth: None,
            mode: BatDisplayMode::CurCapacity,
            cur_status: BatStatus::Unknown,
            p_hist: VecDeque::with_capacity(p_hist_maxlen),
            p_hist_maxlen,
            uevent_path,
        }
    }

    fn parse_uevent(&self) -> Result<HashMap<String, String>> {
        let file = File::open(&self.uevent_path)?;
        let reader = BufReader::new(file);
        let mut out = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            if let Some((k, v)) = line.trim().split_once('=') {
                let key = k
                    .strip_prefix("POWER_SUPPLY_")
                    .unwrap_or(k)
                    .to_ascii_lowercase();
                let val = v.trim().to_ascii_lowercase();
                out.insert(key, val);
            }
        }
        Ok(out)
    }
}

const UH_TO_SI: f64 = 0.0036;

pub struct BatteryInfo {
    pub charged_frac: f64,
    pub charged_frac_design: f64,
    pub power: f64,
    pub energy: f64,
    pub energy_max: f64,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
}

impl BatteryInfo {
    pub fn from_charge(u: &HashMap<String, String>) -> Option<Self> {
        let charge_now = u.get("charge_now")?.parse::<i64>().ok()?;
        let charge_full = u.get("charge_full")?.parse::<i64>().ok()?;
        let charge_full_design = u.get("charge_full_design")?.parse::<i64>().ok()?;
        let voltage_now = u.get("voltage_now")?.parse::<i64>().ok()?;
        let voltage_min_design = u.get("voltage_min_design")?.parse::<i64>().ok()?;
        let current_now = u.get("current_now")?.parse::<i64>().ok()?;

        let q = UH_TO_SI * charge_now as f64;
        let qmx = UH_TO_SI * charge_full as f64;
        let qmxd = UH_TO_SI * charge_full_design as f64;

        let voltage = voltage_now as f64 / 1e6;
        let vmn = voltage_min_design as f64 / 1e6;
        let current = current_now as f64 / 1e6;

        let power = current * voltage;

        let vmx = voltage * (qmx / q);
        let energy_max = qmx * (vmn + vmx) / 2.0;

        let vmxd = voltage * (qmxd / q);
        let energy_max_design = qmxd * (vmn + vmxd) / 2.0;

        let energy = q * (vmn + q * (vmxd - vmn) / (2.0 * qmxd));

        let charged_frac = energy / energy_max;
        let charged_frac_design = energy / energy_max_design;

        Some(Self {
            charged_frac,
            charged_frac_design,
            power,
            energy,
            energy_max,
            voltage: Some(voltage),
            current: Some(current),
        })
    }

    /// Construct from `energy_now`, `energy_full`, etc.
    pub fn from_energy(u: &HashMap<String, String>) -> Option<Self> {
        let energy_now = u.get("energy_now")?.parse::<i64>().ok()?;
        let energy_full = u.get("energy_full")?.parse::<i64>().ok()?;
        let energy_full_design = u.get("energy_full_design")?.parse::<i64>().ok()?;
        let power_now = u.get("power_now")?.parse::<i64>().ok()?;

        let energy = UH_TO_SI * energy_now as f64;
        let energy_max = UH_TO_SI * energy_full as f64;
        let energy_max_design = UH_TO_SI * energy_full_design as f64;
        let power = power_now as f64 / 1e6;

        let charged_frac = energy / energy_max;
        let charged_frac_design = energy / energy_max_design;

        Some(Self {
            charged_frac,
            charged_frac_design,
            power,
            energy,
            energy_max,
            voltage: None,
            current: None,
        })
    }
}

#[async_trait]
impl Unit for Bat {
    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read_formatted(&mut self) -> Result<String> {
        let mut missing = false;
        let uevent = match self.parse_uevent() {
            Ok(map) => map,
            Err(_) => {
                missing = true;
                HashMap::new()
            }
        };

        if missing || uevent.get("present").map(|v| v == "0").unwrap_or(false) {
            return Ok(color("No battery", RED));
        }

        let bi =
            match BatteryInfo::from_charge(&uevent).or_else(|| BatteryInfo::from_energy(&uevent)) {
                Some(bi) => bi,
                None => {
                    return Ok(color("invalid data", RED));
                }
            };

        // Power smoothing
        if self.p_hist.len() == self.p_hist_maxlen {
            self.p_hist.pop_front();
        }
        self.p_hist.push_back(bi.power);
        let av_p = if !self.p_hist.is_empty() {
            self.p_hist.iter().sum::<f64>() / self.p_hist.len() as f64
        } else {
            bi.power
        };

        let pct = if self.mode == BatDisplayMode::DesignCapacity {
            100.0 * bi.charged_frac_design
        } else {
            100.0 * bi.charged_frac
        };
        let pct_str = color(
            format!("{pct:3.0}"),
            &get_color(
                pct,
                &[20.0, 40.0, 60.0, 80.0],
                &[BLUE, GREEN, ORANGE, RED, VIOLET],
                true,
            ),
        );

        // Determine battery state
        let mut bs = BatStatus::from_uevent(&uevent);
        if bs == BatStatus::Other && av_p == 0.0 {
            bs = BatStatus::Balanced;
        }

        // Reset smoothing if status changes
        if bs != self.cur_status {
            self.min_rem_smooth = None;
            self.cur_status = bs;
            self.p_hist.clear();
        }

        // Estimate time remaining in seconds
        let sec_rem: Option<f64> = match bs {
            BatStatus::Charging => {
                if av_p > 0.0 {
                    Some((bi.energy_max - bi.energy) / av_p)
                } else {
                    None
                }
            }
            BatStatus::Discharging => {
                if av_p > 0.0 {
                    Some(bi.energy / av_p)
                } else {
                    None
                }
            }
            _ => None,
        };

        let rem_string = match sec_rem {
            Some(sec) => {
                let mins = (sec / 60.0).round() as i64;
                let hours = mins / 60;
                let min_rem = mins % 60;
                format!("{hours:02}:{min_rem:02}")
            }
            None => String::from("--:--"),
        };

        if self.mode != BatDisplayMode::VoltageCurrent {
            let brackets = match self.mode {
                BatDisplayMode::CurCapacity => ["{", "}"],
                BatDisplayMode::DesignCapacity => ["<", ">"],
                _ => unreachable!(), // TODO fugly learn how to handle shared destructure with
                                     // fallthrough
            };

            Ok(format!(
                "bat {}{}%{} [{} rem, {}]",
                brackets[0],
                pct_str,
                brackets[1],
                rem_string,
                bs.state_string()
            ))
        } else {
            let voltage = bi.voltage.map_or("--".to_string(), |v| format!("{v:.2} V"));
            let current = bi.current.map_or("--".to_string(), |c| format!("{c:.2} A"));
            Ok(format!("({voltage} | {current})"))
        }
    }

    fn handle_click(&mut self, _click: ClickEvent) {
        match self.mode {
            BatDisplayMode::CurCapacity => {
                self.mode = BatDisplayMode::DesignCapacity;
            }
            BatDisplayMode::DesignCapacity => {
                self.mode = BatDisplayMode::VoltageCurrent;
            }
            BatDisplayMode::VoltageCurrent => {
                self.mode = BatDisplayMode::CurCapacity;
            }
        }
    }
}
