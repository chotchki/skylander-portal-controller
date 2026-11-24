//! The lips part over every level archive on the disc (openspec AS.820): each
//! level's voice lines' viseme streams and its placed models' viseme sets,
//! read as the level stage reads them. Every stream and every set must
//! read; it prints, per level, the lines with a stream, the faces (by
//! model file) and the shapes a face holds, so "who else speaks" is a
//! count. Ignored by default: it reads `GUAC_GIANTS_DUMP` from the
//! environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --profile disc --test giants_mouths -- --ignored --nocapture`

use std::path::{Path, PathBuf};

use guac_extract::levels;
use guac_formats::igz::Igz;
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

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

#[test]
#[ignore = "reads the Giants dump"]
fn every_levels_lines_and_faces_read() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let (global, global_refused) = levels::global_lines(&usrdir).unwrap();
    println!(
        "GLOBAL\tlines {}\trefused {}",
        global.len(),
        global_refused.len()
    );
    assert!(global_refused.is_empty(), "{global_refused:#?}");
    assert!(!global.is_empty());
    let (mut lines, mut faces, mut refused) = (0usize, 0usize, Vec::new());
    let mut names: Vec<String> = std::fs::read_dir(usrdir.join(levels::LEVEL_FOLDER))
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "bld").then(|| p.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    for level in &names {
        let level = level.as_str();
        let archive = std::fs::read(levels::archive_path(&usrdir, level)).unwrap();
        let Ok(entry) = levels::level_bld(&archive) else {
            println!("LIPS\t{level}\tno level.bld");
            continue;
        };
        let igz = Igz::parse(&entry).unwrap();
        let pack = guac_extract::interface::english_pack(&archive).unwrap();
        let pack = pack.as_deref().map(|p| Igz::parse(p).unwrap());
        let r = levels::lips(&igz, pack.as_ref(), &[], level, identity())
            .unwrap()
            .ir;
        let shapes: Vec<String> = r
            .faces
            .iter()
            .map(|f| {
                let file = f.owner.rsplit(['/', '\\']).next().unwrap_or(&f.owner);
                format!("{file}:{}", f.shapes.len())
            })
            .collect();
        println!(
            "LIPS\t{level}\tlines {}\tfaces {}\t{}",
            r.lines.len(),
            r.faces.len(),
            shapes.join(" ")
        );
        for f in &r.faces {
            assert_eq!(f.shapes.len(), 9, "{level} {}: a shape per track", f.owner);
        }
        for l in &r.lines {
            assert!(
                l.visemes.blocks.iter().all(|b| b.len() == 9),
                "{level} {:?}: nine tracks a block",
                l.name
            );
        }
        lines += r.lines.len();
        faces += r.faces.len();
        refused.extend(
            r.refused
                .iter()
                .map(|(n, why)| format!("{level} {n}: {why}")),
        );
    }
    println!(
        "TOTAL\tlines {lines}\tfaces {faces}\trefused {}",
        refused.len()
    );
    for r in &refused {
        println!("REFUSED\t{r}");
    }
    // The one set on the disc that doesn't read: Kaos' on the final level,
    // a shape bound past his skeleton (as-mouths, 2026-11-24). He has no
    // face there; every other stream and set must read.
    let known = |r: &String| {
        r.starts_with("level_211_final_level ")
            && r.contains("EvilPortalMaster_kaos.mdl")
            && r.contains("transformTrackToBoneIndices")
    };
    assert!(refused.iter().all(known), "{refused:#?}");
    assert!(lines > 0 && faces > 0);
}
