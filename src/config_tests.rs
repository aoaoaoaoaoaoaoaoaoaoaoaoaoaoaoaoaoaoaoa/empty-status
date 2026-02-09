#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use crate::config::{GlobalConfig, SchedulingCfg};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct RootConfigForTest {
        global: GlobalConfig,
        #[serde(default)]
        units: Vec<UnitConfigForTest>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "type")]
    #[allow(dead_code)]
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
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
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

        let cfg: RootConfigForTest = toml::from_str(text).unwrap();
        assert_eq!(cfg.units.len(), 2);
        assert!((cfg.global.min_polling_interval - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        let _: RootConfigForTest = toml::from_str(text).unwrap();
    }
}
