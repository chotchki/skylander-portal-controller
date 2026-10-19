//! A per-frame recording of the oracle (openspec AG.1, design decisions 5 and
//! 6): what O5's watch copied at each flip's first pad read, decoded into the
//! game's clock, the hero, the camera, the stick and the hero's clips, keyed by
//! flip and by game time. The harness decodes Giants' guest bytes into these
//! frames (`tools/oracle-harness`'s `watch`); nothing here knows an address.
//!
//! Game time is the sum of each frame's step, never flips and never the raw
//! frame time: Giants integrates its measured frame time (`_frameDelta`)
//! clamped to between half and three times its nominal frame
//! (`_presentationRate`, 1/60 s), so [1/120, 1/20] s, times the game logic
//! scale (none while its stop flag is up) and the actor's own
//! `_objectTimeScale` (ag-motion section 1, the time scales found in AG.1). At
//! full speed a step is 1/60 s and a long frame is a hitch. [`Clock::step`]
//! does that arithmetic in f32, as the game does.
//!
//! Which frame's step moved the hero from one record to the next is the
//! recording's [`Alignment`], and whether the step moves him by the velocity
//! read before or after it is its [`Order`]: [`Recording::residuals`] measures
//! both against the position steps, and [`Recording::game_time`] keys the
//! frames under the alignment that held (AG.1's tick has it).

use serde::{Deserialize, Serialize};

use crate::MetricError;
use crate::input::Ds3Pad;

/// What one frame's step is made of, as the game read it at that pad read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clock {
    /// `_frameDelta`: the last frame's measured time, seconds.
    pub frame_delta: f32,
    /// `_presentationRate`: the nominal frame, seconds (1/60).
    pub presentation_rate: f32,
    /// What the game multiplies every actor's frame by (a slow-motion
    /// message sets it; 1 otherwise).
    pub logic_scale: f32,
    /// The game logic scale's stop flag: no step at all while it's up.
    pub logic_stopped: bool,
    /// The actor's `_objectTimeScale` (1 for an actor without parameters).
    pub object_scale: f32,
}

impl Clock {
    /// The clamp's bounds, times the nominal frame.
    pub const LOW: f32 = 0.5;
    pub const HIGH: f32 = 3.0;

    /// `_frameDelta` clamped to [LOW, HIGH] x the nominal frame, in f32.
    pub fn clamped(&self) -> f32 {
        let (low, high) = (
            self.presentation_rate * Self::LOW,
            self.presentation_rate * Self::HIGH,
        );
        if self.frame_delta < low {
            low
        } else if high < self.frame_delta {
            high
        } else {
            self.frame_delta
        }
    }

    /// The actor's step, seconds: the clamped frame times the logic scale and
    /// then the object's, each an f32 multiply; 0 while the logic is stopped.
    pub fn step(&self) -> f32 {
        if self.logic_stopped {
            return 0.0;
        }
        (self.clamped() * self.logic_scale) * self.object_scale
    }

    fn finite(&self) -> bool {
        [
            self.frame_delta,
            self.presentation_rate,
            self.logic_scale,
            self.object_scale,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}

/// An actor's placement and motion: metres, then tilt, heading and roll in
/// degrees (a heading θ faces (sin θ, 0, cos θ)), and m/s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub velocity: [f32; 3],
}

/// The camera: its placement (the heading its facing, the tilt its pitch),
/// field of view in degrees and far clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub fov: f32,
    pub far_clip: f32,
}

/// One clip the hero's animation plays this frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Which clip: the game's id for it, stable for the run (the harness keeps
    /// the name beside it).
    pub id: u32,
    /// Its blend weight, percent: the one at the top is what shows.
    pub weight: f32,
    /// Its playback rate, percent.
    pub rate: f32,
    /// Its length, seconds.
    pub period: f32,
    /// Seconds left of it.
    pub remaining: f32,
}

impl Clip {
    /// Seconds into the clip.
    pub fn time(&self) -> f32 {
        self.period - self.remaining
    }
}

/// One record, the frame's state at its first pad read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// The flip whose first pad read took it.
    pub flip: u64,
    /// The host's steady clock then, microseconds.
    pub host_us: u64,
    /// The game's own frame count (`_curFrame`).
    pub game_frame: u32,
    pub clock: Clock,
    pub hero: Body,
    /// The camera, when the watch read one.
    pub camera: Option<View>,
    /// The controller's two stick floats the movement code reads.
    pub stick: [f32; 2],
    /// What the guest read on the hero's pad that flip (O4's readback), when
    /// it was joined.
    pub pad: Option<Ds3Pad>,
    /// The hero's clips, in the game's order.
    pub clips: Vec<Clip>,
}

impl Frame {
    /// The clip at the top of the blend, the first of equal weights.
    pub fn top_clip(&self) -> Option<&Clip> {
        self.clips
            .iter()
            .reduce(|best, c| if c.weight > best.weight { c } else { best })
    }

    fn finite(&self) -> bool {
        let floats = self
            .hero
            .position
            .iter()
            .chain(&self.hero.rotation)
            .chain(&self.hero.velocity)
            .chain(&self.stick);
        let view = self.camera.iter().flat_map(|v| {
            v.position
                .into_iter()
                .chain(v.rotation)
                .chain([v.fov, v.far_clip])
        });
        let clips = self
            .clips
            .iter()
            .flat_map(|c| [c.weight, c.rate, c.period, c.remaining]);
        self.clock.finite() && floats.copied().chain(view).chain(clips).all(f32::is_finite)
    }
}

/// Which frame's step moved the hero from one record's pose to the next's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    /// The step read in the earlier record: the frame time a record holds is
    /// the one the update after its pad read integrates.
    Earlier,
    /// The step read in the later record: the frame time a record holds is
    /// the one the update before its pad read integrated.
    Later,
}

impl Alignment {
    pub const ALL: [Alignment; 2] = [Alignment::Earlier, Alignment::Later];

    /// The record whose step moves record `k`'s pose to record `k + 1`'s.
    fn step_index(self, k: usize) -> usize {
        match self {
            Alignment::Earlier => k,
            Alignment::Later => k + 1,
        }
    }
}

/// Which velocity a step integrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Order {
    /// The velocity is updated first and the position moves by the new one:
    /// the later record's velocity.
    VelocityFirst,
    /// The position moves by the velocity it had, then the velocity updates:
    /// the earlier record's.
    PositionFirst,
}

impl Order {
    pub const ALL: [Order; 2] = [Order::VelocityFirst, Order::PositionFirst];
}

/// How far each position step sits from velocity times step under one
/// alignment and order: ground-plane (x, z) metres and height separately,
/// over the record pairs asked for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Residuals {
    pub alignment: Alignment,
    pub order: Order,
    /// Consecutive flips whose earlier one the caller kept.
    pub pairs: usize,
    /// The largest and the root-mean-square ground-plane miss, metres.
    pub max_ground: f64,
    pub rms_ground: f64,
    /// The pair (its earlier record's flip) with the largest ground-plane miss.
    pub worst_flip: Option<u64>,
    /// The largest miss in y, metres: the ground snap and the slope move the
    /// hero up and down past what his velocity carries.
    pub max_height: f64,
}

/// Frames in flip order, each flip once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Frame>", into = "Vec<Frame>")]
pub struct Recording {
    frames: Vec<Frame>,
}

impl Recording {
    /// Refuses no frames, flips that don't increase and values that aren't
    /// finite.
    pub fn new(frames: Vec<Frame>) -> Result<Self, MetricError> {
        if frames.is_empty() {
            return Err(MetricError::EmptyTrace);
        }
        for pair in frames.windows(2) {
            if pair[1].flip <= pair[0].flip {
                return Err(MetricError::OffsetOrder {
                    previous: pair[0].flip,
                    offset: pair[1].flip,
                });
            }
        }
        if let Some(f) = frames.iter().find(|f| !f.finite()) {
            return Err(MetricError::NotFinite {
                offset: f.flip,
                component: 0,
            });
        }
        Ok(Self { frames })
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Each frame's step, seconds ([`Clock::step`]).
    pub fn steps(&self) -> Vec<f64> {
        self.frames
            .iter()
            .map(|f| f64::from(f.clock.step()))
            .collect()
    }

    /// Game time at each record, seconds from the first: the sum of the steps
    /// that moved the hero there under `alignment`. Under
    /// [`Alignment::Later`] the first record's own step is the one before it,
    /// so it isn't counted; under [`Alignment::Earlier`] the last record's is
    /// the one after it.
    pub fn game_time(&self, alignment: Alignment) -> Vec<f64> {
        let steps = self.steps();
        let mut t = 0.0;
        let mut out = Vec::with_capacity(steps.len());
        out.push(t);
        for k in 0..steps.len() - 1 {
            t += steps[alignment.step_index(k)];
            out.push(t);
        }
        out
    }

    /// The frames keyed by game time under `alignment`: `(flip, seconds)`.
    pub fn keyed(&self, alignment: Alignment) -> Vec<(u64, f64)> {
        self.frames
            .iter()
            .map(|f| f.flip)
            .zip(self.game_time(alignment))
            .collect()
    }

    /// Each position step from record `k` to `k + 1` against the velocity
    /// `order` names times the step `alignment` names, over the pairs whose
    /// earlier flip `keep` accepts and whose flips are consecutive.
    pub fn residuals(
        &self,
        alignment: Alignment,
        order: Order,
        keep: impl Fn(u64) -> bool,
    ) -> Residuals {
        let steps = self.steps();
        let mut out = Residuals {
            alignment,
            order,
            pairs: 0,
            max_ground: 0.0,
            rms_ground: 0.0,
            worst_flip: None,
            max_height: 0.0,
        };
        let mut sum_sq = 0.0;
        for k in 0..self.frames.len() - 1 {
            let (a, b) = (&self.frames[k], &self.frames[k + 1]);
            if b.flip != a.flip + 1 || !keep(a.flip) {
                continue;
            }
            let v = match order {
                Order::VelocityFirst => b.hero.velocity,
                Order::PositionFirst => a.hero.velocity,
            };
            let dt = steps[alignment.step_index(k)];
            let miss = |i: usize| {
                (f64::from(b.hero.position[i]) - f64::from(a.hero.position[i]))
                    - f64::from(v[i]) * dt
            };
            let ground = miss(0).hypot(miss(2));
            out.pairs += 1;
            sum_sq += ground * ground;
            if ground > out.max_ground || out.worst_flip.is_none() {
                out.max_ground = ground;
                out.worst_flip = Some(a.flip);
            }
            out.max_height = out.max_height.max(miss(1).abs());
        }
        if out.pairs > 0 {
            out.rms_ground = (sum_sq / out.pairs as f64).sqrt();
        }
        out
    }
}

impl TryFrom<Vec<Frame>> for Recording {
    type Error = MetricError;

    fn try_from(frames: Vec<Frame>) -> Result<Self, Self::Error> {
        Recording::new(frames)
    }
}

impl From<Recording> for Vec<Frame> {
    fn from(r: Recording) -> Self {
        r.frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOMINAL: f32 = 1.0 / 60.0;

    fn clock(frame_delta: f32) -> Clock {
        Clock {
            frame_delta,
            presentation_rate: NOMINAL,
            logic_scale: 1.0,
            logic_stopped: false,
            object_scale: 1.0,
        }
    }

    fn frame(flip: u64, frame_delta: f32, position: [f32; 3], velocity: [f32; 3]) -> Frame {
        Frame {
            flip,
            host_us: flip * 16_667,
            game_frame: 6_000 + flip as u32,
            clock: clock(frame_delta),
            hero: Body {
                position,
                rotation: [0.0, 15.0, 0.0],
                velocity,
            },
            camera: None,
            stick: [0.0, -1.0],
            pad: None,
            clips: Vec::new(),
        }
    }

    #[test]
    fn a_step_is_the_frame_time_clamped_then_scaled_in_f32() {
        // Inside the clamp, the frame time as measured.
        assert_eq!(clock(0.0158).step(), 0.0158);
        // A hitch is held to three nominal frames, a short frame to half of one.
        assert_eq!(clock(0.2).clamped(), NOMINAL * 3.0);
        assert_eq!(clock(0.001).clamped(), NOMINAL * 0.5);
        // The bounds are themselves inside.
        assert_eq!(clock(NOMINAL * 3.0).clamped(), NOMINAL * 3.0);
        assert_eq!(clock(NOMINAL * 0.5).clamped(), NOMINAL * 0.5);
        // The logic scale and then the object's, each an f32 multiply.
        let scaled = Clock {
            logic_scale: 0.5,
            object_scale: 1.5,
            ..clock(0.0162)
        };
        assert_eq!(scaled.step(), (0.0162f32 * 0.5) * 1.5);
        // A stopped logic steps nothing, whatever the frame took.
        let stopped = Clock {
            logic_stopped: true,
            ..clock(0.0162)
        };
        assert_eq!(stopped.step(), 0.0);
    }

    /// Frames of a hero walking along x from rest under a known law: the
    /// velocity ramps, every step's frame time differs, frame 4 is a hitch.
    fn walk(alignment: Alignment, order: Order) -> Vec<Frame> {
        let deltas = [
            0.0167f32, 0.0158, 0.0175, 0.0166, 0.2, 0.0161, 0.0170, 0.0164, 0.0169,
        ];
        let speeds = [0.0f32, 0.8, 1.6, 2.4, 3.2, 4.0, 4.8, 5.6, 6.4];
        let steps: Vec<f64> = deltas.iter().map(|d| f64::from(clock(*d).step())).collect();
        let mut x = 10.0f64;
        let mut frames = Vec::new();
        for k in 0..deltas.len() {
            if k > 0 {
                let dt = steps[alignment.step_index(k - 1)];
                let v = match order {
                    Order::VelocityFirst => speeds[k],
                    Order::PositionFirst => speeds[k - 1],
                };
                x += f64::from(v) * dt;
            }
            frames.push(frame(
                100 + k as u64,
                deltas[k],
                [x as f32, 43.38, -93.39],
                [speeds[k], 0.0, 0.0],
            ));
        }
        // The last record's step under `Later` is the one after the walk.
        frames
    }

    #[test]
    fn game_time_sums_the_clamped_steps_a_hitch_included() {
        let r = Recording::new(walk(Alignment::Earlier, Order::VelocityFirst)).unwrap();
        // walk()'s frame times, record 4 a 0.2 s hitch.
        let steps: Vec<f64> = [
            0.0167f32, 0.0158, 0.0175, 0.0166, 0.05, 0.0161, 0.0170, 0.0164, 0.0169,
        ]
        .iter()
        .map(|&d| f64::from(d))
        .collect();
        assert_eq!(r.steps()[4], f64::from(NOMINAL * 3.0));
        for (got, want) in r.steps().iter().zip(&steps) {
            assert!((got - want).abs() < 1e-8, "{got} {want}");
        }
        let earlier = r.game_time(Alignment::Earlier);
        let later = r.game_time(Alignment::Later);
        assert_eq!((earlier.len(), later.len()), (9, 9));
        assert_eq!((earlier[0], later[0]), (0.0, 0.0));
        for k in 1..9 {
            // Earlier: the steps of records 0 to k - 1; Later: of records 1 to k.
            let e: f64 = r.steps()[..k].iter().sum();
            let l: f64 = r.steps()[1..=k].iter().sum();
            assert!((earlier[k] - e).abs() < 1e-12 && (later[k] - l).abs() < 1e-12);
        }
        // The hitch counts three nominal frames (an f32 product), never its 0.2 s.
        let hitch = f64::from(NOMINAL * 3.0);
        assert!((earlier[5] - earlier[4] - hitch).abs() < 1e-12);
        assert!((later[4] - later[3] - hitch).abs() < 1e-12);
        // Eight flips past the first record are 0.1661 s of game time, not 8/60.
        assert!((earlier[8] - 0.1661).abs() < 1e-6, "{}", earlier[8]);
        let keyed = r.keyed(Alignment::Later);
        assert_eq!(keyed.len(), 9);
        assert_eq!(keyed[0], (100, 0.0));
        assert_eq!(keyed[8].0, 108);
        assert!((keyed[8].1 - later[8]).abs() < 1e-15);
    }

    #[test]
    fn the_residuals_find_the_alignment_and_order_a_walk_was_made_with() {
        for alignment in Alignment::ALL {
            for order in Order::ALL {
                let r = Recording::new(walk(alignment, order)).unwrap();
                for a in Alignment::ALL {
                    for o in Order::ALL {
                        let got = r.residuals(a, o, |_| true);
                        assert_eq!(got.pairs, 8);
                        if (a, o) == (alignment, order) {
                            // f32 positions near 10 m: a few ulps.
                            assert!(got.max_ground < 1e-5, "{a:?} {o:?}: {got:?}");
                        } else {
                            assert!(
                                got.max_ground > 1e-3,
                                "{a:?} {o:?} on {alignment:?} {order:?}"
                            );
                        }
                        assert!(got.max_height < 1e-12);
                    }
                }
            }
        }
        // Only consecutive flips, and only those kept, are measured.
        let mut frames = walk(Alignment::Earlier, Order::VelocityFirst);
        frames[5].flip += 10;
        frames[6].flip += 10;
        frames[7].flip += 10;
        frames[8].flip += 10;
        let r = Recording::new(frames).unwrap();
        assert_eq!(
            r.residuals(Alignment::Earlier, Order::VelocityFirst, |_| true)
                .pairs,
            7
        );
        assert_eq!(
            r.residuals(Alignment::Earlier, Order::VelocityFirst, |f| f < 102)
                .pairs,
            2
        );
    }

    #[test]
    fn a_recording_refuses_disordered_flips_and_values_that_arent_numbers() {
        let frames = walk(Alignment::Earlier, Order::VelocityFirst);
        assert!(Recording::new(Vec::new()).is_err());
        let mut repeated = frames.clone();
        repeated[3].flip = repeated[2].flip;
        assert_eq!(
            Recording::new(repeated),
            Err(MetricError::OffsetOrder {
                previous: 102,
                offset: 102
            })
        );
        let mut nan = frames.clone();
        nan[4].hero.velocity[1] = f32::NAN;
        assert!(Recording::new(nan).is_err());
        let mut inf = frames.clone();
        inf[2].clock.frame_delta = f32::INFINITY;
        assert!(Recording::new(inf).is_err());
        // JSON round trip, and the same checks on the way in.
        let r = Recording::new(frames).unwrap();
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Recording>(&text).unwrap(), r);
        let mut v: Vec<Frame> = r.into();
        v.swap(0, 1);
        let text = serde_json::to_string(&v).unwrap();
        assert!(serde_json::from_str::<Recording>(&text).is_err());
    }

    #[test]
    fn the_top_clip_is_the_heaviest_and_its_time_counts_up() {
        let mut f = frame(1, 0.0167, [0.0; 3], [0.0; 3]);
        assert!(f.top_clip().is_none());
        let clip = |id, weight, remaining| Clip {
            id,
            weight,
            rate: 100.0,
            period: 1.333_333_4,
            remaining,
        };
        f.clips = vec![
            clip(1, 0.0, 0.2),
            clip(2, 100.0, 1.300_959_5),
            clip(3, 100.0, 0.5),
        ];
        let top = f.top_clip().unwrap();
        assert_eq!(top.id, 2);
        assert!((top.time() - 0.032_373_9).abs() < 1e-6);
    }
}
