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

/// Read RPCS3's `<install>/config/games.yml` and return a serial → game
/// directory map. Trailing slashes / backslashes are stripped so callers
/// can join sub-paths like `PS3_GAME/USRDIR/EBOOT.BIN` without
/// double-separators.
pub fn read_games_yml(rpcs3_install_dir: &Path) -> Result<HashMap<String, PathBuf>> {
    let path = rpcs3_install_dir.join("config").join("games.yml");
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("read games.yml at {}", path.display()))?;
    Ok(parse(&body))
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
        let path = path.trim_end_matches(|c| c == '/' || c == '\\');
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
}
