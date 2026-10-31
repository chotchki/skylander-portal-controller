//! The sounds part on the real Giants dump (openspec AR.5, AR.6): the roster's
//! stage and then the part alone into a throwaway identity dir, then kept
//! on a second run. Checked: `global.bld`, the 67 modelled characters and
//! level 213 read with none missing, every sound of the 69 resolving to an
//! audio file the part holds (none unresolved, none whose bank didn't
//! read), each audio file an MPEG stream on disk, and every sound answered
//! by `guac-sound`'s lookup over the part. Printed: the part's
//! time, size and file count with the load average, the audio's share and
//! the largest file. Ignored by default: it reads `GUAC_GIANTS_DUMP` and
//! writes under `GUAC_EXTRACT_DIR` (removed after), from the environment
//! or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_sounds -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::sounds::{self, AUDIO_DIR, PART, sound_stage};
use guac_extract::{DirState, ExtractRoot, open, stages};
use guac_ir::sound::{DataHome, DataRef, Sounds};

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

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_sounds_chapter_1_plays_extract_and_resolve() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ar5-sounds-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    println!("load average at the start: {}", load_average());
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let dir = opened.dir.path().to_path_buf();
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();

    println!("load average before the part: {}", load_average());
    let s = sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    println!(
        "  first run: {} archives, {} sounds, {} data infos, {} audio files; {:?} (audio {:?}) in {:.2} s; load average {}",
        s.archives,
        s.sounds,
        s.data,
        s.audio,
        s.files,
        s.audio_files,
        s.took.as_secs_f64(),
        load_average()
    );
    assert!(s.failed.is_none(), "{:?}", s.failed);
    assert!(s.missing.is_empty(), "{:?}", s.missing);
    assert_eq!(s.archives, 1 + 67 + 1);
    for (what, why) in s.refused.iter().take(20) {
        println!("  refused: {what}: {why}");
    }
    assert!(s.refused.is_empty(), "{} refused", s.refused.len());

    // Every sound resolves to an audio file the part holds.
    let part = opened.dir.manifest().parts[PART].clone();
    let archives = sounds::archives(&roster.roster);
    let read = |a: &str| -> Sounds {
        IrFormat::Ron
            .decode(&opened.dir, &sounds::path(IrFormat::Ron, a))
            .unwrap()
    };
    let global = read(&archives[0]);
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut music = Vec::new();
    for a in &archives {
        let ir = read(a);
        for snd in &ir.sounds {
            let data = match snd.data {
                DataRef::Here(home, o) => {
                    *kinds
                        .entry(match home {
                            DataHome::Level => "level.bld",
                            DataHome::Pack => "ENGLISH.pak",
                        })
                        .or_default() += 1;
                    ir.data_at(home, o)
                }
                DataRef::Global(o) => {
                    *kinds.entry("global.bld").or_default() += 1;
                    global.data_at(DataHome::Level, o)
                }
                DataRef::None => {
                    *kinds.entry("none").or_default() += 1;
                    continue;
                }
                DataRef::Unresolved => panic!("{a}: {:?} unresolved", snd.name),
            };
            let audio = data
                .and_then(|d| d.audio.as_ref())
                .unwrap_or_else(|| panic!("{a}: {:?} has no audio", snd.name));
            assert!(part.contains(&audio.file), "{}", audio.file);
            if (snd.properties >> 1) & 0x1F == 28 {
                music.push((snd.name.clone(), audio.samples, audio.rate));
            }
        }
    }
    println!("  sounds by where their data is: {kinds:?}");
    println!("  music: {music:?}");
    assert!(
        music
            .iter()
            .any(|m| m.0.as_deref() == Some("mu_213_explore"))
    );

    // Every sound answered by the lookup over the part (AR.6).
    let book = guac_sound::SoundBook::new(archives.iter().map(|a| read(a)));
    let mut answers: BTreeMap<String, usize> = BTreeMap::new();
    let (mut rumble_is_length, mut rumble_differs) = (0, Vec::new());
    for a in &archives {
        let stem = guac_sound::stem(a);
        let ir = book.file(&stem).expect("every archive in the book");
        for snd in &ir.sounds {
            let key = match book.lookup(&stem, snd.object) {
                guac_sound::Lookup::Playable(p) => {
                    assert!(part.contains(&p.file.to_owned()), "{}", p.file);
                    if (p.params.rumble_duration - p.duration).abs() < 1e-4 {
                        rumble_is_length += 1;
                    } else if rumble_differs.len() < 5 {
                        rumble_differs.push((
                            snd.name.clone(),
                            p.params.rumble_duration,
                            p.duration,
                        ));
                    }
                    "playable".to_owned()
                }
                guac_sound::Lookup::Silent { why, .. } => format!("silent {why:?}"),
                guac_sound::Lookup::Unknown => "unknown".to_owned(),
            };
            *answers.entry(key).or_default() += 1;
        }
    }
    println!("  lookups: {answers:?}");
    println!(
        "  rumble duration the recording's length on {rumble_is_length} playable sounds; others, first five: {rumble_differs:?}"
    );
    assert_eq!(answers.get("unknown"), None);
    assert_eq!(
        answers.get("playable").copied().unwrap_or(0)
            + answers.get("silent NoData").copied().unwrap_or(0),
        s.sounds,
        "every sound plays but those with no data"
    );

    // Size, and every audio file an MPEG stream at its recorded size.
    let mut audio_bytes = 0u64;
    let mut ir_bytes = 0u64;
    let mut largest = (String::new(), 0u64);
    let mut files = 0;
    for o in &opened.dir.manifest().outputs {
        if !part.contains(&o.path) {
            continue;
        }
        files += 1;
        if o.path.starts_with(AUDIO_DIR) {
            audio_bytes += o.size;
            if o.size > largest.1 {
                largest = (o.path.clone(), o.size);
            }
            let head = std::fs::read(dir.join(&o.path)).unwrap();
            assert_eq!(head.len() as u64, o.size);
            assert_eq!(head[0], 0xFF, "{}", o.path);
        } else {
            ir_bytes += o.size;
        }
    }
    println!(
        "  part: {files} files, {:.1} MiB ({:.1} MiB audio in {} files, {:.2} MiB IR); largest {} at {:.2} MiB",
        (audio_bytes + ir_bytes) as f64 / 1048576.0,
        audio_bytes as f64 / 1048576.0,
        s.audio,
        ir_bytes as f64 / 1048576.0,
        largest.0,
        largest.1 as f64 / 1048576.0
    );

    // A second run finds the part whole and writes nothing.
    let again = sound_stage(&mut opened.dir, &roster.roster, IrFormat::Ron).unwrap();
    println!(
        "  second run: kept {}, {:?} in {:.2} s; load average {}",
        again.kept,
        again.files,
        again.took.as_secs_f64(),
        load_average()
    );
    assert!(again.kept);
    assert_eq!(again.files.written, 0);
    assert_eq!(again.files.kept, part.len());
    drop(opened);
    drop(tmp);
    println!("load average at the end: {}", load_average());
}
