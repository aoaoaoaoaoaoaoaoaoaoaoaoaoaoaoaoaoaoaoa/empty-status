use crate::core::{color, Unit, BLUE, BROWN, ORANGE};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::time::Instant;

pub struct Py9Disk {
    disk: String,
    stat_path: String,
    sector_size: u64,
    last_r: u64,
    last_w: u64,
    last_t: Instant,
    fail: bool,
}

impl Py9Disk {
    pub fn new(disk: &str) -> Result<Self> {
        let stat_path = format!("/sys/class/block/{}/stat", disk);
        let sector_size = Self::get_sector_size(disk)?;
        let (last_r, last_w) = Self::read_rw(&stat_path, sector_size)?;
        Ok(Self {
            disk: disk.to_string(),
            stat_path,
            sector_size,
            last_r,
            last_w,
            last_t: Instant::now(),
            fail: false,
        })
    }

    fn get_sector_size(disk: &str) -> Result<u64> {
        let path = format!("/sys/block/{}/queue/hw_sector_size", disk);
        let mut f = File::open(path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        Ok(buf.trim().parse().unwrap_or(512))
    }

    fn read_rw(stat_path: &str, sector_size: u64) -> Result<(u64, u64)> {
        let mut f = File::open(stat_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let spl: Vec<&str> = buf.split_whitespace().collect();
        let r = spl.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * sector_size;
        let w = spl.get(6).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * sector_size;
        Ok((r, w))
    }
}

#[async_trait]
impl Unit for Py9Disk {
    fn name(&self) -> String {
        format!("Py9Disk-{}", self.disk)
    }

    fn poll_interval(&self) -> f64 {
        1.0
    }

    async fn read(&mut self) -> Result<HashMap<String, Value>> {
        let mut result = HashMap::new();

        if self.sector_size == 0 {
            self.sector_size = Self::get_sector_size(&self.disk)?;
        }

        let (r, w) = match Self::read_rw(&self.stat_path, self.sector_size) {
            Ok((r, w)) => (r, w),
            Err(_) => {
                result.insert("err_no_disk".to_string(), json!(true));
                return Ok(result);
            }
        };

        let now = Instant::now();
        let dt = now.duration_since(self.last_t).as_secs_f64();
        let dr = r.saturating_sub(self.last_r);
        let dw = w.saturating_sub(self.last_w);
        self.last_r = r;
        self.last_w = w;
        self.last_t = now;

        result.insert(
            "bps_read".to_string(),
            json!(if dt > 0.0 { dr as f64 / dt } else { 0.0 }),
        );
        result.insert(
            "bps_write".to_string(),
            json!(if dt > 0.0 { dw as f64 / dt } else { 0.0 }),
        );

        Ok(result)
    }

    fn format(&self, data: &HashMap<String, Value>) -> String {
        let context = format!("disk [{} {{}}]", self.disk);

        if data
            .get("err_no_disk")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return context.replace("{}", &color("absent", BROWN));
        }

        let bars = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let threshs = [
            1.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 16777216.0,
        ];

        let bps_read = data.get("bps_read").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bps_write = data
            .get("bps_write")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let r_bar = bars[threshs
            .iter()
            .position(|&t| bps_read < t)
            .unwrap_or(bars.len() - 1)];
        let w_bar = bars[threshs
            .iter()
            .position(|&t| bps_write < t)
            .unwrap_or(bars.len() - 1)];

        context.replace("{}", &(color(r_bar, BLUE) + &color(w_bar, ORANGE)))
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        // No click handling for disk
    }
}
