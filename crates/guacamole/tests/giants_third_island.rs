//! Level 214's third island to its end (openspec AS.76, AS.114, AS.115,
//! AU.23) and level 201's start (AS.113), headless with guacamole's host on
//! the TV's listing. Ignored by default; they read `GUAC_GIANTS_DUMP`
//! (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_third_island -- --ignored --nocapture --test-threads=1`
//!
//! No recording of 214 or 201 exists: every step is the levels' own scripts
//! on the TV's listing, the hero walked on the stick from the second ship
//! part, woken as `Temp_Teleporter.ai` wakes it (the dev entry's `part2`
//! anchor; the walk from the first island is AU.33's).

use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::{NodePose, Pad};
use guacamole::turret;

mod common;

use common::*;

const LEVEL_214: &str = "level_214_tutotial_2";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn global(run: &Run, name: &str) -> Option<Value> {
    let g = run.vm.program().global_named(name)?;
    run.vm.global_peek(g).map(|v| run.vm.value(v))
}

fn step(run: &mut Run, stops: &mut Vec<String>, stick: Stick, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick,
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

fn still(run: &mut Run, stops: &mut Vec<String>, actions: [bool; 4]) {
    step(run, stops, Stick::new(0.0, 0.0), actions);
}

/// The stick that walks him toward `to`, the view the step before's.
fn stick_to(run: &Run, to: [f32; 2]) -> Stick {
    let at = run.hero().pivot;
    let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
    let d = (heading - run.bench.view).to_radians();
    Stick::new(-d.sin(), d.cos())
}

fn walk(run: &mut Run, stops: &mut Vec<String>, points: &[[f32; 2]]) {
    for &to in points {
        for _ in 0..1500 {
            let at = run.hero().pivot;
            if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                break;
            }
            let s = stick_to(run, to);
            step(run, stops, s, [false; 4]);
        }
        let at = run.hero().pivot;
        assert!(
            (to[0] - at.x).hypot(to[1] - at.z) <= 1.0,
            "walking to {to:?}: stopped at {at}"
        );
    }
}

fn gone(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
        || run.vm.placement_of(h).is_none_or(|id| run.vm.removed(id))
}

fn behaviour(run: &Run, h: Handle) -> Option<String> {
    run.vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_owned)
}

/// Fought on the stick: toward the nearest of `foes` standing, his first
/// attack mashed once within 4.5 m, facing it. The step each went.
fn fight(run: &mut Run, stops: &mut Vec<String>, foes: &[Handle], steps: u32) -> Vec<Option<u32>> {
    let mut beaten: Vec<Option<u32>> = vec![None; foes.len()];
    // Pressed into a post with the foe past it: round it, as a hand would
    // (the gate posts by Setup_006's trigger stop him dead on some runs).
    let (mut from, mut stuck, mut round) = (run.hero().pivot, 0u32, None);
    for k in 0..steps {
        for (b, h) in beaten.iter_mut().zip(foes) {
            if b.is_none() && gone(run, *h) {
                *b = Some(run.steps);
            }
        }
        if beaten.iter().all(Option::is_some) {
            break;
        }
        let me = run.hero().pivot;
        let near = foes
            .iter()
            .filter(|h| !gone(run, **h))
            .filter_map(|h| run.world.actor(*h).map(|a| a.position))
            .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)))
            .expect("one standing");
        let far = (near.x - me.x).hypot(near.z - me.z);
        stuck = if far > 4.5 && me.distance(from) < 0.01 {
            stuck + 1
        } else {
            0
        };
        from = me;
        if stuck > 30 {
            let side = if (k / 120) % 2 == 0 { 1.0 } else { -1.0 };
            let across = Vec3::new(near.z - me.z, 0.0, me.x - near.x).normalize_or_zero();
            round = Some((k + 45, me + across * 6.0 * side));
            stuck = 0;
        }
        let s = match round {
            Some((until, to)) if k < until => stick_to(run, [to.x, to.z]),
            _ => stick_to(run, [near.x, near.z]),
        };
        let mash = far <= 4.5 && k % 4 < 2;
        step(run, stops, s, [mash, false, false, false]);
    }
    beaten
}

/// Joined on 214, its intro skipped, by the second ship part with it woken
/// as the dev entry's `part2` anchor has it.
fn by_the_second_part(setup: Setup) -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, LEVEL_214, setup);
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    stops.extend(skip_214s_intro(&mut run).iter().map(ToString::to_string));
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let (part, at) = run.placed("Ship_part_2(1)");
    assert!(run.world.wake(part));
    run.put(at, 0.0);
    // The second island's fights left behind, as the anchor leaves them.
    let setup_003 = run.placed("Setup_003_Controller").0;
    let Run { vm, world, .. } = &mut run;
    let off =
        guacamole::scripts::take_off_sets(vm, world, &guacamole::anchors::SECOND_ISLAND_FIGHTS);
    assert!(off > 20, "{off} taken off");
    assert!(gone(&run, setup_003), "its controller among them");
    (run, stops)
}

/// A press takes the second ship part, whose shutdown wakes the final
/// island's cutscene; the step it went.
fn take_the_second_part(run: &mut Run, stops: &mut Vec<String>) -> u32 {
    let part = run.placed("Ship_part_2(1)").0;
    for k in 0..600 {
        still(run, stops, [k % 20 < 2, false, false, false]);
        if gone(run, part) {
            return run.steps;
        }
    }
    panic!("the second ship part never went");
}

/// The rig's node a player hangs on, posed by hand: the headless run draws
/// nothing, so a drawn rig's clip is stood in for by a path of our own.
fn fly(run: &mut Run, steps_in: u32) -> Vec3 {
    let at = Vec3::new(100.0 + steps_in as f32 * 0.1, 40.0, -200.0);
    let hung = run.world.actor(run.hero).map(|a| a.attach);
    if let Some(v) = hung
        && let Some(owner) = owner_of(run, v)
    {
        let a = run.world.actor(owner).expect("what he hangs from");
        let turn = guac_motion::seam::Quat::from_rotation_y(-a.angles.facing.to_radians());
        let off = turn * (at - a.position);
        run.world.set_nodes(owner, vec![NodePose::at(off); 13]);
    }
    at
}

fn owner_of(run: &Run, v: Value) -> Option<Handle> {
    run.world.node_owner(v)
}

/// openspec AS.76, AS.115, AS.114: the second ship part taken, the final
/// island's cutscene plays its nine scenes to their end, the players hung
/// on the animator camera's rig and the ship for scenes 4-7: his body rides
/// the node with his controller off, no body of his in the seam, and he's
/// handed back at the detach and set down on the third island's ship.
/// MoneyBags reads his 120 %. Nothing stops.
/// openspec AS.146's review: over a gap with no mesh under him he falls at
/// his gravity to his 120 ft/s, the minion's own check sets him back on his
/// safe point trail, and the stick walks him on. Until then the mover
/// update read his ground's type as -1 and took him off its movers every
/// step: he hung there, gravity and stick gone, and stayed so on any ground a
/// catch brought him back to (`Hero::catch_fall`, below all this now).
#[test]
#[ignore = "reads the Giants dump"]
fn off_the_edge_he_falls_and_his_own_scripts_set_him_back() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap();
    stops.extend(skip_214s_intro(&mut run).iter().map(ToString::to_string));
    let floor = guac_motion::ground::find(
        guacamole::script_engine::Engine::seam(&run.bench),
        Vec3::new(34.0, 60.0, 6.0),
        &[],
    )
    .expect("the first island");
    run.put(floor.point + Vec3::Y * 0.5, 180.0);
    for _ in 0..300 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let stood = run.hero().pivot;
    assert!(run.hero().footing.supported, "on the island");
    // Knocked over the gap west of here, past the wall a walk stops at.
    run.hero_mut().pivot = Vec3::new(16.0, stood.y, 6.0);
    let edge = run.hero().pivot;
    let (mut fastest, mut lowest, mut back) = (0.0f32, edge.y, None);
    for k in 0..1200u32 {
        still(&mut run, &mut stops, [false; 4]);
        let h = run.hero();
        fastest = fastest.min(h.walk.velocity.y);
        lowest = lowest.min(h.pivot.y);
        if h.footing.supported {
            back = Some(k);
            break;
        }
    }
    println!(
        "over nothing at {edge}, fell to y {lowest:.1} at {fastest:.1} ft/s, back after \
         {back:?} at {}",
        run.hero().pivot
    );
    assert!((fastest + 120.0).abs() < 0.5, "his gravity: {fastest}");
    assert!(back.is_some(), "set back");
    assert!(
        edge.y - lowest > 225.0 / 3.28084,
        "fell {}",
        edge.y - lowest
    );
    assert!(
        run.hero().pivot.distance(stood) < 15.0,
        "where he'd walked: {}",
        run.hero().pivot
    );
    let from = run.hero().pivot;
    for _ in 0..60 {
        let s = stick_to(&run, [40.0, 6.0]);
        step(&mut run, &mut stops, s, [false; 4]);
    }
    let walked = run.hero().pivot.distance(from);
    println!(
        "walked {walked:.2} m on, {:.1} s, load at end: {}",
        t.elapsed().as_secs_f32(),
        load()
    );
    assert!(walked > 3.0, "the stick moves him: {walked}");
    assert!(stops.is_empty(), "{stops:#?}");
}

#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_final_island_carries_him_hung_on_its_rigs() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = by_the_second_part(setup());
    let taken = take_the_second_part(&mut run, &mut stops);
    let fin = run.placement("214_Final_Island_IGC");
    let mut scenes: Vec<(usize, u32)> = Vec::new();
    let mut hung_steps = 0;
    let mut rode = 0;
    let mut bodied = 0;
    let mut sizes = std::collections::BTreeMap::new();
    for k in 0..6000u32 {
        let expect = fly(&mut run, k);
        still(&mut run, &mut stops, [false; 4]);
        if let Some((i, n)) = scene_of(&run) {
            assert_eq!(n, 9, "its nine scenes");
            if scenes.last().is_none_or(|(at, _)| *at != i) {
                scenes.push((i, run.steps));
            }
        }
        if run.hero().hung {
            hung_steps += 1;
            if run.hero().pivot.distance(expect) < 0.5 {
                rode += 1;
            }
            if run.bench.bodies.contains_key(&run.hero) {
                bodied += 1;
            }
            let walking = run.hero().walk.velocity.length();
            assert!(walking < 1e-3, "his controller's off: {walking}");
        }
        for h in run.world.clones() {
            if run.world.name(h) == "214_RFP_MoneyBags"
                && let Some(a) = run.world.actor(h).filter(|a| !a.removed)
            {
                sizes.insert(h, a.scale[0]);
            }
        }
        if run.vm.removed(fin) {
            break;
        }
    }
    assert!(run.vm.removed(fin), "the cutscene ends: {scenes:?}");
    println!(
        "taken at {taken}; scenes {scenes:?}; hung {hung_steps} steps, on the node {rode}; {:?} at the end",
        run.hero().pivot
    );
    assert_eq!(
        scenes.iter().map(|s| s.0).collect::<Vec<_>>(),
        (0..9).collect::<Vec<_>>()
    );
    assert!(hung_steps > 500, "hung for scenes 4-7: {hung_steps}");
    assert!(rode > hung_steps * 9 / 10, "{rode} of {hung_steps}");
    assert_eq!(bodied, 0, "no body while hung");
    assert!(!run.hero().hung, "handed back");
    let at = run.hero().pivot;
    assert!(
        at.distance(Vec3::new(583.02, -16.89, 465.05)) < 1.0,
        "set down on the third island's ship: {at}"
    );
    assert!(
        !sizes.is_empty() && sizes.values().all(|s| (*s - 120.0).abs() < 1e-3),
        "MoneyBags at 120 %: {sizes:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Skipped by △ from the part on, as a player would: on the third island's
/// ship with the final island's cutscene over and Flynn's pan skipped.
fn on_the_third_ship(run: &mut Run, stops: &mut Vec<String>) {
    take_the_second_part(run, stops);
    let fin = run.placement("214_Final_Island_IGC");
    for k in 0..6000u32 {
        still(run, stops, [false, false, false, k % 20 == 10]);
        if run.vm.removed(fin) {
            break;
        }
    }
    assert!(run.vm.removed(fin));
    for k in 0..900u32 {
        let cs = cutscene_name(run);
        still(
            run,
            stops,
            [false, false, false, cs.is_some() && k % 20 == 10],
        );
    }
}

/// Into Persephone's room and out again on foot: her cutscene's four
/// scenes, its menu backed out of; `Visited Persephony` from then on.
fn visit_persephone(run: &mut Run, stops: &mut Vec<String>) {
    let (door, facing) =
        guacamole::interiors::door(&run.world, LEVEL, "Interior_controller_Persephone_ship3")
            .expect("her door");
    run.put(door, facing);
    let pers = run.placed("Persephone_214").0;
    let mut played = false;
    for k in 0..4800u32 {
        let s = if k < 120 {
            let d = (facing - run.bench.view).to_radians();
            Stick::new(-d.sin(), d.cos())
        } else {
            Stick::new(0.0, 0.0)
        };
        step(run, stops, s, [false; 4]);
        played |= cutscene_name(run).as_deref() == Some("214_Upgrade_IGC");
        if played && cutscene_name(run).is_none() {
            break;
        }
    }
    assert!(played, "her cutscene played");
    assert!(
        cutscene_name(run).is_none(),
        "and ended on its menu's back-out"
    );
    assert_eq!(global(run, "Visited Persephony"), Some(Value::Int(1)));
    for _ in 0..240 {
        still(run, stops, [false; 4]);
    }
    assert_eq!(behaviour(run, pers).as_deref(), Some("idle"));
    let wps: Vec<Vec3> = {
        let ctl = run
            .files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == "Interior_controller_Persephone_ship3")
            .expect("her room's controller");
        ctl.waypoints.iter().map(|w| run.files.points[w]).collect()
    };
    run.put(wps[2], 0.0);
    for _ in 0..300 {
        still(run, stops, [false; 4]);
    }
    assert!(
        run.hero().pivot.y < 0.0,
        "back on deck: {}",
        run.hero().pivot
    );
}

/// A turret entered by its prompt (△) from where he stands, its pointer
/// calibrated on the circle, each target shot in turn, then out.
fn play_turret(run: &mut Run, stops: &mut Vec<String>, count: usize) -> u32 {
    into_turret(run, stops);
    shoot_padlocks(run, stops, count);
    out_of_turret(run, stops)
}

/// A turret entered by its prompt (△) from where he stands and its
/// pointer calibrated on the circle, to its game.
fn into_turret(run: &mut Run, stops: &mut Vec<String>) {
    for k in 0..240u32 {
        still(run, stops, [false, false, false, k % 30 < 2]);
        if turret::view(&run.vm, &run.world).is_some() && !run.hero().hung && k > 200 {
            break;
        }
    }
    let view = |run: &Run| turret::view(&run.vm, &run.world).expect("a turret running");
    assert!(run.hero().hung, "in the turret, hung on it");
    // Onto the circle.
    for _ in 0..600 {
        let v = view(run);
        let (Some(at), Some(p)) = (v.calibration, v.pointer) else {
            if v.state.ends_with("State - Active") {
                break;
            }
            still(run, stops, [false; 4]);
            continue;
        };
        let s = Stick::new(((at[0] - p[0]) / 40.0).clamp(-1.0, 1.0), 0.0);
        step(run, stops, s, [false; 4]);
    }
    for _ in 0..600 {
        if view(run).state.ends_with("State - Active") {
            break;
        }
        still(run, stops, [false; 4]);
    }
    assert!(
        view(run).state.ends_with("State - Active"),
        "{:?}",
        view(run)
    );
}

/// The turret's padlocks shot until `count` are hit.
fn shoot_padlocks(run: &mut Run, stops: &mut Vec<String>, count: usize) {
    let from = turret::view(&run.vm, &run.world)
        .expect("a turret running")
        .hits;
    for k in 0..7200u32 {
        let Some(v) = turret::view(&run.vm, &run.world) else {
            break;
        };
        if v.hits as usize >= count || v.targets.is_empty() {
            break;
        }
        let p = v.pointer.expect("its pointer");
        let t = v
            .targets
            .iter()
            .copied()
            .min_by(|a, b| {
                let d = |q: &[f32; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
                d(a).total_cmp(&d(b))
            })
            .expect("a target");
        let s = Stick::new(
            ((t[0] - p[0]) / 40.0).clamp(-1.0, 1.0),
            (-(t[1] - p[1]) / 40.0).clamp(-1.0, 1.0),
        );
        step(run, stops, s, [k % 20 < 10, false, false, false]);
    }
    let hit = turret::view(&run.vm, &run.world).map_or(count as i32, |v| v.hits);
    assert_eq!(hit as usize, count, "every padlock: from {from}");
}

/// Out of the turret its game let go of him: carried over the arc its
/// get-out works out and set down on the ground by its spot (openspec
/// AS.189). The step he was let go.
fn out_of_turret(run: &mut Run, stops: &mut Vec<String>) -> u32 {
    let middle = turret_middle(run).expect("its turret");
    let (mut spot, mut peak) = (None, f32::MIN);
    for _ in 0..900 {
        still(run, stops, [false; 4]);
        let Some(v) = turret::view(&run.vm, &run.world) else {
            break;
        };
        if let Some(at) = v.get_out {
            spot = Some(Vec3::from_array(at));
            peak = peak.max(run.hero().pivot.y);
        }
    }
    assert!(turret::active(&run.vm).is_none(), "its game over");
    assert!(!run.hero().hung, "out of it");
    let over = run.steps;
    for _ in 0..120 {
        still(run, stops, [false; 4]);
    }
    let spot = spot.expect("the spot its get-out works out");
    let at = run.hero().pivot;
    let off = (at.x - spot.x).hypot(at.z - spot.z);
    let out = (at.x - middle.x).hypot(at.z - middle.z);
    println!(
        "out at {over}: the turret's middle {middle}, its spot {spot}, his highest {:.2} m over it, him 2 s on at {at}: {off:.2} m from the spot, {out:.2} m from the middle",
        peak - middle.y
    );
    // openspec AS.189: over the arc and onto the ground by the spot, off
    // the ball (it lifted him onto its top, 4.5 m up, from its middle).
    assert!(
        peak - middle.y > 7.0,
        "the arc's height: {}",
        peak - middle.y
    );
    assert!(off < 1.0, "by the spot: {off}");
    assert!((at.y - spot.y).abs() < 0.3, "on the ground: {at} by {spot}");
    assert!(out > 8.0, "beside the turret: {out}");
    over
}

/// The turret lost as its last hurt loses it (`New Turret - Flash Turret
/// On Hurt` once its health is gone): no win, its players getting out.
fn lose_turret(run: &mut Run) {
    let t = turret::active(&run.vm).expect("a turret running");
    let out = run
        .vm
        .program()
        .object_named(turret::GETTING_OUT)
        .expect("its getting out");
    assert!(run.vm.set_field(t, "win?", Value::Int(0)));
    assert!(run.vm.set_reference(t, "turret state", Value::Object(out)));
}

/// The middle of the turret running.
fn turret_middle(run: &Run) -> Option<Vec3> {
    let t = turret::active(&run.vm)?;
    match run.vm.value(run.vm.field(t, "turret actor")?) {
        Value::Entity(h) => run.world.actor(h).map(|a| a.position),
        _ => None,
    }
}

/// openspec AS.76, AU.23: the third island walked to its end. From the
/// ship: Persephone's room (the drawbridge wants her first visit), across
/// the bridge to Setup_006's trigger, its Tree Rexes and spawner then its
/// seven side enemies fought; the first turret lost, used again and its
/// three padlocks shot open the first gate, out of it on the ground by the
/// spot its get-out works out each time (AS.189); `PvP_JumpPad_SUPER(6)` onto the blocks' plateau; a stone
/// block pushed three spans north and, past a barrel, two back to bridge
/// the gap; round by the ramp and across it to the second turret and its
/// four padlocks; through the second gate to the third ship part; its
/// outro's seven scenes to their end, whose level end names the village:
/// hub stage 1. 214's own objectives read for its completion screen.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_third_island_is_walked_to_its_end() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = by_the_second_part(setup());
    run.world.sequence.shows_completion = true;
    on_the_third_ship(&mut run, &mut stops);
    println!("on the ship at {}: {}", run.steps, run.hero().pivot);
    visit_persephone(&mut run, &mut stops);
    println!("back on deck at {}: {}", run.steps, run.hero().pivot);
    let bridge = run.placed("DrawBridge").0;
    let barrier = run.placed("Bridge_Barrier").0;
    walk(&mut run, &mut stops, &[[590.0, 469.2]]);
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert_eq!(behaviour(&run, bridge).as_deref(), Some("drop"));
    assert!(gone(&run, barrier), "the barrier off as the bridge drops");
    walk(&mut run, &mut stops, &[[620.0, 468.9]]);
    let first: Vec<Handle> = [
        "Enemy_TreeRex(4)",
        "Enemy_TreeRex(5)",
        "Enemy_Chompy_Spawner(4)",
    ]
    .iter()
    .map(|n| run.placed(n).0)
    .collect();
    let beaten = fight(&mut run, &mut stops, &first, 6000);
    println!("Setup_006's first: {beaten:?}, health {:?}", run.health());
    assert!(beaten.iter().all(Option::is_some), "{beaten:?}");
    for _ in 0..300 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let side: Vec<Handle> = [
        "Enemy_Elf_Lobber(4)",
        "Enemy_Elf_Lobber(6)",
        "Enemy_Elf_Lobber(7)",
        "Enemy_Elf_Near_S2(5)",
        "Enemy_Elf_Near_S2(6)",
        "Enemy_Elf_Near_S2(7)",
        "Enemy_Elf_Near_S2(8)",
    ]
    .iter()
    .map(|n| run.placed(n).0)
    .collect();
    let beaten = fight(&mut run, &mut stops, &side, 9000);
    println!("its side enemies: {beaten:?}, health {:?}", run.health());
    assert!(beaten.iter().all(Option::is_some), "{beaten:?}");
    for _ in 0..300 {
        still(&mut run, &mut stops, [false; 4]);
    }
    // Up to the turret's sphere, solid to him (its 26 ft its scripts give
    // it, openspec AS.168), short of its middle.
    walk(&mut run, &mut stops, &[[665.2, 457.6]]);
    // Lost first, as its health running out loses it: out by its spot, its
    // prompt back and into it again from there (openspec AS.189).
    into_turret(&mut run, &mut stops);
    lose_turret(&mut run);
    let lost = out_of_turret(&mut run, &mut stops);
    println!("the first turret lost at {lost}");
    let t1 = play_turret(&mut run, &mut stops, 3);
    println!("the first turret over at {t1}");
    assert_eq!(global(&run, "First Gate Openned"), Some(Value::Int(1)));
    // Through the gate and up the pad.
    walk(
        &mut run,
        &mut stops,
        &[[676.0, 456.0], [695.0, 456.0], [702.3, 458.0]],
    );
    for _ in 0..360 {
        let s = stick_to(&run, [702.3, 472.0]);
        step(&mut run, &mut stops, s, [false; 4]);
        let p = run.hero().pivot;
        if p.z > 468.0 && p.y > -12.0 {
            break;
        }
    }
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let at = run.hero().pivot;
    assert!(at.y > -12.0 && at.z > 467.0, "on the plateau: {at}");
    // The block three spans north, then two back past the barrel.
    let block = run.placed("Push_Block_Template(4)").0;
    let z = |run: &Run| run.world.actor(block).map_or(0.0, |a| a.position.z);
    walk(&mut run, &mut stops, &[[705.6, 473.0]]);
    for _ in 0..900 {
        let s = stick_to(&run, [705.6, 520.0]);
        step(&mut run, &mut stops, s, [false; 4]);
        if z(&run) > 491.0 {
            break;
        }
    }
    assert!(z(&run) > 491.0, "three spans north: {}", z(&run));
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    walk(&mut run, &mut stops, &[[702.0, 487.0], [702.0, 496.0]]);
    for k in 0..240u32 {
        let s = stick_to(&run, [705.0, 494.3]);
        step(&mut run, &mut stops, s, [k % 4 < 2, false, false, false]);
    }
    println!("the barrel broken, him at {}", run.hero().pivot);
    // Square behind the block before pushing. His swings leave him flush on
    // its face at its west corner, facing aside (the walk that stood through
    // them until AU.37 ran him into it and pushed it two spans there): a push
    // begun there slides off west. Walked up to it the block goes from 0.9 m
    // off its middle (705.6) as from the middle, so this is the route's
    // start, not his push.
    for _ in 0..120 {
        if run.hero().pivot.x > 705.8 {
            break;
        }
        let s = stick_to(&run, [706.0, 495.0]);
        step(&mut run, &mut stops, s, [false; 4]);
    }
    println!("behind the block at {}", run.hero().pivot);
    for _ in 0..900 {
        let s = stick_to(&run, [705.4, 440.0]);
        step(&mut run, &mut stops, s, [false; 4]);
        if z(&run) < 482.5 {
            break;
        }
    }
    assert!(
        (z(&run) - 482.0).abs() < 0.5,
        "the bridge: {} him at {}",
        z(&run),
        run.hero().pivot
    );
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    walk(
        &mut run,
        &mut stops,
        &[
            [702.0, 496.0],
            [696.0, 492.0],
            [690.5, 492.3],
            [690.0, 498.0],
            [692.5, 502.0],
            [700.5, 503.0],
            [709.0, 503.0],
            [712.0, 499.0],
            [712.0, 492.0],
            [710.5, 488.0],
            [710.0, 482.0],
            [703.0, 482.0],
            [697.0, 482.0],
            [691.0, 480.0],
            [686.8, 473.9],
        ],
    );
    let t2 = play_turret(&mut run, &mut stops, 4);
    println!("the second turret over at {t2}");
    walk(
        &mut run,
        &mut stops,
        &[
            [691.0, 480.0],
            [697.0, 482.0],
            [703.0, 482.0],
            [710.0, 482.0],
            [711.5, 492.0],
            [711.5, 503.0],
            [713.0, 512.0],
        ],
    );
    for _ in 0..120 {
        let s = stick_to(&run, [715.0, 521.5]);
        step(&mut run, &mut stops, s, [false; 4]);
    }
    // The third ship part, its outro to the screen.
    let part = run.placed("Ship_part_3").0;
    let mut scenes = Vec::new();
    let mut outro_sizes = std::collections::BTreeMap::new();
    let mut part_runs = false;
    for k in 0..8000u32 {
        let cs = cutscene_name(&run);
        let taking = cs.is_none() && !gone(&run, part) && k % 20 < 2;
        still(&mut run, &mut stops, [taking, false, false, false]);
        if let Some((i, n)) = scene_of(&run)
            && scenes.last() != Some(&i)
        {
            assert_eq!(n, 7, "its seven scenes");
            scenes.push(i);
        }
        for h in run.world.clones() {
            let name = run.world.name(h).to_owned();
            // `IGC_Music_Player.ai`'s STARTUP clones its body.
            part_runs |= name == "214_RP_Static_Parts";
            if ["Flynn_NPC", "Cali_NPC"].contains(&name.as_str())
                && let Some(a) = run.world.actor(h).filter(|a| !a.removed)
            {
                outro_sizes.insert(name, a.scale[0]);
            }
        }
        if run.world.sequence.completion.is_some() {
            break;
        }
    }
    println!(
        "the outro's scenes {scenes:?} to step {}; sizes {outro_sizes:?}",
        run.steps
    );
    assert_eq!(scenes, (0..7).collect::<Vec<_>>());
    assert_eq!(outro_sizes.get("Flynn_NPC"), Some(&130.0));
    assert_eq!(outro_sizes.get("Cali_NPC"), Some(&110.0));
    assert!(part_runs, "the outro's ship part made its body");
    let results = run
        .world
        .sequence
        .completion
        .as_ref()
        .expect("the completion screen's state")
        .results
        .clone()
        .expect("its results read");
    println!(
        "{} objectives, stars {:?}",
        results.objectives.len(),
        results.stars
    );
    assert!(!results.objectives.is_empty());
    assert!(
        run.world.level_change().is_none(),
        "the screen before the level"
    );
    run.world
        .sequence
        .completion
        .as_mut()
        .unwrap()
        .continue_asked = true;
    for _ in 0..1200 {
        still(&mut run, &mut stops, [false; 4]);
        if run.world.level_change().is_some() {
            break;
        }
    }
    let change = run.world.level_change().cloned().expect("a level change");
    assert_eq!(change.name, "level_hub_2_stage_1");
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().take(5).collect::<Vec<_>>()
    );
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// openspec AS.115: △ pressed while he hangs from the final island's rigs:
/// the cutscene skips as the original's own skip does and he's handed back,
/// off the rig with his body in the seam, on the third island's ship and
/// walking. A kid skipping mid-flight never leaves him hung.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_final_island_skipped_while_hung_hands_him_back() {
    println!("load at start: {}", load());
    let (mut run, mut stops) = by_the_second_part(setup());
    take_the_second_part(&mut run, &mut stops);
    let fin = run.placement("214_Final_Island_IGC");
    let mut hung_when_pressed = 0;
    for k in 0..6000u32 {
        fly(&mut run, k);
        let hung = run.hero().hung;
        let press = hung && k % 20 == 10;
        hung_when_pressed += u32::from(press);
        still(&mut run, &mut stops, [false, false, false, press]);
        if run.vm.removed(fin) {
            break;
        }
    }
    assert!(run.vm.removed(fin), "the cutscene ends");
    assert!(hung_when_pressed > 0, "△ pressed while he hung");
    assert!(!run.hero().hung, "handed back");
    // Flynn's pan, skipped as well.
    for k in 0..900u32 {
        let cs = cutscene_name(&run);
        still(
            &mut run,
            &mut stops,
            [false, false, false, cs.is_some() && k % 20 == 10],
        );
    }
    let at = run.hero().pivot;
    println!("△ {hung_when_pressed} times hung; at {at}");
    assert!(
        at.distance(Vec3::new(583.02, -16.89, 465.05)) < 2.0,
        "on the third island's ship: {at}"
    );
    assert!(run.bench.bodies.contains_key(&run.hero), "his body back");
    for _ in 0..240 {
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let moved = run.hero().pivot.distance(at);
    println!("walked {moved:.2} m; load at end: {}", load());
    assert!(moved > 1.0, "{moved}");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.114: the sail's MoneyBags clone at his 120 %, nothing
/// stopped: `214_RFP_MoneyBags` is an object the cutscene adds, so nothing
/// but the size script's listing gives it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_sail_shows_moneybags_at_his_size() {
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    stops.extend(skip_214s_intro(&mut run).iter().map(ToString::to_string));
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let (_, at) = run.placed("Ship_part_1(1)");
    run.put(at, 0.0);
    let sail = run.placement("214_Recover_First_Part_IGC");
    let mut sizes = std::collections::BTreeMap::new();
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [k % 20 < 2, false, false, false]);
        for h in run.world.clones() {
            if run.world.name(h) == "214_RFP_MoneyBags"
                && let Some(a) = run.world.actor(h).filter(|a| !a.removed)
            {
                sizes.insert(h, a.scale[0]);
            }
        }
        if run.vm.removed(sail) {
            break;
        }
    }
    println!("sizes {sizes:?}; load at end: {}", load());
    assert!(run.vm.removed(sail), "the sail ends");
    assert!(
        !sizes.is_empty() && sizes.values().all(|s| (*s - 120.0).abs() < 1e-3),
        "MoneyBags at 120 %: {sizes:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.113: level 201's intro played to its end, the landing pod's
/// rig posed by hand as a drawn clone's nodes are: he hangs on it from its
/// first scene, is let go in its last, the scene walks him in along his
/// path and hands him over at its end, the HUD back. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_201s_intro_plays_to_its_end_and_walks_him_in() {
    println!("load at start: {}", load());
    let (mut run, _) = run_on_level(Mode::Play, "level_201", setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let (mut scenes, mut hung) = (Vec::new(), 0);
    let landed = Vec3::new(160.8, 13.5, -126.7);
    let mut ended = None;
    for k in 0..6000u32 {
        if let Some(v) = run.world.actor(run.hero).map(|a| a.attach)
            && let Some(owner) = owner_of(&run, v)
        {
            let a = run.world.actor(owner).expect("what he hangs from");
            let turn = guac_motion::seam::Quat::from_rotation_y(-a.angles.facing.to_radians());
            let off = turn * (landed - a.position);
            run.world.set_nodes(owner, vec![NodePose::at(off); 13]);
        }
        still(&mut run, &mut stops, [false; 4]);
        hung += u32::from(run.hero().hung);
        if let Some((i, n)) = scene_of(&run)
            && scenes.last() != Some(&i)
        {
            assert_eq!(n, 5, "its five scenes");
            scenes.push(i);
        }
        if k > 200 && !guacamole::anchors::scene_holds(&run.vm) {
            ended = Some(run.steps);
            break;
        }
    }
    println!(
        "scenes {scenes:?}, hung {hung} steps, over at {ended:?}: {}",
        run.hero().pivot
    );
    assert_eq!(scenes, (0..5).collect::<Vec<_>>());
    assert!(ended.is_some(), "its last walk ends");
    assert!(hung > 1000, "{hung}");
    assert!(!run.hero().hung);
    let at = run.hero().pivot;
    assert!(at.distance(Vec3::new(158.99, 12.7, -159.0)) < 1.5, "{at}");
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.113, AS.126: level 201's start: its intro held for the hero
/// and skipped by △, the HUD back, him where its path ends, on the ground
/// and walking on the stick, nothing stopped; its stray `Temp_Initalize`
/// (where he started before, in the air and held by the level's scene) off
/// his way.
#[test]
#[ignore = "reads the Giants dump"]
fn level_201s_intro_hands_him_over_and_he_walks() {
    println!("load at start: {}", load());
    let (mut run, _) = run_on_level(Mode::Play, "level_201", setup());
    let end = Vec3::new(158.99, 12.70, -159.0);
    let start = run.hero().pivot;
    println!("his start {start}");
    assert!(start.distance(end) < 2.0, "where its path ends: {start}");
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let mut played = false;
    for k in 0..3000u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        played |= cutscene_name(&run).as_deref() == Some("Level 201 Intro CS");
        if k > 200 && played && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(played, "its intro ran");
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    let at = run.hero().pivot;
    assert!(at.distance(end) < 0.5, "at its path's end: {at}");
    assert!(run.hero().footing.supported, "on the ground");
    for _ in 0..600 {
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let moved = run.hero().pivot.distance(at);
    println!("walked {moved:.2} m from {at}");
    assert!(moved > 3.0, "{moved}");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}
