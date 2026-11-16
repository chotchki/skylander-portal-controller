//! The screens' part on a synthetic dump (openspec AS.53): the objectives
//! screen's archive holding a script entry built in code, then the same
//! dump without it and with one that won't unpack. The real dump's runs in
//! guacamole's `giants_level_end`, which links the entry.

use std::fs;
use std::path::{Path, PathBuf};

use guac_extract::known::KnownDump;
use guac_extract::screens::{
    ARCHIVES, CARD_GAME, CARD_GAME_PART, CARD_PALETTES_PART, CARD_STONES_PART, PART, card_stones,
    path,
};
use guac_extract::stages::{ScriptStage, card_palette_part, card_part, screen_part, stone_part};
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

/// openspec AS.366: the stones' decks and assets, a part of their own by the screens'
/// rule: the archives the dump holds land raw at their stems' entries, the
/// rest named as missing; the card game's three streams' part untouched; a
/// second run keeps it, writing nothing.
#[test]
fn the_card_stones_land_by_the_screens_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let stones = card_stones();
    let dump = dump_with(tmp.path(), |usr| {
        let bld = IgaBuilder::new().lzma("level.bld", &entry()).build();
        fs::create_dir_all(usr.join("character")).unwrap();
        for d in &stones[..3] {
            fs::write(usr.join(d), &bld).unwrap();
        }
    });
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    stone_part(&mut dir, &usr, &mut st).unwrap();
    assert!(st.stones.read && !st.stones.kept, "{:?}", st.failed);
    assert_eq!((st.stones.entries, st.stones.files.written), (3, 3));
    assert_eq!(st.stones.missing.len(), stones.len() - 3);
    let rels: Vec<String> = stones[..3].iter().map(|a| path(a)).collect();
    assert_eq!(dir.manifest().parts[CARD_STONES_PART], rels);
    for rel in &rels {
        assert_eq!(fs::read(dir.path().join(rel)).unwrap(), entry(), "raw");
    }
    assert!(!dir.manifest().parts.contains_key(CARD_GAME_PART));
    let mut again = ScriptStage::default();
    stone_part(&mut dir, &usr, &mut again).unwrap();
    assert!(again.stones.kept && !again.stones.read);
    assert_eq!(
        (again.stones.files.written, again.stones.files.kept),
        (0, 3)
    );
}

/// openspec AS.483: the card game's palettes, one file for every archive's
/// templates: a dump holding one of the archives writes it and records the
/// part with it, a second run keeps it, writing nothing; a dump holding none
/// records the part with no file.
#[test]
fn the_card_palettes_land_in_one_file_and_a_second_run_keeps_it() {
    use guac_extract::intros::IrFormat;
    let tmp = tempfile::tempdir().unwrap();
    let stones = card_stones();
    let dump = dump_with(tmp.path(), |usr| {
        let bld = IgaBuilder::new().lzma("level.bld", &entry()).build();
        fs::write(usr.join(&stones[0]), &bld).unwrap();
    });
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    card_palette_part(&mut dir, &usr, IrFormat::Ron, &mut st).unwrap();
    assert!(
        st.card_palettes.read && !st.card_palettes.kept,
        "{:?}",
        st.failed
    );
    assert_eq!(st.card_palettes.files.written, 1);
    assert_eq!(
        st.card_palettes.missing.len(),
        CARD_GAME.len() + stones.len() - 1
    );
    let rel = IrFormat::Ron.card_palettes_path();
    assert_eq!(dir.manifest().parts[CARD_PALETTES_PART], [rel]);
    assert!(dir.path().join(rel).is_file());
    let mut again = ScriptStage::default();
    card_palette_part(&mut dir, &usr, IrFormat::Ron, &mut again).unwrap();
    assert!(again.card_palettes.kept && !again.card_palettes.read);
    assert_eq!(
        (
            again.card_palettes.files.written,
            again.card_palettes.files.kept
        ),
        (0, 1)
    );

    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |_| {});
    let mut dir = opened(tmp.path(), &dump);
    let mut st = ScriptStage::default();
    card_palette_part(
        &mut dir,
        &dump.join("PS3_GAME/USRDIR"),
        IrFormat::Ron,
        &mut st,
    )
    .unwrap();
    assert!(st.card_palettes.read, "{:?}", st.failed);
    assert!(dir.manifest().parts[CARD_PALETTES_PART].is_empty());
}

/// A dump holding `global.bld` and `bootstrap.bld` (the sounds' exports),
/// a stone's deck whose `level.bld` places models (the levels' stage
/// sample: a static and a skinned model read, one refused) and a stone's
/// asset whose `level.bld` holds the sound builder's sample.
fn art_dump(tmp: &Path) -> PathBuf {
    use guac_formats::exports::builder::{bootstrap, names, rooted};
    let stones = card_stones();
    dump_with(tmp, |usr| {
        fs::create_dir_all(usr.join("permanent")).unwrap();
        let one = |level: &[u8]| IgaBuilder::new().lzma("level.bld", level).build();
        fs::write(usr.join("permanent/bootstrap.bld"), one(&bootstrap())).unwrap();
        fs::write(
            usr.join("permanent/global.bld"),
            one(&rooted(&names("global", 24), &[])),
        )
        .unwrap();
        let models = guac_formats::level::builder::stage_level(
            &guac_formats::level::builder::stage_sample(),
        );
        fs::write(usr.join(&stones[0]), one(&models)).unwrap();
        let sounds = guac_formats::sound::builder::sample().bytes;
        fs::write(usr.join(&stones[82]), one(&sounds)).unwrap();
    })
}

/// openspec AS.365: the card game's art by the readers the rest of the
/// extract uses: the deck's models as the levels' props (one refused, by
/// name, in the record), the asset's sounds where the TV's sound book looks
/// for its stem and its interface record in the part's own folder (the
/// deck's is the interface part's); the rest named as missing; a second
/// run keeps it, writing nothing; a dump with none records no file.
#[test]
fn the_card_art_lands_by_the_extracts_own_readers_and_a_second_run_keeps_it() {
    use guac_extract::intros::IrFormat;
    use guac_extract::screens::{
        CARD_ART_PART, CARD_ART_RECORD, CardArtRecord, card_interface_path, card_model_path,
    };
    use guac_extract::stages::card_art_part;
    let tmp = tempfile::tempdir().unwrap();
    let stones = card_stones();
    let dump = art_dump(tmp.path());
    let mut dir = opened(tmp.path(), &dump);
    let usr = dump.join("PS3_GAME/USRDIR");
    let mut st = ScriptStage::default();
    card_art_part(&mut dir, &usr, IrFormat::Ron, &mut st).unwrap();
    let art = &st.card_art;
    assert!(art.read && !art.kept, "{:?}", st.failed);
    assert_eq!(art.archives, 2);
    assert_eq!(art.missing.len(), CARD_GAME.len() + stones.len() - 2);
    assert_eq!(art.models, 2, "the gourd and the chest");
    assert_eq!(art.interfaces, 1, "the asset's alone");
    assert!(art.sounds > 0 && art.textures > 0);
    assert_eq!(
        art.refused
            .iter()
            .filter(|(what, _)| what.contains("model ") && what.contains("fairy_upgrade_gem"))
            .count(),
        1,
        "{:?}",
        art.refused
    );
    let files = &dir.manifest().parts[CARD_ART_PART];
    let model = card_model_path(IrFormat::Ron, &stones[0]);
    let iface = card_interface_path(IrFormat::Ron, &stones[82]);
    let sound = guac_extract::sounds::path(IrFormat::Ron, &stones[82]);
    for rel in [&model, &iface, &sound] {
        assert!(files.contains(rel), "{rel} in {files:?}");
        assert!(dir.path().join(rel).is_file(), "{rel}");
    }
    assert_eq!(sound, "sounds/cardgame_card_000.ron");
    assert!(!files.contains(&card_interface_path(IrFormat::Ron, &stones[0])));
    assert!(
        !files.contains(&guac_extract::sounds::path(IrFormat::Ron, &stones[0])),
        "a deck with no sound writes none"
    );
    assert!(files.iter().any(|f| f.starts_with("textures/")));
    assert!(files.iter().any(|f| f.starts_with("audio/sounds/")));
    let models: guac_ir::spawn::SpawnModels = IrFormat::Ron.decode(&dir, &model).unwrap();
    assert_eq!(models.props.len(), 2);
    let record: CardArtRecord =
        serde_json::from_slice(&fs::read(dir.path().join(CARD_ART_RECORD)).unwrap()).unwrap();
    assert_eq!(record.archives.len(), 2);
    assert_eq!(record.archives[0].archive, stones[0]);
    assert_eq!(record.archives[0].refused.len(), 1);
    assert_eq!(record.archives[1].images, Some(0));
    assert!(!record.archives[1].sounds.is_empty());

    let written = st.card_art.files.written;
    let mut again = ScriptStage::default();
    card_art_part(&mut dir, &usr, IrFormat::Ron, &mut again).unwrap();
    assert!(again.card_art.kept && !again.card_art.read);
    assert_eq!(
        (again.card_art.files.written, again.card_art.files.kept),
        (0, written)
    );

    // An interface record of another shape isn't current, though every
    // file the part names is intact: the part reads again (AT.27's rule).
    let mut stale = dir.manifest().parts[CARD_ART_PART].clone();
    let at = stale.iter().position(|f| *f == iface).unwrap();
    let old_shape = iface.replace(".v", ".v0");
    let bytes = fs::read(dir.path().join(&iface)).unwrap();
    dir.write_output(&old_shape, &bytes).unwrap();
    stale[at] = old_shape;
    dir.record_part(CARD_ART_PART, stale).unwrap();
    let mut third = ScriptStage::default();
    card_art_part(&mut dir, &usr, IrFormat::Ron, &mut third).unwrap();
    assert!(third.card_art.read && !third.card_art.kept);

    let tmp = tempfile::tempdir().unwrap();
    let dump = dump_with(tmp.path(), |_| {});
    let mut dir = opened(tmp.path(), &dump);
    let mut st = ScriptStage::default();
    card_art_part(
        &mut dir,
        &dump.join("PS3_GAME/USRDIR"),
        IrFormat::Ron,
        &mut st,
    )
    .unwrap();
    assert!(st.card_art.read, "{:?}", st.failed);
    assert!(dir.manifest().parts[CARD_ART_PART].is_empty());
}

/// An art archive that's there and won't unpack is the part's failure
/// alone, by name, and the part isn't recorded.
#[test]
fn a_card_art_archive_that_wont_read_fails_its_part_alone() {
    use guac_extract::intros::IrFormat;
    use guac_extract::screens::CARD_ART_PART;
    use guac_extract::stages::card_art_part;
    let tmp = tempfile::tempdir().unwrap();
    let stones = card_stones();
    let dump = art_dump(tmp.path());
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::write(usr.join(&stones[1]), b"not an archive").unwrap();
    let mut dir = opened(tmp.path(), &dump);
    let mut st = ScriptStage::default();
    card_art_part(&mut dir, &usr, IrFormat::Ron, &mut st).unwrap();
    let [f] = &st.failed[..] else {
        panic!("{:?}", st.failed)
    };
    assert_eq!(
        (f.part.as_str(), f.archive.as_str()),
        (CARD_ART_PART, stones[1].as_str())
    );
    assert!(!st.card_art.read);
    assert!(!dir.manifest().parts.contains_key(CARD_ART_PART));
}
