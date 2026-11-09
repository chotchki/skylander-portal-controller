//! The chests and hat boxes off the way (openspec AU.39), headless with
//! guacamole's host on the TV's listing for each level, the story save
//! answered as the TV answers it. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_chests -- --ignored --nocapture --test-threads=1`

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guac_server::saves::SaveValues;
use guacamole::announcements;
use guacamole::level_end::{ObjectiveKind, Status};
use guacamole::script_engine::Pad;
use guacamole::story_save::{self, Session};

mod common;

use common::*;

const CHAPTER_1: &str = "level_213_tutotial";
const LEVEL_214: &str = "level_214_tutotial_2";
const HOST: &str = "01HOSTPROFILE";
const TRIANGLE: [bool; 4] = [false, false, false, true];

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// A run with its story save answered and a store stand-in writing at once,
/// the rows kept.
struct Story {
    run: Run,
    session: Session,
    rows: Vec<(String, SaveValues)>,
    level: &'static str,
    stops: Vec<String>,
}

impl Story {
    fn on(level: &'static str, session: Session) -> Self {
        let (run, _found) = run_on_level(Mode::Play, level, setup());
        Self {
            run,
            session,
            rows: Vec::new(),
            level,
            stops: Vec::new(),
        }
    }

    /// The level from its start, its hero joined and settled.
    fn joined(level: &'static str, session: Session) -> Self {
        let mut s = Self::on(level, session);
        for _ in 0..2 {
            s.step([false; 4]);
        }
        s.run.join().unwrap_or_else(|h| panic!("{h}"));
        if level == LEVEL_214 {
            let stops = skip_214s_intro(&mut s.run);
            s.stops.extend(stops.iter().map(ToString::to_string));
        }
        for _ in 0..120 {
            s.step([false; 4]);
        }
        s
    }

    fn step(&mut self, actions: [bool; 4]) {
        self.act(Stick::new(0.0, 0.0), actions);
    }

    /// A step with the stick toward `to` across the ground.
    fn toward(&mut self, to: [f32; 2], actions: [bool; 4]) {
        let at = self.run.hero().pivot;
        let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
        let stick = stick_for(self.run.bench.view, heading);
        self.act(stick, actions);
    }

    /// On the stick to each point in turn, within 1 m, at most 600 steps a
    /// point.
    fn walk(&mut self, points: &[[f32; 2]]) {
        for &to in points {
            for _ in 0..600 {
                let at = self.run.hero().pivot;
                if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                    break;
                }
                self.toward(to, [false; 4]);
            }
            let at = self.run.hero().pivot;
            assert!(
                (to[0] - at.x).hypot(to[1] - at.z) <= 1.0,
                "walking to {to:?}: stopped at {at}"
            );
        }
    }

    fn act(&mut self, stick: Stick, actions: [bool; 4]) {
        self.run.bench.pads[0] = Pad {
            stick,
            actions,
            connected: true,
        };
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

    fn behaviour(&self, h: Handle) -> Option<String> {
        self.run
            .vm
            .placement_of(h)
            .and_then(|id| self.run.vm.behaviour(id))
            .map(str::to_owned)
    }

    fn local(&self, h: Handle, name: &str) -> Option<Value> {
        let id = self.run.vm.placement_of(h)?;
        self.run.vm.local(id, name).map(|v| self.run.vm.value(v))
    }

    /// The level's saved flags as `Save - Get/Adjust Level Saved Flags`
    /// reads them.
    fn flags(&mut self) -> Vec<(&'static str, i32)> {
        const F: [&str; 5] = ["completed", "chest 1", "chest 2", "chest 3", "chest 4"];
        self.run.with_host(|vm, host| {
            let args: Vec<(&str, Value)> = F.iter().map(|f| (*f, Value::Int(-1))).collect();
            let (_, back) = vm
                .call_with(None, "Save - Get/Adjust Level Saved Flags", &args, &F, host)
                .unwrap();
            F.into_iter()
                .zip(back)
                .map(|(f, v)| {
                    let v = v.map(|v| vm.value(v)).and_then(|v| v.as_i32());
                    (f, v.unwrap_or(-2))
                })
                .collect()
        })
    }

    /// The level's two dares as the same macro reads them.
    fn dare_flags(&mut self) -> Vec<(&'static str, i32)> {
        const F: [&str; 2] = ["achieved target time", "achieved no lives lost"];
        self.run.with_host(|vm, host| {
            let mut args = vec![("completed", Value::Int(-1))];
            args.extend(F.iter().map(|f| (*f, Value::Int(-1))));
            let (_, back) = vm
                .call_with(None, "Save - Get/Adjust Level Saved Flags", &args, &F, host)
                .unwrap();
            F.into_iter()
                .zip(back)
                .map(|(f, v)| {
                    let v = v.map(|v| vm.value(v)).and_then(|v| v.as_i32());
                    (f, v.unwrap_or(-2))
                })
                .collect()
        })
    }

    fn gold(&self) -> u32 {
        self.run.figure_values().map_or(0, |v| v.gold)
    }
}

/// The chest's own behaviours, by name.
const WAITING: &str = "wait for player to collect me";
const AT_THE_APEX: &str = "chest bouncing at apex waiting for player to open me or go out of range";
const PAYING: &str = "shoot treasure at player";

/// △ mashed for a step: down two steps in eight, as a thumb manages, each
/// press inside the gesture's 0.25 s of the last.
fn mash(step: u32) -> [bool; 4] {
    if step % 8 < 2 { TRIANGLE } else { [false; 4] }
}

/// The hero by `chest`, then △ mashed until it leaves the apex: the steps
/// the mashing took.
fn open(s: &mut Story, chest: Handle, at: Vec3) -> u32 {
    s.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    let start = s.run.steps;
    while s.behaviour(chest).as_deref() != Some(AT_THE_APEX) {
        assert!(s.run.steps < start + 240, "{:?}", s.behaviour(chest));
        s.step([false; 4]);
    }
    let start = s.run.steps;
    while s.behaviour(chest).as_deref() == Some(AT_THE_APEX) {
        assert!(s.run.steps < start + 600, "never opened");
        let k = s.run.steps - start;
        s.step(mash(k));
    }
    s.run.steps - start
}

/// The chest's treasure in play, each with what it's worth: `lootinplay`'s
/// entries `Loot - Init Loot` made from a chest.
fn treasure(s: &Story) -> Vec<(Handle, Vec3, i32)> {
    let vm = &s.run.vm;
    let ui = guacamole::scripts::running(vm, &s.run.world, "/UI.ai").unwrap();
    let set = vm.local(ui, "lootinplay").map(|v| vm.value(v)).unwrap();
    vm.set(set)
        .map(<[Value]>::to_vec)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let e = vm.value(e);
            let field = |n: &str| vm.field(e, n).map(|v| vm.value(v));
            let Value::Entity(h) = field("lootactor")? else {
                return None;
            };
            (field("from chest")?.as_i32()? > 0).then(|| {
                let at = s.run.world.actor(h).unwrap().position;
                (h, at, field("monetaryvalue").unwrap().as_i32().unwrap())
            })
        })
        .collect()
}

/// openspec AU.39: chapter 1's first chest. It waits out of reach; a hero
/// in reach sees it rise and hang there with the mash prompt up, a lone tap
/// not opening it; mashing opens it in the script's 3 s, it flings out its
/// treasure, and takes itself off; walked up, the treasure pays his figure;
/// its flag reads set. A restart (the session's words carried, as the TV's
/// Restart chapter carries them) and a later session (the row the quit's
/// save wrote) both find it gone, the level's other chests standing.
#[test]
#[ignore = "reads the Giants dump"]
fn a_chest_opens_pays_and_stays_opened() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(CHAPTER_1, Session::new(Some(HOST.into())));
    let (chest, at) = s.run.placed("Chest(1)");
    assert_eq!(s.behaviour(chest).as_deref(), Some(WAITING));
    s.run.put(at + Vec3::new(7.0, 1.0, 0.0), 0.0);
    for _ in 0..60 {
        s.step([false; 4]);
    }
    assert_eq!(s.behaviour(chest).as_deref(), Some(WAITING), "out of reach");
    assert!(announcements::gesture_prompts(&s.run.vm, &s.run.world).is_empty());
    let flags = s.flags();
    assert!(flags.iter().all(|f| f.1 == 0), "{flags:?}");

    // Risen, the prompt up, one tap doing nothing.
    s.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    let start = s.run.steps;
    while s.behaviour(chest).as_deref() != Some(AT_THE_APEX) {
        assert!(s.run.steps < start + 240, "{:?}", s.behaviour(chest));
        s.step([false; 4]);
    }
    for _ in 0..30 {
        s.step([false; 4]);
    }
    let prompts = announcements::gesture_prompts(&s.run.vm, &s.run.world);
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(prompts[0].text.is_some(), "the prompt's words don't read");
    s.step(TRIANGLE);
    for _ in 0..240 {
        s.step([false; 4]);
    }
    assert_eq!(
        s.behaviour(chest).as_deref(),
        Some(AT_THE_APEX),
        "a lone tap"
    );

    let at_open = s.gold();
    let took = open(&mut s, chest, at);
    println!("opened after {took} steps of mashing");
    assert!((170..=200).contains(&took), "{took}");
    assert_eq!(s.behaviour(chest).as_deref(), Some(PAYING));
    assert_eq!(s.local(chest, "coin amount"), Some(Value::Int(63)));
    // Every piece it throws, with its worth, as each comes into play: one
    // thrown past the ledge's rim is out of play while it comes back, so
    // what's in play at any one step can miss it (the au-idle-hats review:
    // two worth 10 were out as the chest went).
    let mut thrown: std::collections::BTreeMap<Handle, i32> = Default::default();
    let start = s.run.steps;
    while !s.run.world.removed(chest) {
        assert!(s.run.steps < start + 900, "{:?}", s.behaviour(chest));
        s.step([false; 4]);
        thrown.extend(treasure(&s).into_iter().map(|p| (p.0, p.2)));
    }
    println!("off {} steps on", s.run.steps - start);
    let before = s.gold();
    let pieces = treasure(&s);
    let worth: i32 = pieces.iter().map(|p| p.2).sum();
    println!(
        "{} pieces worth {worth} in play, gold {before}",
        pieces.len()
    );
    assert!(pieces.len() >= 3, "{pieces:?}");
    // Each piece in play walked up: the hero set down on it and held there
    // until it's his.
    for _ in 0..40 {
        let Some((_, at, _)) = treasure(&s).first().copied() else {
            break;
        };
        s.run.put(at, 0.0);
        for _ in 0..90 {
            s.step([false; 4]);
            thrown.extend(treasure(&s).into_iter().map(|p| (p.0, p.2)));
        }
    }
    assert!(treasure(&s).is_empty());
    let gold = s.gold();
    let all: i32 = thrown.values().sum();
    println!(
        "the chest paid {gold}: {} pieces worth {all} thrown",
        thrown.len()
    );
    assert_eq!(gold as i32, at_open as i32 + all);
    // Its count runs a piece's worth and one more a piece until it passes
    // the level's value, and every piece of it is walked up: one thrown past
    // the ledge's rim comes back (`already corrected trajectory`).
    let thrown = s.local(chest, "loot counter").and_then(|v| v.as_i32());
    let thrown = thrown.expect("the pieces thrown");
    println!("{thrown} pieces thrown");
    assert!(gold as i32 + thrown >= 63, "{gold} of {thrown} pieces");
    let flags = s.flags();
    println!("flags {flags:?}");
    assert_eq!(flags[1], ("chest 1", 1));
    assert!(flags[2..].iter().all(|f| f.1 == 0), "{flags:?}");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);

    // Restart chapter: the session's words carried into a new run.
    let mut session = s.session.clone();
    let mut again = Story::joined(CHAPTER_1, session.clone());
    assert_eq!(again.session.report.start.as_deref(), Some("carried"));
    let (chest, _) = again.run.placed("Chest(1)");
    assert!(again.run.world.removed(chest), "open again on a restart");
    let (other, at) = again.run.placed("Chest(2)");
    again.run.put(at + Vec3::new(0.0, 1.0, -7.0), 0.0);
    again.step([false; 4]);
    again.step([false; 4]);
    assert_eq!(again.behaviour(other).as_deref(), Some(WAITING));

    // A later session: the quit's save read back from the store's row.
    {
        let run = &mut s.run;
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        assert!(story_save::quit_save_run(
            &mut run.vm,
            &mut host,
            &mut session
        ));
    }
    let (_, layout, saved) = session.take_write().expect("the quit's save");
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout, saved));
    let again = Story::joined(CHAPTER_1, later);
    assert_eq!(again.session.report.start.as_deref(), Some("stored"));
    let (chest, _) = again.run.placed("Chest(1)");
    assert!(again.run.world.removed(chest), "open again a session on");
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The story levels past chapter 1 and 214, each holding chests and hat
/// boxes, but for level 212, which has no start the headless run can read.
const LEVELS: [&str; 13] = [
    "level_200",
    "level_201",
    "level_202_arkeyancopter",
    "level_203_the_machine",
    "level_204_choices",
    "level_205_piratecards",
    "level_206",
    "level_207_arkus_adventure",
    "level_208_ice",
    "level_209_giantrobot",
    "level_210_thevillage",
    "level_211_final_level",
    "level_215_enter_arkus",
];

/// The level's first ranged placement running `script`, by name and where
/// it stands.
fn first_ranged(s: &Story, script: &str) -> Option<(Handle, Vec3, String)> {
    let mut found: Vec<_> = s
        .run
        .files
        .placed
        .iter()
        .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4 && p.range > 0.0)
        .filter(|p| p.script.as_deref().is_some_and(|x| x.ends_with(script)))
        .collect();
    found.sort_by_key(|p| p.name.clone());
    let p = found.first()?;
    Some((
        s.run.handles[&p.obj],
        Vec3::from_array(p.position),
        p.name.clone(),
    ))
}

/// Where to set a hero down beside a placement at `at`: the first of eight
/// bearings 2 m off that drops him at its height. A chest on a ledge's rim
/// has ground on one side only (level 209's first hangs him over nothing
/// from the east, 6.5 ft over its ground, out of its 5 ft), and level 210's
/// hat box has a prop to its north he'd stand on.
fn beside(s: &mut Story, at: Vec3) -> Vec3 {
    let spots = (0..8)
        .map(|k| (k as f32 * 45.0).to_radians())
        .map(|a| at + Vec3::new(a.sin() * 2.0, 1.0, a.cos() * 2.0));
    for spot in spots {
        s.run.put(spot, 0.0);
        if (s.run.hero().pivot.y - at.y).abs() < 1.5 {
            return spot;
        }
    }
    at + Vec3::new(2.0, 1.0, 0.0)
}

/// Steps until the level's intro is over: it takes the hero where its own
/// scene ends (level 201's landing), whatever set him down before it.
fn past_the_intro(s: &mut Story) {
    let cutscene = s.run.vm.program().global_named("Cutscene State").unwrap();
    for _ in 0..3600 {
        let v = s.run.vm.global_peek(cutscene).map(|v| s.run.vm.value(v));
        if v.and_then(|v| v.as_i32()) == Some(0) {
            return;
        }
        s.step([false; 4]);
    }
}

/// Mashes by `h` until it's gone or `steps` have run: the steps it took.
fn mash_off(s: &mut Story, h: Handle, at: Vec3, steps: u32) -> Option<u32> {
    let by = beside(s, at);
    s.run.put(by, 0.0);
    let start = s.run.steps;
    while !s.run.world.removed(h) {
        let k = s.run.steps - start;
        if k > steps {
            return None;
        }
        s.step(mash(k));
    }
    Some(s.run.steps - start)
}

/// openspec AU.39: every story level's chests and hat boxes on the TV's
/// listing. Each level's first of each, a hero set down by it and △ mashed
/// through: nothing stops, on what the level's own start leaves running. A
/// chest's value is its level's (`Loot - Set Treasure Chest Value`), listed.
#[test]
#[ignore = "reads the Giants dump"]
fn every_levels_chest_and_hat_box_run_clean() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut bad, mut unopened) = (Vec::new(), Vec::new());
    for level in [CHAPTER_1, LEVEL_214].into_iter().chain(LEVELS) {
        let t = std::time::Instant::now();
        let mut s = Story::joined(level, Session::new(Some(HOST.into())));
        past_the_intro(&mut s);
        let chest = first_ranged(&s, "/Chest.ai");
        let (mut opened, mut value, mut taken) = (None, None, None);
        if let Some((h, at, _)) = &chest {
            let by = beside(&mut s, *at);
            s.run.put(by, 0.0);
            for _ in 0..4 {
                s.step([false; 4]);
            }
            opened = mash_off(&mut s, *h, *at, 1500);
            value = s.local(*h, "coin amount");
        }
        let hat = first_ranged(&s, "/Hat_Pickup.ai");
        if let Some((h, at, _)) = &hat {
            taken = mash_off(&mut s, *h, *at, 2400);
        }
        println!(
            "{level}: chest {:?} off after {opened:?} steps, worth {value:?}; hat box {:?} off after {taken:?}; stops {}, {:.1} s",
            chest.as_ref().map(|c| &c.2),
            hat.as_ref().map(|c| &c.2),
            s.stops.len(),
            t.elapsed().as_secs_f64(),
        );
        if !s.stops.is_empty() {
            bad.push((level, s.stops.clone()));
        }
        if opened.is_none() {
            unopened.push(level);
        }
        assert!(taken.is_some(), "{level}: its hat box stayed");
    }
    assert!(bad.is_empty(), "{bad:#?}");
    assert!(unopened.is_empty(), "{unopened:?}");
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

const HAT_BOX: &str = "HatBox_Pickup(1)_wind_Area";

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

/// openspec AS.152: chapter 1's air ground, under `Chest(2)` and the first
/// hat box, is walked on. Its material byte reads surface 42, an elemental
/// surface, not one of the 50 and over a walk stops at, and the hero on it
/// is Eruptor, no air hero: on any element's ground every hero walks.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_walks_chapter_1s_elemental_ground_to_its_chest() {
    let mut s = Story::joined(CHAPTER_1, Session::new(Some(HOST.into())));
    let (chest, at) = s.run.placed("Chest(2)");
    let from = at + Vec3::new(0.0, 1.0, -7.0);
    s.run.put(from, 0.0);
    for _ in 0..10 {
        s.step([false; 4]);
    }
    let start = s.run.hero().pivot;
    let view = s.run.bench.view;
    for _ in 0..120 {
        s.run.bench.pads[0] = Pad {
            stick: stick_for(view, 0.0),
            actions: [false; 4],
            connected: true,
        };
        let stops = s.run.step_play().unwrap();
        s.stops.extend(stops.iter().map(ToString::to_string));
        if s.behaviour(chest).as_deref() != Some(WAITING) {
            break;
        }
    }
    let walked = (s.run.hero().pivot - start).length();
    println!("walked {walked} m to {:?}", s.behaviour(chest));
    assert!(walked > 1.0, "he turned and stood");
    assert_ne!(s.behaviour(chest).as_deref(), Some(WAITING), "out of reach");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
}

/// openspec AU.39: chapter 1's first hat box. Mashed open, its hat is taken:
/// the box runs to its end with nothing to press (its report's continue and
/// the wear question answered, stand-in ledger `hats`), the hero freed, the
/// hat collected in the save's memory; on a restart and a session on the box
/// is gone, and the hat worn by no one.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hat_box_gives_its_hat_and_stays_taken() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(CHAPTER_1, Session::new(Some(HOST.into())));
    let (hat, at) = s.run.placed(HAT_BOX);
    let id = s.run.world.actor(hat).unwrap().id;
    println!("hat box {id}");
    s.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    let start = s.run.steps;
    while s.behaviour(hat).as_deref() != Some("shaking") {
        assert!(s.run.steps < start + 240, "{:?}", s.behaviour(hat));
        s.step([false; 4]);
    }
    for _ in 0..30 {
        s.step([false; 4]);
    }
    assert_eq!(
        announcements::gesture_prompts(&s.run.vm, &s.run.world).len(),
        1
    );
    let start = s.run.steps;
    let mut named = false;
    while !s.run.world.removed(hat) {
        assert!(s.run.steps < start + 1800, "{:?}", s.behaviour(hat));
        let k = s.run.steps - start;
        s.step(mash(k));
        let lines = announcements::hat_reports(&s.run.vm, &s.run.world);
        named |= lines.iter().any(|l| l.text.is_some());
    }
    println!("taken and off {} steps on", s.run.steps - start);
    assert!(named, "its name never read");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
    // Its hat is nothing the TV draws, never the hero (openspec AS.158): the
    // box's `Remove` of it took him off, the stick dead for good.
    let minion = s.run.minion.expect("the hero left the portal");
    assert!(!s.run.vm.removed(minion), "the box took the hero off");
    assert!(s.run.health().is_some(), "the hero's health gone");
    let from = s.run.hero().pivot;
    for _ in 0..120 {
        s.run.bench.pads[0] = Pad {
            stick: stick_for(s.run.bench.view, 0.0),
            actions: [false; 4],
            connected: true,
        };
        let stops = s.run.step_play().unwrap();
        s.stops.extend(stops.iter().map(ToString::to_string));
    }
    let walked = (s.run.hero().pivot - from).length();
    println!("the stick walks him {walked:.2} after the box");
    assert!(walked > 1.0, "the stick dead after the box: {walked}");
    let collected = |s: &mut Story, id: i32| {
        s.run.with_host(|vm, host| {
            let (_, back) = vm
                .call_with(
                    None,
                    "Save - Collection Get/Set - Hat Collectable",
                    &[("hat id", Value::Int(id)), ("collected", Value::Int(-1))],
                    &["collected"],
                    host,
                )
                .unwrap();
            back[0].map(|v| vm.value(v)).and_then(|v| v.as_i32())
        })
    };
    assert_eq!(collected(&mut s, id), Some(1));
    let cutscene = s.run.vm.program().global_named("Cutscene State").unwrap();
    let held = s.run.vm.global_peek(cutscene).map(|v| s.run.vm.value(v));
    assert_eq!(held.and_then(|v| v.as_i32()), Some(0), "the hero left held");

    let mut again = Story::joined(CHAPTER_1, s.session.clone());
    assert_eq!(again.session.report.start.as_deref(), Some("carried"));
    for _ in 0..30 {
        again.step([false; 4]);
    }
    let (hat, at) = again.run.placed(HAT_BOX);
    again.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    for _ in 0..30 {
        again.step([false; 4]);
    }
    assert!(again.run.world.removed(hat), "a box again on a restart");

    // A session on: the quit's save read back from the store's row, the hat
    // still in the collection for AK.
    let mut session = s.session.clone();
    {
        let run = &mut s.run;
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        assert!(story_save::quit_save_run(
            &mut run.vm,
            &mut host,
            &mut session
        ));
    }
    let (_, layout, saved) = session.take_write().expect("the quit's save");
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout, saved));
    let mut again = Story::joined(CHAPTER_1, later);
    assert_eq!(again.session.report.start.as_deref(), Some("stored"));
    assert_eq!(collected(&mut again, id), Some(1), "lost a session on");
    let (hat, at) = again.run.placed(HAT_BOX);
    again.run.put(at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    for _ in 0..30 {
        again.step([false; 4]);
    }
    assert!(again.run.world.removed(hat), "a box again a session on");

    // The level's hats objective counts the one taken of its three.
    let (_, r) = clear(again.session.clone(), false);
    let counts: Vec<_> = r
        .objectives
        .iter()
        .filter(|o| o.category == 2)
        .map(|o| (o.kind, o.status, o.count, o.total))
        .collect();
    println!("the collection: {counts:?}");
    assert!(
        counts
            .iter()
            .any(|c| c.0 == ObjectiveKind::Count && c.2 == 1.0 && c.3 == 3.0),
        "{counts:?}"
    );
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The outro's player waypoint, facing `Temp_Level_Ender`: the dev entry's
/// end anchor.
const END: ([f32; 3], f32) = ([-10.282436, -153.01357, 318.01202], 17.6);

/// Chapter 1 cleared from a joined run on `session`, its clock taken past
/// any target when `late`: the objectives screen's results, and the run at
/// its completion screen.
fn clear(session: Session, late: bool) -> (Story, guacamole::level_end::LevelResults) {
    let mut s = Story::joined(CHAPTER_1, session);
    s.run.world.sequence.shows_completion = true;
    if late {
        let g = s
            .run
            .vm
            .program()
            .global_named("level control data")
            .unwrap();
        let data = s.run.vm.global_peek(g).unwrap();
        let stats = s.run.vm.field(data, "level stat tracking").unwrap();
        assert!(
            s.run
                .vm
                .set_field(stats, "playtime", Value::Float(36_000.0))
        );
    }
    s.run.put(Vec3::from_array(END.0), END.1);
    let (ender, _) = s.run.placed("Temp_Level_Ender");
    assert!(s.run.world.wake(ender));
    let start = s.run.steps;
    while s.run.world.sequence.completion.is_none() {
        assert!(s.run.steps < start + 2400, "no completion screen");
        s.step([false; 4]);
    }
    let shown = s.run.world.sequence.completion.clone().unwrap();
    let results = shown.results.expect("the objectives screen's results");
    (s, results)
}

/// The dares among `r`'s objectives: the time dare and the check beside it
/// in the dares' tab, no lives lost, each with its status.
fn dares(r: &guacamole::level_end::LevelResults) -> Vec<(ObjectiveKind, Status)> {
    r.objectives
        .iter()
        .filter(|o| o.category == 1)
        .map(|o| (o.kind, o.status))
        .collect()
}

/// openspec AS.139: a dare met is saved met. Chapter 1 cleared in time and
/// with no life lost reads both newly done, the flags hold them, and the
/// next clear, on the words the first left, reads them done from before. A
/// dare missed stays open and its flag unset, the other beside it met.
#[test]
#[ignore = "reads the Giants dump"]
fn a_dare_met_is_saved_met_and_reads_done_on_the_next_clear() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut s, r) = clear(Session::new(Some(HOST.into())), false);
    println!("the first clear: {:?}, stars {:?}", dares(&r), r.stars);
    let met = dares(&r);
    assert!(met.iter().any(|d| d.0 == ObjectiveKind::Time), "{met:?}");
    let time =
        |d: &[(ObjectiveKind, Status)]| d.iter().find(|x| x.0 == ObjectiveKind::Time).map(|x| x.1);
    assert_eq!(time(&met), Some(Status::NewlyDone));
    let flags = s.dare_flags();
    println!("its flags {flags:?}");
    assert_eq!(
        flags,
        [("achieved target time", 1), ("achieved no lives lost", 1)]
    );
    assert!(s.stops.is_empty(), "{:#?}", s.stops);

    let (again, r) = clear(s.session.clone(), false);
    assert_eq!(again.session.report.start.as_deref(), Some("carried"));
    println!("the next clear: {:?}, stars {:?}", dares(&r), r.stars);
    assert_eq!(time(&dares(&r)), Some(Status::Done));

    let (mut s, r) = clear(Session::new(Some(HOST.into())), true);
    println!("a late clear: {:?}, stars {:?}", dares(&r), r.stars);
    assert_eq!(time(&dares(&r)), Some(Status::Open));
    assert_eq!(
        s.dare_flags(),
        [("achieved target time", 0), ("achieved no lives lost", 1)]
    );
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// openspec AS.151: chapter 1's Eagle Paint. Walked up to, it takes itself
/// into the collection, spins up through its scene, pays its definition's
/// 100 gold past its report and goes; collected, it's gone at the next
/// start, on the words carried and a session on.
#[test]
#[ignore = "reads the Giants dump"]
fn a_legendary_treasure_pays_out_and_stays_collected() {
    const TREASURE: &str = "Legendary_Treasure_Eagle_Paint(2)";
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(CHAPTER_1, Session::new(Some(HOST.into())));
    let (treasure, at) = s.run.placed(TREASURE);
    assert!(!s.run.world.removed(treasure));
    s.run.put(at + Vec3::new(0.0, 1.0, -7.0), 0.0);
    let (start, gold) = (s.run.steps, s.gold());
    let mut named = false;
    while !s.run.world.removed(treasure) {
        assert!(s.run.steps < start + 900, "{:?}", s.behaviour(treasure));
        let view = s.run.bench.view;
        let near = s.run.hero().pivot.distance(at) < 3.0;
        s.run.bench.pads[0] = Pad {
            stick: match near {
                true => Stick::new(0.0, 0.0),
                false => stick_for(view, 0.0),
            },
            actions: [false; 4],
            connected: true,
        };
        let stops = s.run.step_play().unwrap();
        s.stops.extend(stops.iter().map(ToString::to_string));
        let lines = announcements::hat_reports(&s.run.vm, &s.run.world);
        named |= lines.iter().any(|l| l.text.is_some());
    }
    println!(
        "collected {} steps on, {} gold",
        s.run.steps - start,
        s.gold() - gold
    );
    assert_eq!(s.gold() - gold, 100, "its definition's gold");
    assert!(named, "its name never read");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
    let cutscene = s.run.vm.program().global_named("Cutscene State").unwrap();
    for _ in 0..120 {
        s.step([false; 4]);
    }
    let held = s.run.vm.global_peek(cutscene).map(|v| s.run.vm.value(v));
    assert_eq!(held.and_then(|v| v.as_i32()), Some(0), "the hero left held");

    let mut again = Story::joined(CHAPTER_1, s.session.clone());
    assert_eq!(again.session.report.start.as_deref(), Some("carried"));
    let (treasure, _) = again.run.placed(TREASURE);
    again.run.put(at + Vec3::new(0.0, 1.0, -7.0), 0.0);
    for _ in 0..30 {
        again.step([false; 4]);
    }
    assert!(again.run.world.removed(treasure), "back on a restart");

    let mut session = s.session.clone();
    {
        let run = &mut s.run;
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        assert!(story_save::quit_save_run(
            &mut run.vm,
            &mut host,
            &mut session
        ));
    }
    let (_, layout, saved) = session.take_write().expect("the quit's save");
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout, saved));
    let mut again = Story::joined(CHAPTER_1, later);
    assert_eq!(again.session.report.start.as_deref(), Some("stored"));
    let (treasure, _) = again.run.placed(TREASURE);
    again.run.put(at + Vec3::new(0.0, 1.0, -7.0), 0.0);
    for _ in 0..30 {
        again.step([false; 4]);
    }
    assert!(again.run.world.removed(treasure), "back a session on");
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The best of four headings the stick walks the hero, a good second of each.
fn stick_reach(s: &mut Story) -> f32 {
    let from = s.run.hero().pivot;
    let mut best = 0.0f32;
    for heading in [0.0f32, 90.0, 180.0, 270.0] {
        for _ in 0..40 {
            s.run.bench.pads[0] = Pad {
                stick: stick_for(s.run.bench.view, heading),
                actions: [false; 4],
                connected: true,
            };
            let stops = s.run.step_play().unwrap();
            s.stops.extend(stops.iter().map(ToString::to_string));
        }
        best = best.max(s.run.hero().pivot.distance(from));
    }
    best
}

/// The hero on his feet: on the portal, his health read and the scene off
/// him.
fn left_free(s: &mut Story, what: &str) {
    let minion = s.run.minion.expect("the hero left the portal");
    assert!(!s.run.vm.removed(minion), "{what}: the hero's off");
    assert!(s.run.health().is_some(), "{what}: his health gone");
    let cutscene = s.run.vm.program().global_named("Cutscene State").unwrap();
    for _ in 0..600 {
        let held = s.run.vm.global_peek(cutscene).map(|v| s.run.vm.value(v));
        if held.and_then(|v| v.as_i32()) == Some(0) {
            break;
        }
        s.step([false; 4]);
    }
    let held = s.run.vm.global_peek(cutscene).map(|v| s.run.vm.value(v));
    assert_eq!(held.and_then(|v| v.as_i32()), Some(0), "{what}: held");
    let reach = stick_reach(s);
    assert!(reach > 0.5, "{what}: a dead stick, {reach}");
}

/// AS.158's review: a box's stale scratch reached past the first box. Every
/// box on chapter 1 and 214 in one run, the hero swapped between them, each
/// leaves the hero walking: the outfit registered once a run holds for a
/// second box and a second hero. A swap's next hero is a new `Minions` entry
/// on the same run.
#[test]
#[ignore = "reads the Giants dump"]
fn every_box_in_a_run_leaves_each_hero_walking() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for level in [CHAPTER_1, LEVEL_214] {
        let mut s = Story::joined(level, Session::new(Some(HOST.into())));
        past_the_intro(&mut s);
        let boxes: Vec<(Handle, Vec3, String)> = s
            .run
            .files
            .placed
            .iter()
            .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4)
            .filter(|p| {
                p.script
                    .as_deref()
                    .is_some_and(|x| x.ends_with("/Hat_Pickup.ai"))
            })
            .map(|p| {
                (
                    s.run.handles[&p.obj],
                    Vec3::from_array(p.position),
                    p.name.clone(),
                )
            })
            .collect();
        assert!(!boxes.is_empty(), "{level}");
        for (k, (h, at, name)) in boxes.iter().enumerate() {
            if k % 2 == 1 {
                s.run.deactivate().unwrap();
                for _ in 0..30 {
                    s.step([false; 4]);
                }
                s.run.join().unwrap();
                for _ in 0..120 {
                    s.step([false; 4]);
                }
            }
            let took = mash_off(&mut s, *h, *at, 2400);
            println!("{level} {name}: off after {took:?} steps");
            assert!(took.is_some(), "{level}'s {name} stayed");
            left_free(&mut s, name);
        }
        assert!(s.stops.is_empty(), "{level}: {:#?}", s.stops);
    }
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// AS.151's review: every story level's legendary treasure, a hero set down
/// by it. Its scene ends, it pays once, the hero's left free and nothing
/// stops: no level's treasure waits on the menu the TV doesn't have.
#[test]
#[ignore = "reads the Giants dump"]
fn every_levels_legendary_treasure_pays_and_frees_the_hero() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut none = Vec::new();
    for level in [CHAPTER_1, LEVEL_214].into_iter().chain(LEVELS) {
        let mut s = Story::joined(level, Session::new(Some(HOST.into())));
        past_the_intro(&mut s);
        let found: Vec<(Handle, Vec3, String)> = s
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
            .map(|p| {
                (
                    s.run.handles[&p.obj],
                    Vec3::from_array(p.position),
                    p.name.clone(),
                )
            })
            .collect();
        if found.is_empty() {
            none.push(level);
        }
        for (h, at, name) in &found {
            let gold = s.gold();
            let by = beside(&mut s, *at);
            s.run.put(by, 0.0);
            // In a room its interior's controller wakes (range 0): as it would.
            s.run.world.wake(*h);
            let start = s.run.steps;
            while !s.run.world.removed(*h) {
                assert!(
                    s.run.steps < start + 1500,
                    "{level}'s {name}: {:?}",
                    s.behaviour(*h)
                );
                s.step([false; 4]);
            }
            let paid = s.gold() - gold;
            println!(
                "{level} {name}: off after {} steps, {paid} gold",
                s.run.steps - start
            );
            assert!(paid > 0, "{level}'s {name} paid nothing");
            left_free(&mut s, name);
        }
        assert!(s.stops.is_empty(), "{level}: {:#?}", s.stops);
    }
    println!(
        "none on {none:?}; {:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Into one of level 214's rooms by its controller's box, at most 300 steps:
/// where the warp landed him.
fn into_the_room(s: &mut Story, controller: &str) -> Vec3 {
    let (_, box_at) = s.run.placed(controller);
    let landed0 = s.run.world.landed.len();
    s.run.put(box_at, 0.0);
    for _ in 0..300 {
        s.step([false; 4]);
        if s.run.world.landed.len() > landed0 {
            break;
        }
    }
    let landed = s.run.world.landed.get(landed0).expect("into the room").at;
    for _ in 0..120 {
        s.step([false; 4]);
    }
    landed
}

/// The bombs standing.
fn bombs(s: &Story) -> Vec<Handle> {
    s.run
        .world
        .clones()
        .into_iter()
        .filter(|h| s.run.world.name(*h) == "Bomb_Template" && !s.run.world.removed(*h))
        .collect()
}

/// openspec AS.106: what level 214's rooms hold that plays without a screen
/// of its own. In the workshop the pretzel heals a hurt hero who walks into
/// it, and the legendary treasure behind its wall, the wall opened by the
/// room's own bomb (a run's first burst reaches 20 ft, so it takes two:
/// AS.96's review), is taken into the collection for its gold. The south
/// room is two rooms by its two doors, each waking its own actors (the
/// controllers' `interior_set`s): by one the pub, by the other the brewery,
/// whose food heals him too. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_rooms_give_their_treasure_and_food() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut s = Story::joined(LEVEL_214, Session::new(Some(HOST.into())));
    past_the_intro(&mut s);
    // The workshop.
    let landed = into_the_room(&mut s, "Interior_Controller_Island_2_1");
    println!("in the workshop at {landed}");
    // The pretzel, hurt.
    let (pretzel, pretzel_at) = s.run.placed("Food_Pretzle(1)");
    let minion = s.run.minion.expect("joined");
    assert!(guacamole::scripts::set_health(&mut s.run.vm, minion, 100));
    // Round the workbench by the room's north side.
    s.walk(&[[388.0, 224.0], [pretzel_at.x, pretzel_at.z]]);
    for _ in 0..180 {
        if s.run.world.removed(pretzel) {
            break;
        }
        s.step([false; 4]);
    }
    let healed = s.run.health();
    println!(
        "the pretzel eaten {}, health {healed:?}",
        s.run.world.removed(pretzel)
    );
    assert!(s.run.world.removed(pretzel), "the pretzel");
    assert!(healed.is_some_and(|h| h > 100.0), "healed: {healed:?}");
    // The wall, by the room's bomb, twice.
    let (wall, _) = s.run.placed("Breakable_Stone_Wall(1)");
    for k in 0..3 {
        if s.run.world.removed(wall) {
            break;
        }
        let mut bomb = None;
        for _ in 0..900 {
            bomb = bombs(&s)
                .into_iter()
                .find(|b| s.behaviour(*b).as_deref() == Some("following player"));
            if bomb.is_some() {
                break;
            }
            let at = s.run.hero().pivot;
            let far = (at.x - 387.43).hypot(at.z - 216.0) > 0.8;
            match far {
                true => s.toward([387.43, 216.0], [false; 4]),
                false => s.step([false; 4]),
            }
        }
        assert!(bomb.is_some(), "throw {k}: the room's bomb");
        for _ in 0..60 {
            s.step([false; 4]);
        }
        s.run.put(Vec3::new(376.8054, -299.9297, 219.5375), 283.167);
        s.step([false; 4]);
        for j in 0..150u32 {
            s.step([false, false, false, j == 0]);
        }
        println!("throw {k}: the wall {}", s.run.world.removed(wall));
    }
    assert!(s.run.world.removed(wall), "the workshop's wall");
    // The treasure behind it.
    let (treasure, at) = s.run.placed("Legendary_Treasure_Propeller_Roof(2)");
    let gold = s.gold();
    s.walk(&[[366.0, 220.5], [359.0, 221.2]]);
    println!(
        "by the treasure at {} ({:.1} m)",
        s.run.hero().pivot,
        s.run.hero().pivot.distance(at)
    );
    let start = s.run.steps;
    while !s.run.world.removed(treasure) {
        assert!(
            s.run.steps < start + 1500,
            "the treasure: {:?}",
            s.behaviour(treasure)
        );
        s.step([false; 4]);
    }
    let paid = s.gold() - gold;
    println!(
        "the treasure at {at} taken {} steps on, {paid} gold",
        s.run.steps - start
    );
    assert!(paid > 0, "the treasure paid nothing");
    left_free(&mut s, "the treasure");
    // The south room by the brewery's door.
    let landed = into_the_room(&mut s, "Interior_Controller_Island_2_2");
    println!("in the brewery at {landed}");
    let (food, food_at) = s.run.placed("Chinese_Food(1)");
    assert!(guacamole::scripts::set_health(&mut s.run.vm, minion, 100));
    s.walk(&[[380.0, 136.0], [food_at.x, 136.0]]);
    for _ in 0..600 {
        if s.run.world.removed(food) {
            break;
        }
        s.toward([food_at.x, food_at.z], [false; 4]);
    }
    println!(
        "the south room's food ({:?}, {:?} at {food_at}, he at {}) eaten {}, health {:?}; {:.1} s; load at end: {}",
        s.behaviour(food),
        s.run.world.actor(food).map(|a| (a.active, a.removed)),
        s.run.hero().pivot,
        s.run.world.removed(food),
        s.run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(s.run.world.removed(food), "the south room's food");
    assert!(s.stops.is_empty(), "{:#?}", s.stops);
}
