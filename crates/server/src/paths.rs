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

/// Directory holding the launcher's shipped assets (`data/`, `phone-dist/`,
/// `rpcs3/`) — normally the exe's own folder. The software-GL fallback
/// re-execs a copy of the exe from the `mesa/` subdir (PLAN 19), so that child
/// would otherwise resolve assets against `mesa/`; it gets the real install
/// dir back via the [`crate::gl_fallback::APP_DIR_ENV`] env var. Every
/// exe-relative asset path goes through here so the re-exec child stays
/// correct.
pub fn app_asset_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(crate::gl_fallback::APP_DIR_ENV) {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

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
            .ok_or_else(|| anyhow::anyhow!("couldn't resolve runtime dir via directories crate"))?;
        // The right path differs per OS; the `directories` crate's
        // `config_dir()` returns a per-platform "canonical" location
        // that we need to massage into our project's chosen layout.
        //
        // - Windows: `config_dir()` =
        //   `%APPDATA%\skylander-portal-controller\config\`. We want
        //   `%APPDATA%\skylander-portal-controller\` per CLAUDE.md, so
        //   walk up one level. `data_dir()` ends in `\data` — same
        //   issue, also wrong shape for us.
        // - macOS: `config_dir()` =
        //   `~/Library/Application Support/skylander-portal-controller/`,
        //   which is already the right level — DON'T walk up
        //   (would land in `~/Library/Application Support/` and pollute
        //   the user's home directory with a stray `config.json`).
        #[cfg(windows)]
        {
            let root = proj
                .config_dir()
                .parent()
                .unwrap_or_else(|| proj.config_dir())
                .to_path_buf();
            Ok(root)
        }
        #[cfg(not(windows))]
        {
            Ok(proj.config_dir().to_path_buf())
        }
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

/// Directory holding per-profile working copies of `.sky` files. Shaped as
/// `<runtime_root>/working/<profile_id>/`. PLAN 3.11.1. Lazy-creates.
pub fn working_copy_dir(profile_id: &str) -> Result<PathBuf> {
    let dir = resolve_runtime_dir()?.join("working").join(profile_id);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create working-copy dir {}", dir.display()))?;
    Ok(dir)
}

/// Path to a specific figure's working copy under a profile:
/// `<runtime_root>/working/<profile_id>/<figure_id>.sky`. Does NOT create
/// the file — callers decide whether to fork from the pack or expect it to
/// exist.
pub fn working_copy_path(profile_id: &str, figure_id: &str) -> Result<PathBuf> {
    Ok(working_copy_dir(profile_id)?.join(format!("{figure_id}.sky")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_copy_path_shape() {
        let p = working_copy_path("alice", "deadbeef").unwrap();
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(
            s.ends_with("/working/alice/deadbeef.sky"),
            "unexpected: {s}"
        );
    }

    #[test]
    fn working_copy_dir_is_created() {
        let d = working_copy_dir("bob").unwrap();
        assert!(d.is_dir(), "dir was not created: {}", d.display());
    }
}
