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

#[macro_export]
macro_rules! register_unit {
    ($ty:ident, $cfg:ident) => {
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
