use crate::core::{color, get_color, Unit};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct Mem {
    poll_interval: f64,
}

impl Mem {
    pub fn new(poll_interval: f64) -> Self {
        Self { poll_interval }
    }
}

#[async_trait]
impl Unit for Mem {
    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read_formatted(&mut self) -> Result<String> {
        let file = File::open("/proc/meminfo")?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().take(3).filter_map(Result::ok).collect();

        if lines.len() < 3 {
            return Err(anyhow::anyhow!("Failed to read memory information"));
        }

        // Parse MemTotal
        let total_parts: Vec<&str> = lines[0].split_whitespace().collect();
        if total_parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid MemTotal format"));
        }
        let total_kib: u64 = total_parts[1].parse()?;

        // Parse MemAvailable
        let available_parts: Vec<&str> = lines[2].split_whitespace().collect();
        if available_parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid MemAvailable format"));
        }
        let available_kib: u64 = available_parts[1].parse()?;

        // Calculate used memory
        let used_kib = total_kib.saturating_sub(available_kib);
        let used_frac = used_kib as f64 / total_kib as f64;

        let used_gib = used_kib as f64 / 1_048_576.0; // KiB to GiB
        let used_percent = used_frac * 100.0;

        let breakpoints = [20.0, 40.0, 60.0, 80.0];
        let col = get_color(
            used_percent,
            &breakpoints,
            &["#81A2BE", "#B5BD68", "#F0C674", "#DE935F", "#CC6666"],
            false,
        );

        let formatted_gib = color(format!("{:.1}", used_gib), &col);
        let formatted_percent = color(format!("{:.0}", used_percent), &col);

        Ok(format!(
            "mem [used {} GiB ({}%)]",
            formatted_gib, formatted_percent
        ))
    }
}
