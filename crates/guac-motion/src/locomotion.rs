//! The locomotion stand-in (openspec AG.30, design decision 14;
//! hero-movement's Idle, walk and run): which of a hero's clips play, at what
//! weight, time and rate, a game frame at a time, from the speed he walks at
//! and his fall. Pure: the clips are AE's and AG.14's, posed by `guac-ir`'s
//! evaluator, and this holds only their loops.
//!
//! No clip is picked. As AG.11 measured it off 244 O5 recordings, the idle,
//! walk, run and fall play at once by weight, by `global.bld`'s locomotion
//! script (run natively here until AH.17's VM runs it):
//! - **Walk and run by speed.** The speed is his walk's length after the
//!   frame's push and the edge probe, before its slowing
//!   ([`crate::hero::Hero::gait_speed`]): pushed into a wall he runs on the
//!   spot. The run's share is (s − walk)/(run − walk) of the authored speeds
//!   ([`Gaits`]), held to 0 and 1: all walk to 12.0019 ft/s and all run from
//!   21.0017 for Eruptor, the same speeding up and slowing down, no timed
//!   crossfade between them.
//! - **Rates.** Moving, a gait alone plays at s over its authored speed; in
//!   the mix the walk plays at 1 + share × (its loop over the run's − 1),
//!   Eruptor's 100 % + 60 % × the share, and the run keeps step with it: its
//!   place in its cycle is the walk's as the frame began plus its own
//!   advance. A gait whose rate isn't being set keeps the one it last had
//!   (the walk under a pure run, the run under a pure walk), and so do all
//!   of them at rest as they fade. The idle and the fall play at 100 %.
//! - **Standing and moving** switch by time. The frame the speed leaves 0
//!   the idle slides out over 0.25 s, the walk restarts a quarter of the
//!   way into its stride at 100 % for that frame and the run's rate is set
//!   to the speed over its own (910 of AG.11's 968 set-offs, the rest at
//!   edges where the body hadn't moved); the frame it's back at 0 the idle
//!   restarts and slides in over 0.30 s. Each slide is eased (its
//!   rate rising over the first 30 % of its time and falling over the last
//!   30 %, `slide value`'s ease 15 and 15) and runs from where the last one
//!   got to over its own full time.
//! - **Held at an edge** (AG.58). A frame whose push the edge probe zeroed
//!   ([`Frame::held`]) reads only the walk's vertical part, a slope's share
//!   of the push (0.12-0.15 ft/s at the bridge's foot), and doesn't set him
//!   off: the walk restarts a quarter in and the run's rate is set, but the
//!   idle's slide in carries on. With the stick held there the probe lets
//!   every second frame's push through and zeroes it again, so the walk's
//!   stride holds at a quarter as it fades (AG.58's three door runs: the
//!   idle's own weight on the eased 0.30 s slide from the frame after the
//!   stop to 1e-7 on every record, 9-10 such restarts each).
//! - **The fall.** A frame reads the move and the height the frame before
//!   left. Down faster than 20 ft/s with his pivot 1.5 ft or more over the
//!   ground below (or over none), and no fall playing, everything above
//!   slides out linearly over 0.2 s under the fall clip, from its start.
//!   Once that's done, the first frame that reads anything else (slower, or
//!   within 1.5 ft of the ground) slides them back over 0.12 s, the walk and
//!   the run restarting three quarters of the way into their strides; the
//!   fall stops once they're whole, and only then can another begin.
//!
//! Each step is a record of AG.11's: a slide begun this frame shows no
//! change until the next, its progress the game time since the frame it
//! began; a restart sets the clip's time and the frame's advance follows.
//! The fall's reading of the frame before is `global.bld`'s, read as an aid
//! (AG.30): on AG.11's 206 recordings it starts all 48 falls on their record
//! and brings 47 back on theirs, where AG.11's fit on the record's own
//! height within half a foot brought 45.
//!
//! Hypotheses, what the recordings don't show:
//! - the mix's walk rate as the gaits' loops' ratio (1.0667/0.6667 = 1.6000
//!   on Eruptor, his 1 + 0.6 × the share to 1.07e-5 % over all 4,583 mix
//!   records): a constant 0.6 reads the same until another character's
//!   recording (AG.27);
//! - which pose the original shows while held at an edge: its records flip
//!   each frame, one with the idle at 100 beside a walk at the rest of the
//!   idle's own weight, the next with the run at the walk's weight; the
//!   stand-in draws the idle at its own weight and no run (AG.58), and its
//!   idle plays on where the original's time steps back a frame each push;
//! - the fall clip takes what the gaits give up (the weights summing to 1)
//!   and plays from its start at 100 %, looping: its weight, time and rate
//!   aren't in any recording (AG.54 names them);
//! - no landing clip: Eruptor has none (AG.14, AG.11), and the one Tree Rex
//!   and 22 others have waits for a walker that plays it (AG.27).

use crate::tuning::{FLOAT_TOLERANCE, FOOT};

/// The idle slides out over this from the frame he sets off, seconds.
pub const SET_OFF: f32 = 0.25;
/// And back in over this from the frame he's at rest.
pub const COME_TO_REST: f32 = 0.30;
/// The share of a stand/move slide's time its rate rises over, and falls
/// over at its end (`slide value`'s ease 15 and 15).
pub const EASE: f32 = 0.3;
/// Where in its stride the walk starts when he sets off, of its loop.
pub const WALK_START: f64 = 0.25;
/// A move down faster than this, ft/s, brings in the fall.
pub const FALL_SPEED: f32 = 20.0;
/// The gaits slide out under the fall over this, seconds, linearly.
pub const FALL_IN: f32 = 0.2;
/// And back over this once he's down.
pub const FALL_OUT: f32 = 0.12;
/// Feet over the ground below: a fast move down from this high or higher
/// brings the fall in; within it the gaits come back.
pub const HIGH: f32 = 1.5;
/// Where in their strides the walk and the run come back from a fall, of
/// their loops.
pub const LAND_START: f64 = 0.75;
/// A slide is done this close to its time, seconds: a guard for a step held
/// a hair short (1/60 s as 16,666,666 ns would leave 12 steps short of
/// 0.2 s). Bevy's 60 Hz fixed step is 16,666,667 ns and the recordings'
/// steps never land within it, so nothing exercises it (AG.30's review).
const SLACK: f64 = 1e-6;

/// The walk's and the run's authored speeds, ft/s: what "Minion - Define
/// Locomotion States" passes, in m/s, times its own 3.281 (AG.11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaits {
    pub walk_speed: f32,
    pub run_speed: f32,
}

impl Gaits {
    /// Eruptor's: walk 3.658 and run 6.401 m/s, 12.0019 and 21.0017 ft/s.
    pub const ERUPTOR: Gaits = Gaits {
        walk_speed: 3.658 * 3.281,
        run_speed: 6.401 * 3.281,
    };

    /// The run's share at `speed` ft/s: 0 to the walk's speed, 1 from the
    /// run's, in proportion between.
    pub fn run_share(&self, speed: f32) -> f32 {
        let span = self.run_speed - self.walk_speed;
        if span <= 0.0 {
            return if speed >= self.run_speed { 1.0 } else { 0.0 };
        }
        ((speed - self.walk_speed) / span).clamp(0.0, 1.0)
    }
}

/// The loops of the clips a hero's locomotion plays, seconds; `None` for a
/// slot the character leaves empty (AG.14: 7 have no walk, 8 Giants no run).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loops {
    pub idle: f32,
    pub walk: Option<f32>,
    pub run: Option<f32>,
    pub fall: Option<f32>,
}

/// The four clips the locomotion blends: AE's idle loop and AG.14's
/// walk (slot 1), run (slot 3) and falling (slot 57).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    Idle,
    Walk,
    Run,
    Fall,
}

impl Clip {
    /// [`Locomotion::weights`]' order.
    pub const ALL: [Clip; 4] = [Clip::Idle, Clip::Walk, Clip::Run, Clip::Fall];

    pub fn name(self) -> &'static str {
        match self {
            Clip::Idle => "idle",
            Clip::Walk => "walk",
            Clip::Run => "run",
            Clip::Fall => "fall",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A clip as it plays: seconds into it, unlooped (the evaluator loops it),
/// and its rate, 1 its authored speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Play {
    pub time: f64,
    pub rate: f32,
}

impl Play {
    const START: Play = Play {
        time: 0.0,
        rate: 1.0,
    };

    /// Where it is in its cycle, 0 to 1, over a loop of `length` seconds.
    pub fn phase(&self, length: f32) -> f64 {
        let l = f64::from(length);
        if l > 0.0 {
            (self.time / l).rem_euclid(1.0)
        } else {
            0.0
        }
    }
}

/// A weight sliding from one value to another over a time, eased in and out
/// by `ease` of it at each end (0 linear).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub from: f32,
    pub to: f32,
    pub time: f32,
    pub ease: f32,
    /// Game time since the frame it began, seconds.
    pub elapsed: f64,
}

impl Slide {
    /// No slide: what the idle's weight starts as, and the fall's on the
    /// ground.
    pub fn at(value: f32) -> Slide {
        Slide {
            from: value,
            to: value,
            time: 0.0,
            ease: 0.0,
            elapsed: 0.0,
        }
    }

    pub fn done(&self) -> bool {
        self.elapsed + SLACK >= f64::from(self.time)
    }

    /// The weight now: `from` at its start, `to` once done.
    pub fn value(&self) -> f32 {
        if self.done() {
            return self.to;
        }
        let x = (self.elapsed / f64::from(self.time)) as f32;
        self.from + (self.to - self.from) * eased(x, self.ease)
    }
}

/// How far through a slide `x` of its time is, eased: its rate rising
/// linearly over the first `ease` of the time, constant, then falling over
/// the last `ease`, continuous, 0 at 0 and 1 at 1; linear with no ease.
pub fn eased(x: f32, ease: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let e = ease.clamp(0.0, 0.5);
    if e == 0.0 {
        return x;
    }
    let top = 1.0 / (1.0 - e);
    if x < e {
        top * x * x / (2.0 * e)
    } else if x <= 1.0 - e {
        top * (e / 2.0 + (x - e))
    } else {
        let y = 1.0 - x;
        1.0 - top * y * y / (2.0 * e)
    }
}

/// Where his fall stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Air {
    /// No fall clip.
    Ground,
    /// The gaits sliding out under the fall, then held out until he's down.
    Falling(Slide),
    /// The gaits sliding back; the fall goes once they're whole.
    Landing(Slide),
}

impl Air {
    pub fn name(&self) -> &'static str {
        match self {
            Air::Ground => "ground",
            Air::Falling(_) => "falling",
            Air::Landing(_) => "landing",
        }
    }

    /// The gaits' share against the fall's.
    fn share(&self) -> f32 {
        match self {
            Air::Ground => 1.0,
            Air::Falling(s) | Air::Landing(s) => s.value(),
        }
    }
}

/// What a game frame hands the locomotion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// His walk's length after the push and the edge probe, before the
    /// slowing, ft/s ([`crate::hero::Hero::gait_speed`]).
    pub speed: f32,
    /// How fast this frame's move took him down, ft/s (negative rising).
    pub falling: f32,
    /// His pivot over the ground below after the move, metres; `None` over
    /// none. The next frame's fall reads it with `falling`.
    pub height: Option<f32>,
    /// The edge probe zeroed his walk across the ground this frame, so
    /// `speed` is its vertical part alone ([`crate::hero::Hero::probe_held`]).
    pub held: bool,
}

impl Frame {
    /// The frame `hero` has just stepped.
    pub fn of(hero: &crate::hero::Hero) -> Frame {
        Frame {
            speed: hero.gait_speed,
            falling: -hero.velocity.y / FOOT,
            height: hero.footing.height,
            held: hero.probe_held,
        }
    }
}

/// A hero's locomotion: his clips' weights, times and rates.
#[derive(Debug, Clone, PartialEq)]
pub struct Locomotion {
    gaits: Gaits,
    loops: Loops,
    plays: [Play; 4],
    /// The stand/move switch's side: moving from the frame the speed left 0.
    moving: bool,
    /// The idle's weight.
    still: Slide,
    air: Air,
    run_share: f32,
    speed: f32,
    /// The last frame's edge probe held him ([`Frame::held`]).
    held: bool,
    /// The frame before's move down, ft/s, and the height it left him at,
    /// metres: the fall reads them.
    fell: f32,
    was: Option<f32>,
    steps: u64,
}

/// The script's "greater than": by more than its float tolerance.
fn over(a: f32, b: f32) -> bool {
    a - b > FLOAT_TOLERANCE
}

impl Locomotion {
    /// Standing in his idle, every clip at its start.
    pub fn new(gaits: Gaits, loops: Loops) -> Locomotion {
        Locomotion {
            gaits,
            loops,
            plays: [Play::START; 4],
            moving: false,
            still: Slide::at(1.0),
            air: Air::Ground,
            run_share: 0.0,
            speed: 0.0,
            held: false,
            fell: 0.0,
            was: None,
            steps: 0,
        }
    }

    pub fn gaits(&self) -> &Gaits {
        &self.gaits
    }

    pub fn loops(&self) -> &Loops {
        &self.loops
    }

    /// The clip's loop, seconds, if he has it.
    pub fn length(&self, clip: Clip) -> Option<f32> {
        match clip {
            Clip::Idle => Some(self.loops.idle),
            Clip::Walk => self.loops.walk,
            Clip::Run => self.loops.run,
            Clip::Fall => self.loops.fall,
        }
    }

    pub fn play(&self, clip: Clip) -> Play {
        self.plays[clip.index()]
    }

    /// The speed the last frame read, ft/s.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// The run's share of the gaits the last frame read.
    pub fn run_share(&self) -> f32 {
        self.run_share
    }

    /// The last frame's edge probe held him, so its speed was his walk's
    /// vertical part alone.
    pub fn held(&self) -> bool {
        self.held
    }

    /// He's on the moving side of the stand/move switch.
    pub fn moving(&self) -> bool {
        self.moving
    }

    pub fn air(&self) -> &Air {
        &self.air
    }

    /// Frames stepped since he came in: the gait log's key (`test-hooks`).
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Each clip's weight, in [`Clip::ALL`]'s order, summing to 1.
    pub fn weights(&self) -> [f32; 4] {
        let idle = self.still.value();
        let gaits = 1.0 - idle;
        let (walk, run) = match (self.loops.walk, self.loops.run) {
            (Some(_), Some(_)) => (gaits * (1.0 - self.run_share), gaits * self.run_share),
            (Some(_), None) => (gaits, 0.0),
            (None, Some(_)) => (0.0, gaits),
            // Neither to move in: he stands in his idle.
            (None, None) => (0.0, 0.0),
        };
        let idle = if self.loops.walk.is_none() && self.loops.run.is_none() {
            1.0
        } else {
            idle
        };
        let share = self.air.share();
        [idle * share, walk * share, run * share, 1.0 - share]
    }

    pub fn weight(&self, clip: Clip) -> f32 {
        self.weights()[clip.index()]
    }

    /// The heaviest clip, the first in [`Clip::ALL`]'s order of equals.
    pub fn top(&self) -> Clip {
        let w = self.weights();
        Clip::ALL
            .into_iter()
            .reduce(|best, c| {
                if w[c.index()] > w[best.index()] {
                    c
                } else {
                    best
                }
            })
            .unwrap_or(Clip::Idle)
    }

    /// One game frame of `step` seconds.
    pub fn step(&mut self, step: f32, frame: Frame) {
        let dt = f64::from(step);
        self.still.elapsed += dt;
        if let Air::Falling(s) | Air::Landing(s) = &mut self.air {
            s.elapsed += dt;
        }
        self.fall_or_land();

        let speed = frame.speed;
        let moving = over(speed, 0.0);
        let mut set_off = false;
        if moving && !self.moving {
            set_off = true;
            // Held at an edge, the push that frame's probe zeroed leaves a
            // speed only on a slope: the walk restarts and the run's rate is
            // set as at any set-off, but he stays on the stood side, so the
            // idle's slide in carries on (AG.58).
            if !frame.held {
                self.still = Slide {
                    from: self.still.value(),
                    to: 0.0,
                    time: SET_OFF,
                    ease: EASE,
                    elapsed: 0.0,
                };
            }
            if let Some(walk) = self.loops.walk {
                self.plays[Clip::Walk.index()] = Play {
                    time: WALK_START * f64::from(walk),
                    rate: 1.0,
                };
            }
            if self.loops.run.is_some() {
                self.plays[Clip::Run.index()].rate = speed / self.gaits.run_speed;
            }
        } else if !moving && self.moving {
            self.still = Slide {
                from: self.still.value(),
                to: 1.0,
                time: COME_TO_REST,
                ease: EASE,
                elapsed: 0.0,
            };
            self.plays[Clip::Idle.index()] = Play::START;
        }
        if !(set_off && frame.held) {
            self.moving = moving;
        }

        let share = match (self.loops.walk, self.loops.run) {
            (Some(_), None) => 0.0,
            (None, Some(_)) => 1.0,
            _ => self.gaits.run_share(speed),
        };
        if moving {
            if share <= 0.0 {
                if !set_off {
                    self.plays[Clip::Walk.index()].rate = speed / self.gaits.walk_speed;
                }
            } else if share >= 1.0 {
                self.plays[Clip::Run.index()].rate = speed / self.gaits.run_speed;
            } else if !set_off && let (Some(walk), Some(run)) = (self.loops.walk, self.loops.run) {
                self.plays[Clip::Walk.index()].rate = 1.0 + share * (walk / run - 1.0);
            }
        }

        // The walk's place as the frame began, which the run keeps step with.
        let walk_was = self.loops.walk.map(|l| self.play(Clip::Walk).phase(l));
        for p in &mut self.plays {
            p.time += dt * f64::from(p.rate);
        }
        if moving
            && share > 0.0
            && share < 1.0
            && let (Some(was), Some(run)) = (walk_was, self.loops.run)
        {
            let r = &mut self.plays[Clip::Run.index()];
            r.time = was * f64::from(run) + dt * f64::from(r.rate);
        }

        self.run_share = share;
        self.speed = speed;
        self.held = frame.held;
        self.fell = frame.falling;
        self.was = frame.height;
        self.steps += 1;
    }

    /// The fall, on the move and the height the frame before left.
    fn fall_or_land(&mut self) {
        if self.loops.fall.is_none() {
            return;
        }
        let fast = over(self.fell, FALL_SPEED);
        let high = self.was.is_none_or(|h| h >= HIGH * FOOT);
        if fast && high {
            if self.air == Air::Ground {
                self.air = Air::Falling(Slide {
                    from: 1.0,
                    to: 0.0,
                    time: FALL_IN,
                    ease: 0.0,
                    elapsed: 0.0,
                });
                self.plays[Clip::Fall.index()] = Play::START;
            }
        } else if let Air::Falling(s) = self.air
            && s.done()
        {
            self.air = Air::Landing(Slide {
                from: 0.0,
                to: 1.0,
                time: FALL_OUT,
                ease: 0.0,
                elapsed: 0.0,
            });
            for (clip, length) in [(Clip::Walk, self.loops.walk), (Clip::Run, self.loops.run)] {
                if let Some(l) = length {
                    self.plays[clip.index()].time = LAND_START * f64::from(l);
                }
            }
        }
        if let Air::Landing(s) = self.air
            && s.done()
        {
            self.air = Air::Ground;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const W: f32 = 3.658 * 3.281;
    const R: f32 = 6.401 * 3.281;

    /// Eruptor's loops (AG.14): idle 1.3333 s, walk 1.0667, run 0.6667,
    /// falling 0.8.
    fn eruptor() -> Locomotion {
        Locomotion::new(
            Gaits::ERUPTOR,
            Loops {
                idle: 4.0 / 3.0,
                walk: Some(1.0667),
                run: Some(0.6667),
                fall: Some(0.8),
            },
        )
    }

    fn on_ground(speed: f32) -> Frame {
        Frame {
            speed,
            falling: 0.0,
            height: Some(0.005),
            held: false,
        }
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// Each frame's weights over `speeds`, one frame each.
    fn run(l: &mut Locomotion, speeds: &[f32]) -> Vec<[f32; 4]> {
        speeds
            .iter()
            .map(|&s| {
                l.step(DT, on_ground(s));
                l.weights()
            })
            .collect()
    }

    #[test]
    fn eruptors_authored_speeds_are_metres_turned_feet() {
        assert!(close(Gaits::ERUPTOR.walk_speed, 12.0019, 1e-4));
        assert!(close(Gaits::ERUPTOR.run_speed, 21.0017, 1e-4));
        assert!(close(Gaits::ERUPTOR.walk_speed * FOOT, 3.658179, 1e-5));
        assert!(close(Gaits::ERUPTOR.run_speed * FOOT, 6.401313, 1e-5));
    }

    /// The clip at and around each threshold, speeding up and slowing down:
    /// all walk to the walk's speed, all run from the run's, in proportion
    /// between, the same either way.
    #[test]
    fn the_run_share_at_and_around_each_threshold_both_ways() {
        let g = Gaits::ERUPTOR;
        for (s, want) in [
            (0.0, 0.0),
            (W - 1e-3, 0.0),
            (W, 0.0),
            (W + 1e-3, 1e-3 / (R - W)),
            ((W + R) / 2.0, 0.5),
            (R - 1e-3, 1.0 - 1e-3 / (R - W)),
            (R, 1.0),
            (R + 1e-3, 1.0),
            (21.5, 1.0),
            (70.0, 1.0),
        ] {
            assert!(close(g.run_share(s), want, 2e-5), "{s}: {}", g.run_share(s));
        }
        // Moving at each: up through the speeds, then down through them.
        let ramp: Vec<f32> = (1..=40).map(|k| 0.6 * k as f32).collect();
        let mut l = eruptor();
        let up = run(&mut l, &ramp);
        let down: Vec<f32> = ramp.iter().rev().copied().collect();
        let back = run(&mut l, &down);
        for (k, &s) in ramp.iter().enumerate() {
            let a = up[k];
            let b = back[ramp.len() - 1 - k];
            // The gaits' split is the share, the same both ways (the idle's
            // weight apart, which slides on time).
            let split = |w: [f32; 4]| w[2] / (w[1] + w[2]);
            if k > 0 {
                assert!(close(split(a), g.run_share(s), 1e-5), "up at {s}");
            }
            assert!(close(split(b), g.run_share(s), 1e-5), "down at {s}");
            if s <= W {
                assert_eq!(a[2], 0.0, "no run up to the walk's speed: {s}");
            }
            if s >= R {
                assert_eq!(b[1], 0.0, "no walk from the run's: {s}");
            }
        }
        // Within one frame: the frame the speed passes a threshold is the
        // frame its weights do.
        let mut l = eruptor();
        for _ in 0..30 {
            l.step(DT, on_ground(W));
        }
        assert_eq!(l.weight(Clip::Run), 0.0);
        l.step(DT, on_ground(W + 0.01));
        assert!(l.weight(Clip::Run) > 0.0);
        l.step(DT, on_ground(R));
        assert_eq!(l.weight(Clip::Walk), 0.0);
        assert_eq!(l.top(), Clip::Run);
        l.step(DT, on_ground(R - 0.01));
        assert!(l.weight(Clip::Walk) > 0.0);
        l.step(DT, on_ground(W));
        assert_eq!((l.weight(Clip::Run), l.top()), (0.0, Clip::Walk));
    }

    /// A gait alone plays at the speed over its own; in the mix the walk at
    /// 100 % + 60 % × the run's share and the run in step with it; a gait
    /// not being set keeps its rate, and all of them do at rest.
    #[test]
    fn the_playback_rate() {
        let mut l = eruptor();
        l.step(DT, on_ground(2.0));
        assert_eq!(l.play(Clip::Walk).rate, 1.0, "the set-off frame: 100 %");
        assert!(
            close(l.play(Clip::Run).rate, 2.0 / R, 1e-6),
            "the run's at s/R"
        );
        l.step(DT, on_ground(0.0683 / FOOT));
        assert!(close(l.play(Clip::Walk).rate, 0.0187, 1e-4), "creeping");
        l.step(DT, on_ground(3.2766 / FOOT));
        assert!(close(l.play(Clip::Walk).rate, 0.8957, 1e-4), "half stick");
        let mid = (W + R) / 2.0;
        l.step(DT, on_ground(mid));
        let mix = l.play(Clip::Walk).rate;
        assert!(close(mix, 1.3, 1e-4), "in the mix: {mix}");
        let walk = l.play(Clip::Walk);
        let run = l.play(Clip::Run);
        assert!(
            close(
                walk.phase(1.0667) as f32,
                run.phase(0.6667) as f32 - DT * run.rate / 0.6667 + DT * walk.rate / 1.0667,
                1e-6
            ),
            "the run's place is the walk's as the frame began plus its own advance"
        );
        l.step(DT, on_ground(21.5));
        assert!(close(l.play(Clip::Run).rate, 1.0237, 1e-4), "full stick");
        assert_eq!(
            l.play(Clip::Walk).rate,
            mix,
            "the walk keeps its last under a pure run"
        );
        // At rest every clip keeps what it had.
        l.step(DT, on_ground(0.0));
        assert!(close(l.play(Clip::Run).rate, 1.0237, 1e-4));
        assert_eq!(l.play(Clip::Walk).rate, mix);
        // A rate times the step is the time it advances.
        let before = l.play(Clip::Run).time;
        l.step(DT, on_ground(0.0));
        let advanced = l.play(Clip::Run).time - before;
        assert!(close(advanced as f32, DT * 1.0237, 1e-6), "{advanced}");
        // The idle and the fall at 100 %.
        assert_eq!(l.play(Clip::Idle).rate, 1.0);
        assert_eq!(l.play(Clip::Fall).rate, 1.0);
    }

    /// The stand/move switch's weights over a switch: the idle out over
    /// 0.25 s eased 30/40/30 from the frame he moves, the walk restarting a
    /// quarter of the way into its stride; back over 0.30 s from the frame
    /// he's at rest, the idle restarting; a switch begun part way runs from
    /// where the other got to over its own full time.
    #[test]
    fn the_switchs_weights_over_a_switch() {
        assert_eq!(eased(0.0, EASE), 0.0);
        assert_eq!(eased(1.0, EASE), 1.0);
        assert!(close(eased(0.5, EASE), 0.5, 1e-6));
        // Its rate: rising over the first 30 %, constant, falling.
        assert!(close(eased(0.3, EASE), 0.15 / 0.7, 1e-6));
        assert!(close(eased(0.15, EASE), 0.0375 / 0.7, 1e-6));
        assert!(close(eased(0.85, EASE), 1.0 - 0.0375 / 0.7, 1e-6));
        assert!(close(eased(0.4, 0.0), 0.4, 1e-7), "no ease: linear");
        let mut l = eruptor();
        for _ in 0..3 {
            l.step(DT, on_ground(0.0));
            assert_eq!(l.weights(), [1.0, 0.0, 0.0, 0.0], "standing");
        }
        // The frame he moves: nothing shows yet, the walk at a quarter in.
        l.step(DT, on_ground(2.6667));
        assert_eq!(l.weight(Clip::Idle), 1.0);
        let t = l.play(Clip::Walk).time;
        assert!(close(t as f32, 0.25 * 1.0667 + DT, 1e-6), "{t}");
        let mut idle = Vec::new();
        for k in 1..=16 {
            l.step(DT, on_ground(2.6667 * (k as f32 + 1.0)));
            idle.push(l.weight(Clip::Idle));
        }
        for (k, w) in idle.iter().enumerate() {
            let x = (k as f32 + 1.0) * DT / SET_OFF;
            assert!(close(*w, 1.0 - eased(x, EASE), 1e-5), "frame {k}: {w}");
        }
        assert_eq!(idle[14], 0.0, "out 0.25 s after the frame he moved");
        assert!(idle[13] > 0.0);
        // The linear crossfade misses the eased one by up to 10.5 % of the
        // weight (AG.11's comparison).
        let worst = (1..15)
            .map(|k| ((k as f32 / 15.0) - eased(k as f32 / 15.0, EASE)).abs())
            .fold(0.0f32, f32::max);
        assert!(worst > 0.09 && worst < 0.11, "{worst}");
        // At rest: the frame his speed is 0 the idle restarts; back over
        // 0.30 s.
        l.step(DT, on_ground(0.0));
        assert_eq!(l.weight(Clip::Idle), 0.0);
        assert!(close(l.play(Clip::Idle).time as f32, DT, 1e-7));
        let back: Vec<f32> = (0..18)
            .map(|_| {
                l.step(DT, on_ground(0.0));
                l.weight(Clip::Idle)
            })
            .collect();
        for (k, w) in back.iter().enumerate() {
            let x = (k as f32 + 1.0) * DT / COME_TO_REST;
            assert!(close(*w, eased(x, EASE), 1e-5), "frame {k}: {w}");
        }
        assert_eq!(back[17], 1.0, "whole 0.30 s after");
        // Begun part way: off again 5 frames into the stop, from there over
        // the set-off's own full 0.25 s.
        let mut l = eruptor();
        l.step(DT, on_ground(5.0));
        for _ in 0..20 {
            l.step(DT, on_ground(5.0));
        }
        l.step(DT, on_ground(0.0));
        for _ in 0..5 {
            l.step(DT, on_ground(0.0));
        }
        let before = l.weight(Clip::Idle);
        // The frame he sets off the stop's slide has got one frame further,
        // and the new slide begins from there.
        l.step(DT, on_ground(2.0));
        let got_to = l.weight(Clip::Idle);
        let stop_on = eased(6.0 * DT / COME_TO_REST, EASE);
        assert!(
            close(got_to, stop_on, 1e-6) && got_to > before,
            "{before} {got_to}"
        );
        assert!(got_to > 0.0 && got_to < 0.5, "{got_to}");
        for k in 1..=15 {
            l.step(DT, on_ground(2.0));
            let x = k as f32 * DT / SET_OFF;
            let want = got_to * (1.0 - eased(x, EASE));
            assert!(close(l.weight(Clip::Idle), want, 1e-5), "{k}");
        }
        assert_eq!(l.weight(Clip::Idle), 0.0);
    }

    /// AG.11's set-off at full stick: 2.67 ft/s more a frame from rest, the
    /// run joining the fifth frame he moves (0.0667 s after the first) and
    /// alone from the eighth (0.1167 s), playing at 102.4 %.
    #[test]
    fn off_at_full_stick() {
        let mut l = eruptor();
        let speeds: Vec<f32> = (1..=12)
            .map(|k| (160.0 * DT * k as f32).min(21.5))
            .collect();
        let w = run(&mut l, &speeds);
        let joins = w.iter().position(|w| w[2] > 0.0).unwrap();
        let alone = w.iter().position(|w| w[1] == 0.0 && w[2] > 0.0).unwrap();
        assert_eq!((joins, alone), (4, 7));
        assert!(close(l.play(Clip::Run).rate, 21.5 / R, 1e-6));
    }

    /// Let go at top speed: 80 ft/s² off, all walk 0.133 s later, the stop
    /// the frame after he's at rest, the idle whole 0.30 s after that.
    #[test]
    fn coming_to_rest() {
        let mut l = eruptor();
        for _ in 0..40 {
            l.step(DT, on_ground(21.5));
        }
        let mut walk = 21.5f32;
        let mut frames = Vec::new();
        for _ in 0..60 {
            let speed = walk;
            l.step(DT, on_ground(speed));
            frames.push((speed, l.weights()));
            walk = (walk - 80.0 * DT).max(0.0);
        }
        let all_walk = frames.iter().position(|(_, w)| w[2] == 0.0).unwrap();
        let rest = frames.iter().position(|(s, _)| *s == 0.0).unwrap();
        let whole = frames.iter().position(|(_, w)| w[0] == 1.0).unwrap();
        eprintln!("all walk {all_walk}, at rest {rest}, the idle whole {whole} frames on");
        assert_eq!(all_walk, 8);
        assert_eq!(whole - rest, 18);
        // The walk keeps its last rate as it fades.
        let rate = l.play(Clip::Walk).rate;
        assert!(rate > 0.0 && rate < 0.2, "{rate}");
    }

    /// Into a wall head-on his walk holds 21.5 ft/s: he runs on the spot.
    #[test]
    fn into_a_wall_he_runs_on_the_spot() {
        let mut l = eruptor();
        for _ in 0..60 {
            l.step(DT, on_ground(21.5));
        }
        assert_eq!(l.weights(), [0.0, 0.0, 1.0, 0.0]);
        assert!(close(l.play(Clip::Run).rate * 100.0, 102.37, 0.01));
    }

    /// The fall: the frame after a move down faster than 20 ft/s from 1.5
    /// ft up or more, out over 0.2 s linearly; held while he falls fast and
    /// high; the first frame after that reads anything else, back over 0.12
    /// s, the walk and the run three quarters into their strides. No fall
    /// under 20 ft/s, nor from within 1.5 ft of the ground.
    #[test]
    fn his_fall_and_a_drop_under_20() {
        let fall = |falling: f32, height: f32| Frame {
            speed: 21.5f32.hypot(falling),
            falling,
            height: Some(height),
            held: false,
        };
        let mut l = eruptor();
        for _ in 0..30 {
            l.step(DT, on_ground(21.5));
        }
        // A drop that never passes 20 ft/s, high up.
        for v in [5.0, 12.0, 19.9, 20.0, 20.0001, 7.0, 0.0] {
            l.step(DT, fall(v, 3.0));
            assert_eq!(l.weight(Clip::Fall), 0.0, "{v}");
        }
        // Fast, but within 1.5 ft of the ground.
        l.step(DT, fall(30.0, 0.4571));
        l.step(DT, on_ground(21.5));
        assert_eq!(*l.air(), Air::Ground, "1.4997 ft up");
        // Past it: nothing this frame, the next the slide begins.
        l.step(DT, fall(20.1, 0.4573));
        assert_eq!(*l.air(), Air::Ground);
        l.step(DT, fall(25.0, 2.5));
        assert!(matches!(l.air(), Air::Falling(_)), "1.5003 ft up");
        assert_eq!(l.weight(Clip::Fall), 0.0, "begun: nothing shows yet");
        assert_eq!(l.play(Clip::Fall).time, f64::from(DT), "from its start");
        let out: Vec<f32> = (1..=13)
            .map(|_| {
                l.step(DT, fall(40.0, 2.0));
                1.0 - l.weight(Clip::Fall)
            })
            .collect();
        for (k, g) in out.iter().take(11).enumerate() {
            let want = 1.0 - (k as f32 + 1.0) * DT / FALL_IN;
            assert!(close(*g, want, 1e-5), "{k}: {g}");
        }
        assert_eq!(out[11], 0.0, "out 0.2 s after it began");
        // Held out while he falls fast from 1.5 ft or more; the frame after
        // one that leaves him lower, back over 0.12 s.
        l.step(DT, fall(60.0, 0.4573));
        l.step(DT, fall(60.0, 0.4571));
        assert!(matches!(l.air(), Air::Falling(_)), "read 1.5003 ft up");
        l.step(DT, on_ground(21.5));
        assert!(matches!(l.air(), Air::Landing(_)), "read 1.4997 ft up");
        assert_eq!(l.weight(Clip::Fall), 1.0);
        let walk = l.play(Clip::Walk).time;
        let run = l.play(Clip::Run);
        assert!(close(
            walk as f32,
            0.75 * 1.0667 + DT * l.play(Clip::Walk).rate,
            1e-6
        ));
        assert!(close(run.time as f32, 0.75 * 0.6667 + DT * run.rate, 1e-6));
        let back: Vec<f32> = (0..8)
            .map(|_| {
                l.step(DT, on_ground(21.5));
                l.weight(Clip::Fall)
            })
            .collect();
        for (k, f) in back.iter().take(7).enumerate() {
            let want = 1.0 - (k as f32 + 1.0) * DT / FALL_OUT;
            assert!(close(*f, want, 1e-5), "{k}: {f}");
        }
        assert_eq!(back[7], 0.0);
        assert_eq!(*l.air(), Air::Ground);
        assert_eq!(l.weights(), [0.0, 0.0, 1.0, 0.0]);
    }

    /// Down before the 0.2 s is done: the slide out finishes first. While
    /// the gaits come back, another fast fall waits for them.
    #[test]
    fn a_landing_waits_for_the_slide_out_and_a_fall_for_the_landing() {
        let fast = Frame {
            speed: 30.0,
            falling: 22.0,
            height: Some(0.5),
            held: false,
        };
        let mut l = eruptor();
        l.step(DT, on_ground(21.5));
        l.step(DT, fast);
        let mut began = None;
        for k in 0..13 {
            l.step(DT, on_ground(21.5));
            if began.is_none() && matches!(l.air(), Air::Landing(_)) {
                began = Some(k);
            }
        }
        assert_eq!(began, Some(12), "the frame the 0.2 s is done");
        // Fast and high again while they come back: no new fall.
        for _ in 0..3 {
            l.step(DT, fast);
            assert!(matches!(l.air(), Air::Landing(_)));
        }
        for _ in 0..5 {
            l.step(DT, fast);
        }
        assert_eq!(*l.air(), Air::Ground, "whole: the fall's gone");
        l.step(DT, fast);
        assert!(matches!(l.air(), Air::Falling(_)), "and another begins");
    }

    /// A fall begun while the idle still has weight (off an edge just after
    /// setting off) takes its share from the idle too: the weights sum to 1.
    #[test]
    fn a_fall_takes_its_share_from_the_idle_too() {
        let mut l = eruptor();
        l.step(DT, on_ground(2.0));
        l.step(DT, on_ground(4.0));
        let fast = Frame {
            speed: 25.0,
            falling: 25.0,
            height: Some(3.0),
            held: false,
        };
        l.step(DT, fast);
        l.step(DT, fast);
        for _ in 0..4 {
            l.step(DT, fast);
            let w = l.weights();
            assert!(
                w[0] > 0.0 && w[3] > 0.0,
                "the idle in, the fall coming: {w:?}"
            );
            assert!(close(w.iter().sum::<f32>(), 1.0, 1e-6), "{w:?}");
        }
    }

    /// Standing: the idle loop alone, at 100 %, frame after frame.
    #[test]
    fn standing_plays_the_idle_alone() {
        let mut l = eruptor();
        for k in 1..=500 {
            l.step(DT, on_ground(0.0));
            assert_eq!(l.weights(), [1.0, 0.0, 0.0, 0.0]);
            assert_eq!(l.top(), Clip::Idle);
            assert!(close(l.play(Clip::Idle).time as f32, k as f32 * DT, 1e-4));
        }
        // A speed under the script's float tolerance is still standing.
        l.step(DT, on_ground(1e-4));
        assert!(!l.moving());
        l.step(DT, on_ground(1.1e-4));
        assert!(l.moving());
    }

    /// A character without a run walks at any speed; without a walk he
    /// runs; without a fall he never falls; with neither gait he stands.
    #[test]
    fn missing_clips() {
        let loops = Loops {
            idle: 1.0,
            walk: Some(1.0),
            run: None,
            fall: None,
        };
        let mut l = Locomotion::new(Gaits::ERUPTOR, loops);
        for _ in 0..30 {
            l.step(
                DT,
                Frame {
                    speed: 21.5,
                    falling: 40.0,
                    height: None,
                    held: false,
                },
            );
        }
        assert_eq!(l.weights(), [0.0, 1.0, 0.0, 0.0]);
        assert!(close(l.play(Clip::Walk).rate, 21.5 / W, 1e-6));
        let mut l = Locomotion::new(
            Gaits::ERUPTOR,
            Loops {
                walk: None,
                run: Some(1.0),
                ..loops
            },
        );
        for _ in 0..30 {
            l.step(DT, on_ground(5.0));
        }
        assert_eq!(l.weights(), [0.0, 0.0, 1.0, 0.0]);
        assert!(close(l.play(Clip::Run).rate, 5.0 / R, 1e-6));
        let mut l = Locomotion::new(
            Gaits::ERUPTOR,
            Loops {
                walk: None,
                run: None,
                ..loops
            },
        );
        for _ in 0..30 {
            l.step(DT, on_ground(5.0));
        }
        assert_eq!(l.weights(), [1.0, 0.0, 0.0, 0.0]);
    }
}
