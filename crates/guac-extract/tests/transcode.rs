//! The intro transcode loop (openspec AB.8) on synthetic dumps, against a fake
//! ffmpeg: a shell script that answers the capability checks, logs each run's
//! input, writes its outputs and prints `-progress` blocks. Unix only; the real
//! ffmpeg runs in `giants_transcode.rs`.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use guac_extract::ffmpeg::{Ffmpeg, Found, Where};
use guac_extract::known::KnownDump;
use guac_extract::transcode::{self, Done, Step, Summary, TARGET, TranscodeEvent};
use guac_extract::{DirState, ExtractError, ExtractRoot, IdentityDir, open};
use guac_formats::sfo::builder::SfoBuilder;
use guac_ir::{Character, CharacterTable, Intro, IntroSource, VoiceTrack};

const ENCODERS: &str = "Encoders:\n ------\n V....D libx264  x\n A....D libmp3lame  y\n";
const DECODERS: &str = "Decoders:\n ------\n V....D binkvideo  x\n A....D binkaudio_dct  y\n A....D binkaudio_rdft  z\n";

/// `<dir>/ffmpeg`, logging every transcode's input path to `<dir>/runs.log`.
fn fake_ffmpeg(dir: &Path, encoders: &str) -> PathBuf {
    fake_ffmpeg_v(dir, encoders, "fake-1")
}

/// [`fake_ffmpeg`] reporting `version`; each output holds the input's bytes,
/// the output's name and the `-map` before it.
fn fake_ffmpeg_v(dir: &Path, encoders: &str, version: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let log = dir.join("runs.log");
    let script = format!(
        r#"#!/bin/sh
case "$1" in
  -version) echo "ffmpeg version {version}"; exit 0;;
  -hide_banner)
    case "$2" in
      -encoders) printf '{encoders}'; exit 0;;
      -decoders) printf '{DECODERS}'; exit 0;;
    esac;;
esac
prev=''; in=''
for a in "$@"; do
  if [ "$prev" = "-i" ]; then in="${{a#file:}}"; fi
  prev="$a"
done
echo "$in" >> '{log}'
case "$in" in *broken*) echo "broken.mov: Invalid data found when processing input" >&2; exit 1;; esac
prev=''; map=''
for a in "$@"; do
  if [ "$prev" = "-map" ]; then case "$a" in 0:v:0|0:a:0) map='';; *) map=" $a";; esac; fi
  case "$a" in file:*) if [ "$prev" != "-i" ]; then {{ cat "$in"; echo " -> ${{a##*/}}$map"; }} > "${{a#file:}}"; fi;; esac
  prev="$a"
done
printf 'frame=10\nout_time_us=1000000\nspeed=2.0x\nprogress=continue\n'
printf 'frame=20\nout_time_us=2000000\nspeed=2.5x\nprogress=end\n'
"#,
        encoders = encoders.replace('\n', "\\n"),
        DECODERS = DECODERS.replace('\n', "\\n"),
        log = log.display(),
        version = version,
    );
    let p = dir.join("ffmpeg");
    fs::write(&p, script).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn runs(ffmpeg: &Path) -> Vec<String> {
    fs::read_to_string(ffmpeg.with_file_name("runs.log"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.rsplit('/').next().unwrap().to_owned())
        .collect()
}

fn checked(path: PathBuf) -> Ffmpeg {
    Ffmpeg::check(Found {
        path,
        how: Where::Configured,
    })
    .unwrap()
}

/// A dump whose `movies/` holds `movies` (name, bytes).
fn make_dump(tmp: &Path, movies: &[(&str, &[u8])]) -> PathBuf {
    let dump = tmp.join("dump");
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::create_dir_all(usr.join("movies")).unwrap();
    fs::write(
        dump.join("PS3_GAME/PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    )
    .unwrap();
    for (name, bytes) in movies {
        fs::write(usr.join(format!("movies/{name}.mov")), bytes).unwrap();
    }
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

/// Toy type → movie, every intro the movie's only track.
fn table(dir: &IdentityDir, intros: &[(u32, &str)]) -> CharacterTable {
    let with: Vec<_> = intros
        .iter()
        .map(|&(t, m)| (t, m, VoiceTrack::Only))
        .collect();
    table_voiced(dir, &with)
}

fn table_voiced(dir: &IdentityDir, intros: &[(u32, &str, VoiceTrack)]) -> CharacterTable {
    CharacterTable {
        identity: dir.manifest().identity.clone(),
        characters: intros
            .iter()
            .map(|&(toy, movie, voice)| {
                let c = Character {
                    archives: vec![],
                    vo_name: None,
                    intro: Some(Intro {
                        movie: movie.into(),
                        source: IntroSource::Disc,
                        voice,
                    }),
                };
                (toy, c)
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

fn run(dir: &mut IdentityDir, t: &CharacterTable, ff: &Ffmpeg) -> (Summary, Vec<TranscodeEvent>) {
    let events = Mutex::new(Vec::new());
    let s = transcode::transcode_intros(dir, t, ff, &|e| events.lock().unwrap().push(e)).unwrap();
    (s, events.into_inner().unwrap())
}

fn partials(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(partials(&p));
        } else if p.to_string_lossy().ends_with(".partial") {
            out.push(p);
        }
    }
    out
}

#[test]
fn transcodes_dedupes_and_resumes() {
    let tmp = tempfile::tempdir().unwrap();
    // `b` is `a`'s bytes under another name; `c` differs.
    let dump = make_dump(
        tmp.path(),
        &[("a", b"movie-a"), ("b", b"movie-a"), ("c", b"movie-c")],
    );
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ff_path = fake_ffmpeg(&tmp.path().join("bin"), ENCODERS);
    let ff = checked(ff_path.clone());
    assert_eq!(ff.version, "ffmpeg version fake-1");

    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    // Toy 28 shares toy 16's movie: one job.
    let t = table(&opened.dir, &[(16, "a"), (28, "a"), (17, "b"), (18, "c")]);
    assert_eq!(transcode::jobs(&t).unwrap().len(), 3);
    let (s, events) = run(&mut opened.dir, &t, &ff);
    assert_eq!(
        s,
        Summary {
            transcoded: 2,
            resumed: 0,
            linked: 1
        }
    );
    assert_eq!(runs(&ff_path), ["a.mov", "c.mov"]);

    let id = opened.dir.path().to_path_buf();
    let read = |rel: &str| fs::read_to_string(id.join(rel)).unwrap();
    assert_eq!(read("video/intro/a.mp4"), "movie-a -> a.mp4.partial\n");
    assert_eq!(read("audio/intro/a.mp3"), "movie-a -> a.mp3.partial\n");
    // Linked from `a`, not re-run.
    assert_eq!(read("video/intro/b.mp4"), "movie-a -> a.mp4.partial\n");
    assert_eq!(read("audio/intro/c.mp3"), "movie-c -> c.mp3.partial\n");
    assert!(partials(&id).is_empty());

    let m = opened.dir.manifest();
    let outs: Vec<&str> = m.outputs.iter().map(|o| o.path.as_str()).collect();
    assert_eq!(
        outs,
        [
            "audio/intro/a.mp3",
            "audio/intro/b.mp3",
            "audio/intro/c.mp3",
            "video/intro/a.mp4",
            "video/intro/b.mp4",
            "video/intro/c.mp4"
        ]
    );
    let rec = m.transcode.as_ref().unwrap();
    assert_eq!(rec.target, TARGET.tag());
    assert_eq!(rec.movies.keys().collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(rec.movies["a"], rec.movies["b"]);
    assert_ne!(rec.movies["a"].recipe, rec.movies["c"].recipe);
    assert!(
        rec.movies
            .values()
            .all(|r| r.ffmpeg == "ffmpeg version fake-1")
    );

    // Per movie: Started, then progress for a run, then Done.
    let steps = |movie: &str| -> Vec<Step> {
        events
            .iter()
            .filter(|e| e.movie == movie)
            .inspect(|e| assert_eq!(e.total, 3))
            .map(|e| e.step.clone())
            .collect()
    };
    let a = steps("a");
    assert_eq!(a.len(), 4, "{a:?}");
    assert_eq!(a[0], Step::Started);
    let Step::Encoding(p) = &a[2] else {
        panic!("{a:?}")
    };
    assert!(p.end && p.frame == 20 && p.out_time_us == Some(2_000_000));
    assert_eq!(a[3], Step::Done(Done::Transcoded));
    assert_eq!(
        steps("b"),
        [Step::Started, Step::Done(Done::Linked { from: "a".into() })]
    );

    // Interrupted: `c`'s audio gone and a stray partial left behind.
    fs::remove_file(id.join("audio/intro/c.mp3")).unwrap();
    fs::write(id.join("video/intro/c.mp4.partial"), b"half").unwrap();
    drop(opened);
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    assert_eq!(opened.state, DirState::Resume);
    let (s, _) = run(&mut opened.dir, &t, &ff);
    assert_eq!(
        s,
        Summary {
            transcoded: 1,
            resumed: 2,
            linked: 0
        }
    );
    assert_eq!(runs(&ff_path), ["a.mov", "c.mov", "c.mov"]);
    assert!(partials(&id).is_empty());

    // An output edited in place no longer hashes to its record. `b`'s is a
    // hard link to it, so it's redone too, linked from the new `a`.
    fs::write(id.join("video/intro/a.mp4"), b"tampered").unwrap();
    let (s, _) = run(&mut opened.dir, &t, &ff);
    assert_eq!((s.transcoded, s.linked, s.resumed), (1, 1, 1));
    assert_eq!(read("video/intro/a.mp4"), "movie-a -> a.mp4.partial\n");
    assert_eq!(read("video/intro/b.mp4"), "movie-a -> a.mp4.partial\n");

    // The dump is as it was.
    let usr = dump.join("PS3_GAME/USRDIR/movies");
    let names: Vec<_> = fs::read_dir(&usr)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(fs::read(usr.join("b.mov")).unwrap(), b"movie-a");
}

#[test]
fn another_target_redoes_a_complete_extract() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), &[("a", b"movie-a")]);
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ff_path = fake_ffmpeg(&tmp.path().join("bin"), ENCODERS);
    let ff = checked(ff_path.clone());

    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let t = table(&opened.dir, &[(1, "a")]);
    run(&mut opened.dir, &t, &ff);
    record_levels(&mut opened.dir);
    opened.dir.mark_complete().unwrap();
    let id = opened.dir.path().to_path_buf();
    drop(opened);
    assert_eq!(
        open(&dump, &root, &known, &|_| {}).unwrap().state,
        DirState::Current
    );

    // As if an older guacamole had transcoded for another target.
    let mpath = id.join("manifest.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mpath).unwrap()).unwrap();
    m["transcode"]["target"] = "an-older-target".into();
    fs::write(&mpath, serde_json::to_string(&m).unwrap()).unwrap();

    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    assert_eq!(opened.state, DirState::Resume);
    assert!(!opened.dir.manifest().complete);
    let (s, _) = run(&mut opened.dir, &t, &ff);
    assert_eq!(s.transcoded, 1);
    assert_eq!(runs(&ff_path), ["a.mov", "a.mov"]);
    assert_eq!(
        opened.dir.manifest().transcode.as_ref().unwrap().target,
        TARGET.tag()
    );
}

#[test]
fn a_changed_voice_reruns_its_movie_and_the_record_names_the_encoder() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), &[("a", b"movie-a"), ("c", b"movie-c")]);
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let old = fake_ffmpeg_v(&tmp.path().join("old"), ENCODERS, "old-1");
    let new = fake_ffmpeg_v(&tmp.path().join("new"), ENCODERS, "new-2");

    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let t = table(&opened.dir, &[(1, "a"), (2, "c")]);
    let (s, _) = run(&mut opened.dir, &t, &checked(old.clone()));
    assert_eq!(s.transcoded, 2);
    let id = opened.dir.path().to_path_buf();
    drop(opened);

    // Reopened (Resume) under another ffmpeg, `a`'s voice now picked with a
    // bed: `a` reruns with the new map, `c` resumes and keeps old-1.
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    assert_eq!(opened.state, DirState::Resume);
    let mixed = VoiceTrack::Stream {
        index: 3,
        bed: Some(13),
    };
    let t = table_voiced(&opened.dir, &[(1, "a", mixed), (2, "c", VoiceTrack::Only)]);
    let (s, events) = run(&mut opened.dir, &t, &checked(new.clone()));
    assert_eq!(
        s,
        Summary {
            transcoded: 1,
            resumed: 1,
            linked: 0
        }
    );
    assert_eq!(runs(&new), ["a.mov"]);
    let done_a = events
        .iter()
        .find(|e| e.movie == "a" && matches!(e.step, Step::Done(_)));
    assert_eq!(done_a.unwrap().step, Step::Done(Done::Transcoded));
    assert_eq!(
        fs::read_to_string(id.join("audio/intro/a.mp3")).unwrap(),
        "movie-a -> a.mp3.partial [voice]\n"
    );
    let movies = &opened.dir.manifest().transcode.as_ref().unwrap().movies;
    assert_eq!(movies["a"].ffmpeg, "ffmpeg version new-2");
    assert_eq!(movies["c"].ffmpeg, "ffmpeg version old-1");
    assert_eq!(
        movies["a"].recipe,
        transcode::recipe(
            &opened
                .dir
                .manifest()
                .sources
                .iter()
                .find(|f| f.path == "movies/a.mov")
                .unwrap()
                .sha256,
            mixed,
            &TARGET
        )
    );

    // Same voice again: nothing reruns.
    let (s, _) = run(&mut opened.dir, &t, &checked(new.clone()));
    assert_eq!(s.resumed, 2);
    assert_eq!(runs(&new), ["a.mov"]);

    // A manifest from before the per-movie record (no `movies`) reads, and
    // its intros are redone rather than trusted.
    drop(opened);
    let mpath = id.join("manifest.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mpath).unwrap()).unwrap();
    let rec = m["transcode"].as_object_mut().unwrap();
    rec.remove("movies");
    rec.insert("ffmpeg".into(), "ffmpeg version 9.0.2".into());
    fs::write(&mpath, serde_json::to_string(&m).unwrap()).unwrap();
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let (s, _) = run(&mut opened.dir, &t, &checked(new.clone()));
    assert_eq!(s.transcoded, 2);
}

#[test]
fn a_failed_run_names_the_movie_and_records_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), &[("broken", b"x")]);
    let known = known_for(&dump);
    let root = ExtractRoot::checked(tmp.path().join("out/extract")).unwrap();
    let ff = checked(fake_ffmpeg(&tmp.path().join("bin"), ENCODERS));
    let mut opened = open(&dump, &root, &known, &|_| {}).unwrap();
    let t = table(&opened.dir, &[(1, "broken")]);
    let e = transcode::transcode_intros(&mut opened.dir, &t, &ff, &|_| {}).unwrap_err();
    assert_eq!(e.code(), "GUAC-E-TRANSCODE-FAILED");
    let text = e.to_string();
    assert!(
        text.contains("`broken`") && text.contains("Invalid data"),
        "{text}"
    );
    assert!(opened.dir.manifest().outputs.is_empty());

    // A movie the dump's manifest doesn't hold.
    let t = table(&opened.dir, &[(1, "nope")]);
    let e = transcode::transcode_intros(&mut opened.dir, &t, &ff, &|_| {}).unwrap_err();
    assert!(e.to_string().contains("movies/nope.mov"), "{e}");
    // A movie name that isn't a plain name.
    let t = table(&opened.dir, &[(1, "../x")]);
    let e = transcode::transcode_intros(&mut opened.dir, &t, &ff, &|_| {}).unwrap_err();
    assert_eq!(e.code(), "GUAC-E-BAD-OUTPUT-PATH");
}

#[test]
fn an_ffmpeg_without_the_codecs_or_none_at_all_is_named() {
    let tmp = tempfile::tempdir().unwrap();
    let lacking = fake_ffmpeg(
        &tmp.path().join("bin"),
        "Encoders:\n ------\n V....D libx264  x\n",
    );
    let e = Ffmpeg::check(Found {
        path: lacking,
        how: Where::Path,
    })
    .unwrap_err();
    assert_eq!(e.code(), "GUAC-E-FFMPEG-UNUSABLE");
    assert!(e.to_string().contains("encoder libmp3lame"), "{e}");

    let gone = tmp.path().join("no/ffmpeg");
    match Ffmpeg::locate(Some(&gone)) {
        Err(ExtractError::NoFfmpeg {
            configured: Some(p),
            install,
        }) => {
            assert_eq!(p, gone);
            assert_eq!(install, guac_extract::ffmpeg::INSTALL_LINE);
        }
        other => panic!("{other:?}"),
    }
}

/// What a whole run's level and progression stages record (AG.15, AH.2),
/// each part with no files of its own here: a current extract needs them.
fn record_levels(dir: &mut guac_extract::IdentityDir) {
    for level in guac_extract::levels::LEVELS {
        for part in guac_extract::levels::PARTS {
            dir.record_level_part(level, part, Vec::new()).unwrap();
        }
    }
    for part in guac_extract::progression::PARTS {
        dir.record_part(part, Vec::new()).unwrap();
    }
}
