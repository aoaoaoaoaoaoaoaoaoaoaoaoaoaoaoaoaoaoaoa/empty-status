use crate::{
    core::{Unit, BROWN, GREEN, RED, VIOLET},
    display::{color, color_by_pct_rev},
    impl_handle_click_rotate_mode, mode_enum, register_unit,
};
use async_trait::async_trait;
use neli_wifi::Socket;
use serde::Deserialize;

mode_enum!(ShowSsid, HideSsid);

#[derive(Debug, Clone, Deserialize)]
pub struct WifiConfig {
    interface: String,
}

#[derive(Debug)]
pub struct Wifi {
    cfg: WifiConfig,
    mode: DisplayMode,
}

impl Wifi {
    pub fn from_cfg(cfg: WifiConfig) -> Self {
        Self {
            cfg,
            mode: DisplayMode::ShowSsid,
        }
    }
}

#[async_trait]
impl Unit for Wifi {
    async fn read_formatted(&mut self) -> String {
        let mut sock = match Socket::connect() {
            Ok(s) => s,
            Err(_) => return format!("wifi {}", color("no netlink", VIOLET)),
        };

        let interface = match sock.get_interfaces_info().ok().and_then(|v| {
            v.into_iter().find(|i| {
                i.name
                    .as_deref()
                    // SAFETY: the last byte is always a null terminator, so never empty
                    .and_then(|b| str::from_utf8(&b[..b.len() - 1]).ok())
                    == Some(self.cfg.interface.as_str())
            })
        }) {
            Some(i) => i,
            None => return format!("wifi {} {}", self.cfg.interface, color("gone", BROWN)),
        };

        let station = match sock
            .get_station_info(interface.index.unwrap_or_default())
            .ok()
            .and_then(|mut v| v.pop())
        {
            Some(s) => s,
            None => return format!("wifi {}", color("down", RED)),
        };

        // linear remap −80 dBm→0 %, −30 dBm→100 %
        let pct = (((station.signal.unwrap_or(-127) as f32 + 80.0) / 50.0).clamp(0.0, 1.0) * 100.0)
            .round() as u8;
        let pct_str = color(format!("{pct:2.0}%"), color_by_pct_rev(pct as f64));

        let ssid_str = match self.mode {
            DisplayMode::ShowSsid => &color(
                format!(
                    " [{}]",
                    interface
                        .ssid
                        .as_deref()
                        .and_then(|b| str::from_utf8(b).ok())
                        .unwrap_or("?")
                ),
                GREEN,
            ),
            DisplayMode::HideSsid => "",
        };

        format!("wifi{ssid_str} {pct_str}%")
    }

    impl_handle_click_rotate_mode!();
}

register_unit!(Wifi, WifiConfig);
