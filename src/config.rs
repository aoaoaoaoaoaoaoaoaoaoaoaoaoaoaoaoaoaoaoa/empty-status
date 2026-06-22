use anyhow::{Context, Result};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::{fs, path::PathBuf};
use tracing::{debug, error, info, warn};
use xdg::BaseDirectories;

use crate::core::{EmptyStatus, RED};
use crate::machine::runtime::{MachineWrapper, spawn_machine_actor};
use crate::machine::types::{Health, View};
use crate::machine::units::bat::BatMachine;
use crate::machine::units::cpu::CpuMachine;
use crate::machine::units::disk::DiskMachine;
use crate::machine::units::mem::MemMachine;
use crate::machine::units::net::NetMachine;
use crate::machine::units::quota::QuotaMachine;
use crate::machine::units::time::TimeMachine;
use crate::machine::units::weather::WeatherMachine;
use crate::machine::units::wifi::WifiMachine;
use crate::render::markup::Markup;

const CONFIG_PREFIX: &str = "empty-status";
const CONFIG_FILE: &str = "config.toml";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct RootConfig {
    #[serde(default)]
    units: Vec<toml::Value>,
    #[serde(default)]
    global: GlobalConfig,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum UnitConfig {
    #[serde(rename = "Weather")]
    Weather(UnitSpec<crate::units::weather::WeatherConfig>),
    #[serde(rename = "Time")]
    Time(UnitSpec<crate::units::time::TimeConfig>),
    #[serde(rename = "Cpu")]
    Cpu(UnitSpec<crate::units::cpu::CpuConfig>),
    #[serde(rename = "Mem")]
    Mem(UnitSpec<crate::units::mem::MemConfig>),
    #[serde(rename = "Disk")]
    Disk(UnitSpec<crate::units::disk::DiskConfig>),
    #[serde(rename = "Wifi")]
    Wifi(UnitSpec<crate::units::wifi::WifiConfig>),
    #[serde(rename = "Bat")]
    Bat(UnitSpec<crate::units::bat::BatConfig>),
    #[serde(rename = "Net")]
    Net(UnitSpec<crate::units::net::NetConfig>),
    #[serde(rename = "Quota")]
    Quota(UnitSpec<crate::units::quota::QuotaConfig>),

    // Stub for future drop-in units. Intentionally not implemented yet.
    // When we do, we should make this a hard boundary with explicit schema and effects.
    #[serde(other)]
    _External,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct UnitSpec<Cfg> {
    #[serde(flatten)]
    sched: SchedulingCfg,
    #[serde(flatten)]
    cfg: Cfg,
}

#[serde_inline_default]
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct SchedulingCfg {
    #[serde_inline_default(0.333)]
    pub poll_interval: f64,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(default)]
pub struct GlobalConfig {
    pub min_polling_interval: f64,
    pub padding: i32,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            min_polling_interval: 0.25,
            padding: 1,
        }
    }
}

pub fn load_status_from_cfg() -> Result<EmptyStatus> {
    let xdg = BaseDirectories::with_prefix(CONFIG_PREFIX);
    let path: PathBuf = xdg.place_config_file(CONFIG_FILE)?;

    let text = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        let sample = sample_config();
        fs::write(&path, sample)?;
        sample.into()
    };

    let raw: RootConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

    let (click_tx, _) = tokio::sync::broadcast::channel::<crate::core::ClickEvent>(16);
    let mut machine_wrappers: Vec<MachineWrapper> = Vec::new();
    let effects = crate::machine::effects::EffectEngine::new();

    for (handle, raw_unit) in raw.units.iter().enumerate() {
        let uc = match decode_unit_config(raw_unit) {
            Ok(uc) => uc,
            Err(e) => {
                error!("Failed to parse unit {handle}: {e}");
                machine_wrappers.push(config_error_wrapper(handle, &e));
                continue;
            }
        };

        let kind = match &uc {
            UnitConfig::Weather(spec) => {
                let mach = std::sync::Arc::new(WeatherMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Weather"
            }
            UnitConfig::Time(spec) => {
                let mach = std::sync::Arc::new(TimeMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Time"
            }
            UnitConfig::Cpu(spec) => {
                let mach = std::sync::Arc::new(CpuMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Cpu"
            }
            UnitConfig::Mem(spec) => {
                let mach = std::sync::Arc::new(MemMachine::new(spec.cfg));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Mem"
            }
            UnitConfig::Disk(spec) => {
                let mach = std::sync::Arc::new(DiskMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Disk"
            }
            UnitConfig::Wifi(spec) => {
                let mach = std::sync::Arc::new(WifiMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Wifi"
            }
            UnitConfig::Bat(spec) => {
                let mach = std::sync::Arc::new(BatMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Bat"
            }
            UnitConfig::Net(spec) => {
                let mach = std::sync::Arc::new(NetMachine::new(spec.cfg.clone()));
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    spec.sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Net"
            }
            UnitConfig::Quota(spec) => {
                let mach = std::sync::Arc::new(QuotaMachine::new(spec.cfg.clone()));
                let sched = canonical_quota_sched(spec.sched);
                machine_wrappers.push(spawn_machine_actor(
                    mach,
                    effects.clone(),
                    sched,
                    raw.global,
                    handle,
                    &click_tx,
                ));
                "Quota"
            }
            UnitConfig::_External => {
                warn!("Skipping external unit type (not implemented yet)");
                "External"
            }
        };

        info!("Successfully loaded unit '{kind}'");
        debug!("Unit config: {uc:?}");
    }

    info!("Using global config: {:?}", raw.global);
    Ok(EmptyStatus::new(raw.global, machine_wrappers, click_tx))
}

fn canonical_quota_sched(sched: SchedulingCfg) -> SchedulingCfg {
    SchedulingCfg {
        poll_interval: crate::machine::units::quota::canonical_poll_interval(sched.poll_interval),
    }
}

fn decode_unit_config(raw: &toml::Value) -> Result<UnitConfig, toml::de::Error> {
    raw.clone().try_into()
}

fn config_error_wrapper(handle: usize, error: &toml::de::Error) -> MachineWrapper {
    let message = clipped_error_message(&error.to_string().replace('\n', " "));

    let body = Markup::text(format!("unit {handle} "))
        .append(Markup::text("bad config: ").fg(RED))
        .append(Markup::text(message).fg(RED));
    let (_view_tx, view_rx) = tokio::sync::watch::channel(View {
        body,
        health: Health::Error,
    });

    MachineWrapper {
        i3_name: format!("Config::{handle}"),
        handle,
        view_rx,
    }
}

fn clipped_error_message(message: &str) -> String {
    const MAX_MESSAGE_CHARS: usize = 160;
    let mut chars = message.chars();
    let clipped = chars.by_ref().take(MAX_MESSAGE_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

fn sample_config() -> &'static str {
    r#"# Global config.

[global]
min_polling_interval = 0.15
padding = 1

# Units appear on the bar in the same order as they are defined here.
# Topmost is rightmost.

[[units]]
type = "Time"
poll_interval = 1.0
format = "%a %b %d %Y - %H:%M"
"#
}

#[cfg(test)]
mod tests {
    use super::{RootConfig, decode_unit_config};

    #[test]
    fn malformed_unit_config_does_not_poison_root_decode() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Time"

[[units]]
type = "Disk"

[[units]]
type = "Mem"
"#;
        let decoded = toml::from_str::<RootConfig>(text);
        assert!(decoded.is_ok());
        let Ok(root) = decoded else {
            return;
        };
        assert_eq!(root.units.len(), 3);
        assert!(decode_unit_config(&root.units[0]).is_ok());
        assert!(decode_unit_config(&root.units[1]).is_err());
        assert!(decode_unit_config(&root.units[2]).is_ok());
    }
}
