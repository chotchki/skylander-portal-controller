//! AE.21's scroll tracks off the real Giants dump: every
//! `tfbScrollUVAnimTrack` in every character and item `level.bld` read for
//! what the scroll requirements of the character-shading spec lean on (each
//! track's start and fixed time unset, so it runs on the scene's clock; its
//! layer one of the three sets; no material with two on one set), then the
//! six hero materials AE.6 found with one, read through the model stage,
//! their IR tracks against the stored values and their transforms worked
//! from them; and every archive on the disc for the slide requirement's
//! clock (no track anywhere stores a start or a fixed time). What the
//! runtime makes of a track was read statically; the constants a scrolling
//! part draws with are AE.26's, on the oracle.
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` from the environment or
//! the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_scrolls -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{models, roster};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    Layout, MaterialAnim, MaterialAnimTrackList, MaterialUvAnimTrack, ScrollUvAnimData, read_all,
};
use guac_ir::model::Scroll;
use guac_ir::scroll::transforms;
use guac_ir::shading::UvTransform;
use guac_ir::{DumpIdentity, Sha256};

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

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap_or_else(|| panic!("no level.bld in {}", path.display()));
    archive.read(entry).unwrap().into_owned()
}

fn blds(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld"))
        .collect();
    paths.sort();
    paths
}

const SCROLL: &str = "tfbScrollUVAnimTrack";

#[derive(Default)]
struct Census {
    files: usize,
    tracks: usize,
    /// (start time, fixed time) as stored.
    clocks: BTreeMap<String, usize>,
    layers: BTreeMap<i32, usize>,
    turned: usize,
    swung: usize,
    /// Material animations driving one set from two scroll tracks.
    doubled: Vec<String>,
}

impl Census {
    fn take(&mut self, name: &str, level: &[u8]) {
        let igz = Igz::parse(level).unwrap();
        self.files += 1;
        for (i, o) in igz.objects().iter().enumerate() {
            if igz.type_name(o) != SCROLL {
                continue;
            }
            let t = MaterialUvAnimTrack::read(&igz, i).unwrap();
            let d = ScrollUvAnimData::read(&igz, t.data.expect("a scroll with data")).unwrap();
            self.tracks += 1;
            let clock = format!("start {} fixed {}", t.base_time, t.cur_time);
            *self.clocks.entry(clock).or_default() += 1;
            *self.layers.entry(t.layer).or_default() += 1;
            self.turned += usize::from(d.rotation_phase != 0.0);
            self.swung += usize::from(d.amplitude != [0.0, 0.0]);
        }
        for (i, anim) in read_all::<MaterialAnim>(&igz) {
            let Some(list) = anim.unwrap().track_list else {
                continue;
            };
            let mut sets = Vec::new();
            for track in MaterialAnimTrackList::read(&igz, list).unwrap().tracks {
                if igz.type_name(&igz.objects()[track]) == SCROLL {
                    sets.push(MaterialUvAnimTrack::read(&igz, track).unwrap().layer);
                }
            }
            let n = sets.len();
            sets.sort_unstable();
            sets.dedup();
            if sets.len() != n {
                self.doubled.push(format!("{name}: material anim #{i}"));
            }
        }
    }
}

fn close(a: UvTransform, b: UvTransform) -> bool {
    let pairs = a.u.iter().chain(&a.v).zip(b.u.iter().chain(&b.v));
    pairs.into_iter().all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_scroll_track_and_the_six_heroes() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let started = Instant::now();
    let mut census = Census::default();
    for dir in ["character", "item"] {
        for path in blds(&usrdir.join(dir)) {
            let name = format!("{dir}/{}", path.file_name().unwrap().to_string_lossy());
            census.take(&name, &level_bld(&path));
        }
    }
    println!(
        "{} character and item level.bld files, {} scroll tracks in {:.1} s",
        census.files,
        census.tracks,
        started.elapsed().as_secs_f64()
    );
    println!("  start and fixed times: {:?}", census.clocks);
    println!(
        "  layers {:?}; turned {}, swinging {}; one set driven twice: {:?}",
        census.layers, census.turned, census.swung, census.doubled
    );
    // No track stores a start time (one under 0 counts as 0) or a fixed
    // time (one under 0 runs), so its time is the scene's clock.
    assert_eq!(census.tracks, 408);
    let clocks: Vec<_> = census.clocks.iter().collect();
    assert_eq!(clocks, [(&"start -1 fixed -1".to_owned(), &408)]);
    assert_eq!(
        census.layers.into_iter().collect::<Vec<_>>(),
        [(0, 340), (1, 68)]
    );
    assert_eq!((census.turned, census.swung), (0, 43));
    assert!(census.doubled.is_empty());

    // The six hero materials with a track, through the model stage.
    let known = KNOWN_DUMPS[0];
    let identity = DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: Sha256::from_hex(known.manifest_hash).unwrap(),
    };
    let started = Instant::now();
    let r = roster::read(&dump, identity.clone()).unwrap();
    let mut scrolling: Vec<(String, String, u32, Vec<Scroll>)> = Vec::new();
    let mut models = 0;
    for load in models::character_loads(&r) {
        let archive = models::archive(&models::archive_path(&usrdir, load), load).unwrap();
        let level = models::level(&archive, load).unwrap();
        let x = models::read(&level, load, identity.clone()).unwrap();
        models += 1;
        for m in x.model.materials.iter().filter(|m| !m.scrolls.is_empty()) {
            println!(
                "  {load}: {} pixel {:#x} {:?}",
                m.name, m.pixel.base, m.scrolls
            );
            scrolling.push((load.into(), m.name.clone(), m.pixel.base, m.scrolls.clone()));
        }
    }
    println!(
        "{models} character models in {:.1} s, {} materials with a scroll track",
        started.elapsed().as_secs_f64(),
        scrolling.len()
    );
    let names: Vec<(&str, &str)> = scrolling
        .iter()
        .map(|(l, n, _, _)| (l.as_str(), n.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("105_FireFeet", "VFX_fireStuff_full_NoDrawBehind"),
            ("105_FireFeet", "VFX_fireStuff_NoDrawBehind"),
            ("105_FireFeet_Alt", "VFX_fireStuff_full_NoDrawBehind"),
            ("105_FireFeet_Alt", "VFX_fireStuff_NoDrawBehind"),
            ("109_Djini", "Genny_transparent_VFX"),
            ("109_Djini_Alt", "Genny_transparent_VFX"),
        ]
    );
    for (load, name, pixel, scrolls) in &scrolling {
        // A colour map with the rim and nothing else: the track slides the
        // colour map's coordinates, the first set.
        assert_eq!(*pixel, 0x4, "{load}: {name}");
        let [s] = scrolls.as_slice() else {
            panic!("{load}: {name}: {scrolls:?}")
        };
        let down = if load.starts_with("105_") {
            -1.869
        } else {
            0.77
        };
        // The IR's track is the disc's as stored.
        let stored = Scroll {
            layer: 0,
            scale: [1.0, 1.0],
            velocity: [0.0, down],
            amplitude: [0.0, 0.0],
            frequency: [0.0, 0.0],
            phase: [0.0, 0.0],
            rotation_phase: 0.0,
        };
        assert_eq!(*s, stored, "{load}: {name}");
        // Worked from the spec: v slides by the velocity's seconds, less
        // the whole part, and nothing else moves.
        for (t, v) in [
            (0.0, 0.0),
            (1.0, f64::from(down).fract()),
            (2.0, (2.0 * f64::from(down)).fract()),
        ] {
            let x = transforms(scrolls, t);
            let want = UvTransform {
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, v as f32],
            };
            assert!(close(x[0], want), "{load}: {name} at {t} s: {:?}", x[0]);
            assert_eq!(x[1..], [UvTransform::IDENTITY; 2]);
        }
    }
    let hot_dog = transforms(&scrolling[0].3, 1.0)[0];
    assert!((hot_dog.v[2] + 0.869).abs() < 1e-6);
}

/// The archives (`.bld` and `.arc`) in `dir`, sorted.
fn archives(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld" || e == "arc"))
        .collect();
    paths.sort();
    paths
}

/// Every archive on the disc: `USRDIR`'s and its folders' (no folder goes
/// deeper); what's left is the EBOOT, `alchemy.xml` and the movies.
fn every_archive(usrdir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(usrdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut paths = archives(usrdir);
    for dir in dirs {
        paths.extend(archives(&dir));
    }
    paths
}

/// One archive's scroll tracks as (start time, fixed time) pairs, and how
/// many igz files it holds.
fn archive_clocks(bytes: &[u8]) -> (Vec<(f32, f32)>, usize) {
    let archive = Archive::parse(bytes).unwrap();
    let mut clocks = Vec::new();
    let mut igzs = 0;
    for entry in archive.entries() {
        let Ok(data) = archive.read(entry) else {
            continue;
        };
        let Ok(igz) = Igz::parse(&data) else {
            continue;
        };
        igzs += 1;
        let Some(scroll) = igz.type_index(SCROLL) else {
            continue;
        };
        for (i, o) in igz.objects().iter().enumerate() {
            if o.type_index == scroll {
                let t = MaterialUvAnimTrack::read(&igz, i).unwrap();
                clocks.push((t.base_time, t.cur_time));
            }
        }
    }
    (clocks, igzs)
}

/// The slide requirement's clock off the whole disc: no scroll track in any
/// archive stores a start time (one under 0 counts as 0) or a fixed time (one
/// under 0 runs), so every one runs on the scene's clock as it stands.
#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn no_scroll_track_on_the_disc_stores_a_start_or_a_fixed_time() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let started = Instant::now();
    let paths = every_archive(&usrdir);
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::with_capacity(paths.len()));
    // Up to 4 threads: other work shares the machine.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(4));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let r = archive_clocks(&std::fs::read(path).unwrap());
                    found.lock().unwrap().push((i, r));
                }
            });
        }
    });
    let found = found.into_inner().unwrap();
    // Tracks per folder and archive kind, and per stored pair.
    let mut by_dir: BTreeMap<String, usize> = BTreeMap::new();
    let mut pairs: BTreeMap<String, usize> = BTreeMap::new();
    let mut igzs = 0;
    for (i, (clocks, n)) in &found {
        igzs += n;
        let rel = paths[*i].strip_prefix(&usrdir).unwrap();
        let dir = rel.parent().unwrap().display().to_string();
        let kind = rel.extension().unwrap().to_string_lossy();
        *by_dir.entry(format!("{dir} {kind}")).or_default() += clocks.len();
        for (start, fixed) in clocks {
            *pairs
                .entry(format!("start {start} fixed {fixed}"))
                .or_default() += 1;
        }
    }
    println!(
        "{} archives, {igzs} igz files in {:.1} s; scroll tracks per folder: {by_dir:?}; \
         start and fixed times: {pairs:?}",
        paths.len(),
        started.elapsed().as_secs_f64()
    );
    assert_eq!(paths.len(), 893);
    assert_eq!(igzs, 5801);
    let tracks: Vec<_> = by_dir.iter().filter(|(_, n)| **n > 0).collect();
    assert_eq!(
        tracks,
        [
            (&"character bld".to_owned(), &377),
            (&"item bld".to_owned(), &31),
            (&"level bld".to_owned(), &4130),
            (&"misc bld".to_owned(), &30),
            (&"permanent bld".to_owned(), &82),
        ]
    );
    assert_eq!(
        pairs.into_iter().collect::<Vec<_>>(),
        [("start -1 fixed -1".to_owned(), 4650)]
    );
}
