//! A Giant swapped in where a smaller hero stood (openspec AS.251), headless
//! on the disc's levels. Tree Rex's capsule is set down where a small hero
//! could stand, at every few metres of the level's walkable ground (a grid
//! of rays down through every layer of it, kept where Eruptor's sphere is
//! clear; those a flood from the start reaches are counted apart), through
//! the controller's set-down
//! (`Hero::push_out`) as the TV's replacement is, and each swap is read for
//! a hero left inside the level (a wall or ceiling deeper than 2 cm) and,
//! on the swaps the set-down moved and one in eight of the rest, for a hang
//! (60 steps of the stick: not supported and not moving for 30). Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_swap_fit -- --ignored --nocapture`
//!
//! `HUNT_EVERY` (default 4) is the metres between samples, `HUNT_CAP`
//! (default 200000) the flood's cells, `HUNT_MAX` (default 8000) the most
//! swaps a level gets (the samples are strided down to it).

use std::collections::{HashMap, VecDeque};

use guac_motion::ground;
use guac_motion::hero::{Hero, PushOut};
use guac_motion::seam::{Quat, Shape, Vec3};
use guac_motion::solid::HavokSolid;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_motion::tuning::Tuning;
use guac_script::Mode;
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_213: &str = "level_213_tutotial";
const LEVEL_214: &str = "level_214_tutotial_2";
const HUB: &str = "level_hub_2_stage_1";
const LEVEL_201: &str = "level_201";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// Tree Rex's capsule as his scripts set it.
fn rex() -> HavokSolid {
    HavokSolid {
        shape: Shape::Capsule {
            radius: 2.0836,
            half_segment: 1.1049,
        },
        centre: Vec3::new(0.0, 3.2385, 0.0),
        rotation: Quat::IDENTITY,
    }
}

/// A run joined and let settle, its intro or hub scene run out as the
/// suites' own do.
fn joined(level: &str) -> Run {
    let (mut run, _) = run_on_level(Mode::Play, level, setup());
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap();
    if level == LEVEL_214 {
        let stops = skip_214s_intro(&mut run);
        assert!(stops.is_empty(), "{stops:#?}");
    }
    if level.contains("hub") {
        let cutscene = run.vm.program().global_named("Cutscene State").unwrap();
        for _ in 0..200 {
            run.step_play().unwrap();
        }
        for _ in 0..6000 {
            let state = run.vm.global_peek(cutscene).map(|v| run.vm.value(v));
            if state.and_then(|v| v.as_i32()) == Some(0) {
                break;
            }
            run.step_play().unwrap();
        }
    }
    for _ in 0..120 {
        run.step_play().unwrap();
    }
    run
}

/// The ground a small hero walks to from `from`: cells a metre across, a
/// step up to 0.6 m and down to 4 m, never through a wall at knee height.
/// The ground point of each cell, in the order the flood reached it.
fn flood(run: &Run, from: Vec3, cap: usize) -> Vec<Vec3> {
    let world = &run.bench.world;
    let key = |p: Vec3| {
        (
            p.x.floor() as i32,
            (p.y / 1.5).floor() as i32,
            p.z.floor() as i32,
        )
    };
    let mut seen: HashMap<(i32, i32, i32), Vec3> = HashMap::new();
    let mut order = Vec::new();
    let mut queue = VecDeque::from([from]);
    seen.insert(key(from), from);
    order.push(from);
    while let Some(p) = queue.pop_front() {
        if seen.len() >= cap {
            break;
        }
        for (dx, dz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let over = p + Vec3::new(dx, 0.7, dz);
            let Some(hit) = ground::ray(world, over, Vec3::NEG_Y, 4.7, &[HERO_BODY]) else {
                continue;
            };
            if hit.normal.y < 0.65 {
                continue;
            }
            let to = hit.point;
            let (a, b) = (
                p + Vec3::Y * 0.8,
                Vec3::new(to.x, to.y.max(p.y) + 0.8, to.z),
            );
            let d = b - a;
            if ground::ray(world, a, d, d.length(), &[HERO_BODY]).is_some() {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(v) = seen.entry(key(to)) {
                v.insert(to);
                order.push(to);
                queue.push_back(to);
            }
        }
    }
    order
}

fn stick_for(view: f32, heading: f32) -> Stick {
    let d = (heading - view).to_radians();
    let raw = |a: f32| {
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
        }
    };
    Stick::new(raw(-d.sin()), raw(d.cos()))
}

fn pad(run: &mut Run, stick: Stick) {
    run.bench.pads[0] = Pad {
        stick,
        actions: [false; 4],
        connected: true,
    };
}

/// How far into a wall or ceiling he is (negative), 0 in none.
fn deepest(run: &Run) -> f32 {
    run.hero()
        .wall_gap(&run.bench.world, &[HERO_BODY], 0.0)
        .unwrap_or(0.0)
        .min(0.0)
}

#[derive(Default)]
struct Tally {
    swaps: usize,
    clear: usize,
    pushed: usize,
    searched: usize,
    stuck: usize,
    inside: Vec<String>,
    hung: Vec<String>,
    far: Vec<String>,
    walked: usize,
    worst_ms: u128,
    reachable: usize,
    reachable_bad: usize,
}

/// Every walkable layer of the level on a grid `every` metres across: rays
/// down from above through the whole level, one per layer, kept where the
/// ground can be stood on and the small hero's own set-down finds him clear.
fn layers(run: &Run, every: f32, small: &HavokSolid) -> Vec<Vec3> {
    let world = &run.bench.world;
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &run.files.placed {
        let at = Vec3::from_array(p.position);
        lo = lo.min(at);
        hi = hi.max(at);
    }
    let start = run.hero().pivot;
    lo = lo.min(start) - Vec3::splat(40.0);
    hi = hi.max(start) + Vec3::splat(40.0);
    let mut out = Vec::new();
    let mut x = lo.x;
    while x <= hi.x {
        let mut z = lo.z;
        while z <= hi.z {
            let mut from = Vec3::new(x, hi.y + 20.0, z);
            for _ in 0..12 {
                let Some(hit) = ground::ray(
                    world,
                    from,
                    Vec3::NEG_Y,
                    hi.y + 20.0 - lo.y + 20.0,
                    &[HERO_BODY],
                ) else {
                    break;
                };
                from = hit.point + Vec3::NEG_Y * 0.2;
                if hit.normal.y < 0.8 {
                    continue;
                }
                let mut hero = Hero::new(
                    small.clone(),
                    Tuning::ERUPTOR,
                    hit.point + Vec3::Y * 0.005,
                    0.0,
                );
                hero.stand(world, &[HERO_BODY]);
                if hero.push_out(world, &[HERO_BODY]) == PushOut::Clear && hero.footing.supported {
                    out.push(hit.point);
                }
            }
            z += every;
        }
        x += every;
    }
    out
}

/// Swaps whose set-down leaves him standing clear but who hang once the stick
/// is held: level 214's five wedged on a crease of two slopes (filed as
/// AS.259), the rest 0.
fn check(t: &Tally, known_hung: usize) {
    assert!(t.inside.is_empty(), "left inside: {:#?}", t.inside);
    assert_eq!(t.stuck, 0, "swaps no spot fit");
    assert!(
        t.hung.len() <= known_hung,
        "hung: {} of {known_hung} known: {:#?}",
        t.hung.len(),
        t.hung
    );
}

fn hunt(level: &str) -> Tally {
    let every: f32 = std::env::var("HUNT_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0);
    let cap: usize = std::env::var("HUNT_CAP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000);
    let max: usize = std::env::var("HUNT_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8000);
    let mut run = joined(level);
    let small = run.hero().solid().clone();
    let start = run.hero().pivot;
    let reached: std::collections::HashSet<(i32, i32, i32)> = flood(&run, start, cap)
        .iter()
        .map(|p| {
            (
                p.x.floor() as i32,
                (p.y / 1.5).floor() as i32,
                p.z.floor() as i32,
            )
        })
        .collect();
    let all = layers(&run, every, &small);
    let stride = all.len().div_ceil(max).max(1);
    let samples: Vec<Vec3> = all.iter().copied().step_by(stride).collect();
    println!(
        "=== {level}: start {start}, {} cells flooded, {} places a small hero stands every {every} m, {} sampled (every {stride})",
        reached.len(),
        all.len(),
        samples.len()
    );
    let mut t = Tally::default();
    for (n, at) in samples.iter().enumerate() {
        // The small hero stood here; the swap sets Tree Rex where he stood.
        run.hero_mut().set_solid(small.clone());
        let mut hero = Hero::new(rex(), Tuning::ERUPTOR, *at + Vec3::Y * 0.005, 0.0);
        hero.stand(&run.bench.world, &[HERO_BODY]);
        let clock = std::time::Instant::now();
        let out = hero.push_out(&run.bench.world, &[HERO_BODY]);
        t.worst_ms = t.worst_ms.max(clock.elapsed().as_millis());
        t.swaps += 1;
        match out {
            PushOut::Clear => t.clear += 1,
            PushOut::Pushed(_) => t.pushed += 1,
            PushOut::Searched(by) => {
                t.searched += 1;
                if by.length() > 15.0 {
                    t.far
                        .push(format!("{at} -> {} ({:.1} m)", hero.pivot, by.length()));
                }
            }
            PushOut::Stuck => t.stuck += 1,
        }
        // The place he's left in.
        *run.hero_mut() = hero;
        let inside = deepest(&run);
        let near = (-1..=1).any(|d| {
            reached.contains(&(
                at.x.floor() as i32,
                (at.y / 1.5).floor() as i32 + d,
                at.z.floor() as i32,
            ))
        });
        t.reachable += usize::from(near);
        if inside < -0.02 {
            t.reachable_bad += usize::from(near);
            t.inside.push(format!("{at}: {out:?}, {inside:.3} m in"));
        }
        // Does he walk? The swaps the set-down moved, and one in eight of the rest.
        if matches!(out, PushOut::Clear) && n % 8 != 0 {
            continue;
        }
        t.walked += 1;
        let heading = (n as f32 * 47.0) % 360.0;
        let (mut still, mut hung, mut last) = (0, 0, run.hero().pivot);
        for _ in 0..60 {
            let view = run.bench.view;
            pad(&mut run, stick_for(view, heading));
            run.step_play().unwrap();
            let h = run.hero();
            still = if !h.footing.supported && h.pivot.distance(last) < 1.0e-4 {
                still + 1
            } else {
                0
            };
            hung = hung.max(still);
            last = h.pivot;
        }
        if hung >= 30 {
            t.reachable_bad += usize::from(near);
            t.hung.push(format!("{at}: {out:?}, hung at {last}"));
        }
        pad(&mut run, Stick::new(0.0, 0.0));
    }
    println!(
        "{level}: {} swaps: {} clear, {} pushed, {} searched ({} over 15 m), {} stuck; {} left inside, {} walked, {} hung; {} on the flood's cells ({} of them bad); set-down worst {} ms",
        t.swaps,
        t.clear,
        t.pushed,
        t.searched,
        t.far.len(),
        t.stuck,
        t.inside.len(),
        t.walked,
        t.hung.len(),
        t.reachable,
        t.reachable_bad,
        t.worst_ms
    );
    for line in t.inside.iter().chain(&t.hung).chain(&t.far) {
        println!("   {line}");
    }
    t
}

/// Chapter 1's way: no swap leaves Tree Rex inside the level or hung.
#[test]
#[ignore = "disc"]
fn chapter_1() {
    let t = hunt(LEVEL_213);
    check(&t, 0);
}

#[test]
#[ignore = "disc"]
fn level_214() {
    let t = hunt(LEVEL_214);
    check(&t, 5);
}

#[test]
#[ignore = "disc"]
fn the_hub() {
    let t = hunt(HUB);
    check(&t, 0);
}

#[test]
#[ignore = "disc"]
fn level_201() {
    let t = hunt(LEVEL_201);
    check(&t, 0);
}
