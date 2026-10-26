//! AH.23's disc tests: Eruptor's figure in the scripts over level 213,
//! headless (the shared run, [`common`]): his tag on the portal holding
//! the words AH.2's rows lay out, the disc's own save macros reading and
//! writing them, his health at the level they hold, the disc's own awards
//! reaching the words and through the TV's map a profile's row. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --release --test giants_figure -- --ignored --nocapture --test-threads=1`

mod common;

use std::time::Instant;

use common::*;
use guac_script::{Mode, Value};
use guac_server::instances::Caps;
use guacamole::figure::Values;
use guacamole::scripts;

/// A save param's structure as `init_setup`'s table made it (`pp_Exp`).
fn param(run: &mut Run, name: &str) -> Value {
    let g = run
        .vm
        .program()
        .global_named(name)
        .unwrap_or_else(|| panic!("no {name}"));
    run.vm.global(g).unwrap()
}

/// `Save - Get/Set Minion Save Val` called as the awards call it, from the
/// portal manager: reads `name`'s value off the figure's tag data, or with
/// `set` writes `val`; returns what its `val` holds after.
fn save_val(run: &mut Run, name: &str, set: Option<i32>) -> Value {
    let info = param(run, name);
    let data = run.figure.expect("joined");
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    let (_, out) = run
        .with_host(|vm, host| {
            vm.call_with(
                Some(pm),
                "Save - Get/Set Minion Save Val",
                &[
                    ("tag data", data),
                    ("save param info", info),
                    ("save param index in", Value::Int(0)),
                    ("set", Value::Int(i32::from(set.is_some()))),
                    ("val", Value::Int(set.unwrap_or(0))),
                ],
                &["val"],
                host,
            )
        })
        .unwrap_or_else(|h| panic!("{h}"));
    out[0].expect("a value")
}

/// The full health Eruptor's 290 comes to at `level` by combat's rule
/// (AH.16: `Round Number Up(290 × (1 + (level − 1) × 0.111))` in single
/// precision).
fn full_health(level: u32) -> i32 {
    let mult = 1.0f32 + (level as f32 - 1.0) * 0.111;
    (290.0f32 * mult + 0.99) as i32
}

/// The level a total reaches on AH.2's table.
fn level_of(xp: u32) -> u32 {
    let table = progression().level_xp;
    table.iter().filter(|&&t| t <= xp).count() as u32
}

/// AH.23's round trips on the disc's own macros: his figure joined holding
/// 0, 32,999, 33,000, 33,001 and 96,500 experience (across the two areas)
/// and gold at the clamp (65,000, whose low byte sets word 0's sign bit),
/// `Save - Get/Set Minion Save Val` reads each total back as the layout
/// laid it out, and its own writes, each across an area's edge, read back
/// through the layout; he joins
/// with the full health of the level his figure holds.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_scripts_read_and_write_his_figure_as_the_layout_lays_it_out() {
    println!("load before: {}", load());
    let started = Instant::now();
    // Each start written over by a total on the far side of an area's
    // edge, or across both.
    for (xp, to) in [
        (0, 33_001),
        (32_999, 96_500),
        (33_000, 32_999),
        (33_001, 33_000),
        (96_500, 1),
    ] {
        let (mut run, _) = run_on(Mode::Strict);
        run.step().unwrap_or_else(|h| panic!("{h}"));
        run.step().unwrap_or_else(|h| panic!("{h}"));
        let values = Values {
            xp,
            gold: 65_000,
            playtime_secs: 4_321,
        };
        run.join_with(values).unwrap_or_else(|h| panic!("{h}"));
        for _ in 0..10 {
            run.step().unwrap_or_else(|h| panic!("{xp}: {h}"));
        }
        let read = [
            save_val(&mut run, "pp_Exp", None),
            save_val(&mut run, "pp_Money", None),
            save_val(&mut run, "pp_CumulativeTime", None),
        ];
        println!("{xp}: the scripts read {read:?}");
        assert_eq!(
            read,
            [Value::Int(xp as i32), Value::Int(65_000), Value::Int(4_321)],
            "{xp}"
        );
        assert_eq!(run.figure_values(), Some(values), "{xp}: unchanged");
        // The disc's own `Save - Set up Tag Data Fields` sized it over the
        // portal's regions: AH.11's live 44 words.
        let data = run.figure.unwrap();
        let area = run.vm.field(data, "save data area").unwrap();
        assert_eq!(run.vm.set(area).map(<[Value]>::len), Some(44), "{xp}");
        let level = level_of(xp);
        let health = run.health().map(|h| h as i32);
        println!("{xp}: level {level}, health {health:?}");
        assert_eq!(health, Some(full_health(level)), "{xp}: level {level}");
        // The scripts' own writes, read back through the layout.
        save_val(&mut run, "pp_Exp", Some(to as i32));
        save_val(&mut run, "pp_Money", Some(64_999));
        let back = run.figure_values().unwrap();
        println!("{xp}: wrote {to} and 64,999, read {back:?}");
        assert_eq!(
            back,
            Values {
                xp: to,
                gold: 64_999,
                playtime_secs: 4_321,
            },
            "{xp}"
        );
    }
    println!(
        "{:.1} s; load after: {}",
        started.elapsed().as_secs_f32(),
        load()
    );
}

/// The disc's own `Game - Award Experience` on the joined hero, as the
/// player loop pays an orb: `amount` added to his figure's words.
fn award_experience(run: &mut Run, amount: i32) {
    assert!(run.minion.is_some(), "joined");
    let hero = Value::Entity(HERO);
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    run.with_host(|vm, host| {
        vm.call_with(
            Some(pm),
            "Game - Award Experience",
            &[("minion", hero), ("amount", Value::Int(amount))],
            &[],
            host,
        )
    })
    .unwrap_or_else(|h| panic!("{h}"));
}

/// The disc's own `Game - Award Money` for the first player.
fn award_money(run: &mut Run, amount: i32) {
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    run.with_host(|vm, host| {
        vm.call_with(
            Some(pm),
            "Game - Award Money",
            &[("amount", Value::Int(amount))],
            &[],
            host,
        )
    })
    .unwrap_or_else(|h| panic!("{h}"));
}

/// What `player stat tracking` holds for the first player: the experience
/// and gold the awards counted this session.
fn tracked(run: &Run) -> (i32, i32) {
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    let players = run.vm.local(pm, "player data").unwrap();
    let first = run.vm.set(players).unwrap()[0];
    let stats = run
        .vm
        .field(first, "player stat tracking")
        .map(|v| run.vm.value(v))
        .unwrap();
    let get = |n| {
        run.vm
            .field(stats, n)
            .and_then(|v| run.vm.value(v).as_i32())
            .unwrap_or(0)
    };
    (get("exp collected"), get("loot collected"))
}

/// Bob's Eruptor beats a Chompy (figure-progress): the disc's own awards
/// for a Chompy's four 1-point orbs and a coin of 2 land in his figure's
/// words as the scripts count them, and the row the TV's map writes holds
/// them; the thousandth point (999 + 1) takes him to level 2 in the words,
/// in the scripts' next target and in the row.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_discs_awards_reach_his_figure_and_the_row() {
    println!("load before: {}", load());
    let (mut run, _) = joined(Mode::Strict);
    let before = run.figure_values().unwrap();
    assert_eq!(before, Values::default(), "a fresh figure");
    let mut sent = Vec::new();
    for _ in 0..4 {
        award_experience(&mut run, 1);
        run.step().unwrap_or_else(|h| panic!("{h}"));
        sent.push(run.figure_values().unwrap());
    }
    award_money(&mut run, 2);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    let after = run.figure_values().unwrap();
    sent.push(after);
    let (exp, loot) = tracked(&run);
    println!("figure {after:?}; the scripts counted {exp} experience and {loot} gold");
    assert_eq!((after.xp, after.gold), (4, 2));
    assert_eq!((exp, loot), (4, 2), "the scripts' own count");
    let row = row_after(&sent);
    println!("the row {row:?}");
    assert_eq!((row.xp, row.gold), (4, 2));

    // The thousandth point.
    let (mut run, _) = run_on(Mode::Strict);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join_with(Values {
        xp: 999,
        ..Values::default()
    })
    .unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    let target = |run: &Run| {
        let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
        let players = run.vm.local(pm, "player data").unwrap();
        let first = run.vm.set(players).unwrap()[0];
        run.vm
            .field(first, "exp target")
            .map(|v| run.vm.value(run.vm.value(v)))
    };
    println!("at 999 his next target {:?}", target(&run));
    award_experience(&mut run, 1);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    let v = run.figure_values().unwrap();
    println!("after 1 more: {v:?}, next target {:?}", target(&run));
    assert_eq!(v.xp, 1_000);
    assert_eq!(target(&run), Some(Value::Int(2_200)), "the next level's");
    let row = row_after(&[v]);
    let p = progression();
    let level = guac_server::progress::Progression::from(&p).level_for(row.xp);
    assert_eq!((row.xp, level), (1_000, 2));
    assert_eq!(
        guac_server::progress::Progression::from(&p).caps(),
        Caps {
            xp: 96_500,
            gold: 65_000
        }
    );
    println!("load after: {}", load());
}

/// AH.220: the orb that takes Bob's Eruptor to the cap (96,499 and 1 more:
/// level 15) runs the level-15 accolade, which unlocks its achievement
/// through `System - Achievements - Complete Achievement by ID`. The host
/// keeps the id, nothing is awarded, and the portal manager runs on, its
/// camera's stretch with it: strict, so any stop halts the test.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_caps_achievement_is_kept_and_the_portal_manager_runs_on() {
    println!("load before: {}", load());
    let (mut run, _) = run_on(Mode::Strict);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join_with(Values {
        xp: 96_499,
        ..Values::default()
    })
    .unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..60 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    assert!(run.world.achievements.is_empty(), "none before the cap");
    award_experience(&mut run, 1);
    for k in 0..300 {
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
    }
    let v = run.figure_values().unwrap();
    println!(
        "at the cap: {v:?}; the achievements unlocked {:?}",
        run.world.achievements
    );
    assert_eq!(v.xp, 96_500);
    assert_eq!(run.world.achievements.len(), 1, "level 15's, once");
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    assert_eq!(run.vm.stopped(pm), None, "the portal manager runs on");
    assert!(
        run.player_loop.camera_stopped(&run.vm, pm).is_none(),
        "the camera's stretch with it"
    );
    println!("load after: {}", load());
}
