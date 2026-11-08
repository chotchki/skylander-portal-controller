//! The elemental areas (openspec AS.153), headless with guacamole's host on
//! the TV's listing for each level. A hero's element is what his toy table
//! entry says, which is what a wall reads; these write Eruptor's entry for a
//! hero of another element. Ignored by default; they read `GUAC_GIANTS_DUMP`
//! (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_elements -- --ignored --nocapture --test-threads=1`

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const CHAPTER_1: &str = "level_213_tutotial";
const LEVEL_214: &str = "level_214_tutotial_2";
const AIR: i32 = 2;
const DEATH: i32 = 3;
const FIRE: i32 = 5;
const EARTH: i32 = 6;
const LIFE: i32 = 7;
/// `achievement mapping`'s `unlock elemental area`.
const UNLOCK_AREA: i32 = 17;
const AIR_WALL: &str = "ElementalWallController(3)";

struct Walk {
    run: Run,
    stops: Vec<String>,
    /// The host's story save, answered each step (openspec AS.156: a hat
    /// box wakes once the run's save is in, AU.39); none, as before.
    session: Option<guacamole::story_save::Session>,
}

impl Walk {
    /// A level from its start with its hero joined and settled.
    fn joined(level: &str) -> Walk {
        Walk::joined_as(level, None)
    }

    /// [`Walk::joined`] with `character` in Eruptor's place.
    fn joined_as(level: &str, character: Option<Character>) -> Walk {
        Walk::joined_listing(level, character, &[])
    }

    /// [`Walk::joined_as`] with `scripts` listed past the TV's listing.
    fn joined_listing(
        level: &str,
        character: Option<Character>,
        scripts: &'static [&'static str],
    ) -> Walk {
        let (mut run, _) = run_on_level(
            Mode::Play,
            level,
            Setup {
                interface: true,
                props: true,
                sounds: true,
                character,
                scripts,
                ..Setup::default()
            },
        );
        let mut stops = Vec::new();
        for _ in 0..2 {
            stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
        }
        run.join().unwrap();
        if level == LEVEL_214 {
            stops.extend(skip_214s_intro(&mut run).iter().map(ToString::to_string));
        }
        let mut w = Walk {
            run,
            stops,
            session: None,
        };
        w.still(120);
        w
    }

    fn step(&mut self, stick: Stick) {
        self.act(stick, [false; 4]);
    }

    fn act(&mut self, stick: Stick, actions: [bool; 4]) {
        self.run.bench.pads[0] = Pad {
            stick,
            actions,
            connected: true,
        };
        let stops = self.run.step_play().unwrap();
        self.stops.extend(stops.iter().map(ToString::to_string));
        if let Some(session) = self.session.as_mut() {
            let run = &mut self.run;
            let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
            let mut engine = guacamole::script_engine::NoEngine;
            let mut host = guacamole::scripts::WorldHost {
                world: &mut run.world,
                engine: &mut engine,
                generator: &mut run.generator,
                spawn: &mut spawn,
            };
            guacamole::story_save::serve(&mut run.vm, &mut host, session, LEVEL_214)
                .unwrap_or_else(|h| panic!("{h}"));
            if session.take_write().is_some() {
                session.written(&mut run.world.save, Ok(()));
            }
        }
    }

    fn still(&mut self, steps: u32) {
        for _ in 0..steps {
            self.step(Stick::new(0.0, 0.0));
        }
    }

    /// The stick along `heading`, past the dead zone per axis.
    fn stick_along(&self, heading: f32) -> Stick {
        let d = (heading - self.run.bench.view).to_radians();
        let raw = |a: f32| {
            if a.abs() < 1e-4 {
                0.0
            } else {
                a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
            }
        };
        Stick::new(raw(-d.sin()), raw(d.cos()))
    }

    fn walk(&mut self, points: &[[f32; 2]]) {
        for &to in points {
            for _ in 0..500 {
                let at = self.run.hero().pivot;
                if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                    break;
                }
                let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
                let s = self.stick_along(heading);
                self.step(s);
            }
            let at = self.run.hero().pivot;
            assert!(
                (to[0] - at.x).hypot(to[1] - at.z) <= 1.0,
                "walking to {to:?}: stopped at {at}"
            );
        }
    }

    /// His player's joined data.
    fn player(&mut self) -> Value {
        let hero = self.run.hero;
        self.run.with_host(|vm, host| {
            let (_, back) = vm
                .call_with(
                    None,
                    "Game - Get Player Joined Data  ",
                    &[("input minion", Value::Entity(hero))],
                    &["return joined data"],
                    host,
                )
                .unwrap();
            vm.value(back[0].unwrap())
        })
    }

    fn field(&self, v: Value, name: &str) -> Value {
        let vm = &self.run.vm;
        vm.field(v, name).map(|v| vm.value(v)).unwrap()
    }

    /// What a wall reads him as: his toy table entry, the one a join leaves
    /// when he's the figure on the portal.
    fn toy(&mut self) -> Value {
        let player = self.player();
        let loaded = self.field(player, "loaded data");
        let toy = self.field(self.field(loaded, "tag data"), "toy load hint");
        assert_ne!(toy, Value::Null, "his tag names no toy");
        toy
    }

    fn element(&mut self) -> i32 {
        let toy = self.toy();
        let vm = &self.run.vm;
        vm.field(toy, "element")
            .map(|v| vm.value(v))
            .and_then(|v| v.as_i32())
            .unwrap()
    }

    /// A hero of `element`, to a wall and to the ground under him (his
    /// element's surface is 40 on, as `Elemental Surface - Define`'s are).
    fn be(&mut self, element: i32) {
        let toy = self.toy();
        assert!(self.run.vm.set_field(toy, "element", Value::Int(element)));
        let player = self.player();
        let minion = self.field(player, "minion description");
        let ground = Value::Int(40 + element);
        assert!(
            self.run
                .vm
                .set_field(minion, "elemental ground type", ground)
        );
    }

    /// The hero set down by the air wall, on the ledge north of it. The
    /// vista's scene wakes there in a walk from the level's start, and
    /// `giants_chapter_1` plays that; here it's off.
    fn by_the_air_wall(&mut self) {
        let (vista, _) = self.run.placed("213_First_Vista_IGC");
        self.run.vm.remove(vista);
        self.run.put(Vec3::new(-40.0, -134.0, 122.0), 180.0);
        self.still(10);
    }

    fn standing(&self, name: &str) -> bool {
        let p = self
            .run
            .files
            .placed
            .iter()
            .find(|p| p.name == name && p.obj.unit == LEVEL)
            .unwrap();
        let props = self.run.bench.props.as_ref().unwrap();
        props.standing.contains(&p.obj.index)
    }

    fn wall_nodes(&self, name: &str) -> Vec<Vec3> {
        let p = self
            .run
            .files
            .placed
            .iter()
            .find(|p| p.name == name && p.obj.unit == LEVEL)
            .unwrap();
        p.waypoints
            .iter()
            .filter_map(|o| self.run.files.points.get(o).copied())
            .collect()
    }

    fn gone(&self, h: Handle) -> bool {
        self.run.world.removed(h)
    }
}

/// openspec AS.153: chapter 1's air wall, on the ledge's rim over the air
/// ground. Eruptor at its foot walks along it and no further; a hero of its
/// element within 20 ft of a node takes it down, its solid with it, the
/// `unlock elemental area` achievement counted, and walks off the ledge.
#[test]
#[ignore = "reads the Giants dump"]
fn the_air_wall_stands_until_an_air_hero_comes() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut w = Walk::joined(CHAPTER_1);
    assert_eq!(w.element(), FIRE, "Eruptor's toy");
    let (wall, _) = w.run.placed(AIR_WALL);
    w.by_the_air_wall();
    for _ in 0..360 {
        let s = w.stick_along(180.0);
        w.step(s);
    }
    println!("Eruptor stops at {}", w.run.hero().pivot);
    assert!(w.run.hero().pivot.y > -135.0, "still on the ledge");
    // Its own art, out of its pool (the review: a reset left every one off).
    let standing = |w: &Walk, name: &str| {
        w.run
            .world
            .clones()
            .into_iter()
            .filter(|h| !w.gone(*h) && w.run.world.name(*h) == name)
            .count()
    };
    let (twisters, mounds) = (
        standing(&w, "AirWallTemplate(1)"),
        standing(&w, "DirtReplacement_DONT_REMOVE"),
    );
    println!("{twisters} twisters, {mounds} mounds");
    assert!(twisters >= 5 && mounds >= 5, "{twisters} {mounds}");
    assert!(!w.gone(wall), "a fire hero");
    assert!(w.standing(AIR_WALL), "its solid");
    assert_eq!(w.run.world.achievements, Vec::<i32>::new());

    w.be(AIR);
    w.run.put(Vec3::new(-40.0, -134.0, 122.0), 180.0);
    for _ in 0..200 {
        w.still(1);
        if w.gone(wall) {
            break;
        }
    }
    assert!(w.gone(wall), "an air hero");
    w.still(1);
    assert!(!w.standing(AIR_WALL), "its solid off");
    assert_eq!(w.run.world.achievements, vec![UNLOCK_AREA]);
    for _ in 0..240 {
        let s = w.stick_along(180.0);
        w.step(s);
    }
    println!(
        "the air hero walks to {}, {:.1} s",
        w.run.hero().pivot,
        t.elapsed().as_secs_f32()
    );
    assert!(w.run.hero().pivot.y < -140.0, "off the ledge");
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}

/// openspec AS.153: every wall on chapter 1 and level 214 opens for its own
/// element and no other, a hero of each of the other elements set down at
/// its nodes in turn, chapter 1's death wall taking its two gates with it.
#[test]
#[ignore = "reads the Giants dump"]
fn each_wall_opens_for_its_own_element_alone() {
    for (level, walls) in [
        (
            CHAPTER_1,
            &[
                ("ElementalWallController(1)", DEATH),
                ("ElementalWallController(2)", LIFE),
                (AIR_WALL, AIR),
            ][..],
        ),
        (
            LEVEL_214,
            &[
                ("ElementalWallController(1)", FIRE),
                ("ElementalWallController(2)", EARTH),
            ][..],
        ),
    ] {
        let mut w = Walk::joined(level);
        if level == CHAPTER_1 {
            let (vista, _) = w.run.placed("213_First_Vista_IGC");
            w.run.vm.remove(vista);
        }
        for &(name, element) in walls {
            let (wall, _) = w.run.placed(name);
            let nodes = w.wall_nodes(name);
            assert!(nodes.len() > 2, "{name}");
            let at = nodes[nodes.len() / 2] + Vec3::Y;
            // The node stands in its own solid: a step to the side of it.
            for other in [AIR, DEATH, FIRE, EARTH, LIFE] {
                if other == element {
                    continue;
                }
                w.be(other);
                w.run.put(at, 0.0);
                w.still(60);
                assert!(!w.gone(wall), "{level}'s {name} opened for {other}");
            }
            w.be(element);
            w.run.put(at, 0.0);
            for _ in 0..240 {
                w.still(1);
                if w.gone(wall) {
                    break;
                }
            }
            assert!(w.gone(wall), "{level}'s {name} stood for {element}");
            println!("{level}'s {name} opened for {element}");
        }
        if level == CHAPTER_1 {
            w.still(1);
            assert!(!w.standing("Death_Collision_Gate_1"));
            assert!(!w.standing("Death_Collision_Gate_2"));
        }
        assert_eq!(w.run.world.achievements.len(), walls.len());
        assert!(w.stops.is_empty(), "{level}: {:#?}", w.stops);
    }
}

/// openspec AS.153: the air area's way round on foot and pads, none of it a
/// way in but the ledge's: off the ledge to `Chest(2)`, up by
/// `PvP_JumpPad_NORMAL(7)` to the high ground, over its north side to the
/// first hat box, and back to the ledge by `PvP_JumpPad_NORMAL(8)`. Taken by
/// a hero swapped in on the air ground for one of another element: the wall
/// went for good, so nothing traps him, and the pads don't ask.
#[test]
#[ignore = "reads the Giants dump"]
fn the_air_areas_way_round_comes_back_to_the_ledge_for_any_hero() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut w = Walk::joined(CHAPTER_1);
    let (wall, _) = w.run.placed(AIR_WALL);
    w.by_the_air_wall();
    w.be(AIR);
    for _ in 0..200 {
        w.still(1);
        if w.gone(wall) {
            break;
        }
    }
    assert!(w.gone(wall));
    w.walk(&[[-41.0, 108.0]]);
    assert!(w.run.hero().pivot.y < -141.0, "down on the air ground");

    // Off the portal here, the next hero in where he stood.
    let gone = w.run.deactivate().unwrap();
    println!("swapped: {gone:?}");
    w.still(30);
    w.run.join().unwrap();
    w.be(FIRE);
    w.still(120);
    println!("swapped in at {}", w.run.hero().pivot);

    w.walk(&[[-45.49, 103.87]]);
    for _ in 0..240 {
        let s = w.stick_along(180.0);
        w.step(s);
        if w.run.hero().pivot.y > -134.0 && w.run.hero().footing.supported {
            break;
        }
    }
    assert!(w.run.hero().pivot.y > -134.0, "up on the high ground");
    // Over its north-west side, the Spinner's sweep or not.
    for _ in 0..600 {
        let at = w.run.hero().pivot;
        if at.y < -137.0 {
            break;
        }
        let heading = (-70.0 - at.x).atan2(110.0 - at.z).to_degrees();
        let s = w.stick_along(heading);
        w.step(s);
    }
    let (hat, hat_at) = w.run.placed("HatBox_Pickup(1)_wind_Area");
    w.walk(&[[hat_at.x + 2.0, hat_at.z]]);
    assert!(!w.gone(hat), "beside it, unopened");
    assert!(w.run.hero().pivot.y < -138.0, "by the hat box");
    w.walk(&[[-59.3, 123.58]]);
    w.still(200);
    let back = w.run.hero().pivot;
    println!("back at {back}, {:.1} s", t.elapsed().as_secs_f32());
    assert!(back.y > -135.0 && back.x > -50.0, "the ledge: {back}");
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}

/// openspec AS.153: on his own element's ground a hero mends, 5 every 4
/// seconds, the first 2 seconds on, with its walking effect on him; on
/// another's he neither mends nor hurts (the wrong element's bubble ends
/// where `Elemental Surface - Create Minion FX` starts, and nothing adds
/// its damage modifier).
#[test]
#[ignore = "reads the Giants dump"]
fn a_heros_own_ground_mends_him_and_anothers_does_nothing() {
    let mut w = Walk::joined(CHAPTER_1);
    let m = w.run.minion.unwrap();
    let set_hp = |w: &mut Walk, hp: f32| {
        let vm = &mut w.run.vm;
        let d = vm.local(m, "description").map(|v| vm.value(v)).unwrap();
        let c = vm.field(d, "creature info").map(|v| vm.value(v)).unwrap();
        assert!(vm.set_field(c, "hp", Value::Float(hp)));
    };
    let (chest, at) = w.run.placed("Chest(2)");
    assert!(!w.gone(chest));
    // Another's ground first: walked on, so the check looks at it.
    w.run.put(at + Vec3::new(-2.0, 1.0, -3.0), 0.0);
    w.still(10);
    set_hp(&mut w, 100.0);
    for k in 0..600 {
        let heading = if (k / 120) % 2 == 0 { 270.0 } else { 90.0 };
        let s = w.stick_along(heading);
        w.step(s);
    }
    let other = w.run.health().unwrap();
    println!("{} s on another's ground: {other}", 600 / 60);
    assert_eq!(other, 100.0);

    w.be(AIR);
    for k in 0..600 {
        let heading = if (k / 120) % 2 == 0 { 270.0 } else { 90.0 };
        let s = w.stick_along(heading);
        w.step(s);
    }
    let own = w.run.health().unwrap();
    println!("{} s on his own ground: {own}", 600 / 60);
    assert!((105.0..=115.0).contains(&own), "{own}");
    // Its walking effect, on its script (the review: the heal comes without).
    let effect: Vec<Handle> = w
        .run
        .world
        .clones()
        .into_iter()
        .filter(|h| !w.gone(*h) && w.run.world.name(*h) == "AirSurface_ParticleManager")
        .collect();
    println!("{} of its walking effect", effect.len());
    assert!(!effect.is_empty(), "no walking effect on him");
    assert!(
        effect.iter().all(|h| !w.run.vm.inert().contains(h)),
        "its script off the listing"
    );
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}

/// The levels past 214 that place walls, on the listing as chapter 1's run
/// (level 212 has no start the harness can stand him on, 204 no wall).
const PAST_214: [&str; 13] = [
    "level_200",
    "level_201",
    "level_202_arkeyancopter",
    "level_203_the_machine",
    "level_205_piratecards",
    "level_206",
    "level_207_arkus_adventure",
    "level_208_ice",
    "level_209_giantrobot",
    "level_210_thevillage",
    "level_211_final_level",
    "level_215_enter_arkus",
    "level_hub_2_stage_1",
];

/// The AS.153 review: every wall past 214, no level of which the story has
/// walked yet. Each stands for an element not its own and opens for its own,
/// on all eight surface definitions, and nothing stops. A wall a level's own
/// trigger wakes (range 0: level 209's, its `202_TechAreaController.ai` off
/// the listing) is named, not tried.
#[test]
#[ignore = "reads the Giants dump"]
fn every_levels_walls_open_for_their_own_element_alone() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut tried, mut waiting) = (0, Vec::new());
    for level in PAST_214 {
        let mut w = Walk::joined(level);
        let cutscene = w.run.vm.program().global_named("Cutscene State").unwrap();
        for _ in 0..3600 {
            let held = w.run.vm.global_peek(cutscene).map(|v| w.run.vm.value(v));
            if held.and_then(|v| v.as_i32()) == Some(0) {
                break;
            }
            w.still(1);
        }
        let walls: Vec<(String, i32, f32)> = w
            .run
            .files
            .placed
            .iter()
            .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4 && !p.waypoints.is_empty())
            .filter(|p| {
                p.script
                    .as_deref()
                    .is_some_and(|s| s.ends_with("/ElementalWallController.ai"))
            })
            .map(|p| (p.name.clone(), p.id, p.range))
            .collect();
        for (name, element, range) in walls {
            if range <= 0.0 {
                waiting.push(format!("{level}'s {name}"));
                continue;
            }
            let (wall, _) = w.run.placed(&name);
            // Gone with the wall it's linked to (level 201's two death walls).
            if w.gone(wall) {
                println!("{level}'s {name} went with its link");
                continue;
            }
            let nodes = w.wall_nodes(&name);
            let at = nodes[nodes.len() / 2] + Vec3::Y;
            w.be((element + 1) % 8);
            w.run.put(at, 0.0);
            w.still(60);
            assert!(!w.gone(wall), "{level}'s {name} opened for another");
            w.be(element);
            w.run.put(at, 0.0);
            for _ in 0..240 {
                w.still(1);
                if w.gone(wall) {
                    break;
                }
            }
            assert!(w.gone(wall), "{level}'s {name} stood for {element}");
            tried += 1;
        }
        assert!(w.stops.is_empty(), "{level}: {:#?}", w.stops);
        println!("{level}: {:.1} s", t.elapsed().as_secs_f32());
    }
    println!(
        "{tried} walls; on a level's trigger {waiting:?}; {:.1} s, load at end: {}",
        t.elapsed().as_secs_f32(),
        load()
    );
    assert_eq!(tried, 23);
}

/// The AS.153 review: what each of chapter 1's areas holds runs on the
/// listing, the air area's Spinner, the death area's spike spawner and the
/// life area's three Chompies, each woken by a hero come near it. The areas'
/// tests pass without them.
#[test]
#[ignore = "reads the Giants dump"]
fn each_areas_own_placements_wake_for_a_hero_inside() {
    let mut w = Walk::joined(CHAPTER_1);
    let (vista, _) = w.run.placed("213_First_Vista_IGC");
    w.run.vm.remove(vista);
    for name in [
        "Spinner",
        "Spike_Floor_Spawner(1)",
        "Enemy_Chompy_Undead(4)",
        "Enemy_Chompy_Undead(5)",
        "Enemy_Chompy_Undead(6)",
    ] {
        let (h, at) = w.run.placed(name);
        w.run.put(at + Vec3::new(3.0, 2.0, 3.0), 0.0);
        w.still(30);
        let id = w.run.vm.placement_of(h).unwrap();
        println!("{name}: {:?}", w.run.vm.behaviour(id));
        assert!(!w.run.vm.inert().contains(&h), "{name} off the listing");
        assert!(!w.run.vm.asleep(id), "{name} asleep");
    }
}

/// Level 214's earth area (openspec AS.156): its rocks, its tools, its key.
impl Walk {
    /// On the stick to each point in turn, within 1 m, at most 900 steps a
    /// point.
    fn walk_on(&mut self, points: &[[f32; 2]]) {
        for &to in points {
            for _ in 0..900 {
                let at = self.run.hero().pivot;
                if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                    break;
                }
                let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
                let s = self.stick_along(heading);
                self.step(s);
            }
            let at = self.run.hero().pivot;
            assert!(
                (to[0] - at.x).hypot(to[1] - at.z) <= 1.0,
                "walking to {to:?}: stopped at {at}"
            );
        }
    }

    fn behaviour_of(&self, h: Handle) -> Option<String> {
        self.run
            .vm
            .placement_of(h)
            .and_then(|id| self.run.vm.behaviour(id))
            .map(str::to_owned)
    }

    /// The bomb he carries.
    fn carried(&self) -> Option<Handle> {
        self.run
            .world
            .clones()
            .into_iter()
            .filter(|h| self.run.world.name(*h) == "Bomb_Template")
            .filter(|h| !self.run.world.removed(*h))
            .find(|b| self.behaviour_of(*b).as_deref() == Some("following player"))
    }

    /// The earth area's rocks still standing, by name.
    fn rocks(&self) -> Vec<String> {
        self.run
            .files
            .placed
            .iter()
            .filter(|p| p.obj.unit == LEVEL && p.position[0] < -700.0)
            .filter(|p| {
                p.name.starts_with("Rock_Breakable_Full(")
                    || p.name.starts_with("Super_Hard_Rocks(")
            })
            .filter(|p| !self.run.world.taken_off(p.obj))
            .map(|p| p.name.clone())
            .collect()
    }

    /// Toward `rock`, his first attack every 12 steps, close in on a touch
    /// of the stick, until it's gone, at most 900 steps: the rocks it took.
    fn smash(&mut self, rock: &str) -> Vec<String> {
        let (h, at) = self.run.placed(rock);
        let before = self.rocks();
        for k in 0..900 {
            if self.gone(h) {
                break;
            }
            let me = self.run.hero().pivot;
            let heading = (at.x - me.x).atan2(at.z - me.z).to_degrees();
            let s = self.stick_along(heading);
            let s = match (at.x - me.x).hypot(at.z - me.z) > 2.0 {
                true => s,
                false => Stick::new(s.x * 0.3, s.y * 0.3),
            };
            self.act(s, [k % 12 == 0, false, false, false]);
        }
        assert!(
            self.gone(h),
            "{rock} stands, he at {}",
            self.run.hero().pivot
        );
        self.still(60);
        let after = self.rocks();
        before.into_iter().filter(|n| !after.contains(n)).collect()
    }

    /// Facing `to`, △ tapped, then `steps` at rest.
    fn tap_toward(&mut self, to: [f32; 2], steps: u32) {
        let at = self.run.hero().pivot;
        let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
        for _ in 0..6 {
            let s = self.stick_along(heading);
            self.step(Stick::new(s.x * 0.3, s.y * 0.3));
        }
        self.act(Stick::new(0.0, 0.0), [false, false, false, true]);
        self.still(steps);
    }
}

/// openspec AS.156: level 214's earth area past its wall, as Crusher, the
/// earth Giant, plays it. His attacks break the super hard rocks
/// (`Super_hard_Rock.ai` answers his damage's crystal-rock property) that
/// shut the area's bomb spawner off; its bomb, carried and thrown, breaks a
/// rock where it bursts; his attacks clear the corridor, the stairs' way up
/// past the next two hard rocks and the east side; the key behind them is
/// taken, turned in the door (△) and the door opens on the hat box, which
/// mashed open gives its hat: collected in the save, the box gone, the hero
/// walking after. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_earth_area_opens_to_crusher_on_to_its_hat_box() {
    let t = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut w = Walk::joined_as(LEVEL_214, Some(CRUSHER));
    w.session = Some(guacamole::story_save::Session::new(Some(
        "01HOSTPROFILE".into(),
    )));
    assert_eq!(w.element(), EARTH, "Crusher's an earth Skylander");
    let wall = "ElementalWallController(2)";
    let (wall_h, _) = w.run.placed(wall);
    let nodes = w.wall_nodes(wall);
    w.run.put(nodes[nodes.len() / 2] + Vec3::Y, 0.0);
    for _ in 0..240 {
        w.still(1);
        if w.gone(wall_h) {
            break;
        }
    }
    assert!(w.gone(wall_h), "the wall stood for him");
    let all = w.rocks().len();
    // The hard rocks before the bomb spawner, by his attacks.
    w.walk_on(&[[-825.0, -575.0], [-836.0, -577.0]]);
    let hard = w.smash("Super_Hard_Rocks(2)");
    println!("his attack took {hard:?}");
    assert!(hard.iter().any(|n| n.starts_with("Super_Hard")));
    // The bomb, taken and thrown at the corridor's rocks.
    w.walk_on(&[[-848.0, -573.0]]);
    for _ in 0..600 {
        if w.carried().is_some() {
            break;
        }
        let at = w.run.hero().pivot;
        let heading = (-855.5 - at.x).atan2(-571.0 - at.z).to_degrees();
        let s = w.stick_along(heading);
        w.step(s);
    }
    let bomb = w.carried().expect("the area's bomb taken");
    w.walk_on(&[[-846.0, -569.0]]);
    let before = w.rocks();
    w.tap_toward([-836.0, -563.0], 240);
    let blown: Vec<String> = before
        .into_iter()
        .filter(|n| !w.rocks().contains(n))
        .collect();
    println!("the bomb's burst took {blown:?}");
    assert!(!blown.is_empty(), "the bomb broke no rock");
    assert_ne!(w.behaviour_of(bomb).as_deref(), Some("following player"));
    // The corridor north, the stairs up west, the hard rocks there.
    w.walk_on(&[[-836.0, -570.0], [-829.0, -566.0]]);
    for rock in [
        "Rock_Breakable_Full(3)",
        "Rock_Breakable_Full(1)",
        "Rock_Breakable_Full(4)",
        "Rock_Breakable_Full(12)",
    ] {
        if w.rocks().iter().any(|n| n == rock) {
            w.smash(rock);
        }
    }
    w.walk_on(&[[-829.0, -560.0], [-829.0, -552.0]]);
    if w.rocks().iter().any(|n| n == "Rock_Breakable_Full(11)") {
        w.smash("Rock_Breakable_Full(11)");
    }
    w.walk_on(&[
        [-840.0, -549.0],
        [-846.0, -549.0],
        [-850.0, -549.0],
        [-854.0, -549.0],
        [-858.0, -549.0],
        [-860.0, -545.0],
        [-861.0, -540.0],
    ]);
    assert!(w.run.hero().pivot.y > 3.0, "up the stairs");
    if w.rocks().iter().any(|n| n == "Super_Hard_Rocks(5)") {
        w.smash("Super_Hard_Rocks(5)");
    }
    // The key, walked into.
    let key = |w: &Walk| {
        w.run
            .world
            .clones()
            .into_iter()
            .filter(|h| w.run.world.name(*h) == "Key Pickup" && !w.run.world.removed(*h))
            .find(|h| w.run.world.actor(*h).unwrap().position.x < -700.0)
    };
    let k = key(&w).expect("the area's key laid");
    w.walk_on(&[
        [-861.0, -536.0],
        [-861.0, -532.0],
        [-858.0, -527.0],
        [-845.0, -526.0],
        [-833.0, -525.0],
        [-831.0, -524.8],
    ]);
    w.still(60);
    assert_eq!(
        w.behaviour_of(k).as_deref(),
        Some("grabbed by player"),
        "the key"
    );
    // Back down, through the main room east to the door.
    w.walk_on(&[
        [-845.0, -526.0],
        [-858.0, -527.0],
        [-861.0, -532.0],
        [-861.0, -536.0],
        [-860.0, -541.0],
        [-858.0, -549.0],
        [-850.0, -549.0],
        [-840.0, -549.0],
        [-829.0, -552.0],
        [-829.0, -560.0],
        [-829.0, -566.0],
        [-820.0, -570.0],
    ]);
    for rock in [
        "Super_Hard_Rocks(4)",
        "Super_Hard_Rocks(3)",
        "Rock_Breakable_Full(5)",
    ] {
        if w.rocks().iter().any(|n| n == rock) {
            w.smash(rock);
        }
    }
    w.walk_on(&[
        [-806.0, -565.0],
        [-806.0, -561.0],
        [-804.0, -556.0],
        [-798.0, -550.0],
        [-792.0, -546.0],
        [-791.6, -545.0],
    ]);
    let (door, _) = w.run.placed("Door_Template(1)");
    w.tap_toward([-791.6, -542.0], 60);
    for _ in 0..600 {
        if w.behaviour_of(door).as_deref() == Some("done") {
            break;
        }
        w.still(1);
    }
    assert_eq!(w.behaviour_of(door).as_deref(), Some("done"), "the door");
    // The hat box, mashed open, its hat taken.
    let (hat, _) = w.run.placed("HatBox_Pickup(1)");
    let id = w.run.world.actor(hat).unwrap().id;
    w.walk_on(&[[-792.0, -538.0], [-793.0, -531.0]]);
    for k in 0..1800u32 {
        if w.gone(hat) {
            break;
        }
        w.act(Stick::new(0.0, 0.0), [false, false, false, k % 8 < 2]);
    }
    assert!(w.gone(hat), "the hat box: {:?}", w.behaviour_of(hat));
    let collected = w.run.with_host(|vm, host| {
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
    });
    assert_eq!(collected, Some(1), "the hat collected");
    let from = w.run.hero().pivot;
    for _ in 0..90 {
        let s = w.stick_along(180.0);
        w.step(s);
    }
    let walked = (w.run.hero().pivot - from).length();
    println!(
        "{} of {all} rocks left; walked {walked:.1} m after the box; {:.1} s, load at end: {}",
        w.rocks().len(),
        t.elapsed().as_secs_f32(),
        load()
    );
    assert!(walked > 1.0, "the stick dead after the box");
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}

/// openspec AS.156: the earth area's mining pick for any earth hero, not a
/// Giant (Eruptor, his toy's element earth): taken off its spawner by
/// walking up to it, carried over him, and struck with △ at the rock he
/// faces until it's mined away, a size a strike. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_earth_area_pick_mines_a_rock_for_any_earth_hero() {
    println!("load at start: {}", load());
    let mut w = Walk::joined(LEVEL_214);
    w.be(EARTH);
    let wall = "ElementalWallController(2)";
    let nodes = w.wall_nodes(wall);
    w.run.put(nodes[nodes.len() / 2] + Vec3::Y, 0.0);
    w.still(240);
    let pick = |w: &Walk| {
        w.run
            .world
            .clones()
            .into_iter()
            .find(|h| w.run.world.name(*h) == "Mining_Pick" && !w.run.world.removed(*h))
    };
    let p = pick(&w).expect("the pick on its spawner");
    assert_eq!(w.behaviour_of(p).as_deref(), Some("wait to be grabbed"));
    w.walk_on(&[[-825.0, -575.0], [-818.3, -567.0]]);
    for _ in 0..300 {
        if w.behaviour_of(p).as_deref() == Some("following  player") {
            break;
        }
        let at = w.run.hero().pivot;
        let heading = (-818.3 - at.x).atan2(-564.35 - at.z).to_degrees();
        let s = w.stick_along(heading);
        w.step(s);
    }
    assert_eq!(
        w.behaviour_of(p).as_deref(),
        Some("following  player"),
        "taken"
    );
    w.walk_on(&[[-822.0, -566.0], [-824.0, -566.0]]);
    let rock = "Rock_Breakable_Full(3)";
    let (h, at) = w.run.placed(rock);
    // What stands of it: the placement, then the half and the quarter each
    // strike leaves in its place.
    let left = |w: &Walk| -> Vec<String> {
        let mut out: Vec<String> = w
            .run
            .world
            .clones()
            .into_iter()
            .filter(|c| !w.run.world.removed(*c))
            .filter(|c| w.run.world.name(*c).starts_with("Rock_Breakable_"))
            .filter(|c| w.run.world.actor(*c).unwrap().position.distance(at) < 3.0)
            .map(|c| w.run.world.name(c))
            .collect();
        if !w.gone(h) {
            out.push(rock.to_owned());
        }
        out
    };
    let mut strikes = 0;
    let mut mining = false;
    let mut sizes = Vec::new();
    for k in 0..1800u32 {
        let now = left(&w);
        if sizes.last() != Some(&now) {
            sizes.push(now.clone());
        }
        if now.is_empty() {
            break;
        }
        let me = w.run.hero().pivot;
        let heading = (at.x - me.x).atan2(at.z - me.z).to_degrees();
        let s = w.stick_along(heading);
        let tap = k % 8 < 2;
        strikes += u32::from(k % 8 == 0);
        w.act(
            Stick::new(s.x * 0.25, s.y * 0.25),
            [false, false, false, tap],
        );
        mining |= w.behaviour_of(p).as_deref() == Some("mining pick");
    }
    println!(
        "{rock} mined in {strikes} taps, {sizes:?}; load at end: {}",
        load()
    );
    assert!(mining, "the pick never struck");
    assert!(left(&w).is_empty(), "{rock} stands: {sizes:?}");
    assert!(sizes.len() >= 3, "a size a strike: {sizes:?}");
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}
