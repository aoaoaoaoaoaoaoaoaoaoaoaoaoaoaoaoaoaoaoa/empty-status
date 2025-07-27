use anyhow::{Context, Result};
use serde::Deserialize;
use serde_inline_default::serde_inline_default;
use std::{fs, path::PathBuf};
use toml::Value;
use tracing::{debug, error, info, warn};
use xdg::BaseDirectories;

use crate::core::{EmptyStatus, UnitWrapper};
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
#[derive(Deserialize, Debug, Clone)]
pub struct SchedulingCfg {
    #[serde_inline_default(0.333)]
    pub poll_interval: f64,
}

#[derive(Deserialize, Debug, Clone)]
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

    let mut wrappers = Vec::with_capacity(raw.units.len());

    for (handle, ruc) in raw.units.iter().enumerate() {
        let factory = match registry::iter().find(|f| f.kind == ruc.kind) {
            Some(factory) => factory,
            None => {
                warn!("Skipping unknown unit type '{}'", ruc.kind);
                continue;
            }
        };

        let unit_obj = match (factory.build)(ruc.rest.clone()) {
            Ok(unit) => {
                info!("Successfully loaded unit '{}'", ruc.kind);
                debug!("Unit config: {unit:?}");
                unit
            }
            Result::Err(e) => {
                error!("Failed to load unit '{}': {}", ruc.kind, e);
                continue;
            }
        };

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

        wrappers.push(UnitWrapper {
            unit: unit_obj,
            cfg: sched,
            handle,
        });
    }

    wrappers.reverse();
    info!("Using global config: {:?}", raw.global);
    EmptyStatus::new(wrappers, raw.global)
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
