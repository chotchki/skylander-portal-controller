//! Every sound bank on the Giants dump (openspec AR.1, AR.2), and every
//! sound resolving to one (AR.3). Ignored by
//! default: it reads the BLUS30968 dump at `GUAC_GIANTS_DUMP` (the folder
//! that holds `PS3_GAME`), from the environment or the workspace's
//! `.env.dev`. A bank lives in one of two places: an `.arc` entry, or a
//! `tfbSoundDataInfo`'s `_data` inside an IGZ (a `.bld`'s, or one of the
//! IGZ some `.arc`s hold). Release mode keeps the LZMA quick, and four
//! threads share the archives:
//!
//! `cargo test -p guac-formats --release --test giants_sounds -- --ignored --nocapture --test-threads=1`
//!
//! `remuxed_banks_decode` also needs `GUAC_EXTRACT_DIR` (it writes its MP3s
//! into a throwaway folder there and removes it) and ffprobe and ffmpeg
//! (`GUAC_FFPROBE`, `GUAC_FFMPEG`, else on the PATH).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use common::{env_or_dotenv, usrdir};
use guac_formats::exports::Exports;
use guac_formats::fsb::{Bank, Codec};
use guac_formats::iga::Archive;
use guac_formats::igz::layouts::BuildDataInfo;
use guac_formats::igz::layouts::{Layout, SoundDataInfo};
use guac_formats::igz::{self, Igz, Location};
use guac_formats::mpeg;
use guac_formats::sound::{
    Bank as SoundBank, Data as SoundData, DataRef, Streams, data_infos, language_of, localized,
    resolve_global, resolve_pack, sounds,
};

/// Which of a bank's two homes it was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Home {
    /// An `.arc` entry of its own.
    Stream,
    /// Inside an IGZ, as a data info's `_data`.
    Embedded,
}

/// One bank, with where it came from.
struct Found<'a> {
    family: &'static str,
    archive: &'a Path,
    entry: &'a str,
    home: Home,
    bytes: &'a [u8],
}

/// The family an archive's path puts its sounds in (design decision 8).
fn family(usrdir: &Path, archive: &Path) -> &'static str {
    let rel = archive.strip_prefix(usrdir).unwrap();
    let folder = rel
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap();
    let name = rel.file_name().unwrap().to_str().unwrap();
    if name.starts_with("pvp_") {
        return "pvp";
    }
    match folder {
        "character" => "characters",
        "permanent" => "global",
        "item" => "item",
        "misc" => "misc",
        "outfit" => "outfit",
        "level" if name.starts_with("arena_") => "arena",
        "level" if name.starts_with("challenge") => "challenge",
        "level" => "story and hub",
        other => panic!("no family for {other}"),
    }
}

fn archives_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            archives_under(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("arc" | "bld")
        ) {
            out.push(path);
        }
    }
}

/// Every embedded bank in an IGZ: each `tfbSoundDataInfo` whose `_data` is
/// present. The data info's own layout is AR.3's; its parent's offset is
/// enough here.
fn embedded<'a>(file: &Igz<'a>) -> Vec<Result<&'a [u8], String>> {
    file.objects()
        .iter()
        .filter(|o| file.type_name(o) == "tfbSoundDataInfo")
        .filter_map(
            |o| match file.memory_ref(Location(o.at.0 + BuildDataInfo::DATA), "_data") {
                Ok(Some(m)) => Some(Ok(m.data)),
                Ok(None) => None,
                Err(e) => Some(Err(e.to_string())),
            },
        )
        .collect()
}

/// Walks every archive on four threads, calling `visit` on each bank and
/// collecting what wouldn't read (an archive, an entry, an IGZ, a data
/// info's `_data`) as failures.
fn walk(visit: impl Fn(Found<'_>) + Sync) -> (usize, Vec<String>) {
    let usrdir = usrdir();
    let mut paths = Vec::new();
    archives_under(&usrdir, &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no archives under {}", usrdir.display());
    let next = Mutex::new(0usize);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        *n += 1;
                        *n - 1
                    };
                    let Some(path) = paths.get(i) else { break };
                    let fail = |what: String| failures.lock().unwrap().push(what);
                    let bytes = std::fs::read(path).unwrap();
                    let archive = match Archive::parse(&bytes) {
                        Ok(a) => a,
                        Err(e) => {
                            fail(format!("{}: {e}", path.display()));
                            continue;
                        }
                    };
                    let fam = family(&usrdir, path);
                    for entry in archive.entries() {
                        let data = match archive.read(entry) {
                            Ok(d) => d,
                            Err(e) => {
                                fail(format!("{} {}: {e}", path.display(), entry.name()));
                                continue;
                            }
                        };
                        if data.starts_with(guac_formats::fsb::MAGIC) {
                            visit(Found {
                                family: fam,
                                archive: path,
                                entry: entry.name(),
                                home: Home::Stream,
                                bytes: &data,
                            });
                        } else if data.starts_with(igz::MAGIC) {
                            let file = match Igz::parse(&data) {
                                Ok(f) => f,
                                Err(e) => {
                                    fail(format!("{} {}: {e}", path.display(), entry.name()));
                                    continue;
                                }
                            };
                            for bank in embedded(&file) {
                                match bank {
                                    Ok(b) => visit(Found {
                                        family: fam,
                                        archive: path,
                                        entry: entry.name(),
                                        home: Home::Embedded,
                                        bytes: b,
                                    }),
                                    Err(e) => fail(format!(
                                        "{} {}: _data: {e}",
                                        path.display(),
                                        entry.name()
                                    )),
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    (paths.len(), failures.into_inner().unwrap())
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_bank_on_the_disc_opens() {
    let started = Instant::now();
    let counts: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    let refused = Mutex::new(Vec::new());
    let (archives, failures) = walk(|f| {
        let key = match f.home {
            Home::Stream => "streamed",
            Home::Embedded => "embedded",
        };
        match Bank::parse(f.bytes) {
            Ok(bank) => {
                let mut c = counts.lock().unwrap();
                *c.entry(format!("banks {key}")).or_default() += 1;
                *c.entry(format!("samples {key}")).or_default() += bank.samples.len() as u64;
                *c.entry(format!("bytes {key}")).or_default() += bank.len() as u64;
            }
            Err(e) => refused.lock().unwrap().push(format!(
                "{} {} ({key}): {e}",
                f.archive.display(),
                f.entry
            )),
        }
    });
    println!("{archives} archives in {:.1?}:", started.elapsed());
    for (k, v) in counts.lock().unwrap().iter() {
        println!("  {k}: {v}");
    }
    let refused = refused.into_inner().unwrap();
    println!("  refused: {}, unread: {}", refused.len(), failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(
        refused.is_empty(),
        "{:#?}",
        &refused[..refused.len().min(20)]
    );
    assert!(
        counts.lock().unwrap().get("banks streamed").copied() == Some(79_509),
        "AB.5's census counted 79,509 FSB5 entries"
    );
}

/// What the census counts per family and codec.
#[derive(Debug, Default)]
struct Row {
    banks: u64,
    streamed: u64,
    samples: u64,
    multi_sample_banks: u64,
    channels: BTreeMap<u32, u64>,
    rates: BTreeMap<u32, u64>,
    looped: u64,
    /// Seconds over every sample, and the longest.
    seconds: f64,
    longest: f64,
    /// MPEG: frames walked, samples they hold against the header's, the
    /// zero bytes between them, the stream's kinds.
    frames: u64,
    frame_minus_header: BTreeMap<i64, u64>,
    padding: u64,
    cut: BTreeMap<usize, u64>,
    kinds: BTreeMap<String, u64>,
    remux_failures: Vec<String>,
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn codec_census_by_family() {
    let started = Instant::now();
    let rows: Mutex<BTreeMap<(&'static str, Codec), Row>> = Mutex::new(BTreeMap::new());
    let refused = Mutex::new(Vec::new());
    let (archives, failures) = walk(|f| {
        let bank = match Bank::parse(f.bytes) {
            Ok(b) => b,
            Err(e) => {
                refused
                    .lock()
                    .unwrap()
                    .push(format!("{} {}: {e}", f.archive.display(), f.entry));
                return;
            }
        };
        // Walk the frames before taking the lock.
        let walked: Vec<_> = if bank.codec == Codec::Mpeg {
            bank.samples
                .iter()
                .map(|s| mpeg::remux(s.data, s.samples))
                .collect()
        } else {
            Vec::new()
        };
        let mut rows = rows.lock().unwrap();
        let row = rows.entry((f.family, bank.codec)).or_default();
        row.banks += 1;
        row.streamed += u64::from(f.home == Home::Stream);
        row.multi_sample_banks += u64::from(bank.samples.len() > 1);
        for (i, s) in bank.samples.iter().enumerate() {
            row.samples += 1;
            *row.channels.entry(s.channels).or_default() += 1;
            *row.rates.entry(s.rate).or_default() += 1;
            row.looped += u64::from(s.loop_points.is_some());
            let secs = f64::from(s.samples) / f64::from(s.rate);
            row.seconds += secs;
            row.longest = row.longest.max(secs);
            match walked.get(i) {
                Some(Ok(r)) => {
                    row.frames += r.frames as u64;
                    *row.frame_minus_header
                        .entry(r.samples as i64 - i64::from(s.samples))
                        .or_default() += 1;
                    row.padding += r.padding as u64;
                    if r.cut > 0 {
                        *row.cut.entry(r.cut).or_default() += 1;
                    }
                    if r.samples != u64::from(s.samples) {
                        row.remux_failures.push(format!(
                            "{} {}: frames hold {} samples, the header {}",
                            f.archive.display(),
                            f.entry,
                            r.samples,
                            s.samples
                        ));
                    }
                    let h = r.first;
                    *row.kinds
                        .entry(format!(
                            "{:?} {:?} {} kbit/s {} Hz {:?}",
                            h.version,
                            h.layer,
                            h.bitrate / 1000,
                            h.rate,
                            h.mode
                        ))
                        .or_default() += 1;
                    if h.mode.channels() != s.channels || h.rate != s.rate {
                        row.remux_failures.push(format!(
                            "{} {}: frames {} ch {} Hz, header {} ch {} Hz",
                            f.archive.display(),
                            f.entry,
                            h.mode.channels(),
                            h.rate,
                            s.channels,
                            s.rate
                        ));
                    }
                }
                Some(Err(e)) => {
                    row.remux_failures
                        .push(format!("{} {}: {e}", f.archive.display(), f.entry))
                }
                None => {}
            }
        }
    });
    println!("{archives} archives in {:.1?}", started.elapsed());
    let rows = rows.into_inner().unwrap();
    let mut remux_failures = 0;
    for ((family, codec), r) in &rows {
        println!(
            "{family} / {codec:?}: {} banks ({} streamed, {} embedded), {} samples, {} banks of more than one",
            r.banks,
            r.streamed,
            r.banks - r.streamed,
            r.samples,
            r.multi_sample_banks
        );
        println!("  channels {:?}, rates {:?}", r.channels, r.rates);
        println!(
            "  looped {}, {:.1} h in all, longest {:.1} s",
            r.looped,
            r.seconds / 3600.0,
            r.longest
        );
        if *codec == Codec::Mpeg {
            println!(
                "  frames {}, padding {} bytes, frame samples minus the header's {:?}",
                r.frames, r.padding, r.frame_minus_header
            );
            println!(
                "  cut last frames {} (bytes: {:?})",
                r.cut.values().sum::<u64>(),
                r.cut
            );
            println!("  streams {:?}", r.kinds);
            println!("  remux failures {}", r.remux_failures.len());
            for f in r.remux_failures.iter().take(5) {
                println!("    {f}");
            }
        }
        remux_failures += r.remux_failures.len();
    }
    let refused = refused.into_inner().unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(
        refused.is_empty(),
        "{:#?}",
        &refused[..refused.len().min(20)]
    );
    assert_eq!(remux_failures, 0, "every MPEG bank walks");
}

/// A remuxed bank written for the probe: what it was, where, and its
/// samples, rate and channels.
type Picked = (String, PathBuf, u32, u32, u32);

/// `GUAC_FFPROBE` or `GUAC_FFMPEG`, else the tool's name for the PATH.
fn tool(key: &str, name: &str) -> String {
    env_or_dotenv(key).unwrap_or_else(|| name.to_owned())
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR, runs ffprobe and ffmpeg"]
fn remuxed_banks_decode() {
    const PER_FAMILY: u64 = 24;
    let root = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    let dir = root.join(format!("ar2-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (ffprobe, ffmpeg) = (
        tool("GUAC_FFPROBE", "ffprobe"),
        tool("GUAC_FFMPEG", "ffmpeg"),
    );
    // Every 97th bank of a family until it has its share, and every 13th
    // stereo or other-rate one, so those are among them where the family
    // has any.
    let seen: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());
    let picked: Mutex<Vec<Picked>> = Mutex::new(Vec::new());
    let (_, failures) = walk(|f| {
        let Ok(bank) = Bank::parse(f.bytes) else {
            return;
        };
        let s = &bank.samples[0];
        let n = {
            let mut seen = seen.lock().unwrap();
            let n = seen.entry(f.family).or_default();
            *n += 1;
            *n
        };
        let mut picked = picked.lock().unwrap();
        let mine = picked.iter().filter(|p| p.0.starts_with(f.family)).count() as u64;
        let rare = (s.channels == 2 || s.rate != 44_100) && n % 13 == 0;
        if mine >= PER_FAMILY || !(n % 97 == 0 || rare) {
            return;
        }
        let r = mpeg::remux(s.data, s.samples).unwrap();
        let path = dir.join(format!(
            "{}-{}.mp3",
            f.family.replace(' ', "_"),
            picked.len()
        ));
        std::fs::write(&path, &r.bytes).unwrap();
        picked.push((
            format!("{} {} {}", f.family, f.archive.display(), f.entry),
            path,
            s.samples,
            s.rate,
            s.channels,
        ));
    });
    assert!(failures.is_empty(), "{failures:#?}");
    let picked = picked.into_inner().unwrap();
    let mut wrong = Vec::new();
    let mut per_family: BTreeMap<String, u64> = BTreeMap::new();
    for (what, path, samples, rate, channels) in &picked {
        *per_family
            .entry(what.split(" /").next().unwrap().to_owned())
            .or_default() += 1;
        let probe = std::process::Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-count_packets",
                "-show_entries",
                "stream=codec_name,sample_rate,channels,nb_read_packets",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .expect("ffprobe runs");
        let out = String::from_utf8_lossy(&probe.stdout).trim().to_owned();
        let frames_per = if *rate >= 32_000 { 1152 } else { 576 };
        let expected = format!("mp3,{rate},{channels},{}", samples / frames_per);
        if out != expected {
            wrong.push(format!("{what}: ffprobe {out:?}, the bank {expected:?}"));
        }
        let decode = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "null", "-"])
            .output()
            .expect("ffmpeg runs");
        if !decode.status.success() || !decode.stderr.is_empty() {
            wrong.push(format!(
                "{what}: ffmpeg {}",
                String::from_utf8_lossy(&decode.stderr)
            ));
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
    println!("{} banks remuxed and probed: {per_family:?}", picked.len());
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Each data info of a file, by object index: whether its bank opened.
type Banks = BTreeMap<usize, Result<(), String>>;

/// What a sound or a data info came to, for the AR.3 tally.
fn bump(counts: &Mutex<BTreeMap<String, u64>>, key: impl Into<String>) {
    *counts.lock().unwrap().entry(key.into()).or_default() += 1;
}

/// A streamed data info's bank: the entry its path names in `own` (its
/// archive's `.arc`), parsed; `Err` says what it found instead.
fn stream_bank(own: Option<&(Vec<u8>, Streams)>, path: &str) -> Result<(), String> {
    let Some((bytes, streams)) = own else {
        return Err("no .arc beside the archive".into());
    };
    let i = streams.find(path).map_err(|m| format!("{m:?}"))?;
    let arc = Archive::parse(bytes).map_err(|e| e.to_string())?;
    let entry = &arc.entries()[i];
    let data = arc.read(entry).map_err(|e| e.to_string())?;
    let bank = Bank::parse(&data).map_err(|e| e.to_string())?;
    if bank.samples.len() != 1 {
        return Err(format!("{} samples", bank.samples.len()));
    }
    Ok(())
}

/// An archive's `.arc` and its streams by path, if it has one.
fn arc_beside(bld: &Path) -> Option<(Vec<u8>, Streams)> {
    let bytes = std::fs::read(bld.with_extension("arc")).ok()?;
    let streams = {
        let arc = Archive::parse(&bytes).ok()?;
        Streams::new(arc.entries().iter().map(|e| (e.index(), e.name())))
    };
    Some((bytes, streams))
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_sound_resolves_to_a_bank() {
    let started = Instant::now();
    let usrdir = usrdir();
    // global.bld's exports and data infos, for the sounds that import theirs.
    let read_level = |rel: &str| {
        let bytes = std::fs::read(usrdir.join(rel)).unwrap();
        let arc = Archive::parse(&bytes).unwrap();
        let e = arc.find("level.bld").unwrap();
        arc.read(e).unwrap().into_owned()
    };
    let (boot, global) = (
        read_level("permanent/bootstrap.bld"),
        read_level("permanent/global.bld"),
    );
    let (boot_igz, global_igz) = (Igz::parse(&boot).unwrap(), Igz::parse(&global).unwrap());
    let exports = Exports::new(&boot_igz, &global_igz).unwrap();
    let global_arc = arc_beside(&usrdir.join("permanent/global.bld"));
    let global_data: BTreeMap<usize, Result<(), String>> = data_infos(&global_igz)
        .into_iter()
        .map(|(i, d)| {
            let r = match d {
                Ok(SoundData {
                    bank: Some(SoundBank::Embedded(b)),
                    ..
                }) => Bank::parse(b).map(drop).map_err(|e| e.to_string()),
                Ok(SoundData {
                    bank: Some(SoundBank::Streamed { path, .. }),
                    ..
                }) => stream_bank(global_arc.as_ref(), &path),
                Ok(_) => Err("no bank".into()),
                Err(e) => Err(e.to_string()),
            };
            (i, r)
        })
        .collect();

    let mut paths = Vec::new();
    archives_under(&usrdir, &mut paths);
    paths.retain(|p| p.extension().is_some_and(|e| e == "bld"));
    paths.sort();
    let counts: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    let misses: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let localized_missing: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        *n += 1;
                        *n - 1
                    };
                    let Some(path) = paths.get(i) else { break };
                    let rel = path.strip_prefix(&usrdir).unwrap().display().to_string();
                    let miss = |what: String| misses.lock().unwrap().push(format!("{rel} {what}"));
                    let own = arc_beside(path);
                    let bytes = std::fs::read(path).unwrap();
                    let archive = Archive::parse(&bytes).unwrap();
                    // Every IGZ entry, the packs first, so a sound reaching
                    // into ENGLISH.pak finds its data infos read.
                    let mut entries: Vec<(String, Vec<u8>)> = archive
                        .entries()
                        .iter()
                        .map(|e| (e.name().to_owned(), archive.read(e).unwrap().into_owned()))
                        .filter(|(_, d)| d.starts_with(igz::MAGIC))
                        .collect();
                    entries.sort_by_key(|(n, _)| !n.ends_with(".pak"));
                    let mut english: Option<(Vec<u8>, Banks)> = None;
                    let mut pack_names: BTreeMap<u32, String> = BTreeMap::new();
                    for (name, data) in &entries {
                        let file = Igz::parse(data).unwrap();
                        let language = name.strip_suffix(".pak").map(|l| l.to_ascii_lowercase());
                        let kind = match &language {
                            Some(l) if l == "english" => "english pack",
                            Some(_) => "other packs",
                            None if name == "level.bld" => "level.bld",
                            None => "other IGZ",
                        };
                        let mut local: BTreeMap<usize, Result<(), String>> = BTreeMap::new();
                        for (di, d) in data_infos(&file) {
                            let r = match d {
                                Ok(SoundData {
                                    bank: Some(SoundBank::Embedded(b)),
                                    ..
                                }) => {
                                    bump(&counts, format!("data embedded in {kind}"));
                                    Bank::parse(b).map(drop).map_err(|e| e.to_string())
                                }
                                Ok(SoundData {
                                    bank: Some(SoundBank::Streamed { path, .. }),
                                    ..
                                }) => {
                                    bump(&counts, format!("data streamed in {kind}"));
                                    let wanted = match language.as_deref() {
                                        Some(l) if l != "english" => localized(&path, l),
                                        _ => path.clone(),
                                    };
                                    if language.is_some() && language_of(&path).is_some() {
                                        bump(&counts, "data in a pack naming a language folder");
                                    }
                                    let r = stream_bank(own.as_ref(), &wanted);
                                    if r.is_err()
                                        && stream_bank(global_arc.as_ref(), &wanted).is_ok()
                                    {
                                        bump(&counts, "data streamed found only in global.arc");
                                    }
                                    r
                                }
                                Ok(_) => Err("no bank".into()),
                                Err(e) => Err(e.to_string()),
                            };
                            match &r {
                                // Another language's line the disc lacks:
                                // counted, not a miss of ours.
                                Err(why) if kind == "other packs" && why == "None" => {
                                    bump(&counts, "data in other packs with no entry");
                                    localized_missing
                                        .lock()
                                        .unwrap()
                                        .push(format!("{rel} {name}: data info #{di}"));
                                }
                                Err(why) => {
                                    miss(format!("{name}: data info #{di}: {why}"));
                                    bump(&counts, "data MISSED");
                                }
                                Ok(()) => {}
                            }
                            local.insert(di, r);
                        }
                        // Every pack's ids land on data infos, the same
                        // name at the same id in each.
                        if language.is_some() {
                            for id in 2.. {
                                let Some(o) = resolve_pack(&file, id) else {
                                    break;
                                };
                                let n = SoundDataInfo::read(&file, o)
                                    .ok()
                                    .and_then(|d| d.data_info.info.name)
                                    .unwrap_or("")
                                    .to_owned();
                                match pack_names.get(&id) {
                                    Some(seen) if *seen != n => {
                                        miss(format!(
                                            "{name}: pack id {id} names {n}, another pack {seen}"
                                        ));
                                    }
                                    Some(_) => {}
                                    None => {
                                        pack_names.insert(id, n);
                                    }
                                }
                            }
                        }
                        if kind == "english pack" {
                            english = Some((data.clone(), local.clone()));
                            continue;
                        }
                        if language.is_some() {
                            continue;
                        }
                        for (si, sound) in sounds(&file) {
                            let sound = match sound {
                                Ok(s) => s,
                                Err(e) => {
                                    miss(format!("{name}: sound #{si}: {e}"));
                                    bump(&counts, "sounds unread");
                                    continue;
                                }
                            };
                            bump(&counts, format!("sounds in {kind}"));
                            let r = match sound.data {
                                DataRef::Local(d) => {
                                    bump(&counts, "sounds data local");
                                    local
                                        .get(&d)
                                        .cloned()
                                        .unwrap_or(Err("not a data info".into()))
                                }
                                DataRef::Global(id) => {
                                    bump(&counts, "sounds data from global.bld");
                                    match resolve_global(&exports, id) {
                                        Some(o) => global_data.get(&o).cloned().unwrap_or(Err(
                                            format!("global #{o} isn't a data info"),
                                        )),
                                        None => Err(format!("export id {id:#x} lands on none")),
                                    }
                                }
                                DataRef::Pack(id) => {
                                    bump(&counts, "sounds data from the language pack");
                                    match &english {
                                        Some((bytes, data)) => {
                                            let pack = Igz::parse(bytes).unwrap();
                                            match resolve_pack(&pack, id) {
                                                Some(o) => {
                                                    // Does the data's file name the sound's?
                                                    let n = SoundDataInfo::read(&pack, o)
                                                        .ok()
                                                        .and_then(|d| d.data_info.info.name)
                                                        .unwrap_or("");
                                                    let stem = n.rsplit('/').next().unwrap_or("");
                                                    let same = sound.info.name.is_some_and(|s| {
                                                        stem.eq_ignore_ascii_case(&format!(
                                                            "{s}.wav"
                                                        ))
                                                    });
                                                    bump(
                                                        &counts,
                                                        if same {
                                                            "sounds pack data named as the sound"
                                                        } else {
                                                            "sounds pack data named otherwise"
                                                        },
                                                    );
                                                    data.get(&o)
                                                        .cloned()
                                                        .unwrap_or(Err("not read".into()))
                                                }
                                                None => Err(format!("pack id {id} lands on none")),
                                            }
                                        }
                                        None => Err("no ENGLISH.pak".into()),
                                    }
                                }
                                DataRef::None => {
                                    bump(&counts, "sounds data null");
                                    Ok(())
                                }
                                DataRef::Other => Err("data ref of another kind".into()),
                            };
                            if let Err(why) = r {
                                miss(format!("{name}: sound #{si} {:?}: {why}", sound.info.name));
                                bump(&counts, "sounds MISSED");
                            }
                        }
                    }
                }
            });
        }
    });
    println!("{} .bld in {:.1?}:", paths.len(), started.elapsed());
    for (k, v) in counts.lock().unwrap().iter() {
        println!("  {k}: {v}");
    }
    let mut localized_missing = localized_missing.into_inner().unwrap();
    localized_missing.sort();
    println!(
        "  other languages' lines with no entry: {:#?}",
        localized_missing
    );
    let misses = misses.into_inner().unwrap();
    println!("  misses: {}", misses.len());
    for m in misses.iter().take(40) {
        println!("    {m}");
    }
    assert!(misses.is_empty(), "{} misses", misses.len());
    // Dutch lacks 2 lines, Swedish 2, Portuguese 4 and Hispanic 7: the
    // entry counts AR.3 read (6,318, 6,318, 6,316 and 6,313 of 6,320).
    assert_eq!(localized_missing.len(), 15);
}
