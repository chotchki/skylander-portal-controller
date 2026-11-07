//! A hero knocked at a rim (openspec AS.155), headless with guacamole's host
//! on the TV's listing for each level. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_lips -- --ignored --nocapture --test-threads=1`

use std::collections::HashSet;

use guac_motion::edge::WALKABLE_RANGE;
use guac_motion::ground;
use guac_motion::proxy::Surfaces;
use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const CHAPTER_1: &str = "level_213_tutotial";
const LEVEL_214: &str = "level_214_tutotial_2";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// The stick along `heading`, past the dead zone per axis, against a view
/// looking along `view`.
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

/// A level from its start with its hero joined and settled.
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
    for _ in 0..120 {
        run.step_play().unwrap();
    }
    run
}

/// How far the stick walks him in the best of four headings, a good
/// second of each.
fn stick_reach(run: &mut Run) -> f32 {
    let from = run.hero().pivot;
    let mut best = 0.0f32;
    for heading in [0.0f32, 90.0, 180.0, 270.0] {
        for _ in 0..40 {
            let view = run.bench.view;
            pad(run, stick_for(view, heading));
            let stops = run.step_play().unwrap();
            assert!(stops.is_empty(), "{stops:#?}");
        }
        best = best.max(run.hero().pivot.distance(from));
    }
    pad(run, Stick::new(0.0, 0.0));
    best
}

/// openspec AS.155: chapter 1's lip by the first hat box, past a rim with
/// nothing under his pivot, where his walkable check refuses every heading
/// (the rim stands over his feet as a wall whose far side is the drop). His
/// gravity slides him off it and he falls; his scripts set him back on his
/// trail and the stick walks him. Before, he read a gap of 0 there, stood,
/// and only turned on the spot for good.
#[test]
#[ignore = "reads the Giants dump"]
fn off_a_lip_he_falls_and_his_scripts_set_him_back() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut run = joined(CHAPTER_1);
    let (_, box_at) = run.placed("HatBox_Pickup(1)_wind_Area");
    // On the ground by the box long enough for his trail to come with him,
    // as a walk there leaves it.
    run.put(box_at + Vec3::new(1.0, 1.0, 0.0), 0.0);
    pad(&mut run, Stick::new(0.0, 0.0));
    for _ in 0..600 {
        run.step_play().unwrap();
    }
    let stood = run.hero().pivot;
    run.put(box_at + Vec3::new(0.0, 1.0, 5.0), 0.0);
    let lip = run.hero().pivot;
    assert!(run.hero().on_a_lip(), "the rim holds him at {lip}");
    run.hero_mut().last_ground = Some(stood.y);
    let (mut lowest, mut back) = (lip.y, None);
    for k in 0..1200u32 {
        let stops = run.step_play().unwrap();
        assert!(stops.is_empty(), "{stops:#?}");
        lowest = lowest.min(run.hero().pivot.y);
        let h = run.hero();
        if lip.y - lowest > 10.0 && h.footing.supported && !h.on_a_lip() {
            back = Some(k);
            break;
        }
    }
    println!(
        "off {lip}, down to y {lowest:.1}, back after {back:?} at {}",
        run.hero().pivot
    );
    assert!(back.is_some(), "still at {}", run.hero().pivot);
    assert!(
        run.hero().pivot.distance(stood) < 15.0,
        "where he'd stood: {}",
        run.hero().pivot
    );
    for _ in 0..30 {
        run.step_play().unwrap();
    }
    let reach = stick_reach(&mut run);
    println!(
        "the stick walks him {reach:.2} m, {:.1} s",
        t.elapsed().as_secs_f32()
    );
    assert!(reach > 1.0, "a dead stick: {reach}");
}

/// A rim: the last ground a walk from a placement crosses with nothing, or
/// only a drop past the walkable check's reach, past it.
#[derive(Debug, Clone, Copy)]
struct Rim {
    edge: Vec3,
    /// Off it, degrees.
    heading: f32,
}

fn along(heading: f32) -> Vec3 {
    let (s, c) = heading.to_radians().sin_cos();
    Vec3::new(s, 0.0, c)
}

/// The rims a level's scripted placements stand near, one per 8 m cell, each
/// with 2 m of near-level ground behind it.
fn rims(run: &Run) -> Vec<Rim> {
    let world = &run.bench.world;
    let mut seeds = vec![run.hero().pivot];
    seeds.extend(
        run.files
            .placed
            .iter()
            .filter(|p| p.obj.unit == LEVEL && p.script.is_some())
            .map(|p| Vec3::from_array(p.position)),
    );
    let mut out = Vec::new();
    let mut cells = HashSet::new();
    for seed in seeds {
        let Some(from) = ground::find(world, seed + Vec3::Y, &[HERO_BODY]) else {
            continue;
        };
        if from.normal.y < 0.9 {
            continue;
        }
        for k in 0..16 {
            let heading = k as f32 * 22.5;
            let (mut last, mut crossed) = (from.point, 0.0f32);
            for s in 1..80 {
                let p = from.point + along(heading) * (s as f32 * 0.2);
                let at = Vec3::new(p.x, last.y + 0.2, p.z);
                let found = ground::find(world, at, &[HERO_BODY]);
                let drop = found.map_or(f32::MAX, |g| last.y - g.point.y);
                if drop > WALKABLE_RANGE {
                    if crossed >= 2.0
                        && cells.insert(((last.x / 8.0) as i32, (last.z / 8.0) as i32))
                    {
                        out.push(Rim {
                            edge: last,
                            heading,
                        });
                    }
                    break;
                }
                let Some(g) = found.filter(|g| drop <= 3.0 && g.normal.y >= 0.9) else {
                    break;
                };
                if g.point.y > last.y + 0.5 {
                    break;
                }
                last = g.point;
                crossed += 0.2;
            }
        }
    }
    out
}

/// The knockback's resist and last strength still on him: `Combat -
/// Knockback` does nothing until the last one has worn off.
fn knock_left(run: &Run) -> f32 {
    let m = run.minion.unwrap();
    let d = run.vm.local(m, "description").map(|v| run.vm.value(v));
    let t = run
        .vm
        .field(d.unwrap(), "state tracking")
        .map(|v| run.vm.value(v))
        .unwrap();
    let read = |n: &str| {
        run.vm
            .field(t, n)
            .map(|v| run.vm.value(v))
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0)
    };
    read("knockback current resist").max(read("knockback last knock strength"))
}

/// What a hit does, `Combat - Knockback` as `Combat - Hurt Creature` calls
/// it: the strengths are the disc's (a Chompy's and an Elf's 25, the
/// Spinner's 50, Tree Rex's 75; 100 a killing blow's).
fn knock(run: &mut Run, heading: f32, strength: i32) {
    let m = run.minion.unwrap();
    let d = run.vm.local(m, "description").map(|v| run.vm.value(v));
    let args = [
        ("target creature description", d.unwrap()),
        ("knockback heading", Value::Float(heading)),
        ("strength", Value::Int(strength)),
    ];
    run.with_host(|vm, host| vm.call_with(Some(m), "Combat - Knockback", &args, &[], host))
        .unwrap_or_else(|h| panic!("{h}"));
}

/// Steps while a scene holds the stick: walking by a trigger wakes one, and
/// the stick is its until it ends.
fn scene_out(run: &mut Run) {
    let cutscene = run.vm.program().global_named("Cutscene State").unwrap();
    for _ in 0..1200 {
        let state = run.vm.global_peek(cutscene).map(|v| run.vm.value(v));
        if state.and_then(|v| v.as_i32()) == Some(0) {
            return;
        }
        run.step_play().unwrap();
    }
}

#[derive(Debug, Default)]
struct Tally {
    knocks: u32,
    moved: u32,
    /// Past the rim, with a rim holding him on some step of it.
    held: u32,
    /// Came to rest with a rim alone holding him.
    rested: u32,
    fell: u32,
    dead: Vec<(Vec3, f32, i32, f32)>,
}

/// Knocks at every `every`th of a level's rims within `reach` of its start,
/// from a run-up that lands him on and past the rim. Each ends with him on
/// ground the stick walks him on or falling back to it: whatever a knock
/// leaves, the stick isn't dead. (The reach keeps him off what a set-down
/// past the fights wakes for good: the pans by level 214's third island's
/// turrets hold him, waiting on foes nobody has beaten.)
fn knocked(level: &'static str, every: usize, reach: f32) -> Tally {
    let t = std::time::Instant::now();
    println!("{level}: load at start: {}", load());
    let mut run = joined(level);
    let start = run.hero().pivot;
    let mut rims = rims(&run);
    rims.retain(|r| r.edge.distance(start) < reach);
    let mut tally = Tally::default();
    for rim in rims.iter().step_by(every) {
        for strength in [25, 50, 75, 100] {
            for back in [0.4f32, 1.0, 1.6, 2.2] {
                let from = rim.edge - along(rim.heading) * back + Vec3::Y * 0.5;
                run.put(from, rim.heading);
                pad(&mut run, Stick::new(0.0, 0.0));
                for _ in 0..6 {
                    run.step_play().unwrap();
                }
                for _ in 0..600 {
                    if knock_left(&run) <= 0.0 {
                        break;
                    }
                    run.step_play().unwrap();
                }
                let at = run.hero().pivot;
                if (at.y - rim.edge.y).abs() > 0.5 {
                    continue;
                }
                tally.knocks += 1;
                knock(&mut run, rim.heading, strength);
                let (mut low, mut still, mut held) = (at.y, 0, false);
                let mut moved = 0.0f32;
                for _ in 0..600 {
                    let stops = run.step_play().unwrap();
                    assert!(stops.is_empty(), "{stops:#?}");
                    let h = run.hero();
                    low = low.min(h.pivot.y);
                    moved = moved.max(h.pivot.distance(at));
                    held |= h.on_a_lip();
                    still = if h.velocity.length() < 0.01 {
                        still + 1
                    } else {
                        0
                    };
                    if still >= 45 {
                        break;
                    }
                }
                tally.moved += u32::from(moved > 0.1);
                tally.held += u32::from(held);
                tally.fell += u32::from(at.y - low > 3.0);
                let rested = run.hero().on_a_lip();
                tally.rested += u32::from(rested);
                if moved > 0.1 && (rested || run.hero().pivot.distance(rim.edge) < 2.5) {
                    scene_out(&mut run);
                    let reach = stick_reach(&mut run);
                    if reach < 0.1 {
                        tally.dead.push((rim.edge, rim.heading, strength, back));
                    }
                }
            }
        }
    }
    println!(
        "{level}: {} rims, every {every}th knocked: {tally:?}, {:.1} s, load {}",
        rims.len(),
        t.elapsed().as_secs_f32(),
        load()
    );
    tally
}

/// openspec AS.155: knocked at chapter 1's rims, by every strength the disc
/// hits a hero with, from short of a rim to on it. Some carry him over, and
/// none leaves him where the stick can't walk him.
#[test]
#[ignore = "reads the Giants dump"]
fn no_knock_at_chapter_1s_rims_kills_the_stick() {
    let tally = knocked(CHAPTER_1, 36, f32::MAX);
    assert!(tally.moved > 0 && tally.fell > 0, "{tally:?}");
    assert!(tally.dead.is_empty(), "{:#?}", tally.dead);
    assert_eq!(tally.rested, 0, "resting on a rim: {tally:?}");
}

/// openspec AS.155: level 214's, the same.
#[test]
#[ignore = "reads the Giants dump"]
fn no_knock_at_214s_rims_kills_the_stick() {
    let tally = knocked(LEVEL_214, 16, 500.0);
    assert!(tally.moved > 0, "{tally:?}");
    assert!(tally.dead.is_empty(), "{:#?}", tally.dead);
    assert_eq!(tally.rested, 0, "resting on a rim: {tally:?}");
}

/// The knocks that came to rest on a lip before the fix, on chapter 1: a rim,
/// off it, a strength and a run-up.
const LIPS: [([f32; 3], f32, i32, f32); 5] = [
    ([-90.07951, -139.60721, 306.2678], 90.0, 50, 1.0),
    ([-90.07951, -139.60721, 306.2678], 90.0, 75, 0.6),
    ([-90.07951, -139.60721, 306.2678], 90.0, 75, 1.2),
    ([-90.07951, -139.60721, 306.2678], 90.0, 100, 1.0),
    ([-33.591026, -153.50325, 301.00708], 247.5, 50, 0.6),
];

/// openspec AS.155: those knocks, each from where the sweep found it. The
/// hero comes off the rim now: nothing leaves him held by it at rest.
#[test]
#[ignore = "reads the Giants dump"]
fn a_knock_onto_a_lip_doesnt_leave_him_on_it() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut run = joined(CHAPTER_1);
    let (mut held, mut knocks) = (0, 0);
    for (edge, heading, strength, back) in LIPS {
        let rim = Rim {
            edge: Vec3::from_array(edge),
            heading,
        };
        let from = rim.edge - along(rim.heading) * back + Vec3::Y * 0.5;
        run.put(from, rim.heading);
        pad(&mut run, Stick::new(0.0, 0.0));
        for _ in 0..6 {
            run.step_play().unwrap();
        }
        for _ in 0..600 {
            if knock_left(&run) <= 0.0 {
                break;
            }
            run.step_play().unwrap();
        }
        scene_out(&mut run);
        let at = run.hero().pivot;
        println!("set down at {at}");
        knock(&mut run, rim.heading, strength);
        knocks += 1;
        let mut still = 0;
        let mut on_it = 0;
        for _ in 0..600 {
            let stops = run.step_play().unwrap();
            assert!(stops.is_empty(), "{stops:#?}");
            on_it += u32::from(run.hero().on_a_lip());
            still = match run.hero().velocity.length() < 0.01 {
                true => still + 1,
                false => 0,
            };
            if still >= 45 {
                break;
            }
        }
        held += u32::from(on_it > 0);
        println!(
            "{strength} from {back} at {edge:?}: {on_it} steps on it, {:.2} m on at {}",
            run.hero().pivot.distance(at),
            run.hero().pivot
        );
        if run.hero().on_a_lip() {
            scene_out(&mut run);
            let reach = stick_reach(&mut run);
            panic!(
                "at rest on a rim: {}, the stick walking him {reach} m",
                run.hero().pivot
            );
        }
    }
    println!(
        "{knocks} knocks, {held} reached one, {:.1} s, load {}",
        t.elapsed().as_secs_f32(),
        load()
    );
    assert!(held > 0, "none reached a lip");
}

/// openspec AS.159: where Eruptor stopped on the TV (`runs/a7`), walking off
/// level 214's top after a foe on the ledge below, no hit on him: 0.39 m into
/// the cliff under its rim, in the air at his fall's full speed. Every cast
/// stopped at once, from every heading, and neither he nor the stick moved
/// again. He comes out of the face, lands and walks.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_in_214s_cliff_comes_out_and_lands() {
    const IN_THE_CLIFF: Vec3 = Vec3::new(-229.5532, -16.840977, 831.4073);
    let mut run = joined(LEVEL_214);
    for heading in [None, Some(270.0), Some(90.0), Some(0.0), Some(180.0)] {
        run.put(IN_THE_CLIFF + Vec3::new(5.0, 2.0, 0.0), 0.0);
        for _ in 0..30 {
            run.step_play().unwrap();
        }
        run.hero_mut().pivot = IN_THE_CLIFF;
        let view = run.bench.view;
        let stick = heading.map_or(Stick::new(0.0, 0.0), |h| stick_for(view, h));
        let mut landed = None;
        for k in 0..90 {
            pad(&mut run, stick);
            run.step_play().unwrap();
            if run.hero().footing.supported {
                landed = Some(k);
                break;
            }
        }
        let at = run.hero().pivot;
        println!("{heading:?}: landed after {landed:?} steps at {at:?}");
        assert!(landed.is_some(), "still hanging at {at:?}");
        assert!(at.y < IN_THE_CLIFF.y, "{at:?}");
        let walked = stick_reach(&mut run);
        println!("  the stick walks him {walked:.2}");
        assert!(walked > 1.0, "a dead stick: {walked}");
    }
}

/// The AS.159 review: what puts him in that cliff. Off the rim's 30° bank he
/// leans up to 40°, and his lean turns upright in the air (AG.71) with no
/// cast moving him, swinging his shape back into the face a centimetre or two
/// a step. No step's move takes him in: a cast keeps him out, as far as it
/// goes. Walked off where `runs/a7` was, after a foe on the ledge below, with
/// the stick on him he's in the face and out of it again, and lands.
#[test]
#[ignore = "reads the Giants dump"]
fn his_lean_turning_upright_swings_him_into_214s_cliff() {
    let mut run = joined(LEVEL_214);
    let foe = Vec3::new(-224.56, -18.6, 826.84);
    let gap = |run: &Run, h: &guac_motion::hero::Hero| {
        h.wall_gap(&run.bench.world, &[HERO_BODY], 0.0)
            .unwrap_or(0.0)
            .min(0.0)
    };
    let (mut by_the_move, mut by_the_lean) = (0.0f32, 0.0f32);
    for (x, z, aim) in [(-231.5f32, 831.6f32, -40.0f32), (-235.5, 832.0, -25.0)] {
        run.put(Vec3::new(x, -14.5, z), 0.0);
        pad(&mut run, Stick::new(0.0, 0.0));
        for _ in 0..20 {
            run.step_play().unwrap();
        }
        let d = foe - run.hero().pivot;
        let heading = d.x.atan2(d.z).to_degrees() + aim;
        let mut landed = false;
        for _ in 0..200 {
            let before = run.hero().clone();
            let view = run.bench.view;
            pad(&mut run, stick_for(view, heading));
            run.step_play().unwrap();
            let after = run.hero().clone();
            let mut moved = after.clone();
            moved.up = before.up;
            let (was, went, now) = (gap(&run, &before), gap(&run, &moved), gap(&run, &after));
            by_the_move += (was - went).max(0.0);
            by_the_lean += (went - now).max(0.0);
            if after.footing.supported && after.pivot.y < -18.0 {
                landed = true;
                break;
            }
        }
        println!("from ({x}, {z}): at {}", run.hero().pivot);
        assert!(landed, "hung at {}", run.hero().pivot);
    }
    println!("into the face by the moves {by_the_move:.3}, by the lean {by_the_lean:.3}");
    assert!(by_the_lean > 0.1, "{by_the_lean}");
    assert!(by_the_move < 0.005, "{by_the_move}");
}

/// openspec AS.159: where Tree Rex stood on the TV with the stick on the way
/// back, slipped off the pit's edge and 68 m under the level's underside.
/// Its ground is the finder's upward ray's, none under him, so he's fallen,
/// past his check's 225 ft by the ground he last had, and it sets him back.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_under_the_level_is_set_back() {
    const UNDER: Vec3 = Vec3::new(-8.2253275, -18.914867, -31.966932);
    let mut run = joined(CHAPTER_1);
    run.hero_mut().pivot = UNDER;
    for _ in 0..3 {
        run.step_play().unwrap();
    }
    let height = run.hero().footing.height;
    println!("the ground at {height:?}");
    assert!(height.is_some_and(|h| h < -60.0), "{height:?}");
    for _ in 0..30 {
        run.step_play().unwrap();
    }
    let at = run.hero().pivot;
    println!("half a second on at {at:?}");
    assert!(at.y > 20.0, "hung under the level: {at:?}");
}

/// The AS.159 review, on `runs/s1` of au-pop-fizz's: the bank on level 214's
/// third island, 61° off level, where that run's Eruptor wedged on his way to
/// a fight, falling at 120 ft/s with the stick only turning him. The first
/// spot is 19 cm in, the second 9, and the recovery out of a plane left him a
/// hair in it, every cast along the bank stopped at once and no plane against
/// it, for good. He slides off it, lands and walks. An engine's fall: no
/// fight of the third island's is woken by the set-down.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_a_hair_in_the_third_islands_bank_slides_off_it() {
    let mut run = joined(LEVEL_214);
    for spot in [
        Vec3::new(658.4268, -17.78636, 484.40433),
        Vec3::new(660.4773, -19.832815, 485.80066),
    ] {
        let start = run.hero().clone();
        let hero = run.hero_mut();
        hero.pivot = spot;
        hero.up = Vec3::Y;
        hero.walk.velocity = Vec3::new(0.0, -120.0, 0.0);
        let world = &run.bench.world;
        let mut h = run.hero().clone();
        let mut landed = None;
        for k in 0..240u32 {
            h.step_engine(world, 1.0 / 60.0, &[HERO_BODY], Surfaces::NONE);
            if h.footing.supported {
                landed = Some(k);
                break;
            }
        }
        println!("{spot}: landed after {landed:?} steps at {}", h.pivot);
        assert!(landed.is_some(), "still hanging at {}", h.pivot);
        assert!(h.pivot.y < -22.0, "off the bank: {}", h.pivot);
        *run.hero_mut() = start;
    }
}

/// The AS.159 review: behind chapter 1's faces, where a hero the level's
/// shell lets past has only triangles' backs round him. A cast stops a few
/// microns on, under what a float moves him here, with no plane to lay him
/// along: he hung at 120 ft/s, the stick dead for good. He falls on.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_behind_chapter_1s_faces_falls_on() {
    let run = joined(CHAPTER_1);
    let world = &run.bench.world;
    for spot in [
        Vec3::new(-24.336582, -134.11949, 128.32382),
        Vec3::new(-34.813004, -149.0437, 315.4434),
    ] {
        let mut h = run.hero().clone();
        h.pivot = spot;
        h.up = Vec3::Y;
        h.walk.velocity = Vec3::new(0.0, -120.0, 0.0);
        for _ in 0..60 {
            h.step_engine(world, 1.0 / 60.0, &[HERO_BODY], Surfaces::NONE);
            if h.footing.supported {
                break;
            }
        }
        println!("{spot}: on to {} in a second", h.pivot);
        assert!(h.pivot.distance(spot) > 0.3, "hung at {}", h.pivot);
    }
}
