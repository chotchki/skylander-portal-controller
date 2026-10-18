//! Seams where the shared server calls code that stays with the product
//! (`crates/server`, openspec AA design decision 1). The product implements all
//! four with the code that used to sit at each call site and passes them in;
//! a binary without one (guacamole, AA.3) gets its Native arm at those sites.
//!
//! - [`FigureFiles`]: the per-profile `.sky` working copies — the figure stats,
//!   edit and reset routes, the fork on a load, Kaos swap or auto-restart
//!   restore, and the rewrite on a slot reset.
//! - [`DisplayModes`]: pre-set and capture the per-game Windows display mode
//!   around a launch.
//! - [`Rpcs3Config`]: the RPCS3 `config.yml` writes — the Net config at boot and
//!   the transient save-state settings.
//! - [`SettingsWriter`]: the product's `config.json` writes behind the
//!   window-mode and 2× admin toggles.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use skylander_core::Figure;

use crate::config::WindowMode;
use crate::state::AppState;

/// Snapshot of a primary-display mode. Stored per-game so subsequent
/// launches can pre-set the same mode and avoid a flicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// The per-profile `.sky` working copies (PLAN 3.11).
pub trait FigureFiles: Send + Sync {
    /// Routes that read or rewrite a working copy: figure stats, edit and
    /// reset. Merged into the API router as they are.
    fn routes(&self) -> Router<Arc<AppState>>;
    /// The working copy to load for `profile_id`, forked from the pack file on
    /// first use.
    fn resolve_load_path(&self, profile_id: &str, figure: &Figure) -> Result<PathBuf>;
    /// Overwrite the profile's working copy with the fresh pack file; returns
    /// its path.
    fn reset_to_fresh(&self, profile_id: &str, figure: &Figure) -> Result<PathBuf>;
}

/// The primary display's mode (PLAN 4.20.x).
pub trait DisplayModes: Send + Sync {
    /// Best-effort apply: logs a failure and returns `false` rather than
    /// refusing the launch.
    fn try_set(&self, mode: DisplayMode) -> bool;
    /// The primary display's current mode; `Ok(None)` when it can't be read.
    fn get_current(&self) -> Result<Option<DisplayMode>>;
}

/// Writes to RPCS3's `config.yml`.
pub trait Rpcs3Config: Send + Sync {
    /// The one Net configuration Skylanders games run under (PLAN 16.10.1).
    fn ensure_net_config(&self, config_dir: &Path) -> Result<()>;
    /// Swap the save-state-only settings into `config_yml` (PLAN 15.12). The
    /// returned guard puts the original back when dropped.
    fn apply_savestate_config(&self, config_yml: &Path) -> Result<Box<dyn Send>>;
}

/// Why a [`SettingsWriter`] write failed; the route maps each to a 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsWriteError {
    /// The settings file's path couldn't be resolved.
    ConfigPath,
    /// The settings file was read but the rewrite failed.
    Persist,
}

/// The product's persisted settings behind the admin toggles (PLAN 20.6, S).
/// Both apply on the next launcher start. `Ok` also covers "nothing to update"
/// (a dev build reads `.env.dev`, not `config.json`); the writer logs that.
pub trait SettingsWriter: Send + Sync {
    fn set_window_mode(&self, mode: WindowMode) -> Result<(), SettingsWriteError>;
    fn set_render_2x(&self, render_2x: bool) -> Result<(), SettingsWriteError>;
}

/// The four hooks, as `AppState` and the background tasks hold them. The
/// product passes all four; `Default` is none of them.
#[derive(Clone, Default)]
pub struct Hooks {
    pub figure_files: Option<Arc<dyn FigureFiles>>,
    pub display_modes: Option<Arc<dyn DisplayModes>>,
    pub rpcs3_config: Option<Arc<dyn Rpcs3Config>>,
    pub settings: Option<Arc<dyn SettingsWriter>>,
}
