//! Runtime configuration. Phase 2 MVP reads `.env.dev` when the `dev-tools`
//! feature is active; a proper first-launch wizard arrives in Phase 3.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    #[allow(dead_code)] // used for game launching in Phase 3
    pub rpcs3_exe: PathBuf,
    pub firmware_pack_root: PathBuf,
    #[allow(dead_code)] // used for game catalogue in Phase 3
    pub games_yaml: PathBuf,
    pub bind_port: u16,
    pub driver_kind: DriverKind,
    /// Directory where the log file(s) live. Differs dev vs release.
    pub log_dir: PathBuf,
    /// Directory containing the phone SPA's built assets.
    pub phone_dist_dir: PathBuf,
    /// Root of committed static data bundles: `images/<figure_id>/{hero,thumb}.png`,
    /// `figures.json`, `figures.manual.json`. Defaults to `<repo>/data/`.
    pub data_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverKind {
    Uia,
    Mock,
}

#[cfg(feature = "dev-tools")]
pub fn load() -> Result<Config> {
    let env = read_env_file(".env.dev").unwrap_or_default();

    let rpcs3_exe = require_path(&env, "RPCS3_EXE")?;
    let firmware_pack_root = require_path(&env, "FIRMWARE_PACK_ROOT")?;

    let games_yaml = env
        .get("GAMES_YAML")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            rpcs3_exe
                .parent()
                .map(|p| p.join("config").join("games.yml"))
                .unwrap_or_else(|| PathBuf::from("games.yml"))
        });

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

    Ok(Config {
        rpcs3_exe,
        firmware_pack_root,
        games_yaml,
        bind_port,
        driver_kind,
        log_dir,
        phone_dist_dir,
        data_root,
    })
}

#[cfg(not(feature = "dev-tools"))]
pub fn load() -> Result<Config> {
    anyhow::bail!(
        "release-build first-launch config wizard is a Phase 3 feature; \
         run with --features dev-tools for now"
    )
}

fn require_path(env: &HashMap<String, String>, key: &str) -> Result<PathBuf> {
    env.get(key)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing {key} in .env.dev"))
}

fn read_env_file(path: &str) -> Result<HashMap<String, String>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {path}"))?;
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
