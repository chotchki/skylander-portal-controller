//! The sounds part on a synthetic dump (openspec AR.5): the roster
//! builder's archives, the sound builder's `level.bld` and English pack as
//! the level's archive and two characters' (the same sounds, so their banks
//! are shared), an `.arc` streaming what they name, the other characters'
//! archives missing. The part runs as its stage, after the roster's; the
//! real dump runs in `giants_sounds.rs`, the whole run in `stages.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use guac_extract::intros::IrFormat;
use guac_extract::known::KnownDump;
use guac_extract::sounds::{self, AUDIO_DIR, PART};
use guac_extract::{DirState, ExtractRoot, levels, open, stages};
use guac_formats::fsb::builder::{BankSpec, SampleSpec, write};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::mpeg::builder::stream;
use guac_formats::roster::builder::{disc, sample};
use guac_formats::sfo::builder::SfoBuilder;
use guac_formats::sound::builder::{pack, sample as sounds_sample};

/// The characters given the sound builder's archive; the roster's other
/// modelled characters have none.
const WITH_SOUNDS: [&str; 2] = ["MinionLoad_000_AirDragon", "MinionLoad_007_GemGolem"];

fn bank(frames: usize) -> Vec<u8> {
    write(&BankSpec::new(vec![SampleSpec::new(
        stream(frames, 3),
        frames as u32 * 1152,
    )]))
    .bytes
}

fn entry(path: &str, hex: &str) -> String {
    format!("c:/tfb/build/ps3/{path}/0x{hex}.wav.hz.wav.enc")
}

/// The sound builder's archive and the `.arc` streaming its three lines.
fn sound_archive() -> (Vec<u8>, Vec<u8>) {
    let bld = IgaBuilder::new()
        .lzma("level.bld", &sounds_sample().bytes)
        .lzma("ENGLISH.pak", &pack("ENGLISH"))
        .build();
    let arc = IgaBuilder::new()
        .stored(
            &entry("levels/level_213/sounds/mu_213_explore.wav", "a350e320"),
            &bank(5),
        )
        .stored(
            &entry("levels/npc_audio/eon/vo_eon_lvl213_001.wav", "11111111"),
            &bank(2),
        )
        .stored(
            &entry("levels/npc_audio/eon/vo_eon_lvl213_014.wav", "22222222"),
            &bank(6),
        )
        .stored(
            &entry(
                "levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav",
                "33333333",
            ),
            &bank(4),
        )
        .build();
    (bld, arc)
}

fn make_dump(tmp: &Path) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::create_dir_all(usr.join("character")).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    for (path, bytes) in disc(&sample()).archives() {
        let path = usr.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let (bld, arc) = sound_archive();
    for load in WITH_SOUNDS {
        let path = usr.join(format!("character/{}.bld", load.to_ascii_lowercase()));
        fs::write(&path, &bld).unwrap();
        fs::write(path.with_extension("arc"), &arc).unwrap();
    }
    let level = levels::archive_path(&usr, levels::LEVELS[0]);
    fs::create_dir_all(level.parent().unwrap()).unwrap();
    fs::write(&level, &bld).unwrap();
    fs::write(level.with_extension("arc"), &arc).unwrap();
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

struct Setup {
    _tmp: tempfile::TempDir,
    dump: PathBuf,
    root: ExtractRoot,
    known: Vec<KnownDump>,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    Setup {
        _tmp: tmp,
        dump,
        root,
        known,
    }
}

/// The manifest's outputs under the audio folder.
fn audio_outputs(dir: &guac_extract::IdentityDir) -> Vec<String> {
    dir.manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with(AUDIO_DIR))
        .map(|o| o.path.clone())
        .collect()
}

#[test]
fn the_part_lands_each_archives_sounds_and_each_bank_once() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    let st = sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    assert!(
        st.read && !st.kept && st.failed.is_none(),
        "{:?}",
        st.failed
    );
    // global.bld, the two characters with archives, the level.
    assert_eq!(st.archives, 4);
    let loads = guac_extract::models::character_loads(&roster.roster).len();
    assert_eq!(st.missing.len(), loads - WITH_SOUNDS.len());
    // Three of the four archives hold the sample's six sounds; global.bld none.
    assert_eq!(st.sounds, 3 * 6);
    // The embedded bank, the music, two English lines and the Swedish one:
    // five banks, each written once though three archives name them.
    assert_eq!(st.audio, 5);
    assert_eq!((st.audio_files.written, st.audio_files.kept), (5, 0));
    assert_eq!(audio_outputs(&opened.dir).len(), 5);
    let part = &opened.dir.manifest().parts[PART];
    assert_eq!(part.len(), 4 + 5, "the IR files and the audio");
    for f in part {
        assert!(opened.dir.output_done(f).unwrap(), "{f}");
    }
    // Each IR file reads back, its sounds pointing at audio in the extract.
    let rel = sounds::path(IrFormat::Ron, "level/level_213_tutotial.bld");
    let ir: guac_ir::sound::Sounds = IrFormat::Ron.decode(&opened.dir, &rel).unwrap();
    assert_eq!(ir.sounds.len(), 6);
    for d in ir.data.iter().filter_map(|d| d.audio.as_ref()) {
        assert!(part.contains(&d.file), "{}", d.file);
        let bytes = fs::read(opened.dir.path().join(&d.file)).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xFB], "an MPEG frame first");
    }
}

#[test]
fn a_second_run_keeps_the_part() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    let again = sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    assert!(again.kept && !again.read);
    assert_eq!(again.files.written, 0);
    assert_eq!(again.files.kept, 4 + 5);
}

/// An audio file damaged at its own size (one byte flipped) no longer
/// hashes to its record, so the part isn't held and the next run writes
/// that file again, the rest kept; the run after keeps the part. Kept by
/// its size alone, it would be kept damaged and the part read again at
/// every start (the AR review found it so on the real dump).
#[test]
fn an_audio_file_damaged_at_its_size_is_written_again() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    let damaged = audio_outputs(&opened.dir)[0].clone();
    let path = opened.dir.path().join(&damaged);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&path, &bytes).unwrap();
    assert!(!opened.dir.output_done(&damaged).unwrap());

    let st = sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    assert!(st.read, "a damaged file means the part isn't held");
    assert_eq!(
        (st.audio_files.written, st.audio_files.kept),
        (1, 4),
        "only the damaged file"
    );
    assert!(opened.dir.output_done(&damaged).unwrap());
    let again = sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    assert!(again.kept && !again.read);
}

#[test]
fn an_extract_from_before_the_part_reads_it_alone_keeping_audio_already_in() {
    let s = setup();
    let mut opened = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    let dir = opened.dir.path().to_path_buf();
    drop(opened);
    // The part's record gone, one audio file gone with its record, the
    // rest of its files left as they are.
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
        .find(|f| f.starts_with(AUDIO_DIR))
        .unwrap()
        .clone();
    let outputs = json["outputs"].as_array_mut().unwrap();
    outputs.retain(|o| o["path"] != lost.as_str());
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    fs::remove_file(dir.join(&lost)).unwrap();

    let mut again = open(&s.dump, &s.root, &s.known, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    let roster = stages::roster_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
    let st = sounds::sound_stage(&mut again.dir, &roster.roster, IrFormat::Ron).unwrap();
    assert!(st.read);
    assert_eq!(
        (st.audio_files.written, st.audio_files.kept),
        (1, 4),
        "only the lost file"
    );
    assert_eq!(st.files.written, 1, "the IR files as recorded, kept");
    assert!(again.dir.output_done(&lost).unwrap());
    assert_eq!(again.dir.manifest().parts[PART].len(), files.len());
}

#[test]
fn an_archive_that_wont_read_refuses_the_part() {
    let s = setup();
    let usr = s.dump.join("PS3_GAME/USRDIR");
    let arc = usr.join(format!(
        "character/{}.arc",
        WITH_SOUNDS[1].to_ascii_lowercase()
    ));
    fs::write(&arc, b"not an archive").unwrap();
    let known = known_for(&s.dump);
    let mut opened = open(&s.dump, &s.root, &known, &|_| {}).unwrap();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    let st = sounds::sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    let (archive, why) = st.failed.expect("refused");
    assert!(archive.contains("gemgolem"), "{archive}");
    assert!(
        why.contains("GUAC-E-SCRIPTS-UNREADABLE") && why.contains(".arc"),
        "{why}"
    );
    assert!(!st.read);
    assert!(!opened.dir.manifest().parts.contains_key(PART));
}
