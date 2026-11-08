//! Chapter 1 from its cave to its end (openspec AU.33), headless with
//! guacamole's host on the TV's listing, every step the level's own scripts.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_chapter_1 -- --ignored --nocapture`
//!
//! No recording of the level past the cave exists: the walk is the one its
//! scripts and its collision leave. He starts in the cave's box, so the pit's
//! fight is `giants_route`'s. The level's people past the cave are met on
//! the way, each drawn, on the ground and in its hat (openspec AS.166).

use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::{Handle, Mode};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const CHAPTER_1: &str = "level_213_tutotial";

struct Walk {
    run: Run,
    stops: Vec<String>,
    people: Vec<Met>,
    hats: Vec<String>,
}

const TURBAN: &str = "Turban_hat";
const BEANIE: &str = "Propellar_Beenie";

/// Chapter 1's people past its first cave (openspec AS.166), each with the
/// hat its script puts on it, if any.
const PEOPLE: [(&str, Option<&str>); 13] = [
    ("1.NPC_Snuckels", Some(BEANIE)),
    ("Slave_Carry(2)", None),
    ("3.NPC_Norticus", Some(TURBAN)),
    ("3.NPC_Rizzo_Life_Elemental_Area", Some(BEANIE)),
    ("4.NPC_Norticus at end", Some(TURBAN)),
    ("Slave_Air_Element", Some(BEANIE)),
    ("Slave_1_hammer(2)", None),
    ("Slave_1_hammer(4)", None),
    ("Slave_Carry(1)", None),
    ("Slave_Carry(4)", None),
    ("Slave_1_hammer", None),
    ("Slave_Carry", None),
    ("Slave_Death_Element", Some(BEANIE)),
];

/// How near the hero a person is met: within its 150 ft range, and as near
/// as the way goes by the furthest of them (the death area's Mabu, 32 m).
const MEETS: f32 = 35.0;

/// One of [`PEOPLE`] as the walk met it: drawn by the TV within [`MEETS`]
/// of the hero (among its people, its script running, on and not taken
/// off), the behaviours its script went through while so, the furthest it
/// stood off the ground under it, and over none (three of the quarry's Mabu
/// stand on its art past its collision) the furthest from its placement.
#[derive(Debug, Default)]
struct Met {
    first: Option<(u32, Vec3)>,
    behaviours: Vec<String>,
    off_ground: f32,
    off_placement: f32,
    hats: Vec<String>,
}

impl Walk {
    fn step(&mut self, stick: Stick, actions: [bool; 4]) {
        self.run.bench.pads[0] = Pad {
            stick,
            actions,
            connected: true,
        };
        let stopped = self.run.step_play().unwrap();
        self.stops.extend(stopped.iter().map(ToString::to_string));
        if self.run.steps % 10 == 0 {
            self.watch_people();
        }
    }

    fn still(&mut self, steps: u32) {
        for _ in 0..steps {
            self.step(Stick::new(0.0, 0.0), [false; 4]);
        }
    }

    fn stick_to(&self, to: [f32; 2]) -> Stick {
        self.stick_at(to, 1.0)
    }

    fn stick_at(&self, to: [f32; 2], push: f32) -> Stick {
        let at = self.run.hero().pivot;
        let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
        let d = (heading - self.run.bench.view).to_radians();
        Stick::new(-d.sin() * push, d.cos() * push)
    }

    /// On the stick to each point in turn, a scene holding him waited out.
    fn walk(&mut self, points: &[[f32; 2]]) {
        for &to in points {
            for _ in 0..1800 {
                let at = self.run.hero().pivot;
                if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                    break;
                }
                let s = self.stick_to(to);
                self.step(s, [false; 4]);
            }
            let at = self.run.hero().pivot;
            assert!(
                (to[0] - at.x).hypot(to[1] - at.z) <= 1.0,
                "walking to {to:?}: stopped at {at}"
            );
        }
    }

    /// On the stick while `until` doesn't hold, at most `steps`.
    fn head(&mut self, to: [f32; 2], steps: u32, until: impl Fn(Vec3) -> bool) {
        for _ in 0..steps {
            if until(self.run.hero().pivot) {
                return;
            }
            let s = self.stick_to(to);
            self.step(s, [false; 4]);
        }
        panic!("heading for {to:?}: at {}", self.run.hero().pivot);
    }

    fn gone(&self, h: Handle) -> bool {
        self.run.world.actor(h).is_none_or(|a| a.removed)
            || self
                .run
                .vm
                .placement_of(h)
                .is_none_or(|id| self.run.vm.removed(id))
    }

    fn awake(&self, h: Handle) -> bool {
        self.run
            .vm
            .placement_of(h)
            .is_some_and(|id| !self.run.vm.asleep(id))
    }

    /// Toward the nearest of `foes` awake and within a storey of him, his
    /// first attack mashed within 5 m, until `over`; pressed up against one
    /// and getting nowhere (a lob at arm's length goes over it, the mouth of
    /// its closet holding it where it stands) he backs off a way, as a hand
    /// on the pad would.
    fn fight(&mut self, foes: &[Handle], over: impl Fn(&Walk) -> bool, steps: u32) {
        let mut pressed = 0;
        for k in 0..steps {
            if over(self) {
                return;
            }
            let me = self.run.hero().pivot;
            let near = foes
                .iter()
                .filter(|h| !self.gone(**h) && self.awake(**h))
                .filter_map(|h| self.run.world.actor(*h).map(|a| a.position))
                .filter(|p| (p.y - me.y).abs() < 3.6)
                .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
            let Some(near) = near else {
                self.still(1);
                continue;
            };
            let far = (near.x - me.x).hypot(near.z - me.z);
            pressed = if far <= 2.5 { pressed + 1 } else { 0 };
            if pressed > 240 {
                let away = [me.x + (me.x - near.x) * 4.0, me.z + (me.z - near.z) * 4.0];
                for _ in 0..30 {
                    let s = self.stick_at(away, 1.0);
                    self.step(s, [false; 4]);
                }
                pressed = 0;
                continue;
            }
            // Close in full, then turn to it on a touch of the stick.
            let push = if far > 2.5 { 1.0 } else { 0.35 };
            let s = self.stick_at([near.x, near.z], push);
            self.step(s, [far <= 5.0 && k % 20 == 0, false, false, false]);
        }
        panic!(
            "the fight's not over at {}: {:?}",
            self.run.hero().pivot,
            foes.iter()
                .filter(|h| !self.gone(**h))
                .filter_map(|h| self.run.world.actor(*h).map(|a| a.position))
                .collect::<Vec<_>>()
        );
    }

    /// His first attack until the post gives (its behaviour leaves its wait)
    /// and the cutscene it starts plays out.
    fn break_post(&mut self, name: &str, steps: u32) {
        let id = self.run.placement(name);
        for k in 0..steps {
            if self.run.vm.behaviour(id) == Some("wait for cs") {
                break;
            }
            self.step(Stick::new(0.0, 0.0), [k % 7 == 0, false, false, false]);
        }
        assert_eq!(self.run.vm.behaviour(id), Some("wait for cs"), "{name}");
        self.scene_out();
        assert!(self.run.vm.removed(id) || self.run.vm.behaviour(id) != Some("wait for cs"));
    }

    fn scene_out(&mut self) {
        for _ in 0..3000 {
            if cutscene_name(&self.run).is_none() {
                self.still(30);
                if cutscene_name(&self.run).is_none() {
                    return;
                }
            }
            self.still(1);
        }
        panic!("{:?} didn't end", cutscene_name(&self.run));
    }

    /// Each of [`PEOPLE`] drawn near him, what it wears and where it stands
    /// (openspec AS.166), and every hat on someone: a hat whose wearer is
    /// gone or taken off would hang over nothing (the first cave's turban
    /// lay on the floor with no Norticus until AS.105).
    fn watch_people(&mut self) {
        let me = self.run.hero().pivot;
        let world = &self.run.world;
        let vm = &self.run.vm;
        let drawn = world.people();
        let inert = vm.inert();
        let live = |h: Handle| world.actor(h).is_some_and(|a| a.active && !a.removed);
        let hats: Vec<(Handle, String, Option<Handle>)> = world
            .clones()
            .into_iter()
            .filter(|h| live(*h))
            .filter_map(|h| {
                let n = world.name(h);
                [TURBAN, BEANIE].contains(&n.as_str()).then(|| {
                    let owner = world.node_owner(world.actor(h).unwrap().attach);
                    (h, n, owner)
                })
            })
            .collect();
        for (h, n, owner) in &hats {
            // The outro's three Mabu leave theirs as the scene takes them
            // off: their script takes no hat off, as the others' do.
            let outro = owner.is_some_and(|o| world.name(o).starts_with("213_Outro_Mabu_"));
            if !owner.is_some_and(live) && !outro {
                self.hats.push(format!(
                    "step {}: {n} {h} on {:?}, at {}",
                    self.run.steps,
                    owner.map(|o| world.name(o)),
                    world.actor(*h).unwrap().position
                ));
            }
        }
        let bodies: Vec<guac_motion::seam::BodyId> = self
            .run
            .bench
            .bodies
            .values()
            .chain(self.run.bench.ids.values())
            .copied()
            .collect();
        for (k, (name, hat)) in PEOPLE.iter().enumerate() {
            let (h, placed) = self.run.placed(name);
            let a = world.actor(h).unwrap();
            let running = vm
                .placement_of(h)
                .is_some_and(|id| !vm.removed(id) && !inert.contains(&h));
            if !(drawn.contains(&h) && running && live(h)) || a.position.distance(me) > MEETS {
                continue;
            }
            let met = &mut self.people[k];
            met.first.get_or_insert((self.run.steps, a.position));
            let b = vm
                .placement_of(h)
                .and_then(|id| vm.behaviour(id))
                .unwrap_or("-");
            if met.behaviours.last().is_none_or(|l| l != b) {
                met.behaviours.push(b.to_owned());
            }
            let ground = guac_motion::ground::ray(
                &self.run.bench.world,
                a.position + Vec3::Y * 2.0,
                Vec3::NEG_Y,
                6.0,
                &bodies,
            )
            .map(|hit| (a.position.y - hit.point.y).abs());
            match ground {
                Some(off) => met.off_ground = met.off_ground.max(off),
                None => met.off_placement = met.off_placement.max(a.position.distance(placed)),
            }
            let worn = hats.iter().filter(|(_, _, o)| *o == Some(h)).count();
            let wants = usize::from(hat.is_some());
            let wearing = hats
                .iter()
                .filter(|(_, n, o)| *o == Some(h) && Some(n.as_str()) == *hat)
                .count();
            if worn != wants || wearing != wants {
                met.hats
                    .push(format!("step {}: {worn} hats", self.run.steps));
            }
        }
    }

    fn foes(&self, names: impl IntoIterator<Item = String>) -> Vec<Handle> {
        names.into_iter().map(|n| self.run.placed(&n).0).collect()
    }
}

fn into_the_cave() -> Walk {
    let (mut run, _) = run_on_level(
        Mode::Play,
        CHAPTER_1,
        Setup {
            interface: true,
            props: true,
            sounds: true,
            ..Setup::default()
        },
    );
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap();
    let mut w = Walk {
        run,
        stops: Vec::new(),
        people: PEOPLE.iter().map(|_| Met::default()).collect(),
        hats: Vec::new(),
    };
    w.still(60);
    w.run.put(Vec3::new(15.6, 22.4, -31.0), 90.0);
    w.still(120);
    assert!(w.run.hero().pivot.x < -900.0, "the cave's interior");
    w
}

/// The first cave: its cutscene frees the Giant; near him he smashes the
/// cracked floor and goes; the way round by the lower ledge drops the hero
/// through it to the second cave. Norticus leads him by the way his
/// waypoints go and stands at the last of them, (-913.4, -299.0), so the
/// walk goes by him to the floor (openspec AS.105: he's the level's people).
fn through_the_first_cave(w: &mut Walk) {
    assert_eq!(cutscene_name(&w.run).as_deref(), Some("213 gigantus igc"));
    w.scene_out();
    w.walk(&[
        [-919.6, -321.0],
        [-921.9, -311.6],
        [-918.2, -301.6],
        [-908.0, -285.0],
        [-908.0, -274.0],
    ]);
    let floor = w.run.placed("Titan_Floor(Template)(1)").0;
    for _ in 0..1500 {
        if w.gone(floor) {
            break;
        }
        w.still(1);
    }
    assert!(w.gone(floor), "the Giant's smash");
    w.still(120);
    w.walk(&[
        [-908.0, -250.0],
        [-907.0, -232.0],
        [-895.0, -226.0],
        [-880.0, -225.0],
        [-876.0, -240.0],
        [-878.0, -262.0],
        [-890.0, -264.0],
    ]);
    w.head([-896.3, -265.5], 200, |at| at.x > -800.0);
    w.still(90);
    assert!(
        (w.run.hero().pivot.x + 669.1).abs() < 1.0,
        "the second cave"
    );
}

/// The second cave: Setup_002's twenty Chompies, out of the two mouths their
/// 6 ft spheres are wider than (a solve that gave up left eight stood there
/// for good: `a_shape_wider_than_the_gap_goes_on_along_it`), its Arkeyan, the
/// gate, the pads to the exit.
fn through_the_second_cave(w: &mut Walk) {
    w.still(800);
    let mut names: Vec<String> = (11..31).map(|n| format!("Enemy_Chompy({n})")).collect();
    names.push("PandoranGruntSimple(1)".to_owned());
    let foes = w.foes(names);
    let controller = w.run.placed("Setup_002_Controller").0;
    w.fight(&foes, |w| w.gone(controller), 6000);
    w.still(120);
    let gate = w.run.placement("Spike_Gate_Wooden_Template(2)");
    assert_eq!(
        w.run.vm.behaviour(gate),
        Some("spikes in down position - wait to be raised")
    );
    w.walk(&[[-645.0, -234.0]]);
    w.scene_out();
    // The pads launch him along his facing, so each is walked over heading
    // the way on; a fall short is walked round again, as a hand would.
    for _ in 0..3600 {
        let at = w.run.hero().pivot;
        if at.y > 41.0 {
            break;
        }
        let to = match (at.x, at.y, at.z) {
            (_, y, _) if y < 19.5 => [-636.4, -228.0],
            (x, y, z) if x > -625.0 && y > 28.0 => match (z + 220.2).abs() < 1.2 {
                true => [-610.0, -220.2],
                false => [-621.0, -220.2],
            },
            (x, y, z) if y > 27.0 && z > -218.0 && x < -626.0 => match x > -637.0 {
                true => [-620.0, -216.5],
                false => [-636.0, -212.5],
            },
            (x, y, _) if y > 25.0 && x < -626.0 => [-636.0, -212.5],
            (x, y, _) if y > 22.0 && x < -626.0 => [-638.4, -218.5],
            (_, y, _) if y > 22.0 => [-640.0, -222.0],
            _ => [-636.4, -228.0],
        };
        let s = w.stick_to(to);
        w.step(s, [false; 4]);
    }
    assert!(
        w.run.hero().pivot.y > 41.0,
        "up the pads: {}",
        w.run.hero().pivot
    );
    w.head([-606.0, -220.0], 300, |at| at.x > -611.0);
    w.still(200);
    w.walk(&[[-595.0, -219.0], [-588.0, -218.0]]);
    w.head([-583.0, -217.0], 300, |at| at.y < -100.0);
}

/// The quarry: the vista, the first post, Setup_003, the two blocks,
/// Setup_004, the pad up to the second post (asleep for good if the vista's
/// reset left its range at 0), Setup_005, the last post. Norticus leads the
/// hero up from the air area and stands at the top of the slope, at his
/// last waypoint, (-47.7, 194.4), so the walk goes by him (openspec AS.166:
/// he's the level's people, on the ground of his walk).
fn through_the_quarry(w: &mut Walk) {
    w.still(120);
    assert_eq!(
        cutscene_name(&w.run).as_deref(),
        Some("213 First Vista IGC")
    );
    w.scene_out();
    // The air wall's hint, for a hero not of its element, on the level's
    // own way there (the AS.153 review: every area's zone title before it
    // left the announce container hidden for good). Off the way, and back.
    w.walk(&[[-38.0, 132.0], [-40.5, 124.0]]);
    let hint = guacamole::announcements::hints(&w.run.vm, &w.run.world);
    println!("the air wall's hints: {}", hint.len());
    let word =
        |l: &guacamole::announcements::Line| l.text.as_deref().is_some_and(|t| t.contains("Air"));
    assert!(hint.iter().any(word), "no air hint at the wall");
    w.walk(&[
        [-38.0, 132.0],
        [-43.6, 147.6],
        [-46.0, 163.0],
        [-47.6, 175.0],
        [-49.0, 188.0],
        [-50.0, 205.0],
        [-52.0, 221.0],
    ]);
    w.head([-52.3, 222.6], 100, |at| at.z > 221.6);
    w.break_post("Rope_around_Post", 600);
    w.still(90);
    w.walk(&[
        [-62.0, 211.0],
        [-72.0, 214.0],
        [-81.0, 219.0],
        [-90.0, 226.0],
    ]);
    let s3 = w.foes((8..11).map(|n| format!("Enemy_Chompy({n})")));
    let chompies = s3.clone();
    w.fight(&s3, move |w| chompies.iter().all(|h| w.gone(*h)), 2400);
    // The two blocks: the first south-west, the second south-east.
    w.walk(&[[-103.0, 233.0]]);
    w.head([-112.0, 243.0], 500, |at| at.z > 240.5 && at.x < -110.0);
    w.walk(&[[-104.5, 236.0], [-108.8, 238.7], [-110.3, 240.8]]);
    let block = w.run.placed("Push_Block_Template(7)").0;
    let was = w.run.world.actor(block).unwrap().position;
    for _ in 0..500 {
        if (w.run.world.actor(block).unwrap().position - was).length() > 4.0 {
            break;
        }
        let s = w.stick_to([-99.0, 248.3]);
        w.step(s, [false; 4]);
    }
    assert!((w.run.world.actor(block).unwrap().position - was).length() > 4.0);
    w.walk(&[
        [-108.0, 244.0],
        [-109.0, 249.0],
        [-112.0, 257.0],
        [-115.0, 268.0],
    ]);
    let s4 = w.foes((2..4).map(|n| format!("PandoranGruntSimple({n})")));
    let controller = w.run.placed("Setup_004_Controller").0;
    w.fight(&s4, |w| w.gone(controller), 2400);
    w.scene_out();
    w.still(200);
    w.walk(&[[-117.0, 288.0], [-123.2, 295.0]]);
    w.head([-112.5, 294.8], 400, |at| at.y > -132.0);
    w.head([-103.0, 300.0], 400, |at| at.y < -139.0 && at.x > -108.0);
    w.walk(&[[-100.3, 301.6]]);
    w.break_post("Rope_around_Post(1)", 600);
    w.still(200);
    w.walk(&[
        [-94.5, 309.0],
        [-93.5, 316.0],
        [-89.0, 317.0],
        [-82.0, 319.0],
        [-76.0, 322.0],
        [-66.0, 326.0],
        [-56.0, 326.0],
    ]);
    let mut names: Vec<String> = [5, 6, 7]
        .iter()
        .map(|n| format!("PandoranGruntSimple({n})"))
        .collect();
    names.extend((1..4).map(|n| format!("Enemy_Chompy_Undead({n})")));
    let s5 = w.foes(names);
    let controller = w.run.placed("Setup_005_Controller").0;
    w.fight(&s5, |w| w.gone(controller), 4000);
    w.still(120);
    w.walk(&[[-44.0, 321.0], [-34.0, 319.5], [-28.0, 312.0]]);
    w.head([-28.8, 306.0], 200, |at| at.z < 308.5);
    w.break_post("Rope_around_Post(2)", 2400);
}

#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1_is_walked_from_its_cave_to_its_end() {
    let t = std::time::Instant::now();
    let mut w = into_the_cave();
    through_the_first_cave(&mut w);
    println!(
        "second cave at step {}, {}",
        w.run.steps,
        w.run.hero().pivot
    );
    through_the_second_cave(&mut w);
    println!("the quarry at step {}, {}", w.run.steps, w.run.hero().pivot);
    through_the_quarry(&mut w);
    println!(
        "the last post at step {}, {}",
        w.run.steps,
        w.run.hero().pivot
    );
    // The last post wakes the end, whose fade wakes the outro.
    let ender = w.run.placed("Temp_Level_Ender").0;
    for _ in 0..3000 {
        if w.gone(ender) {
            break;
        }
        w.still(1);
    }
    assert!(w.gone(ender), "the level's end");
    w.still(60);
    println!(
        "over at step {}, health {:?}, {:.1} s, load {}",
        w.run.steps,
        w.run.health(),
        t.elapsed().as_secs_f32(),
        load()
    );
    assert_eq!(
        w.run.world.level_change().map(|c| c.name.as_str()),
        Some("level_214_tutotial_2")
    );
    people_met(&w);
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
}

/// openspec AS.166: each of chapter 1's people past its first cave met on
/// the way, drawn on its model, standing on the ground (within 0.3 m), or
/// over none where it's placed, and in its hat, no hat anywhere hanging on no one; Norticus at the top of the
/// second cave's pads leads the hero on and goes, his turban with him;
/// Norticus at the quarry leads him up its slope and stands at the top of
/// it, at his last waypoint; the Mabu carrying by the pit turns to him.
fn people_met(w: &Walk) {
    let skinned: Vec<String> = skinned_models().into_iter().map(|(p, _)| p).collect();
    for hat in [TURBAN, BEANIE] {
        assert!(clones_drawn(&w.run, &skinned, hat), "{hat} drawn");
    }
    for ((name, _), met) in PEOPLE.iter().zip(&w.people) {
        println!("{name}: {met:?}");
        let (at, _) = met.first.unwrap_or_else(|| panic!("{name} never met"));
        let model = w
            .run
            .files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == *name)
            .and_then(|p| p.model.clone())
            .unwrap();
        assert!(skinned.contains(&model), "{name}'s {model} drawn");
        assert!(met.off_ground < 0.3, "{name} off the ground at {at:?}");
        assert!(met.off_placement < 0.05, "{name} off its placement");
        assert!(met.hats.is_empty(), "{name}: {:?}", met.hats);
    }
    assert!(w.hats.is_empty(), "{:#?}", w.hats);
    let behaviours = |name: &str| {
        let k = PEOPLE.iter().position(|(n, _)| *n == name).unwrap();
        w.people[k].behaviours.clone()
    };
    assert!(
        behaviours("3.NPC_Norticus").contains(&"advance with player".to_owned()),
        "{:?}",
        behaviours("3.NPC_Norticus")
    );
    assert!(w.gone(w.run.placed("3.NPC_Norticus").0), "he's gone on");
    let end = w.run.placed("4.NPC_Norticus at end").0;
    assert_eq!(
        behaviours("4.NPC_Norticus at end")
            .last()
            .map(String::as_str),
        Some("flynn end talker")
    );
    let at = w.run.world.actor(end).unwrap().position;
    println!("Norticus at the quarry's top at {at}");
    assert!(
        (at.x + 47.68).hypot(at.z - 194.37) < 1.0 && (at.y + 128.57).abs() < 0.3,
        "at his last waypoint: {at}"
    );
    assert!(behaviours("Slave_Carry(2)").contains(&"turn to player".to_owned()));
}
