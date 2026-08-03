use std::{str, time::Duration};

use neli_wifi::Socket;
use serde::Deserialize;

use crate::{
    core::{Button, Health, View},
    display::color_by_percent_remaining,
    probe_io::ProbeIo,
    render::{
        color::{BROWN, GREEN, RED, VIOLET},
        markup::Markup,
    },
    units::{ProbeError, Reaction, error_view},
};

pub const TIMEOUT: Duration = Duration::from_secs(3);

cycle!(
    enum Mode {
        ShowSsid,
        HideSsid,
    }
);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    interface: String,
}

#[derive(Debug)]
pub struct Model {
    interface: String,
    mode: Mode,
}

persist!(Model.mode);

#[derive(Debug)]
pub struct Request {
    interface: String,
}

#[derive(Debug)]
pub enum Sample {
    NoNetlink,
    Gone,
    Down,
    Connected { ssid: String, signal: u8 },
}

pub type Reply = Result<Sample, ProbeError>;

impl Model {
    pub fn new(config: Config) -> Result<Self, String> {
        if config.interface.is_empty() {
            return Err("interface must not be empty".to_owned());
        }
        Ok(Self {
            interface: config.interface,
            mode: Mode::ShowSsid,
        })
    }

    pub fn request(&self) -> Request {
        Request {
            interface: self.interface.clone(),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> View {
        match reply {
            Ok(Sample::NoNetlink) => View::new(
                Markup::text("wifi ") + Markup::text("no netlink").fg(VIOLET),
                Health::Degraded,
            ),
            Ok(Sample::Gone) => View::new(
                Markup::text(format!("wifi {} ", self.interface)) + Markup::text("gone").fg(BROWN),
                Health::Error,
            ),
            Ok(Sample::Down) => View::new(
                Markup::text("wifi ") + Markup::text("down").fg(RED),
                Health::Error,
            ),
            Ok(Sample::Connected { ssid, signal }) => {
                let ssid = match self.mode {
                    Mode::ShowSsid => {
                        Markup::text(" ")
                            + Markup::bracketed(Markup::text(ssid).fg(GREEN))
                            + Markup::text(" ")
                    }
                    Mode::HideSsid => Markup::text(" "),
                };
                View::ok(
                    Markup::text("wifi")
                        + ssid
                        + Markup::text(format!("{signal:2}%"))
                            .fg(color_by_percent_remaining(f64::from(signal))),
                )
            }
            Err(error) => error_view("wifi", error),
        }
    }

    pub fn click(&mut self, _button: Button) -> Reaction {
        self.mode.advance();
        Reaction::refresh()
    }
}

pub async fn probe(request: Request, io: &ProbeIo) -> Reply {
    io.blocking(move || read_wifi(&request.interface))
        .await
        .map_err(Into::into)
}

fn read_wifi(interface_name: &str) -> Sample {
    let Ok(mut socket) = Socket::connect() else {
        return Sample::NoNetlink;
    };
    let Some(interface) = socket.get_interfaces_info().ok().and_then(|interfaces| {
        interfaces.into_iter().find(|interface| {
            interface
                .name
                .as_deref()
                .and_then(decode_nul_string)
                .as_deref()
                == Some(interface_name)
        })
    }) else {
        return Sample::Gone;
    };
    let Some(index) = interface.index else {
        return Sample::Down;
    };
    let Some(station) = socket
        .get_station_info(index)
        .ok()
        .and_then(|mut stations| stations.pop())
    else {
        return Sample::Down;
    };

    let signal_dbm = f32::from(station.signal.unwrap_or(-127));
    let signal = ((signal_dbm + 80.0) / 50.0 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u8;
    let ssid = interface
        .ssid
        .as_deref()
        .and_then(decode_nul_string)
        .unwrap_or_else(|| "?".to_owned());
    Sample::Connected { ssid, signal }
}

fn decode_nul_string(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    str::from_utf8(bytes).ok().map(str::to_owned)
}
