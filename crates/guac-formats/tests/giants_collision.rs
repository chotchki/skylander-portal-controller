//! AG.2's collision reader off the Giants dump: every `tfbPhysicsWorld` in
//! every `level/*.bld` read through to its mesh, ag-levels' census
//! reproduced (75 worlds, 68 bodies, 140,603 triangles, 94,181 vertices),
//! then chapter 1's body pinned (level 213: 2,168 vertices, 3,080
//! triangles, its translation, bounds and materials).
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_collision -- --ignored --nocapture --test-threads 1`

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use common::usrdir;
use guac_formats::collision::{self, Body};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;

/// Chapter 1, "Time of the Giants" (ag-motion section 4).
const LEVEL_213: &str = "level/level_213_tutotial.bld";

/// Every `level/*.bld`, sorted so the printed rows come out in the same
/// order on every run.
fn levels(usrdir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(usrdir.join("level"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bld"))
        .collect();
    out.sort();
    out
}

/// Runs `f` on every archive, on up to 4 threads (other work shares the
/// machine), and returns the per-archive results in path order.
fn each_archive<T: Send>(paths: &[PathBuf], f: impl Fn(&Path, &[u8]) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(paths.len()));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(4));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let bytes = std::fs::read(path).unwrap();
                    let r = f(path, &bytes);
                    out.lock().unwrap().push((i, r));
                }
            });
        }
    });
    let mut out = out.into_inner().unwrap();
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

/// One world as the census prints it.
#[derive(Debug, Clone)]
struct WorldRow {
    archive: String,
    name: String,
    body: Option<BodyRow>,
}

#[derive(Debug, Clone)]
struct BodyRow {
    vertices: usize,
    triangles: usize,
    mopp_bytes: usize,
    translation: [f32; 3],
    local: ([f32; 3], [f32; 3]),
    world: ([f32; 3], [f32; 3]),
    materials: BTreeMap<u8, usize>,
    radius: f32,
    welding_type: u8,
    /// Triangles whose welding word isn't zero.
    welded: usize,
    /// Triangles naming one vertex twice.
    degenerate: usize,
}

fn bounds(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    points.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), v| {
        (
            std::array::from_fn(|i| lo[i].min(v[i])),
            std::array::from_fn(|i| hi[i].max(v[i])),
        )
    })
}

fn row(b: &Body) -> BodyRow {
    let m = &b.mesh;
    BodyRow {
        vertices: m.vertices.len(),
        triangles: m.triangles.len(),
        mopp_bytes: b.mopp.code_len,
        translation: b.translation,
        local: bounds(m.vertices.iter().copied()),
        world: bounds(b.world_vertices()),
        materials: m.material_counts(),
        radius: m.radius,
        welding_type: m.welding_type,
        welded: m.triangles.iter().filter(|t| t.welding != 0).count(),
        degenerate: m
            .triangles
            .iter()
            .filter(|t| {
                let [a, b, c] = t.indices;
                a == b || b == c || a == c
            })
            .count(),
    }
}

#[derive(Debug, Default)]
struct LevelRun {
    archive: String,
    has_level: bool,
    worlds: Vec<WorldRow>,
    failures: Vec<String>,
}

fn read_level(usrdir: &Path, path: &Path, bytes: &[u8]) -> LevelRun {
    let mut run = LevelRun {
        archive: path.strip_prefix(usrdir).unwrap().display().to_string(),
        ..LevelRun::default()
    };
    let arc = Archive::parse(bytes).unwrap();
    let Some(entry) = arc.find("level.bld") else {
        return run;
    };
    run.has_level = true;
    let data = arc.read(entry).unwrap();
    let igz = Igz::parse(&data).unwrap();
    for (i, w) in collision::worlds(&igz) {
        match w {
            Ok(w) => run.worlds.push(WorldRow {
                archive: run.archive.clone(),
                name: w.name().unwrap_or("").to_owned(),
                body: w.collision.body.as_ref().map(row),
            }),
            Err(e) => run
                .failures
                .push(format!("{}: object {i}: {e}", run.archive)),
        }
    }
    run
}

fn close(a: [f32; 3], b: [f64; 3], eps: f64) -> bool {
    (0..3).all(|i| (f64::from(a[i]) - b[i]).abs() <= eps)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_collision_census_reproduces() {
    let start = Instant::now();
    let usrdir = usrdir();
    let paths = levels(&usrdir);
    let runs = each_archive(&paths, |path, bytes| read_level(&usrdir, path, bytes));

    let with_level = runs.iter().filter(|r| r.has_level).count();
    let worlds: Vec<&WorldRow> = runs.iter().flat_map(|r| &r.worlds).collect();
    let failures: Vec<&String> = runs.iter().flat_map(|r| &r.failures).collect();
    let bodies: Vec<&BodyRow> = worlds.iter().filter_map(|w| w.body.as_ref()).collect();
    let sum = |f: fn(&BodyRow) -> usize| bodies.iter().map(|b| f(b)).sum::<usize>();
    let max = |f: fn(&BodyRow) -> usize| bodies.iter().map(|b| f(b)).max().unwrap_or(0);
    let mut materials: BTreeMap<u8, usize> = BTreeMap::new();
    let mut radii: BTreeMap<u32, usize> = BTreeMap::new();
    let mut welding_types: BTreeMap<u8, usize> = BTreeMap::new();
    for b in &bodies {
        for (m, n) in &b.materials {
            *materials.entry(*m).or_default() += n;
        }
        *radii.entry(b.radius.to_bits()).or_default() += 1;
        *welding_types.entry(b.welding_type).or_default() += 1;
    }

    println!("archive\tworld\tvertices\ttriangles\tmopp bytes\ttranslation");
    for w in &worlds {
        match &w.body {
            Some(b) => println!(
                "{}\t{}\t{}\t{}\t{}\t{:?}",
                w.archive, w.name, b.vertices, b.triangles, b.mopp_bytes, b.translation
            ),
            None => println!("{}\t{}\tno body", w.archive, w.name),
        }
    }
    let mut by_count: Vec<(u8, usize)> = materials.iter().map(|(m, n)| (*m, *n)).collect();
    by_count.sort_by_key(|(m, n)| (std::cmp::Reverse(*n), *m));
    println!(
        "{} archives, {with_level} with a level.bld; {} worlds, {} bodies",
        paths.len(),
        worlds.len(),
        bodies.len()
    );
    println!(
        "{} vertices, {} triangles, {} MOPP bytes, {} welded, {} degenerate; largest {} triangles, {} vertices",
        sum(|b| b.vertices),
        sum(|b| b.triangles),
        sum(|b| b.mopp_bytes),
        sum(|b| b.welded),
        sum(|b| b.degenerate),
        max(|b| b.triangles),
        max(|b| b.vertices),
    );
    println!(
        "radius {:?}, welding type {welding_types:?}",
        radii
            .iter()
            .map(|(r, n)| (f32::from_bits(*r), *n))
            .collect::<Vec<_>>()
    );
    println!("materials ({}): {by_count:?}", materials.len());
    println!("failures: {}", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    println!("wall {:.1} s", start.elapsed().as_secs_f64());

    assert!(failures.is_empty());
    assert_eq!((paths.len(), with_level), (143, 143));
    assert_eq!((worlds.len(), bodies.len()), (75, 68));
    assert_eq!(sum(|b| b.triangles), 140_603);
    assert_eq!(sum(|b| b.vertices), 94_181);
    assert_eq!(sum(|b| b.mopp_bytes), 1_585_593);
    assert_eq!(sum(|b| b.welded), 140_594);
    assert_eq!(sum(|b| b.degenerate), 0);
    assert_eq!((max(|b| b.triangles), max(|b| b.vertices)), (11_488, 7_244));
    assert_eq!(radii, [(0.05f32.to_bits(), 68)].into());
    assert_eq!(welding_types, [(4, 68)].into());
    // ag-levels' table, most used first.
    assert_eq!(
        by_count,
        [
            (0, 87_718),
            (1, 21_237),
            (4, 9_551),
            (252, 3_241),
            (81, 2_506),
            (168, 1_607),
            (21, 1_542),
            (20, 1_440),
            (40, 1_413),
            (5, 1_411),
            (180, 1_210),
            (184, 1_136),
            (172, 957),
            (176, 811),
            (164, 705),
            (188, 690),
            (160, 564),
            (80, 546),
            (120, 540),
            (165, 401),
            (181, 361),
            (253, 310),
            (16, 189),
            (121, 176),
            (41, 173),
            (12, 111),
            (161, 55),
            (240, 2),
        ]
    );
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn level_213s_body_is_ag_levels() {
    let usrdir = usrdir();
    let path = usrdir.join(LEVEL_213);
    let run = read_level(&usrdir, &path, &std::fs::read(&path).unwrap());
    assert!(run.failures.is_empty(), "{:?}", run.failures);
    let names: Vec<&str> = run.worlds.iter().map(|w| w.name.as_str()).collect();
    println!("worlds: {names:?}");
    let [art, grass] = &run.worlds[..] else {
        panic!("{} worlds", run.worlds.len());
    };
    assert!(
        art.name.ends_with("Level_213_ART_MASTER.ma"),
        "{}",
        art.name
    );
    assert!(grass.name.ends_with("Level_213_GRASS.ma"), "{}", grass.name);
    assert!(grass.body.is_none());
    let b = art.body.as_ref().unwrap();
    println!(
        "{} vertices, {} triangles, {} MOPP bytes, translation {:?}",
        b.vertices, b.triangles, b.mopp_bytes, b.translation
    );
    println!("local bounds {:?}, world bounds {:?}", b.local, b.world);
    println!("materials {:?}", b.materials);
    assert_eq!(
        (b.vertices, b.triangles, b.mopp_bytes),
        (2_168, 3_080, 35_253)
    );
    // Unrotated (the reader refuses anything else), translated:
    assert!(close(b.translation, [-425.419, -37.632, -15.966], 5e-4));
    assert!(close(b.local.0, [-511.06, -115.93, -354.64], 0.01));
    assert!(close(b.local.1, [511.06, 115.93, 354.65], 0.01));
    assert!(close(b.world.0, [-936.48, -153.56, -370.61], 0.01));
    assert!(close(b.world.1, [85.64, 78.29, 338.68], 0.01));
    assert_eq!(
        b.materials,
        [
            (0, 1_933),
            (1, 784),
            (21, 8),
            (168, 112),
            (172, 25),
            (188, 134),
            (252, 52),
            (253, 32)
        ]
        .into()
    );
    assert_eq!(b.degenerate, 0);
}
