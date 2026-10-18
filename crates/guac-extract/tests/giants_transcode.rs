//! One real Giants intro through the real ffmpeg (openspec AB.8). Ignored by
//! default: it reads the dump at `GUAC_GIANTS_DUMP`, writes a throwaway
//! extract under `GUAC_EXTRACT_DIR` and needs ffmpeg and ffprobe
//! (`GUAC_FFMPEG` / `GUAC_FFPROBE`, else beside the test binary, PATH or
//! Homebrew's bin folders on macOS). `GUAC_TRANSCODE_INTRO` picks
//! the movie (default `eruptor`). Release mode, for the hashing:
//!
//! `cargo test -p guac-extract --release --test giants_transcode -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use guac_extract::census::find_ffprobe;
use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::transcode::{self, Summary, TARGET};
use guac_extract::{ExtractRoot, intros, open};
use guac_ir::{CharacterTable, VoiceTrack};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// ffprobe's streams for `file`, with frames counted.
fn streams(ffprobe: &Path, file: &Path) -> Vec<serde_json::Value> {
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-count_frames",
            "-of",
            "json",
            "-show_entries",
        ])
        .arg(
            "stream=index,codec_type,codec_name,profile,width,height,r_frame_rate,\
             sample_rate,channels,duration,nb_read_frames",
        )
        .arg(file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["streams"].as_array().unwrap().clone()
}

fn num(s: &serde_json::Value, k: &str) -> f64 {
    s[k].as_f64()
        .or_else(|| s[k].as_str().and_then(|t| t.parse().ok()))
        .unwrap_or_else(|| panic!("no {k} in {s}"))
}

fn rate(s: &serde_json::Value) -> f64 {
    let (n, d) = s["r_frame_rate"].as_str().unwrap().split_once('/').unwrap();
    n.parse::<f64>().unwrap() / d.parse::<f64>().unwrap()
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, runs ffmpeg into GUAC_EXTRACT_DIR"]
fn one_giants_intro_transcodes() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    let movie = env_or_dotenv("GUAC_TRANSCODE_INTRO").unwrap_or_else(|| "eruptor".into());
    let ffmpeg = Ffmpeg::locate(env_or_dotenv("GUAC_FFMPEG").map(PathBuf::from).as_deref())
        .unwrap_or_else(|e| panic!("{e}"));
    let ffprobe = find_ffprobe(env_or_dotenv("GUAC_FFPROBE").map(PathBuf::from)).expect("ffprobe");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ab8-transcode-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();

    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let idx = intros::index(&opened.dir, &Default::default()).unwrap();
    let (&toy, ch) = idx
        .table
        .characters
        .iter()
        .find(|(_, c)| c.intro.as_ref().is_some_and(|i| i.movie == movie))
        .unwrap_or_else(|| panic!("no toy type's intro is {movie}"));
    let voice = ch.intro.as_ref().unwrap().voice;
    let table = CharacterTable {
        identity: idx.table.identity.clone(),
        characters: [(toy, ch.clone())].into(),
    };

    let src = PathBuf::from(&opened.dir.manifest().source_dir)
        .join(intros::MOVIE_DIR)
        .join(format!("{movie}.mov"));
    let src_streams = streams(&ffprobe, &src);
    let src_video = src_streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .unwrap();
    let src_audio: Vec<_> = src_streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect();
    // The IR's choice agrees with the file: one track, so `Only`.
    assert_eq!(src_audio.len(), 1, "{movie}: {src_audio:?}");
    assert_eq!(voice, VoiceTrack::Only);

    let started = Instant::now();
    let summary = transcode::transcode_intros(&mut opened.dir, &table, &ffmpeg, &|_| {}).unwrap();
    let took = started.elapsed().as_secs_f64();
    assert_eq!(
        summary,
        Summary {
            transcoded: 1,
            resumed: 0,
            linked: 0
        }
    );

    let (video_rel, audio_rel) = TARGET.outputs(&movie);
    let video = opened.dir.path().join(&video_rel);
    let audio = opened.dir.path().join(&audio_rel);
    let v = streams(&ffprobe, &video);
    let a = streams(&ffprobe, &audio);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(a.len(), 1, "{a:?}");
    let (v, a, sa) = (&v[0], &a[0], src_audio[0]);
    let frame = 1.0 / rate(src_video);

    assert_eq!(v["codec_type"], "video");
    assert_eq!(v["codec_name"], "h264");
    assert_eq!(v["profile"], "Constrained Baseline");
    assert_eq!(
        (v["width"].clone(), v["height"].clone()),
        (1280.into(), 720.into())
    );
    assert_eq!(v["width"], src_video["width"]);
    assert_eq!(v["height"], src_video["height"]);
    assert_eq!(v["r_frame_rate"], src_video["r_frame_rate"]);
    assert_eq!(v["nb_read_frames"], src_video["nb_read_frames"]);
    let src_dur = num(src_video, "duration");
    assert!(
        (num(v, "duration") - src_dur).abs() <= frame,
        "{v} vs {src_video}"
    );

    assert_eq!(a["codec_type"], "audio");
    assert_eq!(a["codec_name"], "mp3");
    assert_eq!(a["sample_rate"], sa["sample_rate"]);
    assert_eq!(a["channels"], sa["channels"]);
    // MP3 pads to its 1,152-sample frames.
    assert!(
        (num(a, "duration") - src_dur).abs() <= frame,
        "{a} vs {src_video}"
    );

    let sizes = |p: &Path| std::fs::metadata(p).unwrap().len();
    println!(
        "{movie} (toy {toy}): {} {} {}x{} at {}, {} frames, {:.3} s; \
         out h264 {} B, mp3 {} ch {} Hz {:.3} s {} B; ffmpeg {:.2} s ({}); source {} B",
        src_video["codec_name"],
        sa["codec_name"],
        v["width"],
        v["height"],
        v["r_frame_rate"],
        v["nb_read_frames"],
        num(v, "duration"),
        sizes(&video),
        a["channels"],
        a["sample_rate"],
        num(a, "duration"),
        sizes(&audio),
        took,
        ffmpeg.version,
        sizes(&src),
    );

    // A second run finds both outputs done and runs nothing.
    let started = Instant::now();
    let again = transcode::transcode_intros(&mut opened.dir, &table, &ffmpeg, &|_| {}).unwrap();
    assert_eq!(again.resumed, 1);
    assert_eq!(again.transcoded, 0);
    println!("resume: {:.3} s", started.elapsed().as_secs_f64());
}
