//! The scripts of the screens a level's end streams in (openspec AS.53):
//! the objectives screen's (`misc/ui_objectivesscreen.bld`), whose own
//! macros compute what the completion screen shows (`UI - Objectives Menu -
//! Collect Data`) and continue past it (`UI - Objectives Menu - Shutdown`).
//! guacamole links the entry with the level's script part, its placements
//! never run (the TV lays the screen out itself: AU.5); its templates,
//! strings and images are the interface part's ([`crate::interface`]).
//!
//! The part ([`PART`]) is each archive's decompressed `level.bld`, copied
//! raw beside the scripts part's entries (`scripts/<stem>.igz`) once it
//! parses and its op graph reads, written by the script stage after the
//! scripts and spawn models, so a first run's chapter 1 links it. It lands
//! whole or not at all, and resumes alone (AG.15's rule): an extract made
//! before it reads this part on its next start and nothing else. An archive
//! the dump lacks (the e2e fixture's dump holds none) is listed and the part
//! lands without it; the level's end then shows its screen without the
//! screen's results.

use std::path::Path;

use crate::scripts;

/// The part's name in the manifest ([`crate::PARTS`]).
pub const PART: &str = "screen scripts";

/// The level's end streams one screen in, the objectives screen.
pub const ARCHIVES: [&str; 1] = ["misc/ui_objectivesscreen.bld"];

/// Where each archive's entry goes: `scripts/<stem>.igz`, as the scripts
/// part's do, so the interface's model finds its record by the same rule.
pub fn path(archive: &str) -> String {
    scripts::script_path(archive)
}

/// What reading the part found: each archive's checked entry and the
/// scripts it holds, and each archive the dump lacks with why.
#[derive(Debug, Default)]
pub struct Read {
    pub entries: Vec<(String, Vec<u8>, usize)>,
    pub missing: Vec<(String, String)>,
}

/// Each of [`ARCHIVES`] under `usrdir`: its entry, checked as the scripts
/// part's are ([`scripts::check`]). An archive the dump lacks is listed in
/// [`Read::missing`]; one that's there and won't read is the error, naming
/// it, and the part doesn't land.
pub fn read(usrdir: &Path) -> Result<Read, (String, String)> {
    let mut out = Read::default();
    for archive in ARCHIVES {
        if !usrdir.join(archive).is_file() {
            out.missing
                .push((archive.to_owned(), "not on the dump".to_owned()));
            continue;
        }
        let bytes = scripts::entry(usrdir, archive).map_err(|e| (archive.to_owned(), e))?;
        let names = scripts::check(&bytes).map_err(|e| (archive.to_owned(), e))?;
        out.entries.push((archive.to_owned(), bytes, names.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part's entry for the objectives screen sits where the scripts
    /// part's would, so the model's record path rule finds its interface.
    #[test]
    fn the_objectives_screens_entry_sits_beside_the_scripts_parts() {
        assert_eq!(path(ARCHIVES[0]), "scripts/ui_objectivesscreen.igz");
    }

    /// A dump without the archive lists it and reads nothing else; one
    /// whose archive won't unpack is refused by name.
    #[test]
    fn a_missing_archive_is_listed_and_a_broken_one_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let read = read(tmp.path()).unwrap();
        assert!(read.entries.is_empty());
        assert_eq!(
            read.missing,
            [(ARCHIVES[0].to_owned(), "not on the dump".to_owned())]
        );
        std::fs::create_dir_all(tmp.path().join("misc")).unwrap();
        std::fs::write(tmp.path().join(ARCHIVES[0]), b"not an archive").unwrap();
        let (archive, _) = super::read(tmp.path()).unwrap_err();
        assert_eq!(archive, ARCHIVES[0]);
    }
}
