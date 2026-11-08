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
    // The list off the manifest on disk (openspec AS.15) is the same; a
    // folder holding none says why.
    assert_eq!(levels::on_disc_at(dir.path()).unwrap(), list);
    let e = levels::on_disc_at(tmp.path()).unwrap_err();
    assert!(e.contains("manifest.json"), "{e}");

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

/// A level stage stopped after a part's textures reached the disc but
/// before the manifest's one save for them (openspec AS.26: a part's new
/// textures are recorded together, then its IR, then the part): on the next
/// start that part alone is read again, its textures written again, and the
/// extract comes out as an uninterrupted one, records and bytes alike.
#[test]
fn a_part_stopped_before_its_textures_were_recorded_resumes_to_the_same_extract() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let known = known_for(&dump);
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    stages::level_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    let path = opened.dir.path().to_path_buf();
    let whole = (opened.dir.manifest().clone(), files(&path));
    let scene = whole.0.levels[FIRST_RUN[0]].parts[levels::SCENE].clone();
    let textures: Vec<&String> = scene
        .iter()
        .filter(|f| f.starts_with("textures/"))
        .collect();
    assert!(!textures.is_empty(), "{scene:?}");
    // What the stop leaves: the scene's files on disc, neither they nor the
    // part recorded.
    let manifest = path.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    let parts = json["levels"][FIRST_RUN[0]]["parts"]
        .as_object_mut()
        .unwrap();
    assert!(parts.remove(levels::SCENE).is_some());
    let outputs = json["outputs"].as_array_mut().unwrap();
    let before = outputs.len();
    outputs.retain(|o| !scene.iter().any(|f| o["path"] == f.as_str()));
    assert_eq!(outputs.len(), before - scene.len());
    fs::write(&manifest, serde_json::to_string(&json).unwrap()).unwrap();
    drop(opened);

    let mut again = open(&dump, &root, &known, &|_| {}).unwrap();
    let done = stages::level_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
    // The scene, and any part naming one of its textures, whose files then
    // aren't all as recorded either.
    let touched = whole.0.levels[FIRST_RUN[0]]
        .parts
        .iter()
        .filter(|(_, fs)| fs.iter().any(|f| scene.contains(f)))
        .count();
    assert!(touched >= 1);
    assert_eq!(
        (done.parts_read, done.parts_kept),
        (touched, PARTS.len() - touched)
    );
    assert_eq!(done.files.written, scene.len());
    assert!(levels::complete(again.dir.manifest()));
    let resumed = (again.dir.manifest().clone(), files(&path));
    assert_eq!(resumed.0.outputs, whole.0.outputs);
    assert_eq!(resumed.0.levels, whole.0.levels);
    let data = |f: &[(String, u64)]| -> Vec<(String, u64)> {
        f.iter()
            .filter(|(p, _)| p != "manifest.json")
            .cloned()
            .collect()
    };
    assert_eq!(data(&resumed.1), data(&whole.1));
    for f in scene {
        assert!(again.dir.output_done(&f).unwrap(), "{f}");
    }
}

/// AS.15's review: what a script's level change can name, by its archive's
/// kind: a level a player is taken into alone, never what a level streams
/// in (the voice pack, read in English off `global.bld`, a ship part, a mesh
/// bundle, a pack) or a menu loads (the title, the credits).
#[test]
fn a_level_change_names_a_level_a_player_is_taken_into() {
    for (name, level) in [
        ("level_214_tutotial_2", true),
        ("level_hub_2_stage_1", true),
        ("pvp_level_001_cubedungeon", true),
        ("challenge_level_000", true),
        ("arena_challenge_200_000", true),
        ("challengevo", false),
        ("ship_part_003", false),
        ("arena_mesh_200", false),
        ("challengemesh_ice", false),
        ("_minions0", false),
        ("_misc0", false),
        ("title", false),
        ("credits_giants", false),
    ] {
        assert_eq!(Kind::of(name).is_level(), level, "{name}");
    }
}

/// openspec AS.157: the files a hub stage streams in (its ship parts)
/// extract in one pass, each its streamed parts alone and no sounds,
/// recorded under its own name beside the first run's chapter 1, which
/// stays complete; a second pass keeps them all; a list naming one file the
/// dump doesn't hold is refused with nothing read or written.
#[test]
fn a_hub_stages_streams_extract_their_streamed_parts_alone() {
    const PART: &str = "ship_part_900";
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::write(
        levels::archive_path(&usr, PART),
        model::archive(&stage_level(&stage_sample())),
    )
    .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let known = known_for(&dump);
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let dir = &mut opened.dir;
    stages::level_stage(dir, IrFormat::Ron, &|_| {}).unwrap();
    assert!(
        levels::on_disc(dir.manifest())
            .iter()
            .any(|l| l.name == PART && l.kind == Kind::ShipPart)
    );

    let before = (dir.manifest().clone(), files(dir.path()));
    let e =
        stages::extract_streams(dir, &[PART, "ship_part_999"], IrFormat::Ron, &|_| {}).unwrap_err();
    assert!(matches!(e, ExtractError::NoSuchLevel { .. }), "{e}");
    assert_eq!((dir.manifest().clone(), files(dir.path())), before);

    let events = std::sync::Mutex::new(Vec::new());
    let stage = stages::extract_streams(dir, &[PART], IrFormat::Ron, &|e| {
        if let stages::Event::Level { done, total } = e {
            events.lock().unwrap().push((done, total));
        }
    })
    .unwrap();
    assert_eq!(
        (stage.parts_read, stage.levels),
        (levels::STREAMED.len(), 1)
    );
    assert!(stage.failed.is_empty(), "{:?}", stage.failed);
    assert!(stage.sounds.is_none(), "a stream reads no sounds");
    let n = levels::STREAMED.len();
    assert_eq!(events.into_inner().unwrap().last(), Some(&(n, n)));
    let m = dir.manifest();
    let mut recorded: Vec<&str> = m.levels[PART].parts.keys().map(String::as_str).collect();
    recorded.sort_unstable();
    let mut want = levels::STREAMED.to_vec();
    want.sort_unstable();
    assert_eq!(recorded, want);
    for part in levels::STREAMED {
        let rel = levels::part_path(IrFormat::Ron, PART, part);
        assert!(dir.path().join(&rel).is_file(), "{rel}");
    }
    let sounds =
        guac_extract::sounds::path(IrFormat::Ron, &guac_extract::sounds::level_archive(PART));
    assert!(!dir.path().join(&sounds).exists(), "{sounds}");
    assert!(
        levels::complete(m),
        "a stream leaves the first run complete"
    );

    let again = stages::extract_streams(dir, &[PART], IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((again.parts_read, again.parts_kept), (0, n));
}
