use std::collections::HashMap;
use std::sync::Arc;

use guac_formats::igz::Igz;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph};
use guac_script::{Config, Mode, Obj, Program, Role, Unit, Value, Whitelist};

use super::*;
use crate::scripts::{Placed, headless_objects};

const PIRATE_PATH: &str =
    "C:/tfb/Content/Levels/Includes/GameElement_CardGame_Sky2/Scripts/CardPirate_Template.ai";
const STARTUP_PATH: &str =
    "C:/tfb/Content/Levels/Includes/CardGame_Cards/scripts/CardArt_Streamed_Startup.ai";

/// The requests the disc's scripts write, as their text reads: each stream
/// by its folder and stem (any case), the board's with what follows its
/// stem; a stone's own archive, a stem with more name after it, the join's
/// character stream and the init chain's are none of them.
#[test]
fn a_request_names_its_stream_by_its_stem() {
    assert_eq!(asked("misc/CardGame_2dSprites_Streamed"), Some(SPRITES));
    assert_eq!(
        asked("misc/UI_CardGame_HandSelect_Streaming"),
        Some(HAND_SELECT)
    );
    assert_eq!(asked("character/CardGame_Streaming"), Some(BOARD));
    assert_eq!(
        asked("character\\cardgame_streaming;cardgame_card_005"),
        Some(BOARD)
    );
    assert_eq!(asked("CardGame_2dSprites_Streamed"), Some(SPRITES));
    for none in [
        "misc/cardgame_card_005",
        "misc/cardgame_sprite_005",
        "character/CardGame_Streaming2",
        "character/CardGame_Streaming_old",
        "character/",
        "debug/debug",
        "",
    ] {
        assert_eq!(asked(none), None, "{none:?}");
    }
}

/// A level places a pirate when one runs from its own file: asleep, a
/// template's own placement, another unit's, or another script is none.
#[test]
fn a_level_with_a_running_pirate_places_one() {
    let files = |p: Vec<Placed>| Files {
        placed: p,
        ..Files::default()
    };
    let at = |unit| Obj { unit, index: 7 };
    let running = files(vec![Placed::named(
        at(5),
        "Dreadbeard",
        Some(PIRATE_PATH),
        4,
    )]);
    assert!(places_a_pirate(&running, 5));
    assert!(!places_a_pirate(&running, 4), "another unit's");
    let back = PIRATE_PATH.replace('/', "\\").to_ascii_lowercase();
    assert!(places_a_pirate(
        &files(vec![Placed::named(at(5), "Fangs", Some(&back), 4)]),
        5
    ));
    for (state, script) in [(5, Some(PIRATE_PATH)), (4, Some(STARTUP_PATH)), (4, None)] {
        let f = files(vec![Placed::named(at(5), "Dreadbeard", script, state)]);
        assert!(!places_a_pirate(&f, 5), "{state} {script:?}");
    }
}

/// A request is kept to join only for a stream the run links.
#[test]
fn only_a_linked_stream_is_kept_to_join() {
    let mut s = CardStreams::linked(&[SPRITES], &[8]);
    assert!(!s.take_request(Some("misc/UI_CardGame_HandSelect_Streaming")));
    assert!(!s.take_request(Some("debug/debug")));
    assert!(!s.take_request(None));
    assert!(s.pending.is_empty());
    assert!(s.take_request(Some("misc/CardGame_2dSprites_Streamed")));
    assert_eq!(s.pending, [SPRITES]);
    let fresh = CardStreams::fresh(&s);
    assert_eq!((fresh.units, fresh.pending.len()), (vec![(SPRITES, 8)], 0));
}

/// The stream's unit: one script whose STARTUP writes `ran` 1, placed
/// running as the startup and asleep as the helper beside it.
fn stream() -> (Arc<Program>, Files, HashMap<Obj, u64>) {
    let mut b = IgzBuilder::new();
    {
        let mut w = Writer::new(&mut b);
        let w = &mut w;
        let ran = w.variable("ran", "ValueInfo", Some("IntMeasurement"));
        let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
        let (start, _) = w.open(Class::StartUp, "STARTUP");
        let one = w.int(1);
        w.set(&[El::Local(ran)], one);
        w.end_block(start);
        let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
        w.end_block(shut);
        w.end_block(pre);
        w.end(STARTUP_PATH);
    }
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let graph = Graph::read(&igz).unwrap();
    let unit = Unit {
        name: "stream",
        role: Role::Other,
        igz: &igz,
        graph: &graph,
    };
    let program = Arc::new(Program::link(&[unit]).unwrap());
    let files = Files {
        placed: vec![
            Placed::named(STARTUP, "2dSpritesStream_Startup", Some(STARTUP_PATH), 4),
            Placed::named(HELPER, "CardGame_Helper_Macros", Some(STARTUP_PATH), 5),
        ],
        ..Files::default()
    };
    let handles = HashMap::from([(STARTUP, 0x40), (HELPER, 0x41)]);
    (program, files, handles)
}

const STARTUP: Obj = Obj { unit: 0, index: 2 };
const HELPER: Obj = Obj { unit: 0, index: 3 };

/// An asked stream's running placement joins the run, its STARTUP run on
/// the next step, once; its asleep helper stays out; asked again while it
/// runs it isn't added twice, and asked once it's gone it joins again. A
/// stream nobody asked for joins nothing.
#[test]
fn an_asked_stream_joins_its_running_placements_once_while_they_run() {
    let (program, files, handles) = stream();
    let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
    world.card_streams = CardStreams::linked(&[SPRITES], &[0]);
    let mut config = Config::new(Mode::Strict);
    config.whitelist = Whitelist::only(&[], &["CardArt_Streamed_Startup.ai"]);
    let mut vm = Vm::new(program, config);
    assert!(join_asked(&mut vm, &mut world).is_empty(), "nothing asked");
    assert_eq!(vm.placement_of(0x40), None);

    assert!(
        world
            .card_streams
            .take_request(Some("misc/CardGame_2dSprites_Streamed"))
    );
    let added = join_asked(&mut vm, &mut world);
    assert_eq!(added.len(), 1);
    assert_eq!(vm.placement_of(0x40), Some(added[0]));
    assert_eq!(vm.placement_of(0x41), None, "the asleep helper stays out");
    assert_eq!(world.card_streams.joined, [(SPRITES, 1)]);
    let mut generator = guac_script::Generator::seeded(1);
    let mut engine = crate::script_engine::NoEngine;
    let mut spawn = crate::scripts::headless_spawner(0x1000);
    vm.step(&mut crate::scripts::WorldHost {
        world: &mut world,
        engine: &mut engine,
        generator: &mut generator,
        spawn: &mut spawn,
    })
    .unwrap();
    let ran = |vm: &Vm| vm.local(added[0], "ran").map(|v| vm.value(v));
    assert_eq!(ran(&vm), Some(Value::Int(1)), "its STARTUP ran");

    assert!(
        world
            .card_streams
            .take_request(Some("misc/CardGame_2dSprites_Streamed"))
    );
    assert!(
        join_asked(&mut vm, &mut world).is_empty(),
        "running: not twice"
    );
    assert!(vm.remove(0x40));
    assert!(
        world
            .card_streams
            .take_request(Some("misc/CardGame_2dSprites_Streamed"))
    );
    assert_eq!(join_asked(&mut vm, &mut world).len(), 1, "gone: again");
    assert_eq!(
        world.card_streams.joined,
        [(SPRITES, 1), (SPRITES, 0), (SPRITES, 1)]
    );
}

/// The streams' running placements are listed and nothing of the level's
/// own changes: one with a script of its own by its script, one sharing its
/// script with the level's by its name, one sharing both left off (a hub's
/// `Menu_MoreInfo`), an asleep one left off; the level's placements read as
/// they did without the streams.
#[test]
fn the_streams_list_their_own_and_nothing_of_the_levels() {
    let at = |unit, index| Obj { unit, index };
    let (own, shared, more) = ("C:/a/Starter.ai", "C:/a/Helper.ai", "C:/a/Menu_MoreInfo.ai");
    let level = [
        Placed::named(at(5, 1), "Menu_MoreInfo", Some(more), 4),
        Placed::named(at(5, 2), "Helpers", Some(shared), 5),
        Placed::named(at(5, 3), "Dreadbeard", Some(PIRATE_PATH), 4),
    ];
    let streams = [
        Placed::named(at(8, 1), "StartupMenu", Some(own), 4),
        Placed::named(at(6, 1), "2dSpritesStream_Startup", Some(shared), 4),
        Placed::named(at(8, 2), "Menu_MoreInfo", Some(more), 4),
        Placed::named(at(7, 1), "Asleep", Some("C:/a/Sleeper.ai"), 5),
    ];
    let files = Files {
        placed: level.iter().chain(&streams).cloned().collect(),
        ..Files::default()
    };
    let mut base = Config::new(Mode::Play);
    base.whitelist = Whitelist::only(&[], &["CardPirate_Template.ai"]);
    let with = with_streams(base.clone(), &files, &[6, 7, 8]);
    for p in &level {
        let s = p.script.as_deref().unwrap();
        assert_eq!(
            with.whitelist.lists(&p.name, s),
            base.whitelist.lists(&p.name, s),
            "{}",
            p.name
        );
    }
    let listed: Vec<bool> = streams
        .iter()
        .map(|p| with.whitelist.lists(&p.name, p.script.as_deref().unwrap()))
        .collect();
    assert_eq!(listed, [true, true, false, false]);
    let Whitelist::Only {
        placements,
        scripts,
        ..
    } = &with.whitelist
    else {
        unreachable!()
    };
    assert_eq!(placements, &["2dSpritesStream_Startup".to_owned()]);
    assert_eq!(
        scripts,
        &["CardPirate_Template.ai".to_owned(), own.to_owned()]
    );
}
