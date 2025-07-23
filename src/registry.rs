use crate::core::Unit;
use anyhow::Result;
use toml::Value;

pub struct UnitFactory {
    pub kind: &'static str,
    pub build: fn(Value) -> Result<Box<dyn Unit>>,
}

pub fn iter() -> impl Iterator<Item = &'static UnitFactory> {
    inventory::iter::<UnitFactory>()
}

// in src/unit_factory.rs
#[macro_export]
macro_rules! register_unit {
    // $ty: the full path to your Unit type
    // $cfg: the full path to your Config type
    ($ty:path, $cfg:path) => {
        use $crate::registry::UnitFactory;
        inventory::submit! {
            UnitFactory {
                kind: stringify!($ty),
                build: |val: toml::Value| -> Result<Box<dyn $crate::core::Unit>> {
                    let cfg: $cfg = val.clone().try_into()?;
                    Ok(Box::new(<$ty>::from_cfg(cfg)))
                }
            }
        }
    };
}
