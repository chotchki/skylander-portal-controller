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

/// The card game's templates' palettes (openspec AS.483): every skinned
/// model a placement of [`CARD_GAME`]'s and [`card_stones`]' archives names
/// (the board's attack rig, the stones' models), read by the levels'
/// palettes rule ([`crate::levels::palettes`]: each slot's clip, its kind
/// and whether it loops), one file for the lot, since the board times its
/// flips by those clips.
pub const CARD_PALETTES_PART: &str = "card game palettes";

/// What [`CardPalettes`] names itself, in place of a level.
pub const CARD_PALETTES_NAME: &str = "card game";

/// [`card_palettes`]' read: the palettes, each owner once, the first
/// archive's that names it, and each archive the dump lacks with why.
#[derive(Debug, Default)]
pub struct CardPalettes {
    pub palettes: Option<guac_ir::palette::LevelPalettes>,
    pub missing: Vec<(String, String)>,
}

/// [`CARD_PALETTES_PART`]'s palettes off the dump at `usrdir`, in
/// [`CARD_GAME`]'s then [`card_stones`]' order. An archive the dump lacks
/// is listed; one that's there and won't read is the error, naming it.
pub fn card_palettes(
    usrdir: &Path,
    identity: guac_ir::DumpIdentity,
) -> Result<CardPalettes, (String, String)> {
    let stones = card_stones();
    let archives = CARD_GAME
        .iter()
        .copied()
        .chain(stones.iter().map(String::as_str));
    let mut out = guac_ir::palette::LevelPalettes {
        identity: identity.clone(),
        level: CARD_PALETTES_NAME.to_owned(),
        palettes: Vec::new(),
        loops: Vec::new(),
    };
    let mut missing = Vec::new();
    for archive in archives {
        if !usrdir.join(archive).is_file() {
            missing.push((archive.to_owned(), "not on the dump".to_owned()));
            continue;
        }
        let read = scripts::entry(usrdir, archive)
            .and_then(|bytes| {
                let igz = guac_formats::igz::Igz::parse(&bytes).map_err(|e| e.to_string())?;
                crate::levels::palettes(&igz, archive, identity.clone())
            })
            .map_err(|e| (archive.to_owned(), e))?;
        let read = read.ir;
        for palette in &read.palettes {
            if out.palettes.iter().any(|p| p.owner == palette.owner) {
                continue;
            }
            let slots = read.loops_of(&palette.owner).to_vec();
            if !slots.is_empty() {
                out.loops.push(guac_ir::palette::Loops {
                    owner: palette.owner.clone(),
                    slots,
                });
            }
            out.palettes.push(palette.clone());
        }
    }
    let any = archives_held(usrdir);
    Ok(CardPalettes {
        palettes: any.then_some(out),
        missing,
    })
}

/// The card game's art (openspec AS.365): what the three streams' and the
/// stones' archives draw and play, read by the readers the rest of the
/// extract uses, so nothing here is a second format. Per archive: every model
/// a placement names, as a level's props and a character's spawns are read
/// ([`crate::scripts::spawn_models`], the levels' `Prop` with its textures),
/// into [`card_model_path`]; the interface record of an archive the
/// interface part doesn't read (the reward card's sprites and the stones'
/// assets, [`card_interface_held`]), into [`card_interface_path`]; and its
/// sounds into the sounds part's place ([`crate::sounds::path`]), where the
/// TV's sound book finds an archive by its linked unit's stem.
pub const CARD_ART_PART: &str = "card game art";

/// Where the part's own files sit.
pub const CARD_ART_DIR: &str = "card_game";

/// The part's shape: 2 since its archives' attach nodes joined it (openspec
/// AS.554), 3 since an attach list's empty slot reads as one (AS.562: the
/// eight weapon models' lists, refused whole under 2). The record's name carries it ([`CARD_ART_RECORD`]), so an
/// extract an older build wrote holds another name and reads the part again
/// ([`card_art_records_current`]).
pub const CARD_ART_SHAPE: u32 = 3;

/// The part's record: what it read and every refusal by name, named for
/// [`CARD_ART_SHAPE`].
pub const CARD_ART_RECORD: &str = "card_game/art.v3.json";

/// Every archive the art part reads, the streams then the stones.
pub fn card_art_archives() -> Vec<String> {
    CARD_GAME
        .iter()
        .map(|a| (*a).to_owned())
        .chain(card_stones())
        .collect()
}

fn ir_ext(ir: crate::intros::IrFormat) -> &'static str {
    match ir {
        crate::intros::IrFormat::Ron => "ron",
        crate::intros::IrFormat::Postcard => "postcard",
    }
}

/// `card_game/models/<stem>.<ron|postcard>`: an archive's models.
pub fn card_model_path(ir: crate::intros::IrFormat, archive: &str) -> String {
    format!(
        "{CARD_ART_DIR}/models/{}.{}",
        scripts::stem(archive),
        ir_ext(ir)
    )
}

/// `card_game/nodes/<stem>.<ron|postcard>`: the attach lists of an
/// archive's models (openspec AS.554), as a level's nodes part keeps a
/// level's ([`guac_ir::nodes::LevelNodes`], named for the archive).
pub fn card_nodes_path(ir: crate::intros::IrFormat, archive: &str) -> String {
    format!(
        "{CARD_ART_DIR}/nodes/{}.{}",
        scripts::stem(archive),
        ir_ext(ir)
    )
}

/// `card_game/interface/<stem>.v<RECORD>.<ron|postcard>`: an archive's
/// interface record, named for its shape as the interface part's are.
pub fn card_interface_path(ir: crate::intros::IrFormat, archive: &str) -> String {
    format!(
        "{CARD_ART_DIR}/interface/{}.v{}.{}",
        scripts::stem(archive),
        guac_ir::interface::RECORD,
        ir_ext(ir)
    )
}

/// Whether the art part's `files` are this build's: every interface record
/// among them named for this build's record (AT.27's rule), and a part that
/// wrote any file holding this shape's record ([`CARD_ART_RECORD`]). The
/// manifest's currency check ([`crate::interface::record_current`]) and the
/// part's own ask it, so an extract holding another shape's opens as a
/// resume and reads the part again. A part with no file (a dump holding
/// none of the archives) has nothing to re-read.
pub fn card_art_records_current(files: &[String]) -> bool {
    let tag = format!(".v{}.", guac_ir::interface::RECORD);
    let dir = format!("{CARD_ART_DIR}/interface/");
    (files.is_empty() || files.iter().any(|f| f == CARD_ART_RECORD))
        && files
            .iter()
            .filter(|f| f.starts_with(&dir))
            .all(|f| f.contains(&tag))
}

/// Whether the interface part already holds `archive`'s record (a
/// character archive or a menu's), so the art part leaves it there.
pub fn card_interface_held(archive: &str) -> bool {
    archive.starts_with(&format!("{}/", crate::models::CHARACTER_DIR))
        || crate::interface::is_menu(archive)
}

/// One archive's art, read.
#[derive(Debug)]
pub struct ArchiveArt {
    pub archive: String,
    pub models: crate::levels::Read<guac_ir::spawn::SpawnModels>,
    /// Its models' attach lists (openspec AS.554), read as a level's nodes
    /// part reads a level's ([`crate::levels::nodes`]).
    pub nodes: guac_ir::nodes::LevelNodes,
    /// `None` where the interface part holds the record.
    pub interface: Option<crate::interface::Read>,
    pub sounds: crate::sounds::Read,
}

/// `archive`'s art off the dump at `usrdir`; `exports`, `global.bld`'s
/// ([`crate::sounds::exports`]). A model, an image or a sound that won't
/// read is listed in its IR's `refused`; the error is an archive that
/// won't read at all.
pub fn card_art(
    usrdir: &Path,
    archive: &str,
    exports: &guac_formats::exports::Exports,
    identity: guac_ir::DumpIdentity,
) -> Result<ArchiveArt, String> {
    let bld_path = usrdir.join(archive);
    let bld = std::fs::read(&bld_path).map_err(|e| e.to_string())?;
    let arc = match std::fs::read(bld_path.with_extension("arc")) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!(".arc: {e}")),
    };
    let entry = scripts::entry(usrdir, archive)?;
    let igz = guac_formats::igz::Igz::parse(&entry).map_err(|e| e.to_string())?;
    let models = scripts::spawn_models(&igz, archive, identity.clone(), None, None)?;
    let nodes = crate::levels::nodes(&igz, archive, identity.clone())?.ir;
    let interface = if card_interface_held(archive) {
        None
    } else {
        Some(crate::interface::read(
            archive,
            &bld,
            arc.as_deref(),
            identity.clone(),
        )?)
    };
    let sounds = crate::sounds::read(archive, &bld, arc.as_deref(), exports, identity)?;
    Ok(ArchiveArt {
        archive: archive.to_owned(),
        models,
        nodes,
        interface,
        sounds,
    })
}

/// [`CARD_ART_RECORD`]'s body: per archive what landed, and every refusal
/// by name with why.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CardArtRecord {
    pub archives: Vec<CardArtEntry>,
    /// The archives the dump lacks.
    pub missing: Vec<String>,
}

/// One archive's line in [`CardArtRecord`].
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CardArtEntry {
    pub archive: String,
    /// Each model read, by the path placements name it by.
    pub models: Vec<String>,
    pub textures: usize,
    /// Each model whose attach list the part keeps, by path (openspec
    /// AS.554).
    #[serde(default)]
    pub nodes: Vec<String>,
    /// Images of the interface record the part keeps, `None` where the
    /// interface part holds it.
    pub images: Option<usize>,
    pub sounds: Vec<String>,
    /// `(what, why)`: models, images and sounds that didn't read.
    pub refused: Vec<(String, String)>,
}

impl CardArtEntry {
    /// `art`'s line.
    pub fn of(art: &ArchiveArt) -> Self {
        let mut refused: Vec<(String, String)> = art
            .models
            .ir
            .refused
            .iter()
            .map(|r| (format!("model {}", r.path), r.reason.clone()))
            .collect();
        if let Some(i) = &art.interface {
            refused.extend(
                i.ir.refused
                    .iter()
                    .map(|(n, why)| (format!("image {n}"), why.clone())),
            );
        }
        refused.extend(
            art.nodes
                .refused
                .iter()
                .map(|r| (format!("nodes {}", r.path), r.reason.clone())),
        );
        refused.extend(
            art.sounds
                .ir
                .refused
                .iter()
                .map(|(n, why)| (format!("sound {n}"), why.clone())),
        );
        let mut textures: Vec<&str> = art
            .models
            .textures
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        if let Some(i) = &art.interface {
            textures.extend(i.textures.iter().map(|(t, _)| t.as_str()));
        }
        textures.sort_unstable();
        textures.dedup();
        Self {
            archive: art.archive.clone(),
            models: art.models.ir.props.iter().map(|p| p.path.clone()).collect(),
            textures: textures.len(),
            nodes: art
                .nodes
                .models
                .iter()
                .map(|m| m.load_file.clone())
                .collect(),
            images: art.interface.as_ref().map(|i| {
                i.ir.templates
                    .iter()
                    .filter(|t| matches!(t.content, guac_ir::interface::Content::Image(_)))
                    .count()
            }),
            sounds: art
                .sounds
                .ir
                .sounds
                .iter()
                .filter_map(|s| s.name.clone())
                .collect(),
            refused,
        }
    }
}

/// Whether the dump holds any of the card game's archives: one with none
/// (the e2e fixture's) writes no file.
fn archives_held(usrdir: &Path) -> bool {
    CARD_GAME
        .iter()
        .copied()
        .chain(card_stones().iter().map(String::as_str))
        .any(|a| usrdir.join(a).is_file())
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
