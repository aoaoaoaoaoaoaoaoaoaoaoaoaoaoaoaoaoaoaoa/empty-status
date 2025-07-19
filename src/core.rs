use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::{channel, Sender};
use tokio::time::{interval, sleep};
use tokio::{select, spawn};
use tracing::debug;

// Color definitions from the base16 tomorrow theme
pub const NEAR_BLACK: &str = "#1D1F21";
pub const DARKER_GREY: &str = "#282A2E";
pub const DARK_GREY: &str = "#373B41";
pub const GREY: &str = "#969896";
pub const LIGHT_GREY: &str = "#B4B7B4";
pub const LIGHTER_GREY: &str = "#C5C8C6";
pub const NEAR_WHITE: &str = "#E0E0E0";
pub const WHITE: &str = "#FFFFFF";
pub const RED: &str = "#CC6666";
pub const ORANGE: &str = "#DE935F";
pub const YELLOW: &str = "#F0C674";
pub const GREEN: &str = "#B5BD68";
pub const CYAN: &str = "#8ABEB7";
pub const BLUE: &str = "#81A2BE";
pub const VIOLET: &str = "#B294BB";
pub const BROWN: &str = "#A3685A";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OutputChunk {
    pub full_text: String,
    pub name: String,
    pub markup: String,
    pub border: String,
    pub separator: String,
    pub separator_block_width: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl OutputChunk {
    pub fn new(name: String, text: String) -> Self {
        Self {
            full_text: text,
            name,
            markup: "pango".to_string(),
            border: DARK_GREY.to_string(),
            separator: "false".to_string(),
            separator_block_width: 0,
            background: None,
            color: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClickEvent {
    pub name: String,
    pub instance: Option<String>,
    pub button: i32,
    pub modifiers: Vec<String>,
    pub x: i32,
    pub y: i32,
    pub relative_x: i32,
    pub relative_y: i32,
    pub width: i32,
    pub height: i32,
}

#[async_trait]
pub trait Unit: Send + Sync {
    fn name(&self) -> String;
    fn poll_interval(&self) -> f64;

    // Main method to read unit's data
    async fn read(&mut self) -> Result<HashMap<String, Value>>;

    // Format the data into a display string
    fn format(&self, data: &HashMap<String, Value>) -> String;

    // Process the unit's output with the padding
    fn process_chunk(&self, text: String, padding: i32) -> OutputChunk {
        let mut chunk = OutputChunk::new(self.name(), text);

        let pad = " ".repeat(padding as usize);
        chunk.full_text = format!("{pad}{}{pad}", chunk.full_text);

        chunk
    }

    // Handle click events
    fn handle_click(&mut self, _click: ClickEvent) {}
}

pub struct Status {
    units: Vec<Box<dyn Unit>>,
    padding: i32,
    min_sleep: f64,
    unit_outputs: Arc<Mutex<HashMap<String, OutputChunk>>>,
    click_tx: Sender<ClickEvent>,
}

async fn read_clicks_task(click_tx: Sender<ClickEvent>) {
    use tokio::io::{stdin, AsyncBufReadExt, BufReader};
    use tracing::{debug, warn};
    let mut lines = BufReader::new(stdin()).lines();
    if let Ok(Some(_)) = lines.next_line().await {
        debug!("Skipped first line of click input");
    }
    if let Ok(Some(_)) = lines.next_line().await {
        debug!("Skipped second line of click input");
    }
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line = line.trim_end_matches(',');
        match serde_json::from_str::<ClickEvent>(line.trim_start_matches(',')) {
            Ok(click) => {
                debug!(?click, "Received click event");
                let _ = click_tx.send(click);
            }
            Err(e) => {
                warn!(%line, %e, "Failed to parse click event");
            }
        }
    }
}

impl Status {
    pub fn new(units: Vec<Box<dyn Unit>>, min_sleep: f64, padding: i32) -> Result<Self> {
        // Check for duplicate unit names
        let mut names = HashSet::new();
        for unit in &units {
            let name = unit.name();
            if !names.insert(name.clone()) {
                return Err(anyhow!("Duplicate unit name: {name}"));
            }
        }

        // Initialize unit outputs
        let mut unit_outputs = HashMap::new();
        for unit in &units {
            let name = unit.name();
            let chunk =
                unit.process_chunk(color(format!("unit '{name}' loading"), VIOLET), padding);
            unit_outputs.insert(name, chunk);
        }

        let (click_tx, _) = channel::<ClickEvent>(16);

        Ok(Self {
            units,
            padding,
            min_sleep,
            unit_outputs: Arc::new(Mutex::new(unit_outputs)),
            click_tx,
        })
    }

    // Main execution loop
    pub async fn run(self) {
        println!("{{\"version\":1,\"click_events\":true}}\n[");

        let Status {
            units,
            padding,
            min_sleep,
            unit_outputs,
            click_tx,
        } = self;

        // Precompute order of names for the writer task (since units are moved away).
        let unit_names: Vec<String> = units.iter().map(|u| u.name()).collect();

        spawn(read_clicks_task(click_tx.clone()));

        // Spawn one task per unit, moving each unit in.
        for mut unit in units.into_iter() {
            let unit_name = unit.name(); // copy String (returned new String each call)
            let poll_interval = unit.poll_interval();
            let outputs = Arc::clone(&unit_outputs);
            let mut rx = click_tx.subscribe();

            let mut ticker = interval(Duration::from_secs_f64(poll_interval));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await; // Initial tick to avoid delay

            spawn(async move {
                loop {
                    let do_refresh = select! {
                        Ok(click) = rx.recv() => {
                            if click.name == unit_name {
                                unit.handle_click(click);
                                true // Refresh on click
                            } else {
                                false // Ignore clicks for other units
                            }
                        },
                        _ = ticker.tick() => true
                    };
                    if do_refresh {
                        let result = match unit.read().await {
                            Ok(data) => unit.format(&data),
                            Err(e) => color(format!("{unit_name} failed: {e}"), BROWN),
                        };

                        let mut guard = outputs.lock().unwrap();
                        guard.insert(unit_name.clone(), unit.process_chunk(result, padding));
                    }

                    sleep(Duration::from_millis((poll_interval * 250.0) as u64)).await;
                }
            });
        }

        // Spawn writer (no self reference; use unit_names + unit_outputs)
        {
            let outputs = Arc::clone(&unit_outputs);
            let unit_names = unit_names.clone();
            spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs_f64(min_sleep));
                loop {
                    interval.tick().await;
                    let guard = outputs.lock().unwrap();
                    let mut chunks = Vec::with_capacity(unit_names.len());
                    for name in &unit_names {
                        if let Some(chunk) = guard.get(name) {
                            chunks.push(serde_json::to_string(chunk).unwrap_or_default());
                        }
                    }
                    let line = format!("[{}],\n", chunks.join(","));
                    let _ = io::stdout().write_all(line.as_bytes());
                    let _ = io::stdout().flush();
                }
            });
        }

        // Initial line
        {
            let guard = unit_outputs.lock().unwrap();
            let mut chunks = Vec::with_capacity(unit_names.len());
            for name in &unit_names {
                if let Some(chunk) = guard.get(name) {
                    chunks.push(serde_json::to_string(chunk).unwrap_or_default());
                }
            }
            println!("[{}],", chunks.join(","));
        }

        // Park forever (or return JoinHandles instead)
        std::future::pending::<()>().await;
    }
}

// Helper functions

// Add color to text using pango markup
pub fn color<S: Into<String>>(text: S, color: &str) -> String {
    pangofy(text, Some(color), None)
}

// Create a pango formatted string
pub fn pangofy<S: Into<String>>(text: S, color: Option<&str>, background: Option<&str>) -> String {
    let mut attrs = Vec::new();

    if let Some(c) = color {
        attrs.push(format!("color='{c}'"));
    }

    if let Some(bg) = background {
        attrs.push(format!("background='{bg}'"));
    }

    let text = text.into();
    if attrs.is_empty() {
        text
    } else {
        format!("<span {}>{}</span>", attrs.join(" "), text)
    }
}

// Get appropriate color based on value and thresholds
pub fn get_color(value: f64, breakpoints: &[f64], colors: &[&str], reverse: bool) -> String {
    assert_eq!(colors.len(), breakpoints.len() + 1);

    let colors = if reverse {
        colors.iter().rev().copied().collect::<Vec<&str>>()
    } else {
        colors.to_vec()
    };

    let mut index = 0;
    for (i, &bp) in breakpoints.iter().enumerate() {
        if value <= bp {
            index = i;
            break;
        }
        index = i + 1;
    }

    colors[index].to_string()
}

// Format a temperature value with color
pub fn make_temp_color_str(temp: f64) -> String {
    let breakpoints = [30.0, 50.0, 70.0, 90.0];
    let colors = [BLUE, GREEN, YELLOW, ORANGE, RED];

    if temp < 100.0 {
        color(
            format!("{temp:.0}"),
            &get_color(temp, &breakpoints, &colors, false),
        )
    } else {
        pangofy(format!("{temp:.0}"), Some(WHITE), Some(RED))
    }
}

// Format a value, automatically choosing a time unit
pub fn format_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        // Handle small values
        let (value, unit) = if seconds < 1e-9 {
            (seconds * 1e12, "ps")
        } else if seconds < 1e-6 {
            (seconds * 1e9, "ns")
        } else if seconds < 1e-3 {
            (seconds * 1e6, "μs")
        } else if seconds < 1.0 {
            (seconds * 1e3, "ms")
        } else {
            (seconds, "s")
        };

        let precision = std::cmp::max(0, 2 - value.log10().floor() as i32);
        format!(
            "  {:.precision$} {:<2} ",
            value,
            unit,
            precision = precision as usize
        )
    } else if seconds < 3155760000.0 {
        // Less than 10 years
        if seconds < 3600.0 {
            // < 1 hour
            let min = (seconds / 60.0).floor() as i32;
            let sec = (seconds % 60.0) as i32;
            format!("{min:2} m {sec:2} s")
        } else if seconds < 86400.0 {
            // < 1 day
            let hr = (seconds / 3600.0).floor() as i32;
            let min = ((seconds % 3600.0) / 60.0) as i32;
            format!("{hr:2} h {min:2} m")
        } else if seconds < 604800.0 {
            // < 1 week
            let day = (seconds / 86400.0).floor() as i32;
            let hr = ((seconds % 86400.0) / 3600.0) as i32;
            format!("{day:2} d {hr:2} h")
        } else if seconds < 31557600.0 {
            // < 1 year
            let week = (seconds / 604800.0).floor() as i32;
            let day = ((seconds % 604800.0) / 86400.0) as i32;
            format!("{week:2} w {day:2} d")
        } else {
            // < 10 years
            let year = (seconds / 31557600.0).floor() as i32;
            let week = ((seconds % 31557600.0) / 604800.0) as i32;
            format!("{year:2} y {week:2} w")
        }
    } else {
        " > 10 y  ".to_string()
    }
}

// Helper to format a float with color based on thresholds
pub fn colorize_float(val: f64, width: usize, prec: usize, breakpoints: &[f64]) -> String {
    color(
        format!("{val:width$.prec$}"),
        &get_color(val, breakpoints, &[BLUE, GREEN, YELLOW, ORANGE, RED], false),
    )
}
