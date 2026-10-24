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

use crate::edge;
use crate::ground::{self, LEAN_SETTLED, MAX_SLOPE_COS, SNAP_HEIGHT, SNAP_REACH, SUPPORT_GAP};
use crate::proxy::{self, AIRBORNE_SHARE, KEEP, LEAVE_NORMAL, LEAVE_SPEED, Proxy, Surfaces};
use crate::seam::{BodyId, PhysicsSeam, Quat, Vec3};
use crate::solid::HavokSolid;
use crate::steer;
use crate::stick::Stick;
use crate::tuning::{FOOT, NOMINAL_STEP, Tuning};
use crate::walk::{Footing, Walk, turn_facing};

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
    /// His last step's move over its step, m/s.
    pub velocity: Vec3,
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
            footing: Footing {
                supported: false,
                height: None,
                slope: Vec3::Y,
                ground: None,
            },
            velocity: Vec3::ZERO,
            held_off: false,
            probe_held: false,
            gait_speed: 0.0,
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
        self.probe_held = false;
        self.gait_speed = self.walk.velocity.length();
        self.engine(seam, step, exclude, surfaces);
        self.steps = [step, before];
    }

    /// One game frame of an actor no stick drives (openspec AH.18, AH design
    /// decision 4): its walk and facing from the engine's own steering
    /// ([`crate::steer::step`]) with its movement numbers `physics` fed
    /// `aim`, then the engine as [`Hero::step_engine`] has it. The hero's own
    /// frame ([`Hero::step`]) never comes here.
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
        self.step_engine(seam, step, exclude, surfaces);
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
        );
        self.pivot = moved.pivot;
        self.velocity = (self.pivot - start) / step;
        self.look(seam, exclude, surfaces, walking, airborne, step);
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
                if inside < 0.0 && g.normal.y > 1e-3 {
                    y -= inside / g.normal.y;
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
