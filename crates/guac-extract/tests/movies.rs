//! The disc's cinematics on demand (openspec AS.9, design decision 7) on a
//! synthetic dump: a PARAM.SFO and three movies built from Bink headers in
//! code (a cinematic laid out as the disc's ten, one with a single track,
//! one whose voices lack English) beside an intro, converted by a fake
//! ffmpeg (a shell script that copies its input into each output). Unix
//! only, as the other transcode tests; the real dump runs in
//! `giants_fmv.rs`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::known::KnownDump;
use guac_extract::transcode::{self, Done, FMV_AUDIO_DIR, FMV_VIDEO_DIR};
use guac_extract::{ExtractError, ExtractRoot, open};
use guac_formats::bink::builder::{cinematic, header, mono, stereo};
use guac_formats::sfo::builder::SfoBuilder;
use guac_ir::VoiceTrack;

const ENCODERS: &str = "Encoders:\\n ------\\n V....D libx264  x\\n A....D libmp3lame  y\\n";
const DECODERS: &str = "Decoders:\\n ------\\n V....D binkvideo  x\\n A....D binkaudio_dct  y\\n A....D binkaudio_rdft  z\\n";

/// A fake ffmpeg: answers the checks, copies its input into each output and
/// logs its arguments one per line to `<dir>/args.log`.
fn fake_ffmpeg(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let log = dir.join("args.log");
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
for a in "$@"; do echo "$a" >> '{log}'; done
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
"#,
        log = log.display()
    );
    let p = dir.join("ffmpeg");
    fs::write(&p, script).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn make_dump(tmp: &Path) -> PathBuf {
    let dump = tmp.join("dump");
    let movies = dump.join("PS3_GAME/USRDIR/movies");
    fs::create_dir_all(&movies).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    fs::write(movies.join("fmv01.mov"), cinematic()).unwrap();
    fs::write(movies.join("fmv12.mov"), header(30, &[stereo(0)])).unwrap();
    // Twelve voices, none of them English's.
    let mut tracks: Vec<_> = [1, 5, 6, 8, 9, 10, 15, 18, 20, 22, 23, 24]
        .into_iter()
        .map(mono)
        .collect();
    tracks.push(stereo(0));
    fs::write(movies.join("fmv03.mov"), header(30, &tracks)).unwrap();
    fs::write(movies.join("eruptor.mov"), header(30, &[stereo(0)])).unwrap();
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
fn a_cinematic_converts_on_demand_in_english_over_its_bed() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let known = known_for(&dump);
    let bin = tmp.path().join("bin");
    let ffmpeg = Ffmpeg::locate(Some(&fake_ffmpeg(&bin))).unwrap();
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let dir = &mut opened.dir;

    assert_eq!(
        transcode::cinematics(dir.manifest()),
        ["fmv01", "fmv03", "fmv12"]
    );

    // fmv01: English (id 7, the fourth track, stream 4) over the bed (id 0,
    // the last of 13, stream 13), in the cinematics' own folders.
    let first = transcode::transcode_movie(dir, "fmv01", &ffmpeg, &|_| {}).unwrap();
    assert_eq!(
        first.voice,
        VoiceTrack::Stream {
            index: 4,
            bed: Some(13)
        }
    );
    assert_eq!(first.done, Done::Transcoded);
    assert_eq!(
        (first.video.as_str(), first.audio.as_str()),
        (
            format!("{FMV_VIDEO_DIR}/fmv01.mp4").as_str(),
            format!("{FMV_AUDIO_DIR}/fmv01.mp3").as_str()
        )
    );
    assert!(dir.output_done(&first.video).unwrap() && dir.output_done(&first.audio).unwrap());
    assert_eq!(first.bytes, 2 * cinematic().len() as u64);
    let args = fs::read_to_string(bin.join("args.log")).unwrap();
    assert!(
        args.contains("[0:13][0:4]amix=inputs=2:duration=first:normalize=0[voice]"),
        "{args}"
    );
    let record = &dir.manifest().transcode.as_ref().unwrap().movies["fmv01"];
    assert_eq!(record.ffmpeg, "ffmpeg version fake");

    // Asked again: kept, ffmpeg not run.
    fs::remove_file(bin.join("args.log")).unwrap();
    let again = transcode::transcode_movie(dir, "fmv01", &ffmpeg, &|_| {}).unwrap();
    assert_eq!(again.done, Done::Resumed);
    assert!(!bin.join("args.log").exists());

    // One track: that track.
    let one = transcode::transcode_movie(dir, "fmv12", &ffmpeg, &|_| {}).unwrap();
    assert_eq!(one.voice, VoiceTrack::Only);

    // No English voice: refused by name, nothing written.
    let before = dir.manifest().outputs.len();
    let e = transcode::transcode_movie(dir, "fmv03", &ffmpeg, &|_| {}).unwrap_err();
    assert!(matches!(e, ExtractError::TranscodeFailed { .. }), "{e}");
    assert!(e.to_string().contains("English"), "{e}");
    assert_eq!(dir.manifest().outputs.len(), before);

    // An intro, and a cinematic the disc doesn't have, aren't cinematics.
    for name in ["eruptor", "fmv05"] {
        let e = transcode::transcode_movie(dir, name, &ffmpeg, &|_| {}).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-NO-SUCH-MOVIE", "{name}: {e}");
    }
}

#[test]
fn a_target_change_redoes_a_cinematic() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path());
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let known = known_for(&dump);
    let ffmpeg = Ffmpeg::locate(Some(&fake_ffmpeg(&tmp.path().join("bin")))).unwrap();
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let first = transcode::transcode_movie(&mut opened.dir, "fmv01", &ffmpeg, &|_| {}).unwrap();
    let path = opened.dir.path().to_path_buf();
    drop(opened);

    // The manifest says another target made it: the next ask forgets it
    // and converts it again.
    let manifest = path.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    json["transcode"]["target"] = "an older target".into();
    fs::write(&manifest, serde_json::to_string(&json).unwrap()).unwrap();
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    // Another cinematic asked for first: the old target's outputs, fmv01's
    // among them, are forgotten as the record is made this target's.
    transcode::transcode_movie(&mut opened.dir, "fmv12", &ffmpeg, &|_| {}).unwrap();
    let paths: Vec<&str> = opened
        .dir
        .manifest()
        .outputs
        .iter()
        .map(|o| o.path.as_str())
        .collect();
    assert!(!paths.contains(&first.video.as_str()), "{paths:?}");
    assert!(!path.join(&first.video).exists());
    let again = transcode::transcode_movie(&mut opened.dir, "fmv01", &ffmpeg, &|_| {}).unwrap();
    assert_eq!(again.done, Done::Transcoded);
    assert_eq!(again.video, first.video);
    assert_eq!(
        opened.dir.manifest().transcode.as_ref().unwrap().target,
        transcode::TARGET.tag()
    );
}

/// The voice choice off a header alone: one track is that track; English is
/// the track whose id is its language id plus one, over the stereo track
/// with id 0, numbered as ffmpeg numbers streams; no English, or no stereo
/// bed, is none.
#[test]
fn the_english_voice_is_its_language_id_plus_one_over_the_stereo_bed() {
    use guac_formats::bink::Header;
    let voice = |tracks: &[guac_formats::bink::AudioTrack]| {
        transcode::english_voice(&Header::read(&header(1, tracks)).unwrap())
    };
    assert_eq!(voice(&[stereo(0)]), Some(VoiceTrack::Only));
    assert_eq!(
        voice(&[stereo(0), mono(1), mono(7)]),
        Some(VoiceTrack::Stream {
            index: 3,
            bed: Some(1)
        })
    );
    assert_eq!(voice(&[mono(6), stereo(0)]), None, "id 6 is Dutch's");
    assert_eq!(
        voice(&[mono(7), mono(0)]),
        None,
        "a mono track 0 isn't a bed"
    );
    assert_eq!(voice(&[mono(7), stereo(3)]), None, "no track 0");
    for (name, is) in [
        ("fmv01", true),
        ("fmv12", true),
        ("fmv", false),
        ("fmvx1", false),
        ("eruptor", false),
        ("tfb_logo_640x456", false),
    ] {
        assert_eq!(transcode::is_cinematic(name), is, "{name}");
    }
}
