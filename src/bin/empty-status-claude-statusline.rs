#![cfg(target_os = "linux")]
#![allow(
    unused_crate_dependencies,
    reason = "thin bin delegates to the shared empty_status library target"
)]

use anyhow::Result;

fn main() -> Result<()> {
    empty_status::run_claude_statusline()
}
