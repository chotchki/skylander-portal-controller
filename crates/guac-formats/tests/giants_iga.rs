//! Every IGA on the Giants dump through the reader (openspec AB.3). Ignored by
//! default: it reads the BLUS30968 dump at `GUAC_GIANTS_DUMP` (the folder that
//! holds `PS3_GAME`), from the environment or the workspace's `.env.dev`.
//! `GUAC_IGA_SAMPLE` is how many LZMA entries to unpack, 200 by default or
//! `all`. Release mode keeps the unpacking quick:
//!
//! `cargo test -p guac-formats --release --test giants_iga -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_formats::iga::{Archive, Compression};

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

fn usrdir() -> PathBuf {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let usrdir = Path::new(&root).join("PS3_GAME/USRDIR");
    assert!(usrdir.is_dir(), "no PS3_GAME/USRDIR under {root}");
    usrdir
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

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_giants_archive_opens_and_sampled_lzma_entries_unpack_to_size() {
    let usrdir = usrdir();
    let mut paths = Vec::new();
    archives_under(&usrdir, &mut paths);
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .arc or .bld under {}",
        usrdir.display()
    );

    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut bump = |key: String, by: u64| *counts.entry(key).or_default() += by;
    let mut open_failures = Vec::new();
    let mut lzma_entries = Vec::new();
    for (file, path) in paths.iter().enumerate() {
        let ext = path.extension().unwrap().to_str().unwrap().to_owned();
        bump(format!("archives .{ext}"), 1);
        let bytes = std::fs::read(path).unwrap();
        let archive = match Archive::parse(&bytes) {
            Ok(archive) => archive,
            Err(e) => {
                open_failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        for entry in archive.entries() {
            let kind = match entry.compression() {
                Compression::Stored => "stored".to_owned(),
                Compression::Lzma => {
                    lzma_entries.push((file, entry.index()));
                    bump("lzma chunks".into(), entry.chunk_count() as u64);
                    "lzma".to_owned()
                }
                Compression::Other(codec) => format!("codec {codec}"),
            };
            bump(format!("entries .{ext} {kind}"), 1);
            bump(format!("bytes {kind}"), entry.size().into());
        }
    }

    // A fixed seed, so a failure names the same entries on a rerun.
    let sample = match env_or_dotenv("GUAC_IGA_SAMPLE").as_deref() {
        Some("all") => lzma_entries.len(),
        Some(n) => n.parse().expect("GUAC_IGA_SAMPLE is a count or `all`"),
        None => 200,
    }
    .min(lzma_entries.len());
    let mut x = 0x9E37_79B9_7F4A_7C15_u64;
    for i in 0..sample {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = i + (x % (lzma_entries.len() - i) as u64) as usize;
        lzma_entries.swap(i, j);
    }
    let mut picked = lzma_entries[..sample].to_vec();
    picked.sort();

    let mut unpack_failures = Vec::new();
    let mut unpacked = 0u64;
    for group in picked.chunk_by(|a, b| a.0 == b.0) {
        let path = &paths[group[0].0];
        let bytes = std::fs::read(path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for (_, index) in group {
            let entry = &archive.entries()[*index];
            match archive.read(entry) {
                Ok(data) if data.len() == entry.size() as usize => unpacked += data.len() as u64,
                Ok(data) => unpack_failures.push(format!(
                    "{} {}: {} bytes, declared {}",
                    path.display(),
                    entry.name(),
                    data.len(),
                    entry.size()
                )),
                Err(e) => unpack_failures.push(format!("{} {}: {e}", path.display(), entry.name())),
            }
        }
    }

    println!("IGA on {}:", usrdir.display());
    for (key, value) in &counts {
        println!("  {key}: {value}");
    }
    println!("  open failures: {}", open_failures.len());
    println!(
        "  lzma entries unpacked: {sample} of {} ({unpacked} bytes), failures: {}",
        lzma_entries.len(),
        unpack_failures.len()
    );
    assert!(open_failures.is_empty(), "{open_failures:#?}");
    assert!(unpack_failures.is_empty(), "{unpack_failures:#?}");
}
