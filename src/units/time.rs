use crate::core::{self, colorize_float, format_duration, Unit};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct Time {
    name: String,
    poll_interval: f64,
    format: String,
    doing_uptime: bool,
    uptime_file: String,
    loadavg_file: String,
    load_color_scale: Vec<f64>,
}

impl Time {
    pub fn new(format: String, poll_interval: f64) -> Self {
        let cpu_count = num_cpus::get() as f64;
        let load_color_scale = vec![
            cpu_count * 0.1,
            cpu_count * 0.25,
            cpu_count * 0.50,
            cpu_count * 0.75,
        ];

        Self {
            name: "RS9Time".to_string(),
            poll_interval,
            format,
            doing_uptime: false,
            uptime_file: "/proc/uptime".to_string(),
            loadavg_file: "/proc/loadavg".to_string(),
            load_color_scale,
        }
    }

    fn read_uptime(&self) -> Result<f64> {
        let file = File::open(&self.uptime_file)?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let uptime = line
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid uptime format"))?;
        let uptime: f64 = uptime.parse()?;

        Ok(uptime)
    }

    fn read_loadavg(&self) -> Result<(f64, f64, f64)> {
        let file = File::open(&self.loadavg_file)?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(anyhow::anyhow!("Invalid loadavg format"));
        }

        Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
    }
}

#[async_trait]
impl Unit for Time {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read(&mut self) -> Result<HashMap<String, Value>> {
        let mut result = HashMap::new();

        // Get current time
        let current_time = Local::now().format(&self.format).to_string();
        result.insert("datestr".to_string(), json!(current_time));

        // Get uptime
        let uptime = self.read_uptime()?;
        result.insert("uptime".to_string(), json!(uptime));

        // Get load averages
        let (l1, l5, l10) = self.read_loadavg()?;
        result.insert("loadavg_1".to_string(), json!(l1));
        result.insert("loadavg_5".to_string(), json!(l5));
        result.insert("loadavg_10".to_string(), json!(l10));

        Ok(result)
    }

    fn format(&self, data: &HashMap<String, Value>) -> String {
        if !self.doing_uptime {
            // Show date/time
            if let Some(Value::String(datestr)) = data.get("datestr") {
                return datestr.clone();
            }
            "Unknown time".to_string()
        } else {
            // Show uptime and load average
            let uptime = data.get("uptime").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let ut_s = format_duration(uptime);

            let mut load_strings = Vec::new();
            for key in ["1", "5", "10"] {
                let load_key = format!("loadavg_{key}");
                let load = data.get(&load_key).and_then(|v| v.as_f64()).unwrap_or(0.0);
                load_strings.push(colorize_float(load, 3, 2, &self.load_color_scale));
            }
            format!(
                "uptime [{}] load [{}/{}/{}]",
                ut_s, load_strings[0], load_strings[1], load_strings[2]
            )
        }
    }

    fn handle_click(&mut self, _click: core::ClickEvent) {
        // Toggle between time display and uptime display
        self.doing_uptime = !self.doing_uptime;
    }
}
