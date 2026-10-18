//! Runtime configuration types the shared server reads. Each binary fills
//! [`Config`] with its own loader: the product's `config::load` (`.env.dev` in
//! dev, `config.json` or the first-launch wizard in release) lives in
//! `crates/server`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    /// Launcher window presentation (PLAN 20) — TV (fullscreen) vs Desktop
    /// (resizable window). Consumed by `main.rs` for the eframe viewport.
    pub window_mode: WindowMode,
    /// 2× (1440p) render pass for the macOS surface-embed (PLAN S). When true the
    /// launcher sets `SKYLANDER_SURFACE_2X` so the patched RPCS3 renders 2560×1440.
    /// Konami-gated admin toggle; applies on next launcher boot.
    pub render_2x: bool,
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
    /// guacamole's (openspec AA, design decision 3): no emulator and no figure
    /// file. Nothing spawns, nothing reads RPCS3's config and loads place by
    /// identity (`PortalDriver::place`).
    Native,
}

/// How the launcher presents its window (PLAN 20). Chosen once in the
/// first-launch wizard, persisted in `config.json`, default [`WindowMode::Tv`]
/// so existing installs keep the fullscreen living-room behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// Living-room / Steam Big Picture: fullscreen launcher overlaying RPCS3,
    /// the family drives the portal from phones. The original experience.
    #[default]
    Tv,
    /// Desktop app: the launcher is a normal resizable window that still sits
    /// above the emulator (PLAN 20.4). For a user at a desk; portal control via
    /// a browser on the same PC.
    Desktop,
}

#[cfg(test)]
mod tests {
    use super::*;

    // PLAN 20.6 — the window-mode wire form is a contract shared by config.json,
    // the `/api/launcher/window-mode` request/response bodies, AND the phone's
    // hand-mirrored `model::WindowMode` enum. Pin it so a rename can't silently
    // desync the phone (which can't share this crate).
    #[test]
    fn window_mode_json_wire_form() {
        assert_eq!(serde_json::to_string(&WindowMode::Tv).unwrap(), "\"tv\"");
        assert_eq!(
            serde_json::to_string(&WindowMode::Desktop).unwrap(),
            "\"desktop\"",
        );
        let parsed: WindowMode = serde_json::from_str("\"desktop\"").unwrap();
        assert_eq!(parsed, WindowMode::Desktop);
        // Existing installs (no persisted value) keep the fullscreen TV mode.
        assert_eq!(WindowMode::default(), WindowMode::Tv);
    }
}
