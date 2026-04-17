//! Skylanders game catalogue — parses RPCS3's `config/games.yml` and filters
//! down to the titles we care about.
//!
//! games.yml format is a flat YAML map of `SERIAL: PATH/`, one per line.
//! No nested keys, no quoting — we parse by line to avoid pulling in a
//! YAML dependency.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use skylander_core::GameSerial;

/// Skylanders PS3 serials the app knows about, with canonical display names.
/// Source: RPCS3's games.yml + Fandom release info.
pub const SKYLANDERS_SERIALS: &[(&str, &str)] = &[
    ("BLUS30906", "Skylanders: Spyro's Adventure"),
    ("BLUS30968", "Skylanders: Giants"),
    ("BLUS31076", "Skylanders: SWAP Force"),
    ("BLUS31442", "Skylanders: Trap Team"),
    ("BLUS31545", "Skylanders: SuperChargers"),
    ("BLUS31600", "Skylanders: Imaginators"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledGame {
    pub serial: GameSerial,
    pub display_name: String,
    /// Game root directory (parent of `PS3_GAME/`). Server-private.
    #[serde(skip)]
    pub sky_root: PathBuf,
}

impl InstalledGame {
    pub fn eboot_path(&self) -> PathBuf {
        self.sky_root
            .join("PS3_GAME")
            .join("USRDIR")
            .join("EBOOT.BIN")
    }
}

/// Parse RPCS3's games.yml and return only the Skylanders titles present.
/// Returns games sorted in release order per `SKYLANDERS_SERIALS`.
pub fn load_installed(games_yaml: &Path) -> Result<Vec<InstalledGame>> {
    let raw = std::fs::read_to_string(games_yaml)
        .with_context(|| format!("reading {}", games_yaml.display()))?;

    let mut by_serial: BTreeMap<String, PathBuf> = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let serial = k.trim().to_string();
            let path = v.trim().trim_matches('"').trim_matches('\'').to_string();
            by_serial.insert(serial, PathBuf::from(path));
        }
    }

    let mut out = Vec::new();
    for (serial, display_name) in SKYLANDERS_SERIALS {
        if let Some(path) = by_serial.get(*serial) {
            let eboot = path.join("PS3_GAME").join("USRDIR").join("EBOOT.BIN");
            if !eboot.is_file() {
                tracing::warn!(
                    game = *display_name,
                    path = %eboot.display(),
                    "EBOOT.BIN missing; skipping",
                );
                continue;
            }
            out.push(InstalledGame {
                serial: GameSerial::new(*serial),
                display_name: (*display_name).to_string(),
                sky_root: path.clone(),
            });
        }
    }
    Ok(out)
}
