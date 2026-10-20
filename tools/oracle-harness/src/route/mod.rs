//! The route driver (openspec AH.7, design decisions 16 and 17): the oracle
//! walked to a point and left there still, closed loop. At each pause it reads
//! the hero and the camera, steers through O4 toward the next point for a few
//! flips and pauses again; at the last it brings him in slowly, lets go, turns
//! him in place and waits for him and the camera to hold still. Then the
//! caller saves the state (`oracle goto`, [`session`]).
//!
//! Pausing moves the run (AF.11: about a frame of game time a pause), so only
//! the approach is driven this way: every measurement starts from the state
//! it ends in, pause-free. What the driver held on the pad, flip by flip, is
//! kept as an AF.7 input script ([`Trace::input`]) for AG.17's headless
//! runner, with every pause's flip and game frame beside it.
//!
//! A plan ([`RoutePlan`], RON) is a list of steps: points to pass and to stop
//! at, a heading to turn to, a wait for stillness, presses and waits, shots and
//! reads. The driver ([`Driver`]) turns each pause's look into an [`Order`]; a
//! [`Session`] carries the orders out, against the oracle or, in the tests
//! here, a fake world on [`crate::actors::fixture`]'s memory, read back
//! through the same reader the live run uses.
//!
//! The stick is the game's read (AG.8; pad-input's The move stick's dead zone):
//! per axis a quarter of the travel is centred and the rest counts from
//! nothing to full, the strength is the two axes' length capped at 1, and the
//! push's heading is the view's plus `atan2(-x, y)`. The walk is AG.38's: a
//! held stick turns his walk to it at once and pushes at 160 ft/s² times the
//! strength up to 21.5 ft/s times it, a held stick never slows him, a let-go
//! takes 80 ft/s² off, and his facing turns 1,440°/s times the strength. So
//! the driver slows him by letting go, never by easing the stick.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use guac_oracle::input::{Button, InputScript, PadState, Pads, Step as InputStep, Stick};
use serde::{Deserialize, Serialize};

use crate::actors::{ActorFrame, heading_deg, wrap_deg};

pub mod session;

/// Ground coordinates, metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub z: f64,
}

/// What the driver does next: walk, stop, turn, settle, press, wait, shoot or
/// read. Each step is done before the next one starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Step {
    /// Walk at full stick toward `to` and go on to the next step once his
    /// pivot is within `radius` metres of it across the ground.
    Via { to: Point, radius: f64 },
    /// Walk to `at` and stop there: his pivot within `radius` metres of it,
    /// at rest.
    Stop { at: Point, radius: f64 },
    /// Turn him where he stands until he faces `toward` (plus `off` degrees,
    /// counter-clockwise from above as headings go) within `within` degrees.
    Face {
        toward: Point,
        within: f64,
        #[serde(default)]
        off: f64,
    },
    /// The pad at rest until his pivot and the camera's eye have held still
    /// over `flips` flips.
    Settle { flips: u64 },
    /// These buttons held for `flips` flips, the stick centred, then let go.
    Press {
        buttons: BTreeSet<Button>,
        flips: u64,
    },
    /// The pad at rest for `flips` flips.
    Wait { flips: u64 },
    /// A `SCREENSHOT` at the pause, named by `label`.
    Shot { label: String },
    /// The actors, the generator and the fight's state read at the pause,
    /// named by `label`.
    Read { label: String },
}

/// A route: its steps in order, and whether the run is saved where it ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePlan {
    pub steps: Vec<Step>,
    /// A `SAVESTATE` once the last step is done.
    #[serde(default)]
    pub save: bool,
}

impl RoutePlan {
    /// A plan as RON, checked.
    pub fn from_ron(text: &str) -> Result<Self> {
        let plan: RoutePlan = ron::from_str(text).context("parsing the route")?;
        plan.check()?;
        Ok(plan)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a route always serializes")
    }

    /// Refuses what no run can do: no steps, a radius or tolerance that isn't
    /// positive, a step of no flips, a label used twice or one that can't name
    /// a file.
    pub fn check(&self) -> Result<()> {
        if self.steps.is_empty() {
            bail!("a route has at least one step");
        }
        let mut labels = BTreeSet::new();
        for (i, step) in self.steps.iter().enumerate() {
            let fine = |x: f64| x.is_finite() && x > 0.0;
            match step {
                Step::Via { to, radius } | Step::Stop { at: to, radius } => {
                    if !fine(*radius) || !to.x.is_finite() || !to.z.is_finite() {
                        bail!("step {i}: a point with a positive radius, not {step:?}");
                    }
                }
                Step::Face {
                    toward,
                    within,
                    off,
                } => {
                    if !fine(*within) || !toward.x.is_finite() || !off.is_finite() {
                        bail!("step {i}: a heading to within a positive tolerance, not {step:?}");
                    }
                }
                Step::Settle { flips } | Step::Wait { flips } => {
                    if *flips == 0 {
                        bail!("step {i}: {step:?} takes at least a flip");
                    }
                }
                Step::Press { buttons, flips } => {
                    if *flips == 0 || buttons.is_empty() {
                        bail!("step {i}: a press holds a button for at least a flip");
                    }
                }
                Step::Shot { label } | Step::Read { label } => {
                    if label.is_empty()
                        || !label
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                    {
                        bail!("step {i}: a label is letters, digits, '_' and '.', not {label:?}");
                    }
                    if label == "f0" {
                        bail!("step {i}: \"f0\" is the session's own read at F0");
                    }
                    if !labels.insert(label.clone()) {
                        bail!("step {i}: the label {label:?} is used twice");
                    }
                }
            }
        }
        Ok(())
    }
}

/// What tunes the driver: how often it looks, how it brings him in and turns
/// him, when it gives a step up and what counts as still, with Eruptor's walk
/// as AG.8 and AG.38 measured it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Knobs {
    /// Flips between looks while the next point is further than [`Knobs::near`].
    pub every_far: u64,
    /// Flips between looks inside it, and while he stops, turns or settles.
    pub every_near: u64,
    /// Metres from a stop's point inside which he's brought in slowly.
    pub near: f64,
    /// A walking step is given up when it's gone this many flips without
    /// getting [`Knobs::progress`] metres nearer than its best.
    pub patience: u64,
    pub progress: f64,
    /// Flips any one step may take, walking, turning or settling.
    pub budget: u64,
    /// A stop's approach speed: this many m/s for each metre still to go.
    pub gain: f64,
    /// The slowest approach, m/s, so a stop's last centimetres still move him.
    pub crawl: f64,
    /// His top speed at full stick, m/s (21.5 ft/s, AG.8).
    pub top: f64,
    /// What a let-go takes off his walk, m/s² (80 ft/s², AG.38).
    pub let_go: f64,
    /// His facing's turn at full stick, °/s (AG.8).
    pub turn_rate: f64,
    /// The weakest and the strongest stick a turn's tap uses.
    pub turn_min: f64,
    pub turn_max: f64,
    /// Flips a turn may take, its taps and their rests.
    pub face_budget: u64,
    /// Still: his pivot and the eye moved no more than these between looks,
    /// metres, and his velocity reads 0.
    pub still: f64,
    pub eye_still: f64,
}

/// One foot, metres.
const FOOT: f64 = 0.3048;

impl Default for Knobs {
    fn default() -> Self {
        Self {
            every_far: 8,
            every_near: 2,
            near: 3.0,
            patience: 180,
            progress: 0.2,
            budget: 3_600,
            gain: 3.0,
            crawl: 0.25,
            top: 21.5 * FOOT,
            let_go: 80.0 * FOOT,
            turn_rate: 1_440.0,
            turn_min: 0.25,
            turn_max: 0.5,
            face_budget: 300,
            still: 1e-4,
            eye_still: 1e-3,
        }
    }
}

/// A quarter of an axis's travel reads as centred (AG.8).
pub const DEAD_ZONE: f64 = 0.25;

/// What the game makes of a stick against a view along `view` degrees: its
/// strength (0 centred) and the heading it pushes along (AG.8's read, as
/// `guac_motion::stick` has it).
pub fn push_of(stick: Stick, view: f64) -> (f64, f64) {
    let axis = |a: f64| {
        if a.abs() <= DEAD_ZONE {
            0.0
        } else {
            a.signum() * (a.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)
        }
    };
    let x = axis((f64::from(stick.x) - 128.0) / 128.0);
    let y = axis((128.0 - f64::from(stick.y)) / 128.0);
    let strength = x.hypot(y).min(1.0);
    if strength <= 0.0 {
        return (0.0, 0.0);
    }
    (
        strength,
        (view + (-x).atan2(y).to_degrees()).rem_euclid(360.0),
    )
}

/// The stick that pushes along `heading` at `strength` (0 to 1) against a view
/// along `view`: [`push_of`] turned round, then of the pad's bytes round that
/// point the pair the game reads nearest, a degree off weighing as much as
/// 0.02 of strength. Near the dead zone the bytes are coarse (a byte is 0.0104
/// of strength), so a weak stick can't point finely: 0.05 is good to about 6°,
/// 0.25 and up to a fraction of a degree.
pub fn stick_toward(heading: f64, view: f64, strength: f64) -> Stick {
    let s = strength.clamp(0.0, 1.0);
    if s == 0.0 {
        return Stick::CENTRED;
    }
    let a = (heading - view).to_radians();
    let (x, y) = (-s * a.sin(), s * a.cos());
    let raw = |c: f64| {
        if c == 0.0 {
            0.0
        } else {
            c.signum() * (DEAD_ZONE + c.abs() * (1.0 - DEAD_ZONE))
        }
    };
    let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    let (x0, y0) = (128.0 + raw(x) * 128.0, 128.0 - raw(y) * 128.0);
    let mut best = (
        f64::INFINITY,
        Stick {
            x: byte(x0),
            y: byte(y0),
        },
    );
    for dx in -2..=2 {
        for dy in -2..=2 {
            let st = Stick {
                x: byte(x0.round() + f64::from(dx)),
                y: byte(y0.round() + f64::from(dy)),
            };
            let (got, h) = push_of(st, view);
            if got == 0.0 {
                continue;
            }
            let cost = wrap_deg(h - heading).abs() + 50.0 * (got - s).abs();
            if cost < best.0 {
                best = (cost, st);
            }
        }
    }
    best.1
}

/// What the driver reads at a pause: the hero and the camera.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub flip: u64,
    pub game_frame: u32,
    pub pivot: [f64; 3],
    /// Degrees, the game's heading (0 along +z, 90 along +x).
    pub facing: f64,
    pub velocity: [f64; 3],
    /// The camera's heading, what the stick walks him against.
    pub view: f64,
    pub eye: [f64; 3],
}

impl Seen {
    /// The hero (the one skinned actor a pad drives) and the camera off a
    /// frame read at the pause at `flip`.
    pub fn of(flip: u64, frame: &ActorFrame) -> Result<Self> {
        let hero = frame.hero()?;
        let camera = frame.camera.as_ref().context("the frame has no camera")?;
        let f = |v: [f32; 3]| v.map(f64::from);
        Ok(Self {
            flip,
            game_frame: frame.game_frame,
            pivot: f(hero.position),
            facing: f64::from(hero.rotation[1]),
            velocity: f(hero.velocity),
            view: f64::from(camera.rotation[1]),
            eye: f(camera.position),
        })
    }

    /// Metres across the ground to `p`.
    pub fn distance(&self, p: Point) -> f64 {
        (p.x - self.pivot[0]).hypot(p.z - self.pivot[2])
    }

    /// The heading from his pivot to `p`.
    pub fn bearing(&self, p: Point) -> f64 {
        heading_deg(p.x - self.pivot[0], p.z - self.pivot[2]).rem_euclid(360.0)
    }

    /// His velocity's x and z, as a speed in m/s: how fast he's crossing the
    /// ground, whatever he's climbing.
    pub fn speed(&self) -> f64 {
        self.velocity[0].hypot(self.velocity[2])
    }
}

/// What the session does next, at the pause the run is in.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// The pads from the next flip on, each from its offset (port 0; port 1
    /// rests), then a pause `flips` flips on.
    Run {
        schedule: Vec<(u64, PadState)>,
        flips: u64,
    },
    Shot(String),
    Read(String),
    Done,
}

/// Why a step was given up: a walk that stopped getting nearer for
/// [`Knobs::patience`] flips, or any step past [`Knobs::budget`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum Stuck {
    #[error(
        "step {step} ({what}): no {progress} m nearer than its best, {best:.3} m at flip \
         {best_at}, by flip {flip}"
    )]
    NoProgress {
        step: usize,
        what: String,
        progress: f64,
        best: f64,
        best_at: u64,
        flip: u64,
    },
    #[error("step {step} ({what}): over its budget of {budget} flips (from {began} to {flip})")]
    Budget {
        step: usize,
        what: String,
        budget: u64,
        began: u64,
        flip: u64,
    },
}

/// The state machine that turns looks into orders, one plan step at a time.
#[derive(Debug, Clone)]
pub struct Driver {
    plan: RoutePlan,
    knobs: Knobs,
    step: usize,
    /// The flip the step began at, set at its first look and cleared when
    /// the driver moves on: its budget counts from here.
    began: Option<u64>,
    /// A walking step's best distance and the flip it was reached at.
    best: Option<(f64, u64)>,
    /// The look before this one, and the flip he's held still since.
    last: Option<Seen>,
    still_since: Option<u64>,
}

fn rest() -> PadState {
    PadState::default()
}

fn walk(stick: Stick) -> PadState {
    PadState {
        left: stick,
        ..PadState::default()
    }
}

impl Driver {
    pub fn new(plan: RoutePlan, knobs: Knobs) -> Self {
        Self {
            plan,
            knobs,
            step: 0,
            began: None,
            best: None,
            last: None,
            still_since: None,
        }
    }

    /// The step the driver is on (the plan's length once it's done).
    pub fn step(&self) -> usize {
        self.step
    }

    fn advance(&mut self) {
        self.step += 1;
        self.began = None;
        self.best = None;
        self.still_since = None;
    }

    /// Held still since the last look: his pivot and the eye where they were
    /// and his velocity 0.
    fn held_still(&self, seen: &Seen) -> bool {
        let Some(last) = &self.last else {
            return false;
        };
        let moved =
            |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max);
        moved(last.pivot, seen.pivot) <= self.knobs.still
            && moved(last.eye, seen.eye) <= self.knobs.eye_still
            && seen.velocity.iter().all(|v| *v == 0.0)
    }

    /// Moving: a velocity, or his pivot moved since the last look.
    fn moving(&self, seen: &Seen) -> bool {
        seen.speed() > 1e-3
            || self.last.as_ref().is_some_and(|l| {
                (l.pivot[0] - seen.pivot[0]).hypot(l.pivot[2] - seen.pivot[2]) > self.knobs.still
            })
    }

    fn track(&mut self, d: f64, flip: u64) -> Result<(), Stuck> {
        match self.best {
            Some((best, best_at)) if d >= best - self.knobs.progress => {
                if flip.saturating_sub(best_at) > self.knobs.patience {
                    return Err(Stuck::NoProgress {
                        step: self.step,
                        what: format!("{:?}", self.plan.steps[self.step]),
                        progress: self.knobs.progress,
                        best,
                        best_at,
                        flip,
                    });
                }
            }
            _ => self.best = Some((d, flip)),
        }
        Ok(())
    }

    /// The order for the pause `seen` was read at.
    pub fn next(&mut self, seen: &Seen) -> Result<Order, Stuck> {
        let order = self.decide(seen);
        self.last = Some(*seen);
        order
    }

    fn decide(&mut self, seen: &Seen) -> Result<Order, Stuck> {
        let k = self.knobs;
        loop {
            let Some(step) = self.plan.steps.get(self.step).cloned() else {
                return Ok(Order::Done);
            };
            let began = *self.began.get_or_insert(seen.flip);
            if seen.flip.saturating_sub(began) > k.budget {
                return Err(Stuck::Budget {
                    step: self.step,
                    what: format!("{step:?}"),
                    budget: k.budget,
                    began,
                    flip: seen.flip,
                });
            }
            let run = |pad: PadState, flips: u64| Order::Run {
                schedule: vec![(0, pad)],
                flips,
            };
            match step {
                Step::Via { to, radius } => {
                    let d = seen.distance(to);
                    if d <= radius {
                        self.advance();
                        continue;
                    }
                    self.track(d, seen.flip)?;
                    let every = if d > k.near {
                        k.every_far
                    } else {
                        k.every_near
                    };
                    return Ok(run(
                        walk(stick_toward(seen.bearing(to), seen.view, 1.0)),
                        every,
                    ));
                }
                Step::Stop { at, radius } => {
                    let d = seen.distance(at);
                    let moving = self.moving(seen);
                    if d <= radius {
                        if !moving {
                            self.advance();
                            continue;
                        }
                        // Inside: let him slide to rest, then look again.
                        return Ok(run(rest(), k.every_near));
                    }
                    self.track(d, seen.flip)?;
                    if d > k.near {
                        return Ok(run(
                            walk(stick_toward(seen.bearing(at), seen.view, 1.0)),
                            k.every_far,
                        ));
                    }
                    // Brought in slowly: the speed the rest of the way allows,
                    // reached by letting go when he's over it (a held stick
                    // never slows him).
                    let want = (k.gain * (d - radius / 2.0)).clamp(k.crawl, k.top);
                    let speed = seen.speed();
                    let sliding = speed * speed / (2.0 * k.let_go);
                    if speed > want * 1.1 || (moving && d - sliding <= radius / 2.0) {
                        return Ok(run(rest(), k.every_near));
                    }
                    return Ok(run(
                        walk(stick_toward(seen.bearing(at), seen.view, want / k.top)),
                        k.every_near,
                    ));
                }
                Step::Face {
                    toward,
                    within,
                    off,
                } => {
                    if self.moving(seen) {
                        return Ok(run(rest(), k.every_near));
                    }
                    let want = (seen.bearing(toward) + off).rem_euclid(360.0);
                    let err = wrap_deg(want - seen.facing);
                    if err.abs() <= within {
                        self.advance();
                        continue;
                    }
                    // A one-flip tap: his facing turns rate x strength a
                    // second and stops on the stick's heading, so a tap that
                    // can cover the error ends on it, and one flip of it moves
                    // him millimetres. Of the strengths from the one that
                    // covers it up to the strongest, the stick the game reads
                    // nearest the heading wants (the bytes point finer at some
                    // strengths than others), the weakest on a tie.
                    let per_flip = k.turn_rate / 60.0;
                    let need = (err.abs() / per_flip + 0.02).clamp(k.turn_min, k.turn_max);
                    let stick = (0..=4)
                        .map(|i| need + (k.turn_max - need) * f64::from(i) / 4.0)
                        .map(|s| stick_toward(want, seen.view, s))
                        .min_by(|a, b| {
                            let off = |st: &Stick| wrap_deg(push_of(*st, seen.view).1 - want).abs();
                            off(a).total_cmp(&off(b))
                        })
                        .expect("five strengths");
                    if seen.flip.saturating_sub(began) > k.face_budget {
                        return Err(Stuck::Budget {
                            step: self.step,
                            what: format!("{step:?}"),
                            budget: k.face_budget,
                            began,
                            flip: seen.flip,
                        });
                    }
                    return Ok(Order::Run {
                        schedule: vec![(0, walk(stick)), (1, rest())],
                        flips: 1 + k.every_near,
                    });
                }
                Step::Settle { flips } => {
                    if self.held_still(seen) {
                        let since = *self
                            .still_since
                            .get_or_insert(self.last.as_ref().map_or(seen.flip, |l| l.flip));
                        if seen.flip - since >= flips {
                            self.advance();
                            continue;
                        }
                    } else {
                        self.still_since = None;
                    }
                    return Ok(run(rest(), flips.min(10)));
                }
                Step::Press { .. } | Step::Wait { .. } => {
                    // A run of presses and waits goes in as one schedule, so
                    // no pause falls between a press and what follows it.
                    let mut schedule = Vec::new();
                    let mut at = 0;
                    while let Some(s) = self.plan.steps.get(self.step) {
                        match s {
                            Step::Press { buttons, flips } => {
                                let pressed = PadState {
                                    buttons: buttons.clone(),
                                    ..PadState::default()
                                };
                                // A press straight after another takes the
                                // other's release flip.
                                match schedule.last_mut() {
                                    Some((o, pad)) if *o == at => *pad = pressed,
                                    _ => schedule.push((at, pressed)),
                                }
                                schedule.push((at + flips, rest()));
                                at += flips;
                            }
                            Step::Wait { flips } => {
                                if schedule.is_empty() {
                                    schedule.push((at, rest()));
                                }
                                at += flips;
                            }
                            _ => break,
                        }
                        self.advance();
                    }
                    // A release that falls on the pause is the next run's.
                    schedule.retain(|(o, _)| *o < at);
                    return Ok(Order::Run {
                        schedule,
                        flips: at,
                    });
                }
                Step::Shot { label } => {
                    self.advance();
                    return Ok(Order::Shot(label));
                }
                Step::Read { label } => {
                    self.advance();
                    return Ok(Order::Read(label));
                }
            }
        }
    }
}

/// What carries the driver's orders out: the oracle ([`session::Live`]) or a
/// fake world.
pub trait Session {
    /// The pause the run is in: its flip and the frame read there.
    fn look(&mut self) -> Result<(u64, ActorFrame)>;
    /// `pads` from the next flip on (port 0; offsets from that flip), then a
    /// pause `flips` flips on.
    fn run(&mut self, script: &InputScript, flips: u64) -> Result<()>;
    fn shot(&mut self, label: &str) -> Result<()>;
    fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()>;
}

/// One pause: what was read and what was done there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub seen: Seen,
    /// The step the driver was on when it decided.
    pub step: usize,
    /// What it did: the pad's first state and the flips run, or the shot,
    /// the read or the end.
    pub did: String,
}

/// Whether the plan was done or a step given up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Done,
    Stuck(Stuck),
}

/// A drive: every pause, what was held on the pad from F0 + 1 on, and how it
/// ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    /// The first look's flip: offset 0 of [`Trace::input`] is the flip after.
    pub f0: u64,
    pub looks: Vec<Look>,
    #[serde(skip)]
    pub input: Option<InputScript>,
    /// The input as RON, the form the trace is kept in (`route.json`).
    pub input_ron: String,
    /// Flips run from F0 to the last pause.
    pub flips: u64,
    pub outcome: Outcome,
}

/// The pad schedule a run loads: port 0 per the order, port 1 at rest.
fn script_of(schedule: &[(u64, PadState)]) -> Result<InputScript> {
    let mut steps: Vec<InputStep> = schedule
        .iter()
        .map(|(at, pad)| InputStep {
            at: *at,
            pads: Pads([(0u8, pad.clone())].into_iter().collect()),
        })
        .collect();
    match steps.iter_mut().find(|s| s.at == 0) {
        Some(s) => {
            s.pads.0.insert(1, rest());
        }
        None => steps.insert(
            0,
            InputStep {
                at: 0,
                pads: Pads([(1u8, rest())].into_iter().collect()),
            },
        ),
    }
    Ok(InputScript::new(steps)?)
}

/// What a run held, appended to the drive's input: each state at its flip
/// from F0 + 1, a repeat of the state before left out. Runs follow each other,
/// so the flips only go forward.
fn record(input: &mut Vec<(u64, PadState)>, base: u64, schedule: &[(u64, PadState)]) -> Result<()> {
    for (at, pad) in schedule {
        let at = base + at;
        if let Some((last_at, last)) = input.last() {
            if at <= *last_at {
                bail!("the input went back: flip {at} after {last_at}");
            }
            if last == pad {
                continue;
            }
        }
        input.push((at, pad.clone()));
    }
    Ok(())
}

/// The whole input script from F0 + 1: port 1 at rest throughout.
fn input_script(input: &[(u64, PadState)]) -> Result<InputScript> {
    let port =
        |p: u8, pad: PadState| -> BTreeMap<u8, PadState> { [(p, pad)].into_iter().collect() };
    let mut steps: Vec<InputStep> = input
        .iter()
        .map(|(at, pad)| InputStep {
            at: *at,
            pads: Pads(port(0, pad.clone())),
        })
        .collect();
    match steps.first_mut() {
        Some(s) if s.at == 0 => {
            s.pads.0.insert(1, rest());
        }
        _ => steps.insert(
            0,
            InputStep {
                at: 0,
                pads: Pads([(0u8, rest()), (1u8, rest())].into_iter().collect()),
            },
        ),
    }
    Ok(InputScript::new(steps)?)
}

/// Drives `plan` through `session` from the pause it's in until the plan is
/// done or a step is given up (which ends the trace, not the call: the caller
/// says what it does then). Each pause is looked at once and the driver's
/// order carried out; a shot or a read leaves the run where it is.
pub fn drive(session: &mut impl Session, plan: &RoutePlan, knobs: Knobs) -> Result<Trace> {
    plan.check()?;
    let mut driver = Driver::new(plan.clone(), knobs);
    let mut looks = Vec::new();
    let mut input: Vec<(u64, PadState)> = Vec::new();
    let (mut flip, mut frame) = session.look()?;
    let f0 = flip;
    let outcome = loop {
        let seen = Seen::of(flip, &frame)?;
        let step = driver.step();
        let order = match driver.next(&seen) {
            Ok(order) => order,
            Err(stuck) => {
                looks.push(Look {
                    seen,
                    step,
                    did: format!("gave up: {stuck}"),
                });
                break Outcome::Stuck(stuck);
            }
        };
        let did = match &order {
            Order::Run { schedule, flips } => format!(
                "{} for {flips}",
                schedule
                    .iter()
                    .map(|(at, p)| format!("+{at} {}", describe(p)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Order::Shot(l) => format!("shot {l}"),
            Order::Read(l) => format!("read {l}"),
            Order::Done => "done".into(),
        };
        looks.push(Look { seen, step, did });
        match order {
            Order::Run { schedule, flips } => {
                record(&mut input, flip - f0, &schedule)?;
                session.run(&script_of(&schedule)?, flips)?;
                let (now, f) = session.look()?;
                if now != flip + flips {
                    bail!(
                        "the run paused at {now}, not at {} ({flip} + {flips})",
                        flip + flips
                    );
                }
                (flip, frame) = (now, f);
            }
            Order::Shot(label) => session.shot(&label)?,
            Order::Read(label) => session.read(&label, &frame)?,
            Order::Done => break Outcome::Done,
        }
    };
    let script = input_script(&input)?;
    Ok(Trace {
        f0,
        looks,
        input_ron: script.to_ron(),
        input: Some(script),
        flips: flip - f0,
        outcome,
    })
}

/// A pad in a few words: the stick's bytes and the buttons.
pub fn describe(pad: &PadState) -> String {
    let mut s = if pad.left.is_centred() {
        "rest".to_owned()
    } else {
        format!("stick ({}, {})", pad.left.x, pad.left.y)
    };
    for b in &pad.buttons {
        s.push_str(&format!(" {b:?}"));
    }
    s
}

#[cfg(test)]
mod tests;
