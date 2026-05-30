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
#[cfg(not(feature = "dev-tools"))]
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[allow(dead_code)] // used for game launching in Phase 3
    pub rpcs3_exe: PathBuf,
    /// RPCS3 data/config root (holds `config/games.yml`, firmware, dev_hdd0).
    /// Defaults to `rpcs3_exe.parent()`; set independently via `RPCS3_CONFIG_DIR`
    /// when the patched binary lives apart from its config (Phase 16). Used to read
    /// `games.yml` and passed to the patched RPCS3 as `RPCS3_CONFIG_DIR`.
    pub config_dir: PathBuf,
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
    /// Legacy GUI-automation driver (UI Automation against the Skylanders Manager
    /// dialog). Demoted to a fallback in Phase 16 (16.5.2.3) — opt-in via
    /// `SKYLANDER_PORTAL_DRIVER=uia`, kept for dev against a *stock* RPCS3.
    Uia,
    /// Patched-RPCS3 IPC driver (Phase 16) — cross-platform AF_UNIX portal control,
    /// no GUI/dialog. **The production default on Windows** (16.5.2.3); the no-GUI
    /// launch + window coordination (PLAN 16.6) is proven on the HTPC.
    Ipc,
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
        Some("ipc") => DriverKind::Ipc,
        Some("uia") => DriverKind::Uia,
        // Phase 16 (16.5.2.3): IPC (patched RPCS3 over AF_UNIX) is the production
        // default on Windows; macOS has no real driver and falls back to the
        // in-memory mock. The explicit `uia` arm keeps the legacy GUI-automation
        // path available for dev against a *stock* RPCS3 (`.env.dev` sets it).
        _ => {
            #[cfg(windows)]
            {
                DriverKind::Ipc
            }
            #[cfg(not(windows))]
            {
                DriverKind::Mock
            }
        }
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

    // Phase 16: the patched RPCS3 can live apart from its data/config root, so
    // RPCS3_CONFIG_DIR is independent. Default to the exe's parent (a normal
    // install keeps `config/` next to the exe).
    let config_dir = env
        .get("RPCS3_CONFIG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            rpcs3_exe
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });

    Ok(Config {
        rpcs3_exe,
        config_dir,
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
    use crate::paths;
    use crate::wizard::{PersistedConfig, PersistedDriverKind};
    use anyhow::Context;
    // The wizard module itself is only referenced on non-macOS (PLAN
    // 10.6.4 short-circuits the Mac path with a default config).
    #[cfg(not(target_os = "macos"))]
    use crate::wizard;

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
        // PLAN 10.6.4: macOS production builds skip the wizard
        // entirely. The wizard is RPCS3-shaped (validates `rpcs3.exe`
        // filename, expects a firmware-pack root), and macOS has no
        // AXUIElement-based driver — the only available DriverKind on
        // Mac is Mock. Write a sensible default + move on. User can
        // hand-edit the resulting `config.json` if they want a
        // different bind port, custom data path, etc.
        #[cfg(target_os = "macos")]
        {
            let cfg = PersistedConfig::macos_default(&runtime_dir);
            cfg.write(&config_path)?;
            cfg
        }
        #[cfg(not(target_os = "macos"))]
        {
            wizard::run_wizard_blocking(&config_path, &runtime_dir)?
        }
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

    // PLAN 10.8.4: data_root + phone_dist_dir are derived from the
    // binary location, not user preferences. Always recompute from
    // `current_exe()` on launch so an MSI upgrade or relocation
    // (e.g. "C:\emuluators\portal_controller\" → "C:\Program Files\
    // Skylander Portal Controller\") doesn't leave the persisted
    // config pointing at the old install dir. The persisted values
    // in config.json are kept (write_through) so external tools can
    // still inspect, but the runtime resolution always wins.
    let exe_parent = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let phone_dist_dir = exe_parent.join("phone-dist");
    let data_root = exe_parent.join("data");
    if persisted.phone_dist_dir != phone_dist_dir || persisted.data_root != data_root {
        info!(
            old_data_root = %persisted.data_root.display(),
            new_data_root = %data_root.display(),
            "overriding stale data_root/phone_dist_dir from config.json with current_exe-derived paths",
        );
    }

    // Phase 16 bundled-RPCS3 model: on Windows the portal *control* binary is
    // ALWAYS the patched RPCS3 we ship next to the app at `<app>/rpcs3/rpcs3.exe`,
    // driven over IPC. Neither is a user preference — both are determined by the
    // install layout — so, exactly like data_root/phone_dist_dir above (PLAN
    // 10.8.4), recompute them from `current_exe` every launch instead of trusting
    // config.json. This transparently repairs configs written by pre-16.5.2.3 /
    // pre-16.9 versions, which persisted `driver_kind: uia` pointed at the user's
    // *stock* RPCS3: stock RPCS3 has no IPC listener, so after an upgrade-in-place
    // (the wizard is skipped when config.json already exists) those boxes silently
    // run the legacy UIA path and figures never load onto the portal. The user's
    // stock install is still used — as `config_dir` (games.yml + firmware). The
    // per-profile working copies that hold figure progress live under the runtime
    // dir keyed by profile+figure id, so nothing here rewrites a `.sky`.
    #[cfg(not(target_os = "macos"))]
    let (rpcs3_exe, driver_kind, config_dir) = {
        let (rpcs3_exe, driver_kind, config_dir) =
            migrate_install_paths(&persisted.rpcs3_exe, &persisted.config_dir, &exe_parent);
        if persisted.rpcs3_exe != rpcs3_exe
            || !matches!(persisted.driver_kind, PersistedDriverKind::Ipc)
        {
            info!(
                old_rpcs3_exe = %persisted.rpcs3_exe.display(),
                new_rpcs3_exe = %rpcs3_exe.display(),
                old_driver = ?persisted.driver_kind,
                "config predates the bundled-RPCS3/IPC model — driving the bundled patched RPCS3 over IPC (working copies untouched)",
            );
        }
        (rpcs3_exe, driver_kind, config_dir)
    };
    // macOS has no patched RPCS3 — keep the persisted (Mock) driver + paths as-is.
    #[cfg(target_os = "macos")]
    let (rpcs3_exe, driver_kind, config_dir) = (
        persisted.rpcs3_exe.clone(),
        match persisted.driver_kind {
            PersistedDriverKind::Uia => DriverKind::Uia,
            PersistedDriverKind::Ipc => DriverKind::Ipc,
            PersistedDriverKind::Mock => DriverKind::Mock,
        },
        if persisted.config_dir.as_os_str().is_empty() {
            persisted
                .rpcs3_exe
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            persisted.config_dir.clone()
        },
    );

    Ok(Config {
        rpcs3_exe,
        config_dir,
        firmware_pack_root: persisted.firmware_pack_root,
        bind_port: persisted.bind_port,
        driver_kind,
        log_dir: persisted.log_dir,
        phone_dist_dir,
        data_root,
        hmac_key,
    })
}

/// (Windows release) Resolve the install-layout-derived RPCS3 fields from the
/// persisted `rpcs3_exe` / `config_dir` plus the running binary's parent dir.
/// Pure; touches no files.
///
/// Under the Phase-16 bundled-RPCS3 model the control binary is ALWAYS the
/// patched RPCS3 shipped at `<exe_parent>/rpcs3/rpcs3.exe`, and the only driver
/// is IPC — both install-layout-derived, not user prefs — so callers recompute
/// them each launch (mirroring data_root/phone_dist_dir, PLAN 10.8.4). This
/// repairs pre-IPC configs that pinned `driver_kind: uia` to the user's *stock*
/// RPCS3 (no IPC listener → figures never load onto the portal after upgrade).
/// The returned `config_dir` (the user's install: games.yml + firmware) is the
/// persisted `config_dir`, falling back to the persisted stock `rpcs3_exe`'s
/// parent for pre-16.9 configs that have no `config_dir`.
///
/// Returns `(rpcs3_exe, driver_kind, config_dir)`. It deliberately ignores the
/// persisted `rpcs3_exe`/`driver_kind` for the first two outputs; figure progress
/// lives in per-profile working copies under the runtime dir, untouched by this.
#[allow(dead_code)] // sole call site is release + non-macOS (`config::load`)
fn migrate_install_paths(
    persisted_rpcs3_exe: &std::path::Path,
    persisted_config_dir: &std::path::Path,
    exe_parent: &std::path::Path,
) -> (PathBuf, DriverKind, PathBuf) {
    let rpcs3_exe = exe_parent.join("rpcs3").join("rpcs3.exe");
    let config_dir = if persisted_config_dir.as_os_str().is_empty() {
        persisted_rpcs3_exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        persisted_config_dir.to_path_buf()
    };
    (rpcs3_exe, DriverKind::Ipc, config_dir)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // The reported bug: a box with an existing (pre-IPC) install. An older app
    // version's wizard persisted the legacy UIA driver pointed at the user's
    // *stock* RPCS3 and no `config_dir`; on upgrade the first-launch wizard is
    // skipped (config.json exists), so without migration the app keeps driving
    // stock RPCS3 over UIA — which has no IPC listener, so figures never load.
    // After migration the control binary is the bundled patched RPCS3 over IPC,
    // and `config_dir` falls back to the stock exe's parent (the user's install:
    // games.yml + firmware).
    #[test]
    fn migrates_pre_ipc_uia_config_to_bundled_ipc() {
        let install = Path::new("stock-install");
        let stock_exe = install.join("rpcs3.exe");
        let exe_parent = Path::new("app-dir");

        // Empty persisted config_dir = a pre-16.9 config.
        let (rpcs3_exe, driver, config_dir) =
            migrate_install_paths(&stock_exe, Path::new(""), exe_parent);

        assert_eq!(
            rpcs3_exe,
            exe_parent.join("rpcs3").join("rpcs3.exe"),
            "control binary must be the bundled patched RPCS3, not the persisted stock path",
        );
        assert_eq!(
            driver,
            DriverKind::Ipc,
            "must drive IPC, not the legacy UIA fallback",
        );
        assert_eq!(
            config_dir, install,
            "config_dir must fall back to the user's stock install (games.yml + firmware)",
        );
    }

    // A current (post-16.9 wizard) config: `config_dir` is set explicitly and is
    // preserved; `rpcs3_exe` is still recomputed to the bundled binary so an MSI
    // relocation of the install is picked up rather than left stale.
    #[test]
    fn keeps_explicit_config_dir_and_rebinds_rpcs3_exe() {
        let exe_parent = Path::new("app-dir");
        let users_install = Path::new("users-rpcs3-install");
        let persisted_exe = exe_parent.join("rpcs3").join("rpcs3.exe");

        let (rpcs3_exe, driver, config_dir) =
            migrate_install_paths(&persisted_exe, users_install, exe_parent);

        assert_eq!(rpcs3_exe, exe_parent.join("rpcs3").join("rpcs3.exe"));
        assert_eq!(driver, DriverKind::Ipc);
        assert_eq!(config_dir, users_install);
    }
}
