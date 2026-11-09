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
//! - `contactinfo.type` is the top six bits of the triangle's collision
//!   material byte, -1 with no ground found (openspec AS.152: a surface's
//!   type runs 0-63, the scripts' own top; an elemental surface's is 40 plus
//!   its element, so `HatBox_Pickup_water`'s 164 reads 41, `_lava`'s 180 45
//!   and chapter 1's `_wind_Area`'s 168 42, which is what the portal manager
//!   gives water's, fire's and air's heroes; what the low two bits say isn't
//!   read); a hero's own reads 0, his footing keeping no material;
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
use guac_motion::seam::{BodyId, CAST_SLOP, PhysicsSeam, Pose, Quat, Shape, Vec3};
use guac_motion::solid::{HavokSolid, euler};
use guac_motion::steer::{Aim, MoveType, Physics as SteerPhysics};
use guac_motion::stick::Push;
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

/// A collision material byte to the surface type the scripts read (the
/// module docs).
const SURFACE_SHIFT: u8 = 2;

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
    /// `system.save state`: the save system's state ([`crate::story_save`],
    /// openspec AS.38).
    SaveState,
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
    /// An attach node's root: the actor it's on (openspec AS.47).
    AttachRoot,
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
    /// `sequence.playback mode` (openspec AU.53): 0 one way, 1 ping pong.
    SeqPlaybackMode,
    /// `sequence.allow restart` (openspec AH.184): a clip's, which a copy
    /// started from it takes, or a sequence's own.
    AllowRestart,
    /// `clonedsequence.clones` (openspec AS.37): a template's copies
    /// playing, on any actor, read whole.
    SeqClones,
    /// `clonedsequence.activator` (openspec AR.10): the actor a copy plays
    /// on.
    SeqActivator,
    /// A sound's `soundinfo` member (openspec AR.10).
    Sound(crate::sound_members::SoundField),
    /// The sound system's (openspec AR.10).
    SoundSystem(crate::sound_members::SystemField),
    // AH.49's: a clip's or a sequence's animation info.
    /// `animationinfo.blend percent`: the weight the engine blends by.
    Blend,
    /// `animationinfo.is layered`.
    IsLayered,
    /// `animationinfo.walk` and `animationinfo.run`, read only: the clip a
    /// `turn to` or `move to` plays on the way. Nothing yet: what asks it on
    /// chapter 1 (the Giant's turn to the cracked floor) turns as well
    /// without one, and AS.39 reads the actor's own.
    /// Stand-in ledger: `locomotion-clip`.
    LocomotionClip,
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
    /// `viewportinfo.audio listener` (openspec AR.11): the viewport itself,
    /// the camera standing as the listener (a hypothesis).
    AudioListener,
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
        add("system", "save state", Member::SaveState);
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
        add("abstractattachnode", "root", Member::AttachRoot);
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
        add("sequence", "playback mode", Member::SeqPlaybackMode);
        add("sequence", "allow restart", Member::AllowRestart);
        add("clonedsequence", "clones", Member::SeqClones);
        add("clonedsequence", "activator", Member::SeqActivator);
        for &(name, f) in crate::sound_members::SOUND_FIELDS {
            add("soundinfo", name, Member::Sound(f));
        }
        for &(name, f) in crate::sound_members::SYSTEM_FIELDS {
            add("soundinfo", name, Member::SoundSystem(f));
        }
        add("animationinfo", "blend percent", Member::Blend);
        add("animationinfo", "is layered", Member::IsLayered);
        add("animationinfo", "type", Member::ClipKind);
        add("animationinfo", "walk", Member::LocomotionClip);
        add("animationinfo", "run", Member::LocomotionClip);
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
        add("viewportinfo", "audio listener", Member::AudioListener);
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

    /// A reset of an actor's movement or collision member (openspec AU.53):
    /// the field back to its file's value, a clone's its template's (a
    /// scene's jump takes its actor's shape off for the arc and resets it to
    /// come down on the ground). Whether `m` is one; a field its file holds
    /// no values for is left as it is.
    pub(crate) fn reset_facet(&mut self, m: Member, receiver: Option<Value>) -> bool {
        match (m, self.object(receiver)) {
            (Member::Physics(f), Some(Object::Facet(h, Facet::Movement))) => {
                let file = self
                    .actors
                    .get(&h)
                    .and_then(|a| a.obj.or(a.template))
                    .and_then(|o| self.template(o))
                    .and_then(|p| p.physics);
                if let (Some(file), Some(a)) = (file, self.actors.get_mut(&h)) {
                    let p = &mut a.physics;
                    use PhysicsField as P;
                    match f {
                        P::GroundMaxSpeed => p.ground_max_speed = file.ground_max_speed,
                        P::GroundAcceleration => p.ground_acceleration = file.ground_acceleration,
                        P::GroundDeceleration => p.ground_deceleration = file.ground_deceleration,
                        P::GroundTurnRate => p.ground_turn_rate = file.ground_turn_rate,
                        P::AirMaxSpeed => p.air_max_speed = file.air_max_speed,
                        P::AirAcceleration => p.air_acceleration = file.air_acceleration,
                        P::AirDeceleration => p.air_deceleration = file.air_deceleration,
                        P::AirTurnRate => p.air_turn_rate = file.air_turn_rate,
                        P::GravityMaxSpeed => p.gravity_max_speed = file.gravity_max_speed,
                        P::GravityAcceleration => {
                            p.gravity_acceleration = file.gravity_acceleration
                        }
                        P::JumpMaxSpeed => p.jump_max_speed = file.jump_max_speed,
                        P::JumpAcceleration => p.jump_acceleration = file.jump_acceleration,
                        P::JumpInitialSpeed => p.jump_initial_speed = file.jump_initial_speed,
                        P::MoveType => p.move_type = file.move_type,
                        P::TiltsWithGround => p.tilts_with_ground = file.tilts_with_ground,
                        P::TimeScale => p.time_scale = file.time_scale,
                    }
                }
                true
            }
            (Member::Solid(f), Some(Object::Facet(h, Facet::Collision))) => {
                let file = self
                    .actors
                    .get(&h)
                    .and_then(|a| a.obj.or(a.template))
                    .and_then(|o| self.template(o))
                    .and_then(|p| p.solid);
                if let (Some(file), Some(a)) = (file, self.actors.get_mut(&h)) {
                    let s = &mut a.solid;
                    use SolidField as S;
                    match f {
                        S::Width => s.width = file.width,
                        S::Length => s.length = file.length,
                        S::Height => s.height = file.height,
                        S::Offset(k) => s.offset[k] = file.offset[k],
                        S::Weight => s.weight = file.weight,
                        S::Rebound => s.rebound = file.rebound,
                        S::InteractsWithActors => {
                            s.interacts_with_actors = file.interacts_with_actors
                        }
                        S::InteractsWithLevelMesh => {
                            s.interacts_with_level_mesh = file.interacts_with_level_mesh
                        }
                        S::ActsAsActorSurface => {
                            s.acts_as_actor_surface = file.acts_as_actor_surface
                        }
                        S::Shape => s.shape = file.shape,
                    }
                }
                true
            }
            _ => false,
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

    /// A script set the reference in cell `cell` to name `target` (crate
    /// docs of guac-script, Variables, structures and sets; openspec
    /// AH.250): an actor it names keeps where that actor stands now as the
    /// reference's point, so the hero's death actor lifts him over where he
    /// fell and the stat-buff icon's measurement actor rises over where it
    /// was put.
    pub(crate) fn referenced(&mut self, cell: u32, target: Value, engine: &mut dyn Engine) {
        let Some(h) = self.actor_of(Some(target)) else {
            return;
        };
        if let Some(at) = self.position(h, engine) {
            self.kept_via.insert((h, cell), at);
        }
    }

    /// The actor (or hero) a receiver names: `None` for a host object.
    pub(crate) fn actor_of(&self, v: Option<Value>) -> Option<Handle> {
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
            && let Some((p, turn)) = self.node_frame(owner, i, engine, depth - 1)
        {
            return Some(p + turn * a.hung);
        }
        Some(a.position)
    }

    /// Node `i` of actor `owner` and the frame what hangs from it is offset
    /// in: its place, and its turn as last posed, else its owner's facing.
    fn node_frame(
        &self,
        owner: Handle,
        i: u32,
        engine: &mut dyn Engine,
        depth: u8,
    ) -> Option<(Vec3, Quat)> {
        let p = self.node_within(owner, i, engine, depth)?;
        let turn = self.node_turn(owner, i, engine, depth).unwrap_or_else(|| {
            let facing = self
                .angles_within(owner, engine, depth)
                .map_or(0.0, |g| g.facing);
            Quat::from_rotation_y(facing.to_radians())
        });
        Some((p, turn))
    }

    /// What actor `h` hangs from, as the frame its offset is kept in
    /// ([`crate::script_engine::Actor::hung`]): a node's place and turn, or
    /// another actor's pivot and facing. None while it hangs from nothing,
    /// from itself or from a ground.
    fn hang_frame(&self, h: Handle, engine: &mut dyn Engine) -> Option<(Vec3, Quat)> {
        let a = self.actors.get(&h)?;
        if a.removed {
            return None;
        }
        match self.object(Some(a.attach)) {
            Some(Object::Node(owner, i)) if owner != h => {
                self.node_frame(owner, i, engine, HUNG_DEPTH - 1)
            }
            None => {
                let owner = self.actor_of(Some(a.attach)).filter(|&o| o != h)?;
                let p = self.position_within(owner, engine, HUNG_DEPTH - 1)?;
                let facing = self
                    .angles_within(owner, engine, HUNG_DEPTH - 1)
                    .map_or(0.0, |g| g.facing);
                Some((p, Quat::from_rotation_y(facing.to_radians())))
            }
            _ => None,
        }
    }

    /// Hung `h` hung so the next placing sets it at `p`: its offset from
    /// what it hangs from (openspec AS.189: a turret's get-out carrying the
    /// hero hung on it). False while it hangs from nothing.
    pub(crate) fn hang_at(&mut self, h: Handle, p: Vec3, engine: &mut dyn Engine) -> bool {
        let Some((at, turn)) = self.hang_frame(h, engine) else {
            return false;
        };
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.hung = turn.inverse() * (p - at);
                true
            }
            None => false,
        }
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
        // Hung, it hangs where it's put: its offset from what it hangs from
        // (openspec AH.226).
        let hung = self
            .hang_frame(h, engine)
            .map(|(at, turn)| turn.inverse() * (p - at));
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.position = p;
                if let Some(off) = hung {
                    a.hung = off;
                }
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
        // a hypothesis, no oracle read has a sleeping one's ground). Taken
        // off, it has none either: Setup_004B's waves count their clones
        // left standing by their ground's type (openspec AS.97).
        if a.removed || (a.managed && !a.active) {
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
            Some(Object::Facet(a, Facet::Ground)) => self
                .ground(a, engine)
                .map(|g| g.0)
                .or_else(|| fall_point(engine.hero(a)?)),
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
        // A new shot's rig stands at the origin until it's drawn and posed
        // (openspec AS.145): the view holds, and what hangs on it.
        if self.rig_unposed(a) {
            return Some(
                self.rig_held
                    .get(i as usize)
                    .map_or(self.view.eye, |(at, _)| *at),
            );
        }
        let pivot = self.position_within(a, engine, depth)?;
        let posed = self.nodes.get(&a).and_then(|n| n.get(i as usize));
        let Some(off) = posed.copied().or_else(|| self.origin_node(a, i)) else {
            return Some(pivot);
        };
        let facing = self.angles(a, engine).map_or(0.0, |g| g.facing);
        Some(pivot + Quat::from_rotation_y(facing.to_radians()) * off.at)
    }

    /// Attach node `i` of a model nothing poses, off its list at the actor's
    /// scale ([`ScriptWorld::set_origin_nodes`]).
    fn origin_node(&self, a: Handle, i: u32) -> Option<crate::script_engine::NodePose> {
        let actor = self.actors.get(&a)?;
        let placed = actor.template.or(actor.obj)?;
        let model = self.templates.get(&placed)?.model.as_deref()?;
        let node = self.origin_nodes.get(model)?.get(i as usize)?;
        Some(crate::script_engine::NodePose {
            at: node.at * Vec3::from_array(actor.scale) / 100.0,
            turn: node.turn,
        })
    }

    /// Attach node `i` of actor `a`'s turn in the level, where the embedder
    /// posed it ([`ScriptWorld::set_nodes`]): its own turned by the actor's
    /// facing as it stands. None for a node never posed.
    fn node_turn(&self, a: Handle, i: u32, engine: &mut dyn Engine, depth: u8) -> Option<Quat> {
        if self.rig_unposed(a) {
            return self.rig_held.get(i as usize).map(|(_, turn)| *turn);
        }
        let posed = self.nodes.get(&a).and_then(|n| n.get(i as usize));
        let node = posed.copied().or_else(|| self.origin_node(a, i))?;
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
        let past = self.actor_bodies(engine);
        self.surface_ray(engine, self.ray.start, d, len, past)
            .map(|hit| (hit.point, hit.normal, hit.material))
    }

    /// The first surface along a ray past `past`: the level, or an actor
    /// whose collision acts as an actor surface, passing through any that
    /// doesn't ([`ScriptWorld::ray_contact_full`]'s rule).
    fn surface_ray(
        &self,
        engine: &mut dyn Engine,
        origin: Vec3,
        direction: Vec3,
        len: f32,
        mut past: Vec<BodyId>,
    ) -> Option<guac_motion::seam::RayHit> {
        loop {
            let hit = guac_motion::ground::ray(engine.seam(), origin, direction, len, &past)?;
            let surface = engine
                .actor_of(hit.body)
                .and_then(|h| self.actors.get(&h))
                .is_none_or(|a| a.solid.acts_as_actor_surface != 0);
            if surface {
                return Some(hit);
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
        // Hung from another actor, it turns with it (openspec AS.76: a
        // turret's body on its base, whose facing `New Turret - Find Target`
        // holds its targets' headings against).
        if depth > 0
            && !a.removed
            && let Some(owner) = self.actor_of(Some(a.attach)).filter(|&o| o != h)
            && let Some(g) = self.angles_within(owner, engine, depth - 1)
        {
            let mut turned = a.angles;
            turned.facing += g.facing;
            return Some(turned);
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
            self.walk_written.insert(h);
            return Ok(());
        }
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.velocity = v;
                // A creature walks by it, and a scene walk steers from it
                // (AS.68), or a rise flies it till one of nothing ends the
                // jump (AS.187); anything else flies.
                if a.walker.is_none()
                    && let Some(walk) = self.scene_walks.get_mut(&h)
                {
                    walk.jump = v.y > 0.0 || (walk.jump && v != Vec3::ZERO);
                }
                a.launched |=
                    a.walker.is_none() && !self.scene_walks.contains_key(&h) && v != Vec3::ZERO;
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
        if let Some(v) = self.sound_member(at, m) {
            return v;
        }
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
                    // A relative angle on an actor hung from no ground turns
                    // it by what's written, about its own axis (the setter:
                    // AG.60's facing, AH.145's tilt and roll), so it reads
                    // 0: a script's own `relative facing + k` turns it by k
                    // (`Boulder_Pebbles.ai`, `Chest_Lock.ai`), where reading
                    // its facing doubled it each tick, and a debris piece's
                    // slide of its relative roll starts from 0 each frame
                    // (AH.9's `lob-00`: each frame's turn a step larger).
                    Some(Object::Facet(h, Facet::Orientation))
                        if matches!(
                            m,
                            Member::RelativeFacing | Member::RelativeTilt | Member::RelativeRoll
                        ) && self.actors.contains_key(&h)
                            && engine.hero(h).is_none()
                            && h != self.viewport
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
                    // Its file's turn until a script turns it (openspec
                    // AS.52); one with neither reads nothing.
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
            // A node's root is the actor it's on (openspec AS.47: a scene's
            // actor attached to the camera rig's node reads the rig by it);
            // an actor named as a node is its own; nothing, nothing.
            Member::AttachRoot => match object {
                Some(Object::Node(owner, _)) => Value::Entity(owner),
                _ => actor.map_or(Value::Null, Value::Entity),
            },
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
                // An awake actor reads 0 over nothing: the original's ground
                // finder, on a miss, leaves its ground far under and its
                // type 0, and -1 is an actor with no corpus, what the
                // scripts' `type >= 0` tests ask for (openspec AG.28: the
                // camera's target, dropped from `New CameraTargets Set` on a
                // -1 where the level's mesh ends under it; the au-chests
                // review: a chest's treasure past a ledge's rim, taken out of
                // play by `UI.ai` before its trajectory's correction could
                // bring it back). A hero over nothing on his own feet too
                // (openspec AS.146): the mover update takes a mover reading
                // under 0 off for gone, every step, so his stick and his
                // gravity died there and stayed dead on any ground he came
                // back to. Hung on a rig or held by a scene he reads as ever:
                // level 201's intro walks him off its pod along a plank with
                // no solid, which its mover's gravity would drop him through.
                // Stand-in ledger: `fall-reset`.
                let free = match object {
                    Some(Object::Facet(h, Facet::Ground)) => match engine.hero(h) {
                        Some(hero) => !hero.hung && !self.in_scene,
                        None => self
                            .actors
                            .get(&h)
                            .is_some_and(|a| !a.removed && !(a.managed && !a.active)),
                    },
                    _ => false,
                };
                Value::Int(match (found, free) {
                    (true, _) => i32::from(m.unwrap_or(0) >> SURFACE_SHIFT),
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
            // A palette's clip, not yet played, has its length (openspec
            // AS.78: an animator camera's shot reads its clip's off the
            // palette entry, `force fixed anim time`, before any copy plays;
            // read as nothing, every such shot lasted no time).
            Member::SeqRawDuration if matches!(object, Some(Object::Clip { .. })) => {
                Value::Float(self.length_of(recv.ok_or(HostFault::Nothing)?))
            }
            Member::SeqTimeRemaining
            | Member::SeqPercentRemaining
            | Member::SeqRate
            | Member::SeqLooping
            | Member::SeqPlaybackMode
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
                    Member::SeqPlaybackMode => Value::Int(i32::from(s.ping_pong)),
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
            Member::LocomotionClip => Value::Null,
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
                // and gravity act only while it's over 0, AG.38). Unless a
                // rim alone holds him, nothing walkable under his pivot
                // (openspec AS.155): what's under him is the gap below, so
                // his gravity slides him off, where a gap of 0 left him
                // standing with his walkable check refusing every heading.
                if let (Some(h), Some(o)) = (actor, at.other_point)
                    && self.object(Some(o)) == Some(Object::Facet(h, Facet::Ground))
                    && let Some(hero) = engine.hero(h)
                {
                    if hero.footing.supported && !hero.on_a_lip() {
                        return Ok(Value::Float(0.0));
                    }
                    // With none under him, how far he's fallen: gravity acts
                    // on him, and the minion's fall check sets him back on
                    // his trail once it's past its 225 ft (openspec AS.146).
                    if let Some(fallen) = hero.fallen() {
                        return Ok(Value::Float((fallen * FEET_PER_METRE).max(1.0)));
                    }
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
            Member::AudioListener if object == Some(Object::Viewport) => {
                self.objs.value(Object::Viewport)
            }
            Member::AudioListener => return Err(HostFault::Nothing),
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
            | Member::SeqClones
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
                    //
                    // A turret's pointer (`New Turret - Find Target`, openspec
                    // AS.76) asks it of its targets along the ray through
                    // where it points: yes for one the ray passes within its
                    // reach of (stand-in ledger: `turret-pick`).
                    if self.ray_picks {
                        let p = self.position(a, engine).ok_or(HostFault::Nothing)?;
                        let (start, end) = (self.ray.start, self.ray.end);
                        let d = end - start;
                        let t = ((p - start).dot(d) / d.length_squared().max(1e-6)).clamp(0.0, 1.0);
                        let off = (start + d * t).distance(p);
                        return Ok(Value::Int(i32::from(off <= crate::turret::PICK_REACH)));
                    }
                    return Ok(Value::Int(0));
                }
                // A turret's shot leaves from inside its base's sphere, its
                // barrel's nodes unposed (stand-in ledger: `turret-pick`).
                if self.turret_base == Some(a) && self.name(b).starts_with(crate::turret::SHOTS) {
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
        if let Some(done) = self.set_sound_member(at, m, v) {
            return done;
        }
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
                // A file waypoint turned (openspec AG.60), from its file's
                // turn (AS.52), the angles not written kept.
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
                    Member::RelativeFacing | Member::RelativeTilt | Member::RelativeRoll => {
                        self.ground_hung(h)
                    }
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
                    Some(g) if matches!(m, Member::RelativeFacing) => {
                        let facing = self.angles(g, engine).map_or(0.0, |a| a.facing);
                        let normal = match engine.hero(g) {
                            Some(hero) => hero.footing.height.map(|_| hero.footing.slope),
                            None => self.ground(g, engine).map(|g| g.1),
                        };
                        normal.map(|n| (n, facing))
                    }
                    _ => None,
                };
                let a = self.actors.get_mut(&h).ok_or(HostFault::Nothing)?;
                match m {
                    Member::Facing => a.angles.facing = x,
                    // One hung from nothing turns by it about its own axis
                    // (openspec AG.60: a spinning coin's loot update writes
                    // its spin speed each tick, and AH.11's recordings have
                    // each coin's facing step by that speed every frame,
                    // 2.82, -5.31 and 4.23 degrees on the walkway's three;
                    // AH.145: a debris piece's relative facing and roll turn
                    // it about one axis fixed in its own frame, its tilt and
                    // facing moving with its roll, AH.9's `lob-00`).
                    Member::RelativeFacing | Member::RelativeTilt | Member::RelativeRoll
                        if ground.is_none() =>
                    {
                        a.angles = turned_by(a.angles, m, x);
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
                // Hung where it hangs from, no offset yet.
                if let Some(a) = self.actors.get_mut(&h) {
                    a.attach = v;
                    a.hung = Vec3::ZERO;
                    a.hung_facing = None;
                }
                // A hero hung has his controller off until he's let go
                // (openspec AS.115); another's ground carries nobody here.
                let carried = v != Value::Null
                    && !matches!(self.object(Some(v)), Some(Object::Facet(_, Facet::Ground)));
                if let Some(hero) = engine.hero(h)
                    && hero.hung != carried
                {
                    hero.hung = carried;
                    hero.walk.velocity = Vec3::ZERO;
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
            Member::SeqRate
            | Member::SeqLooping
            | Member::SeqPlaybackMode
            | Member::SeqPercentRemaining => {
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
                    Member::SeqPlaybackMode => {
                        s.ping_pong = v.as_i32().ok_or(HostFault::Nothing)? == 1;
                    }
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
            // screen's; openspec AS.76: a turret's pointer): on the line
            // from the eye through that place.
            Member::ConverterScreen => {
                let Value::Screen(s) = v else {
                    return Err(HostFault::Nothing);
                };
                let (x, y) = crate::viewport::unpack_screen(s);
                self.view.converter = self.view.unproject(x, y);
            }
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
                // Clips alone: a sound's copy (openspec AR.9, a sequence with
                // a length since) is in `playing sounds`, never here, or a
                // walker's locomotion takes his footsteps for animations.
                let this = &*self;
                let playing = |whole: bool| {
                    this.sequences
                        .iter()
                        .enumerate()
                        .filter(move |(_, s)| {
                            s.actor == h
                                && s.plays()
                                && s.is_layered() != whole
                                && this.clip_of(s).is_some()
                        })
                        .map(|(i, _)| Object::Sequence(i as u32))
                };
                let mut v: Vec<Object> = playing(true).collect();
                v.reverse();
                v.extend(playing(false));
                v.into_iter().map(|o| self.objs.value(o)).collect()
            }
            // The sound copies playing on him, in the order started
            // (openspec AR.10).
            Member::PlayingSounds => {
                let h = self.actor_of(recv).ok_or(HostFault::Nothing)?;
                let on = self.sounds_on(h);
                on.into_iter()
                    .map(|i| self.objs.value(Object::Sequence(i)))
                    .collect()
            }
            Member::SoundSystem(f) => self.sound_set(f)?,
            // A template's copies still playing, on whatever actor, in the
            // order started (openspec AS.37): a clip's that its starts made,
            // a sound's while one plays; a copy named stands for its
            // template.
            Member::SeqClones => {
                let template = match self.object(recv) {
                    Some(Object::Sequence(i)) => {
                        self.sequences
                            .get(i as usize)
                            .ok_or(HostFault::Nothing)?
                            .template
                    }
                    _ => recv.ok_or(HostFault::Nothing)?,
                };
                let copies: Vec<u32> = self
                    .sequences
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.template == template && s.plays())
                    .map(|(i, _)| i as u32)
                    .collect();
                copies
                    .into_iter()
                    .map(|i| self.objs.value(Object::Sequence(i)))
                    .collect()
            }
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
                // The actor the block tells, every `control` on the dump
                // reading that actor's clips (openspec AU.37: Pop Fizz's
                // beast; AS.52: a cutscene's own actors, on which the game
                // systems' placement starts each scene's animations; AS.97:
                // the enemy system's action queue, a setup controller's
                // Mabu), the running placement's where it holds none. Told
                // by the game systems a file's placement reads nothing
                // still: the pit's intro, which tells `Enemy_Chompy(1)` its
                // three clips, runs AH.8's 449 frames that way and 64 more
                // with them played (AS.80; stand-in ledger: `told-palette`).
                let of = |w: &Self, h: Handle| {
                    let t = w.template_of(h)?;
                    w.palettes.get(&t).map(|p| (t, p.len()))
                };
                let scene = self.is_game_systems(at.placement);
                let told = self
                    .actor_of(Some(self.told))
                    .filter(|&h| {
                        h != at.placement
                            && self
                                .actors
                                .get(&h)
                                .is_some_and(|a| a.obj.is_none() || !scene)
                    })
                    .and_then(|h| of(self, h));
                let lengths = told.or_else(|| of(self, at.placement));
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
    fn template_of(&self, h: Handle) -> Option<Obj> {
        let a = self.actors.get(&h)?;
        a.template.or(a.obj)
    }

    /// Whether `h` is a camera's rig ([`ScriptWorld::set_rig`]).
    pub(crate) fn is_rig(&self, h: Handle) -> bool {
        self.template_of(h).is_some_and(|t| self.rigs.contains(&t))
    }

    /// Whether `h` is made from a template with no model: what a shot hangs
    /// on its rig for the view to follow.
    fn follower(&self, h: Handle) -> bool {
        self.template_of(h)
            .and_then(|t| self.templates.get(&t))
            .is_some_and(|p| p.model.is_none())
    }

    /// Whether `h` is a rig no pose has reached yet.
    fn rig_unposed(&self, h: Handle) -> bool {
        !self.nodes.contains_key(&h) && self.is_rig(h)
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

    /// Whether `h` is the game systems' placement, on which a cutscene's
    /// steps run (openspec AS.52), by its script.
    fn is_game_systems(&self, h: Handle) -> bool {
        self.actors
            .get(&h)
            .and_then(|a| a.obj)
            .and_then(|o| self.templates.get(&o))
            .and_then(|p| p.script.as_deref())
            .is_some_and(|s| s.ends_with("GameSystems.ai"))
    }

    /// A play of the sequence at `i` on `actor` (openspec AH.184): refused
    /// while it plays with time left and no `allow restart` (the op fails
    /// and its block is skipped), else started again from its start, his.
    pub(crate) fn start_again(&mut self, i: usize, actor: Handle) -> Answer {
        let s = &mut self.sequences[i];
        if s.refuses_restart() {
            return Answer::Skip;
        }
        s.actor = actor;
        s.restart();
        self.restarts.push(i as u32);
        // A sound's copy started over is a new play (openspec AR.9).
        self.sound_restarted(i as u32);
        Answer::Run {
            receiver: None,
            result: Some(self.objs.value(Object::Sequence(i as u32))),
        }
    }

    /// `template` (a clip or a sequence) played on `actor`, as `start
    /// sequence` plays it: one copy per clip and actor (openspec AH.184): a
    /// sequence played is itself while it's his or stopped (the original's
    /// stop clears its activator), else its clip's on him; a clip played on
    /// him while its copy plays is that copy, refused while it has time left
    /// and no `allow restart`.
    fn play_on(&mut self, template: Value, actor: Handle) -> Answer {
        let mut template = template;
        if let Some(Object::Sequence(i)) = self.object(Some(template))
            && let Some(s) = self.sequences.get(i as usize)
        {
            if s.actor == actor || s.stopped {
                return self.start_again(i as usize, actor);
            }
            template = s.template;
        }
        // A sound info the sounds part gives audio for plays as a sound
        // (openspec AR.9: `crate::sound_members`).
        if let Value::Object(o) = template
            && let Some(answer) = self.play_sound(template, o, actor)
        {
            return answer;
        }
        if matches!(self.object(Some(template)), Some(Object::Clip { .. }))
            && let Some(i) = self
                .sequences
                .iter()
                .rposition(|s| s.actor == actor && s.template == template && s.plays())
        {
            return self.start_again(i, actor);
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
            ping_pong: false,
            back: false,
        });
        let s = self
            .objs
            .value(Object::Sequence((self.sequences.len() - 1) as u32));
        Answer::Run {
            receiver: None,
            result: Some(s),
        }
    }

    /// A clip's length; 0 for a slot the extract refused, so an animator
    /// camera's shot on a clip past one Havok block lasts no time (openspec
    /// AS.52, AS.78; stand-in ledger: `cinematic-camera`).
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
                Ok(self.play_on(template, actor))
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
                    // A sound info's copies on the receiver (openspec AR.9).
                    _ => {
                        if let Some(sound) = first {
                            let on = self.handle(call.receiver);
                            self.stop_sound(sound, on);
                        }
                    }
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
                let i = self.along(mover).min(p.len() - 1);
                (p[i], i + 1 == p.len())
            }
            None => (named("_LHS").ok_or(HostFault::Nothing)?, true),
        };
        let point = self.point(target, engine).ok_or(HostFault::Nothing)?;
        let walks = self.actors.get(&mover).is_some_and(|a| a.walker.is_some());
        let done = if toward {
            guac_motion::steer::reached(at, point, distance)
        } else {
            // One a scene moves from itself goes on along its facing, never
            // clear of its own place (openspec AU.34: the hub's fairies; a
            // reading, `steer::input` already heading a mover at its point
            // along its facing). A creature's is clear at once: the Chompies
            // ask it all through a fight, and walked on it they'd eat and
            // chase on other steps.
            (walks || self.actor_of(Some(target)) != Some(mover))
                && guac_motion::steer::cleared(at, point, distance)
        };
        if self
            .actors
            .get(&mover)
            .and_then(|a| a.walker.as_deref())
            .is_none()
        {
            return self.scene_move(
                mover,
                call,
                path.as_deref(),
                point,
                (toward, done, last),
                engine,
            );
        }
        // `with anim` (openspec AH.282): the palette's clip at `_indexRHS`
        // played on the mover while he moves, as `start sequence` plays it,
        // so the op's reissue each tick finds its copy playing and starts
        // nothing new (AH.184); the script's own stop ends it.
        if !(done && last)
            && let Some(clip) = self.move_clip(call)
        {
            self.play_on(clip, mover);
        }
        let Some(w) = self
            .actors
            .get_mut(&mover)
            .and_then(|a| a.walker.as_deref_mut())
        else {
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

    /// Which waypoint of a path `mover` is walking to: its walker's, or its
    /// scene walk's ([`ScriptWorld::scene_move`]); the first for one neither
    /// has.
    fn along(&self, mover: Handle) -> usize {
        match self.actors.get(&mover).and_then(|a| a.walker.as_deref()) {
            Some(w) => w.along,
            None => self.scene_walks.get(&mover).map_or(0, |s| s.along),
        }
    }

    /// [`ScriptWorld::op_move`] for an actor the engine doesn't walk as a
    /// creature (openspec AS.52): steered all the same, as the engine's
    /// velocity model steers every actor with movement numbers, with no
    /// body under it ([`ScriptWorld::move_scene_walks`]; chapter 1's
    /// outro walks its Mabu so, `CS Walk Obj to Location` handing the
    /// engine an actor no movement data of the scripts' moves). Reached,
    /// the block runs, as a creature's.
    /// (Stand-in ledger: `scene-walk`.)
    fn scene_move(
        &mut self,
        mover: Handle,
        call: &OpCall,
        path: Option<&[Value]>,
        point: Vec3,
        (toward, done, last): (bool, bool, bool),
        engine: &mut dyn Engine,
    ) -> Result<Answer, HostFault> {
        if !(done && last)
            && let Some(clip) = self.move_clip(call)
        {
            self.play_on(clip, mover);
        }
        let mut aim = Aim::Still;
        let mut along = self.along(mover);
        if done {
            if let Some(p) = path
                && !last
            {
                along += 1;
                if let Some(next) = self.point(p[along], engine) {
                    aim = Aim::To(next);
                }
            }
        } else {
            aim = if toward {
                Aim::To(point)
            } else {
                Aim::From(point)
            };
        }
        // A hero's steps are `move_hero_walk`'s (his scripts zero his walk
        // each tick while a scene holds him).
        let walk = self.scene_walks.entry(mover).or_default();
        walk.aim = aim;
        walk.along = along;
        walk.own = call.placement == mover;
        Ok(if done {
            Answer::Run {
                receiver: None,
                result: None,
            }
        } else {
            Answer::Done
        })
    }

    /// One frame of each scene walk ([`ScriptWorld::scene_move`]): the
    /// engine's velocity model ([`guac_motion::steer::step`]) on its
    /// movement numbers under the aim its scripts left this tick, on level
    /// ground, from the actor's velocity as its scripts left it (AS.68: the
    /// homing charge sets its speed each tick before its `move to`), its
    /// place moved by the walk and its facing turned as the model turns it,
    /// `actorinfo.velocity` the walk's. The walk is its one motion: a
    /// velocity a script writes never also flies it
    /// ([`ScriptWorld::move_actors`]). The aim lasts the tick it was asked
    /// for, as a creature's does. One taken off, hung on something or
    /// walked as a creature since is let go. One of the people the TV draws
    /// whose solid meets the level stands on the ground under each step
    /// (openspec AS.166: chapter 1's Norticus at the quarry leads the hero
    /// up its slope, 5.4 m in all, where at the height he stood he'd end
    /// under it), within 2 m over it to 4 m under it, keeping its height
    /// over none; the ground the ray caster's (the level, or an actor that
    /// acts as an actor surface: the hub's walking Mabu crossing a barrel
    /// stepped 1.78 m up onto it); a scene's actors stay at the height
    /// they stand. A scene's jump flies instead (openspec AS.187: level
    /// 201's practice pit's scene jumps Brock 25 ft to his landing 3.4 m
    /// below, his walk carried it flat and short): from the velocity its
    /// sequence writes with a rise, on that velocity under the actor's own
    /// gravity, through whatever's there as its shape is off; within a foot
    /// of its landing's height the sequence gives it its shape back, and
    /// one whose solid meets the level then comes down on the ground under
    /// it and stops there, the jump over (a write of nothing ends it too),
    /// to walk and stand as before.
    pub(crate) fn move_scene_walks(&mut self, dt: f32, engine: &mut dyn Engine) {
        let mut walking: Vec<Handle> = self
            .scene_walks
            .keys()
            .copied()
            .filter(|h| !self.resting.contains(h))
            .collect();
        walking.sort_unstable();
        let walks = &self.scene_walks;
        self.scene_bodies.retain(|h, _| walks.contains_key(h));
        let people = self.people();
        let mut bodies = self.actor_bodies(engine);
        bodies.extend(people.iter().filter_map(|p| engine.body(*p)));
        bodies.sort_unstable();
        bodies.dedup();
        for h in walking {
            if engine.hero(h).is_some() {
                self.move_hero_walk(h, dt, engine);
                continue;
            }
            let Some(a) = self
                .actors
                .get_mut(&h)
                .filter(|a| !a.removed && a.walker.is_none() && a.attach == Value::Null)
            else {
                self.scene_walks.remove(&h);
                continue;
            };
            let Some(walk) = self.scene_walks.get_mut(&h) else {
                continue;
            };
            let aim = std::mem::take(&mut walk.aim);
            let dt = dt * a.physics.time_scale / 100.0;
            // A scene's jump (AS.187): on its velocity under its gravity,
            // through whatever's there while its shape is off; given its
            // shape back (the sequence's, within a foot of its landing's
            // height), coming down it lands on the ground under it and
            // stops, as a person's walk stands it.
            if walk.jump {
                let p = &a.physics;
                if p.gravity_acceleration > 0.0 {
                    a.velocity.y -= p.gravity_acceleration * dt;
                    if p.gravity_max_speed > 0.0 {
                        a.velocity.y = a.velocity.y.max(-p.gravity_max_speed);
                    }
                }
                let next = a.position + a.velocity * (dt * METRES_PER_FOOT);
                let lands = a.solid.shape != 0
                    && a.solid.interacts_with_level_mesh != 0
                    && a.velocity.y < 0.0;
                let ground = lands
                    .then(|| {
                        self.surface_ray(
                            engine,
                            next + Vec3::Y * 2.0,
                            Vec3::NEG_Y,
                            6.0,
                            bodies.clone(),
                        )
                    })
                    .flatten()
                    .filter(|hit| hit.point.y >= next.y);
                if let Some(a) = self.actors.get_mut(&h) {
                    match ground {
                        Some(hit) => {
                            a.position = Vec3::new(next.x, hit.point.y, next.z);
                            a.velocity = Vec3::ZERO;
                        }
                        None => a.position = next,
                    }
                }
                if ground.is_some()
                    && let Some(walk) = self.scene_walks.get_mut(&h)
                {
                    walk.jump = false;
                }
                continue;
            }
            // One whose solid meets the level, not one of the people the
            // ground under each step stands (AS.166), walks on a body as
            // the engine walks a creature (AS.79): its solid swept through
            // the level, so a wall stops it, and its gravity under it, so
            // it comes down onto the ground and off an edge, through its
            // walk and once it ends. Chapter 1's outro's Mabu and level
            // 214's intro's: capsules that meet the level, 32 and 50 ft/s².
            let person = people.binary_search(&h).is_ok();
            let solid = a
                .havok()
                .filter(|_| !person && a.solid.interacts_with_level_mesh != 0);
            if let Some(solid) = solid {
                let own: Vec<BodyId> = engine.body(h).into_iter().collect();
                let fresh = self
                    .scene_bodies
                    .get(&h)
                    .is_none_or(|b| *b.solid() != solid);
                if fresh {
                    let mut b = Hero::actor(solid, a.position, a.angles.facing);
                    b.settle(engine.seam(), &own);
                    self.scene_bodies.insert(h, b);
                }
                let Some(b) = self.scene_bodies.get_mut(&h) else {
                    continue;
                };
                b.pivot = a.position;
                b.facing = a.angles.facing;
                b.walk.velocity = a.velocity;
                b.step_steered(
                    engine.seam(),
                    &steer_physics(&a.physics),
                    aim,
                    dt,
                    &own,
                    Surfaces::NONE,
                );
                a.position = b.pivot;
                a.angles.facing = b.facing;
                a.velocity = b.walk.velocity;
                continue;
            }
            let frame = guac_motion::steer::step(
                guac_motion::steer::Frame {
                    velocity: a.velocity * METRES_PER_FOOT,
                    facing: a.angles.facing,
                    airborne: false,
                },
                &steer_physics(&a.physics),
                aim,
                a.position,
                Some(Vec3::Y),
                true,
                dt,
            );
            let next = a.position + frame.velocity * dt;
            let grounded =
                a.solid.interacts_with_level_mesh != 0 && people.binary_search(&h).is_ok();
            a.angles.facing = frame.facing;
            a.velocity = frame.velocity * FEET_PER_METRE;
            let ground = grounded
                .then(|| {
                    self.surface_ray(
                        engine,
                        next + Vec3::Y * 2.0,
                        Vec3::NEG_Y,
                        6.0,
                        bodies.clone(),
                    )
                })
                .flatten();
            if let Some(a) = self.actors.get_mut(&h) {
                a.position = Vec3::new(next.x, ground.map_or(next.y, |hit| hit.point.y), next.z);
            }
        }
    }

    /// A step of a scene's walk on a hero (openspec AS.113: level 201's
    /// intro walks him in along his path): his pivot across the ground at a
    /// scene walk's speed, on what's under him, facing where he goes. One
    /// further from where he's going than any scene walks him starts there:
    /// what he hung on let him go where its clip didn't pose it (level 201's
    /// landing pod, stand-in ledger: `scene-walk`). His own script's walk is
    /// [`ScriptWorld::move_hero_own_walk`]'s.
    fn move_hero_walk(&mut self, h: Handle, dt: f32, engine: &mut dyn Engine) {
        let Some(walk) = self.scene_walks.get_mut(&h) else {
            return;
        };
        let Aim::To(to) = std::mem::take(&mut walk.aim) else {
            walk.speed = 0.0;
            return;
        };
        if walk.own {
            self.move_hero_own_walk(h, to, dt, engine);
            return;
        }
        let Some(at) = engine.hero(h).map(|hero| hero.pivot) else {
            return;
        };
        let across = Vec3::new(to.x - at.x, 0.0, to.z - at.z);
        let far = across.length();
        if far < 1e-4 {
            return;
        }
        let next = if far > SCENE_WALK_REACH {
            to
        } else {
            at + across / far * (SCENE_WALK_SPEED * METRES_PER_FOOT * dt).min(far)
        };
        let actors = self.actor_bodies(engine);
        let ground = guac_motion::ground::ray(
            engine.seam(),
            next + Vec3::Y * 2.0,
            Vec3::NEG_Y,
            6.0,
            &actors,
        )
        .map(|hit| hit.point.y);
        if let Some(hero) = engine.hero(h) {
            hero.pivot = Vec3::new(next.x, ground.unwrap_or(next.y), next.z);
            hero.facing = crate::script_engine::heading_of(across);
        }
    }

    /// A step of a hero's own script's walk (openspec AU.9: Sunburn's
    /// teleport hands `Movement - Move To` his destination while he plays):
    /// up to his movement numbers as his scripts left them (200 ft/s, 999
    /// ft/s² for the teleport; a scene walk's speed with none set) straight
    /// at it, through his controller, so a wall stops him and past a ledge
    /// the ground isn't under him. The walk his scripts left him is his
    /// again after it. What carries him in the original is AU.9's to read
    /// (stand-in ledger: `scene-walk`).
    fn move_hero_own_walk(&mut self, h: Handle, to: Vec3, dt: f32, engine: &mut dyn Engine) {
        let Some(mut body) = engine.hero(h).cloned() else {
            return;
        };
        let across = Vec3::new(to.x - body.pivot.x, 0.0, to.z - body.pivot.z);
        let far = across.length();
        if far < 1e-4 {
            return;
        }
        let numbers = self.actors.get(&h).map(|a| a.physics).unwrap_or_default();
        let Some(walk) = self.scene_walks.get_mut(&h) else {
            return;
        };
        walk.speed = if numbers.ground_acceleration > 0.0 {
            (walk.speed + numbers.ground_acceleration * dt).min(numbers.ground_max_speed)
        } else {
            SCENE_WALK_SPEED
        };
        // No further than it is: the step ends on it, not past it.
        let speed = walk.speed.min(far / (METRES_PER_FOOT * dt));
        let left = body.walk.velocity;
        body.walk.velocity = across / far * speed;
        let own: Vec<BodyId> = engine.body(h).into_iter().collect();
        body.step_engine(engine.seam(), dt, &own, Surfaces::NONE);
        body.walk.velocity = left;
        body.facing = crate::script_engine::heading_of(across);
        if let Some(hero) = engine.hero(h) {
            *hero = body;
        }
    }

    /// The clip a `move to ... with anim` plays: its palette (`_NP`, read as
    /// a set) at `_indexRHS`, or `_NP` itself when it names one clip; none
    /// without one, or for anything but a clip or a sequence.
    fn move_clip(&self, call: &OpCall) -> Option<Value> {
        let index = call
            .operands
            .iter()
            .find(|(n, _)| *n == "_indexRHS")
            .and_then(|(_, v)| *v)
            .and_then(|v| v.as_i32());
        let indexed = call
            .sets
            .iter()
            .find(|(n, _)| *n == "_NP")
            .and_then(|(_, e)| index.and_then(|i| e.get(usize::try_from(i).ok()?)))
            .copied();
        indexed
            .or_else(|| {
                call.operands
                    .iter()
                    .find(|(n, _)| *n == "_NP")
                    .and_then(|(_, v)| *v)
            })
            .filter(|v| {
                matches!(
                    self.object(Some(*v)),
                    Some(Object::Clip { .. } | Object::Sequence(_))
                )
            })
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
    /// within a centimetre (AH.20); with one of them shapeless (no solid,
    /// a mesh, or no actor at all) they don't (openspec AS.85, a reading:
    /// the original gives a shapeless `CollisionInfo` a sphere its world
    /// never holds, AG.7, and the state system's two dead-falling states
    /// take a creature's shape off as it dies, so a lob in flight can't
    /// burst on the corpse; unmodelled, level 214's dying Elf Near against
    /// Eruptor's lava lob stopped the portal manager here).
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
        // Neither a body in the seam (a thrown ball against a breakable the
        // loot system listed, AH.20): their solids where they stand, where
        // a shapeless one touches nothing (AS.85).
        Ok(!asked && near)
    }

    /// The engine's move for the actors the scripts set going, a frame of
    /// `dt` seconds: one hung from a node sits at it; one launched falls
    /// under its own gravity at its time scale and, with a solid that meets
    /// the level, sweeps it through the seam, stopping at what it hits and
    /// sliding along it, its contacts there kept for
    /// `collisioninfo.environment contacts`.
    pub(crate) fn move_actors(&mut self, dt: f32, engine: &mut dyn Engine) {
        self.move_walkers(dt, engine);
        self.move_scene_walks(dt, engine);
        // A clone that has stood as a surface and that its script lets go of
        // (no longer solid to actors, given a gravity: level 201's bridge
        // planks as Brock smashes it) falls from where it stands (openspec
        // AU.53, AS.168).
        for (&h, a) in self.actors.iter_mut() {
            if a.removed {
                continue;
            }
            if a.stands_for_actors() {
                self.surfaces_stood.insert(h);
            } else if !a.launched
                && a.solid.interacts_with_actors == 0
                && a.physics.gravity_acceleration > 0.0
                && self.surfaces_stood.contains(&h)
            {
                a.launched = true;
            }
        }
        // A scene walk is its actor's one motion (AS.68).
        let mut flying: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(h, a)| {
                !a.removed
                    && a.attach == Value::Null
                    && a.launched
                    && !self.scene_walks.contains_key(h)
                    && !self.resting.contains(h)
            })
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

    /// The actor a node value stands on, if it names one.
    pub fn node_owner(&self, v: Value) -> Option<Handle> {
        match self.object(Some(v)) {
            Some(Object::Node(owner, _)) => Some(owner),
            _ => None,
        }
    }

    /// The actors something not taken off hangs on a node of, each once in
    /// handle order: whose nodes an embedder poses (openspec AH.138), the
    /// rest left at their pivots for nothing.
    pub fn hung_on(&self) -> Vec<Handle> {
        let mut owners: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, a)| !a.removed)
            .filter_map(|(&h, a)| match self.object(Some(a.attach)) {
                Some(Object::Node(owner, _)) if owner != h => Some(owner),
                _ => None,
            })
            .collect();
        owners.sort_unstable();
        owners.dedup();
        owners
    }

    /// Whether what actor `h` hangs on, a node of another actor or that
    /// actor itself, was taken off, or what that hangs on was, up the
    /// chain (openspec AS.186): what hangs on nothing, which the TV doesn't
    /// draw. It stays in the world, as the engine keeps a minion hung on an
    /// actor destroyed under him (the cutscene system's own clean-up
    /// detaches him), its scripts and its own reads unchanged; whether the
    /// original draws it is unread (stand-in ledger: `hung-draw`). False for
    /// one hanging from nothing, from itself or from a ground.
    pub fn hangs_on_nothing(&self, h: Handle) -> bool {
        let mut at = h;
        for _ in 0..HUNG_DEPTH {
            let Some(a) = self.actors.get(&at) else {
                return false;
            };
            let owner = match self.object(Some(a.attach)) {
                Some(Object::Node(owner, _)) => Some(owner),
                None => self.actor_of(Some(a.attach)),
                _ => None,
            };
            let Some(owner) = owner.filter(|&o| o != at) else {
                return false;
            };
            if self.actors.get(&owner).is_some_and(|o| o.removed) {
                return true;
            }
            at = owner;
        }
        false
    }

    /// Each actor hung from a node set at it, as the engine's frame leaves
    /// it: [`ScriptWorld::move_actors`]' last part, and what an embedder
    /// posing the nodes after that frame (openspec AH.50) runs again.
    pub(crate) fn place_hung_actors(&mut self, engine: &mut dyn Engine) {
        // One the pause lets rest holds where it hangs (openspec AS.205).
        let mut hung: Vec<(Handle, Value)> = self
            .actors
            .iter()
            .filter(|(h, a)| !a.removed && a.attach != Value::Null && !self.resting.contains(h))
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
            // The first shot's rig, unposed, has no last place to read: what
            // hangs on it and draws stays where it stands until it's posed,
            // where the shot's cam actor, a model of none, goes to the view.
            if self.rig_held.is_empty()
                && matches!(self.object(Some(node)), Some(Object::Node(owner, _)) if self.rig_unposed(owner))
                && !self.follower(h)
            {
                continue;
            }
            let turn = match self.object(Some(node)) {
                Some(Object::Node(owner, i)) if owner != h => {
                    self.node_turn(owner, i, engine, HUNG_DEPTH)
                }
                _ => None,
            };
            // A skinned clone hung on an actor itself turns as that actor
            // turns (openspec AU.37: Pop Fizz's beast, hung on him), by what
            // it turned through, so a spin of its own goes on. What has no
            // palette, a collider or a pivot, keeps the scripts' facing:
            // turned, Dino-Rang's whirlwind misses. Stand-in ledger:
            // `hung-turn`.
            let skinned = self
                .template_of(h)
                .is_some_and(|t| self.palettes.contains_key(&t));
            let owner_facing = match self.object(Some(node)) {
                None if skinned => self
                    .actor_of(Some(node))
                    .filter(|&o| o != h)
                    .and_then(|o| self.angles_within(o, engine, HUNG_DEPTH - 1))
                    .map(|g| g.facing),
                _ => None,
            };
            // Its offset from what it hangs from, in that frame (openspec
            // AH.226); what it hangs from at once otherwise.
            let at = match self.hang_frame(h, engine) {
                Some((p, frame)) => self.actors.get(&h).map(|a| p + frame * a.hung),
                None => self.point(node, engine),
            };
            if let Some(p) = at
                && let Some(a) = self.actors.get_mut(&h)
            {
                a.position = p;
                if let Some(q) = turn {
                    a.angles = angles_of(q);
                }
                if let (Some(now), Some(last)) = (owner_facing, a.hung_facing)
                    && now != last
                {
                    a.angles.facing = (a.angles.facing + now - last).rem_euclid(360.0);
                }
                a.hung_facing = owner_facing;
            }
            // A hung hero's body goes with his node (openspec AS.115), his
            // place being his controller's.
            if let Some(p) = at
                && let Some(hero) = engine.hero(h)
                && hero.hung
            {
                hero.pivot = p;
                if let Some(q) = turn {
                    hero.facing = angles_of(q).facing;
                }
            }
        }
    }
}

/// The speed a scene walks a hero at, ft/s: what the engine's scene walk
/// gives an actor with no numbers of its own (stand-in ledger: `scene-walk`).
const SCENE_WALK_SPEED: f32 = 20.0;
/// How far a scene walks a hero, metres: from further he starts there.
const SCENE_WALK_REACH: f32 = 30.0;

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
    /// own scripts: AH.168.) And the solid of each of the people whose
    /// script runs, unwalked (openspec AU.34), while it interacts with
    /// actors, as a prop's is ([`crate::props::actor_body`]): the hero
    /// stops on Flynn as he stops on a Chompy.
    pub fn creature_bodies(&self) -> Vec<(Handle, HavokSolid, Pose)> {
        let people = self.people();
        let mut out: Vec<(Handle, HavokSolid, Pose)> = self
            .actors
            .iter()
            .filter(|(h, a)| {
                !a.removed
                    && a.active
                    && (a.walker.is_some()
                        || (a.solid.interacts_with_actors != 0
                            && !self.inert.contains(h)
                            && people.binary_search(h).is_ok())
                        || a.stands_for_actors())
            })
            .filter_map(|(&h, a)| {
                let solid = a.havok()?;
                // A surface turns as its actor does, tilted and rolled too,
                // as a prop's solid ([`crate::props::actor_body`]): level
                // 201's fourth vine bridge rises 1° to the strip it reaches,
                // a step its flat solid left too high to climb (openspec
                // AS.183). A creature's controller body stands upright.
                let rotation = if a.stands_for_actors() {
                    euler([a.angles.tilt, a.angles.facing, a.angles.roll])
                } else {
                    Quat::from_rotation_y(a.angles.facing.to_radians())
                };
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
            .filter(|(h, a)| {
                a.walker.is_some()
                    && !a.removed
                    && a.active
                    && a.attach == Value::Null
                    && !self.resting.contains(h)
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
                // Nothing to sweep: it moves by its velocity, falling by its
                // gravity as a launched actor does (openspec AU.53: a scene's
                // jump takes its actor's shape off for the arc and waits for
                // it to come down to its waypoint).
                if a.physics.gravity_acceleration > 0.0 {
                    a.velocity.y -= a.physics.gravity_acceleration * dt;
                    if a.physics.gravity_max_speed > 0.0 {
                        a.velocity.y = a.velocity.y.max(-a.physics.gravity_max_speed);
                    }
                }
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

/// `angles` turned by `by` degrees about the actor's own axis a relative
/// angle names (openspec AH.145): its facing about its own up, its tilt
/// about its own side (tilting it nose up, as the tilt's sign has it), its
/// roll about its own forward, then read back as the original keeps them
/// (`Ry(facing) Rx(-tilt) Rz(roll)`). The roll is the innermost turn, so a
/// relative roll adds to it; so does a relative tilt with no roll, and a
/// relative facing with neither (a spinning coin's, exactly as AG.60 kept
/// it). Any other is composed and read back.
fn turned_by(angles: Angles, m: Member, by: f32) -> Angles {
    match m {
        Member::RelativeRoll => {
            return Angles {
                roll: angles.roll + by,
                ..angles
            };
        }
        Member::RelativeTilt if angles.roll == 0.0 => {
            return Angles {
                tilt: angles.tilt + by,
                ..angles
            };
        }
        Member::RelativeFacing if angles.tilt == 0.0 && angles.roll == 0.0 => {
            return Angles {
                facing: (angles.facing + by).rem_euclid(360.0),
                ..angles
            };
        }
        _ => {}
    }
    let q = guac_motion::solid::euler([angles.tilt, angles.facing, angles.roll]);
    let turn = match m {
        Member::RelativeFacing => Quat::from_rotation_y(by.to_radians()),
        _ => Quat::from_rotation_x(-by.to_radians()),
    };
    angles_of(q * turn)
}

/// An actor's movement numbers as the engine's steering reads them.
/// Where a hero with no ground under him reads his ground: under him by how
/// far he's fallen, past the 5 ft the safe point trail asks to be within, so
/// no place over nothing goes on the trail he's set back on (openspec
/// AS.146).
/// Stand-in ledger: `fall-reset`.
fn fall_point(hero: &Hero) -> Option<Vec3> {
    let fallen = hero.fallen()?;
    Some(hero.pivot - Vec3::Y * fallen.max(6.0 * METRES_PER_FOOT))
}

/// The top speed `Movement - Add Mover` leaves in the engine's numbers: a
/// mover's walk is the scripts', and the engine's own model does nothing to
/// it (openspec AU.37).
const MOVER_TOP_SPEED: f32 = 10_000.0;

impl ScriptWorld {
    /// What the engine walks `hero` with this step, if it does (openspec
    /// AU.37): none while he's one of the scripts' movers, whose velocity
    /// the movement scripts keep and hand the engine, nor on a step a script
    /// set his walk itself (Swarm's flight sets it every step and loses its
    /// hits otherwise). Off them (`Movement - Remove Mover` hands the engine
    /// his numbers: 31 of the sweep's 268 presses, and for good where an
    /// attack cut short leaves him there) the engine's own velocity model
    /// runs on him as it runs on every actor, the stick its input with a pad
    /// as his `actorinfo.controller`, which `Movement - Activate/Deactivate
    /// Controller` swaps for the inactive one off the movers alone. Stand-in
    /// ledger: `engine-walk`.
    pub fn engine_walk(&self, hero: Handle, push: Push) -> Option<(SteerPhysics, Aim)> {
        let a = self.actors.get(&hero)?;
        if a.physics.ground_max_speed >= MOVER_TOP_SPEED || self.walk_written.contains(&hero) {
            return None;
        }
        let pad = self
            .controllers
            .iter()
            .any(|c| a.controller == Value::Entity(*c));
        let aim = match pad && push.held() {
            true => Aim::Along {
                heading: push.heading,
                strength: push.strength,
            },
            false => Aim::Still,
        };
        Some((steer_physics(&a.physics), aim))
    }

    /// One game frame of a hero whose scripts walk him: the engine alone
    /// while he's one of their movers, its velocity model on his own numbers
    /// off them ([`ScriptWorld::engine_walk`]).
    #[allow(clippy::too_many_arguments)]
    pub fn step_scripted(
        &self,
        handle: Handle,
        hero: &mut Hero,
        seam: &dyn PhysicsSeam,
        push: Push,
        step: f32,
        exclude: &[BodyId],
        surfaces: &Surfaces,
    ) {
        match self.engine_walk(handle, push).filter(|_| !hero.hung) {
            Some((physics, aim)) => hero.step_steered(seam, &physics, aim, step, exclude, surfaces),
            None => hero.step_engine(seam, step, exclude, surfaces),
        }
    }
}

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
/// off his model's attach list, AH.50), else off its model's list
/// ([`ScriptWorld::set_origin_nodes`]), else at the actor's pivot (stand-in
/// ledger: `nodes`).
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
            turns: HashMap::new(),
        };
        let handles = HashMap::new();
        ScriptWorld::new(&files, &handles, headless_objects(&handles))
    }

    /// AU.37: a walk a script sets holds the engine's model off him for that
    /// step, and the step's end lets it go.
    #[test]
    fn a_walk_a_script_sets_stays_as_written_for_the_step() {
        let (mut w, mut e) = (world(), bench());
        let solid = guac_motion::tuning::eruptor_solid();
        e.hero = Some(Hero::actor(solid, Vec3::ZERO, 0.0));
        let mut him = Actor::blank();
        him.physics.ground_max_speed = 20.0;
        w.actors.insert(HERO, him);
        assert!(w.engine_walk(HERO, Push::NONE).is_some(), "off the movers");
        w.set_velocity(HERO, Vec3::X, &mut e).unwrap();
        assert_eq!(w.engine_walk(HERO, Push::NONE), None);
        w.advance(STEP, &mut e);
        assert!(w.engine_walk(HERO, Push::NONE).is_some(), "the next step's");
    }

    /// AU.33: `animationinfo.walk` and `run` read as nothing, so a `turn to`
    /// naming one to play turns on with no clip (the Giant's at the floor).
    #[test]
    fn a_locomotion_clip_reads_as_nothing() {
        let mut w = world();
        let mut e = crate::script_engine::NoEngine;
        for name in ["walk", "run"] {
            assert_eq!(
                w.get(&at("animationinfo", name, None, None), &mut e),
                Ok(Value::Null),
                "{name}"
            );
        }
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

    /// AR.11: the gameplay viewport's audio listener is the viewport, the
    /// camera standing as the listener, so a point taken from it is the
    /// eye; nothing on any other receiver.
    #[test]
    fn the_audio_listener_is_the_viewport() {
        let mut w = world();
        let mut e = bench();
        w.view.eye = Vec3::new(1.0, 2.0, 3.0);
        let viewport = w.objs.value(Object::Viewport);
        let listener = w
            .get(
                &at("viewportinfo", "audio listener", Some(viewport), None),
                &mut e,
            )
            .unwrap();
        assert_eq!(listener, viewport);
        assert_eq!(w.point(listener, &mut e), Some(Vec3::new(1.0, 2.0, 3.0)));
        let ray = w.objs.value(Object::RayCast);
        assert_eq!(
            w.get(
                &at("viewportinfo", "audio listener", Some(ray), None),
                &mut e
            ),
            Err(HostFault::Nothing)
        );
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
    /// run did (openspec AH.21): a piece's relative roll reads 0 and a write
    /// turns it by that much about its own forward (openspec AH.145), in
    /// degrees and unclamped as registered; its solid's rebound reads its
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
            Ok(Value::Float(0.0))
        );
        w.set_member(&roll, Member::RelativeRoll, Value::Float(83.5), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap();
        assert_eq!(
            (a.angles.roll, a.angles.tilt, a.angles.facing),
            (128.5, 0.0, 0.0)
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

    /// A relative facing and roll turn a tilted actor about its own up and
    /// forward (openspec AH.145): AH.9's `lob-00` first piece of debris,
    /// from 45° tilt and roll, writes each frame a relative facing and roll
    /// (its slide read from 0, a step larger each frame; here the frame's
    /// own turn off the record times its axis' parts, 0.2941 and 0.9558),
    /// and its angles, tilt and facing moving with its roll, come out as the
    /// record's within 0.002° over 13 frames. Each relative angle reads 0.
    #[test]
    fn a_relative_facing_and_roll_turn_a_tilted_piece_about_its_own_axes() {
        // (relative facing, relative roll) written, then the record's
        // [tilt, facing, roll] that frame.
        const FRAMES: [((f32, f32), [f32; 3]); 13] = [
            ((0.0842, 0.2735), [45.059, 0.084, 45.214]),
            ((0.1693, 0.5502), [45.180, 0.253, 45.644]),
            ((0.2518, 0.8183), [45.359, 0.504, 46.285]),
            ((0.3337, 1.0846), [45.600, 0.834, 47.134]),
            ((0.4167, 1.3543), [45.905, 1.241, 48.197]),
            ((0.5003, 1.6261), [46.277, 1.723, 49.475]),
            ((0.5837, 1.8969), [46.719, 2.277, 50.971]),
            ((0.6674, 2.1689), [47.236, 2.896, 52.687]),
            ((0.7507, 2.4399), [47.831, 3.573, 54.627]),
            ((0.8340, 2.7104), [48.509, 4.302, 56.794]),
            ((0.9173, 2.9811), [49.274, 5.072, 59.195]),
            ((1.0006, 3.2519), [50.130, 5.871, 61.838]),
            ((1.0844, 3.5241), [51.084, 6.686, 64.732]),
        ];
        let mut e = bench();
        let mut w = world();
        let mut piece = ball(Vec3::ZERO, 100.0);
        piece.angles = Angles {
            tilt: 45.0,
            facing: 0.0,
            roll: 45.0,
        };
        w.actors.insert(BALL, piece);
        let turn = w.objs.value(Object::Facet(BALL, Facet::Orientation));
        let facing = at(
            "orientationmeasurement",
            "relative facing",
            Some(turn),
            None,
        );
        let roll = at("orientationmeasurement", "relative roll", Some(turn), None);
        let tilt = at("orientationmeasurement", "relative tilt", Some(turn), None);
        for (k, ((f, r), want)) in FRAMES.iter().enumerate() {
            for (m, at) in [
                (Member::RelativeFacing, &facing),
                (Member::RelativeRoll, &roll),
                (Member::RelativeTilt, &tilt),
            ] {
                assert_eq!(w.get_member(at, m, &mut e), Ok(Value::Float(0.0)), "{m:?}");
            }
            w.set_member(&facing, Member::RelativeFacing, Value::Float(*f), &mut e)
                .unwrap();
            w.set_member(&roll, Member::RelativeRoll, Value::Float(*r), &mut e)
                .unwrap();
            let a = w.actor(BALL).unwrap().angles;
            let got = [a.tilt, a.facing, a.roll];
            for i in 0..3 {
                assert!(
                    (got[i] - want[i]).abs() < 2e-3,
                    "frame {}: {got:?} against {want:?}",
                    k + 1
                );
            }
        }
        // A relative tilt tips it about its own side: on an unturned actor,
        // its tilt.
        let mut flat = ball(Vec3::ZERO, 100.0);
        flat.angles = Angles::default();
        w.actors.insert(BALL, flat);
        w.set_member(&tilt, Member::RelativeTilt, Value::Float(30.0), &mut e)
            .unwrap();
        w.set_member(&tilt, Member::RelativeTilt, Value::Float(15.0), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap().angles;
        assert_eq!((a.tilt, a.facing, a.roll), (45.0, 0.0, 0.0));
        // Rolled onto its side, its own side is up: a relative tilt of 30
        // turns its facing back 30.
        let mut rolled = ball(Vec3::ZERO, 100.0);
        rolled.angles = Angles {
            roll: 90.0,
            ..Angles::default()
        };
        w.actors.insert(BALL, rolled);
        w.set_member(&tilt, Member::RelativeTilt, Value::Float(30.0), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap().angles;
        assert!(
            a.tilt.abs() < 1e-3 && (a.facing - 330.0).abs() < 1e-3 && (a.roll - 90.0).abs() < 1e-3,
            "{a:?}"
        );
        // And a relative facing of 30 turns it about its own up, now its
        // side: its tilt.
        let mut rolled = ball(Vec3::ZERO, 100.0);
        rolled.angles = Angles {
            roll: 90.0,
            ..Angles::default()
        };
        w.actors.insert(BALL, rolled);
        w.set_member(&facing, Member::RelativeFacing, Value::Float(30.0), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap().angles;
        assert!(
            (a.tilt - 30.0).abs() < 1e-3 && a.facing.abs() < 1e-3 && (a.roll - 90.0).abs() < 1e-3,
            "{a:?}"
        );
        // Tilted 30° nose up and not rolled, a relative facing of 90 turns
        // it about its own up, which leans back: its nose comes level along
        // x and its up keeps leaning back, toward -z, where adding to the
        // facing would have left it pointing 30° up.
        let mut tilted = ball(Vec3::ZERO, 100.0);
        tilted.angles = Angles {
            tilt: 30.0,
            ..Angles::default()
        };
        w.actors.insert(BALL, tilted);
        w.set_member(&facing, Member::RelativeFacing, Value::Float(90.0), &mut e)
            .unwrap();
        let a = w.actor(BALL).unwrap().angles;
        let q = guac_motion::solid::euler([a.tilt, a.facing, a.roll]);
        let (nose, up) = (q * Vec3::Z, q * Vec3::Y);
        assert!((nose - Vec3::X).length() < 1e-4, "{a:?}: nose {nose}");
        assert!(
            (up - Vec3::new(0.0, 0.866_025_4, -0.5)).length() < 1e-4,
            "{a:?}: up {up}"
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

    /// The hero's death actor lifts the dying minion over its `dying actor`'s
    /// point a tick at a time (openspec AH.250): set to him as it spawns,
    /// the reference keeps where he stood then, so each write puts him that
    /// far over where he fell, wherever the last one left him, and back down
    /// with the slide. Set to him again once he's moved, it keeps where he
    /// stands then (the VM tells the host each set: guac-script's own
    /// test), as the chest and the Titan boulder use it. One set to another
    /// actor keeps where that one stands; one no script set reads where he
    /// stands now.
    #[test]
    fn a_reference_keeps_where_its_actor_stood_when_it_was_set() {
        const OTHER: Handle = 0x7777;
        const CELL: u32 = 9;
        let mut e = bench();
        let mut w = world();
        let here = Vec3::new(3.0, 1.0, 4.0);
        let there = Vec3::new(-2.0, 5.0, 0.0);
        w.actors.insert(BALL, ball(here, 0.0));
        w.actors.insert(OTHER, ball(there, 0.0));
        let remembered = |w: &mut ScriptWorld, e: &mut Bench, of: Handle| {
            let mut a = at(
                "placementreference",
                "remembered",
                Some(Value::Entity(of)),
                None,
            );
            a.via = Some(CELL);
            w.get_member(&a, Member::Remembered, e).unwrap()
        };
        let lift = |w: &mut ScriptWorld, e: &mut Bench, from: Value, feet: f32| {
            w.set_member(
                &at(
                    "positionmeasurement",
                    "vertical offset",
                    Some(Value::Entity(BALL)),
                    Some(from),
                ),
                Member::VerticalOffset,
                Value::Float(feet),
                e,
            )
            .unwrap();
            w.actor(BALL).unwrap().position
        };
        let over = |p: Vec3, feet: f32| Vec3::new(here.x, p.y + feet * METRES_PER_FOOT, here.z);
        // No script set it: it reads where he stands, each write lifting
        // him from there.
        let own = remembered(&mut w, &mut e, BALL);
        let once = lift(&mut w, &mut e, own, 12.0);
        assert!((once - over(here, 12.0)).length() < 1e-5, "{once:?}");
        assert!((lift(&mut w, &mut e, own, 12.0) - over(once, 12.0)).length() < 1e-5);
        w.actors.get_mut(&BALL).unwrap().position = here;
        // Set to him: where he stood then, through the slide's rise and
        // fall, however far the last write lifted him.
        w.referenced(CELL, Value::Entity(BALL), &mut e);
        for feet in [0.0, 6.0, 12.0, 12.0, 6.0, 0.0] {
            let p = lift(&mut w, &mut e, own, feet);
            assert!((p - over(here, feet)).length() < 1e-5, "{feet} ft: {p:?}");
        }
        // Moved, then set to him again: where he stands now.
        let risen = here + Vec3::new(0.0, 3.6, 0.0);
        w.actors.get_mut(&BALL).unwrap().position = risen;
        w.referenced(CELL, Value::Entity(BALL), &mut e);
        assert_eq!(w.point(own, &mut e), Some(risen), "taken afresh");
        w.actors.get_mut(&BALL).unwrap().position = here;
        assert_eq!(w.point(own, &mut e), Some(risen), "kept, not followed");
        // Set to another actor: where that one stands, apart from his.
        w.referenced(CELL, Value::Entity(OTHER), &mut e);
        let theirs = remembered(&mut w, &mut e, OTHER);
        assert_eq!(w.point(theirs, &mut e), Some(there));
        w.actors.get_mut(&OTHER).unwrap().position = here;
        assert_eq!(w.point(theirs, &mut e), Some(there), "kept, not followed");
        // A host object names no actor: nothing kept.
        let n = w.kept_via.len();
        w.referenced(CELL + 1, Value::Int(3), &mut e);
        assert_eq!(w.kept_via.len(), n);
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
    /// with one shapeless they don't, however near (openspec AS.85: a
    /// creature whose dead state took its shape off, against a lob in
    /// flight), nor with one no actor stands for.
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
        // Overlapping again, then its shape taken off as the state
        // system's dead states take a creature's (`collisioninfo.shape` 0).
        w.actors.get_mut(&OTHER).unwrap().position = here + Vec3::X;
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(1)), "overlapping");
        w.actors.get_mut(&OTHER).unwrap().solid.shape = 0;
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)), "shapeless");
        w.actors.get_mut(&OTHER).unwrap().solid.shape = 5;
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)), "a mesh");
        w.actors.insert(OTHER, Actor::blank());
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)), "no solid");
        w.actors.remove(&OTHER);
        assert_eq!(collision(&mut w, &mut e), Ok(Value::Int(0)), "no actor");
    }

    /// An actor others hang from in the tests below.
    const OWNER: Handle = 0x6000_0021;

    /// openspec AS.145: each shot's rig is a clone drawn, so posed, a frame
    /// after it's made. Until then its node reads where the last rig left
    /// it, turned as it was, so the view and what hangs there hold instead
    /// of standing at the rig's origin; posed, it reads its own. The first
    /// shot's has no last: the view reads where it stands, and what hangs
    /// on the rig stays put. Any other clone unposed reads its pivot.
    #[test]
    fn a_rigs_node_unposed_reads_as_its_last() {
        const RIG: Obj = Obj { unit: 0, index: 77 };
        const LAST: Handle = 0x6000_0031;
        const NEXT: Handle = 0x6000_0032;
        const HUNG: Handle = 0x6000_0033;
        const PLAIN: Handle = 0x6000_0034;
        let mut e = bench();
        let mut w = world();
        w.set_rig(RIG);
        let clone = |at: Vec3, template: Option<Obj>| {
            let mut a = Actor::blank();
            a.position = at;
            a.template = template;
            a
        };
        w.actors.insert(LAST, clone(Vec3::ZERO, Some(RIG)));
        w.actors.insert(NEXT, clone(Vec3::ZERO, Some(RIG)));
        w.actors
            .insert(PLAIN, clone(Vec3::new(2.0, 0.0, 0.0), None));
        let mut hung = Actor::blank();
        hung.position = Vec3::new(8.0, 8.0, 8.0);
        hung.attach = w.objs.value(Object::Node(NEXT, 0));
        w.actors.insert(HUNG, hung);
        w.view.eye = Vec3::new(1.0, 1.0, 1.0);
        // The first shot's: nothing to read, and what hangs stays, but for
        // its cam actor, a template of no model, which goes to the view.
        const CAM: Obj = Obj { unit: 0, index: 78 };
        const CAM_ACTOR: Handle = 0x6000_0036;
        let mut cam = crate::scripts::Placed::named(CAM, "follower", None, 4);
        cam.model = None;
        w.templates.insert(CAM, cam);
        let mut follower = clone(Vec3::ZERO, Some(CAM));
        follower.attach = w.objs.value(Object::Node(NEXT, 0));
        w.actors.insert(CAM_ACTOR, follower);
        assert_eq!(w.node(NEXT, 0, &mut e), Some(w.view.eye));
        w.place_hung_actors(&mut e);
        assert_eq!(w.actors[&HUNG].position, Vec3::new(8.0, 8.0, 8.0));
        assert_eq!(w.actors[&CAM_ACTOR].position, w.view.eye);
        // A shot posed: the next, unposed, reads it.
        let turned = Quat::from_rotation_y(90f32.to_radians());
        let at = Vec3::new(30.0, 40.0, 50.0);
        w.set_nodes(LAST, vec![NodePose { at, turn: turned }]);
        assert_eq!(w.node(NEXT, 0, &mut e), Some(at));
        assert_eq!(w.node_turn(NEXT, 0, &mut e, HUNG_DEPTH), Some(turned));
        w.place_hung_actors(&mut e);
        assert_eq!(w.actors[&HUNG].position, at);
        // Posed, it reads its own, and the next after it reads that.
        let own = Vec3::new(-3.0, 4.0, -5.0);
        w.set_nodes(
            NEXT,
            vec![NodePose {
                at: own,
                turn: Quat::IDENTITY,
            }],
        );
        assert_eq!(w.node(NEXT, 0, &mut e), Some(own));
        w.place_hung_actors(&mut e);
        assert_eq!(w.actors[&HUNG].position, own);
        assert_eq!(w.rig_held[0].0, own);
        // Not a rig: at its pivot, as ever.
        assert_eq!(w.node(PLAIN, 0, &mut e), Some(Vec3::new(2.0, 0.0, 0.0)));
        // The last rig taken off and the step over, a later scene's first
        // shot reads nothing of the old scene's.
        w.actors.get_mut(&NEXT).unwrap().removed = true;
        w.end_step();
        const LATER: Handle = 0x6000_0035;
        w.actors.insert(LATER, clone(Vec3::ZERO, Some(RIG)));
        assert_eq!(w.node(LATER, 0, &mut e), Some(w.view.eye));
    }

    /// openspec AS.115: a hero hung on a node rides it, his controller's
    /// step taking nothing from it and the host carrying his body as the
    /// node moves, turned as it is; let go, his place is his to walk from,
    /// from where the node left him. Hung on his own ground he isn't hung.
    #[test]
    fn a_hero_hung_on_a_node_rides_it_and_is_let_go_where_it_left_him() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0));
        e.hero.as_mut().unwrap().pivot = pivot;
        e.hero.as_mut().unwrap().walk.velocity = Vec3::X;
        let mut w = world();
        w.actors.insert(HERO, Actor::blank());
        w.actors.insert(OWNER, Actor::blank());
        let posed = |w: &mut ScriptWorld, at: Vec3, turn: Quat| {
            w.set_nodes(OWNER, vec![NodePose { at, turn }]);
        };
        posed(&mut w, Vec3::new(5.0, 6.0, 7.0), Quat::IDENTITY);
        let attach = |w: &mut ScriptWorld, e: &mut Bench, v: Value| {
            w.set_member(
                &at(
                    "abstractplacement",
                    "attach node",
                    Some(Value::Entity(HERO)),
                    None,
                ),
                Member::AttachNode,
                v,
                e,
            )
            .unwrap();
        };
        let node = w.objs.value(Object::Node(OWNER, 0));
        attach(&mut w, &mut e, node);
        let hero = e.hero.as_ref().unwrap();
        assert!(hero.hung);
        assert_eq!(hero.pivot, Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(
            hero.walk.velocity,
            Vec3::ZERO,
            "his walk stops as he's hung"
        );
        // The rig flies on, turned a quarter.
        posed(
            &mut w,
            Vec3::new(9.0, 2.0, 3.0),
            Quat::from_rotation_y(90f32.to_radians()),
        );
        w.place_hung(&mut e);
        let hero = e.hero.as_ref().unwrap();
        assert_eq!(hero.pivot, Vec3::new(9.0, 2.0, 3.0));
        assert!((hero.facing - 90.0).abs() < 1e-3, "{}", hero.facing);
        attach(&mut w, &mut e, Value::Null);
        assert!(!e.hero.as_ref().unwrap().hung);
        posed(&mut w, Vec3::new(1.0, 1.0, 1.0), Quat::IDENTITY);
        w.place_hung(&mut e);
        assert_eq!(
            e.hero.as_ref().unwrap().pivot,
            Vec3::new(9.0, 2.0, 3.0),
            "let go, the node carries him no more"
        );
        let ground = w.objs.value(Object::Facet(OWNER, Facet::Ground));
        attach(&mut w, &mut e, ground);
        assert!(
            !e.hero.as_ref().unwrap().hung,
            "another's ground hangs nobody"
        );
    }

    /// openspec AS.76: an actor hung from another turns with it: what a
    /// turret's body reads against its base's facing, its own turn added.
    #[test]
    fn an_actor_hung_from_another_reads_its_turn() {
        let mut e = bench();
        let mut w = world();
        let mut base = Actor::blank();
        base.angles.facing = 81.0;
        w.actors.insert(OWNER, base);
        let mut body = Actor::blank();
        body.attach = Value::Entity(OWNER);
        w.actors.insert(BALL, body);
        assert_eq!(w.angles(BALL, &mut e).map(|g| g.facing), Some(81.0));
        w.actors.get_mut(&BALL).unwrap().angles.facing = 9.0;
        assert_eq!(w.angles(BALL, &mut e).map(|g| g.facing), Some(90.0));
        w.actors.get_mut(&BALL).unwrap().attach = Value::Null;
        assert_eq!(w.angles(BALL, &mut e).map(|g| g.facing), Some(9.0));
    }

    /// openspec AS.189: a hero hung on a turret hung at a place is carried
    /// there, the turret turned or not, and goes on there as it turns;
    /// hung from nothing, nothing is kept.
    #[test]
    fn a_hero_hung_at_a_place_is_set_there() {
        let mut e = bench();
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0));
        let mut w = world();
        let mut turret = Actor::blank();
        turret.position = Vec3::new(4.0, 1.0, -2.0);
        turret.angles.facing = 70.0;
        w.actors.insert(OWNER, turret);
        w.actors.insert(HERO, Actor::blank());
        w.set_member(
            &at(
                "abstractplacement",
                "attach node",
                Some(Value::Entity(HERO)),
                None,
            ),
            Member::AttachNode,
            Value::Entity(OWNER),
            &mut e,
        )
        .unwrap();
        assert!(e.hero.as_ref().unwrap().hung);
        let to = Vec3::new(7.0, 3.5, 1.0);
        assert!(w.hang_at(HERO, to, &mut e));
        w.place_hung(&mut e);
        let pivot = e.hero.as_ref().unwrap().pivot;
        assert!(pivot.distance(to) < 1e-4, "{pivot}");
        // Turned a quarter about itself, the turret takes him round with it.
        w.actors.get_mut(&OWNER).unwrap().angles.facing = 160.0;
        w.place_hung(&mut e);
        let turned = Quat::from_rotation_y(90f32.to_radians()) * (to - Vec3::new(4.0, 1.0, -2.0));
        let pivot = e.hero.as_ref().unwrap().pivot;
        assert!(
            pivot.distance(Vec3::new(4.0, 1.0, -2.0) + turned) < 1e-4,
            "{pivot}"
        );
        w.actors.get_mut(&HERO).unwrap().attach = Value::Null;
        assert!(!w.hang_at(HERO, to, &mut e), "hung from nothing");
    }

    /// openspec AS.76: the ray caster's collision picks what the ray passes
    /// within reach of while a turret's game runs, and its own shots pass
    /// its base; with none running it answers no touch as ever.
    #[test]
    fn a_turrets_pointer_picks_along_the_ray_and_its_shots_pass_its_base() {
        let mut e = bench();
        let mut w = world();
        let mut lock = Actor::blank();
        lock.position = Vec3::new(10.0, 1.0, 0.0);
        w.actors.insert(BALL, lock);
        w.ray.start = Vec3::ZERO;
        w.ray.end = Vec3::new(20.0, 0.0, 0.0);
        let ray = w.objs.value(Object::RayCast);
        let asked = |w: &mut ScriptWorld, e: &mut Bench| {
            let a = at(
                "positionmeasurement",
                "collision",
                Some(Value::Entity(BALL)),
                Some(ray),
            );
            w.get_member(&a, Member::Collision, e)
        };
        assert_eq!(
            asked(&mut w, &mut e),
            Ok(Value::Int(0)),
            "no turret: no touch"
        );
        w.ray_picks = true;
        assert_eq!(asked(&mut w, &mut e), Ok(Value::Int(1)), "1 m off the ray");
        w.ray.end = Vec3::new(20.0, 20.0, 0.0);
        assert_eq!(asked(&mut w, &mut e), Ok(Value::Int(0)), "well off it");
        // A shot starts inside its turret's base and passes it; anything
        // else overlapping the base touches it, and its shot touches another.
        let (base, shot) = (0x6000_0031, 0x6000_0032);
        let solid = Vec3::new(1.0, 2.0, 3.0);
        w.actors.insert(base, ball(solid, 0.0));
        w.actors.insert(shot, ball(solid, 0.0));
        let name = |w: &mut ScriptWorld, name: &str| {
            let o = Obj { unit: 9, index: 1 };
            w.templates
                .insert(o, crate::scripts::Placed::named(o, name, None, 5));
            w.actors.get_mut(&shot).unwrap().template = Some(o);
        };
        name(&mut w, "New_Turret_Projectile_Fire");
        let touch = |w: &mut ScriptWorld, e: &mut Bench| {
            let a = at(
                "positionmeasurement",
                "collision",
                Some(Value::Entity(base)),
                Some(Value::Entity(shot)),
            );
            w.get_member(&a, Member::Collision, e)
        };
        assert_eq!(
            touch(&mut w, &mut e),
            Ok(Value::Int(1)),
            "no turret: it touches"
        );
        w.turret_base = Some(base);
        assert_eq!(
            touch(&mut w, &mut e),
            Ok(Value::Int(0)),
            "its own shot passes"
        );
        name(&mut w, "009_LavaProjectile");
        assert_eq!(
            touch(&mut w, &mut e),
            Ok(Value::Int(1)),
            "another's doesn't"
        );
    }

    /// openspec AS.76: a point put at a place on the screen lands there, on
    /// the line from the eye; a value that's no screen place is refused.
    #[test]
    fn the_converters_screen_location_puts_its_point_on_that_line() {
        let mut e = bench();
        let mut w = world();
        w.view.eye = Vec3::new(1.0, 2.0, 3.0);
        let converter = w.objs.value(Object::ViewConverter);
        let write = |w: &mut ScriptWorld, e: &mut Bench, v: Value| {
            w.set_member(
                &at("viewconverter", "screen location", Some(converter), None),
                Member::ConverterScreen,
                v,
                e,
            )
        };
        let s = crate::viewport::pack_screen(300.0, 500.0);
        write(&mut w, &mut e, Value::Screen(s)).unwrap();
        let (x, y) = w.view.project(w.view.converter).unwrap();
        assert!(
            (x - 300.0).abs() < 0.01 && (y - 500.0).abs() < 0.01,
            "{x} {y}"
        );
        assert!(write(&mut w, &mut e, Value::Int(3)).is_err());
    }

    /// openspec AS.113: a scene's walk takes a hero across the ground at a
    /// scene walk's speed, facing where he goes, step by step, the block
    /// run once he's there; from further than a scene walks him he starts
    /// there.
    #[test]
    fn a_scene_walks_a_hero_across_the_ground() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0));
        e.hero.as_mut().unwrap().pivot = pivot;
        let mut w = world();
        w.actors.insert(HERO, Actor::blank());
        let call = OpCall {
            placement: 1,
            receiver: Value::Null,
            class: Class::MoveTo,
            operands: Vec::new(),
            words: Vec::new(),
            sets: Vec::new(),
        };
        let to = Vec3::new(1.0, 5.0, 11.0);
        let answer = w
            .scene_move(HERO, &call, None, to, (true, false, true), &mut e)
            .unwrap();
        assert_eq!(answer, Answer::Done);
        w.move_scene_walks(1.0, &mut e);
        let hero = e.hero.as_ref().unwrap();
        let stride = SCENE_WALK_SPEED * METRES_PER_FOOT;
        assert!(
            (hero.pivot.z - (1.0 + stride)).abs() < 1e-4,
            "{}",
            hero.pivot
        );
        assert_eq!(
            (hero.pivot.x, hero.pivot.y),
            (1.0, 0.0),
            "across the ground"
        );
        assert_eq!(hero.facing, 0.0);
        let answer = w
            .scene_move(HERO, &call, None, to, (true, true, true), &mut e)
            .unwrap();
        assert!(matches!(answer, Answer::Run { .. }));
        let at = e.hero.as_ref().unwrap().pivot;
        w.move_scene_walks(1.0, &mut e);
        assert_eq!(e.hero.as_ref().unwrap().pivot, at, "reached: he stands");
        let far = Vec3::new(200.0, 8.0, 1.0);
        w.scene_move(HERO, &call, None, far, (true, false, true), &mut e)
            .unwrap();
        w.move_scene_walks(1.0 / 60.0, &mut e);
        assert_eq!(
            e.hero.as_ref().unwrap().pivot,
            far,
            "too far to walk: he's there"
        );
    }

    /// openspec AU.9: a hero's own script walking him goes through his
    /// controller: a wall stops him short of where he's going, and with
    /// nothing in the way he gets there on his own numbers; let go, his
    /// walk's what his scripts left it.
    #[test]
    fn a_heros_own_walk_stops_at_a_wall() {
        let floor = Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(0.0, -0.5, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(40.0, 0.5, 40.0),
                margin: 0.0,
            }),
        };
        let wall = Body {
            id: BodyId(2),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(0.0, 2.0, 5.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(10.0, 2.0, 0.2),
                margin: 0.0,
            }),
        };
        let walked = |wall: Option<Body>, to: Vec3| {
            let mut bodies = vec![floor.clone()];
            bodies.extend(wall);
            let mut e = Bench {
                world: ParryWorld::new(&bodies).unwrap(),
                hero: None,
            };
            let mut w = world();
            let mut hero = Actor::blank();
            hero.physics.ground_max_speed = 200.0;
            hero.physics.ground_acceleration = 999.0;
            w.actors.insert(HERO, hero);
            let start = Vec3::new(0.0, 0.01, 0.0);
            let mut h = Hero::new(eruptor_solid(), Tuning::ERUPTOR, start, 0.0);
            h.settle(&e.world, &[]);
            h.walk.velocity = Vec3::new(0.0, 0.0, 2.0);
            e.hero = Some(h);
            let call = OpCall {
                placement: HERO,
                receiver: Value::Null,
                class: Class::MoveTo,
                operands: Vec::new(),
                words: Vec::new(),
                sets: Vec::new(),
            };
            for _ in 0..30 {
                w.scene_move(HERO, &call, None, to, (true, false, true), &mut e)
                    .unwrap();
                w.move_scene_walks(1.0 / 60.0, &mut e);
            }
            e.hero.unwrap()
        };
        let to = Vec3::new(0.0, 0.0, 10.0);
        let clear = walked(None, to);
        assert!((clear.pivot.z - 10.0).abs() < 1e-3, "{}", clear.pivot);
        assert_eq!(
            clear.walk.velocity,
            Vec3::new(0.0, 0.0, 2.0),
            "his scripts'"
        );
        let stopped = walked(Some(wall), to);
        assert!(
            stopped.pivot.z < 4.8 && stopped.pivot.z > 3.0,
            "{}",
            stopped.pivot
        );
    }

    /// openspec AS.115: only a change of what carries him stops his walk:
    /// a character's attacks write a null over a null on him (Gill Grunt's,
    /// Spyro's, the shell's), which leaves his walk as his scripts set it.
    #[test]
    fn writing_nothing_over_nothing_leaves_his_walk() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0));
        e.hero.as_mut().unwrap().walk.velocity = Vec3::X;
        let mut w = world();
        w.actors.insert(HERO, Actor::blank());
        w.set_member(
            &at(
                "abstractplacement",
                "attach node",
                Some(Value::Entity(HERO)),
                None,
            ),
            Member::AttachNode,
            Value::Null,
            &mut e,
        )
        .unwrap();
        assert_eq!(e.hero.as_ref().unwrap().walk.velocity, Vec3::X);
    }

    /// AS.186: what hangs on an actor taken off, by a node of his or on him
    /// itself, or on what hangs so, hangs on nothing (chapter 1's outro
    /// takes its Mabu off with their beanies on their node 2, a hat box's
    /// bow hangs on its lid on the box); put back, it hangs on him again.
    /// What hangs from nothing, from itself, from a ground or round a loop
    /// of live actors hangs on something.
    #[test]
    fn what_hangs_on_an_actor_taken_off_hangs_on_nothing() {
        const MABU: Handle = 0x60;
        const HAT: Handle = 0x61;
        const BOX: Handle = 0x62;
        const LID: Handle = 0x63;
        const BOW: Handle = 0x64;
        const LOOSE: Handle = 0x65;
        const A: Handle = 0x66;
        const B: Handle = 0x67;
        let mut w = world();
        let hung = |w: &mut ScriptWorld, h: Handle, attach: Value| {
            let mut a = Actor::blank();
            a.attach = attach;
            w.actors.insert(h, a);
        };
        for h in [MABU, BOX] {
            hung(&mut w, h, Value::Null);
        }
        let node =
            |w: &mut ScriptWorld, owner: Handle, i: u32| w.objs.value(Object::Node(owner, i));
        let on_mabu = node(&mut w, MABU, 2);
        hung(&mut w, HAT, on_mabu);
        let on_box = node(&mut w, BOX, 0);
        hung(&mut w, LID, on_box);
        let on_lid = node(&mut w, LID, 0);
        hung(&mut w, BOW, on_lid);
        hung(&mut w, LOOSE, Value::Entity(MABU));
        assert!(!w.hangs_on_nothing(HAT), "on him");
        assert!(!w.hangs_on_nothing(BOW), "on the lid on the box");
        w.actors.get_mut(&MABU).unwrap().removed = true;
        assert!(w.hangs_on_nothing(HAT), "on his node, him taken off");
        assert!(w.hangs_on_nothing(LOOSE), "on him, him taken off");
        assert!(!w.hangs_on_nothing(MABU), "he hangs on nothing himself");
        w.actors.get_mut(&MABU).unwrap().removed = false;
        assert!(!w.hangs_on_nothing(HAT), "put back");
        w.actors.get_mut(&BOX).unwrap().removed = true;
        assert!(w.hangs_on_nothing(LID));
        assert!(w.hangs_on_nothing(BOW), "up the chain");
        // Hung from itself, from a ground of something taken off, round a
        // loop: on something.
        hung(&mut w, A, Value::Entity(A));
        assert!(!w.hangs_on_nothing(A));
        let ground = w.objs.value(Object::Facet(BOX, Facet::Ground));
        hung(&mut w, A, ground);
        assert!(!w.hangs_on_nothing(A), "a ground isn't a hang");
        hung(&mut w, A, Value::Entity(B));
        hung(&mut w, B, Value::Entity(A));
        assert!(!w.hangs_on_nothing(A), "a loop of live actors");
        assert!(!w.hangs_on_nothing(0x99), "no such actor");
    }

    /// AU.37: what hangs on an actor itself turns as he turns (Pop Fizz's
    /// beast, hung on him), by what he turned through, so a spin of its own
    /// goes on; what hangs on nothing keeps its facing.
    #[test]
    fn what_hangs_on_an_actor_turns_as_he_turns() {
        const BEAST: Handle = 0x55;
        const PIVOT: Handle = 0x56;
        let mut e = bench();
        let mut w = world();
        let solid = guac_motion::tuning::eruptor_solid();
        let mut him = Hero::actor(solid, Vec3::ZERO, 0.0);
        him.facing = 10.0;
        e.hero = Some(him);
        w.actors.insert(HERO, Actor::blank());
        let skinned = Obj { unit: 9, index: 1 };
        w.set_palette(skinned, vec![1.0]);
        let mut beast = Actor::blank();
        beast.template = Some(skinned);
        beast.attach = Value::Entity(HERO);
        beast.angles.facing = 30.0;
        w.actors.insert(BEAST, beast);
        let mut pivot = Actor::blank();
        pivot.template = Some(Obj { unit: 9, index: 2 });
        pivot.attach = Value::Entity(HERO);
        pivot.angles.facing = 30.0;
        w.actors.insert(PIVOT, pivot);
        let facing = |w: &ScriptWorld| w.actors[&BEAST].angles.facing;
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 30.0, "as it was hung");
        e.hero.as_mut().unwrap().facing = 50.0;
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 70.0, "40 degrees with him");
        assert_eq!(
            w.actors[&PIVOT].angles.facing, 30.0,
            "no palette, no turn: the scripts' facing stands"
        );
        w.actors.get_mut(&BEAST).unwrap().angles.facing = 100.0;
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 100.0, "a turn of its own stays");
        e.hero.as_mut().unwrap().facing = 350.0;
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 40.0, "round through north");
        // Let go and hung again, what he turned through unhung isn't its
        // turn (the au-pop-fizz review).
        let node = at(
            "abstractplacement",
            "attach node",
            Some(Value::Entity(BEAST)),
            None,
        );
        w.set(&node, Value::Null, &mut e).unwrap();
        e.hero.as_mut().unwrap().facing = 100.0;
        w.place_hung(&mut e);
        w.set(&node, Value::Entity(HERO), &mut e).unwrap();
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 40.0, "hung afresh");
        e.hero.as_mut().unwrap().facing = 110.0;
        w.place_hung(&mut e);
        assert_eq!(facing(&w), 50.0, "turning with him again");
        // A clone hung on itself (Dino-Rang's script hangs his on themselves)
        // has no one to turn with: its own turn isn't turned again.
        const ITSELF: Handle = 0x57;
        let mut own = Actor::blank();
        own.template = Some(skinned);
        own.attach = Value::Entity(ITSELF);
        own.angles.facing = 20.0;
        w.actors.insert(ITSELF, own);
        w.place_hung(&mut e);
        w.actors.get_mut(&ITSELF).unwrap().angles.facing = 35.0;
        w.place_hung(&mut e);
        assert_eq!(w.actors[&ITSELF].angles.facing, 35.0, "its own turn, once");
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

    /// AH.226's prompt and AH.138's turban: a hung actor hangs where a
    /// script puts it from what it hangs from. The cannon's prompt, hung on
    /// the cannon, set 15 ft over itself, stays 15 ft over the cannon as the
    /// cannon is pushed and bobs as its bob writes it over the cannon; the
    /// turban, hung on a posed node and displaced 0.5 ft down (pitch 270),
    /// keeps that offset in the node's frame as the node turns; hung again,
    /// it hangs at its new node; let go, a write puts it anywhere.
    #[test]
    fn a_hung_actor_keeps_where_a_script_put_it_from_what_it_hangs_from() {
        let mut e = bench();
        let pivot = Vec3::new(1.0, 0.0, 1.0);
        e.hero = Some(Hero::new(eruptor_solid(), Tuning::ERUPTOR, pivot, 0.0));
        e.hero.as_mut().unwrap().pivot = pivot;
        let mut w = world();
        const CANNON: Handle = 0x6000_0010;
        const PROMPT: Handle = 0x6000_0011;
        const HAT: Handle = 0x6000_0012;
        let base = Vec3::new(-3.0, 2.0, 5.0);
        let mut cannon = ball(base, 0.0);
        cannon.angles.facing = 30.0;
        w.actors.insert(CANNON, cannon);
        w.actors
            .insert(PROMPT, ball(base + Vec3::new(1.2, 4.57, 0.0), 0.0));
        w.actors.insert(HAT, ball(pivot, 0.0));
        let hang = |w: &mut ScriptWorld, e: &mut Bench, who: Handle, on: Value| {
            w.set_member(
                &at(
                    "abstractplacement",
                    "attach node",
                    Some(Value::Entity(who)),
                    None,
                ),
                Member::AttachNode,
                on,
                e,
            )
            .unwrap();
        };
        let lift = |w: &mut ScriptWorld, e: &mut Bench, who: Handle, over: Handle, feet: f32| {
            w.set_member(
                &at(
                    "positionmeasurement",
                    "vertical offset",
                    Some(Value::Entity(who)),
                    Some(Value::Entity(over)),
                ),
                Member::VerticalOffset,
                Value::Float(feet),
                e,
            )
            .unwrap();
        };
        let ft = |f: f32| Vec3::Y * f * METRES_PER_FOOT;
        // The prompt on the cannon: at its pivot, then 15 ft over it.
        hang(&mut w, &mut e, PROMPT, Value::Entity(CANNON));
        assert_eq!(w.actor(PROMPT).unwrap().position, base);
        lift(&mut w, &mut e, PROMPT, PROMPT, 15.0);
        w.place_hung(&mut e);
        let p = w.actor(PROMPT).unwrap().position;
        assert!(p.distance(base + ft(15.0)) < 1e-5, "{p:?}");
        // The cannon pushed and turned: still 15 ft over it.
        let pushed = base + Vec3::new(3.0, 0.0, -1.0);
        w.actors.get_mut(&CANNON).unwrap().position = pushed;
        w.actors.get_mut(&CANNON).unwrap().angles.facing = 120.0;
        w.place_hung(&mut e);
        let p = w.actor(PROMPT).unwrap().position;
        assert!(p.distance(pushed + ft(15.0)) < 1e-5, "{p:?}");
        // Its bob writes it over the cannon: there it stays.
        lift(&mut w, &mut e, PROMPT, CANNON, 14.5);
        w.place_hung(&mut e);
        let p = w.actor(PROMPT).unwrap().position;
        assert!(p.distance(pushed + ft(14.5)) < 1e-5, "{p:?}");
        // The turban on his node 2, displaced 0.5 ft down.
        let head = Vec3::new(0.0, 2.0, 0.3);
        let mut nodes = vec![NodePose::at(Vec3::ZERO); 3];
        nodes[2] = NodePose::at(head);
        w.set_nodes(HERO, nodes.clone());
        let node = w.objs.value(Object::Node(HERO, 2));
        hang(&mut w, &mut e, HAT, node);
        w.displace(HERO, Value::Entity(HAT), 0, 0.5, 0.0, 270.0, &mut e)
            .unwrap();
        let down = pivot + head - ft(0.5);
        assert!(w.position(HAT, &mut e).unwrap().distance(down) < 1e-5);
        w.place_hung(&mut e);
        assert!(w.actor(HAT).unwrap().position.distance(down) < 1e-5);
        // His head tips a quarter forward: the offset tips with it.
        let tip = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        nodes[2] = NodePose {
            at: head,
            turn: tip,
        };
        w.set_nodes(HERO, nodes.clone());
        w.place_hung(&mut e);
        let tipped = pivot + head + tip * -ft(0.5);
        let p = w.actor(HAT).unwrap().position;
        assert!(p.distance(tipped) < 1e-5, "{p:?} want {tipped:?}");
        assert!(w.position(HAT, &mut e).unwrap().distance(tipped) < 1e-5);
        // Hung again on node 1: at it, no offset.
        nodes[1] = NodePose::at(Vec3::new(0.4, 1.0, 0.0));
        w.set_nodes(HERO, nodes);
        let node = w.objs.value(Object::Node(HERO, 1));
        hang(&mut w, &mut e, HAT, node);
        w.place_hung(&mut e);
        let p = w.actor(HAT).unwrap().position;
        assert!(p.distance(pivot + Vec3::new(0.4, 1.0, 0.0)) < 1e-5, "{p:?}");
        // Let go: a write puts it where it says, and nothing moves it after.
        hang(&mut w, &mut e, PROMPT, Value::Null);
        lift(&mut w, &mut e, PROMPT, CANNON, 20.0);
        w.actors.get_mut(&CANNON).unwrap().position = base;
        w.place_hung(&mut e);
        let p = w.actor(PROMPT).unwrap().position;
        assert!(p.distance(pushed + ft(20.0)) < 1e-5, "{p:?}");
    }

    /// AH.226's offset off to the side (the slave's hammer 0.5 ft off his
    /// hand at heading 230, the spinner's halves 4 ft either side of it):
    /// put 4 ft to one side of an actor facing a quarter round, it turns
    /// with him as he turns and walks; put 1 ft to one side of a posed node
    /// turned a quarter, it turns with the node. Kept in the frame of what
    /// it hangs from as it was put, not in the level's.
    #[test]
    fn a_hung_actors_offset_turns_with_what_it_hangs_from() {
        let mut e = bench();
        let mut w = world();
        const OWNER: Handle = 0x6000_0020;
        const SIDE: Handle = 0x6000_0021;
        let base = Vec3::new(2.0, 1.0, -3.0);
        let mut owner = ball(base, 0.0);
        owner.angles.facing = 90.0;
        w.actors.insert(OWNER, owner);
        w.actors.insert(SIDE, ball(base, 0.0));
        let hang = |w: &mut ScriptWorld, e: &mut Bench, on: Value| {
            w.set_member(
                &at(
                    "abstractplacement",
                    "attach node",
                    Some(Value::Entity(SIDE)),
                    None,
                ),
                Member::AttachNode,
                on,
                e,
            )
            .unwrap();
        };
        // On the actor, 4 ft along heading 30.
        hang(&mut w, &mut e, Value::Entity(OWNER));
        w.displace(OWNER, Value::Entity(SIDE), 0, 4.0, 30.0, 0.0, &mut e)
            .unwrap();
        let off = w.actor(SIDE).unwrap().position - base;
        assert!(
            off.y.abs() < 1e-5 && off.length() > 1.0,
            "to the side: {off}"
        );
        w.place_hung(&mut e);
        let p = w.actor(SIDE).unwrap().position;
        assert!(p.distance(base + off) < 1e-5, "where it was put: {p}");
        // He turns a quarter more and walks: it turns and walks with him.
        let walked = base + Vec3::new(-1.5, 0.0, 2.0);
        w.actors.get_mut(&OWNER).unwrap().position = walked;
        w.actors.get_mut(&OWNER).unwrap().angles.facing = 180.0;
        w.place_hung(&mut e);
        let want = walked + Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * off;
        let p = w.actor(SIDE).unwrap().position;
        assert!(p.distance(want) < 1e-4, "{p} want {want}");
        // On his node 0, posed a quarter turned about the vertical, 1 ft
        // along heading 0: turned with the node as the node turns back.
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let head = Vec3::new(0.0, 1.5, 0.0);
        w.set_nodes(
            OWNER,
            vec![NodePose {
                at: head,
                turn: quarter,
            }],
        );
        let node = w.objs.value(Object::Node(OWNER, 0));
        hang(&mut w, &mut e, node);
        let on = w.position(SIDE, &mut e).unwrap();
        w.displace(OWNER, Value::Entity(SIDE), 0, 1.0, 0.0, 0.0, &mut e)
            .unwrap();
        let off = w.position(SIDE, &mut e).unwrap() - on;
        assert!(
            off.y.abs() < 1e-5 && off.length() > 0.2,
            "to the side: {off}"
        );
        w.set_nodes(
            OWNER,
            vec![NodePose {
                at: head,
                turn: Quat::IDENTITY,
            }],
        );
        w.place_hung(&mut e);
        let want = on + quarter.inverse() * off;
        let p = w.actor(SIDE).unwrap().position;
        assert!(p.distance(want) < 1e-4, "{p} want {want}");
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
    use guac_motion::seam::{Body, BodyKind, Collider, Mesh, PhysicsSeam};
    use guac_motion::tuning::{FOOT, Tuning, eruptor_solid};
    use guac_script::{Answer, MemberId, Obj, OpCall};

    use super::*;
    use crate::script_engine::{Actor, NodePose, Pad, Physics, Solid, Walker};
    use crate::scripts::{Files, Placed, headless_objects};

    pub(super) const C1: Handle = 0x6100_0001;
    const C2: Handle = 0x6100_0002;
    const C3: Handle = 0x6100_0003;
    const HERO: Handle = 0x6100_0010;
    pub(super) const STEP: f32 = 1.0 / 60.0;
    pub(super) const WP: Obj = Obj { unit: 9, index: 1 };
    const WP2: Obj = Obj { unit: 9, index: 2 };

    /// Open level ground whose top is y = 0; the hero and the creatures
    /// bodies in it as the embedder keeps them.
    pub(super) struct Pit {
        pub(super) world: ParryWorld,
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
            turns: HashMap::new(),
        };
        let handles = HashMap::new();
        ScriptWorld::new(&files, &handles, headless_objects(&handles))
    }

    /// openspec AU.53, AS.168: a clone whose solid is a shape, solid to
    /// actors and an actor surface (level 201's bridge planks) stands in the
    /// world as a solid body where its script leaves it, so a hero walks on
    /// it; let go (no longer solid to actors, given a gravity) it falls and
    /// stands nowhere. A clone short of any of the three gets no body, nor
    /// does a placed actor (its prop's), and one let go that never stood
    /// doesn't fall.
    #[test]
    fn a_clone_solid_to_actors_and_a_surface_stands_until_let_go() {
        let mut e = pit();
        let template = |index| Obj { unit: 5, index };
        let mut solid = chompy(Vec3::ZERO, 0.0).solid;
        solid.shape = 1;
        solid.interacts_with_actors = 1;
        solid.acts_as_actor_surface = 1;
        let files = Files {
            placed: (3..8)
                .map(|i| crate::scripts::Placed {
                    solid: Some(solid),
                    ..crate::scripts::Placed::named(template(i), "it", Some("C:/x/It.ai"), 5)
                })
                .collect(),
            ..Files::default()
        };
        let handles = HashMap::new();
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let clone = |index, solid| Actor {
            template: Some(template(index)),
            position: Vec3::new(0.0, 7.0, 0.0),
            solid,
            ..Actor::blank()
        };
        let (plank, no_surface, shapeless, not_solid, placed) =
            (C3, 0x6100_0020, 0x6100_0021, 0x6100_0022, 0x6100_0023);
        w.actors.insert(plank, clone(3, solid));
        w.actors.insert(
            no_surface,
            clone(
                4,
                Solid {
                    acts_as_actor_surface: 0,
                    ..solid
                },
            ),
        );
        w.actors
            .insert(shapeless, clone(5, Solid { shape: 5, ..solid }));
        w.actors.insert(
            not_solid,
            clone(
                6,
                Solid {
                    interacts_with_actors: 0,
                    ..solid
                },
            ),
        );
        w.actors.insert(
            placed,
            Actor {
                obj: Some(template(7)),
                template: None,
                ..clone(7, solid)
            },
        );
        let bodies: Vec<Handle> = w.creature_bodies().iter().map(|b| b.0).collect();
        assert_eq!(bodies, [plank]);
        let stands: Vec<Handle> = [plank, no_surface, shapeless, not_solid, placed]
            .into_iter()
            .filter(|h| w.actor(*h).is_some_and(Actor::stands_for_actors))
            .collect();
        assert_eq!(stands, [plank], "a mesh shape stands for none either");
        // A clone the engine walks as a creature is a creature's body, never
        // one to stand on (the au-ship-parts review).
        let mut walking = clone(3, solid);
        walking.walker = Some(Box::new(Walker {
            body: None,
            aim: guac_motion::steer::Aim::Still,
            along: 0,
        }));
        assert!(!walking.stands_for_actors());
        // Let go: no longer solid to actors, falling at 60 ft/s²; the one
        // that never stood given the same gravity stays put.
        let fall = |a: &mut Actor| {
            a.physics.gravity_acceleration = 60.0;
            a.physics.gravity_max_speed = 60.0;
            a.physics.time_scale = 100.0;
        };
        w.advance(STEP, &mut e);
        let a = w.actors.get_mut(&plank).unwrap();
        a.solid.interacts_with_actors = 0;
        fall(a);
        fall(w.actors.get_mut(&not_solid).unwrap());
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        assert!(w.creature_bodies().is_empty());
        let y = w.actor(plank).unwrap().position.y;
        assert!(y < 6.0, "falling: {y}");
        assert_eq!(w.actor(not_solid).unwrap().position.y, 7.0);
    }

    /// openspec AS.183: a surface's body turns as its actor does, its tilt
    /// and roll too: level 201's fourth vine bridge, a box laid along its
    /// facing from its activator and tilted 1° nose up, rises at its far end
    /// to the ground it reaches. A creature's body stands upright whatever
    /// its actor's tilt.
    #[test]
    fn a_surface_stands_tilted_as_its_actor_is() {
        let template = Obj { unit: 5, index: 3 };
        let mut solid = chompy(Vec3::ZERO, 0.0).solid;
        (
            solid.shape,
            solid.interacts_with_actors,
            solid.acts_as_actor_surface,
        ) = (1, 1, 1);
        (solid.width, solid.height, solid.length) = (15.0, 6.0, 64.0);
        solid.offset = [0.0, 3.0, 30.0];
        let files = Files {
            placed: vec![crate::scripts::Placed {
                solid: Some(solid),
                ..crate::scripts::Placed::named(template, "it", Some("C:/x/It.ai"), 5)
            }],
            ..Files::default()
        };
        let handles = HashMap::new();
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let angles = Angles {
            facing: 270.0,
            tilt: 1.0,
            roll: 0.0,
        };
        w.actors.insert(
            C3,
            Actor {
                template: Some(template),
                position: Vec3::new(10.0, 7.0, 0.0),
                angles,
                solid,
                ..Actor::blank()
            },
        );
        let mut walking = chompy(Vec3::new(0.0, 7.0, 0.0), 270.0);
        walking.angles.tilt = 20.0;
        w.actors.insert(C1, walking);
        let bodies = w.creature_bodies();
        let pose = |h| bodies.iter().find(|b| b.0 == h).unwrap().2;
        let (bridge, creature) = (pose(C3), pose(C1));
        assert!(
            bridge.rotation.angle_between(euler([1.0, 270.0, 0.0])) < 1e-3,
            "{bridge:?}"
        );
        // Its far end (west, along its facing) rises over its near end.
        let top = |along: f32| bridge.position + bridge.rotation * Vec3::new(0.0, 0.0, along);
        let (near, far) = (top(-9.0), top(9.0));
        assert!(far.x < near.x, "laid west: {near} {far}");
        assert!(far.y - near.y > 0.3, "rises to the far side: {near} {far}");
        assert!(
            creature
                .rotation
                .angle_between(Quat::from_rotation_y(270f32.to_radians()))
                < 1e-3,
            "a creature stands upright: {creature:?}"
        );
    }

    /// openspec AU.53: a scene's jump (level 201's practice pit elves) takes
    /// its actor's shape off for the arc, so it falls by its gravity with no
    /// body to sweep; back within a foot of its waypoint's height the scene
    /// resets the shape to the file's, and with its body back it lands on
    /// the ground and stops. A reset of a movement member puts back the
    /// file's value too.
    #[test]
    fn a_scene_jump_falls_by_its_gravity_and_lands_once_its_shape_is_reset() {
        let mut e = pit();
        let obj = Obj { unit: 4, index: 7 };
        let mut elf = chompy(Vec3::new(0.0, 3.0, 0.0), 0.0);
        elf.obj = Some(obj);
        let mut placed = crate::scripts::Placed::named(
            obj,
            "Practice_Pit_Cutscene_Elf_Near",
            Some("C:/x/Elf.ai"),
            5,
        );
        placed.solid = Some(elf.solid);
        placed.physics = Some(elf.physics);
        let files = Files {
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: vec![placed],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
        };
        let handles = HashMap::from([(obj, C1)]);
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        // As the jump's start leaves it: no shape, 128 ft/s² down, launched
        // up at 30 ft/s.
        elf.solid.shape = 0;
        elf.physics.gravity_acceleration = 128.0;
        elf.physics.gravity_max_speed = 9999.0;
        elf.physics.ground_deceleration = 1.0;
        elf.velocity = Vec3::new(0.0, 30.0, 0.0);
        w.actors.insert(C1, elf);
        let mut top: f32 = 0.0;
        for _ in 0..30 {
            w.advance(STEP, &mut e);
            top = top.max(w.actor(C1).unwrap().position.y);
        }
        let a = w.actor(C1).unwrap();
        assert!(top > 3.9, "up: {top}");
        assert!(
            a.velocity.y < 0.0 && a.position.y < top,
            "coming down: {:?}",
            a.position
        );
        // Its shape and deceleration back as its file has them.
        let collision = w.objs.value(Object::Facet(C1, Facet::Collision));
        let movement = w.objs.value(Object::Facet(C1, Facet::Movement));
        assert!(w.reset_facet(Member::Solid(SolidField::Shape), Some(collision)));
        assert!(w.reset_facet(
            Member::Physics(PhysicsField::GroundDeceleration),
            Some(movement)
        ));
        assert!(!w.reset_facet(
            Member::Physics(PhysicsField::GroundDeceleration),
            Some(collision)
        ));
        assert_eq!(w.actor(C1).unwrap().solid.shape, 3);
        assert_eq!(w.actor(C1).unwrap().physics.ground_deceleration, 80.0);
        for _ in 0..120 {
            w.advance(STEP, &mut e);
        }
        let a = w.actor(C1).unwrap();
        assert!(
            a.position.y.abs() < 0.2 && a.position.y > -0.2,
            "landed: {:?}",
            a.position
        );
        assert!(a.velocity.y.abs() < 1.0, "at rest: {:?}", a.velocity);
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

    /// openspec AU.34: a `move from` itself is never done: the mover goes
    /// on along its facing at its speed (the hub's fairies, whose scripts
    /// turn it each tick), where one from another point stops once clear.
    #[test]
    fn a_move_from_itself_goes_on_along_its_facing() {
        let mut e = pit();
        let mut w = world();
        let mut fairy = chompy(Vec3::new(0.0, 3.0, 0.0), 90.0);
        fairy.walker = None;
        w.actors.insert(C1, fairy);
        let from = |target: Value, beyond: i32| {
            op(
                Class::MoveFrom,
                C1,
                vec![
                    ("_indexRHS", None),
                    ("_LHS", Some(target)),
                    ("_RHS", Some(Value::Int(beyond))),
                    ("_NP", None),
                ],
                Vec::new(),
            )
        };
        for _ in 0..60 {
            assert_eq!(
                ask(&mut w, &mut e, &from(Value::Entity(C1), 0)),
                Answer::Done
            );
            w.advance(STEP, &mut e);
        }
        let a = w.actor(C1).unwrap();
        assert!(a.position.x > 2.0, "along its facing: {}", a.position);
        assert!(a.position.z.abs() < 1e-3, "{}", a.position);
        assert!((a.position.y - 3.0).abs() < 1e-6, "{}", a.position);
        // From a point it's already clear of: done, as ever.
        assert!(matches!(
            ask(&mut w, &mut e, &from(Value::Object(WP), 0)),
            Answer::Run { .. }
        ));
        // A creature's own place it's clear of at once, and it stands.
        let creature = chompy(Vec3::new(0.0, 3.0, 0.0), 90.0);
        w.actors.insert(C1, creature);
        assert!(matches!(
            ask(&mut w, &mut e, &from(Value::Entity(C1), 0)),
            Answer::Run { .. }
        ));
        w.advance(STEP, &mut e);
        let at = w.actor(C1).unwrap().position;
        assert!(at.x.abs() < 1e-3, "it stands: {at}");
    }

    /// openspec AU.34: of those the engine doesn't walk, the people whose
    /// script runs stand solid while they're on and interact with actors;
    /// a person off the listing, asleep, taken off or solid to nothing has
    /// no body, and neither has any other placement.
    #[test]
    fn the_people_stand_solid_while_their_scripts_run() {
        let person = Obj { unit: 3, index: 40 };
        let quiet = Obj { unit: 3, index: 41 };
        let crate_ = Obj { unit: 3, index: 42 };
        let dir = "C:/tfb/Content/Levels/Level_Hub_2/Scripts";
        let files = Files {
            placed: vec![
                crate::scripts::Placed::named(person, "Flynn", Some(&format!("{dir}/Flynn.ai")), 4),
                crate::scripts::Placed::named(quiet, "Cali", Some(&format!("{dir}/Cali.ai")), 4),
                crate::scripts::Placed::named(crate_, "Crate", None, 4),
            ],
            ..Files::default()
        };
        let handles = HashMap::from([(person, C1), (quiet, C2), (crate_, C3)]);
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        for h in [C1, C2, C3] {
            let mut a = chompy(Vec3::ZERO, 0.0);
            a.walker = None;
            a.obj = w.actor(h).and_then(|old| old.obj);
            w.actors.insert(h, a);
        }
        let bodies = |w: &ScriptWorld| {
            w.creature_bodies()
                .into_iter()
                .map(|b| b.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(bodies(&w), [C1, C2], "the people, not the crate");
        w.actors.get_mut(&C2).unwrap().solid.interacts_with_actors = 0;
        assert_eq!(bodies(&w), [C1], "solid to nothing");
        w.set_inert([C1]);
        assert!(bodies(&w).is_empty(), "off the listing");
        w.set_inert([]);
        w.actors.get_mut(&C1).unwrap().active = false;
        assert!(bodies(&w).is_empty(), "asleep");
        w.actors.get_mut(&C1).unwrap().active = true;
        w.actors.get_mut(&C1).unwrap().removed = true;
        assert!(bodies(&w).is_empty(), "taken off");
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

    /// AS.52: an actor the engine doesn't walk as a creature (a cutscene's
    /// actor, no body under it) steered by `move to` all the same: toward
    /// the point at its ground speed, turning at its rate, at the height it
    /// stands, the block running once it's within reach, a wall in its way
    /// no stop; asked no more, it slows to a stop; along a path, each
    /// waypoint in turn.
    #[test]
    fn an_actor_no_creature_walks_is_steered_by_move_to_with_no_body() {
        let mut e = pit();
        // A wall across its way, which a creature's body would stop on.
        let wall = Body {
            id: BodyId(2),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(4.0, 1.0, -3.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(0.2, 1.0, 5.0),
                margin: 0.0,
            }),
        };
        e.world = ParryWorld::new(&[wall]).unwrap();
        let mut w = world();
        let mut mabu = chompy(Vec3::new(0.0, 0.3, 0.0), 0.0);
        mabu.walker = None;
        // Its solid meets no level, as the Tree Rex's homing charge's: no
        // body (one that does walks on one: AS.79's test).
        mabu.solid.interacts_with_level_mesh = 0;
        w.actors.insert(C1, mabu);
        let goal = Vec3::new(8.0, 0.0, -6.0);
        let (mut frames, mut top) = (0, 0.0f32);
        let mut last = (Vec3::new(0.0, 0.3, 0.0), 0.0f32);
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
            top = top.max(across.length() / STEP / FOOT);
            let turned = wrap(a.angles.facing - last.1).abs();
            assert!(turned <= 12.0 + 1e-3, "frame {frames}: {turned}°");
            assert!(
                (a.position.y - 0.3).abs() < 1e-6,
                "at its height: {}",
                a.position
            );
            last = (a.position, a.angles.facing);
            assert!(frames < 200, "never got there: {:?}", a.position);
        }
        assert!(top <= 20.0 + 1e-3 && top > 19.9, "{top}");
        let at = w.actor(C1).unwrap().position;
        assert!(Vec3::new(at.x - goal.x, 0.0, at.z - goal.z).length() <= 5.0 * FOOT);
        assert!((92..=100).contains(&frames), "{frames}");
        assert_eq!(w.scene_walks[&C1].aim, Aim::Still);
        assert!(
            w.actor(C1).unwrap().walker.is_none(),
            "no creature made of it"
        );
        // Asked no more, it slows (80 ft/s² from 20 ft/s) to a stop.
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        let stopped = w.actor(C1).unwrap().position;
        w.advance(STEP, &mut e);
        assert_eq!(w.actor(C1).unwrap().position, stopped);
        assert_eq!(w.actor(C1).unwrap().velocity, Vec3::ZERO);
        // Along a path: each waypoint in turn, the next its aim at once.
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
                    assert_eq!(w.scene_walks[&C1].aim, Aim::To(Vec3::new(8.0, 0.0, 6.0)));
                }
            }
            w.advance(STEP, &mut e);
        }
        assert_eq!(w.scene_walks[&C1].along, 1, "on the last waypoint");
        let at = w.actor(C1).unwrap().position;
        assert!(
            (at - Vec3::new(8.0, at.y, 6.0)).length() <= 2.0 * FOOT,
            "{at}"
        );
    }

    /// openspec AS.205: what the game's pause lets rest the engine moves
    /// none of, mid-motion as the pause finds them: a Chompy walking toward
    /// a point, a scene's actor led there with no body and a clone flying
    /// on its velocity each stand where they were for as long as the rest
    /// holds, then go on from there.
    #[test]
    fn the_engine_holds_what_the_pause_lets_rest() {
        let mut e = pit();
        let mut w = world();
        w.actors.insert(C1, chompy(Vec3::ZERO, 0.0));
        let mut led = chompy(Vec3::new(0.0, 0.3, 4.0), 0.0);
        led.walker = None;
        led.solid.interacts_with_level_mesh = 0;
        w.actors.insert(C2, led);
        let mut flying = Actor::blank();
        flying.position = Vec3::new(-5.0, 10.0, 0.0);
        flying.launched = true;
        flying.velocity = Vec3::new(10.0, 0.0, 0.0);
        flying.physics.gravity_acceleration = 32.0;
        w.actors.insert(C3, flying);
        let go = |w: &mut ScriptWorld, e: &mut Pit| {
            for walker in [C1, C2] {
                let mut call = move_to(Value::Object(WP), 1);
                (call.placement, call.receiver) = (walker, Value::Entity(walker));
                ask(w, e, &call);
            }
            w.advance(STEP, e);
        };
        for _ in 0..10 {
            go(&mut w, &mut e);
        }
        let at = |w: &ScriptWorld| [C1, C2, C3].map(|h| w.actor(h).unwrap().position);
        let before = at(&w);
        assert!(w.scene_walks.contains_key(&C2), "led as a scene leads it");
        w.resting = [C1, C2, C3].into_iter().collect();
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        assert_eq!(at(&w), before, "moved while resting");
        w.resting.clear();
        for _ in 0..10 {
            go(&mut w, &mut e);
        }
        let after = at(&w);
        for k in 0..3 {
            assert!(after[k].distance(before[k]) > 0.05, "{k}: {after:?}");
        }
    }

    /// AS.166: one of the people the TV draws, its solid meeting the level,
    /// led up a slope by `move to` stands on the ground under each step, as
    /// chapter 1's Norticus at the quarry climbs its slope; a scene's actor
    /// and a person whose solid meets no level stay at the height they
    /// stand; past the ramp's end, over nothing, a person keeps its height.
    /// The person's own solid in the seam (as the embedder keeps the
    /// people's, `ScriptWorld::creature_bodies`) is never his ground: met,
    /// he'd step up onto his own head; nor a barrel on his way, whose solid
    /// doesn't act as an actor surface (the hub's walking Mabu stepped 1.78 m
    /// up onto one).
    #[test]
    fn a_person_led_up_a_slope_stands_on_the_ground() {
        // y = x / 4 from x = -20 to 20.
        let ramp = Mesh::new(
            vec![
                Vec3::new(-20.0, -5.0, -20.0),
                Vec3::new(20.0, 5.0, -20.0),
                Vec3::new(20.0, 5.0, 20.0),
                Vec3::new(-20.0, -5.0, 20.0),
            ],
            vec![[0, 3, 2], [0, 2, 1]],
            vec![0, 0],
        )
        .unwrap();
        let ground = Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::ZERO),
            collider: Collider::Mesh(std::sync::Arc::new(ramp)),
        };
        let person = Obj { unit: 3, index: 50 };
        let actor = Obj { unit: 3, index: 51 };
        let barrel = Obj { unit: 3, index: 52 };
        let up = Obj { unit: 9, index: 7 };
        let past = Obj { unit: 9, index: 8 };
        let files = Files {
            placed: vec![
                Placed::named(person, "4.NPC_Norticus at end", None, 4),
                Placed::named(actor, "213_Gigantus_Norticus", None, 4),
                Placed::named(barrel, "barrel_snow_intact(24)", None, 4),
            ],
            points: HashMap::from([
                (up, Vec3::new(14.0, 3.5, 0.0)),
                (past, Vec3::new(30.0, 3.5, 0.0)),
            ]),
            ..Files::default()
        };
        let handles = HashMap::from([(person, C1), (actor, C2), (barrel, C3)]);
        // The person's solid where he starts, 1.6 m tall on the ramp.
        let own = Body {
            id: BodyId(2),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(0.0, 0.8, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(0.5, 0.8, 0.5),
                margin: 0.0,
            }),
        };
        // A barrel 1.5 m tall on his way at x = 8, no actor surface.
        let cask = Body {
            id: BodyId(3),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(8.0, 2.75, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(0.5, 0.75, 0.5),
                margin: 0.0,
            }),
        };
        // Each walked toward `to` until reached or 300 frames: its heights
        // off the ramp under it at each frame, its last place.
        let walk = |h: Handle, meets_level: i32, to: Obj| {
            let mut e = pit();
            e.world = ParryWorld::new(&[ground.clone(), own.clone(), cask.clone()]).unwrap();
            e.bodies.insert(C1, own.id);
            e.bodies.insert(C3, cask.id);
            let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            // Chapter 1's run: whose people its placements are (AS.176).
            w.set_level("level_213_tutotial");
            w.actors.get_mut(&C3).unwrap().solid.acts_as_actor_surface = 0;
            let a = w.actors.get_mut(&h).unwrap();
            let mut c = chompy(Vec3::ZERO, 90.0);
            c.obj = a.obj;
            c.walker = None;
            c.solid.interacts_with_level_mesh = meets_level;
            *a = c;
            let mut off = 0.0f32;
            let mut call = move_to(Value::Object(to), 1);
            call.placement = h;
            call.receiver = Value::Entity(h);
            for _ in 0..300 {
                if matches!(ask(&mut w, &mut e, &call), Answer::Run { .. }) {
                    break;
                }
                w.advance(STEP, &mut e);
                let at = w.actor(h).unwrap().position;
                if at.x <= 20.0 {
                    off = off.max((at.y - at.x / 4.0).abs());
                }
            }
            (off, w.actor(h).unwrap().position)
        };
        let mut chapter_1 = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        assert!(chapter_1.people().is_empty(), "on no level, no level's");
        chapter_1.set_level("level_213_tutotial");
        assert_eq!(chapter_1.people(), [C1]);
        let (off, at) = walk(C1, 1, up);
        assert!(off < 0.01, "on the ramp: {off}");
        assert!(
            (at.x - 14.0).abs() < 1.0 && (at.y - 3.5).abs() < 0.3,
            "{at}"
        );
        // A person whose solid meets no level stays at its height (a
        // scene's actor whose solid meets the level walks on a body: AS.79).
        let (_, at) = walk(C1, 0, up);
        assert!(at.y.abs() < 1e-6, "at its height: {at}");
        assert!(at.x > 13.0, "walked all the same: {at}");
        // Off the ramp's end at 5 m, over nothing: at the height it left it.
        let (_, at) = walk(C1, 1, past);
        assert!(at.x > 25.0 && (at.y - 5.0).abs() < 0.05, "{at}");
    }

    /// openspec AS.79: an actor a scene walks whose solid meets the level
    /// (chapter 1's outro's Mabu, level 214's intro's) walks on a body, as
    /// the engine walks a creature: a wall across its way stops it short of
    /// its aim, standing on the floor; walked off a ledge with its gravity
    /// (32 ft/s² to 120) it falls and stands on the ground below, and one
    /// whose gravity's top speed is nothing (level 214's key's, as its
    /// script sets it) walks on at its height; put over the floor and its
    /// walk over, it comes down onto it.
    #[test]
    fn a_scene_walk_meeting_the_level_walks_on_a_body() {
        let floor = |id: u32, x: [f32; 2], top: f32| Body {
            id: BodyId(id),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new((x[0] + x[1]) / 2.0, top - 0.5, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new((x[1] - x[0]) / 2.0, 0.5, 20.0),
                margin: 0.0,
            }),
        };
        let wall = Body {
            id: BodyId(9),
            kind: BodyKind::Static,
            pose: Pose::at(Vec3::new(4.0, 1.0, 0.0)),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::new(0.2, 1.0, 20.0),
                margin: 0.0,
            }),
        };
        let mabu = |at: Vec3, max: f32| {
            let mut a = chompy(at, 90.0);
            a.walker = None;
            a.physics.gravity_acceleration = 32.0;
            a.physics.gravity_max_speed = max;
            a
        };
        // Walked toward `to` (metres) for up to 200 frames: where it ends.
        let walk = |bodies: &[Body], a: Actor, to: Vec3| {
            let mut e = pit();
            e.world = ParryWorld::new(bodies).unwrap();
            let mut w = world();
            w.points.insert(WP, to);
            w.actors.insert(C1, a);
            for _ in 0..200 {
                if matches!(
                    ask(&mut w, &mut e, &move_to(Value::Object(WP), 1)),
                    Answer::Run { .. }
                ) {
                    break;
                }
                w.advance(STEP, &mut e);
            }
            for _ in 0..60 {
                w.advance(STEP, &mut e);
            }
            w.actor(C1).unwrap().position
        };
        let to = Vec3::new(8.0, 0.0, 0.0);
        // A wall at x = 4: stopped short of it, on the floor.
        let at = walk(
            &[floor(1, [-20.0, 20.0], 0.0), wall.clone()],
            mabu(Vec3::ZERO, 120.0),
            to,
        );
        assert!(at.x < 3.8 && at.x > 2.0, "stopped by the wall: {at}");
        assert!(at.y.abs() < 0.05, "on the floor: {at}");
        // Its solid meeting no level: through the wall at its height.
        let mut ghost = mabu(Vec3::ZERO, 120.0);
        ghost.solid.interacts_with_level_mesh = 0;
        let at = walk(&[floor(1, [-20.0, 20.0], 0.0), wall], ghost, to);
        assert!(at.x > 6.0 && at.y.abs() < 1e-6, "through it: {at}");
        // A ledge at x = 5, 2 m down: off it, onto the ground below.
        let ledge = [floor(1, [-20.0, 5.0], 0.0), floor(2, [5.0, 20.0], -2.0)];
        let at = walk(&ledge, mabu(Vec3::ZERO, 120.0), to);
        assert!(at.x > 6.0, "walked on: {at}");
        assert!((at.y + 2.0).abs() < 0.05, "down on the ground below: {at}");
        // Its gravity's top speed nothing: on at its height over nothing.
        let at = walk(&ledge, mabu(Vec3::ZERO, 0.0), to);
        assert!(at.x > 6.0 && at.y.abs() < 0.05, "at its height: {at}");
        // Over the floor with its walk over: down onto it.
        let at = walk(
            &[floor(1, [-20.0, 20.0], 0.0)],
            mabu(Vec3::new(8.0, 1.5, 0.0), 120.0),
            to,
        );
        assert!(at.y.abs() < 0.05, "down on the floor: {at}");
    }

    /// AS.68: a scene walk steps from its actor's velocity, which is its
    /// one motion. Level 214's homing charge (no body: it meets neither
    /// actors nor the level) writes its speed, 25 ft/s, each tick before
    /// its `move to` on its own numbers (25 ft/s, 250 ft/s² up, none down,
    /// 50°/s): it gets up to that speed and never past it, turning no
    /// faster than its rate, the write never flying it besides; a velocity
    /// left over from a flight doesn't fly it while it's walked; and asked
    /// no more, with nothing to slow it, it slides on at its speed.
    #[test]
    fn a_scene_walk_steps_from_its_actors_velocity_and_alone_moves_it() {
        let mut e = pit();
        let mut w = world();
        let mut charge = chompy(Vec3::new(0.0, 0.3, 0.0), 90.0);
        charge.walker = None;
        charge.solid.shape = 1;
        charge.solid.interacts_with_actors = 0;
        charge.solid.interacts_with_level_mesh = 0;
        charge.physics = Physics {
            ground_max_speed: 25.0,
            ground_acceleration: 250.0,
            ground_deceleration: 0.0,
            ground_turn_rate: 50.0,
            time_scale: 100.0,
            ..Physics::default()
        };
        w.actors.insert(C1, charge);
        let velocity = w.objs.value(Object::Facet(C1, Facet::Velocity));
        let length = Access {
            placement: C1,
            receiver: Some(velocity),
            member: MemberId::named("vectormeasurement", "length"),
            other_point: None,
            via: None,
            op: None,
        };
        let mut speeds = Vec::new();
        let mut last = w.actor(C1).unwrap().clone();
        for k in 0..120 {
            if k == 60 {
                // As if a flight had set it going: the walk still moves it
                // alone.
                w.actors.get_mut(&C1).unwrap().launched = true;
            }
            w.set_member(
                &length,
                Member::Vector(VectorField::Length),
                Value::Float(25.0),
                &mut e,
            )
            .unwrap();
            assert!(matches!(
                ask(&mut w, &mut e, &move_to(Value::Object(WP), 0)),
                Answer::Done
            ));
            w.advance(STEP, &mut e);
            let a = w.actor(C1).unwrap().clone();
            let across = Vec3::new(
                a.position.x - last.position.x,
                0.0,
                a.position.z - last.position.z,
            );
            speeds.push(across.length() / STEP / FOOT);
            let turned = wrap(a.angles.facing - last.angles.facing).abs();
            assert!(turned <= 50.0 * STEP + 1e-3, "frame {k}: {turned}°");
            assert!(k >= 60 || !a.launched, "frame {k}: the write launched it");
            last = a;
        }
        let top = speeds.iter().copied().fold(0.0f32, f32::max);
        assert!(top <= 25.0 + 1e-3, "past its speed: {top}");
        assert!(speeds[10..].iter().all(|s| *s > 24.9), "{speeds:?}");
        // Asked no more: no deceleration, so on at its speed.
        let before = w.actor(C1).unwrap().position;
        w.advance(STEP, &mut e);
        let after = w.actor(C1).unwrap().position;
        let slid = Vec3::new(after.x - before.x, 0.0, after.z - before.z).length() / STEP / FOOT;
        assert!((slid - 25.0).abs() < 1e-2, "{slid}");
    }

    /// openspec AS.187: a scene's jump on an actor its scene walks (level
    /// 201's practice pit jumps Brock to his landing): from a velocity
    /// written with a rise, it flies on it under its own gravity (128
    /// ft/s², as the jump sets it), never steered toward the walk's last
    /// aim, up and down through the floor under it as its shape is off;
    /// a velocity of nothing, the jump's end, holds it where its sequence
    /// sets it down. Its shape given back on the way down, it lands on the
    /// floor and stops there, the jump over. A velocity with no rise is the
    /// walk's to steer from.
    #[test]
    fn a_scene_jump_flies_its_arc_until_its_end() {
        let mut e = pit();
        let mut w = world();
        let mut brock = chompy(Vec3::new(0.0, 1.0, 0.0), 0.0);
        brock.walker = None;
        brock.solid.shape = 0;
        brock.physics.gravity_acceleration = 128.0;
        brock.physics.gravity_max_speed = 9999.0;
        w.actors.insert(C1, brock);
        assert!(matches!(
            ask(&mut w, &mut e, &move_to(Value::Object(WP), 0)),
            Answer::Done
        ));
        // A write with no rise: the walk's, no jump.
        w.set_velocity(C1, Vec3::new(0.0, 0.0, 5.0), &mut e)
            .unwrap();
        assert!(!w.scene_walks[&C1].jump);
        // Up at 40 ft/s, across at 20 along +z.
        w.set_velocity(C1, Vec3::new(0.0, 40.0, 20.0), &mut e)
            .unwrap();
        assert!(w.scene_walks[&C1].jump);
        let start = w.actor(C1).unwrap().position;
        let mut top = start.y;
        let mut path = Vec::new();
        for _ in 0..60 {
            w.advance(STEP, &mut e);
            let a = w.actor(C1).unwrap();
            top = top.max(a.position.y);
            path.push(a.position);
        }
        // 40 ft/s up against 128 ft/s² peaks 6.25 ft over, less half a
        // step's climb (a frame's velocity before its move, as a launched
        // actor's): 5.92 ft, 1.80 m.
        let rise = top - start.y;
        let want = (6.25 - 40.0 * STEP / 2.0) * FOOT;
        assert!((rise - want).abs() < 0.01, "rose {rise} m, want {want}");
        let a = w.actor(C1).unwrap().clone();
        assert!(a.velocity.y < -80.0, "coming down: {:?}", a.velocity);
        assert!(
            a.position.y < 0.0,
            "through the floor under it: {}",
            a.position
        );
        assert!(a.position.x.abs() < 1e-4, "never steered to the walk's aim");
        assert!(
            (a.position.z - 20.0 * FOOT).abs() < 0.01,
            "a second along at 20 ft/s: {}",
            a.position
        );
        // Its end: nothing written, set down at its landing.
        w.set_velocity(C1, Vec3::ZERO, &mut e).unwrap();
        assert!(!w.scene_walks[&C1].jump);
        let landing = Vec3::new(0.0, -1.0, 6.1);
        w.actors.get_mut(&C1).unwrap().position = landing;
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        let held = w.actor(C1).unwrap().position;
        assert!(held.distance(landing) < 1e-4, "held at its landing: {held}");
        // Again from the floor, its shape back (a box meeting the level) as
        // it comes down: on the floor, and there.
        let a = w.actors.get_mut(&C1).unwrap();
        a.position = Vec3::new(0.0, 0.0, 0.0);
        a.solid.interacts_with_level_mesh = 1;
        w.set_velocity(C1, Vec3::new(0.0, 40.0, 0.0), &mut e)
            .unwrap();
        for _ in 0..20 {
            w.advance(STEP, &mut e);
        }
        assert!(w.actor(C1).unwrap().position.y > 1.0, "up");
        w.actors.get_mut(&C1).unwrap().solid.shape = 1;
        // Given back over the floor, it falls on to it rather than being
        // set down on it at once (the review: the ground a ray meets under
        // it is a landing only once the step reaches it).
        w.advance(STEP, &mut e);
        let y = w.actor(C1).unwrap().position.y;
        assert!(y > 1.0, "still falling: {y}");
        for _ in 0..60 {
            w.advance(STEP, &mut e);
        }
        let a = w.actor(C1).unwrap();
        // On it as its body stands it (AS.79: a creature's skin over it).
        assert!(
            a.position.y >= 0.0 && a.position.y < 0.05,
            "on the floor: {}",
            a.position
        );
        assert_eq!(a.velocity, Vec3::ZERO);
        assert!(!w.scene_walks[&C1].jump, "the jump over");
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

    /// openspec AS.155: a hero held up by a rim alone has a gap under him,
    /// so the mover update's gravity acts, a foot at least with nothing
    /// under him and the drop itself over one past the walkable check's
    /// reach; on ground, under a rim or not, he reads none, whatever is
    /// under his pivot within that reach.
    #[test]
    fn a_hero_a_rim_holds_has_a_gap_under_him() {
        let mut e = pit();
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0);
        hero.settle(&e.world, &[]);
        e.hero = Some(hero);
        e.bodies.insert(HERO, BodyId(9));
        let mut w = world();
        let ground = w.objs.value(Object::Facet(HERO, Facet::Ground));
        let mut gap = |e: &mut Pit, supported: bool, height: Option<f32>| {
            let h = e.hero.as_mut().unwrap();
            h.footing.supported = supported;
            h.footing.height = height;
            h.footing.ground = height.map(|_| Vec3::Y);
            let v = w
                .get(
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
                .unwrap();
            v.as_f32().unwrap()
        };
        assert_eq!(gap(&mut e, true, Some(0.005)), 0.0, "on the ground");
        assert_eq!(
            gap(&mut e, true, Some(40.0)),
            0.0,
            "a ledge's drop in reach"
        );
        assert!(gap(&mut e, true, None) >= 1.0, "a rim over nothing");
        assert!(gap(&mut e, false, None) >= 1.0, "over nothing");
        let deep = guac_motion::edge::WALKABLE_RANGE + 1.0;
        assert!(gap(&mut e, true, Some(deep)) > 225.0, "a rim over the deep");
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
        // A relative tilt or roll on it is set outright and reads back as
        // set: AH.145's turn about its own axes is for an actor hung from
        // nothing, and one on a ground stays as it was.
        for (name, m, x) in [
            ("relative tilt", Member::RelativeTilt, 12.5f32),
            ("relative roll", Member::RelativeRoll, -7.0),
        ] {
            let at = Access {
                member: MemberId::named("orientationmeasurement", name),
                ..at
            };
            w.set_member(&at, m, Value::Float(x), &mut e).unwrap();
            w.set_member(&at, m, Value::Float(x), &mut e).unwrap();
            assert_eq!(w.get_member(&at, m, &mut e), Ok(Value::Float(x)), "{name}");
        }
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

    /// The type of the ground under an awake actor with none found reads 0,
    /// not -1 (openspec AG.28: the camera's target, a clone of
    /// `Generic_FollowActor`, over the edge of the level's mesh; the
    /// au-chests review: a chest's treasure in flight past a ledge's rim, a
    /// solid of its own); asleep or taken off, -1 stays.
    #[test]
    fn an_awake_actor_with_no_ground_found_reads_a_type_of_0() {
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
        // A hero over nothing, his ground found by nothing (openspec AS.146):
        // under 0 the mover update takes him off its movers.
        let mut hero = Hero::new(
            guac_motion::tuning::eruptor_solid(),
            guac_motion::tuning::Tuning::ERUPTOR,
            Vec3::new(0.0, 500.0, 0.0),
            0.0,
        );
        hero.footing.ground = None;
        e.hero = Some(hero);
        w.actors.insert(HERO, Actor::blank());
        assert_eq!(type_under(&mut w, &mut e, HERO), Value::Int(0));
        // How far over his ground he reads: how far he's fallen, a foot at
        // least so gravity acts, on a lip over nothing as anywhere (openspec
        // AS.155), 0 with ground under him, and past the safe point trail's
        // 5 ft either way.
        let ground = get(&mut w, &mut e, "abstractplacement", "ground", HERO, HERO);
        let over = |w: &mut ScriptWorld, e: &mut Pit, m: &str| {
            let a = Access {
                placement: HERO,
                receiver: Some(Value::Entity(HERO)),
                member: MemberId::named("positionmeasurement", m),
                other_point: Some(ground),
                via: None,
                op: None,
            };
            w.get(&a, e).unwrap().as_f32().unwrap()
        };
        // Hung on a rig or held by a scene his type stands as it was: level
        // 201's pod and the walk off it.
        e.hero.as_mut().unwrap().hung = true;
        assert_eq!(type_under(&mut w, &mut e, HERO), Value::Int(-1));
        e.hero.as_mut().unwrap().hung = false;
        w.in_scene = true;
        assert_eq!(type_under(&mut w, &mut e, HERO), Value::Int(-1));
        w.in_scene = false;
        assert_eq!(over(&mut w, &mut e, "separation"), 1.0);
        assert!(over(&mut w, &mut e, "vertical offset") > 5.0);
        e.hero.as_mut().unwrap().footing.supported = true;
        assert_eq!(over(&mut w, &mut e, "separation"), 1.0);
        let hero = e.hero.as_mut().unwrap();
        hero.footing.height = Some(0.01);
        hero.footing.ground = Some(Vec3::Y);
        assert_eq!(over(&mut w, &mut e, "separation"), 0.0);
        e.hero = None;
        // With a solid, far over the floor: the same.
        let mut solid = chompy(Vec3::new(0.0, 500.0, 0.0), 0.0);
        solid.active = true;
        w.actors.insert(C1, solid);
        assert_eq!(type_under(&mut w, &mut e, C1), Value::Int(0));
        // Asleep it has no corpus, and taken off none either.
        let mut asleep = chompy(Vec3::new(0.0, 500.0, 0.0), 0.0);
        asleep.active = false;
        w.actors.insert(C1, asleep);
        assert_eq!(type_under(&mut w, &mut e, C1), Value::Int(-1));
        let mut off = Actor::blank();
        off.position = Vec3::new(0.0, 500.0, 0.0);
        off.removed = true;
        w.actors.insert(C2, off);
        assert_eq!(type_under(&mut w, &mut e, C2), Value::Int(-1));
    }

    /// The au-chests review: a static prop's attach nodes stand where its
    /// model's list has them, turned by its facing and at its scale, and a
    /// clone's by its template's model: a hat box's lid hangs on its `Lid`
    /// node, not at its feet. One posed goes by its pose, one with no list
    /// stands at its pivot.
    #[test]
    fn a_static_props_attach_nodes_stand_on_its_models_list() {
        let mut e = pit();
        let mut w = world();
        const BOX: Obj = Obj { unit: 3, index: 7 };
        let mut template =
            crate::scripts::Placed::named(BOX, "HatBox_Pickup", Some("C:/x/Hat_Pickup.ai"), 5);
        template.model = Some("box.mdl".into());
        w.templates.insert(BOX, template);
        w.set_origin_nodes(HashMap::from([(
            "box.mdl".to_owned(),
            vec![NodePose::at(Vec3::new(0.0, 1.8, 0.0))],
        )]));
        let mut clone = Actor::blank();
        clone.position = Vec3::new(2.0, 0.0, 0.0);
        clone.template = Some(BOX);
        clone.scale = [50.0; 3];
        w.actors.insert(C1, clone);
        let at = w.node(C1, 0, &mut e).unwrap();
        assert!(at.distance(Vec3::new(2.0, 0.9, 0.0)) < 1e-5, "{at}");
        assert_eq!(w.node(C1, 1, &mut e), Some(Vec3::new(2.0, 0.0, 0.0)));
        w.set_nodes(C1, vec![NodePose::at(Vec3::new(0.0, 0.1, 0.0))]);
        let at = w.node(C1, 0, &mut e).unwrap();
        assert!(at.distance(Vec3::new(2.0, 0.1, 0.0)) < 1e-5, "{at}");
    }

    /// openspec AS.152: a surface's type is its material byte's top six
    /// bits, so the disc's elemental grounds read 40 plus their element and
    /// stay under the 50 a walk stops at, its plain ground's bytes 0 and 1
    /// read 0, and only its top surfaces (252 and 253, type 63) stop him.
    #[test]
    fn a_surfaces_type_is_its_materials_top_six_bits() {
        for (material, surface) in [
            (0u8, 0),
            (1, 0),
            (40, 10),
            (120, 30),
            (164, 41),
            (168, 42),
            (172, 43),
            (180, 45),
            (188, 47),
            (252, 63),
            (253, 63),
        ] {
            let mesh = Mesh::new(
                vec![
                    Vec3::new(-10.0, 0.0, -10.0),
                    Vec3::new(10.0, 0.0, -10.0),
                    Vec3::new(0.0, 0.0, 10.0),
                ],
                vec![[0, 2, 1]],
                vec![material],
            )
            .unwrap();
            let ground = Body {
                id: BodyId(22),
                kind: BodyKind::Static,
                pose: Pose::at(Vec3::Y),
                collider: Collider::Mesh(std::sync::Arc::new(mesh)),
            };
            let mut e = pit();
            e.world = ParryWorld::new(&[ground]).unwrap();
            let mut w = world();
            let mut over = Actor::blank();
            over.position = Vec3::new(0.0, 2.0, 0.0);
            w.actors.insert(C2, over);
            let ground = get(&mut w, &mut e, "abstractplacement", "ground", C2, C2);
            let read = w.get(
                &Access {
                    placement: C2,
                    receiver: Some(ground),
                    member: MemberId::named("contactinfo", "type"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                &mut e,
            );
            assert_eq!(read.unwrap(), Value::Int(surface), "byte {material}");
        }
    }

    /// AS.97: a creature taken off has no ground, standing over the floor
    /// or not, as one asleep has none: Setup_004B's waves count the clones
    /// left standing by their ground's type.
    #[test]
    fn a_creature_taken_off_has_no_ground() {
        let mut e = pit();
        let mut w = world();
        let mut stands = chompy(Vec3::new(0.0, 1.0, 0.0), 0.0);
        stands.active = true;
        w.actors.insert(C1, stands);
        assert!(w.ground(C1, &mut e).is_some(), "on the floor");
        w.actors.get_mut(&C1).unwrap().removed = true;
        assert!(w.ground(C1, &mut e).is_none(), "off, none");
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

    /// AS.37: a clip's copies, as `clonedsequence.clones` reads them off
    /// the clip: one on each actor its starts played it on, in the order
    /// started, read off a copy too; the one that ends leaves the set, a
    /// stopped one with it; none of another clip's.
    #[test]
    fn a_clips_copies_read_while_they_play_and_go_once_they_end() {
        let mut e = pit();
        let mut w = world();
        let template = Obj { unit: 4, index: 77 };
        w.set_palette(template, vec![0.5, 1.0]);
        for (h, x) in [(C1, 0.0), (C2, 1.0), (C3, 2.0)] {
            let mut c = chompy(Vec3::new(x, 0.0, 0.0), 0.0);
            c.obj = Some(template);
            w.actors.insert(h, c);
        }
        let short = w.objs.value(Object::Clip { template, slot: 0 });
        let long = w.objs.value(Object::Clip { template, slot: 1 });
        let start = |w: &mut ScriptWorld, e: &mut Pit, on: Handle, clip: Value| {
            let call = op(Class::StartSequence, on, vec![("_LHS", Some(clip))], vec![]);
            match ask(w, e, &call) {
                Answer::Run { result, .. } => result.unwrap(),
                other => panic!("{other:?}"),
            }
        };
        let clones = |w: &mut ScriptWorld, e: &mut Pit, of: Value| {
            let at = Access {
                placement: 1,
                receiver: Some(of),
                member: MemberId::named("clonedsequence", "clones"),
                other_point: None,
                via: None,
                op: None,
            };
            w.elements(&at, e).unwrap()
        };
        let on_c1 = start(&mut w, &mut e, C1, short);
        let on_c2 = start(&mut w, &mut e, C2, long);
        let on_c3 = start(&mut w, &mut e, C3, short);
        assert_eq!(clones(&mut w, &mut e, short), vec![on_c1, on_c3]);
        assert_eq!(clones(&mut w, &mut e, on_c3), vec![on_c1, on_c3]);
        assert_eq!(clones(&mut w, &mut e, long), vec![on_c2]);
        // Half a second on, the short clip's copies have played out.
        for _ in 0..30 {
            w.advance(STEP, &mut e);
        }
        assert_eq!(clones(&mut w, &mut e, short), Vec::<Value>::new());
        assert_eq!(clones(&mut w, &mut e, long), vec![on_c2]);
        let stop = op(Class::StopSequence, C2, vec![("_LHS", Some(on_c2))], vec![]);
        ask(&mut w, &mut e, &stop);
        assert_eq!(clones(&mut w, &mut e, long), Vec::<Value>::new());
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
    /// as its file holds it (AS.52) until a script turns it; one with no
    /// turn read reads nothing; an object no waypoint is has no turn.
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
        let tilt = member("orientationmeasurement", "tilt", turn, None);
        assert_eq!(w.get(&facing, e), Err(HostFault::Nothing), "no turn read");
        // Its file's turn, read with the files (AS.52: the outro's camera
        // looks from its waypoint as the waypoint faces), until a script's.
        w.point_turns.insert(
            wp,
            Angles {
                tilt: -12.5,
                facing: 196.0,
                roll: 0.0,
            },
        );
        assert_eq!(w.get(&facing, e), Ok(Value::Float(196.0)));
        assert_eq!(w.get(&tilt, e), Ok(Value::Float(-12.5)));
        w.set(&facing, Value::Float(90.0), e).unwrap();
        assert_eq!(w.get(&facing, e), Ok(Value::Float(90.0)));
        assert_eq!(
            w.get(&tilt, e),
            Ok(Value::Float(-12.5)),
            "the rest as it was"
        );
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

    use super::fight_tests::{C1, STEP, WP, chompy, pit, world};
    use super::*;

    const TEMPLATE: Obj = Obj { unit: 4, index: 77 };

    /// AH.282: `move to ... with anim` plays the palette's clip at its index
    /// on the mover while he moves, as `start sequence` would, and its
    /// reissue each tick (the script's loop) finds that copy playing and
    /// starts nothing new; a move with no clip starts none.
    #[test]
    fn a_move_with_anim_plays_its_clip_once_while_he_moves() {
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        let call = |index: Option<i32>| OpCall {
            placement: C1,
            receiver: Value::Entity(C1),
            class: Class::MoveTo,
            operands: vec![
                ("_indexRHS", index.map(Value::Int)),
                ("_LHS", Some(Value::Object(WP))),
                ("_RHS", Some(Value::Int(1))),
                ("_NP", Some(Value::Null)),
            ],
            words: vec![("_dir", 0), ("_relOperator", 0)],
            sets: vec![("_NP", clips.clone())],
        };
        let playing = |w: &ScriptWorld| -> Vec<(Value, f32)> {
            w.sequences()
                .iter()
                .filter(|s| s.actor == C1 && s.plays())
                .map(|s| (s.template, s.time))
                .collect()
        };
        assert_eq!(w.op(&call(None), &mut |_| 0, &mut e).unwrap(), Answer::Done);
        assert!(playing(&w).is_empty(), "no index, no clip");
        assert_eq!(
            w.op(&call(Some(2)), &mut |_| 0, &mut e).unwrap(),
            Answer::Done
        );
        assert_eq!(playing(&w), [(clips[2], 0.0)], "slot 2 on the mover");
        let started = w.sequences().len();
        for _ in 0..3 {
            w.advance(STEP, &mut e);
            assert_eq!(
                w.op(&call(Some(2)), &mut |_| 0, &mut e).unwrap(),
                Answer::Done
            );
        }
        assert_eq!(w.sequences().len(), started, "the reissue starts no copy");
        let now = playing(&w);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].0, clips[2]);
        assert!(now[0].1 > 2.5 * STEP, "never started again: {now:?}");
    }

    /// AS.52, AS.97: the static palette is the running placement's; one
    /// with none of its own reads the actor its tell's block tells (a
    /// cutscene's game systems' placement starting an animation on each of
    /// its clones; the enemy system's queue a jump's clips on its enemy),
    /// never its own again, and nothing outside a tell; a file's placement
    /// the game systems tell reads nothing yet (AS.80).
    #[test]
    fn a_static_palette_under_a_tell_is_the_told_actors_with_none_of_its_own() {
        const SYSTEMS: Handle = 0x6100_0040;
        const FILES: Handle = 0x6100_0041;
        const ENEMIES: Handle = 0x6100_0042;
        const SYSTEMS_OBJ: Obj = Obj {
            unit: 1,
            index: 901,
        };
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        // C1 a clone of the template; FILES a placement of its own file.
        let mut clone = w.actors.remove(&C1).unwrap();
        clone.template = clone.obj.take();
        w.actors.insert(C1, clone);
        let mut placed = chompy(Vec3::ZERO, 0.0);
        placed.obj = Some(TEMPLATE);
        w.actors.insert(FILES, placed);
        let mut systems = crate::script_engine::Actor::blank();
        systems.obj = Some(SYSTEMS_OBJ);
        w.actors.insert(SYSTEMS, systems);
        w.templates.insert(
            SYSTEMS_OBJ,
            crate::scripts::Placed::named(
                SYSTEMS_OBJ,
                "Macros_GameSystems",
                Some("C:/tfb/Content/Global/GameSystems.ai"),
                4,
            ),
        );
        w.actors
            .insert(ENEMIES, crate::script_engine::Actor::blank());
        let static_palette = Access {
            placement: SYSTEMS,
            receiver: None,
            member: MemberId::named("animationinfo", "animation palette"),
            other_point: None,
            via: None,
            op: None,
        };
        w.told = Value::Entity(SYSTEMS);
        assert_eq!(
            w.elements(&static_palette, &mut e),
            Err(HostFault::Nothing),
            "its own, which it hasn't"
        );
        w.told = Value::Entity(C1);
        assert_eq!(w.elements(&static_palette, &mut e).unwrap(), clips);
        w.told = Value::Entity(FILES);
        assert_eq!(
            w.elements(&static_palette, &mut e),
            Err(HostFault::Nothing),
            "a file's placement the game systems tell: nothing yet"
        );
        let enemies = Access {
            placement: ENEMIES,
            ..static_palette
        };
        assert_eq!(
            w.elements(&enemies, &mut e).unwrap(),
            clips,
            "a file's placement anything else tells: its own"
        );
        w.told = Value::Entity(ENEMIES);
        assert_eq!(
            w.elements(&enemies, &mut e),
            Err(HostFault::Nothing),
            "telling itself is no tell"
        );
        // One with a palette of its own reads its own told no one with one.
        let mine = Access {
            placement: C1,
            ..static_palette
        };
        w.told = Value::Entity(SYSTEMS);
        assert_eq!(w.elements(&mine, &mut e).unwrap(), clips);
    }

    /// AU.37: inside a `control` the static palette is the actor told, a
    /// running placement's own palette or none: Pop Fizz's script reads his
    /// beast's clips under a `control` of it, a Chompy's its target's.
    #[test]
    fn a_palette_read_under_a_control_reads_the_told_actors_first() {
        const BEAST: Obj = Obj {
            unit: 5,
            index: 900,
        };
        const CLONE: Handle = 0x77;
        let (mut w, mut e) = (world(), pit());
        let own = palette(&mut w);
        w.set_palette(BEAST, vec![2.0, 0.0, 3.0]);
        let mut beast = chompy(Vec3::ZERO, 0.0);
        beast.obj = None;
        beast.template = Some(BEAST);
        w.actors.insert(CLONE, beast);
        let read = Access {
            placement: C1,
            receiver: None,
            member: MemberId::named("animationinfo", "animation palette"),
            other_point: None,
            via: None,
            op: None,
        };
        w.told = Value::Entity(C1);
        assert_eq!(w.elements(&read, &mut e).unwrap(), own, "his own, untold");
        w.told = Value::Entity(CLONE);
        let told = w.elements(&read, &mut e).unwrap();
        assert_eq!(told.len(), 3, "the beast's slots");
        assert_ne!(told[0], own[0]);
        // Its clip lasts its own length, not the running placement's.
        let length = |w: &mut ScriptWorld, e: &mut dyn Engine, v: Value| {
            w.get(
                &Access {
                    placement: C1,
                    receiver: Some(v),
                    member: MemberId::named("sequence", "raw duration"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                e,
            )
            .unwrap()
        };
        assert_eq!(length(&mut w, &mut e, told[2]), Value::Float(3.0));
    }

    /// AU.37: `Movement - Add Mover` leaves the engine a top speed of 10,000
    /// and his walk is the scripts'; off the movers the engine walks him on
    /// his own numbers, the stick its input while a pad is his controller,
    /// and with the inactive controller he slows to a stop.
    #[test]
    fn a_hero_off_the_movers_walks_on_the_engines_model() {
        const HERO: Handle = 0x77;
        const STEP: f32 = 1.0 / 60.0;
        let (mut w, e) = (world(), pit());
        let pad = Value::Entity(w.controllers[0]);
        let mut hero = chompy(Vec3::ZERO, 0.0);
        hero.physics.ground_max_speed = 10_000.0;
        hero.physics.ground_acceleration = 0.0;
        hero.controller = Value::Entity(9);
        w.actors.insert(HERO, hero);
        let push = Push {
            strength: 0.8,
            heading: 45.0,
        };
        assert_eq!(w.engine_walk(HERO, push), None, "a mover");
        assert_eq!(w.engine_walk(404, push), None, "no such actor");
        let a = w.actors.get_mut(&HERO).unwrap();
        a.physics.ground_max_speed = 25.0;
        a.physics.ground_acceleration = 160.0;
        a.physics.ground_deceleration = 80.0;
        let (physics, aim) = w.engine_walk(HERO, push).unwrap();
        assert_eq!(physics.ground_max_speed, 25.0);
        assert_eq!(aim, Aim::Still, "the inactive controller");
        w.actors.get_mut(&HERO).unwrap().controller = pad;
        let along = Aim::Along {
            heading: 45.0,
            strength: 0.8,
        };
        assert_eq!(w.engine_walk(HERO, push).map(|x| x.1), Some(along));
        assert_eq!(
            w.engine_walk(HERO, Push::NONE).map(|x| x.1),
            Some(Aim::Still),
            "the stick let go"
        );
        // A 50 ft/s dash left on him: the model slows it; a mover's stays.
        let solid = guac_motion::tuning::eruptor_solid();
        let mut body = Hero::actor(solid, Vec3::ZERO, 0.0);
        body.settle(&e.world, &[]);
        body.walk.velocity = Vec3::new(0.0, 0.0, 50.0);
        for _ in 0..60 {
            w.step_scripted(
                HERO,
                &mut body,
                &e.world,
                Push::NONE,
                STEP,
                &[],
                Surfaces::NONE,
            );
        }
        assert!(
            body.walk.velocity.length() < 1.0,
            "{:?}",
            body.walk.velocity
        );
        w.actors.get_mut(&HERO).unwrap().physics.ground_max_speed = 10_000.0;
        body.walk.velocity = Vec3::new(0.0, 0.0, 5.0);
        w.step_scripted(
            HERO,
            &mut body,
            &e.world,
            Push::NONE,
            STEP,
            &[],
            Surfaces::NONE,
        );
        assert_eq!(body.walk.velocity.z, 5.0, "the scripts' walk is left alone");
        // Hung on something the engine leaves him be, off the movers or on
        // them: his rig has him, his walk kept for his letting go (the
        // au-pop-fizz review).
        let a = w.actors.get_mut(&HERO).unwrap();
        a.physics.ground_max_speed = 25.0;
        a.controller = pad;
        body.hung = true;
        body.walk.velocity = Vec3::new(0.0, 0.0, 5.0);
        body.facing = 10.0;
        w.step_scripted(HERO, &mut body, &e.world, push, STEP, &[], Surfaces::NONE);
        assert_eq!(body.walk.velocity.z, 5.0, "hung");
        assert_eq!(body.facing, 10.0, "hung, the stick turning nothing");
    }

    /// AS.78: a palette's clip read before any copy of it plays has its
    /// length as its raw duration (an animator camera's shot lasts what
    /// `Cutscene Camera - Set Camera - Animator Camera` reads off its palette
    /// entry); a slot the extract refused reads 0, a sequence its own.
    #[test]
    fn a_palette_clips_raw_duration_is_its_length() {
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        w.set_palette(TEMPLATE, vec![1.0, 0.5, 0.0, 10.0]);
        let raw = |w: &mut ScriptWorld, e: &mut dyn Engine, v: Value| {
            w.get(
                &Access {
                    placement: C1,
                    receiver: Some(v),
                    member: MemberId::named("sequence", "raw duration"),
                    other_point: None,
                    via: None,
                    op: None,
                },
                e,
            )
        };
        for (clip, length) in clips.iter().zip([1.0, 0.5, 0.0, 10.0]) {
            assert_eq!(raw(&mut w, &mut e, *clip), Ok(Value::Float(length)));
        }
        // A copy playing: the sequence's own length, as before.
        let Answer::Run {
            result: Some(s), ..
        } = w.play_on(clips[3], C1)
        else {
            panic!("a copy");
        };
        assert_eq!(raw(&mut w, &mut e, s), Ok(Value::Float(10.0)));
    }

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

    /// openspec AU.53: a life mage's death controller sets the death pose
    /// it plays to ping pong. The host keeps it on the sequence (read back,
    /// played out and back), the TV's config routing the member to it; a
    /// clip itself takes none.
    #[test]
    fn a_sequence_takes_its_playback_mode() {
        let (mut w, mut e) = (world(), pit());
        let clips = palette(&mut w);
        let pose = play(&mut w, &mut e, clips[2]);
        assert_eq!(
            w.get(&on(pose, "sequence", "playback mode"), &mut e)
                .unwrap(),
            Value::Int(0)
        );
        w.set(
            &on(pose, "sequence", "playback mode"),
            Value::Int(1),
            &mut e,
        )
        .unwrap();
        assert!(w.sequences().last().unwrap().ping_pong);
        assert_eq!(
            w.get(&on(pose, "sequence", "playback mode"), &mut e)
                .unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            w.set(
                &on(clips[2], "sequence", "playback mode"),
                Value::Int(1),
                &mut e
            ),
            Err(HostFault::Nothing)
        );
        let id = guac_script::MemberId::named("sequence", "playback mode");
        let c = crate::scripts::hero_config(guac_script::Mode::Play);
        assert!(
            c.members
                .get(id)
                .is_some_and(|(_, p)| p == guac_script::Place::Host),
            "the host's"
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
