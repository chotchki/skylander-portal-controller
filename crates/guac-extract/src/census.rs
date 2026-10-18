//! The census (AB.5): every file under a dump and every entry of every IGA it
//! can read, sniffed by [`crate::magic`], with entropy for whatever has no
//! known magic and ffprobe's audio tracks for every Bink file. The report
//! (names, sizes, kinds, numbers; never content) goes into the extract root as
//! `census-<TITLE_ID>-<APP_VER>-<UTC stamp>.json`. The dump is only opened
//! with `File::open`.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use guac_formats::iga::{self, Archive};
use serde::Serialize;

use crate::dump::read_sfo;
use crate::error::{ExtractError, Result};
use crate::known::path_safe;
use crate::magic::{Entropy, Sniff, sniff};
use crate::root::{ExtractRoot, check_disjoint, enclosing_git_tree};

/// How much of a file or entry the sniff sees.
const HEAD: usize = 4096;
/// At or above this, an unknown file reads as compressed or encrypted.
pub const HIGH_ENTROPY: f64 = 7.9;
const READ_BUF: usize = 1 << 20;
/// A v0x08 IGA is read whole only up to this; a bigger one is recorded unread.
pub const MAX_WHOLE_ARCHIVE: u64 = 1 << 30;
const BINK: [&str; 2] = ["Bink 1 video", "Bink 2 video"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Game,
    Allowed,
    Unknown,
}

impl From<Sniff> for Class {
    fn from(s: Sniff) -> Self {
        match s {
            Sniff::Game(_) => Self::Game,
            Sniff::Allowed(_) => Self::Allowed,
            Sniff::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CensusFile {
    /// Relative to the dump, `/`-separated.
    pub path: String,
    pub size: u64,
    pub class: Class,
    pub what: &'static str,
    /// Bits per byte over the whole file, for unknowns only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entropy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iga: Option<IgaCensus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bink: Option<BinkProbe>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IgaCensus {
    /// Why the archive wasn't read, when it wasn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<String>,
    pub entries: usize,
    pub by_what: BTreeMap<&'static str, usize>,
    pub unknown: Vec<UnknownEntry>,
    /// Entries whose data didn't read, as `name: error`.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownEntry {
    pub name: String,
    pub size: u32,
    pub entropy: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BinkProbe {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoStream>,
    pub audio: Vec<AudioTrack>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoStream {
    pub codec: String,
    pub width: u64,
    pub height: u64,
    pub frame_rate: String,
    pub duration_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioTrack {
    /// ffprobe's stream index, what `-map 0:<index>` takes.
    pub index: u64,
    /// The container's own track id (Bink's per-track ids), as ffprobe prints it.
    pub id: Option<String>,
    pub codec: String,
    pub sample_rate: Option<u64>,
    pub channels: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
    pub files_by_what: BTreeMap<&'static str, usize>,
    /// Neither flagged nor allowed, as `(path, size, entropy)`.
    pub unknown_files: Vec<(String, u64, f64)>,
    pub high_entropy_unknown_files: Vec<String>,
    pub iga_archives: usize,
    pub iga_unread: Vec<(String, String)>,
    pub iga_entries: usize,
    pub entries_by_what: BTreeMap<&'static str, usize>,
    pub unknown_entries: usize,
    /// As `archive: entry`.
    pub high_entropy_unknown_entries: Vec<String>,
    pub entry_errors: usize,
    pub bink_files: usize,
    /// As `(path, audio tracks)`, for every Bink with more than one.
    pub multi_track_bink: Vec<(String, usize)>,
    /// Bink files ffprobe didn't answer for, with why.
    pub bink_unprobed: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Census {
    pub extractor: &'static str,
    pub dump_dir: String,
    pub title_id: String,
    pub app_ver: String,
    /// ffprobe's version line, or `None` when it wasn't found.
    pub ffprobe: Option<String>,
    pub high_entropy_threshold: f64,
    pub summary: Summary,
    pub files: Vec<CensusFile>,
}

/// `ffprobe` by ffmpeg's discovery order ([`crate::ffmpeg::find_tool`]):
/// `configured` if given, else beside this executable, else on PATH, else
/// (macOS) in Homebrew's bin folders.
pub fn find_ffprobe(configured: Option<PathBuf>) -> Option<PathBuf> {
    crate::ffmpeg::find_tool_here("ffprobe", configured.as_deref())
        .ok()
        .map(|f| f.path)
}

fn ffprobe_version(ffprobe: &Path) -> Option<String> {
    let out = Command::new(ffprobe).arg("-version").output().ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

/// Runs the census over `dump` (the folder holding `PS3_GAME`) and writes its
/// report into `root`. `ffprobe` of `None` leaves every Bink unprobed.
pub fn census(
    dump: &Path,
    root: &ExtractRoot,
    ffprobe: Option<&Path>,
) -> Result<(Census, PathBuf)> {
    // Again at use, as `open` does: the root may have been resolved long before.
    if let Some(repo) = enclosing_git_tree(root.path()) {
        return Err(ExtractError::InCheckout {
            root: root.path().to_path_buf(),
            repo,
        });
    }
    check_disjoint(root, dump)?;
    let census = run(dump, ffprobe)?;
    let path = write(&census, root)?;
    Ok((census, path))
}

/// The census without writing it.
pub fn run(dump: &Path, ffprobe: Option<&Path>) -> Result<Census> {
    // Absolute, so ffprobe never reads a path as a URL scheme or an option.
    let dump = &dump
        .canonicalize()
        .map_err(|e| ExtractError::unreadable(dump, e))?;
    let (title_id, app_ver) = read_sfo(dump)?;
    let version = ffprobe.and_then(ffprobe_version);
    let ffprobe = ffprobe.filter(|_| version.is_some());
    let listed = list(dump)?;

    let next = AtomicUsize::new(0);
    // One whole archive in memory at a time, in one reused buffer: a fresh
    // buffer per archive left macOS's footprint at the sum of the largest few.
    let whole = Mutex::new(Vec::new());
    let results = Mutex::new(Vec::with_capacity(listed.len()));
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 4)
        .min(listed.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((rel, abs)) = listed.get(i) else {
                        break;
                    };
                    let r = file_census(rel, abs, ffprobe, &whole);
                    let failed = r.is_err();
                    results.lock().unwrap().push((i, r));
                    if failed {
                        next.store(listed.len(), Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    let files = results
        .into_iter()
        .map(|(_, r)| r)
        .collect::<Result<Vec<_>>>()?;

    Ok(Census {
        extractor: env!("CARGO_PKG_VERSION"),
        dump_dir: dump.to_string_lossy().into_owned(),
        title_id,
        app_ver,
        ffprobe: version,
        high_entropy_threshold: HIGH_ENTROPY,
        summary: summarize(&files, ffprobe.is_some()),
        files,
    })
}

fn summarize(files: &[CensusFile], probed: bool) -> Summary {
    let mut s = Summary {
        files: files.len(),
        ..Summary::default()
    };
    for f in files {
        s.bytes += f.size;
        *s.files_by_what.entry(f.what).or_default() += 1;
        if let Some(e) = f.entropy {
            s.unknown_files.push((f.path.clone(), f.size, e));
            if e >= HIGH_ENTROPY {
                s.high_entropy_unknown_files.push(f.path.clone());
            }
        }
        if let Some(iga) = &f.iga {
            s.iga_archives += 1;
            if let Some(why) = &iga.unread {
                s.iga_unread.push((f.path.clone(), why.clone()));
            }
            s.iga_entries += iga.entries;
            for (what, n) in &iga.by_what {
                *s.entries_by_what.entry(what).or_default() += n;
            }
            s.unknown_entries += iga.unknown.len();
            s.high_entropy_unknown_entries.extend(
                iga.unknown
                    .iter()
                    .filter(|u| u.entropy >= HIGH_ENTROPY)
                    .map(|u| format!("{}: {}", f.path, u.name)),
            );
            s.entry_errors += iga.errors.len();
        }
        if BINK.contains(&f.what) {
            s.bink_files += 1;
            match &f.bink {
                Some(BinkProbe {
                    error: None, audio, ..
                }) => {
                    if audio.len() > 1 {
                        s.multi_track_bink.push((f.path.clone(), audio.len()));
                    }
                }
                Some(BinkProbe { error: Some(e), .. }) => {
                    s.bink_unprobed.push((f.path.clone(), e.clone()))
                }
                None => s.bink_unprobed.push((
                    f.path.clone(),
                    if probed { "not probed" } else { "no ffprobe" }.to_owned(),
                )),
            }
        }
    }
    s
}

/// Every regular file under `dump`, sorted by relative path. A symlink is
/// refused, never followed out of the dump.
fn list(dump: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![(dump.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| ExtractError::unreadable(&dir, e))? {
            let entry = entry.map_err(|e| ExtractError::unreadable(&dir, e))?;
            let abs = entry.path();
            let rel = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let meta = fs::symlink_metadata(&abs).map_err(|e| ExtractError::unreadable(&abs, e))?;
            if meta.is_dir() {
                stack.push((abs, format!("{rel}/")));
            } else if meta.is_file() {
                out.push((rel, abs));
            } else {
                return Err(ExtractError::unreadable(
                    &abs,
                    "not a regular file or folder",
                ));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_head(path: &Path) -> Result<(Vec<u8>, u64)> {
    let file = File::open(path).map_err(|e| ExtractError::unreadable(path, e))?;
    let size = file
        .metadata()
        .map_err(|e| ExtractError::unreadable(path, e))?
        .len();
    let mut head = Vec::with_capacity(HEAD);
    file.take(HEAD as u64)
        .read_to_end(&mut head)
        .map_err(|e| ExtractError::unreadable(path, e))?;
    Ok((head, size))
}

fn file_entropy(path: &Path) -> Result<f64> {
    let mut file = File::open(path).map_err(|e| ExtractError::unreadable(path, e))?;
    let mut buf = vec![0u8; READ_BUF];
    let mut e = Entropy::default();
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| ExtractError::unreadable(path, e))?;
        if n == 0 {
            return Ok(e.bits_per_byte());
        }
        e.update(&buf[..n]);
    }
}

fn file_census(
    rel: &str,
    abs: &Path,
    ffprobe: Option<&Path>,
    whole: &Mutex<Vec<u8>>,
) -> Result<CensusFile> {
    let (head, size) = read_head(abs)?;
    let s = sniff(&head, rel);
    let what = s.what();
    let entropy = match s {
        Sniff::Unknown => Some(file_entropy(abs)?),
        _ => None,
    };
    let readable =
        head.starts_with(iga::MAGIC) && head.get(4..8) == Some(&iga::VERSION.to_le_bytes());
    let iga = if readable && size > MAX_WHOLE_ARCHIVE {
        Some(IgaCensus {
            unread: Some(format!(
                "{size} bytes, over the census's {MAX_WHOLE_ARCHIVE}-byte whole-read cap"
            )),
            ..IgaCensus::default()
        })
    } else if readable {
        let mut buf = whole.lock().unwrap_or_else(|p| p.into_inner());
        buf.clear();
        buf.reserve_exact(size as usize);
        File::open(abs)
            .and_then(|f| f.take(size).read_to_end(&mut buf))
            .map_err(|e| ExtractError::unreadable(abs, e))?;
        Some(iga_census(&buf))
    } else if head.starts_with(iga::MAGIC) || head.starts_with(iga::MAGIC_BIG_ENDIAN) {
        // Another version or byte order: the reader refuses it from the header.
        Some(iga_census(&head))
    } else {
        None
    };
    let bink = match ffprobe {
        Some(ffprobe) if BINK.contains(&what) => Some(probe_bink(ffprobe, abs)),
        _ => None,
    };
    Ok(CensusFile {
        path: rel.to_owned(),
        size,
        class: s.into(),
        what,
        entropy,
        iga,
        bink,
    })
}

/// Every entry of one archive sniffed by its first chunk; an unknown one is
/// read whole for its entropy.
pub fn iga_census(bytes: &[u8]) -> IgaCensus {
    let archive = match Archive::parse(bytes) {
        Ok(a) => a,
        Err(e) => {
            return IgaCensus {
                unread: Some(e.to_string()),
                ..IgaCensus::default()
            };
        }
    };
    let mut c = IgaCensus {
        entries: archive.entries().len(),
        ..IgaCensus::default()
    };
    for entry in archive.entries() {
        let head = if entry.chunk_count() == 0 {
            Ok(Vec::new())
        } else {
            archive
                .read_chunk(entry, 0)
                .map(|chunk| chunk[..chunk.len().min(HEAD)].to_vec())
        };
        let head = match head {
            Ok(h) => h,
            Err(e) => {
                c.errors.push(format!("{}: {e}", entry.name()));
                continue;
            }
        };
        let s = sniff(&head, entry.name());
        *c.by_what.entry(s.what()).or_default() += 1;
        if s == Sniff::Unknown {
            let mut acc = Entropy::default();
            let read = archive
                .chunks(entry)
                .try_for_each(|chunk| chunk.map(|c| acc.update(&c)));
            match read {
                Ok(()) => c.unknown.push(UnknownEntry {
                    name: entry.name().to_owned(),
                    size: entry.size(),
                    entropy: acc.bits_per_byte(),
                }),
                Err(e) => c.errors.push(format!("{}: {e}", entry.name())),
            }
        }
    }
    c
}

fn probe_bink(ffprobe: &Path, file: &Path) -> BinkProbe {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-of", "json", "-show_entries"])
        .arg("stream=index,id,codec_type,codec_name,sample_rate,channels,width,height,r_frame_rate,duration")
        .arg(file)
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            return BinkProbe {
                error: Some(String::from_utf8_lossy(&o.stderr).trim().to_owned()),
                ..BinkProbe::default()
            };
        }
        Err(e) => {
            return BinkProbe {
                error: Some(e.to_string()),
                ..BinkProbe::default()
            };
        }
    };
    parse_ffprobe(&out).unwrap_or_else(|e| BinkProbe {
        error: Some(e),
        ..BinkProbe::default()
    })
}

/// ffprobe's `-of json` stream list, as the census keeps it.
pub fn parse_ffprobe(json: &[u8]) -> Result<BinkProbe, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let streams = v["streams"]
        .as_array()
        .ok_or("no streams in ffprobe's output")?;
    let text = |s: &serde_json::Value, k: &str| s[k].as_str().map(str::to_owned);
    // sample_rate comes as a string, channels as a number.
    let num = |s: &serde_json::Value, k: &str| {
        s[k].as_u64()
            .or_else(|| s[k].as_str().and_then(|t| t.parse().ok()))
    };
    let mut probe = BinkProbe::default();
    for s in streams {
        match s["codec_type"].as_str() {
            Some("video") if probe.video.is_none() => {
                probe.video = Some(VideoStream {
                    codec: text(s, "codec_name").unwrap_or_default(),
                    width: num(s, "width").unwrap_or(0),
                    height: num(s, "height").unwrap_or(0),
                    frame_rate: text(s, "r_frame_rate").unwrap_or_default(),
                    duration_s: text(s, "duration").and_then(|d| d.parse().ok()),
                })
            }
            Some("audio") => probe.audio.push(AudioTrack {
                index: num(s, "index").ok_or("an audio stream with no index")?,
                id: text(s, "id"),
                codec: text(s, "codec_name").unwrap_or_default(),
                sample_rate: num(s, "sample_rate"),
                channels: num(s, "channels"),
            }),
            _ => {}
        }
    }
    Ok(probe)
}

/// `census-<TITLE_ID>-<APP_VER>-<UTC stamp>[-N].json` in the root, written
/// to `.partial` and renamed. A title or version unfit for a file name is
/// named `unsafe` there; the JSON keeps the real one.
fn write(census: &Census, root: &ExtractRoot) -> Result<PathBuf> {
    let dir = root.path();
    fs::create_dir_all(dir).map_err(ExtractError::io(dir))?;
    let mut text = serde_json::to_string_pretty(census).expect("the census serializes");
    text.push('\n');
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let safe = |s: &str| {
        if path_safe(s) {
            s.to_owned()
        } else {
            "unsafe".to_owned()
        }
    };
    let base = format!(
        "census-{}-{}-{stamp}",
        safe(&census.title_id),
        safe(&census.app_ver)
    );
    let path = (0u32..)
        .map(|n| match n {
            0 => dir.join(format!("{base}.json")),
            n => dir.join(format!("{base}-{n}.json")),
        })
        .find(|p| !p.exists())
        .expect("u32 names run out");
    let partial = path.with_extension("json.partial");
    fs::write(&partial, text).map_err(ExtractError::io(&partial))?;
    fs::rename(&partial, &path).map_err(ExtractError::io(&path))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffprobe_streams_parse() {
        let json = br#"{"streams": [
            {"index": 0, "codec_name": "binkvideo", "codec_type": "video", "width": 1280,
             "height": 720, "id": "0x0", "r_frame_rate": "30/1", "duration": "12.5"},
            {"index": 1, "codec_name": "binkaudio_dct", "codec_type": "audio",
             "sample_rate": "44100", "channels": 1, "id": "0x1"},
            {"index": 2, "codec_name": "binkaudio_dct", "codec_type": "audio",
             "sample_rate": "44100", "channels": 2, "id": "0x5"}]}"#;
        let p = parse_ffprobe(json).unwrap();
        let v = p.video.unwrap();
        assert_eq!(
            (v.width, v.height, v.frame_rate.as_str()),
            (1280, 720, "30/1")
        );
        assert_eq!(v.duration_s, Some(12.5));
        assert_eq!(p.audio.len(), 2);
        assert_eq!(p.audio[1].index, 2);
        assert_eq!(p.audio[1].id.as_deref(), Some("0x5"));
        assert_eq!(
            (p.audio[1].sample_rate, p.audio[1].channels),
            (Some(44100), Some(2))
        );
        assert!(parse_ffprobe(b"{}").is_err());
        assert!(parse_ffprobe(b"not json").is_err());
    }

    #[test]
    fn a_big_endian_iga_is_reported_unread() {
        let c = iga_census(b"\x1aAGI\0\0\0\x0b");
        assert!(c.unread.unwrap().contains("big-endian"));
        assert_eq!(c.entries, 0);
    }
}
