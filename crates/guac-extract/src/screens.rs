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

/// The card game's part (openspec AS.364): the three archives a card
/// pirate's game streams in, their entries beside the screens' by the same
/// rule. guacamole links them after a level that places a pirate and runs
/// their placements as a script asks for each; their art and sounds are
/// other parts' (AS.365).
pub const CARD_GAME_PART: &str = "card game scripts";

/// The card game's archives: the reward card's sprites, the board and the
/// hand select.
pub const CARD_GAME: [&str; 3] = [
    "misc/cardgame_2dsprites_streamed.bld",
    "character/cardgame_streaming.bld",
    "misc/ui_cardgame_handselect_streaming.bld",
];

/// The card game's stones (openspec AS.366): two archives a stone, loaded
/// as its game needs them. Its deck (`character/cardgame_deck_NNN`), named
/// in the board's load string for every stone either hand holds, is a
/// header script defining the stone for the game (`System - Card Game -
/// Card Header`) beside its model; its asset (`misc/cardgame_card_NNN`),
/// asked as the stone is played, defines what the board shows of it. Their
/// own part, so an extract that already holds [`CARD_GAME_PART`] takes it at
/// its next pick.
pub const CARD_STONES_PART: &str = "card game stones";

/// How many stones the disc defines (`Init Setup - Define Card`'s calls),
/// numbered from 0.
pub const CARD_STONE_COUNT: usize = 82;

/// Stone `n`'s deck archive.
pub fn card_deck(n: usize) -> String {
    format!("character/cardgame_deck_{n:03}.bld")
}

/// Stone `n`'s asset archive.
pub fn card_asset(n: usize) -> String {
    format!("misc/cardgame_card_{n:03}.bld")
}

/// Every stone's deck, then every stone's asset, in their numbers' order.
pub fn card_stones() -> Vec<String> {
    (0..CARD_STONE_COUNT)
        .map(card_deck)
        .chain((0..CARD_STONE_COUNT).map(card_asset))
        .collect()
}

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
    read_archives(usrdir, &ARCHIVES)
}

/// [`read`] over `archives`: the card game's part reads [`CARD_GAME`] so.
pub fn read_archives(usrdir: &Path, archives: &[&str]) -> Result<Read, (String, String)> {
    let mut out = Read::default();
    for &archive in archives {
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

    /// The card game's entries sit by the same rule, each its own stem, none
    /// on a screen's place.
    #[test]
    fn the_card_games_entries_sit_by_their_stems() {
        let paths: Vec<String> = CARD_GAME.iter().map(|a| path(a)).collect();
        assert_eq!(
            paths,
            [
                "scripts/cardgame_2dsprites_streamed.igz",
                "scripts/cardgame_streaming.igz",
                "scripts/ui_cardgame_handselect_streaming.igz",
            ]
        );
        assert!(!paths.contains(&path(ARCHIVES[0])));
    }

    /// The stones' entries sit by the same rule, numbered as the board's
    /// load strings name them, none on another part's place.
    #[test]
    fn the_card_stones_entries_sit_by_their_numbers() {
        let stones = card_stones();
        assert_eq!(stones.len(), 2 * CARD_STONE_COUNT);
        assert_eq!(stones[0], "character/cardgame_deck_000.bld");
        assert_eq!(path(&stones[81]), "scripts/cardgame_deck_081.igz");
        assert_eq!(stones[82], "misc/cardgame_card_000.bld");
        assert_eq!(path(&stones[163]), "scripts/cardgame_card_081.igz");
        let others: Vec<String> = CARD_GAME.iter().chain(&ARCHIVES).map(|a| path(a)).collect();
        let mut paths: Vec<String> = stones.iter().map(|d| path(d)).collect();
        assert!(paths.iter().all(|p| !others.contains(p)));
        paths.dedup();
        assert_eq!(paths.len(), stones.len());
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
