//! The census over real dumps (openspec AB.5), ignored by default. The dumps
//! are every `GUAC_*_DUMP` but `GUAC_TEST_KNOWN_DUMP`, plus `GUAC_CENSUS_DUMPS`,
//! a path list of dumps or of folders holding dumps up to two levels down
//! (RPCS3's `game_discs` works), from the environment or the workspace's
//! `.env.dev`. Reports go into `GUAC_EXTRACT_DIR`; ffprobe is `GUAC_FFPROBE`,
//! else found the way ffmpeg is. Fails on a file neither flagged nor allowed, a
//! high-entropy unknown file or IGA entry, an unprobed Bink, and on Giants any
//! unknown entry or an archive or entry that didn't read:
//!
//! `cargo test -p guac-extract --release --test census_dumps -- --ignored --nocapture`

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use guac_extract::ExtractRoot;
use guac_extract::census::{census, find_ffprobe};

fn dotenv() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (k, v) = line.trim().split_once('=')?;
            Some((k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
        })
        .collect()
}

fn var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .or_else(|| dotenv().into_iter().find(|(k, _)| k == key).map(|(_, v)| v))
}

fn is_dump(dir: &Path) -> bool {
    dir.join("PS3_GAME/PARAM.SFO").is_file()
}

fn dumps_under(dir: &Path, depth: usize, out: &mut BTreeSet<PathBuf>) {
    if is_dump(dir) {
        out.insert(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            dumps_under(&e.path(), depth - 1, out);
        }
    }
}

fn dumps() -> BTreeSet<PathBuf> {
    // GUAC_TEST_KNOWN_DUMP is an identity line for a `test-hooks` guacamole
    // (openspec AB.11), not a folder.
    let is_dump_key =
        |k: &str| k.starts_with("GUAC_") && k.ends_with("_DUMP") && k != "GUAC_TEST_KNOWN_DUMP";
    let mut named: Vec<(String, String)> = dotenv()
        .into_iter()
        .filter(|(k, _)| is_dump_key(k))
        .collect();
    named.extend(std::env::vars().filter(|(k, _)| is_dump_key(k)));
    let mut out = BTreeSet::new();
    for (key, dir) in named {
        let dir = PathBuf::from(dir);
        assert!(
            is_dump(&dir),
            "{key}={} holds no PS3_GAME/PARAM.SFO",
            dir.display()
        );
        out.insert(dir);
    }
    if let Some(list) = var("GUAC_CENSUS_DUMPS") {
        for dir in std::env::split_paths(&list) {
            let before = out.len();
            dumps_under(&dir, 2, &mut out);
            assert!(out.len() > before, "no dump in {}", dir.display());
        }
    }
    out
}

#[test]
#[ignore = "reads every dump in GUAC_*_DUMP and GUAC_CENSUS_DUMPS, writes into GUAC_EXTRACT_DIR"]
fn every_file_of_every_dump_is_flagged_or_allowed() {
    let dumps = dumps();
    assert!(
        !dumps.is_empty(),
        "set GUAC_GIANTS_DUMP or GUAC_CENSUS_DUMPS"
    );
    let root =
        ExtractRoot::checked(var("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR")).unwrap();
    let ffprobe = find_ffprobe(var("GUAC_FFPROBE").map(PathBuf::from));
    let mut failures = Vec::new();
    for dump in &dumps {
        let started = std::time::Instant::now();
        let (c, report) = census(dump, &root, ffprobe.as_deref()).unwrap();
        let s = &c.summary;
        let tag = format!("{} {}", c.title_id, c.app_ver);
        println!(
            "\n== {tag}: {} ({:.1} s)",
            dump.display(),
            started.elapsed().as_secs_f64()
        );
        println!("report {}", report.display());
        println!("files {} ({} bytes)", s.files, s.bytes);
        for (what, n) in &s.files_by_what {
            println!("  {n:6}  {what}");
        }
        println!(
            "iga {} archives, {} unread, {} entries, {} unknown, {} entry errors",
            s.iga_archives,
            s.iga_unread.len(),
            s.iga_entries,
            s.unknown_entries,
            s.entry_errors
        );
        if let Some((_, why)) = s.iga_unread.first() {
            println!("  unread: {why}");
        }
        for (what, n) in &s.entries_by_what {
            println!("  {n:6}  {what}");
        }
        for (path, size, e) in &s.unknown_files {
            println!("unknown file {path} ({size} bytes, {e:.3} bits/byte)");
        }
        for name in &s.high_entropy_unknown_entries {
            println!("high-entropy unknown entry {name}");
        }
        println!(
            "bink {} files, {} multi-track",
            s.bink_files,
            s.multi_track_bink.len()
        );
        for (path, n) in &s.multi_track_bink {
            println!("  {n:3} tracks  {path}");
        }

        for (path, _, _) in &s.unknown_files {
            failures.push(format!("{tag}: {path} is neither flagged nor allowed"));
        }
        for path in &s.high_entropy_unknown_files {
            failures.push(format!("{tag}: {path} is high-entropy with no known magic"));
        }
        for name in &s.high_entropy_unknown_entries {
            failures.push(format!("{tag}: {name} is high-entropy with no known magic"));
        }
        for (path, why) in &s.bink_unprobed {
            failures.push(format!("{tag}: {path} not probed: {why}"));
        }
        if c.title_id == "BLUS30968" {
            for (path, why) in &s.iga_unread {
                failures.push(format!("{tag}: {path} unread: {why}"));
            }
            if s.unknown_entries > 0 {
                failures.push(format!(
                    "{tag}: {} IGA entries neither flagged nor allowed",
                    s.unknown_entries
                ));
            }
            if s.entry_errors > 0 {
                failures.push(format!("{tag}: {} entries didn't read", s.entry_errors));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
