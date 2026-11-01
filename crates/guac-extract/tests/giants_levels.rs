//! AS.2's whole-disc level read: every `level/*.bld` on the Giants dump
//! through each part's reader as the level stage runs it, encoded in memory,
//! nothing written. Per level and part it prints pass or the reason, the
//! part's bytes (its postcard IR, or the raw entry for the script, plus the
//! textures it binds) and a hash over its RON, its postcard and its
//! textures, so a run before a change and one after compare part by part.
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` from the environment or
//! the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --profile disc --test giants_levels -- --ignored --nocapture`
//!
//! Lines start with a tag for grepping: `PART` (level, part, ok or FAIL,
//! bytes, hash, reason), `LEVEL` (level, kind, parts passing, bytes, read
//! time), `REFUSED` (a prop the props part lists) and `TOTAL`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::levels::{self, Read};
use guac_formats::igz::Igz;
use guac_ir::{DumpIdentity, Sha256};
use sha2::Digest;

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

/// A fixed identity: the parts carry it, and two runs compare only if it
/// holds still.
fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

/// One part read: its bytes and hash, or why not.
struct Part {
    bytes: u64,
    hash: String,
    refused: Vec<(String, String)>,
}

fn hex(d: &[u8]) -> String {
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A read part encoded both ways, its textures beside it.
fn encoded<T: serde::Serialize>(r: Result<Read<T>, String>) -> Result<Part, String> {
    let r = r?;
    let ron = guac_ir::to_ron(&r.ir).map_err(|e| e.to_string())?;
    let post = guac_ir::to_postcard(&r.ir).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    h.update(ron.as_bytes());
    h.update(&post);
    let mut textures = r.textures;
    textures.sort_by(|a, b| a.0.cmp(&b.0));
    let mut bytes = post.len() as u64;
    for (path, b) in &textures {
        h.update(path.as_bytes());
        h.update(b);
        bytes += b.len() as u64;
    }
    Ok(Part {
        bytes,
        hash: hex(&h.finalize()),
        refused: Vec::new(),
    })
}

/// Every part of one level, in the stage's order.
fn read_level(
    igz: &Igz<'_>,
    entry: &[u8],
    level: &str,
) -> Vec<(&'static str, Result<Part, String>)> {
    let id = identity;
    let mut out = Vec::new();
    for part in levels::PARTS {
        let read = match part {
            levels::SCENE => encoded(levels::scene(igz, level, id())),
            levels::PROPS => {
                let r = levels::props(igz, level, id());
                let refused = r
                    .as_ref()
                    .map(|r| {
                        r.ir.refused
                            .iter()
                            .map(|p| (p.path.clone(), p.reason.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                encoded(r).map(|mut p| {
                    p.refused = refused;
                    p
                })
            }
            levels::COLLISION => encoded(levels::collision(igz, level, id())),
            levels::PLACEMENTS => encoded(levels::placements(igz, level, id())),
            levels::LIGHTS => encoded(levels::lights(igz, level, id())),
            levels::ATMOSPHERE => encoded(levels::atmosphere(igz, level, id())),
            levels::SCRIPT => levels::script(igz).map(|_| Part {
                bytes: entry.len() as u64,
                hash: hex(&sha2::Sha256::digest(entry)),
                refused: Vec::new(),
            }),
            levels::PALETTES => encoded(levels::palettes(igz, level, id())),
            levels::NODES => encoded(levels::nodes(igz, level, id())),
            other => Err(format!("no reader for {other}")),
        };
        out.push((part, read));
    }
    out
}

/// The reason, on one line, cut to a readable length.
fn one_line(s: &str) -> String {
    let s = s.replace(['\n', '\t'], " ");
    if s.len() > 300 {
        let mut cut = 300;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &s[..cut])
    } else {
        s
    }
}

/// The parts the disc's levels still don't read, each a box of its own in
/// openspec AS: chapter 203's definition switches zones in its STARTUP
/// (AS.23) and PvP arena 4's copies one zone's lights into another (AS.22).
const KNOWN: [(&str, &str); 2] = [
    ("level_203_the_machine", levels::ATMOSPHERE),
    ("pvp_level_004_ancientsewer", levels::ATMOSPHERE),
];

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_level_reads_part_by_part() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let mut names: Vec<String> = std::fs::read_dir(usrdir.join(levels::LEVEL_FOLDER))
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().ok()?;
            name.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    names.sort();
    println!("load average at the start: {}", load_average());
    let started = Instant::now();
    let mut failed: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failing: Vec<(String, &'static str)> = Vec::new();
    let mut levels_whole = 0;
    let mut total_bytes = 0u64;
    for level in &names {
        let t = Instant::now();
        let archive = std::fs::read(levels::archive_path(&usrdir, level)).unwrap();
        let read = levels::level_bld(&archive).and_then(|entry| {
            let igz = Igz::parse(&entry).map_err(|e| e.to_string())?;
            Ok(read_level(&igz, &entry, level))
        });
        let secs = t.elapsed().as_secs_f64();
        let parts = match read {
            Ok(parts) => parts,
            Err(e) => {
                println!(
                    "LEVEL\t{level}\t{:?}\t0/{}\t0\t{secs:.2}\t{}",
                    levels::Kind::of(level),
                    levels::PARTS.len(),
                    one_line(&e)
                );
                *failed.entry("archive").or_default() += 1;
                failing.push((level.clone(), "archive"));
                continue;
            }
        };
        let mut ok = 0;
        let mut bytes = 0;
        for (part, r) in &parts {
            match r {
                Ok(p) => {
                    ok += 1;
                    bytes += p.bytes;
                    println!("PART\t{level}\t{part}\tok\t{}\t{}\t", p.bytes, p.hash);
                    for (path, why) in &p.refused {
                        println!("REFUSED\t{level}\t{path}\t{}", one_line(why));
                    }
                }
                Err(e) => {
                    *failed.entry(part).or_default() += 1;
                    failing.push((level.clone(), part));
                    println!("PART\t{level}\t{part}\tFAIL\t0\t-\t{}", one_line(e));
                }
            }
        }
        if ok == parts.len() {
            levels_whole += 1;
        }
        total_bytes += bytes;
        println!(
            "LEVEL\t{level}\t{:?}\t{ok}/{}\t{bytes}\t{secs:.2}\t",
            levels::Kind::of(level),
            parts.len()
        );
    }
    let mut kinds: BTreeMap<levels::Kind, usize> = BTreeMap::new();
    for level in &names {
        *kinds.entry(levels::Kind::of(level)).or_default() += 1;
    }
    println!("KINDS\t{kinds:?}");
    println!(
        "TOTAL\t{} levels\t{levels_whole} whole\t{total_bytes} bytes\t{:.1} s\tfailures by part {failed:?}\tload average at the end: {}",
        names.len(),
        started.elapsed().as_secs_f64(),
        load_average()
    );
    assert_eq!(names.len(), 143, "the disc's level files");
    let known: Vec<(String, &str)> = KNOWN.iter().map(|(l, p)| ((*l).to_owned(), *p)).collect();
    assert_eq!(
        failing, known,
        "every level's every part reads, bar the boxes'"
    );
}
