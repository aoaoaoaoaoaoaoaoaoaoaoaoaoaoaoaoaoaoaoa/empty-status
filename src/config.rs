use anyhow::{Context, Result};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::{fs, path::PathBuf};
use toml::Value;
use tracing::{debug, error, info, warn};
use xdg::BaseDirectories;

use crate::core::EmptyStatus;
use crate::machine::runtime::{spawn_machine_actor, MachineWrapper};
use crate::machine::units::legacy::LegacyMachine;
use crate::machine::units::net::NetMachine;
use crate::machine::units::weather::WeatherMachine;
use crate::registry;

const CONFIG_PREFIX: &str = "empty-status";
const CONFIG_FILE: &str = "config.toml";

#[derive(Deserialize)]
struct RawRootConfig {
    #[serde(default)]
    units: Vec<RawUnitConfig>,
    #[serde(default)]
    global: GlobalConfig,
}

#[derive(Deserialize)]
struct RawUnitConfig {
    #[serde(rename = "type")]
    kind: String,
    // we do not include `SchedulingCfg` as a `flatten` field because some units
    // may want to see the scheduling keys, so we want to keep them in `rest`
    #[serde(flatten)]
    rest: Value,
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

    let raw: RawRootConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

    let (click_tx, _) = tokio::sync::broadcast::channel::<crate::core::ClickEvent>(16);
    let mut machine_wrappers: Vec<MachineWrapper> = Vec::new();

    for (handle, ruc) in raw.units.iter().enumerate() {
        let Some(factory) = registry::iter().find(|f| f.kind == ruc.kind) else {
            warn!("Skipping unknown unit type '{}'", ruc.kind);
            continue;
        };

        // Legacy config validation: ensure this unit can be constructed.
        // The actual instance is owned by the per-unit machine state.
        match (factory.build)(ruc.rest.clone()) {
            Ok(unit) => {
                info!("Successfully loaded unit '{}'", ruc.kind);
                debug!("Unit config: {unit:?}");
            }
            Result::Err(e) => {
                error!("Failed to load unit '{}': {}", ruc.kind, e);
                continue;
            }
        }

        let sched = match ruc.rest.clone().try_into::<SchedulingCfg>() {
            Ok(cfg) => cfg,
            Err(e) => {
                error!(
                    "Failed to parse scheduling config for unit '{}': {e}",
                    ruc.kind
                );
                continue;
            }
        };

        // Machines: Weather and Net are bespoke; everything else is wrapped as a polled unit.
        if ruc.kind == "Weather" {
            let cfg: crate::units::weather::WeatherConfig = ruc.rest.clone().try_into()?;
            let mach = std::sync::Arc::new(WeatherMachine::new(cfg));
            machine_wrappers.push(spawn_machine_actor(
                mach, sched, raw.global, handle, &click_tx,
            ));
            continue;
        }
        if ruc.kind == "Net" {
            let cfg: crate::units::net::NetConfig = ruc.rest.clone().try_into()?;
            let mach = std::sync::Arc::new(NetMachine::new(cfg));
            machine_wrappers.push(spawn_machine_actor(
                mach, sched, raw.global, handle, &click_tx,
            ));
            continue;
        }

        let legacy = std::sync::Arc::new(LegacyMachine::new(
            factory.kind,
            factory.build,
            ruc.rest.clone(),
        ));
        machine_wrappers.push(spawn_machine_actor(
            legacy, sched, raw.global, handle, &click_tx,
        ));
    }

    info!("Using global config: {:?}", raw.global);
    Ok(EmptyStatus::new(raw.global, machine_wrappers, click_tx))
}

fn sample_config() -> &'static str {
    r#"# each unit defines its own polling interval. to save resources you can define a global floor here
min_polling_interval = 0.15
padding = 1

# units will appear on the bar in the same order as they are defined here
# topmost is rightmost
[[units]]
type = "Time"
format = "%a %b %d %Y - %H:%M"
"#
}
