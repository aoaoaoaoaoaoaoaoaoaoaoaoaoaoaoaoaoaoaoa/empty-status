use std::time::Duration;

use chrono::Local;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use sysinfo::System;

use crate::{
    core::{Button, View},
    display::{color_by_thresholds, format_duration},
    probe_io::ProbeIo,
    render::markup::Markup,
    units::{ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(2);

cycle!(
    enum Mode {
        DateTime,
        Uptime,
    }
);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde_inline_default("%a %b %d %Y - %H:%M".to_owned())]
    format: String,
}

#[derive(Debug)]
pub struct Model {
    format: String,
    mode: Mode,
    load_breakpoints: [f64; 4],
}

persist!(Model.mode);

#[derive(Debug)]
pub enum Request {
    DateTime(String),
    Uptime,
}

#[derive(Debug)]
pub enum Sample {
    DateTime(String),
    Uptime { seconds: u64, loads: [f64; 3] },
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        if chrono::format::StrftimeItems::new(&config.format)
            .any(|item| matches!(item, chrono::format::Item::Error))
        {
            return Err("invalid strftime format".to_owned());
        }

        let cpus =
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get) as f64;
        Ok(Self {
            format: config.format,
            mode: Mode::DateTime,
            load_breakpoints: [cpus * 0.1, cpus * 0.25, cpus * 0.5, cpus * 0.75],
        })
    }

    pub fn request(&self) -> Request {
        match self.mode {
            Mode::DateTime => Request::DateTime(self.format.clone()),
            Mode::Uptime => Request::Uptime,
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        match reply {
            Ok(Sample::DateTime(datetime)) => View::ok(Markup::text(datetime)),
            Ok(Sample::Uptime { seconds, loads }) => {
                let loads = loads.map(|load| {
                    Markup::text(format!("{load:>3.2}"))
                        .fg(color_by_thresholds(load, self.load_breakpoints))
                });
                View::ok(
                    Markup::text("uptime ")
                        + Markup::bracketed(Markup::text(format_duration(seconds)))
                        + Markup::text(" load ")
                        + Markup::bracketed(Markup::join("/", loads)),
                )
            }
            Err(error) => error_view("time", error),
        }
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        Reaction::refresh()
    }
}

pub async fn probe(request: Request, _io: &ProbeIo) -> Reply {
    Ok(match request {
        Request::DateTime(format) => Sample::DateTime(Local::now().format(&format).to_string()),
        Request::Uptime => {
            let load = System::load_average();
            Sample::Uptime {
                seconds: System::uptime(),
                loads: [load.one, load.five, load.fifteen],
            }
        }
    })
}
