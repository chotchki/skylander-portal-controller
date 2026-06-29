//! Tiny line-by-line parser for RPCS3's `<install>/config/games.yml`.
//!
//! RPCS3 maintains a flat `<serial>: <directory>` map at this path; it's the
//! authoritative serial → game-directory mapping (PLAN 10.8.4). Boot path
//! is `<directory>/PS3_GAME/USRDIR/EBOOT.BIN`.
//!
//! We don't pull in a real YAML dep because the file format is trivial and
//! controlled by RPCS3 — every line is either a comment, a blank, or
//! `BLUS30968: C:/games/ps3/Skylanders Giants/`. Quotes around the path are
//! optional. No nested structure, no anchors, no flow style.
//!
//! Example:
//! ```yaml
//! BLUS30968: C:/games/ps3/Skylanders Giants/
//! BLUS31076: "C:/games/ps3/Skylanders Swap Force/"
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Read RPCS3's `games.yml` and return a serial → game directory map. Trailing
/// slashes / backslashes are stripped so callers can join sub-paths like
/// `PS3_GAME/USRDIR/EBOOT.BIN` without double-separators.
///
/// Current RPCS3 keeps `games.yml` at the **config-dir root**
/// (`<install>/games.yml`); older layouts used `<install>/config/games.yml`.
/// Prefer the root, fall back to the legacy path — on macOS the root is where
/// it actually lives, so the old config-only lookup silently loaded 0 games.
///
/// RELATIVE game directories are anchored to `rpcs3_install_dir` (the config-dir
/// root). RPCS3 itself only ever writes ABSOLUTE paths, but a relative entry is a
/// deliberate hand-edit that makes the whole install PORTABLE: drop the games
/// under `<config>/game_discs/...`, point games.yml at `game_discs/<Game>`, and
/// the bundle survives a copy to another machine or user account with no edits
/// (no `~`-expansion, no hardcoded `/Users/<name>/...`). Absolute entries pass
/// through unchanged.
pub fn read_games_yml(rpcs3_install_dir: &Path) -> Result<HashMap<String, PathBuf>> {
    let root = rpcs3_install_dir.join("games.yml");
    let legacy = rpcs3_install_dir.join("config").join("games.yml");
    let path = if root.is_file() { root } else { legacy };
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("read games.yml at {}", path.display()))?;
    Ok(anchor_relative(parse(&body), rpcs3_install_dir))
}

/// Anchor any RELATIVE game directory to `base` (the RPCS3 config-dir root that
/// holds games.yml). `Path::join` is a no-op on an absolute argument, so
/// RPCS3-written absolute entries pass through untouched while a portable
/// hand-edit like `game_discs/Skylanders Giants` resolves to
/// `<base>/game_discs/Skylanders Giants`. Pure so it's unit-tested without IO.
fn anchor_relative(map: HashMap<String, PathBuf>, base: &Path) -> HashMap<String, PathBuf> {
    map.into_iter()
        .map(|(serial, dir)| (serial, base.join(dir)))
        .collect()
}

/// Resolve the `EBOOT.BIN` inside a game directory using RPCS3's standard
/// disc-image layout. Returns `None` if the EBOOT doesn't exist on disk.
pub fn eboot_for(game_dir: &Path) -> Option<PathBuf> {
    let eboot = game_dir.join("PS3_GAME").join("USRDIR").join("EBOOT.BIN");
    eboot.is_file().then_some(eboot)
}

fn parse(body: &str) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let serial = k.trim();
        let path = v.trim().trim_matches('"');
        // Strip a single trailing separator if present — RPCS3 emits
        // both `.../Game/` and `.../Game` depending on how the entry
        // was added.
        let path = path.trim_end_matches(['/', '\\']);
        if serial.is_empty() || path.is_empty() {
            continue;
        }
        out.insert(serial.to_string(), PathBuf::from(path));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_world_sample() {
        let body = r#"
BLUS30968: C:/games/ps3/Skylanders Giants/
BLUS31076: "C:/games/ps3/Skylanders Swap Force/"
BLUS31441: C:/games/ps3/Digimon All-Star Rumble/BLUS31441-[Digimon All-Star Rumble]/
"#;
        let map = parse(body);
        assert_eq!(
            map.get("BLUS30968"),
            Some(&PathBuf::from("C:/games/ps3/Skylanders Giants"))
        );
        assert_eq!(
            map.get("BLUS31076"),
            Some(&PathBuf::from("C:/games/ps3/Skylanders Swap Force"))
        );
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let body = "
# top comment
BLUS30968: C:/games/Giants/

BLUS31076: C:/games/Swap/
";
        let map = parse(body);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn handles_backslash_paths() {
        let body = r#"BLUS30968: C:\games\ps3\Skylanders Giants\"#;
        let map = parse(body);
        assert_eq!(
            map.get("BLUS30968"),
            Some(&PathBuf::from(r"C:\games\ps3\Skylanders Giants"))
        );
    }

    #[test]
    fn anchor_relative_resolves_relative_keeps_absolute() {
        let base = Path::new("/Users/kid/Library/Application Support/rpcs3");
        let mut map = HashMap::new();
        // Portable hand-edit: relative to the config-dir root.
        map.insert(
            "BLUS30968".to_string(),
            PathBuf::from("game_discs/Skylanders Giants"),
        );
        // RPCS3-written absolute path: must pass through untouched.
        map.insert(
            "BLUS31076".to_string(),
            PathBuf::from("/Volumes/ext/Skylanders Swap Force"),
        );
        let out = anchor_relative(map, base);
        assert_eq!(
            out.get("BLUS30968").unwrap(),
            &base.join("game_discs/Skylanders Giants"),
            "relative dir should anchor under the config-dir root",
        );
        assert_eq!(
            out.get("BLUS31076").unwrap(),
            Path::new("/Volumes/ext/Skylanders Swap Force"),
            "absolute dir should be left as-is",
        );
    }

    #[test]
    fn anchor_relative_then_eboot_for_builds_full_path() {
        // End-to-end shape: a relative games.yml entry, once anchored, yields the
        // absolute EBOOT path eboot_for joins onto (file existence aside).
        let base = Path::new("/srv/rpcs3");
        let mut map = HashMap::new();
        map.insert("BLUS30968".to_string(), PathBuf::from("game_discs/Giants"));
        let anchored = anchor_relative(map, base);
        let dir = anchored.get("BLUS30968").unwrap();
        assert_eq!(
            dir.join("PS3_GAME").join("USRDIR").join("EBOOT.BIN"),
            Path::new("/srv/rpcs3/game_discs/Giants/PS3_GAME/USRDIR/EBOOT.BIN"),
        );
    }
}
