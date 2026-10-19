//! A level's placements (openspec AG.3): everything in a level that isn't
//! scenery or collision is a `Placement` (ag-levels): `tfbActorInfo` (props,
//! creatures, triggers, cameras, spawn points), `tfbParticleInfo` (emitters)
//! and `tfbLightInfo` (point lights). An actor's solid is a `CollisionInfo`
//! primitive, its movement numbers an `ActorParameters`, its path an
//! `ActorWaypointList`.
//!
//! Units are the data's feet and degrees (ag-motion: the engine works in
//! metres and converts). Bit numbering in `CollisionInfo._properties` and
//! its shape codes are from the EBOOT's field function and its Havok shape
//! builder, checked live (ag-motion section 1); `Placement._properties`'
//! bitfields aren't pinned, so they stay raw.

use super::script::{EulerTransform, Rgba8};
use super::{AbstractPlacement, DataList, Layout, ObjectRef, View};
use crate::FormatError;

/// `Placement`: size 0xD8, an `AbstractPlacement` with a script, a scale
/// and a solid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement<'a> {
    pub placement: AbstractPlacement<'a>,
    /// Storage for the shadow, alpha, bloom, fog and draw-order bitfields.
    pub properties: u32,
    pub major_index: i32,
    /// A `tfbScriptInfo`, whose name is the `.ai` source path.
    pub script: Option<usize>,
    pub exec_behavior: ObjectRef<'a>,
    pub local_var_list: ObjectRef<'a>,
    pub active_time: f32,
    /// Per axis in percent: 100 is unit scale. It scales the solid too.
    pub scale: [f32; 3],
    /// A `CollisionInfo`.
    pub collision: Option<usize>,
    pub tint: Rgba8,
    pub replace_color: Rgba8,
    pub activation_range: f32,
    pub activation_range_buffer: f32,
}

impl<'a> Placement<'a> {
    pub const PROPERTIES: u32 = 0xA0;
    pub const MAJOR_INDEX: u32 = 0xA4;
    pub const SCRIPT: u32 = 0xA8;
    pub const EXEC_BEHAVIOR: u32 = 0xAC;
    pub const LOCAL_VAR_LIST: u32 = 0xB0;
    pub const ACTIVE_TIME: u32 = 0xB4;
    pub const SCALE: u32 = 0xB8;
    pub const COLLISION: u32 = 0xC4;
    pub const TINT: u32 = 0xC8;
    pub const REPLACE_COLOR: u32 = 0xCC;
    pub const ACTIVATION_RANGE: u32 = 0xD0;
    pub const ACTIVATION_RANGE_BUFFER: u32 = 0xD4;
}

impl<'a> Layout<'a> for Placement<'a> {
    const TYPE: &'static str = "Placement";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            placement: AbstractPlacement::fields(v)?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            major_index: v.i32(Self::MAJOR_INDEX, "_majorIndex")?,
            script: v.object(Self::SCRIPT, "_script", "tfbScriptInfo")?,
            exec_behavior: v.object_ref(Self::EXEC_BEHAVIOR, "_execBehavior")?,
            local_var_list: v.object_ref(Self::LOCAL_VAR_LIST, "_localVarList")?,
            active_time: v.f32(Self::ACTIVE_TIME, "_activeTime")?,
            scale: v.floats(Self::SCALE, "_scaleVec")?,
            collision: v.object(Self::COLLISION, "_collision", CollisionInfo::TYPE)?,
            tint: v.u32(Self::TINT, "_tint")?.to_be_bytes(),
            replace_color: v.u32(Self::REPLACE_COLOR, "_replaceColor")?.to_be_bytes(),
            activation_range: v.f32(Self::ACTIVATION_RANGE, "_activationRange")?,
            activation_range_buffer: v
                .f32(Self::ACTIVATION_RANGE_BUFFER, "_activationRangeBuffer")?,
        })
    }
}

/// `tfbActorInfo`: size 0xF8, an actor: a model, its movement numbers and
/// its waypoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorInfo<'a> {
    pub placement: Placement<'a>,
    pub controller: ObjectRef<'a>,
    /// A `tfbModelInfo` (a `tfbPhysicsModel` on the disc), shared between
    /// placements: level 213's 613 model-bearing actors use 222.
    pub model: Option<usize>,
    /// An `ActorParameters`, over the model's defaults.
    pub actor_parameters: Option<usize>,
    /// An `ActorWaypointList`.
    pub waypoints: Option<usize>,
    pub destination: [f32; 3],
    pub game_actor: ObjectRef<'a>,
}

impl ActorInfo<'_> {
    pub const CONTROLLER: u32 = 0xD8;
    pub const MODEL: u32 = 0xDC;
    pub const ACTOR_PARAMETERS: u32 = 0xE0;
    pub const WAYPOINTS: u32 = 0xE4;
    pub const DESTINATION: u32 = 0xE8;
    pub const GAME_ACTOR: u32 = 0xF4;
}

impl<'a> Layout<'a> for ActorInfo<'a> {
    const TYPE: &'static str = "tfbActorInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            placement: Placement::fields(v)?,
            controller: v.object_ref(Self::CONTROLLER, "_controller")?,
            model: v.object(Self::MODEL, "_model", "tfbModelInfo")?,
            actor_parameters: v.object(
                Self::ACTOR_PARAMETERS,
                "_actorParameters",
                ActorParameters::TYPE,
            )?,
            waypoints: v.object(Self::WAYPOINTS, "_wayPoints", ActorWaypointList::TYPE)?,
            destination: v.floats(Self::DESTINATION, "_destination")?,
            game_actor: v.object_ref(Self::GAME_ACTOR, "_gameActor")?,
        })
    }
}

/// `tfbParticleInfo`: size 0xE0, an emitter. Its generators stay
/// unfollowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleInfo<'a> {
    pub placement: Placement<'a>,
    /// A `GeneratorInfoList`.
    pub generator_info_list: ObjectRef<'a>,
    pub game_particle_system: ObjectRef<'a>,
}

impl ParticleInfo<'_> {
    pub const GENERATOR_INFO_LIST: u32 = 0xD8;
    pub const GAME_PARTICLE_SYSTEM: u32 = 0xDC;
}

impl<'a> Layout<'a> for ParticleInfo<'a> {
    const TYPE: &'static str = "tfbParticleInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            placement: Placement::fields(v)?,
            generator_info_list: v.object_ref(Self::GENERATOR_INFO_LIST, "_generatorInfoList")?,
            game_particle_system: v
                .object_ref(Self::GAME_PARTICLE_SYSTEM, "_gameParticleSystem")?,
        })
    }
}

/// `tfbLightInfo`: size 0x10C, a placed light, beside the level rig's
/// `DirectionalLightInfo`s. Level 213 has one, `213_EvilRobot_Beam_light`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightInfo<'a> {
    pub placement: Placement<'a>,
    /// An enum, kept raw: 1 on level 213's point light.
    pub light_type: u32,
    pub visible: bool,
    pub falloff_rate: f32,
    pub cutoff_angle: f32,
    pub constant_attenuation: f32,
    pub linear_attenuation: f32,
    pub quadratic_attenuation: f32,
    pub diffuse: Rgba8,
    pub ambient: Rgba8,
    pub specular: Rgba8,
    pub multiplier: f32,
    pub light_attr: ObjectRef<'a>,
    pub light_state_attr: ObjectRef<'a>,
}

impl LightInfo<'_> {
    pub const LIGHT_TYPE: u32 = 0xD8;
    pub const VISIBLE: u32 = 0xDC;
    pub const FALLOFF_RATE: u32 = 0xE0;
    pub const CUTOFF_ANGLE: u32 = 0xE4;
    pub const CONSTANT_ATTENUATION: u32 = 0xE8;
    pub const LINEAR_ATTENUATION: u32 = 0xEC;
    pub const QUADRATIC_ATTENUATION: u32 = 0xF0;
    pub const DIFFUSE: u32 = 0xF4;
    pub const AMBIENT: u32 = 0xF8;
    pub const SPECULAR: u32 = 0xFC;
    pub const MULTIPLIER: u32 = 0x100;
    pub const LIGHT_ATTR: u32 = 0x104;
    pub const LIGHT_STATE_ATTR: u32 = 0x108;
}

impl<'a> Layout<'a> for LightInfo<'a> {
    const TYPE: &'static str = "tfbLightInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            placement: Placement::fields(v)?,
            light_type: v.u32(Self::LIGHT_TYPE, "_lightType")?,
            visible: v.bool(Self::VISIBLE, "_visible")?,
            falloff_rate: v.f32(Self::FALLOFF_RATE, "_falloffRate")?,
            cutoff_angle: v.f32(Self::CUTOFF_ANGLE, "_cutoffAngle")?,
            constant_attenuation: v.f32(Self::CONSTANT_ATTENUATION, "_constantAttenuationCoeff")?,
            linear_attenuation: v.f32(Self::LINEAR_ATTENUATION, "_linearAttenuationCoeff")?,
            quadratic_attenuation: v
                .f32(Self::QUADRATIC_ATTENUATION, "_quadraticAttenuationCoeff")?,
            diffuse: v.u32(Self::DIFFUSE, "_diffuseColor")?.to_be_bytes(),
            ambient: v.u32(Self::AMBIENT, "_ambientColor")?.to_be_bytes(),
            specular: v.u32(Self::SPECULAR, "_specularColor")?.to_be_bytes(),
            multiplier: v.f32(Self::MULTIPLIER, "_multiplier")?,
            light_attr: v.object_ref(Self::LIGHT_ATTR, "_lightAttr")?,
            light_state_attr: v.object_ref(Self::LIGHT_STATE_ATTR, "_lightStateAttr")?,
        })
    }
}

/// `CollisionInfo._shapeType`, in the order the EBOOT lists the names.
/// Sizes are full extents in feet, times the placement's scale, with the
/// pivot at the bottom centre (ag-motion, live): a box is width x height x
/// length, a capsule's and a cylinder's radius is half the width and their
/// height the height, a sphere's diameter is the width. `None` builds a
/// shared 1 m sphere and `Mesh` is built as `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    None,
    Box,
    Capsule,
    Sphere,
    Cylinder,
    Mesh,
}

impl Shape {
    pub const ALL: [Shape; 6] = [
        Shape::None,
        Shape::Box,
        Shape::Capsule,
        Shape::Sphere,
        Shape::Cylinder,
        Shape::Mesh,
    ];
}

/// `CollisionInfo`: size 0x58, a placement's or a model's solid: a
/// primitive and how it collides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionInfo<'a> {
    pub name: Option<&'a str>,
    /// Shape in bits 0-2, then solid, interacts-with-world and
    /// actor-surface; no higher bit is set on the disc, so one is refused.
    pub properties: u32,
    /// The primitive's offset from its placement.
    pub transform: EulerTransform,
    pub width: f32,
    pub height: f32,
    pub length: f32,
    /// x 0.4545 is kilograms (ag-motion).
    pub weight: f32,
    /// Percent.
    pub rebound: f32,
    pub object_owner: ObjectRef<'a>,
    pub object_contacts: ObjectRef<'a>,
    pub environment_contacts: ObjectRef<'a>,
    pub object_collisions: ObjectRef<'a>,
}

impl CollisionInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const PROPERTIES: u32 = 0x18;
    pub const TRANSFORM: u32 = 0x1C;
    pub const WIDTH: u32 = 0x34;
    pub const HEIGHT: u32 = 0x38;
    pub const LENGTH: u32 = 0x3C;
    pub const WEIGHT: u32 = 0x40;
    pub const REBOUND: u32 = 0x44;
    pub const OBJECT_OWNER: u32 = 0x48;
    pub const OBJECT_CONTACTS: u32 = 0x4C;
    pub const ENVIRONMENT_CONTACTS: u32 = 0x50;
    pub const OBJECT_COLLISIONS: u32 = 0x54;
    /// The bits the disc sets: shape (0-2), solid, world, actor surface.
    pub const KNOWN_BITS: u32 = 0x3F;

    pub fn shape(&self) -> Shape {
        // Read refuses codes 6 and 7.
        Shape::ALL[(self.properties & 7) as usize % Shape::ALL.len()]
    }

    /// Set on a solid prop, clear on a trigger volume (design decision 10).
    pub fn is_solid(&self) -> bool {
        self.properties & (1 << 3) != 0
    }

    pub fn interacts_with_world(&self) -> bool {
        self.properties & (1 << 4) != 0
    }

    pub fn is_actor_surface(&self) -> bool {
        self.properties & (1 << 5) != 0
    }
}

impl<'a> Layout<'a> for CollisionInfo<'a> {
    const TYPE: &'static str = "CollisionInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let properties = v.u32(Self::PROPERTIES, "_properties")?;
        if properties & 7 >= Shape::ALL.len() as u32 {
            return Err(v.refused(Self::PROPERTIES, "_shapeType"));
        }
        if properties & !Self::KNOWN_BITS != 0 {
            return Err(v.refused(Self::PROPERTIES, "_properties"));
        }
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            properties,
            transform: EulerTransform::read(v, Self::TRANSFORM, "_transform")?,
            width: v.f32(Self::WIDTH, "_width")?,
            height: v.f32(Self::HEIGHT, "_height")?,
            length: v.f32(Self::LENGTH, "_length")?,
            weight: v.f32(Self::WEIGHT, "_weight")?,
            rebound: v.f32(Self::REBOUND, "_rebound")?,
            object_owner: v.object_ref(Self::OBJECT_OWNER, "_objectOwner")?,
            object_contacts: v.object_ref(Self::OBJECT_CONTACTS, "_objectContacts")?,
            environment_contacts: v
                .object_ref(Self::ENVIRONMENT_CONTACTS, "_environmentContacts")?,
            object_collisions: v.object_ref(Self::OBJECT_COLLISIONS, "_objectCollisions")?,
        })
    }
}

/// `ModelParameters`: size 0x48, through `ModelPhysics`: how a model moves,
/// in the data's feet and degrees. A model's are its defaults; an actor's
/// `ActorParameters` replace them, and scripts change them at runtime
/// (ag-levels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelParameters<'a> {
    pub object_owner: ObjectRef<'a>,
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
    /// An enum, kept raw.
    pub motion_type: u32,
    pub tilts_with_ground: bool,
}

impl<'a> ModelParameters<'a> {
    pub const OBJECT_OWNER: u32 = 0x08;
    pub const GROUND_MAX_SPEED: u32 = 0x0C;
    pub const GROUND_ACCELERATION: u32 = 0x10;
    pub const GROUND_DECELERATION: u32 = 0x14;
    pub const GROUND_TURN_RATE: u32 = 0x18;
    pub const AIR_MAX_SPEED: u32 = 0x1C;
    pub const AIR_ACCELERATION: u32 = 0x20;
    pub const AIR_DECELERATION: u32 = 0x24;
    pub const AIR_TURN_RATE: u32 = 0x28;
    pub const GRAVITY_MAX_SPEED: u32 = 0x2C;
    pub const GRAVITY_ACCELERATION: u32 = 0x30;
    pub const JUMP_MAX_SPEED: u32 = 0x34;
    pub const JUMP_ACCELERATION: u32 = 0x38;
    pub const JUMP_INITIAL_SPEED: u32 = 0x3C;
    pub const MOTION_TYPE: u32 = 0x40;
    pub const TILTS_WITH_GROUND: u32 = 0x44;
}

impl<'a> Layout<'a> for ModelParameters<'a> {
    const TYPE: &'static str = "ModelParameters";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            object_owner: v.object_ref(Self::OBJECT_OWNER, "_objectOwner")?,
            ground_max_speed: v.f32(Self::GROUND_MAX_SPEED, "_groundMaxSpeed")?,
            ground_acceleration: v.f32(Self::GROUND_ACCELERATION, "_groundAcceleration")?,
            ground_deceleration: v.f32(Self::GROUND_DECELERATION, "_groundDeceleration")?,
            ground_turn_rate: v.f32(Self::GROUND_TURN_RATE, "_groundTurnRate")?,
            air_max_speed: v.f32(Self::AIR_MAX_SPEED, "_airMaxSpeed")?,
            air_acceleration: v.f32(Self::AIR_ACCELERATION, "_airAcceleration")?,
            air_deceleration: v.f32(Self::AIR_DECELERATION, "_airDeceleration")?,
            air_turn_rate: v.f32(Self::AIR_TURN_RATE, "_airTurnRate")?,
            gravity_max_speed: v.f32(Self::GRAVITY_MAX_SPEED, "_gravityMaxSpeed")?,
            gravity_acceleration: v.f32(Self::GRAVITY_ACCELERATION, "_gravityAcceleration")?,
            jump_max_speed: v.f32(Self::JUMP_MAX_SPEED, "_jumpMaxSpeed")?,
            jump_acceleration: v.f32(Self::JUMP_ACCELERATION, "_jumpAcceleration")?,
            jump_initial_speed: v.f32(Self::JUMP_INITIAL_SPEED, "_jumpInitialSpeed")?,
            motion_type: v.u32(Self::MOTION_TYPE, "_motionType")?,
            tilts_with_ground: v.bool(Self::TILTS_WITH_GROUND, "_tiltsWithGround")?,
        })
    }
}

/// `ActorParameters`: size 0x4C, a model's parameters plus a time scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorParameters<'a> {
    pub parameters: ModelParameters<'a>,
    pub object_time_scale: f32,
}

impl ActorParameters<'_> {
    pub const OBJECT_TIME_SCALE: u32 = 0x48;
}

impl<'a> Layout<'a> for ActorParameters<'a> {
    const TYPE: &'static str = "ActorParameters";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            parameters: ModelParameters::fields(v)?,
            object_time_scale: v.f32(Self::OBJECT_TIME_SCALE, "_objectTimeScale")?,
        })
    }
}

/// `ActorWaypoint`: size 0x38, through `AbstractAttachNode`: one point of
/// an actor's path, in the level's frame (`213_Init_Location`'s first is
/// where the first player starts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorWaypoint<'a> {
    pub name: Option<&'a str>,
    pub tag_list: ObjectRef<'a>,
    pub height: f32,
    pub init_matrix: EulerTransform,
}

impl ActorWaypoint<'_> {
    pub const NAME: u32 = 0x08;
    pub const TAG_LIST: u32 = 0x18;
    pub const HEIGHT: u32 = 0x1C;
    pub const INIT_MATRIX: u32 = 0x20;
}

impl<'a> Layout<'a> for ActorWaypoint<'a> {
    const TYPE: &'static str = "ActorWaypoint";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            tag_list: v.object_ref(Self::TAG_LIST, "_tagList")?,
            height: v.f32(Self::HEIGHT, "_height")?,
            init_matrix: EulerTransform::read(v, Self::INIT_MATRIX, "_initMatrix")?,
        })
    }
}

/// `ActorWaypointList`: an actor's waypoints in path order. A null slot is
/// refused: the disc has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorWaypointList<'a> {
    pub list: DataList<'a>,
    /// Indexes of the `ActorWaypoint` objects.
    pub waypoints: Vec<usize>,
}

impl<'a> Layout<'a> for ActorWaypointList<'a> {
    const TYPE: &'static str = "ActorWaypointList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let waypoints = v
            .elements(&list, Some(ActorWaypoint::TYPE))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { list, waypoints })
    }
}
