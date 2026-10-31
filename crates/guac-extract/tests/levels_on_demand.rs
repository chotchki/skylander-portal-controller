//! Levels on demand (openspec AS.1, design decisions 1 and 2) on a
//! synthetic dump: chapter 1 and a PvP arena built by the level builder, a
//! file under `level/` that isn't a level archive, and a PARAM.SFO. The
//! level list comes off the dump scan; a first run's level stage writes
//! chapter 1 alone; any other level on the list extracts by name beside it,
//! resumes alone, reports a part that won't read and leaves the first run's
//! completeness as it was; a name off the list is refused with nothing
//! written.

use std::fs;
use std::path::{Path, PathBuf};

use guac_extract::intros::IrFormat;
use guac_extract::known::KnownDump;
use guac_extract::levels::{self, FIRST_RUN, Kind, PARTS};
use guac_extract::stages;
use guac_extract::{ExtractError, ExtractRoot, open};
use guac_formats::level::builder::{stage_level, stage_sample};
use guac_formats::model::builder as model;
use guac_formats::sfo::builder::SfoBuilder;

const ARENA: &str = "pvp_level_900_test";

fn make_dump(tmp: &Path) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::create_dir_all(usr.join(levels::LEVEL_FOLDER)).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    let chapter_1 = model::archive(&stage_level(&stage_sample()));
    fs::write(levels::archive_path(&usr, FIRST_RUN[0]), &chapter_1).unwrap();
    // The arena: the sample with a material asking for bit 9, which no
    // reader takes, so its scene part alone won't read.
    let mut spec = stage_sample();
    spec.scene[0].pixel = Some((0x21C, 0x3FF));
    fs::write(
        levels::archive_path(&usr, ARENA),
        model::archive(&stage_level(&spec)),
    )
    .unwrap();
    fs::write(usr.join("level/readme.txt"), b"not a level").unwrap();
    dump
}

fn known_for(dump: &Path) -> Vec<KnownDump> {
    let scan = guac_extract::dump::scan(dump, &Default::default(), &|_| {}).unwrap();
    vec![KnownDump {
        title_id: "BLUS30968",
        app_ver: "01.00",
        manifest_hash: Box::leak(scan.identity.manifest_hash.to_string().into_boxed_str()),
        source: "test",
    }]
}

#[test]
fn any_level_on_the_disc_extracts_on_demand_beside_the_first_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let known = known_for(&dump);
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let dir = &mut opened.dir;

    // The list: the two archives, by kind, sizes as scanned; the text file
    // isn't one.
    let list = levels::on_disc(dir.manifest());
    let names: Vec<(&str, Kind)> = list.iter().map(|l| (l.name.as_str(), l.kind)).collect();
    assert_eq!(
        names,
        [(FIRST_RUN[0], Kind::Story), (ARENA, Kind::Pvp)],
        "{list:?}"
    );
    assert!(list.iter().all(|l| l.bytes > 0));

    // A first run's stage: chapter 1 alone, whole.
    let first = stages::level_stage(dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((first.levels, first.parts_read), (1, PARTS.len()));
    assert!(first.failed.is_empty(), "{:?}", first.failed);
    let m = dir.manifest();
    assert_eq!(m.levels.keys().collect::<Vec<_>>(), [FIRST_RUN[0]]);
    assert!(levels::complete(m));

    // A name off the list: refused, nothing read or written.
    let before = (dir.manifest().clone(), files(dir.path()));
    let e = stages::extract_level(dir, "level_999", IrFormat::Ron, &|_| {}).unwrap_err();
    assert!(matches!(e, ExtractError::NoSuchLevel { .. }), "{e}");
    assert_eq!(e.code(), "GUAC-E-NO-SUCH-LEVEL");
    assert_eq!((dir.manifest().clone(), files(dir.path())), before);

    // The arena on demand: its parts beside chapter 1's, recorded under its
    // own name, its scene reported and the rest landing; chapter 1 kept.
    let events = std::sync::Mutex::new(Vec::new());
    let arena = stages::extract_level(dir, ARENA, IrFormat::Ron, &|e| {
        if let stages::Event::Level { done, total } = e {
            events.lock().unwrap().push((done, total));
        }
    })
    .unwrap();
    assert_eq!(arena.parts_read, PARTS.len() - 1);
    assert_eq!(arena.levels, 0, "a part failed, so the arena isn't whole");
    let [failed] = &arena.failed[..] else {
        panic!("{:?}", arena.failed);
    };
    assert_eq!(
        (failed.level.as_str(), failed.part.as_str()),
        (ARENA, levels::SCENE)
    );
    assert_eq!(
        events.into_inner().unwrap().last(),
        Some(&(PARTS.len(), PARTS.len()))
    );
    let m = dir.manifest();
    assert_eq!(m.levels.keys().collect::<Vec<_>>(), [FIRST_RUN[0], ARENA]);
    assert_eq!(m.levels[ARENA].parts.len(), PARTS.len() - 1);
    assert!(!m.levels[ARENA].parts.contains_key(levels::SCENE));
    for part in PARTS.iter().filter(|p| **p != levels::SCENE) {
        let rel = levels::part_path(IrFormat::Ron, ARENA, part);
        assert!(dir.path().join(&rel).is_file(), "{rel}");
    }
    assert!(
        levels::complete(m),
        "an on-demand level leaves the first run complete"
    );

    // Resumed: one part lost and one cut short after its record, as a crash
    // before an unflushed write reached the disc leaves it (openspec AS.4):
    // those two alone are read again, each whole again; the failing scene
    // tried again and reported again; chapter 1 untouched.
    let lights = levels::part_path(IrFormat::Ron, ARENA, levels::LIGHTS);
    let collision = levels::part_path(IrFormat::Ron, ARENA, levels::COLLISION);
    let whole = fs::read(dir.path().join(&collision)).unwrap();
    fs::remove_file(dir.path().join(&lights)).unwrap();
    fs::write(dir.path().join(&collision), &whole[..whole.len() / 2]).unwrap();
    let again = stages::extract_level(dir, ARENA, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((again.parts_read, again.parts_kept), (2, PARTS.len() - 3));
    assert_eq!(again.failed.len(), 1);
    assert!(dir.path().join(&lights).is_file());
    assert_eq!(fs::read(dir.path().join(&collision)).unwrap(), whole);
    let kept = stages::level_stage(dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((kept.parts_read, kept.parts_kept), (0, PARTS.len()));
}

/// Every file under `dir`, relative, with its size.
fn files(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                out.push((rel, p.metadata().unwrap().len()));
            }
        }
    }
    out.sort();
    out
}
