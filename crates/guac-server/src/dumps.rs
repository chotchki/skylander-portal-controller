//! Finds the user's disc dumps for guacamole's setup (AA design decision 6).
//!
//! A dump is a folder holding `PS3_GAME/PARAM.SFO`, looked for in each chosen
//! folder and up to [`MAX_DEPTH`] levels below. Only TITLE_ID is read.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use guac_formats::sfo::ParamSfo;
use skylander_core::{GameSerial, InstalledGame, SKYLANDERS_SERIALS};

/// The one game guacamole offers.
pub const GIANTS_TITLE_ID: &str = "BLUS30968";

/// How many folder levels below a chosen folder a dump can sit.
pub const MAX_DEPTH: usize = 2;

/// Real PARAM.SFO files are about 1 KiB; anything past this isn't one.
const MAX_SFO_LEN: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDump {
    pub title_id: String,
    /// The folder holding `PS3_GAME`; stays on the machine, never sent to a phone.
    pub dir: PathBuf,
}

impl FoundDump {
    /// The Skylanders title's display name; `None` for any other game.
    pub fn display_name(&self) -> Option<&'static str> {
        SKYLANDERS_SERIALS
            .iter()
            .find(|(serial, _)| *serial == self.title_id)
            .map(|(_, name)| *name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpScan {
    /// Every dump found, in walk order (chosen folders in order, names sorted).
    pub found: Vec<FoundDump>,
    /// `PARAM.SFO` files that wouldn't parse or had no TITLE_ID.
    pub unreadable: Vec<PathBuf>,
    /// Chosen folders that couldn't be listed (missing or access denied).
    pub unlisted: Vec<PathBuf>,
}

impl DumpScan {
    /// The Giants dump, if one was found.
    pub fn offered(&self) -> Option<&FoundDump> {
        self.found.iter().find(|d| d.title_id == GIANTS_TITLE_ID)
    }

    /// The phone's game list: Giants when found, else nothing.
    pub fn games(&self) -> Vec<InstalledGame> {
        self.offered()
            .map(|d| InstalledGame {
                serial: GameSerial::new(GIANTS_TITLE_ID),
                display_name: d.display_name().unwrap_or(GIANTS_TITLE_ID).to_string(),
            })
            .into_iter()
            .collect()
    }
}

/// Scan each folder in `dirs`; a chosen folder that can't be listed finds
/// nothing and lands in `unlisted`.
pub fn scan(dirs: &[PathBuf]) -> DumpScan {
    let mut out = DumpScan::default();
    let mut seen = HashSet::new();
    for dir in dirs {
        walk(dir, 0, &mut out, &mut seen);
    }
    out
}

fn walk(dir: &Path, depth: usize, out: &mut DumpScan, seen: &mut HashSet<PathBuf>) {
    let sfo = dir.join("PS3_GAME").join("PARAM.SFO");
    if sfo.is_file() {
        // Overlapping chosen folders reach the same dump twice.
        let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if seen.insert(key) {
            match read_title_id(&sfo) {
                Some(title_id) => out.found.push(FoundDump {
                    title_id,
                    dir: dir.to_path_buf(),
                }),
                None => {
                    tracing::warn!(path = %sfo.display(), "unreadable PARAM.SFO");
                    out.unreadable.push(sfo);
                }
            }
        }
        // A dump's own folders hold no other dump.
        return;
    }
    if depth == MAX_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // Only a chosen folder: volume roots hold system folders nobody can list.
            if depth == 0 {
                tracing::warn!(path = %dir.display(), error = %e, "can't list dumps folder");
                out.unlisted.push(dir.to_path_buf());
            }
            return;
        }
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort();
    for child in children {
        walk(&child, depth + 1, out, seen);
    }
}

fn read_title_id(sfo: &Path) -> Option<String> {
    let file = std::fs::File::open(sfo).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_SFO_LEN + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_SFO_LEN {
        return None;
    }
    let parsed = ParamSfo::parse(&bytes).ok()?;
    parsed
        .title_id()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_formats::sfo::builder::SfoBuilder;

    /// A dump at `dir`: `PS3_GAME/PARAM.SFO` and, unless `usrdir` is false, `USRDIR`.
    fn dump(dir: &Path, title_id: &str, usrdir: bool) {
        let game = dir.join("PS3_GAME");
        std::fs::create_dir_all(&game).unwrap();
        if usrdir {
            std::fs::create_dir_all(game.join("USRDIR")).unwrap();
        }
        let sfo = SfoBuilder::disc_game(title_id, "01.00", "01.00").build();
        std::fs::write(game.join("PARAM.SFO"), sfo).unwrap();
    }

    fn giants_at(rel: &str) -> DumpScan {
        let root = tempfile::tempdir().unwrap();
        dump(&root.path().join(rel), GIANTS_TITLE_ID, true);
        scan(&[root.path().to_path_buf()])
    }

    #[test]
    fn giants_is_found_at_depth_0_1_and_2_and_not_3() {
        for rel in ["", "a", "a/b"] {
            let found = giants_at(rel);
            let offered = found.offered().unwrap_or_else(|| panic!("depth {rel:?}"));
            assert_eq!(offered.title_id, GIANTS_TITLE_ID);
            assert_eq!(found.games().len(), 1);
            assert_eq!(found.games()[0].serial.as_str(), GIANTS_TITLE_ID);
            assert_eq!(found.games()[0].display_name, "Skylanders: Giants");
        }
        let too_deep = giants_at("a/b/c");
        assert!(too_deep.found.is_empty(), "{too_deep:?}");
        assert!(too_deep.games().is_empty());
    }

    #[test]
    fn other_titles_are_listed_and_not_offered() {
        let root = tempfile::tempdir().unwrap();
        dump(&root.path().join("Giants"), GIANTS_TITLE_ID, true);
        dump(&root.path().join("Swap Force"), "BLUS31076", true);
        dump(&root.path().join("Other/Disc"), "BLES00001", true);
        let found = scan(&[root.path().to_path_buf()]);

        let ids: Vec<&str> = found.found.iter().map(|d| d.title_id.as_str()).collect();
        assert_eq!(ids, ["BLUS30968", "BLES00001", "BLUS31076"]);
        assert_eq!(
            found.found[2].display_name(),
            Some("Skylanders: SWAP Force")
        );
        assert_eq!(found.found[1].display_name(), None);
        let games = found.games();
        let serials: Vec<&str> = games.iter().map(|g| g.serial.as_str()).collect();
        assert_eq!(serials, [GIANTS_TITLE_ID]);

        // Without Giants, the others are still listed and nothing is offered.
        let no_giants = tempfile::tempdir().unwrap();
        dump(&no_giants.path().join("Swap Force"), "BLUS31076", true);
        let found = scan(&[no_giants.path().to_path_buf()]);
        assert_eq!(found.found.len(), 1);
        assert!(found.offered().is_none());
        assert!(found.games().is_empty());
    }

    #[test]
    fn a_dump_without_usrdir_still_identifies() {
        let root = tempfile::tempdir().unwrap();
        dump(&root.path().join("Giants"), GIANTS_TITLE_ID, false);
        assert!(!root.path().join("Giants/PS3_GAME/USRDIR").exists());
        let found = scan(&[root.path().to_path_buf()]);
        assert_eq!(
            found.offered().map(|d| d.title_id.as_str()),
            Some(GIANTS_TITLE_ID)
        );
    }

    #[test]
    fn several_folders_overlapping_or_missing_list_each_dump_once() {
        let root = tempfile::tempdir().unwrap();
        dump(&root.path().join("Giants"), GIANTS_TITLE_ID, true);
        let dirs = [
            root.path().to_path_buf(),
            root.path().join("Giants"),
            root.path().join("missing"),
        ];
        let found = scan(&dirs);
        assert_eq!(found.found.len(), 1, "{found:?}");
        assert_eq!(found.unlisted, [root.path().join("missing")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cant_be_listed_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let denied = root.path().join("denied");
        dump(&denied.join("Giants"), GIANTS_TITLE_ID, true);
        let set = |mode| {
            std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        set(0o311);
        if std::fs::read_dir(&denied).is_ok() {
            set(0o755);
            return; // root lists anything
        }
        let found = scan(std::slice::from_ref(&denied));
        set(0o755);
        assert!(found.found.is_empty(), "{found:?}");
        assert_eq!(found.unlisted, [denied]);
    }

    #[test]
    fn a_broken_sfo_is_reported_not_found() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Broken/PS3_GAME");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("PARAM.SFO"), b"not an sfo").unwrap();
        dump(&root.path().join("No Id"), "", true);
        let found = scan(&[root.path().to_path_buf()]);
        assert!(found.found.is_empty());
        assert_eq!(found.unreadable.len(), 2);
    }
}
