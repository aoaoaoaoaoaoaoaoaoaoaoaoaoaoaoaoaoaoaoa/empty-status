use crate::core::{color, get_color, make_temp_color_str, Unit, ORANGE, RED};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

pub struct CPU {
    name: String,
    poll_interval: f64,
    show_breakdown: bool,
    is_intel: bool,
    no_temps: bool,
    prev_total: u64,
    prev_user: u64,
    prev_kernel: u64,
    usage_queue: VecDeque<(f64, f64)>, // (user, kernel) fractions
    time_queue: VecDeque<u64>,         // time deltas
    temp_queue: VecDeque<f64>,         // temperatures
    queue_max_size: usize,
}

impl CPU {
    pub fn new(poll_interval: f64) -> Self {
        let queue_max_size = (2.0 / poll_interval) as usize;
        let is_intel = Self::check_is_intel();
        let no_temps = !is_intel && !Self::has_sensors();

        // Initial CPU times
        let (total, user, kernel) = Self::read_cpu_times().unwrap_or((0, 0, 0));

        Self {
            name: "RS9CPU".to_string(),
            poll_interval,
            show_breakdown: false,
            is_intel,
            no_temps,
            prev_total: total,
            prev_user: user,
            prev_kernel: kernel,
            usage_queue: VecDeque::with_capacity(queue_max_size),
            time_queue: VecDeque::with_capacity(queue_max_size),
            temp_queue: VecDeque::with_capacity(queue_max_size),
            queue_max_size,
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
impl Unit for CPU {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read(&mut self) -> Result<HashMap<String, Value>> {
        let mut result = HashMap::new();

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

        result.insert("p_u".to_string(), json!(p_user));
        result.insert("p_k".to_string(), json!(p_kernel));

        // Update usage queue
        if self.usage_queue.len() >= self.queue_max_size {
            self.usage_queue.pop_front();
            self.time_queue.pop_front();
        }

        self.usage_queue.push_back((p_user, p_kernel));
        self.time_queue.push_back(d_total as u64);

        // Calculate average usage
        let mut total_user = 0.0;
        let mut total_kernel = 0.0;
        let mut _total_time = 0.0;

        for &(u, k) in &self.usage_queue {
            total_user += u;
            total_kernel += k;
        }

        for &t in &self.time_queue {
            _total_time += t as f64;
        }

        let queue_size = self.usage_queue.len() as f64;
        if queue_size > 0.0 {
            result.insert("p_u_avg".to_string(), json!(total_user / queue_size));
            result.insert("p_k_avg".to_string(), json!(total_kernel / queue_size));
        }

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
                    result.insert("temp_C".to_string(), json!(avg_temp));
                }
                Err(_) => {
                    result.insert("err_no_temp".to_string(), json!(true));
                }
            }
        } else {
            result.insert("err_no_sensors".to_string(), json!(true));
        }

        Ok(result)
    }

    fn format(&self, data: &HashMap<String, Value>) -> String {
        // Get CPU usage values
        let p_user = data.get("p_u").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;

        let p_kernel = data.get("p_k").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;

        let total_usage = p_user + p_kernel;

        // Format temperature
        let temp_str = if data.contains_key("err_no_sensors") {
            color("no sensors", RED)
        } else if data.contains_key("err_no_temp") {
            color("unk", ORANGE)
        } else if let Some(Value::Number(temp_val)) = data.get("temp_C") {
            if let Some(temp) = temp_val.as_f64() {
                format!("{} C", make_temp_color_str(temp))
            } else {
                color("err", RED)
            }
        } else {
            color("err", RED)
        };

        // Format load
        let load_str = if self.show_breakdown {
            format!(
                "u {} k {}",
                color(
                    format!("{:.0}%", p_user),
                    &get_color(
                        p_user,
                        &[20.0, 40.0, 60.0, 80.0],
                        &["#81A2BE", "#B5BD68", "#F0C674", "#DE935F", "#CC6666"],
                        false
                    )
                ),
                color(
                    format!("{:.0}%", p_kernel),
                    &get_color(
                        p_kernel,
                        &[20.0, 40.0, 60.0, 80.0],
                        &["#81A2BE", "#B5BD68", "#F0C674", "#DE935F", "#CC6666"],
                        false
                    )
                )
            )
        } else {
            format!(
                "load {}",
                color(
                    format!("{:.0}%", total_usage),
                    &get_color(
                        total_usage,
                        &[20.0, 40.0, 60.0, 80.0],
                        &["#81A2BE", "#B5BD68", "#F0C674", "#DE935F", "#CC6666"],
                        false
                    )
                )
            )
        };

        format!("cpu [{}] [temp {}]", load_str, temp_str)
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        // Toggle between combined and breakdown view
        self.show_breakdown = !self.show_breakdown;
    }
}
