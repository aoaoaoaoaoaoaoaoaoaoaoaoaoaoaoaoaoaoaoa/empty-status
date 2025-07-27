use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::type_name;
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::Mutex;
use tokio::time::interval;
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
// pub const WHITE: &str = "#FFFFFF";
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
    // we make this an infallible string since units are, in essence, already
    // mixing data and error information in the display band. a formal error
    // path would be artificial make-work when the unit can and should just format
    // its own error message prettily.
    async fn read_formatted(&mut self) -> String;
    fn handle_click(&mut self, _click: ClickEvent);
}

pub struct UnitWrapper {
    pub unit: Box<dyn Unit>,
    pub cfg: SchedulingCfg,
    pub handle: usize,
}

pub struct EmptyStatus {
    wrappers: Vec<UnitWrapper>,
    cfg: GlobalConfig,
    unit_outputs: Arc<Mutex<std::collections::HashMap<usize, OutputChunk>>>,
    click_tx: Sender<ClickEvent>,
}

impl EmptyStatus {
    pub fn new(units: Vec<UnitWrapper>, cfg: GlobalConfig) -> Result<Self> {
        let mut handles = HashSet::<usize>::new();
        for su in &units {
            if !handles.insert(su.handle) {
                return Err(anyhow!("Duplicate unit handle: {}", su.handle));
            }
        }

        let mut unit_outputs = std::collections::HashMap::new();
        for su in &units {
            let name = su.unit.name();
            let chunk = process_chunk(
                su.unit.name(),
                color(format!("unit '{name}' loading"), VIOLET),
                cfg.padding,
            );
            unit_outputs.insert(su.handle, chunk);
        }

        let (click_tx, _) = channel::<ClickEvent>(16);

        Ok(Self {
            wrappers: units,
            cfg,
            unit_outputs: Arc::new(Mutex::new(unit_outputs)),
            click_tx,
        })
    }

    pub async fn run(self) {
        println!("{{\"version\":1,\"click_events\":true}}\n[");

        let EmptyStatus {
            wrappers,
            cfg,
            unit_outputs,
            click_tx,
        } = self;

        let handles: Vec<usize> = wrappers.iter().map(|u| u.handle).collect();

        spawn(read_clicks_task(click_tx.clone()));

        for mut uwrp in wrappers.into_iter() {
            let unit_name = uwrp.unit.name();
            let outputs = Arc::clone(&unit_outputs);
            let mut rx = click_tx.subscribe();

            let mut ticker = interval(Duration::from_secs_f64(
                uwrp.cfg.poll_interval.max(cfg.min_polling_interval),
            ));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await;

            spawn(async move {
                loop {
                    let do_refresh = select! {
                        Ok(click) = rx.recv() => {
                            if click.name == unit_name {
                                uwrp.unit.handle_click(click);
                                true
                            } else {
                                false
                            }
                        },
                        _ = ticker.tick() => true
                    };
                    if do_refresh {
                        let result = uwrp.unit.read_formatted().await;
                        let mut guard = outputs.lock().await;
                        guard.insert(uwrp.handle, process_chunk(unit_name, result, cfg.padding));
                    }
                }
            });
        }
        {
            let outputs = Arc::clone(&unit_outputs);
            let handles = handles.clone();
            spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs_f64(cfg.min_polling_interval));
                loop {
                    interval.tick().await;
                    let guard = outputs.lock().await;
                    let mut chunks = Vec::with_capacity(handles.len());
                    for name in &handles {
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
        {
            let guard = unit_outputs.lock().await;
            let mut chunks = Vec::with_capacity(handles.len());
            for name in &handles {
                if let Some(chunk) = guard.get(name) {
                    chunks.push(serde_json::to_string(chunk).unwrap_or_default());
                }
            }
            println!("[{}],", chunks.join(","));
        }
        std::future::pending::<()>().await;
    }
}
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
