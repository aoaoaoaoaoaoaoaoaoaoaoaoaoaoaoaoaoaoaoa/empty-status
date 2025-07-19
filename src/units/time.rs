use crate::{
    core::{self, format_duration, Unit},
    display::{color, RangeColorizer, RangeColorizerBuilder},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use sysinfo::{LoadAvg, System};

#[derive(Debug, Clone)]
pub struct TimeData {
    datestr: String,
    uptime: u64,
    load_avg: LoadAvg,
}

pub struct Time {
    poll_interval: f64,
    format: String,
    doing_uptime: bool,
    colorizer: RangeColorizer,
}

impl Time {
    pub fn new(format: String, poll_interval: f64) -> Self {
        let ncpu = num_cpus::get() as f64;
        let load_color_scale = RangeColorizerBuilder::default()
            .breakpoints(vec![ncpu * 0.1, ncpu * 0.25, ncpu * 0.50, ncpu * 0.75])
            .build()
            .unwrap();

        Self {
            poll_interval,
            format,
            doing_uptime: false,
            colorizer: load_color_scale,
        }
    }
}

#[async_trait]
impl Unit for Time {
    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read_formatted(&mut self) -> Result<String> {
        let current_time = Local::now().format(&self.format).to_string();
        let uptime = System::uptime();
        let load_avg = System::load_average();

        let data = TimeData {
            datestr: current_time,
            uptime,
            load_avg,
        };

        if !self.doing_uptime {
            // Show date/time
            Ok(data.datestr)
        } else {
            // Show uptime and load average
            let ut_s = format_duration(data.uptime as f64);

            let mut load_strings = Vec::new();
            for data in [
                &data.load_avg.one,
                &data.load_avg.five,
                &data.load_avg.fifteen,
            ] {
                load_strings.push(color(format!("{data:>3.2}"), self.colorizer.get(*data)));
            }

            Ok(format!(
                "uptime [{ut_s}] load [{}/{}/{}]",
                load_strings[0], load_strings[1], load_strings[2]
            ))
        }
    }

    fn handle_click(&mut self, _click: core::ClickEvent) {
        // Toggle between time display and uptime display
        self.doing_uptime = !self.doing_uptime;
    }
}
