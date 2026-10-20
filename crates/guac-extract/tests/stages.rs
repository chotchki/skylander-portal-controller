//! The extraction's stages in order (openspec AE.3, AE.6, AG.14, AG.15,
//! AH.2) on synthetic dumps: a PARAM.SFO, the roster builder's four archives
//! (the progression tables in two of them), the
//! model builder's character archive under nine of the roster's eleven
//! character load files (one of them without a hero, one more refused for a
//! gloss-map bit, one missing), the level builder's chapter 1, two
//! voice-over archives and two movies, transcoded by a fake ffmpeg (a shell
//! script that copies its input into each output). Unix only, as AB's
//! transcode tests; the real dump runs in `giants_stages.rs`,
//! `giants_model_stage.rs` and `giants_level_stage.rs`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use guac_extract::intros::IrFormat;
use guac_extract::known::KnownDump;
use guac_extract::levels::{self, LEVELS, PARTS};
use guac_extract::stages::{self, Event, Files};
use guac_extract::{DirState, ExtractRoot, IR_VERSION, LAYOUT_VERSION, Manifest, Stale, open};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::level::builder::{StageSpec, stage_level, stage_sample};
use guac_formats::model::builder as model;
use guac_formats::roster::builder::{DiscSpec, disc, sample};
use guac_formats::sfo::builder::SfoBuilder;
use guac_ir::{IntroSource, Roster};

/// The sample roster's figure count, each with a portrait.
const FIGURES: usize = 24;
/// Its character load files, and those [`make_dump`] gives a good model:
/// all but a missing one and one whose body asks for bit 6.
const LOADS: usize = 11;
const MODELS: usize = 9;
const MISSING: &str = "MinionLoad_016_SpyroDark";
const REFUSED: &str = "MinionLoad_112_TitanLifeAlt";
/// Of the nine, the one whose archive holds no actor: its model extracts and
/// its hero doesn't settle.
const HEROES: usize = 8;
const HEROLESS: &str = "MinionLoad_106_IceSorcLegendary";
/// The model builder's sample binds eight textures, five distinct.
const TEXTURES: usize = 5;
/// The level builder's sample: eight scene textures, the props' five (the
/// chest is the model builder's sample, the gourd its colour map), one
/// shared between them: twelve, of which the characters' store already
/// holds five.
const LEVEL_TEXTURES: usize = 12;
const LEVEL_SHARED: usize = 5;
/// A level's events: none of its five parts done, then each.
const LEVEL_EVENTS: usize = 6;
/// The progression builder's sample level table.
const SAMPLE_LEVELS: [u32; 5] = [0, 400, 1100, 2500, 4500];

const ENCODERS: &str = "Encoders:\\n ------\\n V....D libx264  x\\n A....D libmp3lame  y\\n";
const DECODERS: &str = "Decoders:\\n ------\\n V....D binkvideo  x\\n A....D binkaudio_dct  y\\n A....D binkaudio_rdft  z\\n";

fn fake_ffmpeg(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let script = format!(
        r#"#!/bin/sh
case "$1" in
  -version) echo "ffmpeg version fake"; exit 0;;
  -hide_banner)
    case "$2" in
      -encoders) printf '{ENCODERS}'; exit 0;;
      -decoders) printf '{DECODERS}'; exit 0;;
    esac;;
esac
prev=''; in=''
for a in "$@"; do
  if [ "$prev" = "-i" ]; then in="${{a#file:}}"; fi
  prev="$a"
done
prev=''
for a in "$@"; do
  case "$a" in file:*) if [ "$prev" != "-i" ]; then cat "$in" > "${{a#file:}}"; fi;; esac
  prev="$a"
done
printf 'frame=10\nout_time_us=1000000\nprogress=end\n'
"#
    );
    let p = dir.join("ffmpeg");
    fs::write(&p, script).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// The roster builder's sample disc, Eruptor (toy 9) by his voice-over and
/// Jet-Vac (toy 100) by the roster's name, with chapter 1.
fn make_dump(tmp: &Path) -> PathBuf {
    make_dump_with(tmp, &stage_sample())
}

fn make_dump_with(tmp: &Path, level: &StageSpec) -> PathBuf {
    make_dump_from(tmp, &sample(), level)
}

fn make_dump_from(tmp: &Path, roster: &DiscSpec, level: &StageSpec) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::create_dir_all(usr.join("movies")).unwrap();
    fs::create_dir_all(usr.join("character")).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    for (path, bytes) in disc(roster).archives() {
        let path = usr.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let vo = "champion_audio/eruptor/vo_eruptor_01.wav/0.wav.hz.wav.enc";
    let eruptor = IgaBuilder::new().stored(vo, b"wav").build();
    fs::write(usr.join("character/009_eruptor.arc"), eruptor).unwrap();
    let eagle = IgaBuilder::new().stored("model.igz", b"igz").build();
    fs::write(usr.join("character/100_eagle.arc"), eagle).unwrap();
    let good = model::archive(&model::character(&model::sample()));
    let mut bad = model::sample();
    bad.materials[0].pixel = Some((0x5C, 0x2FF));
    let bad = model::archive(&model::character(&bad));
    let mut heroless = model::sample();
    heroless.actors.clear();
    let heroless = model::archive(&model::character(&heroless));
    for load in [
        "MinionLoad_000_AirDragon",
        "MinionLoad_000_AirDragonLegendary",
        "MinionLoad_007_GemGolem",
        "MinionLoad_016_Spyro",
        "MinionLoad_100_Eagle",
        "MinionLoad_100_EagleLegendary",
        "MinionLoad_106_IceSorc",
        HEROLESS,
        "MinionLoad_112_TitanLife",
    ] {
        let path = usr.join(format!("character/{}.bld", load.to_ascii_lowercase()));
        let archive = if load == HEROLESS { &heroless } else { &good };
        fs::write(path, archive).unwrap();
    }
    let path = usr.join(format!("character/{}.bld", REFUSED.to_ascii_lowercase()));
    fs::write(path, &bad).unwrap();
    fs::write(usr.join("movies/eruptor.mov"), b"eruptor bink").unwrap();
    fs::write(usr.join("movies/jetvac.mov"), b"jetvac bink").unwrap();
    let path = levels::archive_path(&usr, LEVELS[0]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, model::archive(&stage_level(level))).unwrap();
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

/// One event, as a test reads it back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    ReadingRoster,
    Portraits(usize, usize),
    RosterReady(usize),
    /// The level table.
    ProgressionReady(Vec<u32>),
    Models(usize, usize),
    ModelsReady(Vec<String>),
    Level(usize, usize),
    Indexing,
    Transcode,
}

impl Seen {
    fn of(e: &Event) -> Self {
        match e {
            Event::ReadingRoster => Self::ReadingRoster,
            Event::Portraits { done, total } => Self::Portraits(*done, *total),
            Event::RosterReady(r) => Self::RosterReady(r.figures.len()),
            Event::ProgressionReady(p) => Self::ProgressionReady(p.level_xp.clone()),
            Event::Models { done, total } => Self::Models(*done, *total),
            Event::ModelsReady { failed } => {
                Self::ModelsReady(failed.iter().map(|f| f.load_file.clone()).collect())
            }
            Event::Level { done, total } => Self::Level(*done, *total),
            Event::Indexing => Self::Indexing,
            Event::Transcode(_) => Self::Transcode,
        }
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    dump: PathBuf,
    root: ExtractRoot,
    known: Vec<KnownDump>,
    ffmpeg: PathBuf,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
    Setup {
        _tmp: tmp,
        dump,
        root,
        known,
        ffmpeg,
    }
}

fn manifest_on_disk(dir: &Path) -> Manifest {
    serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap()
}

fn recorded(m: &Manifest, prefix: &str) -> usize {
    m.outputs
        .iter()
        .filter(|o| o.path.starts_with(prefix))
        .count()
}

#[test]
fn the_roster_is_written_before_any_intro_work_starts() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(opened.state, DirState::Fresh);
    let dir = opened.dir.path().to_path_buf();
    let seen = Mutex::new(Vec::new());
    // What the manifest on disk held when the intros started.
    let at_indexing = Mutex::new(None);
    let done = stages::run(&mut opened.dir, Some(&s.ffmpeg), IrFormat::Ron, &|e| {
        if matches!(e, Event::Indexing) {
            *at_indexing.lock().unwrap() = Some(manifest_on_disk(&dir));
        }
        seen.lock().unwrap().push(Seen::of(&e));
    })
    .unwrap();

    let seen = seen.into_inner().unwrap();
    let ready = seen.iter().position(|e| *e == Seen::RosterReady(FIGURES));
    let indexing = seen.iter().position(|e| *e == Seen::Indexing);
    let first_transcode = seen.iter().position(|e| *e == Seen::Transcode);
    assert_eq!(seen[0], Seen::ReadingRoster);
    assert_eq!(seen[1], Seen::Portraits(0, FIGURES));
    assert_eq!(seen[FIGURES + 1], Seen::Portraits(FIGURES, FIGURES));
    assert_eq!(ready, Some(FIGURES + 2));
    // The progression tables next, then the models, one event each, the
    // turntable's cue with what didn't extract, then the level's parts,
    // before the intros.
    assert_eq!(
        seen[FIGURES + 3],
        Seen::ProgressionReady(SAMPLE_LEVELS.into())
    );
    let models: Vec<Seen> = (0..=LOADS).map(|i| Seen::Models(i, LOADS)).collect();
    assert_eq!(seen[FIGURES + 4..FIGURES + 5 + LOADS], models[..]);
    assert_eq!(
        seen[FIGURES + 5 + LOADS],
        Seen::ModelsReady(vec![MISSING.into(), REFUSED.into()])
    );
    let level: Vec<Seen> = (0..LEVEL_EVENTS).map(|i| Seen::Level(i, 5)).collect();
    let at = FIGURES + 6 + LOADS;
    assert_eq!(seen[at..at + LEVEL_EVENTS], level[..]);
    assert_eq!(indexing, Some(at + LEVEL_EVENTS));
    assert!(first_transcode > indexing, "{seen:?}");

    // The roster's IR, every portrait, the models, their textures, the
    // heroes and the rig were recorded before the intro index.
    let m = at_indexing
        .into_inner()
        .unwrap()
        .expect("the intros started");
    assert_eq!(recorded(&m, "ir/roster.ron"), 1);
    assert_eq!(recorded(&m, "ir/progression.ron"), 1);
    assert!(guac_extract::progression::complete(&m));
    assert_eq!(recorded(&m, "portraits/"), FIGURES);
    assert_eq!(recorded(&m, "models/"), MODELS);
    assert_eq!(
        recorded(&m, "textures/"),
        TEXTURES + LEVEL_TEXTURES - LEVEL_SHARED
    );
    assert_eq!(recorded(&m, "heroes/"), HEROES);
    assert_eq!(recorded(&m, "ir/rig.ron"), 1);
    assert_eq!(recorded(&m, "levels/"), PARTS.len());
    assert!(levels::complete(&m), "every part recorded before ffmpeg");
    assert_eq!(recorded(&m, "ir/characters"), 0);
    assert_eq!(recorded(&m, "video/"), 0);
    assert!(!m.complete);
    let p = &done.progression;
    let tables = p.progression.as_ref().unwrap();
    assert!(p.read);
    assert_eq!((p.files.written, p.files.kept), (1, 0));
    assert_eq!(
        (tables.level_xp.as_slice(), tables.gold_cap),
        (&SAMPLE_LEVELS[..], 9000)
    );
    let on_disk: guac_ir::progression::Progression =
        guac_ir::from_ron(&fs::read_to_string(dir.join("ir/progression.ron")).unwrap()).unwrap();
    assert_eq!(on_disk, **tables);
    let lv = &done.levels;
    assert_eq!((lv.levels, lv.parts_read, lv.parts_kept), (1, 5, 0));
    assert!(lv.failed.is_empty(), "{:?}", lv.failed);
    assert_eq!((lv.props, lv.refused_props.len()), (2, 1));
    assert_eq!((lv.textures, lv.shared), (LEVEL_TEXTURES, LEVEL_SHARED));
    assert_eq!(
        (lv.files.written, lv.files.kept),
        (PARTS.len() + LEVEL_TEXTURES - LEVEL_SHARED, LEVEL_SHARED)
    );
    let scene: guac_ir::level::Scene = guac_ir::from_ron(
        &fs::read_to_string(dir.join("levels/level_213_tutotial/scene.ron")).unwrap(),
    )
    .unwrap();
    assert_eq!(scene.scenes[0].segments.len(), 5);

    // Nine models; the one asking for bit 6 and the missing one reported.
    let md = &done.models;
    assert_eq!(
        (md.models, md.textures, md.binds),
        (MODELS, TEXTURES, 8 * MODELS)
    );
    let failed: Vec<&str> = md.failed.iter().map(|f| f.load_file.as_str()).collect();
    assert_eq!(failed, [MISSING, REFUSED]);
    assert!(
        md.failed[0].reason.contains("no archive"),
        "{:?}",
        md.failed[0]
    );
    assert!(
        md.failed[1].reason.contains("layer4UsesPositions"),
        "{:?}",
        md.failed[1]
    );
    assert!(
        md.failed
            .iter()
            .all(|f| f.reason.starts_with("GUAC-E-MODEL-UNREADABLE"))
    );
    // Eight heroes; the one with no actor reported, its model kept.
    assert_eq!(md.heroes, HEROES);
    let unsettled: Vec<&str> = md.unsettled.iter().map(|f| f.load_file.as_str()).collect();
    assert_eq!(unsettled, [HEROLESS]);
    assert!(
        md.unsettled[0]
            .reason
            .starts_with("GUAC-E-HERO-UNSETTLED: MinionLoad_106_IceSorcLegendary's hero"),
        "{:?}",
        md.unsettled[0]
    );
    assert!(dir.join(format!("models/{HEROLESS}.ron")).is_file());
    assert!(!dir.join(format!("heroes/{HEROLESS}.ron")).exists());
    assert!(md.heroes_took <= md.took);
    let hero: guac_ir::Hero = guac_ir::from_ron(
        &fs::read_to_string(dir.join("heroes/MinionLoad_100_Eagle.ron")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (hero.load_file.as_str(), hero.actor.as_str()),
        ("MinionLoad_100_Eagle", "009_Eruptor")
    );
    let walk = hero.locomotion.walk.as_ref().unwrap();
    assert_eq!((walk.slot, walk.clip.name.as_str()), (1, "Eruptor_Walk.ma"));
    let eagle: guac_ir::Model = guac_ir::from_ron(
        &fs::read_to_string(dir.join("models/MinionLoad_100_Eagle.ron")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (eagle.load_file.as_str(), eagle.vertices()),
        ("MinionLoad_100_Eagle", 8)
    );
    for slot in eagle.materials.iter().flat_map(|m| &m.textures) {
        assert!(dir.join(&slot.texture).is_file(), "{}", slot.texture);
    }
    let rig: guac_ir::Rig =
        guac_ir::from_ron(&fs::read_to_string(dir.join("ir/rig.ron")).unwrap()).unwrap();
    assert_eq!(rig.lights.len(), 3);

    // What landed: the roster reads back, the intros by disc and by the
    // roster's name, the manifest marked.
    let roster: Roster =
        guac_ir::from_ron(&fs::read_to_string(dir.join("ir/roster.ron")).unwrap()).unwrap();
    assert_eq!(roster, *done.roster.roster);
    assert_eq!(roster.figures.len(), FIGURES);
    for f in &roster.figures {
        let png = fs::read(dir.join(f.portrait.as_deref().unwrap())).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{}", f.id());
    }
    assert_eq!(
        done.roster.files,
        Files {
            written: FIGURES + 1,
            kept: 0,
            bytes: done.roster.files.bytes
        }
    );
    let intros = done.intros.unwrap();
    let c = &intros.index.table.characters;
    assert_eq!(c[&9].intro.as_ref().unwrap().source, IntroSource::Disc);
    assert_eq!(
        c[&100].intro.as_ref().unwrap().source,
        IntroSource::NameFallback
    );
    assert_eq!(intros.summary.transcoded, 2);
    assert!(manifest_on_disk(&dir).complete);
    assert_eq!(
        open(&s.dump, &s.root, &s.known, &|_| {}).unwrap().state,
        DirState::Current
    );
}

#[test]
fn without_ffmpeg_the_roster_and_portraits_land_and_the_intros_are_unavailable() {
    let s = setup();
    let missing = s.ffmpeg.with_file_name("missing-ffmpeg");
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    let seen = Mutex::new(Vec::new());
    let done = stages::run(&mut opened.dir, Some(&missing), IrFormat::Ron, &|e| {
        seen.lock().unwrap().push(Seen::of(&e));
    })
    .unwrap();

    let e = done.intros.unwrap_err();
    assert_eq!(e.code(), "GUAC-E-NO-FFMPEG", "{e}");
    assert_eq!(done.roster.roster.figures.len(), FIGURES);
    let seen = seen.into_inner().unwrap();
    // The models, the turntable's cue, then the level: nothing after it.
    let at = seen.len() - LEVEL_EVENTS;
    assert_eq!(seen[at - 2], Seen::Models(LOADS, LOADS));
    assert_eq!(
        seen[at - 1],
        Seen::ModelsReady(vec![MISSING.into(), REFUSED.into()])
    );
    assert_eq!(seen[at], Seen::Level(0, 5));
    assert_eq!(seen.last(), Some(&Seen::Level(5, 5)));
    let m = manifest_on_disk(&dir);
    assert_eq!(recorded(&m, "ir/roster.ron"), 1);
    assert_eq!(recorded(&m, "portraits/"), FIGURES);
    assert_eq!(recorded(&m, "models/"), MODELS);
    assert_eq!(recorded(&m, "heroes/"), HEROES);
    // No ffmpeg costs the intros alone: the level is written and its
    // entry complete.
    assert_eq!(recorded(&m, "levels/"), PARTS.len());
    assert!(levels::complete(&m));
    for part in PARTS {
        let files = &m.levels[LEVELS[0]].parts[part];
        assert!(
            files.contains(&IrFormat::Ron.level_path(LEVELS[0], part)),
            "{part}"
        );
        for f in files {
            assert!(dir.join(f).is_file(), "{f}");
        }
    }
    assert_eq!(recorded(&m, "ir/characters"), 0);
    assert!(!m.complete, "only the whole extraction marks it");

    // With ffmpeg, the next start resumes: the roster's files are kept and
    // only the intros run.
    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    let done = stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(
        (done.roster.files.written, done.roster.files.kept),
        (0, FIGURES + 1)
    );
    assert_eq!(
        (done.models.files.written, done.models.files.kept),
        (0, MODELS + HEROES + TEXTURES + 1)
    );
    // The progression and the level, whole, are kept without reading them.
    assert!(!done.progression.read);
    assert_eq!(
        (done.progression.files.written, done.progression.files.kept),
        (0, 1)
    );
    assert_eq!((done.levels.parts_read, done.levels.parts_kept), (0, 5));
    assert_eq!(done.levels.files.written, 0);
    assert_eq!(done.intros.unwrap().summary.transcoded, 2);
    assert!(manifest_on_disk(&dir).complete);
}

#[test]
fn a_resume_rewrites_only_the_files_that_arent_as_recorded() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let missing = s.ffmpeg.with_file_name("missing-ffmpeg");
    stages::run(&mut opened.dir, Some(&missing), IrFormat::Ron, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);

    // One portrait gone, one changed, one cut off mid-write: three redone.
    fs::remove_file(dir.join("portraits/000000-0000.png")).unwrap();
    fs::write(dir.join("portraits/000064-0000.png"), b"not a png").unwrap();
    fs::rename(
        dir.join("portraits/00001c-0000.png"),
        dir.join("portraits/00001c-0000.png.partial"),
    )
    .unwrap();
    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    let stage = stages::roster_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((stage.files.written, stage.files.kept), (3, FIGURES - 2));
    for id in ["000000-0000", "000064-0000", "00001c-0000"] {
        let rel = format!("portraits/{id}.png");
        assert!(again.dir.output_done(&rel).unwrap(), "{rel}");
    }
    assert!(!dir.join("portraits/00001c-0000.png.partial").exists());

    // A model, a hero and a texture gone, the rig changed: exactly those
    // redone.
    fs::remove_file(dir.join("models/MinionLoad_007_GemGolem.ron")).unwrap();
    fs::remove_file(dir.join("heroes/MinionLoad_016_Spyro.ron")).unwrap();
    let texture = again
        .dir
        .manifest()
        .outputs
        .iter()
        .find(|o| o.path.starts_with("textures/"))
        .unwrap()
        .path
        .clone();
    fs::remove_file(dir.join(&texture)).unwrap();
    fs::write(dir.join("ir/rig.ron"), b"()").unwrap();
    let roster = stage.roster.clone();
    let models = stages::model_stage(&mut again.dir, &roster, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(
        (models.files.written, models.files.kept),
        (4, MODELS + HEROES + TEXTURES + 1 - 4)
    );
    assert!(again.dir.output_done(&texture).unwrap());
    assert!(
        again
            .dir
            .output_done("heroes/MinionLoad_016_Spyro.ron")
            .unwrap()
    );

    // A level part's file and a texture only the scene binds gone: those
    // two parts are read again, and only the two files written.
    let gloss = {
        let scene: guac_ir::level::Scene = IrFormat::Ron
            .decode(&again.dir, &IrFormat::Ron.level_path(LEVELS[0], "scene"))
            .unwrap();
        let guac_ir::level::LevelMaterial::Multi2(m) = &scene.scenes[0].materials[1] else {
            panic!("the gloss material");
        };
        m.texture(guac_ir::model::TextureRole::Layer3ColorMap)
            .unwrap()
            .texture
            .clone()
    };
    fs::remove_file(dir.join(&gloss)).unwrap();
    fs::remove_file(dir.join(IrFormat::Ron.level_path(LEVELS[0], "placements"))).unwrap();
    let lv = stages::level_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((lv.parts_read, lv.parts_kept), (2, 3));
    assert_eq!(lv.files.written, 2);
    assert!(again.dir.output_done(&gloss).unwrap());
    assert!(lv.failed.is_empty());
    // A part is in for the format asked for: in postcard every part is
    // read again and only its IR written, beside the RON, the textures
    // kept.
    let lv = stages::level_stage(&mut again.dir, IrFormat::Postcard, &|_| {}).unwrap();
    assert_eq!(
        (lv.parts_read, lv.files.written),
        (PARTS.len(), PARTS.len())
    );
    assert_eq!(lv.files.kept, LEVEL_TEXTURES);
    for part in PARTS {
        assert!(
            dir.join(IrFormat::Postcard.level_path(LEVELS[0], part))
                .is_file()
        );
        assert!(
            dir.join(IrFormat::Ron.level_path(LEVELS[0], part))
                .is_file()
        );
    }

    // An IR in the other format is a file of its own, written beside.
    let stage = stages::roster_stage(&mut again.dir, IrFormat::Postcard, &|_| {}).unwrap();
    assert_eq!((stage.files.written, stage.files.kept), (1, FIGURES));
    let bytes = fs::read(dir.join("ir/roster.postcard")).unwrap();
    assert_eq!(
        guac_ir::from_postcard::<Roster>(&bytes).unwrap(),
        *stage.roster
    );
}

#[test]
fn a_v1_extract_is_rebuilt_and_extracted_again() {
    let s = setup();
    // A finished AB-era extract: layout 1, IR 1, intros and no roster.
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    opened
        .dir
        .write_output("video/intro/eruptor.mp4", b"old")
        .unwrap();
    opened
        .dir
        .write_output("ir/characters.ron", b"(ir_version: 1, body: ())")
        .unwrap();
    opened.dir.mark_complete().unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    json["layout"] = 1.into();
    json["ir_version"] = 1.into();
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Rebuilt(Stale::Layout { found: 1 }));
    assert!(!dir.join("video/intro/eruptor.mp4").exists());
    let m = again.dir.manifest();
    assert_eq!((m.layout, m.ir_version), (LAYOUT_VERSION, IR_VERSION));
    assert_eq!((LAYOUT_VERSION, IR_VERSION), (4, 4));
    let done = stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(done.roster.files.written, FIGURES + 1);
    assert!(done.intros.is_ok());
    let m = manifest_on_disk(&dir);
    assert!(m.complete);
    assert_eq!(recorded(&m, "portraits/"), FIGURES);
    // The IR alone stale, with the layout current, rebuilds too.
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    json["ir_version"] = 1.into();
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(
        again.state,
        DirState::Rebuilt(Stale::IrVersion { found: 1 })
    );
    drop(again);
    // An AE.3-era extract (layout 2, IR 2: a roster, no models) too.
    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    drop(again);
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    json["layout"] = 2.into();
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Rebuilt(Stale::Layout { found: 2 }));
    drop(again);
    // An AE.6-era extract (layout 3, IR 2: models, no heroes) too, and it
    // comes back with them.
    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    drop(again);
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    json["layout"] = 3.into();
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Rebuilt(Stale::Layout { found: 3 }));
    assert!(!dir.join("heroes").exists() && !dir.join("levels").exists());
    let done = stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(done.models.heroes, HEROES);
    assert_eq!(done.levels.parts_read, PARTS.len());
    let m = manifest_on_disk(&dir);
    assert_eq!(recorded(&m, "heroes/"), HEROES);
    assert_eq!(recorded(&m, "levels/"), PARTS.len());
    // Once: the next start finds it current.
    drop(again);
    assert_eq!(
        open(&s.dump, &s.root, &s.known, &|_| {}).unwrap().state,
        DirState::Current
    );
    // An extract either branch's IR v3 wrote (layout 4: AG.15's multiply
    // and material tracks, or AE.11's glow, each without the other) too.
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["layout"], 4);
    json["ir_version"] = 3.into();
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(
        again.state,
        DirState::Rebuilt(Stale::IrVersion { found: 3 })
    );
}

#[test]
fn a_roster_that_doesnt_read_stops_the_run_before_ffmpeg() {
    let s = setup();
    // The toy table's archive holds the collection's level instead.
    let usr = s.dump.join("PS3_GAME/USRDIR");
    fs::copy(
        usr.join("misc/ui_collection_champions.bld"),
        usr.join("character/init_setup.bld"),
    )
    .unwrap();
    let known = known_for(&s.dump);
    let mut opened = open(&s.dump, &s.root, &known, &|_| {}).unwrap();
    let seen = Mutex::new(Vec::new());
    let e = stages::run(&mut opened.dir, Some(&s.ffmpeg), IrFormat::Ron, &|e| {
        seen.lock().unwrap().push(Seen::of(&e));
    })
    .unwrap_err();
    assert_eq!(e.code(), "GUAC-E-ROSTER-UNREADABLE", "{e}");
    assert!(e.to_string().contains("character/init_setup.bld"), "{e}");
    assert_eq!(seen.into_inner().unwrap(), [Seen::ReadingRoster]);
    let m = manifest_on_disk(opened.dir.path());
    assert!(m.outputs.is_empty() && !m.complete);
}

/// Every file under `dir` with its bytes and modified time.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let modified = fs::metadata(&p).unwrap().modified().unwrap();
                out.push((p.clone(), fs::read(&p).unwrap(), modified));
            }
        }
    }
    out.sort();
    out
}

/// AE.4's start-time lookup: a complete extract is found and its roster
/// loads, with nothing read off the dump but its PARAM.SFO and listing and
/// nothing written; anything short of complete is no extract at all.
#[test]
fn a_current_extract_is_found_without_writing_and_its_roster_loads() {
    let s = setup();
    // Nothing extracted yet: no manifest vouches for the dump's files.
    assert!(
        guac_extract::find_current(&s.dump, &s.root, &s.known)
            .unwrap()
            .is_none()
    );

    // The roster stage alone: not complete, so not current.
    let missing = s.ffmpeg.with_file_name("missing-ffmpeg");
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    let done = stages::run(&mut opened.dir, Some(&missing), IrFormat::Ron, &|_| {}).unwrap();
    assert!(done.intros.is_err());
    assert!(
        guac_extract::find_current(&s.dump, &s.root, &s.known)
            .unwrap()
            .is_none()
    );

    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    let before = tree(s.root.path());
    let found = guac_extract::find_current(&s.dump, &s.root, &s.known)
        .unwrap()
        .expect("a complete extract");
    assert_eq!(found.path(), dir);
    let roster = guac_extract::roster::load(&found, IrFormat::Ron).unwrap();
    assert_eq!(roster, *done.roster.roster);
    assert_eq!(tree(s.root.path()), before, "the lookup wrote something");

    // A dump file changed since: telling would mean hashing, so no.
    let movie = s.dump.join("PS3_GAME/USRDIR/movies/jetvac.mov");
    fs::write(&movie, b"jetvac bink, edited").unwrap();
    assert!(
        guac_extract::find_current(&s.dump, &s.root, &s.known)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_roster_naming_another_dump_or_unreadable_is_refused() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    let done = stages::run(&mut opened.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    let found = guac_extract::find_current(&s.dump, &s.root, &s.known)
        .unwrap()
        .unwrap();

    let mut other = (*done.roster.roster).clone();
    other.identity.app_ver = "01.02".into();
    fs::write(dir.join("ir/roster.ron"), guac_ir::to_ron(&other).unwrap()).unwrap();
    let e = guac_extract::roster::load(&found, IrFormat::Ron).unwrap_err();
    assert!(e.to_string().contains("names another dump"), "{e}");

    fs::write(dir.join("ir/roster.ron"), b"(not a roster)").unwrap();
    assert!(guac_extract::roster::load(&found, IrFormat::Ron).is_err());
}

/// A part the extract lacks, as an extract made before the part existed
/// does (AG.27's script data will be one): the extract isn't current, and
/// the next run reads that part alone, the roster's, models' and intros'
/// files and the other parts kept.
#[test]
fn a_level_part_the_manifest_lacks_resumes_alone() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    stages::run(&mut opened.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    assert_eq!(
        open(&s.dump, &s.root, &s.known, &|_| {}).unwrap().state,
        DirState::Current
    );
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let parts = json["levels"][LEVELS[0]]["parts"].as_object_mut().unwrap();
    assert!(parts.remove("lights").is_some());
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let lights = IrFormat::Ron.level_path(LEVELS[0], "lights");
    let before = fs::metadata(dir.join(&lights)).unwrap().modified().unwrap();

    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    assert!(
        guac_extract::find_current(&s.dump, &s.root, &s.known)
            .unwrap()
            .is_none()
    );
    let done = stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(done.roster.files.written, 0);
    assert_eq!(done.models.files.written, 0);
    let lv = &done.levels;
    assert_eq!((lv.parts_read, lv.parts_kept), (1, 4));
    // Its file was there as recorded: kept, not written again.
    assert_eq!(
        (lv.files.written, lv.files.kept),
        (0, 1 + 4 + LEVEL_TEXTURES)
    );
    assert_eq!(
        fs::metadata(dir.join(&lights)).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(done.intros.unwrap().summary.transcoded, 0);
    drop(again);
    assert_eq!(
        open(&s.dump, &s.root, &s.known, &|_| {}).unwrap().state,
        DirState::Current
    );
}

/// An extract made before the progression tables (its manifest has no
/// `parts`): not current, and the next run reads the tables alone, every
/// other file kept, then the extract is current.
#[test]
fn the_progression_part_an_older_manifest_lacks_resumes_alone() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    stages::run(&mut opened.dir, Some(&s.ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(json.as_object_mut().unwrap().remove("parts").is_some());
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    let seen = Mutex::new(Vec::new());
    let done = stages::run(&mut again.dir, Some(&s.ffmpeg), IrFormat::Ron, &|e| {
        seen.lock().unwrap().push(Seen::of(&e));
    })
    .unwrap();
    let p = &done.progression;
    assert!(p.read, "read again off the dump");
    // Its file was there as recorded: kept, not written again.
    assert_eq!((p.files.written, p.files.kept), (0, 1));
    assert!(
        seen.into_inner()
            .unwrap()
            .contains(&Seen::ProgressionReady(SAMPLE_LEVELS.into()))
    );
    assert_eq!(done.roster.files.written, 0);
    assert_eq!(done.models.files.written, 0);
    assert_eq!((done.levels.parts_read, done.levels.parts_kept), (0, 5));
    assert_eq!(done.intros.unwrap().summary.transcoded, 0);
    drop(again);
    assert_eq!(
        open(&s.dump, &s.root, &s.known, &|_| {}).unwrap().state,
        DirState::Current
    );
}

/// Tables that don't read are reported and stop nothing: every other stage
/// lands and the manifest is marked, but the extract isn't current, so the
/// next start tries them again.
#[test]
fn progression_tables_that_dont_read_leave_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let mut roster = sample();
    roster.progression = None;
    let dump = make_dump_from(tmp.path(), &roster, &stage_sample());
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let seen = Mutex::new(Vec::new());
    let done = stages::run(&mut opened.dir, Some(&ffmpeg), IrFormat::Ron, &|e| {
        seen.lock().unwrap().push(Seen::of(&e));
    })
    .unwrap();
    let why = done.progression.progression.unwrap_err();
    assert!(why.starts_with("GUAC-E-PROGRESSION-UNREADABLE"), "{why}");
    assert!(
        !seen
            .into_inner()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Seen::ProgressionReady(_)))
    );
    assert_eq!(done.models.models, MODELS);
    assert_eq!(done.levels.parts_read, 5);
    assert_eq!(done.intros.unwrap().summary.transcoded, 2);
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    let m = manifest_on_disk(&dir);
    assert!(m.complete && !guac_extract::progression::complete(&m));
    assert_eq!(
        open(&dump, &root, &known, &|_| {}).unwrap().state,
        DirState::Resume
    );
}

/// The level's colour map is the character sample's: one file, written by
/// the model stage, kept by the level's, recorded once.
#[test]
fn a_texture_already_in_the_store_isnt_written_twice() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Ron, &|_| {}).unwrap();
    let store: Vec<String> = opened
        .dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with("textures/"))
        .map(|o| o.path.clone())
        .collect();
    let dir = opened.dir.path().to_path_buf();
    let modified = |p: &str| fs::metadata(dir.join(p)).unwrap().modified().unwrap();
    let stamps: Vec<_> = store.iter().map(|p| modified(p)).collect();
    let lv = stages::level_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(lv.shared, LEVEL_SHARED);
    assert_eq!(
        lv.files.written,
        PARTS.len() + LEVEL_TEXTURES - LEVEL_SHARED
    );
    for (p, stamp) in store.iter().zip(&stamps) {
        assert_eq!(modified(p), *stamp, "{p} rewritten");
    }
    let m = opened.dir.manifest();
    let scene = &m.levels[LEVELS[0]].parts["scene"];
    let shared: Vec<&String> = scene.iter().filter(|f| store.contains(f)).collect();
    assert_eq!(shared.len(), 1, "the ground's colour map");
    assert_eq!(m.outputs.iter().filter(|o| &o.path == shared[0]).count(), 1);
}

/// A part that won't read (the scene asking for bit 6) is reported; the
/// others land; the extract isn't current, so the next start tries it.
#[test]
fn a_level_part_that_doesnt_read_leaves_the_others() {
    let tmp = tempfile::tempdir().unwrap();
    let mut level = stage_sample();
    level.scene[0].pixel = Some((0x5C, 0x3FF));
    let dump = make_dump_with(tmp.path(), &level);
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let done = stages::run(&mut opened.dir, Some(&ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    let lv = &done.levels;
    assert_eq!((lv.levels, lv.parts_read), (0, 4));
    let [f] = &lv.failed[..] else {
        panic!("{:?}", lv.failed);
    };
    assert_eq!((f.level.as_str(), f.part.as_str()), (LEVELS[0], "scene"));
    assert!(
        f.reason
            .starts_with("GUAC-E-LEVEL-UNREADABLE: level_213_tutotial's scene didn't read:"),
        "{}",
        f.reason
    );
    assert!(f.reason.contains("layer4UsesPositions"), "{}", f.reason);
    assert!(done.intros.is_ok());
    let m = manifest_on_disk(opened.dir.path());
    assert!(m.complete && !levels::complete(&m));
    assert_eq!(recorded(&m, "levels/"), 4);
    drop(opened);
    assert_eq!(
        open(&dump, &root, &known, &|_| {}).unwrap().state,
        DirState::Resume
    );
    // A dump without the level reports every part, and the rest extracts.
    fs::remove_file(levels::archive_path(
        &dump.join("PS3_GAME/USRDIR"),
        LEVELS[0],
    ))
    .unwrap();
    let known = known_for(&dump);
    let mut again = open(&dump, &root, &known, &|_| {}).unwrap();
    let done = stages::run(&mut again.dir, Some(&ffmpeg), IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!(done.levels.failed.len(), PARTS.len());
    assert!(
        done.levels.failed[0].reason.contains("no archive at"),
        "{:?}",
        done.levels.failed[0]
    );
    assert!(done.intros.is_ok());
}
