//! The hero controller (openspec AG.16, design decisions 4 and 5): one game
//! frame of a hero, in the original's order, on any [`PhysicsSeam`].
//!
//! Each [`Hero::step`] runs, as the movement script and the engine do:
//! 1. Face and Move ([`crate::walk`]): with the stick held his facing turns
//!    toward it, and unless the edge probe holds his controller off, the walk
//!    turns and is pushed;
//! 2. the edge probe ([`crate::edge`]), which can zero the walk's across part
//!    before he moves and hold the controller off;
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
//! Hypotheses here: the engine walks him along the ground (and the snap holds
//! him to it from up to 0.375 m) only while the support check passed, the
//! ground faces up past 0.69 and the walk isn't leaving it faster than 0.01
//! m/s (the velocity model as read; it's what lifts him for a frame over a
//! crest and off a slope he's let go on uphill); the edge probe's hold turns
//! his facing to the stick at his usual rate; his collision shape turns with
//! his lean about his pivot; he leans only while he's on the ground (the
//! post-step's grounded test, as read), so not up in the air or on ground
//! steeper than 45°.

use crate::edge;
use crate::ground::{self, LEAN_SETTLED, MAX_SLOPE_COS, SNAP_REACH, SUPPORT_GAP};
use crate::proxy::{self, KEEP, LEAVE_NORMAL, LEAVE_SPEED, Proxy};
use crate::seam::{BodyId, PhysicsSeam, Quat, Vec3};
use crate::solid::HavokSolid;
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
            steps: [NOMINAL_STEP; 2],
            solid,
            tuning,
        }
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    pub fn solid(&self) -> &HavokSolid {
        &self.solid
    }

    /// For drawing him: his facing is a heading on the ground, so it turns
    /// him before the lean tips him.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_arc(Vec3::Y, self.up) * Quat::from_rotation_y(self.facing.to_radians())
    }

    fn proxy<'a>(&self, seam: &'a dyn PhysicsSeam, exclude: &'a [BodyId]) -> Proxy<'a> {
        Proxy {
            seam,
            shape: self.solid.grown(0.0),
            centre: self.solid.centre,
            rotation: self.solid.rotation,
            exclude,
        }
    }

    /// Sets him down from where he stands onto what's under him, his shape
    /// [`KEEP`] off it (AF.9's idle pose rides 5 mm over the ground), and reads
    /// his footing there.
    pub fn settle(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId]) {
        let proxy = self.proxy(seam, exclude);
        let from = self.pivot + Vec3::Y;
        if let Some(hit) = seam.cast(
            &proxy.shape,
            proxy.pose(from, self.up),
            Vec3::NEG_Y * 4.0,
            0.0,
            exclude,
        ) {
            let facing = hit.planes.iter().map(|p| p.normal.y).fold(0.0f32, f32::max);
            let back = if facing > 1e-3 { KEEP / facing } else { 0.0 };
            self.pivot = from + Vec3::NEG_Y * (hit.distance - back).max(0.0);
        }
        self.look(seam, exclude, true, NOMINAL_STEP);
    }

    /// One game frame of `step` seconds.
    pub fn step(&mut self, seam: &dyn PhysicsSeam, input: &Input, step: f32, exclude: &[BodyId]) {
        let tuning = self.tuning;
        let push = input.stick.push(input.view_heading);
        let [before, two_before] = self.steps;

        self.facing = turn_facing(self.facing, push, tuning.turn_rate, two_before);
        if !self.held_off {
            self.walk
                .face_and_move(push, &self.footing, &tuning, two_before);
        }
        if let Some(point) = edge::point(self.pivot, self.walk.velocity, push, &tuning) {
            if edge::walkable(seam, point, exclude) {
                self.held_off = false;
            } else {
                self.walk.velocity.x = 0.0;
                self.walk.velocity.z = 0.0;
                self.held_off = true;
            }
        }
        self.walk
            .update_movers(push.held(), &self.footing, &tuning, before);

        let v = self.walk.velocity * FOOT;
        let n = self.footing.ground.unwrap_or(Vec3::Y);
        let walking = self.footing.supported && n.y >= LEAVE_NORMAL && n.dot(v) <= LEAVE_SPEED;
        let v = if walking { proxy::lay_along(v, n) } else { v };
        let start = self.pivot;
        let moved = self
            .proxy(seam, exclude)
            .integrate(self.pivot, self.up, v, step, walking);
        self.pivot = moved.pivot;
        self.velocity = (self.pivot - start) / step;
        self.look(seam, exclude, walking, step);
        self.steps = [step, before];
    }

    /// The engine's look after a move: the support check, the ground finder,
    /// the snap and the lean, into his footing.
    fn look(&mut self, seam: &dyn PhysicsSeam, exclude: &[BodyId], walking: bool, step: f32) {
        let planes = self
            .proxy(seam, exclude)
            .planes(self.pivot, self.up, SUPPORT_GAP);
        let support = ground::support(&planes);
        let found = ground::find(seam, self.pivot, exclude);
        let mut height = found.map(|g| self.pivot.y - g.point.y);
        if let (Some(g), Some(h)) = (found, height) {
            let near = walking && h < SNAP_REACH && g.normal.y > MAX_SLOPE_COS;
            let grounded = support.supported || near;
            if grounded && let Some(to) = ground::snap(h) {
                self.pivot.y = g.point.y + to;
                height = Some(to);
            }
            if grounded {
                self.up = ground::lean(self.up, g.normal, step);
            }
        }
        let settled = found.is_some_and(|g| ground::angle(self.up, g.normal) <= LEAN_SETTLED);
        self.footing = Footing {
            supported: support.supported,
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
