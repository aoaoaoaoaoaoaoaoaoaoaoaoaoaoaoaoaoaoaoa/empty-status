use crate::core::{color, get_color, Unit, BLUE, CYAN, GREEN, ORANGE, RED, VIOLET};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct Py9Bat {
    name: String,
    poll_interval: f64,
    bat_id: usize,
    min_rem_smooth: Option<f64>,
    called: usize,
    clicked: bool,
    cur_status: Option<u8>,
    p_hist: VecDeque<f64>,
    p_hist_maxlen: usize,
    uevent_path: String,
}

impl Py9Bat {
    pub fn new(poll_interval: f64, bat_id: usize) -> Self {
        let uevent_path = format!("/sys/class/power_supply/BAT{}/uevent", bat_id);
        let p_hist_maxlen = (10.0 / poll_interval).ceil() as usize;
        Self {
            name: "py9bat".to_string(),
            poll_interval,
            bat_id,
            min_rem_smooth: None,
            called: 0,
            clicked: false,
            cur_status: None,
            p_hist: VecDeque::with_capacity(p_hist_maxlen),
            p_hist_maxlen,
            uevent_path,
        }
    }

    fn parse_uevent(&self) -> Result<HashMap<String, Value>> {
        let file = File::open(&self.uevent_path)?;
        let reader = BufReader::new(file);
        let mut out = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            if let Some((k, v)) = line.trim().split_once('=') {
                let key = k
                    .strip_prefix("POWER_SUPPLY_")
                    .unwrap_or(k)
                    .to_ascii_lowercase();
                let val = v.trim().to_ascii_lowercase();
                // Try to parse as int, else as float, else as string
                if let Ok(i) = val.parse::<i64>() {
                    out.insert(key, json!(i));
                } else if let Ok(f) = val.parse::<f64>() {
                    out.insert(key, json!(f));
                } else {
                    out.insert(key, json!(val));
                }
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl Unit for Py9Bat {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn poll_interval(&self) -> f64 {
        self.poll_interval
    }

    async fn read(&mut self) -> Result<HashMap<String, Value>> {
        let mut result = HashMap::new();
        let uh_to_si = 0.0036; // micro-X-hour to SI

        self.called += 1;
        let uevent = match self.parse_uevent() {
            Ok(u) => u,
            Err(_) => {
                result.insert("err_no_bat".to_string(), json!(true));
                return Ok(result);
            }
        };

        if !uevent
            .get("present")
            .and_then(|v| v.as_i64())
            .unwrap_or(1)
            .eq(&1)
        {
            result.insert("err_no_bat".to_string(), json!(true));
            return Ok(result);
        }

        let mut charged_f = 0.0;
        let mut charged_f_design = 0.0;
        let status_str = uevent
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mut p = 0.0;
        let mut e = 0.0;
        let mut emx = 0.0;
        let mut emxd = 0.0;

        // Try charge_* first, else energy_*
        if let (
            Some(charge_now),
            Some(charge_full),
            Some(charge_full_design),
            Some(voltage_now),
            Some(voltage_min_design),
            Some(current_now),
        ) = (
            uevent.get("charge_now").and_then(|v| v.as_i64()),
            uevent.get("charge_full").and_then(|v| v.as_i64()),
            uevent.get("charge_full_design").and_then(|v| v.as_i64()),
            uevent.get("voltage_now").and_then(|v| v.as_i64()),
            uevent.get("voltage_min_design").and_then(|v| v.as_i64()),
            uevent.get("current_now").and_then(|v| v.as_i64()),
        ) {
            let q = uh_to_si * charge_now as f64;
            let qmx = uh_to_si * charge_full as f64;
            let qmxd = uh_to_si * charge_full_design as f64;
            let v = voltage_now as f64 / 1e6;
            let vmn = voltage_min_design as f64 / 1e6;
            let i = current_now as f64 / 1e6;
            p = i * v;

            let vmx = v * (qmx / q);
            emx = qmx * (vmn + vmx) / 2.0;
            let vmxd = v * (qmxd / q);
            emxd = qmxd * (vmn + vmxd) / 2.0;
            e = q * (vmn + q * (vmxd - vmn) / (2.0 * qmxd));
            charged_f = e / emx;
            charged_f_design = e / emxd;
        } else if let (
            Some(energy_now),
            Some(energy_full),
            Some(energy_full_design),
            Some(power_now),
        ) = (
            uevent.get("energy_now").and_then(|v| v.as_i64()),
            uevent.get("energy_full").and_then(|v| v.as_i64()),
            uevent.get("energy_full_design").and_then(|v| v.as_i64()),
            uevent.get("power_now").and_then(|v| v.as_i64()),
        ) {
            e = uh_to_si * energy_now as f64;
            emx = uh_to_si * energy_full as f64;
            emxd = uh_to_si * energy_full_design as f64;
            p = power_now as f64 / 1e6;
            charged_f = e / emx;
            charged_f_design = e / emxd;
        } else {
            result.insert("err_bad_format".to_string(), json!(true));
            return Ok(result);
        }

        // Maintain power history for smoothing
        if self.p_hist.len() == self.p_hist_maxlen {
            self.p_hist.pop_front();
        }
        self.p_hist.push_back(p);
        let av_p = if !self.p_hist.is_empty() {
            self.p_hist.iter().sum::<f64>() / self.p_hist.len() as f64
        } else {
            p
        };

        result.insert("charged_f".to_string(), json!(charged_f));
        result.insert("charged_f_design".to_string(), json!(charged_f_design));

        // Status mapping
        let status = match status_str.as_str() {
            "charging" => 1,
            "discharging" => 0,
            "full" => 3,
            "unknown" => 4,
            _ => {
                if av_p == 0.0 {
                    2 // balancing
                } else {
                    4 // unknown
                }
            }
        };
        result.insert("status".to_string(), json!(status));

        // Reset smoothing if status changes
        if Some(status) != self.cur_status {
            self.min_rem_smooth = None;
            self.cur_status = Some(status);
            self.p_hist.clear();
        }

        // Estimate seconds remaining
        let sec_rem = match status {
            1 => {
                if av_p > 0.0 {
                    (emx - e) / av_p
                } else {
                    -1.0
                }
            }
            0 => {
                if av_p > 0.0 {
                    e / av_p
                } else {
                    -1.0
                }
            }
            _ => -1.0,
        };
        result.insert("sec_rem".to_string(), json!(sec_rem));

        Ok(result)
    }

    fn format(&self, info: &HashMap<String, Value>) -> String {
        let e_prefix = format!("bat{} [{}]", self.bat_id, "{}");

        if info
            .get("err_no_bat")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return e_prefix.replace("{}", &color("no bat", RED));
        }
        if info
            .get("err_bad_format")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return e_prefix.replace("{}", &color("loading", ORANGE));
        }

        let charged_f = info
            .get(if self.clicked {
                "charged_f_design"
            } else {
                "charged_f"
            })
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let pct = 100.0 * charged_f;
        let pct_str = color(
            format!("{:3.0}", pct),
            &get_color(
                pct,
                &[20.0, 40.0, 60.0, 80.0],
                &[BLUE, GREEN, ORANGE, RED, VIOLET],
                true,
            ),
        );

        let st = info.get("status").and_then(|v| v.as_u64()).unwrap_or(4);
        let st_string = match st {
            1 => color("chr", GREEN),
            0 => color("dis", ORANGE),
            3 => color("ful", BLUE),
            2 => color("bal", CYAN),
            _ => color("unk", VIOLET),
        };

        let raw_sec_rem = info.get("sec_rem").and_then(|v| v.as_f64());
        let rem_string = if let Some(sec) = raw_sec_rem {
            if sec < 0.0 {
                "--:--".to_string()
            } else {
                let isr = sec.round() as i64;
                let min_rem = (isr / 60) % 60;
                let hr_rem = isr / 3600;
                format!("{:02}:{:02}", hr_rem, min_rem)
            }
        } else {
            color("loading", VIOLET)
        };

        let x = if !self.clicked {
            ["[", "]"]
        } else {
            ["<", ">"]
        };

        format!(
            "bat {}{}%{} [{} rem, {}]",
            x[0], pct_str, x[1], rem_string, st_string
        )
    }

    fn handle_click(&mut self, _click: crate::core::ClickEvent) {
        self.clicked = !self.clicked;
    }
}
