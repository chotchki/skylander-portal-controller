//! A per-frame recording of the oracle (openspec AG.1 and AG.35, design
//! decisions 5 and 6): what O5's watch copied at every read of the pad port it
//! names (Giants reads port 0 once a game frame), decoded into the game's
//! clock, the hero, the camera, the stick and the hero's clips, keyed by flip
//! and by game time. The harness decodes Giants' guest bytes into these frames
//! (`tools/oracle-harness`'s `watch`); nothing here knows an address.
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
//!
//! Game frames don't fall one to a flip: a hitch can leave a flip without a pad
//! read and the next flip with two game frames (AG.1's walk-W-03). O5 records
//! every read of its port, so such a flip holds two records, its reads 0 and 1
//! (AG.35; before it O5 took each flip's first read only and lost the second).
//! Each record keeps its flip and that read, so a record is keyed by both: by
//! flip and read, its place against the flip counter (the first read of each
//! flip, [`Recording::per_flip`], is the record a trace keyed by flip offset
//! reads, as before), and by `_curFrame` and game time, its place in the game.
//! Records pair by `_curFrame`, never by flip: two in one flip are one update
//! apart, and two flips apart can be one. Game time can't be summed across a
//! game frame the recording lacks or counts twice, so [`Recording::game_time`]
//! refuses either rather than drop or double a step.

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

/// One entry of the hero's character proxy's manifold: a contact his last
/// step's casts found (AG.9). The point is on the other body's surface, the
/// normal points from it toward the hero, and the distance is the gap between
/// them, negative when he's inside it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub position: [f32; 3],
    /// The point's fourth word: 1 on the entries a step's start points gave,
    /// 0 or a fraction on a cast's hit (as read, not decoded further).
    pub position_w: f32,
    pub normal: [f32; 3],
    pub distance: f32,
    /// The other body's collidable, its guest address: the harness names it.
    pub collider: u32,
}

/// The ground under the hero as the game's ground finder last left it (his
/// corpus's `ContactInfo`): the point it hit, its normal, and the contact's
/// context and flags words as read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ground {
    pub location: [f32; 3],
    /// `_magnitude`: in Giants' hero, the height his pivot was at when it ran.
    pub magnitude: f32,
    pub normal: [f32; 3],
    pub context: u32,
    pub flags: u32,
}

/// What the hero's character proxy held at a record (AG.9): the velocity its
/// last solve left, the support its last check found and the manifold's
/// entries. The game zeroes the manifold's count after every step, so
/// `planes` is every entry the array's capacity holds as that step left them:
/// the step rebuilds the array from its start each cast, so the first is that
/// step's, and a later one can be an earlier step's left behind, which only
/// the records around it tell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contacts {
    pub velocity: [f32; 3],
    /// The support check's state word: 2 supported.
    pub support: u32,
    /// The surface normal the support check found.
    pub surface: [f32; 3],
    /// The proxy's four flag bytes after its step, as read.
    pub flags: [u8; 4],
    pub ground: Option<Ground>,
    pub planes: Vec<Plane>,
}

/// One record: the game's state at a read of the watched pad port, once a
/// game frame in Giants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// The flip the read fell in.
    pub flip: u64,
    /// Which of the port's reads in that flip took it, 0 the first: 1 is a
    /// second game frame run in the same flip. 0 in a recording made before
    /// AG.35, which took each flip's first read alone.
    #[serde(default)]
    pub read: u32,
    /// The host's steady clock then, microseconds.
    pub host_us: u64,
    /// The game's own frame count (`_curFrame`).
    pub game_frame: u32,
    pub clock: Clock,
    pub hero: Body,
    /// The camera, when the watch read one.
    pub camera: Option<View>,
    /// The stick as the game holds it: x, -1 full left, and y, +1 full
    /// forward, after the game's reading of the pad's bytes.
    pub stick: [f32; 2],
    /// What the guest read on the hero's pad that flip (O4's readback), when
    /// it was joined.
    pub pad: Option<Ds3Pad>,
    /// The hero's clips, in the game's order.
    pub clips: Vec<Clip>,
    /// His character proxy's contacts, when the watch held them (AG.9 on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Contacts>,
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
    /// Consecutive records one game frame apart whose earlier one's flip the
    /// caller kept.
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

/// A record's keys: its flip and which of the port's reads there it was, its
/// game frame (`_curFrame`) and the game time at it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    /// Seconds of game time from the recording's first record.
    pub time: f64,
}

/// Frames in the order the port was read: by flip, and within a flip by read,
/// each flip's reads 0, 1, ... with none missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Frame>", into = "Vec<Frame>")]
pub struct Recording {
    frames: Vec<Frame>,
}

impl Recording {
    /// Refuses no frames, flips that go back, a record that isn't the read
    /// after the one before it in its flip or the first of a new flip (the
    /// first record may be a later read, the ring having overwritten the ones
    /// before it) and values that aren't finite.
    pub fn new(frames: Vec<Frame>) -> Result<Self, MetricError> {
        if frames.is_empty() {
            return Err(MetricError::EmptyTrace);
        }
        for pair in frames.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if b.flip < a.flip {
                return Err(MetricError::OffsetOrder {
                    previous: a.flip,
                    offset: b.flip,
                });
            }
            let next = if b.flip == a.flip {
                a.read.checked_add(1) == Some(b.read)
            } else {
                b.read == 0
            };
            if !next {
                return Err(MetricError::ReadOrder {
                    previous_flip: a.flip,
                    previous_read: a.read,
                    flip: b.flip,
                    read: b.read,
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

    /// The first record of each flip that has one: what O5 recorded before
    /// AG.35, and the record a trace keyed by flip offset (AF.10's, an AF.7
    /// script's) reads at that flip.
    pub fn per_flip(&self) -> impl Iterator<Item = &Frame> {
        self.frames.iter().filter(|f| f.read == 0)
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
    /// the one after it. Refused when two records' `_curFrame` aren't one
    /// apart: [`MetricError::FrameGap`] when the frame between them is missing
    /// and so is its step, [`MetricError::FrameRepeat`] when they're the same
    /// frame, whose step would count twice. Flips that skip, or hold two
    /// records, with the game frames one apart are fine.
    pub fn game_time(&self, alignment: Alignment) -> Result<Vec<f64>, MetricError> {
        if let Some(w) = self
            .frames
            .windows(2)
            .find(|w| !consecutive_frames(&w[0], &w[1]))
        {
            let (a, b) = (&w[0], &w[1]);
            return Err(if b.game_frame == a.game_frame {
                MetricError::FrameRepeat {
                    flip: b.flip,
                    frame: b.game_frame,
                }
            } else {
                MetricError::FrameGap {
                    flip: b.flip,
                    previous: a.game_frame,
                    frame: b.game_frame,
                }
            });
        }
        let steps = self.steps();
        let mut t = 0.0;
        let mut out = Vec::with_capacity(steps.len());
        out.push(t);
        for k in 0..steps.len() - 1 {
            t += steps[alignment.step_index(k)];
            out.push(t);
        }
        Ok(out)
    }

    /// Each frame's keys, its game time under `alignment`, refused as
    /// [`Recording::game_time`] refuses.
    pub fn keyed(&self, alignment: Alignment) -> Result<Vec<Key>, MetricError> {
        Ok(self
            .frames
            .iter()
            .zip(self.game_time(alignment)?)
            .map(|(f, time)| Key {
                flip: f.flip,
                read: f.read,
                game_frame: f.game_frame,
                time,
            })
            .collect())
    }

    /// Each position step from record `k` to `k + 1` against the velocity
    /// `order` names times the step `alignment` names, over the pairs whose
    /// earlier flip `keep` accepts and whose game frames are consecutive, so
    /// one update lies between them (their flips may be one apart, further
    /// apart or the same, AG.35).
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
            if !consecutive_frames(a, b) || !keep(a.flip) {
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

/// Whether `b` is the game frame right after `a`'s (`_curFrame` one on), so
/// exactly one update lies between the two records.
fn consecutive_frames(a: &Frame, b: &Frame) -> bool {
    b.game_frame == a.game_frame.wrapping_add(1)
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
            read: 0,
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
            contacts: None,
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
        let earlier = r.game_time(Alignment::Earlier).unwrap();
        let later = r.game_time(Alignment::Later).unwrap();
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
        let keyed = r.keyed(Alignment::Later).unwrap();
        assert_eq!(keyed.len(), 9);
        assert_eq!(
            keyed[0],
            Key {
                flip: 100,
                read: 0,
                game_frame: 6_100,
                time: 0.0
            }
        );
        assert_eq!((keyed[8].flip, keyed[8].game_frame), (108, 6_108));
        assert!((keyed[8].time - later[8]).abs() < 1e-15);
    }

    /// `walk()` with flip 104 left without a pad read and flip 105 running two
    /// game frames, its reads 0 and 1 (AG.1's walk-W-03, AG.35).
    fn two_in_a_flip() -> Vec<Frame> {
        let mut frames = walk(Alignment::Earlier, Order::VelocityFirst);
        frames[4].flip = 105;
        frames[5].read = 1;
        frames
    }

    #[test]
    fn a_flip_holding_two_game_frames_keys_both_and_loses_neither() {
        let whole = Recording::new(walk(Alignment::Earlier, Order::VelocityFirst)).unwrap();
        let r = Recording::new(two_in_a_flip()).unwrap();
        // Same frames, same steps: the same game time and the same fit.
        for a in Alignment::ALL {
            assert_eq!(r.game_time(a), whole.game_time(a));
        }
        let fit = r.residuals(Alignment::Earlier, Order::VelocityFirst, |_| true);
        assert_eq!(fit.pairs, 8);
        assert!(fit.max_ground < 1e-5, "{fit:?}");
        // Keyed by both: flip 105's two reads are game frames 6104 and 6105.
        let keys = r.keyed(Alignment::Earlier).unwrap();
        let k: Vec<(u64, u32, u32)> = keys
            .iter()
            .map(|k| (k.flip, k.read, k.game_frame))
            .collect();
        assert_eq!(
            k[3..7],
            [
                (103, 0, 6_103),
                (105, 0, 6_104),
                (105, 1, 6_105),
                (106, 0, 6_106)
            ]
        );
        assert!(keys[5].time > keys[4].time);
        // One record a flip is what O5 took before AG.35: game frame 6105 lost.
        let firsts: Vec<u32> = r.per_flip().map(|f| f.game_frame).collect();
        assert_eq!(firsts.len(), 8);
        assert!(!firsts.contains(&6_105));
        assert_eq!(whole.per_flip().count(), 9);
        // And kept alone, those records can't sum game time across the loss.
        let old = Recording::new(r.per_flip().cloned().collect()).unwrap();
        assert_eq!(
            old.game_time(Alignment::Earlier),
            Err(MetricError::FrameGap {
                flip: 106,
                previous: 6_104,
                frame: 6_106
            })
        );
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
        // Only pairs one game frame apart, and only those kept, are measured,
        // whatever their flips (AG.1's walk-W-03, AG.35): a flip with no pad
        // read leaves two flips one game frame apart, which counts...
        let mut skipped = walk(Alignment::Earlier, Order::VelocityFirst);
        for f in &mut skipped[5..] {
            f.flip += 1;
        }
        let r = Recording::new(skipped).unwrap();
        let fit = r.residuals(Alignment::Earlier, Order::VelocityFirst, |_| true);
        assert_eq!(fit.pairs, 8);
        assert!(fit.max_ground < 1e-5, "{fit:?}");
        assert_eq!(
            r.residuals(Alignment::Earlier, Order::VelocityFirst, |f| f < 102)
                .pairs,
            2
        );
        // ...and the next flip holding two game frames, one unrecorded, leaves
        // two consecutive flips two updates apart, which doesn't.
        let mut lost = walk(Alignment::Earlier, Order::VelocityFirst);
        for f in &mut lost[5..] {
            f.game_frame += 1;
        }
        let r = Recording::new(lost).unwrap();
        assert_eq!(
            r.residuals(Alignment::Earlier, Order::VelocityFirst, |_| true)
                .pairs,
            7
        );
    }

    #[test]
    fn game_time_refuses_a_lost_game_frame_and_takes_a_skipped_flip() {
        // Two flips one game frame apart (a flip without a pad read): the
        // same steps, the same game time.
        let whole = Recording::new(walk(Alignment::Earlier, Order::VelocityFirst)).unwrap();
        let mut skipped = walk(Alignment::Earlier, Order::VelocityFirst);
        for f in &mut skipped[3..] {
            f.flip += 1;
        }
        let skipped = Recording::new(skipped).unwrap();
        for a in Alignment::ALL {
            assert_eq!(skipped.game_time(a), whole.game_time(a));
            let keyed = skipped.keyed(a).unwrap();
            assert_eq!((keyed[3].flip, keyed[3].game_frame), (104, 6_103));
        }
        // A game frame the ring never took: its step is missing, so no sum.
        let mut lost = walk(Alignment::Earlier, Order::VelocityFirst);
        for f in &mut lost[6..] {
            f.game_frame += 1;
        }
        let lost = Recording::new(lost).unwrap();
        for a in Alignment::ALL {
            let want = MetricError::FrameGap {
                flip: 106,
                previous: 6_105,
                frame: 6_107,
            };
            assert_eq!(lost.game_time(a).unwrap_err(), want);
            assert_eq!(lost.keyed(a).unwrap_err(), want);
        }
        // The port read twice in one game frame: its step would count twice.
        let mut twice = two_in_a_flip();
        twice[5].game_frame = twice[4].game_frame;
        for f in &mut twice[6..] {
            f.game_frame -= 1;
        }
        let twice = Recording::new(twice).unwrap();
        for a in Alignment::ALL {
            let want = MetricError::FrameRepeat {
                flip: 105,
                frame: 6_104,
            };
            assert_eq!(twice.game_time(a).unwrap_err(), want);
            assert_eq!(twice.keyed(a).unwrap_err(), want);
        }
        assert_eq!(
            twice
                .residuals(Alignment::Earlier, Order::VelocityFirst, |_| true)
                .pairs,
            7
        );
    }

    #[test]
    fn a_recording_refuses_disordered_flips_and_values_that_arent_numbers() {
        let frames = walk(Alignment::Earlier, Order::VelocityFirst);
        assert!(Recording::new(Vec::new()).is_err());
        // A flip that goes back.
        let mut back = frames.clone();
        back[3].flip = 101;
        assert_eq!(
            Recording::new(back),
            Err(MetricError::OffsetOrder {
                previous: 102,
                offset: 101
            })
        );
        // Two records at one flip are its reads 0 and 1, and only those.
        let mut repeated = frames.clone();
        repeated[3].flip = repeated[2].flip;
        assert_eq!(
            Recording::new(repeated.clone()),
            Err(MetricError::ReadOrder {
                previous_flip: 102,
                previous_read: 0,
                flip: 102,
                read: 0
            })
        );
        repeated[3].read = 1;
        assert!(Recording::new(repeated.clone()).is_ok());
        repeated[3].read = 2;
        assert!(Recording::new(repeated).is_err());
        // A later read with its flip's first missing, unless the ring overwrote
        // it (the first record).
        let mut missing_first = frames.clone();
        missing_first[3].read = 1;
        assert_eq!(
            Recording::new(missing_first),
            Err(MetricError::ReadOrder {
                previous_flip: 102,
                previous_read: 0,
                flip: 103,
                read: 1
            })
        );
        let mut wrapped = frames.clone();
        wrapped[0].read = 1;
        assert!(Recording::new(wrapped).is_ok());
        let mut nan = frames.clone();
        nan[4].hero.velocity[1] = f32::NAN;
        assert!(Recording::new(nan).is_err());
        let mut inf = frames.clone();
        inf[2].clock.frame_delta = f32::INFINITY;
        assert!(Recording::new(inf).is_err());
        // JSON round trip, and the same checks on the way in.
        let r = Recording::new(frames.clone()).unwrap();
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Recording>(&text).unwrap(), r);
        let mut v: Vec<Frame> = r.into();
        v.swap(0, 1);
        let text = serde_json::to_string(&v).unwrap();
        assert!(serde_json::from_str::<Recording>(&text).is_err());
        // A recording from before AG.35 carries no read: each record is read 0.
        let mut old = serde_json::to_value(&frames).unwrap();
        for f in old.as_array_mut().unwrap() {
            f.as_object_mut().unwrap().remove("read");
        }
        let old: Recording = serde_json::from_value(old).unwrap();
        assert_eq!(old.frames(), &frames[..]);
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
