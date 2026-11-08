//! A scene that keeps the hero's figure on the portal (openspec AS.112;
//! level-scene's A scene keeps the hero's figure), on AH.24's synthetic part
//! with the lock written as `Game - Cutscene` leaves it: `Cutscene State` and
//! a toy on `portal control data`'s `need keep`.

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use guac_script::Value;
use guac_server::progress::ProgressSink;
use guacamole::scripts::ScriptRuntime;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::{FigureProgress, PlacementOwner};

use crate::cards::PortalChanged;
use crate::driver::PortalChange;
use crate::hero_scripts::HeroRun;
use crate::hero_scripts::Join;
use crate::play::{Alert, DEFEAT_BACKSTOP, HELD_SWAP_BACKSTOP, Mode, Play, same_toy};
use crate::progress::{Earning, ProgressOut};
use crate::test_support::level::{lights, write_level};
use crate::test_support::play::{
    ERUPTOR, ONE, clear, join, joined_eruptor, level_state, mode, place, play_app, until,
    write_attack_parts, write_figure_rows,
};
use crate::test_support::test_pad;
use crate::test_support::turntable::{placed, set};

fn alert(app: &App) -> Option<Alert> {
    app.world().resource::<Play>().alert()
}

fn lifted(app: &App) -> bool {
    app.world().resource::<Play>().lifted()
}

fn hero(app: &App) -> Option<Entity> {
    app.world().resource::<Play>().hero()
}

fn clock(app: &App) -> std::time::Duration {
    app.world().resource::<Time<Virtual>>().elapsed()
}

/// The run under a scene's lock, as `Game - Cutscene` leaves it starting one.
fn lock(app: &mut App) {
    let vm = &mut app
        .world_mut()
        .resource_mut::<ScriptRuntime>()
        .into_inner()
        .vm;
    let g = vm.program().global_named("Cutscene State").unwrap();
    assert!(vm.set_global(g, Value::Int(1)));
    let g = vm.program().global_named("portal control data").unwrap();
    let pcd = vm.global(g).unwrap();
    let requirements = vm.field(pcd, "requirements").unwrap();
    let need_keep = vm.field(requirements, "need keep").unwrap();
    assert!(vm.set_add(need_keep, Value::Int(9)));
    assert!(guacamole::scripts::scene_keeps_figures(vm));
}

/// The scene over.
fn unlock(app: &mut App) {
    let vm = &mut app
        .world_mut()
        .resource_mut::<ScriptRuntime>()
        .into_inner()
        .vm;
    let g = vm.program().global_named("Cutscene State").unwrap();
    assert!(vm.set_global(g, Value::Int(0)));
    assert!(!guacamole::scripts::scene_keeps_figures(vm));
}

/// Eruptor's figure as `profile`'s, generation `generation`.
fn owned(slot: u8, profile: &str, generation: i64) -> PortalChange {
    let mut change = placed(slot, ERUPTOR, "Eruptor");
    if let PortalChange::Placed { placement, .. } = &mut change {
        placement.owner = Some(PlacementOwner {
            profile_id: profile.to_owned(),
            initial: "A".into(),
            color: "#e63946".into(),
            progress: FigureProgress {
                generation,
                ..FigureProgress::default()
            },
        });
    }
    change
}

fn put(app: &mut App, change: PortalChange) {
    set(app, [change.clone()]);
    app.world_mut().write_message(PortalChanged(change));
}

/// Another character that walks: Eruptor's figure as Bob's, not the hero's
/// own.
fn bobs(slot: u8) -> PortalChange {
    owned(slot, "bob", 0)
}

/// The hero's health at `health`, as a bite leaves it.
fn set_health(app: &mut App, health: i32) {
    let e = hero(app).expect("in the level");
    let Join::Joined(minion) = app.world().entity(e).get::<HeroRun>().unwrap().join else {
        panic!("not joined");
    };
    let mut rt = app.world_mut().resource_mut::<ScriptRuntime>();
    assert!(guacamole::scripts::set_health(&mut rt.vm, minion, health));
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// The figure lifted under the lock: he stays, joined, the level holds
/// behind the put-back alert with the clock stopped, nothing of his taken
/// off. Back in another slot, the alert goes, the clock runs, his run reads
/// the new slot and it's the same hero: no swap happened.
#[test]
fn a_figure_lifted_under_a_scenes_lock_holds_him_until_its_back() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    let first = hero(&app).unwrap();
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    assert_eq!(mode(&app), Mode::Held);
    assert!(lifted(&app));
    assert_eq!(hero(&app), Some(first), "he stays");
    app.update();
    let held = clock(&app);
    frames(&mut app, 30);
    assert_eq!(clock(&app), held, "the scene frozen");
    assert_eq!(hero(&app), Some(first));
    assert_eq!(
        app.world().resource::<Play>().hero_slot(),
        None,
        "the server hears no hero's slot while it stands empty"
    );
    place(&mut app, 4, ERUPTOR, "Eruptor");
    until(&mut app, "the level", |a| mode(a) == Mode::Level);
    assert_eq!(alert(&app), None);
    assert!(!lifted(&app));
    assert_eq!(hero(&app), Some(first), "the same hero, no swap");
    frames(&mut app, 3);
    assert!(clock(&app) > held, "the scene runs on");
    let run = app.world().entity(first).get::<HeroRun>().unwrap();
    assert_eq!(
        run.slot,
        SlotIndex::new(4).unwrap(),
        "his run on the new slot"
    );
    assert_eq!(
        app.world().resource::<Play>().hero_slot(),
        Some(SlotIndex::new(4).unwrap())
    );
    // The scene over, his figure lifted is a swap again.
    unlock(&mut app);
    clear(&mut app, 4);
    until(&mut app, "the call", |a| {
        alert(a) == Some(Alert::PlaceSkylander)
    });
    assert_eq!(hero(&app), None);
}

/// What a scene pays him with his figure off the portal is his own, with
/// Bob's figure stood in his slot: the step's read-back goes to Alice's row
/// and leaves the slot's new occupant as he came.
#[test]
fn what_a_scene_pays_him_off_the_portal_stays_on_his_own_figure() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, tx) = play_app(tmp.path());
    write_attack_parts(tmp.path());
    write_figure_rows(tmp.path());
    let sink = ProgressSink::new();
    app.insert_resource(ProgressOut(sink.clone()));
    tx.send(write_level(tmp.path(), &lights())).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    put(&mut app, owned(0, "alice", 0));
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    put(&mut app, bobs(0));
    until(&mut app, "the scene on", |a| mode(a) == Mode::Level);
    assert!(lifted(&app));
    let _ = sink.waiting();
    test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
    frames(&mut app, 3);
    let waiting = sink.waiting();
    assert!(
        waiting
            .iter()
            .any(|(profile, _, sent)| profile == "alice" && sent.progress.xp == 1),
        "the award on his row: {waiting:?}"
    );
    let earning = app.world().resource::<Earning>();
    assert_eq!(
        earning.slots[0].as_ref().map(|e| e.progress.xp),
        Some(0),
        "Bob's earned nothing"
    );
    // The HUD shows his own, not Bob's slot nor the row he came in with.
    let shown = app.world().resource::<crate::hud::Hud>().shown.clone();
    assert_eq!(shown.map(|v| v.xp), Some(1), "his own on the HUD");
}

/// Nothing changes with no lock: a scene that lets switching through (the
/// cutscene system's own, Norticus') or none takes him off at once, and a
/// list with no scene isn't a lock.
#[test]
fn a_scene_that_doesnt_keep_him_takes_him_off_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    lock(&mut app);
    unlock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the call", |a| {
        alert(a) == Some(Alert::PlaceSkylander)
    });
    assert!(!lifted(&app));
    assert_eq!(hero(&app), None);
}

/// Another character placed under the alert lets the scene run on with him
/// in it, then comes in once the scene lets go (ours: the original keeps
/// asking for the toy, which a phone may not be able to place). Taken off
/// again before that, the alert is back: the scene can't end with no one on
/// the portal.
#[test]
fn another_character_under_the_alert_comes_in_as_the_scene_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    let first = hero(&app).unwrap();
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    // One who can't walk can't answer it.
    place(&mut app, 3, ONE, "One");
    frames(&mut app, 10);
    assert_eq!(alert(&app), Some(Alert::PutBack), "One doesn't walk");
    clear(&mut app, 3);
    // Placed over the slot he left: another character all the same.
    put(&mut app, bobs(0));
    until(&mut app, "the scene running on", |a| mode(a) == Mode::Level);
    assert_eq!(alert(&app), None);
    assert_eq!(hero(&app), Some(first), "he's the scene's yet");
    assert!(lifted(&app));
    let run = clock(&app);
    frames(&mut app, 3);
    assert!(clock(&app) > run, "the scene runs on");
    clear(&mut app, 0);
    until(&mut app, "the put-back again", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    assert_eq!(hero(&app), Some(first));
    put(&mut app, bobs(2));
    until(&mut app, "the scene running on", |a| mode(a) == Mode::Level);
    unlock(&mut app);
    // The next frame: the backstop is a long way off.
    frames(&mut app, 2);
    assert!(!lifted(&app), "the scene let go");
    until(&mut app, "the swap", |a| {
        hero(a).is_some_and(|h| h != first)
    });
    assert_eq!(
        app.world().resource::<Play>().hero_slot(),
        Some(SlotIndex::new(2).unwrap())
    );
}

/// The soft lock, closed: a scene that never lets go (a script stopped in
/// it) with a swap held lets him off at the backstop, on the level's clock.
/// Behind the alert that clock stands still, so a kid's chance to put him
/// back is never spent.
#[test]
fn a_scene_that_never_ends_lets_a_held_swap_go_at_the_backstop() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    let first = hero(&app).unwrap();
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    // Behind the alert the level's clock stands: past the backstop on the
    // wall clock, nothing has let go of him.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(1),
    ));
    frames(&mut app, 70);
    assert!(lifted(&app), "held past the backstop's wall time");
    assert_eq!(alert(&app), Some(Alert::PutBack));
    assert_eq!(hero(&app), Some(first));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ));
    put(&mut app, bobs(2));
    until(&mut app, "the scene running on", |a| mode(a) == Mode::Level);
    assert_eq!(hero(&app), Some(first));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .advance_by(HELD_SWAP_BACKSTOP);
    until(&mut app, "the swap", |a| {
        hero(a).is_some_and(|h| h != first)
    });
    assert!(!lifted(&app));
}

/// A hero dying isn't kept: the original finalizes his defeat as the scene
/// starts. Lifted while dying he leaves at once, beaten; dying with a swap
/// held, he leaves by his defeat and the swap's over with him.
#[test]
fn a_dying_hero_is_no_scenes_to_keep() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    lock(&mut app);
    set_health(&mut app, 0);
    until(&mut app, "dying", |a| a.world().resource::<Play>().dying());
    clear(&mut app, 0);
    until(&mut app, "out", |a| hero(a).is_none());
    assert!(!lifted(&app));
    assert_eq!(app.world().resource::<Play>().beaten().len(), 1);

    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    put(&mut app, bobs(2));
    until(&mut app, "the scene running on", |a| mode(a) == Mode::Level);
    set_health(&mut app, 0);
    until(&mut app, "dying", |a| a.world().resource::<Play>().dying());
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .advance_by(DEFEAT_BACKSTOP);
    until(&mut app, "beaten", |a| {
        !a.world().resource::<Play>().beaten().is_empty()
    });
    assert!(!lifted(&app), "the swap's over with him");
}

/// A level end under the hold ends it: the TV leaves the level, nothing of
/// the lift is left for the next entry.
#[test]
fn leaving_the_level_ends_the_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    app.world_mut().write_message(crate::play::RestartLevel);
    until(&mut app, "the level's end", |a| {
        !a.world().resource::<Play>().started()
    });
    assert!(!lifted(&app));
    assert_eq!(alert(&app), None);
    assert!(app.world().resource::<Play>().lifted_name().is_none());
}

/// The same figure under another profile, or after an edit or a reset, isn't
/// the toy he stood as: his words aren't its. It waits as another character
/// does; his own put back ends the lift.
#[test]
fn only_the_toy_he_stood_as_is_him_put_back() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    let first = hero(&app).unwrap();
    lock(&mut app);
    clear(&mut app, 0);
    until(&mut app, "the put-back", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    put(&mut app, bobs(1));
    until(&mut app, "the scene on", |a| mode(a) == Mode::Level);
    assert!(lifted(&app), "Bob's Eruptor isn't his");
    clear(&mut app, 1);
    until(&mut app, "the put-back again", |a| {
        alert(a) == Some(Alert::PutBack)
    });
    place(&mut app, 1, ERUPTOR, "Eruptor");
    until(&mut app, "him back", |a| !lifted(a));
    assert_eq!(hero(&app), Some(first));

    let figure = |change: PortalChange| match change {
        PortalChange::Placed { placement, .. } => placement,
        PortalChange::Cleared { .. } => unreachable!(),
    };
    let alice = figure(owned(0, "alice", 3));
    assert!(same_toy(&alice, &figure(owned(5, "alice", 3))), "any slot");
    assert!(
        !same_toy(&alice, &figure(owned(0, "alice", 4))),
        "reset since"
    );
    assert!(!same_toy(&alice, &figure(owned(0, "bob", 3))), "another's");
    assert!(
        !same_toy(&alice, &figure(placed(0, ONE, "One"))),
        "another toy"
    );
}
