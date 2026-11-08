//! The ship's parts at the hub (openspec AS.157), headless with guacamole's
//! host on the TV's listing, a hub stage's ship part streams linked after
//! it as the TV's load links them, the story save answered as the TV
//! answers it. Ignored by default; they read `GUAC_GIANTS_DUMP` (process
//! env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_ship_parts -- --ignored --nocapture --test-threads=1`

use guac_motion::seam::Vec3;
use guac_script::{Handle, Mode};
use guac_server::saves::SaveValues;
use guacamole::ship_parts::{self, Equipped, ShipReport};
use guacamole::story_save::{self, Session};

mod common;

use common::*;

const LEVEL_214: &str = "level_214_tutotial_2";
const HUB: &str = "level_hub_2_stage_1";
const HOST: &str = "01HOSTPROFILE";
/// The save's params holding each part's bits, by id.
const COLLECTED: &str = "collection - legendary treasure";
const EQUIPPED: &str = "collection - legendary treasure equipped";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        streams: true,
        ..Setup::default()
    }
}

/// A run with its story save answered and a store stand-in writing at
/// once, the rows kept.
struct Story {
    run: Run,
    session: Session,
    rows: Vec<(String, SaveValues)>,
    level: &'static str,
    stops: Vec<String>,
}

impl Story {
    /// The level from its start, its hero joined and settled.
    fn joined(level: &'static str, session: Session) -> Self {
        Self::joined_with(level, session, setup())
    }

    /// [`Story::joined`] on `setup`.
    fn joined_with(level: &'static str, session: Session, setup: Setup) -> Self {
        let (run, _found) = run_on_level(Mode::Play, level, setup);
        let mut s = Self {
            run,
            session,
            rows: Vec::new(),
            level,
            stops: Vec::new(),
        };
        for _ in 0..2 {
            s.step();
        }
        s.run.join().unwrap_or_else(|h| panic!("{h}"));
        if level == LEVEL_214 {
            let stops = skip_214s_intro(&mut s.run);
            s.stops.extend(stops.iter().map(ToString::to_string));
        }
        for _ in 0..120 {
            s.step();
        }
        s
    }

    fn step(&mut self) {
        let stops = self.run.step_play().unwrap();
        self.stops.extend(stops.iter().map(ToString::to_string));
        let (run, session) = (&mut self.run, &mut self.session);
        {
            let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
            let mut engine = guacamole::script_engine::NoEngine;
            let mut host = guacamole::scripts::WorldHost {
                world: &mut run.world,
                engine: &mut engine,
                generator: &mut run.generator,
                spawn: &mut spawn,
            };
            story_save::serve(&mut run.vm, &mut host, session, self.level)
                .unwrap_or_else(|h| panic!("{h}"));
        }
        if let Some((_, layout, values)) = session.take_write() {
            self.rows.push((layout, values));
            session.written(&mut run.world.save, Ok(()));
        }
    }

    fn report(&self) -> ShipReport {
        ship_parts::report(&self.run.vm, &self.run.world)
    }

    /// Each part's bits as the save's accessor reads them now.
    fn parts(&mut self) -> Vec<ship_parts::Part> {
        self.run
            .with_host(|vm, host| ship_parts::parts(vm, host).unwrap())
    }

    fn equipped(&mut self) -> Vec<i32> {
        self.parts()
            .iter()
            .filter(|p| p.equipped)
            .map(|p| p.id)
            .collect()
    }

    fn removed(&self, name: &str) -> bool {
        self.run.world.removed(self.run.placed(name).0)
    }

    fn controller(&self) -> Option<String> {
        let h = self.run.placed("_Utilities_Controller").0;
        self.run
            .vm
            .placement_of(h)
            .and_then(|id| self.run.vm.behaviour(id))
            .map(str::to_owned)
    }

    /// Steps until the controller has hung its parts.
    fn until_hung(&mut self) {
        let start = self.run.steps;
        while self.controller().as_deref() != Some("main") {
            assert!(
                self.run.steps < start + 300,
                "the controller stays in {:?}: {:?}",
                self.controller(),
                self.report()
            );
            self.step();
        }
        for _ in 0..10 {
            self.step();
        }
    }
}

/// The ship parts hung by their templates' names, sorted: the order the
/// controller hangs them in is the streams', which the asserts don't hold.
fn hung(r: &ShipReport) -> Vec<&str> {
    let mut h: Vec<&str> = r.hung.iter().map(String::as_str).collect();
    h.sort_unstable();
    h
}

/// openspec AS.157: a new save's hub links the disc's 19 ship part
/// streams, which join once its save is in and define every part in id
/// order, and the controller hangs the three the save starts equipped,
/// the default engine, masthead and roof; with no Dragon Engine the galley
/// stays shut. The engine's own script plays its clip. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn a_new_saves_hub_hangs_the_ships_three_defaults() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(HUB, Session::new(Some(HOST.into())));
    s.until_hung();
    let r = s.report();
    println!("{r:?}");
    assert_eq!((r.streams, r.joined), (19, true));
    assert_eq!(r.defined, (0..19).collect::<Vec<_>>());
    assert_eq!(r.equipped, [16, 17, 18], "a new save's defaults");
    assert!(r.stand_in.is_empty(), "{:?}", r.stand_in);
    assert_eq!(hung(&r), ["ShipPart_016", "ShipPart_017", "ShipPart_018"]);
    assert_eq!(r.dragon_engine, Some(0));
    assert!(!s.removed("Galley_Blocker"), "the galley opened");
    // The default engine's clone plays its own palette's moving clip
    // (`Engine_Default.ai`), once: no script sets it looping.
    let engine = s
        .run
        .world
        .clones()
        .into_iter()
        .filter(|&h| {
            let obj = s.run.world.actor(h).and_then(|a| a.template.or(a.obj));
            obj.and_then(|o| s.run.world.template(o))
                .is_some_and(|p| p.name == "ShipPart_016")
        })
        .filter(|&h| !s.run.world.removed(h))
        .collect::<Vec<Handle>>();
    assert_eq!(engine.len(), 1, "{engine:?}");
    let turning = s
        .run
        .world
        .sequences()
        .iter()
        .any(|q| q.actor == engine[0] && q.length > 0.0);
    assert!(turning, "the engine played nothing");
    assert!(r.stopped.is_none(), "{:?}", r.stopped);
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// openspec AS.157: level 214's legendary treasure walked up to is its ship
/// part (12, a roof) collected, which the TV equips in the default roof's
/// place as it's collected (stand-in ledger: `ship-parts`); at the hub, on
/// the words carried, it hangs on the ship and the default roof doesn't.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_treasure_hangs_on_the_ship_at_the_hub() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(LEVEL_214, Session::new(Some(HOST.into())));
    let found: Vec<(Handle, Vec3)> = s
        .run
        .files
        .placed
        .iter()
        .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4)
        .filter(|p| {
            p.script
                .as_deref()
                .is_some_and(|x| x.ends_with("/Loot_Specials.ai"))
        })
        .map(|p| (s.run.handles[&p.obj], Vec3::from_array(p.position)))
        .collect();
    let [(treasure, at)] = found[..] else {
        panic!("214's treasures: {found:?}");
    };
    assert_eq!(s.equipped(), [16, 17, 18]);
    s.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    s.run.world.wake(treasure);
    let start = s.run.steps;
    while !s.run.world.removed(treasure) {
        assert!(s.run.steps < start + 1500, "214's treasure stayed");
        s.step();
    }
    let r = s.report();
    println!(
        "collected {} steps on: {:?}",
        s.run.steps - start,
        r.stand_in
    );
    assert_eq!(r.stand_in, [(12, Equipped::Collected)]);
    assert_eq!(
        s.equipped(),
        [12, 16, 17],
        "the roof in the default's place"
    );
    assert_eq!(r.streams, 0, "214 streams no part");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);

    let mut hub = Story::joined(HUB, s.session.clone());
    assert_eq!(hub.session.report.start.as_deref(), Some("carried"));
    hub.until_hung();
    let r = hub.report();
    println!("{r:?}");
    assert_eq!(r.equipped, [12, 16, 17]);
    assert!(r.stand_in.is_empty(), "nothing left to settle");
    assert_eq!(hung(&r), ["ShipPart_012", "ShipPart_016", "ShipPart_017"]);
    assert!(hub.stops.is_empty(), "{:#?}", hub.stops);
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// `values` with part `id` collected and left unequipped, as a build from
/// before AS.157 left a part a kid walked up to.
fn collected_before(values: &mut SaveValues, id: usize) {
    values.params.get_mut(COLLECTED).expect(COLLECTED)[id] = 1;
    assert_eq!(values.params[EQUIPPED][id], 0);
}

/// openspec AS.157: a save from before the TV equipped as it collected,
/// holding 214's roof (12) and the Dragon Engine (1, level 215's) collected
/// and neither equipped, loads, and at its run's start each goes on in its
/// category's default's place (stand-in ledger: `ship-parts`), the start's
/// save keeping it. The hub hangs both; the Dragon Engine lifts the
/// galley's blocker.
#[test]
#[ignore = "reads the Giants dump"]
fn a_save_from_before_shows_its_parts_and_the_engine_opens_the_galley() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let fresh = Story::joined(HUB, Session::new(Some(HOST.into())));
    let (layout, mut values) = fresh.rows.first().cloned().expect("the start's save");
    assert_eq!(values.params[COLLECTED].len(), 19);
    assert_eq!(
        values.params[EQUIPPED][16..],
        [1, 1, 1],
        "a new save's defaults"
    );
    collected_before(&mut values, 12);
    collected_before(&mut values, 1);
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout, values));
    let mut s = Story::joined(HUB, later);
    assert_eq!(s.session.report.start.as_deref(), Some("stored"));
    s.until_hung();
    let r = s.report();
    println!("{r:?}");
    assert_eq!(r.collected, [1, 12, 16, 17, 18]);
    assert_eq!(
        r.stand_in,
        [(1, Equipped::Settled), (12, Equipped::Settled)]
    );
    assert_eq!(s.equipped(), [1, 12, 17]);
    assert_eq!(hung(&r), ["ShipPart_001", "ShipPart_012", "ShipPart_017"]);
    assert_eq!(r.dragon_engine, Some(1));
    assert!(s.removed("Galley_Blocker"), "the galley stays shut");
    // The start's save keeps what was settled.
    let (_, saved) = s.rows.last().cloned().expect("the start's save");
    let on: Vec<usize> = saved.params[EQUIPPED]
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == 1)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(on, [1, 12, 17]);
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
    let blocker = s.run.placed("Galley_Blocker").0;
    println!(
        "blocker off {}; {:.1} s, load at end: {}",
        s.run.world.removed(blocker),
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// openspec AS.157: a stream left out of the run (its parts didn't
/// extract) takes its own part alone off the ship: every other stream keeps
/// its own id, so 214's roof (12) still hangs as itself, and the part left
/// out is defined by none.
#[test]
#[ignore = "reads the Giants dump"]
fn a_stream_left_out_takes_its_own_part_alone() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let fresh = Story::joined(HUB, Session::new(Some(HOST.into())));
    let (layout, mut values) = fresh.rows.first().cloned().expect("the start's save");
    collected_before(&mut values, 12);
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout, values));
    let mut s = Story::joined_with(
        HUB,
        later,
        Setup {
            streams_left_out: &["ship_part_005"],
            ..setup()
        },
    );
    s.until_hung();
    let r = s.report();
    println!("{r:?}");
    assert_eq!(r.streams, 18);
    let mut defined = r.defined.clone();
    defined.sort_unstable();
    assert_eq!(defined, (0..19).filter(|&i| i != 5).collect::<Vec<_>>());
    assert_eq!(hung(&r), ["ShipPart_012", "ShipPart_016", "ShipPart_017"]);
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}
