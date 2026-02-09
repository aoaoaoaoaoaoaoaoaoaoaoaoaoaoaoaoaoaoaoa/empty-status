use crate::core::{BLUE, BROWN, ORANGE, VIOLET};
use crate::render::markup::Markup;
use crate::util::{Ema, Smoother};
use cute::c;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::time::Instant;
use tracing::info;
const BARS: &[&str; 9] = &[" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct DiskConfig {
    disk: String,
    #[serde(default)]
    partlabel: Option<String>,
    #[serde(default)]
    partuuid: Option<String>,

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
    sector_size: Option<u64>,
    root: Option<String>,
    name: Option<String>,
    initialized: bool,
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
        // TODO evetually we'll make these Results and handle construction with toml config
        let sector_size = None;
        let (last_r, last_w): (u64, u64) = (0, 0);
        let name = if cfg.partlabel.is_none() && cfg.partuuid.is_none() {
            Some(cfg.disk.clone())
        } else {
            None
        };

        let read_threshs = c![cfg.read_peak_ref.powf(i as f64 /9.0), for i in 1..10];
        info!("computed read thresholds: {:?}", read_threshs);
        let write_threshs = c![cfg.write_peak_ref.powf(i as f64 / 9.0), for i in 1..10];
        info!("computed write thresholds: {:?}", write_threshs);

        Self {
            sector_size,
            root: None,
            name,
            initialized: false,
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

    fn parse_stat(buf: &str, sector_size: u64) -> Option<(u64, u64)> {
        let spl: Vec<&str> = buf.split_whitespace().collect();
        let r = spl.get(2).and_then(|s| s.parse::<u64>().ok())? * sector_size;
        let w = spl.get(6).and_then(|s| s.parse::<u64>().ok())? * sector_size;
        Some((r, w))
    }

    pub fn select_root(&mut self, entries: &[String]) {
        if self.root.is_some() {
            return;
        }
        let Some(disk_name) = self.name.as_ref() else {
            return;
        };
        self.root = entries
            .iter()
            .filter(|name| disk_name.starts_with(*name))
            .max_by_key(|name| name.len())
            .cloned();
    }

    pub fn set_sector_size(&mut self, size: Option<u64>) {
        if self.sector_size.is_none() {
            self.sector_size = size;
        }
    }

    pub fn disk_root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn disk_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn set_disk_name(&mut self, name: String) {
        if self.name.is_none() {
            self.name = Some(name);
        }
    }

    pub fn display_name(&self) -> &str {
        if let Some(label) = self.cfg.partlabel.as_deref() {
            return label;
        }
        if let Some(uuid) = self.cfg.partuuid.as_deref() {
            return uuid;
        }
        &self.cfg.disk
    }

    pub fn selector_partlabel(&self) -> Option<&str> {
        self.cfg.partlabel.as_deref()
    }

    pub fn selector_partuuid(&self) -> Option<&str> {
        self.cfg.partuuid.as_deref()
    }
}

impl Disk {
    pub fn read_markup_from_bytes(
        &mut self,
        stat_bytes: &[u8],
        sector_size_bytes: Option<&[u8]>,
    ) -> Markup {
        if self.name.is_none() {
            return Markup::text(format!("disk {} ", self.display_name()))
                .append(Markup::text("resolving").fg(VIOLET));
        }

        if self.sector_size.is_none() {
            let size = sector_size_bytes.and_then(|bytes| {
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
            });
            self.set_sector_size(size.or(Some(512)));
        }

        let Some(sector_size) = self.sector_size else {
            return Markup::text(format!("disk {} ", self.display_name()))
                .append(Markup::bracketed(Markup::text("no such disk").fg(BROWN)));
        };

        let buf = std::str::from_utf8(stat_bytes).unwrap_or_default();
        let Some((r, w)) = Self::parse_stat(buf, sector_size) else {
            return Markup::text(format!("disk {} ", self.display_name()))
                .append(Markup::bracketed(Markup::text("no such disk").fg(BROWN)));
        };

        let now = Instant::now();
        if !self.initialized {
            self.initialized = true;
            self.last_r = r;
            self.last_w = w;
            self.last_t = now;
            return Markup::text(format!("disk {} ", self.display_name()))
                .append(Markup::text("loading").fg(VIOLET));
        }

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

        Markup::text(format!("disk {} ", self.display_name())).append(Markup::bracketed(
            Markup::text(r_bar)
                .fg(BLUE)
                .append(Markup::text(w_bar).fg(ORANGE)),
        ))
    }

    pub fn handle_click(_click: crate::core::ClickEvent) {}

    pub fn fix_up_and_validate() {}
}
