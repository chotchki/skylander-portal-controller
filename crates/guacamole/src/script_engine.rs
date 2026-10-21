//! The engine Giants' scripts reach for the actors they move (openspec
//! AH.17, design decisions 4, 5 and 21): an actor's place, turn, scale,
//! velocity, movement numbers and solid as the original's members read and
//! write them, the objects its members hand back (an actor's orientation,
//! its ground, an attach node, a clip of its palette, a sequence playing,
//! a pad's button), and the [`Engine`] the host asks for what the TV's
//! entities own: a hero's controller state (guac-motion's [`Hero`]), the
//! physics seam's rays and casts, the camera's view and the pads.
//!
//! Units are the members' as the original registers them (aj-vm's live
//! member table): distances and speeds in feet, angles in degrees, scales in
//! percent; the engine keeps metres and converts at the member, as the
//! original's getters do (× 3.2808399). Headings face (sin θ, 0, cos θ)
//! (AF.9); a pitch is up from the ground's plane.

use std::collections::HashMap;

use guac_motion::hero::Hero;
use guac_motion::seam::{BodyId, PhysicsSeam, Vec3};
use guac_motion::stick::Stick;
use guac_script::{Handle, Obj, Value};

/// Feet in a metre, as the original's distance members convert (aj-vm:
/// `√(d² + FLT_MIN) × 3.2808399`, a foot's 0.3048 m inverted).
pub const FEET_PER_METRE: f32 = 1.0 / METRES_PER_FOOT;
/// A foot in metres: what a displacement's feet move.
pub const METRES_PER_FOOT: f32 = 0.3048;

/// An actor's turn as the original keeps it, degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Angles {
    pub facing: f32,
    pub tilt: f32,
    pub roll: f32,
}

/// `ActorPhysics`: an actor's movement numbers, as its `ActorParameters`
/// hold them and the scripts write them (feet, degrees, percent).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Physics {
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_max_speed: f32,
    pub air_acceleration: f32,
    pub air_deceleration: f32,
    pub air_turn_rate: f32,
    pub gravity_max_speed: f32,
    pub gravity_acceleration: f32,
    pub jump_max_speed: f32,
    pub jump_acceleration: f32,
    pub jump_initial_speed: f32,
    pub move_type: i32,
    pub tilts_with_ground: i32,
    /// Percent.
    pub time_scale: f32,
}

/// `CollisionInfo`: an actor's solid as the scripts read and write it
/// (feet, pounds, percent; the shape by its enum: none, box, capsule,
/// sphere, cylinder, mesh).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Solid {
    pub width: f32,
    pub length: f32,
    pub height: f32,
    /// Metres, as its file's `_transform` keeps it; the `local * offset`
    /// members read and write it in feet (the original's getter multiplies
    /// by 3.28084, its setter by 0.3048).
    pub offset: [f32; 3],
    pub weight: f32,
    pub rebound: f32,
    pub interacts_with_actors: i32,
    pub interacts_with_level_mesh: i32,
    pub acts_as_actor_surface: i32,
    pub shape: i32,
}

/// An actor the scripts reach: a placement of a loaded file or a clone a
/// script made. A hero's place, turn and velocity are his controller's
/// ([`Engine::hero`]) and never kept here (design decision 21: no copy).
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    /// Its object in a file; a clone has none.
    pub obj: Option<Obj>,
    /// The template a clone copies, by its file object.
    pub template: Option<Obj>,
    pub id: i32,
    /// `placement.deactivation buffer`, feet: its file's and its now.
    pub buffer_loaded: f32,
    pub buffer: f32,
    /// `placement.activation range`, feet.
    pub range: f32,
    pub waypoints: Vec<Value>,
    pub removed: bool,
    /// Metres, the level's frame.
    pub position: Vec3,
    /// Where it was placed or spawned (`abstractplacement.origin`).
    pub origin: Vec3,
    /// A reference's own point (`placementreference.remembered`), kept on
    /// the actor it names; its place until a script moves it.
    pub remembered: Option<Vec3>,
    pub angles: Angles,
    /// Percent per axis.
    pub scale: [f32; 3],
    pub tint: u32,
    /// Ft/s: `actorinfo.velocity`.
    pub velocity: Vec3,
    pub physics: Physics,
    pub solid: Solid,
    /// What it hangs from (`abstractplacement.attach node`): it follows that
    /// object's place while set.
    pub attach: Value,
    /// `actorinfo.controller`.
    pub controller: Value,
    /// Its clones, made from it as a template, in the order made.
    pub clones: Vec<Handle>,
    /// A clone's template as a value (`abstractplacement.parent`).
    pub parent: Value,
    /// Set going by a script's velocity: the engine moves it each frame
    /// under its own gravity ([`crate::scripts::ScriptWorld::advance`]).
    pub launched: bool,
    /// What its solid touched in the level over the engine's last move:
    /// each contact's point, its normal out of the surface and its
    /// triangle's material (`collisioninfo.environment contacts`).
    pub contacts: Vec<Contact>,
    /// An UNMANAGED placement of a loaded file: the engine wakes and sleeps
    /// it by the activity rule ([`crate::scripts::ScriptWorld::activity`]).
    pub managed: bool,
    /// Awake (the activity rule's ACTIVE); a clone always is.
    pub active: bool,
    /// The engine moves it as a creature walks (openspec AH.18): its
    /// controller body, steered or carried by its scripts' velocity.
    pub walker: Option<Box<Walker>>,
}

/// A creature the engine moves (openspec AH.18): guac-motion's controller
/// body under it, and what the scripts last asked of the engine's own
/// steering this tick (`move to`, `move from`, `turn to`).
#[derive(Debug, Clone, PartialEq)]
pub struct Walker {
    /// Made in the engine's first frame with a solid, again when its
    /// scripts change it.
    pub body: Option<Hero>,
    pub aim: guac_motion::steer::Aim,
    /// Which waypoint of a path it's walking to (`move to` along a path).
    pub along: usize,
}

/// A contact the engine reports: a point, the normal out of the surface
/// and the triangle's collision material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub point: Vec3,
    pub normal: Vec3,
    pub material: Option<u8>,
    /// What it touched in the seam.
    pub body: BodyId,
}

impl Actor {
    /// An actor with nothing set: at the origin, unturned, unit scale.
    pub fn blank() -> Actor {
        Actor {
            obj: None,
            template: None,
            id: 0,
            buffer_loaded: 0.0,
            buffer: 0.0,
            range: 0.0,
            waypoints: Vec::new(),
            removed: false,
            position: Vec3::ZERO,
            origin: Vec3::ZERO,
            remembered: None,
            angles: Angles::default(),
            scale: [100.0; 3],
            tint: 0x8080_80FF,
            velocity: Vec3::ZERO,
            physics: Physics {
                time_scale: 100.0,
                ..Physics::default()
            },
            solid: Solid::default(),
            attach: Value::Null,
            controller: Value::Null,
            clones: Vec::new(),
            parent: Value::Null,
            launched: false,
            contacts: Vec::new(),
            managed: false,
            active: true,
            walker: None,
        }
    }

    /// Its solid as the original builds it at its scale, if it has one
    /// that stops anything.
    pub fn havok(&self) -> Option<guac_motion::solid::HavokSolid> {
        use guac_motion::solid::{SolidInfo, SolidKind};
        let kind = match self.solid.shape {
            1 => SolidKind::Box,
            2 => SolidKind::Capsule,
            3 => SolidKind::Sphere,
            4 => SolidKind::Cylinder,
            5 => SolidKind::Mesh,
            _ => SolidKind::None,
        };
        SolidInfo {
            kind,
            offset: self.solid.offset,
            rotation: [0.0; 3],
            width: self.solid.width,
            height: self.solid.height,
            length: self.solid.length,
        }
        .havok(self.scale)
    }
}

/// The unit vector a heading and a pitch point along (degrees; a pitch up
/// from the ground's plane): (cos p sin h, sin p, cos p cos h).
pub fn direction(heading: f32, pitch: f32) -> Vec3 {
    let (sh, ch) = heading.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    Vec3::new(cp * sh, sp, cp * ch)
}

/// A vector's heading across the ground, degrees, as `atan2(x, z)` (the
/// original's `× 57.29578`); 0 with no across part.
pub fn heading_of(v: Vec3) -> f32 {
    v.x.atan2(v.z) * 57.295_78
}

/// A vector's pitch up from the ground's plane, degrees.
pub fn pitch_of(v: Vec3) -> f32 {
    let across = v.x.hypot(v.z);
    v.y.atan2(across) * 57.295_78
}

/// `system.normalized angle`'s range: a write lands in [-180, 180).
pub fn normalized(degrees: f32) -> f32 {
    let d = degrees.rem_euclid(360.0);
    if d >= 180.0 { d - 360.0 } else { d }
}

/// A host object a member hands back: an actor's facet, an attach node, a
/// clip, a sequence, a pad's part, the ray caster's points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Object {
    /// A facet of an actor: the object a member like
    /// `matrixmeasurement.orientation` or `actorinfo.movement` returns.
    Facet(Handle, Facet),
    /// An actor's attach node by its index in `actorinfo.attach nodes`.
    Node(Handle, u32),
    /// Slot `slot` of a template's animation palette.
    Clip {
        template: Obj,
        slot: u32,
    },
    /// A started sequence, by its index in the host's list.
    Sequence(u32),
    /// An actor's environment contact, by its index.
    Contact(Handle, u32),
    /// Where an actor was placed or spawned.
    Origin(Handle),
    /// The point a reference to an actor keeps.
    Remembered(Handle),
    /// A contact's normal, as a vector.
    Normal(Normal),
    /// A pad as the scripts reach it (`scriptcontroller.controller 1`,
    /// `physical controller 1`): its index, 0 for the first.
    Controller(u8),
    /// The inactive controller: no input ever.
    Inactive,
    /// A pad's button by its action number (1-4) or a named one.
    Button(u8, Button),
    Stick(u8),
    /// `system.ray cast`, its start point and its end point.
    RayCast,
    RayStart,
    RayEnd,
    /// `viewportinfo.default`: the gameplay camera.
    Viewport,
    /// `redoctaneperipheralsystemobject.portal`.
    Portal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facet {
    Orientation,
    Scale,
    Movement,
    Collision,
    Velocity,
    /// The ground under it: a contact with a type and a normal, and a
    /// point a measurement can be taken from.
    Ground,
}

/// What a `contactinfo.normal` vector is the normal of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Normal {
    /// The ground under an actor.
    Ground(Handle),
    /// An actor's environment contact, by its index.
    Contact(Handle, u32),
    /// The ray caster's contact.
    Ray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Action(u8),
    Accept,
    Cancel,
    Alt,
    Pause,
    Start,
    Select,
    DpadLeft,
    DpadRight,
    DpadUp,
    DpadDown,
}

/// The tag in a handle's top bits that marks a host object: Bevy's entity
/// bits keep their generation in the high word, which never reaches it.
pub const OBJECT_TAG: u64 = 1 << 62;

/// The host's objects, each made once and handed out under one handle, so
/// a script comparing two reads of one sees the same object.
#[derive(Debug, Clone, Default)]
pub struct Objects {
    list: Vec<Object>,
    index: HashMap<Object, u32>,
}

impl Objects {
    /// `object`'s handle, made on first use.
    pub fn handle(&mut self, object: Object) -> Handle {
        let i = *self.index.entry(object).or_insert_with(|| {
            self.list.push(object);
            (self.list.len() - 1) as u32
        });
        OBJECT_TAG | u64::from(i)
    }

    pub fn value(&mut self, object: Object) -> Value {
        Value::Entity(self.handle(object))
    }

    /// The object a handle names, if it's one of the host's.
    pub fn get(&self, h: Handle) -> Option<Object> {
        if h & OBJECT_TAG == 0 {
            return None;
        }
        self.list.get((h & !OBJECT_TAG) as usize).copied()
    }
}

/// A sequence a script started: a clip of an actor's palette (or a sound,
/// which plays nothing here), its time and its rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// The actor it plays on.
    pub actor: Handle,
    /// What was started: a clip or another object (a sound).
    pub template: Value,
    /// Its length at 100 %, seconds; 0 for what has none (a sound).
    pub length: f32,
    /// Seconds into it.
    pub time: f32,
    /// Percent.
    pub rate: f32,
    pub looping: bool,
    pub stopped: bool,
    /// Whether it has played to its end.
    pub done: bool,
}

impl Sequence {
    /// `sequence.percent remaining`: what's left of it, percent.
    pub fn percent_remaining(&self) -> f32 {
        if self.length <= 0.0 || self.done || self.stopped {
            0.0
        } else {
            100.0 * (1.0 - self.time / self.length)
        }
    }

    /// `sequence.time remaining`: seconds left at its rate.
    pub fn time_remaining(&self) -> f32 {
        if self.length <= 0.0 || self.done || self.stopped {
            return 0.0;
        }
        let rate = self.rate * 0.01;
        if rate <= 0.0 {
            return 0.0;
        }
        (self.length - self.time) / rate
    }

    /// One frame of `dt` seconds: on by its rate, done at its end unless it
    /// loops.
    pub fn advance(&mut self, dt: f32) {
        if self.done || self.stopped {
            return;
        }
        if self.length <= 0.0 {
            self.done = true;
            return;
        }
        self.time += dt * self.rate * 0.01;
        if self.time >= self.length {
            if self.looping {
                self.time = self.time.rem_euclid(self.length);
            } else {
                self.time = self.length;
                self.done = true;
            }
        }
    }
}

/// A pad as a step reads it: its stick and which buttons are down, by the
/// action each fires (pad-input's The face buttons attack: the bottom
/// position action 1, the left and both right shoulders action 2).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pad {
    pub stick: Stick,
    /// Actions 1-4 held.
    pub actions: [bool; 4],
    pub connected: bool,
}

/// What the scripts reach that the TV's entities own (design decision 21:
/// the host reads and writes them where they live): a hero's controller,
/// the seam's queries, the camera and the pads.
pub trait Engine {
    /// The hero whose actor runs under `h`, if one does.
    fn hero(&mut self, h: Handle) -> Option<&mut Hero>;
    /// Every hero's handle.
    fn heroes(&self) -> Vec<Handle>;
    /// The physics seam: the level and the props.
    fn seam(&self) -> &dyn PhysicsSeam;
    /// The heading the gameplay camera's view looked along the frame before.
    fn view_heading(&self) -> f32;
    /// Pad `n` (0 for the first) as this step reads it.
    fn pad(&self, n: usize) -> Pad;
    /// The body in the seam an actor's solid is, a creature's or a prop's;
    /// none by default.
    fn body(&self, _actor: Handle) -> Option<BodyId> {
        None
    }
    /// The actor a body in the seam is, if any: the level's own is none.
    fn actor_of(&self, _body: BodyId) -> Option<Handle> {
        None
    }
}

/// An engine with nothing in it: no hero, no solid, the view along 0°, no
/// pad (AH.16's runs, which reach none of them).
pub struct NoEngine;

struct NoSeam;

impl PhysicsSeam for NoSeam {
    fn cast(
        &self,
        _: &guac_motion::seam::Shape,
        _: guac_motion::seam::Pose,
        _: Vec3,
        _: f32,
        _: &[guac_motion::seam::BodyId],
    ) -> Option<guac_motion::seam::Cast> {
        None
    }
    fn contacts(
        &self,
        _: &guac_motion::seam::Shape,
        _: guac_motion::seam::Pose,
        _: f32,
        _: &[guac_motion::seam::BodyId],
    ) -> Vec<guac_motion::seam::ContactPlane> {
        Vec::new()
    }
    fn ray(
        &self,
        _: Vec3,
        _: Vec3,
        _: f32,
        _: &[guac_motion::seam::BodyId],
    ) -> Option<guac_motion::seam::RayHit> {
        None
    }
    fn overlaps(
        &self,
        _: &guac_motion::seam::Shape,
        _: guac_motion::seam::Pose,
        _: &[guac_motion::seam::BodyId],
    ) -> Vec<guac_motion::seam::BodyId> {
        Vec::new()
    }
}

impl Engine for NoEngine {
    fn hero(&mut self, _: Handle) -> Option<&mut Hero> {
        None
    }
    fn heroes(&self) -> Vec<Handle> {
        Vec::new()
    }
    fn seam(&self) -> &dyn PhysicsSeam {
        &NoSeam
    }
    fn view_heading(&self) -> f32 {
        0.0
    }
    fn pad(&self, _: usize) -> Pad {
        Pad::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_and_a_pitch_point_where_the_original_faces() {
        let d = direction(90.0, 0.0);
        assert!((d - Vec3::X).length() < 1e-6, "{d}");
        let d = direction(0.0, 0.0);
        assert!((d - Vec3::Z).length() < 1e-6, "{d}");
        let d = direction(0.0, 90.0);
        assert!((d - Vec3::Y).length() < 1e-6, "{d}");
        let v = direction(250.0, 63.6) * 58.6;
        assert!((normalized(heading_of(v)) - normalized(250.0)).abs() < 1e-3);
        assert!((pitch_of(v) - 63.6).abs() < 1e-3);
    }

    #[test]
    fn a_normalized_angle_lands_in_minus_180_to_180() {
        assert_eq!(normalized(190.0), -170.0);
        assert_eq!(normalized(-190.0), 170.0);
        assert_eq!(normalized(180.0), -180.0);
        assert_eq!(normalized(-180.0), -180.0);
        assert_eq!(normalized(15.0), 15.0);
        assert_eq!(normalized(375.0), 15.0);
    }

    #[test]
    fn an_object_keeps_one_handle() {
        let mut o = Objects::default();
        let a = o.handle(Object::Viewport);
        let b = o.handle(Object::RayCast);
        assert_ne!(a, b);
        assert_eq!(o.handle(Object::Viewport), a);
        assert_eq!(o.get(a), Some(Object::Viewport));
        assert_eq!(o.get(7), None);
        assert!(a & OBJECT_TAG != 0);
    }

    #[test]
    fn a_sequence_plays_at_its_rate_and_ends_unless_it_loops() {
        let mut s = Sequence {
            actor: 1,
            template: Value::Null,
            length: 0.5,
            time: 0.0,
            rate: 150.0,
            looping: false,
            stopped: false,
            done: false,
        };
        assert_eq!(s.percent_remaining(), 100.0);
        assert!((s.time_remaining() - 0.5 / 1.5).abs() < 1e-6);
        // Four frames at 1/60 s and 150 %: 0.1 s of a 0.5 s clip.
        for _ in 0..4 {
            s.advance(1.0 / 60.0);
        }
        assert!(
            (s.percent_remaining() - 80.0).abs() < 1e-3,
            "{}",
            s.percent_remaining()
        );
        for _ in 0..16 {
            s.advance(1.0 / 60.0);
        }
        assert!(s.done);
        assert_eq!(s.percent_remaining(), 0.0);
        let mut l = Sequence {
            looping: true,
            done: false,
            time: 0.0,
            ..s.clone()
        };
        for _ in 0..30 {
            l.advance(1.0 / 60.0);
        }
        assert!(!l.done);
        assert!(l.time < l.length);
    }
}
