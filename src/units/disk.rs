use crate::core::{Unit, BLUE, BROWN, ORANGE};
use crate::display::color;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs::{read_dir, read_to_string, File};
use std::io::Read;
use std::time::Instant;
use std::u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskData {
    bps_read: f64,
    bps_write: f64,
    err_no_disk: bool,
}

pub struct Disk {
    disk: String,
    stat_path: String,
    sector_size: u64,
    last_r: u64,
    last_w: u64,
    last_t: Instant,
    fail: bool,
}

impl Disk {
    pub fn new(disk: &str) -> Self {
        let stat_path = format!("/sys/class/block/{disk}/stat");
        // TODO evetually we'll make these Results and handle construction with toml config
        let sector_size = Self::get_sector_size(disk).unwrap();
        let (last_r, last_w) = Self::read_rw(&stat_path, sector_size).unwrap();
        Self {
            disk: disk.to_string(),
            stat_path,
            sector_size,
            last_r,
            last_w,
            last_t: Instant::now(),
            fail: false,
        }
    }

    fn get_sector_size(disk: &str) -> Option<u64> {
        let dir_list = read_dir("/sys/block").ok()?;
        let best = dir_list
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| disk.starts_with(name))
            .max_by_key(String::len)?;

        let out = read_to_string(format!("/sys/block/{best}/queue/hw_sector_size"))
            .ok()?
            .trim()
            .parse::<u64>()
            .unwrap_or(512);

        Some(out)
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
impl Unit for Disk {
    fn poll_interval(&self) -> f64 {
        1.0
    }

    async fn read_formatted(&mut self) -> Result<String> {
        if self.sector_size == 0 {
            self.sector_size = Self::get_sector_size(&self.disk)
                .ok_or_else(|| anyhow!("could not get sector size"))?;
        }

        let mut data = DiskData {
            bps_read: 0.0,
            bps_write: 0.0,
            err_no_disk: false,
        };

        let (r, w) = match Self::read_rw(&self.stat_path, self.sector_size) {
            Ok((r, w)) => (r, w),
            Err(_) => {
                data.err_no_disk = true;
                let context = format!("disk [{} {{}}]", self.disk);
                return Ok(context.replace("{}", &color("absent", BROWN)));
            }
        };

        let now = Instant::now();
        let dt = now.duration_since(self.last_t).as_secs_f64();
        let dr = r.saturating_sub(self.last_r);
        let dw = w.saturating_sub(self.last_w);
        self.last_r = r;
        self.last_w = w;
        self.last_t = now;

        data.bps_read = if dt > 0.0 { dr as f64 / dt } else { 0.0 };
        data.bps_write = if dt > 0.0 { dw as f64 / dt } else { 0.0 };

        let context = format!("disk [{} {{}}]", self.disk);

        if data.err_no_disk {
            return Ok(context.replace("{}", &color("absent", BROWN)));
        }

        let bars = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let threshs = [
            1.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 16777216.0,
        ];

        let bps_read = data.bps_read;
        let bps_write = data.bps_write;

        let r_bar = bars[threshs
            .iter()
            .position(|&t| bps_read < t)
            .unwrap_or(bars.len() - 1)];
        let w_bar = bars[threshs
            .iter()
            .position(|&t| bps_write < t)
            .unwrap_or(bars.len() - 1)];

        Ok(context.replace("{}", &(color(r_bar, BLUE) + &color(w_bar, ORANGE))))
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        // No click handling for disk
    }
}
