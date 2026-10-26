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
    /// AH.11's steps carried out, each with the flip it was done at.
    acts: Vec<(Step, u64)>,
    /// The hero left off every look from this flip on (AH.11: beaten, or
    /// taken off the portal).
    gone_from: Option<u64>,
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
            acts: Vec::new(),
            gone_from: None,
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
        let mut frame = actors::read(&mut self.world)?;
        if self.gone_from.is_some_and(|f| self.flip >= f) {
            frame.actors.retain(|a| a.corpus != self.hero.corpus);
        }
        Ok((self.flip, frame))
    }

    fn act(&mut self, step: &Step, frame: &ActorFrame) -> Result<()> {
        assert!(step.acts(), "{step:?} isn't one of AH.11's steps");
        assert_eq!(
            frame.game_frame, self.game_frame,
            "acted on the pause's own frame"
        );
        self.acts.push((step.clone(), self.flip));
        Ok(())
    }

    fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
        assert_eq!(
            script.state_at(1, 0),
            Some(PadState::default()),
            "port 1 rests from the first flip"
        );
        self.runs.push((self.flip + 1, flips));
        // A drive that never ends fails here rather than hanging the tests.
        if self.flip + flips > 100_000 {
            anyhow::bail!("the fake ran past 100,000 flips");
        }
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

/// AH.11's steps act at the pause they fall on, in plan order, with no run
/// between them, and the runs around them go on as before.
#[test]
fn ah11s_steps_act_at_the_pause_in_plan_order() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let poke = Step::Poke {
        placement: "009_Eruptor".into(),
        path: "description.creature info.hp".into(),
        value: 40.0,
    };
    let figure = Step::Figure {
        field: FigureField::Experience,
        value: 999,
    };
    let load = Step::Portal {
        slot: 1,
        figure: Some("/figures/Tree Rex.sky".into()),
    };
    let clear = Step::Portal {
        slot: 0,
        figure: None,
    };
    let vars = Step::Vars {
        placement: "Enemy_Chompy(2)".into(),
        label: "c2".into(),
    };
    let route = plan(vec![
        poke.clone(),
        Step::Wait { flips: 10 },
        figure.clone(),
        load.clone(),
        clear.clone(),
        Step::Reconnect,
        vars.clone(),
        Step::Wait { flips: 5 },
        Step::Read {
            label: "end".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(
        fake.acts,
        vec![
            (poke, 2),
            (figure, 12),
            (load, 12),
            (clear, 12),
            (Step::Reconnect, 12),
            (vars, 12)
        ]
    );
    assert_eq!(fake.runs, vec![(3, 10), (13, 5)]);
    assert_eq!(fake.reads, vec![("end".to_owned(), 17)]);
    // Each act is a look of its own in the trace.
    assert_eq!(
        trace
            .looks
            .iter()
            .filter(|l| l.did.starts_with("act "))
            .count(),
        6
    );
}

/// A session that can't act refuses AH.11's steps rather than skipping them.
#[test]
fn a_session_that_cant_act_refuses_ah11s_steps() {
    struct Plain(Fake);
    impl Session for Plain {
        fn look(&mut self) -> Result<(u64, ActorFrame)> {
            self.0.look()
        }
        fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
            self.0.run(script, flips)
        }
        fn shot(&mut self, label: &str) -> Result<()> {
            self.0.shot(label)
        }
        fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
            self.0.read(label, frame)
        }
    }
    let mut plain = Plain(Fake::new(SPAWN, 15.0));
    let e = drive(&mut plain, &plan(vec![Step::Reconnect]), Knobs::default()).unwrap_err();
    assert!(
        format!("{e:#}").contains("can't carry out Reconnect"),
        "{e:#}"
    );
}

/// With no hero at a pause (beaten, or off the portal) the driver keeps the
/// last look's pose and the waits, shots and acts go on; with none at F0
/// there's nothing to keep and the drive is refused.
#[test]
fn a_look_with_no_hero_keeps_the_last_pose() {
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.gone_from = Some(12);
    let route = plan(vec![
        Step::Wait { flips: 10 },
        Step::Shot {
            label: "gone".into(),
        },
        Step::Portal {
            slot: 0,
            figure: Some("/figures/Eruptor.sky".into()),
        },
        Step::Wait { flips: 20 },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(fake.shots, vec![("gone".to_owned(), 12)]);
    assert_eq!(fake.acts.len(), 1);
    let first = trace.looks[0].seen;
    for l in &trace.looks[1..] {
        assert_eq!((l.seen.pivot, l.seen.view), (first.pivot, first.view));
        assert_eq!(l.seen.game_frame, 5_962 + (l.seen.flip - 2) as u32);
    }
    assert_eq!(trace.looks.last().unwrap().seen.flip, 32);
    let mut none = Fake::new(SPAWN, 15.0);
    none.gone_from = Some(0);
    assert!(
        drive(
            &mut none,
            &plan(vec![Step::Wait { flips: 5 }]),
            Knobs::default()
        )
        .is_err()
    );
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
    // Each step changes what port 0 holds: a repeat is left out.
    let entries = &script.entries()[0];
    assert!(entries.windows(2).all(|w| w[0].pad != w[1].pad));
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
fn a_hold_keeps_the_stick_on_its_point_for_its_flips_whatever_he_does() {
    // Into a wall: a Via would be given up, a hold just runs its flips.
    let mut fake = Fake::new(SPAWN, 15.0);
    fake.walls = vec![(Point { x: 13.0, z: -93.4 }, 1.5)];
    let to = Point { x: 30.0, z: -93.4 };
    let route = plan(vec![
        Step::Hold {
            toward: to,
            flips: 300,
        },
        Step::Read {
            label: "held".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done, "{:?}", trace.looks.last());
    // 300 flips in stretches of 8 (every_far), the last cut to 4, then the
    // read at the hold's last flip.
    assert_eq!(fake.runs.len(), 38);
    assert!(fake.runs[..37].iter().all(|(_, f)| *f == 8));
    assert_eq!(fake.runs[37].1, 4);
    assert_eq!(fake.reads, vec![("held".to_owned(), 302)]);
    assert_eq!(fake.held.len(), 300);
    // Every flip held the stick full, pushing along the bearing to the point
    // from where he stood at that stretch's look (the wall stopped him short).
    for pad in &fake.held {
        let (s, h) = push_of(pad.left, 15.0);
        assert!(s > 0.97, "{s}");
        assert!(wrap_deg(h - 90.0).abs() < 2.0, "{h}");
    }
    assert!(
        fake.pos[0] < 13.0 - 1.4,
        "the wall held him: {:?}",
        fake.pos
    );
    // A hold one flip past a stretch: 8, then the 1 left.
    let mut short = Fake::new(SPAWN, 15.0);
    let route = plan(vec![Step::Hold {
        toward: to,
        flips: 9,
    }]);
    assert_eq!(
        drive(&mut short, &route, Knobs::default()).unwrap().outcome,
        Outcome::Done
    );
    assert_eq!(short.held.len(), 9);
    assert_eq!(short.runs, vec![(3, 8), (11, 1)]);
    // And on open ground he walks the hold's way, re-aimed at each look.
    let mut open = Fake::new(SPAWN, 15.0);
    open.view_turn = -1.5;
    let trace = drive(
        &mut open,
        &plan(vec![Step::Hold {
            toward: Point { x: 30.0, z: -80.0 },
            flips: 120,
        }]),
        Knobs::default(),
    )
    .unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(open.held.len(), 120);
    let gone = (open.pos[0] - SPAWN.x).hypot(open.pos[2] - SPAWN.z);
    assert!(gone > 10.0, "{gone}");
    let bearing = heading_deg(30.0 - open.pos[0], -80.0 - open.pos[2]);
    let walked = heading_deg(open.vel[0], open.vel[1]);
    assert!(wrap_deg(walked - bearing).abs() < 6.0, "{walked} {bearing}");
}

#[test]
fn a_tilt_goes_in_with_the_presses_and_waits_around_it_as_one_schedule() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let to = Point { x: 20.0, z: -93.4 };
    let route = plan(vec![
        Step::Tilt {
            toward: to,
            flips: 10,
        },
        Step::Wait { flips: 20 },
        Step::Press {
            buttons: [Button::West].into(),
            flips: 6,
        },
        Step::Tilt {
            toward: to,
            flips: 4,
        },
        Step::Read {
            label: "end".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    // One run of 40 flips, no pause inside it, the read at its end.
    assert_eq!(fake.runs, vec![(3, 40)]);
    assert_eq!(fake.reads, vec![("end".to_owned(), 42)]);
    let stick = stick_toward(heading_deg(to.x - SPAWN.x, to.z - SPAWN.z), 15.0, 1.0);
    for (k, pad) in fake.held.iter().enumerate() {
        let tilted = k < 10 || (36..40).contains(&k);
        assert_eq!(
            pad.left,
            if tilted { stick } else { Stick::CENTRED },
            "flip {k}"
        );
        assert_eq!(!pad.buttons.is_empty(), (30..36).contains(&k), "flip {k}");
    }
    // Both tilts aimed from where he stood at the run's first look: east.
    let (s, h) = push_of(stick, 15.0);
    assert!(s > 0.97 && wrap_deg(h - 90.0).abs() < 1.0, "{s} {h}");
    assert!(fake.pos[0] > SPAWN.x + 0.5, "he walked: {:?}", fake.pos);
    // Its input is what was held, so the anchor's opening replays open loop.
    let script = trace.input.unwrap();
    for (k, held) in fake.held.iter().enumerate() {
        assert_eq!(
            script.state_at(0, k as u64).as_ref(),
            Some(held),
            "flip {k}"
        );
    }
}

/// AG.90: a rock goes in after a tilt in one schedule, the stick toward the
/// point and away from it in turn a flip each, both aimed on one line from
/// where he stood at the run's first look, then let go.
#[test]
fn a_rock_turns_the_stick_round_every_flip_in_one_schedule() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let to = Point { x: 20.0, z: -93.4 };
    let route = plan(vec![
        Step::Tilt {
            toward: to,
            flips: 10,
        },
        Step::Rock {
            toward: to,
            flips: 7,
        },
        Step::Wait { flips: 5 },
        Step::Read {
            label: "end".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(fake.runs, vec![(3, 22)], "no pause inside it");
    let bearing = heading_deg(to.x - SPAWN.x, to.z - SPAWN.z);
    let toward = stick_toward(bearing, 15.0, 1.0);
    let away = stick_toward(bearing + 180.0, 15.0, 1.0);
    assert_ne!(toward, away);
    for (k, pad) in fake.held.iter().enumerate() {
        let want = match k {
            0..10 => toward,
            10..17 if (k - 10) % 2 == 0 => toward,
            10..17 => away,
            _ => Stick::CENTRED,
        };
        assert_eq!(pad.left, want, "flip {k}");
    }
    let (s, h) = push_of(away, 15.0);
    assert!(
        s > 0.97 && wrap_deg(h - bearing - 180.0).abs() < 1.0,
        "{s} {h}"
    );
    assert!(RoutePlan::from_ron("(steps: [Rock(toward: (x: 1.0, z: 2.0), flips: 0)])").is_err());
    assert!(RoutePlan::from_ron("(steps: [Rock(toward: (x: 1.0, z: 2.0), flips: 4)])").is_ok());
}

/// AH.41, for AG.28: the right stick held with the left at rest, then with
/// the left aimed as a tilt is, in one schedule with the waits around them.
#[test]
fn a_right_stick_goes_in_with_the_left_at_rest_or_aimed_as_one_schedule() {
    let mut fake = Fake::new(SPAWN, 15.0);
    let to = Point { x: 20.0, z: -93.4 };
    let route = plan(vec![
        Step::RightStick {
            x: 255,
            y: 128,
            flips: 12,
            toward: None,
        },
        Step::Wait { flips: 8 },
        Step::RightStick {
            x: 128,
            y: 0,
            flips: 10,
            toward: Some(to),
        },
        Step::Read {
            label: "end".into(),
        },
    ]);
    let trace = drive(&mut fake, &route, Knobs::default()).unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    // One run of 30 flips, the read at its end.
    assert_eq!(fake.runs, vec![(3, 30)]);
    assert_eq!(fake.reads, vec![("end".to_owned(), 32)]);
    let walked = stick_toward(heading_deg(to.x - SPAWN.x, to.z - SPAWN.z), 15.0, 1.0);
    for (k, pad) in fake.held.iter().enumerate() {
        let (left, right) = match k {
            0..12 => (Stick::CENTRED, Stick { x: 255, y: 128 }),
            20..30 => (walked, Stick { x: 128, y: 0 }),
            _ => (Stick::CENTRED, Stick::CENTRED),
        };
        assert_eq!((pad.left, pad.right), (left, right), "flip {k}");
        assert!(pad.buttons.is_empty(), "flip {k}");
    }
    // The first stretch left him where he stood; the second walked him.
    assert!(fake.pos[0] > SPAWN.x + 0.5, "{:?}", fake.pos);
    let script = trace.input.unwrap();
    assert_eq!(
        script.state_at(0, 5).unwrap().right,
        Stick { x: 255, y: 128 }
    );
    assert_eq!(
        script.state_at(0, 25).unwrap().right,
        Stick { x: 128, y: 0 }
    );
    assert!(script.state_at(0, 15).unwrap().right.is_centred());
    // The looks say what was held.
    assert!(
        trace.looks[0]
            .did
            .starts_with("+0 rest right (255, 128), +12 rest, +20 stick"),
        "{}",
        trace.looks[0].did
    );
}

#[test]
fn settle_waits_for_the_eye_as_well_as_him() {
    let mut fake = Fake::new(SPAWN, 15.0);
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
    // A settle in a still world ends once it has seen its flips of stillness.
    let mut still = Fake::new(SPAWN, 15.0);
    let trace = drive(
        &mut still,
        &plan(vec![Step::Settle { flips: 30 }]),
        Knobs::default(),
    )
    .unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(trace.flips, 30);
    // Straight after a run of 100 flips it never looked inside, it still
    // sees its 30 from its own first look: the look before the press saw
    // nothing of what the run did.
    let mut after = Fake::new(SPAWN, 15.0);
    let trace = drive(
        &mut after,
        &plan(vec![
            Step::Press {
                buttons: [Button::South].into(),
                flips: 1,
            },
            Step::Wait { flips: 99 },
            Step::Settle { flips: 30 },
        ]),
        Knobs::default(),
    )
    .unwrap();
    assert_eq!(trace.outcome, Outcome::Done);
    assert_eq!(trace.flips, 100 + 30);
}

#[test]
fn a_turn_that_never_comes_round_is_given_up_at_its_own_budget() {
    // A world that undoes every tap: the face is given up past its 300
    // flips, well inside a step's 3,600.
    struct Stiff<'a>(&'a mut Fake);
    impl Session for Stiff<'_> {
        fn look(&mut self) -> Result<(u64, ActorFrame)> {
            self.0.look()
        }
        fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
            let (pos, facing) = (self.0.pos, self.0.facing);
            self.0.run(script, flips)?;
            (self.0.pos, self.0.facing, self.0.vel) = (pos, facing, [0.0; 2]);
            self.0.write();
            Ok(())
        }
        fn shot(&mut self, label: &str) -> Result<()> {
            self.0.shot(label)
        }
        fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
            self.0.read(label, frame)
        }
    }
    let mut fake = Fake::new(SPAWN, 15.0);
    let west = Point { x: 0.0, z: SPAWN.z };
    let trace = drive(
        &mut Stiff(&mut fake),
        &plan(vec![Step::Face {
            toward: west,
            within: 0.5,
            off: 0.0,
        }]),
        Knobs::default(),
    )
    .unwrap();
    let Outcome::Stuck(Stuck::Budget {
        step,
        budget,
        began,
        flip,
        ..
    }) = trace.outcome
    else {
        panic!("{:?}", trace.outcome);
    };
    assert_eq!((step, budget, began), (0, 300, 2));
    assert!(flip - began > 300 && flip - began <= 303, "{flip}");
}

#[test]
fn a_run_that_pauses_anywhere_but_where_it_was_sent_is_refused() {
    struct Late<'a>(&'a mut Fake);
    impl Session for Late<'_> {
        fn look(&mut self) -> Result<(u64, ActorFrame)> {
            self.0.look()
        }
        fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
            self.0.run(script, flips)?;
            self.0.step(&PadState::default());
            Ok(())
        }
        fn shot(&mut self, label: &str) -> Result<()> {
            self.0.shot(label)
        }
        fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
            self.0.read(label, frame)
        }
    }
    let mut fake = Fake::new(SPAWN, 15.0);
    let e = drive(
        &mut Late(&mut fake),
        &plan(vec![Step::Wait { flips: 10 }]),
        Knobs::default(),
    )
    .unwrap_err();
    assert!(
        format!("{e:#}").contains("paused at 13, not at 12"),
        "{e:#}"
    );
}

#[test]
fn the_recorded_input_only_goes_forward_and_leaves_repeats_out() {
    let pad = |b: Button| PadState {
        buttons: [b].into(),
        ..PadState::default()
    };
    let mut input = vec![(5, pad(Button::South))];
    // Another state at the same flip, or an earlier one, is refused...
    assert!(record(&mut input, 5, &[(0, pad(Button::West))]).is_err());
    assert!(record(&mut input, 3, &[(0, pad(Button::West))]).is_err());
    assert_eq!(input, vec![(5, pad(Button::South))]);
    // ...a repeat of the state before is left out and a change kept.
    record(
        &mut input,
        6,
        &[(0, pad(Button::South)), (2, PadState::default())],
    )
    .unwrap();
    assert_eq!(
        input,
        vec![(5, pad(Button::South)), (8, PadState::default())]
    );
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
            Hold(toward: (x: 4.5, z: -12.0), flips: 45),
            Tilt(toward: (x: 1.797, z: -20.974), flips: 10),
            Poke(placement: "009_Eruptor", path: "description.creature info.armor", value: 200),
            Figure(field: Gold, value: 64999),
            Portal(slot: 0, figure: None),
            Portal(slot: 1, figure: Some("/figures/Tree Rex.sky")),
            Reconnect,
            Vars(placement: "009_Eruptor", label: "anchor"),
            Globals(names: ["portal save control"], label: "anchor"),
            Figure(field: Upgrade(4), value: 1),
            RightStick(x: 255, y: 128, flips: 120),
            RightStick(x: 128, y: 0, flips: 120, toward: Some((x: 21.6, z: -89.6))),
        ],
        save: true,
    )"#;
    let plan = RoutePlan::from_ron(text).unwrap();
    assert_eq!(plan.steps.len(), 20);
    assert_eq!(
        plan.steps[17],
        Step::Figure {
            field: FigureField::Upgrade(4),
            value: 1
        }
    );
    assert!(plan.steps[17].acts() && !plan.steps[18].acts() && !plan.steps[19].acts());
    assert_eq!(
        plan.steps[18],
        Step::RightStick {
            x: 255,
            y: 128,
            flips: 120,
            toward: None
        }
    );
    assert_eq!(
        plan.steps[19],
        Step::RightStick {
            x: 128,
            y: 0,
            flips: 120,
            toward: Some(Point { x: 21.6, z: -89.6 })
        }
    );
    assert_eq!(
        plan.steps[11],
        Step::Figure {
            field: FigureField::Gold,
            value: 64_999
        }
    );
    assert_eq!(
        plan.steps[13],
        Step::Portal {
            slot: 1,
            figure: Some("/figures/Tree Rex.sky".into())
        }
    );
    assert!(plan.steps[10..18].iter().all(Step::acts));
    assert!(!plan.steps[..10].iter().any(Step::acts));
    assert_eq!(
        plan.steps[9],
        Step::Tilt {
            toward: Point {
                x: 1.797,
                z: -20.974
            },
            flips: 10
        }
    );
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
        "(steps: [Hold(toward: (x: 1.0, z: 2.0), flips: 0)])",
        "(steps: [Tilt(toward: (x: 1.0, z: 2.0), flips: 0)])",
        "(steps: [Shot(label: \"a/b\")])",
        "(steps: [Shot(label: \"\")])",
        "(steps: [Shot(label: \"x\"), Shot(label: \"x\")])",
        "(steps: [Read(label: \"x\"), Wait(flips: 2), Read(label: \"x\")])",
        "(steps: [Read(label: \"f0\")])",
        "(steps: [Poke(placement: \"\", path: \"a.b\", value: 1.0)])",
        "(steps: [Poke(placement: \"x\", path: \"a..b\", value: 1.0)])",
        "(steps: [Poke(placement: \"x\", path: \"\", value: 1.0)])",
        "(steps: [Figure(field: Level, value: 2)])",
        "(steps: [Figure(field: Upgrade(0), value: 1)])",
        "(steps: [Figure(field: Upgrade(4), value: 2)])",
        "(steps: [RightStick(x: 0, y: 128, flips: 0)])",
        "(steps: [RightStick(x: 256, y: 128, flips: 3)])",
        "(steps: [RightStick(x: 0, flips: 3)])",
        "(steps: [Portal(slot: 8, figure: None)])",
        "(steps: [Portal(slot: 0, figure: Some(\"\"))])",
        "(steps: [Vars(placement: \"x\", label: \"a b\")])",
        "(steps: [Vars(placement: \"\", label: \"ab\")])",
        "(steps: [Vars(placement: \"x\", label: \"v\"), Vars(placement: \"y\", label: \"v\")])",
        "(steps: [Globals(names: [], label: \"g\")])",
        "(steps: [Globals(names: [\"\"], label: \"g\")])",
        "(steps: [Globals(names: [\"a\"], label: \"g\"), Globals(names: [\"b\"], label: \"g\")])",
        "(steps: [Hop(to: (x: 1.0, z: 2.0))])",
        "(steps: [Wait(flips: 3)], extra: 1)",
    ] {
        assert!(RoutePlan::from_ron(bad).is_err(), "{bad}");
    }
}

/// AH.200: a dump of globals reads as many levels down as it asks, the
/// whole read without a depth, and asks 1 to the whole read's.
#[test]
fn a_globals_dump_reads_as_deep_as_it_asks() {
    let plan = RoutePlan::from_ron(
        "(steps: [Globals(names: [\"combat control data\"], label: \"a\", depth: Some(3)), \
         Globals(names: [\"Minions_Loaded\"], label: \"b\"), Vars(placement: \"x\", label: \"c\")])",
    )
    .unwrap();
    assert_eq!(
        plan.steps[0],
        Step::Globals {
            names: vec!["combat control data".into()],
            label: "a".into(),
            depth: Some(3),
        }
    );
    assert_eq!(plan.steps[0].dump_depth(), 3);
    assert_eq!(plan.steps[1].dump_depth(), crate::script::TREE_DEPTH);
    assert_eq!(plan.steps[2].dump_depth(), crate::script::TREE_DEPTH);
    assert_eq!(RoutePlan::from_ron(&plan.to_ron()).unwrap(), plan);
    let deepest = format!(
        "(steps: [Globals(names: [\"a\"], label: \"g\", depth: Some({}))])",
        crate::script::TREE_DEPTH
    );
    assert!(RoutePlan::from_ron(&deepest).is_ok());
    for bad in [
        "(steps: [Globals(names: [\"a\"], label: \"g\", depth: Some(0))])".to_owned(),
        format!(
            "(steps: [Globals(names: [\"a\"], label: \"g\", depth: Some({}))])",
            crate::script::TREE_DEPTH + 1
        ),
    ] {
        assert!(RoutePlan::from_ron(&bad).is_err(), "{bad}");
    }
}

/// A look made by hand: his pivot at (`x`, `z`), still unless `speed` says
/// otherwise (along -x), the view and eye fixed unless `eye` moves it.
fn look(flip: u64, x: f64, z: f64, speed: f64, eye: f64) -> Seen {
    Seen {
        flip,
        game_frame: 5_960 + flip as u32,
        pivot: [x, 43.38, z],
        facing: 270.0,
        velocity: [-speed, 0.0, 0.0],
        view: 15.0,
        eye: [eye, 52.0, -107.0],
    }
}

fn is_rest(order: &Order) -> bool {
    matches!(order, Order::Run { schedule, .. } if schedule.len() == 1 && schedule[0].1 == PadState::default())
}

#[test]
fn the_drivers_decisions_on_looks_made_by_hand() {
    let k = Knobs::default();
    // A stop a metre off at 6 m/s: he's let go of (a held stick never slows
    // him), though his slide alone wouldn't carry him there.
    let stop = plan(vec![stop(0.0, -93.0, 0.1)]);
    let mut d = Driver::new(stop.clone(), k);
    assert!(!is_rest(&d.next(&look(2, 3.0, -93.0, 0.0, 0.0)).unwrap()));
    assert!(is_rest(&d.next(&look(10, 1.0, -93.0, 6.0, 0.0)).unwrap()));
    // Inside the radius with his velocity read 0 but his pivot moved since
    // the last look: not done until a look finds him where he was.
    let mut d = Driver::new(stop.clone(), k);
    d.next(&look(2, 0.3, -93.0, 0.0, 0.0)).unwrap();
    assert!(is_rest(&d.next(&look(4, 0.05, -93.0, 0.0, 0.0)).unwrap()));
    assert_eq!(
        d.next(&look(6, 0.05, -93.0, 0.0, 0.0)).unwrap(),
        Order::Done
    );
    // Creeping a millimetre a look is no progress: given up once patience
    // runs out after the last 0.2 m gained, well inside the budget.
    let mut d = Driver::new(plan(vec![via(0.0, -93.0)]), k);
    let mut stuck = None;
    for i in 0..1_000u64 {
        let x = 10.0 - 0.001 * i as f64;
        if let Err(s) = d.next(&look(2 + 2 * i, x, -93.0, 0.03, 0.0)) {
            stuck = Some(s);
            break;
        }
    }
    let Some(Stuck::NoProgress { best_at, flip, .. }) = stuck else {
        panic!("{stuck:?}");
    };
    assert_eq!(best_at, 2);
    assert_eq!(flip - best_at, k.patience + 2);
    // An eye that never holds still: the settle is given up at its budget.
    let mut d = Driver::new(plan(vec![Step::Settle { flips: 30 }]), k);
    let mut stuck = None;
    for i in 0..1_000u64 {
        if let Err(s) = d.next(&look(2 + 10 * i, 0.0, -93.0, 0.0, i as f64 * 0.01)) {
            stuck = Some(s);
            break;
        }
    }
    let Some(Stuck::Budget { began, flip, .. }) = stuck else {
        panic!("{stuck:?}");
    };
    assert_eq!(began, 2);
    assert!(flip - began > k.budget && flip - began <= k.budget + 10);
    // A long walk that keeps gaining is never given up however long it
    // takes: patience counts from the last 0.2 m gained, not the first look
    // (0.15 m a look, so every other look gains less than that).
    let mut d = Driver::new(plan(vec![via(0.0, -93.0)]), k);
    for i in 0..100u64 {
        let x = 60.0 - 0.15 * i as f64;
        assert!(
            d.next(&look(2 + 8 * i, x, -93.0, 1.9, 0.0)).is_ok(),
            "look {i}"
        );
    }
    // Half a metre off a stop: let go above 1.1 × 3 m/s for each metre left
    // to the radius's half (1.485 m/s here), the stick held under it.
    let mut d = Driver::new(stop.clone(), k);
    assert!(is_rest(&d.next(&look(2, 0.5, -93.0, 1.5, 0.0)).unwrap()));
    let mut d = Driver::new(stop.clone(), k);
    assert!(!is_rest(&d.next(&look(2, 0.5, -93.0, 1.4, 0.0)).unwrap()));
    // Inside the radius, his pivot not moved but a velocity reading: still
    // moving, so he's let slide; at 0 the stop is done.
    let mut d = Driver::new(stop.clone(), k);
    assert!(is_rest(&d.next(&look(2, 0.05, -93.0, 0.5, 0.0)).unwrap()));
    assert_eq!(
        d.next(&look(4, 0.05, -93.0, 0.0, 0.0)).unwrap(),
        Order::Done
    );
    // Pushing against something, his pivot and the eye fixed but his
    // velocity reading: not still, however long; the ten flips up to the
    // first look reading 0 settle it.
    let mut d = Driver::new(plan(vec![Step::Settle { flips: 10 }]), k);
    for i in 0..5u64 {
        assert!(is_rest(
            &d.next(&look(2 + 10 * i, 0.0, -93.0, 0.5, 0.0)).unwrap()
        ));
    }
    assert_eq!(
        d.next(&look(52, 0.0, -93.0, 0.0, 0.0)).unwrap(),
        Order::Done
    );
}
