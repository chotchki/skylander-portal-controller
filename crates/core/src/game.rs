//! Installed-game metadata + the canonical Skylanders serial catalogue.
//!
//! Pure domain data; no I/O. The `PortalDriver` trait returns
//! `Vec<InstalledGame>` — RPCS3-yaml parsing lives in the UIA driver, and
//! the mock driver seeds a default list from `SKYLANDERS_SERIALS`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::GameSerial;

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
    /// Game root directory (parent of `PS3_GAME/`). Server-private — phone
    /// never sees it. Empty under the mock driver (no real PS3 payload).
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
