#![cfg(target_os = "linux")]
mod config;
mod core;
mod display;
mod registry;
mod units;
mod util;

use anyhow::Result;
use tracing::{info, level_filters::LevelFilter};
use tracing_appender::{
    non_blocking,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{fmt, EnvFilter};

use crate::config::load_status_from_cfg;

inventory::collect!(crate::registry::UnitFactory);

fn init_file_logger() -> Option<non_blocking::WorkerGuard> {
    let bd = xdg::BaseDirectories::with_prefix("empty-status");
    let log_dir = bd.get_state_home()?;
    let filename = "last.log";
    let file_appender: RollingFileAppender =
        RollingFileAppender::new(Rotation::NEVER, log_dir, filename);
    let (non_blocking_appender, guard) = non_blocking(file_appender);

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env_lossy();
    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking_appender)
        .init();

    Some(guard) // must be held for the lifetime of the program so it can flush
}

#[tokio::main]
async fn main() -> Result<()> {
    let _guard = init_file_logger();
    info!("Starting empty-status!");
    let status = load_status_from_cfg()?;
    status.run().await;
    Ok(())
}
