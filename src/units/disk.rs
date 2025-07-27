use crate::core::{Unit, BLUE, BROWN, ORANGE};
use crate::display::color;
use crate::util::{Ema, Smoother};
use crate::{impl_handle_click_nop, register_unit};
use anyhow::Result;
use async_trait::async_trait;
use cute::c;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use std::fs::{read_dir, read_to_string, File};
use std::io::Read;
use std::time::Instant;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskData {
    bps_read: f64,
    bps_write: f64,
    err_no_disk: bool,
}

const BARS: &[&str; 9] = &[" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct DiskConfig {
    disk: String,

    #[serde_inline_default(0.5)]
    smoothing_sec: f64,

    #[serde_inline_default(3e8)]
    write_peak_ref: f64,

    #[serde_inline_default(1.5e9)]
    read_peak_ref: f64,
}

#[derive(Debug)]
pub struct Disk {
    cfg: DiskConfig,
    stat_path: String,
    sector_size: Option<u64>,
    write_ema: Ema<f64>,
    read_ema: Ema<f64>,
    read_threshs: Vec<f64>,
    write_threshs: Vec<f64>,
    last_r: u64,
    last_w: u64,
    last_t: Instant,
}

impl Disk {
    pub fn from_cfg(cfg: DiskConfig) -> Self {
        let stat_path = format!("/sys/class/block/{}/stat", cfg.disk);
        // TODO evetually we'll make these Results and handle construction with toml config
        let sector_size = Self::get_sector_size(&cfg.disk);
        let (last_r, last_w): (u64, u64) = sector_size
            .and_then(|ss| Self::read_rw(&stat_path, ss).ok())
            .unwrap_or((0, 0));

        let read_threshs = c![cfg.read_peak_ref.powf(i as f64 /9.0), for i in 1..10];
        info!("computed read thresholds: {:?}", read_threshs);
        let write_threshs = c![cfg.write_peak_ref.powf(i as f64 / 9.0), for i in 1..10];
        info!("computed write thresholds: {:?}", write_threshs);

        Self {
            stat_path,
            sector_size,
            write_ema: Ema::new(cfg.smoothing_sec),
            read_ema: Ema::new(cfg.smoothing_sec),
            read_threshs,
            write_threshs,
            last_r,
            last_w,
            last_t: Instant::now(),
            cfg,
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
    async fn read_formatted(&mut self) -> String {
        let sector_size = match self.sector_size {
            Some(size) => size,
            None => {
                let context = format!("disk {} [{{}}]", self.cfg.disk);
                return context.replace("{}", &color("no such disk", BROWN));
            }
        };

        let (r, w) = match Self::read_rw(&self.stat_path, sector_size).ok() {
            Some((r, w)) => (r, w),
            None => {
                let context = format!("disk {} [{{}}]", self.cfg.disk);
                return context.replace("{}", &color("no such disk", BROWN));
            }
        };

        let now = Instant::now();
        let dt = now.duration_since(self.last_t).as_secs_f64();
        let dr = r.saturating_sub(self.last_r);
        let dw = w.saturating_sub(self.last_w);
        self.last_r = r;
        self.last_w = w;
        self.last_t = now;

        let bps_read = if dt > 0.0 { dr as f64 / dt } else { 0.0 };
        let bps_write = if dt > 0.0 { dw as f64 / dt } else { 0.0 };
        let bps_read = self.read_ema.feed_and_read(bps_read, now).unwrap_or(&0.0);
        let bps_write = self.write_ema.feed_and_read(bps_write, now).unwrap_or(&0.0);

        let context = format!("disk {} [{{}}]", self.cfg.disk);
        let r_bar = BARS[self
            .read_threshs
            .iter()
            .position(|&t| *bps_read < t)
            .unwrap_or(BARS.len() - 1)];
        let w_bar = BARS[self
            .write_threshs
            .iter()
            .position(|&t| *bps_write < t)
            .unwrap_or(BARS.len() - 1)];

        context.replace("{}", &(color(r_bar, BLUE) + &color(w_bar, ORANGE)))
    }

    impl_handle_click_nop!();
}

register_unit!(Disk, DiskConfig);
