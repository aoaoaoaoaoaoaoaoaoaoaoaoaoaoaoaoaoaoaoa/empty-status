use std::{
    collections::VecDeque,
    process::Stdio,
    str,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use tokio::process::Command;

use crate::{
    core::{Button, Health, View},
    display::color_by_thresholds,
    probe_io::ProbeIo,
    render::{
        color::{GREEN, GREY, ORANGE, VIOLET, YELLOW},
        markup::Markup,
    },
    units::{ProbeError, Reaction, error_view, positive_duration},
    util::Ema,
};

pub const TIMEOUT: Duration = Duration::from_secs(3);
const NET_ROOT: &str = "/sys/class/net";

cycle!(
    enum Mode {
        Bandwidth,
        Ping,
    }
);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    interface: String,
    #[serde_inline_default(0.333)]
    smoothing_window_sec: f64,
    #[serde_inline_default("8.8.8.8".to_owned())]
    ping_server: String,
    #[serde_inline_default(25)]
    ping_window: usize,
}

#[derive(Debug)]
pub struct Model {
    interface: String,
    ping_server: String,
    ping_window: usize,
    mode: Mode,
    counters: Option<Counters>,
    received: Ema,
    transmitted: Ema,
    pings: VecDeque<Option<f64>>,
}

persist!(Model.mode);

#[derive(Debug)]
pub enum Request {
    Bandwidth { interface: String },
    Ping { server: String },
}

#[derive(Debug)]
pub enum Sample {
    Bandwidth(Counters),
    Down,
    Ping(Option<f64>),
}

#[derive(Debug, Clone, Copy)]
pub struct Counters {
    captured_at: Instant,
    received: u64,
    transmitted: u64,
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        if config.interface.is_empty() || config.interface.contains('/') {
            return Err("interface must be one nonempty path component".to_owned());
        }
        if config.ping_server.is_empty() {
            return Err("ping_server must not be empty".to_owned());
        }
        if config.ping_window < 2 {
            return Err("ping_window must be at least 2".to_owned());
        }
        if config.ping_window > 1024 {
            return Err("ping_window cannot exceed 1024".to_owned());
        }
        let smoothing = positive_duration("smoothing_window_sec", config.smoothing_window_sec)?;
        Ok(Self {
            interface: config.interface,
            ping_server: config.ping_server,
            ping_window: config.ping_window,
            mode: Mode::Bandwidth,
            counters: None,
            received: Ema::new(smoothing),
            transmitted: Ema::new(smoothing),
            pings: VecDeque::with_capacity(config.ping_window),
        })
    }

    pub fn request(&self) -> Request {
        match self.mode {
            Mode::Bandwidth => Request::Bandwidth {
                interface: self.interface.clone(),
            },
            Mode::Ping => Request::Ping {
                server: self.ping_server.clone(),
            },
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        match reply {
            Ok(Sample::Bandwidth(current)) => self.render_bandwidth(current),
            Ok(Sample::Down) => {
                self.reset_bandwidth();
                View::error(&format!("net {}", self.interface), "down")
            }
            Ok(Sample::Ping(latency)) => self.render_ping(latency),
            Err(error) => {
                self.reset_bandwidth();
                error_view(&format!("net {}", self.interface), error)
            }
        }
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        if self.mode == Mode::Bandwidth {
            self.pings.clear();
            self.reset_bandwidth();
        }
        Reaction::refresh()
    }

    fn reset_bandwidth(&mut self) {
        self.counters = None;
        self.received.reset();
        self.transmitted.reset();
    }

    fn render_bandwidth(&mut self, current: Counters) -> View {
        let Some(previous) = self.counters.replace(current) else {
            return View::loading(&format!("net {}", self.interface));
        };
        let elapsed = current
            .captured_at
            .saturating_duration_since(previous.captured_at)
            .as_secs_f64();
        let down = rate(current.received, previous.received, elapsed);
        let up = rate(current.transmitted, previous.transmitted, elapsed);
        let down = self.received.push(down, current.captured_at);
        let up = self.transmitted.push(up, current.captured_at);
        View::ok(
            Markup::text(format!("net {} ", self.interface))
                + Markup::bracketed(Markup::text("u ") + format_rate(up))
                + Markup::text(" ")
                + Markup::bracketed(Markup::text("d ") + format_rate(down)),
        )
    }

    fn render_ping(&mut self, latency: Option<f64>) -> View {
        if self.pings.len() == self.ping_window {
            let _ = self.pings.pop_front();
        }
        self.pings.push_back(latency);
        let prefix = Markup::text(format!(
            "net {} [ping {}] ",
            self.interface, self.ping_server
        ));
        if self.pings.len() < 2 {
            return View::new(
                prefix + Markup::text("loading").fg(VIOLET),
                Health::Degraded,
            );
        }

        let mut successes = self
            .pings
            .iter()
            .filter_map(|sample| *sample)
            .collect::<Vec<_>>();
        successes.sort_by(f64::total_cmp);
        let latency = if successes.is_empty() {
            Markup::text("no replies").fg(ORANGE)
        } else {
            let median = successes[successes.len() / 2];
            let mut deviations = successes
                .iter()
                .map(|sample| (sample - median).abs())
                .collect::<Vec<_>>();
            deviations.sort_by(f64::total_cmp);
            let mad = deviations[deviations.len() / 2];
            Markup::bracketed(
                Markup::text("med ")
                    + Markup::text(format!("{median:>3.1}"))
                        .fg(color_by_thresholds(median, [10.0, 20.0, 30.0, 90.0]))
                    + Markup::text(" mad ")
                    + Markup::text(format!("{mad:>2.1}"))
                        .fg(color_by_thresholds(mad, [2.0, 5.0, 10.0, 30.0]))
                    + Markup::text(" ms"),
            )
        };
        let losses = self.pings.iter().filter(|sample| sample.is_none()).count();
        let loss = losses as f64 / self.pings.len() as f64 * 100.0;
        let loss = if losses == 0 {
            Markup::text("no loss").fg(GREEN)
        } else {
            Markup::text(format!("{loss:>3.1}% loss")).fg(ORANGE)
        };
        let health = if losses == 0 {
            Health::Ok
        } else {
            Health::Degraded
        };
        View::new(
            prefix + latency + Markup::text(" ") + Markup::bracketed(loss),
            health,
        )
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    match request {
        Request::Bandwidth { interface } => probe_bandwidth(&interface, io).await,
        Request::Ping { server } => probe_ping(&server).await.map(Sample::Ping),
    }
}

async fn probe_bandwidth(interface: &str, io: &ProbeIo) -> Reply {
    let root = format!("{NET_ROOT}/{interface}");
    let (carrier, received, transmitted) = tokio::join!(
        io.read(format!("{root}/carrier")),
        io.read(format!("{root}/statistics/rx_bytes")),
        io.read(format!("{root}/statistics/tx_bytes")),
    );
    if parse_u64(&carrier?)? == 0 {
        return Ok(Sample::Down);
    }
    Ok(Sample::Bandwidth(Counters {
        captured_at: Instant::now(),
        received: parse_u64(&received?)?,
        transmitted: parse_u64(&transmitted?)?,
    }))
}

async fn probe_ping(server: &str) -> Result<Option<f64>, ProbeError> {
    let output = Command::new("ping")
        .args(["-n", "-c", "1", "-W", "1", "--", server])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| ProbeError::Unit(format!("cannot run ping: {error}")))?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(ProbeError::Unit("ping rejected its target".to_owned()));
    }
    Ok(str::from_utf8(&output.stdout).ok().and_then(parse_ping))
}

fn parse_ping(output: &str) -> Option<f64> {
    output.lines().find_map(|line| {
        line.split_once("time=")?
            .1
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn parse_u64(bytes: &[u8]) -> Result<u64, ProbeError> {
    str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| ProbeError::Unit("invalid network counter".to_owned()))
}

fn rate(current: u64, previous: u64, elapsed: f64) -> f64 {
    if elapsed > 0.0 {
        current.saturating_sub(previous) as f64 / elapsed
    } else {
        0.0
    }
}

fn format_rate(bytes_per_second: f64) -> Markup {
    let (value, suffix, color) = if bytes_per_second >= 1_073_741_824.0 {
        (bytes_per_second / 1_073_741_824.0, "G/s", ORANGE)
    } else if bytes_per_second >= 1_048_576.0 {
        (bytes_per_second / 1_048_576.0, "M/s", YELLOW)
    } else if bytes_per_second >= 1024.0 {
        (bytes_per_second / 1024.0, "K/s", GREEN)
    } else {
        (bytes_per_second, "B/s", GREY)
    };
    Markup::text(format!("{value:>4.0} ")) + Markup::text(suffix).fg(color)
}

#[cfg(test)]
mod tests {
    use super::parse_ping;

    #[test]
    fn parses_iputils_latency() {
        assert_eq!(
            parse_ping("64 bytes from 8.8.8.8: icmp_seq=1 ttl=117 time=25.6 ms"),
            Some(25.6)
        );
    }
}
