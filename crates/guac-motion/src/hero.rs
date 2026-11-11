//! The hero controller (openspec AG.16, design decisions 4 and 5): one game
//! frame of a hero, in the original's order, on any [`PhysicsSeam`].
//!
//! Each [`Hero::step`] runs, as the movement script and the engine do:
//! 1. Face and Move ([`crate::walk`]): with the stick held his facing turns
//!    toward it, and unless the edge probe holds his controller off, the walk
//!    turns and is pushed;
//! 2. the edge probe ([`crate::edge`]), which can zero the walk's across part
//!    before he moves and hold the controller off; the walk's length here is
//!    the speed his locomotion reads ([`Hero::gait_speed`], AG.30);
//! 3. Update Movers ([`crate::walk`]): the slowing, the stop, the band,
//!    gravity, the landing;
//! 4. the engine: the walk in m/s, laid along the ground while he's on it
//!    ([`crate::proxy::lay_along`]), moved through the cast-and-slide loop
//!    ([`crate::proxy`]) over this frame's step, then the support check, the
//!    ground finder, the snap and the lean ([`crate::ground`]), which become
//!    the next frame's [`Footing`].
//!
//! The frame's own step moves him; the push and the facing integrate the step
//! two frames before it and the slowing and gravity the one a frame before it
//! (AG.8, AG.10, AG.38: "the step three records back" and "two records back"
//! counted from the records either side of a step). At a steady 1/60 s these
//! are all the same, and AG.18's replay feeds a recording's own.
//!
//! Hypotheses here, each with the reading or the recording behind it:
//! - the engine walks him along the ground (and the snap holds him to it
//!   from up to 0.375 m) only while the support check passed, the ground
//!   faces up past 0.69 and the walk isn't leaving it faster than 0.01 m/s
//!   (the velocity model as read);
//! - a frame that fails that test ends without support whatever the support
//!   check finds when the walk also leaves the ground faster than 0.01 of
//!   its own speed (the post-step's airborne test, as read; the corpus's
//!   0x200 flag, set only in frames without support, AG.38): it's what takes
//!   him off the ground for a frame over a crest (AG.38's dip edge) and on
//!   the first frame he's let go walking uphill (AG.38's `g38_t2979_up`,
//!   record 381), while a slow walk rising off a slope stays supported (of
//!   the 632 frames of AG.38's run that fail the leave test, the two tests
//!   together call 608 as recorded, the leave test alone 97);
//! - the lean turns before the snap, and the snap never sets his shape
//!   inside the ground: until his lean reaches a slope he rides as high as
//!   his shape needs there (AG.39: no landing rose after it, where a snap to
//!   10 mm with his sphere still upright would set it 35 mm into a 17°
//!   slope), and sets him down no higher than his move left him (the
//!   ground finder's face, its plane run on past a convex crest, lifted him
//!   2.3-5.7 m at the ledge route's edge in 29 of AG.39's 48 runs off it,
//!   none of theirs over 1.2 cm: AG.16's review);
//! - the post-step as read (AG.70, AG.71), each part checked on the
//!   recordings: within the snap's 0.375 m over ground facing up past 45°
//!   he counts as on the ground, so his lean turns to that ground and a
//!   frame begun on the ground (the walk not leaving it) ends supported
//!   whatever the support check found (along AG.58's door he rode up to
//!   0.288 m over ground falling away under him, supported); the snap from
//!   over the ground runs only on a frame his lean turns, from under it on
//!   any (he rode up to 25.2 mm up, unsnapped, while his lean held);
//! - the edge probe's hold turns his facing to the stick at his usual rate;
//! - his collision shape turns with his lean about his pivot (AG.9: on the
//!   slopes his proxy's plane distance matched his pivot's height, which only
//!   a leaned sphere gives);
//! - off the ground his up axis turns back toward straight up at the same
//!   120°/s over the frame's own step (the post-step as read; AG.71: over
//!   AG.39's falls 109 of 111 frames in the air and 55 of 56 near the
//!   ground turned as predicted to 0.01°, and AG.48's four air replays'
//!   lean follows theirs to 0.03°), and his lean turns with his facing
//!   about the vertical, being his transform's (990 of 1,197 frames in the
//!   air or near the ground and 6,147 of 6,760 on it that turned him, to
//!   0.01°);
//! - the downhill hug rays never run for a hero: they need a ground
//!   acceleration in his `ActorParameters`, which read 0 through every walk
//!   (AG.8), so none are built.

use std::collections::{BTreeMap, VecDeque};

use crate::edge;
use crate::ground::{self, LEAN_SETTLED, MAX_SLOPE_COS, SNAP_HEIGHT, SNAP_REACH, SUPPORT_GAP};
use crate::proxy::{self, AIRBORNE_SHARE, KEEP, LEAVE_NORMAL, LEAVE_SPEED, Proxy, Surfaces};
use crate::seam::{BodyId, PhysicsSeam, Pose, Quat, Shape, Vec3};
use crate::solid::HavokSolid;
use crate::steer;
use crate::stick::Stick;
use crate::tuning::{FOOT, NOMINAL_STEP, Tuning};
use crate::walk::{Footing, Walk, turn_facing};

/// What a steered actor's fall presses into the ground past, ft/s, to be
/// taken off as it lands ([`Hero::step_steered`]): over the 35 ft/s every
/// other landing the suites hold comes to, a Chompy's off its pod, a
/// knockback's, and under the 47-89 of Setup_004B's lobber's drop and its
/// closets' jumps, so only those reach it. A hero's own frame never comes
/// here. A hypothesis; no recording has an enemy's fall (stand-in ledger:
/// `hard-landing`).
pub const HARD_LANDING: f32 = 40.0;

/// Passes [`Hero::push_out`] takes off the walls he's in before it falls
/// back on the search.
pub const PUSH_PASSES: usize = 4;
/// The search's reach and lift (the original's `Portal - Minion Entry Find
/// Safe Location` as AG.289's diagnosis reads it): eight headings, 10 ft out
/// at 2 ft up, ground within 3 ft below.
pub const SEARCH_HEADINGS: usize = 8;
pub const SEARCH_OUT: f32 = 10.0 * FOOT;
pub const SEARCH_UP: f32 = 2.0 * FOOT;
pub const SEARCH_DOWN: f32 = 3.0 * FOOT;
/// The wider search (AS.251), when none of those eight is a fit: a ring
/// every [`FAR_STEP`] out to [`FAR_OUT`], [`FAR_HEADINGS`] round, nearest
/// first, each spot the ground found under a lift of [`SEARCH_UP`] plus
/// [`FAR_SLOPE`] a metre out, standing within the same lift above and
/// [`SEARCH_DOWN`] plus that below the place he was set down, and only where
/// a walk from there reaches ([`REACH_UP`], [`REACH_DOWN`]). A Giant swapped
/// in where a smaller hero stood (a ledge route 4 m wide, his capsule 4.3
/// across) fits nowhere near it, and the nearest place that does is 9-10 m
/// along.
pub const FAR_OUT: f32 = 30.0;
pub const FAR_STEP: f32 = 1.0;
pub const FAR_HEADINGS: usize = 24;
pub const FAR_SLOPE: f32 = 0.5;
/// How far past a solid the line to a spot resumes.
pub const FAR_THROUGH: f32 = 0.05;
/// The walk the wider search's spots must be reached by (AS.251's review):
/// a step [`FAR_STEP`] across climbs at most this (the search's own lift)
/// and drops at most [`REACH_DOWN`], on ground he can stand on, with
/// nothing solid across it from [`REACH_KNEE`] to [`REACH_HEAD`] over the
/// ground. A gap, a ledge over his step, a closed door, a gate or an
/// elemental wall ends it, so a spot past one is never his, however clear
/// the line to it.
pub const REACH_UP: f32 = SEARCH_UP;
pub const REACH_DOWN: f32 = 4.0;
pub const REACH_KNEE: f32 = SEARCH_UP + 0.2;
pub const REACH_HEAD: f32 = 1.5;
/// How far past [`KEEP`] into a wall [`Hero::push_out`] counts him in it: a
/// pass leaves him on the keep, give or take the float's noise.
pub const IN_A_WALL: f32 = 1e-4;
/// How far into a flat ceiling (AS.251) counts him in it: a centimetre, since
/// no push moves him off one and the search is for a hero that doesn't fit.
pub const IN_A_CEILING: f32 = 0.01;

/// How far below the last ground under him a fall ends, metres, with no
/// script to end it: past the 225 ft the minion's own check reads, and past
/// the seconds its trail's timer can hold that check off at his 120 ft/s.
pub const FALL_DEPTH: f32 = 1000.0 * FOOT;
/// Steps between one safe place and the next: he's set back where he stood
/// this long ago at least, not on the edge he went off.
pub const FALL_SAFE_STEPS: u32 = 30;

/// What [`Hero::catch_fall`] keeps between frames.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FallCatch {
    /// Where he stood on the ground a while ago.
    pub safe: Option<Vec3>,
    /// Where he stood the last time it moved on.
    next: Option<Vec3>,
    /// Steps since then, on the ground.
    ground_steps: u32,
}

/// What [`Hero::push_out`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PushOut {
    /// He was in no wall: left as he was.
    Clear,
    /// Pushed off the walls he was in, by this much across the ground.
    Pushed(Vec3),
    /// Still in after [`PUSH_PASSES`]: set down at the first of the search's
    /// spots that's clear with ground under it (past the eight, one a walk
    /// from his place reaches), by this much.
    Searched(Vec3),
    /// In, and no spot the search tried was clear: left where he was.
    Stuck,
}

/// What a frame reads of the player: the walking pad and the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Input {
    pub stick: Stick,
    /// The heading the view looked along across the ground the frame before
    /// (gameplay-camera: the stick walks him against it).
    pub view_heading: f32,
    /// The four face buttons by position (bottom, right, left, top). Nothing
    /// in his walk reads them: no hero jumps in Giants (hero-movement's No
    /// jumping).
    pub buttons: [bool; 4],
}

impl Input {
    pub fn stick(stick: Stick, view_heading: f32) -> Input {
        Input {
            stick,
            view_heading,
            buttons: [false; 4],
        }
    }
}

/// A ground this far over his pivot (metres, negative) can only be the
/// finder's upward ray's: its downward one starts [`ground::find`]'s 0.1 m up.
const OVER_HIS_HEAD: f32 = -0.11;

/// Two heights at one point of [`Reach`]'s grid this close are one ground.
const REACH_LAYER: f32 = 1.5;
/// How far past a step's far point [`Reach`] looks for a wall, metres.
const REACH_PAST: f32 = 0.1;
/// How wide a gap [`Reach`]'s step passes, metres: under any hero's width.
const REACH_BALL: f32 = 0.2;

/// Where a walk from a place goes (AS.251's review): a grid [`FAR_STEP`]
/// apart from it out past [`FAR_OUT`], each point the ground a step from
/// one already reached, by [`REACH_UP`], [`REACH_DOWN`] and [`REACH_KNEE`].
/// A walk, not the hero's: a thin rule for "nothing the level holds shut
/// lies between", so the wider search's spot is one he could have walked to
/// had he been small enough.
struct Reach {
    from: Vec3,
    /// The heights reached at each point, by its steps across from `from`.
    at: BTreeMap<(i32, i32), Vec<f32>>,
}

impl Reach {
    fn from(seam: &dyn PhysicsSeam, exclude: &[BodyId], from: Vec3) -> Reach {
        let mut at = BTreeMap::from([((0, 0), vec![from.y])]);
        let mut queue = VecDeque::from([(0i32, 0i32, from.y)]);
        let most = (FAR_OUT / FAR_STEP).ceil() as i32 + 1;
        let point = |i: i32, j: i32, y: f32| {
            Vec3::new(
                from.x + i as f32 * FAR_STEP,
                y,
                from.z + j as f32 * FAR_STEP,
            )
        };
        while let Some((i, j, y)) = queue.pop_front() {
            let p = point(i, j, y);
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (ni, nj) = (i + di, j + dj);
                if ni * ni + nj * nj > most * most {
                    continue;
                }
                let over = point(ni, nj, y + REACH_UP);
                let Some(hit) =
                    ground::ray(seam, over, Vec3::NEG_Y, REACH_UP + REACH_DOWN, exclude)
                else {
                    continue;
                };
                if hit.normal.y <= MAX_SLOPE_COS {
                    continue;
                }
                let q = hit.point;
                if !Reach::open(seam, exclude, p, q) {
                    continue;
                }
                let heights = at.entry((ni, nj)).or_insert_with(Vec::new);
                if heights.iter().any(|h| (h - q.y).abs() < REACH_LAYER) {
                    continue;
                }
                heights.push(q.y);
                queue.push_back((ni, nj, q.y));
            }
        }
        Reach { from, at }
    }

    /// Nothing solid across the step from `p` to `q`: a post
    /// [`REACH_BALL`] across standing from [`REACH_KNEE`] to [`REACH_HEAD`]
    /// over the ground, swept from over `p` to over the higher of the two
    /// and [`REACH_PAST`] beyond, so a wall standing on `q` itself ends the
    /// step, a barrier he couldn't walk under does, and the seam between two
    /// of a wall's triangles lets nothing through, as a ray along it can.
    fn open(seam: &dyn PhysicsSeam, exclude: &[BodyId], p: Vec3, q: Vec3) -> bool {
        let mid = (REACH_KNEE + REACH_HEAD) / 2.0;
        let a = p + Vec3::Y * mid;
        let b = Vec3::new(q.x, q.y.max(p.y) + mid, q.z);
        let d = b - a;
        let Some(way) = d.try_normalize() else {
            return true;
        };
        let post = Shape::Capsule {
            radius: REACH_BALL / 2.0,
            half_segment: (REACH_HEAD - REACH_KNEE) / 2.0,
        };
        seam.cast(
            &post,
            Pose::at(a),
            way * (d.length() + REACH_PAST),
            0.0,
            exclude,
        )
        .is_none()
    }

    /// Whether the walk reaches the ground at `ground`: a grid point beside
    /// it reached at its height, with nothing solid across to it.
    fn reaches(&self, seam: &dyn PhysicsSeam, exclude: &[BodyId], ground: Vec3) -> bool {
        let (x, z) = (
            (ground.x - self.from.x) / FAR_STEP,
            (ground.z - self.from.z) / FAR_STEP,
        );
        let (i0, j0) = (x.floor() as i32, z.floor() as i32);
        [(i0, j0), (i0 + 1, j0), (i0, j0 + 1), (i0 + 1, j0 + 1)]
            .into_iter()
            .any(|(i, j)| {
                self.at.get(&(i, j)).is_some_and(|heights| {
                    heights.iter().any(|&h| {
                        (h - ground.y).abs() <= REACH_UP
                            && Reach::open(
                                seam,
                                exclude,
                                Vec3::new(
                                    self.from.x + i as f32 * FAR_STEP,
                                    h,
                                    self.from.z + j as f32 * FAR_STEP,
                                ),
                                ground,
                            )
                    })
                })
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hero {
    /// His pivot, the bottom centre of his solid, metres.
    pub pivot: Vec3,
    /// Degrees, θ facing (sin θ, 0, cos θ).
    pub facing: f32,
    /// His up axis, which leans with the ground.
    pub up: Vec3,
    pub walk: Walk,
    pub footing: Footing,
    /// The height of the last ground under him, metres: what a fall with
    /// none under him is measured from ([`Hero::fallen`]).
    pub last_ground: Option<f32>,
    /// Where [`Hero::catch_fall`] sets him back to.
    pub catch: FallCatch,
    /// His last step's move over its step, m/s.
    pub velocity: Vec3,
    /// His last step's walk as the move solved it against what it met, m/s
    /// ([`crate::proxy::Moved::velocity`]): what a steered actor's walk
    /// becomes ([`Hero::step_steered`]).
    pub solved: Vec3,
    /// The edge probe holds his controller off.
    pub held_off: bool,
    /// This frame's edge probe found no ground and zeroed his walk across
    /// the ground before it was read: what [`Hero::gait_speed`] holds of it
    /// is the walk's vertical part alone (AG.58; [`crate::locomotion`]
    /// doesn't let such a frame set him off).
    pub probe_held: bool,
    /// His walk's length, ft/s, after this frame's push and the edge probe
    /// and before its slowing: the speed his locomotion mixes his gaits by
    /// (AG.11, [`crate::locomotion`]).
    pub gait_speed: f32,
    /// Hung on something a script hangs him from (openspec AS.115: a
    /// scene's rig or ship, a turret): his controller takes no step, and his
    /// place is what he hangs from's.
    pub hung: bool,
    /// His last step off the ground got him nowhere, asked to move
    /// (`Proxy::integrate`'s `stalled`, openspec AS.159).
    stalled: bool,
    /// The steps one and two frames before the next.
    steps: [f32; 2],
    solid: HavokSolid,
    tuning: Tuning,
}

impl Hero {
    /// At rest with his pivot at `pivot`, upright, facing `facing`.
    pub fn new(solid: HavokSolid, tuning: Tuning, pivot: Vec3, facing: f32) -> Hero {
        Hero {
            pivot,
            facing,
            up: Vec3::Y,
            walk: Walk::STILL,
            last_ground: None,
            catch: FallCatch::default(),
            footing: Footing {
                supported: false,
                height: None,
                slope: Vec3::Y,
                ground: None,
            },
            velocity: Vec3::ZERO,
            solved: Vec3::ZERO,
            held_off: false,
            probe_held: false,
            gait_speed: 0.0,
            hung: false,
            stalled: false,
            steps: [NOMINAL_STEP; 2],
            solid,
            tuning,
        }
    }

    /// An actor moved by the engine alone (a creature, an NPC: openspec
    /// AH.18) at rest with its pivot at `pivot`, upright, facing `facing`:
    /// its walk is its scripts' ([`Hero::step_engine`]) or the engine's own
    /// steering ([`Hero::step_steered`]), so it has no tuning of a hero's.
    pub fn actor(solid: HavokSolid, pivot: Vec3, facing: f32) -> Hero {
        Hero::new(solid, Tuning::NONE, pivot, facing)
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    pub fn solid(&self) -> &HavokSolid {
        &self.solid
    }

    /// His solid rebuilt (his scripts set a new scale, openspec AG.27): the
    /// pivot stays, the shape grows from it.
    pub fn set_solid(&mut self, solid: HavokSolid) {
        self.solid = solid;
    }

    /// The walk AG.16's controller takes from now on: what a hero whose
    /// scripts stopped walks on, the numbers they had set (AG.27).
    pub fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning = tuning;
    }

    /// The steps one and two frames before the next, for a run that starts
    /// partway into a recording (a replay feeds the recording's own).
    pub fn set_steps(&mut self, before: f32, two_before: f32) {
        self.steps = [before, two_before];
    }

    /// For drawing him: his facing is a heading on the ground, so it turns
    /// him before the lean tips him.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_arc(Vec3::Y, self.up) * Quat::from_rotation_y(self.facing.to_radians())
    }

    fn proxy<'a>(
        &self,
        seam: &'a dyn PhysicsSeam,
        exclude: &'a [BodyId],
        surfaces: &'a Surfaces,
    ) -> Proxy<'a> {
        Proxy {
            seam,
            shape: self.solid.grown(0.0),
            centre: self.solid.centre,
            rotation: self.solid.rotation,
            exclude,
            surfaces,
        }
    }

    /// Sets him down from where he stands onto what's under him, his shape
    /// [`KEEP`] off it (AF.9's idle pose rides 5 mm over the ground), and reads
    /// his footing there.
    pub fn settle(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId]) {
        let proxy = self.proxy(seam, exclude, Surfaces::NONE);
        let from = self.pivot + Vec3::Y;
        if let Some(hit) = seam.cast(
            &proxy.shape,
            proxy.pose(from, self.up),
            Vec3::NEG_Y * 10.0,
            0.0,
            exclude,
        ) {
            let facing = hit.planes.iter().map(|p| p.normal.y).fold(0.0f32, f32::max);
            let back = if facing > 1e-3 { KEEP / facing } else { 0.0 };
            self.pivot = from + Vec3::NEG_Y * (hit.distance - back).max(0.0);
        }
        self.look(seam, exclude, Surfaces::NONE, true, false, NOMINAL_STEP);
    }

    /// Reads his footing where he stands, moving him by no cast: a hero put
    /// down where another stood (AG.29's replacement), whose spot a cast from
    /// above, as [`Self::settle`]'s, could meet an overhang or a slope beside
    /// it first and set him on that.
    pub fn stand(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId]) {
        self.look(seam, exclude, Surfaces::NONE, true, false, NOMINAL_STEP);
    }

    /// Takes him out of any wall he's set down in, before his first step
    /// (openspec AG.289): a hero put where the last stood, or whose scripts
    /// grew his solid, can start inside a wall the smaller one only touched
    /// (Warnado's capsule came in 0.15 m into the start's boundary wall and
    /// stayed stuck 26 s), and the step's recovery doesn't walk him out of a
    /// corner. In no wall, he's left as he was, footing and all (a solid
    /// can change mid-air). Each pass moves him across the ground off every
    /// wall he's in (a plane steeper than he can stand on, [`KEEP`] off it),
    /// the floor left to [`Self::stand`]; still in one after
    /// [`PUSH_PASSES`] (two walls' pushes cancel in a gap narrower than he
    /// is), the search tries eight headings round him from his facing,
    /// [`SEARCH_OUT`] out and [`SEARCH_UP`] up, for a spot that's clear with
    /// ground within [`SEARCH_DOWN`] under it and nothing between his centre
    /// and its (never through a wall), and settles him there (the original's
    /// `Portal - Minion Entry Find Safe Location`, which its portal manager
    /// runs for a second minion coming in at once). None fitting (AS.251),
    /// the wider search's rings out to [`FAR_OUT`], each spot one a walk from
    /// his place reaches ([`Reach`]), so never past a gap, a ledge or
    /// anything solid the level holds shut; none of those fitting, `Stuck`
    /// where he was. His footing is read wherever he's moved to. Our own
    /// rule past the eight, what the original does with a hero too big for
    /// his place unread (stand-in ledger: `set-down`).
    pub fn push_out(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId]) -> PushOut {
        let from = self.pivot;
        if !self.in_a_wall(seam, exclude, from) {
            return PushOut::Clear;
        }
        for _ in 0..PUSH_PASSES {
            let push = self.wall_push(seam, exclude, self.pivot);
            if push == Vec3::ZERO {
                break;
            }
            self.pivot += push;
            if !self.in_a_wall(seam, exclude, self.pivot) {
                break;
            }
        }
        if !self.in_a_wall(seam, exclude, self.pivot) {
            let pushed = self.clone();
            self.stand(seam, exclude);
            // Reading his footing snaps him to the ground and leans him on
            // it, which on a slope beside a round prop sets him back in
            // something (AS.251's hunt: 0.1-0.8 m in): then the search.
            if !self.in_a_wall(seam, exclude, self.pivot) {
                return PushOut::Pushed(self.pivot - from);
            }
            *self = pushed;
            self.pivot = from;
        }
        // The search reads him as he stands while `self` moves.
        let probe = self.clone();
        let centre = |pivot: Vec3| {
            probe
                .proxy(seam, exclude, Surfaces::NONE)
                .pose(pivot, probe.up)
                .position
        };
        // A spot fits when his shape is clear there with walkable ground
        // under it and nothing lies between his centre and its: no
        // triangle, and no convex solid unless `through_solids` lets the
        // line pass them (a prop standing in the way he's set past).
        let fits = |spot: Vec3, through_solids: bool| -> bool {
            if probe.in_a_wall(seam, exclude, spot) || !probe.is_clear(seam, exclude, spot) {
                return false;
            }
            let (start, end) = (centre(from), centre(spot));
            let (way, mut at) = ((end - start).normalize(), 0.0f32);
            let length = (end - start).length();
            while let Some(hit) = seam.ray(start + way * at, way, length - at, exclude) {
                if hit.triangle.is_some() || !through_solids {
                    return false;
                }
                at += hit.distance + FAR_THROUGH;
                if at >= length {
                    break;
                }
            }
            ground::find(seam, spot, exclude).is_some_and(|g| {
                (0.0..=SEARCH_DOWN).contains(&(spot.y - g.point.y)) && g.normal.y > MAX_SLOPE_COS
            })
        };
        for k in 0..SEARCH_HEADINGS {
            let heading = (self.facing + k as f32 * 360.0 / SEARCH_HEADINGS as f32).to_radians();
            let out = Vec3::new(heading.sin(), 0.0, heading.cos()) * SEARCH_OUT;
            let spot = from + out + Vec3::Y * SEARCH_UP;
            if fits(spot, false) && self.settled_at(seam, exclude, spot) {
                return PushOut::Searched(self.pivot - from);
            }
        }
        // Where a walk from his place goes, read once the eight have failed.
        let reach = Reach::from(seam, exclude, from);
        for through_solids in [false, true] {
            for ring in 1..=(FAR_OUT / FAR_STEP) as usize {
                let out = ring as f32 * FAR_STEP;
                let lift = SEARCH_UP + FAR_SLOPE * out;
                for k in 0..FAR_HEADINGS {
                    let heading =
                        (self.facing + k as f32 * 360.0 / FAR_HEADINGS as f32).to_radians();
                    let over =
                        from + Vec3::new(heading.sin(), 0.0, heading.cos()) * out + Vec3::Y * lift;
                    // The ground under a spot lifted to clear a slope, the
                    // spot then set a lift over that.
                    let Some(ground) = ground::find(seam, over, exclude) else {
                        continue;
                    };
                    if ground.point.y > over.y
                        || ground.point.y < from.y - SEARCH_DOWN - FAR_SLOPE * out
                    {
                        continue;
                    }
                    if !reach.reaches(seam, exclude, ground.point) {
                        continue;
                    }
                    let spot = Vec3::new(over.x, ground.point.y + SEARCH_UP, over.z);
                    if !fits(spot, through_solids) {
                        continue;
                    }
                    if self.settled_at(seam, exclude, spot) {
                        return PushOut::Searched(self.pivot - from);
                    }
                }
            }
        }
        self.pivot = from;
        self.stand(seam, exclude);
        PushOut::Stuck
    }

    /// Sets him down at `spot` and settles him on the ground there, when
    /// that leaves him supported and in no wall: a settle's cast starts a
    /// metre up and can meet an overhang's edge on the way down, or end
    /// against a round prop. Otherwise he's left as he was.
    fn settled_at(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId], spot: Vec3) -> bool {
        let mut trial = self.clone();
        trial.pivot = spot;
        trial.settle(seam, exclude);
        if !trial.footing.supported || trial.in_a_wall(seam, exclude, trial.pivot) {
            return false;
        }
        *self = trial;
        true
    }

    /// What takes his shape at `pivot` off every wall it's in, across the
    /// ground: each wall's share of the move its depth (and [`KEEP`])
    /// needs along it; zero when he's in none.
    fn wall_push(&self, seam: &dyn PhysicsSeam, exclude: &[BodyId], pivot: Vec3) -> Vec3 {
        self.proxy(seam, exclude, Surfaces::NONE)
            .planes(pivot, self.up, 0.0)
            .iter()
            .filter(|p| p.distance < 0.0 && p.normal.y < MAX_SLOPE_COS)
            .fold(Vec3::ZERO, |push, p| {
                let across = Vec3::new(p.normal.x, 0.0, p.normal.z);
                let share = across.length_squared();
                if share < 1e-4 {
                    return push;
                }
                push + across * (-p.distance / share)
            })
    }

    /// Whether his shape at `pivot` is in a wall [`Self::wall_push`] moves
    /// him off, past [`IN_A_WALL`]: asked of the walls themselves, since two
    /// facing each other push him by nothing between them.
    fn in_a_wall(&self, seam: &dyn PhysicsSeam, exclude: &[BodyId], pivot: Vec3) -> bool {
        self.proxy(seam, exclude, Surfaces::NONE)
            .planes(pivot, self.up, 0.0)
            .iter()
            .any(|p| {
                let across = p.normal.x * p.normal.x + p.normal.z * p.normal.z;
                p.normal.y < MAX_SLOPE_COS
                    && if across >= 1e-4 {
                        p.distance < -IN_A_WALL
                    } else {
                        // A flat ceiling moves him by no push across the
                        // ground: only the search can take him from it.
                        p.normal.y < 0.0 && p.distance < -IN_A_CEILING
                    }
            })
    }

    /// The nearest wall's gap (a plane steeper than he can stand on) within
    /// `reach` past his skin, [`KEEP`] off it reading 0 and negative when
    /// he's in it; none with no wall that near.
    pub fn wall_gap(&self, seam: &dyn PhysicsSeam, exclude: &[BodyId], reach: f32) -> Option<f32> {
        self.proxy(seam, exclude, Surfaces::NONE)
            .planes(self.pivot, self.up, reach)
            .iter()
            .filter(|p| p.normal.y < MAX_SLOPE_COS)
            .map(|p| p.distance)
            .min_by(f32::total_cmp)
    }

    /// Whether his shape at `pivot` is in nothing at all, floors and
    /// ceilings included.
    fn is_clear(&self, seam: &dyn PhysicsSeam, exclude: &[BodyId], pivot: Vec3) -> bool {
        self.proxy(seam, exclude, Surfaces::NONE)
            .planes(pivot, self.up, 0.0)
            .iter()
            .all(|p| p.distance >= 0.0)
    }

    /// One game frame of `step` seconds in a world where nothing moves.
    pub fn step(&mut self, seam: &dyn PhysicsSeam, input: &Input, step: f32, exclude: &[BodyId]) {
        self.step_among(seam, input, step, exclude, Surfaces::NONE);
    }

    /// One game frame of `step` seconds, `surfaces` holding how fast each
    /// moving solid went over its last move. Move a kinematic body after the
    /// hero's step, not before: then he meets it where it stood, as Avian's
    /// queries see it until its physics schedule runs.
    pub fn step_among(
        &mut self,
        seam: &dyn PhysicsSeam,
        input: &Input,
        step: f32,
        exclude: &[BodyId],
        surfaces: &Surfaces,
    ) {
        let tuning = self.tuning;
        let push = input.stick.push(input.view_heading);
        let [before, two_before] = self.steps;

        let facing = turn_facing(self.facing, push, tuning.turn_rate, two_before);
        // His lean is his transform's, so turning him about the vertical
        // turns it with him (AG.71: 25 records at a swing in the air).
        self.up = Quat::from_rotation_y((facing - self.facing).to_radians()) * self.up;
        self.facing = facing;
        if !self.held_off {
            self.walk
                .face_and_move(push, &self.footing, &tuning, two_before);
        }
        self.probe_held = false;
        if let Some(point) = edge::point(self.pivot, self.walk.velocity, push, &tuning) {
            if edge::walkable(seam, self.pivot, point, exclude) {
                self.held_off = false;
            } else {
                self.walk.velocity.x = 0.0;
                self.walk.velocity.z = 0.0;
                self.held_off = true;
                self.probe_held = true;
            }
        }
        self.gait_speed = self.walk.velocity.length();
        self.walk
            .update_movers(push.held(), &self.footing, &tuning, before);
        self.engine(seam, step, exclude, surfaces);
        self.steps = [step, before];
    }

    /// One game frame of the engine alone, for a hero whose walk Giants' own
    /// scripts keep on the VM (openspec AH.17, AH design decision 5: one
    /// writer): [`Hero::walk`]'s velocity is what the movement script last
    /// handed the engine (its `actorinfo.velocity`, ft/s) and his facing is
    /// the script's to set. Nothing here turns him, pushes or slows the walk
    /// or probes an edge; the walk is laid along the ground while he's on it,
    /// moved through the cast-and-slide over this frame's `step`, then the
    /// support check, the ground finder, the snap and the lean.
    pub fn step_engine(
        &mut self,
        seam: &dyn PhysicsSeam,
        step: f32,
        exclude: &[BodyId],
        surfaces: &Surfaces,
    ) {
        let [before, _] = self.steps;
        self.steps = [step, before];
        self.probe_held = false;
        if self.hung {
            self.gait_speed = 0.0;
            self.velocity = Vec3::ZERO;
            return;
        }
        self.gait_speed = self.walk.velocity.length();
        self.engine(seam, step, exclude, surfaces);
    }

    /// One game frame of an actor no stick drives (openspec AH.18, AH design
    /// decision 4): its walk and facing from the engine's own steering
    /// ([`crate::steer::step`]) with its movement numbers `physics` fed
    /// `aim`, then the engine as [`Hero::step_engine`] has it. The hero's own
    /// frame ([`Hero::step`]) never comes here.
    ///
    /// With a destination (`move to`, `move from`) the walk it keeps is the
    /// move's as solved against what it met ([`Hero::solved`]), as Havok's
    /// proxy hands it back (openspec AH.232): pressed head-on into another
    /// actor's body it keeps only the slide along it, which, more than 45°
    /// off its facing, the next frame doesn't push, so it stands rather than
    /// walking on the spot (AH.10's `still_p1`: a Chompy 0.1° off the line
    /// through another's middle at 0.00 m/s, its velocity 90° off that line,
    /// for 120 frames). A walk its scripts set with no destination (a
    /// lunge) is theirs and stays as set, bar a fall's landing: a frame that
    /// ends on the ground begun off it, pressing into it past
    /// [`HARD_LANDING`], loses what pressed in, as the proxy's solve takes
    /// it (openspec AS.97: kept, the next frame's walk along the ground
    /// turned a 12 m fall's 83 ft/s into a skid across Setup_004B's yard).
    /// Any landing under it stays as the walk turns it, a knockback's dips
    /// among them (AH.10's run holds a Chompy's slide's speeds).
    pub fn step_steered(
        &mut self,
        seam: &dyn PhysicsSeam,
        physics: &steer::Physics,
        aim: steer::Aim,
        step: f32,
        exclude: &[BodyId],
        surfaces: &Surfaces,
    ) {
        let frame = steer::step(
            steer::Frame {
                velocity: self.walk.velocity * FOOT,
                facing: self.facing,
                airborne: false,
            },
            physics,
            aim,
            self.pivot,
            self.footing.ground,
            self.footing.supported,
            step,
        );
        self.walk.velocity = frame.velocity / FOOT;
        self.facing = frame.facing;
        let off_the_ground = !self.footing.supported;
        self.step_engine(seam, step, exclude, surfaces);
        if matches!(
            aim,
            steer::Aim::To(_) | steer::Aim::From(_) | steer::Aim::Along { .. }
        ) {
            self.walk.velocity = self.solved / FOOT;
        } else if off_the_ground && self.footing.supported {
            let n = self.footing.ground.unwrap_or(Vec3::Y);
            let into = self.walk.velocity.dot(n);
            if into < -HARD_LANDING {
                self.walk.velocity -= n * into;
            }
        }
    }

    /// The engine's part of a frame: the walk moved and looked after.
    fn engine(
        &mut self,
        seam: &dyn PhysicsSeam,
        step: f32,
        exclude: &[BodyId],
        surfaces: &Surfaces,
    ) {
        let v = self.walk.velocity * FOOT;
        let n = self.footing.ground.unwrap_or(Vec3::Y);
        let walking = self.footing.supported && n.y >= LEAVE_NORMAL && n.dot(v) <= LEAVE_SPEED;
        let airborne = self.footing.supported && !walking && n.dot(v) > AIRBORNE_SHARE * v.length();
        let v = if walking { proxy::lay_along(v, n) } else { v };
        let start = self.pivot;
        let moved = self.proxy(seam, exclude, surfaces).integrate(
            self.pivot,
            self.up,
            v,
            step,
            self.footing.supported,
            self.stalled,
        );
        self.pivot = moved.pivot;
        self.stalled = !self.footing.supported
            && v != Vec3::ZERO
            && self.pivot.distance(start) < proxy::TOUCHING;
        self.velocity = (self.pivot - start) / step;
        self.solved = moved.velocity;
        self.look(seam, exclude, surfaces, walking, airborne, step);
    }

    /// How far he's fallen below the last ground under him, metres, with
    /// none under him now; `None` with ground under him. What the minion
    /// script's fall check reads as his separation from the ground: past its
    /// 225 ft it sets him back on his safe point trail (openspec AS.146; a
    /// reading: what the original's ground reads with none under him isn't
    /// recorded).
    /// Stand-in ledger: `fall-reset`.
    pub fn fallen(&self) -> Option<f32> {
        match self.footing.ground {
            Some(_) if !self.ground_over_him() => None,
            _ => Some((self.last_ground.unwrap_or(self.pivot.y) - self.pivot.y).max(0.0)),
        }
    }

    /// What the ground finder holds is its upward ray's, over his head with
    /// nothing under him (openspec AS.159: Tree Rex slipped off the pit's
    /// edge read a gap of 0 under it, 68 m below the level's underside, and
    /// hung there with no gravity while the stick stayed on the way back).
    fn ground_over_him(&self) -> bool {
        self.footing.height.is_some_and(|h| h < OVER_HIS_HEAD)
    }

    /// Held up by a rim alone (openspec AS.155): the support check passes on
    /// a plane at his shape, as a proxy's does up to its 45° (ag-motion section
    /// 1, as read), with nothing to stand on under his pivot within the
    /// walkable check's 225 ft. No script's walk leaves him there: its check
    /// looks past him for ground and, from under a rim's height, reads the
    /// rim as a wall to stand off, out over the drop, so it holds his stick
    /// from every heading. So his gravity acts ([`Self::fallen`]'s reading,
    /// his scripts' separation), the solve slides him off the rim's plane
    /// and he falls, the minion's own check setting him back (a reading: no
    /// recording has a hero at rest on one).
    pub fn on_a_lip(&self) -> bool {
        self.footing.supported
            && (self.ground_over_him()
                || self.footing.height.is_none_or(|h| h > edge::WALKABLE_RANGE))
    }

    /// The catch under the minion's own fall check (openspec AS.146): falling
    /// past [`FALL_DEPTH`] with no ground under him, he's set back where he
    /// stood on the ground, walk gone. Only a hero no script ends a fall for
    /// gets this far (one fallen back on the controller). True when it set
    /// him back. A hung hero's place is his rig's, and one not falling is
    /// where a scene or a hook holds him, however deep.
    /// Stand-in ledger: `fall-reset`.
    pub fn catch_fall(&mut self) -> bool {
        if self.hung {
            return false;
        }
        let c = &mut self.catch;
        if self.footing.ground.is_some() {
            if self.footing.supported {
                c.ground_steps += 1;
                if c.next.is_none() || c.ground_steps >= FALL_SAFE_STEPS {
                    c.safe = c.next.or(Some(self.pivot));
                    c.next = Some(self.pivot);
                    c.ground_steps = 0;
                }
            }
            return false;
        }
        let deep = self.walk.velocity.y < 0.0
            && self
                .last_ground
                .is_some_and(|y| y - self.pivot.y > FALL_DEPTH);
        let Some(safe) = c.safe.filter(|_| deep) else {
            return false;
        };
        self.pivot = safe;
        self.walk.velocity = Vec3::ZERO;
        self.up = Vec3::Y;
        true
    }

    /// The engine's look after a move: the support check, the ground finder,
    /// the snap and the lean, into his footing. A frame whose walk left the
    /// ground (the velocity model's leave test and the post-step's airborne
    /// test both) ends without support, as the corpus's airborne flag marks
    /// only frames without it (AG.38).
    fn look(
        &mut self,
        seam: &dyn PhysicsSeam,
        exclude: &[BodyId],
        surfaces: &Surfaces,
        walking: bool,
        airborne: bool,
        step: f32,
    ) {
        let planes = self
            .proxy(seam, exclude, surfaces)
            .planes(self.pivot, self.up, SUPPORT_GAP);
        let checked = ground::support(&planes).supported && !airborne;
        let found = ground::find(seam, self.pivot, exclude);
        let mut height = found.map(|g| self.pivot.y - g.point.y);
        // Within the snap's reach over ground he can stand on, the post-step
        // counts him on the ground: his lean turns to it, and a frame begun
        // on the ground (the walk not leaving it) ends supported.
        let near = matches!((found, height),
            (Some(g), Some(h)) if h < SNAP_REACH && g.normal.y > MAX_SLOPE_COS);
        let supported = checked || (near && walking);
        if let Some(g) = found {
            self.last_ground = Some(g.point.y);
        }
        let target = match found {
            Some(g) if checked || near => g.normal,
            _ => Vec3::Y,
        };
        let turned = ground::lean(self.up, target, step);
        if let Some(up) = turned {
            self.up = up;
        }
        if let (true, Some(g), Some(h)) = (supported, found, height) {
            // The snap from over the ground runs only while his lean turns;
            // from under it, always.
            let snapped = if turned.is_some() {
                ground::snap(h)
            } else {
                (h < 0.0).then_some(SNAP_HEIGHT)
            };
            if let Some(to) = snapped {
                let proxy = self.proxy(seam, exclude, surfaces);
                let mut y = g.point.y + to;
                let inside = proxy.clearance(
                    Vec3::new(self.pivot.x, y, self.pivot.z),
                    self.up,
                    g.point,
                    g.normal,
                );
                // No further than the snap reaches: off a face all but upright
                // (a body beside him met by the finder's ray) the lift out
                // runs to tens of metres, and a spawner a hero stood in went
                // 73 m up and out of its activation range for good, its fight
                // never ending (the au-whole-story review).
                if inside < 0.0 && g.normal.y > 1e-3 {
                    y -= (inside / g.normal.y).max(-SNAP_REACH);
                }
                // Set down, never above where the move left him: past a
                // convex crest the face's plane runs on under his shape
                // where only the crest is solid (it lifted him 0.34 m
                // over a 50° bank's top, metres at the ledge route's edge).
                // On a plane the move already left him at least this high.
                if h > 0.0 {
                    y = y.min(self.pivot.y);
                }
                self.pivot.y = y;
                height = Some(y - g.point.y);
            }
        }
        let settled = found.is_some_and(|g| ground::angle(self.up, g.normal) <= LEAN_SETTLED);
        self.footing = Footing {
            supported,
            height,
            slope: if settled {
                self.up
            } else {
                found.map_or(Vec3::Y, |g| g.normal)
            },
            ground: found.map(|g| g.normal),
        };
    }
}

#[cfg(test)]
mod catch {
    use super::*;
    use crate::parry::ParryWorld;
    use crate::seam::{Body, BodyKind, Collider};
    use crate::tuning::{Tuning, eruptor_solid};

    fn floor() -> ParryWorld {
        ParryWorld::new(&[Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            collider: Collider::Convex(crate::seam::Shape::Box {
                half_extents: Vec3::new(5.0, 0.5, 5.0),
                margin: 0.0,
            }),
            pose: crate::seam::Pose::new(Vec3::new(0.0, -0.5, 0.0), Quat::IDENTITY),
        }])
        .unwrap()
    }

    fn walked(world: &ParryWorld, steps: u32) -> Hero {
        let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        h.settle(world, &[]);
        for _ in 0..steps {
            h.step_engine(world, 1.0 / 60.0, &[], Surfaces::NONE);
            assert!(!h.catch_fall());
        }
        h
    }

    /// AS.146: a fall past the depth ends where he stood a while ago, his
    /// walk gone; short of it, it goes on.
    #[test]
    fn a_long_fall_ends_where_he_stood() {
        let world = floor();
        let mut h = walked(&world, 90);
        let stood = h.pivot;
        h.last_ground = Some(0.0);
        h.footing.ground = None;
        h.walk.velocity = Vec3::new(1.0, -120.0, 0.0);
        h.pivot = Vec3::new(9.0, -FALL_DEPTH + 0.1, 0.0);
        assert!(!h.catch_fall(), "short of the depth");
        h.pivot = Vec3::new(9.0, -FALL_DEPTH - 0.1, 0.0);
        assert!(h.catch_fall());
        assert_eq!((h.pivot, h.walk.velocity), (stood, Vec3::ZERO));
    }

    /// AS.146: where a scene or a hook holds him he isn't falling, however
    /// far under the last ground, and a hung hero's place is his rig's: the
    /// outro of level 214 stood him over nothing and the first catch took
    /// him back across it.
    #[test]
    fn one_held_over_nothing_is_left_there() {
        let world = floor();
        let mut h = walked(&world, 90);
        let at = Vec3::new(9.0, -FALL_DEPTH - 1.0, 0.0);
        h.last_ground = Some(0.0);
        h.footing.ground = None;
        h.pivot = at;
        h.walk.velocity = Vec3::ZERO;
        for _ in 0..300 {
            assert!(!h.catch_fall());
        }
        assert_eq!(h.pivot, at);
        h.walk.velocity = Vec3::new(0.0, -120.0, 0.0);
        h.hung = true;
        assert!(!h.catch_fall());
        assert_eq!(h.pivot, at);
    }

    /// AS.146: where he's set back is behind him, not the edge he left.
    #[test]
    fn the_safe_place_lags_him_on_the_ground() {
        let world = floor();
        let mut h = walked(&world, 2);
        let first = h.pivot;
        for k in 0..FALL_SAFE_STEPS {
            h.pivot.x = k as f32 * 0.01;
            h.catch_fall();
        }
        let moved_to = h.catch.next.unwrap();
        assert_ne!(moved_to, first);
        assert_eq!(h.catch.safe, Some(first), "a step behind");
    }

    /// AS.146: his fall is measured from the last ground, none under him.
    #[test]
    fn a_fall_is_measured_from_the_last_ground() {
        let world = floor();
        let mut h = walked(&world, 30);
        assert_eq!(h.fallen(), None);
        let top = h.last_ground.unwrap();
        h.pivot = Vec3::new(50.0, top - 3.0, 0.0);
        h.step_engine(&world, 1.0 / 60.0, &[], Surfaces::NONE);
        assert!(!h.footing.supported);
        assert!(h.fallen().unwrap() > 2.9);
    }

    /// The au-whole-story review: the ground finder meeting a body at its
    /// side, a face all but upright over his pivot, sets him down off it no
    /// further than the snap reaches: on level 214's third island it lifted
    /// a Chompy spawner 73 m in a step.
    #[test]
    fn a_snap_off_an_upright_face_goes_no_further_than_it_reaches() {
        let world = ParryWorld::new(&[
            Body {
                id: BodyId(1),
                kind: BodyKind::Static,
                collider: Collider::Convex(crate::seam::Shape::Box {
                    half_extents: Vec3::new(5.0, 0.5, 5.0),
                    margin: 0.0,
                }),
                pose: crate::seam::Pose::new(Vec3::new(0.0, 0.01, 0.0), Quat::IDENTITY),
            },
            Body {
                id: BodyId(2),
                kind: BodyKind::Kinematic,
                collider: Collider::Convex(crate::seam::Shape::Sphere { radius: 1.0 }),
                pose: crate::seam::Pose::at(Vec3::new(0.999, 0.5, 0.0)),
            },
        ])
        .unwrap();
        let at = Vec3::new(0.0, 0.52, 0.0);
        let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, 0.0);
        h.stand(&world, &[]);
        let ground = h.footing.ground.expect("the body's side");
        assert!(ground.y < 0.1, "all but upright: {ground}");
        assert!(h.footing.supported, "the floor's plane holds him");
        assert!(
            h.pivot.y - at.y <= crate::ground::SNAP_REACH + 0.05,
            "lifted to {}",
            h.pivot
        );
    }

    /// AS.146's review: over nothing with his shape on the lip the support
    /// check holds him, as on any ledge, a floor under his pivot or none:
    /// it's the planes at his shape, the ground finder its own. Eruptor and a
    /// giant's twice his size, on a floor's edge and on a beam narrower than
    /// either, walked to its side. AS.155: that's the engine's half, a walk
    /// that stops once he's supported; his scripts' gravity doesn't, and
    /// [`lip_gravity_slides_him_off_the_rim`] takes him off.
    #[test]
    fn a_lip_with_nothing_under_his_pivot_holds_him() {
        let beam = ParryWorld::new(&[Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            collider: Collider::Convex(crate::seam::Shape::Box {
                half_extents: Vec3::new(0.2, 0.5, 5.0),
                margin: 0.0,
            }),
            pose: crate::seam::Pose::new(Vec3::new(0.0, -0.5, 0.0), Quat::IDENTITY),
        }])
        .unwrap();
        let floor = floor();
        for (solid, what) in [
            (eruptor_solid(), "Eruptor"),
            (
                crate::tuning::ERUPTOR_SHAPE
                    .havok([crate::tuning::ERUPTOR_SCALE * 200.0; 3])
                    .unwrap(),
                "a giant",
            ),
        ] {
            let r = match solid.shape {
                crate::seam::Shape::Capsule { radius, .. }
                | crate::seam::Shape::Sphere { radius } => radius,
                s => panic!("{s:?}"),
            };
            for (world, edge) in [(&floor, 5.0), (&beam, 0.2)] {
                let mut over = Hero::new(solid, Tuning::ERUPTOR, Vec3::ZERO, 0.0);
                over.settle(world, &[]);
                over.pivot.x = edge + r * 0.3;
                // Gravity brings him onto the lip and sleeps there.
                let mut settled = 0;
                for _ in 0..60 {
                    over.walk.velocity = match over.footing.supported {
                        true => Vec3::ZERO,
                        false => Vec3::new(0.0, -1.667, 0.0),
                    };
                    over.step_engine(world, 1.0 / 60.0, &[], Surfaces::NONE);
                    settled = if over.footing.supported {
                        settled + 1
                    } else {
                        0
                    };
                }
                assert_eq!(over.footing.ground, None, "{what}: nothing under his pivot");
                assert!(settled >= 30, "{what} on the lip at {edge}: {}", over.pivot);
                assert!(over.on_a_lip(), "{what} on the lip at {edge}");
            }
        }
    }

    /// AS.155: his scripts' gravity on a lip (136.21 ft/s², the mover
    /// update's, kept on while he's [`Hero::on_a_lip`]) presses him into the
    /// rim's plane, the solve slides him down and out along it, and past 45°
    /// the rim lets him go: he's over the drop and falling, from anywhere a
    /// rim can hold him.
    #[test]
    fn lip_gravity_slides_him_off_the_rim() {
        let world = floor();
        for past in [0.05f32, 0.2, 0.4, 0.55] {
            let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
            h.settle(&world, &[]);
            h.pivot.x = 5.0 + past;
            h.walk.velocity = Vec3::new(0.0, -1.667, 0.0);
            let mut steps = 0;
            for _ in 0..180 {
                h.step_engine(&world, 1.0 / 60.0, &[], Surfaces::NONE);
                if h.on_a_lip() || !h.footing.supported {
                    h.walk.velocity.y -= 136.21 / 60.0;
                }
                steps += 1;
                if h.pivot.y < -2.0 {
                    break;
                }
            }
            assert!(
                h.pivot.y < -2.0,
                "{past}: held at {} after {steps}",
                h.pivot
            );
            assert!(h.pivot.x > 5.0, "{past}: out over the drop: {}", h.pivot);
            assert!(!h.footing.supported, "{past}: falling");
        }
    }

    /// A seam with one plane, facing +x, that stops a cast going at it at
    /// once under `below` and that the contact query never holds: AG.61's
    /// twin as a falling hero meets it.
    struct Snag {
        below: f32,
    }

    impl PhysicsSeam for Snag {
        fn cast(
            &self,
            _: &crate::seam::Shape,
            from: crate::seam::Pose,
            motion: Vec3,
            _: f32,
            _: &[BodyId],
        ) -> Option<crate::seam::Cast> {
            (from.position.y < self.below && motion.x < -1e-6).then(|| crate::seam::Cast {
                fraction: 0.0,
                distance: 0.0,
                planes: vec![crate::seam::ContactPlane {
                    body: BodyId(1),
                    triangle: Some(1),
                    material: Some(0),
                    normal: Vec3::X,
                    point: from.position,
                    distance: 0.0,
                }],
            })
        }

        fn contacts(
            &self,
            _: &crate::seam::Shape,
            _: crate::seam::Pose,
            _: f32,
            _: &[BodyId],
        ) -> Vec<crate::seam::ContactPlane> {
            Vec::new()
        }

        fn ray(&self, _: Vec3, _: Vec3, _: f32, _: &[BodyId]) -> Option<crate::seam::RayHit> {
            None
        }

        fn overlaps(
            &self,
            _: &crate::seam::Shape,
            _: crate::seam::Pose,
            _: &[BodyId],
        ) -> Vec<BodyId> {
            Vec::new()
        }
    }

    /// AS.159: in the air a plane the contact query doesn't hold stops him
    /// for the step he meets it, as AG.16's replays have it, however fast
    /// he came, and the next he goes on along it: the stop is his one step's,
    /// where it used to be every step's.
    #[test]
    fn a_falling_hero_stops_on_a_twin_plane_for_a_step_alone() {
        let solid = eruptor_solid();
        let start = Vec3::new(0.0, 1.0, 0.0);
        let seam = Snag {
            below: start.y + solid.centre.y - 0.01,
        };
        let mut h = Hero::new(solid, Tuning::ERUPTOR, start, 0.0);
        h.walk.velocity = Vec3::new(-10.0, -10.0, 0.0);
        let step = 1.0 / 60.0;
        h.step_engine(&seam, step, &[], Surfaces::NONE);
        let met = h.pivot;
        assert!(met.y < start.y && met.x < start.x, "free to it: {met}");
        h.step_engine(&seam, step, &[], Surfaces::NONE);
        assert_eq!(h.pivot, met, "his one step's stop");
        h.step_engine(&seam, step, &[], Surfaces::NONE);
        assert!(h.pivot.y < met.y, "on down it: {}", h.pivot);
        assert!(h.pivot.x >= met.x, "none of it into the plane: {}", h.pivot);
    }

    /// AS.159: ground only over his head isn't ground under him: he's
    /// falling, and a rim holding him there is a lip. The down ray's own
    /// reach above his pivot still reads as ground under him.
    #[test]
    fn ground_over_his_head_is_none_under_him() {
        let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        h.footing.ground = Some(Vec3::Y);
        h.last_ground = Some(49.0);
        h.pivot.y = -19.0;
        h.footing.height = Some(-68.0);
        assert_eq!(h.fallen(), Some(68.0));
        assert!(!h.on_a_lip(), "not held by anything");
        h.footing.supported = true;
        assert!(h.on_a_lip());
        h.footing.supported = false;
        h.footing.height = Some(-0.1);
        assert_eq!(h.fallen(), None);
        h.footing.supported = true;
        assert!(!h.on_a_lip());
    }

    /// AS.155: a hero with ground under his pivot, however far, within the
    /// walkable check's reach isn't on a lip: the walk's own check finds it,
    /// and he walks off. Past 225 ft, or none, he is; off the ground he isn't.
    #[test]
    fn a_lip_is_support_with_nothing_walkable_under_him() {
        let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        for (supported, height, lip) in [
            (true, None, true),
            (true, Some(0.005), false),
            (true, Some(edge::WALKABLE_RANGE - 0.1), false),
            (true, Some(edge::WALKABLE_RANGE + 0.1), true),
            (false, None, false),
            (false, Some(edge::WALKABLE_RANGE + 0.1), false),
        ] {
            h.footing.supported = supported;
            h.footing.height = height;
            assert_eq!(h.on_a_lip(), lip, "{supported} {height:?}");
        }
    }
}

#[cfg(test)]
mod hung {
    use super::*;
    use crate::parry::ParryWorld;
    use crate::tuning::eruptor_solid;

    /// openspec AS.115: hung, his controller takes no step: his place holds
    /// whatever walk he's left with, and let go it moves him again.
    #[test]
    fn a_hung_hero_takes_no_step_until_hes_let_go() {
        let seam = ParryWorld::new(&[]).unwrap();
        let pivot = Vec3::new(1.0, 2.0, 3.0);
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0);
        hero.walk.velocity = Vec3::X * 20.0;
        hero.hung = true;
        hero.step_engine(&seam, 1.0 / 60.0, &[], Surfaces::NONE);
        assert_eq!(hero.pivot, pivot);
        assert_eq!(hero.velocity, Vec3::ZERO);
        assert_eq!(hero.gait_speed, 0.0);
        hero.hung = false;
        hero.step_engine(&seam, 1.0 / 60.0, &[], Surfaces::NONE);
        assert_ne!(hero.pivot, pivot);
    }
}
