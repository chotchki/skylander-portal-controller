//! A level past chapter 1 on the TV with a hero in it (openspec AS.51) and a
//! level change with the hero walking (openspec AS.49), on AH.24's synthetic
//! part and the made-up level, copied to another level's name.

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use guacamole::scripts::ScriptRuntime;

use crate::hero_scripts::ScriptSource;
use crate::level::level_channel;
use crate::play::{Mode, Play};
use crate::test_support::level::{lights, write_level, write_level_as};
use crate::test_support::play::{
    ERUPTOR, join, joined_eruptor, level_state, mode, place, play_app, until, write_attack_parts,
};
use crate::test_support::test_pad;

/// The level the copies are named for: chapter 1's end's next.
const OTHER: &str = "level_214_tutotial_2";

/// The run's level, and whether its listing is chapter 1's.
fn run_level(app: &App) -> Option<(String, bool)> {
    let s = app.world().get_resource::<ScriptSource>()?;
    Some((s.level().to_owned(), s.chapter_1()))
}

/// AS.51: a level past chapter 1 links its own script part and installs its
/// run, its listing not chapter 1's, and a hero placed comes in and joins
/// that run, as on chapter 1.
#[test]
fn a_hero_joins_a_level_past_chapter_1s_own_run() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, tx) = play_app(tmp.path());
    write_attack_parts(tmp.path());
    write_level(tmp.path(), &lights());
    tx.send(write_level_as(tmp.path(), OTHER)).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    assert_eq!(run_level(&app), Some((OTHER.to_owned(), false)));
    place(&mut app, 0, ERUPTOR, "Eruptor");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    assert_eq!(run_level(&app), Some((OTHER.to_owned(), false)));
}

/// AS.49: the level left while the hero walks it (another level's source
/// coming, as a script's level change or an ask brings it) takes the first
/// level's run and voices down with him in it, and the portal's figures come
/// into the next as they stood: the same figure on the same slot is the
/// hero again, joined to the next level's own run.
#[test]
fn a_level_left_with_the_hero_in_it_brings_the_portals_figures_into_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    let first = app.world().resource::<ScriptRuntime>().vm.program_arc();
    assert_eq!(
        run_level(&app),
        Some((crate::test_support::level::LEVEL.to_owned(), true))
    );
    let before = app
        .world()
        .resource::<Play>()
        .hero_placement()
        .map(|(_, slot, p)| (slot, p.figure_id.clone()))
        .expect("in the level");
    let cards = app
        .world()
        .resource::<crate::cards::PortalCards>()
        .slots
        .clone();
    let voice = app
        .world_mut()
        .spawn(crate::sound_player::Heard::of_run(1))
        .id();
    let (tx, feed) = level_channel();
    app.insert_resource(feed);
    tx.send(write_level_as(tmp.path(), OTHER)).unwrap();
    app.update();
    assert!(
        app.world().get_entity(voice).is_err(),
        "the first run's voice went"
    );
    assert!(
        app.world()
            .get_resource::<ScriptRuntime>()
            .is_none_or(|rt| !std::sync::Arc::ptr_eq(&rt.vm.program_arc(), &first)),
        "the first level's run went with it"
    );
    until(&mut app, "the hero in the next level, joined", |a| {
        run_level(a).is_some_and(|(l, _)| l == OTHER) && join(a).as_deref() == Some("joined")
    });
    let after = app
        .world()
        .resource::<Play>()
        .hero_placement()
        .map(|(_, slot, p)| (slot, p.figure_id.clone()));
    assert_eq!(after, Some(before), "the same figure, the same slot");
    assert_eq!(
        app.world().resource::<crate::cards::PortalCards>().slots,
        cards,
        "the portal as it stood"
    );
    assert_eq!(mode(&app), Mode::Level);
}

/// AS.49: while the next level is asked and read (waiting for the fetch,
/// then its parts read), the level the hero walks is held (the pads move no
/// one, no step runs); once it's handed to the load the hold is the
/// portal's again.
#[test]
fn the_level_holds_while_the_next_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = joined_eruptor(tmp.path());
    assert_eq!(mode(&app), Mode::Level);
    app.insert_resource(crate::level_choice::LevelChoice::waiting_for_tests(OTHER));
    app.update();
    assert_eq!(mode(&app), Mode::Held);
    // Its fetch running, the parts being read (AS.15's review: the hold
    // covered the ask's wait alone in the tests).
    app.insert_resource(crate::level_choice::LevelChoice::running_for_tests(OTHER));
    app.update();
    assert_eq!(mode(&app), Mode::Held, "held while its parts are read");
    app.insert_resource(crate::level_choice::LevelChoice::default());
    app.update();
    assert_eq!(mode(&app), Mode::Level);
}
