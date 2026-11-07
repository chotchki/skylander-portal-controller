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
}

impl Walk {
    /// A level from its start with its hero joined and settled.
    fn joined(level: &str) -> Walk {
        let (mut run, _) = run_on_level(
            Mode::Play,
            level,
            Setup {
                interface: true,
                props: true,
                sounds: true,
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
        let mut w = Walk { run, stops };
        w.still(120);
        w
    }

    fn step(&mut self, stick: Stick) {
        self.run.bench.pads[0] = Pad {
            stick,
            actions: [false; 4],
            connected: true,
        };
        let stops = self.run.step_play().unwrap();
        self.stops.extend(stops.iter().map(ToString::to_string));
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
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}
