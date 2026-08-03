use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use xdg::BaseDirectories;

use crate::units::Posture;

const PREFIX: &str = "empty-status";
const FILE: &str = "posture.json";
const VERSION: u8 = 1;
const MAX_BYTES: u64 = 64 * 1024;
const FNV_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const FNV_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIdentity(u128);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    postures: BTreeMap<String, Posture>,
}

#[derive(Debug)]
pub struct Store {
    path: Option<PathBuf>,
    postures: BTreeMap<String, Posture>,
}

impl SlotIdentity {
    pub fn from_config(raw: &toml::Value) -> Self {
        let mut identity = raw.clone();
        if let Some(table) = identity.as_table_mut() {
            let _ = table.remove("poll_interval");
        }
        let mut hash = FNV_OFFSET;
        for byte in identity.to_string().bytes() {
            hash = (hash ^ u128::from(byte)).wrapping_mul(FNV_PRIME);
        }
        Self(hash)
    }

    pub fn key(self, occurrence: usize) -> String {
        format!("{:032x}:{occurrence}", self.0)
    }
}

impl Store {
    pub fn load() -> Self {
        let path = BaseDirectories::with_prefix(PREFIX).get_state_file(FILE);
        if path.is_none() {
            tracing::warn!("state directory unavailable; using default posture");
        }
        Self::from_path(path)
    }

    pub fn get(&self, key: &str) -> Option<&Posture> {
        self.postures.get(key)
    }

    pub fn reconcile(&mut self, postures: BTreeMap<String, Posture>) {
        self.postures = postures;
    }

    pub fn record(&mut self, key: String, posture: Posture) {
        let _ = self.postures.insert(key, posture);
        let Some(path) = self.path.as_deref() else {
            return;
        };
        if let Err(error) = persist(path, &self.postures) {
            tracing::warn!(%error, path = %path.display(), "cannot persist posture; continuing");
        }
    }

    fn from_path(path: Option<PathBuf>) -> Self {
        let postures = path.as_deref().map_or_else(BTreeMap::new, load);
        Self { path, postures }
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self::from_path(Some(path))
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self::from_path(None)
    }
}

fn load(path: &Path) -> BTreeMap<String, Posture> {
    let bytes = match read_bounded(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "cannot read posture; using defaults");
            return BTreeMap::new();
        }
    };
    let ledger = match serde_json::from_slice::<Ledger>(&bytes) {
        Ok(ledger) => ledger,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "invalid posture; using defaults");
            return BTreeMap::new();
        }
    };
    if ledger.version == VERSION {
        ledger.postures
    } else {
        tracing::warn!(
            version = ledger.version,
            path = %path.display(),
            "unknown posture version; using defaults"
        );
        BTreeMap::new()
    }
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let _ = File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "posture file exceeds 64 KiB",
        ))
    } else {
        Ok(bytes)
    }
}

fn persist(path: &Path, postures: &BTreeMap<String, Posture>) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "posture path has no parent"))?;
    fs::create_dir_all(parent)?;
    let payload = serde_json::to_vec(&Ledger {
        version: VERSION,
        postures: postures.clone(),
    })
    .map_err(io::Error::other)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("posture.json");
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&payload)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{SlotIdentity, Store};
    use crate::units::Posture;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn path(name: &str) -> PathBuf {
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "empty-status-state-{}-{nonce}/{name}",
            std::process::id()
        ))
    }

    #[test]
    fn malformed_state_is_just_empty_state() {
        let path = path("posture.json");
        let Some(parent) = path.parent() else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(&path, b"rot").is_ok());
        let store = Store::at(path.clone());
        assert!(store.get("anything").is_none());
        assert!(fs::remove_dir_all(parent).is_ok());
    }

    #[test]
    fn missing_and_unknown_state_versions_are_just_empty_state() {
        let path = path("posture.json");
        assert!(Store::at(path.clone()).get("anything").is_none());
        let Some(parent) = path.parent() else { return };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(&path, br#"{"version":2,"postures":{"time":["Uptime"]}}"#).is_ok());
        assert!(Store::at(path.clone()).get("time").is_none());
        assert!(fs::remove_dir_all(parent).is_ok());
    }

    #[test]
    fn atomic_round_trip_and_failed_write_both_preserve_process_state() {
        let ledger_path = path("posture.json");
        let posture = serde_json::from_str::<Posture>(r#"["Uptime"]"#);
        assert!(posture.is_ok());
        let Ok(posture) = posture else { return };
        let mut store = Store::at(ledger_path.clone());
        store.record("time".to_owned(), posture.clone());
        assert_eq!(Store::at(ledger_path.clone()).get("time"), Some(&posture));
        let Some(parent) = ledger_path.parent() else {
            return;
        };
        assert!(fs::remove_dir_all(parent).is_ok());

        let obstruction = path("obstruction");
        let Some(parent) = obstruction.parent() else {
            return;
        };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(&obstruction, b"not a directory").is_ok());
        let mut store = Store::at(obstruction.join("posture.json"));
        store.record("time".to_owned(), posture.clone());
        assert_eq!(store.get("time"), Some(&posture));
        assert!(fs::remove_dir_all(parent).is_ok());
    }

    #[test]
    fn slot_identity_ignores_cadence_and_table_order() {
        let a = toml::from_str::<toml::Value>(
            r#"type = "Net"
               interface = "e0"
               poll_interval = 1"#,
        );
        let b = toml::from_str::<toml::Value>(
            r#"poll_interval = 9
               interface = "e0"
               type = "Net""#,
        );
        assert!(a.is_ok());
        assert!(b.is_ok());
        let (Ok(a), Ok(b)) = (a, b) else { return };
        assert_eq!(SlotIdentity::from_config(&a), SlotIdentity::from_config(&b));
    }
}
