use crate::display::color_by_pct_custom;
use crate::mode_enum;
use crate::render::markup::Markup;
use crate::{
    core::Unit,
    display::{color, format_duration},
    impl_handle_click_rotate_mode, register_unit,
};
use async_trait::async_trait;
use chrono::Local;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use sysinfo::System;

mode_enum!(DateTime, Uptime);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct TimeConfig {
    #[serde_inline_default("%a %b %d %Y - %H:%M".to_string())]
    format: String,
}

#[derive(Debug)]
pub struct Time {
    cfg: TimeConfig,
    mode: DisplayMode,
    uptime_breakpints: [f64; 4],
}

impl Time {
    pub fn from_cfg(cfg: TimeConfig) -> Self {
        let ncpu = f64::from(num_cpus::get().min(u32::MAX as usize) as u32);
        Self {
            cfg,
            mode: DisplayMode::DateTime,
            uptime_breakpints: [ncpu * 0.1, ncpu * 0.25, ncpu * 0.50, ncpu * 0.75],
        }
    }

    fn read_formatted_datetime(&self) -> String {
        Local::now().format(&self.cfg.format).to_string()
    }

    fn read_formatted_uptime(&self) -> String {
        let uptime = System::uptime();
        let load_avg = System::load_average();
        let ut_s = format_duration(f64::from(uptime.min(u32::MAX as u64) as u32));
        let mut load_strings = Vec::new();
        for data in [&load_avg.one, &load_avg.five, &load_avg.fifteen] {
            load_strings.push(color(
                format!("{data:>3.2}"),
                color_by_pct_custom(*data, &self.uptime_breakpints),
            ));
        }
        format!(
            "uptime [{ut_s}] load [{}/{}/{}]",
            load_strings[0], load_strings[1], load_strings[2]
        )
    }
}

#[async_trait]
impl Unit for Time {
    async fn read_formatted(&mut self) -> crate::core::Readout {
        crate::core::Readout::ok(Markup::text(match self.mode {
            DisplayMode::DateTime => self.read_formatted_datetime(),
            DisplayMode::Uptime => self.read_formatted_uptime(),
        }))
    }
    impl_handle_click_rotate_mode!();
}

register_unit!(Time, TimeConfig);
