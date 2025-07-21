use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::type_name;
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast::{channel, Sender};
use tokio::time::{interval, sleep};
use tokio::{select, spawn};
use tracing::warn;

use crate::config::{GlobalConfig, SchedulingCfg};
use crate::display::color;

// Color definitions from the base16 tomorrow theme
// pub const NEAR_BLACK: &str = "#1D1F21";
// pub const DARKER_GREY: &str = "#282A2E";
pub const DARK_GREY: &str = "#373B41";
pub const GREY: &str = "#969896";
// pub const LIGHT_GREY: &str = "#B4B7B4";
// pub const LIGHTER_GREY: &str = "#C5C8C6";
// pub const NEAR_WHITE: &str = "#E0E0E0";
pub const WHITE: &str = "#FFFFFF";
pub const RED: &str = "#CC6666";
pub const ORANGE: &str = "#DE935F";
pub const YELLOW: &str = "#F0C674";
pub const GREEN: &str = "#B5BD68";
pub const CYAN: &str = "#8ABEB7";
pub const BLUE: &str = "#81A2BE";
pub const VIOLET: &str = "#B294BB";
pub const BROWN: &str = "#A3685A";

// Define a trait for unit data structs
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
    pub fn new(name: &str, text: String) -> Self {
        Self {
            full_text: text,
            name: name.to_string(),
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
pub trait Unit: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    // Main method to read and format unit's data
    async fn read_formatted(&mut self) -> Result<String>;

    // Handle click events
    fn handle_click(&mut self, _click: ClickEvent);
}

pub struct UnitWrapper {
    pub unit: Box<dyn Unit>,
    pub cfg: SchedulingCfg,
}

pub struct EmptyStatus {
    wrappers: Vec<UnitWrapper>,
    cfg: GlobalConfig,
    unit_outputs: Arc<Mutex<std::collections::HashMap<&'static str, OutputChunk>>>,
    click_tx: Sender<ClickEvent>,
}

impl EmptyStatus {
    pub fn new(units: Vec<UnitWrapper>, cfg: GlobalConfig) -> Result<Self> {
        // Check for duplicate unit names
        let mut names = HashSet::new();
        for su in &units {
            let name = su.unit.name();
            if !names.insert(name) {
                return Err(anyhow!("Duplicate unit name: {name}"));
            }
        }

        // Initialize unit outputs
        let mut unit_outputs = std::collections::HashMap::new();
        for su in &units {
            let name = su.unit.name();
            let chunk = process_chunk(
                su.unit.name(),
                color(format!("unit '{name}' loading"), VIOLET),
                cfg.padding,
            );
            unit_outputs.insert(name, chunk);
        }

        let (click_tx, _) = channel::<ClickEvent>(16);

        Ok(Self {
            wrappers: units,
            cfg,
            unit_outputs: Arc::new(Mutex::new(unit_outputs)),
            click_tx,
        })
    }

    // Main execution loop
    pub async fn run(self) {
        println!("{{\"version\":1,\"click_events\":true}}\n[");

        let EmptyStatus {
            wrappers,
            cfg,
            unit_outputs,
            click_tx,
        } = self;

        // Precompute order of names for the writer task (since units are moved away).
        let unit_names: Vec<&'static str> = wrappers.iter().map(|u| u.unit.name()).collect();

        spawn(read_clicks_task(click_tx.clone()));

        // Spawn one task per unit, moving each unit in.
        for mut uwrp in wrappers.into_iter() {
            let unit_name = uwrp.unit.name();
            let outputs = Arc::clone(&unit_outputs);
            let mut rx = click_tx.subscribe();

            let mut ticker = interval(Duration::from_secs_f64(uwrp.cfg.poll_interval));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await; // Initial tick to avoid delay

            spawn(async move {
                loop {
                    let do_refresh = select! {
                        Ok(click) = rx.recv() => {
                            if click.name == unit_name {
                                uwrp.unit.handle_click(click);
                                true // Refresh on click
                            } else {
                                false // Ignore clicks for other units
                            }
                        },
                        _ = ticker.tick() => true
                    };
                    if do_refresh {
                        let result = match uwrp.unit.read_formatted().await {
                            Err(e) => color(format!("{unit_name} failed: {e}"), BROWN),
                            Ok(formatted) => formatted,
                        };

                        let mut guard = outputs.lock().unwrap();
                        guard.insert(unit_name, process_chunk(unit_name, result, cfg.padding));
                    }

                    sleep(Duration::from_millis(
                        (uwrp.cfg.poll_interval * 250.0) as u64,
                    ))
                    .await;
                }
            });
        }

        // Spawn writer (no self reference; use unit_names + unit_outputs)
        {
            let outputs = Arc::clone(&unit_outputs);
            let unit_names = unit_names.clone();
            spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs_f64(cfg.min_sleep));
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
// Process the unit's output with the padding
fn process_chunk(name: &str, text: String, padding: i32) -> OutputChunk {
    let mut chunk = OutputChunk::new(name, text);

    let pad = " ".repeat(padding as usize);
    chunk.full_text = format!("{pad}{}{pad}", chunk.full_text);

    chunk
}

async fn read_clicks_task(click_tx: Sender<ClickEvent>) {
    let mut lines = BufReader::new(stdin()).lines();
    let _ = lines.next_line().await;
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line = line.trim_end_matches(',');
        match serde_json::from_str::<ClickEvent>(line.trim_start_matches(',')) {
            Ok(click) => {
                let _ = click_tx.send(click);
            }
            Err(e) => {
                warn!(%line, %e, "Failed to parse click event");
            }
        }
    }
}
