mod core;
mod units;

use chrono::Local;
use tracing_appender::non_blocking;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt;

use crate::core::Status;

fn init_file_logger() -> tracing_appender::non_blocking::WorkerGuard {
    // produce a timestamped filename under /tmp
    let filename = format!("i3status-{}.log", Local::now().format("%Y%m%d-%H%M%S"));
    // Rotation::NEVER means "never roll over"; it just creates a single file.
    let file_appender: RollingFileAppender =
        RollingFileAppender::new(Rotation::NEVER, "/tmp", filename);
    // non-blocking wrapper to avoid blocking your status‐bar event loop
    let (non_blocking_appender, guard) = non_blocking(file_appender);
    // install a subscriber that writes only to our file
    fmt()
        .with_writer(non_blocking_appender)
        .with_max_level(tracing::Level::TRACE)
        .init();
    guard // must be held for the lifetime of the program so it can flush
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _guard = init_file_logger();

    // Define units to display in the status bar
    let units: Vec<Box<dyn core::Unit>> = vec![
        Box::new(units::bat::RS9Bat::new(5.0)),
        Box::new(units::disk::RS9Disk::new(30.0)),
        Box::new(units::wifi::RS9Wifi::new(5.0)),
        Box::new(units::mem::Mem::new(3.0)),
        Box::new(units::cpu::CPU::new(0.33)),
        Box::new(units::time::Time::new("%a %b %d %Y - %H:%M".to_string(), 0.7)),
    ];

    let rs9status = Status::new(units, 0.1, 1)?;
    rs9status.run().await;

    Ok(())
}
