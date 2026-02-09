use crate::core::{BLUE, CYAN, GREEN, ORANGE, RED, VIOLET};
use crate::display::color_by_pct_rev;
use crate::mode_enum;
use crate::render::markup::Markup;
use crate::util::{Ema, Smoother};
use anyhow::Result;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::collections::HashMap;
use std::time::Instant;

mode_enum!(CurCapacity, DesignCapacity);

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
    pub fn state_markup(self) -> Markup {
        match self {
            Self::Discharging => Markup::text("DIS").fg(ORANGE),
            Self::Charging => Markup::text("CHR").fg(GREEN),
            Self::Full => Markup::text("FUL").fg(CYAN),
            Self::Balanced => Markup::text("BAL").fg(BLUE),
            _ => Markup::text("UNK").fg(VIOLET),
        }
    }
}

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct BatConfig {
    pub bat_id: usize,
    #[serde_inline_default(2.5)]
    pub power_smoothing_sec: f64,
}

#[derive(Debug)]
pub struct Bat {
    cfg: BatConfig,
    mode: DisplayMode,
    cur_status: BatStatus,
    uevent_path: String,
    power_ema: Ema<f64>,
}

impl Bat {
    pub fn from_cfg(cfg: BatConfig) -> Self {
        // TODO seems fragile? use a crate etc.
        let uevent_path = format!("/sys/class/power_supply/BAT{}/uevent", cfg.bat_id);
        Self {
            mode: DisplayMode::CurCapacity,
            cur_status: BatStatus::Unknown,
            uevent_path,
            power_ema: Ema::new(cfg.power_smoothing_sec),
            cfg,
        }
    }

    fn parse_uevent_bytes(bytes: &[u8]) -> Result<HashMap<String, String>> {
        let mut out = HashMap::new();
        let s = std::str::from_utf8(bytes)?;
        for line in s.lines() {
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
        })
    }

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
        })
    }
}

impl Bat {
    pub fn read_markup_from_bytes(&mut self, bytes: &[u8]) -> Markup {
        let mut missing = false;
        let uevent = if let Ok(map) = Self::parse_uevent_bytes(bytes) {
            map
        } else {
            missing = true;
            HashMap::new()
        };

        if missing || uevent.get("present").is_some_and(|v| v == "0") {
            return Markup::text("No battery").fg(RED);
        }

        let bi =
            match BatteryInfo::from_charge(&uevent).or_else(|| BatteryInfo::from_energy(&uevent)) {
                Some(bi) => bi,
                None => {
                    return Markup::text("invalid data").fg(RED);
                }
            };

        let p_smooth = *self
            .power_ema
            .feed_and_read(bi.power, Instant::now())
            .unwrap_or(&bi.power);

        let pct = if self.mode == DisplayMode::DesignCapacity {
            100.0 * bi.charged_frac_design
        } else {
            100.0 * bi.charged_frac
        };

        let pct_str = Markup::text(format!("{pct:3.0}")).fg(color_by_pct_rev(pct));

        let mut bs = BatStatus::from_uevent(&uevent);
        if bs == BatStatus::Other && p_smooth == 0.0 {
            bs = BatStatus::Balanced;
        }

        if bs != self.cur_status {
            self.cur_status = bs;
            self.power_ema = Ema::new(self.cfg.power_smoothing_sec);
        }

        let sec_rem: Option<f64> = match bs {
            BatStatus::Charging => {
                if p_smooth > 0.0 {
                    Some((bi.energy_max - bi.energy) / p_smooth)
                } else {
                    None
                }
            }
            BatStatus::Discharging => {
                if p_smooth > 0.0 {
                    Some(bi.energy / p_smooth)
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
        let (br0, br1) = match self.mode {
            DisplayMode::CurCapacity => ("[", "]"),
            DisplayMode::DesignCapacity => ("&lt;", "&gt;"),
        };
        Markup::text("bat ")
            .append(Markup::delimited(
                br0,
                pct_str.append(Markup::text("%")),
                br1,
            ))
            .append(Markup::text(" "))
            .append(bs.state_markup())
            .append(Markup::text(format!(" {p_smooth:2.2} W ")))
            .append(Markup::bracketed(Markup::text(format!("{rem_string} rem"))))
    }

    pub fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        self.mode = DisplayMode::next(self.mode);
    }

    pub fn fix_up_and_validate() {}

    pub fn uevent_path(&self) -> &str {
        &self.uevent_path
    }
}
