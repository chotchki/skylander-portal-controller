//! AS.9 on the real Giants dump: one cinematic converted through the
//! on-demand entry (`transcode::transcode_movie`) into a throwaway identity
//! dir under `GUAC_EXTRACT_DIR`, then asked for again (kept). It prints the
//! voice choice, the outputs' sizes, the source's length against the
//! outputs' (by ffprobe, when `GUAC_FFPROBE` or the PATH has it), the wall
//! time and the load. The dir is removed after unless `AS_KEEP=1`, which
//! leaves it for AS.10's listen and prints where. `GUAC_AS_FMV` names the
//! movie (fmv11, the shortest with 13 tracks, by default). Ignored by
//! default: it reads `GUAC_GIANTS_DUMP`, needs ffmpeg (`GUAC_FFMPEG`, else
//! where guac-extract looks) and writes under `GUAC_EXTRACT_DIR`:
//!
//! `cargo test -p guac-extract --release --test giants_fmv -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::transcode::{self, Done};
use guac_extract::{ExtractRoot, open};
use guac_ir::VoiceTrack;

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

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// A file's length in seconds by ffprobe, if there's one to ask.
fn seconds(ffprobe: &str, path: &Path) -> Option<f64> {
    let out = std::process::Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of"])
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR, runs ffmpeg"]
fn a_cinematic_converts_in_english_and_is_kept() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let movie = env_or_dotenv("GUAC_AS_FMV").unwrap_or_else(|| "fmv11".into());
    let ffmpeg =
        Ffmpeg::locate(env_or_dotenv("GUAC_FFMPEG").map(PathBuf::from).as_deref()).expect("ffmpeg");
    let tmp = tempfile::Builder::new()
        .prefix("as9-fmv-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    let dir = &mut opened.dir;
    let all = transcode::cinematics(dir.manifest());
    println!("cinematics on the dump: {all:?}");
    assert_eq!(all.len(), 11);
    println!("load average at the start: {}", load_average());
    let t = Instant::now();
    let first = transcode::transcode_movie(dir, &movie, &ffmpeg, &|_| {}).unwrap();
    let took = t.elapsed();
    println!(
        "{movie}: {:?}, {:?}, {} + {} = {} bytes in {:.1} s; load average {}",
        first.voice,
        first.done,
        first.video,
        first.audio,
        first.bytes,
        took.as_secs_f64(),
        load_average()
    );
    assert_eq!(first.done, Done::Transcoded);
    if movie != "fmv12" {
        assert_eq!(
            first.voice,
            VoiceTrack::Stream {
                index: 4,
                bed: Some(13)
            }
        );
    }
    let src = dump.join(format!("PS3_GAME/USRDIR/movies/{movie}.mov"));
    let ffprobe = env_or_dotenv("GUAC_FFPROBE").unwrap_or_else(|| "ffprobe".into());
    let lengths = [
        seconds(&ffprobe, &src),
        seconds(&ffprobe, &dir.path().join(&first.video)),
        seconds(&ffprobe, &dir.path().join(&first.audio)),
    ];
    println!(
        "  lengths: source {:?} s, video {:?} s, audio {:?} s",
        lengths[0], lengths[1], lengths[2]
    );
    if let [Some(s), Some(v), Some(a)] = lengths {
        assert!((s - v).abs() < 0.2 && (s - a).abs() < 0.2, "{s} {v} {a}");
    }
    let again = transcode::transcode_movie(dir, &movie, &ffmpeg, &|_| {}).unwrap();
    assert_eq!(again.done, Done::Resumed);
    if std::env::var("AS_KEEP").is_ok_and(|v| v == "1") {
        let kept = tmp.keep();
        println!("  kept for listening: {}", kept.display());
    }
}
