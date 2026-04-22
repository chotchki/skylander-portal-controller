//! Parse RPCS3's `games.yml` — the emulator's own record of installed PS3
//! games. Filters down to the Skylanders titles in
//! `skylander_core::SKYLANDERS_SERIALS` and drops any whose `EBOOT.BIN`
//! is missing.
//!
//! Used by `UiaPortalDriver::list_installed_games`. Kept as a free
//! function so both the production driver and the `library_probe`
//! example can share it without the driver's UIA machinery.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use skylander_core::{GameSerial, InstalledGame, SKYLANDERS_SERIALS};

/// Parse `games.yml` and return the installed Skylanders titles in
/// release order. games.yml format is a flat YAML map of `SERIAL: PATH/`,
/// one per line — parsed by line to avoid pulling in a YAML crate.
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
