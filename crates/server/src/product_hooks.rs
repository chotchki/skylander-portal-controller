//! The product's implementations of the four `guac_server::hooks`. Each one is
//! the code that used to sit at the shared server's call site, so the product
//! runs the same steps in the same order with the same logs and errors.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
#[cfg(feature = "sky-stats")]
use axum::routing::{get, post};
use guac_server::hooks::{
    DisplayMode, DisplayModes, FigureFiles, Hooks, Rpcs3Config, SettingsWriteError, SettingsWriter,
};
use skylander_core::Figure;
use tracing::{info, warn};

use crate::config::WindowMode;
use crate::state::AppState;

/// All four, as `main.rs` hands them to the server.
pub fn hooks() -> Hooks {
    Hooks {
        figure_files: Some(Arc::new(WorkingCopies)),
        display_modes: Some(Arc::new(WindowsDisplayMode)),
        rpcs3_config: Some(Arc::new(Rpcs3ConfigYml)),
        settings: Some(Arc::new(ConfigJson)),
    }
}

/// Per-profile `.sky` working copies (`working_copies`, `sky_stats`, `sky_edit`).
pub struct WorkingCopies;

impl FigureFiles for WorkingCopies {
    fn routes(&self) -> Router<Arc<AppState>> {
        let routes = Router::new();
        #[cfg(feature = "sky-stats")]
        let routes = routes
            .route(
                "/api/profiles/:profile_id/figures/:figure_id/stats",
                get(crate::sky_stats::get_figure_stats),
            )
            .route(
                "/api/profiles/:profile_id/figures/:figure_id/edit",
                post(crate::sky_edit::edit_figure),
            )
            .route(
                "/api/profiles/:profile_id/figures/:figure_id/reset",
                post(crate::sky_edit::reset_figure),
            );
        routes
    }

    fn resolve_load_path(&self, profile_id: &str, figure: &Figure) -> Result<PathBuf> {
        crate::working_copies::resolve_load_path(profile_id, figure)
    }

    fn reset_to_fresh(&self, profile_id: &str, figure: &Figure) -> Result<PathBuf> {
        crate::working_copies::reset_to_fresh(profile_id, figure)
    }
}

/// The primary display's mode via Win32 (`display_mode`; a no-op elsewhere).
pub struct WindowsDisplayMode;

impl DisplayModes for WindowsDisplayMode {
    fn try_set(&self, mode: DisplayMode) -> bool {
        crate::display_mode::try_set(mode)
    }

    fn get_current(&self) -> Result<Option<DisplayMode>> {
        crate::display_mode::get_current()
    }
}

/// RPCS3's `config.yml` (`rpcs3_config`).
pub struct Rpcs3ConfigYml;

impl Rpcs3Config for Rpcs3ConfigYml {
    fn ensure_net_config(&self, config_dir: &Path) -> Result<()> {
        crate::rpcs3_config::ensure_rpcs3_net_config(config_dir)
    }

    fn apply_savestate_config(&self, config_yml: &Path) -> Result<Box<dyn Send>> {
        Ok(Box::new(crate::rpcs3_config::apply_savestate_config(
            config_yml,
        )?))
    }
}

/// The launcher's `config.json` (`wizard::PersistedConfig`). Dev builds read
/// `.env.dev` rather than config.json, so there the write is a logged no-op.
pub struct ConfigJson;

impl SettingsWriter for ConfigJson {
    fn set_window_mode(&self, mode: WindowMode) -> Result<(), SettingsWriteError> {
        let path = match crate::paths::config_json_path() {
            Ok(p) => p,
            Err(e) => {
                warn!("window-mode: config path error: {e}");
                return Err(SettingsWriteError::ConfigPath);
            }
        };
        match crate::wizard::PersistedConfig::read(&path) {
            Ok(mut persisted) => {
                persisted.window_mode = mode;
                if let Err(e) = persisted.write(&path) {
                    warn!("window-mode: write config.json failed: {e}");
                    return Err(SettingsWriteError::Persist);
                }
                info!("window mode set to {:?} (restart to apply)", mode);
            }
            Err(e) => {
                // Dev builds (no config.json — `.env.dev` is the source) land here.
                warn!("window-mode: no config.json to update ({e}); dev build? not persisted");
            }
        }
        Ok(())
    }

    fn set_render_2x(&self, render_2x: bool) -> Result<(), SettingsWriteError> {
        let path = match crate::paths::config_json_path() {
            Ok(p) => p,
            Err(e) => {
                warn!("render-2x: config path error: {e}");
                return Err(SettingsWriteError::ConfigPath);
            }
        };
        match crate::wizard::PersistedConfig::read(&path) {
            Ok(mut persisted) => {
                persisted.render_2x = render_2x;
                if let Err(e) = persisted.write(&path) {
                    warn!("render-2x: write config.json failed: {e}");
                    return Err(SettingsWriteError::Persist);
                }
                info!("2x render set to {} (restart to apply)", render_2x);
            }
            Err(e) => {
                warn!("render-2x: no config.json to update ({e}); dev build? not persisted");
            }
        }
        Ok(())
    }
}
