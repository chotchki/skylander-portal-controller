//! AS.7: any named level off the Giants dump extracted end to end through
//! the on-demand entry (`stages::extract_level`), each into a throwaway
//! identity dir of its own under `GUAC_EXTRACT_DIR`, then again as a resume,
//! every part's file checked against the same level read in memory with the
//! dir's identity (its RON byte for byte, the script entry and each texture
//! it binds), the dir removed after. It prints the dump's level list by kind
//! off the dir's manifest (AS.1) and each level's parts, files, bytes, first
//! and resumed wall time with the load average beside. Ignored by default:
//! it reads `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR`, from the
//! environment or the workspace's `.env.dev`; `GUAC_AS_LEVELS` (a comma
//! list of archive names) picks the levels, chapter 1, a story level past
//! it, a PvP arena and a hub stage by default:
//!
//! `cargo test -p guac-extract --release --test giants_level_on_demand -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::levels::{self, Kind, Read};
use guac_extract::stages;
use guac_extract::{ExtractRoot, open};
use guac_formats::igz::Igz;

const DEFAULT: [&str; 4] = [
    "level_213_tutotial",
    "level_200",
    "pvp_level_001_cubedungeon",
    "level_hub_2_stage_1",
];

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

/// A part read in memory, as the stage writes it in RON: its IR's bytes and
/// its textures by path.
fn ron<T: serde::Serialize>(r: Result<Read<T>, String>) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    let r = r.expect("the part reads");
    (guac_ir::to_ron(&r.ir).unwrap().into_bytes(), r.textures)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn a_named_level_extracts_end_to_end_and_resumes() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let names: Vec<String> = env_or_dotenv("GUAC_AS_LEVELS")
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|| DEFAULT.map(str::to_owned).to_vec());
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let mut listed = false;
    for name in &names {
        let tmp = tempfile::Builder::new()
            .prefix("as7-level-")
            .tempdir_in(&base)
            .unwrap();
        let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
        let t = Instant::now();
        let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
        let opened_in = t.elapsed();
        let dir = &mut opened.dir;
        if !listed {
            listed = true;
            let list = levels::on_disc(dir.manifest());
            let mut kinds: BTreeMap<Kind, usize> = BTreeMap::new();
            for l in &list {
                *kinds.entry(l.kind).or_default() += 1;
            }
            println!("the dump's levels: {} by kind {kinds:?}", list.len());
            assert_eq!(list.len(), 143);
            assert_eq!(
                kinds,
                BTreeMap::from([
                    (Kind::Story, 20),
                    (Kind::Hub, 4),
                    (Kind::Pvp, 18),
                    (Kind::Challenge, 45),
                    (Kind::ArenaChallenge, 21),
                    (Kind::MeshBundle, 9),
                    (Kind::VoicePack, 1),
                    (Kind::ShipPart, 19),
                    (Kind::Title, 1),
                    (Kind::Credits, 1),
                    (Kind::Pack, 4),
                ])
            );
        }
        println!(
            "{name}: opened in {:.2} s; load average {}",
            opened_in.as_secs_f64(),
            load_average()
        );
        let first = stages::extract_level(dir, name, IrFormat::Ron, &|_| {}).unwrap();
        println!(
            "  first: {} parts read, {} failed, {} props ({} refused), {} textures, {:?} in {:.2} s; load average {}",
            first.parts_read,
            first.failed.len(),
            first.props,
            first.refused_props.len(),
            first.textures,
            first.files,
            first.took.as_secs_f64(),
            load_average()
        );
        for f in &first.failed {
            println!("  FAILED {}", f.reason);
        }
        assert!(first.failed.is_empty(), "{name}: every part reads");
        assert_eq!(first.parts_read, levels::PARTS.len());
        let again = stages::extract_level(dir, name, IrFormat::Ron, &|_| {}).unwrap();
        println!(
            "  resumed: {} kept, {:?} in {:.3} s",
            again.parts_kept,
            again.files,
            again.took.as_secs_f64()
        );
        assert_eq!(
            (again.parts_read, again.parts_kept),
            (0, levels::PARTS.len())
        );
        assert_eq!(again.files.written, 0);

        // Every file as the same level read in memory gives it.
        let identity = dir.manifest().identity.clone();
        let archive = std::fs::read(levels::archive_path(&usrdir, name)).unwrap();
        let entry = levels::level_bld(&archive).unwrap();
        let igz = Igz::parse(&entry).unwrap();
        let id = || identity.clone();
        let reads = [
            (levels::SCENE, ron(levels::scene(&igz, name, id()))),
            (levels::PROPS, ron(levels::props(&igz, name, id()))),
            (levels::COLLISION, ron(levels::collision(&igz, name, id()))),
            (
                levels::PLACEMENTS,
                ron(levels::placements(&igz, name, id())),
            ),
            (levels::LIGHTS, ron(levels::lights(&igz, name, id()))),
            (
                levels::ATMOSPHERE,
                ron(levels::atmosphere(&igz, name, id())),
            ),
            (levels::PALETTES, ron(levels::palettes(&igz, name, id()))),
            (levels::NODES, ron(levels::nodes(&igz, name, id()))),
        ];
        let m = dir.manifest().clone();
        let mut checked = 0;
        for (part, (bytes, textures)) in &reads {
            let rel = levels::part_path(IrFormat::Ron, name, part);
            assert!(
                std::fs::read(dir.path().join(&rel)).unwrap() == *bytes,
                "{rel}"
            );
            let recorded = &m.levels[name.as_str()].parts[*part];
            for (path, ktx2) in textures {
                assert!(recorded.contains(path), "{part}: {path} recorded");
                assert!(
                    std::fs::read(dir.path().join(path)).unwrap() == *ktx2,
                    "{path}"
                );
                checked += 1;
            }
        }
        let script = std::fs::read(dir.path().join(levels::script_path(name))).unwrap();
        assert!(script == entry, "the script entry is the disc's");
        let bytes: u64 = m.outputs.iter().map(|o| o.size).sum();
        println!(
            "  checked: every part's RON, the script entry and {checked} texture binds against the in-memory read; {} files, {bytes} bytes in the dir",
            m.outputs.len()
        );
        drop(opened);
        drop(tmp);
    }
}
