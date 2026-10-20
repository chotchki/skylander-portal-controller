//! The level stage on the real Giants dump (openspec AG.15): chapter 1
//! (`level/level_213_tutotial.bld`) after the roster and the models, into a
//! throwaway identity dir, then as a resume, once in the install format,
//! and as a whole first run after a layout-3 extract is rebuilt. Its
//! collision is checked against AG.2's read of the same file, its counts
//! against AG.3's and AG.15's census; its time, size and texture dedupe
//! against the characters' store are printed for AG.15's tick. Ignored by
//! default: it reads `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR`,
//! from the environment or the workspace's `.env.dev`; the whole run needs
//! ffmpeg (`GUAC_FFMPEG`, else where guac-extract looks):
//!
//! `cargo test -p guac-extract --release --test giants_level_stage -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::levels::{self, LEVELS};
use guac_extract::stages::{self, Event};
use guac_extract::{DirState, ExtractRoot, Stale, open};
use guac_formats::collision;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_ir::level::{Atmosphere, Collision, LevelMaterial, Lights, Placements, Props, Scene};
use guac_ir::model::{Blend, MaterialTrack};
use guac_ir::palette::LevelPalettes;

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

/// Files under `prefix` in the manifest, and their bytes.
fn recorded(prefix: &str, m: &guac_extract::Manifest) -> (usize, u64) {
    m.outputs
        .iter()
        .filter(|o| o.path.starts_with(prefix))
        .fold((0, 0), |(n, b), o| (n + 1, b + o.size))
}

fn read<T: serde::de::DeserializeOwned>(dir: &Path, rel: &str) -> T {
    let text = std::fs::read_to_string(dir.join(rel)).unwrap();
    guac_ir::from_ron(&text).unwrap()
}

/// `part`'s RON decoded and encoded as postcard is its postcard file, byte
/// for byte (postcard keeps each float's bits as they are).
fn agree<T: serde::de::DeserializeOwned + serde::Serialize>(dir: &Path, level: &str, part: &str) {
    let ron: T = read(dir, &IrFormat::Ron.level_path(level, part));
    let post = std::fs::read(dir.join(IrFormat::Postcard.level_path(level, part))).unwrap();
    assert!(guac_ir::to_postcard(&ron).unwrap() == post, "{part}");
    println!("  {part}: RON and postcard agree, {} bytes", post.len());
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn the_level_stage_extracts_chapter_1() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ag15-level-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let started = Instant::now();
    println!("load average at the start: {}", load_average());
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    let models =
        stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "roster {:.2} s, models {:.2} s ({} models, {} textures)",
        roster.took.as_secs_f64(),
        models.took.as_secs_f64(),
        models.models,
        models.textures
    );
    let dir = opened.dir.path().to_path_buf();

    // The stage, fresh, after the characters' store.
    let events = std::sync::Mutex::new(Vec::new());
    let stage = stages::level_stage(&mut opened.dir, IrFormat::Ron, &|e| {
        if let Event::Level { done, total } = e {
            events.lock().unwrap().push((done, total));
        }
    })
    .unwrap();
    println!(
        "level stage: {} level(s), {} parts read, {} kept, {} props ({} refused), {} textures ({} already in the store), {:?} in {:.2} s; load average {}",
        stage.levels,
        stage.parts_read,
        stage.parts_kept,
        stage.props,
        stage.refused_props.len(),
        stage.textures,
        stage.shared,
        stage.files,
        stage.took.as_secs_f64(),
        load_average()
    );
    for f in &stage.failed {
        println!("  FAILED {}", f.reason);
    }
    for (p, r) in &stage.refused_props {
        println!("  REFUSED PROP {p}: {r}");
    }
    assert!(stage.failed.is_empty(), "every part of level 213 reads");
    // AG.15's five, AG.41's atmosphere and AH.22's script and palettes.
    let parts = levels::PARTS.len();
    assert_eq!((stage.levels, stage.parts_read), (1, parts));
    let events = events.into_inner().unwrap();
    assert_eq!(events.first(), Some(&(0, parts)));
    assert_eq!(events.last(), Some(&(parts, parts)));
    let level = LEVELS[0];
    let m = opened.dir.manifest().clone();
    for part in levels::PARTS {
        let rel = levels::part_path(IrFormat::Ron, level, part);
        let size = m.outputs.iter().find(|o| o.path == rel).unwrap().size;
        let files = m.levels[level].parts[part].len();
        println!("  {part}: {rel} {size} bytes, {files} files recorded");
    }
    let (n_tex, tex_bytes) = recorded("textures/", &m);
    println!("  textures/ now {n_tex} files, {tex_bytes} bytes");
    assert_eq!(n_tex, models.textures + stage.textures - stage.shared);

    // The scene, against AG.3's census of the file and AG.15's of its
    // materials.
    let scene: Scene = read(&dir, &IrFormat::Ron.level_path(level, levels::SCENE));
    assert_eq!(scene.scenes.len(), 2);
    let mut effects: BTreeMap<String, usize> = BTreeMap::new();
    let (mut gloss, mut multiply, mut tracks, mut scrolls) = (0, 0, BTreeMap::new(), 0);
    for (k, g) in scene.scenes.iter().enumerate() {
        println!(
            "  scene {k} {}: {} materials, {} segments, {} vertices, {} triangles, {} sky groups over {} segments",
            g.world,
            g.materials.len(),
            g.segments.len(),
            g.vertices(),
            g.triangles(),
            g.sky.len(),
            g.sky.iter().map(|s| s.segments.len()).sum::<usize>()
        );
        for m in &g.materials {
            match m {
                LevelMaterial::Multi2(m) => {
                    *effects.entry("tfbmulti2".into()).or_default() += 1;
                    if m.pixel.base & 0x80 != 0 {
                        gloss += 1;
                    }
                    if m.states.blend == Blend::Multiply {
                        multiply += 1;
                    }
                    scrolls += m.scrolls.len();
                    for t in &m.tracks {
                        let kind = match t {
                            MaterialTrack::Transform { .. } => "transform",
                            MaterialTrack::FlipBook { .. } => "flip book",
                            MaterialTrack::Tint { .. } => "tint",
                        };
                        *tracks.entry(kind).or_insert(0) += 1;
                    }
                }
                LevelMaterial::Effect(e) => {
                    *effects.entry(format!("{:#x}", e.effect)).or_default() += 1;
                }
            }
        }
        // Every segment's material is there, and a level's carries no skin.
        for s in &g.segments {
            assert!(usize::from(s.material) < g.materials.len());
            assert!(s.joints.is_empty() && s.weights.is_empty());
        }
    }
    println!(
        "  scene materials {effects:?}; gloss {gloss}, multiply {multiply}, scrolls {scrolls}, tracks {tracks:?}"
    );
    let g = &scene.scenes[0];
    assert_eq!(g.world, "Levels/Level_213/Level_213_ART_MASTER.ma");
    assert_eq!((g.materials.len(), g.segments.len()), (478, 524));
    assert_eq!(g.triangles(), 395_214);
    assert_eq!(g.sky.len(), 7);
    assert_eq!(g.sky.iter().map(|s| s.segments.len()).sum::<usize>(), 16);
    let grass = &scene.scenes[1];
    assert_eq!((grass.segments.len(), grass.triangles()), (45, 37_740));
    assert_eq!(effects["tfbmulti2"], 471);
    assert_eq!(
        (effects["0xaa6"], effects["0xaa8"], effects["0xaa9"]),
        (45, 6, 1)
    );
    assert_eq!((gloss, multiply), (1, 2));

    // The props: every model a placement names, read or listed.
    let props: Props = read(&dir, &IrFormat::Ron.level_path(level, levels::PROPS));
    let skinned = props.props.iter().filter(|p| p.skeleton.is_some()).count();
    let idles = props.props.iter().filter(|p| p.idle.is_some()).count();
    let (pv, pt): (usize, usize) = props.props.iter().fold((0, 0), |(v, t), p| {
        (
            v + p.segments.iter().map(|s| s.positions.len()).sum::<usize>(),
            t + p
                .segments
                .iter()
                .map(|s| s.indices.len() / 3)
                .sum::<usize>(),
        )
    });
    let mut prop_effects: BTreeMap<String, usize> = BTreeMap::new();
    let mut prop_tracks: BTreeMap<&str, usize> = BTreeMap::new();
    for m in props.props.iter().flat_map(|p| &p.materials) {
        match m {
            LevelMaterial::Multi2(m) => {
                *prop_effects.entry("tfbmulti2".into()).or_default() += 1;
                for t in &m.tracks {
                    let kind = match t {
                        MaterialTrack::Transform { .. } => "transform",
                        MaterialTrack::FlipBook { .. } => "flip book",
                        MaterialTrack::Tint { .. } => "tint",
                    };
                    *prop_tracks.entry(kind).or_default() += 1;
                }
            }
            LevelMaterial::Effect(e) => {
                *prop_effects.entry(format!("{:#x}", e.effect)).or_default() += 1
            }
        }
    }
    println!(
        "  props: {} read ({skinned} skinned, {idles} with an idle), {} refused, {pv} vertices, {pt} triangles, materials {prop_effects:?}, tracks {prop_tracks:?}",
        props.props.len(),
        props.refused.len()
    );
    assert_eq!(props.props.len() + props.refused.len(), 219);

    // The collision, against AG.2's read of the same level.
    let archive =
        std::fs::read(levels::archive_path(&dump.join("PS3_GAME/USRDIR"), level)).unwrap();
    let arc = Archive::parse(&archive).unwrap();
    let bld = arc
        .read(arc.find("level.bld").unwrap())
        .unwrap()
        .into_owned();
    let igz = Igz::parse(&bld).unwrap();
    let ours: Collision = read(&dir, &IrFormat::Ron.level_path(level, levels::COLLISION));
    let theirs: Vec<_> = collision::worlds(&igz)
        .map(|(_, w)| w.unwrap())
        .filter(|w| w.collision.body.is_some())
        .collect();
    assert_eq!(ours.meshes.len(), theirs.len());
    assert_eq!(ours.meshes.len(), 1);
    for (o, t) in ours.meshes.iter().zip(&theirs) {
        let body = t.collision.body.as_ref().unwrap();
        assert_eq!(o.world, t.name().unwrap());
        // Bits, not values: `==` on floats would take -0.0 for 0.0.
        let bits =
            |v: &[[f32; 3]]| -> Vec<[u32; 3]> { v.iter().map(|p| p.map(f32::to_bits)).collect() };
        let world: Vec<[f32; 3]> = body.world_vertices().collect();
        assert_eq!(
            bits(&o.vertices),
            bits(&world),
            "vertex for vertex, bit for bit"
        );
        let tris: Vec<[u16; 3]> = body.mesh.triangles.iter().map(|t| t.indices).collect();
        assert_eq!(o.triangles, tris);
        assert_eq!(o.materials, body.mesh.materials);
        assert_eq!(
            (o.radius.to_bits(), o.welding),
            (body.mesh.radius.to_bits(), body.mesh.welding_type)
        );
        println!(
            "  collision {}: {} vertices, {} triangles, radius {}, welding {}: equal to AG.2's read",
            o.world,
            o.vertices.len(),
            o.triangles.len(),
            o.radius,
            o.welding
        );
        assert_eq!((o.vertices.len(), o.triangles.len()), (2_168, 3_080));
    }

    // The placements and lights.
    let placements: Placements = read(&dir, &IrFormat::Ron.level_path(level, levels::PLACEMENTS));
    let waypoints: usize = placements.waypoints.iter().map(Vec::len).sum();
    let with_model = placements
        .actors
        .iter()
        .filter(|a| a.model.is_some())
        .count();
    println!(
        "  placements: {} actors ({with_model} with a model), {waypoints} waypoints in {} lists",
        placements.actors.len(),
        placements.waypoints.len()
    );
    assert_eq!(placements.actors.len(), 997);
    assert_eq!((waypoints, placements.waypoints.len()), (743, 153));
    let init = placements
        .actors
        .iter()
        .find(|a| a.name == "213_Init_Location")
        .unwrap();
    let first = &placements.waypoints[init.waypoints.unwrap() as usize][0];
    assert_eq!(first.position, [10.191352, 43.37944, -93.393616]);
    for a in &placements.actors {
        if let Some(model) = &a.model {
            assert!(
                props.props.iter().any(|p| &p.path == model)
                    || props.refused.iter().any(|p| &p.path == model),
                "{model}"
            );
        }
    }
    let lights: Lights = read(&dir, &IrFormat::Ron.level_path(level, levels::LIGHTS));
    let names: Vec<&str> = lights.directional.iter().map(|l| l.name.as_str()).collect();
    println!(
        "  lights: {names:?}, placed {:?}",
        lights
            .placed
            .iter()
            .map(|l| (&l.name, l.light_type))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        names,
        [
            "_worldLight1",
            "_worldLight2",
            "_worldLight3",
            "_causticLight"
        ]
    );
    assert_eq!(lights.placed.len(), 1);
    assert_eq!(lights.placed[0].name, "213_EvilRobot_Beam_light");

    // A resume reads nothing and keeps every file.
    let again = stages::level_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "  resumed: {} parts kept, {:?} in {:.3} s",
        again.parts_kept,
        again.files,
        again.took.as_secs_f64()
    );
    assert_eq!((again.parts_read, again.parts_kept), (0, parts));
    assert_eq!(again.files.written, 0);

    // The install format writes only its IR, beside.
    let post = stages::level_stage(&mut opened.dir, IrFormat::Postcard, &|_| {}).unwrap();
    let m = opened.dir.manifest().clone();
    let postcard: u64 = m
        .outputs
        .iter()
        .filter(|o| o.path.starts_with("levels/") && o.path.ends_with(".postcard"))
        .map(|o| o.size)
        .sum();
    println!(
        "  postcard: {:?} in {:.2} s; the parts' IR {postcard} bytes",
        post.files,
        post.took.as_secs_f64()
    );
    // The script entry is the same file in either format: kept.
    assert_eq!(post.files.written, parts - 1);
    // Every part's RON, read back and put in postcard, is the postcard file
    // byte for byte: the two formats hold the same bits.
    agree::<Scene>(&dir, level, levels::SCENE);
    agree::<Props>(&dir, level, levels::PROPS);
    agree::<Collision>(&dir, level, levels::COLLISION);
    agree::<Placements>(&dir, level, levels::PLACEMENTS);
    agree::<Lights>(&dir, level, levels::LIGHTS);
    agree::<Atmosphere>(&dir, level, levels::ATMOSPHERE);
    agree::<LevelPalettes>(&dir, level, levels::PALETTES);
    drop(opened);

    // A layout-3 extract (AE.6's: models, no heroes, no level) is rebuilt,
    // and a whole first run, ffmpeg included, leaves it current: once.
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["layout"] = 3.into();
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Rebuilt(Stale::Layout { found: 3 }));
    let ffmpeg = env_or_dotenv("GUAC_FFMPEG").map(PathBuf::from);
    let run_started = Instant::now();
    let marks = std::sync::Mutex::new(Vec::new());
    let done = stages::run(&mut again.dir, ffmpeg.as_deref(), IrFormat::Ron, &|e| {
        let what = match e {
            Event::RosterReady(_) => "roster ready",
            Event::Models { done: 0, .. } => "models start",
            Event::Level { done: 0, .. } => "level start",
            Event::Indexing => "indexing",
            _ => return,
        };
        marks.lock().unwrap().push((what, run_started.elapsed()));
    })
    .unwrap();
    let total = run_started.elapsed();
    for (what, at) in marks.into_inner().unwrap() {
        println!("  first run: {what} at {:.2} s", at.as_secs_f64());
    }
    println!(
        "  first run: roster {:.2} s, models {:.2} s, level {:.2} s ({} parts, {:?}), intros {:?}, {:.1} s in all; load average {}",
        done.roster.took.as_secs_f64(),
        done.models.took.as_secs_f64(),
        done.levels.took.as_secs_f64(),
        done.levels.parts_read,
        done.levels.files,
        done.intros
            .as_ref()
            .map(|i| i.took.as_secs_f64())
            .map_err(|e| e.code()),
        total.as_secs_f64(),
        load_average()
    );
    assert!(done.levels.failed.is_empty());
    assert!(done.intros.is_ok(), "ffmpeg is needed for the whole run");
    let after = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    assert_eq!(after.state, DirState::Current, "re-extracted once");
    println!("took {:.1} s in all", started.elapsed().as_secs_f64());
}
