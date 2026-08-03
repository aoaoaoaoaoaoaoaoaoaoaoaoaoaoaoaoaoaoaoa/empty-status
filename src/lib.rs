#![cfg(target_os = "linux")]

use std::fs::OpenOptions;

mod config;
mod core;
mod display;
mod probe_io;
mod reactor;
mod render;
mod state;
mod units;
mod util;

use anyhow::Result;
use tracing::{info, level_filters::LevelFilter};
use tracing_appender::non_blocking;
use tracing_subscriber::{EnvFilter, fmt};

fn init_file_logger() -> Option<non_blocking::WorkerGuard> {
    let path = xdg::BaseDirectories::with_prefix("empty-status")
        .place_state_file("last.log")
        .ok()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .ok()?;
    let (writer, guard) = non_blocking(file);
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    fmt().with_env_filter(filter).with_writer(writer).init();
    Some(guard)
}

pub async fn run_bar() -> Result<()> {
    let _guard = init_file_logger();
    info!("starting empty-status");
    config::load()?.run().await
}

pub fn run_claude_statusline() -> Result<()> {
    units::quota::run_claude_statusline()
}
