//! The particle parts on the real Giants dump. The extract-wide one
//! (openspec AH.87): the stage
//! alone into a throwaway identity dir, then kept on a second run, then in
//! postcard agreeing with the RON byte for byte. Checked: every emitter of
//! `global.bld` (71) and Eruptor's archive (26) reads, every generator
//! textured with its texture in the store, the level-up arrow's (109 x 128,
//! no whole blocks across, which no GPU takes in DXT) stored decoded to
//! RGBA8 at its own size (AH.244), and the emitters his attacks spawn hold
//! the generators the disc gives them. Printed: the stage's time
//! and size with the load average.
//!
//! A level's own (openspec AS.17): every level file's placed emitters read
//! in memory through the level part's reader, a table of them per level
//! (emitters, read and refused with why, templates, standing, its scripts'
//! to wake, generators by kind, untextured, knocked out) and the totals;
//! then chapter 1 extracted on demand into a throwaway identity dir, its
//! particles part as the in-memory read gives it, and taken out of the
//! manifest alone to come back on the level's next ask, every other part
//! kept. Ignored by default: they read
//! `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR`, from the
//! environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --profile disc --test giants_particles -- --ignored --nocapture --test-threads=1`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::particles::{self, particle_stage};
use guac_extract::{DirState, ExtractRoot, open};
use guac_formats::igz::Igz;
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

    let s = particle_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
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
    // Every generator textured (AH.244): the level-up's arrow, 109 x 128 in
    // DXT blocks (no whole 4 x 4 across), which no GPU takes, is stored
    // decoded, an RGBA8 KTX2 at its own size.
    assert!(untextured.is_empty(), "{untextured:?}");
    let arrow = global
        .emitters
        .iter()
        .find(|e| e.name == "UI_LevelUp_ArrowParticle")
        .expect("the level-up's arrow");
    let t = arrow.generators[0].texture.as_ref().expect("textured");
    let ktx2 = std::fs::read(dir.join(&t.texture)).unwrap();
    let word = |at: usize| u32::from_le_bytes(ktx2[at..at + 4].try_into().unwrap());
    let (format, width, height, levels) = (word(12), word(20), word(24), word(40));
    println!(
        "  the arrow: {} ({width} x {height}, {levels} levels, VkFormat {format})",
        t.texture
    );
    // One level: a linear image, as every linear one on the disc is.
    assert_eq!((format, width, height, levels), (37, 109, 128, 1));
    let level0 = u64::from_le_bytes(ktx2[88..96].try_into().unwrap());
    assert_eq!(level0, 109 * 128 * 4, "RGBA8, 4 bytes a texel");

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
    let k = particle_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
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
    let p = particle_stage(&mut again.dir, IrFormat::Postcard, &|_| {}).unwrap();
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

/// One level's placed emitters, counted.
#[derive(Debug, Default, Clone)]
struct Count {
    emitters: usize,
    refused: usize,
    templates: usize,
    standing: usize,
    scripts: usize,
    sprites: usize,
    ribbons: usize,
    unread: usize,
    untextured: usize,
    knockout: usize,
    textures: usize,
}

impl Count {
    fn of(e: &guac_ir::particle::LevelEmitters, textures: usize) -> Self {
        let mut c = Count {
            emitters: e.emitters.len() + e.refused.len(),
            refused: e.refused.len(),
            textures,
            ..Count::default()
        };
        for p in &e.emitters {
            match (p.activity_state, p.activation_range > 0.0) {
                (5, _) => c.templates += 1,
                (4, true) => c.standing += 1,
                (4, false) if p.activation_range_buffer <= 0.0 => c.standing += 1,
                _ => c.scripts += 1,
            }
            c.unread += p.emitter.unread.len();
            for g in &p.emitter.generators {
                match g.kind {
                    Kind::Sprite(_) => c.sprites += 1,
                    Kind::Ribbon(_) => c.ribbons += 1,
                }
                c.untextured += usize::from(g.texture.is_none());
                c.knockout += usize::from(g.blend == Blend::Knockout);
            }
        }
        c
    }

    fn add(&mut self, o: &Count) {
        self.emitters += o.emitters;
        self.refused += o.refused;
        self.templates += o.templates;
        self.standing += o.standing;
        self.scripts += o.scripts;
        self.sprites += o.sprites;
        self.ribbons += o.ribbons;
        self.unread += o.unread;
        self.untextured += o.untextured;
        self.knockout += o.knockout;
        self.textures += o.textures;
    }
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn every_levels_own_emitters_read_and_land_with_the_level() {
    use guac_extract::levels;
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let identity = guac_ir::DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: guac_ir::Sha256([0; 32]),
    };
    println!("load average at the start: {}", load_average());
    let started = Instant::now();
    let mut names: Vec<String> = std::fs::read_dir(usrdir.join(levels::LEVEL_FOLDER))
        .unwrap()
        .filter_map(|e| {
            let n = e.ok()?.file_name().into_string().ok()?;
            n.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    names.sort();
    let mut table: BTreeMap<String, Count> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = Count::default();
    println!(
        "level\temitters\trefused\ttemplates\tstanding\tscripts\tsprites\tribbons\tunread\tuntextured\tknockout\ttextures"
    );
    for name in &names {
        let archive = std::fs::read(levels::archive_path(&usrdir, name)).unwrap();
        let entry = levels::level_bld(&archive).unwrap_or_else(|e| panic!("{name}: {e}"));
        let igz = Igz::parse(&entry).unwrap_or_else(|e| panic!("{name}: {e}"));
        let read = levels::particles(&igz, name, identity.clone()).unwrap();
        let c = Count::of(&read.ir, read.textures.len());
        for (n, why) in &read.ir.refused {
            println!("  REFUSED {name}: {n}: {why}");
            *reasons.entry(why.clone()).or_default() += 1;
        }
        println!(
            "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            c.emitters,
            c.refused,
            c.templates,
            c.standing,
            c.scripts,
            c.sprites,
            c.ribbons,
            c.unread,
            c.untextured,
            c.knockout,
            c.textures
        );
        total.add(&c);
        table.insert(name.clone(), c);
    }
    println!(
        "  {} levels: {total:?}; refused by why {reasons:?}; {:.1} s, load average {}",
        names.len(),
        started.elapsed().as_secs_f64(),
        load_average()
    );
    // AS.17's read of the disc (the research tool's table): 143 level
    // files, 15,383 placed emitters, every one reading; 12,986 templates,
    // 2,343 ranged and 54 without a range, 29 of those with a buffer,
    // waiting on their scripts; 36,368 sprite generators, 1,056 ribbons,
    // 558 lightning generators kept by name.
    assert_eq!(names.len(), 143);
    assert_eq!((total.emitters, total.refused), (15_383, 0));
    assert_eq!(total.templates, 12_986);
    assert_eq!((total.standing, total.scripts), (2_343 + 25, 29));
    assert_eq!(
        (total.sprites, total.ribbons, total.unread),
        (36_368, 1_056, 558)
    );
    let count = |name: &str| table[name].clone();
    let ch1 = count("level_213_tutotial");
    // Chapter 1: AG.7's live read found 188 emitters, 157 templates and 31
    // ranged.
    assert_eq!(
        (ch1.emitters, ch1.templates, ch1.standing, ch1.scripts),
        (188, 157, 31, 0)
    );
    let l214 = count("level_214_tutotial_2");
    assert_eq!(
        (l214.emitters, l214.templates, l214.standing, l214.scripts),
        (336, 211, 124, 1)
    );

    // Chapter 1 on demand, then its particles part taken out alone: it
    // comes back on the next ask, every other part kept.
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("as17-particles-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let name = levels::FIRST_RUN[0];
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    let first =
        guac_extract::stages::extract_level(&mut opened.dir, name, IrFormat::Ron, &|_| {}).unwrap();
    assert!(first.failed.is_empty(), "{:?}", first.failed);
    let rel = levels::part_path(IrFormat::Ron, name, levels::PARTICLES);
    let dir = opened.dir.path().to_path_buf();
    let archive = std::fs::read(levels::archive_path(&usrdir, name)).unwrap();
    let entry = levels::level_bld(&archive).unwrap();
    let igz = Igz::parse(&entry).unwrap();
    let read = levels::particles(&igz, name, opened.dir.manifest().identity.clone()).unwrap();
    assert!(
        std::fs::read(dir.join(&rel)).unwrap() == guac_ir::to_ron(&read.ir).unwrap().into_bytes(),
        "{rel} as the in-memory read"
    );
    let recorded = opened.dir.manifest().levels[name].parts[levels::PARTICLES].clone();
    for (path, ktx2) in &read.textures {
        assert!(recorded.contains(path), "{path} recorded");
        assert!(std::fs::read(dir.join(path)).unwrap() == *ktx2, "{path}");
    }
    let bytes: u64 = opened
        .dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| recorded.contains(&o.path))
        .map(|o| o.size)
        .sum();
    println!(
        "  chapter 1's particles part: {} files, {bytes} bytes ({} textures); the level {:?} in {:.2} s",
        recorded.len(),
        read.textures.len(),
        first.files,
        first.took.as_secs_f64()
    );
    drop(opened);
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    manifest["levels"][name]["parts"]
        .as_object_mut()
        .unwrap()
        .remove(levels::PARTICLES);
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let t = Instant::now();
    let resumed =
        guac_extract::stages::extract_level(&mut again.dir, name, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "  resumed: {} read, {} kept, {:?} in {:.3} s; load average {}",
        resumed.parts_read,
        resumed.parts_kept,
        resumed.files,
        t.elapsed().as_secs_f64(),
        load_average()
    );
    assert_eq!(
        (resumed.parts_read, resumed.parts_kept),
        (1, levels::PARTS.len() - 1)
    );
    assert!(
        again.dir.manifest().levels[name]
            .parts
            .contains_key(levels::PARTICLES)
    );
    assert!(
        std::fs::read(dir.join(&rel)).unwrap() == guac_ir::to_ron(&read.ir).unwrap().into_bytes()
    );
}
