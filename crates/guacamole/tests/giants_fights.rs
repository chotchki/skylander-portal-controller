//! Level 214 fought through past its first fight (openspec AS.58, AS.59,
//! AS.67), headless with guacamole's host on the TV's listing for the level.
//! Ignored by default; they read `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_fights -- --ignored --nocapture --test-threads=1`
//!
//! No recording of 214 exists: every step is the level's own scripts on the
//! TV's listing, the hero set down where a walk would take him.

use std::collections::{BTreeMap, HashMap};

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

/// The level the fights are in.
const LEVEL_214: &str = "level_214_tutotial_2";

/// Where level 214's first bomb spawner stands (metres, its file's place).
const BOMB_SPAWNER: [f32; 3] = [39.005, -16.018, 27.887];
/// Short of the castle wall in front of the pods, facing it.
const BEFORE_THE_WALL: ([f32; 3], f32) = ([50.0, -17.9, 30.0], 77.7);
/// Among the pods, short of the castle wall past them
/// (`Breakable_Stone_Wall_Castle_206(6)`), facing it.
const BEFORE_THE_SECOND_WALL: ([f32; 3], f32) = ([67.6, -17.9, 36.0], 73.9);

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// The level's run with Eruptor joined and his arrival played out (the
/// TV's entry settles him so), and what stopped on the way.
fn joined_214() -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    // Its intro skipped as a player would (AS.81).
    stops.extend(skip_214s_intro(&mut run));
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    (run, stops.iter().map(ToString::to_string).collect())
}

fn step(run: &mut Run, stops: &mut Vec<String>, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick: Stick::new(0.0, 0.0),
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

fn behaviour(run: &Run, h: Handle) -> Option<String> {
    run.vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_owned)
}

/// The bombs standing: a burst bomb that lay where it burst would be one.
fn bombs(run: &Run) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Bomb_Template")
        .filter(|h| run.world.actor(*h).is_some_and(|a| !a.removed))
        .collect()
}

/// Set down 3 m short of the first spawner until its bomb is carried, at
/// most `steps`: the bomb and the step it was taken at.
fn take_the_bomb(run: &mut Run, stops: &mut Vec<String>, steps: u32) -> Option<(Handle, u32)> {
    run.put(Vec3::from_array(BOMB_SPAWNER) - Vec3::X * 3.0, 90.0);
    for _ in 0..steps {
        step(run, stops, [false; 4]);
        if let Some(&b) = bombs(run).first()
            && behaviour(run, b).as_deref() == Some("following player")
        {
            return Some((b, run.steps));
        }
    }
    None
}

/// Set down at `before`, facing its wall, △ tapped a step: the step it was
/// thrown at and the step `wall` went, within 300 steps.
fn throw_at(
    run: &mut Run,
    stops: &mut Vec<String>,
    bomb: Handle,
    before: ([f32; 3], f32),
    wall: Handle,
) -> (Option<u32>, Option<u32>) {
    run.put(Vec3::from_array(before.0), before.1);
    for _ in 0..30 {
        step(run, stops, [false; 4]);
    }
    let (mut thrown, mut opened) = (None, None);
    for k in 0..300u32 {
        step(run, stops, [false, false, false, k == 0]);
        if thrown.is_none() && behaviour(run, bomb).as_deref() == Some("bomb") {
            thrown = Some(run.steps);
        }
        if opened.is_none() && run.world.actor(wall).is_some_and(|a| a.removed) {
            opened = Some(run.steps);
        }
        if opened.is_some() {
            break;
        }
    }
    (thrown, opened)
}

/// openspec AS.67: level 214's bomb back on its spawner after its burst.
/// The first bomb opens the wall before the pods (AS.57's throw); its burst
/// done, it resets itself, which sends its script round from its STARTUP and
/// stands it again where its spawner cloned it, waiting to be taken: the
/// same bomb, nothing left standing at the blast, the spawner cloning no
/// other. Taken again from the spawner and carried among the pods, a second
/// tap of △ throws it at the castle wall past them,
/// `Breakable_Stone_Wall_Castle_206(6)`, which its burst opens. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_bomb_comes_back_for_the_next_wall() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let (bomb, taken) = take_the_bomb(&mut run, &mut stops, 240).expect("the bomb taken");
    let (first_wall, _) = run.placed("Breakable_Stone_Wall_Castle_206(1)");
    let (second_wall, _) = run.placed("Breakable_Stone_Wall_Castle_206(6)");
    let (thrown, opened) = throw_at(&mut run, &mut stops, bomb, BEFORE_THE_WALL, first_wall);
    let (thrown, opened) = (thrown.expect("thrown"), opened.expect("the wall opened"));
    let burst_at = run.world.actor(bomb).map(|a| a.position);
    // Its burst booms once: a second start the step after was refused while
    // the burst bomb stayed in its burst (the as-214-walk review).
    let mut booms: std::collections::BTreeSet<u64> = run
        .world
        .plays()
        .iter()
        .filter(|p| p.name.as_deref() == Some("Bomb_Boom"))
        .map(|p| p.serial)
        .collect();
    // Its burst done, it goes round: back where it was cloned, waiting.
    let spawner = Vec3::from_array(BOMB_SPAWNER);
    let mut back = None;
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        booms.extend(
            run.world
                .plays()
                .iter()
                .filter(|p| p.name.as_deref() == Some("Bomb_Boom"))
                .map(|p| p.serial),
        );
        let at = run.world.actor(bomb).map(|a| a.position);
        if back.is_none()
            && behaviour(&run, bomb).as_deref() == Some("wait to be grabbed")
            && at.is_some_and(|p| Vec3::new(p.x - spawner.x, 0.0, p.z - spawner.z).length() < 0.5)
        {
            back = Some((run.steps, at.unwrap()));
        }
    }
    let back = back.expect("the bomb back on its spawner, waiting");
    let refused = run
        .world
        .sounds_refused()
        .filter(|r| r.name.as_deref() == Some("Bomb_Boom"))
        .count();
    println!(
        "taken at {taken}, thrown at {thrown}, the wall opened at {opened}, burst at {burst_at:?}; back at step {} at {}; booms {booms:?}, refused {refused}",
        back.0, back.1
    );
    assert_eq!(booms.len(), 1, "one boom: {booms:?}");
    assert_eq!(refused, 0, "no second start refused");
    // The second spawner, past the pods, clones its own as he nears it.
    let from_first: Vec<Handle> = bombs(&run)
        .into_iter()
        .filter(|h| {
            run.world
                .actor(*h)
                .is_some_and(|a| a.origin.distance(spawner) < 3.0)
        })
        .collect();
    assert_eq!(from_first, [bomb], "the same bomb, no other");
    assert!(
        run.world.actor(bomb).unwrap().origin.distance(spawner) < 0.01,
        "cloned on the spawner"
    );
    // Off the spawner first, so the take is a walk-up again.
    run.put(Vec3::from_array(BEFORE_THE_WALL.0), BEFORE_THE_WALL.1);
    for _ in 0..30 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (again, retaken) = take_the_bomb(&mut run, &mut stops, 240).expect("taken again");
    assert_eq!(again, bomb);
    let (thrown2, opened2) = throw_at(
        &mut run,
        &mut stops,
        bomb,
        BEFORE_THE_SECOND_WALL,
        second_wall,
    );
    println!(
        "taken again at {retaken}, thrown at {thrown2:?}, the second wall opened at {opened2:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(thrown2.is_some(), "thrown again");
    assert!(opened2.is_some(), "the second wall opened");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// A countdown number of a carried bomb (openspec AS.68): its timer, its
/// count, the step it was made and went, how far it stood off its bomb at
/// most across the ground once made, and its sprite.
#[derive(Debug)]
struct Timer {
    handle: Handle,
    count: i32,
    made: u32,
    gone: Option<u32>,
    off: f32,
    sprite: Option<Handle>,
}

/// openspec AS.68: a carried bomb counts down over its carrier. The first
/// spawner's bomb taken (its count 12), each count past the first clones a
/// timer at it (`BombTImer.ai`), its id the count: 11 to 1, a count every
/// 1.1 s, then the bomb bursts in his hands at 0. Each timer follows its
/// bomb by `move to` (steered with no body, at its own 17 ft/s, so it lags
/// a walking carrier), fades its sprite out over 2 s, then takes itself
/// off, its sprite with it. He paces on the stick as he carries it. Each
/// count draws in its digits' glyphs (openspec AS.99), fading out with its
/// sprite. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_carried_bomb_counts_down_over_him() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let (bomb, taken) = take_the_bomb(&mut run, &mut stops, 240).expect("the bomb taken");
    let ends = [[36.0, 27.9], [50.0, 30.0]];
    let mut timers: Vec<Timer> = Vec::new();
    let mut burst = None;
    let mut drawn: BTreeMap<i32, (u32, u32, u8)> = BTreeMap::new();
    let mut off: f32 = 0.0;
    for k in 0..1000u32 {
        push_toward(&mut run, &mut stops, ends[((k / 150) % 2) as usize]);
        let b = run.world.actor(bomb).map(|a| a.position);
        if burst.is_none() && behaviour(&run, bomb).as_deref() != Some("following player") {
            burst = Some(run.steps);
        }
        for h in run.world.clones() {
            if run.world.name(h) != "Bomb_Timer" {
                continue;
            }
            let Some(a) = run.world.actor(h) else {
                continue;
            };
            let t = match timers.iter().position(|t| t.handle == h) {
                Some(i) => &mut timers[i],
                None => {
                    timers.push(Timer {
                        handle: h,
                        count: a.id,
                        made: run.steps,
                        gone: None,
                        off: 0.0,
                        sprite: None,
                    });
                    timers.last_mut().unwrap()
                }
            };
            if t.sprite.is_none()
                && let Some(id) = run.vm.placement_of(h)
                && let Some(Value::Entity(s)) =
                    run.vm.local(id, "my sprite").map(|v| run.vm.value(v))
            {
                t.sprite = Some(s);
            }
            if a.removed {
                t.gone.get_or_insert(run.steps);
            } else if let Some(b) = b
                && run.steps > t.made
            {
                t.off = t
                    .off
                    .max(Vec3::new(a.position.x - b.x, 0.0, a.position.z - b.z).length());
            }
        }
        // What the TV draws of them (openspec AS.99): each count in glyphs,
        // the sprite fading as its timer says.
        let quads = run.world.drawn_sprites();
        for q in &quads {
            let d = drawn.entry(q.value as i32).or_insert((0, 0, 255));
            d.0 = d.0.max(1);
            d.1 += 1;
            d.2 = d.2.min(q.colour[3]);
            assert_eq!(q.sprite, "Bomb_Counter");
            assert!(q.rect[2] > q.rect[0] && q.rect[3] > q.rect[1], "{q:?}");
            // Over its timer on the screen: the follow's update keeps it
            // there as it moves, a step behind at most, 20 units across.
            let timer = run.world.clones().into_iter().find(|h| {
                run.world.name(*h) == "Bomb_Timer"
                    && run.world.actor(*h).is_some_and(|a| a.id == q.value as i32 && !a.removed)
            });
            if let Some(a) = timer.and_then(|h| run.world.actor(h))
                && let Some((x, _)) = run.world.viewport().project(a.position)
            {
                off = off.max((q.rect[0] - (x + 20.0)).abs());
            }
        }
        let by_count = quads.iter().fold(HashMap::new(), |mut m, q| {
            *m.entry(q.value as i32).or_insert(0usize) += 1;
            m
        });
        for (count, glyphs) in by_count {
            let digits = if count >= 10 { 2 } else { 1 };
            assert_eq!(glyphs, digits, "count {count} at step {}", run.steps);
        }
        // Its last timer gone, before he takes it again off the spawner.
        if burst.is_some_and(|s| run.steps > s + 60) {
            break;
        }
    }
    let burst = burst.expect("the bomb burst in his hands");
    let mut counts_drawn: Vec<i32> = drawn.keys().copied().collect();
    counts_drawn.sort_unstable();
    println!("drawn {drawn:?}, at most {off:.1} px off its timer");
    assert!(off < 30.0, "{off}");
    assert_eq!(
        counts_drawn,
        (1..=11).collect::<Vec<_>>(),
        "every count drawn"
    );
    for (count, (_, frames, least)) in &drawn {
        let digits = if *count >= 10 { 2 } else { 1 };
        assert!(*frames / digits > 100, "{count} on {frames} glyph frames");
        assert!(*least < 40, "{count} fades out: {least}");
    }
    // All but the first count of the bomb he's taken again by now.
    let left = run.world.drawn_sprites();
    assert!(left.iter().all(|q| q.value == 11.0), "{left:?}");
    timers.retain(|t| t.made < burst);
    for t in &timers {
        println!(
            "  timer {} made at {}, gone at {:?}, at most {:.2} m off its bomb, its sprite {:?} gone {}",
            t.count,
            t.made,
            t.gone,
            t.off,
            t.sprite,
            t.sprite
                .is_some_and(|s| run.world.actor(s).is_none_or(|a| a.removed))
        );
    }
    println!(
        "taken at {taken}, burst at {burst} ({:.2} s); {} timers; {:.1} s; load at end: {}",
        (burst - taken) as f32 / 60.0,
        timers.len(),
        started.elapsed().as_secs_f64(),
        load()
    );
    let counts: Vec<i32> = timers.iter().map(|t| t.count).collect();
    assert_eq!(
        counts,
        (1..=11).rev().collect::<Vec<_>>(),
        "a timer a count"
    );
    for (t, next) in timers.iter().zip(timers.iter().skip(1)) {
        assert_eq!(
            next.made - t.made,
            66,
            "a count every 1.1 s: {t:?} {next:?}"
        );
    }
    for t in &timers {
        assert_eq!(t.gone.map(|g| g - t.made), Some(120), "gone 2 s on: {t:?}");
        assert!(t.off < 4.0, "it follows its bomb: {t:?}");
        let s = t.sprite.expect("its sprite");
        assert!(
            run.world.actor(s).is_none_or(|a| a.removed),
            "its sprite gone with it: {t:?}"
        );
    }
    assert!(burst > timers.last().unwrap().made, "the burst at 0");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// The stick that pushes at `strength` along `heading` against a view
/// looking along `view`, past the dead zone per axis
/// (`guac_motion::stick`), as giants_route's driver makes it.
fn stick_for(view: f32, heading: f32, strength: f32) -> Stick {
    let d = (heading - view).to_radians();
    let raw = |a: f32| {
        let a = a * strength;
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
        }
    };
    Stick::new(raw(-d.sin()), raw(d.cos()))
}

/// A step with the stick held toward `to` (across the ground), the view
/// the step before's.
fn push_toward(run: &mut Run, stops: &mut Vec<String>, to: [f32; 2]) {
    let at = run.hero().pivot;
    let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
    run.bench.pads[0] = Pad {
        stick: stick_for(run.bench.view, heading, 1.0),
        actions: [false; 4],
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

/// Walked on the stick through `points` (across the ground), each within
/// 1 m, at most 1200 steps each, then 20 steps at rest: whether he got to
/// each, and where he stopped if not.
fn walk(run: &mut Run, stops: &mut Vec<String>, points: &[[f32; 2]]) -> Result<(), Vec3> {
    for &to in points {
        let mut reached = false;
        for _ in 0..1200 {
            let at = run.hero().pivot;
            if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                reached = true;
                break;
            }
            push_toward(run, stops, to);
        }
        if !reached {
            return Err(run.hero().pivot);
        }
    }
    for _ in 0..20 {
        step(run, stops, [false; 4]);
    }
    Ok(())
}

/// The push blocks by the pods (openspec AS.74), each with where its push
/// is made from (on its far side from its track, across the ground) and the
/// end marker its track drops it onto: `Push_Block_Template(1)` from the
/// east terrace into the channel below at `Push_block_End_Template(2)`,
/// then `(2)` beside it (carrying `(3)`, stacked on it) at `(1)`, then
/// `(3)` from `(1)`'s top at `(3)`, the third span of the bridge.
const PUSHES: [(&str, [f32; 2], &str); 3] = [
    (
        "Push_Block_Template(1)",
        [66.9, 49.8],
        "Push_block_End_Template(2)",
    ),
    (
        "Push_Block_Template(2)",
        [63.2, 52.4],
        "Push_block_End_Template(1)",
    ),
    (
        "Push_Block_Template(3)",
        [61.3, 43.8],
        "Push_block_End_Template(3)",
    ),
];

/// What each push came to: the block, the step it lay inactive at its end
/// (`special - push block inactive`) and where.
#[derive(Debug)]
struct Pushed {
    block: &'static str,
    inactive: Option<u32>,
    at: Vec3,
}

/// openspec AS.74: from the east terrace above the pods (where Setup_002's
/// second trigger leaves him), the three push blocks pushed on the stick
/// into the channel along their tracks, across the bridge they make to the
/// west terrace and up its slope onto the rise, by the second bomb spawner;
/// each push's outcome, then where he stopped if a walk didn't get there.
fn up_the_push_blocks(run: &mut Run, stops: &mut Vec<String>) -> (Vec<Pushed>, Result<(), Vec3>) {
    let mut pushed = Vec::new();
    // Up onto the terrace, round its rocks, to the first block's far side.
    if let Err(at) = walk(
        run,
        stops,
        &[[72.0, 62.0], [68.0, 58.0], [68.0, 52.0], PUSHES[0].1],
    ) {
        return (pushed, Err(at));
    }
    for (k, (block, from, end)) in PUSHES.into_iter().enumerate() {
        let (h, _) = run.placed(block);
        if k > 0
            && let Err(at) = walk(run, stops, &[from])
        {
            return (pushed, Err(at));
        }
        let end_at = run.at(end);
        let mut inactive = None;
        for _ in 0..360 {
            push_toward(run, stops, [end_at.x, end_at.z]);
            if behaviour(run, h).as_deref() == Some("special - push block inactive") {
                inactive = Some(run.steps);
                break;
            }
        }
        for _ in 0..30 {
            step(run, stops, [false; 4]);
        }
        let at = run.world.actor(h).map_or(Vec3::ZERO, |a| a.position);
        pushed.push(Pushed {
            block,
            inactive,
            at,
        });
    }
    // Across the bridge to the west terrace, up its slope onto the rise.
    let walked = walk(
        run,
        stops,
        &[[56.0, 47.5], [50.5, 50.0], [51.0, 57.0], [56.0, 63.0]],
    );
    (pushed, walked)
}

/// openspec AS.74: level 214's rise past the pods reached on foot over its
/// push blocks. From the east terrace above the pods the hero pushes
/// `Push_Block_Template(1)` off its edge into the channel below, where it
/// drops onto its end marker and goes inactive, a stepping stone level
/// with the terrace; `(2)`, with `(3)` stacked on it, the same, so `(3)`
/// rides down onto it; and `(3)`, pushed from `(1)`'s top, off `(2)` onto
/// the last marker: the three make a bridge from the east terrace to the
/// west one, whose slope climbs onto the rise holding the second bomb
/// spawner. Every push and walk is the stick; every block moves by its own
/// script (`PushBlock_Template.ai`, its track and its drop to
/// `Push_Block_End.ai`). Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_push_blocks_bridge_the_way_up_to_the_rise() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    // Where Setup_002's second trigger leaves him (AS.58's walk).
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let (pushed, walked) = up_the_push_blocks(&mut run, &mut stops);
    let at = run.hero().pivot;
    println!(
        "{pushed:?}; walked {walked:?}, on the rise at {at} at step {}; {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
    assert_eq!(pushed.len(), 3, "{pushed:?}");
    for p in &pushed {
        let end = PUSHES.iter().find(|(b, ..)| *b == p.block).unwrap().2;
        let end_at = run.at(end);
        assert!(p.inactive.is_some(), "{} pushed in", p.block);
        assert!(
            Vec3::new(p.at.x - end_at.x, 0.0, p.at.z - end_at.z).length() < 0.6
                && (p.at.y - end_at.y).abs() < 0.2,
            "{} on {end}: {}",
            p.block,
            p.at
        );
    }
    assert_eq!(walked, Ok(()), "across the bridge and up");
    assert!(at.y > -12.6, "on the rise: {at}");
    assert!(
        at.distance(Vec3::from_array(SECOND_SPAWNER)) < 4.0,
        "by the second spawner: {at}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's second bomb spawner stands, on the rise past the
/// pods (metres, its file's place).
const SECOND_SPAWNER: [f32; 3] = [57.78, -12.1, 65.85];
/// On the rise at the ridge along its south edge, facing the castle wall
/// that shuts the two elves in (`Breakable_Stone_Wall_Castle_206(3)`) over
/// it, 9.9 m off: a bomb thrown there lands on it (AS.74).
const BEFORE_THE_ELVES_WALL: ([f32; 3], f32) = ([51.7, -11.8, 70.1], 318.7);
/// On the rise at the ridge, facing the castle wall
/// (`Breakable_Stone_Wall_Castle_206(5)`) that shuts the rise's south part
/// off from the east terrace over it, 7.9 m off.
const BEFORE_THE_RIDGE_WALL: ([f32; 3], f32) = ([58.3, -11.9, 70.6], 29.6);
/// From the rise's ridge down to the elves (AS.74): east off its edge onto
/// the terrace below, round through the opened `(5)` into the rise's south
/// part, west across it through the opened `(3)`.
const ROUND_TO_THE_ELVES: [[f32; 2]; 9] = [
    [63.0, 66.0],
    [68.0, 67.0],
    [68.0, 72.0],
    [64.5, 77.5],
    [60.0, 77.5],
    [55.0, 77.0],
    [50.0, 77.0],
    [46.0, 77.5],
    [42.0, 76.5],
];

/// The second spawner's bomb taken on the stick, then carried to `before`
/// on the stick, turned there to face its wall and thrown at it: the step
/// it was thrown at and the step `wall` went (AS.74).
fn bomb_over_the_ridge(
    run: &mut Run,
    stops: &mut Vec<String>,
    before: ([f32; 3], f32),
    wall: Handle,
) -> (Option<u32>, Option<u32>) {
    let mut bomb = None;
    for _ in 0..600 {
        push_toward(run, stops, [SECOND_SPAWNER[0], SECOND_SPAWNER[2]]);
        bomb = bombs(run)
            .into_iter()
            .find(|b| behaviour(run, *b).as_deref() == Some("following player"));
        if bomb.is_some() {
            break;
        }
    }
    let Some(bomb) = bomb else {
        return (None, None);
    };
    if walk(run, stops, &[[before.0[0], before.0[2]]]).is_err() {
        return (None, None);
    }
    // Turned where he stands, as a tap of the stick would.
    let at = run.hero().pivot;
    throw_at(run, stops, bomb, (at.to_array(), before.1), wall)
}

/// The placement named `name` in the level's file, by handle.
fn handle(run: &Run, name: &str) -> Handle {
    run.placed(name).0
}

/// Whether `h` is beaten: off the run or its actor gone.
fn gone(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
        || run.vm.placement_of(h).is_none_or(|id| run.vm.removed(id))
}

/// Up to `steps` steps of a fight with `foes`: every 23 steps the hero set
/// down 1.5 m short of the nearest standing, facing it, his first attack
/// pulsed two steps in four (a hand mashing South), until every one is
/// beaten. The step each was beaten at; a foe standing shapeless (a dead
/// state's) with a body in the bench fails it (AS.85).
fn fight(run: &mut Run, stops: &mut Vec<String>, foes: &[Handle], steps: u32) -> Vec<Option<u32>> {
    let mut beaten: Vec<Option<u32>> = vec![None; foes.len()];
    for k in 0..steps {
        // 23, off his attack's own cycle: set down every 20, the same lob
        // flies over a Chompy standing to nip him, on some of the session's
        // number streams for good (AS.115's review: 2 of 8 seeds, the four
        // gone by 1,376 on all 16 runs at 23).
        if k % 23 == 0 {
            let me = run.hero().pivot;
            let near = foes
                .iter()
                .filter(|h| !gone(run, **h))
                .filter_map(|h| run.world.actor(*h).map(|a| a.position))
                .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
            if let Some(at) = near {
                let to = Vec3::new(at.x - me.x, 0.0, at.z - me.z);
                let back = to.normalize_or_zero() * 1.5;
                let facing = guacamole::script_engine::heading_of(to);
                run.put(Vec3::new(at.x - back.x, at.y, at.z - back.z), facing);
            }
        }
        step(run, stops, [k % 4 < 2, false, false, false]);
        for (i, h) in foes.iter().enumerate() {
            if beaten[i].is_none() && gone(run, *h) {
                beaten[i] = Some(run.steps);
            }
            // A foe its dead states took the shape off has no body, as the
            // TV's (openspec AS.85): the scripts' touch test sees it so.
            if let Some(a) = run.world.actor(*h)
                && !a.removed
                && a.havok().is_none()
            {
                assert!(
                    !run.bench.bodies.contains_key(h),
                    "{} shapeless but a body",
                    run.world.name(*h)
                );
            }
        }
        if beaten.iter().all(Option::is_some) {
            break;
        }
    }
    beaten
}

/// openspec AS.58: level 214's second fight, on the first island past the
/// pods, on the TV's listing (`scripts::ENCOUNTERS`). The first spawner's
/// bomb, taken twice (AS.67), opens the wall before the pods and the one
/// past them. Past it `Setup_002_Controller` sees its first trigger, sets
/// its four undead Chompies going for a scene and waits for Eon's elemental
/// hint (`Explain_Elemental`) to take itself off: the hero walking into it,
/// its line plays and it goes. At its second trigger the Chompies run in and
/// fight; beaten, the second spawner's bomb opens the wall the two Elf
/// Nears spar behind (`Setup_002_Controller_Helper`), whose scene sets them
/// on him; both beaten, the helper takes itself off. Nothing stops. The
/// hero is set down where a walk would take him up to the Chompies' fight;
/// from there he walks on the stick (AS.74): up to the rise over the push
/// blocks ([`up_the_push_blocks`]), to the second spawner for a bomb to
/// throw over the ridge along the rise's south edge at the wall between its
/// south part and the terrace, and another at the elves' wall, then down
/// off the rise's east edge and round through both walls to the elves.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_second_fight_is_fought_on_the_first_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let controller = run.placement("Setup_002_Controller");
    let helper = handle(&run, "Setup_002_Controller_Helper");
    let hint = handle(&run, "Explain_Elemental");
    let chompies: Vec<Handle> = ["E", "F", "G", "H"]
        .iter()
        .map(|c| handle(&run, &format!("Enemy_Chompy2_{c}")))
        .collect();
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    // The two walls the first spawner's bomb opens (AS.57, AS.67).
    let (first_wall, second_wall) = (
        handle(&run, "Breakable_Stone_Wall_Castle_206(1)"),
        handle(&run, "Breakable_Stone_Wall_Castle_206(6)"),
    );
    for (before, wall) in [
        (BEFORE_THE_WALL, first_wall),
        (BEFORE_THE_SECOND_WALL, second_wall),
    ] {
        let (bomb, _) = take_the_bomb(&mut run, &mut stops, 300).expect("the bomb taken");
        let (_, opened) = throw_at(&mut run, &mut stops, bomb, before, wall);
        assert!(opened.is_some(), "the wall opened");
        for _ in 0..60 {
            step(&mut run, &mut stops, [false; 4]);
        }
    }
    let (health_before, xp_before) = (
        run.health().unwrap(),
        run.figure_values().map_or(0, |v| v.xp),
    );
    // Its first trigger: the Chompies set going, the wait for the hint.
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    let mut waits = None;
    for _ in 0..300 {
        step(&mut run, &mut stops, [false; 4]);
        if waits.is_none() && run.vm.behaviour(controller) == Some("wait for cutscene to finish") {
            waits = Some(run.steps);
        }
    }
    let waits = waits.expect("it waits on Eon's hint");
    // Into the hint: its line, then it goes, and the controller with it.
    run.put(Vec3::new(89.4, -16.5, 51.9), 0.0);
    let mut hint_gone = None;
    for _ in 0..900 {
        step(&mut run, &mut stops, [false; 4]);
        if hint_gone.is_none() && gone(&run, hint) {
            hint_gone = Some(run.steps);
        }
        if run.vm.behaviour(controller) == Some("wait for next trigger") {
            break;
        }
    }
    let hint_gone = hint_gone.expect("Eon's hint took itself off");
    assert_eq!(run.vm.behaviour(controller), Some("wait for next trigger"));
    // Its second trigger: the Chompies run in; fought.
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let chompies_beaten = fight(&mut run, &mut stops, &chompies, 2400);
    println!(
        "the wait from step {waits}, the hint gone at {hint_gone}; Chompies beaten at {chompies_beaten:?}"
    );
    assert!(
        chompies_beaten.iter().all(Option::is_some),
        "the four beaten: {chompies_beaten:?}"
    );
    // The elves spar behind their wall until a bomb opens it, invulnerable
    // meanwhile (AS.83). Up to the rise on foot over the push blocks
    // (AS.74), from where the Chompies' fight leaves him, then to the
    // second spawner: its bomb his as he comes within its reach.
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let (pushed, walked) = up_the_push_blocks(&mut run, &mut stops);
    assert!(
        pushed.len() == 3 && pushed.iter().all(|p| p.inactive.is_some()) && walked.is_ok(),
        "up the push blocks: {pushed:?}, {walked:?}"
    );
    // The rise's ridge shuts its south part off: the second spawner's bombs
    // thrown over it open the wall between that part and the terrace, then
    // the elves' wall; down off the rise's east edge, round through the one
    // and across through the other to the elves (AS.74).
    let ridge_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(5)");
    let (_, ridge_opened) =
        bomb_over_the_ridge(&mut run, &mut stops, BEFORE_THE_RIDGE_WALL, ridge_wall);
    let ridge_opened = ridge_opened.expect("the ridge's wall opened");
    for _ in 0..90 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (_, opened) = bomb_over_the_ridge(&mut run, &mut stops, BEFORE_THE_ELVES_WALL, elves_wall);
    let opened = opened.expect("the elves' wall opened");
    println!(
        "the ridge's wall opened at {ridge_opened}, the elves' at {opened}; health {:?}, the helper {:?}",
        run.health(),
        behaviour(&run, helper)
    );
    assert_eq!(
        walk(&mut run, &mut stops, &ROUND_TO_THE_ELVES),
        Ok(()),
        "round to the elves"
    );
    let elves_beaten = fight(&mut run, &mut stops, &elves, 3600);
    println!(
        "the elves beaten at {elves_beaten:?}; health {:?}, hero at {}, his script {:?}",
        run.health(),
        run.hero().pivot,
        run.minion
            .and_then(|m| run.vm.stopped(m))
            .map(|h| h.to_string())
    );
    for _ in 0..120 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (health, xp) = (
        run.health().unwrap(),
        run.figure_values().map_or(0, |v| v.xp),
    );
    println!(
        "the elves' wall opened at {opened}, the elves beaten at {elves_beaten:?}, the helper gone {}; health {health_before} -> {health}, experience {xp_before} -> {xp}; {:.1} s; load at end: {}",
        gone(&run, helper),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        elves_beaten.iter().all(Option::is_some),
        "both elves beaten: {elves_beaten:?}"
    );
    assert!(gone(&run, helper), "the helper's done");
    assert!(xp > xp_before, "his figure's paid: {xp}");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// A creature's hit points as its scripts hold them
/// (`description.creature info.hp`), once its STARTUP has made them.
fn hit_points(run: &Run, h: Handle) -> Option<f32> {
    let vm = &run.vm;
    let id = vm.placement_of(h)?;
    let d = vm.local(id, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_f32()
}

/// openspec AS.83: level 214's two Elf Nears can't be hurt while they spar.
/// At Setup_002's first trigger their helper gives them a range that wakes
/// them and, a tick on, asks combat for each to make them invulnerable and
/// immune to every state. They wake, and so register with combat, the step
/// of the write (the asleep placements' checks after the walk, `guac_script`'s
/// crate docs, The tick), so the helper's writes land: set down among them
/// behind their shut wall, the hero mashing his first attack hurts neither,
/// where on the step-late wake he beat both. Once the second spawner's bomb
/// opens their wall their scene clears it, and both are beaten. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_elves_cant_be_hurt_while_they_spar() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let helper = handle(&run, "Setup_002_Controller_Helper");
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    // Its first trigger: the helper wakes them and makes them invulnerable.
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let full: Vec<Option<f32>> = elves.iter().map(|h| hit_points(&run, *h)).collect();
    assert!(full.iter().all(Option::is_some), "both awake: {full:?}");
    // Among them behind their shut wall, mashing (the review beat both by
    // step 1477 on the step-late wake).
    let mashed = fight(&mut run, &mut stops, &elves, 1500);
    let after: Vec<Option<f32>> = elves.iter().map(|h| hit_points(&run, *h)).collect();
    let sparring = behaviour(&run, helper);
    println!(
        "hit points {full:?} -> {after:?} after 1500 steps of mashing, beaten {mashed:?}, the helper {sparring:?}, health {:?}",
        run.health()
    );
    assert_eq!(mashed, [None, None], "neither beaten while they spar");
    assert_eq!(after, full, "neither hurt");
    assert!(!gone(&run, elves_wall), "their wall still shut");
    // Their wall opened by the second spawner's bomb: their scene, then the
    // fight.
    run.put(Vec3::from_array(SECOND_SPAWNER) - Vec3::X * 3.0, 90.0);
    let mut bomb = None;
    for _ in 0..300 {
        step(&mut run, &mut stops, [false; 4]);
        bomb = bombs(&run)
            .into_iter()
            .find(|b| behaviour(&run, *b).as_deref() == Some("following player"));
        if bomb.is_some() {
            break;
        }
    }
    let bomb = bomb.expect("the second spawner's bomb taken");
    let (_, opened) = throw_at(
        &mut run,
        &mut stops,
        bomb,
        BEFORE_THE_ELVES_WALL,
        elves_wall,
    );
    let opened = opened.expect("the elves' wall opened");
    let beaten = fight(&mut run, &mut stops, &elves, 3600);
    println!(
        "the wall opened at {opened}, the elves beaten at {beaten:?}; health {:?}; {:.1} s; load at end: {}",
        run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        beaten.iter().all(Option::is_some),
        "both beaten: {beaten:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's first ship part floats, on the rise the jump pad
/// reaches (metres, its file's place).
const SHIP_PART_1: [f32; 3] = [29.45, -6.92, 56.37];
/// Where the sail's cutscene sets the hero down on the second island's
/// ship (metres, the run's own read: its second scene's player waypoint).
const ON_THE_SECOND_ISLAND: [f32; 3] = [-383.178, -12.581, 714.414];

/// Whether `h`'s actor is off the world (a placement whose script the
/// listing leaves out has no run to be off).
fn off_the_world(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
}

/// What the sail's STARTUP takes off and its SHUTDOWN resets (its
/// `deactivated actors`, openspec AS.77): the second island's ship, an NPC
/// of the island's set and a door of the ship's.
const OFF_FOR_THE_SAIL: [&str; 3] = ["The_Ship_2", "Flynn_Island_2", "Door_(1)"];

/// The sail's lines, in the order its scenes voice them.
const SAIL_LINES: [&str; 7] = [
    "vo_flynn_lvl214_igc03_007",
    "vo_flynn_boom_01",
    "vo_cali_lvl214_igc03_002",
    "vo_flynn_lvl214_igc03_008",
    // Its ninth scene's, an animator camera's shot read since AS.78.
    "vo_flynn_lvl214_igc03_009",
    "vo_cali_lvl214_igc03_010",
    "vo_flynn_lvl214_igc03_011",
];

/// The sail's run from the first ship part: the part taken after a press
/// (South, pulsed as a hand would: with none it never goes), then every
/// step `actions` until the sail goes or `steps` pass. Its scenes (index,
/// first step), the step the part went and the sail, the objects' actors
/// named and the objects reading none (step, place), its lines and what
/// its STARTUP took off seen off.
struct Sail {
    scenes: Vec<(usize, u32)>,
    ended: Option<u32>,
    taken: Option<u32>,
    sailed: Option<u32>,
    actors: std::collections::BTreeSet<String>,
    unnamed: Vec<(u32, usize)>,
    lines: Lines,
    taken_off: Vec<bool>,
    /// The darkest an interior's darkness got while it ran (AS.96).
    dark: f32,
}

fn sail(
    run: &mut Run,
    stops: &mut Vec<String>,
    steps: u32,
    mut actions: impl FnMut(&Run, u32) -> [bool; 4],
) -> Sail {
    let part = handle(run, "Ship_part_1(1)");
    let sail = handle(run, "214_Recover_First_Part_IGC");
    let off: Vec<Handle> = OFF_FOR_THE_SAIL.iter().map(|n| handle(run, n)).collect();
    run.put(Vec3::from_array(SHIP_PART_1), 0.0);
    let mut out = Sail {
        scenes: Vec::new(),
        ended: None,
        taken: None,
        sailed: None,
        actors: Default::default(),
        unnamed: Vec::new(),
        lines: Lines::default(),
        taken_off: vec![false; off.len()],
        dark: 0.0,
    };
    for k in 0..steps {
        let pressed = actions(run, k);
        step(run, stops, pressed);
        out.lines.record(run);
        out.dark = out
            .dark
            .max(guacamole::interiors::darkness(&run.vm, &run.world));
        if out.taken.is_none() && gone(run, part) {
            out.taken = Some(run.steps);
        }
        for (seen, h) in out.taken_off.iter_mut().zip(&off) {
            *seen |= off_the_world(run, *h);
        }
        match scene_of(run) {
            Some((k, n)) if cutscene_name(run).as_deref() == Some("214 recover 1st part IGC") => {
                assert_eq!(n, 11, "the sail's eleven scenes");
                if out.scenes.last().is_none_or(|(at, _)| *at != k) {
                    out.scenes.push((k, run.steps));
                }
                for (i, a) in scene_actors(run).into_iter().enumerate() {
                    match a {
                        Some(a) => {
                            out.actors.insert(a);
                        }
                        None => out.unnamed.push((run.steps, i)),
                    }
                }
            }
            _ if !out.scenes.is_empty() && out.ended.is_none() => out.ended = Some(run.steps),
            _ => {}
        }
        if out.sailed.is_none() && gone(run, sail) {
            out.sailed = Some(run.steps);
            break;
        }
    }
    for _ in 0..120 {
        step(run, stops, [false; 4]);
    }
    out
}

/// The pads at step `k` of a walk past the first ship part that skips the
/// sail as a player does: South pulsed for the part's press until a
/// cutscene plays, then △ pulsed (the original's skip, past the cutscene's
/// first second), so a test past the sail needn't play its eleven scenes.
fn skipping_the_sail(run: &Run, k: u32) -> [bool; 4] {
    match cutscene_name(run) {
        Some(_) => [false, false, false, k % 20 == 10],
        None => [k % 20 < 2, false, false, false],
    }
}

/// openspec AS.59, AS.82: level 214's way from its first island to its
/// second, on the TV's listing (`scripts::PASSAGES`). The hero at the first
/// ship part takes it; it waits on a press, then goes and its shutdown
/// wakes the sail, whose cutscene now plays to its end
/// (`level_end::SKIPPED_CUTSCENES` lists it no more): its eleven scenes in
/// order, every scene object reading its actor (Flynn's and Cali's from
/// their definitions, `scripts::with_cutscene_objects`: unlisted, the
/// second scene's walk held on an object reading none), its seven lines
/// voiced in order and over by its end, its four animator camera shots each
/// its clip's length (AS.78); its second scene's player waypoint
/// sets him down on the second island's ship, and the sail takes itself
/// off, so Setup_003, which waits for it, can start. AS.77: what the sail
/// took off stands again once its SHUTDOWN resets the set holding it, the
/// ship he stands on among it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_first_ship_part_sails_to_the_second_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    assert!(!guacamole::level_end::SKIPPED_CUTSCENES.contains(&"214 recover 1st part IGC"));
    let (mut run, mut stops) = joined_214();
    let off: Vec<Handle> = OFF_FOR_THE_SAIL.iter().map(|n| handle(&run, n)).collect();
    let s = sail(&mut run, &mut stops, 6000, |_, k| {
        [k % 20 < 2, false, false, false]
    });
    let at = run.hero().pivot;
    let standing: Vec<bool> = off.iter().map(|h| !off_the_world(&run, *h)).collect();
    let ended = s.ended.expect("the sail's cutscene ends");
    for (k, (_, from)) in s.scenes.iter().enumerate() {
        let to = s.scenes.get(k + 1).map_or(ended, |(_, n)| *n);
        println!(
            "scene {k}: from {from}, {} steps ({:.2} s)",
            to - from,
            f64::from(to - from) / 60.0
        );
    }
    println!(
        "the part taken at {:?}, the sail done at {:?}; the hero at {at}, the HUD hidden {}; actors {:?}; voices {:?}; {OFF_FOR_THE_SAIL:?} taken off {:?}, standing after {standing:?}; {:.1} s; load at end: {}",
        s.taken,
        s.sailed,
        guacamole::scripts::player_hud_hidden(&run.vm),
        s.actors,
        s.lines
            .voices
            .iter()
            .map(|v| (v.sound.as_str(), v.started, v.ended, v.length))
            .collect::<Vec<_>>(),
        s.taken_off,
        started.elapsed().as_secs_f64(),
        load()
    );
    assert_eq!(s.taken_off, [true; 3], "taken off for the sail");
    assert_eq!(standing, [true; 3], "back once the sail's done");
    assert!(s.taken.is_some(), "the part taken");
    assert!(s.sailed.is_some(), "the sail done");
    assert_eq!(
        s.scenes.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        (0..11).collect::<Vec<_>>(),
        "every scene, in order, none skipped"
    );
    // AS.78: the fifth to seventh and the ninth scenes are animator camera
    // shots (`214_Recovered.mdl`'s four: 3.13, 2.93, 2.17 and 4.67 s), each
    // lasting its clip; until AS.78 a step each.
    let length = |k: usize| s.scenes.get(k + 1).map_or(ended, |(_, n)| *n) - s.scenes[k].1;
    for (k, clip) in [
        (4, 3.133_333_3),
        (5, 2.933_333_3),
        (6, 2.166_666_7),
        (8, 4.666_666_5),
    ] {
        let want = clip * 60.0;
        assert!(
            (f64::from(length(k)) - want).abs() <= 2.0,
            "scene {k}: {} steps, its camera's clip {want:.0}",
            length(k)
        );
    }
    assert!(
        s.unnamed.is_empty(),
        "objects reading no actor: {:?}",
        s.unnamed
    );
    assert_eq!(s.dark, 0.0, "an interior's darkness under the sail");
    for who in ["Flynn_NPC", "Cali_NPC", "214_RFP_MoneyBags", "009_Eruptor"] {
        assert!(
            s.actors.contains(who),
            "{who} in its scenes: {:?}",
            s.actors
        );
    }
    assert_eq!(
        s.lines
            .voices
            .iter()
            .map(|v| v.sound.as_str())
            .collect::<Vec<_>>(),
        SAIL_LINES
    );
    for v in &s.lines.voices {
        assert!(
            v.ended.is_some_and(|e| e <= u64::from(ended)),
            "{} on past the cutscene's end at {ended}: {:?}",
            v.sound,
            v.ended
        );
    }
    assert!(
        at.distance(Vec3::from_array(ON_THE_SECOND_ISLAND)) < 2.0,
        "on the second island: {at}"
    );
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.82, AS.81: the original's own skip still ends the sail once
/// it plays: △ pressed in its second scene ends it there (the cutscene
/// system's check), sets him down on the second island's ship where
/// playing it does, the HUD back within a few steps, and puts back what it
/// took off; no line plays on past it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_sail_skips_on_triangle() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let off: Vec<Handle> = OFF_FOR_THE_SAIL.iter().map(|n| handle(&run, n)).collect();
    let mut pressed = None;
    let s = sail(&mut run, &mut stops, 6000, |run, k| {
        let second = scene_of(run).is_some_and(|(k, _)| k >= 1)
            && cutscene_name(run).as_deref() == Some("214 recover 1st part IGC");
        let triangle = second && pressed.is_none();
        if triangle {
            pressed = Some(run.steps);
        }
        [
            k % 20 < 2 && scene_of(run).is_none(),
            false,
            false,
            triangle,
        ]
    });
    let at = run.hero().pivot;
    let standing: Vec<bool> = off.iter().map(|h| !off_the_world(&run, *h)).collect();
    println!(
        "△ at {pressed:?}; scenes {:?}; the cutscene over at {:?}, the sail done at {:?}; the hero at {at}; voices {:?}; standing after {standing:?}; {:.1} s; load at end: {}",
        s.scenes,
        s.ended,
        s.sailed,
        s.lines
            .voices
            .iter()
            .map(|v| (v.sound.as_str(), v.started, v.ended))
            .collect::<Vec<_>>(),
        started.elapsed().as_secs_f64(),
        load()
    );
    let pressed = pressed.expect("△ pressed in its second scene");
    let skip = s.ended.expect("the cutscene over");
    assert!(skip <= pressed + 3, "skipped at {skip}, △ at {pressed}");
    assert!(s.sailed.is_some_and(|d| d <= skip + 2), "{:?}", s.sailed);
    assert_eq!(s.scenes.last().map(|(k, _)| *k), Some(1), "{:?}", s.scenes);
    assert!(
        s.lines
            .voices
            .iter()
            .all(|v| v.ended.is_some_and(|e| e <= u64::from(skip))),
        "{:?}",
        s.lines.voices
    );
    assert_eq!(standing, [true; 3], "back once the sail's done");
    assert!(
        at.distance(Vec3::from_array(ON_THE_SECOND_ISLAND)) < 2.0,
        "on the second island: {at}"
    );
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// The first island's jump pad (its file's place) and where the walk onto
/// it starts, 4 m south of it in the elves' yard.
const JUMP_PAD: [f32; 3] = [25.149, -10.869, 71.414];
const ONTO_THE_PAD: [f32; 2] = [25.15, 75.4];

/// openspec AS.75: level 214's way off its first island on foot. Both elves
/// beaten, `Jumpad_Activator_First_Island` pans to `PvP_JumpPad_NORMAL(2)`
/// and gives it a range. Stepped onto with the stick let go, the pad
/// launches him at its own pitch and speed (80° and 90 ft/s, its
/// placement's `pitch` and `velocity`) along his facing, and the mover
/// update (`movementUpdate.ai`, on the VM) takes the launch's ground speed
/// off at his air deceleration while no stick pushes, as it does any
/// mover's: he goes up 9 m and lands back on it. Walked onto with the stick
/// held toward `Ship_part_1(1)`, the same launch, the ground speed his
/// stick's, carries him up onto the rise holding the part, a ledge no walk
/// climbs; at the part, a press (South) takes it, and the sail takes him to
/// the second island's ship, which stands (AS.77). The elves are fought as
/// AS.58's are, set down by them; from there every step is the stick.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_jump_pad_carries_him_to_the_first_ship_part() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    let activator = handle(&run, "Jumpad_Activator_First_Island");
    let part = handle(&run, "Ship_part_1(1)");
    let sail = handle(&run, "214_Recover_First_Part_IGC");
    let ship = handle(&run, "The_Ship_2");
    // Setup_002's first trigger wakes the elves; the second spawner's bomb
    // opens their wall; both beaten (AS.58, AS.83).
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
    run.put(Vec3::from_array(SECOND_SPAWNER) - Vec3::X * 3.0, 90.0);
    let mut bomb = None;
    for _ in 0..300 {
        step(&mut run, &mut stops, [false; 4]);
        bomb = bombs(&run)
            .into_iter()
            .find(|b| behaviour(&run, *b).as_deref() == Some("following player"));
        if bomb.is_some() {
            break;
        }
    }
    let (_, opened) = throw_at(
        &mut run,
        &mut stops,
        bomb.expect("the second spawner's bomb taken"),
        BEFORE_THE_ELVES_WALL,
        elves_wall,
    );
    assert!(opened.is_some(), "the elves' wall opened");
    let beaten = fight(&mut run, &mut stops, &elves, 3600);
    assert!(
        beaten.iter().all(Option::is_some),
        "both beaten: {beaten:?}"
    );
    // The activator's pan, then the pad is given its range.
    let mut panned = None;
    for _ in 0..900 {
        step(&mut run, &mut stops, [false; 4]);
        if gone(&run, activator) {
            panned = Some(run.steps);
            break;
        }
    }
    let panned = panned.expect("the activator's pan done");
    // In the elves' yard (the fight's set-downs can leave him wedged at its
    // edge), then south of the pad on the stick.
    run.put(Vec3::new(24.0, -10.8, 79.0), 0.0);
    assert_eq!(walk(&mut run, &mut stops, &[ONTO_THE_PAD]), Ok(()));
    // Onto the pad, the stick let go once he's on it: up and back down.
    let pad = Vec3::from_array(JUMP_PAD);
    let on_pad = |run: &Run| {
        let at = run.hero().pivot;
        Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length() < 1.0
    };
    for _ in 0..240 {
        if on_pad(&run) {
            break;
        }
        push_toward(&mut run, &mut stops, [pad.x, pad.z]);
    }
    let (mut top, mut left_at, mut bounces, mut was) = (f32::MIN, None, 0, 0.0);
    let mut drift = 0.0f32;
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        let at = run.hero().pivot;
        top = top.max(at.y);
        if left_at.is_none() && at.y > pad.y + 0.2 {
            left_at = Some(run.steps);
        }
        let vy = run.hero().walk.velocity.y;
        bounces += u32::from(was < 0.0 && vy > 0.0);
        was = vy;
        drift = drift.max(Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length());
    }
    println!(
        "let go on the pad: launched at {left_at:?}, {:.2} m up, {bounces} bounces more in 240 steps, at most {drift:.2} m off it",
        top - pad.y
    );
    assert!(left_at.is_some(), "launched");
    assert!(bounces >= 2, "bouncing on it: {bounces}");
    assert!(drift < 1.5, "straight up and down over it: {drift}");
    assert!(
        (8.0..10.0).contains(&(top - pad.y)),
        "9 m up: {}",
        top - pad.y
    );
    // Off it: the stick toward the yard as he bounces again, then at rest.
    for _ in 0..600 {
        push_toward(&mut run, &mut stops, ONTO_THE_PAD);
        let at = run.hero().pivot;
        if Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length() > 2.5
            && run.hero().walk.velocity.y == 0.0
            && at.y < pad.y + 0.3
        {
            break;
        }
    }
    assert_eq!(walk(&mut run, &mut stops, &[ONTO_THE_PAD]), Ok(()));
    // Onto it again with the stick held toward the part.
    let target = [SHIP_PART_1[0], SHIP_PART_1[2]];
    let (mut launched, mut landed, mut taken, mut sailed) = (None, None, None, None);
    let mut flight = Vec::new();
    for k in 0..1800u32 {
        let at = run.hero().pivot;
        // South pulsed as a hand would once he's by the part (with none it
        // never goes, AS.59).
        let by_part = Vec3::new(at.x - target[0], 0.0, at.z - target[1]).length() < 6.0;
        if by_part && taken.is_none() && landed.is_some() {
            step(&mut run, &mut stops, [k % 20 < 2, false, false, false]);
        } else if sailed.is_none() && taken.is_none() {
            push_toward(&mut run, &mut stops, target);
        } else {
            let pads = skipping_the_sail(&run, k);
            step(&mut run, &mut stops, pads);
        }
        let at = run.hero().pivot;
        if launched.is_none() && at.y > pad.y + 0.2 {
            launched = Some((run.steps, run.hero().walk.velocity));
        }
        if launched.is_some() && landed.is_none() {
            flight.push(at);
            if run.hero().walk.velocity.y.abs() < 1e-3 && at.y > SHIP_PART_1[1] - 0.5 {
                landed = Some((run.steps, at));
            }
        }
        if taken.is_none() && gone(&run, part) {
            taken = Some(run.steps);
        }
        if sailed.is_none() && gone(&run, sail) {
            sailed = Some(run.steps);
        }
        if sailed.is_some() && run.steps > sailed.unwrap() + 120 {
            break;
        }
    }
    let apex = flight.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let at = run.hero().pivot;
    println!(
        "on the stick: launched at {launched:?}, apex {apex:.2} (pad {:.2}), landed at {landed:?}, the part taken at {taken:?}, the sail done at {sailed:?}; the hero at {at}, The_Ship_2 standing {}; the pan done at {panned}; {:.1} s; load at end: {}",
        pad.y,
        !off_the_world(&run, ship),
        started.elapsed().as_secs_f64(),
        load()
    );
    let (_, down) = landed.expect("landed on the part's rise");
    assert!(
        down.y > SHIP_PART_1[1] - 0.5 && down.z < pad.z - 3.0,
        "up on the part's rise, toward it: {down}"
    );
    assert!(taken.is_some(), "the part taken");
    assert!(sailed.is_some(), "the sail done");
    assert!(
        at.distance(Vec3::from_array(ON_THE_SECOND_ISLAND)) < 2.0,
        "on the second island's ship: {at}"
    );
    assert!(!off_the_world(&run, ship), "The_Ship_2 standing under him");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where Setup_003's trigger lies on the second island, off the ship's
/// gangplank (metres, its file's place).
const SETUP_003_TRIGGER: [f32; 2] = [-357.64, 723.11];

/// A Tree Rex's homing charge (openspec AS.68): the step it was made and
/// where, how far it went and its top speed across the ground (m/s, a
/// step's move), the step it struck or ran out and the step it went, the
/// hero's health either side of its strike and how far off him across the
/// ground it ended (m).
#[derive(Debug, Default)]
struct Charge {
    handle: Handle,
    made: u32,
    from: Vec3,
    at: Vec3,
    flew: f32,
    top: f32,
    ended: Option<u32>,
    gone: Option<u32>,
    health: Option<(f32, f32)>,
    off_him: Option<f32>,
}

/// Each homing charge standing or gone, followed a step: made, moved,
/// ended (its script's `tree growing out of ground`, its strike or its
/// lifespan's end), taken off.
fn watch_charges(run: &Run, charges: &mut Vec<Charge>, health: (f32, f32)) {
    for h in run.world.clones() {
        if run.world.name(h) != "TreeRex_special_homing_actor" {
            continue;
        }
        let Some(a) = run.world.actor(h) else {
            continue;
        };
        let c = match charges.iter().position(|c| c.handle == h) {
            Some(i) => &mut charges[i],
            None => {
                charges.push(Charge {
                    handle: h,
                    made: run.steps,
                    from: a.position,
                    at: a.position,
                    ..Charge::default()
                });
                charges.last_mut().unwrap()
            }
        };
        if a.removed {
            c.gone.get_or_insert(run.steps);
            continue;
        }
        let across = Vec3::new(a.position.x - c.at.x, 0.0, a.position.z - c.at.z).length();
        c.flew += across;
        c.top = c.top.max(across * 60.0);
        c.at = a.position;
        if c.ended.is_none() && behaviour(run, h).as_deref() == Some("tree growing out of ground") {
            c.ended = Some(run.steps);
            c.health = Some(health);
            let me = run.hero().pivot;
            c.off_him = Some((a.position.x - me.x).hypot(a.position.z - me.z));
        }
    }
}

/// openspec AS.75: level 214's second island's first fight, Setup_003, on
/// the TV's listing (`scripts::ENCOUNTERS`). Set down at the first ship part,
/// a press takes it and the sail sets him on the second island's ship; off
/// it on the stick to Setup_003's trigger, its controller wakes its Tree Rex
/// and four undead Chompies and goes; the Tree Rex eats a Chompy, and the
/// rest and the Tree Rex are beaten. AS.68: the Tree Rex's ranged attack
/// makes its homing charge, which its script steers at him (25 ft/s,
/// 50°/s) for 2.5 s at most and which takes itself off once it has struck
/// or run out: it strikes him as he goes at the Tree Rex on the stick from
/// the trigger, where he stood a while, mashing his first attack once by
/// it; then he fights the rest. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_third_fight_is_fought_on_the_second_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let controller = handle(&run, "Setup_003_Controller");
    let foes: Vec<Handle> = [
        "Enemy_TreeRex(2)",
        "Enemy_Chompy2_E(1)",
        "Enemy_Chompy2_E(2)",
        "Enemy_Chompy2_E(3)",
        "Enemy_Chompy2_E(4)",
    ]
    .iter()
    .map(|n| handle(&run, n))
    .collect();
    let rex = foes[0];
    let sail = handle(&run, "214_Recover_First_Part_IGC");
    run.put(Vec3::from_array(SHIP_PART_1), 0.0);
    for k in 0..600u32 {
        let pads = skipping_the_sail(&run, k);
        step(&mut run, &mut stops, pads);
        if gone(&run, sail) {
            break;
        }
    }
    assert!(gone(&run, sail), "the sail done");
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let on_ship = run.hero().pivot;
    // Off the ship on the stick to the trigger: the controller wakes the
    // fight and goes.
    assert_eq!(walk(&mut run, &mut stops, &[SETUP_003_TRIGGER]), Ok(()));
    let mut charges: Vec<Charge> = Vec::new();
    let mut health = run.health().unwrap();
    let mut woke = None;
    for _ in 0..120 {
        step(&mut run, &mut stops, [false; 4]);
        if woke.is_none() && gone(&run, controller) {
            woke = Some(run.steps);
        }
    }
    let woke = woke.expect("the controller woke its fight and went");
    // Before his first attack, the Tree Rex has eaten one of the four.
    let eaten: Vec<String> = foes[1..]
        .iter()
        .filter(|h| gone(&run, **h))
        .map(|h| run.world.name(*h))
        .collect();
    assert_eq!(eaten.len(), 1, "the Tree Rex eats one: {eaten:?}");
    let health_before = run.health().unwrap();
    // Where the trigger leaves him, a while: the Tree Rex comes on and
    // throws its charge from afar, which meets him as he goes at it.
    for _ in 0..40 {
        step(&mut run, &mut stops, [false; 4]);
        let now = run.health().unwrap_or(0.0);
        watch_charges(&run, &mut charges, (health, now));
        health = now;
    }
    // At the Tree Rex on the stick, South mashed once by it.
    let mut on_the_stick: Vec<Option<u32>> = vec![None; foes.len()];
    for k in 0..2400u32 {
        for (b, h) in on_the_stick.iter_mut().zip(&foes) {
            if b.is_none() && gone(&run, *h) {
                *b = Some(run.steps);
            }
        }
        let Some(at) = run
            .world
            .actor(rex)
            .filter(|_| !gone(&run, rex))
            .map(|a| a.position)
        else {
            break;
        };
        let me = run.hero().pivot;
        if (at.x - me.x).hypot(at.z - me.z) > 4.5 {
            push_toward(&mut run, &mut stops, [at.x, at.z]);
        } else {
            step(&mut run, &mut stops, [k % 4 < 2, false, false, false]);
        }
        let now = run.health().unwrap_or(0.0);
        watch_charges(&run, &mut charges, (health, now));
        health = now;
    }
    let rest = fight(&mut run, &mut stops, &foes, 3600);
    let beaten: Vec<Option<u32>> = on_the_stick
        .iter()
        .zip(&rest)
        .map(|(a, b)| a.or(*b))
        .collect();
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        let now = run.health().unwrap_or(0.0);
        watch_charges(&run, &mut charges, (health, now));
        health = now;
    }
    let inert = run.vm.inert();
    for c in &charges {
        println!(
            "  homing charge made at {} at {}, flew {:.2} m (top {:.2} m/s), ended at {:?} ({} s in), gone at {:?}, his health across its end {:?}, ran {}",
            c.made,
            c.from,
            c.flew,
            c.top,
            c.ended,
            c.ended.map_or(0.0, |e| (e - c.made) as f32 / 60.0),
            c.gone,
            c.health,
            !inert.contains(&c.handle)
        );
    }
    println!(
        "off the ship at {on_ship}, the controller gone at {woke}, {eaten:?} eaten; beaten at {beaten:?} (the Tree Rex, then the four); {} homing charges; health {health_before} -> {:?}; {:.1} s; load at end: {}",
        charges.len(),
        run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(beaten.iter().all(Option::is_some), "all beaten: {beaten:?}");
    assert!(!charges.is_empty(), "its ranged attack made its charge");
    for c in &charges {
        assert!(!inert.contains(&c.handle), "it runs: {c:?}");
        assert!(c.top <= 25.0 * 0.3048 + 0.01, "at its speed: {c:?}");
        assert!(
            c.ended.is_some_and(|e| e - c.made <= 151) && c.gone.is_some_and(|g| g - c.made <= 180),
            "struck or ran out, then gone: {c:?}"
        );
    }
    // Struck him, not a Chompy: it ended before its 2.5 s, within its 4 ft
    // of him across the ground, with 0.3 m for the step it's seen in (the
    // two closing at 12-13 m/s).
    assert!(
        charges.iter().any(|c| c.flew > 5.0
            && c.ended.is_some_and(|e| e - c.made < 150)
            && c.off_him.is_some_and(|d| d < 4.0 * 0.3048 + 0.3)),
        "one flew at him and struck: {charges:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// A fight on the stick ([`on_the_stick_at`]): the step each foe was
/// beaten at, every drop in his health (a bite) with its step, and where
/// the watched actor last stood before it went, if it did.
struct OnTheStick {
    beaten: Vec<Option<u32>>,
    bites: Vec<(u32, f32)>,
    last: Option<Vec3>,
}

/// Up to `steps` steps on the stick at the nearest of `foes` standing (the
/// first of them while it stands), his first attack mashed once within
/// 4.5 m of it, stepping in a second in every four, as a hand would, until
/// every one is beaten, `watch` followed until it goes.
fn on_the_stick_at(
    run: &mut Run,
    stops: &mut Vec<String>,
    foes: &[Handle],
    steps: u32,
    watch: Option<Handle>,
) -> OnTheStick {
    let mut beaten: Vec<Option<u32>> = vec![None; foes.len()];
    let mut bites = Vec::new();
    let mut health = run.health().unwrap_or(0.0);
    let mut last = None;
    for k in 0..steps {
        if let Some(w) = watch
            && !gone(run, w)
        {
            last = run.world.actor(w).map(|a| a.position);
        }
        for (b, h) in beaten.iter_mut().zip(foes) {
            if b.is_none() && gone(run, *h) {
                *b = Some(run.steps);
            }
        }
        if beaten.iter().all(Option::is_some) {
            break;
        }
        let me = run.hero().pivot;
        let first = foes.iter().copied().find(|h| !gone(run, *h));
        let near = first
            .filter(|_| first == Some(foes[0]))
            .into_iter()
            .chain(foes.iter().copied().filter(|h| !gone(run, *h)))
            .filter_map(|h| run.world.actor(h).map(|a| a.position))
            .next()
            .or_else(|| {
                foes.iter()
                    .filter(|h| !gone(run, **h))
                    .filter_map(|h| run.world.actor(*h).map(|a| a.position))
                    .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)))
            });
        match near {
            // Past 4.5 m he walks at it, and a while in every 4 s he steps
            // in (a Mabu pinned between him and it stops the lobs: a hand
            // wouldn't mash into him for ever).
            Some(at) if (at.x - me.x).hypot(at.z - me.z) > 4.5 || k % 240 >= 180 => {
                push_toward(run, stops, [at.x, at.z]);
            }
            _ => step(run, stops, [k % 4 < 2, false, false, false]),
        }
        let now = run.health().unwrap_or(0.0);
        if now < health {
            bites.push((run.steps, health - now));
        }
        health = now;
    }
    OnTheStick {
        beaten,
        bites,
        last,
    }
}

/// Where the key below the gangplank's yard lies (its spawner's place),
/// the lobe it's in entered by its trigger, and the way back up to the
/// locked door, where a press of △ turns the key (metres, across the
/// ground).
const KEY_1: [[f32; 2]; 2] = [[-325.76, 744.22], [-329.62, 745.18]];
const TO_THE_DOOR: [[f32; 2]; 3] = [[-331.0, 740.0], [-331.0, 728.0], [-324.5, 718.5]];
/// Past the door, Setup_004's second and third triggers.
const TO_SETUP_004: [[f32; 2]; 4] = [
    [-318.0, 718.0],
    [-300.0, 719.0],
    [-285.49, 721.46],
    [-270.91, 727.46],
];

/// What Setup_004's fight came to ([`fourth_fight`]): Setup_003's beaten,
/// the locked door opened, Setup_004's five beaten (the elves A-C, the Tree
/// Rex, the spawner) and their bites, the Mabu's last place standing and
/// where he started, health before and after and the experience.
struct FourthFight {
    beaten_003: Vec<Option<u32>>,
    opened: u32,
    at_the_door: u32,
    beaten: Vec<Option<u32>>,
    bites: Vec<(u32, f32)>,
    mabu: Handle,
    mabu_from: Option<Vec3>,
    mabu_last: Option<Vec3>,
    controller: Handle,
    health_before: f32,
    health: f32,
    xp: u32,
}

/// Level 214's fourth fight on foot from the sail (AS.84's walk): off the
/// ship to Setup_003's trigger, Setup_003 fought on the stick, the key in
/// the lobe, △ by the locked door, past it to Setup_004's triggers and its
/// five fought on the stick, then 240 steps at rest.
fn fourth_fight(run: &mut Run, stops: &mut Vec<String>) -> FourthFight {
    let sail = handle(run, "214_Recover_First_Part_IGC");
    let setup_003: Vec<Handle> = [
        "Enemy_TreeRex(2)",
        "Enemy_Chompy2_E(1)",
        "Enemy_Chompy2_E(2)",
        "Enemy_Chompy2_E(3)",
        "Enemy_Chompy2_E(4)",
    ]
    .iter()
    .map(|n| handle(run, n))
    .collect();
    let controller = handle(run, "Setup_004_Controller");
    let mabu = handle(run, "Setup_004_NPC_Mabu");
    let door = handle(run, "Door_Template_Skylanders_2(1)");
    let foes: Vec<Handle> = [
        "Enemy_Elf_Near_A",
        "Enemy_Elf_Near_B",
        "Enemy_Elf_Near_C",
        "Enemy_TreeRex(1)",
        "Enemy_Chompy_Spawner(2)",
    ]
    .iter()
    .map(|n| handle(run, n))
    .collect();
    run.put(Vec3::from_array(SHIP_PART_1), 0.0);
    for k in 0..600u32 {
        let pads = skipping_the_sail(run, k);
        step(run, stops, pads);
        if gone(run, sail) {
            break;
        }
    }
    assert!(gone(run, sail), "the sail done");
    assert_eq!(walk(run, stops, &[SETUP_003_TRIGGER]), Ok(()));
    let beaten_003 = on_the_stick_at(run, stops, &setup_003, 3600, None).beaten;
    assert!(
        beaten_003.iter().all(Option::is_some),
        "Setup_003 beaten: {beaten_003:?}"
    );
    let health_before = run.health().unwrap();
    // The key in the lobe, then up to the door and △ by it.
    assert_eq!(walk(run, stops, &KEY_1), Ok(()), "to the key");
    assert_eq!(walk(run, stops, &TO_THE_DOOR), Ok(()), "to the door");
    let mut opened = None;
    for k in 0..600u32 {
        step(run, stops, [false, false, false, k % 20 < 2]);
        if behaviour(run, door).as_deref() == Some("done") {
            opened = Some(run.steps);
            break;
        }
    }
    let opened = opened.expect("the door opened to its key");
    // Past it: the second trigger scares the Mabu, the third wakes the rest.
    let at_the_door = run.steps;
    assert_eq!(
        walk(run, stops, &TO_SETUP_004),
        Ok(()),
        "to Setup_004's triggers"
    );
    let mabu_from = run.world.actor(mabu).map(|a| a.position);
    let OnTheStick {
        beaten,
        bites,
        last: mabu_last,
    } = on_the_stick_at(run, stops, &foes, 6000, Some(mabu));
    for _ in 0..240 {
        step(run, stops, [false; 4]);
    }
    FourthFight {
        beaten_003,
        opened,
        at_the_door,
        beaten,
        bites,
        mabu,
        mabu_from,
        mabu_last,
        controller,
        health_before,
        health: run.health().unwrap(),
        xp: run.figure_values().map_or(0, |v| v.xp),
    }
}

/// openspec AS.84: level 214's fourth fight, Setup_004, fought on foot from
/// the sail. Off the ship to Setup_003's trigger, which wakes Setup_004 too;
/// Setup_003 fought on the stick. Down into the lobe below the yard (its
/// first trigger: the three Elf Nears walk to their posts), its key taken,
/// back up to the locked door (`Door_Template_Skylanders_2(1)`), which a tap
/// of △ by it opens; past it to the second trigger (the Mabu runs his
/// waypoints into the house by the yard and goes, the elves come on) and
/// the third (the Tree Rex and the Chompy spawner wake). All five fought on
/// the stick; the controller takes itself off. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_fourth_fight_is_fought_past_the_locked_door() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let f = fourth_fight(&mut run, &mut stops);
    let house = run.at("Interior_Door(3)");
    let charges = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "TreeRex_special_homing_actor")
        .count();
    let chompies = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "_Template_Undead_Chompy")
        .count();
    let mabu_gone = gone(&run, f.mabu).then_some(run.steps);
    println!(
        "Setup_003 beaten at {:?}; the door opened at {}; from the door at {}, Setup_004's beaten at {:?} (the elves A-C, the Tree Rex, the spawner), the Mabu gone {:?}, the controller gone {}; bites {:?}; health {} -> {}, experience {}; {charges} homing charges, {chompies} Chompies cloned; {:.1} s; load at end: {}",
        f.beaten_003,
        f.opened,
        f.at_the_door,
        f.beaten,
        mabu_gone,
        gone(&run, f.controller),
        f.bites,
        f.health_before,
        f.health,
        f.xp,
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        f.beaten.iter().all(Option::is_some),
        "all beaten: {:?}",
        f.beaten
    );
    // He ran his waypoints to the house's door and went there.
    let off_the_door = f.mabu_last.map(|p| (p.x - house.x).hypot(p.z - house.z));
    println!(
        "the Mabu from {:?} to {:?}, {off_the_door:?} m from the house's door at {house}",
        f.mabu_from, f.mabu_last
    );
    assert!(
        gone(&run, f.mabu) && off_the_door.is_some_and(|d| d < 2.0),
        "the Mabu ran into the house and went: {:?}, {off_the_door:?} m off {house}",
        f.mabu_last
    );
    assert!(gone(&run, f.controller), "Setup_004's controller done");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// From where Setup_004's fight leaves him to the house's door by the yard
/// (`Interior_Controller_Island_2_1`'s box), across the ground.
const TO_THE_HOUSE: [[f32; 2]; 2] = [[-262.0, 733.0], [-253.0, 739.0]];
/// The house's door, and the box past it.
const HOUSE_DOOR: [f32; 2] = [-249.54, 742.73];
const HOUSE_BOX: [f32; 2] = [-246.43, 745.75];
/// In the house's room: by its bomb spawner, before its wall to the
/// treasure (`Breakable_Stone_Wall(1)`, facing it) and at its way out.
const BY_THE_HOUSE_BOMBS: [f32; 2] = [387.43, 216.0];
const BEFORE_THE_HOUSE_WALL: ([f32; 2], [f32; 2]) = ([378.5, 220.3], [370.0, 220.5]);
const HOUSE_EXIT: [f32; 2] = [409.63, 210.65];
/// Out of the house: round the hay wagon to short of the wall before the
/// second key (`Breakable_Stone_Wall(7)`, faced), the key, and back round
/// the wagon to the second locked door, where △ turns it.
const ROUND_THE_WAGON: [[f32; 2]; 3] = [[-255.0, 741.0], [-261.0, 740.5], [-263.5, 746.3]];
const WALL_7: [f32; 2] = [-267.2, 750.7];
const KEY_2: [f32; 2] = [-264.9, 756.97];
const TO_THE_SECOND_DOOR: [[f32; 2]; 4] = [
    [-263.0, 745.0],
    [-261.0, 740.5],
    [-250.0, 733.0],
    [-246.5, 719.0],
];

/// Steps with the stick toward `to` until `h`'s behaviour reads `want`,
/// at most `steps`: the step it did.
fn walk_until(
    run: &mut Run,
    stops: &mut Vec<String>,
    to: [f32; 2],
    h: Handle,
    want: &str,
    steps: u32,
) -> Option<u32> {
    for _ in 0..steps {
        if behaviour(run, h).as_deref() == Some(want) {
            return Some(run.steps);
        }
        push_toward(run, stops, to);
    }
    None
}

/// A few steps with the stick toward `at`, to face it, then △ tapped (down
/// a step, up the next) every 20 steps, at most `steps`, until `wall` is
/// gone: the step it went.
fn throw_at_wall(
    run: &mut Run,
    stops: &mut Vec<String>,
    at: [f32; 2],
    wall: Handle,
    steps: u32,
) -> Option<u32> {
    for _ in 0..4 {
        push_toward(run, stops, at);
    }
    for k in 0..steps {
        step(run, stops, [false, false, false, k % 20 < 2]);
        if run.world.actor(wall).is_some_and(|a| a.removed) {
            return Some(run.steps);
        }
    }
    None
}

/// openspec AS.96: level 214's way from Setup_004's yard to the second
/// locked door on foot, as the level's own scripts and data have it (no
/// recording of 214 exists). Setup_004 fought as AS.84 fights it, then
/// into the house by the yard, where the Mabu ran
/// (`Interior_Controller_Island_2_1`): the darkness, and the hero lands in
/// its room under the island 0.7 s on. Its bomb spawner's bomb, carried to
/// its wall and thrown, opens the treasure behind it; back on its spawner
/// after its burst, it's taken again and carried out by the room's exit
/// (the warp keeps it over him: stand-in ledger `warp`), round the hay
/// wagon to the wall before the second key (`Breakable_Stone_Wall(7)`) and
/// thrown, which opens it; the key taken, the second locked door
/// (`Door_Template_Skylanders_2(2)`) opens to a tap of △ by it. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_second_door_opens_to_the_key_a_bomb_from_the_house_frees() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let f = fourth_fight(&mut run, &mut stops);
    assert!(
        f.beaten.iter().all(Option::is_some),
        "Setup_004 beaten: {:?}",
        f.beaten
    );
    let fought = run.steps;
    let house = handle(&run, "Interior_Controller_Island_2_1");
    let wall_1 = handle(&run, "Breakable_Stone_Wall(1)");
    let wall_7 = handle(&run, "Breakable_Stone_Wall(7)");
    let key = handle(&run, "Key Spawner(1)s(3)");
    let door = handle(&run, "Door_Template_Skylanders_2(2)");
    let house_door = handle(&run, "Interior_Door(3)");
    // Into the house: its door, which the Mabu opened and shut, swings
    // open as he comes near (`Generic_House_door_Script.ai`).
    assert_eq!(
        walk(&mut run, &mut stops, &TO_THE_HOUSE),
        Ok(()),
        "to the house"
    );
    assert_eq!(
        walk(&mut run, &mut stops, &[HOUSE_DOOR]),
        Ok(()),
        "to its door"
    );
    assert_eq!(
        behaviour(&run, house_door).as_deref(),
        Some("trigger door close"),
        "the house's door open by him"
    );
    let landed0 = run.world.landed.len();
    let dark = walk_until(
        &mut run,
        &mut stops,
        HOUSE_BOX,
        house,
        "2. fade out fade in",
        300,
    )
    .expect("the house takes him");
    for _ in 0..90 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let inside = run
        .world
        .landed
        .get(landed0)
        .copied()
        .expect("he lands inside");
    // Its wall to the treasure, by its bomb.
    assert_eq!(walk(&mut run, &mut stops, &[BY_THE_HOUSE_BOMBS]), Ok(()));
    let taken = |run: &Run| {
        bombs(run)
            .into_iter()
            .find(|b| behaviour(run, *b).as_deref() == Some("following player"))
    };
    let first = taken(&run).expect("the house's bomb taken");
    assert_eq!(
        walk(&mut run, &mut stops, &[BEFORE_THE_HOUSE_WALL.0]),
        Ok(())
    );
    let opened_1 = throw_at_wall(&mut run, &mut stops, BEFORE_THE_HOUSE_WALL.1, wall_1, 300)
        .expect("the house's wall opened by its bomb");
    // Its bomb back on its spawner, taken again and carried out.
    let mut retaken = None;
    for _ in 0..1200 {
        push_toward(&mut run, &mut stops, BY_THE_HOUSE_BOMBS);
        if let Some(b) = taken(&run) {
            retaken = Some((b, run.steps));
            break;
        }
    }
    let (bomb, retaken) = retaken.expect("the bomb taken again");
    let landed1 = run.world.landed.len();
    walk_until(
        &mut run,
        &mut stops,
        HOUSE_EXIT,
        house,
        "2. fade out fade in",
        600,
    )
    .expect("out by its exit");
    let mut out = None;
    for _ in 0..90 {
        step(&mut run, &mut stops, [false; 4]);
        if out.is_none() {
            out = run.world.landed.get(landed1).copied();
        }
    }
    let out = out.expect("he lands outside");
    assert_eq!(
        behaviour(&run, bomb).as_deref(),
        Some("following player"),
        "the bomb still over him"
    );
    // The wall before the second key, the key, the door.
    assert_eq!(
        walk(&mut run, &mut stops, &ROUND_THE_WAGON),
        Ok(()),
        "round the wagon"
    );
    let opened_7 = throw_at_wall(&mut run, &mut stops, WALL_7, wall_7, 300)
        .expect("the wall before the second key opened by the house's bomb");
    assert_eq!(walk(&mut run, &mut stops, &[KEY_2]), Ok(()), "to the key");
    for _ in 0..30 {
        step(&mut run, &mut stops, [false; 4]);
    }
    // Setup_004's spawner's Chompies come after him, and on some of the
    // session's number streams two stand in the gap by the door: he fights
    // them as a kid would (the au-hub-people review: the notifications'
    // queue's draws put them there on this run's stream).
    if let Err(at) = walk(&mut run, &mut stops, &TO_THE_SECOND_DOOR) {
        let in_the_way: Vec<Handle> = run
            .world
            .creatures()
            .into_iter()
            .filter(|h| !gone(&run, *h))
            .filter(|h| {
                run.world
                    .actor(*h)
                    .is_some_and(|a| a.position.distance(at) < 6.0)
            })
            .collect();
        assert!(!in_the_way.is_empty(), "to the door: stopped at {at:?}");
        let beaten = fight(&mut run, &mut stops, &in_the_way, 1200);
        assert!(beaten.iter().all(Option::is_some), "{beaten:?}");
        assert_eq!(
            walk(&mut run, &mut stops, &TO_THE_SECOND_DOOR),
            Ok(()),
            "to the door, past them"
        );
    }
    let mut done = None;
    for k in 0..600u32 {
        step(&mut run, &mut stops, [false, false, false, k % 20 < 2]);
        if behaviour(&run, door).as_deref() == Some("done") {
            done = Some(run.steps);
            break;
        }
    }
    println!(
        "Setup_004 done at {fought}; the house's darkness at {dark}, landed at {:?} facing {}; its first bomb {first:#x}, its wall open at {opened_1}, the bomb taken again at {retaken}; out at {:?} facing {}; the wall before the key open at {opened_7}; the door done at {done:?}; health {} -> {:?}; {:.1} s; load at end: {}",
        inside.at,
        inside.facing,
        out.at,
        out.facing,
        f.health,
        run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(done.is_some(), "the second door opened to its key");
    // The key's spawner goes once its key is in the lock.
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
    assert!(gone(&run, key), "the second key's spawner done");
    assert!(stops.is_empty(), "{stops:#?}");
}
