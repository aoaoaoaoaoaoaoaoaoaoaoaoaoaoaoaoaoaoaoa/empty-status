#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use crate::config::{GlobalConfig, SchedulingCfg};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "deserialization-only schema mirror in tests")]
    struct RootConfigForTest {
        global: GlobalConfig,
        #[serde(default)]
        units: Vec<UnitConfigForTest>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "type")]
    #[allow(dead_code, reason = "deserialization-only schema mirror in tests")]
    enum UnitConfigForTest {
        #[serde(rename = "Weather")]
        Weather(UnitSpecForTest<crate::units::weather::WeatherConfig>),
        #[serde(rename = "Time")]
        Time(UnitSpecForTest<crate::units::time::TimeConfig>),
        #[serde(rename = "Cpu")]
        Cpu(UnitSpecForTest<crate::units::cpu::CpuConfig>),
        #[serde(rename = "Mem")]
        Mem(UnitSpecForTest<crate::units::mem::MemConfig>),
        #[serde(rename = "Disk")]
        Disk(UnitSpecForTest<crate::units::disk::DiskConfig>),
        #[serde(rename = "Wifi")]
        Wifi(UnitSpecForTest<crate::units::wifi::WifiConfig>),
        #[serde(rename = "Bat")]
        Bat(UnitSpecForTest<crate::units::bat::BatConfig>),
        #[serde(rename = "Net")]
        Net(UnitSpecForTest<crate::units::net::NetConfig>),
        #[serde(rename = "Quota")]
        Quota(UnitSpecForTest<crate::units::quota::QuotaConfig>),
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "deserialization-only schema mirror in tests")]
    struct UnitSpecForTest<Cfg> {
        #[serde(flatten)]
        sched: SchedulingCfg,
        #[serde(flatten)]
        cfg: Cfg,
    }

    #[test]
    fn config_deserializes_minimal() {
        let text = r#"
[global]
min_polling_interval = 0.15
padding = 1

[[units]]
type = "Time"
poll_interval = 1.0
format = "%H:%M"

[[units]]
type = "Mem"
poll_interval = 0.5
"#;

        let cfg = toml::from_str::<RootConfigForTest>(text).ok();
        assert!(cfg.is_some(), "minimal config failed to parse");

        if let Some(cfg) = cfg {
            assert_eq!(cfg.units.len(), 2);
            assert!((cfg.global.min_polling_interval - 0.15).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        assert!(toml::from_str::<RootConfigForTest>(text).is_ok());
    }

    #[test]
    fn disk_unit_allows_partlabel_only() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Disk"
poll_interval = 0.333
partlabel = "ROOT"
"#;
        assert!(toml::from_str::<RootConfigForTest>(text).is_ok());
    }

    #[test]
    fn disk_unit_rejects_multiple_selectors() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Disk"
poll_interval = 0.333
disk = "nvme0n1p1"
partlabel = "ROOT"
"#;
        assert!(toml::from_str::<RootConfigForTest>(text).is_err());
    }

    #[test]
    fn bat_id_defaults_to_zero() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Bat"
"#;
        let parsed = toml::from_str::<RootConfigForTest>(text);
        assert!(parsed.is_ok());
        let Ok(cfg) = parsed else {
            return;
        };
        assert!(matches!(cfg.units.first(), Some(UnitConfigForTest::Bat(_))));
        if let Some(UnitConfigForTest::Bat(spec)) = cfg.units.first() {
            assert_eq!(spec.cfg.bat_id, 0);
        }
    }

    #[test]
    fn quota_providers_are_selectable() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Quota"
providers = ["codex"]
"#;
        assert!(toml::from_str::<RootConfigForTest>(text).is_ok());
    }

    #[test]
    fn quota_rejects_empty_provider_set() {
        let text = r#"
[global]
min_polling_interval = 0.25
padding = 1

[[units]]
type = "Quota"
providers = []
"#;
        assert!(toml::from_str::<RootConfigForTest>(text).is_err());
    }
}
