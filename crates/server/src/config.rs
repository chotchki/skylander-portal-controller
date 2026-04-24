//! Runtime configuration.
//!
//! - Dev builds (`dev-tools` feature, default): read `.env.dev` at startup.
//! - Release builds: read `%APPDATA%/skylander-portal-controller/config.json`,
//!   or kick off the first-launch egui wizard when that file is missing
//!   (see `crate::wizard`).

#[cfg(feature = "dev-tools")]
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "dev-tools")]
use anyhow::Context;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[allow(dead_code)] // used for game launching in Phase 3
    pub rpcs3_exe: PathBuf,
    pub firmware_pack_root: PathBuf,
    pub bind_port: u16,
    pub driver_kind: DriverKind,
    /// Directory where the log file(s) live. Differs dev vs release.
    pub log_dir: PathBuf,
    /// Directory containing the phone SPA's built assets.
    pub phone_dist_dir: PathBuf,
    /// Root of committed static data bundles: `images/<figure_id>/{hero,thumb}.png`,
    /// `figures.json`, `figures.manual.json`. Defaults to `<repo>/data/`.
    pub data_root: PathBuf,
    /// 32-byte HMAC-SHA256 key shared with the phone via the TV's QR fragment
    /// (`?k=<hex>` query param). Every mutating REST request carries an HMAC + timestamp
    /// header computed with this key (PLAN 3.13). Stable across restarts —
    /// regenerating invalidates any phone that still has the old QR cached.
    #[serde(with = "hex_key")]
    pub hmac_key: Vec<u8>,
}

/// Serde helper: persist `hmac_key` as a hex string in `config.json`.
mod hex_key {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Generate a fresh 32-byte HMAC key using the OS RNG. Called once at
/// first-launch (dev or release) if the persisted config doesn't have one.
pub fn generate_hmac_key() -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut key = vec![0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Mock only exists under `dev-tools`; kept for config-file round-trip
pub enum DriverKind {
    Uia,
    Mock,
}

#[cfg(feature = "dev-tools")]
pub fn load() -> Result<Config> {
    let env = read_env_file(".env.dev").unwrap_or_default();

    let rpcs3_exe = require_path(&env, "RPCS3_EXE")?;
    // `FIRMWARE_PACK_ROOT` is now optional (PLAN 6.5.4): reader-only users
    // don't need a pack, and a zero-collection boot is valid (Imaginators
    // "instant Skylander" flow, for one). Empty / unset collapses to
    // PathBuf::new(); `skylander_indexer::scan()` already returns Ok(vec![])
    // for a missing root, so main.rs's boot path is safe without changes.
    let firmware_pack_root = env
        .get("FIRMWARE_PACK_ROOT")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_default();

    let bind_port: u16 = env
        .get("BIND_PORT")
        .map(|s| s.parse())
        .transpose()
        .context("BIND_PORT must be an integer")?
        .unwrap_or(8765);

    let driver_kind = match env.get("SKYLANDER_PORTAL_DRIVER").map(String::as_str) {
        Some("mock") => DriverKind::Mock,
        _ => DriverKind::Uia,
    };

    let log_dir = PathBuf::from("logs");
    let phone_dist_dir = env
        .get("PHONE_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("phone/dist"));
    let data_root = env
        .get("DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));

    // HMAC key lives in `./dev-data/hmac.key` so it survives `cargo clean`
    // but regenerates on `rm -rf dev-data/`. Dev mode doesn't push through
    // the full config.json round-trip because `.env.dev` is the source of
    // truth; the key is the one piece of runtime state that can't live in
    // .env.dev without committing secrets.
    let hmac_key = load_or_create_dev_hmac_key()?;

    Ok(Config {
        rpcs3_exe,
        firmware_pack_root,
        bind_port,
        driver_kind,
        log_dir,
        phone_dist_dir,
        data_root,
        hmac_key,
    })
}

#[cfg(feature = "dev-tools")]
fn load_or_create_dev_hmac_key() -> Result<Vec<u8>> {
    let path = PathBuf::from("dev-data").join("hmac.key");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let decoded = hex::decode(raw.trim())
            .with_context(|| format!("parse hex from {}", path.display()))?;
        if decoded.len() == 32 {
            return Ok(decoded);
        }
        // Wrong length; regenerate.
    }
    let key = generate_hmac_key();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, hex::encode(&key))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(key)
}

#[cfg(not(feature = "dev-tools"))]
pub fn load() -> Result<Config> {
    use anyhow::Context;
    use crate::paths;
    use crate::wizard::{self, PersistedConfig, PersistedDriverKind};

    let config_path = paths::config_json_path()?;

    let persisted = if config_path.exists() {
        PersistedConfig::read(&config_path).with_context(|| {
            format!(
                "parse {} — delete it to re-run the first-launch wizard",
                config_path.display()
            )
        })?
    } else {
        let runtime_dir = paths::resolve_runtime_dir()?;
        wizard::run_wizard_blocking(&config_path, &runtime_dir)?
    };

    // Ensure the persisted config has an HMAC key. The wizard writes a
    // fresh one on first launch; older configs (pre-3.13) won't have the
    // field. `PersistedConfig` keeps `hmac_key` as `Option<Vec<u8>>` with a
    // `#[serde(default)]`, so the `None` case here means a config from a
    // server version before this feature existed — regenerate + persist.
    let hmac_key: Vec<u8> = match persisted.hmac_key {
        Some(k) if k.len() == 32 => k,
        _ => {
            let k = generate_hmac_key();
            let mut updated = persisted.clone();
            updated.hmac_key = Some(k.clone());
            updated.write(&config_path)?;
            k
        }
    };

    Ok(Config {
        rpcs3_exe: persisted.rpcs3_exe,
        firmware_pack_root: persisted.firmware_pack_root,
        bind_port: persisted.bind_port,
        driver_kind: match persisted.driver_kind {
            PersistedDriverKind::Uia => DriverKind::Uia,
            PersistedDriverKind::Mock => DriverKind::Mock,
        },
        log_dir: persisted.log_dir,
        phone_dist_dir: persisted.phone_dist_dir,
        data_root: persisted.data_root,
        hmac_key,
    })
}

#[cfg(feature = "dev-tools")]
fn require_path(env: &HashMap<String, String>, key: &str) -> Result<PathBuf> {
    env.get(key)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing {key} in .env.dev"))
}

#[cfg(feature = "dev-tools")]
fn read_env_file(path: &str) -> Result<HashMap<String, String>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let mut out = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(out)
}
