//! The screens' part on a synthetic dump (openspec AS.53): the objectives
//! screen's archive holding a script entry built in code, then the same
//! dump without it and with one that won't unpack. The real dump's runs in
//! guacamole's `giants_level_end`, which links the entry.

use std::fs;
use std::path::{Path, PathBuf};

use guac_extract::known::KnownDump;
use guac_extract::screens::{ARCHIVES, CARD_GAME, CARD_GAME_PART, PART, path};
use guac_extract::stages::{ScriptStage, card_part, screen_part};
use guac_extract::{ExtractRoot, IdentityDir, open};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::sfo::builder::SfoBuilder;

/// A dump whose `USRDIR` `edit` makes, beside its `PARAM.SFO`.
fn dump_with(tmp: &Path, edit: impl FnOnce(&Path)) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::create_dir_all(usr.join("misc")).unwrap();
    fs::create_dir_all(usr.join("character")).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    edit(&usr);
    dump
}

/// The script entry the archive holds: one script built by guac-formats.
fn entry() -> Vec<u8> {
    guac_formats::script::builder::creature().0
}

/// The extract `dump` opens into, trusted as a known dump.
fn opened(tmp: &Path, dump: &Path) -> IdentityDir {
    let scan = guac_extract::dump::scan(dump, &Default::default(), &|_| {}).unwrap();
    let known = vec![KnownDump {
        title_id: "BLUS30968",
        app_ver: "01.00",
        manifest_hash: Box::leak(scan.identity.manifest_hash.to_string().into_boxed_str()),
        source: "test",
    }];
    let root = ExtractRoot::checked(tmp.join("out/extract")).unwrap();
    open(dump, &root, &known, &|_| {}).unwrap().dir
}

/// The archive's entry lands raw at `scripts/ui_objectivesscreen.igz`, the
/// part recorded with it; a second run keeps it, writing nothing.
#[test]
fn the_screens_entry_lands_beside_the_scripts_and_a_second_run_keeps_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |usr| {
        let bld = IgaBuilder::new().lzma("level.bld", &entry()).build();
        fs::write(usr.join(ARCHIVES[0]), bld).unwrap();
    });
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    screen_part(&mut dir, &usr, &mut st).unwrap();
    assert!(st.screens_read && !st.screens_kept, "{:?}", st.failed);
    assert_eq!((st.screens, st.screen_files.written), (1, 1));
    assert!(st.screens_missing.is_empty());
    let rel = path(ARCHIVES[0]);
    assert_eq!(dir.manifest().parts[PART], std::slice::from_ref(&rel));
    assert_eq!(fs::read(dir.path().join(&rel)).unwrap(), entry(), "raw");
    let mut again = ScriptStage::default();
    screen_part(&mut dir, &usr, &mut again).unwrap();
    assert!(again.screens_kept && !again.screens_read);
    assert_eq!(
        (again.screen_files.written, again.screen_files.kept),
        (0, 1)
    );
}

/// A dump without the archive (the e2e fixture's) names it and records the
/// part with no file, so a later start keeps it; one whose archive won't
/// unpack is the part's failure, named, and the part left unrecorded.
#[test]
fn a_missing_archive_is_named_and_a_broken_one_leaves_the_part_unrecorded() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |_| {});
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    screen_part(&mut dir, &usr, &mut st).unwrap();
    assert!(st.screens_read);
    assert_eq!(st.screens, 0);
    assert_eq!(
        st.screens_missing,
        [(ARCHIVES[0].to_owned(), "not on the dump".to_owned())]
    );
    assert!(dir.manifest().parts[PART].is_empty());
    let mut again = ScriptStage::default();
    screen_part(&mut dir, &usr, &mut again).unwrap();
    assert!(again.screens_kept, "a part recorded with no file is kept");

    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |usr| {
        let bld = IgaBuilder::new().lzma("level.bld", b"not an igz").build();
        fs::write(usr.join(ARCHIVES[0]), bld).unwrap();
    });
    let mut dir = opened(tmp.path(), &dump);
    let mut st = ScriptStage::default();
    screen_part(&mut dir, &dump.join("PS3_GAME/USRDIR"), &mut st).unwrap();
    assert!(!st.screens_read && !st.screens_kept);
    assert_eq!(st.failed.len(), 1);
    assert_eq!(
        (st.failed[0].part.as_str(), st.failed[0].archive.as_str()),
        (PART, ARCHIVES[0])
    );
    assert!(!dir.manifest().parts.contains_key(PART));
}

/// openspec AS.364: the card game's part, by the screens' rule: the two
/// archives the dump holds land raw at their stems' entries and the part
/// records them, the one it lacks named; the screens' part is untouched,
/// and a second run keeps the card part, writing nothing.
#[test]
fn the_card_games_entries_land_by_the_screens_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |usr| {
        let bld = IgaBuilder::new().lzma("level.bld", &entry()).build();
        for a in &CARD_GAME[..2] {
            fs::write(usr.join(a), &bld).unwrap();
        }
    });
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    card_part(&mut dir, &usr, &mut st).unwrap();
    assert!(st.cards.read && !st.cards.kept, "{:?}", st.failed);
    assert_eq!((st.cards.entries, st.cards.files.written), (2, 2));
    assert_eq!(
        st.cards.missing,
        [(CARD_GAME[2].to_owned(), "not on the dump".to_owned())]
    );
    let rels: Vec<String> = CARD_GAME[..2].iter().map(|a| path(a)).collect();
    assert_eq!(dir.manifest().parts[CARD_GAME_PART], rels);
    for rel in &rels {
        assert_eq!(fs::read(dir.path().join(rel)).unwrap(), entry(), "raw");
    }
    assert!(!dir.manifest().parts.contains_key(PART), "the screens' own");
    let mut again = ScriptStage::default();
    card_part(&mut dir, &usr, &mut again).unwrap();
    assert!(again.cards.kept && !again.cards.read);
    assert_eq!((again.cards.files.written, again.cards.files.kept), (0, 2));
}
