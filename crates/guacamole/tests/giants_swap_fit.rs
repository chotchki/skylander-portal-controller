//! A Giant swapped in where a smaller hero stood (openspec AS.251), headless
//! on the disc's levels. Tree Rex's capsule is set down where a small hero
//! could stand, at every few metres of the level's walkable ground (a grid
//! of rays down through every layer of it, kept where Eruptor's sphere is
//! clear; those a flood from the start reaches are counted apart), through
//! the controller's set-down
//! (`Hero::push_out`) as the TV's replacement is, and each swap is read for
//! a hero left inside the level (a wall or ceiling deeper than 2 cm) by a
//! set-down that moved him, for one it set where this file's own walk from
//! his place doesn't reach (past a gap, a ledge or a solid: AS.251's review,
//! where the lane's search set him past an elemental wall, a breakable door
//! and Flynn's blocker), for one it couldn't move (`Stuck`), and, on the
//! swaps the set-down moved and one in eight of the rest, for a hang (60
//! steps of the stick: not supported and not moving for 30, and again so on
//! a fresh run, since the hunt's one run carries a walk's state into the
//! next: AS.259). Each swap carries the trail a small hero walked there
//! (AS.264): the flood's way from the level's start, or, for a swap stuck
//! with none, a walk in from the farthest ground within 40 m he walks both
//! ways. A sample the small hero stands inside a prop at once its props
//! stand round him (214's super hard rocks, which no small hero breaks) is
//! no place he stands, and is skipped by name. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_swap_fit -- --ignored --nocapture`
//!
//! `HUNT_EVERY` (default 4) is the metres between samples, `HUNT_CAP`
//! (default 200000) the flood's cells, `HUNT_MAX` (default 8000) the most
//! swaps a level gets (the samples are strided down to it).

use std::collections::{HashMap, VecDeque};

use guac_motion::ground;
use guac_motion::hero::{Hero, PushOut, Trail};
use guac_motion::seam::{BodyId, PhysicsSeam, Quat, Shape, Vec3};
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
    if level.contains("hub") || level == LEVEL_201 {
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
    // 201 opens with the hero coming down in his landing pod (AS.115), where
    // a put doesn't hold: the hunt waits until the pod lets him go.
    for _ in 0..3000 {
        if !run.hero().hung && run.hero().pivot.length() > 1.0 {
            break;
        }
        run.step_play().unwrap();
    }
    assert!(
        !run.hero().hung && run.hero().pivot.length() > 1.0,
        "{level}: still in the pod"
    );
    for _ in 0..120 {
        run.step_play().unwrap();
    }
    run
}

/// The ground a small hero walks to from `from`: cells a metre across, a
/// step up to 0.6 m and down to 4 m, never through a wall at knee height,
/// within `radius` of `from` across the ground, the bodies in `exclude`
/// out of the way of the knee-high ray but still ground under him, as the
/// set-down's walk has them (the hub's hatch by its steam pipes, a clone
/// the scripts made, is the deck a small hero walks onto off the perch
/// there: AS.264's review). The ground point of each cell, in the order the
/// flood reached it.
fn flood(run: &Run, from: Vec3, cap: usize, radius: f32, exclude: &[BodyId]) -> Vec<Vec3> {
    walk_from(run, from, cap, radius, exclude, 4.0).0
}

fn key(p: Vec3) -> (i32, i32, i32) {
    (
        p.x.floor() as i32,
        (p.y / 1.5).floor() as i32,
        p.z.floor() as i32,
    )
}

/// [`flood`]'s cells and, for each, the cell it was reached from.
fn walk_from(
    run: &Run,
    from: Vec3,
    cap: usize,
    radius: f32,
    exclude: &[BodyId],
    down: f32,
) -> (Vec<Vec3>, Vec<usize>) {
    let world = &run.bench.world;
    let mut seen: HashMap<(i32, i32, i32), Vec3> = HashMap::new();
    let mut order = Vec::new();
    let mut parent = Vec::new();
    let mut queue = VecDeque::from([(from, 0usize)]);
    seen.insert(key(from), from);
    order.push(from);
    parent.push(0);
    while let Some((p, at)) = queue.pop_front() {
        if seen.len() >= cap {
            break;
        }
        for (dx, dz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let over = p + Vec3::new(dx, 0.7, dz);
            if Vec3::new(over.x - from.x, 0.0, over.z - from.z).length() > radius {
                continue;
            }
            let Some(hit) = ground::ray(world, over, Vec3::NEG_Y, 4.7, &[HERO_BODY]) else {
                continue;
            };
            if hit.normal.y < 0.65 {
                continue;
            }
            let to = hit.point;
            if p.y - to.y > down {
                continue;
            }
            let (a, b) = (
                p + Vec3::Y * 0.8,
                Vec3::new(to.x, to.y.max(p.y) + 0.8, to.z),
            );
            let d = b - a;
            if ground::ray(world, a, d, d.length(), exclude).is_some() {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(v) = seen.entry(key(to)) {
                v.insert(to);
                order.push(to);
                parent.push(at);
                queue.push_back((to, order.len() - 1));
            }
        }
    }
    (order, parent)
}

/// The way a small hero walked from the level's start to `at` (AS.264): the
/// flood's cells back from the one under `at` to the start, oldest first,
/// as the step's [`Hero::catch_fall`] keeps his trail. Empty where no walk
/// from the start reaches.
fn trail_to(
    cells: &[Vec3],
    parent: &[usize],
    index: &HashMap<(i32, i32, i32), usize>,
    at: Vec3,
) -> Trail {
    let mut trail = Trail::default();
    let k = key(at);
    let Some(&end) = (-1..=1)
        .filter_map(|d| index.get(&(k.0, k.1 + d, k.2)))
        .min_by(|a, b| cells[**a].distance(at).total_cmp(&cells[**b].distance(at)))
    else {
        return trail;
    };
    let mut way = vec![at];
    let mut i = end;
    loop {
        way.push(cells[i]);
        if i == 0 {
            break;
        }
        i = parent[i];
    }
    for p in way.into_iter().rev() {
        trail.record(p);
    }
    trail
}

/// The level's placements within `r` of the way from `a` back along `way`,
/// by name: what a set-down back on the trail passed (AS.264).
fn near_way(run: &Run, way: &[Vec3], r: f32) -> Vec<String> {
    let mut out: Vec<(f32, String)> = Vec::new();
    for p in &run.files.placed {
        if p.obj.unit != LEVEL {
            continue;
        }
        let q = Vec3::from_array(p.position);
        let d = way
            .windows(2)
            .map(|w| {
                let ab = w[1] - w[0];
                let t = if ab.length_squared() < 1e-8 {
                    0.0
                } else {
                    ((q - w[0]).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                };
                (w[0] + ab * t).distance(q)
            })
            .fold(f32::MAX, f32::min);
        if d < r {
            out.push((d, p.name.clone()));
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out.dedup_by(|a, b| a.1 == b.1);
    out.into_iter()
        .map(|(d, n)| format!("{n}@{d:.1}"))
        .collect()
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

/// How far he's in the level's mesh or a prop (negative), the creatures and
/// the actors the scripts move left out, and what he's deepest in by name.
fn in_the_level(run: &Run) -> (f32, String) {
    let h = run.hero();
    let shape = h.solid().grown(0.0);
    let pose = guac_motion::seam::Pose::at(h.pivot + h.solid().centre);
    let mut exclude: Vec<BodyId> = run.bench.bodies.values().copied().collect();
    exclude.push(HERO_BODY);
    let planes = run.bench.world.contacts(&shape, pose, 0.0, &exclude);
    let Some(p) = planes
        .iter()
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
    else {
        return (0.0, "nothing".to_owned());
    };
    (p.distance.min(0.0), body_name(run, p))
}

fn body_name(run: &Run, p: &guac_motion::seam::ContactPlane) -> String {
    if p.triangle.is_some() {
        return "the level's mesh".to_owned();
    }
    let object = p.body.0.wrapping_sub(guacamole::props::PROP_BODIES);
    run.files
        .placed
        .iter()
        .find(|q| q.obj.unit == LEVEL && q.obj.index == object)
        .map_or(format!("body {}", p.body.0), |q| q.name.clone())
}

#[derive(Default)]
struct Tally {
    swaps: usize,
    clear: usize,
    pushed: usize,
    searched: usize,
    stuck: usize,
    /// Set back on the small hero's trail (AS.264), and where.
    trailed: usize,
    trailed_at: Vec<String>,
    /// Swaps no walk from the start reaches, given a walk in from nearby.
    local_trails: usize,
    /// Samples the small hero stands inside a prop at once its props stand.
    small_inside: Vec<String>,
    /// Hangs in the hunt's run that the same swap on a fresh run doesn't make.
    carried: Vec<String>,
    inside: Vec<String>,
    stuck_at: Vec<String>,
    /// Stuck swaps the stick moved more than half a metre in 60 steps.
    stuck_walks: usize,
    /// Clear swaps the stand after leaves more than 2 cm in something.
    leaned: Vec<String>,
    unwalked: Vec<String>,
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
                let mut hero = Hero::new(*small, Tuning::ERUPTOR, hit.point + Vec3::Y * 0.005, 0.0);
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

/// What a level's hunt is known to find: swaps no reachable spot or point
/// of the trail fits (`Stuck`, left where they came in: AS.264, the hub's
/// eight in Flynn's cabin's hall, a room Tree Rex fits nowhere in, AS.277,
/// and a perch on the hub's deck by its steam pipes and 201's one, each a
/// spot 1-2 m over the floor no small hero walks onto, which he walks off),
/// set-downs this file's walk doesn't reach (none: the AS.251 lane's search
/// made 3, 24, 10 and 4 on chapter 1, 214, the hub and 201, and the hub's
/// one off that perch onto a hatch 1.1 m down, which a small hero walks
/// onto, was this file's walk leaving the hatch out: AS.264's review), swaps
/// read clear that the stand after leans into something (AS.265), and
/// swaps whose set-down leaves him clear
/// but who hang once the stick is held on a fresh run as well as the
/// hunt's (none: AS.259's were the hunt's run, a scene a walk set off
/// holding him).
struct Known {
    stuck: usize,
    unwalked: usize,
    leaned: usize,
    hung: usize,
}

fn check(t: &Tally, known: Known) {
    assert!(t.inside.is_empty(), "left inside: {:#?}", t.inside);
    assert!(
        t.stuck <= known.stuck,
        "stuck: {} of {} known: {:#?}",
        t.stuck,
        known.stuck,
        t.stuck_at
    );
    assert!(
        t.unwalked.len() <= known.unwalked,
        "set where no walk reaches: {} of {} known: {:#?}",
        t.unwalked.len(),
        known.unwalked,
        t.unwalked
    );
    assert!(
        t.leaned.len() <= known.leaned,
        "leaned in after a clear: {} of {} known: {:#?}",
        t.leaned.len(),
        known.leaned,
        t.leaned
    );
    assert!(
        t.hung.len() <= known.hung,
        "hung: {} of {} known: {:#?}",
        t.hung.len(),
        known.hung,
        t.hung
    );
}

/// The swap of sample `n` at `at` on a fresh run of `level`, set up and
/// walked as [`hunt`] does it: whether he hangs there alone (AS.259).
fn hangs_alone(level: &str, at: Vec3, n: usize, small: HavokSolid) -> bool {
    let mut run = joined(level);
    run.hero_mut().set_solid(small);
    let facing = (n as f32 * 47.0) % 360.0;
    run.put(at, facing);
    pad(&mut run, Stick::new(0.0, 0.0));
    run.step_play().unwrap();
    let (from, facing) = (run.hero().pivot, run.hero().facing);
    let mut hero = Hero::new(rex(), Tuning::ERUPTOR, from, facing);
    if hero.push_out(&run.bench.world, &[HERO_BODY]) == PushOut::Clear {
        hero.stand(&run.bench.world, &[HERO_BODY]);
    }
    *run.hero_mut() = hero;
    run.hero_mut().pivot += Vec3::Y * guacamole::walkers::REPLACEMENT_DROP;
    let (mut still, mut hung, mut last) = (0, 0, run.hero().pivot);
    for _ in 0..60 {
        let view = run.bench.view;
        pad(&mut run, stick_for(view, (n as f32 * 47.0) % 360.0));
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
    hung >= 30
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
    let small = *run.hero().solid();
    let start = run.hero().pivot;
    let (cells, parent) = walk_from(&run, start, cap, f32::MAX, &[HERO_BODY], 4.0);
    let reached: HashMap<(i32, i32, i32), usize> = cells
        .iter()
        .enumerate()
        .map(|(i, p)| (key(*p), i))
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
        // The small hero stood here a step, the level's props standing and
        // its placements awake round him as the TV's are when a kid swaps
        // (AS.251's review: set down straight after the last sample's walk,
        // the props by an earth area's rocks weren't standing, and the
        // hunt's set-down parted from the TV's); the swap sets Tree Rex
        // where he stands, facing as he faced, read where he comes in (no
        // cast, as `play::enter`), and he stands there when it finds him in
        // no wall.
        run.hero_mut().set_solid(small);
        let facing = (n as f32 * 47.0) % 360.0;
        run.put(*at, facing);
        pad(&mut run, Stick::new(0.0, 0.0));
        run.step_play().unwrap();
        // With the props by it standing (they wake by their range as he
        // comes near), a spot can be inside one: among 214's earth area's
        // super hard rocks, which no small hero breaks. No small hero stands
        // there, so no Giant is swapped in for one (AS.264).
        let (small_in, what) = in_the_level(&run);
        if small_in < -0.25 {
            t.small_inside
                .push(format!("{at}: {small_in:.3} m in {what}"));
            continue;
        }
        let (from, facing) = (run.hero().pivot, run.hero().facing);
        let mut hero = Hero::new(rex(), Tuning::ERUPTOR, from, facing);
        // The way he walked there from the start, as the TV's replacement
        // takes the last hero's (AS.264).
        hero.trail = trail_to(&cells, &parent, &reached, from);
        let trail = hero.trail.clone();
        let clock = std::time::Instant::now();
        let mut out = hero.push_out(&run.bench.world, &[HERO_BODY]);
        t.worst_ms = t.worst_ms.max(clock.elapsed().as_millis());
        let mut trail = trail;
        if out == PushOut::Stuck && trail.is_empty() {
            // No walk from the level's start reaches here (a door, a gate or
            // an elemental wall the hunt's run holds shut): the walk in from
            // the farthest of the ground within 40 m a small hero walks both
            // ways (the flood's last cell), and the swap again with it.
            let (near, back) = walk_from(&run, from, 20_000, 40.0, &[HERO_BODY], 0.6);
            let mut way = Vec::new();
            let mut i = near.len() - 1;
            loop {
                way.push(near[i]);
                if i == 0 {
                    break;
                }
                i = back[i];
            }
            trail = Trail::default();
            for p in way {
                trail.record(p);
            }
            hero = Hero::new(rex(), Tuning::ERUPTOR, from, facing);
            hero.trail = trail.clone();
            out = hero.push_out(&run.bench.world, &[HERO_BODY]);
            t.local_trails += 1;
        }
        t.swaps += 1;
        match out {
            PushOut::Clear => {
                t.clear += 1;
                hero.stand(&run.bench.world, &[HERO_BODY]);
            }
            PushOut::Pushed(_) => t.pushed += 1,
            PushOut::Searched(by) => {
                t.searched += 1;
                if by.length() > 15.0 {
                    t.far
                        .push(format!("{from} -> {} ({:.1} m)", hero.pivot, by.length()));
                }
                // A small hero's walk from where he stood, the creatures out
                // of its way, reaches where the Giant was set.
                let mut exclude: Vec<BodyId> = run.bench.bodies.values().copied().collect();
                exclude.push(HERO_BODY);
                let to = hero.pivot;
                let walked = flood(&run, from, 20_000, 34.0, &exclude).iter().any(|p| {
                    Vec3::new(p.x - to.x, 0.0, p.z - to.z).length() < 1.2
                        && (p.y - to.y).abs() < 1.6
                });
                if !walked {
                    t.unwalked
                        .push(format!("{from} -> {to} ({:.1} m)", by.length()));
                }
            }
            PushOut::Trailed(by) => {
                t.trailed += 1;
                let to = hero.pivot;
                let mut exclude: Vec<BodyId> = run.bench.bodies.values().copied().collect();
                exclude.push(HERO_BODY);
                let across = Vec3::new(by.x, 0.0, by.z).length();
                let walked = flood(&run, from, 200_000, across + 8.0, &exclude)
                    .iter()
                    .any(|p| {
                        Vec3::new(p.x - to.x, 0.0, p.z - to.z).length() < 1.2
                            && (p.y - to.y).abs() < 1.6
                    });
                // The way walked back, and the placements beside it by name.
                let mut way = vec![from];
                for p in trail.points() {
                    way.push(p);
                    if Vec3::new(p.x - to.x, 0.0, p.z - to.z).length() < 0.6 {
                        break;
                    }
                }
                let names = near_way(&run, &way, 2.5);
                let line = format!(
                    "{from} -> {to} ({:.1} m back, {} trail points of {}): {}",
                    by.length(),
                    way.len() - 1,
                    trail.len(),
                    names.join(", ")
                );
                if !walked {
                    t.unwalked.push(format!("trailed {line}"));
                }
                t.trailed_at.push(line);
            }
            PushOut::Stuck => {
                t.stuck += 1;
                t.stuck_at.push(format!(
                    "{from} (trail {} points{})",
                    trail.len(),
                    if trail.is_empty() {
                        ", no walk from the start"
                    } else {
                        ""
                    }
                ));
            }
        }
        // The place he's left in.
        *run.hero_mut() = hero;
        let inside = deepest(&run);
        let near = (-1..=1).any(|d| {
            reached.contains_key(&(
                at.x.floor() as i32,
                (at.y / 1.5).floor() as i32 + d,
                at.z.floor() as i32,
            ))
        });
        t.reachable += usize::from(near);
        match out {
            PushOut::Stuck => {}
            // Read clear upright, then leaned on his ground by the stand
            // `play::enter` gives a clear replacement (AS.265).
            PushOut::Clear if inside < -0.02 => {
                t.leaned.push(format!("{from}: {inside:.3} m in"));
            }
            _ if inside < -0.02 => {
                t.reachable_bad += usize::from(near);
                t.inside.push(format!("{at}: {out:?}, {inside:.3} m in"));
            }
            _ => {}
        }
        // Does he walk? The swaps the set-down moved, and one in eight of the rest.
        if matches!(out, PushOut::Clear) && n % 8 != 0 {
            continue;
        }
        t.walked += 1;
        // He comes in over his place and drops onto it (AG.100), as the TV's.
        run.hero_mut().pivot += Vec3::Y * guacamole::walkers::REPLACEMENT_DROP;
        let heading = (n as f32 * 47.0) % 360.0;
        let start = run.hero().pivot;
        let (mut still, mut hung, mut last) = (0, 0, start);
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
        if out == PushOut::Stuck {
            t.stuck_walks += usize::from(last.distance(start) > 0.5);
        } else if hung >= 30 && !hangs_alone(level, *at, n, small) {
            // The hunt's one run carries each walk's state into the next (a
            // scene a walk set off holding the hero, AS.259's reading): a
            // hang the same swap doesn't make on a fresh run is the hunt's.
            t.carried
                .push(format!("{at}: hung at {last} in the hunt's run alone"));
        } else if hung >= 30 {
            t.reachable_bad += usize::from(near);
            // What held him: a script's hang (a scene's rig) or a scene.
            let cutscene = run.vm.program().global_named("Cutscene State");
            let scene = cutscene
                .and_then(|g| run.vm.global_peek(g))
                .and_then(|v| run.vm.value(v).as_i32());
            t.hung.push(format!(
                "{at}: {out:?}, hung at {last}, hung by a script {}, cutscene state {scene:?}",
                run.hero().hung
            ));
        }
        pad(&mut run, Stick::new(0.0, 0.0));
    }
    println!(
        "{level}: {} swaps ({} with a walk in from nearby, none from the start; {} samples skipped, the small hero inside a prop there): {} clear ({} leaned in by the stand), {} pushed, {} searched ({} over 15 m), {} set back on the trail, {} where no walk reaches, {} stuck ({} of them walking out); {} left inside, {} walked, {} hung ({} more the hunt's run alone); {} on the flood's cells ({} of them bad); set-down worst {} ms",
        t.swaps,
        t.local_trails,
        t.small_inside.len(),
        t.clear,
        t.leaned.len(),
        t.pushed,
        t.searched,
        t.far.len(),
        t.trailed,
        t.unwalked.len(),
        t.stuck,
        t.stuck_walks,
        t.inside.len(),
        t.walked,
        t.hung.len(),
        t.carried.len(),
        t.reachable,
        t.reachable_bad,
        t.worst_ms
    );
    for line in t
        .inside
        .iter()
        .chain(&t.stuck_at)
        .chain(&t.unwalked)
        .chain(&t.leaned)
        .chain(&t.hung)
        .chain(&t.far)
    {
        println!("   {line}");
    }
    for line in &t.trailed_at {
        println!("   TRAILED {line}");
    }
    for line in &t.small_inside {
        println!("   SMALL INSIDE {line}");
    }
    for line in &t.carried {
        println!("   CARRIED {line}");
    }
    t
}

/// Chapter 1's way: no swap leaves Tree Rex inside the level or hung.
#[test]
#[ignore = "disc"]
fn chapter_1() {
    let t = hunt(LEVEL_213);
    check(
        &t,
        Known {
            stuck: 0,
            unwalked: 0,
            leaned: 3,
            hung: 0,
        },
    );
}

#[test]
#[ignore = "disc"]
fn level_214() {
    let t = hunt(LEVEL_214);
    check(
        &t,
        Known {
            stuck: 0,
            unwalked: 0,
            leaned: 4,
            hung: 0,
        },
    );
}

#[test]
#[ignore = "disc"]
fn the_hub() {
    let t = hunt(HUB);
    check(
        &t,
        Known {
            stuck: 9,
            unwalked: 0,
            leaned: 0,
            hung: 0,
        },
    );
}

#[test]
#[ignore = "disc"]
fn level_201() {
    let t = hunt(LEVEL_201);
    check(
        &t,
        Known {
            stuck: 1,
            unwalked: 0,
            leaned: 3,
            hung: 0,
        },
    );
}
