//! The real Giants dump is the known one (openspec AB.4). Ignored by default:
//! it reads the dump at `GUAC_GIANTS_DUMP` (the folder holding `PS3_GAME`) and
//! makes a throwaway extract root under `GUAC_EXTRACT_DIR`, from the
//! environment or the workspace's `.env.dev`. A cold run (no manifest to cache
//! from) then a warm one; release mode, since debug SHA-256 is slow:
//!
//! `cargo test -p guac-extract --release --test giants_identity -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{DirState, ExtractError, ExtractRoot, open};

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

/// Every path under the dump with its size and mtime: the content side is
/// the manifest hash itself.
fn stat_tree(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, SystemTime)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let meta = std::fs::symlink_metadata(&p).unwrap();
        out.insert(p.clone(), (meta.len(), meta.modified().unwrap()));
        if meta.is_dir() {
            stat_tree(&p, out);
        }
    }
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn the_giants_dump_is_the_known_one() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ab4-identity-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();

    let mut before = BTreeMap::new();
    stat_tree(&dump, &mut before);

    let started = Instant::now();
    let cold = match open(&dump, &root, KNOWN_DUMPS, &|_| {}) {
        Ok(opened) => opened,
        Err(ExtractError::UnknownDump { identity, report }) => panic!(
            "unknown: {} {} {} after {:.1} s; report {}",
            identity.title_id,
            identity.app_ver,
            identity.manifest_hash,
            started.elapsed().as_secs_f64(),
            report.display()
        ),
        Err(e) => panic!("{e}"),
    };
    let cold_s = started.elapsed().as_secs_f64();
    assert_eq!(cold.state, DirState::Fresh);
    assert_eq!(cold.files_cached, 0);
    let m = cold.dir.manifest();
    assert_eq!(
        (m.identity.title_id.as_str(), m.identity.app_ver.as_str()),
        ("BLUS30968", "01.00")
    );
    println!(
        "identity {} {} {}",
        m.identity.title_id, m.identity.app_ver, m.identity.manifest_hash
    );
    println!(
        "cold: {} files, {} bytes hashed in {cold_s:.1} s ({:.0} MB/s)",
        cold.files_hashed,
        cold.bytes_hashed,
        cold.bytes_hashed as f64 / 1e6 / cold_s
    );

    let started = Instant::now();
    let warm = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let warm_s = started.elapsed().as_secs_f64();
    assert_eq!(warm.files_hashed, 0);
    assert_eq!(warm.files_cached, cold.files_hashed);
    assert_eq!(warm.dir.manifest().identity, m.identity);
    println!(
        "warm: {} files from the cache in {warm_s:.2} s",
        warm.files_cached
    );

    let mut after = BTreeMap::new();
    stat_tree(&dump, &mut after);
    assert_eq!(before.len(), after.len(), "no file added or removed");
    assert!(before == after, "a size or mtime under the dump changed");
    println!(
        "dump untouched: {} paths keep their sizes and mtimes",
        before.len()
    );
}
