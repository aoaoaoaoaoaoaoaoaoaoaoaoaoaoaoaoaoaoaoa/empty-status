use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::fs::File;
use std::io::{BufRead, BufReader};
use sysinfo::Components;

use crate::core::{Unit, BROWN, VIOLET};
use crate::display::{color_by_pct, color_by_pct_custom};
use crate::render::markup::Markup;
use crate::{impl_handle_click_rotate_mode, mode_enum, register_unit};

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
        let (total, user, kernel) = Self::read_cpu_times().unwrap_or((0, 0, 0));

        Self {
            mode: DisplayMode::Combined,
            prev_total: total,
            prev_user: user,
            prev_kernel: kernel,
        }
    }

    fn read_cpu_times() -> Result<(u64, u64, u64)> {
        let file = File::open("/proc/stat")?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();

        reader.read_line(&mut line)?;

        let parts: Vec<u64> = line
            .split_whitespace()
            .skip(1) // Skip "cpu" prefix
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();

        if parts.len() < 4 {
            return Err(anyhow!("Invalid CPU stat format"));
        }

        let total: u64 = parts.iter().sum();
        let user = parts[0] + parts[1]; // user + nice
        let kernel = parts[2]; // system

        Ok((total, user, kernel))
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
}
#[async_trait]
impl Unit for Cpu {
    async fn read_formatted(&mut self) -> crate::core::Readout {
        let (total, user, kernel) = match Self::read_cpu_times() {
            Ok(times) => times,
            Err(_) => {
                return crate::core::Readout::err(Markup::text("read err").fg(BROWN));
            }
        };

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
        crate::core::Readout::ok(
            Markup::text("cpu ")
                .append(Markup::bracketed(load_str))
                .append(Markup::text(" "))
                .append(Markup::bracketed(Markup::text("temp ").append(temp_str))),
        )
    }

    impl_handle_click_rotate_mode!();
}

register_unit!(Cpu, CpuConfig);
