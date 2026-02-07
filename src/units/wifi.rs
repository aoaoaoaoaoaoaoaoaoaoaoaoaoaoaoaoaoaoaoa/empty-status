use crate::{
    core::{Unit, BROWN, GREEN, RED, VIOLET},
    display::color_by_pct_rev,
    impl_handle_click_rotate_mode, mode_enum, register_unit,
    render::markup::Markup,
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
    async fn read_formatted(&mut self) -> crate::core::Readout {
        let Ok(mut sock) = Socket::connect() else {
            return crate::core::Readout::err(
                Markup::text("wifi ") + Markup::text("no netlink").fg(VIOLET),
            );
        };

        let Some(interface) = sock.get_interfaces_info().ok().and_then(|v| {
            v.into_iter().find(|i| {
                i.name
                    .as_deref()
                    // SAFETY: the last byte is always a null terminator, so never empty
                    .and_then(|b| str::from_utf8(&b[..b.len() - 1]).ok())
                    == Some(self.cfg.interface.as_str())
            })
        }) else {
            return crate::core::Readout::err(
                Markup::text(format!("wifi {} ", self.cfg.interface))
                    + Markup::text("gone").fg(BROWN),
            );
        };

        let Some(station) = sock
            .get_station_info(interface.index.unwrap_or_default())
            .ok()
            .and_then(|mut v| v.pop())
        else {
            return crate::core::Readout::err(Markup::text("wifi ") + Markup::text("down").fg(RED));
        };

        // linear remap −80 dBm→0 %, −30 dBm→100 %
        let pct = (((f32::from(station.signal.unwrap_or(-127)) + 80.0) / 50.0).clamp(0.0, 1.0)
            * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let pct_str = Markup::text(format!("{pct:2.0}%")).fg(color_by_pct_rev(f64::from(pct)));

        let ssid = interface
            .ssid
            .as_deref()
            .and_then(|b| str::from_utf8(b).ok())
            .unwrap_or("?");
        let ssid_str = match self.mode {
            DisplayMode::ShowSsid => {
                Markup::text(" ")
                    + Markup::bracketed(Markup::text(ssid).fg(GREEN))
                    + Markup::text(" ")
            }
            DisplayMode::HideSsid => Markup::text(" "),
        };

        crate::core::Readout::ok(Markup::text("wifi") + ssid_str + pct_str)
    }

    impl_handle_click_rotate_mode!();
}

register_unit!(Wifi, WifiConfig);
