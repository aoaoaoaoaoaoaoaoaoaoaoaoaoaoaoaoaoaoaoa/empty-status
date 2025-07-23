use std::path::Path;

use crate::display::{color_by_pct, color_by_pct_custom};
use crate::mode_enum;
use crate::util::RotateEnum;
use crate::{core::Unit, display::color, impl_handle_click_rotate_mode, register_unit};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use sysinfo::{ProcessesToUpdate, System};

mode_enum!(Totals, WorstProcess);

#[derive(Debug, Deserialize)]
pub struct MemConfig {}

#[derive(Debug)]
pub struct Mem {
    #[allow(dead_code)]
    cfg: MemConfig,
    mode: DisplayMode,
}

impl Mem {
    pub fn from_cfg(cfg: MemConfig) -> Self {
        Self {
            cfg,
            mode: DisplayMode::Totals,
        }
    }
    fn read_formatted_totals(&self) -> String {
        let mut sys = System::new();
        sys.refresh_memory();

        let total_bytes = sys.total_memory();
        let used_bytes = sys.used_memory();

        let used_frac = used_bytes as f64 / total_bytes as f64;

        let used_gib = used_bytes as f64 / (1 << 30) as f64; // Convert bytes to GiB
        let used_percent = used_frac * 100.0;

        let col = color_by_pct(used_percent);
        let formatted_gib = color(format!("{used_gib:>2.1}"), &col);
        let formatted_percent = color(format!("{used_percent:>2.0}"), &col);

        format!("mem [used {formatted_gib} GiB ({formatted_percent}%)]",)
    }

    fn read_formatted_worst_rss(&self) -> String {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.refresh_memory();
        // aggregate by simple stem, removing flags etc.
        let mut max_name = "";
        let mut max_rss_bytes = 0;

        for process in sys.processes().values() {
            if let Some(name) = process
                .exe()
                .and_then(Path::file_name)
                .and_then(|s| s.to_str())
            {
                let rss = process.memory();
                if rss > max_rss_bytes {
                    max_name = name;
                    max_rss_bytes = rss;
                }
            }
        }

        let max_rss_gib = max_rss_bytes as f64 / (1 << 30) as f64;
        let max_rss_rel = max_rss_gib / sys.total_memory() as f64 * 100.0;
        let col = color_by_pct_custom(max_rss_rel, &[5.0, 10.0, 20.0, 50.0]);
        let max_rss_str = color(format!("{max_rss_gib:>2.3}"), col);
        format!("mem [worst {max_name}: {max_rss_str} GiB rss]")
    }
}

#[async_trait]
impl Unit for Mem {
    async fn read_formatted(&mut self) -> String {
        match self.mode {
            DisplayMode::Totals => self.read_formatted_totals(),
            DisplayMode::WorstProcess => self.read_formatted_worst_rss(),
        }
    }
    impl_handle_click_rotate_mode!();
}

register_unit!(Mem, MemConfig);
