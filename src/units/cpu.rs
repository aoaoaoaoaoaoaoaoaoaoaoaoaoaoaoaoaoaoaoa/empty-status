use crate::util::RotateEnum;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::core::{Unit, BROWN, RED, VIOLET};
use crate::display::{color, color_by_pct, color_by_pct_custom};
use crate::{impl_handle_click_rotate_mode, mode_enum, register_unit};

mode_enum!(Combined, Breakdown);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct CpuConfig {}

#[derive(Debug)]
pub struct Cpu {
    mode: DisplayMode,
    // TODO hot mess, look at rust native apis
    no_temps: bool,
    // TODO use sysinfo -- iterating over cpus might be annoying though, its api
    // looks a little bare bones...
    prev_total: u64,
    prev_user: u64,
    prev_kernel: u64,
    is_intel: bool,
}

impl Cpu {
    pub fn from_cfg(_cfg: CpuConfig) -> Self {
        let is_intel = Self::check_is_intel();
        let no_temps = !is_intel && !Self::has_sensors();

        // Initial CPU times
        let (total, user, kernel) = Self::read_cpu_times().unwrap_or((0, 0, 0));

        Self {
            mode: DisplayMode::Combined,
            is_intel,
            no_temps,
            prev_total: total,
            prev_user: user,
            prev_kernel: kernel,
        }
    }

    fn check_is_intel() -> bool {
        let output = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        output.contains("Intel")
    }

    fn has_sensors() -> bool {
        Command::new("which")
            .arg("sensors")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
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

    fn read_temp(&self) -> Result<f64> {
        if self.is_intel {
            self.read_temp_intel()
        } else {
            self.read_temp_amd()
        }
    }

    // TODO jank use Sensors
    fn read_temp_intel(&self) -> Result<f64> {
        let mut temp_sum = 0.0;
        let mut count = 0;

        for entry in fs::read_dir("/sys/class/thermal/")? {
            let entry = entry?;
            let path = entry.path();
            if path.to_string_lossy().contains("thermal_zone") {
                if let Ok(temp_path) = path.join("temp").into_os_string().into_string() {
                    if let Ok(temp_str) = fs::read_to_string(temp_path) {
                        if let Ok(temp) = temp_str.trim().parse::<f64>() {
                            temp_sum += temp / 1000.0; // Convert from milli-degrees to degrees
                            count += 1;
                        }
                    }
                }
            }
        }

        if count > 0 {
            Ok(temp_sum / count as f64)
        } else {
            Err(anyhow!("No temperature sensors found"))
        }
    }

    // TODO jank use Sensors
    fn read_temp_amd(&self) -> Result<f64> {
        let output = Command::new("/usr/bin/sensors").output()?;
        let output = String::from_utf8(output.stdout)?;

        // Parse temperatures from sensors output
        // This is a simplified version and might need refinement for different AMD CPUs
        let mut temps = Vec::new();
        for line in output.lines() {
            if !(line.contains("Tdie") || line.contains("Tccd")) {
                continue;
            }
            let Some(temp_str) = line.split('+').nth(1) else {
                continue;
            };
            let Some(temp_str) = temp_str.split('°').next() else {
                continue;
            };
            if let Ok(temp) = temp_str.trim().parse::<f64>() {
                temps.push(temp);
            }
        }
        if temps.is_empty() {
            Err(anyhow!("Could not parse AMD temperature"))
        } else {
            Ok(temps.iter().sum::<f64>() / temps.len() as f64)
        }
    }
}
#[async_trait]
impl Unit for Cpu {
    async fn read_formatted(&mut self) -> String {
        // Read CPU times
        let (total, user, kernel) = match Self::read_cpu_times() {
            Ok(times) => times,
            Err(_) => {
                self.no_temps = true; // If we can't read times, assume no sensors
                return color("read err", BROWN);
            }
        };

        let d_total = total.saturating_sub(self.prev_total) as f64;
        let d_user = user.saturating_sub(self.prev_user) as f64;
        let d_kernel = kernel.saturating_sub(self.prev_kernel) as f64;

        // Update previous values
        self.prev_total = total;
        self.prev_user = user;
        self.prev_kernel = kernel;

        // Calculate usage fractions
        let p_user = if d_total > 0.0 { d_user / d_total } else { 0.0 };
        let p_kernel = if d_total > 0.0 {
            d_kernel / d_total
        } else {
            0.0
        };

        let p_user = p_user * 100.0;
        let p_kernel = p_kernel * 100.0;
        let total_usage = p_user + p_kernel;

        let temp_str = if self.no_temps {
            color("no sensors", RED)
        } else {
            match self.read_temp() {
                Err(_) => color("unk", VIOLET),
                Ok(tc) => {
                    format!(
                        "{} C",
                        color(
                            format!("{tc:>3.0}"),
                            color_by_pct_custom(tc, &[40.0, 50.0, 70.0, 90.0])
                        )
                    )
                }
            }
        };

        let load_str = if self.mode == DisplayMode::Breakdown {
            format!(
                "u {} k {}",
                color(format!("{p_user:>3.0}%"), color_by_pct(p_user)),
                color(format!("{p_kernel:>3.0}%"), color_by_pct(p_kernel))
            )
        } else {
            format!(
                "load {}",
                color(format!("{total_usage:>3.0}%"), color_by_pct(total_usage),)
            )
        };
        format!("cpu [{load_str}] [temp {temp_str}]")
    }

    impl_handle_click_rotate_mode!();
}

register_unit!(Cpu, CpuConfig);
