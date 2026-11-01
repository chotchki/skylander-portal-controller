//! The interface part on a synthetic dump (openspec AT.8): guac-formats'
//! interface archive as `global.bld`, a character archive with the `.arc`
//! its streamed image comes from and a menu archive with the same `.arc`
//! beside it (which isn't read), `bootstrap.bld` missing, and chapter 1's
//! archive holding the same interface for its `sprites` part. The real dump
//! runs in `giants_interface.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use guac_extract::interface::{self, PART};
use guac_extract::intros::IrFormat;
use guac_extract::known::KnownDump;
use guac_extract::{ExtractRoot, IdentityDir, levels, models, open, stages};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::interface::builder::{image, sample};
use guac_formats::language::builder::pack;
use guac_formats::sfo::builder::SfoBuilder;

fn bld() -> Vec<u8> {
    IgaBuilder::new()
        .lzma("level.bld", &sample().bytes)
        .lzma(
            "ENGLISH.pak",
            &pack(
                "ENGLISH",
                guac_formats::language::ENGLISH,
                &["Unused", "Gold"],
            ),
        )
        .build()
}

fn arc() -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let o = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);
    image(&mut b, o, blocks, "batteries_low_1.png", 0x77);
    IgaBuilder::new()
        .stored(
            "C:/tfb/build/ps3/levels/includes/portalmessaging/batteries_low_1.png/0x3e79592d.png.igb.tex.igz",
            &b.build(),
        )
        .build()
}

/// `global.bld`, `character/009_eruptor.bld` with its `.arc`,
/// `misc/ui_options.bld` with the same `.arc` and chapter 1, all the same
/// archive; no `bootstrap.bld`.
fn make_dump(tmp: &Path) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    for d in ["permanent", "character", "misc", levels::LEVEL_FOLDER] {
        fs::create_dir_all(usr.join(d)).unwrap();
    }
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    let bld = bld();
    for p in [
        "permanent/global.bld",
        "character/009_eruptor.bld",
        "misc/ui_options.bld",
    ] {
        fs::write(usr.join(p), &bld).unwrap();
    }
    fs::write(levels::archive_path(&usr, levels::FIRST_RUN[0]), &bld).unwrap();
    fs::write(usr.join("character/009_eruptor.arc"), arc()).unwrap();
    fs::write(usr.join("misc/ui_options.arc"), arc()).unwrap();
    dump
}

struct Setup {
    _tmp: tempfile::TempDir,
    dump: PathBuf,
    root: ExtractRoot,
    known: Vec<KnownDump>,
}

fn setup() -> Setup {
    setup_with(|_| {})
}

/// [`setup`] with `edit` run on the dump's `USRDIR` before it's scanned.
fn setup_with(edit: impl FnOnce(&Path)) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    edit(&dump.join("PS3_GAME/USRDIR"));
    let scan = guac_extract::dump::scan(&dump, &Default::default(), &|_| {}).unwrap();
    let known = vec![KnownDump {
        title_id: "BLUS30968",
        app_ver: "01.00",
        manifest_hash: Box::leak(scan.identity.manifest_hash.to_string().into_boxed_str()),
        source: "test",
    }];
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    Setup {
        _tmp: tmp,
        dump,
        root,
        known,
    }
}

fn textures(dir: &IdentityDir) -> Vec<String> {
    dir.manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with(models::TEXTURE_DIR))
        .map(|o| o.path.clone())
        .collect()
}

#[test]
fn the_part_lands_each_archives_interface_and_names_what_the_dump_lacks() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(
        interface::archives(opened.dir.manifest()),
        vec![
            "permanent/global.bld",
            "permanent/bootstrap.bld",
            "character/009_eruptor.bld",
            "misc/ui_options.bld",
        ]
    );
    let st = interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    assert!(
        st.read && !st.kept && st.failed.is_none(),
        "{:?}",
        st.failed
    );
    assert_eq!(st.missing, vec!["permanent/bootstrap.bld".to_owned()]);
    assert_eq!(st.archives, 3);
    assert_eq!((st.templates, st.fonts, st.strings), (3 * 8, 3, 3 * 2));
    // The local image and the atlas are the same three times over; only
    // the character archive has the .arc its streamed image comes from.
    assert_eq!(st.textures, 3);
    assert_eq!((st.texture_files.written, st.texture_files.kept), (3, 0));
    // global.bld has no .arc and the menu archive's isn't read: their
    // streamed image keeps no texture and isn't refused.
    assert!(st.refused.is_empty(), "{:?}", st.refused);
    assert_eq!(textures(&opened.dir).len(), 3);
    let part = &opened.dir.manifest().parts[PART];
    assert_eq!(part.len(), 3 + 3, "the IR files and the textures");
    for f in part {
        assert!(opened.dir.output_done(f).unwrap(), "{f}");
    }
    // The bytes it reports are the part's files' as recorded.
    let recorded: u64 = opened
        .dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| part.contains(&o.path))
        .map(|o| o.size)
        .sum();
    assert_eq!(st.files.bytes, recorded);
    let menu = interface::path(IrFormat::Ron, "misc/ui_options.bld");
    let menu: guac_ir::interface::Interface = IrFormat::Ron.decode(&opened.dir, &menu).unwrap();
    match &menu.templates[2].content {
        guac_ir::interface::Content::Image(a) => assert_eq!(a.texture, None, "no .arc read"),
        other => panic!("{other:?}"),
    }
    let rel = interface::path(IrFormat::Ron, "character/009_eruptor.bld");
    let ir: guac_ir::interface::Interface = IrFormat::Ron.decode(&opened.dir, &rel).unwrap();
    assert_eq!(ir.templates.len(), 8);
    assert_eq!(ir.strings[1], "Gold");
    for t in &ir.templates {
        if let guac_ir::interface::Content::Image(a) = &t.content {
            assert!(
                a.texture.as_ref().is_some_and(|p| part.contains(p)),
                "{}",
                t.name
            );
        }
    }
}

#[test]
fn a_second_run_keeps_the_part() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    let again = interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    assert!(again.kept && !again.read);
    assert_eq!(again.files.written, 0);
    assert_eq!(again.files.kept, 3 + 3);
}

/// An archive on the dump that won't read refuses the part alone: named in
/// `failed`, the part left off the manifest (so the extract isn't current
/// and the next start reads it again), what read before it kept for then.
#[test]
fn an_archive_that_wont_read_leaves_the_part_unrecorded() {
    let broken = IgaBuilder::new().lzma("level.bld", b"not an igz").build();
    let s = setup_with(|usr| fs::write(usr.join("character/009_eruptor.bld"), &broken).unwrap());
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let st = interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    let (archive, why) = st.failed.clone().expect("the broken archive named");
    assert_eq!(archive, "character/009_eruptor.bld", "{why}");
    assert!(!st.read && !st.kept);
    assert!(!opened.dir.manifest().parts.contains_key(PART));
    let global = interface::path(IrFormat::Ron, "permanent/global.bld");
    assert!(
        opened.dir.output_done(&global).unwrap(),
        "global.bld's kept"
    );
    // The next run reads the part again and fails the same way, keeping
    // what's in.
    let again = interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    assert_eq!(again.failed.map(|(a, _)| a), Some(archive));
    assert_eq!((again.files.written, again.texture_files.written), (0, 0));
    assert!(!opened.dir.manifest().parts.contains_key(PART));
}

/// An extract made before the part: its record gone and one texture gone
/// with its record. The next run reads the part alone, writing that one
/// texture again and keeping every other file already in.
#[test]
fn an_extract_from_before_the_part_reads_it_alone() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let files: Vec<String> = json["parts"]
        .as_object_mut()
        .unwrap()
        .remove(PART)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_owned())
        .collect();
    let lost = files
        .iter()
        .find(|f| f.starts_with(models::TEXTURE_DIR))
        .unwrap()
        .clone();
    json["outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|o| o["path"] != lost.as_str());
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    fs::remove_file(dir.join(&lost)).unwrap();

    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert!(!opened.dir.manifest().parts.contains_key(PART));
    let st = interface::interface_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    assert!(st.read);
    assert_eq!(
        (st.texture_files.written, st.texture_files.kept),
        (1, 2),
        "only the lost texture"
    );
    assert_eq!(st.files.written, 1, "the IR files kept as they were");
    assert!(opened.dir.output_done(&lost).unwrap());
}

/// A level's `sprites` part rides the level stage: chapter 1's archive here
/// is the interface sample and its English pack (the other parts read it as
/// an empty level), and its sprites land with the level's text and images.
#[test]
fn a_levels_sprites_part_reads_with_the_level_stage() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let lv = stages::level_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    assert!(lv.failed.is_empty(), "{:?}", lv.failed);
    assert_eq!(lv.parts_read, levels::PARTS.len());
    let level = levels::FIRST_RUN[0];
    let rel = IrFormat::Ron.level_path(level, levels::SPRITES);
    let ir: guac_ir::interface::Interface = IrFormat::Ron.decode(&opened.dir, &rel).unwrap();
    assert_eq!(ir.archive, format!("level/{level}.bld"));
    assert_eq!(ir.templates.len(), 8);
    assert_eq!(ir.strings, vec!["Unused".to_owned(), "Gold".to_owned()]);
    // The local image and the atlas, recorded with the part; a level's
    // streamed image (its .arc isn't read) keeps none.
    let recorded = &opened.dir.manifest().levels[level].parts[levels::SPRITES];
    let mut named = vec![ir.fonts[0].atlas.clone()];
    for t in &ir.templates {
        if let guac_ir::interface::Content::Image(a) = &t.content
            && let Some(p) = &a.texture
        {
            named.push(p.clone());
        }
    }
    assert_eq!(named.len(), 2, "{named:?}");
    for p in named.iter().chain([&rel]) {
        assert!(recorded.contains(p), "{p}");
        assert!(opened.dir.output_done(p).unwrap(), "{p}");
    }
}
