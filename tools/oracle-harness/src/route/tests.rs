//! The driver on a fake world: the hero and the camera kept in
//! [`crate::actors::fixture`]'s memory and read back through
//! [`crate::actors::read`], as the live run reads the oracle's, and moved
//! flip by flip by the walk the module doc gives (AG.8, AG.38).

use super::*;
use crate::actors::fixture::{Added, Spec, World};
use crate::actors::{self};

const DT: f64 = 1.0 / 60.0;
const SPAWN: Point = Point {
    x: 10.19027,
    z: -93.39406,
};

/// A world the driver can't tell from the oracle by what it reads.
struct Fake {
    world: World,
    hero: Added,
    flip: u64,
    game_frame: u32,
    pos: [f64; 3],
    /// His walk across the ground, m/s.
    vel: [f64; 2],
    facing: f64,
    view: f64,
    /// Degrees the view turns for every metre he walks (the sheep's ground's
    /// view turns as he crosses it, AG.36).
    view_turn: f64,
    /// The eye closes on a point behind him 8 % a frame (AG.13's law), so it
    /// keeps moving a while after he stops.
    eye: [f64; 3],
    /// Discs his pivot can't enter: a step into one is refused and stops him.
    walls: Vec<(Point, f64)>,
    /// What port 0 held at each flip, from the first flip after F0.
    held: Vec<PadState>,
    shots: Vec<(String, u64)>,
    reads: Vec<(String, u64)>,
    /// The flips each run went in at and lasted.
    runs: Vec<(u64, u64)>,
}

impl Fake {
    fn new(at: Point, view: f64) -> Self {
        let mut world = World::new();
        let hero = world.actor(&Spec {
            name: "009_Eruptor".into(),
            position: [at.x as f32, 43.38, at.z as f32],
            scale: [150.0; 3],
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        let mut fake = Self {
            world,
            hero,
            flip: 2,
            game_frame: 5_962,
            pos: [at.x, 43.38, at.z],
            vel: [0.0; 2],
            facing: 0.0,
            view,
            view_turn: 0.0,
            eye: [0.0; 3],
            walls: Vec::new(),
            held: Vec::new(),
            shots: Vec::new(),
            reads: Vec::new(),
            runs: Vec::new(),
        };
        fake.eye = fake.follow();
        fake.write();
        fake
    }

    /// The point the eye closes on: behind him along the view, about where
    /// AF.8's camera sits from the spawn, so the view follows his walk.
    fn follow(&self) -> [f64; 3] {
        let (s, c) = self.view.to_radians().sin_cos();
        [
            self.pos[0] - 14.0 * s,
            self.pos[1] + 9.0,
            self.pos[2] - 14.0 * c,
        ]
    }

    fn write(&mut self) {
        let p = self.hero.placement;
        let pos = self.pos.map(|v| v as f32);
        self.world.put_floats(p + 0x24, &pos);
        self.world.put_floats(p + 0x3C, &pos);
        self.world
            .put_floats(p + 0x48, &[0.0, self.facing as f32, 0.0]);
        self.world.put_floats(
            self.hero.corpus + 0x58,
            &[self.vel[0] as f32, 0.0, self.vel[1] as f32],
        );
        self.world
            .camera(self.eye.map(|v| v as f32), [-23.36, self.view as f32, 0.0]);
        let app = self.world.app;
        self.world.put32(app + 0x5C, self.game_frame);
    }

    /// A game frame: the walk turned and pushed or slowed by `pad`, his facing
    /// turned, the step taken unless a wall refuses it, the view and the eye
    /// moved, all written back to the memory the reader reads.
    fn step(&mut self, pad: &PadState) {
        let k = Knobs::default();
        let (s, h) = push_of(pad.left, self.view);
        let speed = self.vel[0].hypot(self.vel[1]);
        let speed = if s > 0.0 {
            let (sin, cos) = h.to_radians().sin_cos();
            let speed = (speed + 160.0 * FOOT * s * DT).min((k.top * s).max(speed));
            self.vel = [speed * sin, speed * cos];
            let err = wrap_deg(h - self.facing);
            let most = k.turn_rate * s * DT;
            self.facing = if err.abs() <= most {
                h
            } else {
                self.facing + most.copysign(err)
            }
            .rem_euclid(360.0);
            speed
        } else {
            let left = (speed - k.let_go * DT).max(0.0);
            if speed > 0.0 {
                self.vel = self.vel.map(|v| v * left / speed);
            }
            left
        };
        let next = [
            self.pos[0] + self.vel[0] * DT,
            self.pos[2] + self.vel[1] * DT,
        ];
        let blocked = self
            .walls
            .iter()
            .any(|(c, r)| (next[0] - c.x).hypot(next[1] - c.z) < *r);
        if blocked {
            self.vel = [0.0; 2];
        } else {
            self.view += self.view_turn * speed * DT;
            self.pos[0] = next[0];
            self.pos[2] = next[1];
        }
        let to = self.follow();
        for (eye, to) in self.eye.iter_mut().zip(to) {
            *eye += (to - *eye) * 0.08;
        }
        self.flip += 1;
        self.game_frame += 1;
        self.held.push(pad.clone());
        self.write();
    }
}

impl Session for Fake {
    fn look(&mut self) -> Result<(u64, ActorFrame)> {
        Ok((self.flip, actors::read(&mut self.world)?))
    }

    fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
        assert_eq!(
            script.state_at(1, 0),
            Some(PadState::default()),
            "port 1 rests from the first flip"
        );
        self.runs.push((self.flip + 1, flips));
        for k in 0..flips {
            let pad = script.state_at(0, k).expect("port 0 from offset 0");
            self.step(&pad);
        }
        Ok(())
    }

    fn shot(&mut self, label: &str) -> Result<()> {
        self.shots.push((label.to_owned(), self.flip));
        Ok(())
    }

    fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
        assert_eq!(frame.hero()?.corpus, self.hero.corpus);
        self.reads.push((label.to_owned(), self.flip));
        Ok(())
    }
}

fn plan(steps: Vec<Step>) -> RoutePlan {
    RoutePlan { steps, save: false }
}

fn via(x: f64, z: f64) -> Step {
    Step::Via {
        to: Point { x, z },
        radius: 1.0,
    }
}

fn stop(x: f64, z: f64, radius: f64) -> Step {
    Step::Stop {
        at: Point { x, z },
        radius,
    }
}

fn ground(f: &Fake, p: Point) -> f64 {
    (f.pos[0] - p.x).hypot(f.pos[2] - p.z)
}

#[test]
fn the_stick_toward_a_heading_is_the_games_read_turned_round() {
    // AG.7's walk: (0, 70) against the start's 15° view walks him along 89.85°.
    let (s, h) = push_of(Stick { x: 0, y: 70 }, 15.0);
    assert!((h - 89.85).abs() < 0.01, "{h}");
    assert!((s - 1.0).abs() < 1e-9);
    // Forward is the view, full left its +90, and a quarter of the travel
    // is centred.
    assert_eq!(push_of(Stick { x: 128, y: 0 }, 15.0), (1.0, 15.0));
    assert!((push_of(Stick { x: 0, y: 128 }, 15.0).1 - 105.0).abs() < 1e-9);
    assert_eq!(push_of(Stick { x: 160, y: 96 }, 15.0), (0.0, 0.0));
    assert_eq!(stick_toward(123.0, 15.0, 0.0), Stick::CENTRED);
    // Round the clock, against views round the clock: back through the
    // game's read to about a degree at a quarter of the stick (coarsest
    // along an axis, where the other axis is a step or two past the dead
    // zone), to about half a degree from half up, and to the bytes' coarseness
    // near the dead zone.
    for view in (0..360).step_by(37).map(f64::from) {
        for heading in (0..360).step_by(7).map(f64::from) {
            for (strength, deg, by) in [
                (0.05, 6.0, 0.04),
                (0.25, 1.3, 0.03),
                (0.5, 0.7, 0.03),
                (0.7, 0.6, 0.03),
                (1.0, 0.6, 0.03),
            ] {
                let (s, h) = push_of(stick_toward(heading, view, strength), view);
                assert!(
                    wrap_deg(h - heading).abs() < deg,
                    "{heading} against {view} at {strength}: {h}"
                );
                assert!(
                    (s - strength).abs() < by,
                    "{heading} against {view} at {strength}: {s}"
                );
            }
        }
    }
}

#[test]
fn the_driver_walks_its_points_and_stops_within_the_radius() {
    // AG.9's corridor walk and on to the ledge route, the view turning as he
    // goes (its own number, not the level's).
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.view_turn = -1.5;
    let end = Point { x: 32.4, z: -74.7 };
    let route = plan(vec![
        via(24.2, -88.7),
        via(36.8, -83.7),
        via(44.5, -79.6),
        via(41.0, -71.5),
        stop(end.x, end.z, 0.1),
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done, "{:#?}", trace.looks.last());
    assert!(ground(&fake, end) <= 0.1, "{:?}", fake.pos);
    assert_eq!(fake.vel, [0.0; 2]);
    assert!(fake.view < 15.0 - 20.0, "the view turned: {}", fake.view);
    // Every pause is a look, the last the end; the looks thin out near the
    // points (every 2 flips) and stay 8 apart far from them.
    let last = trace.looks.last().unwrap();
    assert_eq!(last.did, "done");
    assert_eq!(last.seen.flip, fake.flip);
    assert_eq!(trace.flips, fake.flip - 2);
    assert!(fake.runs.iter().any(|(_, f)| *f == 8));
    assert!(fake.runs.iter().any(|(_, f)| *f == 2));
    // Each pass point came within its metre on the way.
    for p in [(24.2, -88.7), (36.8, -83.7), (44.5, -79.6), (41.0, -71.5)] {
        let nearest = trace
            .looks
            .iter()
            .map(|l| l.seen.distance(Point { x: p.0, z: p.1 }))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest <= 1.0 + 0.9, "{p:?}: {nearest}");
    }
}

#[test]
fn the_recorded_input_takes_a_fresh_world_to_the_same_place_without_a_pause() {
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.view_turn = -1.5;
    let route = plan(vec![
        via(24.2, -88.7),
        stop(30.0, -86.0, 0.15),
        Step::Face {
            toward: Point { x: 30.0, z: -80.0 },
            within: 1.0,
            off: 0.0,
        },
        Step::Settle { flips: 30 },
        Step::Press {
            buttons: [Button::South].into(),
            flips: 6,
        },
        Step::Wait { flips: 40 },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    let script = trace.input.clone().unwrap();
    assert_eq!(
        InputScript::from_ron(&trace.input_ron).unwrap(),
        script,
        "the RON is the script"
    );
    // The script holds what port 0 held at every flip, and port 1 rests.
    for (k, held) in fake.held.iter().enumerate() {
        assert_eq!(
            script.state_at(0, k as u64).as_ref(),
            Some(held),
            "flip {k}"
        );
        assert_eq!(script.state_at(1, k as u64), Some(PadState::default()));
    }
    // So, run open loop from the spawn, it lands him where the drive did.
    let mut again = Fake::new(SPAWN, 15.0);
    again.view_turn = -1.5;
    again.run(&script, trace.flips).unwrap();
    assert_eq!(again.pos, fake.pos);
    assert_eq!(again.facing, fake.facing);
    assert_eq!(again.held, fake.held);
    // The press is in it once, 6 flips long.
    let presses: Vec<_> = script.entries()[0]
        .iter()
        .filter(|e| e.pad.buttons.contains(&Button::South))
        .map(|e| e.at)
        .collect();
    assert_eq!(presses.len(), 1);
    assert_eq!(
        script.state_at(0, presses[0] + 6),
        Some(PadState::default())
    );
}

#[test]
fn a_point_behind_a_wall_is_given_up_with_how_near_he_got() {
    // A wall across the way: the point beyond it can't be reached.
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.walls = (0..30)
        .map(|i| {
            (
                Point {
                    x: 14.0,
                    z: -100.0 + f64::from(i) * 0.5,
                },
                0.9,
            )
        })
        .collect();
    let knobs = Knobs::default();
    let route = plan(vec![
        via(12.0, -90.0),
        stop(18.0, -92.0, 0.1),
        Step::Shot {
            label: "never".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, knobs).unwrap();
    let Outcome::Stuck(Stuck::NoProgress {
        step,
        best,
        best_at,
        flip,
        ..
    }) = trace.outcome.clone()
    else {
        panic!("{:?}", trace.outcome);
    };
    assert_eq!(step, 1);
    assert!(best > 3.0 && best < 5.5, "{best}");
    assert!(flip - best_at > knobs.patience);
    assert!(flip - best_at <= knobs.patience + knobs.every_far);
    assert!(fake.shots.is_empty(), "nothing after the stuck step ran");
    assert!(
        trace
            .looks
            .last()
            .unwrap()
            .did
            .starts_with("gave up: step 1")
    );
    // A pass point inside the wall is given up the same way.
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.walls = vec![(Point { x: 16.0, z: -93.0 }, 2.0)];
    let trace = drive(&mut fake, &plan(vec![via(16.0, -93.0)]), knobs).unwrap();
    assert!(
        matches!(
            trace.outcome,
            Outcome::Stuck(Stuck::NoProgress { step: 0, .. })
        ),
        "{:?}",
        trace.outcome
    );
}

#[test]
fn a_step_that_never_settles_is_given_up_at_its_budget() {
    // A view that never stops turning keeps the eye moving.
    let mut fake = Fake::new(SPAWN, 15.0);
    let knobs = Knobs {
        budget: 300,
        ..Knobs::default()
    };
    struct Spin<'a>(&'a mut Fake);
    impl Session for Spin<'_> {
        fn look(&mut self) -> Result<(u64, ActorFrame)> {
            self.0.look()
        }
        fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
            self.0.view += 0.5 * flips as f64;
            self.0.run(script, flips)
        }
        fn shot(&mut self, label: &str) -> Result<()> {
            self.0.shot(label)
        }
        fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
            self.0.read(label, frame)
        }
    }
    let trace = drive(
        &mut Spin(&mut fake),
        &plan(vec![Step::Settle { flips: 30 }]),
        knobs,
    )
    .unwrap();
    assert!(
        matches!(
            trace.outcome,
            Outcome::Stuck(Stuck::Budget {
                step: 0,
                budget: 300,
                ..
            })
        ),
        "{:?}",
        trace.outcome
    );
}

#[test]
fn face_turns_him_where_he_stands() {
    // A few degrees off, as a stop's last leg leaves him: a few centimetres
    // at most.
    let mut fake = Fake::new(SPAWN, 15.0);
    let ahead = Point {
        x: SPAWN.x + 7.0 * 12f64.to_radians().sin(),
        z: SPAWN.z + 7.0 * 12f64.to_radians().cos(),
    };
    let route = plan(vec![
        Step::Face {
            toward: ahead,
            within: 0.5,
            off: 0.0,
        },
        Step::Read {
            label: "ahead".into(),
        },
        Step::Face {
            toward: ahead,
            within: 0.5,
            off: -20.0,
        },
        Step::Read {
            label: "off".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    let facing_at = |label: &str| {
        let flip = fake.reads.iter().find(|(l, _)| l == label).unwrap().1;
        trace
            .looks
            .iter()
            .find(|l| l.seen.flip == flip && l.did == format!("read {label}"))
            .unwrap()
            .seen
            .facing
    };
    assert!(wrap_deg(facing_at("ahead") - 12.0).abs() <= 0.5);
    assert!(wrap_deg(facing_at("off") + 8.0).abs() <= 0.5);
    assert!(ground(&fake, SPAWN) < 0.03, "{:?}", fake.pos);
    assert_eq!(fake.vel, [0.0; 2]);
    // A quarter turn takes taps of a flip each, every one ending at rest:
    // centimetres, not a walk.
    let mut fake = Fake::new(SPAWN, 15.0);
    let west = Point { x: 0.0, z: SPAWN.z };
    let trace = drive(
        &mut fake,
        &plan(vec![Step::Face {
            toward: west,
            within: 0.5,
            off: 0.0,
        }]),
        Knobs::default(),
    )
    .unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert!(
        wrap_deg(fake.facing - 270.0).abs() <= 0.5,
        "{}",
        fake.facing
    );
    assert!(ground(&fake, SPAWN) < 0.1, "{:?}", fake.pos);
    // Each tap a flip of stick and two at rest, each followed by a rest of two
    // while he slides to a stop: eight taps for 90° at 12° a tap.
    assert!(
        fake.runs.iter().all(|(_, f)| *f == 3 || *f == 2),
        "{:?}",
        fake.runs
    );
    let taps = fake.runs.iter().filter(|(_, f)| *f == 3).count();
    assert!((8..=10).contains(&taps), "{taps} taps");
}

#[test]
fn presses_and_waits_go_in_as_one_schedule_between_their_shots_and_reads() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let route = plan(vec![
        Step::Press {
            buttons: [Button::South].into(),
            flips: 6,
        },
        Step::Wait { flips: 20 },
        Step::Shot {
            label: "ball".into(),
        },
        Step::Wait { flips: 10 },
        Step::Press {
            buttons: [Button::West, Button::RightTrigger].into(),
            flips: 4,
        },
        Step::Press {
            buttons: [Button::North].into(),
            flips: 2,
        },
        Step::Read {
            label: "end".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    // Two runs, 26 and 16 flips, no pause inside either.
    assert_eq!(fake.runs, vec![(3, 26), (29, 16)]);
    assert_eq!(fake.shots, vec![("ball".to_owned(), 28)]);
    assert_eq!(fake.reads, vec![("end".to_owned(), 44)]);
    let pressed: Vec<(u64, Vec<Button>)> = fake
        .held
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.buttons.is_empty())
        .map(|(k, p)| (k as u64, p.buttons.iter().copied().collect()))
        .collect();
    let want: Vec<(u64, Vec<Button>)> = (0..6)
        .map(|k| (k, vec![Button::South]))
        .chain((36..40).map(|k| (k, vec![Button::West, Button::RightTrigger])))
        .chain((40..42).map(|k| (k, vec![Button::North])))
        .collect();
    assert_eq!(pressed, want);
    // The last press is let go on the pause, so it ends the input as held.
    assert_eq!(fake.held.len(), 42);
    let script = trace.input.unwrap();
    assert_eq!(
        script.state_at(0, 41).unwrap().buttons,
        [Button::North].into()
    );
    assert_eq!(script.last_offset(), 40);
}

#[test]
fn settle_waits_for_the_eye_as_well_as_him() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let route = plan(vec![
        Step::Press {
            buttons: [Button::South].into(),
            flips: 1,
        },
        Step::Read {
            label: "moved".into(),
        },
    ]);
    // Moved off, then the view swung: he's still at once, the eye isn't.
    let walk = plan(vec![via(14.0, -93.0), Step::Settle { flips: 30 }]);
    let trace = drive(&mut fake, &walk, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    let still_from = trace
        .looks
        .iter()
        .position(|l| l.seen.speed() == 0.0 && l.step == 1)
        .unwrap();
    let eye_settled = trace.looks.last().unwrap().seen.flip;
    // The eye closes 8 % a frame: well past the walk's own stop.
    assert!(eye_settled > trace.looks[still_from].seen.flip + 30);
    let end = trace.looks.last().unwrap().seen;
    let before = trace.looks[trace.looks.len() - 2].seen;
    assert!((end.eye[0] - before.eye[0]).abs() <= 1e-3);
    // And a settle that follows nothing moving ends after its flips.
    let mut still = Fake::new(SPAWN, 15.0);
    let trace = drive(&mut still, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
}

#[test]
fn a_route_reads_from_ron_and_what_no_run_can_do_is_refused() {
    let text = r#"(
        steps: [
            Via(to: (x: 24.2, z: -88.7), radius: 1.0),
            Stop(at: (x: 32.4, z: -74.7), radius: 0.1),
            Face(toward: (x: 25.67, z: -74.58), within: 0.5),
            Settle(flips: 60),
            Read(label: "anchor"),
            Press(buttons: [South], flips: 6),
            Wait(flips: 30),
            Shot(label: "lob_1"),
        ],
        save: true,
    )"#;
    let plan = RoutePlan::from_ron(text).unwrap();
    assert_eq!(plan.steps.len(), 8);
    assert!(plan.save);
    assert_eq!(
        plan.steps[2],
        Step::Face {
            toward: Point {
                x: 25.67,
                z: -74.58
            },
            within: 0.5,
            off: 0.0
        }
    );
    assert_eq!(RoutePlan::from_ron(&plan.to_ron()).unwrap(), plan);
    // A shot and a read write different files: one label serves both.
    assert!(RoutePlan::from_ron("(steps: [Read(label: \"pre\"), Shot(label: \"pre\")])").is_ok());
    for bad in [
        "(steps: [])",
        "(steps: [Via(to: (x: 1.0, z: 2.0), radius: 0.0)])",
        "(steps: [Stop(at: (x: 1.0, z: 2.0), radius: -1.0)])",
        "(steps: [Face(toward: (x: 1.0, z: 2.0), within: 0.0)])",
        "(steps: [Settle(flips: 0)])",
        "(steps: [Wait(flips: 0)])",
        "(steps: [Press(buttons: [], flips: 3)])",
        "(steps: [Press(buttons: [South], flips: 0)])",
        "(steps: [Shot(label: \"a/b\")])",
        "(steps: [Shot(label: \"\")])",
        "(steps: [Shot(label: \"x\"), Shot(label: \"x\")])",
        "(steps: [Read(label: \"x\"), Wait(flips: 2), Read(label: \"x\")])",
        "(steps: [Read(label: \"f0\")])",
        "(steps: [Hop(to: (x: 1.0, z: 2.0))])",
        "(steps: [Wait(flips: 3)], extra: 1)",
    ] {
        assert!(RoutePlan::from_ron(bad).is_err(), "{bad}");
    }
}
