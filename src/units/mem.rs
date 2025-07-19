use std::path::Path;

use crate::{
    core::{Unit, GREEN},
    display::{color, RangeColorizer, RangeColorizerBuilder},
};
use anyhow::Result;
use async_trait::async_trait;
use sysinfo::{ProcessesToUpdate, System};
use tracing::debug;

const MEM_POLL_IVAL: f64 = 0.5;

enum MemReadMode {
    Totals,
    WorstProcess,
}

pub struct Mem {
    mode: MemReadMode,
    col_load_tot: RangeColorizer,
    col_load_worst: RangeColorizer,
}
impl Mem {
    pub fn new() -> Self {
        Self {
            mode: MemReadMode::Totals,
            col_load_tot: RangeColorizerBuilder::default().build().unwrap(),
            col_load_worst: RangeColorizerBuilder::default()
                .breakpoints(vec![5.0, 10.0, 20.0, 50.0])
                .build()
                .unwrap(),
        }
    }
}

impl Mem {
    fn read_formatted_totals(&self) -> Result<String> {
        let mut sys = System::new();
        sys.refresh_memory();

        let total_bytes = sys.total_memory();
        let used_bytes = sys.used_memory();

        let used_frac = used_bytes as f64 / total_bytes as f64;

        let used_gib = used_bytes as f64 / (1 << 30) as f64; // Convert bytes to GiB
        let used_percent = used_frac * 100.0;

        let col = self.col_load_tot.get(used_percent);
        let formatted_gib = color(format!("{used_gib:>2.1}"), col);
        let formatted_percent = color(format!("{used_percent:>2.0}"), col);

        Ok(format!(
            "mem [used {formatted_gib} GiB ({formatted_percent}%)]",
        ))
    }

    fn read_formatted_worst_rss(&self) -> Result<String> {
        debug!("entering worst rss!");
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
        let col = self.col_load_worst.get(max_rss_rel);
        let max_rss_str = color(format!("{max_rss_gib:>2.3}"), col);
        debug!(
            "about to return worst rss: {max_rss_str}, {max_rss_rel}, {:?}",
            self.col_load_worst
        );
        Ok(format!(
            "mem [worst {}: {max_rss_str} GiB rss]",
            color(max_name, GREEN)
        ))
    }
}

#[async_trait]
impl Unit for Mem {
    fn poll_interval(&self) -> f64 {
        MEM_POLL_IVAL
    }

    async fn read_formatted(&mut self) -> Result<String> {
        match self.mode {
            MemReadMode::Totals => self.read_formatted_totals(),
            MemReadMode::WorstProcess => self.read_formatted_worst_rss(),
        }
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        // Toggle between totals and worst process mode
        self.mode = match self.mode {
            MemReadMode::Totals => MemReadMode::WorstProcess,
            MemReadMode::WorstProcess => MemReadMode::Totals,
        };
    }
}
