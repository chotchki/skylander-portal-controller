//! Shared resolver for runtime-state roots.
//!
//! Per `CLAUDE.md`, runtime state lives under:
//!   - Release: `%APPDATA%\skylander-portal-controller\`
//!   - Dev (`dev-tools`): `./dev-data/` relative to CWD.
//!
//! All callers that touch the DB, working-copies, or `config.json` route
//! through here so the dev/release split stays consistent across the crate.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolve the base runtime-state directory. Creates it if missing.
pub fn resolve_runtime_dir() -> Result<PathBuf> {
    let dir = runtime_dir_unchecked()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create runtime dir {}", dir.display()))?;
    Ok(dir)
}

/// Resolve the base runtime-state directory *without* creating it. Used by
/// the first-launch wizard so we can test for config presence before
/// side-effecting the filesystem.
pub fn runtime_dir_unchecked() -> Result<PathBuf> {
    #[cfg(feature = "dev-tools")]
    {
        Ok(PathBuf::from("dev-data"))
    }
    #[cfg(not(feature = "dev-tools"))]
    {
        use directories::ProjectDirs;
        let proj = ProjectDirs::from("", "", "skylander-portal-controller")
            .ok_or_else(|| anyhow::anyhow!("couldn't resolve APPDATA via directories crate"))?;
        // ProjectDirs::config_dir() on Windows = %APPDATA%\skylander-portal-controller\config
        // We want %APPDATA%\skylander-portal-controller\ (no `config` subdir) per CLAUDE.md.
        // data_dir() on Windows returns %APPDATA%\skylander-portal-controller\data, also wrong.
        // Walk up from config_dir() to the project root.
        let root = proj
            .config_dir()
            .parent()
            .unwrap_or_else(|| proj.config_dir())
            .to_path_buf();
        Ok(root)
    }
}

/// Path to the persisted `config.json`. Does NOT create the file.
pub fn config_json_path() -> Result<PathBuf> {
    Ok(runtime_dir_unchecked()?.join("config.json"))
}

/// Path to the SQLite DB. Creates the parent directory.
pub fn db_path() -> Result<PathBuf> {
    Ok(resolve_runtime_dir()?.join("db.sqlite"))
}

/// Path to the logs directory. Creates it.
pub fn log_dir() -> Result<PathBuf> {
    let dir = if cfg!(feature = "dev-tools") {
        PathBuf::from("logs")
    } else {
        resolve_runtime_dir()?.join("logs")
    };
    std::fs::create_dir_all(&dir).ok();
    Ok(dir)
}
