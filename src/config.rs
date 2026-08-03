use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, de::DeserializeOwned};
use serde_inline_default::serde_inline_default;
use tracing::{error, info};
use xdg::BaseDirectories;

use crate::{
    probe_io::ProbeIo,
    reactor::{Reactor, Slot},
    state::{SlotIdentity, Store},
    units::{self, Unit},
};

const CONFIG_PREFIX: &str = "empty-status";
const CONFIG_FILE: &str = "config.toml";
const DEFAULT_FAST_POLL_INTERVAL: f64 = 0.333;
const DEFAULT_SLOW_POLL_INTERVAL: f64 = 300.0;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootConfig {
    #[serde(default)]
    global: GlobalConfig,
    #[serde(default)]
    units: Vec<toml::Value>,
}

#[serde_inline_default]
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalConfig {
    #[serde_inline_default(1)]
    padding: u8,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self { padding: 1 }
    }
}

pub fn load() -> Result<Reactor> {
    let path = config_path()?;
    let text = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        fs::write(&path, sample_config())?;
        sample_config().to_owned()
    };
    let root: RootConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut slots = Vec::with_capacity(root.units.len());
    let mut occurrences = HashMap::new();
    for (index, raw) in root.units.into_iter().enumerate() {
        let identity = SlotIdentity::from_config(&raw);
        let occurrence = occurrences.entry(identity).or_insert(0);
        let state_key = identity.key(*occurrence);
        *occurrence += 1;
        match decode_unit(raw) {
            Ok((unit, cadence)) => {
                info!(index, kind = unit.name(), ?cadence, "loaded unit");
                slots.push(Slot::live(index, state_key, unit, cadence));
            }
            Err(fault) => {
                error!(index, error = %fault, "unit configuration rejected");
                slots.push(Slot::broken(index, clipped(&fault.to_string())));
            }
        }
    }
    Ok(Reactor::new(
        slots,
        root.global.padding,
        ProbeIo::new()?,
        Store::load(),
    ))
}

fn config_path() -> Result<PathBuf> {
    BaseDirectories::with_prefix(CONFIG_PREFIX)
        .place_config_file(CONFIG_FILE)
        .map_err(Into::into)
}

fn decode_unit(raw: toml::Value) -> Result<(Unit, Duration)> {
    let mut table = raw
        .as_table()
        .cloned()
        .ok_or_else(|| anyhow!("unit must be a TOML table"))?;
    let kind = table
        .remove("type")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| anyhow!("unit requires a string `type`"))?;
    let poll_interval = table
        .remove("poll_interval")
        .map_or(Ok(default_poll_interval(&kind)), number)?;
    let config = toml::Value::Table(table);
    let unit = match kind.as_str() {
        "Bat" => Unit::Bat(
            units::bat::Model::new(decode(config)?).map_err(|error| anyhow!("Bat: {error}"))?,
        ),
        "Cpu" => Unit::Cpu(units::cpu::Model::new(decode(config)?)),
        "Disk" => Unit::Disk(
            units::disk::Model::new(decode(config)?).map_err(|error| anyhow!("Disk: {error}"))?,
        ),
        "Mem" => Unit::Mem(units::mem::Model::new(decode(config)?)),
        "Net" => Unit::Net(
            units::net::Model::new(decode(config)?).map_err(|error| anyhow!("Net: {error}"))?,
        ),
        "Quota" => Unit::Quota(
            units::quota::Model::new(decode(config)?).map_err(|error| anyhow!("Quota: {error}"))?,
        ),
        "Time" => Unit::Time(
            units::time::Model::new(decode(config)?).map_err(|error| anyhow!("Time: {error}"))?,
        ),
        "Weather" => Unit::Weather(
            units::weather::Model::new(decode(config)?)
                .map_err(|error| anyhow!("Weather: {error}"))?,
        ),
        "Wifi" => Unit::Wifi(
            units::wifi::Model::new(decode(config)?).map_err(|error| anyhow!("Wifi: {error}"))?,
        ),
        _ => return Err(anyhow!("unknown unit type `{kind}`")),
    };
    let requested =
        units::positive_duration("poll_interval", poll_interval).map_err(anyhow::Error::msg)?;
    let cadence = unit.canonical_cadence(requested);
    Ok((unit, cadence))
}

fn default_poll_interval(kind: &str) -> f64 {
    match kind {
        "Quota" | "Weather" => DEFAULT_SLOW_POLL_INTERVAL,
        _ => DEFAULT_FAST_POLL_INTERVAL,
    }
}

fn decode<C: DeserializeOwned>(value: toml::Value) -> Result<C> {
    value.try_into().map_err(Into::into)
}

fn number(value: toml::Value) -> Result<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|value| value as f64))
        .ok_or_else(|| anyhow!("poll_interval must be a number"))
}

fn clipped(message: &str) -> String {
    const LIMIT: usize = 160;
    let mut characters = message.chars();
    let prefix = characters.by_ref().take(LIMIT).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn sample_config() -> &'static str {
    r#"# Units are declared from rightmost to leftmost.

[global]
padding = 1

[[units]]
type = "Time"
poll_interval = 1.0
format = "%a %b %d %Y - %H:%M"
"#
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RootConfig, decode_unit};

    #[test]
    fn malformed_unit_does_not_poison_root() {
        let root = toml::from_str::<RootConfig>(
            r#"
                [[units]]
                type = "Time"

                [[units]]
                type = "Disk"

                [[units]]
                type = "Mem"
            "#,
        );
        assert!(root.is_ok());
        let Ok(root) = root else { return };
        assert!(decode_unit(root.units[0].clone()).is_ok());
        assert!(decode_unit(root.units[1].clone()).is_err());
        assert!(decode_unit(root.units[2].clone()).is_ok());
    }

    #[test]
    fn example_is_normative() {
        let root = toml::from_str::<RootConfig>(include_str!("../config.example.toml"));
        assert!(root.is_ok());
        let Ok(root) = root else { return };
        for unit in root.units {
            assert!(decode_unit(unit).is_ok());
        }
    }

    #[test]
    fn expensive_units_have_cadence_floors() {
        let weather = toml::from_str::<toml::Value>(
            r#"type = "Weather"
               poll_interval = 1
               lat = 0
               lon = 0"#,
        );
        let quota = toml::from_str::<toml::Value>(
            r#"type = "Quota"
               poll_interval = 1"#,
        );
        assert!(weather.is_ok());
        assert!(quota.is_ok());
        let (Ok(weather), Ok(quota)) = (weather, quota) else {
            return;
        };
        let weather = decode_unit(weather).map(|(_, cadence)| cadence);
        let quota = decode_unit(quota).map(|(_, cadence)| cadence);
        assert_eq!(weather.ok(), Some(Duration::from_mins(2)));
        assert_eq!(quota.ok(), Some(Duration::from_secs(15)));
    }

    #[test]
    fn absurd_cadence_becomes_a_broken_slot_instead_of_panicking() {
        let unit = toml::from_str::<toml::Value>(
            r#"type = "Time"
               poll_interval = 1e300"#,
        );
        assert!(unit.is_ok());
        let Ok(unit) = unit else { return };
        assert!(decode_unit(unit).is_err());
    }
}
