use crate::core::{self, colorize_float, format_duration, Unit};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeData {
    datestr: String,
    uptime: f64,
    loadavg_1: f64,
    loadavg_5: f64,
    loadavg_10: f64,
}

pub struct Time {
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
    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read_formatted(&mut self) -> Result<String> {
        // Get current time
        let current_time = Local::now().format(&self.format).to_string();

        // Get uptime
        let uptime = self.read_uptime()?;

        // Get load averages
        let (l1, l5, l10) = self.read_loadavg()?;

        let data = TimeData {
            datestr: current_time,
            uptime,
            loadavg_1: l1,
            loadavg_5: l5,
            loadavg_10: l10,
        };

        if !self.doing_uptime {
            // Show date/time
            Ok(data.datestr)
        } else {
            // Show uptime and load average
            let ut_s = format_duration(data.uptime);

            let mut load_strings = Vec::new();
            load_strings.push(colorize_float(data.loadavg_1, 3, 2, &self.load_color_scale));
            load_strings.push(colorize_float(data.loadavg_5, 3, 2, &self.load_color_scale));
            load_strings.push(colorize_float(
                data.loadavg_10,
                3,
                2,
                &self.load_color_scale,
            ));

            Ok(format!(
                "uptime [{}] load [{}/{}/{}]",
                ut_s, load_strings[0], load_strings[1], load_strings[2]
            ))
        }
    }

    fn handle_click(&mut self, _click: core::ClickEvent) {
        // Toggle between time display and uptime display
        self.doing_uptime = !self.doing_uptime;
    }
}
