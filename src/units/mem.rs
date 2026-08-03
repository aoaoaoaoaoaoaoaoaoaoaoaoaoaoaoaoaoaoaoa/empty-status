use std::time::Duration;

use serde::Deserialize;
use sysinfo::{ProcessesToUpdate, System};

use crate::{
    core::{Button, View},
    display::{color_by_percent, color_by_thresholds},
    probe_io::ProbeIo,
    render::markup::Markup,
    units::{ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(5);

cycle!(
    enum Mode {
        Totals,
        WorstProcess,
    }
);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {}

#[derive(Debug)]
pub struct Model {
    mode: Mode,
}

#[derive(Debug, Clone, Copy)]
pub enum Request {
    Totals,
    WorstProcess,
}

#[derive(Debug)]
pub enum Sample {
    Totals {
        used_bytes: u64,
        total_bytes: u64,
    },
    WorstProcess {
        name: String,
        rss_bytes: u64,
        total_bytes: u64,
    },
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub const fn new(_config: Config) -> Self {
        Self { mode: Mode::Totals }
    }

    pub const fn request(&self) -> Request {
        match self.mode {
            Mode::Totals => Request::Totals,
            Mode::WorstProcess => Request::WorstProcess,
        }
    }

    pub fn apply(reply: Reply) -> View {
        match reply {
            Ok(Sample::Totals {
                used_bytes,
                total_bytes,
            }) => {
                let used_percent = percent(used_bytes, total_bytes);
                let color = color_by_percent(used_percent);
                let used_gib = used_bytes as f64 / (1_u64 << 30) as f64;
                View::ok(
                    Markup::text("mem ")
                        + Markup::bracketed(
                            Markup::text("used ")
                                + Markup::text(format!("{used_gib:>4.1}")).fg(color)
                                + Markup::text(" GiB (")
                                + Markup::text(format!("{used_percent:>2.0}")).fg(color)
                                + Markup::text("%)"),
                        ),
                )
            }
            Ok(Sample::WorstProcess {
                name,
                rss_bytes,
                total_bytes,
            }) => {
                let rss_gib = rss_bytes as f64 / (1_u64 << 30) as f64;
                let color =
                    color_by_thresholds(percent(rss_bytes, total_bytes), [5.0, 10.0, 20.0, 50.0]);
                View::ok(
                    Markup::text("mem ")
                        + Markup::bracketed(
                            Markup::text("worst ")
                                + Markup::text(name)
                                + Markup::text(": ")
                                + Markup::text(format!("{rss_gib:>2.3}")).fg(color)
                                + Markup::text(" GiB rss"),
                        ),
                )
            }
            Err(error) => error_view("mem", error),
        }
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        Reaction::refresh()
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    io.blocking(move || match request {
        Request::Totals => {
            let mut system = System::new();
            system.refresh_memory();
            Sample::Totals {
                used_bytes: system.used_memory(),
                total_bytes: system.total_memory(),
            }
        }
        Request::WorstProcess => {
            let mut system = System::new();
            let _ = system.refresh_processes(ProcessesToUpdate::All, true);
            system.refresh_memory();
            let (name, rss_bytes) = system
                .processes()
                .values()
                .map(|process| {
                    let name = process
                        .exe()
                        .and_then(|path| path.file_name())
                        .unwrap_or_else(|| process.name())
                        .to_string_lossy()
                        .into_owned();
                    (name, process.memory())
                })
                .max_by_key(|(_, rss)| *rss)
                .unwrap_or_else(|| ("?".to_owned(), 0));
            Sample::WorstProcess {
                name,
                rss_bytes,
                total_bytes: system.total_memory(),
            }
        }
    })
    .await
    .map_err(Into::into)
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}
