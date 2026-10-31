//! The engine members Eruptor's scripts reach (openspec AH.17, design
//! decisions 4 and 21), answered by [`ScriptWorld`] over the actors it keeps
//! and the [`Engine`] the TV's entities own: what each member reads and
//! writes, in the units the original registers it with
//! ([`guac_script::members::HERO`] has each one's class, units and range).
//!
//! Points a measurement is taken from (`positionmeasurement.* .@other`):
//! an actor's pivot (a hero's, his controller's), an attach node (where the
//! embedder set it in the actor's frame, as AH.50 poses a hero's on his
//! clips, else its pivot; stand-in ledger: `nodes`), the ground under an
//! actor (the ground finder's point), its origin, a reference's own point,
//! the ray caster's two points and its contact.
//!
//! The engine's move for what the scripts set going ([`ScriptWorld::advance`]):
//! a launched actor falls by its own gravity at its time scale, velocity
//! first, then its solid is swept through the seam and stops at what it
//! hits, its velocity slid along it; its contacts are what its solid touches
//! where it ends (within [`TOUCH`]).
//!
//! Hypotheses here, each a reading the oracle hasn't settled:
//! - `contactinfo.type` is the triangle's collision material byte, -1 with
//!   no ground found (level 213's reachable ground is material 1, AG.7,
//!   which no surface handler's type matches);
//! - `positionmeasurement.separation` from a point is the vertical gap from
//!   the bottom of the actor's solid (its pivot with none; a hero's, his
//!   controller's pivot) down to it, in feet, never under 0; from his own
//!   ground, a hero his support check holds has none (his skin's 5 mm isn't
//!   a gap: Update Movers' band and gravity act only over 0, and AG.38 saw
//!   neither on a supported frame);
//!   from another actor with a solid, the gap between the two solids as
//!   authored (AH.18: a Chompy nips at 10 ft from a hero, AH.10's middles
//!   14.7-15.2 ft apart, both 3 ft spheres);
//! - a sweep stops at a creature's body as at a wall, though the ball's solid
//!   doesn't meet actors (`interacts with actors` 0): the touch is what the
//!   projectile manager's Targets check reads as `collision`; contacts with
//!   another actor's body aren't `environment contacts`;
//! - nothing rebounds (`collisioninfo.rebound` is 0 on what he throws);
//! - a distance set from a point the actor sits on moves it nowhere;
//! - a relative facing on an actor hung from the ground turns it from the
//!   ground's owner's facing in the ground's plane (a hero's ground leaning
//!   as his footing's slope), so its tilt reads the slope along that heading
//!   (what Face and Move's push takes the slope's pitch from, AG.38, AG.90).

use std::collections::HashMap;

use guac_formats::script::Class;
use guac_motion::hero::Hero;
use guac_motion::proxy::Surfaces;
use guac_motion::seam::{BodyId, CAST_SLOP, Pose, Quat, Shape, Vec3};
use guac_motion::solid::HavokSolid;
use guac_motion::steer::{Aim, MoveType, Physics as SteerPhysics};
use guac_script::{Access, Answer, Handle, HostFault, MemberId, Obj, OpCall, Value};

use crate::script_engine::{
    Angles, Button, Contact, Engine, FEET_PER_METRE, Facet, METRES_PER_FOOT, Normal, Object,
    Sequence, Walker, angles_of, direction, heading_of, normalized, pitch_of,
};
use crate::scripts::{ClipInfo, ClipState, HeroWrite, ScriptWorld, Spawner};

/// A clip's weight until a script writes one: a script's throw, which none
/// weights, blends whole (AH.50's recordings hold every throw at 100), and
/// the locomotion writes each of its own every tick (openspec AH.49).
const FULL_BLEND: f32 = 100.0;

/// What a member answers, by what it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Member {
    // AH.16's: the console and the game, placements' numbers, the pads and
    // the portal.
    Platform,
    Configuration,
    GameLanguage,
    ConsoleLanguage,
    TimeScale,
    LoadLevel,
    /// `system.game achievement`: an unlock's id, kept and logged (openspec
    /// AH.220; stand-in ledger: `achievements`).
    Achievement,
    Buffer,
    Id,
    Waypoints,
    Controller(usize),
    Physical(usize),
    Portal,
    /// `redoctaneperipheralscriptobject.size of region 1` and `2` on the
    /// portal: a figure's tag's two data regions in bytes (AH.23).
    Region(u8),
    // AH.17's.
    Range,
    Orientation,
    Facing,
    Tilt,
    Roll,
    RelativeFacing,
    RelativeTilt,
    RelativeRoll,
    Scale,
    ScaleUniform,
    ScaleAxis(usize),
    Tint,
    Movement,
    Physics(PhysicsField),
    CollisionVolume,
    Solid(SolidField),
    Velocity,
    Vector(VectorField),
    ActorController,
    AttachNodes,
    AttachNode,
    AttachedObjects,
    Ground,
    ContactType,
    ContactNormal,
    ContactMagnitude,
    ContactIsTrigger,
    Clones,
    EnvironmentContacts,
    Origin,
    Remembered,
    Parent,
    Activator,
    PlayingAnimations,
    PlayingSounds,
    Palette,
    SeqTimeRemaining,
    SeqPercentRemaining,
    SeqRate,
    SeqLooping,
    SeqRawDuration,
    /// `sequence.allow restart` (openspec AH.184): a clip's, which a copy
    /// started from it takes, or a sequence's own.
    AllowRestart,
    // AH.49's: a clip's or a sequence's animation info.
    /// `animationinfo.blend percent`: the weight the engine blends by.
    Blend,
    /// `animationinfo.is layered`.
    IsLayered,
    /// `animationinfo.type`, read only.
    ClipKind,
    Distance,
    Heading,
    Pitch,
    VerticalOffset,
    HorizontalDistance,
    Separation,
    Collision,
    NormalizedAngle,
    RayCast,
    RayStart,
    RayEnd,
    RayHasContact,
    Viewport,
    // AG.28's: the gameplay camera's scripts.
    /// `viewportinfo.field of view` and `far clip` on the viewport.
    ViewportFov,
    ViewportFarClip,
    /// `viewportinfo.view converter`: the viewport's projection.
    Converter,
    /// `viewconverter.world position`: the point it projects.
    ConverterWorld,
    /// `viewconverter.screen location`: where that point lands.
    ConverterScreen,
    /// `viewportinfo.container sprite`: the screen the picture fills.
    Container,
    /// `spriteinfo.screen width` and `screen height` on the container.
    ScreenWidth,
    ScreenHeight,
    /// `actorinfo.players`: every hero in the level.
    Players,
    /// `system.absolute zero`: the world's origin as a point (AH.20).
    AbsoluteZero,
    InactiveController,
    Pad(Button),
    MainStick,
    PressureState,
    PressureValue,
    StickX,
    StickY,
    StickHeading,
    StickMagnitude,
    StickConnection,
    // AH.18's.
    /// `actorinfo.inactive`: every placement asleep by the activity rule.
    Inactive,
    /// `actorinfo.removed`: every placement and clone taken off.
    Removed,
    /// `collisioninfo.contacts with me`: the actors whose solids touch this
    /// one's (within [`CONTACT_REACH`]), nearest first.
    ContactsWithMe,
    /// `collisioninfo.object collisions` (openspec AG.160): read as the same
    /// set, the actors his solid touches. A reading of the two getters, each
    /// a set the owner's actor refreshes before it's read (aj-vm's 0x733b4
    /// and 0x73594); no oracle read tells them apart.
    ObjectCollisions,
    // AH.65's.
    /// A placed light's or its clone's `lightinfo` values.
    Light(LightField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LightField {
    Diffuse,
    Ambient,
    Specular,
    Multiplier,
    /// Constant, linear or quadratic, by its index.
    Attenuation(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicsField {
    GroundMaxSpeed,
    GroundAcceleration,
    GroundDeceleration,
    GroundTurnRate,
    AirMaxSpeed,
    AirAcceleration,
    AirDeceleration,
    AirTurnRate,
    GravityMaxSpeed,
    GravityAcceleration,
    JumpMaxSpeed,
    JumpAcceleration,
    JumpInitialSpeed,
    MoveType,
    TiltsWithGround,
    TimeScale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidField {
    Width,
    Length,
    Height,
    Offset(usize),
    Weight,
    Rebound,
    InteractsWithActors,
    InteractsWithLevelMesh,
    ActsAsActorSurface,
    Shape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VectorField {
    Length,
    Heading,
    Pitch,
    VerticalDifference,
    HorizontalLength,
}

impl Member {
    /// Every member the host answers, by its import id.
    pub(crate) fn table() -> HashMap<MemberId, Member> {
        let mut t = HashMap::new();
        let mut add = |ns: &str, name: &str, m: Member| {
            t.insert(MemberId::named(ns, name), m);
        };
        add("system", "hardware platform", Member::Platform);
        add("system", "game configuration", Member::Configuration);
        add("system", "game language", Member::GameLanguage);
        add("system", "console language", Member::ConsoleLanguage);
        add("system", "game time scale", Member::TimeScale);
        add("system", "load level", Member::LoadLevel);
        add("system", "game achievement", Member::Achievement);
        add("placement", "deactivation buffer", Member::Buffer);
        add("abstractplacement", "id", Member::Id);
        add("spritematrixmeasurement", "id", Member::Id);
        add("actorinfo", "waypoints", Member::Waypoints);
        for n in 0..2 {
            add(
                "scriptcontroller",
                &format!("controller {}", n + 1),
                Member::Controller(n),
            );
        }
        for n in 0..4 {
            add(
                "scriptcontroller",
                &format!("physical controller {}", n + 1),
                Member::Physical(n),
            );
        }
        add("redoctaneperipheralsystemobject", "portal", Member::Portal);
        for n in 1..=2u8 {
            add(
                "redoctaneperipheralscriptobject",
                &format!("size of region {n}"),
                Member::Region(n),
            );
        }
        add("placement", "activation range", Member::Range);
        add("matrixmeasurement", "orientation", Member::Orientation);
        add("orientationmeasurement", "facing", Member::Facing);
        add("orientationmeasurement", "tilt", Member::Tilt);
        add("orientationmeasurement", "roll", Member::Roll);
        add(
            "orientationmeasurement",
            "relative facing",
            Member::RelativeFacing,
        );
        add(
            "orientationmeasurement",
            "relative tilt",
            Member::RelativeTilt,
        );
        add(
            "orientationmeasurement",
            "relative roll",
            Member::RelativeRoll,
        );
        add("placement", "scale", Member::Scale);
        add("scalemeasurement", "uniform", Member::ScaleUniform);
        add("scalemeasurement", "x", Member::ScaleAxis(0));
        add("scalemeasurement", "y", Member::ScaleAxis(1));
        add("scalemeasurement", "z", Member::ScaleAxis(2));
        add("placement", "tint", Member::Tint);
        use LightField as L;
        for (name, f) in [
            ("diffuse color", L::Diffuse),
            ("ambient color", L::Ambient),
            ("specular color", L::Specular),
            ("multiplier", L::Multiplier),
            ("constant attenuation factor", L::Attenuation(0)),
            ("linear attenuation factor", L::Attenuation(1)),
            ("quadratic attenuation factor", L::Attenuation(2)),
        ] {
            add("lightinfo", name, Member::Light(f));
        }
        add("actorinfo", "movement", Member::Movement);
        use PhysicsField as P;
        for (name, f) in [
            ("ground max speed", P::GroundMaxSpeed),
            ("ground acceleration", P::GroundAcceleration),
            ("ground deceleration", P::GroundDeceleration),
            ("ground turn rate", P::GroundTurnRate),
            ("air max speed", P::AirMaxSpeed),
            ("air acceleration", P::AirAcceleration),
            ("air deceleration", P::AirDeceleration),
            ("air turn rate", P::AirTurnRate),
            ("gravity max speed", P::GravityMaxSpeed),
            ("gravity acceleration", P::GravityAcceleration),
            ("jump max speed", P::JumpMaxSpeed),
            ("jump acceleration", P::JumpAcceleration),
            ("jump initial speed", P::JumpInitialSpeed),
            ("move type", P::MoveType),
            ("tilts with ground", P::TiltsWithGround),
            ("time scale", P::TimeScale),
        ] {
            add("actorphysics", name, Member::Physics(f));
        }
        add("actorinfo", "collision volume", Member::CollisionVolume);
        use SolidField as S;
        for (name, f) in [
            ("width", S::Width),
            ("length", S::Length),
            ("height", S::Height),
            ("local x offset", S::Offset(0)),
            ("local y offset", S::Offset(1)),
            ("local z offset", S::Offset(2)),
            ("weight", S::Weight),
            ("rebound", S::Rebound),
            ("interacts with actors", S::InteractsWithActors),
            ("interacts with level mesh", S::InteractsWithLevelMesh),
            ("acts as actor surface", S::ActsAsActorSurface),
            ("shape", S::Shape),
        ] {
            add("collisioninfo", name, Member::Solid(f));
        }
        add("actorinfo", "velocity", Member::Velocity);
        use VectorField as V;
        for (name, f) in [
            ("length", V::Length),
            ("heading", V::Heading),
            ("pitch", V::Pitch),
            ("vertical difference", V::VerticalDifference),
            ("horizontal length", V::HorizontalLength),
        ] {
            add("vectormeasurement", name, Member::Vector(f));
        }
        add("actorinfo", "controller", Member::ActorController);
        add("actorinfo", "attach nodes", Member::AttachNodes);
        add("abstractplacement", "attach node", Member::AttachNode);
        add(
            "abstractplacement",
            "attached objects",
            Member::AttachedObjects,
        );
        add("abstractplacement", "ground", Member::Ground);
        add("contactinfo", "type", Member::ContactType);
        add("contactinfo", "normal", Member::ContactNormal);
        add("contactinfo", "magnitude", Member::ContactMagnitude);
        add("contactinfo", "is trigger", Member::ContactIsTrigger);
        add("abstractplacement", "clones", Member::Clones);
        add(
            "collisioninfo",
            "environment contacts",
            Member::EnvironmentContacts,
        );
        add("abstractplacement", "parent", Member::Parent);
        add("abstractplacement", "origin", Member::Origin);
        add("placementreference", "remembered", Member::Remembered);
        add("abstractplacement", "activator", Member::Activator);
        add("actorinfo", "playing animations", Member::PlayingAnimations);
        add("actorinfo", "playing sounds", Member::PlayingSounds);
        add("animationinfo", "animation palette", Member::Palette);
        add("sequence", "time remaining", Member::SeqTimeRemaining);
        add("sequence", "percent remaining", Member::SeqPercentRemaining);
        add("sequence", "playback rate", Member::SeqRate);
        add("sequence", "looping", Member::SeqLooping);
        add("sequence", "raw duration", Member::SeqRawDuration);
        add("sequence", "allow restart", Member::AllowRestart);
        add("animationinfo", "blend percent", Member::Blend);
        add("animationinfo", "is layered", Member::IsLayered);
        add("animationinfo", "type", Member::ClipKind);
        add("positionmeasurement", "distance", Member::Distance);
        add("positionmeasurement", "heading", Member::Heading);
        add("positionmeasurement", "pitch", Member::Pitch);
        add(
            "positionmeasurement",
            "vertical offset",
            Member::VerticalOffset,
        );
        add(
            "positionmeasurement",
            "horizontal distance",
            Member::HorizontalDistance,
        );
        add("positionmeasurement", "separation", Member::Separation);
        add("positionmeasurement", "collision", Member::Collision);
        add("system", "normalized angle", Member::NormalizedAngle);
        add("system", "ray cast", Member::RayCast);
        add("raycontact", "start", Member::RayStart);
        add("raycontact", "end", Member::RayEnd);
        add("raycontact", "has contact", Member::RayHasContact);
        add("viewportinfo", "default", Member::Viewport);
        add("viewportinfo", "field of view", Member::ViewportFov);
        add("viewportinfo", "far clip", Member::ViewportFarClip);
        add("viewportinfo", "view converter", Member::Converter);
        add("viewconverter", "world position", Member::ConverterWorld);
        add("viewconverter", "screen location", Member::ConverterScreen);
        add("viewportinfo", "container sprite", Member::Container);
        add("spriteinfo", "screen width", Member::ScreenWidth);
        add("spriteinfo", "screen height", Member::ScreenHeight);
        add("actorinfo", "players", Member::Players);
        add("system", "absolute zero", Member::AbsoluteZero);
        add(
            "scriptcontroller",
            "inactive controller",
            Member::InactiveController,
        );
        for n in 1..=4u8 {
            add(
                "scriptcontroller",
                &format!("action {n}"),
                Member::Pad(Button::Action(n)),
            );
        }
        for (name, b) in [
            ("accept button", Button::Accept),
            ("cancel button", Button::Cancel),
            ("alt button", Button::Alt),
            ("pause button", Button::Pause),
            ("start button", Button::Start),
            ("select button", Button::Select),
            ("dpad left", Button::DpadLeft),
            ("dpad right", Button::DpadRight),
            ("dpad up", Button::DpadUp),
            ("dpad down", Button::DpadDown),
        ] {
            add("scriptcontroller", name, Member::Pad(b));
        }
        add("scriptcontroller", "main stick", Member::MainStick);
        add("buttonmeasurement", "pressure state", Member::PressureState);
        add("buttonmeasurement", "pressure value", Member::PressureValue);
        add("joystickmeasurement", "x", Member::StickX);
        add("joystickmeasurement", "y", Member::StickY);
        add("joystickmeasurement", "heading", Member::StickHeading);
        add("joystickmeasurement", "magnitude", Member::StickMagnitude);
        add("joystickmeasurement", "connection", Member::StickConnection);
        add("actorinfo", "inactive", Member::Inactive);
        add("actorinfo", "removed", Member::Removed);
        add("collisioninfo", "contacts with me", Member::ContactsWithMe);
        add(
            "collisioninfo",
            "object collisions",
            Member::ObjectCollisions,
        );
        t
    }
}

/// `pressure state`'s labels: just released, released, pressed, just
/// pressed.
fn pressure(was: bool, now: bool) -> i32 {
    match (was, now) {
        (true, false) => 0,
        (false, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    }
}

fn float(v: Value) -> Result<f32, HostFault> {
    v.as_f32().ok_or(HostFault::Nothing)
}

impl ScriptWorld {
    /// The handle a value stands for: an entity's, a file object's, a host
    /// object's.
    pub(crate) fn handle(&self, v: Value) -> Option<Handle> {
        match v {
            Value::Entity(h) => Some(h),
            Value::Object(o) => self.objects.get(&o).copied(),
            _ => None,
        }
    }

    /// The host object a value names: `None` for an actor, a file object
    /// no handle stands for, or a value that isn't an object.
    fn object(&self, v: Option<Value>) -> Option<Object> {
        let h = self.handle(v?)?;
        self.objs.get(h)
    }

    /// A fresh engine object a script grows a set of them by
    /// ([`guac_script::Host::make`]): a waypoint, not put anywhere yet. Any
    /// other class makes none.
    pub(crate) fn make(&mut self, class: &str) -> Option<Value> {
        if class != "actorwaypoint" {
            return None;
        }
        self.waypoints_made.push(None);
        let i = (self.waypoints_made.len() - 1) as u32;
        Some(self.objs.value(Object::Waypoint(i)))
    }

    /// The actor (or hero) a receiver names: `None` for a host object.
    fn actor_of(&self, v: Option<Value>) -> Option<Handle> {
        let h = self.handle(v?)?;
        if self.objs.get(h).is_some() {
            return None;
        }
        Some(h)
    }

    /// An actor's pivot, metres: a hero's from his controller.
    pub(crate) fn position(&self, h: Handle, engine: &mut dyn Engine) -> Option<Vec3> {
        self.position_within(h, engine, HUNG_DEPTH)
    }

    /// [`ScriptWorld::position`], through at most `depth` actors hung one
    /// from another's node. One hung from an actor's node reads where that
    /// node is now, its offset as the last frame posed it turned by its
    /// owner's facing as it stands (openspec AH.50: an aimed ball leaves
    /// from Eruptor's hand, where his collider hangs, turned by the facing
    /// his throw's own turn just wrote, 0.67 m from where the collider stood
    /// after a 20° turn and within 1e-5 m of the turned node in every drive).
    fn position_within(&self, h: Handle, engine: &mut dyn Engine, depth: u8) -> Option<Vec3> {
        if let Some(hero) = engine.hero(h) {
            return Some(hero.pivot);
        }
        let a = self.actors.get(&h)?;
        if depth > 0
            && !a.removed
            && let Some(Object::Node(owner, i)) = self.object(Some(a.attach))
            && owner != h
            && let Some(p) = self.node_within(owner, i, engine, depth - 1)
        {
            return Some(p);
        }
        Some(a.position)
    }

    /// Actor `h` put at `p` by placement `by`.
    fn set_position(
        &mut self,
        by: Handle,
        h: Handle,
        p: Vec3,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        if let Some(hero) = engine.hero(h) {
            hero.pivot = p;
            // Off his scripts, still his place (openspec AH.151): counted.
            self.writes_place(by, h);
            return Ok(());
        }
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.position = p;
                Ok(())
            }
            None => Err(HostFault::Nothing),
        }
    }

    /// The ground under an actor: the ground finder's point, its normal and
    /// its material. A hero's is his footing's.
    fn ground(&self, h: Handle, engine: &mut dyn Engine) -> Option<(Vec3, Vec3, Option<u8>)> {
        if let Some(hero) = engine.hero(h) {
            let height = hero.footing.height?;
            let normal = hero.footing.ground.unwrap_or(Vec3::Y);
            let point = hero.pivot - Vec3::Y * height;
            return Some((point, normal, Some(1)));
        }
        let a = self.actors.get(&h)?;
        // Asleep, a placement has no corpus (AG.7's INACTIVE) and so no
        // ground: its ground's type reads -1, which the destructibles
        // system shuts a breakable down by and wakes it on (openspec AH.21;
        // a hypothesis, no oracle read has a sleeping one's ground).
        if a.managed && !a.active {
            return None;
        }
        let at = a.position;
        let origin = at + Vec3::Y * 0.1;
        let actors = self.actor_bodies(engine);
        let hit = guac_motion::ground::ray(engine.seam(), origin, Vec3::NEG_Y, 160.0, &actors)
            .or_else(|| guac_motion::ground::ray(engine.seam(), origin, Vec3::Y, 80.0, &actors))?;
        let normal = if hit.normal.y < -0.05 {
            -hit.normal
        } else {
            hit.normal
        };
        Some((hit.point, normal, hit.material))
    }

    /// Where a measurement taken from `v` is taken from.
    pub(crate) fn point(&mut self, v: Value, engine: &mut dyn Engine) -> Option<Vec3> {
        let Some(h) = self.handle(v) else {
            // A waypoint is a point in its file, no placement's.
            return match v {
                Value::Object(o) => self.points.get(&o).copied(),
                _ => None,
            };
        };
        match self.objs.get(h) {
            None => self.position(h, engine),
            Some(Object::Node(a, i)) => self.node(a, i, engine),
            Some(Object::Facet(a, Facet::Ground)) => self.ground(a, engine).map(|g| g.0),
            Some(Object::RayStart) => Some(self.ray.start),
            Some(Object::RayEnd) => Some(self.ray.end),
            Some(Object::RayCast) => self.ray_contact(engine).map(|c| c.0),
            Some(Object::Origin(a)) => self.actors.get(&a).map(|k| k.origin),
            Some(Object::Remembered(a)) => {
                let kept = self.actors.get(&a).and_then(|k| k.remembered);
                kept.or_else(|| self.position(a, engine))
            }
            Some(Object::RememberedRef(c)) => self.kept_points.get(&c).copied(),
            // Not kept yet, where its actor is.
            Some(Object::RememberedVia(a, c)) => self
                .kept_via
                .get(&(a, c))
                .copied()
                .or_else(|| self.position(a, engine)),
            Some(Object::Waypoint(i)) => self.waypoints_made.get(i as usize).copied().flatten(),
            Some(Object::AbsoluteZero) => Some(Vec3::ZERO),
            // The gameplay camera's eye, and the point its view converter
            // projects (openspec AG.28).
            Some(Object::Viewport) => Some(self.view.eye),
            Some(Object::ConverterPoint) => Some(self.view.converter),
            _ => None,
        }
    }

    /// Attach node `i` of actor `a` in the level: its offset turned by the
    /// actor's facing, from its pivot.
    fn node(&self, a: Handle, i: u32, engine: &mut dyn Engine) -> Option<Vec3> {
        self.node_within(a, i, engine, HUNG_DEPTH)
    }

    fn node_within(&self, a: Handle, i: u32, engine: &mut dyn Engine, depth: u8) -> Option<Vec3> {
        let pivot = self.position_within(a, engine, depth)?;
        let Some(off) = self.nodes.get(&a).and_then(|n| n.get(i as usize)) else {
            return Some(pivot);
        };
        let facing = self.angles(a, engine).map_or(0.0, |g| g.facing);
        Some(pivot + Quat::from_rotation_y(facing.to_radians()) * off.at)
    }

    /// Attach node `i` of actor `a`'s turn in the level, where the embedder
    /// posed it ([`ScriptWorld::set_nodes`]): its own turned by the actor's
    /// facing as it stands. None for a node never posed.
    fn node_turn(&self, a: Handle, i: u32, engine: &mut dyn Engine, depth: u8) -> Option<Quat> {
        let node = self.nodes.get(&a)?.get(i as usize)?;
        let facing = self
            .angles_within(a, engine, depth)
            .map_or(0.0, |g| g.facing);
        Some(Quat::from_rotation_y(facing.to_radians()) * node.turn)
    }

    fn set_point(
        &mut self,
        by: Handle,
        v: Value,
        p: Vec3,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        // A file waypoint moves where it's put (openspec AG.60: a push block
        // lays its track's waypoints from its own place at its start).
        if let Value::Object(o) = v
            && self.handle(v).is_none()
            && let Some(at) = self.points.get_mut(&o)
        {
            *at = p;
            return Ok(());
        }
        let h = self.handle(v).ok_or(HostFault::Nothing)?;
        match self.objs.get(h) {
            None => self.set_position(by, h, p, engine),
            Some(Object::RayStart) => {
                self.ray.start = p;
                Ok(())
            }
            Some(Object::Remembered(a)) => {
                let k = self.actors.get_mut(&a).ok_or(HostFault::Nothing)?;
                k.remembered = Some(p);
                Ok(())
            }
            Some(Object::RememberedRef(c)) => {
                self.kept_points.insert(c, p);
                Ok(())
            }
            Some(Object::RememberedVia(a, c)) => {
                self.kept_via.insert((a, c), p);
                Ok(())
            }
            Some(Object::Waypoint(i)) => {
                let w = self
                    .waypoints_made
                    .get_mut(i as usize)
                    .ok_or(HostFault::Nothing)?;
                *w = Some(p);
                Ok(())
            }
            Some(Object::RayEnd) => {
                self.ray.end = p;
                Ok(())
            }
            // The scripts placing the gameplay camera (openspec AG.28): once
            // the view is handed back to the TV, a write goes nowhere.
            Some(Object::Viewport) => {
                if self.view.take() {
                    self.view.eye = p;
                }
                Ok(())
            }
            Some(Object::ConverterPoint) => {
                self.view.converter = p;
                Ok(())
            }
            _ => Err(HostFault::Nothing),
        }
    }

    /// The ray caster's contact between its two points, through the seam.
    fn ray_contact(&self, engine: &mut dyn Engine) -> Option<(Vec3, Vec3)> {
        self.ray_contact_full(engine).map(|(p, n, _)| (p, n))
    }

    /// The ray cast's first contact: the level, or an actor whose collision
    /// acts as an actor surface. One that doesn't (a barrel, pot or gourd)
    /// the ray passes through: `Minion - Check Target Blocked` reads any
    /// contact as blocking unless its target's collision acts as an actor
    /// surface, so in the original the ray never meets one that doesn't
    /// (merging AH.50 into AH.21: the lob's burst from anchor A met the
    /// lying `barrel_b_intact(4)`'s own solid on the way to it and spared
    /// it, where AH.9's burst breaks it).
    fn ray_contact_full(&self, engine: &mut dyn Engine) -> Option<(Vec3, Vec3, Option<u8>)> {
        let d = self.ray.end - self.ray.start;
        let len = d.length();
        if len <= 0.0 {
            return None;
        }
        let mut past = self.actor_bodies(engine);
        loop {
            let hit = guac_motion::ground::ray(engine.seam(), self.ray.start, d, len, &past)?;
            let surface = engine
                .actor_of(hit.body)
                .and_then(|h| self.actors.get(&h))
                .is_none_or(|a| a.solid.acts_as_actor_surface != 0);
            if surface {
                return Some((hit.point, hit.normal, hit.material));
            }
            // Each pass leaves one more body out, so it ends.
            past.push(hit.body);
        }
    }

    /// The bodies in the seam that are actors (the heroes, the creatures
    /// the engine moves): what the ground finder and the ray caster see
    /// past (AH.18: the original's rays meet the level and the props, and a
    /// Chompy's look at a hero would stop on its own body or his; the ray
    /// caster also passes a prop that doesn't act as an actor surface,
    /// [`ScriptWorld::ray_contact_full`]).
    fn actor_bodies(&self, engine: &mut dyn Engine) -> Vec<BodyId> {
        let mut out: Vec<BodyId> = engine
            .heroes()
            .into_iter()
            .filter_map(|h| engine.body(h))
            .collect();
        for (&h, a) in &self.actors {
            if a.walker.is_some()
                && let Some(b) = engine.body(h)
            {
                out.push(b);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// An actor's turn: a hero's facing his controller's; his tilt and roll
    /// read 0 (his lean is the engine's, never the script's).
    fn angles(&self, h: Handle, engine: &mut dyn Engine) -> Option<Angles> {
        self.angles_within(h, engine, HUNG_DEPTH)
    }

    /// [`ScriptWorld::angles`], through at most `depth` actors hung one from
    /// another's node: one hung from a posed node reads that node's turn as
    /// it stands (openspec AH.60: Eruptor's collider on his hand, within
    /// 0.06° of the oracle's on every frame of AH.50's throws; the
    /// autoaim's strip runs along it).
    fn angles_within(&self, h: Handle, engine: &mut dyn Engine, depth: u8) -> Option<Angles> {
        if h == self.objs_viewport() {
            // The camera's scripts' turn once they place it (openspec AG.28),
            // else the TV's own camera's heading, as a headless run steering
            // by a recorded camera's gives it.
            if self.view.scripted() && !engine.holds_view() {
                return Some(self.view.angles);
            }
            return Some(crate::script_engine::Angles {
                facing: engine.view_heading(),
                ..Default::default()
            });
        }
        if let Some(hero) = engine.hero(h) {
            return Some(crate::script_engine::Angles {
                facing: hero.facing,
                ..Default::default()
            });
        }
        // An actor's remembered point is turned as the actor is (AH.60: the
        // autoaim's strip runs along its aimer's remembered point's facing).
        if let Some(Object::Remembered(a) | Object::RememberedVia(a, _)) = self.objs.get(h) {
            return self.angles_within(a, engine, depth);
        }
        let a = self.actors.get(&h)?;
        if depth > 0
            && !a.removed
            && let Some(Object::Node(owner, i)) = self.object(Some(a.attach))
            && owner != h
            && let Some(q) = self.node_turn(owner, i, engine, depth - 1)
        {
            return Some(angles_of(q));
        }
        Some(a.angles)
    }

    fn objs_viewport(&self) -> Handle {
        self.viewport
    }

    fn velocity(&self, h: Handle, engine: &mut dyn Engine) -> Option<Vec3> {
        if let Some(hero) = engine.hero(h) {
            return Some(hero.walk.velocity);
        }
        self.actors.get(&h).map(|a| a.velocity)
    }

    fn set_velocity(
        &mut self,
        h: Handle,
        v: Vec3,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        if let Some(hero) = engine.hero(h) {
            hero.walk.velocity = v;
            return Ok(());
        }
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.velocity = v;
                // A creature walks by it; anything else flies.
                a.launched |= a.walker.is_none() && v != Vec3::ZERO;
                Ok(())
            }
            None => Err(HostFault::Nothing),
        }
    }

    /// The vector a `vectormeasurement` facet reads: an actor's velocity,
    /// ft/s, or its ground's normal.
    fn vector(&mut self, f: Object, engine: &mut dyn Engine) -> Option<Vec3> {
        match f {
            Object::Facet(h, Facet::Velocity) => self.velocity(h, engine),
            Object::Normal(Normal::Ground(h)) => self.ground(h, engine).map(|g| g.1),
            Object::Normal(Normal::Contact(h, i)) => self
                .actors
                .get(&h)
                .and_then(|a| a.contacts.get(i as usize))
                .map(|c| c.normal),
            Object::Normal(Normal::Ray) => self.ray_contact(engine).map(|c| c.1),
            _ => None,
        }
    }

    /// What a `contactinfo` member reads on: the ground under an actor, one
    /// of its environment contacts or the ray caster's contact; its point,
    /// its normal's source and its material.
    fn contact(
        &mut self,
        o: Option<Object>,
        engine: &mut dyn Engine,
    ) -> Option<(Normal, Option<u8>, bool)> {
        match o? {
            Object::Facet(h, Facet::Ground) => Some(match self.ground(h, engine) {
                Some((_, _, m)) => (Normal::Ground(h), m, true),
                None => (Normal::Ground(h), None, false),
            }),
            Object::Contact(h, i) => {
                let c = self.actors.get(&h)?.contacts.get(i as usize).copied()?;
                Some((Normal::Contact(h, i), c.material, true))
            }
            Object::RayCast => Some(match self.ray_contact_full(engine) {
                Some((_, _, m)) => (Normal::Ray, m, true),
                None => (Normal::Ray, None, false),
            }),
            _ => None,
        }
    }

    /// The light a receiver names (openspec AH.65): a clone's own, or a
    /// light placement's as its file has it (no handle stands for one).
    fn light_mut(&mut self, recv: Option<Value>) -> Option<&mut crate::script_engine::Light> {
        match recv? {
            Value::Object(o) if !self.objects.contains_key(&o) => self.light_templates.get_mut(&o),
            v => {
                let h = self.handle(v)?;
                self.actors.get_mut(&h)?.light.as_mut()
            }
        }
    }

    /// The owner of the ground facet `h` hangs from, if it hangs from one
    /// (Face and Move's velocity actor on its mover's ground): what a
    /// relative facing turns it from.
    fn ground_hung(&self, h: Handle) -> Option<Handle> {
        self.actors
            .get(&h)
            .and_then(|a| self.object(Some(a.attach)))
            .and_then(|o| match o {
                Object::Facet(g, Facet::Ground) => Some(g),
                _ => None,
            })
    }

    pub(crate) fn get_member(
        &mut self,
        at: &Access,
        m: Member,
        engine: &mut dyn Engine,
    ) -> Result<Value, HostFault> {
        let recv = at.receiver;
        let actor = self.actor_of(recv);
        let object = self.object(recv);
        let other = |w: &mut Self, engine: &mut dyn Engine| -> Result<Vec3, HostFault> {
            let o = at.other_point.ok_or(HostFault::Nothing)?;
            w.point(o, engine).ok_or(HostFault::Nothing)
        };
        let me = |w: &mut Self, engine: &mut dyn Engine| -> Result<Vec3, HostFault> {
            let r = recv.ok_or(HostFault::Nothing)?;
            w.point(r, engine).ok_or(HostFault::Nothing)
        };
        Ok(match m {
            Member::Range => Value::Float(
                self.actors
                    .get(&actor.ok_or(HostFault::Nothing)?)
                    .ok_or(HostFault::Nothing)?
                    .range,
            ),
            Member::Orientation => match recv {
                // A file waypoint's turn (openspec AG.60).
                Some(Value::Object(o)) if self.handle(Value::Object(o)).is_none() => {
                    if !self.points.contains_key(&o) {
                        return Err(HostFault::Nothing);
                    }
                    self.objs.value(Object::PointTurn(o))
                }
                // A solid's own turn (openspec AG.170): its collision
                // volume's orientation, apart from its actor's.
                Some(_) if matches!(object, Some(Object::Facet(_, Facet::Collision))) => {
                    let Some(Object::Facet(h, _)) = object else {
                        unreachable!("matched as a collision volume")
                    };
                    self.objs.value(Object::Facet(h, Facet::SolidTurn))
                }
                Some(v) => {
                    let h = self.handle(v).ok_or(HostFault::Nothing)?;
                    self.objs.value(Object::Facet(h, Facet::Orientation))
                }
                None => return Err(HostFault::Nothing),
            },
            Member::Facing
            | Member::RelativeFacing
            | Member::Tilt
            | Member::RelativeTilt
            | Member::Roll
            | Member::RelativeRoll => {
                let a = match object {
                    // A relative facing on an actor hung from no ground turns
                    // it by what's written (the setter, AG.60), so it reads
                    // 0: a script's own `relative facing + k` turns it by k
                    // (`Boulder_Pebbles.ai`, `Chest_Lock.ai`), where reading
                    // its facing doubled it each tick. AH.145's recordings
                    // read a relative roll's slide from 0 the same way.
                    Some(Object::Facet(h, Facet::Orientation))
                        if matches!(m, Member::RelativeFacing)
                            && self.actors.contains_key(&h)
                            && engine.hero(h).is_none()
                            && self.ground_hung(h).is_none() =>
                    {
                        return Ok(Value::Float(0.0));
                    }
                    Some(Object::Facet(h, Facet::Orientation)) => {
                        self.angles(h, engine).ok_or(HostFault::Nothing)?
                    }
                    // A solid's own turn (AG.170): its absolute angles alone.
                    Some(Object::Facet(h, Facet::SolidTurn))
                        if matches!(m, Member::Facing | Member::Tilt | Member::Roll) =>
                    {
                        let [tilt, facing, roll] =
                            self.actors.get(&h).ok_or(HostFault::Nothing)?.solid.angles;
                        Angles { tilt, facing, roll }
                    }
                    // One the scripts never turned reads nothing, as before:
                    // its file's turn isn't read.
                    Some(Object::PointTurn(o)) => {
                        *self.point_turns.get(&o).ok_or(HostFault::Nothing)?
                    }
                    _ => return Err(HostFault::Nothing),
                };
                Value::Float(match m {
                    Member::Facing | Member::RelativeFacing => a.facing,
                    Member::Tilt | Member::RelativeTilt => a.tilt,
                    _ => a.roll,
                })
            }
            Member::Scale => self.facet(recv, Facet::Scale)?,
            Member::ScaleUniform | Member::ScaleAxis(_) => {
                let Some(Object::Facet(h, Facet::Scale)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self.actors.get(&h).map_or([100.0; 3], |a| a.scale);
                match m {
                    Member::ScaleAxis(i) => Value::Float(s[i]),
                    // aj-vm: the uniform getter fails unless x = y = z.
                    _ if s[0] == s[1] && s[1] == s[2] => Value::Float(s[0]),
                    _ => return Err(HostFault::Nothing),
                }
            }
            Member::Tint => Value::Color(
                self.actors
                    .get(&actor.ok_or(HostFault::Nothing)?)
                    .map_or(0x8080_80FF, |a| a.tint),
            ),
            Member::Light(f) => {
                let l = *self.light_mut(recv).ok_or(HostFault::Nothing)?;
                match f {
                    LightField::Diffuse => Value::Color(l.diffuse),
                    LightField::Ambient => Value::Color(l.ambient),
                    LightField::Specular => Value::Color(l.specular),
                    LightField::Multiplier => Value::Float(l.multiplier),
                    LightField::Attenuation(i) => Value::Float(l.attenuation[i]),
                }
            }
            Member::Movement => self.facet(recv, Facet::Movement)?,
            Member::Physics(f) => {
                let Some(Object::Facet(h, Facet::Movement)) = object else {
                    return Err(HostFault::Nothing);
                };
                let p = self.actors.get(&h).ok_or(HostFault::Nothing)?.physics;
                use PhysicsField as P;
                match f {
                    P::GroundMaxSpeed => Value::Float(p.ground_max_speed),
                    P::GroundAcceleration => Value::Float(p.ground_acceleration),
                    P::GroundDeceleration => Value::Float(p.ground_deceleration),
                    P::GroundTurnRate => Value::Float(p.ground_turn_rate),
                    P::AirMaxSpeed => Value::Float(p.air_max_speed),
                    P::AirAcceleration => Value::Float(p.air_acceleration),
                    P::AirDeceleration => Value::Float(p.air_deceleration),
                    P::AirTurnRate => Value::Float(p.air_turn_rate),
                    P::GravityMaxSpeed => Value::Float(p.gravity_max_speed),
                    P::GravityAcceleration => Value::Float(p.gravity_acceleration),
                    P::JumpMaxSpeed => Value::Float(p.jump_max_speed),
                    P::JumpAcceleration => Value::Float(p.jump_acceleration),
                    P::JumpInitialSpeed => Value::Float(p.jump_initial_speed),
                    P::MoveType => Value::Int(p.move_type),
                    P::TiltsWithGround => Value::Int(p.tilts_with_ground),
                    P::TimeScale => Value::Float(p.time_scale),
                }
            }
            Member::CollisionVolume => self.facet(recv, Facet::Collision)?,
            Member::Solid(f) => {
                let Some(Object::Facet(h, Facet::Collision)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self.actors.get(&h).ok_or(HostFault::Nothing)?.solid;
                use SolidField as S;
                match f {
                    S::Width => Value::Float(s.width),
                    S::Length => Value::Float(s.length),
                    S::Height => Value::Float(s.height),
                    S::Offset(i) => Value::Float(s.offset[i] * FEET_PER_METRE),
                    S::Weight => Value::Float(s.weight),
                    S::Rebound => Value::Float(s.rebound),
                    S::InteractsWithActors => Value::Int(s.interacts_with_actors),
                    S::InteractsWithLevelMesh => Value::Int(s.interacts_with_level_mesh),
                    S::ActsAsActorSurface => Value::Int(s.acts_as_actor_surface),
                    S::Shape => Value::Int(s.shape),
                }
            }
            Member::Velocity => self.facet(recv, Facet::Velocity)?,
            Member::Vector(f) => {
                let o = object.ok_or(HostFault::Nothing)?;
                let v = self.vector(o, engine).ok_or(HostFault::Nothing)?;
                use VectorField as V;
                Value::Float(match f {
                    V::Length => v.length(),
                    V::Heading => heading_of(v),
                    V::Pitch => pitch_of(v),
                    V::VerticalDifference => v.y,
                    V::HorizontalLength => v.x.hypot(v.z),
                })
            }
            Member::ActorController => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors.get(&h).ok_or(HostFault::Nothing)?.controller
            }
            Member::AttachNode => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors.get(&h).ok_or(HostFault::Nothing)?.attach
            }
            Member::Ground => self.facet(recv, Facet::Ground)?,
            Member::Origin => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors.get(&h).ok_or(HostFault::Nothing)?;
                self.objs.value(Object::Origin(h))
            }
            Member::Remembered => match (recv, at.via) {
                // A reference naming nothing: its own point.
                (Some(Value::Cell(c)), _) => self.objs.value(Object::RememberedRef(c)),
                // A reference to an actor: the point that reference keeps,
                // apart from any other's to the same actor (openspec AG.28:
                // the camera's look-from and look-at name one follower).
                (_, Some(c)) => {
                    let h = actor.ok_or(HostFault::Nothing)?;
                    self.objs.value(Object::RememberedVia(h, c))
                }
                _ => {
                    let h = actor.ok_or(HostFault::Nothing)?;
                    self.objs.value(Object::Remembered(h))
                }
            },
            Member::ContactType => {
                let (_, m, found) = self.contact(object, engine).ok_or(HostFault::Nothing)?;
                // An awake actor with no solid touches nothing, so a ground
                // it hasn't is no reason to read -1 (openspec AG.28: the
                // camera's target is one, dropped from `New CameraTargets
                // Set` by `Camera - Clean Up Targets` on that reading where
                // the level's mesh ends under it, which AG.13's corridor
                // recordings never do; a hypothesis, the ground facet of such
                // an actor not read).
                let free = match object {
                    Some(Object::Facet(h, Facet::Ground)) => {
                        engine.hero(h).is_none()
                            && self
                                .actors
                                .get(&h)
                                .is_some_and(|a| a.havok().is_none() && !(a.managed && !a.active))
                    }
                    _ => false,
                };
                Value::Int(match (found, free) {
                    (true, _) => i32::from(m.unwrap_or(0)),
                    (false, true) => 0,
                    (false, false) => -1,
                })
            }
            Member::ContactNormal => {
                let (n, _, _) = self.contact(object, engine).ok_or(HostFault::Nothing)?;
                self.objs.value(Object::Normal(n))
            }
            Member::ContactMagnitude => Value::Float(0.0),
            Member::ContactIsTrigger => Value::Int(0),
            Member::Parent => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors.get(&h).ok_or(HostFault::Nothing)?.parent
            }
            // A clone's is the placement whose script spawned it (openspec
            // AG.60: the fence's cross beam places itself from the fence's
            // facing and width each tick); a file's placement, or a hero,
            // answers itself (AH.17's reading).
            Member::Activator => {
                let r = recv.ok_or(HostFault::Nothing)?;
                match actor.and_then(|h| self.actors.get(&h)) {
                    Some(a) if a.activator != Value::Null => a.activator,
                    _ => r,
                }
            }
            // A clip itself (openspec AH.49): whether it loops, its weight,
            // its layered bit and its kind, as a sequence started from it
            // starts with them.
            Member::SeqLooping | Member::Blend | Member::IsLayered | Member::ClipKind
                if matches!(object, Some(Object::Clip { .. })) =>
            {
                let Some(Object::Clip { template, slot }) = object else {
                    return Err(HostFault::Nothing);
                };
                let (info, state) = self.clip_flags(template, slot);
                match m {
                    Member::SeqLooping => Value::Int(i32::from(state.looping.unwrap_or(false))),
                    Member::Blend => Value::Float(state.blend.unwrap_or(FULL_BLEND)),
                    Member::IsLayered => Value::Int(i32::from(
                        state.layered.unwrap_or(info.layered) || info.kind != ClipInfo::BASE,
                    )),
                    _ => Value::Int(i32::from(info.kind)),
                }
            }
            Member::Blend | Member::IsLayered | Member::ClipKind => {
                let Some(Object::Sequence(i)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self.sequences.get(i as usize).ok_or(HostFault::Nothing)?;
                match m {
                    Member::Blend => Value::Float(s.blend),
                    Member::IsLayered => Value::Int(i32::from(s.is_layered())),
                    _ => Value::Int(i32::from(s.kind)),
                }
            }
            Member::SeqTimeRemaining
            | Member::SeqPercentRemaining
            | Member::SeqRate
            | Member::SeqLooping
            | Member::SeqRawDuration => {
                let Some(Object::Sequence(i)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self.sequences.get(i as usize).ok_or(HostFault::Nothing)?;
                match m {
                    Member::SeqTimeRemaining => Value::Float(s.time_remaining()),
                    Member::SeqPercentRemaining => Value::Float(s.percent_remaining()),
                    Member::SeqRate => Value::Float(s.rate),
                    Member::SeqLooping => Value::Int(i32::from(s.looping)),
                    _ => Value::Float(s.length),
                }
            }
            // A clip's (what its next copy starts with) or a sequence's.
            Member::AllowRestart => Value::Int(i32::from(match object {
                Some(Object::Clip { template, slot }) => self
                    .clip_flags(template, slot)
                    .1
                    .allow_restart
                    .unwrap_or(false),
                Some(Object::Sequence(i)) => {
                    self.sequences
                        .get(i as usize)
                        .ok_or(HostFault::Nothing)?
                        .allow_restart
                }
                _ => return Err(HostFault::Nothing),
            })),
            Member::Distance => {
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                let d = a - b;
                Value::Float((d.length_squared() + f32::MIN_POSITIVE).sqrt() * FEET_PER_METRE)
            }
            Member::HorizontalDistance => {
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                Value::Float((a.x - b.x).hypot(a.z - b.z) * FEET_PER_METRE)
            }
            Member::VerticalOffset => {
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                Value::Float((a.y - b.y) * FEET_PER_METRE)
            }
            Member::Heading => {
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                Value::Float(heading_of(a - b))
            }
            Member::Pitch => {
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                Value::Float(pitch_of(a - b))
            }
            Member::Separation => {
                // A hero his support check holds is touching what's under
                // him: no gap, whatever his skin leaves (Update Movers' band
                // and gravity act only while it's over 0, AG.38).
                if let (Some(h), Some(o)) = (actor, at.other_point)
                    && self.object(Some(o)) == Some(Object::Facet(h, Facet::Ground))
                    && engine.hero(h).is_some_and(|hero| hero.footing.supported)
                {
                    return Ok(Value::Float(0.0));
                }
                // From another actor's body: the gap between the two solids
                // as authored (a Chompy nips at 10 ft of it, AH.10's middles
                // 15 ft apart).
                if let (Some(h), Some(o)) = (actor, at.other_point)
                    && let Some(b) = self.actor_of(Some(o))
                    && b != h
                    && let Some((sa, pa)) = self.query(h, engine)
                    && let Some((sb, pb)) = self.query(b, engine)
                {
                    return Ok(Value::Float(
                        guac_motion::parry::gap(&sa, pa, &sb, pb) * FEET_PER_METRE,
                    ));
                }
                let (a, b) = (me(self, engine)?, other(self, engine)?);
                let low = actor.and_then(|h| self.bottom(h, engine)).unwrap_or(a.y);
                Value::Float(((low - b.y) * FEET_PER_METRE).max(0.0))
            }
            Member::NormalizedAngle => Value::Float(self.normalized),
            Member::RayCast => self.objs.value(Object::RayCast),
            Member::RayStart => self.objs.value(Object::RayStart),
            Member::RayEnd => self.objs.value(Object::RayEnd),
            Member::RayHasContact => Value::Int(i32::from(self.ray_contact(engine).is_some())),
            Member::Viewport => Value::Entity(self.viewport),
            // The gameplay camera's own (openspec AG.28).
            Member::ViewportFov
            | Member::ViewportFarClip
            | Member::Converter
            | Member::Container
                if recv.and_then(|r| self.handle(r)) != Some(self.viewport) =>
            {
                return Err(HostFault::Nothing);
            }
            Member::ViewportFov => Value::Float(self.view.fov),
            Member::ViewportFarClip => Value::Float(self.view.far_clip),
            Member::Converter => self.objs.value(Object::ViewConverter),
            Member::Container => self.objs.value(Object::ContainerSprite),
            Member::ConverterWorld | Member::ConverterScreen
                if object != Some(Object::ViewConverter) =>
            {
                return Err(HostFault::Nothing);
            }
            Member::ConverterWorld => self.objs.value(Object::ConverterPoint),
            // Where the converter's point lands on the screen through the
            // camera as its scripts left it; nowhere at or behind the eye.
            Member::ConverterScreen => {
                let (x, y) = self
                    .view
                    .project(self.view.converter)
                    .ok_or(HostFault::Nothing)?;
                Value::Screen(crate::viewport::pack_screen(x, y))
            }
            // Only the container's size is known: a sprite's own is the
            // interface's, which nothing models.
            Member::ScreenWidth | Member::ScreenHeight
                if object != Some(Object::ContainerSprite) =>
            {
                return Err(HostFault::Unmodelled);
            }
            Member::ScreenWidth => Value::Float(crate::viewport::SCREEN_WIDTH),
            Member::ScreenHeight => Value::Float(crate::viewport::SCREEN_HEIGHT),
            Member::AbsoluteZero => self.objs.value(Object::AbsoluteZero),
            Member::InactiveController => self.objs.value(Object::Inactive),
            Member::Pad(b) => {
                let pad = self.pad_of(recv).ok_or(HostFault::Nothing)?;
                self.objs.value(Object::Button(pad, b))
            }
            Member::MainStick => {
                let pad = self.pad_of(recv).ok_or(HostFault::Nothing)?;
                self.objs.value(Object::Stick(pad))
            }
            Member::PressureState | Member::PressureValue => {
                let Some(Object::Button(pad, b)) = object else {
                    return Err(HostFault::Nothing);
                };
                let held = |p: &crate::script_engine::Pad| match b {
                    Button::Action(n) => {
                        p.actions.get(usize::from(n) - 1).copied().unwrap_or(false)
                    }
                    _ => false,
                };
                let (was, now) = if pad == u8::MAX {
                    (false, false)
                } else {
                    let i = usize::from(pad);
                    (
                        self.pads_before.get(i).is_some_and(held),
                        self.pads.get(i).is_some_and(held),
                    )
                };
                match m {
                    Member::PressureState => Value::Int(pressure(was, now)),
                    _ => Value::Float(if now { 1.0 } else { 0.0 }),
                }
            }
            Member::StickX
            | Member::StickY
            | Member::StickHeading
            | Member::StickMagnitude
            | Member::StickConnection => {
                let Some(Object::Stick(pad)) = object else {
                    return Err(HostFault::Nothing);
                };
                let p = if pad == u8::MAX {
                    crate::script_engine::Pad::default()
                } else {
                    self.pads.get(usize::from(pad)).copied().unwrap_or_default()
                };
                let push = p.stick.push(0.0);
                match m {
                    Member::StickX => Value::Float(p.stick.x),
                    Member::StickY => Value::Float(p.stick.y),
                    Member::StickHeading => {
                        Value::Float(if push.held() { push.heading } else { 0.0 })
                    }
                    Member::StickMagnitude => Value::Float(push.strength),
                    _ => Value::Int(i32::from(p.connected)),
                }
            }
            // Sets are read whole through `elements`.
            Member::AttachNodes
            | Member::AttachedObjects
            | Member::Clones
            | Member::PlayingAnimations
            | Member::PlayingSounds
            | Member::Palette
            | Member::EnvironmentContacts
            | Member::Waypoints
            | Member::Inactive
            | Member::Removed
            | Member::ContactsWithMe
            | Member::ObjectCollisions
            | Member::Players => return Err(HostFault::Nothing),
            Member::Collision => {
                let a = actor.ok_or(HostFault::Nothing)?;
                let o = at.other_point.ok_or(HostFault::Nothing)?;
                let b = self.handle(o).ok_or(HostFault::Nothing)?;
                if b == self.viewport {
                    // An ask read as gating play, off the view the camera's
                    // scripts leave while they drive it (openspec AH.150;
                    // its hypotheses `Viewport::shows`').
                    if at.op.is_some_and(|op| self.view_asks.contains(&op)) && self.view.scripted()
                    {
                        let p = self
                            .point(Value::Entity(a), engine)
                            .ok_or(HostFault::Nothing)?;
                        return Ok(Value::Int(i32::from(self.view.shows(p))));
                    }
                    // Every other: on screen, yes for every point (AH.20;
                    // stand-in ledger: `on-screen`): the fight's one reader
                    // is the orbs' idle bounce, which only shows on screen,
                    // and AH.150's cosmetic asks gate nothing in play.
                    return Ok(Value::Int(1));
                }
                if self.objs.get(b) == Some(Object::RayCast) {
                    // Against the ray caster (the autoaim's check for
                    // collision shapes, `Projectiles.ai`, its targets' solids
                    // against the ray from its start to its end): no touch,
                    // as the seam's side has always answered it for a target
                    // with a body there (a hypothesis, AH.90;
                    // stand-in ledger: `ray-touch`). The fight's headless
                    // runs keep no props in the seam, and AG.90's fix walked
                    // a lob there onto this check.
                    return Ok(Value::Int(0));
                }
                Value::Int(i32::from(self.touching(a, b, engine)?))
            }
            // AH.16's are answered by `ScriptWorld::get` before this.
            _ => return Err(HostFault::Unmodelled),
        })
    }

    /// A pad a controller object stands for: 0 and 1 for the two players'
    /// controllers and the physical ones, `u8::MAX` for the inactive one.
    fn pad_of(&self, v: Option<Value>) -> Option<u8> {
        let h = self.handle(v?)?;
        if let Some(i) = self.controllers.iter().position(|&c| c == h) {
            return Some(i as u8);
        }
        if let Some(i) = self.physical.iter().position(|&c| c == h) {
            return Some(i as u8);
        }
        if self.objs.get(h) == Some(Object::Inactive) {
            return Some(u8::MAX);
        }
        None
    }

    fn facet(&mut self, recv: Option<Value>, f: Facet) -> Result<Value, HostFault> {
        let h = self
            .handle(recv.ok_or(HostFault::Nothing)?)
            .ok_or(HostFault::Nothing)?;
        Ok(self.objs.value(Object::Facet(h, f)))
    }

    pub(crate) fn set_member(
        &mut self,
        at: &Access,
        m: Member,
        v: Value,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        let recv = at.receiver;
        let actor = self.actor_of(recv);
        let object = self.object(recv);
        match m {
            Member::Range => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors.get_mut(&h).ok_or(HostFault::Nothing)?.range = float(v)?;
                self.note_wake(h, at.placement);
            }
            Member::Facing
            | Member::RelativeFacing
            | Member::Tilt
            | Member::RelativeTilt
            | Member::Roll
            | Member::RelativeRoll => {
                // A file waypoint turned (openspec AG.60), from nothing where
                // the scripts never turned it.
                if let Some(Object::PointTurn(o)) = object {
                    let x = float(v)?;
                    let a = self.point_turns.entry(o).or_default();
                    match m {
                        Member::Facing | Member::RelativeFacing => a.facing = x,
                        Member::Tilt | Member::RelativeTilt => a.tilt = x,
                        _ => a.roll = x,
                    }
                    return Ok(());
                }
                // A solid's own turn (openspec AG.170): `Set Actor Collision
                // Settings` tips a character's onto its side; his solid is
                // built turned so (`walkers::rebuilt`).
                if let Some(Object::Facet(h, Facet::SolidTurn)) = object
                    && matches!(m, Member::Facing | Member::Tilt | Member::Roll)
                {
                    let x = float(v)?;
                    let a = &mut self.actors.get_mut(&h).ok_or(HostFault::Nothing)?.solid;
                    match m {
                        Member::Tilt => a.angles[0] = x,
                        Member::Facing => a.angles[1] = x,
                        _ => a.angles[2] = x,
                    }
                    return Ok(());
                }
                let Some(Object::Facet(h, Facet::Orientation)) = object else {
                    return Err(HostFault::Nothing);
                };
                let x = float(v)?;
                // The gameplay camera turned by its scripts (openspec AG.28):
                // kept unless the view was handed back; the eye's place is
                // what gives the scripts the view.
                if h == self.viewport {
                    if self.view.source != crate::viewport::Source::HandedBack {
                        let a = &mut self.view.angles;
                        match m {
                            Member::Facing | Member::RelativeFacing => a.facing = x,
                            Member::Tilt | Member::RelativeTilt => a.tilt = x,
                            _ => a.roll = x,
                        }
                    }
                    return Ok(());
                }
                if engine.hero(h).is_some() {
                    // Off his scripts his facing is the controller's
                    // (openspec AH.151).
                    if matches!(m, Member::Facing | Member::RelativeFacing)
                        && !self.drops(at.placement, h, HeroWrite::Facing)
                        && let Some(hero) = engine.hero(h)
                    {
                        hero.facing = x.rem_euclid(360.0);
                    }
                    return Ok(());
                }
                let ground = match m {
                    Member::RelativeFacing => self.ground_hung(h),
                    _ => None,
                };
                // The ground's frame faces its owner's way, so a relative
                // facing turns from his facing (Face and Move writes the way
                // it asks less his facing), and a hero's leans as his
                // footing's slope does (AG.38's push tilt: his settled lean's,
                // else the ground finder's; which of the two is AG.16's
                // hypothesis (2), one plane under AG.90's rock passing
                // either). Read as a heading of its own, it
                // tilted his push along whatever lay that far from 0° (AG.90:
                // 7° up on the ledge route's 17° slope whichever way he
                // walked, where the slope along his climb is 13°).
                let slope = match ground {
                    Some(g) => {
                        let facing = self.angles(g, engine).map_or(0.0, |a| a.facing);
                        let normal = match engine.hero(g) {
                            Some(hero) => hero.footing.height.map(|_| hero.footing.slope),
                            None => self.ground(g, engine).map(|g| g.1),
                        };
                        normal.map(|n| (n, facing))
                    }
                    None => None,
                };
                let a = self.actors.get_mut(&h).ok_or(HostFault::Nothing)?;
                match m {
                    Member::Facing => a.angles.facing = x,
                    // One hung from nothing turns by it (openspec AG.60: a
                    // spinning coin's loot update writes its spin speed each
                    // tick, and AH.11's recordings have each coin's facing
                    // step by that speed every frame, 2.82, -5.31 and 4.23
                    // degrees on the walkway's three).
                    Member::RelativeFacing if ground.is_none() => {
                        a.angles.facing = (a.angles.facing + x).rem_euclid(360.0);
                    }
                    Member::RelativeFacing => {
                        a.angles.facing = x;
                        if let Some((n, facing)) = slope {
                            let dir = guac_motion::walk::push_direction(facing + x, Some(n));
                            a.angles.tilt = pitch_of(dir);
                        }
                    }
                    Member::Tilt | Member::RelativeTilt => a.angles.tilt = x,
                    _ => a.angles.roll = x,
                }
            }
            Member::ScaleUniform | Member::ScaleAxis(_) => {
                let Some(Object::Facet(h, Facet::Scale)) = object else {
                    return Err(HostFault::Nothing);
                };
                let x = float(v)?;
                let a = self.actors.get_mut(&h).ok_or(HostFault::Nothing)?;
                match m {
                    Member::ScaleAxis(i) => a.scale[i] = x,
                    _ => a.scale = [x; 3],
                }
            }
            Member::Tint => {
                let h = actor.ok_or(HostFault::Nothing)?;
                if let Value::Color(c) = v {
                    self.actors.get_mut(&h).ok_or(HostFault::Nothing)?.tint = c;
                }
            }
            Member::Light(f) => {
                let l = self.light_mut(recv).ok_or(HostFault::Nothing)?;
                match (f, v) {
                    (LightField::Diffuse, Value::Color(c)) => l.diffuse = c,
                    (LightField::Ambient, Value::Color(c)) => l.ambient = c,
                    (LightField::Specular, Value::Color(c)) => l.specular = c,
                    (LightField::Multiplier, v) => l.multiplier = float(v)?,
                    (LightField::Attenuation(i), v) => l.attenuation[i] = float(v)?,
                    _ => {}
                }
            }
            // A light's vertical offset: how far over what it hangs from it
            // sits (openspec AH.65: the point-light manager writes its
            // `V Offset` measured from the light's parent each tick, and
            // hangs it on that parent each tick too, so it's kept as the
            // light's lift, not as a place the next hang would undo).
            Member::VerticalOffset if self.light_mut(recv).is_some() => {
                let x = float(v)?;
                if let Some(l) = self.light_mut(recv) {
                    l.lift = x;
                }
            }
            Member::Physics(f) => {
                let Some(Object::Facet(h, Facet::Movement)) = object else {
                    return Err(HostFault::Nothing);
                };
                let p = &mut self.actors.get_mut(&h).ok_or(HostFault::Nothing)?.physics;
                use PhysicsField as P;
                match f {
                    P::GroundMaxSpeed => p.ground_max_speed = float(v)?,
                    P::GroundAcceleration => p.ground_acceleration = float(v)?,
                    P::GroundDeceleration => p.ground_deceleration = float(v)?,
                    P::GroundTurnRate => p.ground_turn_rate = float(v)?,
                    P::AirMaxSpeed => p.air_max_speed = float(v)?,
                    P::AirAcceleration => p.air_acceleration = float(v)?,
                    P::AirDeceleration => p.air_deceleration = float(v)?,
                    P::AirTurnRate => p.air_turn_rate = float(v)?,
                    P::GravityMaxSpeed => p.gravity_max_speed = float(v)?,
                    P::GravityAcceleration => p.gravity_acceleration = float(v)?,
                    P::JumpMaxSpeed => p.jump_max_speed = float(v)?,
                    P::JumpAcceleration => p.jump_acceleration = float(v)?,
                    P::JumpInitialSpeed => p.jump_initial_speed = float(v)?,
                    P::MoveType => p.move_type = v.as_i32().ok_or(HostFault::Nothing)?,
                    P::TiltsWithGround => {
                        p.tilts_with_ground = v.as_i32().ok_or(HostFault::Nothing)?
                    }
                    P::TimeScale => p.time_scale = float(v)?,
                }
            }
            Member::Solid(f) => {
                let Some(Object::Facet(h, Facet::Collision)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = &mut self.actors.get_mut(&h).ok_or(HostFault::Nothing)?.solid;
                use SolidField as S;
                let i = || v.as_i32().ok_or(HostFault::Nothing);
                match f {
                    S::Width => s.width = float(v)?,
                    S::Length => s.length = float(v)?,
                    S::Height => s.height = float(v)?,
                    S::Offset(k) => s.offset[k] = float(v)? * METRES_PER_FOOT,
                    S::Weight => s.weight = float(v)?,
                    S::Rebound => s.rebound = float(v)?,
                    S::InteractsWithActors => s.interacts_with_actors = i()?,
                    S::InteractsWithLevelMesh => s.interacts_with_level_mesh = i()?,
                    S::ActsAsActorSurface => s.acts_as_actor_surface = i()?,
                    S::Shape => s.shape = i()?,
                }
            }
            Member::Vector(f) => {
                let Some(Object::Facet(h, Facet::Velocity)) = object else {
                    return Err(HostFault::Nothing);
                };
                // Off his scripts his walk is the controller's (openspec
                // AH.151): the mover update's hand-over among these.
                if self.drops(at.placement, h, HeroWrite::Walk) {
                    return Ok(());
                }
                let x = float(v)?;
                let mut u = self.velocity(h, engine).ok_or(HostFault::Nothing)?;
                use VectorField as V;
                match f {
                    V::Length => {
                        let l = u.length();
                        u = if l > 0.0 { u * (x / l) } else { Vec3::ZERO };
                    }
                    V::Heading => {
                        let a = u.x.hypot(u.z);
                        let d = direction(x, 0.0);
                        u = Vec3::new(d.x * a, u.y, d.z * a);
                    }
                    V::Pitch => u = direction(heading_of(u), x) * u.length(),
                    V::VerticalDifference => u.y = x,
                    V::HorizontalLength => {
                        let a = u.x.hypot(u.z);
                        if a > 0.0 {
                            u.x *= x / a;
                            u.z *= x / a;
                        }
                    }
                }
                self.set_velocity(h, u, engine)?;
            }
            Member::ActorController => {
                let h = actor.ok_or(HostFault::Nothing)?;
                self.actors
                    .get_mut(&h)
                    .ok_or(HostFault::Nothing)?
                    .controller = v;
            }
            Member::AttachNode => {
                let h = actor.ok_or(HostFault::Nothing)?;
                // An actor's own ground isn't something it hangs from (where
                // it stands is found from where it is): the eruption's lava
                // is hung so as it spawns, and AH.54's recordings read its
                // hang null on every record while it stands on the ground.
                // Another actor's ground still takes it (the movement
                // update's helper, turned along its mover's slope).
                if self.object(Some(v)) == Some(Object::Facet(h, Facet::Ground)) {
                    return Ok(());
                }
                // Hung from a node, an actor sits at it.
                if let Some(at_node) = self.point(v, engine)
                    && !matches!(self.object(Some(v)), Some(Object::Facet(_, Facet::Ground)))
                {
                    self.set_position(at.placement, h, at_node, engine)?;
                }
                if let Some(a) = self.actors.get_mut(&h) {
                    a.attach = v;
                }
            }
            // On a clip (openspec AH.49): what a sequence started from it
            // starts with. The layered bit takes a write only on a clip,
            // never on a sequence playing (the original's setter tests the
            // object's parent, which only a started one has; aj-vm's
            // 0x36a4b8, read as an aid); `type` takes none.
            Member::SeqLooping | Member::Blend | Member::IsLayered
                if matches!(object, Some(Object::Clip { .. })) =>
            {
                let Some(Object::Clip { template, slot }) = object else {
                    return Err(HostFault::Nothing);
                };
                let state = self.clip_state.entry((template, slot)).or_default();
                match m {
                    Member::SeqLooping => {
                        state.looping = Some(v.as_i32().ok_or(HostFault::Nothing)? != 0);
                    }
                    Member::Blend => state.blend = Some(float(v)?),
                    _ => state.layered = Some(v.as_i32().ok_or(HostFault::Nothing)? != 0),
                }
            }
            Member::Blend => {
                let Some(Object::Sequence(i)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self
                    .sequences
                    .get_mut(i as usize)
                    .ok_or(HostFault::Nothing)?;
                s.blend = float(v)?;
            }
            Member::IsLayered | Member::ClipKind => {
                if !matches!(object, Some(Object::Sequence(_))) {
                    return Err(HostFault::Nothing);
                }
            }
            Member::SeqRate | Member::SeqLooping | Member::SeqPercentRemaining => {
                let Some(Object::Sequence(i)) = object else {
                    return Err(HostFault::Nothing);
                };
                let s = self
                    .sequences
                    .get_mut(i as usize)
                    .ok_or(HostFault::Nothing)?;
                match m {
                    Member::SeqRate => s.rate = float(v)?,
                    Member::SeqLooping => s.looping = v.as_i32().ok_or(HostFault::Nothing)? != 0,
                    _ => s.time = s.length * (1.0 - float(v)? * 0.01),
                }
            }
            Member::AllowRestart => {
                let on = v.as_i32().ok_or(HostFault::Nothing)? != 0;
                match object {
                    Some(Object::Clip { template, slot }) => {
                        self.clip_state
                            .entry((template, slot))
                            .or_default()
                            .allow_restart = Some(on);
                    }
                    Some(Object::Sequence(i)) => {
                        self.sequences
                            .get_mut(i as usize)
                            .ok_or(HostFault::Nothing)?
                            .allow_restart = on;
                    }
                    _ => return Err(HostFault::Nothing),
                }
            }
            Member::Distance
            | Member::HorizontalDistance
            | Member::VerticalOffset
            | Member::Heading
            | Member::Pitch => {
                let r = recv.ok_or(HostFault::Nothing)?;
                let o = at.other_point.ok_or(HostFault::Nothing)?;
                // An actor put from its own remembered point while none is
                // kept isn't moved, since that point reads where he stands
                // and each vertical-offset write would lift him again (the
                // hero's death actor flew him off the map so); the original
                // lifts him 3.44 m and back as his burst comes (AH.11's
                // `restart1`, `restart2`), its point kept and its ping-pong
                // slider returning (stand-in ledger: `death-hop`).
                let unkept = match self.object(Some(o)) {
                    Some(Object::Remembered(a)) => self
                        .actors
                        .get(&a)
                        .is_some_and(|k| k.remembered.is_none())
                        .then_some(a),
                    Some(Object::RememberedVia(a, c)) => {
                        (!self.kept_via.contains_key(&(a, c))).then_some(a)
                    }
                    _ => None,
                };
                if unkept.is_some() && unkept == self.handle(r) {
                    return Ok(());
                }
                let b = self.point(o, engine).ok_or(HostFault::Nothing)?;
                let a = match self.point(r, engine) {
                    Some(a) => a,
                    // A reference's own point not kept yet, or a waypoint
                    // a script made and hasn't put anywhere: put from the
                    // other point.
                    None if matches!(
                        self.handle(r).and_then(|h| self.objs.get(h)),
                        Some(Object::RememberedRef(_) | Object::Waypoint(_))
                    ) =>
                    {
                        b
                    }
                    None => return Err(HostFault::Nothing),
                };
                let x = float(v)?;
                let d = a - b;
                let to = match m {
                    Member::Distance => {
                        let l = d.length();
                        if l > 0.0 {
                            b + d * (x * METRES_PER_FOOT / l)
                        } else {
                            b
                        }
                    }
                    Member::HorizontalDistance => {
                        let l = d.x.hypot(d.z);
                        if l > 0.0 {
                            let k = x * METRES_PER_FOOT / l;
                            Vec3::new(b.x + d.x * k, a.y, b.z + d.z * k)
                        } else {
                            Vec3::new(b.x, a.y, b.z)
                        }
                    }
                    Member::VerticalOffset => Vec3::new(a.x, b.y + x * METRES_PER_FOOT, a.z),
                    Member::Heading => {
                        let l = d.x.hypot(d.z);
                        let u = direction(x, 0.0);
                        Vec3::new(b.x + u.x * l, a.y, b.z + u.z * l)
                    }
                    _ => b + direction(heading_of(d), x) * d.length(),
                };
                self.set_point(at.placement, r, to, engine)?;
            }
            Member::NormalizedAngle => self.normalized = normalized(float(v)?),
            // The gameplay camera's picture (openspec AG.28).
            Member::ViewportFov | Member::ViewportFarClip => {
                if recv.and_then(|r| self.handle(r)) != Some(self.viewport) {
                    return Err(HostFault::Nothing);
                }
                // Kept whoever writes them (the level's adventure data sets
                // the far clip as the level loads): the eye's place is what
                // gives the scripts the view.
                let x = float(v)?;
                match m {
                    Member::ViewportFov => self.view.fov = x,
                    _ => self.view.far_clip = x,
                }
            }
            // A point put where it lands on the screen (its lock type the
            // screen's): no camera script does, so it's not modelled.
            Member::ConverterScreen => return Err(HostFault::Unmodelled),
            // The rest are read-only: a write goes nowhere, as the
            // original's member with no setter.
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn elements_of(
        &mut self,
        at: &Access,
        m: Member,
        engine: &mut dyn Engine,
    ) -> Result<Vec<Value>, HostFault> {
        let recv = at.receiver;
        Ok(match m {
            Member::AttachNodes => {
                let h = self.actor_of(recv).ok_or(HostFault::Nothing)?;
                (0..NODES)
                    .map(|i| self.objs.value(Object::Node(h, i)))
                    .collect()
            }
            Member::AttachedObjects => {
                let r = recv.ok_or(HostFault::Nothing)?;
                self.actors
                    .iter()
                    .filter(|(_, a)| a.attach == r && !a.removed)
                    .map(|(h, _)| Value::Entity(*h))
                    .collect()
            }
            Member::Clones => {
                let h = self.actor_of(recv).ok_or(HostFault::Nothing)?;
                self.actors
                    .get(&h)
                    .map(|a| {
                        a.clones
                            .iter()
                            .filter(|c| self.actors.get(c).is_some_and(|a| !a.removed))
                            .map(|c| Value::Entity(*c))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            // The copies playing on him, not their clips (openspec AH.184:
            // the original's set holds the sequences its play started,
            // aj-vm's 0x6aa68 and 0x118b10, read as aids), a whole base
            // clip's at the front, the latest first, the rest after in the
            // order started (its insert at 0 and append).
            Member::PlayingAnimations => {
                let h = self.actor_of(recv).ok_or(HostFault::Nothing)?;
                let playing = |whole: bool| {
                    self.sequences
                        .iter()
                        .enumerate()
                        .filter(move |(_, s)| s.actor == h && s.plays() && s.is_layered() != whole)
                        .map(|(i, _)| Object::Sequence(i as u32))
                };
                let mut v: Vec<Object> = playing(true).collect();
                v.reverse();
                v.extend(playing(false));
                v.into_iter().map(|o| self.objs.value(o)).collect()
            }
            Member::PlayingSounds => Vec::new(),
            // The level's and the props' contacts, not another actor's.
            Member::EnvironmentContacts => {
                let Some(Object::Facet(h, Facet::Collision)) = self.object(recv) else {
                    return Err(HostFault::Nothing);
                };
                let level: Vec<u32> = self.actors.get(&h).map_or(Vec::new(), |a| {
                    (0..a.contacts.len() as u32)
                        .filter(|&i| engine.actor_of(a.contacts[i as usize].body).is_none())
                        .collect()
                });
                level
                    .into_iter()
                    .map(|i| self.objs.value(Object::Contact(h, i)))
                    .collect()
            }
            Member::Palette => {
                // The running placement's, as the static member reads it.
                let template = self.template_of(at.placement, engine);
                let lengths = template.and_then(|t| self.palettes.get(&t).map(|p| (t, p.len())));
                match lengths {
                    Some((t, n)) => (0..n as u32)
                        .map(|slot| self.objs.value(Object::Clip { template: t, slot }))
                        .collect(),
                    None => return Err(HostFault::Nothing),
                }
            }
            Member::Waypoints => {
                let h = self.actor_of(recv).ok_or(HostFault::Nothing)?;
                self.actors
                    .get(&h)
                    .ok_or(HostFault::Nothing)?
                    .waypoints
                    .clone()
            }
            Member::ContactsWithMe | Member::ObjectCollisions => {
                let Some(Object::Facet(h, Facet::Collision)) = self.object(recv) else {
                    return Err(HostFault::Nothing);
                };
                let Some((sa, pa)) = self.query(h, engine) else {
                    return Ok(Vec::new());
                };
                let mut near: Vec<(f32, Handle)> = Vec::new();
                let others: Vec<Handle> = self
                    .actors
                    .iter()
                    .filter(|(o, a)| **o != h && !a.removed && a.active && a.obj.is_some())
                    .map(|(o, _)| *o)
                    .chain(engine.heroes())
                    .collect();
                for o in others {
                    if o == h {
                        continue;
                    }
                    if let Some((sb, pb)) = self.query(o, engine) {
                        let g = guac_motion::parry::gap(&sa, pa, &sb, pb);
                        if g <= CONTACT_REACH {
                            near.push((g, o));
                        }
                    }
                }
                near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                near.dedup_by_key(|k| k.1);
                near.into_iter().map(|(_, o)| Value::Entity(o)).collect()
            }
            // Every hero in the level (openspec AG.28: the tether's check for
            // a second player), in handle order.
            Member::Players => {
                let mut v = engine.heroes();
                v.sort_unstable();
                v.into_iter().map(Value::Entity).collect()
            }
            Member::Inactive | Member::Removed => {
                let mut v: Vec<Handle> = self
                    .actors
                    .iter()
                    .filter(|(_, a)| match m {
                        Member::Inactive => a.managed && !a.removed && !a.active,
                        _ => a.removed,
                    })
                    .map(|(h, _)| *h)
                    .collect();
                v.sort_unstable();
                v.into_iter().map(Value::Entity).collect()
            }
            _ => return Err(HostFault::Unmodelled),
        })
    }

    /// The template an actor was made from (itself for a file's).
    fn template_of(&self, h: Handle, _engine: &mut dyn Engine) -> Option<Obj> {
        let a = self.actors.get(&h)?;
        a.template.or(a.obj)
    }

    /// A clip's length, seconds at 100 %: 0 for an empty slot or anything
    /// that isn't a clip (a sound plays nothing here).
    /// Slot `slot` of `template`'s palette: its file's animation info and
    /// what the scripts wrote on it (openspec AH.49).
    fn clip_flags(&self, template: Obj, slot: u32) -> (ClipInfo, ClipState) {
        (
            self.clip_info(template, slot),
            self.clip_state
                .get(&(template, slot))
                .copied()
                .unwrap_or_default(),
        )
    }

    /// A play of the sequence at `i` on `actor` (openspec AH.184): refused
    /// while it plays with time left and no `allow restart` (the op fails
    /// and its block is skipped), else started again from its start, his.
    fn start_again(&mut self, i: usize, actor: Handle) -> Answer {
        let s = &mut self.sequences[i];
        if s.refuses_restart() {
            return Answer::Skip;
        }
        s.actor = actor;
        s.restart();
        self.restarts.push(i as u32);
        Answer::Run {
            receiver: None,
            result: Some(self.objs.value(Object::Sequence(i as u32))),
        }
    }

    fn length_of(&self, v: Value) -> f32 {
        match self.object(Some(v)) {
            Some(Object::Clip { template, slot }) => self
                .palettes
                .get(&template)
                .and_then(|p| p.get(slot as usize).copied())
                .unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// The world ops AH.17's scripts reach past AH.16's: a sequence started
    /// and stopped, a displacement. `None` for an op this doesn't answer.
    pub(crate) fn op_hero(
        &mut self,
        call: &OpCall,
        _spawn: &mut Spawner<'_>,
        engine: &mut dyn Engine,
    ) -> Option<Result<Answer, HostFault>> {
        let first = call.operands.first().and_then(|(_, v)| *v);
        Some(match call.class {
            Class::StartSequence => {
                // A set and an index (`play palette [index]` written as the
                // op's own index operand): the set's element at it.
                let index = call
                    .operands
                    .iter()
                    .find(|(n, _)| *n == "_indexRHS")
                    .and_then(|(_, v)| *v)
                    .and_then(|v| v.as_i32());
                let indexed = call
                    .sets
                    .iter()
                    .find(|(n, _)| *n == "_LHS")
                    .and_then(|(_, e)| index.and_then(|i| e.get(usize::try_from(i).ok()?)))
                    .copied();
                let Some(template) = indexed.or(first) else {
                    return Some(Ok(Answer::Skip));
                };
                let Some(actor) = self.handle(call.receiver) else {
                    return Some(Ok(Answer::Skip));
                };
                // One copy per clip and actor (openspec AH.184): a sequence
                // played is itself while it's his or stopped (the original's
                // stop clears its activator), else its clip's on him; a clip
                // played on him while its copy plays is that copy.
                let mut template = template;
                if let Some(Object::Sequence(i)) = self.object(Some(template))
                    && let Some(s) = self.sequences.get(i as usize)
                {
                    if s.actor == actor || s.stopped {
                        return Some(Ok(self.start_again(i as usize, actor)));
                    }
                    template = s.template;
                }
                if matches!(self.object(Some(template)), Some(Object::Clip { .. }))
                    && let Some(i) = self
                        .sequences
                        .iter()
                        .rposition(|s| s.actor == actor && s.template == template && s.plays())
                {
                    return Some(Ok(self.start_again(i, actor)));
                }
                let length = self.length_of(template);
                // A clip's own looping, weight and layered bit (AH.49).
                let (info, state) = match self.object(Some(template)) {
                    Some(Object::Clip { template, slot }) => self.clip_flags(template, slot),
                    _ => (ClipInfo::default(), ClipState::default()),
                };
                self.sequences.push(Sequence {
                    actor,
                    template,
                    length,
                    time: 0.0,
                    rate: 100.0,
                    looping: state.looping.unwrap_or(false),
                    stopped: false,
                    done: false,
                    blend: state.blend.unwrap_or(FULL_BLEND),
                    kind: info.kind,
                    layered: state.layered.unwrap_or(info.layered),
                    allow_restart: state.allow_restart.unwrap_or(false),
                });
                let s = self
                    .objs
                    .value(Object::Sequence((self.sequences.len() - 1) as u32));
                Ok(Answer::Run {
                    receiver: None,
                    result: Some(s),
                })
            }
            Class::StopSequence => {
                // A sequence stops itself; a clip its copies on the
                // receiver, whose ever else plays on (openspec AH.184: the
                // stop hands the clip the receiver, aj-vm's 0x9ba54 and its
                // clones' stop at 0x9ac14, read as aids).
                match self.object(first) {
                    Some(Object::Sequence(i)) => {
                        if let Some(s) = self.sequences.get_mut(i as usize) {
                            s.stopped = true;
                        }
                    }
                    Some(Object::Clip { .. }) => {
                        let clip = first;
                        if let Some(actor) = self.handle(call.receiver) {
                            for s in self
                                .sequences
                                .iter_mut()
                                .filter(|s| s.actor == actor && Some(s.template) == clip)
                            {
                                s.stopped = true;
                            }
                        }
                    }
                    _ => {}
                }
                Ok(Answer::Done)
            }
            Class::Displace => {
                let read = |name: &str| {
                    call.operands
                        .iter()
                        .find(|(n, _)| *n == name)
                        .and_then(|(_, v)| *v)
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0)
                };
                let mode = call
                    .words
                    .iter()
                    .find(|(n, _)| *n == "_combineMode")
                    .map_or(0, |(_, w)| *w);
                let (len, head, pitch) = (read("_lenNP"), read("_headNP"), read("_pitchNP"));
                let Some(target) = first else {
                    return Some(Err(HostFault::Nothing));
                };
                Ok(
                    match self.displace(call.placement, target, mode, len, head, pitch, engine) {
                        Ok(()) => Answer::Done,
                        Err(e) => return Some(Err(e)),
                    },
                )
            }
            Class::MoveTo | Class::MoveFrom => self.op_move(call, engine),
            Class::TurnTo => self.op_turn(call, engine),
            Class::TeleportTo => self.op_teleport(call, engine),
            Class::CheckFov => self.op_fov(call, engine),
            _ => return None,
        })
    }

    /// `move to` and `move from` (openspec AH.18): the mover (the op's
    /// receiver) steered toward or away from a point, an actor, or along a
    /// path of waypoints, until within (beyond) the op's distance in feet
    /// across the ground; reached, the op's block runs and the engine has
    /// no destination. Along a path the actor walks to each waypoint in turn,
    /// the block running as it reaches each and the next its destination in
    /// the same frame (a reading: the engine keeps the path's place per
    /// actor; AH.10's Norticus turns for the next waypoint the frame he
    /// reaches one, never slowing, where a frame without a destination left
    /// him 0.07 m and a frame behind by the cave mouth).
    fn op_move(&mut self, call: &OpCall, engine: &mut dyn Engine) -> Result<Answer, HostFault> {
        let named = |n: &str| {
            call.operands
                .iter()
                .find(|(k, _)| *k == n)
                .and_then(|(_, v)| *v)
        };
        let mover = self
            .actor_of(Some(call.receiver))
            .ok_or(HostFault::Nothing)?;
        let distance = named("_RHS").and_then(|v| v.as_f32()).unwrap_or(0.0);
        let path = call
            .sets
            .iter()
            .find(|(n, _)| *n == "_LHS")
            .map(|(_, e)| e.clone());
        let at = self.position(mover, engine).ok_or(HostFault::Nothing)?;
        let toward = call.class == Class::MoveTo;
        let (target, last) = match &path {
            Some(p) if p.is_empty() => return Ok(Answer::Skip),
            Some(p) => {
                let i = self
                    .actors
                    .get(&mover)
                    .and_then(|a| a.walker.as_ref())
                    .map_or(0, |w| w.along)
                    .min(p.len() - 1);
                (p[i], i + 1 == p.len())
            }
            None => (named("_LHS").ok_or(HostFault::Nothing)?, true),
        };
        let point = self.point(target, engine).ok_or(HostFault::Nothing)?;
        let done = if toward {
            guac_motion::steer::reached(at, point, distance)
        } else {
            guac_motion::steer::cleared(at, point, distance)
        };
        let Some(w) = self
            .actors
            .get_mut(&mover)
            .and_then(|a| a.walker.as_deref_mut())
        else {
            // Only a creature the engine moves steers.
            return Err(HostFault::Unmodelled);
        };
        if done {
            w.aim = Aim::Still;
            if let Some(p) = &path
                && !last
            {
                w.along += 1;
                let next = p[w.along];
                if let Some(point) = self.point(next, engine)
                    && let Some(w) = self
                        .actors
                        .get_mut(&mover)
                        .and_then(|a| a.walker.as_deref_mut())
                {
                    w.aim = Aim::To(point);
                }
            }
            return Ok(Answer::Run {
                receiver: None,
                result: None,
            });
        }
        w.aim = if toward {
            Aim::To(point)
        } else {
            Aim::From(point)
        };
        Ok(Answer::Done)
    }

    /// `turn to`: the receiver turned toward a facing, degrees; the block
    /// runs once it faces it (a reading: within 0.01°). A creature turns at
    /// its turn rate in the engine's frame; anything else at once.
    fn op_turn(&mut self, call: &OpCall, engine: &mut dyn Engine) -> Result<Answer, HostFault> {
        let to = call
            .operands
            .iter()
            .find(|(k, _)| *k == "_facingRHS")
            .and_then(|(_, v)| *v)
            .and_then(|v| v.as_f32())
            .ok_or(HostFault::Nothing)?;
        let h = self
            .actor_of(Some(call.receiver))
            .ok_or(HostFault::Nothing)?;
        let facing = self.angles(h, engine).ok_or(HostFault::Nothing)?.facing;
        let faces = normalized(facing - to).abs() <= 0.01;
        if engine.hero(h).is_some() {
            // Off his scripts his facing is the controller's (openspec
            // AH.151): the turn runs on, as one made at once does.
            if !self.drops(call.placement, h, HeroWrite::Facing)
                && let Some(hero) = engine.hero(h)
            {
                hero.facing = to.rem_euclid(360.0);
            }
            return Ok(Answer::Run {
                receiver: None,
                result: None,
            });
        }
        let a = self.actors.get_mut(&h).ok_or(HostFault::Nothing)?;
        match a.walker.as_deref_mut() {
            Some(w) if !faces => {
                w.aim = Aim::Face(to);
                Ok(Answer::Done)
            }
            Some(w) => {
                w.aim = Aim::Still;
                Ok(Answer::Run {
                    receiver: None,
                    result: None,
                })
            }
            None => {
                a.angles.facing = to;
                Ok(Answer::Run {
                    receiver: None,
                    result: None,
                })
            }
        }
    }

    /// `teleport to`: the receiver set at a point (an actor's pivot, a
    /// waypoint) facing the op's facing, at once.
    fn op_teleport(&mut self, call: &OpCall, engine: &mut dyn Engine) -> Result<Answer, HostFault> {
        let named = |n: &str| {
            call.operands
                .iter()
                .find(|(k, _)| *k == n)
                .and_then(|(_, v)| *v)
        };
        let to = named("_LHS").ok_or(HostFault::Nothing)?;
        let p = self.point(to, engine).ok_or(HostFault::Nothing)?;
        let facing = named("_facingRHS").and_then(|v| v.as_f32());
        let h = self
            .actor_of(Some(call.receiver))
            .ok_or(HostFault::Nothing)?;
        if let Some(hero) = engine.hero(h) {
            hero.pivot = p;
            if let Some(f) = facing {
                hero.facing = f.rem_euclid(360.0);
            }
            hero.walk.velocity = Vec3::ZERO;
            // Off his scripts, the level setting him down still does
            // (openspec AH.151): counted.
            self.writes_place(call.placement, h);
            return Ok(Answer::Done);
        }
        let a = self.actors.get_mut(&h).ok_or(HostFault::Nothing)?;
        a.position = p;
        if let Some(f) = facing {
            a.angles.facing = f;
        }
        Ok(Answer::Done)
    }

    /// `check fov`: the targets (one, or a set's) inside the cone of the
    /// op's width centred on its facing from its point, whose distance in
    /// feet passes the op's comparison, nearest first (aj-vm's reading of
    /// the op; its `_mode` unread). The cone is across the ground, no ray
    /// asked: the original's routine casts none.
    fn op_fov(&mut self, call: &OpCall, engine: &mut dyn Engine) -> Result<Answer, HostFault> {
        let named = |n: &str| {
            call.operands
                .iter()
                .find(|(k, _)| *k == n)
                .and_then(|(_, v)| *v)
        };
        let word = |n: &str| call.words.iter().find(|(k, _)| *k == n).map(|(_, w)| *w);
        let from = named("_fromLHS").ok_or(HostFault::Nothing)?;
        let from = self.point(from, engine).ok_or(HostFault::Nothing)?;
        let facing = named("_RHSfacing").and_then(|v| v.as_f32()).unwrap_or(0.0);
        let width = named("_RHSfov").and_then(|v| v.as_f32()).unwrap_or(0.0);
        let limit = named("_RHS").unwrap_or(Value::Int(0));
        let code = word("_relOperator").unwrap_or(0);
        let targets = match call.sets.iter().find(|(n, _)| *n == "_LHS") {
            Some((_, e)) => e.clone(),
            None => named("_LHS").into_iter().collect(),
        };
        let mut found = Vec::new();
        for t in targets {
            let Some(p) = self.point(t, engine) else {
                continue;
            };
            let d = p - from;
            let off = normalized(heading_of(d) - facing).abs();
            let feet = (d.length_squared() + f32::MIN_POSITIVE).sqrt() * FEET_PER_METRE;
            let passes = guac_script::compare(code, Value::Float(feet), limit) == Some(true);
            if off <= width * 0.5 && passes {
                found.push((feet, t));
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Answer::Found(found.into_iter().map(|(_, t)| t).collect()))
    }

    /// OpDisplace by placement `by`: `len` feet along a heading and a
    /// pitch, added in the world's axes (mode 0), set outright (1) or added
    /// turned by the target's facing (2); on a velocity facet, ft/s.
    #[allow(clippy::too_many_arguments)]
    fn displace(
        &mut self,
        by: Handle,
        target: Value,
        mode: u32,
        len: f32,
        head: f32,
        pitch: f32,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        if let Some(Object::Facet(h, Facet::Velocity)) = self.object(Some(target)) {
            // Off his scripts his walk is the controller's (openspec
            // AH.151).
            if self.drops(by, h, HeroWrite::Walk) {
                return Ok(());
            }
            let mut v = self.velocity(h, engine).ok_or(HostFault::Nothing)?;
            let head = if mode == 2 {
                head + self.angles(h, engine).map_or(0.0, |a| a.facing)
            } else {
                head
            };
            let d = direction(head, pitch) * len;
            v = if mode == 1 { d } else { v + d };
            return self.set_velocity(h, v, engine);
        }
        let at = self.point(target, engine).ok_or(HostFault::Nothing)?;
        // Turned by the target's whole orientation, its tilt and roll too
        // (openspec AG.28: the camera's shake up is along the viewport's
        // up, which leans forward with its tilt: AG.13's rock slide
        // moves the eye along (-2, 1, 1) of its two axes' shake at 15
        // degrees facing and 23.36 down, where turned by the facing alone
        // it went (-1.9, 1, 0.5)).
        let d = if mode == 2 {
            let h = self.handle(target).ok_or(HostFault::Nothing)?;
            let a = self.angles(h, engine).unwrap_or_default();
            guac_motion::solid::euler([a.tilt, a.facing, a.roll]) * direction(head, pitch)
        } else {
            direction(head, pitch)
        } * (len * METRES_PER_FOOT);
        let to = if mode == 1 { d } else { at + d };
        self.set_point(by, target, to, engine)
    }
}

impl ScriptWorld {
    /// The lowest point of an actor's solid, metres, as it's posed; `None`
    /// for a hero (his pivot is his feet) or an actor with no solid. A hero
    /// is known by his controller, not by a missing actor: his placement has
    /// one, whose position is where he joined and never moves (decision 21:
    /// his place is his controller's), so read as his it held the script's
    /// separation at 0 wherever the ground stood above the spawn and no
    /// frame off it pulled his walk down (AG.90).
    fn bottom(&self, h: Handle, engine: &mut dyn Engine) -> Option<f32> {
        if engine.hero(h).is_some() {
            return None;
        }
        let a = self.actors.get(&h)?;
        let solid = a.havok()?;
        let reach = match solid.shape {
            Shape::Sphere { radius } => radius,
            Shape::Capsule {
                radius,
                half_segment,
            } => radius + half_segment,
            Shape::Box {
                half_extents,
                margin,
            } => half_extents.y + margin,
            Shape::Cylinder {
                half_height,
                margin,
                ..
            } => half_height + margin,
        };
        Some(a.position.y + solid.centre.y - reach)
    }

    /// Its solid as the seam's query shape, posed where it stands: a hero's
    /// his controller's.
    fn query(&self, h: Handle, engine: &mut dyn Engine) -> Option<(Shape, Pose)> {
        if let Some(hero) = engine.hero(h) {
            let solid = *hero.solid();
            return Some((
                solid.grown(0.0),
                solid.pose(Pose::new(hero.pivot, hero.rotation())),
            ));
        }
        let a = self.actors.get(&h)?;
        let solid = a.havok()?;
        let rotation = guac_motion::solid::euler([a.angles.tilt, a.angles.facing, a.angles.roll]);
        Some((
            solid.grown(0.0),
            solid.pose(Pose::new(a.position, rotation)),
        ))
    }

    /// `positionmeasurement.collision`: whether two actors' solids touch,
    /// one of them a body in the seam (a hero, a creature, a prop): the
    /// other's last move stopped on it, or its solid is within [`TOUCH`] of
    /// it where it stands (openspec AH.18: what a Chompy's bite reads). Two
    /// actors neither of which the seam holds touch when their solids are
    /// within a centimetre (AH.20); one with no solid isn't modelled.
    fn touching(&self, a: Handle, b: Handle, engine: &mut dyn Engine) -> Result<bool, HostFault> {
        let pair = [(a, b), (b, a)];
        let mut asked = false;
        // Two solids within a centimetre of each other touch (AH.18:
        // AH.10's 68 bites read at 5.54-6.01 ft between the middles of two
        // 3 ft spheres, gaps of -0.14 to +0.003 m).
        let near = match (self.query(a, engine), self.query(b, engine)) {
            (Some((sa, pa)), Some((sb, pb))) => {
                guac_motion::parry::gap(&sa, pa, &sb, pb) <= CONTACT_REACH
            }
            _ => false,
        };
        for (me, other) in pair {
            let Some(body) = engine.body(other) else {
                continue;
            };
            asked = true;
            let stopped = self
                .actors
                .get(&me)
                .is_some_and(|k| k.contacts.iter().any(|c| c.body == body));
            if stopped || near {
                return Ok(true);
            }
            let own: Vec<BodyId> = engine.body(me).into_iter().collect();
            if let Some((shape, pose)) = self.query(me, engine)
                && engine
                    .seam()
                    .contacts(&shape, pose, TOUCH, &own)
                    .iter()
                    .any(|p| p.body == body)
            {
                return Ok(true);
            }
        }
        if asked {
            Ok(false)
        } else if self.query(a, engine).is_some() && self.query(b, engine).is_some() {
            // Neither a body in the seam (a thrown ball against a breakable
            // the loot system listed, AH.20): their solids where they
            // stand.
            Ok(near)
        } else {
            Err(HostFault::Unmodelled)
        }
    }

    /// The engine's move for the actors the scripts set going, a frame of
    /// `dt` seconds: one hung from a node sits at it; one launched falls
    /// under its own gravity at its time scale and, with a solid that meets
    /// the level, sweeps it through the seam, stopping at what it hits and
    /// sliding along it, its contacts there kept for
    /// `collisioninfo.environment contacts`.
    pub(crate) fn move_actors(&mut self, dt: f32, engine: &mut dyn Engine) {
        self.move_walkers(dt, engine);
        let mut flying: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, a)| !a.removed && a.attach == Value::Null && a.launched)
            .map(|(&h, _)| h)
            .collect();
        flying.sort_unstable();
        for h in flying {
            let Some(a) = self.actors.get(&h) else {
                continue;
            };
            let dt = dt * a.physics.time_scale / 100.0;
            let mut v = a.velocity;
            if a.physics.gravity_acceleration > 0.0 {
                v.y -= a.physics.gravity_acceleration * dt;
                if a.physics.gravity_max_speed > 0.0 {
                    v.y = v.y.max(-a.physics.gravity_max_speed);
                }
            }
            let motion = v * (dt * METRES_PER_FOOT);
            let rotation =
                guac_motion::solid::euler([a.angles.tilt, a.angles.facing, a.angles.roll]);
            let placed = Pose::new(a.position, rotation);
            let solid = a.havok().filter(|_| a.solid.interacts_with_level_mesh != 0);
            let mut at = a.position + motion;
            let mut contacts = Vec::new();
            if let Some(solid) = solid {
                let shape = solid.grown(0.0);
                let from = solid.pose(placed);
                let own: Vec<BodyId> = engine.body(h).into_iter().collect();
                if let Some(cast) = engine.seam().cast(&shape, from, motion, CAST_SLOP, &own) {
                    at = a.position + motion * cast.fraction;
                    for p in &cast.planes {
                        let into = v.dot(p.normal);
                        if into < 0.0 {
                            v -= p.normal * into;
                        }
                    }
                }
                // What it touches where it ends up, at rest against it too.
                let end = solid.pose(Pose::new(at, rotation));
                for p in engine.seam().contacts(&shape, end, TOUCH, &own) {
                    contacts.push(Contact {
                        point: p.point,
                        normal: p.normal,
                        material: p.material,
                        body: p.body,
                    });
                }
            }
            if let Some(a) = self.actors.get_mut(&h) {
                a.position = at;
                a.velocity = v;
                a.contacts = contacts;
            }
        }
        self.place_hung_actors(engine);
    }

    /// Each actor hung from a node set at it, as the engine's frame leaves
    /// it: [`ScriptWorld::move_actors`]' last part, and what an embedder
    /// posing the nodes after that frame (openspec AH.50) runs again.
    pub(crate) fn place_hung_actors(&mut self, engine: &mut dyn Engine) {
        let mut hung: Vec<(Handle, Value)> = self
            .actors
            .iter()
            .filter(|(_, a)| !a.removed && a.attach != Value::Null)
            .map(|(&h, a)| (h, a.attach))
            .collect();
        hung.sort_unstable_by_key(|k| k.0);
        for (h, node) in hung {
            if matches!(
                self.object(Some(node)),
                Some(Object::Facet(_, Facet::Ground))
            ) {
                continue;
            }
            let turn = match self.object(Some(node)) {
                Some(Object::Node(owner, i)) if owner != h => {
                    self.node_turn(owner, i, engine, HUNG_DEPTH)
                }
                _ => None,
            };
            if let Some(p) = self.point(node, engine)
                && let Some(a) = self.actors.get_mut(&h)
            {
                a.position = p;
                if let Some(q) = turn {
                    a.angles = angles_of(q);
                }
            }
        }
    }
}

impl ScriptWorld {
    /// Makes the actor under `h` a creature the engine moves (openspec
    /// AH.18): from the next frame its controller body carries it by the
    /// velocity its scripts leave, or steers it where a `move to`, `move
    /// from` or `turn to` asked this tick. False for no such actor.
    pub fn walk(&mut self, h: Handle) -> bool {
        match self.actors.get_mut(&h) {
            Some(a) => {
                if a.walker.is_none() {
                    a.walker = Some(Box::new(Walker {
                        body: None,
                        aim: Aim::Still,
                        along: 0,
                    }));
                }
                true
            }
            None => false,
        }
    }

    /// Each creature's controller body, where it stands: what the embedder
    /// keeps in the seam so a hero's sweep meets it. (Named for the
    /// creatures since AG.27's walkers, the playable characters on their
    /// own scripts: AH.168.)
    pub fn creature_bodies(&self) -> Vec<(Handle, HavokSolid, Pose)> {
        let mut out: Vec<(Handle, HavokSolid, Pose)> = self
            .actors
            .iter()
            .filter(|(_, a)| a.walker.is_some() && !a.removed && a.active)
            .filter_map(|(&h, a)| {
                let solid = a.havok()?;
                let rotation = Quat::from_rotation_y(a.angles.facing.to_radians());
                Some((h, solid, solid.pose(Pose::new(a.position, rotation))))
            })
            .collect();
        out.sort_unstable_by_key(|k| k.0);
        out
    }

    /// The engine's frame for each awake creature not hung on a node: its
    /// controller body set where its scripts left it (made, or made again
    /// when its scripts change its solid), then the engine's velocity model
    /// with its movement numbers, fed this tick's aim (none when no script
    /// asked: the deceleration still takes its share), and the move; the aim
    /// spent. AH.10's Chompies show it: a lunge the scripts set at 35 ft/s
    /// moves at 33.7 and slows through the recovery, and back in 'battle'
    /// a `move to` speeds it up 1.33 ft/s a frame to 20.0.
    fn move_walkers(&mut self, dt: f32, engine: &mut dyn Engine) {
        let mut walkers: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, a)| {
                a.walker.is_some() && !a.removed && a.active && a.attach == Value::Null
            })
            .map(|(h, _)| *h)
            .collect();
        walkers.sort_unstable();
        for h in walkers {
            let own: Vec<BodyId> = engine.body(h).into_iter().collect();
            let Some(a) = self.actors.get_mut(&h) else {
                continue;
            };
            let solid = a.havok().filter(|_| a.solid.interacts_with_level_mesh != 0);
            let dt = dt * a.physics.time_scale / 100.0;
            let physics = steer_physics(&a.physics);
            let (position, facing, velocity) = (a.position, a.angles.facing, a.velocity);
            let Some(w) = a.walker.as_mut() else {
                continue;
            };
            let aim = std::mem::take(&mut w.aim);
            let Some(solid) = solid else {
                // Nothing to sweep: it moves by its velocity as it is.
                a.position += a.velocity * (dt * METRES_PER_FOOT);
                continue;
            };
            if w.body.as_ref().is_none_or(|b| *b.solid() != solid) {
                let mut b = Hero::actor(solid, position, facing);
                b.settle(engine.seam(), &own);
                w.body = Some(b);
            }
            let Some(b) = w.body.as_mut() else {
                continue;
            };
            b.pivot = position;
            b.facing = facing;
            b.walk.velocity = velocity;
            b.step_steered(engine.seam(), &physics, aim, dt, &own, Surfaces::NONE);
            let (p, f, v) = (b.pivot, b.facing, b.walk.velocity);
            a.position = p;
            a.angles.facing = f;
            a.velocity = v;
        }
    }
}

/// An actor's movement numbers as the engine's steering reads them.
fn steer_physics(p: &crate::script_engine::Physics) -> SteerPhysics {
    SteerPhysics {
        ground_max_speed: p.ground_max_speed,
        ground_acceleration: p.ground_acceleration,
        ground_deceleration: p.ground_deceleration,
        ground_turn_rate: p.ground_turn_rate,
        air_max_speed: p.air_max_speed,
        air_acceleration: p.air_acceleration,
        air_deceleration: p.air_deceleration,
        air_turn_rate: p.air_turn_rate,
        gravity_acceleration: p.gravity_acceleration,
        gravity_max_speed: p.gravity_max_speed,
        move_type: MoveType::from_value(p.move_type),
    }
}

/// How near a solid counts as touching for its contacts: twice what a cast
/// leaves between a shape and what stopped it.
const TOUCH: f32 = 2.0 * CAST_SLOP;

/// How near two actors' solids touch, metres, what
/// `positionmeasurement.collision` and `collisioninfo.contacts with me` read:
/// a centimetre (AH.10's bites at most 0.003 m apart, a hypothesis past it).
pub const CONTACT_REACH: f32 = 0.01;

/// How many actors hung one from another's node a position read follows:
/// the disc hangs a clone on a clone at most (Gill Grunt's spear on his
/// gun, AE.23), and a loop of them stops here.
const HUNG_DEPTH: u8 = 8;

/// How many attach nodes an actor's set holds: the scripts index up to 13
/// on a hero. A node sits where [`ScriptWorld::set_nodes`] put it (a hero's
/// off his model's attach list, AH.50), else at the actor's pivot
/// (stand-in ledger: `nodes`).
pub const NODES: u32 = 32;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use guac_motion::hero::Hero;
    use guac_motion::parry::ParryWorld;
    use guac_motion::proxy::Surfaces;
    use guac_motion::seam::{Body, BodyKind, Collider, PhysicsSeam};
    use guac_motion::solid::MESH_RADIUS;
    use guac_motion::tuning::{Tuning, eruptor_solid};
    use guac_script::MemberId;

    use super::*;
    use crate::script_engine::{Actor, NodePose, Pad, Physics, Solid};
    use crate::scripts::{Files, headless_objects};

    const BALL: Handle = 0x6000_0001;
    const CHOMPY: Handle = 0x6000_0002;
    const HERO: Handle = 0x6000_0003;
    const STEP: f32 = 1.0 / 60.0;

    /// A floor whose top is y = 0, a wall whose face is z = 2.5, and a
    /// creature's capsule at x = 4: the parry side of the seam.
    struct Bench {
        world: ParryWorld,
        hero: Option<Hero>,
    }

    impl Engine for Bench {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            self.hero.as_mut().filter(|_| h == HERO)
        }
        fn heroes(&self) -> Vec<Handle> {
            self.hero.iter().map(|_| HERO).collect()
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.world
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
        fn body(&self, actor: Handle) -> Option<BodyId> {
            (actor == CHOMPY).then_some(BodyId(3))
        }
        fn actor_of(&self, body: BodyId) -> Option<Handle> {
            (body == BodyId(3)).then_some(CHOMPY)
        }
    }

    fn bench() -> Bench {
        let convex = |id, kind, at: Vec3, shape| Body {
            id: BodyId(id),
            kind,
            pose: Pose::at(at),
            collider: Collider::Convex(shape),
        };
        let floor = Shape::Box {
            half_extents: Vec3::new(20.0, 0.5, 20.0),
            margin: 0.0,
        };
        let wall = Shape::Box {
            half_extents: Vec3::new(1.0, 5.0, 0.5),
            margin: 0.0,
        };
        let chompy = Shape::Capsule {
            radius: 0.5,
            half_segment: 0.5,
        };
        let world = ParryWorld::new(&[
            convex(1, BodyKind::Static, Vec3::new(0.0, -0.5, 0.0), floor),
            convex(2, BodyKind::Static, Vec3::new(0.0, 1.0, 3.0), wall),
            convex(3, BodyKind::Kinematic, Vec3::new(4.0, 1.0, 3.0), chompy),
        ])
        .unwrap();
        Bench { world, hero: None }
    }

    /// The lava ball's numbers (`009_LavaProjectile`): a 5 ft sphere whose
    /// centre is its pivot, meeting the level, 200 ft/s² of gravity.
    fn ball(at: Vec3, gravity: f32) -> Actor {
        Actor {
            position: at,
            physics: Physics {
                gravity_acceleration: gravity,
                gravity_max_speed: 9999.0,
                time_scale: 100.0,
                ..Physics::default()
            },
            solid: Solid {
                width: 5.0,
                length: 3.0,
                height: 6.0,
                offset: [0.0, -0.762, 0.0],
                interacts_with_level_mesh: 1,
                shape: 3,
                ..Solid::default()
            },
            ..Actor::blank()
        }
    }

    fn world() -> ScriptWorld {
        let files = Files {
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: Vec::new(),
            ids: HashMap::new(),
            points: HashMap::new(),
        };
        let handles = HashMap::new();
        ScriptWorld::new(&files, &handles, headless_objects(&handles))
    }

    fn at(ns: &str, name: &str, receiver: Option<Value>, other: Option<Value>) -> Access {
        Access {
            placement: 1,
            receiver,
            member: MemberId::named(ns, name),
            other_point: other,
            via: None,
            op: None,
        }
    }

    fn fly(w: &mut ScriptWorld, e: &mut Bench, frames: u32) {
        for _ in 0..frames {
            w.advance(STEP, e);
        }
    }

    #[test]
    fn a_thrown_ball_falls_by_its_gravity_at_its_time_scale() {
        let mut e = bench();
        let mut w = world();
        w.actors
            .insert(BALL, ball(Vec3::new(-5.0, 5.0, -5.0), 200.0));
        w.set_velocity(BALL, Vec3::new(0.0, 60.0, 0.0), &mut e)
            .unwrap();
        fly(&mut w, &mut e, 1);
        let a = w.actor(BALL).unwrap();
        assert!((a.velocity.y - (60.0 - 200.0 / 60.0)).abs() < 1e-4);
        // Velocity first, then the move: 56.67 ft/s for a sixtieth.
        let rise = (60.0 - 200.0 / 60.0) * STEP * METRES_PER_FOOT;
        assert!((a.position.y - (5.0 + rise)).abs() < 1e-5);
        // At half its time scale a frame is half as long.
        w.actors.get_mut(&BALL).unwrap().physics.time_scale = 50.0;
        let before = w.actor(BALL).unwrap().velocity.y;
        fly(&mut w, &mut e, 1);
        let after = w.actor(BALL).unwrap().velocity.y;
        assert!((before - after - 100.0 / 60.0).abs() < 1e-4);
    }

    #[test]
    fn a_balls_cast_stops_on_a_wall_and_the_wall_is_its_contact() {
        let mut e = bench();
        let mut w = world();
        w.actors.insert(BALL, ball(Vec3::new(0.0, 1.5, 0.0), 0.0));
        w.set_velocity(BALL, Vec3::new(0.0, 0.0, 30.0), &mut e)
            .unwrap();
        fly(&mut w, &mut e, 30);
        let a = w.actor(BALL).unwrap();
        // Its solid as the original builds it, 0.712 m, and the level's
        // 5 cm skin: the centre stops 0.762 m short of the face.
        let reach = a.havok().unwrap().grown(0.0);
        let Shape::Sphere { radius } = reach else {
            panic!("{reach:?}")
        };
        assert!((radius - (0.712 + MESH_RADIUS)).abs() < 1e-5);
        assert!(
            (a.position.z - (2.5 - radius)).abs() < 2e-3,
            "{}",
            a.position.z
        );
        assert_eq!(a.contacts.len(), 1);
        let c = a.contacts[0];
        assert_eq!(c.body, BodyId(2));
        assert!(c.normal.dot(Vec3::NEG_Z) > 0.999);
        // Slid along it: nothing left into the wall.
        assert!(a.velocity.z.abs() < 1e-3);
        let collision = w.objs.value(Object::Facet(BALL, Facet::Collision));
        let contacts = w
            .elements_of(
                &at(
                    "collisioninfo",
                    "environment contacts",
                    Some(collision),
                    None,
                ),
                Member::EnvironmentContacts,
                &mut e,
            )
            .unwrap();
        assert_eq!(contacts.len(), 1);
        let normal = w
            .get_member(
                &at("contactinfo", "normal", Some(contacts[0]), None),
                Member::ContactNormal,
                &mut e,
            )
            .unwrap();
        let n = w.vector(w.object(Some(normal)).unwrap(), &mut e).unwrap();
        assert!(n.dot(Vec3::NEG_Z) > 0.999);
    }

    #[test]
    fn a_balls_cast_stops_on_a_creature_and_reads_as_its_collision() {
        let mut e = bench();
        let mut w = world();
        w.actors.insert(BALL, ball(Vec3::new(4.0, 1.0, 0.0), 0.0));
        w.actors.insert(CHOMPY, Actor::blank());
        w.set_velocity(BALL, Vec3::new(0.0, 0.0, 30.0), &mut e)
            .unwrap();
        let collision = |w: &mut ScriptWorld, e: &mut Bench| {
            w.get_member(
                &at(
                    "positionmeasurement",
                    "collision",
                    Some(Value::Entity(CHOMPY)),
                    Some(Value::Entity(BALL)),
                ),
                Member::Collision,
                e,
            )
        };
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)));
        fly(&mut w, &mut e, 30);
        let a = w.actor(BALL).unwrap();
        // The capsule's face is 0.5 m from its axis at z = 3.
        assert!(
            (a.position.z - (2.5 - 0.762)).abs() < 2e-3,
            "{}",
            a.position.z
        );
        assert_eq!(a.contacts.first().map(|c| c.body), Some(BodyId(3)));
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(1)));
        // Not the level's: no environment contact.
        let facet = w.objs.value(Object::Facet(BALL, Facet::Collision));
        let contacts = w
            .elements_of(
                &at("collisioninfo", "environment contacts", Some(facet), None),
                Member::EnvironmentContacts,
                &mut e,
            )
            .unwrap();
        assert!(contacts.is_empty());
        // Two actors the seam holds neither of touch by their solids
        // (AH.20): these 5 m apart don't.
        w.actors
            .insert(0x6000_0009, ball(Vec3::new(9.0, 1.0, 0.0), 0.0));
        let neither = w.get_member(
            &at(
                "positionmeasurement",
                "collision",
                Some(Value::Entity(0x6000_0009)),
                Some(Value::Entity(BALL)),
            ),
            Member::Collision,
            &mut e,
        );
        assert_eq!(neither, Ok(Value::Int(0)));
    }

    /// A solid's offset is metres in its file and feet at its members, as
    /// the original's getter (× 3.28084) and setter (× 0.3048) convert:
    /// the ball's −0.762 m reads −2.5 ft, and 1 ft written puts its centre
    /// 0.3048 m over its pivot plus its half width.
    #[test]
    fn a_solids_offset_reads_and_writes_in_feet() {
        let mut e = bench();
        let mut w = world();
        w.actors.insert(BALL, ball(Vec3::new(0.0, 1.0, 0.0), 0.0));
        let facet = w.objs.value(Object::Facet(BALL, Facet::Collision));
        let y = at("collisioninfo", "local y offset", Some(facet), None);
        let read = w
            .get_member(&y, Member::Solid(SolidField::Offset(1)), &mut e)
            .unwrap();
        let Value::Float(ft) = read else {
            panic!("{read:?}")
        };
        assert!((ft + 2.5).abs() < 1e-4, "{ft}");
        w.set_member(
            &y,
            Member::Solid(SolidField::Offset(1)),
            Value::Float(1.0),
            &mut e,
        )
        .unwrap();
        let a = w.actor(BALL).unwrap();
        assert!((a.solid.offset[1] - METRES_PER_FOOT).abs() < 1e-6);
        let centre = a.havok().unwrap().centre.y;
        assert!(
            (centre - (0.762 + METRES_PER_FOOT)).abs() < 1e-5,
            "{centre}"
        );
    }

    /// AG.170: a solid's own turn reads and writes through its collision
    /// volume's orientation, as `Set Actor Collision Settings` tips a
    /// character's capsule onto its side, apart from its actor's turn: its
    /// file's angles read back, a tilt written tips the solid alone, and
    /// the relative angles stay unmodelled there.
    #[test]
    fn a_solids_own_turn_is_its_collision_volumes_orientation() {
        let mut e = bench();
        let mut w = world();
        let mut ball = ball(Vec3::ZERO, 0.0);
        ball.solid.angles = [0.0, 90.0, 0.0];
        w.actors.insert(BALL, ball);
        let facet = w.objs.value(Object::Facet(BALL, Facet::Collision));
        let turn = w
            .get_member(
                &at("matrixmeasurement", "orientation", Some(facet), None),
                Member::Orientation,
                &mut e,
            )
            .unwrap();
        assert_eq!(
            w.object(Some(turn)),
            Some(Object::Facet(BALL, Facet::SolidTurn))
        );
        let tilt = at("orientationmeasurement", "tilt", Some(turn), None);
        let facing = at("orientationmeasurement", "facing", Some(turn), None);
        assert_eq!(
            w.get_member(&facing, Member::Facing, &mut e),
            Ok(Value::Float(90.0)),
            "its file's"
        );
        w.set_member(&tilt, Member::Tilt, Value::Float(-90.0), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap();
        assert_eq!(a.solid.angles, [-90.0, 90.0, 0.0]);
        assert_eq!(a.angles, Angles::default(), "its actor unturned");
        assert_eq!(
            w.get_member(&tilt, Member::Tilt, &mut e),
            Ok(Value::Float(-90.0))
        );
        let relative = at("orientationmeasurement", "relative tilt", Some(turn), None);
        assert_eq!(
            w.get_member(&relative, Member::RelativeTilt, &mut e),
            Err(HostFault::Nothing)
        );
    }

    /// The two members a breakable's debris reach that no earlier root's
    /// run did (openspec AH.21): a piece's relative roll reads its roll and
    /// a write turns it, in degrees and unclamped as registered (the debris'
    /// roll eased 20-40° on from its 45°); its solid's rebound reads its
    /// file's percent and keeps a write.
    #[test]
    fn a_pieces_relative_roll_and_its_solids_rebound() {
        use guac_script::Kind;
        use guac_script::members::{HERO as TABLE, find};
        let mut e = bench();
        let mut w = world();
        let mut piece = ball(Vec3::ZERO, 100.0);
        piece.angles.roll = 45.0;
        piece.solid.rebound = 25.0;
        w.actors.insert(BALL, piece);
        let turn = w.objs.value(Object::Facet(BALL, Facet::Orientation));
        let roll = at("orientationmeasurement", "relative roll", Some(turn), None);
        assert_eq!(
            w.get_member(&roll, Member::RelativeRoll, &mut e),
            Ok(Value::Float(45.0))
        );
        w.set_member(&roll, Member::RelativeRoll, Value::Float(83.5), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap();
        assert_eq!(
            (a.angles.roll, a.angles.tilt, a.angles.facing),
            (83.5, 0.0, 0.0)
        );
        let facet = w.objs.value(Object::Facet(BALL, Facet::Collision));
        let rebound = at("collisioninfo", "rebound", Some(facet), None);
        let field = Member::Solid(SolidField::Rebound);
        assert_eq!(
            w.get_member(&rebound, field, &mut e),
            Ok(Value::Float(25.0))
        );
        w.set_member(&rebound, field, Value::Float(0.0), &mut e)
            .unwrap();
        assert_eq!(w.actor(BALL).unwrap().solid.rebound, 0.0);
        let r = find(TABLE, "orientationmeasurement", "relative roll").unwrap();
        assert_eq!((r.units, r.kind()), ("degrees", Kind::Float));
        assert_eq!(
            guac_script::convert(&r.spec(), Value::Int(400)),
            Some(Value::Float(400.0)),
            "open both ways"
        );
        let r = find(TABLE, "collisioninfo", "rebound").unwrap();
        assert_eq!((r.units, r.kind()), ("percent", Kind::Float));
        assert_eq!(
            guac_script::convert(&r.spec(), Value::Float(-5.0)),
            Some(Value::Float(-5.0))
        );
    }

    #[test]
    fn separation_from_the_ground_is_the_gap_under_the_solid_in_feet() {
        let mut e = bench();
        let mut w = world();
        // Centre 1 m up: the 0.712 m sphere's bottom is 0.288 m off the floor.
        w.actors.insert(BALL, ball(Vec3::new(-5.0, 1.0, -5.0), 0.0));
        let ground = w.objs.value(Object::Facet(BALL, Facet::Ground));
        let gap = w
            .get_member(
                &at(
                    "positionmeasurement",
                    "separation",
                    Some(Value::Entity(BALL)),
                    Some(ground),
                ),
                Member::Separation,
                &mut e,
            )
            .unwrap();
        let Value::Float(ft) = gap else {
            panic!("{gap:?}")
        };
        assert!((ft - 0.288 * FEET_PER_METRE).abs() < 1e-3, "{ft}");
    }

    #[test]
    fn origin_stays_where_it_was_placed_and_a_reference_keeps_its_own_point() {
        let mut e = bench();
        let mut w = world();
        let mut a = ball(Vec3::new(1.0, 2.0, 3.0), 0.0);
        a.origin = a.position;
        w.actors.insert(BALL, a);
        w.set_velocity(BALL, Vec3::new(30.0, 0.0, 0.0), &mut e)
            .unwrap();
        fly(&mut w, &mut e, 10);
        let origin = w
            .get_member(
                &at(
                    "abstractplacement",
                    "origin",
                    Some(Value::Entity(BALL)),
                    None,
                ),
                Member::Origin,
                &mut e,
            )
            .unwrap();
        assert_eq!(w.point(origin, &mut e), Some(Vec3::new(1.0, 2.0, 3.0)));
        let kept = w
            .get_member(
                &at(
                    "placementreference",
                    "remembered",
                    Some(Value::Entity(BALL)),
                    None,
                ),
                Member::Remembered,
                &mut e,
            )
            .unwrap();
        let now = w.actor(BALL).unwrap().position;
        assert_eq!(w.point(kept, &mut e), Some(now));
        // Set 10 ft from the origin, it stays there while the ball moves on.
        w.set_member(
            &at("positionmeasurement", "distance", Some(kept), Some(origin)),
            Member::Distance,
            Value::Float(10.0),
            &mut e,
        )
        .unwrap();
        let p = w.point(kept, &mut e).unwrap();
        assert!(((p - Vec3::new(1.0, 2.0, 3.0)).length() - 10.0 * METRES_PER_FOOT).abs() < 1e-5);
        fly(&mut w, &mut e, 5);
        assert_eq!(w.point(kept, &mut e), Some(p));
    }

    /// AH.20's points: a reference naming nothing keeps a point of its own,
    /// asked on its cell and put from the other point the first time; a
    /// waypoint a script makes is a point of its own, nowhere until put;
    /// any other class makes none.
    #[test]
    fn a_reference_naming_nothing_and_a_made_waypoint_keep_their_points() {
        let mut e = bench();
        let mut w = world();
        let target = Vec3::new(3.0, 0.0, 4.0);
        w.actors.insert(BALL, ball(target, 0.0));
        let remembered = |w: &mut ScriptWorld, e: &mut Bench, cell: u32| {
            w.get_member(
                &at(
                    "placementreference",
                    "remembered",
                    Some(Value::Cell(cell)),
                    None,
                ),
                Member::Remembered,
                e,
            )
            .unwrap()
        };
        let kept = remembered(&mut w, &mut e, 7);
        assert_eq!(w.point(kept, &mut e), None, "nowhere yet");
        w.set_member(
            &at(
                "positionmeasurement",
                "distance",
                Some(kept),
                Some(Value::Entity(BALL)),
            ),
            Member::Distance,
            Value::Float(0.0),
            &mut e,
        )
        .unwrap();
        assert_eq!(w.point(kept, &mut e), Some(target));
        assert_eq!(remembered(&mut w, &mut e, 7), kept, "the same reference");
        let other = remembered(&mut w, &mut e, 8);
        assert_ne!(other, kept);
        assert_eq!(w.point(other, &mut e), None, "another reference's own");
        let a = w.make("actorwaypoint").unwrap();
        let b = w.make("actorwaypoint").unwrap();
        assert_ne!(a, b);
        assert_eq!(w.point(a, &mut e), None);
        w.set_member(
            &at(
                "positionmeasurement",
                "vertical offset",
                Some(a),
                Some(Value::Entity(BALL)),
            ),
            Member::VerticalOffset,
            Value::Float(2.5),
            &mut e,
        )
        .unwrap();
        let p = w.point(a, &mut e).unwrap();
        assert!((p - (target + Vec3::Y * 2.5 * METRES_PER_FOOT)).length() < 1e-5);
        assert_eq!(w.point(b, &mut e), None, "the other made one untouched");
        assert_eq!(w.make("spriteinfo"), None);
    }

    /// The hero's death actor lifts the dying minion 12 ft over his own
    /// remembered point a tick at a time (AH.39's Mac step): with none kept
    /// that point reads where he stands, so each write would lift him the
    /// whole way again, and he isn't moved (stand-in ledger: `death-hop`).
    /// Another actor's unkept point
    /// still puts him from where that actor stands; once his own is kept the
    /// write puts him over it, the same each time. By the actor's own point
    /// and through a reference to it alike.
    #[test]
    fn an_actor_put_from_its_own_unkept_remembered_point_stays_put() {
        const OTHER: Handle = 0x7777;
        let mut e = bench();
        let mut w = world();
        let here = Vec3::new(3.0, 1.0, 4.0);
        let there = Vec3::new(-2.0, 5.0, 0.0);
        w.actors.insert(BALL, ball(here, 0.0));
        w.actors.insert(OTHER, ball(there, 0.0));
        let remembered = |w: &mut ScriptWorld, e: &mut Bench, of: Handle, via: Option<u32>| {
            let mut a = at(
                "placementreference",
                "remembered",
                Some(Value::Entity(of)),
                None,
            );
            a.via = via;
            w.get_member(&a, Member::Remembered, e).unwrap()
        };
        let lift = |w: &mut ScriptWorld, e: &mut Bench, from: Value| {
            w.set_member(
                &at(
                    "positionmeasurement",
                    "vertical offset",
                    Some(Value::Entity(BALL)),
                    Some(from),
                ),
                Member::VerticalOffset,
                Value::Float(12.0),
                e,
            )
            .unwrap();
            w.actor(BALL).unwrap().position
        };
        let over = |p: Vec3| Vec3::new(here.x, p.y + 12.0 * METRES_PER_FOOT, here.z);
        for via in [None, Some(9)] {
            let own = remembered(&mut w, &mut e, BALL, via);
            for _ in 0..3 {
                assert_eq!(lift(&mut w, &mut e, own), here, "{via:?}: not moved");
            }
            let theirs = remembered(&mut w, &mut e, OTHER, via);
            assert!((lift(&mut w, &mut e, theirs) - over(there)).length() < 1e-5);
            w.actors.get_mut(&BALL).unwrap().position = here;
            // His own kept where he stands: over it, the same each time.
            w.set_member(
                &at(
                    "positionmeasurement",
                    "distance",
                    Some(own),
                    Some(Value::Entity(BALL)),
                ),
                Member::Distance,
                Value::Float(0.0),
                &mut e,
            )
            .unwrap();
            for _ in 0..2 {
                assert!(
                    (lift(&mut w, &mut e, own) - over(here)).length() < 1e-5,
                    "{via:?}"
                );
            }
            w.actors.get_mut(&BALL).unwrap().position = here;
            w.actors.get_mut(&BALL).unwrap().remembered = None;
        }
    }

    /// `system.absolute zero` is the world's origin as a point: what a
    /// hero's death measures from (AH.20).
    #[test]
    fn absolute_zero_is_the_worlds_origin() {
        let mut e = bench();
        let mut w = world();
        let zero = w
            .get_member(
                &at("system", "absolute zero", None, None),
                Member::AbsoluteZero,
                &mut e,
            )
            .unwrap();
        assert_eq!(w.point(zero, &mut e), Some(Vec3::ZERO));
        w.actors.insert(BALL, ball(Vec3::new(3.0, 0.0, 4.0), 0.0));
        let d = w
            .get_member(
                &at(
                    "positionmeasurement",
                    "distance",
                    Some(Value::Entity(BALL)),
                    Some(zero),
                ),
                Member::Distance,
                &mut e,
            )
            .unwrap();
        assert!(
            (d.as_f32().unwrap() - 5.0 * FEET_PER_METRE).abs() < 1e-4,
            "{d:?}"
        );
    }

    /// Against the gameplay camera every point is on screen (AH.20, the
    /// orbs' idle bounce its reader; stand-in ledger: `on-screen`), near or
    /// far.
    #[test]
    fn every_point_is_on_screen_to_the_gameplay_camera() {
        let mut e = bench();
        let mut w = world();
        for at_x in [0.0, 500.0] {
            w.actors.insert(BALL, ball(Vec3::new(at_x, 0.0, 0.0), 0.0));
            let c = w
                .get_member(
                    &at(
                        "positionmeasurement",
                        "collision",
                        Some(Value::Entity(BALL)),
                        Some(Value::Entity(w.viewport)),
                    ),
                    Member::Collision,
                    &mut e,
                )
                .unwrap();
            assert_eq!(c, Value::Int(1));
        }
    }

    /// AH.150: an ask read as gating play is answered off the view the
    /// camera's scripts leave while they drive it, on screen ahead of the
    /// eye and off screen behind it or past its side; any other op's ask,
    /// and the same op's before the scripts hold the view or once it's
    /// handed back, reads on screen for every point (AH.20).
    #[test]
    fn a_gating_ask_is_answered_off_the_scripts_view() {
        const ASK: guac_script::OpId = 77;
        let mut e = bench();
        let mut w = world();
        w.answer_from_view([ASK]);
        let ask = |w: &mut ScriptWorld, e: &mut Bench, op: Option<guac_script::OpId>| {
            let at = Access {
                op,
                ..at(
                    "positionmeasurement",
                    "collision",
                    Some(Value::Entity(BALL)),
                    Some(Value::Entity(w.viewport)),
                )
            };
            w.get_member(&at, Member::Collision, e).unwrap()
        };
        let place = |w: &mut ScriptWorld, p: Vec3| {
            w.actors.insert(BALL, ball(p, 0.0));
        };
        // Before the scripts place the view: every point.
        place(&mut w, Vec3::new(0.0, 0.0, -50.0));
        assert_eq!(ask(&mut w, &mut e, Some(ASK)), Value::Int(1), "engine's");
        // The scripts' view: from 10 m back looking along +z, level.
        w.view.take();
        w.view.eye = Vec3::new(0.0, 1.0, -10.0);
        w.view.angles = Angles {
            facing: 0.0,
            tilt: 0.0,
            roll: 0.0,
        };
        w.view.far_clip = 500.0;
        place(&mut w, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(ask(&mut w, &mut e, Some(ASK)), Value::Int(1), "ahead");
        place(&mut w, Vec3::new(0.0, 1.0, -20.0));
        assert_eq!(ask(&mut w, &mut e, Some(ASK)), Value::Int(0), "behind");
        place(&mut w, Vec3::new(30.0, 1.0, 0.0));
        assert_eq!(ask(&mut w, &mut e, Some(ASK)), Value::Int(0), "aside");
        assert_eq!(ask(&mut w, &mut e, Some(ASK + 1)), Value::Int(1), "unread");
        assert_eq!(ask(&mut w, &mut e, None), Value::Int(1), "no op");
        w.hand_back_view();
        assert_eq!(ask(&mut w, &mut e, Some(ASK)), Value::Int(1), "handed back");
    }

    /// Two actors the seam doesn't hold (a thrown ball and a breakable the
    /// loot system lists, AH.20) touch by their solids where they stand;
    /// one with no solid isn't modelled.
    #[test]
    fn two_actors_off_the_seam_touch_by_their_solids() {
        let mut e = bench();
        let mut w = world();
        const OTHER: Handle = 0x6000_0009;
        let here = Vec3::new(10.0, 3.0, -5.0);
        w.actors.insert(BALL, ball(here, 0.0));
        let collision = |w: &mut ScriptWorld, e: &mut Bench| {
            w.get_member(
                &at(
                    "positionmeasurement",
                    "collision",
                    Some(Value::Entity(BALL)),
                    Some(Value::Entity(OTHER)),
                ),
                Member::Collision,
                e,
            )
        };
        w.actors.insert(OTHER, ball(here + Vec3::X, 0.0));
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(1)), "overlapping");
        w.actors.get_mut(&OTHER).unwrap().position = here + Vec3::X * 10.0;
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)), "10 m apart");
        w.actors.insert(OTHER, Actor::blank());
        assert_eq!(
            collision(&mut w, &mut e),
            Err(HostFault::Unmodelled),
            "no solid"
        );
    }

    /// AH.50's hand: an actor hung on his node 7 sits there; posed again,
    /// the node moves what hangs from it only once the hung are placed
    /// (the frame's end); and read while he's turned since, it's his node's
    /// offset turned by the facing he has now, where an aimed ball leaves
    /// from, while what the frame placed stays put. Taken off, it reads
    /// where it was left.
    #[test]
    fn a_hung_actor_follows_its_node_posed_and_reads_it_turned_as_he_stands() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0));
        e.hero.as_mut().unwrap().pivot = pivot;
        let mut w = world();
        w.actors.insert(BALL, ball(Vec3::ZERO, 0.0));
        let hand = Vec3::new(0.5, 2.0, 1.5);
        let mut nodes = vec![NodePose::at(Vec3::ZERO); 9];
        nodes[7] = NodePose::at(hand);
        w.set_nodes(HERO, nodes.clone());
        let node = w.objs.value(Object::Node(HERO, 7));
        w.set_member(
            &at(
                "abstractplacement",
                "attach node",
                Some(Value::Entity(BALL)),
                None,
            ),
            Member::AttachNode,
            node,
            &mut e,
        )
        .unwrap();
        assert_eq!(w.actor(BALL).unwrap().position, pivot + hand);
        // Posed again: the hung ball follows at the frame's end.
        let raised = Vec3::new(0.5, 3.0, 0.5);
        nodes[7] = NodePose::at(raised);
        w.set_nodes(HERO, nodes);
        assert_eq!(w.actor(BALL).unwrap().position, pivot + hand);
        w.place_hung(&mut e);
        assert_eq!(w.actor(BALL).unwrap().position, pivot + raised);
        // Turned a quarter since: read, it's the node turned as he faces now.
        e.hero.as_mut().unwrap().facing = 90.0;
        let read = w.position(BALL, &mut e).unwrap();
        let turned = pivot + Quat::from_rotation_y(90f32.to_radians()) * raised;
        assert!(read.distance(turned) < 1e-6, "{read:?}");
        assert!(read.distance(pivot + Vec3::new(0.5, 3.0, -0.5)) < 1e-6);
        assert_eq!(
            w.actor(BALL).unwrap().position,
            pivot + raised,
            "where the frame left it"
        );
        // Taken off, where it was left.
        w.actors.get_mut(&BALL).unwrap().removed = true;
        assert_eq!(w.position(BALL, &mut e), Some(pivot + raised));
    }

    /// AH.60's aim: an actor hung on a posed node takes the node's turn,
    /// turned by its owner's facing (stored when the hung are placed, read
    /// as the owner stands), its remembered point turned so too, and a turn
    /// is the angles the original keeps for it; an actor hung on a node
    /// never posed keeps its own turn.
    #[test]
    fn a_hung_actor_is_turned_as_its_node() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 30.0));
        e.hero.as_mut().unwrap().facing = 30.0;
        let mut w = world();
        w.actors.insert(BALL, ball(Vec3::ZERO, 0.0));
        w.actors.insert(CHOMPY, ball(Vec3::ZERO, 0.0));
        // The hand tilted 20° up, rolled 10° and turned 40° left of him.
        let mut nodes = vec![NodePose::at(Vec3::ZERO); 9];
        nodes[7] = NodePose {
            at: Vec3::new(0.5, 2.0, 1.5),
            turn: guac_motion::solid::euler([20.0, 40.0, 10.0]),
        };
        w.set_nodes(HERO, nodes);
        let hang = |w: &mut ScriptWorld, e: &mut Bench, who: Handle, owner: Handle| {
            let node = w.objs.value(Object::Node(owner, 7));
            w.set_member(
                &at(
                    "abstractplacement",
                    "attach node",
                    Some(Value::Entity(who)),
                    None,
                ),
                Member::AttachNode,
                node,
                e,
            )
            .unwrap();
        };
        hang(&mut w, &mut e, BALL, HERO);
        w.place_hung(&mut e);
        let stored = w.actor(BALL).unwrap().angles;
        let read = w.angles(BALL, &mut e).unwrap();
        // Its remembered point is turned as it is.
        let Value::Entity(kept) = w.objs.value(Object::Remembered(BALL)) else {
            unreachable!()
        };
        let remembered = w.angles(kept, &mut e).unwrap();
        for g in [stored, read, remembered] {
            assert!((g.facing - 70.0).abs() < 1e-3, "{g:?}");
            assert!((g.tilt - 20.0).abs() < 1e-3, "{g:?}");
            assert!((g.roll - 10.0).abs() < 1e-3, "{g:?}");
        }
        // He turns: read, it turns with him; stored, the frame's.
        e.hero.as_mut().unwrap().facing = 300.0;
        let read = w.angles(BALL, &mut e).unwrap();
        assert!((read.facing - 340.0).abs() < 1e-3, "{read:?}");
        assert!((w.actor(BALL).unwrap().angles.facing - 70.0).abs() < 1e-3);
        // A node never posed (the creature has none): its own turn kept.
        w.actors.get_mut(&CHOMPY).unwrap().angles.facing = 123.0;
        const OTHER: Handle = 0x6000_0009;
        w.actors.insert(OTHER, ball(Vec3::ZERO, 0.0));
        hang(&mut w, &mut e, OTHER, CHOMPY);
        w.place_hung(&mut e);
        assert_eq!(w.angles(OTHER, &mut e).unwrap(), Angles::default());
        // The angles a turn reads back as compose to it again.
        for angles in [
            [20.0, 40.0, 10.0],
            [-35.0, 250.0, -120.0],
            [0.0, 359.0, 0.0],
        ] {
            let q = guac_motion::solid::euler(angles);
            let back = angles_of(q);
            let again = guac_motion::solid::euler([back.tilt, back.facing, back.roll]);
            assert!(q.angle_between(again) < 1e-5, "{angles:?} -> {back:?}");
        }
    }

    /// A recorded state's creature set down mid-stride (AH.60's pinned
    /// scenes): its place, facing and velocity as given; `place` at rest.
    #[test]
    fn a_placed_actor_keeps_the_velocity_it_was_set_with() {
        let mut w = world();
        w.actors.insert(CHOMPY, ball(Vec3::ZERO, 0.0));
        let (at, v) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 0.0, 20.0));
        assert!(w.place_moving(CHOMPY, at, 90.0, v));
        let a = w.actor(CHOMPY).unwrap();
        assert_eq!((a.position, a.angles.facing, a.velocity), (at, 90.0, v));
        assert!(w.place(CHOMPY, Vec3::ZERO, 0.0));
        assert_eq!(w.actor(CHOMPY).unwrap().velocity, Vec3::ZERO);
        assert!(!w.place_moving(0x6000_0099, at, 0.0, v), "no such actor");
    }

    #[test]
    fn the_scripts_are_the_one_writer_of_his_speed_turn_and_clip() {
        let mut e = bench();
        let mut hero = Hero::new(
            eruptor_solid(),
            Tuning::ERUPTOR,
            Vec3::new(-5.0, 0.0, -5.0),
            0.0,
        );
        hero.settle(&e.world, &[]);
        e.hero = Some(hero);
        let mut w = world();
        w.actors.insert(HERO, Actor::blank());
        // A script's write lands in his controller at once.
        let walk = Vec3::new(0.0, 0.0, -21.5);
        w.set_velocity(HERO, walk, &mut e).unwrap();
        let turn = w.objs.value(Object::Facet(HERO, Facet::Orientation));
        w.set_member(
            &at("orientationmeasurement", "facing", Some(turn), None),
            Member::Facing,
            Value::Float(180.0),
            &mut e,
        )
        .unwrap();
        let h = e.hero.as_ref().unwrap();
        assert_eq!(h.walk.velocity, walk);
        assert_eq!(h.facing, 180.0);
        // The engine's frames move him by it and neither speed him up nor
        // turn him; no clip starts but a script's.
        let start = h.pivot;
        for _ in 0..30 {
            let hero = e.hero.as_mut().unwrap();
            hero.step_engine(&e.world, STEP, &[], Surfaces::NONE);
            w.advance(STEP, &mut e);
        }
        let h = e.hero.as_ref().unwrap();
        assert_eq!(h.walk.velocity, walk);
        assert_eq!(h.facing, 180.0);
        let moved = (h.pivot - start).length();
        assert!(
            (moved - 21.5 * METRES_PER_FOOT * 0.5).abs() < 1e-3,
            "{moved}"
        );
        assert!(w.sequences().is_empty());
    }

    /// AH.151: off his scripts AG.16's controller is the one writer of his
    /// walk and facing. A script's write to his velocity (a member, as the
    /// mover update's #608-#610, or a displace), to his facing and a `turn
    /// to` are dropped, counted by their writer, and the turn runs on; a
    /// `teleport to` still sets him down (and stops his walk, as it does on
    /// his scripts) and a displace still moves him, each counted; his
    /// scripts joining him again give both back.
    #[test]
    fn off_his_scripts_the_controller_alone_writes_his_walk_and_facing() {
        const MOVER: Handle = BALL;
        let mut e = bench();
        let mut hero = Hero::new(
            eruptor_solid(),
            Tuning::ERUPTOR,
            Vec3::new(-5.0, 0.0, -5.0),
            0.0,
        );
        hero.settle(&e.world, &[]);
        let walk = Vec3::new(0.0, 0.0, 21.5);
        hero.walk.velocity = walk;
        hero.facing = 90.0;
        e.hero = Some(hero);
        let mut w = world();
        w.actors.insert(HERO, Actor::blank());
        let to = Vec3::new(2.0, 0.0, -6.0);
        w.actors.insert(CHOMPY, ball(to, 0.0));
        w.hand_back_walk(HERO);
        assert!(w.walks_alone(HERO));
        let velocity = w.objs.value(Object::Facet(HERO, Facet::Velocity));
        let by = |a: Access| Access {
            placement: MOVER,
            ..a
        };
        let length = by(at("vectormeasurement", "length", Some(velocity), None));
        w.set_member(
            &length,
            Member::Vector(VectorField::Length),
            Value::Float(0.0),
            &mut e,
        )
        .unwrap();
        w.displace(MOVER, velocity, 1, 0.0, 0.0, 0.0, &mut e)
            .unwrap();
        let turn = w.objs.value(Object::Facet(HERO, Facet::Orientation));
        let facing = by(at("orientationmeasurement", "facing", Some(turn), None));
        w.set_member(&facing, Member::Facing, Value::Float(180.0), &mut e)
            .unwrap();
        let turn_to = OpCall {
            placement: MOVER,
            receiver: Value::Entity(HERO),
            class: Class::TurnTo,
            operands: vec![("_facingRHS", Some(Value::Float(270.0)))],
            words: Vec::new(),
            sets: Vec::new(),
        };
        let answer = w.op(&turn_to, &mut |_| 0, &mut e).unwrap();
        assert!(matches!(answer, Answer::Run { .. }), "{answer:?}");
        let h = e.hero.as_ref().unwrap();
        assert_eq!((h.walk.velocity, h.facing), (walk, 90.0), "none landed");
        let dropped = w.hero_writes();
        assert_eq!(dropped.get(&(MOVER, HeroWrite::Walk)), Some(&2));
        assert_eq!(dropped.get(&(MOVER, HeroWrite::Facing)), Some(&2));
        // The level setting him down still does.
        let teleport = OpCall {
            placement: MOVER,
            receiver: Value::Entity(HERO),
            class: Class::TeleportTo,
            operands: vec![
                ("_LHS", Some(Value::Entity(CHOMPY))),
                ("_facingRHS", Some(Value::Float(45.0))),
            ],
            words: Vec::new(),
            sets: Vec::new(),
        };
        w.op(&teleport, &mut |_| 0, &mut e).unwrap();
        let h = e.hero.as_ref().unwrap();
        assert_eq!((h.pivot, h.facing, h.walk.velocity), (to, 45.0, Vec3::ZERO));
        // A displace of his place as well (a foot along +z), each counted.
        w.displace(MOVER, Value::Entity(HERO), 0, 1.0, 0.0, 0.0, &mut e)
            .unwrap();
        let moved = e.hero.as_ref().unwrap().pivot - to;
        assert!(
            (moved - Vec3::Z * METRES_PER_FOOT).length() < 1e-6,
            "{moved}"
        );
        assert_eq!(w.hero_writes().get(&(MOVER, HeroWrite::Place)), Some(&2));
        // His scripts joining him again (the join's `hero_template`), they
        // write him.
        w.hero_template(Obj { unit: 0, index: 0 }, HERO);
        assert!(!w.walks_alone(HERO));
        w.set_member(&facing, Member::Facing, Value::Float(180.0), &mut e)
            .unwrap();
        assert_eq!(e.hero.as_ref().unwrap().facing, 180.0);
    }
}

/// What chapter 1's first fight reaches of the host (openspec AH.18), on the
/// parry side of the seam over open ground: creatures walked by their
/// scripts' velocity and steered by `move to`, `move from` and `turn to`,
/// `teleport to`, `check fov`, separation, collision and contacts between
/// two actors' solids, a play's index operand, and the engine's sets of
/// placements asleep and taken off.
#[cfg(test)]
mod fight_tests {
    use std::collections::HashMap;

    use guac_formats::script::Class;
    use guac_motion::hero::Hero;
    use guac_motion::parry::ParryWorld;
    use guac_motion::seam::{Body, BodyKind, Collider, PhysicsSeam};
    use guac_motion::tuning::{FOOT, Tuning, eruptor_solid};
    use guac_script::{Answer, MemberId, Obj, OpCall};

    use super::*;
    use crate::script_engine::{Actor, Pad, Physics, Solid, Walker};
    use crate::scripts::{Files, headless_objects};

    pub(super) const C1: Handle = 0x6100_0001;
    const C2: Handle = 0x6100_0002;
    const C3: Handle = 0x6100_0003;
    const HERO: Handle = 0x6100_0010;
    pub(super) const STEP: f32 = 1.0 / 60.0;
    const WP: Obj = Obj { unit: 9, index: 1 };
    const WP2: Obj = Obj { unit: 9, index: 2 };

    /// Open level ground whose top is y = 0; the hero and the creatures
    /// bodies in it as the embedder keeps them.
    pub(super) struct Pit {
        world: ParryWorld,
        hero: Option<Hero>,
        bodies: HashMap<Handle, BodyId>,
    }

    impl Engine for Pit {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            self.hero.as_mut().filter(|_| h == HERO)
        }
        fn heroes(&self) -> Vec<Handle> {
            self.hero.iter().map(|_| HERO).collect()
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.world
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
        fn body(&self, actor: Handle) -> Option<BodyId> {
            self.bodies.get(&actor).copied()
        }
        fn actor_of(&self, body: BodyId) -> Option<Handle> {
            self.bodies
                .iter()
                .find(|(_, b)| **b == body)
                .map(|(h, _)| *h)
        }
    }

    pub(super) fn pit() -> Pit {
        let floor = Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(0.0, -0.5, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(40.0, 0.5, 40.0),
                margin: 0.0,
            }),
        };
        Pit {
            world: ParryWorld::new(&[floor]).unwrap(),
            hero: None,
            bodies: HashMap::new(),
        }
    }

    /// A chapter 1 Chompy as its STARTUP leaves it: a 6 x 3 x 3 ft sphere
    /// meeting the level and actors, 20 ft/s, 160 up, 80 down, 720°/s, a
    /// creature the engine walks.
    pub(super) fn chompy(at: Vec3, facing: f32) -> Actor {
        let mut a = Actor::blank();
        a.position = at;
        a.angles.facing = facing;
        a.obj = Some(Obj {
            unit: 4,
            index: at.x.to_bits(),
        });
        a.managed = true;
        a.physics = Physics {
            ground_max_speed: 20.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 720.0,
            air_max_speed: 20.0,
            air_acceleration: 160.0,
            air_deceleration: 80.0,
            air_turn_rate: 720.0,
            gravity_acceleration: -1.0,
            gravity_max_speed: -1.0,
            time_scale: 100.0,
            ..Physics::default()
        };
        a.solid = Solid {
            width: 6.0,
            length: 3.0,
            height: 3.0,
            interacts_with_level_mesh: 1,
            interacts_with_actors: 1,
            shape: 3,
            ..Solid::default()
        };
        a.walker = Some(Box::new(Walker {
            body: None,
            aim: guac_motion::steer::Aim::Still,
            along: 0,
        }));
        a
    }

    pub(super) fn world() -> ScriptWorld {
        let files = Files {
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: Vec::new(),
            ids: HashMap::new(),
            points: HashMap::from([
                (WP, Vec3::new(8.0, 0.0, -6.0)),
                (WP2, Vec3::new(8.0, 0.0, 6.0)),
            ]),
        };
        let handles = HashMap::new();
        ScriptWorld::new(&files, &handles, headless_objects(&handles))
    }

    fn op(
        class: Class,
        receiver: Handle,
        operands: Vec<(&'static str, Option<Value>)>,
        sets: Vec<(&'static str, Vec<Value>)>,
    ) -> OpCall {
        OpCall {
            placement: receiver,
            receiver: Value::Entity(receiver),
            class,
            operands,
            words: vec![("_dir", 0), ("_relOperator", 0)],
            sets,
        }
    }

    fn ask(w: &mut ScriptWorld, e: &mut Pit, call: &OpCall) -> Answer {
        w.op(call, &mut |_| 0, e).unwrap()
    }

    fn get(
        w: &mut ScriptWorld,
        e: &mut Pit,
        ns: &str,
        name: &str,
        on: Handle,
        other: Handle,
    ) -> Value {
        w.get(
            &Access {
                placement: on,
                receiver: Some(Value::Entity(on)),
                member: MemberId::named(ns, name),
                other_point: Some(Value::Entity(other)),
                via: None,
                op: None,
            },
            e,
        )
        .unwrap()
    }

    fn move_to(target: Value, within: i32) -> OpCall {
        op(
            Class::MoveTo,
            C1,
            vec![
                ("_indexRHS", None),
                ("_LHS", Some(target)),
                ("_RHS", Some(Value::Int(within))),
                ("_NP", None),
            ],
            Vec::new(),
        )
    }

    fn wrap(d: f32) -> f32 {
        (d + 180.0).rem_euclid(360.0) - 180.0
    }

    #[test]
    fn a_chompy_moved_to_a_point_reaches_it_at_its_speed_and_turn_rate() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::new(0.0, 0.0, 0.0), 0.0));
        let goal = Vec3::new(8.0, 0.0, -6.0);
        let mut frames = 0;
        let mut last = (Vec3::ZERO, 0.0f32);
        let mut top = 0.0f32;
        loop {
            match ask(&mut w, &mut e, &move_to(Value::Object(WP), 5)) {
                Answer::Run { .. } => break,
                Answer::Done => {}
                other => panic!("{other:?}"),
            }
            w.advance(STEP, &mut e);
            frames += 1;
            let a = w.actor(C1).unwrap();
            let across = Vec3::new(a.position.x - last.0.x, 0.0, a.position.z - last.0.z);
            if frames > 1 {
                top = top.max(across.length() / STEP / FOOT);
                let turned = wrap(a.angles.facing - last.1).abs();
                assert!(turned <= 12.0 + 1e-3, "frame {frames}: {turned}°");
            }
            last = (a.position, a.angles.facing);
            assert!(frames < 200, "never got there: {:?}", a.position);
        }
        // AH.10's 20 ft/s and 12° a frame; within 5 ft of it across the
        // ground, the engine has no destination.
        assert!(top <= 20.0 + 1e-3 && top > 19.9, "{top}");
        let at = w.actor(C1).unwrap().position;
        assert!(Vec3::new(at.x - goal.x, 0.0, at.z - goal.z).length() <= 5.0 * FOOT);
        assert!((92..=100).contains(&frames), "{frames}");
        assert!(at.y.abs() < 0.02, "on the ground: {at}");
        let w2 = w.actor(C1).unwrap().walker.as_ref().unwrap();
        assert_eq!(w2.aim, guac_motion::steer::Aim::Still);
    }

    #[test]
    fn along_a_path_it_walks_to_each_waypoint_in_turn() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::new(0.0, 0.0, 0.0), 0.0));
        let path = vec![Value::Object(WP), Value::Object(WP2)];
        let mut reached = Vec::new();
        for k in 0..400 {
            let call = op(
                Class::MoveTo,
                C1,
                vec![
                    ("_indexRHS", None),
                    ("_LHS", Some(Value::Null)),
                    ("_RHS", Some(Value::Int(2))),
                    ("_NP", None),
                ],
                vec![("_LHS", path.clone())],
            );
            if matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }) {
                reached.push(k);
                if reached.len() == 1 {
                    let aim = w.actor(C1).unwrap().walker.as_ref().unwrap().aim;
                    assert_eq!(aim, Aim::To(Vec3::new(8.0, 0.0, 6.0)), "the next at once");
                }
            }
            w.advance(STEP, &mut e);
        }
        let along = w.actor(C1).unwrap().walker.as_ref().unwrap().along;
        assert_eq!(along, 1, "on the last waypoint");
        assert!(!reached.is_empty());
        let at = w.actor(C1).unwrap().position;
        assert!(
            (at - Vec3::new(8.0, at.y, 6.0)).length() <= 2.0 * FOOT,
            "{at}"
        );
    }

    #[test]
    fn moved_from_a_point_it_goes_until_beyond() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::new(8.0, 0.0, -3.0), 0.0));
        let mut done = None;
        for k in 0..200 {
            let call = op(
                Class::MoveFrom,
                C1,
                vec![
                    ("_indexRHS", None),
                    ("_LHS", Some(Value::Object(WP))),
                    ("_RHS", Some(Value::Int(20))),
                    ("_NP", None),
                ],
                Vec::new(),
            );
            if matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }) {
                done = Some(k);
                break;
            }
            w.advance(STEP, &mut e);
        }
        assert!(done.is_some());
        let at = w.actor(C1).unwrap().position;
        assert!(Vec3::new(at.x - 8.0, 0.0, at.z + 6.0).length() >= 20.0 * FOOT);
    }

    #[test]
    fn turned_to_a_facing_it_turns_in_place_at_its_rate() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::new(0.0, 0.0, 0.0), 0.0));
        let at = w.actor(C1).unwrap().position;
        let turn = |f: f32| {
            op(
                Class::TurnTo,
                C1,
                vec![("_facingRHS", Some(Value::Float(f))), ("_NP", None)],
                Vec::new(),
            )
        };
        let mut facings = Vec::new();
        let mut ran = None;
        for k in 0..12 {
            if matches!(ask(&mut w, &mut e, &turn(60.0)), Answer::Run { .. }) {
                ran = Some(k);
                break;
            }
            w.advance(STEP, &mut e);
            facings.push(w.actor(C1).unwrap().angles.facing);
        }
        assert!((facings[0] - 12.0).abs() < 1e-3, "{facings:?}");
        assert_eq!(ran, Some(5), "facing it after 5 frames: {facings:?}");
        assert!((w.actor(C1).unwrap().position - at).length() < 0.02);
        // Anything the engine doesn't walk turns at once.
        let mut prop = Actor::blank();
        prop.angles.facing = 10.0;
        w.actors.insert(C2, prop);
        let mut call = turn(90.0);
        call.receiver = Value::Entity(C2);
        assert!(matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }));
        assert_eq!(w.actor(C2).unwrap().angles.facing, 90.0);
    }

    #[test]
    fn a_creatures_scripts_velocity_carries_it_along_the_ground_slowing() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::new(0.0, 0.0, 0.0), 0.0));
        w.set_velocity(C1, Vec3::new(0.0, 0.0, 20.0), &mut e)
            .unwrap();
        assert!(!w.actor(C1).unwrap().launched, "a creature walks");
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        let at = w.actor(C1).unwrap().position;
        // With no destination the engine's model slows it by its ground
        // deceleration, 80 ft/s^2 (a lunge's end so): 1.33 ft/s a frame,
        // still after 15 frames and 2.33 ft.
        assert!((at.z - 7.0 / 3.0 * FOOT).abs() < 0.005, "{at}");
        assert_eq!(w.actor(C1).unwrap().velocity, Vec3::ZERO);
        assert!(at.y.abs() < 0.02, "{at}");
    }

    #[test]
    fn separation_collision_and_contacts_are_between_the_solids() {
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        hero.settle(&e.world, &[]);
        e.hero = Some(hero);
        e.bodies.insert(HERO, BodyId(9));
        let mut w = world();
        // Both 3 ft spheres as authored (the seam holds them 0.05 m in).
        let r = 3.0 * FOOT;
        let pivot = e.hero.as_ref().unwrap().pivot;
        // 5 m apart at the same height: 5 - 2r between the spheres.
        w.actors
            .insert(C1, chompy(pivot + Vec3::new(0.0, 0.0, 5.0), 0.0));
        let sep = get(
            &mut w,
            &mut e,
            "positionmeasurement",
            "separation",
            C1,
            HERO,
        );
        let want = (5.0 - 2.0 * r) * FEET_PER_METRE;
        assert!(
            (sep.as_f32().unwrap() - want).abs() < 0.01,
            "{sep:?} {want}"
        );
        // Within a centimetre they touch; past it they don't.
        for (gap, touches) in [(0.008f32, 1), (0.012, 0)] {
            w.actors.get_mut(&C1).unwrap().position = pivot + Vec3::new(0.0, 0.0, 2.0 * r + gap);
            let c = get(&mut w, &mut e, "positionmeasurement", "collision", HERO, C1);
            assert_eq!(c, Value::Int(touches), "gap {gap}");
        }
        // Contacts with the hero: a Chompy 5 mm off and one 2 mm off,
        // nearest first; one 3 m off isn't.
        w.actors.get_mut(&C1).unwrap().position = pivot + Vec3::new(0.0, 0.0, 2.0 * r + 0.005);
        w.actors.insert(
            C2,
            chompy(pivot + Vec3::new(2.0 * r + 0.002, 0.0, 0.0), 0.0),
        );
        w.actors
            .insert(C3, chompy(pivot + Vec3::new(-3.0, 0.0, 0.0), 0.0));
        let facet = w.objs.value(Object::Facet(HERO, Facet::Collision));
        let contacts = w
            .elements(
                &Access {
                    placement: HERO,
                    receiver: Some(facet),
                    member: MemberId::named("collisioninfo", "contacts with me"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                &mut e,
            )
            .unwrap();
        assert_eq!(contacts, vec![Value::Entity(C2), Value::Entity(C1)]);
    }

    /// AG.90: a hero's separation from his ground is his pivot's height over
    /// it once his support check fails, wherever his placement's own copy of
    /// a place stands. That copy is where he joined and never moves (his
    /// place is his controller's): read as his, a hero off the ground above
    /// it read no separation at all, and one below it read the whole drop
    /// to where he joined.
    #[test]
    fn a_heros_separation_from_his_ground_is_his_controllers() {
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        hero.settle(&e.world, &[]);
        let rest = hero.pivot;
        e.hero = Some(hero);
        let mut w = world();
        // His placement as the join leaves it: his solid as authored, a
        // place of its own (Eruptor's 4 x 3 x 6 ft sphere at his 150 %).
        let mut placed = Actor::blank();
        placed.scale = [150.0; 3];
        placed.solid = Solid {
            width: 4.0,
            length: 3.0,
            height: 6.0,
            shape: 3,
            interacts_with_level_mesh: 1,
            ..Solid::default()
        };
        let ground = w.objs.value(Object::Facet(HERO, Facet::Ground));
        let separation = |w: &mut ScriptWorld, e: &mut Pit| {
            w.get(
                &Access {
                    placement: HERO,
                    receiver: Some(Value::Entity(HERO)),
                    member: MemberId::named("positionmeasurement", "separation"),
                    other_point: Some(ground),
                    via: None,
                    op: None,
                },
                e,
            )
            .unwrap()
            .as_f32()
            .unwrap()
        };
        for joined_at in [-10.0, 0.0, 10.0] {
            placed.position = Vec3::new(3.0, joined_at, 3.0);
            w.actors.insert(HERO, placed.clone());
            // Supported: no gap, whatever his skin leaves.
            e.hero.as_mut().unwrap().pivot = rest;
            e.hero.as_mut().unwrap().stand(&e.world, &[]);
            assert!(e.hero.as_ref().unwrap().footing.supported);
            assert_eq!(separation(&mut w, &mut e), 0.0, "joined at {joined_at}");
            // Off the ground 3 cm up (a frame his walk left it): 3 cm.
            let hero = e.hero.as_mut().unwrap();
            hero.pivot = rest + Vec3::Y * 0.03;
            hero.footing.supported = false;
            hero.footing.height = Some(hero.pivot.y);
            let ft = separation(&mut w, &mut e);
            assert!(
                (ft - hero_height_ft(e.hero.as_ref().unwrap())).abs() < 1e-4,
                "joined at {joined_at}: {ft} ft"
            );
        }
    }

    fn hero_height_ft(hero: &Hero) -> f32 {
        hero.footing.height.unwrap() * FEET_PER_METRE
    }

    /// A spinning coin (openspec AG.60): an actor hung from nothing turns
    /// by each relative facing written, as the loot update writes its spin
    /// speed every tick (AH.11's walkway coins stepping 2.82, -5.31 and 4.23
    /// degrees a frame), round past 360 and back below 0; its absolute
    /// facing still sets it outright, and a write of 0 leaves it (a placed
    /// gem, no spin, holds its file's 45 degrees). Its relative facing reads
    /// 0, so a script adding to it turns it by what it adds.
    #[test]
    fn a_relative_facing_turns_an_actor_hung_from_nothing_by_it() {
        let mut e = pit();
        let mut w = world();
        let mut coin = Actor::blank();
        coin.angles.facing = 350.0;
        w.actors.insert(C1, coin);
        let turn = w.objs.value(Object::Facet(C1, Facet::Orientation));
        let access = |name: &str| Access {
            placement: C1,
            receiver: Some(turn),
            member: MemberId::named("orientationmeasurement", name),
            other_point: None,
            via: None,
            op: None,
        };
        let mut facings = Vec::new();
        for speed in [4.5f32, 4.5, 4.5, -5.31, 0.0] {
            w.set_member(
                &access("relative facing"),
                Member::RelativeFacing,
                Value::Float(speed),
                &mut e,
            )
            .unwrap();
            facings.push(w.actor(C1).unwrap().angles.facing);
        }
        let want = [354.5, 359.0, 3.5, 358.19, 358.19];
        for (got, want) in facings.iter().zip(want) {
            assert!((got - want).abs() < 1e-3, "{facings:?}");
        }
        w.set_member(
            &access("facing"),
            Member::Facing,
            Value::Float(45.0),
            &mut e,
        )
        .unwrap();
        assert_eq!(w.actor(C1).unwrap().angles.facing, 45.0);
        // It reads 0, so a script's own `relative facing + 4` (a pebble's
        // bounce, an ejected lock's spin) turns it by 4 a tick, its facing
        // still read whole.
        for want in [49.0, 53.0] {
            let Value::Float(now) = w
                .get_member(&access("relative facing"), Member::RelativeFacing, &mut e)
                .unwrap()
            else {
                panic!("a float");
            };
            assert_eq!(now, 0.0);
            w.set_member(
                &access("relative facing"),
                Member::RelativeFacing,
                Value::Float(now + 4.0),
                &mut e,
            )
            .unwrap();
            assert_eq!(
                w.get_member(&access("facing"), Member::Facing, &mut e),
                Ok(Value::Float(want))
            );
        }
    }

    /// AG.90: an actor hung from a hero's ground and turned there by a
    /// relative facing (Face and Move's velocity actor, turned by the way
    /// the stick asks less his facing) tilts as the slope along his facing
    /// plus it, the ground leaning as his footing's slope. Read as a heading
    /// of its own, it tilted along whatever lay that far from 0°.
    #[test]
    fn a_relative_facing_on_a_heros_ground_turns_from_his_facing() {
        use guac_motion::walk::{Footing, push_direction};
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        // The ledge route's 17° slope by barrel_b_intact(1), falling toward
        // 113.4°, and him facing as he did through AG.90's rock.
        let n = Vec3::new(0.270_066, 0.955_700_3, -0.117_053_23);
        hero.footing = Footing {
            supported: true,
            height: Some(0.005),
            slope: n,
            ground: Some(n),
        };
        hero.facing = 233.5;
        e.hero = Some(hero);
        let mut w = world();
        let mut hung = Actor::blank();
        hung.attach = w.objs.value(Object::Facet(HERO, Facet::Ground));
        w.actors.insert(C1, hung);
        let turn = w.objs.value(Object::Facet(C1, Facet::Orientation));
        let at = Access {
            placement: C1,
            receiver: Some(turn),
            member: MemberId::named("orientationmeasurement", "relative facing"),
            other_point: None,
            via: None,
            op: None,
        };
        for (asked, up) in [
            (257.4f32, true),
            (77.4, false),
            (233.5, true),
            (113.4, false),
        ] {
            w.set_member(
                &at,
                Member::RelativeFacing,
                Value::Float(asked - 233.5),
                &mut e,
            )
            .unwrap();
            let tilt = w.actor(C1).unwrap().angles.tilt;
            let want = crate::script_engine::pitch_of(push_direction(asked, Some(n)));
            assert!((tilt - want).abs() < 1e-3, "{asked}: {tilt} against {want}");
            assert_eq!(tilt > 0.0, up, "{asked}: {tilt}");
        }
        // Straight down the fall line, the slope's whole 17°.
        assert!((w.actor(C1).unwrap().angles.tilt + 17.1).abs() < 0.1);
    }

    #[test]
    fn check_fov_finds_whats_in_its_cone_by_distance_nearest_first() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::ZERO, 0.0));
        // Ahead at 3 m and 1 m; off to the side at 90°; ahead but 20 m off.
        w.actors.insert(C2, chompy(Vec3::new(0.0, 0.0, 3.0), 0.0));
        w.actors.insert(C3, chompy(Vec3::new(0.0, 0.0, 1.0), 0.0));
        let side = 0x6100_0020;
        let far = 0x6100_0021;
        w.actors.insert(side, chompy(Vec3::new(3.0, 0.0, 0.0), 0.0));
        w.actors.insert(far, chompy(Vec3::new(0.0, 0.0, 20.0), 0.0));
        let call = op(
            Class::CheckFov,
            C1,
            vec![
                ("_RHSfacing", Some(Value::Float(0.0))),
                ("_RHSfov", Some(Value::Int(90))),
                ("_fromLHS", Some(Value::Entity(C1))),
                ("_LHS", Some(Value::Null)),
                ("_RHS", Some(Value::Int(15))),
            ],
            vec![("_LHS", [C2, C3, side, far].map(Value::Entity).to_vec())],
        );
        assert_eq!(
            ask(&mut w, &mut e, &call),
            Answer::Found(vec![Value::Entity(C3), Value::Entity(C2)])
        );
    }

    #[test]
    fn teleport_to_sets_the_hero_or_an_actor_at_a_point() {
        let mut e = pit();
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0));
        let mut w = world();
        let mut call = op(
            Class::TeleportTo,
            HERO,
            vec![
                ("_LHS", Some(Value::Object(WP))),
                ("_facingRHS", Some(Value::Float(-90.0))),
            ],
            Vec::new(),
        );
        assert_eq!(ask(&mut w, &mut e, &call), Answer::Done);
        let hero = e.hero.as_ref().unwrap();
        assert_eq!(hero.pivot, Vec3::new(8.0, 0.0, -6.0));
        assert_eq!(hero.facing, 270.0);
        w.actors.insert(C1, chompy(Vec3::ZERO, 0.0));
        call.receiver = Value::Entity(C1);
        assert_eq!(ask(&mut w, &mut e, &call), Answer::Done);
        let a = w.actor(C1).unwrap();
        assert_eq!(
            (a.position, a.angles.facing),
            (Vec3::new(8.0, 0.0, -6.0), -90.0)
        );
    }

    /// `displace` turned by the target (combine 2) turns by its whole
    /// orientation (openspec AG.28): up from a viewport looking 23.36 down
    /// at 15 degrees leans forward, where at no tilt it is straight up.
    #[test]
    fn a_displace_turned_by_the_target_leans_with_its_tilt() {
        let mut e = pit();
        let mut w = world();
        let displaced = |w: &mut ScriptWorld, e: &mut Pit, tilt: f32, mode: u32| {
            let mut a = Actor::blank();
            a.position = Vec3::new(1.0, 2.0, 3.0);
            a.angles.facing = 15.0;
            a.angles.tilt = tilt;
            w.actors.insert(C2, a);
            let call = OpCall {
                placement: C2,
                receiver: Value::Entity(C2),
                class: Class::Displace,
                operands: vec![
                    ("_NP", Some(Value::Entity(C2))),
                    ("_lenNP", Some(Value::Float(1.0))),
                    ("_headNP", Some(Value::Float(0.0))),
                    ("_pitchNP", Some(Value::Float(90.0))),
                ],
                words: vec![("_combineMode", mode)],
                sets: Vec::new(),
            };
            assert_eq!(ask(w, e, &call), Answer::Done);
            w.actor(C2).unwrap().position - Vec3::new(1.0, 2.0, 3.0)
        };
        let foot = 0.3048;
        let up = displaced(&mut w, &mut e, 0.0, 2);
        assert!((up - Vec3::Y * foot).length() < 1e-5, "{up}");
        let t = 23.36f32.to_radians();
        let (s, c) = 15f32.to_radians().sin_cos();
        let leaning = displaced(&mut w, &mut e, -23.36, 2);
        let want = Vec3::new(t.sin() * s, t.cos(), t.sin() * c) * foot;
        assert!((leaning - want).length() < 1e-5, "{leaning} against {want}");
        // In the world's axes (combine 0) it is straight up whatever the tilt.
        let world_up = displaced(&mut w, &mut e, -23.36, 0);
        assert!((world_up - Vec3::Y * foot).length() < 1e-5, "{world_up}");
    }

    /// The type of the ground under an awake actor with no solid and none
    /// found reads 0, not -1 (openspec AG.28: the camera's target, a clone
    /// of `Generic_FollowActor`, over the edge of the level's mesh); with a
    /// solid, or asleep, -1 stays.
    #[test]
    fn a_solidless_actor_with_no_ground_found_reads_a_type_of_0() {
        let mut e = pit();
        let mut w = world();
        let type_under = |w: &mut ScriptWorld, e: &mut Pit, h: Handle| {
            let ground = get(w, e, "abstractplacement", "ground", h, h);
            w.get(
                &Access {
                    placement: h,
                    receiver: Some(ground),
                    member: MemberId::named("contactinfo", "type"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                e,
            )
            .unwrap()
        };
        // Far over the floor: nothing within the ray's 160 m.
        let mut free = Actor::blank();
        free.position = Vec3::new(0.0, 500.0, 0.0);
        w.actors.insert(C2, free);
        assert_eq!(type_under(&mut w, &mut e, C2), Value::Int(0));
        // A creature with a solid, far over the floor: still none.
        w.actors.insert(C1, chompy(Vec3::new(0.0, 500.0, 0.0), 0.0));
        assert_eq!(type_under(&mut w, &mut e, C1), Value::Int(-1));
    }

    /// A follow actor at rest has every point remembered through a
    /// reference to it on the recorded one, and no other actor's moves.
    #[test]
    fn a_follow_actor_at_rest_has_its_remembered_points_set_together() {
        let mut w = world();
        let (here, there) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(9.0, 9.0, 9.0));
        for (h, c) in [(C1, 4), (C1, 5), (C1, 6), (C2, 4)] {
            w.kept_via.insert((h, c), there);
        }
        assert_eq!(w.rest_remembered(C1, here), 3);
        assert_eq!(w.kept_via[&(C1, 5)], here);
        assert_eq!(w.kept_via[&(C2, 4)], there);
    }

    #[test]
    fn a_plays_index_operand_picks_its_slot_of_the_set() {
        let mut e = pit();
        let mut w = world();
        let template = Obj { unit: 4, index: 77 };
        w.set_palette(template, vec![1.6, 1.0, 0.0, 0.4]);
        let mut c = chompy(Vec3::ZERO, 0.0);
        c.obj = Some(template);
        w.actors.insert(C1, c);
        let clips: Vec<Value> = (0..4)
            .map(|slot| w.objs.value(Object::Clip { template, slot }))
            .collect();
        let call = op(
            Class::StartSequence,
            C1,
            vec![
                ("_LHS", Some(Value::Null)),
                ("_indexRHS", Some(Value::Int(3))),
            ],
            vec![("_LHS", clips.clone())],
        );
        assert!(matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }));
        let s = w.sequences().last().unwrap();
        assert_eq!((s.template, s.length), (clips[3], 0.4));
    }

    #[test]
    fn the_engines_sets_hold_whats_asleep_and_whats_taken_off() {
        let mut e = pit();
        let mut w = world();
        let mut a = chompy(Vec3::ZERO, 0.0);
        a.active = false;
        w.actors.insert(C1, a);
        w.actors.insert(C2, chompy(Vec3::ONE, 0.0));
        let mut gone = chompy(Vec3::NEG_ONE, 0.0);
        gone.removed = true;
        w.actors.insert(C3, gone);
        let set = |w: &mut ScriptWorld, e: &mut Pit, name: &str| {
            w.elements(
                &Access {
                    placement: 1,
                    receiver: None,
                    member: MemberId::named("actorinfo", name),
                    other_point: None,
                    via: None,
                    op: None,
                },
                e,
            )
            .unwrap()
        };
        assert_eq!(set(&mut w, &mut e, "inactive"), vec![Value::Entity(C1)]);
        assert_eq!(set(&mut w, &mut e, "removed"), vec![Value::Entity(C3)]);
        // Asleep is the activity rule's: a removed placement isn't asleep,
        // nor is an actor the rule doesn't manage (a clone).
        w.actors.get_mut(&C3).unwrap().active = false;
        let mut clone = chompy(Vec3::new(2.0, 0.0, 0.0), 0.0);
        (clone.managed, clone.active) = (false, false);
        w.actors.insert(HERO + 1, clone);
        assert_eq!(set(&mut w, &mut e, "inactive"), vec![Value::Entity(C1)]);
    }

    /// AG.160: a clone of nothing makes nothing: its block is skipped (the
    /// locomotion clones each state's sound control, which a character
    /// whose states play no footsteps has none of; a blank clone answering
    /// for it stopped `031_GhostEater`'s first tick).
    #[test]
    fn a_clone_of_nothing_is_skipped() {
        let mut e = pit();
        let mut w = world();
        let call = op(
            Class::Spawn,
            C1,
            vec![("_LHS", Some(Value::Null))],
            Vec::new(),
        );
        w.actors.insert(C1, chompy(Vec3::ZERO, 0.0));
        let before = w.actors.len();
        assert_eq!(ask(&mut w, &mut e, &call), Answer::Skip);
        assert_eq!(w.actors.len(), before, "no clone");
    }

    /// AG.160: a Giant's footstep reads `object collisions` on his solid,
    /// the actors his solid touches (`Minion - Titan Abilities`): a Chompy
    /// against him, none past reach, none taken off; nothing without one.
    #[test]
    fn object_collisions_are_the_actors_his_solid_touches() {
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        hero.settle(&e.world, &[]);
        e.hero = Some(hero);
        e.bodies.insert(HERO, BodyId(9));
        let mut w = world();
        let r = 3.0 * FOOT;
        let pivot = e.hero.as_ref().unwrap().pivot;
        let at = |x: f32, z: f32| pivot + Vec3::new(x, 0.0, z);
        let facet = w.objs.value(Object::Facet(HERO, Facet::Collision));
        let read = |w: &mut ScriptWorld, e: &mut Pit| {
            w.elements(
                &Access {
                    placement: HERO,
                    receiver: Some(facet),
                    member: MemberId::named("collisioninfo", "object collisions"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                e,
            )
            .unwrap()
        };
        assert_eq!(read(&mut w, &mut e), Vec::<Value>::new());
        w.actors.insert(C1, chompy(at(0.0, 2.0 * r + 0.005), 0.0));
        w.actors.insert(C2, chompy(at(0.0, -2.0 * r - 0.5), 0.0));
        let mut gone = chompy(at(2.0 * r + 0.002, 0.0), 0.0);
        gone.removed = true;
        w.actors.insert(C3, gone);
        assert_eq!(read(&mut w, &mut e), vec![Value::Entity(C1)]);
    }

    /// The review of AH.18: what touches a hero is what's awake and on:
    /// a Chompy taken off or asleep against him isn't among his contacts.
    #[test]
    fn contacts_with_me_leave_out_whats_taken_off_or_asleep() {
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        hero.settle(&e.world, &[]);
        e.hero = Some(hero);
        e.bodies.insert(HERO, BodyId(9));
        let mut w = world();
        let r = 3.0 * FOOT;
        let pivot = e.hero.as_ref().unwrap().pivot;
        let at = |x: f32, z: f32| pivot + Vec3::new(x, 0.0, z);
        w.actors.insert(C1, chompy(at(0.0, 2.0 * r + 0.005), 0.0));
        let mut gone = chompy(at(2.0 * r + 0.002, 0.0), 0.0);
        gone.removed = true;
        w.actors.insert(C2, gone);
        let mut asleep = chompy(at(-2.0 * r - 0.002, 0.0), 0.0);
        asleep.active = false;
        w.actors.insert(C3, asleep);
        let facet = w.objs.value(Object::Facet(HERO, Facet::Collision));
        let contacts = w
            .elements(
                &Access {
                    placement: HERO,
                    receiver: Some(facet),
                    member: MemberId::named("collisioninfo", "contacts with me"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                &mut e,
            )
            .unwrap();
        assert_eq!(contacts, vec![Value::Entity(C1)]);
    }

    /// The review of AH.18: the ray caster and the ground finder meet the
    /// level, never an actor's body (a creature the engine walks): a ray
    /// down through a Chompy lands on the floor under it, and so does the
    /// ground under an actor standing over one.
    #[test]
    fn rays_and_the_ground_finder_pass_through_the_actors_bodies() {
        let mut e = pit();
        let r = 3.0 * FOOT;
        let centre = Vec3::new(0.0, r, 3.0);
        e.world
            .set_body(&Body {
                id: BodyId(21),
                kind: BodyKind::Kinematic,
                pose: Pose::at(centre),
                collider: Collider::Convex(Shape::Sphere { radius: r }),
            })
            .unwrap();
        e.bodies.insert(C1, BodyId(21));
        let mut w = world();
        w.actors.insert(C1, chompy(centre, 0.0));
        // Through it from above: the floor (y 0), not its top (y 2r).
        w.ray = crate::scripts::Ray {
            start: centre + Vec3::Y * 3.0,
            end: centre - Vec3::Y * 3.0,
        };
        let (hit, _) = w.ray_contact(&mut e).expect("the floor");
        assert!(hit.y.abs() < 0.01, "{hit}");
        // The ground under an actor over it: the floor too.
        let mut over = Actor::blank();
        over.position = centre + Vec3::Y * 2.0;
        w.actors.insert(C2, over);
        let (ground, ..) = w.ground(C2, &mut e).expect("ground");
        assert!(ground.y.abs() < 0.01, "{ground}");
        // Without the actor's body in the way the seam alone would stop on
        // it: the exclusion is what lets these through.
        let top = guac_motion::ground::ray(&e.world, centre + Vec3::Y * 3.0, Vec3::NEG_Y, 6.0, &[])
            .unwrap();
        assert!((top.point.y - 2.0 * r).abs() < 0.01, "{}", top.point);
    }

    /// The ray cast passes through a prop whose collision doesn't act as an
    /// actor surface (a barrel) to what's past it, and stops on one whose
    /// does (a wall); the ground finder still stands on either (AH.21's
    /// props; merging AH.50 in, a burst's look at the barrel behind
    /// another met the near one).
    #[test]
    fn the_ray_cast_passes_props_that_dont_act_as_actor_surfaces() {
        let mut e = pit();
        let half = Vec3::new(1.0, 1.0, 0.25);
        e.world
            .set_body(&Body {
                id: BodyId(31),
                kind: BodyKind::Kinematic,
                pose: Pose::at(Vec3::new(0.0, 1.0, 3.0)),
                collider: Collider::Convex(Shape::Box {
                    half_extents: half,
                    margin: 0.0,
                }),
            })
            .unwrap();
        e.bodies.insert(C3, BodyId(31));
        let mut w = world();
        let mut barrel = Actor::blank();
        barrel.position = Vec3::new(0.0, 0.0, 3.0);
        w.actors.insert(C3, barrel);
        // Along the ground through it: nothing past it but open level.
        w.ray = crate::scripts::Ray {
            start: Vec3::new(0.0, 1.0, 0.0),
            end: Vec3::new(0.0, 1.0, 6.0),
        };
        assert_eq!(w.ray_contact(&mut e), None, "through the barrel");
        // Down onto it: the floor under it.
        w.ray = crate::scripts::Ray {
            start: Vec3::new(0.0, 4.0, 3.0),
            end: Vec3::new(0.0, -1.0, 3.0),
        };
        let (floor, _) = w.ray_contact(&mut e).expect("the floor");
        assert!(floor.y.abs() < 0.01, "{floor}");
        // A wall's collision acts as an actor surface: the ray stops on it.
        w.actors.get_mut(&C3).unwrap().solid.acts_as_actor_surface = 1;
        w.ray = crate::scripts::Ray {
            start: Vec3::new(0.0, 1.0, 0.0),
            end: Vec3::new(0.0, 1.0, 6.0),
        };
        let (wall, _) = w.ray_contact(&mut e).expect("the wall");
        assert!((wall.z - (3.0 - half.z)).abs() < 0.01, "{wall}");
        // The ground finder stands on the barrel either way.
        w.actors.get_mut(&C3).unwrap().solid.acts_as_actor_surface = 0;
        let mut over = Actor::blank();
        over.position = Vec3::new(0.0, 3.0, 3.0);
        w.actors.insert(C2, over);
        let (ground, ..) = w.ground(C2, &mut e).expect("ground");
        assert!((ground.y - 2.0).abs() < 0.01, "{ground}");
    }

    /// The review of AH.18: `turn to` a hero turns him at once, his facing
    /// his controller's, and its block runs.
    #[test]
    fn turn_to_turns_a_hero_at_once() {
        let mut e = pit();
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0));
        let mut w = world();
        let call = op(
            Class::TurnTo,
            HERO,
            vec![("_facingRHS", Some(Value::Float(-90.0)))],
            Vec::new(),
        );
        assert!(matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }));
        assert_eq!(e.hero.as_ref().unwrap().facing, 270.0);
    }
}

/// What AH.19's hurt chain asks of the VM and the host, on a synthetic
/// chain written in code the hurt chain's way (our numbers): a hit's
/// damage, a critical by a fraction rolled 0-100 under a chance, a hit
/// turned away by a whole number rolled 0-100 under half the armour, a
/// push (the creature's deceleration put to 0 and slid back to 600 over
/// the knock time with ease out 50, its velocity displaced 80 ft/s along a
/// heading, its resistance up half the strength, the push ended at its
/// time) carried by guac-motion's controller on the parry twin, and a kill
/// taking it off.
#[cfg(test)]
mod hurt_tests {
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph};
    use guac_motion::tuning::FOOT;
    use guac_script::{Generator, Mode, Program, Role, Unit, Value, Vm, Whitelist};

    use super::fight_tests::{C1, Pit, STEP, chompy, pit, world};
    use super::*;
    use crate::scripts::{WorldHost, headless_spawner};

    const INT: &str = "IntMeasurement";
    const FLOAT: &str = "FloatMeasurement";

    fn my() -> El<'static> {
        El::Member("optoplevelbehavior", "my")
    }

    fn decel() -> [El<'static>; 4] {
        [
            my(),
            El::Member("actorinfo", "movement"),
            El::Member("actorphysics", "ground deceleration"),
            El::Null,
        ]
    }

    /// The synthetic chain: a creature whose every tick ends its push once
    /// its time is up, then takes a hit when `go` reads 1.
    fn chain() -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let v = |w: &mut Writer<'_>, n: &str, c: &str| w.variable(n, "ValueInfo", Some(c));
            let go = v(&mut w, "go", INT);
            let hp = v(&mut w, "hp", FLOAT);
            let armor = v(&mut w, "armor", INT);
            let chance = v(&mut w, "chance", INT);
            let damage = v(&mut w, "damage", FLOAT);
            let strength = v(&mut w, "strength", INT);
            let heading = v(&mut w, "heading", FLOAT);
            let resist = v(&mut w, "resist", FLOAT);
            let end = v(&mut w, "knock end", FLOAT);
            let roll = v(&mut w, "roll", FLOAT);
            let t = v(&mut w, "knock time", FLOAT);
            let (crits, turned, kills, hits) = (
                v(&mut w, "crits", INT),
                v(&mut w, "turned", INT),
                v(&mut w, "kills", INT),
                v(&mut w, "hits", INT),
            );
            let timer = [El::Member("system", "script timer")];
            let decel = decel();
            let decel = &decel[..3];
            w.script(
                "C:/tfb/Synthetic/Hurt.ai",
                |_| {},
                |_| {},
                |w| {
                    let body = w.behaviour("idle");
                    // The push's end: its deceleration back, the creature
                    // stopped, as the combat manager's clear leaves it.
                    let zero = w.int(0);
                    let pushing = w.check(&[El::Local(end)], 4, zero);
                    let at = w.rhs(FLOAT, &[El::Local(end)]);
                    let due = w.check(&timer, 2, at);
                    let eighty = w.int(80);
                    w.set(decel, eighty);
                    let still = w.group(&[my(), El::Member("actorinfo", "velocity")], None);
                    let o = w.op(Class::Displace, "displace");
                    let (l, h, pt) = (w.float(0.0), w.float(0.0), w.float(0.0));
                    w.refer(o, 0x1C, still);
                    // Combine 1 sets it: to nothing.
                    w.word(o, 0x20, 1);
                    w.refer(o, 0x24, l);
                    w.refer(o, 0x28, h);
                    w.refer(o, 0x2C, pt);
                    w.push(o);
                    let zero = w.int(0);
                    w.set(&[El::Local(end)], zero);
                    w.end_block(due);
                    w.end_block(pushing);
                    // A hit when asked.
                    let one = w.int(1);
                    let asked = w.check(&[El::Local(go)], 1, one);
                    let zero = w.int(0);
                    w.set(&[El::Local(go)], zero);
                    w.inc(&[El::Local(hits)]);
                    // Critical: the chance less a fraction rolled 0-100.
                    let c = w.rhs(INT, &[El::Local(chance)]);
                    let r = w.literal(FLOAT, 100f32.to_bits(), true);
                    let e = w.expr(c, Some((1, r)));
                    w.set(&[El::Local(roll)], e);
                    let zero = w.int(0);
                    let crit = w.check(&[El::Local(roll)], 4, zero);
                    let d = w.rhs(FLOAT, &[El::Local(damage)]);
                    let half = w.float(0.5);
                    let e = w.expr(d, Some((2, half)));
                    w.set(&[El::Member("system", "cast to integer")], e);
                    let d = w.rhs(FLOAT, &[El::Local(damage)]);
                    let cast = w.rhs(INT, &[El::Member("system", "cast to integer")]);
                    let e = w.expr(d, Some((0, cast)));
                    w.set(&[El::Local(damage)], e);
                    w.inc(&[El::Local(crits)]);
                    w.end_block(crit);
                    // Armour: half of it less a whole number rolled 0-100.
                    let a = w.rhs(INT, &[El::Local(armor)]);
                    let half = w.float(0.5);
                    let a = w.expr(a, Some((2, half)));
                    let r = w.literal(INT, 100, true);
                    let e = w.expr(a, Some((1, r)));
                    w.set(&[El::Local(roll)], e);
                    let zero = w.int(0);
                    let away = w.check(&[El::Local(roll)], 4, zero);
                    w.inc(&[El::Local(turned)]);
                    w.flow_end();
                    w.end_block(away);
                    // The damage, and a kill at none left.
                    let h = w.rhs(FLOAT, &[El::Local(hp)]);
                    let d = w.rhs(FLOAT, &[El::Local(damage)]);
                    let e = w.expr(h, Some((1, d)));
                    w.set(&[El::Local(hp)], e);
                    let zero = w.int(0);
                    let dead = w.check(&[El::Local(hp)], 0, zero);
                    w.inc(&[El::Local(kills)]);
                    w.remove(&[my()]);
                    w.flow_end();
                    w.end_block(dead);
                    // The push: (strength - resistance) / 100 x 0.6 s, at
                    // least 0.125.
                    let s = w.rhs(INT, &[El::Local(strength)]);
                    let r = w.rhs(FLOAT, &[El::Local(resist)]);
                    let e = w.expr(s, Some((1, r)));
                    let hundred = w.float(100.0);
                    let e = w.expr(e, Some((3, hundred)));
                    let k = w.float(0.6);
                    let e = w.expr(e, Some((2, k)));
                    w.set(&[El::Local(t)], e);
                    let floor = w.float(0.125);
                    let short = w.check(&[El::Local(t)], 3, floor);
                    let floor = w.float(0.125);
                    w.set(&[El::Local(t)], floor);
                    w.end_block(short);
                    let r = w.rhs(FLOAT, &[El::Local(resist)]);
                    let s = w.rhs(INT, &[El::Local(strength)]);
                    let half = w.float(0.5);
                    let s = w.expr(s, Some((2, half)));
                    let e = w.expr(r, Some((0, s)));
                    w.set(&[El::Local(resist)], e);
                    let zero = w.int(0);
                    w.set(decel, zero);
                    let lhs = w.value(decel);
                    let to = w.int(600);
                    let secs = w.rhs(FLOAT, &[El::Local(t)]);
                    let (out, in_) = (w.int(50), w.int(0));
                    let (sp, so) = w.open(Class::SlideValue, "slide value");
                    w.refer(so, 0x1C, lhs);
                    w.refer(so, 0x20, to);
                    w.refer(so, 0x24, secs);
                    w.refer(so, 0x28, out);
                    w.refer(so, 0x2C, in_);
                    w.end_block(sp);
                    let target = w.group(&[my(), El::Member("actorinfo", "velocity")], None);
                    let o = w.op(Class::Displace, "displace");
                    let len = w.int(80);
                    let head = w.rhs(FLOAT, &[El::Local(heading)]);
                    let pitch = w.int(0);
                    w.refer(o, 0x1C, target);
                    w.word(o, 0x20, 1);
                    w.refer(o, 0x24, len);
                    w.refer(o, 0x28, head);
                    w.refer(o, 0x2C, pitch);
                    w.push(o);
                    let now = w.rhs(FLOAT, &timer);
                    let tt = w.rhs(FLOAT, &[El::Local(t)]);
                    let e = w.expr(now, Some((0, tt)));
                    w.set(&[El::Local(end)], e);
                    w.end_block(asked);
                    w.end_block(body);
                },
            );
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "synthetic",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// The chain's creature on the twin's open ground, its locals set, and
    /// a step: the frame's slides, its tick, then the engine's frame.
    struct Bench {
        vm: Vm,
        world: ScriptWorld,
        pit: Pit,
        generator: Generator,
        id: guac_script::PlacementId,
    }

    impl Bench {
        fn new(seed: u32) -> Bench {
            let mut config = crate::scripts::fight_config(Mode::Strict);
            config.whitelist = Whitelist::All;
            let mut vm = Vm::new(chain(), config);
            let id = vm.add(C1, "Chompy", 0);
            let mut world = world();
            world.actors.insert(C1, chompy(Vec3::ZERO, 0.0));
            let mut b = Bench {
                vm,
                world,
                pit: pit(),
                generator: Generator::seeded(seed),
                id,
            };
            for (n, v) in [
                ("hp", Value::Float(20.0)),
                ("damage", Value::Float(8.0)),
                ("strength", Value::Int(25)),
                ("heading", Value::Float(0.0)),
                ("resist", Value::Float(4.0)),
            ] {
                assert!(b.vm.set_local(id, n, v));
            }
            b
        }

        fn set(&mut self, n: &str, v: Value) {
            assert!(self.vm.set_local(self.id, n, v));
        }

        fn int(&self, n: &str) -> i32 {
            self.vm
                .local(self.id, n)
                .and_then(|v| v.as_i32())
                .unwrap_or(0)
        }

        fn float(&self, n: &str) -> f32 {
            self.vm
                .local(self.id, n)
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
        }

        /// The step, and the creature's speed across the ground after it
        /// (ft/s).
        fn step(&mut self) -> f32 {
            let mut spawn = headless_spawner(1 << 40);
            {
                let mut host = WorldHost {
                    world: &mut self.world,
                    engine: &mut self.pit,
                    generator: &mut self.generator,
                    spawn: &mut spawn,
                };
                self.vm.slide(&mut host).unwrap();
                self.vm.step(&mut host).unwrap();
            }
            self.world.end_step();
            self.world.advance(STEP, &mut self.pit);
            let a = self.world.actor(C1).unwrap();
            Vec3::new(a.velocity.x, 0.0, a.velocity.z).length()
        }

        fn hit(&mut self) -> Vec<f32> {
            self.set("go", Value::Int(1));
            let mut speeds = vec![self.step()];
            while self.float("knock end") > 0.0 {
                speeds.push(self.step());
                assert!(speeds.len() < 60, "a push that never ends");
            }
            speeds
        }
    }

    /// A hit's damage and its push: 8 off 20, no critical at a chance of 0,
    /// none turned away with no armour; 80 ft/s along its heading from the
    /// hit's own frame (the creature's velocity, as the engine path has a
    /// Chompy's), slowing as the slide brings its deceleration in from
    /// nothing as x², for its knock time from the strength less the
    /// resistance, (25 - 4) / 100 x 0.6 = 0.126 s: 8 frames, then still; its
    /// resistance up half the strength. A second push on that resistance
    /// takes the floor, 0.125 s.
    #[test]
    fn a_hit_hurts_and_pushes_for_its_strength_less_the_resistance() {
        let mut b = Bench::new(7);
        let speeds = b.hit();
        assert_eq!(b.float("hp"), 12.0);
        assert_eq!((b.int("crits"), b.int("turned")), (0, 0));
        assert!((b.float("knock time") - 0.126).abs() < 1e-6);
        assert!((b.float("resist") - 16.5).abs() < 1e-6);
        // 80 ft/s the frame it's hit, then each frame the slid
        // deceleration, 600 x (k / 60 / 0.126)^2, off it.
        assert!((speeds[0] - 80.0).abs() < 1e-3, "{speeds:?}");
        let mut want = 80.0f32;
        for (k, v) in speeds.iter().enumerate().skip(1).take(7) {
            let x = k as f32 * STEP / 0.126;
            want -= 600.0 * x * x * STEP;
            assert!((v - want).abs() < 0.05, "frame {k}: {v} against {want}");
        }
        assert_eq!(
            speeds.len(),
            9,
            "8 frames moving, still the ninth: {speeds:?}"
        );
        assert_eq!(*speeds.last().unwrap(), 0.0);
        let moved = b.world.actor(C1).unwrap().position;
        assert!(moved.z > 2.0 * FOOT && moved.x.abs() < 1e-3, "{moved}");
        // The second: (25 - 16.5) / 100 x 0.6 = 0.051 s, under the floor.
        let speeds = b.hit();
        assert!((b.float("knock time") - 0.125).abs() < 1e-6);
        assert_eq!(speeds.len(), 9, "{speeds:?}");
        assert_eq!(b.float("hp"), 4.0);
    }

    /// A critical: at a chance of 100 every hit adds half its damage,
    /// rounded down, 8 to 12, as long as the roll isn't the top (one in
    /// 2^25); a chance of 6 lands one in about 17, exactly when the roll is
    /// under 6.
    #[test]
    fn a_critical_adds_half_and_lands_when_the_roll_is_under_the_chance() {
        let mut b = Bench::new(11);
        b.set("chance", Value::Int(100));
        b.set("hp", Value::Float(1000.0));
        b.hit();
        assert_eq!(b.float("hp"), 988.0);
        assert_eq!(b.int("crits"), 1);
        let mut crits = 0;
        for n in 0..300 {
            let mut b = Bench::new(100 + n);
            b.set("chance", Value::Int(6));
            let first = b.generator.clone().next_u32();
            let roll = guac_script::random_literal(Value::Float(100.0), first)
                .unwrap()
                .as_f32()
                .unwrap();
            b.hit();
            let critical = 6.0 - roll > 0.0;
            assert_eq!(b.int("crits"), i32::from(critical), "roll {roll}");
            assert_eq!(b.float("hp"), if critical { 8.0 } else { 12.0 });
            crits += i32::from(critical);
        }
        assert!((5..=40).contains(&crits), "{crits}");
    }

    /// Armour turns a hit away whole: no damage, no push, the creature
    /// still; at 200 it lands only on the roll's top.
    #[test]
    fn a_turned_away_hit_takes_nothing_and_pushes_nothing() {
        let mut b = Bench::new(3);
        b.set("armor", Value::Int(200));
        let first = b.generator.clone().next_u32();
        let roll = guac_script::random_literal(Value::Int(100), first)
            .unwrap()
            .as_i32()
            .unwrap();
        assert!(roll < 100, "the seed's roll isn't the top");
        b.set("go", Value::Int(1));
        assert_eq!(b.step(), 0.0);
        assert_eq!(b.int("turned"), 1);
        assert_eq!(b.float("hp"), 20.0);
        assert_eq!(b.float("knock end"), 0.0, "no push");
        let at = b.world.actor(C1).unwrap().position;
        assert_eq!(
            (at.x, at.z),
            (0.0, 0.0),
            "not moved across the ground: {at}"
        );
    }

    /// A kill: the third 8 takes 20 to -4, and the creature is taken off,
    /// its script ended.
    #[test]
    fn a_kill_takes_the_creature_off() {
        let mut b = Bench::new(5);
        b.hit();
        b.hit();
        b.set("go", Value::Int(1));
        b.step();
        assert_eq!(b.float("hp"), -4.0);
        assert_eq!(b.int("kills"), 1);
        assert!(b.vm.removed(b.id));
        assert!(b.world.removed(C1));
        b.set("go", Value::Int(1));
        b.step();
        assert_eq!(b.int("hits"), 3, "its script ended with it");
    }
}

/// The placed lights the point-light manager hands out (openspec AH.65).
#[cfg(test)]
mod light_tests {
    use super::*;
    use crate::script_engine::{Actor, Light, NoEngine};
    use crate::scripts::{Files, headless_objects};

    /// `global.bld`'s `PointLightTemplate` and Eruptor's
    /// `009_Eruptor_projectile_light`, as their files hold them.
    const TEMPLATE: Obj = Obj { unit: 1, index: 15 };
    const BALL_LIGHT: Obj = Obj { unit: 3, index: 21 };
    const CLONE: Handle = 0x77;
    const BALL: Handle = 0x78;

    fn light(diffuse: u32, specular: u32, linear: f32) -> Light {
        Light {
            diffuse,
            ambient: 0x0000_00FF,
            specular,
            multiplier: 1.0,
            attenuation: [0.0, linear, 0.0],
            lift: 0.0,
        }
    }

    fn world() -> ScriptWorld {
        let files = Files {
            lights: HashMap::from([
                (TEMPLATE, light(0xFFFF_FFFF, 0xFFFF_FFFF, 0.0)),
                (BALL_LIGHT, light(0x5F29_1BFF, 0x3B26_12FF, 1.5)),
            ]),
            ..Files::default()
        };
        let handles = HashMap::new();
        ScriptWorld::new(&files, &handles, headless_objects(&handles))
    }

    fn call(class: Class, first: Value) -> OpCall {
        OpCall {
            placement: 1,
            receiver: first,
            class,
            operands: vec![("_LHS", Some(first))],
            words: Vec::new(),
            sets: Vec::new(),
        }
    }

    fn member(ns: &str, name: &str, receiver: Value, other: Option<Value>) -> Access {
        Access {
            placement: 1,
            receiver: Some(receiver),
            member: MemberId::named(ns, name),
            other_point: other,
            via: None,
            op: None,
        }
    }

    fn at(name: &str, receiver: Value, other: Option<Value>) -> Access {
        member("lightinfo", name, receiver, other)
    }

    /// A clone of a placed light starts from its template's values; what
    /// the manager writes onto it reads back; a remove takes it off and a
    /// reset brings it back on with its template's values (the manager
    /// removes a light it returns to its pool and resets one it takes out).
    #[test]
    fn a_light_clone_starts_from_its_template_and_a_reset_brings_it_back() {
        let mut w = world();
        let e = &mut NoEngine;
        let made = w
            .op(
                &call(Class::Spawn, Value::Object(TEMPLATE)),
                &mut |_| CLONE,
                e,
            )
            .unwrap();
        assert!(matches!(made, Answer::Run { .. }));
        let template = light(0xFFFF_FFFF, 0xFFFF_FFFF, 0.0);
        assert_eq!(w.actor(CLONE).unwrap().light, Some(template));
        // The manager copies the asker's colours and falloff on.
        for name in [
            "diffuse color",
            "specular color",
            "linear attenuation factor",
        ] {
            let v = w
                .get(&at(name, Value::Object(BALL_LIGHT), None), e)
                .unwrap();
            w.set(&at(name, Value::Entity(CLONE), None), v, e).unwrap();
        }
        let lit = w.actor(CLONE).unwrap().light.unwrap();
        assert_eq!(
            (lit.diffuse, lit.specular, lit.attenuation),
            (0x5F29_1BFF, 0x3B26_12FF, [0.0, 1.5, 0.0])
        );
        assert_eq!(
            w.get(&at("multiplier", Value::Entity(CLONE), None), e),
            Ok(Value::Float(1.0))
        );
        w.op(&call(Class::Remove, Value::Entity(CLONE)), &mut |_| 0, e)
            .unwrap();
        assert!(w.removed(CLONE));
        w.op(&call(Class::Reset, Value::Entity(CLONE)), &mut |_| 0, e)
            .unwrap();
        assert!(!w.removed(CLONE), "a reset puts it back on");
        assert_eq!(w.actor(CLONE).unwrap().light, Some(template));
        // A member on something that isn't a light is nothing.
        w.actors.insert(BALL, Actor::blank());
        assert_eq!(
            w.get(&at("diffuse color", Value::Entity(BALL), None), e),
            Err(HostFault::Nothing)
        );
    }

    /// A file waypoint moves where a script puts it and turns as a script
    /// turns it (openspec AG.60: a push block lays its track's waypoints
    /// from its own place and faces along the first, so it stands where
    /// its file has it, as AG.7's live read does); one never turned reads
    /// nothing, its file's turn unread; an object no waypoint is has no
    /// turn.
    #[test]
    fn a_file_waypoint_moves_and_turns_as_the_scripts_put_it() {
        let mut w = world();
        let e = &mut NoEngine;
        let wp = Obj {
            unit: 4,
            index: 31186,
        };
        w.points
            .insert(wp, Vec3::new(4.787_217_6, 45.873_01, -36.796_494));
        let to = Vec3::new(2.171_143_8, 46.128_128, -36.826_355);
        w.set_point(1, Value::Object(wp), to, e).unwrap();
        assert_eq!(w.point(Value::Object(wp), e), Some(to));
        let turn = w
            .get(
                &member("matrixmeasurement", "orientation", Value::Object(wp), None),
                e,
            )
            .unwrap();
        let facing = member("orientationmeasurement", "facing", turn, None);
        assert_eq!(w.get(&facing, e), Err(HostFault::Nothing), "never turned");
        w.set(&facing, Value::Float(90.0), e).unwrap();
        assert_eq!(w.get(&facing, e), Ok(Value::Float(90.0)));
        let none = Value::Object(Obj { unit: 4, index: 1 });
        assert_eq!(
            w.get(&member("matrixmeasurement", "orientation", none, None), e),
            Err(HostFault::Nothing)
        );
        assert_eq!(
            w.set_point(1, none, to, e),
            Err(HostFault::Nothing),
            "no such waypoint"
        );
    }

    /// A clone's activator is the placement whose script spawned it
    /// (openspec AG.60: the fence's cross beam reads the fence's facing and
    /// width through it every tick, and each tick's displacements start from
    /// the fence again); a file's placement answers itself.
    #[test]
    fn a_clones_activator_is_the_placement_that_spawned_it() {
        let mut w = world();
        let e = &mut NoEngine;
        w.op(
            &call(Class::Spawn, Value::Object(TEMPLATE)),
            &mut |_| CLONE,
            e,
        )
        .unwrap();
        let activator = |w: &mut ScriptWorld, e: &mut NoEngine, of: Value| {
            w.get(&member("abstractplacement", "activator", of, None), e)
        };
        assert_eq!(
            activator(&mut w, e, Value::Entity(CLONE)),
            Ok(Value::Entity(1)),
            "the spawner, placement 1"
        );
        w.actors.insert(BALL, Actor::blank());
        assert_eq!(
            activator(&mut w, e, Value::Entity(BALL)),
            Ok(Value::Entity(BALL)),
            "itself"
        );
    }

    /// What the scripts have lit, as the TV draws it: each clone of a
    /// light on, where the manager last hung it raised by its vertical
    /// offset (the eruption's 22 ft), a removed one not.
    #[test]
    fn the_lit_lights_hang_where_the_manager_puts_them_raised_by_their_offset() {
        let mut w = world();
        let e = &mut NoEngine;
        let mut ball = Actor::blank();
        ball.position = Vec3::new(10.0, 43.0, -93.0);
        w.actors.insert(BALL, ball);
        w.op(
            &call(Class::Spawn, Value::Object(TEMPLATE)),
            &mut |_| CLONE,
            e,
        )
        .unwrap();
        w.set(
            &member(
                "abstractplacement",
                "attach node",
                Value::Entity(CLONE),
                None,
            ),
            Value::Entity(BALL),
            e,
        )
        .unwrap();
        w.set(
            &member(
                "positionmeasurement",
                "vertical offset",
                Value::Entity(CLONE),
                Some(Value::Entity(BALL)),
            ),
            Value::Float(22.0),
            e,
        )
        .unwrap();
        let lit = w.lit_lights();
        assert_eq!(lit.len(), 1);
        let (place, l) = lit[0];
        assert_eq!(l.lift, 22.0);
        let want = Vec3::new(10.0, 43.0 + 22.0 * METRES_PER_FOOT, -93.0);
        assert!(place.abs_diff_eq(want, 1e-5), "{place}");
        w.op(&call(Class::Remove, Value::Entity(CLONE)), &mut |_| 0, e)
            .unwrap();
        assert!(w.lit_lights().is_empty());
    }
}

/// A clip's and a sequence's animation info (openspec AH.49): the palette's
/// flags, what a script writes on a clip, and what a sequence started from
/// it starts with.
#[cfg(test)]
mod animation_tests {
    use guac_formats::script::Class;
    use guac_script::{Answer, MemberId, Obj, OpCall};

    use super::fight_tests::{C1, chompy, pit, world};
    use super::*;

    const TEMPLATE: Obj = Obj { unit: 4, index: 77 };

    /// Slot 0 a base clip, 1 partial, 2 additive (and partial: additive
    /// wins), 3 a base clip its file marks layered.
    fn palette(w: &mut ScriptWorld) -> Vec<Value> {
        w.set_palette(TEMPLATE, vec![1.0, 0.5, 0.8, 0.4]);
        w.set_clip_info(
            TEMPLATE,
            vec![
                ClipInfo::of(false, false, false),
                ClipInfo::of(false, true, false),
                ClipInfo::of(true, true, false),
                ClipInfo::of(false, false, true),
            ],
        );
        let mut c = chompy(Vec3::ZERO, 0.0);
        c.obj = Some(TEMPLATE);
        w.actors.insert(C1, c);
        (0..4)
            .map(|slot| {
                w.objs.value(Object::Clip {
                    template: TEMPLATE,
                    slot,
                })
            })
            .collect()
    }

    fn on(receiver: Value, ns: &str, name: &str) -> Access {
        Access {
            placement: C1,
            receiver: Some(receiver),
            member: MemberId::named(ns, name),
            other_point: None,
            via: None,
            op: None,
        }
    }

    fn play(w: &mut ScriptWorld, e: &mut super::fight_tests::Pit, clip: Value) -> Value {
        let call = OpCall {
            placement: C1,
            receiver: Value::Entity(C1),
            class: Class::StartSequence,
            operands: vec![("_LHS", Some(clip))],
            words: Vec::new(),
            sets: Vec::new(),
        };
        match w.op(&call, &mut |_| 0, e).unwrap() {
            Answer::Run {
                result: Some(s), ..
            } => s,
            other => panic!("{other:?}"),
        }
    }

    /// `type` and `is layered` read off the palette's flags, as the
    /// original's getters read the wrapper's bits: partial 1, additive 2
    /// (over partial), a partial or additive clip layered whatever its bit;
    /// a clip's weight 100 until written.
    #[test]
    fn a_clips_kind_and_layered_bit_come_from_its_palettes_flags() {
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        let read = |w: &mut ScriptWorld, e: &mut super::fight_tests::Pit, c: Value, name: &str| {
            w.get(&on(c, "animationinfo", name), e).unwrap()
        };
        let kinds: Vec<Value> = clips
            .iter()
            .map(|c| read(&mut w, &mut e, *c, "type"))
            .collect();
        assert_eq!(kinds, [0, 1, 2, 0].map(Value::Int));
        let layered: Vec<Value> = clips
            .iter()
            .map(|c| read(&mut w, &mut e, *c, "is layered"))
            .collect();
        assert_eq!(layered, [0, 1, 1, 1].map(Value::Int));
        assert_eq!(
            read(&mut w, &mut e, clips[0], "blend percent"),
            Value::Float(100.0)
        );
        // `type` takes no write.
        assert_eq!(
            w.set(
                &on(clips[0], "animationinfo", "type"),
                Value::Int(2),
                &mut e
            ),
            Err(HostFault::Nothing)
        );
    }

    /// The locomotion's way (AH.49): it sets a clip's layered bit and its
    /// looping before it plays it, and its weight on what it started. The
    /// sequence starts with the clip's looping, bit and weight; a write of
    /// the bit on a sequence playing is no write (the original's setter
    /// tests for a parent, which only a started one has), its weight its
    /// own.
    #[test]
    fn a_sequence_starts_with_what_its_clip_was_given() {
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        let set = |w: &mut ScriptWorld,
                   e: &mut super::fight_tests::Pit,
                   c: Value,
                   ns: &str,
                   name: &str,
                   v: Value| { w.set(&on(c, ns, name), v, e).unwrap() };
        set(
            &mut w,
            &mut e,
            clips[0],
            "animationinfo",
            "is layered",
            Value::Int(1),
        );
        set(
            &mut w,
            &mut e,
            clips[0],
            "sequence",
            "looping",
            Value::Int(1),
        );
        set(
            &mut w,
            &mut e,
            clips[3],
            "animationinfo",
            "blend percent",
            Value::Float(30.0),
        );
        assert_eq!(
            w.get(&on(clips[0], "sequence", "looping"), &mut e).unwrap(),
            Value::Int(1)
        );
        let walk = play(&mut w, &mut e, clips[0]);
        let s = w.sequences().last().unwrap().clone();
        assert!(s.looping && s.layered && s.weighted() && !s.laid_over());
        assert_eq!((s.kind, s.blend), (0, 100.0));
        // A clip the file marks layered starts layered; its written weight
        // goes with it.
        play(&mut w, &mut e, clips[3]);
        let s = w.sequences().last().unwrap().clone();
        assert!(s.weighted() && !s.looping);
        assert_eq!(s.blend, 30.0);
        // The bit stays as it started; the weight is the sequence's own.
        w.set(
            &on(walk, "animationinfo", "is layered"),
            Value::Int(0),
            &mut e,
        )
        .unwrap();
        w.set(
            &on(walk, "animationinfo", "blend percent"),
            Value::Float(42.0),
            &mut e,
        )
        .unwrap();
        assert_eq!(
            w.get(&on(walk, "animationinfo", "is layered"), &mut e)
                .unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            w.get(&on(walk, "animationinfo", "blend percent"), &mut e)
                .unwrap(),
            Value::Float(42.0)
        );
        assert_eq!(
            w.get(&on(clips[0], "animationinfo", "blend percent"), &mut e)
                .unwrap(),
            Value::Float(100.0)
        );
        // A clip never given its bit or looping starts with neither: a
        // throw, partial, laid over whatever weights; an additive one
        // neither weighted nor laid over (a lean, drawn by nothing yet).
        play(&mut w, &mut e, clips[1]);
        let s = w.sequences().last().unwrap().clone();
        assert!(!s.looping && s.is_layered() && !s.weighted() && s.laid_over());
        play(&mut w, &mut e, clips[2]);
        let s = w.sequences().last().unwrap().clone();
        assert!(!s.weighted() && !s.laid_over());
    }
}

/// One sequence per clip and actor (openspec AH.184), on the VM: scripts
/// written with AH.13's writer play a clip of his palette again, stop it on
/// him and on another actor, and read what plays on him, over Eruptor's
/// palette as `picks`' tests have it (his walk at slot 1, 1.0667 s; his idle
/// at slot 0, a base clip).
#[cfg(test)]
mod copy_tests {
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::Location;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph};
    use guac_script::{
        Generator, Mode, Obj, PlacementId, Program, Role, Unit, Value, Vm, Whitelist,
    };

    use super::*;
    use crate::script_engine::{DONE_MARGIN, NoEngine};
    use crate::scripts::{
        Files, Placed, WorldHost, headless_handles, headless_objects, headless_spawner,
        hero_config, init_chain,
    };

    const PATH: &str = "C:/tfb/Synthetic/Copies.ai";
    const MY: El<'static> = El::Member("optoplevelbehavior", "my");
    const PALETTE: El<'static> = El::Member("animationinfo", "animation palette");
    const PLAYING: El<'static> = El::Member("actorinfo", "playing animations");
    const INT: &str = "IntMeasurement";
    const ME: Obj = Obj {
        unit: 0,
        index: 100_000,
    };
    const STEP: f32 = 1.0 / 60.0;

    /// The script's placement and the actor it tells, run tick by tick.
    struct Run {
        vm: Vm,
        world: ScriptWorld,
        id: PlacementId,
        me: Handle,
        other: Handle,
    }

    impl Run {
        /// `script` (given the object it names the other actor by) as his,
        /// running from the level's start, strict, his palette Eruptor's.
        fn new(script: impl FnOnce(&mut Writer<'_>, Location)) -> Run {
            let mut b = IgzBuilder::new();
            let other = {
                let w = &mut Writer::new(&mut b);
                let other = w.b.object(0, "tfbActorInfo", 0xF8);
                script(w, other);
                other
            };
            let bytes = b.build();
            let igz = Igz::parse(&bytes).unwrap();
            let graph = Graph::read(&igz).unwrap();
            let program = Arc::new(
                Program::link(&[Unit {
                    name: "level",
                    role: Role::Other,
                    igz: &igz,
                    graph: &graph,
                }])
                .unwrap(),
            );
            let other = Obj {
                unit: 0,
                index: igz.objects().iter().position(|o| o.at == other).unwrap() as u32,
            };
            let files = Files {
                placed: vec![
                    Placed::named(ME, "walker", Some(PATH), 4),
                    Placed::named(other, "other", None, 4),
                ],
                ..Files::default()
            };
            let handles = headless_handles(&files);
            let mut c = hero_config(Mode::Strict);
            c.whitelist = Whitelist::All;
            let vm = init_chain(program, &files, &handles, &[0], c);
            let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            world.set_palette_of(ME, &crate::picks::tests::palette());
            let me = handles[&ME];
            Run {
                id: vm.placement_of(me).unwrap(),
                vm,
                world,
                me,
                other: handles[&other],
            }
        }

        /// One tick of the scripts and the engine's frame after it.
        fn tick(&mut self) {
            let mut spawn = headless_spawner(1 << 40);
            let mut host = WorldHost {
                world: &mut self.world,
                engine: &mut NoEngine,
                generator: &mut Generator::seeded(1),
                spawn: &mut spawn,
            };
            self.vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
            self.world.advance(STEP, &mut NoEngine);
        }

        /// The sequences of palette slot `slot` on `actor`, in the order
        /// started, by index.
        fn copies(&self, actor: Handle, slot: u32) -> Vec<(usize, &Sequence)> {
            self.world
                .sequences()
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.actor == actor && self.world.clip_of(s).is_some_and(|(_, x)| x == slot)
                })
                .collect()
        }

        fn local(&self, name: &str) -> Value {
            self.vm.local(self.id, name).unwrap_or(Value::Null)
        }

        /// What a reference local names, as the host's object.
        fn names(&self, name: &str) -> Option<Object> {
            match self.local(name) {
                Value::Entity(h) => self.world.objs.get(h),
                _ => None,
            }
        }
    }

    /// `setref <var> = my.animation palette [slot]`.
    fn clip(w: &mut Writer<'_>, var: Location, slot: i32) {
        let lhs = w.value(&[El::Local(var)]);
        let rhs = w.reference(&[MY, PALETTE], None);
        let index = w.int(slot);
        let o = w.op(Class::SetReference, "setref||=");
        w.refer(o, 0x1C, lhs);
        w.refer(o, 0x20, rhs);
        w.refer(o, 0x24, index);
        w.push(o);
    }

    /// `setref <lhs> = <rhs>`.
    fn setref(w: &mut Writer<'_>, lhs: &[El<'_>], rhs: &[El<'_>]) {
        let l = w.value(lhs);
        let r = w.reference(rhs, None);
        let o = w.op(Class::SetReference, "set reference");
        w.refer(o, 0x1C, l);
        w.refer(o, 0x20, r);
        w.push(o);
    }

    /// `start sequence <what> { body }`: the op, which a path reads as what
    /// it started.
    fn start(
        w: &mut Writer<'_>,
        what: &[El<'_>],
        body: impl FnOnce(&mut Writer<'_>, Location),
    ) -> Location {
        let lhs = w.group(what, None);
        let (p, o) = w.open(Class::StartSequence, "start sequence");
        w.refer(o, 0x1C, lhs);
        body(w, o);
        w.end_block(p);
        o
    }

    /// `stop sequence <what>`.
    fn stop(w: &mut Writer<'_>, what: &[El<'_>]) {
        let np = w.group(what, None);
        let o = w.op(Class::StopSequence, "stop sequence");
        w.refer(o, 0x1C, np);
        w.push(o);
    }

    /// `tell <what> { body }`.
    fn tell(w: &mut Writer<'_>, what: &[El<'_>], body: impl FnOnce(&mut Writer<'_>)) {
        let lhs = w.group(what, None);
        let (p, o) = w.open(Class::Control, "control||||controlled");
        w.refer(o, 0x20, lhs);
        body(w);
        w.end_block(p);
    }

    /// `if <var> == <n> { body }`.
    fn when(w: &mut Writer<'_>, var: Location, n: i32, body: impl FnOnce(&mut Writer<'_>)) {
        let rhs = w.int(n);
        let p = w.check(&[El::Local(var)], 1, rhs);
        body(w);
        w.end_block(p);
    }

    /// A clip played on him while its copy plays is that copy (aj-vm's
    /// 0x68e2c, read as an aid): his walk, played every tick, is one
    /// sequence; the play is refused (its block skipped, the op reading as
    /// nothing after) while the copy has time left, and starts it again in
    /// its pass's last two frames (0x118d28's test of what's left, by
    /// [`DONE_MARGIN`]); with its `allow restart` set every play starts it
    /// again; stopped by its clip and played again by itself, it's still
    /// the one copy.
    #[test]
    fn a_clip_played_twice_on_one_actor_is_one_sequence_started_again() {
        let mut r = Run::new(|w, _| {
            let clip_var = w.variable("clip", "ScriptReference", None);
            let kept = w.variable("kept", "ScriptReference", None);
            let last = w.variable("last", "ScriptReference", None);
            let started = w.variable("started", "ValueInfo", Some(INT));
            let phase = w.variable("phase", "ValueInfo", Some(INT));
            w.script(
                PATH,
                |w| {
                    clip(w, clip_var, 1);
                    let one = w.int(1);
                    w.set(
                        &[El::Local(clip_var), El::Member("sequence", "looping")],
                        one,
                    );
                },
                |_| {},
                |w| {
                    let b = w.behaviour("walk");
                    when(w, phase, 0, |w| {
                        let o = start(w, &[El::Local(clip_var)], |w, o| {
                            w.inc(&[El::Local(started)]);
                            setref(w, &[El::Local(kept)], &[El::Local(o)]);
                        });
                        setref(w, &[El::Local(last)], &[El::Local(o)]);
                    });
                    when(w, phase, 1, |w| {
                        let one = w.int(1);
                        w.set(
                            &[El::Local(kept), El::Member("sequence", "allow restart")],
                            one,
                        );
                        start(w, &[El::Local(clip_var)], |w, _| {
                            w.inc(&[El::Local(started)]);
                        });
                    });
                    when(w, phase, 2, |w| {
                        stop(w, &[El::Local(clip_var)]);
                        start(w, &[El::Local(kept)], |w, _| {
                            w.inc(&[El::Local(started)]);
                        });
                        let four = w.int(4);
                        w.set(&[El::Local(phase)], four);
                    });
                    w.end_block(b);
                },
            );
        });
        let me = r.me;
        let (mut started, mut restarts) = (0, 0);
        for k in 0..80 {
            // Due a start: none yet, or its pass's last two frames.
            let due = r
                .copies(me, 1)
                .first()
                .is_none_or(|(_, s)| s.time_remaining() <= 0.0);
            let since = r.world.start_count();
            r.tick();
            let copies = r.copies(me, 1);
            assert_eq!(copies.len(), 1, "tick {k}: one copy of his walk");
            let (i, s) = copies[0];
            assert!(s.plays() && s.looping, "tick {k}");
            if due {
                started += 1;
                restarts += usize::from(k > 0);
            }
            assert_eq!(r.local("started"), Value::Int(started), "tick {k}");
            assert_eq!(r.names("kept"), Some(Object::Sequence(i as u32)));
            // The op reads as what it started, nothing when refused.
            let last = due.then_some(Object::Sequence(i as u32));
            assert_eq!(r.names("last"), last, "tick {k}");
            assert_eq!(r.world.restarts().len(), restarts, "tick {k}");
            // What the scripts' log reads as started this tick.
            let want: Vec<usize> = if due { vec![i] } else { Vec::new() };
            assert_eq!(r.world.started_since(since), want, "tick {k}");
        }
        assert_eq!((started, restarts), (2, 1), "his walk's second pass");
        let left = r.copies(me, 1)[0].1.length - r.copies(me, 1)[0].1.time;
        assert!(left > DONE_MARGIN, "{left}");
        // With its restart flag every play starts it again.
        r.vm.set_local(r.id, "phase", Value::Int(1));
        let before = r.world.start_count();
        for k in 0..10 {
            r.tick();
            started += 1;
            assert_eq!(r.local("started"), Value::Int(started), "tick {k}");
            let copies = r.copies(me, 1);
            assert_eq!(copies.len(), 1);
            assert!(
                (copies[0].1.time - STEP).abs() < 1e-6,
                "{}",
                copies[0].1.time
            );
        }
        // Ten starts of the one copy: the log reads it once over them.
        let i = r.copies(me, 1)[0].0;
        assert_eq!(
            r.world.started_since(before),
            vec![i],
            "once, not ten times"
        );
        // Stopped by its clip, then played by itself: the same copy again.
        r.vm.set_local(r.id, "phase", Value::Int(2));
        r.tick();
        assert_eq!(r.local("started"), Value::Int(started + 1));
        let copies = r.copies(me, 1);
        assert_eq!(copies.len(), 1);
        assert!(copies[0].1.plays() && (copies[0].1.time - STEP).abs() < 1e-6);
        assert_eq!(r.local("phase"), Value::Int(4));
    }

    /// A stop of a clip stops its copy on the receiver alone (the stop hands
    /// the clip the receiver, aj-vm's 0x9ba54; the clip stops its clones on
    /// it, 0x9ac14): his walk and another actor's copy of the same clip,
    /// his stopped by his stop, his idle playing on, the other's walk
    /// playing on until a stop told to it. His copy, playing, played on the
    /// other is a play of its clip on the other, a copy of the other's own
    /// (0x68e2c: a copy another actor holds hands the play to its clip); a
    /// copy stopped is no one's (the stop clears its activator), so his,
    /// played on the other then, is the other's now.
    #[test]
    fn a_stop_of_a_clip_stops_its_copy_on_the_receiver_alone() {
        let mut r = Run::new(|w, other| {
            let clip_var = w.variable("clip", "ScriptReference", None);
            let idle_var = w.variable("idle", "ScriptReference", None);
            let mine = w.variable("mine", "ScriptReference", None);
            let phase = w.variable("phase", "ValueInfo", Some(INT));
            w.script(
                PATH,
                |w| {
                    clip(w, clip_var, 1);
                    start(w, &[El::Local(clip_var)], |w, o| {
                        setref(w, &[El::Local(mine)], &[El::Local(o)]);
                    });
                    clip(w, idle_var, 0);
                    start(w, &[El::Local(idle_var)], |_, _| {});
                },
                |_| {},
                |w| {
                    let b = w.behaviour("walk");
                    when(w, phase, 7, |w| {
                        tell(w, &[El::Local(other)], |w| {
                            start(w, &[El::Local(mine)], |_, _| {});
                        });
                        let zero = w.int(0);
                        w.set(&[El::Local(phase)], zero);
                    });
                    when(w, phase, 1, |w| {
                        stop(w, &[El::Local(clip_var)]);
                        let two = w.int(2);
                        w.set(&[El::Local(phase)], two);
                    });
                    when(w, phase, 3, |w| {
                        tell(w, &[El::Local(other)], |w| stop(w, &[El::Local(clip_var)]));
                        let four = w.int(4);
                        w.set(&[El::Local(phase)], four);
                    });
                    when(w, phase, 5, |w| {
                        tell(w, &[El::Local(other)], |w| {
                            start(w, &[El::Local(mine)], |_, _| {});
                        });
                        let six = w.int(6);
                        w.set(&[El::Local(phase)], six);
                    });
                    w.end_block(b);
                },
            );
        });
        let (me, other) = (r.me, r.other);
        let plays = |r: &Run, actor| -> Vec<bool> {
            r.copies(actor, 1).iter().map(|(_, s)| s.plays()).collect()
        };
        r.tick();
        r.tick();
        assert_eq!((plays(&r, me), plays(&r, other)), (vec![true], vec![]));
        let his = r.copies(me, 1)[0].0;
        assert_eq!(r.names("mine"), Some(Object::Sequence(his as u32)));
        let n = r.world.sequences().len();
        r.vm.set_local(r.id, "phase", Value::Int(7));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(0));
        assert_eq!(r.world.sequences().len(), n + 1, "a copy of the other's");
        assert_eq!((plays(&r, me), plays(&r, other)), (vec![true], vec![true]));
        assert_eq!(r.copies(me, 1)[0].0, his, "his still his");
        r.vm.set_local(r.id, "phase", Value::Int(1));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(2));
        assert_eq!((plays(&r, me), plays(&r, other)), (vec![false], vec![true]));
        let idle: Vec<bool> = r.copies(me, 0).iter().map(|(_, s)| s.plays()).collect();
        assert_eq!(idle, [true], "his idle plays on");
        r.vm.set_local(r.id, "phase", Value::Int(3));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(4));
        assert_eq!(
            (plays(&r, me), plays(&r, other)),
            (vec![false], vec![false])
        );
        let started = r.world.sequences().len();
        r.vm.set_local(r.id, "phase", Value::Int(5));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(6));
        assert_eq!(r.world.sequences().len(), started, "no new copy");
        assert_eq!(plays(&r, me), Vec::<bool>::new(), "his copy is the other's");
        assert_eq!(plays(&r, other), vec![true, false], "his, then its own");
    }

    /// `actorinfo.playing animations` holds the copies playing on him, not
    /// their clips (aj-vm's 0x6aa68; 0x118b10 inserts a whole base clip's
    /// copy at the front and appends the rest): his walk, layered, then his
    /// idle, a whole clip; the set counts two, its first his idle's copy;
    /// the walk's copy is in it, the walk's clip isn't; the walk's clip's
    /// `allow restart`, set before it played, is the clip's and its copy's;
    /// his walk's copy stopped, the set holds his idle's alone.
    #[test]
    fn the_playing_set_holds_his_copies_a_whole_clips_first() {
        let mut r = Run::new(|w, _| {
            let walk_clip = w.variable("walk clip", "ScriptReference", None);
            let idle_clip = w.variable("idle clip", "ScriptReference", None);
            let walk = w.variable("walk", "ScriptReference", None);
            let idle = w.variable("idle", "ScriptReference", None);
            let first = w.variable("first", "ScriptReference", None);
            let count = w.variable("count", "ValueInfo", Some(INT));
            let copy_in = w.variable("copy in", "ValueInfo", Some(INT));
            let clip_in = w.variable("clip in", "ValueInfo", Some(INT));
            let walk_restarts = w.variable("walk restarts", "ValueInfo", Some(INT));
            let idle_restarts = w.variable("idle restarts", "ValueInfo", Some(INT));
            let clip_restarts = w.variable("clip restarts", "ValueInfo", Some(INT));
            let phase = w.variable("phase", "ValueInfo", Some(INT));
            w.script(
                PATH,
                |w| {
                    clip(w, walk_clip, 1);
                    for member in [
                        ("animationinfo", "is layered"),
                        ("sequence", "allow restart"),
                    ] {
                        let one = w.int(1);
                        w.set(&[El::Local(walk_clip), El::Member(member.0, member.1)], one);
                    }
                    start(w, &[El::Local(walk_clip)], |w, o| {
                        setref(w, &[El::Local(walk)], &[El::Local(o)]);
                    });
                    clip(w, idle_clip, 0);
                    start(w, &[El::Local(idle_clip)], |w, o| {
                        setref(w, &[El::Local(idle)], &[El::Local(o)]);
                    });
                },
                |_| {},
                |w| {
                    let b = w.behaviour("walk");
                    when(w, phase, 1, |w| {
                        stop(w, &[El::Local(walk)]);
                        let two = w.int(2);
                        w.set(&[El::Local(phase)], two);
                    });
                    let n = w.rhs(INT, &[MY, PLAYING, El::Member("setvariant", "count")]);
                    w.set(&[El::Local(count)], n);
                    setref(
                        w,
                        &[El::Local(first)],
                        &[MY, PLAYING, El::Member("setvariant", "first")],
                    );
                    for (element, flag) in [(walk, copy_in), (walk_clip, clip_in)] {
                        let p = w.check_in(&[MY, PLAYING], 0, &[El::Local(element)]);
                        let one = w.int(1);
                        w.set(&[El::Local(flag)], one);
                        w.end_block(p);
                    }
                    for (copy, flag) in [
                        (walk, walk_restarts),
                        (idle, idle_restarts),
                        (walk_clip, clip_restarts),
                    ] {
                        let r = w.rhs(
                            INT,
                            &[El::Local(copy), El::Member("sequence", "allow restart")],
                        );
                        w.set(&[El::Local(flag)], r);
                    }
                    w.end_block(b);
                },
            );
        });
        r.tick();
        r.tick();
        let (walk, idle) = (r.copies(r.me, 1), r.copies(r.me, 0));
        assert_eq!((walk.len(), idle.len()), (1, 1));
        let (walk, idle) = (walk[0].0 as u32, idle[0].0 as u32);
        assert!(walk < idle, "his walk started first");
        assert_eq!(r.names("walk"), Some(Object::Sequence(walk)));
        assert_eq!(r.local("count"), Value::Int(2));
        assert_eq!(
            r.names("first"),
            Some(Object::Sequence(idle)),
            "the whole clip's first"
        );
        assert_eq!(r.local("copy in"), Value::Int(1), "the copy is in the set");
        assert_eq!(r.local("clip in"), Value::Null, "its clip isn't");
        // A clip's `allow restart` goes with the copy it starts.
        assert_eq!(r.local("clip restarts"), Value::Int(1));
        assert_eq!(r.local("walk restarts"), Value::Int(1));
        assert_eq!(r.local("idle restarts"), Value::Int(0));
        // His walk's copy stopped: no longer in the set.
        r.vm.set_local(r.id, "phase", Value::Int(1));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(2));
        assert_eq!(r.local("count"), Value::Int(1));
        assert_eq!(r.names("first"), Some(Object::Sequence(idle)));
    }

    /// A copy played out is its clip's no longer: the engine's sweep takes
    /// a copy that doesn't loop and has nothing left out of its clip's
    /// copies (aj-vm's 0x118568 through 0x1183f4, read as aids), so the
    /// clip's next play on him makes a new one: his walk, not looping,
    /// played out, then played again.
    #[test]
    fn a_clip_played_once_its_copy_played_out_is_a_new_copy() {
        let mut r = Run::new(|w, _| {
            let clip_var = w.variable("clip", "ScriptReference", None);
            let phase = w.variable("phase", "ValueInfo", Some(INT));
            w.script(
                PATH,
                |w| {
                    clip(w, clip_var, 1);
                    start(w, &[El::Local(clip_var)], |_, _| {});
                },
                |_| {},
                |w| {
                    let b = w.behaviour("walk");
                    when(w, phase, 1, |w| {
                        start(w, &[El::Local(clip_var)], |_, _| {});
                        let two = w.int(2);
                        w.set(&[El::Local(phase)], two);
                    });
                    w.end_block(b);
                },
            );
        });
        let me = r.me;
        r.tick();
        assert_eq!(r.copies(me, 1).len(), 1, "his walk started");
        let mut ticks = 0;
        while r.copies(me, 1)[0].1.plays() {
            r.tick();
            ticks += 1;
            assert!(ticks < 200, "his walk never played out");
        }
        let n = r.world.sequences().len();
        r.vm.set_local(r.id, "phase", Value::Int(1));
        r.tick();
        assert_eq!(r.local("phase"), Value::Int(2));
        assert_eq!(r.world.sequences().len(), n + 1, "a new copy");
        let plays: Vec<bool> = r.copies(me, 1).iter().map(|(_, s)| s.plays()).collect();
        assert_eq!(plays, [false, true], "the played-out one, then the new one");
    }
}
