use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::core::{Unit, ORANGE, RED};
use crate::display::{color, RangeColorizer, RangeColorizerBuilder};
use crate::util::RotateEnum;
use crate::{impl_handle_click_rotate_mode, mode_enum, register_unit};

#[derive(Debug, Clone)]
struct CpuData {
    p_user: f64,
    p_kernel: f64,
    temp_c: Option<f64>,
    err_no_temp: bool,
    err_no_sensors: bool,
}

mode_enum!(Combined, Breakdown);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct CpuConfig {
    #[serde_inline_default(0.25)]
    poll_interval: f64,
}

#[derive(Debug)]
pub struct Cpu {
    cfg: CpuConfig,
    mode: DisplayMode,
    // TODO hot mess, look at rust native apis
    no_temps: bool,
    prev_total: u64,
    prev_user: u64,
    prev_kernel: u64,
    is_intel: bool,
    usage_queue: VecDeque<(f64, f64)>, // (user, kernel) fractions
    time_queue: VecDeque<u64>,         // time deltas
    temp_queue: VecDeque<f64>,         // temperatures
    queue_max_size: usize,
    col_load: RangeColorizer,
    col_temp: RangeColorizer,
}

impl Cpu {
    pub fn from_cfg(cfg: CpuConfig) -> Self {
        let queue_max_size = (2.0 / cfg.poll_interval) as usize;
        let is_intel = Self::check_is_intel();
        let no_temps = !is_intel && !Self::has_sensors();

        // Initial CPU times
        let (total, user, kernel) = Self::read_cpu_times().unwrap_or((0, 0, 0));
        let colorizer = RangeColorizerBuilder::default().build().unwrap();

        Self {
            cfg,
            mode: DisplayMode::Combined,
            is_intel,
            no_temps,
            prev_total: total,
            prev_user: user,
            prev_kernel: kernel,
            usage_queue: VecDeque::with_capacity(queue_max_size),
            time_queue: VecDeque::with_capacity(queue_max_size),
            temp_queue: VecDeque::with_capacity(queue_max_size),
            queue_max_size,
            col_load: colorizer,
            col_temp: RangeColorizerBuilder::default()
                .breakpoints(vec![40.0, 50.0, 70.0, 90.0])
                .build()
                .unwrap(),
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
    async fn read_formatted(&mut self) -> Result<String> {
        // Read CPU times
        let (total, user, kernel) = Self::read_cpu_times()?;

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

        // Update usage queue
        if self.usage_queue.len() >= self.queue_max_size {
            self.usage_queue.pop_front();
            self.time_queue.pop_front();
        }

        self.usage_queue.push_back((p_user, p_kernel));
        self.time_queue.push_back(d_total as u64);

        let mut data = CpuData {
            p_user,
            p_kernel,
            temp_c: None,
            err_no_temp: false,
            err_no_sensors: false,
        };

        // Read temperature
        if !self.no_temps {
            match self.read_temp() {
                Ok(temp) => {
                    // Update temp queue
                    if self.temp_queue.len() >= self.queue_max_size {
                        self.temp_queue.pop_front();
                    }
                    self.temp_queue.push_back(temp);

                    // Calculate average temp
                    let avg_temp: f64 =
                        self.temp_queue.iter().sum::<f64>() / self.temp_queue.len() as f64;
                    data.temp_c = Some(avg_temp);
                }
                Err(_) => {
                    data.err_no_temp = true;
                }
            }
        } else {
            data.err_no_sensors = true;
        }

        // Get CPU usage values
        let p_user = data.p_user * 100.0;
        let p_kernel = data.p_kernel * 100.0;
        let total_usage = p_user + p_kernel;

        // Format temperature
        let temp_str = if data.err_no_sensors {
            color("no sensors", RED)
        } else if data.err_no_temp {
            color("unk", ORANGE)
        } else if let Some(temp) = data.temp_c {
            format!(
                "{} C",
                color(format!("{temp:>3.0}"), self.col_temp.get(temp))
            )
        } else {
            color("err", RED)
        };

        // Format load
        let load_str = if self.mode == DisplayMode::Breakdown {
            format!(
                "u {} k {}",
                color(format!("{p_user:>3.0}%"), self.col_load.get(p_user)),
                color(format!("{p_kernel:>3.0}%"), self.col_load.get(p_kernel))
            )
        } else {
            format!(
                "load {}",
                color(
                    format!("{total_usage:>3.0}%"),
                    self.col_load.get(total_usage),
                )
            )
        };
        Ok(format!("cpu [{load_str}] [temp {temp_str}]"))
    }

    impl_handle_click_rotate_mode!();
}

register_unit!(Cpu, CpuConfig);
