//! Walks a directory of real `.sky` dumps and reports parse results.
//!
//! Usage: `cargo run --example validate_samples -p skylander-sky-parser -- <dir>`
//!
//! Answers PLAN 6.2.0: does RPCS3 emit plaintext tags that our parser reads
//! cleanly? For each `.sky` file we report figure_kind classification, the
//! CRC16 verdict on header + both dual-write areas, and basic identity bytes
//! so the output is cross-checkable against filenames.
//!
//! Samples are deliberately not committed (copyright + CLAUDE.md's no-`.sky`
//! rule). Point this at whatever local path holds them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skylander_sky_parser::{ParseError, parse};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "./dev-data/validation-figures".to_string());
    let root = PathBuf::from(&dir);
    if !root.exists() {
        eprintln!("directory not found: {}", root.display());
        std::process::exit(2);
    }

    let mut files = Vec::new();
    collect_sky_files(&root, &mut files);
    files.sort();

    let mut total = 0usize;
    let mut parse_ok = 0usize;
    let mut checksums_ok = 0usize;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    let mut bad_checksum: Vec<(PathBuf, String)> = Vec::new();

    println!(
        "{:<60} {:>10} {:>7} {:>10} {:>10} {:>10}",
        "file", "figure_id", "variant", "serial", "kind", "crc_ok"
    );
    println!("{}", "-".repeat(120));

    for path in &files {
        total += 1;
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                failures.push((path.clone(), format!("read error: {e}")));
                continue;
            }
        };
        match parse(&bytes) {
            Ok(stats) => {
                parse_ok += 1;
                let kind_label = format!("{:?}", stats.figure_kind);
                *by_kind.entry(kind_label.clone()).or_default() += 1;
                if stats.checksums_valid {
                    checksums_ok += 1;
                } else {
                    bad_checksum.push((
                        path.clone(),
                        format!(
                            "figure_id=0x{:06X} variant=0x{:04X} kind={}",
                            stats.figure_id.get(),
                            stats.variant.get(),
                            kind_label
                        ),
                    ));
                }
                let rel = path.strip_prefix(&root).unwrap_or(path);
                println!(
                    "{:<50} fid={:>8} var={:>6} lvl={:>3} xp11={:>6} xp13={:>7} gold={:>5} play_s={:>8} nick={:?} crc={}",
                    truncate(&rel.display().to_string(), 50),
                    format!("0x{:06X}", stats.figure_id.get()),
                    format!("0x{:04X}", stats.variant.get()),
                    stats.level,
                    stats.xp_2011,
                    stats.xp_2013,
                    stats.gold,
                    stats.playtime_secs,
                    stats.nickname,
                    if stats.checksums_valid { "ok" } else { "NO" },
                );
            }
            Err(e) => {
                failures.push((path.clone(), describe_parse_error(&e)));
            }
        }
    }

    println!();
    println!("{}", "=".repeat(60));
    println!("total files:             {total}");
    println!("parsed without error:    {parse_ok}");
    println!("checksums valid:         {checksums_ok}");
    println!("parse failures:          {}", failures.len());
    println!("checksum failures:       {}", bad_checksum.len());
    println!();
    println!("by figure_kind:");
    for (kind, n) in &by_kind {
        println!("  {kind:<16} {n}");
    }

    if !bad_checksum.is_empty() {
        println!();
        println!("--- checksum failures (parsed but CRC16 mismatch) ---");
        for (p, why) in &bad_checksum {
            println!("  {} :: {}", p.display(), why);
        }
    }
    if !failures.is_empty() {
        println!();
        println!("--- parse failures ---");
        for (p, why) in &failures {
            println!("  {} :: {}", p.display(), why);
        }
    }
}

fn collect_sky_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sky_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("sky") {
            out.push(path);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - (max - 1)..])
    }
}

fn describe_parse_error(e: &ParseError) -> String {
    format!("{e:?}")
}
