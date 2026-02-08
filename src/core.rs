use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::broadcast::{channel, Sender};
use tracing::warn;

use crate::config::GlobalConfig;
use crate::machine::runtime::{run_empty_status_machines, MachineWrapper};

// Color definitions from the base16 tomorrow theme
pub const DARK_GREY: &str = "#373B41";
pub const GREY: &str = "#969896";
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

/// the fields of this object come directly from i3 and should not be touched
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkHealth {
    Ok,
    Warn,
    Err,
}

#[derive(Debug, Clone)]
pub struct Readout {
    pub markup: crate::render::markup::Markup,
    pub health: ChunkHealth,
}

impl Readout {
    pub fn ok(markup: crate::render::markup::Markup) -> Self {
        Self {
            markup,
            health: ChunkHealth::Ok,
        }
    }

    pub fn warn(markup: crate::render::markup::Markup) -> Self {
        Self {
            markup,
            health: ChunkHealth::Warn,
        }
    }

    pub fn err(markup: crate::render::markup::Markup) -> Self {
        Self {
            markup,
            health: ChunkHealth::Err,
        }
    }
}

#[async_trait::async_trait]
pub trait Unit: Send + std::fmt::Debug {
    async fn read_formatted(&mut self) -> Readout;

    fn handle_click(&mut self, _click: ClickEvent) {}

    fn fix_up_and_validate(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct EmptyStatus {
    cfg: GlobalConfig,
    machine_wrappers: Vec<MachineWrapper>,
    machine_click_tx: tokio::sync::broadcast::Sender<ClickEvent>,
}

impl EmptyStatus {
    pub fn new(
        cfg: GlobalConfig,
        machine_wrappers: Vec<MachineWrapper>,
        machine_click_tx: tokio::sync::broadcast::Sender<ClickEvent>,
    ) -> Self {
        Self {
            cfg,
            machine_wrappers,
            machine_click_tx,
        }
    }

    pub async fn run(self) {
        let (click_tx, _) = channel::<ClickEvent>(16);
        tokio::spawn(read_clicks_task(click_tx.clone()));

        // bridge i3 click events to machine bus
        {
            let machine_click_tx = self.machine_click_tx.clone();
            let mut legacy_rx = click_tx.subscribe();
            tokio::spawn(async move {
                while let Ok(click) = legacy_rx.recv().await {
                    let _ = machine_click_tx.send(click);
                }
            });
        }

        run_empty_status_machines(self.machine_wrappers, self.cfg, self.machine_click_tx).await;
    }
}

async fn read_clicks_task(click_tx: Sender<ClickEvent>) {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
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
