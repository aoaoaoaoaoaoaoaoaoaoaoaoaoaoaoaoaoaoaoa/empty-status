use anyhow::{Context, Result};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::{fs, path::PathBuf};
use tracing::{debug, error, info, warn};
use xdg::BaseDirectories;

use crate::core::EmptyStatus;
use crate::machine::runtime::{spawn_machine_actor, MachineWrapper};
use crate::machine::units::bat::BatMachine;
use crate::machine::units::cpu::CpuMachine;
use crate::machine::units::disk::DiskMachine;
use crate::machine::units::mem::MemMachine;
use crate::machine::units::net::NetMachine;
use crate::machine::units::time::TimeMachine;
use crate::machine::units::weather::WeatherMachine;
use crate::machine::units::wifi::WifiMachine;

const CONFIG_PREFIX: &str = "empty-status";
const CONFIG_FILE: &str = "config.toml";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct RootConfig {
    #[serde(default)]
    units: Vec<UnitConfig>,
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

    for (handle, uc) in raw.units.iter().enumerate() {
        let spawn_result: Result<&'static str> = match uc {
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
                Ok("Weather")
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
                Ok("Time")
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
                Ok("Cpu")
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
                Ok("Mem")
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
                Ok("Disk")
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
                Ok("Wifi")
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
                Ok("Bat")
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
                Ok("Net")
            }
            UnitConfig::_External => {
                warn!("Skipping external unit type (not implemented yet)");
                Ok("External")
            }
        };

        match spawn_result {
            Ok(kind) => {
                info!("Successfully loaded unit '{kind}'");
                debug!("Unit config: {uc:?}");
            }
            Err(e) => {
                error!("Failed to load unit: {e:#}");
            }
        }
    }

    info!("Using global config: {:?}", raw.global);
    Ok(EmptyStatus::new(raw.global, machine_wrappers, click_tx))
}

fn sample_config() -> &'static str {
    r#"# Global config.

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
