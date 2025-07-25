use crate::core::{ClickEvent, Unit, GREEN, GREY, ORANGE, RED, VIOLET};
use crate::display::{color, color_by_pct_custom, COL_USE_HIGH, COL_USE_NORM, COL_USE_VERY_HIGH};
use crate::util::{Ema, Smoother};
use crate::{mode_enum, register_unit};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use serde_scan::scan;
use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Instant;
use sysinfo::Networks;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::warn;

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
    cfg: NetConfig,
    mode: DisplayMode,
    // stats
    rxtx: Option<RxTxRecord>,
    rx_ema: Ema<f64>,
    tx_ema: Ema<f64>,
    // ping
    ping_child: Option<Child>,
    ping_rx: mpsc::UnboundedReceiver<PingOutput>,
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
        // create a dummy closed rx until ping is started
        // TODO suggested by o3... is this just to avoid Option?
        let (_tx, rx) = mpsc::unbounded_channel();
        let ping_times = VecDeque::with_capacity(cfg.ping_window);
        Self {
            cfg: cfg.clone(),
            mode: DisplayMode::Bandwidth,
            rxtx: None,
            rx_ema: Ema::new(cfg.smoothing_window_sec),
            tx_ema: Ema::new(cfg.smoothing_window_sec),
            ping_child: None,
            ping_rx: rx,
            ping_times,
            ping_received: 0,
            ping_last_seq: 0,
        }
    }
    /// Spawn the system ping command and stream rtt values (ms) into an mpsc.
    fn start_ping(&mut self) -> Result<()> {
        // If already running, nothing to do
        if self.ping_child.is_some() {
            return Ok(());
        }

        let mut child = Command::new("ping")
            .arg("-n") // flood-protect per platform (works on Linux/macOS)
            .arg("-O") // report duplicates/lost early so every pkt has RTT
            .arg("-I")
            .arg(&self.cfg.interface)
            .arg(&self.cfg.ping_server)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .context("failed to spawn ping")?;

        let stdout = child.stdout.take().context("could not open ping stdout")?;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let (tx, rx) = mpsc::unbounded_channel();
        self.ping_rx = rx;

        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                let slice = &line;
                let po: PingOutput = match scan!("{} bytes from {}: icmp_seq={} ttl={} time={} ms" <- slice)
                {
                    Ok(po) => po,
                    Err(_) => {
                        continue;
                    }
                };
                let _ = tx.send(po);
            }
        });

        self.ping_child = Some(child);
        Ok(())
    }

    /// Stop and clean up the running ping command, if any.
    fn stop_ping(&mut self) {
        if let Some(mut child) = self.ping_child.take() {
            // fire-and-forget kill (ignore errors)
            std::mem::drop(child.kill());
        }
        self.ping_times.clear();
    }

    /// Drain any new rtt samples from the mpsc into our circular buffer.
    fn refresh_ping_buffer(&mut self) {
        while let Ok(po) = self.ping_rx.try_recv() {
            if self.ping_times.len() == self.cfg.ping_window {
                self.ping_times.pop_front();
            }
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

    fn read_formatted_ping(&mut self) -> String {
        self.refresh_ping_buffer();
        let prefix = format!(
            "net {} [ping {}] ",
            &self.cfg.interface, &self.cfg.ping_server
        );

        if self.ping_times.len() < 2 {
            return prefix + &color("loading", VIOLET);
        }

        let (med, mad) = Self::median_and_mad(self.ping_times.make_contiguous());
        let med_str = color(
            format!("{med:>3.1}"),
            color_by_pct_custom(med, &[10.0, 20.0, 30.0, 90.0]),
        );
        let mad_str = color(
            format!("{mad:>2.1}"),
            color_by_pct_custom(mad, &[2.0, 5.0, 10.0, 30.0]),
        );
        let loss_pct = 100.0 - 100.0 * self.ping_received as f64 / self.ping_last_seq as f64;
        let loss_str = if loss_pct > 0.0 {
            color(format!("{loss_pct:>3.1}% loss"), ORANGE)
        } else {
            color("no loss", GREEN)
        };
        format!("{prefix}[med {med_str} mad {mad_str} ms] [{loss_str}]")
    }

    // STATS
    fn read_formatted_stats(&mut self) -> String {
        // Query sysinfo network interface data
        let nets = Networks::new_with_refreshed_list();
        let Some(net) = nets.get(self.cfg.interface.as_str()) else {
            // If interface not found, return gone
            return format!("net {} {}", self.cfg.interface, color("gone", RED));
        };
        // Check carrier state
        let carrier_path = format!("/sys/class/net/{}/carrier", self.cfg.interface);
        if let Ok(carrier) = std::fs::read_to_string(&carrier_path) {
            if carrier.trim() == "0" {
                return format!("net {} {}", self.cfg.interface, color("down", RED));
            }
        }

        let prefix = format!("net {} ", self.cfg.interface);

        // always work with totals and our own timestamps
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

        let prev_rxtx = match self.rxtx.take() {
            Some(prev_rxtx) => prev_rxtx,
            None => {
                self.rxtx = Some(cur_rxtx);
                return prefix + &color("loading", VIOLET);
            }
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

        // Output formatting logic (ping block on click, bandwidth stats otherwise)
        // For colorizing magnitude
        let mut sfs = [color("B/s", GREY), color("B/s", GREY)];
        let mut vals = [*bps_down, *bps_up];
        // Order: [down, up]
        for ix in 0..2 {
            for (mag, sf) in [
                (30u64, color("G/s", COL_USE_VERY_HIGH)),
                (20u64, color("M/s", COL_USE_HIGH)),
                (10u64, color("K/s", COL_USE_NORM)),
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

        // Compose bandwidth stats line
        format!(
            "{}[u {:>4.0} {:>3}] [d {:4.0} {:>3}]",
            prefix, vals[1], sfs[1], vals[0], sfs[0],
        )
    }
}

#[async_trait]
impl Unit for Net {
    async fn read_formatted(&mut self) -> String {
        match self.mode {
            DisplayMode::Bandwidth => self.read_formatted_stats(),
            DisplayMode::Ping => self.read_formatted_ping(),
        }
    }
    fn handle_click(&mut self, _click: ClickEvent) {
        self.mode = match self.mode {
            DisplayMode::Bandwidth => {
                if let Err(err) = self.start_ping() {
                    warn!("failed to start ping: {err}");
                }
                DisplayMode::Ping
            }
            DisplayMode::Ping => {
                self.stop_ping();
                DisplayMode::Bandwidth
            }
        };
    }
}

register_unit!(Net, NetConfig);
