use crate::core::{ClickEvent, Unit, GREY, ORANGE, RED, VIOLET, WHITE};
use crate::display::color;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::Instant;
use sysinfo::Networks;

const NET_POLL_INTERVAL: f64 = 0.5;
const MAX_HISTORY_SECONDS: f64 = 2.0; // match py, cover 2 seconds of stats

/// Rust net unit for interface statistics and ping
pub struct Net {
    if_name: String,
    rx_hist: VecDeque<u64>, // (bytes, time)
    tx_hist: VecDeque<u64>,
    time_hist: VecDeque<Instant>,
    is_pinging: bool,
}

impl Net {
    pub fn new<S: Into<String>>(if_name: S) -> Self {
        let maxlen = ((MAX_HISTORY_SECONDS / NET_POLL_INTERVAL).ceil() as usize).max(2);
        Self {
            if_name: if_name.into(),
            rx_hist: VecDeque::with_capacity(maxlen),
            tx_hist: VecDeque::with_capacity(maxlen),
            time_hist: VecDeque::with_capacity(maxlen),
            // TODO mode enum
            is_pinging: false,
        }
    }
}

#[async_trait]
impl Unit for Net {
    fn poll_interval(&self) -> f64 {
        NET_POLL_INTERVAL
    }

    async fn read_formatted(&mut self) -> Result<String> {
        // Query sysinfo network interface data
        let nets = Networks::new_with_refreshed_list();
        let Some(net) = nets.get(self.if_name.as_str()) else {
            // If interface not found, return gone
            return Ok(format!("net {} {}", self.if_name, color("gone", RED)));
        };
        let prefix = format!("net {} ", self.if_name);

        // always work with totals and our own timestamps
        let now0 = Instant::now();
        let rx_bytes = net.total_received();
        let tx_bytes = net.total_transmitted();
        let now1 = Instant::now();
        let now = now0 + (now1.duration_since(now0) / 2);

        // Update buffer
        let maxlen = ((MAX_HISTORY_SECONDS / NET_POLL_INTERVAL).ceil() as usize).max(2);
        if self.rx_hist.len() == maxlen {
            self.rx_hist.pop_front();
            self.tx_hist.pop_front();
            self.time_hist.pop_front();
        }
        self.rx_hist.push_back(rx_bytes);
        self.tx_hist.push_back(tx_bytes);
        self.time_hist.push_back(now);

        if self.rx_hist.len() < 2 {
            return Ok(prefix + &color("loading", VIOLET));
        }
        // Calculate Bps in/out (down/up) from buffer
        let (old_rx, old_tx, old_time) = {
            let first_ix = 0;
            (
                self.rx_hist[first_ix],
                self.tx_hist[first_ix],
                self.time_hist[first_ix],
            )
        };
        let (new_rx, new_tx, new_time) = {
            let last_ix = self.rx_hist.len() - 1;
            (
                self.rx_hist[last_ix],
                self.tx_hist[last_ix],
                self.time_hist[last_ix],
            )
        };
        let dt = new_time - old_time;
        let drx = new_rx.saturating_sub(old_rx);
        let dtx = new_tx.saturating_sub(old_tx);
        let bps_down = drx as f64 / dt.as_secs_f64();
        let bps_up = dtx as f64 / dt.as_secs_f64();

        // Output formatting logic (ping block on click, bandwidth stats otherwise)
        // For colorizing magnitude
        let mut sfs = [color("B/s", GREY), color("B/s", GREY)];
        let mut vals = [bps_down, bps_up];
        // Order: [down, up]
        for ix in 0..2 {
            for (mag, sf) in [
                (30u64, color("G/s", VIOLET)),
                (20u64, color("M/s", WHITE)),
                (10u64, "K/s".to_string()),
            ]
            .iter()
            {
                if vals[ix] > (1u64 << *mag) as f64 {
                    vals[ix] /= (1u64 << *mag) as u32 as f64;
                    sfs[ix] = sf.clone();
                    break;
                }
            }
        }

        // Ping: toggling not implemented, stub results as not pinging
        if self.is_pinging {
            // Dummy ping stub
            // let ping_prefix = format!(
            //     "net {} [ping {}] ",
            //     self.if_name,
            //     self.ping_server.as_deref().unwrap_or("HOST")
            // );
            // Not actually pinging, so display as loading
            // return Ok(ping_prefix + &color("loading", VIOLET));
            unimplemented!("Ping functionality not implemented yet");
        }

        // Compose bandwidth stats line
        Ok(format!(
            "{}[u {:6.1} {:>3}] [d {:6.1} {:>3}]",
            prefix, vals[1], sfs[1], vals[0], sfs[0],
        ))
    }

    fn handle_click(&mut self, _click: ClickEvent) {
        // Toggle is_pinging
        self.is_pinging = !self.is_pinging;
        // In production: spawn/cancel async ping logic here.
    }
}
