use crate::core::{ClickEvent, GREEN, GREY, ORANGE, RED, VIOLET};
use crate::display::{color_by_pct_custom, COL_USE_HIGH, COL_USE_NORM, COL_USE_VERY_HIGH};
use crate::mode_enum;
use crate::render::markup::Markup;
use crate::util::{Ema, Smoother};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use serde_scan::scan;
use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::Networks;

mode_enum!(Bandwidth, Ping);

#[serde_inline_default]
#[derive(Debug, Clone, Deserialize)]
pub struct NetConfig {
    pub interface: String,
    // enough to give us snappy updates without being totally thrashy
    #[serde_inline_default(0.333)]
    pub smoothing_window_sec: f64,

    #[serde_inline_default("8.8.8.8".to_string())]
    pub ping_server: String,

    #[serde_inline_default(25)]
    pub ping_window: usize,
}

#[derive(Debug)]
struct RxTxRecord {
    rx: u64,
    tx: u64,
    time: Instant,
}

#[derive(Debug)]
pub struct Net {
    pub(crate) cfg: NetConfig,
    pub(crate) mode: DisplayMode,
    // stats
    rxtx: Option<RxTxRecord>,
    rx_ema: Ema<f64>,
    tx_ema: Ema<f64>,
    // ping
    ping_times: VecDeque<f64>,
    ping_received: usize,
    ping_last_seq: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct PingOutput {
    // Typical line: "64 bytes from 8.8.8.8: icmp_seq=1 ttl=117 time=25.6 ms"
    bytes: u32,
    ip: String,
    icmp_seq: u32,
    ttl: u32,
    time_ms: f64,
}

impl Net {
    pub fn from_cfg(cfg: NetConfig) -> Self {
        let ping_times = VecDeque::with_capacity(cfg.ping_window);
        Self {
            mode: DisplayMode::Bandwidth,
            rxtx: None,
            rx_ema: Ema::new(cfg.smoothing_window_sec),
            tx_ema: Ema::new(cfg.smoothing_window_sec),
            ping_times,
            ping_received: 0,
            ping_last_seq: 0,
            cfg,
        }
    }
    fn stop_ping(&mut self) {
        self.ping_times.clear();
    }

    fn refresh_ping_buffer_from(&mut self, lines: Vec<String>) {
        for line in lines {
            if self.ping_times.len() == self.cfg.ping_window {
                self.ping_times.pop_front();
            }

            let slice = line.as_str();
            let po: PingOutput = match scan!("{} bytes from {}: icmp_seq={} ttl={} time={} ms" <- slice)
            {
                Ok(po) => po,
                Err(_) => continue,
            };

            self.ping_times.push_back(po.time_ms);
            self.ping_last_seq = po.icmp_seq;
            self.ping_received += 1;
        }
    }

    fn median_and_mad(samples: &[f64]) -> (f64, f64) {
        let mut v = samples.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = v[v.len() / 2];
        // absolute deviations
        let mut devs: Vec<f64> = v.iter().map(|x| (x - median).abs()).collect();
        devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = devs[devs.len() / 2];
        (median, mad)
    }

    pub(crate) fn read_formatted_ping(&mut self, lines: Vec<String>) -> Markup {
        self.refresh_ping_buffer_from(lines);
        let prefix = Markup::text(format!(
            "net {} [ping {}] ",
            &self.cfg.interface, &self.cfg.ping_server
        ));

        if self.ping_times.len() < 2 {
            return prefix + Markup::text("loading").fg(VIOLET);
        }

        let (med, mad) = Self::median_and_mad(self.ping_times.make_contiguous());
        let med_str = Markup::text(format!("{med:>3.1}"))
            .fg(color_by_pct_custom(med, &[10.0, 20.0, 30.0, 90.0]));
        let mad_str = Markup::text(format!("{mad:>2.1}"))
            .fg(color_by_pct_custom(mad, &[2.0, 5.0, 10.0, 30.0]));
        let loss_pct = 100.0 - 100.0 * self.ping_received as f64 / self.ping_last_seq as f64;
        let loss_str = if loss_pct > 0.0 {
            Markup::text(format!("{loss_pct:>3.1}% loss")).fg(ORANGE)
        } else {
            Markup::text("no loss").fg(GREEN)
        };

        prefix
            + Markup::bracketed(
                Markup::text("med ")
                    + med_str
                    + Markup::text(" mad ")
                    + mad_str
                    + Markup::text(" ms"),
            )
            + Markup::text(" ")
            + Markup::bracketed(loss_str)
    }

    // STATS
    pub(crate) fn read_formatted_stats(&mut self, carrier: Option<&[u8]>) -> Markup {
        let nets = Networks::new_with_refreshed_list();
        let Some(net) = nets.get(self.cfg.interface.as_str()) else {
            return Markup::text(format!("net {} ", self.cfg.interface))
                + Markup::text("gone").fg(RED);
        };
        if carrier
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .is_some_and(|v| v.trim() == "0")
        {
            return Markup::text(format!("net {} ", self.cfg.interface))
                + Markup::text("down").fg(RED);
        }

        let prefix = Markup::text(format!("net {} ", self.cfg.interface));

        let now0 = Instant::now();
        let rx_bytes = net.total_received();
        let tx_bytes = net.total_transmitted();
        let now1 = Instant::now();
        let now = now0 + (now1.duration_since(now0) / 2);

        let cur_rxtx = RxTxRecord {
            rx: rx_bytes,
            tx: tx_bytes,
            time: now,
        };

        let Some(prev_rxtx) = self.rxtx.take() else {
            self.rxtx = Some(cur_rxtx);
            return prefix + Markup::text("loading").fg(VIOLET);
        };

        let dt_sec = cur_rxtx.time.duration_since(prev_rxtx.time).as_secs_f64();
        let drx = cur_rxtx.rx.saturating_sub(prev_rxtx.rx);
        let dtx = cur_rxtx.tx.saturating_sub(prev_rxtx.tx);
        let bps_down = drx as f64 / dt_sec;
        let bps_up = dtx as f64 / dt_sec;

        self.rxtx = Some(cur_rxtx);

        self.rx_ema.feed(bps_down, now);
        self.tx_ema.feed(bps_up, now);

        let bps_down = self.rx_ema.read().unwrap_or(&0.0);
        let bps_up = self.tx_ema.read().unwrap_or(&0.0);

        let mut sfs = [Markup::text("B/s").fg(GREY), Markup::text("B/s").fg(GREY)];
        let mut vals = [*bps_down, *bps_up];
        // Order: [down, up]
        for ix in 0..2 {
            for (mag, sf) in &[
                (30u64, Markup::text("G/s").fg(COL_USE_VERY_HIGH)),
                (20u64, Markup::text("M/s").fg(COL_USE_HIGH)),
                (10u64, Markup::text("K/s").fg(COL_USE_NORM)),
            ] {
                let den = f64::from(1u32 << *mag as u32);
                if vals[ix] > den {
                    vals[ix] /= den;
                    sfs[ix] = sf.clone();
                    break;
                }
            }
        }

        prefix
            + Markup::bracketed(Markup::text(format!("u {:>4.0} ", vals[1])) + sfs[1].clone())
            + Markup::text(" ")
            + Markup::bracketed(Markup::text(format!("d {:4.0} ", vals[0])) + sfs[0].clone())
    }
}

impl Net {
    pub fn handle_click(&mut self, _click: ClickEvent) {
        self.mode = match self.mode {
            DisplayMode::Bandwidth => DisplayMode::Ping,
            DisplayMode::Ping => {
                self.stop_ping();
                DisplayMode::Bandwidth
            }
        };
    }
}
