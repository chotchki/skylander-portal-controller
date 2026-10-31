//! The particle part on the real Giants dump (openspec AH.87): the stage
//! alone into a throwaway identity dir, then kept on a second run, then in
//! postcard agreeing with the RON byte for byte. Checked: every emitter of
//! `global.bld` (71) and Eruptor's archive (26) reads, every generator
//! textured with its texture in the store but the level-up arrow's (no
//! whole blocks across, so none a GPU takes), and the emitters his attacks
//! spawn hold the generators the disc gives them. Printed: the stage's time
//! and size with the load average. Ignored by default: it reads
//! `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR`, from the
//! environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_particles -- --ignored --nocapture`

use std::path::{Path, PathBuf};

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::particles::{self, particle_stage};
use guac_extract::{DirState, ExtractRoot, open};
use guac_ir::particle::{Blend, Emitters, Kind};

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

/// Each generator's kind, in list order.
fn kinds(e: &Emitters, name: &str) -> Vec<&'static str> {
    e.emitters
        .iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("no emitter {name}"))
        .generators
        .iter()
        .map(|g| match g.kind {
            Kind::Sprite(_) => "sprite",
            Kind::Ribbon(_) => "ribbon",
        })
        .collect()
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn every_emitter_the_spawn_archives_place_extracts_and_resumes() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ah87-particles-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    println!("load average at the start: {}", load_average());
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let dir = opened.dir.path().to_path_buf();

    let s = particle_stage(&mut opened.dir, IrFormat::Ron).unwrap();
    println!(
        "  first run: {} emitters, {} refused, {:?} in {:.2} s; load average {}",
        s.emitters,
        s.refused.len(),
        s.files,
        s.took.as_secs_f64(),
        load_average()
    );
    for (n, why) in &s.refused {
        println!("  REFUSED {n}: {why}");
    }
    assert!(s.read && s.failed.is_none(), "{:?}", s.failed);
    assert!(s.refused.is_empty());
    assert_eq!(s.emitters, 71 + 26);

    let read = |archive: &str| -> Emitters {
        let text =
            std::fs::read_to_string(dir.join(particles::path(IrFormat::Ron, archive))).unwrap();
        guac_ir::from_ron(&text).unwrap()
    };
    let global = read("permanent/global.bld");
    let eruptor = read("character/009_eruptor.bld");
    assert_eq!((global.emitters.len(), eruptor.emitters.len()), (71, 26));
    let m = opened.dir.manifest().clone();
    let mut textures = std::collections::BTreeSet::new();
    let mut generators = 0;
    let mut untextured = Vec::new();
    for e in global.emitters.iter().chain(&eruptor.emitters) {
        for g in &e.generators {
            generators += 1;
            let Some(t) = g.texture.as_ref() else {
                untextured.push(e.name.as_str());
                continue;
            };
            assert!(
                m.outputs.iter().any(|o| o.path == t.texture),
                "{}: {}",
                e.name,
                t.texture
            );
            textures.insert(t.texture.clone());
        }
    }
    // global.bld's lightning (the level-up's and the tether's) is kept by
    // name, not drawn.
    let unread: Vec<(&str, usize)> = global
        .emitters
        .iter()
        .filter(|e| !e.unread.is_empty())
        .map(|e| (e.name.as_str(), e.unread.len()))
        .collect();
    println!(
        "  {generators} generators, {} textures; unread {unread:?}",
        textures.len()
    );
    assert_eq!(generators, 168 + 84);
    assert_eq!(unread.len(), 2);
    // The level-up's arrow (109 x 128, no whole 4 x 4 blocks across) is
    // the one no GPU takes: drawn untextured, which draws nothing.
    assert_eq!(untextured, ["UI_LevelUp_ArrowParticle"]);

    // What his attacks spawn: the ball's trail and burst, the throw's flash,
    // the eruption's.
    for (name, want) in [
        ("009_LavaBall_Trail", &["sprite", "sprite", "ribbon"][..]),
        ("009_LavaBall_Detonate", &["sprite"; 5][..]),
        ("009_FireBlobFireSmoke", &["sprite"; 5][..]),
        ("009_Steam", &["sprite"; 2][..]),
        ("009_MuzzleFlash", &["sprite"; 2][..]),
        ("009_Eruption_head", &["sprite"; 3][..]),
        ("009_EruptionLavaSmokeSparks", &["sprite"; 2][..]),
        ("009_EruptionFireMini", &["sprite"; 1][..]),
    ] {
        assert_eq!(kinds(&eruptor, name), want, "{name}");
    }
    // Every one of his draws blended or added; none knocked out.
    assert!(
        eruptor
            .emitters
            .iter()
            .flat_map(|e| &e.generators)
            .all(|g| g.blend != Blend::Knockout)
    );
    drop(opened);

    // Kept on the next run, then in postcard agreeing with the RON.
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let k = particle_stage(&mut again.dir, IrFormat::Ron).unwrap();
    assert!(k.kept && !k.read);
    assert_eq!(k.files.written, 0);
    println!("  kept: {:?} in {:.3} s", k.files, k.took.as_secs_f64());
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    manifest["parts"]
        .as_object_mut()
        .unwrap()
        .remove(particles::PART);
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    drop(again);
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let p = particle_stage(&mut again.dir, IrFormat::Postcard).unwrap();
    assert!(p.read);
    for archive in particles::archives() {
        let ron = read(&archive);
        let post = std::fs::read(dir.join(particles::path(IrFormat::Postcard, &archive))).unwrap();
        assert!(guac_ir::to_postcard(&ron).unwrap() == post, "{archive}");
    }
    println!(
        "  postcard: {:?} in {:.2} s, agreeing with the RON; load average {}",
        p.files,
        p.took.as_secs_f64(),
        load_average()
    );
}
