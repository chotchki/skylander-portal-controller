//! A creature staged as the TV's stage stages one (openspec AS.680), headless
//! on guacamole's host: listed and walked by `guacamole::staging`, the hero
//! set down by it as the dev entry's `by` anchor sets him, its wake left to
//! the activity rule. Ignored by default; reads `GUAC_GIANTS_DUMP` (process
//! env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_stage -- --ignored --nocapture`

use guac_motion::seam::Vec3;
use guac_script::Mode;
use guacamole::staging::{frame, in_view};

mod common;

use common::*;

/// How far off its facing the dev entry's `by` anchor sets him down, metres
/// (the binary's `dev_entry::BY_DISTANCE`).
const BY: f32 = 7.0;

/// Steps with no wake before the test gives up: the activity rule wakes a
/// creature the step the hero is in its range, so a few dozen is plenty.
const STALL: u32 = 120;

/// `name` on `level` staged: Eruptor joined, set down by it, stepped until
/// it wakes. The run, whether it was awake at the load, the step it woke
/// and what hangs on it by name.
fn staged(level: &str, name: &'static str) -> (Run, bool, u32, Vec<String>) {
    let (mut run, _) = run_on_level(
        Mode::Play,
        level,
        Setup {
            props: true,
            posed_rigs: true,
            stage: Some(name),
            ..Setup::default()
        },
    );
    let (h, at) = run.placed(name);
    assert!(
        !run.vm.inert().contains(&h),
        "{name}: listed by the stage, not inert"
    );
    assert!(run.world.creatures().contains(&h), "{name}: walked");
    // Nothing of the stage's is written on its range, buffer or state: at
    // the load it is as the activity rule has it from where he starts.
    // 203's gunner is awake there: the level's start (AS.703, its intro's
    // walk-in) is 205 ft from him and his file's range is 250.
    let a = run.world.actor(h).unwrap();
    let d = (at - run.hero().pivot).length() * guacamole::script_engine::FEET_PER_METRE;
    let at_load = a.active;
    assert_eq!(
        at_load,
        guacamole::scripts::awake(a.range, a.buffer, false, Some(d)),
        "{name}: at the load {d:.0} ft from his start, range {}",
        a.range
    );
    run.world.skip_cutscenes = true;
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap_or_else(|e| panic!("{e}"));
    let facing = run.world.actor(h).unwrap().angles.facing;
    let f = facing.to_radians();
    let by = at + Vec3::new(f.sin(), 0.0, f.cos()) * BY;
    let mut woke = None;
    for k in 0..STALL {
        run.put(by, (facing + 180.0).rem_euclid(360.0));
        run.step_play().unwrap();
        if run.world.actor(h).is_some_and(|a| a.active) {
            woke = Some(k);
            break;
        }
    }
    let woke = woke.unwrap_or_else(|| panic!("{name} didn't wake in {STALL} steps by {by}"));
    // Its setup run on: what its scripts hang on it.
    for _ in 0..60 {
        run.put(by, (facing + 180.0).rem_euclid(360.0));
        run.step_play().unwrap();
    }
    let hung: Vec<String> = run
        .world
        .clones()
        .into_iter()
        .filter(|&c| !run.world.removed(c))
        .filter(|&c| {
            let a = run.world.actor(c).unwrap();
            run.world.node_owner(a.attach) == Some(h) || a.attach == guac_script::Value::Entity(h)
        })
        .map(|c| run.world.name(c))
        .collect();
    println!(
        "{level} {name}: woke {woke} steps after the set-down by {by}, behaviour {:?}, hung {hung:?}",
        run.vm.behaviour(run.placement(name))
    );
    (run, at_load, woke, hung)
}

/// openspec AS.680: 208's bomber and 203's gunner, whose files leave them
/// waking by range, wake by it once staged and stand in the stage's frame
/// (the bomber asleep until the set-down puts him in range, the gunner
/// awake from the load, his range holding 203's start since AS.703):
/// the camera framed on the creature (a 2 m sphere off its pivot, the
/// follow camera's picture) holds its pivot and its head height, in front;
/// its setup hangs its piece on it.
#[test]
#[ignore = "reads the Giants dump"]
fn a_staged_creature_wakes_by_its_range_and_stands_in_the_frame() {
    for (level, name, awake) in [
        ("level_208_ice", "Enemy_BombThrower(3)", false),
        ("level_203_the_machine", "Enemy_Troll_BFG(1)", true),
    ] {
        let (run, at_load, _, hung) = staged(level, name);
        assert_eq!(at_load, awake, "{name}: awake at the load");
        // What its setup hangs on it (AS.660's look): the bomber's bomb in
        // hand, the gunner's gun.
        assert!(!hung.is_empty(), "{name}: nothing hung on it");
        let (h, _) = run.placed(name);
        let a = run.world.actor(h).unwrap();
        assert!(!a.removed, "{name} still on");
        assert!(
            run.vm.behaviour(run.placement(name)).is_some(),
            "{name}: its script started"
        );
        let centre = a.position + Vec3::Y;
        let fr = frame(centre, 2.0, a.angles.facing, 0.0, 45.0, 16.0 / 9.0);
        for p in [a.position, a.position + Vec3::Y * 2.0, centre] {
            assert!(
                in_view(&fr, 45.0, 16.0 / 9.0, p),
                "{name}: {p} out of {fr:?}"
            );
        }
    }
}
