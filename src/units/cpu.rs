use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use sysinfo::Components;

use crate::core::{BROWN, VIOLET};
use crate::display::{color_by_pct, color_by_pct_custom};
use crate::mode_enum;
use crate::render::markup::Markup;

mode_enum!(Combined, Breakdown);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct CpuConfig {}

#[derive(Debug)]
pub struct Cpu {
    mode: DisplayMode,
    prev_total: u64,
    prev_user: u64,
    prev_kernel: u64,
}

const KNOWN_CPU_HWMON_NAMES: &[&str] = &[
    "coretemp", "k10temp",
    // TODO needs to be expanded to be more robust.
];

impl Cpu {
    pub fn from_cfg(_cfg: CpuConfig) -> Self {
        Self {
            mode: DisplayMode::Combined,
            prev_total: 0,
            prev_user: 0,
            prev_kernel: 0,
        }
    }

    pub fn read_markup_from_proc_stat(&mut self, proc_stat: &[u8]) -> Markup {
        let line = std::str::from_utf8(proc_stat)
            .ok()
            .and_then(|s| s.lines().next())
            .unwrap_or_default();

        let parts: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if parts.len() < 4 {
            return Markup::text("read err").fg(BROWN);
        }

        let total: u64 = parts.iter().sum();
        let user = parts[0] + parts[1];
        let kernel = parts[2];
        self.read_markup_from_times(total, user, kernel)
    }

    fn read_markup_from_times(&mut self, total: u64, user: u64, kernel: u64) -> Markup {
        let d_total = total.saturating_sub(self.prev_total) as f64;
        let d_user = user.saturating_sub(self.prev_user) as f64;
        let d_kernel = kernel.saturating_sub(self.prev_kernel) as f64;

        self.prev_total = total;
        self.prev_user = user;
        self.prev_kernel = kernel;

        let p_user = if d_total > 0.0 { d_user / d_total } else { 0.0 };
        let p_kernel = if d_total > 0.0 {
            d_kernel / d_total
        } else {
            0.0
        };

        let p_user = p_user * 100.0;
        let p_kernel = p_kernel * 100.0;
        let total_usage = p_user + p_kernel;

        let temp_str = match Self::read_temp() {
            Err(_) => Markup::text("unk").fg(VIOLET),
            Ok(tc) => Markup::text(format!("{tc:>3.0}"))
                .fg(color_by_pct_custom(tc, &[40.0, 50.0, 70.0, 90.0]))
                .append(Markup::text(" C")),
        };

        let load_str = if self.mode == DisplayMode::Breakdown {
            Markup::text("u ")
                .append(Markup::text(format!("{p_user:>3.0}%")).fg(color_by_pct(p_user)))
                .append(Markup::text(" k "))
                .append(Markup::text(format!("{p_kernel:>3.0}%")).fg(color_by_pct(p_kernel)))
        } else {
            Markup::text("load ")
                .append(Markup::text(format!("{total_usage:>3.0}%")).fg(color_by_pct(total_usage)))
        };
        Markup::text("cpu ")
            .append(Markup::bracketed(load_str))
            .append(Markup::text(" "))
            .append(Markup::bracketed(Markup::text("temp ").append(temp_str)))
    }

    fn read_temp() -> Result<f64> {
        let cs = Components::new_with_refreshed_list();
        for component in &cs {
            if let Some((name, _)) = &component.label().split_once(' ') {
                if KNOWN_CPU_HWMON_NAMES.contains(name) {
                    if let Some(temp) = component.temperature() {
                        return Ok(temp as f64);
                    }
                }
            }
        }
        Err(anyhow!("No temperature sensors found in components"))
    }

    pub fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        self.mode = DisplayMode::next(self.mode);
    }

    pub fn fix_up_and_validate() {}
}
