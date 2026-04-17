//! Per-profile working copies of `.sky` firmware files (PLAN 3.11).
//!
//! Motivation: when a kid loads a figure onto the portal, RPCS3 writes back
//! to the `.sky` file (XP, gold, etc.). Without working copies, every profile
//! would share the same physical file → one kid's progress clobbers another.
//! With working copies, each profile+figure pair gets its own
//! `<runtime_root>/working/<profile_id>/<figure_id>.sky` path that's
//! forked-on-first-use from the pack's fresh copy.
//!
//! See `CLAUDE.md` "Profiles & PINs" — one working copy per profile+figure
//! that's shared across games (so you level up Spyro in SSA and the same
//! levelled-up state appears in Giants).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use skylander_core::Figure;
use tracing::{debug, info};

use crate::paths;

/// Resolve the path the driver should load from, forking the pack's fresh
/// `.sky` into the per-profile working path on first use. Subsequent calls
/// with the same `(profile_id, figure)` return the existing working path
/// (save progress is preserved).
///
/// `pack_path` is `figure.sky_path` — the fresh master file.
pub fn resolve_load_path(profile_id: &str, figure: &Figure) -> Result<PathBuf> {
    let working = paths::working_copy_path(profile_id, &figure.id.0)?;
    if !working.exists() {
        fork_from_pack(&figure.sky_path, &working)?;
        info!(
            profile = %profile_id,
            figure = %figure.id.0,
            "forked working copy from pack",
        );
    } else {
        debug!(
            profile = %profile_id,
            figure = %figure.id.0,
            "reusing existing working copy",
        );
    }
    Ok(working)
}

/// Re-fork a working copy from the pack's fresh `.sky`, destroying any
/// progress on the current working copy. Used by the reset-to-fresh flow
/// (PLAN 3.11.3). Caller is responsible for confirming with the user.
pub fn reset_to_fresh(profile_id: &str, figure: &Figure) -> Result<PathBuf> {
    let working = paths::working_copy_path(profile_id, &figure.id.0)?;
    fork_from_pack(&figure.sky_path, &working)?;
    info!(
        profile = %profile_id,
        figure = %figure.id.0,
        "reset working copy to fresh pack contents",
    );
    Ok(working)
}

fn fork_from_pack(pack_path: &Path, working_path: &Path) -> Result<()> {
    if let Some(parent) = working_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::copy(pack_path, working_path)
        .with_context(|| format!("copy {} → {}", pack_path.display(), working_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skylander_core::{Category, Element, FigureId, GameOfOrigin};
    use tempfile::TempDir;

    fn fixture_figure(tmp: &TempDir, id: &str, body: &[u8]) -> Figure {
        let pack_path = tmp.path().join(format!("{id}.sky"));
        std::fs::write(&pack_path, body).unwrap();
        Figure {
            id: FigureId::new(id),
            canonical_name: format!("Figure-{id}"),
            variant_tag: "base".into(),
            variant_group: format!("Figure-{id}"),
            game: GameOfOrigin::SpyrosAdventure,
            element: Some(Element::Fire),
            category: Category::Figure,
            sky_path: pack_path,
            element_icon_path: None,
        }
    }

    // These tests rely on `paths::working_copy_path` returning a path under
    // `./dev-data/working/...` (dev mode). Each test uses a unique profile
    // id so they don't stomp each other.

    #[test]
    fn forks_on_first_use() {
        let tmp = TempDir::new().unwrap();
        let fig = fixture_figure(&tmp, "fork_aaaa", b"fresh-pack-bytes");
        let path = resolve_load_path("test_fork_first", &fig).unwrap();
        assert!(path.exists());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes, b"fresh-pack-bytes");
        // Cleanup so re-runs start clean.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reuses_existing_working_copy() {
        let tmp = TempDir::new().unwrap();
        let fig = fixture_figure(&tmp, "reuse_bbbb", b"fresh-v1");
        let first = resolve_load_path("test_reuse", &fig).unwrap();
        // Pretend the game wrote progress to it.
        std::fs::write(&first, b"modified-by-game").unwrap();
        // Second resolve: should return the same path without re-forking.
        let second = resolve_load_path("test_reuse", &fig).unwrap();
        assert_eq!(first, second);
        let bytes = std::fs::read(&second).unwrap();
        assert_eq!(
            &bytes, b"modified-by-game",
            "working copy should NOT have been overwritten"
        );
        let _ = std::fs::remove_file(&first);
    }

    #[test]
    fn reset_restores_fresh_copy() {
        let tmp = TempDir::new().unwrap();
        let fig = fixture_figure(&tmp, "reset_cccc", b"pack-contents");
        let wc = resolve_load_path("test_reset", &fig).unwrap();
        std::fs::write(&wc, b"progress-to-be-destroyed").unwrap();

        let after = reset_to_fresh("test_reset", &fig).unwrap();
        assert_eq!(wc, after);
        let bytes = std::fs::read(&after).unwrap();
        assert_eq!(&bytes, b"pack-contents");
        let _ = std::fs::remove_file(&wc);
    }

    #[test]
    fn isolates_profiles() {
        let tmp = TempDir::new().unwrap();
        let fig = fixture_figure(&tmp, "isolate_dddd", b"shared-pack");
        let a = resolve_load_path("test_iso_alice", &fig).unwrap();
        let b = resolve_load_path("test_iso_bob", &fig).unwrap();
        assert_ne!(a, b, "each profile gets its own working path");
        // Modifying one must not affect the other.
        std::fs::write(&a, b"alice-only").unwrap();
        let b_bytes = std::fs::read(&b).unwrap();
        assert_eq!(&b_bytes, b"shared-pack");
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }
}
