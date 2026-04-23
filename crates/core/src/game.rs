//! Installed-game metadata + the canonical Skylanders serial catalogue.
//!
//! Pure domain data; no I/O. The `PortalDriver` trait returns
//! `Vec<InstalledGame>` — UIA enumeration lives in the driver, and the
//! mock driver seeds a default list from `SKYLANDERS_SERIALS`.

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
}
