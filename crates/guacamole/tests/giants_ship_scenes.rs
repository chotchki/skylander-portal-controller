//! The ship drawn whole through its scenes (openspec AS.443), headless over
//! `GUAC_GIANTS_DUMP` on the TV's listing with the camera rigs posed: each
//! part the scripts hang on the hull draws where its hang puts it, and the
//! people on board stand on its deck, at steps across each scene:
//!
//! - level 214's intro, the ship flying in (its masthead, roof and engine
//!   cloned onto it by `Ships.ai`, the engine's oars and propeller on the
//!   engine's nodes, Flynn and Cali on the deck's nodes) and docked;
//! - level 214's sail from the first island (`214 recover 1st part IGC`),
//!   to its end on the second island's dock;
//! - level 201's intro, the landing pod (its base, plank, propeller and
//!   wheel cloned onto its nodes by `LandingPod.ai`).
//!
//! What a part's place is measured by: every vertex of its model as it's
//! drawn ([`guacamole::props::actor_transform`], what the TV draws a prop and
//! a clone by) against the same vertex drawn in the frame it hangs in, the
//! hull's for a part hung on the hull itself. A person's feet against the
//! deck: the highest surface of the ship's drawn models (hull and parts)
//! straight under his pivot, within reach of it.
//!
//! `cargo test -p guacamole --profile disc --test giants_ship_scenes -- --ignored --nocapture --test-threads=1`

use std::collections::HashMap;

use guac_motion::seam::Vec3;
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_214: &str = "level_214_tutotial_2";
const LEVEL_201: &str = "level_201";

/// How far a part hung on the hull itself may draw from where the hull's
/// frame puts it, metres, at any vertex.
const PART_SLACK: f32 = 0.05;
/// How far a part hung on a node of the ship may draw from the ship's other
/// drawn parts, metres, from its nearest vertex to theirs: it touches them
/// (the engine's oars row out to 2.6 m of the hull's nearest vertex, whose
/// panels are a few metres across).
const TOUCH: f32 = 4.0;
/// A person's feet against the deck under him, metres: above it by at most
/// `FEET_ABOVE`, below it (sunk through it) by at most `FEET_BELOW`.
const FEET_ABOVE: f32 = 0.35;
const FEET_BELOW: f32 = 0.15;
/// How high over his pivot the deck under a person is looked for, metres:
/// a floor up to his waist is one he's sunk through.
const REACH: f32 = 1.2;

fn setup() -> Setup {
    Setup {
        props: true,
        interface: true,
        sounds: true,
        posed_rigs: true,
        ..Setup::default()
    }
}

/// Each static model of the level's props part as triangles in its own
/// frame, by path.
fn models(level: &str) -> HashMap<String, Vec<[Vec3; 3]>> {
    let usr = dump().join("PS3_GAME/USRDIR");
    let bytes = guac_extract::scripts::entry(&usr, &format!("level/{level}.bld")).unwrap();
    let igz = guac_formats::igz::Igz::parse(&bytes).unwrap();
    let props = guac_extract::levels::props(&igz, level, identity())
        .expect("the props")
        .ir;
    props
        .props
        .iter()
        .filter(|p| p.skeleton.is_none() && !guacamole::props::see_through(p))
        .map(|p| {
            let tris = p
                .segments
                .iter()
                .flat_map(|s| {
                    s.indices
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|t| t.map(|i| Vec3::from_array(s.positions[i as usize])))
                        .collect::<Vec<_>>()
                })
                .collect();
            (p.path.clone(), tris)
        })
        .collect()
}

/// The level's skinned models' paths.
fn skinned_paths(level: &str) -> Vec<String> {
    skinned_models_of(level)
        .into_iter()
        .map(|(p, _)| p)
        .collect()
}

/// The model an actor draws, by its template's (or its own) placement.
fn model_of(run: &Run, h: Handle) -> Option<String> {
    let a = run.world.actor(h)?;
    let o = a.template.or(a.obj)?;
    run.files
        .placed
        .iter()
        .find(|p| p.obj == o)
        .and_then(|p| p.model.clone())
}

/// What `h` hangs on: the actor itself, or the actor whose node.
fn hung_on(run: &Run, h: Handle) -> Option<Handle> {
    let a = run.world.actor(h)?;
    if a.attach == Value::Null {
        return None;
    }
    run.world.node_owner(a.attach).or(match a.attach {
        Value::Entity(o) if run.world.actor(o).is_some() => Some(o),
        _ => None,
    })
}

/// Whether `h` hangs on `hull` itself (not on a node of it).
fn on_itself(run: &Run, h: Handle, hull: Handle) -> bool {
    run.world
        .actor(h)
        .is_some_and(|a| a.attach == Value::Entity(hull))
}

/// Whether `h` hangs on `hull`, a node of it, or what hangs so.
fn rides(run: &Run, h: Handle, hull: Handle) -> bool {
    let mut at = h;
    for _ in 0..6 {
        match hung_on(run, at) {
            Some(o) if o == hull => return true,
            Some(o) if o != at => at = o,
            _ => return false,
        }
    }
    false
}

/// Every live clone named `name`, newest last.
fn clones_named(run: &Run, name: &str) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|&h| run.world.name(h) == name)
        .filter(|&h| run.world.actor(h).is_some_and(|a| !a.removed))
        .collect()
}

/// Every vertex of an actor's model placed as the TV draws it, metres in the
/// level.
fn drawn_verts(run: &Run, models: &HashMap<String, Vec<[Vec3; 3]>>, h: Handle) -> Vec<Vec3> {
    let (Some(a), Some(m)) = (run.world.actor(h), model_of(run, h)) else {
        return Vec::new();
    };
    let Some(tris) = models.get(&m) else {
        return Vec::new();
    };
    let t = guacamole::props::actor_transform(a);
    tris.iter()
        .flat_map(|tri| tri.iter().map(|v| t.transform_point(*v)))
        .collect()
}

/// The ship as the step leaves it: what rides the hull, drawn, and what
/// hangs on it itself; each part's displacement from the hull's frame; each
/// node part's nearest vertex to the rest of the ship.
#[derive(Debug, Default)]
struct Sample {
    step: u32,
    scene: Option<(usize, usize)>,
    /// Each part hung on the hull itself: its name and its largest vertex
    /// displacement from the hull's frame, metres.
    on_hull: Vec<(String, f32)>,
    /// Each static part hung on a node of the ship: its name and its
    /// nearest vertex to the rest of the ship, metres.
    on_nodes: Vec<(String, f32)>,
    /// Each person on board: his name and his feet over the deck, metres
    /// (negative: sunk through it), or none with no deck under him.
    feet: Vec<(String, Option<f32>)>,
}

fn sample(
    run: &Run,
    models: &HashMap<String, Vec<[Vec3; 3]>>,
    skinned: &[String],
    hull: Handle,
    people: &[&str],
    riding: bool,
) -> Sample {
    let mut s = Sample {
        step: run.steps,
        scene: scene_of(run),
        ..Sample::default()
    };
    let parts: Vec<Handle> = run
        .world
        .clones()
        .into_iter()
        .filter(|&h| h != hull && run.world.actor(h).is_some_and(|a| !a.removed))
        .filter(|&h| !run.world.hangs_on_nothing(h) && rides(run, h, hull))
        .filter(|&h| {
            model_of(run, h).is_some_and(|m| models.contains_key(&m) || skinned.contains(&m))
        })
        .collect();
    let hull_t = guacamole::props::actor_transform(run.world.actor(hull).unwrap());
    let mut ship: Vec<(Handle, Vec<Vec3>)> = vec![(hull, drawn_verts(run, models, hull))];
    for &p in &parts {
        ship.push((p, drawn_verts(run, models, p)));
    }
    for &p in &parts {
        let name = run.world.name(p);
        if on_itself(run, p, hull) {
            let a = run.world.actor(p).unwrap();
            let t = guacamole::props::actor_transform(a);
            let m = model_of(run, p).unwrap();
            // A skinned part (the engine) by the points 5 m out along its
            // axes, a static one by its model's vertices.
            let axes: Vec<Vec3> = [Vec3::X, Vec3::Y, Vec3::Z]
                .into_iter()
                .flat_map(|a| [a * 5.0, a * -5.0])
                .collect();
            let points: Vec<Vec3> = match models.get(&m) {
                Some(tris) => tris.iter().flat_map(|tri| tri.iter().copied()).collect(),
                None => axes,
            };
            let worst = points
                .iter()
                .map(|v| t.transform_point(*v).distance(hull_t.transform_point(*v)))
                .fold(0.0f32, f32::max);
            s.on_hull.push((name, worst));
        } else if models.contains_key(&model_of(run, p).unwrap()) {
            let mine = &ship.iter().find(|(h, _)| *h == p).unwrap().1;
            let rest: Vec<Vec3> = ship
                .iter()
                .filter(|(h, _)| *h != p)
                .flat_map(|(_, v)| v.iter().copied().step_by(2))
                .collect();
            let near = mine
                .iter()
                .map(|v| {
                    rest.iter()
                        .map(|r| r.distance_squared(*v))
                        .fold(f32::MAX, f32::min)
                })
                .fold(f32::MAX, f32::min)
                .sqrt();
            s.on_nodes.push((name, near));
        }
    }
    // The deck: the ship's drawn triangles, the hull's and its parts'.
    let mut tris: Vec<[Vec3; 3]> = Vec::new();
    for (h, _) in &ship {
        let (Some(a), Some(tri)) = (
            run.world.actor(*h),
            model_of(run, *h).and_then(|m| models.get(&m)),
        ) else {
            continue;
        };
        let t = guacamole::props::actor_transform(a);
        tris.extend(tri.iter().map(|tri| tri.map(|v| t.transform_point(v))));
    }
    for who in people {
        for h in clones_named(run, who) {
            if riding && !rides(run, h, hull) {
                continue;
            }
            let feet = run.world.actor(h).unwrap().position;
            s.feet.push((
                (*who).to_owned(),
                deck_under(&tris, feet).map(|d| feet.y - d),
            ));
        }
    }
    s
}

/// The highest surface of `tris` straight under `feet`, within [`REACH`]
/// over it.
fn deck_under(tris: &[[Vec3; 3]], feet: Vec3) -> Option<f32> {
    let (x, z) = (feet.x, feet.z);
    let mut best: Option<f32> = None;
    for [a, b, c] in tris {
        let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
        if d.abs() < 1e-9 {
            continue;
        }
        let l1 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / d;
        let l2 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / d;
        let l3 = 1.0 - l1 - l2;
        if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
            continue;
        }
        let y = l1 * a.y + l2 * b.y + l3 * c.y;
        if y <= feet.y + REACH && best.is_none_or(|b| y > b) {
            best = Some(y);
        }
    }
    best
}

fn still(run: &mut Run, stops: &mut Vec<String>, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick: guac_motion::stick::Stick::new(0.0, 0.0),
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

/// Each sample's worst part on the hull, worst node part, and each person's
/// feet, printed.
fn print(samples: &[Sample]) {
    for s in samples {
        let hull = s.on_hull.iter().map(|p| p.1).fold(0.0f32, f32::max);
        let nodes = s.on_nodes.iter().map(|p| p.1).fold(0.0f32, f32::max);
        println!(
            "step {} scene {:?}: on the hull {} worst {hull:.3} m, on nodes {} worst {nodes:.3} m, feet {:?}",
            s.step,
            s.scene,
            s.on_hull.len(),
            s.on_nodes.len(),
            s.feet
        );
    }
}

/// Every sample's parts and people within their bounds.
fn check(samples: &[Sample], what: &str) {
    assert!(samples.len() >= 3, "{what}: three steps at least");
    for s in samples {
        for (name, d) in &s.on_hull {
            assert!(
                *d <= PART_SLACK,
                "{what}, step {}: {name} {d:.3} m off the hull",
                s.step
            );
        }
        for (name, d) in &s.on_nodes {
            assert!(
                *d <= TOUCH,
                "{what}, step {}: {name} {d:.3} m from the ship",
                s.step
            );
        }
        for (name, gap) in &s.feet {
            let gap =
                gap.unwrap_or_else(|| panic!("{what}, step {}: no deck under {name}", s.step));
            assert!(
                (-FEET_BELOW..=FEET_ABOVE).contains(&gap),
                "{what}, step {}: {name}'s feet {gap:.3} m over the deck",
                s.step
            );
        }
    }
}

/// Level 214's intro played through (comments 1 and 2 of AS.443): the ship
/// flown in with its masthead, roof and engine on it, its oars and
/// propeller on the engine, Flynn and Cali on its deck; then docked, to
/// the intro's end, the docked ship's masthead on it and Flynn set down on
/// its deck.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_intro_flies_and_docks_the_ship_whole() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let models = models(LEVEL_214);
    let skinned = skinned_paths(LEVEL_214);
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let intro = run.placement("214_Intro_IGC");
    let (mut flight, mut docked) = (Vec::new(), Vec::new());
    let mut stops: Vec<String> = stops.iter().map(ToString::to_string).collect();
    while !run.vm.removed(intro) {
        assert!(run.steps < 4000, "the intro over");
        still(&mut run, &mut stops, [false; 4]);
        let ships = clones_named(&run, "214_Intro_Ship");
        let Some(&hull) = ships.last() else { continue };
        let flying = run
            .world
            .actor(hull)
            .is_some_and(|a| a.attach != Value::Null);
        if run.steps % 30 == 0 {
            let s = sample(
                &run,
                &models,
                &skinned,
                hull,
                &["Flynn_NPC", "Cali_NPC"],
                flying,
            );
            if flying {
                flight.push(s);
            } else {
                docked.push(s);
            }
        }
    }
    println!("the flight:");
    print(&flight);
    println!("docked:");
    print(&docked);
    check(&flight, "the flight");
    check(&docked, "docked");
    assert!(
        flight
            .iter()
            .any(|s| s.on_hull.len() >= 3 && s.on_nodes.len() >= 4),
        "the masthead, roof and engine on the hull, the engine's four on its nodes"
    );
    assert!(stops.is_empty(), "{stops:?}");
    println!(
        "{} steps, {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Level 214's sail from the first island (comment 3 of AS.443): the ship
/// carried by the scene's rigs to the second island, its parts on it, the
/// people on its deck; from scene 9 on, its last shot's rig taken off, it
/// stands on the second island's dock where the scene put it, drawn, with
/// Flynn on its deck (on 2.0 it hung on nothing and drew nowhere).
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_sail_carries_the_ship_whole_to_the_second_island() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let models = models(LEVEL_214);
    let skinned = skinned_paths(LEVEL_214);
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops: Vec<String> = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    stops.extend(skip_214s_intro(&mut run).iter().map(ToString::to_string));
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let sail = run.placement("214_Recover_First_Part_IGC");
    // By the first ship part; a press takes it (as `giants_fights` walks it).
    run.put(Vec3::new(29.45, -6.92, 56.37), 0.0);
    let (mut carried, mut docked) = (Vec::new(), Vec::new());
    let mut k = 0u32;
    while !run.vm.removed(sail) {
        assert!(k < 6000, "the sail over");
        let up = cutscene_name(&run).is_some();
        still(
            &mut run,
            &mut stops,
            [!up && k % 20 < 2, false, false, false],
        );
        k += 1;
        let scene = scene_of(&run).map_or(0, |s| s.0);
        // Its last two scenes are short: sampled more often.
        let every = if scene >= 9 { 10 } else { 30 };
        if !up || run.steps % every != 0 {
            continue;
        }
        let Some(&hull) = clones_named(&run, "214_Intro_Ship").last() else {
            continue;
        };
        let on_nothing = run.world.hangs_on_nothing(hull);
        if scene >= 9 {
            assert!(
                !on_nothing,
                "step {}: the ship hangs on nothing, drawn nowhere",
                run.steps
            );
            docked.push(sample(&run, &models, &skinned, hull, &["Flynn_NPC"], false));
        } else if (4..9).contains(&scene) {
            carried.push(sample(
                &run,
                &models,
                &skinned,
                hull,
                &["Flynn_NPC", "Cali_NPC"],
                true,
            ));
        }
    }
    println!("carried:");
    print(&carried);
    println!("on the second island:");
    print(&docked);
    check(&carried, "carried");
    check(&docked, "on the second island");
    assert!(stops.is_empty(), "{stops:?}");
    println!(
        "{} steps, {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Level 201's intro (comment 4 of AS.443): the landing pod flies in with
/// its base, plank, propeller and wheel on its nodes (`LandingPod.ai`'s
/// STARTUP, off the listing on 2.0, so none was made) and Flynn and Cali
/// stand on its base.
#[test]
#[ignore = "reads the Giants dump"]
fn level_201s_intro_flies_the_landing_pod_whole() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let models = models(LEVEL_201);
    let skinned = skinned_paths(LEVEL_201);
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_201, setup());
    let mut stops: Vec<String> = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let mut flight = Vec::new();
    let mut parts = 0;
    for _ in 0..3000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if cutscene_name(&run).as_deref() != Some("Level 201 Intro CS") {
            if !flight.is_empty() {
                break;
            }
            continue;
        }
        if run.steps % 30 != 0 {
            continue;
        }
        let Some(&hull) = clones_named(&run, "Landing_Pod").last() else {
            continue;
        };
        if run
            .world
            .actor(hull)
            .is_some_and(|a| a.attach == Value::Null)
        {
            continue;
        }
        let s = sample(
            &run,
            &models,
            &skinned,
            hull,
            &["Flynn NPC", "Cali NPC"],
            true,
        );
        parts = parts.max(s.on_nodes.len());
        flight.push(s);
    }
    // The intro over, its rig taken off: the pod stays drawn where it
    // landed, its parts on it (the rig rule; on 2.0 it hung on nothing
    // and drew nowhere).
    let pod = *clones_named(&run, "Landing_Pod")
        .last()
        .expect("the pod kept after the intro");
    let rig = hung_on(&run, pod).expect("on its rig's node");
    let mut parked = 0;
    for _ in 0..120u32 {
        still(&mut run, &mut stops, [false; 4]);
        if run.world.actor(rig).is_some_and(|a| a.removed) {
            assert!(
                !run.world.hangs_on_nothing(pod),
                "step {}: the landed pod hangs on nothing, drawn nowhere",
                run.steps
            );
            for part in ["LP_Base", "LP_Plank", "LP_Propeller", "LP_Wheel"] {
                for h in clones_named(&run, part) {
                    assert!(!run.world.hangs_on_nothing(h), "{part}");
                }
            }
            parked += 1;
        }
    }
    println!("parked, its rig taken off: {parked} steps of 120");
    assert!(parked >= 60, "the rig taken off after the intro: {parked}");
    println!("the pod's flight:");
    print(&flight);
    assert_eq!(parts, 4, "its base, plank, propeller and wheel on it");
    check(&flight, "the pod's flight");
    assert!(
        flight.iter().filter(|s| s.feet.len() == 2).count() >= 3,
        "Flynn and Cali on it, three steps at least"
    );
    assert!(stops.is_empty(), "{stops:?}");
    println!(
        "{} steps, {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}
