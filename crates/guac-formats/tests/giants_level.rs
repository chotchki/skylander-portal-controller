//! AG.3's scene, placement and light readers off the Giants dump: every
//! scene of every `level/*.bld` walked and its segments decoded, ag-levels'
//! scene census reproduced (75 scenes, 16,912 segments, 11,385,121
//! triangles, 0 errors) and every placement read; then chapter 1 (level
//! 213) pinned: 997 actors, 188 emitters, 1 light, 743 waypoints in 153
//! lists, `213_Init_Location`'s first waypoint, and its collision (AG.2's
//! reader) checked against its render triangles: 823 of its 2,168 vertices
//! (ag-levels' 38.0 %) within 0.01 with the body's translation, none
//! without.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_level -- --ignored --nocapture --test-threads 1`

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use common::usrdir;
use guac_formats::collision;
use guac_formats::edge::Segment;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    ActorParameters, CollisionInfo, Layout, ObjectRef, PhysicsModel, PhysicsWorld, Shape, read_all,
};
use guac_formats::level::{
    GroupKind, IDENTITY, Placements, Scene, directional_lights, scenes, transform_point,
};

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

/// A level archive's `level.bld`, unpacked, if it has one.
fn level_bld(bytes: &[u8]) -> Option<Vec<u8>> {
    let arc = Archive::parse(bytes).unwrap();
    let entry = arc.find("level.bld")?;
    Some(arc.read(entry).unwrap().into_owned())
}

/// One scene as the census counts it.
#[derive(Debug, Default, Clone)]
struct SceneRow {
    groups: BTreeMap<&'static str, usize>,
    draws: usize,
    segments: usize,
    vertices: usize,
    triangles: usize,
    /// Segments that didn't decode.
    errors: usize,
    non_identity: usize,
    animated: usize,
    up: [f32; 3],
}

fn kind_name(kind: &GroupKind<'_>) -> &'static str {
    match kind {
        GroupKind::Group(_) => "igGroup",
        GroupKind::Spatial(_) => "igSpatialNode",
        GroupKind::Transform(_) => "igTransform",
        GroupKind::UserInfo(_) => "igUserInfo",
    }
}

fn scene_row(igz: &Igz<'_>, scene: &Scene<'_>) -> SceneRow {
    let mut row = SceneRow {
        draws: scene.draws.len(),
        up: scene.info.up_vector,
        ..SceneRow::default()
    };
    for group in scene.groups.values() {
        *row.groups.entry(kind_name(&group.kind)).or_default() += 1;
    }
    for (_, t) in scene.transforms() {
        row.non_identity += usize::from(t.m != IDENTITY);
        row.animated += usize::from(t.is_animated());
    }
    for draw in &scene.draws {
        for (i, layout) in scene.draw_segments(draw) {
            row.segments += 1;
            match Segment::decode(igz, i, layout) {
                Ok(s) => {
                    row.vertices += s.vertex_count();
                    row.triangles += s.triangles().count();
                }
                Err(_) => row.errors += 1,
            }
        }
    }
    row
}

/// A level's placements as the census counts them.
#[derive(Debug, Default, Clone)]
struct PlacementRow {
    actors: usize,
    emitters: usize,
    lights: usize,
    waypoints: usize,
    waypoint_lists: usize,
    collision_infos: usize,
    actor_parameters: usize,
    rig_lights: usize,
    shapes: BTreeMap<u32, usize>,
}

#[derive(Debug, Default)]
struct LevelRun {
    archive: String,
    has_level: bool,
    scenes: Vec<SceneRow>,
    placements: PlacementRow,
    /// Worlds whose `_sceneInfo` names a scene, and the scenes they name.
    worlds: usize,
    world_scenes: BTreeSet<usize>,
    scene_indexes: BTreeSet<usize>,
    failures: Vec<String>,
}

fn read_level(usrdir: &Path, path: &Path, bytes: &[u8]) -> LevelRun {
    let mut run = LevelRun {
        archive: path.strip_prefix(usrdir).unwrap().display().to_string(),
        ..LevelRun::default()
    };
    let Some(data) = level_bld(bytes) else {
        return run;
    };
    run.has_level = true;
    let igz = Igz::parse(&data).unwrap();
    let mut fail = |what: String| run.failures.push(format!("{}: {what}", run.archive));
    for (i, scene) in scenes(&igz) {
        match scene {
            Ok(s) => {
                run.scene_indexes.insert(i);
                run.scenes.push(scene_row(&igz, &s));
            }
            Err(e) => fail(format!("scene {i}: {e}")),
        }
    }
    for (i, w) in read_all::<PhysicsWorld>(&igz) {
        match w {
            Ok(w) => {
                run.worlds += 1;
                if let ObjectRef::Object(s) = w.world.scene_info {
                    run.world_scenes.insert(s);
                }
            }
            Err(e) => fail(format!("world {i}: {e}")),
        }
    }
    match Placements::read(&igz) {
        Ok(p) => {
            let row = &mut run.placements;
            row.actors = p.actors.len();
            row.emitters = p.emitters.len();
            row.lights = p.lights.len();
            row.waypoint_lists = p.waypoint_lists.len();
            row.waypoints = p.waypoint_lists.values().map(Vec::len).sum();
            for a in &p.actors {
                if let Some(c) = a.collision {
                    *row.shapes.entry(c.properties & 7).or_default() += 1;
                }
            }
        }
        Err(e) => fail(format!("placements: {e}")),
    }
    // Every solid and parameter block in the file, models' defaults too.
    for (i, c) in read_all::<CollisionInfo>(&igz) {
        match c {
            Ok(_) => run.placements.collision_infos += 1,
            Err(e) => fail(format!("CollisionInfo {i}: {e}")),
        }
    }
    for (i, a) in read_all::<ActorParameters>(&igz) {
        match a {
            Ok(_) => run.placements.actor_parameters += 1,
            Err(e) => fail(format!("ActorParameters {i}: {e}")),
        }
    }
    match directional_lights(&igz) {
        Ok(l) => run.placements.rig_lights = l.len(),
        Err(e) => fail(format!("rig: {e}")),
    }
    run
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_scene_census_reproduces_and_every_placement_reads() {
    let start = Instant::now();
    let usrdir = usrdir();
    let paths = levels(&usrdir);
    let runs = each_archive(&paths, |path, bytes| read_level(&usrdir, path, bytes));

    let with_level = runs.iter().filter(|r| r.has_level).count();
    let failures: Vec<&String> = runs.iter().flat_map(|r| &r.failures).collect();
    let rows: Vec<&SceneRow> = runs.iter().flat_map(|r| &r.scenes).collect();
    let sum = |f: fn(&SceneRow) -> usize| rows.iter().map(|r| f(r)).sum::<usize>();
    let mut groups: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        for (k, n) in &r.groups {
            *groups.entry(k).or_default() += n;
        }
    }
    let placed =
        |f: fn(&PlacementRow) -> usize| runs.iter().map(|r| f(&r.placements)).sum::<usize>();
    let mut shapes: BTreeMap<u32, usize> = BTreeMap::new();
    for r in &runs {
        for (s, n) in &r.placements.shapes {
            *shapes.entry(*s).or_default() += n;
        }
    }
    let worlds: usize = runs.iter().map(|r| r.worlds).sum();
    // Each world names one scene of its own file, and every scene is one's.
    let paired = runs
        .iter()
        .filter(|r| r.world_scenes == r.scene_indexes && r.worlds == r.scene_indexes.len())
        .count();
    let ups: BTreeSet<[u32; 3]> = rows.iter().map(|r| r.up.map(f32::to_bits)).collect();

    println!("archive\tscenes\tsegments\ttriangles\tactors\temitters\tlights\twaypoints");
    for r in runs.iter().filter(|r| r.has_level) {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.archive,
            r.scenes.len(),
            r.scenes.iter().map(|s| s.segments).sum::<usize>(),
            r.scenes.iter().map(|s| s.triangles).sum::<usize>(),
            r.placements.actors,
            r.placements.emitters,
            r.placements.lights,
            r.placements.waypoints,
        );
    }
    println!(
        "{} archives, {with_level} with a level.bld; {} scenes, {worlds} worlds, {paired} files pairing them",
        paths.len(),
        rows.len(),
    );
    println!(
        "{} draws, {} segments, {} vertices, {} triangles, {} segment errors; groups {groups:?}; {} transforms not identity, {} animated; up vectors {:?}",
        sum(|r| r.draws),
        sum(|r| r.segments),
        sum(|r| r.vertices),
        sum(|r| r.triangles),
        sum(|r| r.errors),
        sum(|r| r.non_identity),
        sum(|r| r.animated),
        ups.iter()
            .map(|u| u.map(f32::from_bits))
            .collect::<Vec<_>>(),
    );
    println!(
        "{} actors, {} emitters, {} placed lights, {} waypoints in {} lists, {} CollisionInfo, {} ActorParameters, {} rig lights; actor shapes {shapes:?}",
        placed(|p| p.actors),
        placed(|p| p.emitters),
        placed(|p| p.lights),
        placed(|p| p.waypoints),
        placed(|p| p.waypoint_lists),
        placed(|p| p.collision_infos),
        placed(|p| p.actor_parameters),
        placed(|p| p.rig_lights),
    );
    println!("failures: {}", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    println!("wall {:.1} s", start.elapsed().as_secs_f64());

    assert!(failures.is_empty());
    assert_eq!((paths.len(), with_level), (143, 143));
    // ag-levels' scene census.
    assert_eq!(rows.len(), 75);
    assert_eq!(sum(|r| r.segments), 16_912);
    assert_eq!(sum(|r| r.triangles), 11_385_121);
    assert_eq!(sum(|r| r.errors), 0);
    assert_eq!(sum(|r| r.vertices), 11_446_230);
    assert_eq!(sum(|r| r.draws), 15_898);
    assert_eq!(
        groups,
        [
            ("igGroup", 75),
            ("igSpatialNode", 112),
            ("igTransform", 54),
            ("igUserInfo", 133),
        ]
        .into()
    );
    assert_eq!((sum(|r| r.non_identity), sum(|r| r.animated)), (28, 40));
    assert_eq!(ups, [[0.0f32, 1.0, 0.0].map(f32::to_bits)].into());
    assert_eq!((worlds, paired), (75, 143));
    // And the placements' disc totals (ag-levels' census).
    assert_eq!(placed(|p| p.actors), 51_591);
    assert_eq!(placed(|p| p.emitters), 15_383);
    assert_eq!(placed(|p| p.lights), 80);
    assert_eq!(placed(|p| p.waypoints), 18_322);
    assert_eq!(placed(|p| p.waypoint_lists), 5_996);
    assert_eq!(placed(|p| p.collision_infos), 65_037);
    assert_eq!(placed(|p| p.actor_parameters), 43_889);
    assert_eq!(placed(|p| p.rig_lights), 572);
    // Every actor has a solid; five are meshes (ag-levels).
    assert_eq!(
        shapes,
        [
            (0, 27_159),
            (1, 8_517),
            (2, 6_700),
            (3, 3_639),
            (4, 5_571),
            (5, 5)
        ]
        .into()
    );
}

fn close(a: [f32; 3], b: [f64; 3], eps: f64) -> bool {
    (0..3).all(|i| (f64::from(a[i]) - b[i]).abs() <= eps)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn level_213s_scenes_and_placements_are_ag_levels() {
    let path = usrdir().join(LEVEL_213);
    let data = level_bld(&std::fs::read(&path).unwrap()).unwrap();
    let igz = Igz::parse(&data).unwrap();

    // Its two scenes: the level and its grass.
    let walked: Vec<Scene<'_>> = scenes(&igz).map(|(_, s)| s.unwrap()).collect();
    let rows: Vec<SceneRow> = walked.iter().map(|s| scene_row(&igz, s)).collect();
    for r in &rows {
        println!(
            "scene: {} draws, {} segments, {} triangles, groups {:?}",
            r.draws, r.segments, r.triangles, r.groups
        );
    }
    let [art, grass] = &rows[..] else {
        panic!("{} scenes", rows.len());
    };
    assert_eq!((art.segments, art.triangles, art.errors), (524, 395_214, 0));
    assert_eq!(
        (grass.segments, grass.triangles, grass.errors),
        (45, 37_740, 0)
    );
    assert_eq!(
        art.groups,
        [("igGroup", 1), ("igSpatialNode", 2), ("igUserInfo", 7)].into()
    );
    assert_eq!(art.draws, 478);
    // The sky: what hangs under the seven user-info groups.
    let sky: usize = walked[0]
        .draws
        .iter()
        .filter(|d| d.user_info.is_some())
        .map(|d| walked[0].draw_segments(d).count())
        .sum();
    println!("sky segments: {sky}");

    let p = Placements::read(&igz).unwrap();
    let waypoints: usize = p.waypoint_lists.values().map(Vec::len).sum();
    let with_model = p.actors.iter().filter(|a| a.info.model.is_some()).count();
    let with_script = p.actors.iter().filter(|a| a.script.is_some()).count();
    let mut states: BTreeMap<u32, usize> = BTreeMap::new();
    for a in &p.actors {
        *states
            .entry(a.placement().placement.activity_state)
            .or_default() += 1;
    }
    println!(
        "{} actors ({with_model} with a model, {} models, {with_script} with a script, states {states:?}), {} emitters, {} lights, {waypoints} waypoints in {} lists",
        p.actors.len(),
        p.models.len(),
        p.emitters.len(),
        p.lights.len(),
        p.waypoint_lists.len(),
    );
    assert_eq!(
        (p.actors.len(), p.emitters.len(), p.lights.len()),
        (997, 188, 1)
    );
    assert_eq!((waypoints, p.waypoint_lists.len()), (743, 153));
    // ag-levels' "222 distinct" counts the file's `tfbPhysicsModel`s; the
    // actors name 219 of them.
    let file_models: Vec<usize> = read_all::<PhysicsModel>(&igz).map(|(i, _)| i).collect();
    let unnamed: Vec<&str> = file_models
        .iter()
        .filter(|i| !p.models.contains_key(i))
        .map(|&i| {
            PhysicsModel::read(&igz, i)
                .unwrap()
                .model
                .info
                .name
                .unwrap_or("")
        })
        .collect();
    println!("models no actor names: {unnamed:?}");
    assert_eq!((with_model, p.models.len(), with_script), (613, 219, 584));
    assert_eq!(file_models.len(), 222);
    assert_eq!(states, [(4, 584), (5, 413)].into());

    // Where the first player starts (ag-motion: AF.9's idle settles there).
    let init = p.actor("213_Init_Location").unwrap();
    let first = p.waypoints(init)[0].init_matrix;
    println!("213_Init_Location's first waypoint: {first:?}");
    assert!(close(first.position, [10.191, 43.379, -93.394], 5e-4));
    assert_eq!(first.rotation, [0.0; 3]);
    assert!(init.script.unwrap().ends_with("InitializeLevel.ai"));
    let start = p.actor("_Start_level").unwrap();
    assert!(close(
        start.placement().placement.init_matrix.position,
        [16.271, 43.363, -90.033],
        5e-4
    ));
    assert!(start.script.unwrap().ends_with("Start_Level.ai"));

    // The gate's door (design decision 10): a solid box at heading 180.
    let door = p.actor("Breakable_Stone_Wall_Castle_206(1)").unwrap();
    let c = door.collision.unwrap();
    println!(
        "door: {:?} {}x{}x{} solid {} at {:?}, model {:?}",
        c.shape(),
        c.width,
        c.height,
        c.length,
        c.is_solid(),
        door.placement().placement.init_matrix,
        p.model_path(door)
    );
    assert_eq!(c.shape(), Shape::Box);
    assert!(c.is_solid());
    assert_eq!((c.width, c.height, c.length), (4.0, 21.0, 34.0));
    assert!((door.placement().placement.init_matrix.rotation[1] - 180.0).abs() < 1e-3);
    assert!(
        p.model_path(door)
            .unwrap()
            .ends_with("Door_Breakable_Quarry.mdl")
    );

    // The light and the rig.
    let (_, light) = p.lights[0];
    println!("light: {light:?}");
    assert_eq!(
        light.placement.placement.name,
        Some("213_EvilRobot_Beam_light")
    );
    assert_eq!(light.light_type, 1);
    let rig = directional_lights(&igz).unwrap();
    let names: Vec<_> = rig.iter().map(|(_, l)| l.name.unwrap_or("")).collect();
    println!("rig: {names:?}");
    assert_eq!(rig.len(), 4);
    for n in [
        "_worldLight1",
        "_worldLight2",
        "_worldLight3",
        "_causticLight",
    ] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
}

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn lerp(a: V, d: V, t: f32) -> V {
    [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]]
}

/// The distance from `p` to triangle abc: the closest point by Voronoi
/// region (Ericson, Real-Time Collision Detection, 5.1.5), as ag-levels'
/// `colcheck` computes it.
fn distance_to_triangle(p: V, a: V, b: V, c: V) -> f32 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        let cp = sub(p, c);
        let (d5, d6) = (dot(ab, cp), dot(ac, cp));
        let vc = d1 * d4 - d3 * d2;
        let vb = d5 * d2 - d1 * d6;
        let va = d3 * d6 - d5 * d4;
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            lerp(a, ab, d1 / (d1 - d3))
        } else if d6 >= 0.0 && d5 <= d6 {
            c
        } else if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            lerp(a, ac, d2 / (d2 - d6))
        } else if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            lerp(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))
        } else {
            let den = 1.0 / (va + vb + vc);
            let (v, w) = (vb * den, vc * den);
            let q = lerp(a, ab, v);
            lerp(q, ac, w)
        }
    };
    let d = sub(p, q);
    dot(d, d).sqrt()
}

/// Render triangles bucketed into `cell`-sized cubes, as `colcheck` does:
/// one spanning more than 4,096 cells (the sky's) is left out, and a
/// query looks in the 27 cells around its point, so it's exact only for
/// distances under a cell.
struct Grid {
    cell: f32,
    cells: HashMap<(i32, i32, i32), Vec<u32>>,
    triangles: Vec<[V; 3]>,
}

impl Grid {
    fn new(triangles: Vec<[V; 3]>, cell: f32) -> Self {
        let mut cells: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
        let at = |x: f32| (x / cell).floor() as i32;
        for (i, t) in triangles.iter().enumerate() {
            let lo: [i32; 3] =
                std::array::from_fn(|k| at(t.iter().map(|p| p[k]).fold(f32::MAX, f32::min)));
            let hi: [i32; 3] =
                std::array::from_fn(|k| at(t.iter().map(|p| p[k]).fold(f32::MIN, f32::max)));
            let span: i64 = (0..3).map(|k| i64::from(hi[k] - lo[k])).product();
            if span > 4096 {
                continue;
            }
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self {
            cell,
            cells,
            triangles,
        }
    }

    fn nearest(&self, p: V) -> f32 {
        let at = |x: f32| (x / self.cell).floor() as i32;
        let c = p.map(at);
        let mut best = f32::MAX;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(ts) = self.cells.get(&(c[0] + dx, c[1] + dy, c[2] + dz)) else {
                        continue;
                    };
                    for &i in ts {
                        let [a, b, c] = self.triangles[i as usize];
                        best = best.min(distance_to_triangle(p, a, b, c));
                    }
                }
            }
        }
        best
    }
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn level_213s_collision_lies_on_its_render_mesh_once_translated() {
    let start = Instant::now();
    let path = usrdir().join(LEVEL_213);
    let data = level_bld(&std::fs::read(&path).unwrap()).unwrap();
    let igz = Igz::parse(&data).unwrap();

    // Every scene's triangles in the level's frame.
    let mut triangles: Vec<[V; 3]> = Vec::new();
    for (_, scene) in scenes(&igz) {
        let scene = scene.unwrap();
        for draw in &scene.draws {
            for (i, layout) in scene.draw_segments(draw) {
                let s = Segment::decode(&igz, i, layout).unwrap();
                let p: Vec<V> = s
                    .positions()
                    .map(|v| transform_point(v, &draw.world))
                    .collect();
                triangles.extend(s.triangles().map(|t| t.map(|k| p[usize::from(k)])));
            }
        }
    }
    let worlds: Vec<_> = collision::worlds(&igz).map(|(_, w)| w.unwrap()).collect();
    let body = worlds
        .iter()
        .find_map(|w| w.collision.body.as_ref())
        .unwrap();
    println!(
        "{} render triangles, {} collision vertices, translation {:?}",
        triangles.len(),
        body.mesh.vertices.len(),
        body.translation
    );
    assert_eq!(triangles.len(), 432_954);
    let grid = Grid::new(triangles, 4.0);
    let within = |d: &[f32], limit: f32| d.iter().filter(|&&x| x <= limit).count();
    let translated: Vec<f32> = body.world_vertices().map(|v| grid.nearest(v)).collect();
    let local: Vec<f32> = body
        .mesh
        .vertices
        .iter()
        .map(|&v| grid.nearest(v))
        .collect();
    let n = translated.len();
    for (label, d) in [("translated", &translated), ("local", &local)] {
        println!(
            "{label}: within 0.01 {} ({:.2} %), 0.1 {} ({:.2} %), 2 {} ({:.2} %)",
            within(d, 0.01),
            100.0 * within(d, 0.01) as f64 / n as f64,
            within(d, 0.1),
            100.0 * within(d, 0.1) as f64 / n as f64,
            within(d, 2.0),
            100.0 * within(d, 2.0) as f64 / n as f64,
        );
    }
    println!("wall {:.1} s", start.elapsed().as_secs_f64());
    // ag-levels' 38.0 %, 57.5 % and 91.6 %, to the vertex.
    assert_eq!(n, 2_168);
    assert_eq!(within(&translated, 0.01), 823);
    assert_eq!(within(&translated, 0.1), 1_247);
    assert_eq!(within(&translated, 2.0), 1_986);
    assert_eq!(within(&local, 0.01), 0);
}
