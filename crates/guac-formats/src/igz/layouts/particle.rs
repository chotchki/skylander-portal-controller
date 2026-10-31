//! Particle generators (openspec AH.87): a `tfbParticleInfo` emitter's
//! `_generatorInfoList` holds its generators, each a sprite generator
//! (`GeneratorInfo`, with its generation, movement and appearance records)
//! or a ribbon (`RibbonGenerator`), and each names the texture it draws
//! through an `AnimatableTextureDataInfo`.
//!
//! Offsets are the registry's (AC.4's method). The bitfields' shifts and
//! widths are their field functions', read in the EBOOT (AH.87's research):
//! the blend mode is `AbstractVFXGenerator._properties`' low 2 bits
//! (knockout, blend, add; add by default) and the overlay layer the next
//! 8; a movement record's bits are uniform distribution (bit 0), inherits
//! translation (bit 1), the spawn volume (bits 2-3: box, sphere,
//! cylinder) and the initial velocity's direction (bits 4-5: along the
//! generator's orientation, outward, inward); an appearance record's are a
//! random rotation direction (bit 0), the view alignment (bits 1-2) and a
//! version (bit 3); a segment generator's the taper (bits 0-1: none, end,
//! start, both), a version (bit 2) and its segment count (bits 3-18).
//!
//! Lengths on the disc are metres (a trail sprite's 0.4572 is 1.5 ft),
//! angles and rotation speeds radians, the initial matrix's turn degrees.

use super::script::{EulerTransform, Rgba8};
use super::{DataList, Layout, ObjectRef, View};
use crate::FormatError;

/// `AbstractVFXGenerator._blendMode`: how a generator's draws land on the
/// frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Drawn opaque where its texture's alpha passes.
    Knockout,
    /// Blended by its alpha.
    Blend,
    /// Added to what's under it.
    Add,
}

impl BlendMode {
    /// The field's code, `None` past the three the EBOOT names.
    pub fn of(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Knockout),
            1 => Some(Self::Blend),
            2 => Some(Self::Add),
            _ => None,
        }
    }
}

/// `GeneratorMovementFields._spawnVolumeType`: the shape particles are born
/// in, `_spawnVolume` its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpawnVolume {
    Box,
    Sphere,
    Cylinder,
}

impl SpawnVolume {
    pub fn of(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Box),
            1 => Some(Self::Sphere),
            2 => Some(Self::Cylinder),
            _ => None,
        }
    }
}

/// `GeneratorMovementFields._initialVelocityDirection`: which way a
/// particle leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VelocityDirection {
    /// Along the generator's own orientation.
    Orientation,
    /// Away from the generator's centre.
    Outward,
    /// Towards it.
    Inward,
}

impl VelocityDirection {
    pub fn of(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Orientation),
            1 => Some(Self::Outward),
            2 => Some(Self::Inward),
            _ => None,
        }
    }
}

/// `SegmentGenerator._taperMode`: which end of a ribbon narrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Taper {
    None,
    End,
    Start,
    Both,
}

impl Taper {
    pub fn of(code: u32) -> Self {
        match code & 3 {
            0 => Self::None,
            1 => Self::End,
            2 => Self::Start,
            _ => Self::Both,
        }
    }
}

/// `AbstractVFXGenerator`'s fields (size 0x44, through
/// `AbstractAttachNode`, whose name and `_tagList` it carries): what every
/// generator kind shares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VfxGenerator<'a> {
    pub name: Option<&'a str>,
    pub tag_list: ObjectRef<'a>,
    /// Storage for the blend mode and overlay layer bitfields.
    pub properties: u32,
    pub bloom_intensity: f32,
    /// Where the generator sits on its emitter: metres, then tilt, heading
    /// and roll in degrees.
    pub init_matrix: EulerTransform,
    /// An `AnimatableTextureDataInfo`.
    pub texture_data: Option<usize>,
    /// Seconds before it starts.
    pub start_delay: f32,
}

impl<'a> VfxGenerator<'a> {
    pub const NAME: u32 = 0x08;
    pub const TAG_LIST: u32 = 0x18;
    pub const PROPERTIES: u32 = 0x1C;
    pub const BLOOM_INTENSITY: u32 = 0x20;
    pub const INIT_MATRIX: u32 = 0x24;
    pub const TEXTURE_DATA: u32 = 0x3C;
    pub const START_DELAY: u32 = 0x40;

    pub(super) fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            tag_list: v.object_ref(Self::TAG_LIST, "_tagList")?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            bloom_intensity: v.f32(Self::BLOOM_INTENSITY, "_bloomIntensity")?,
            init_matrix: EulerTransform::read(v, Self::INIT_MATRIX, "_initMatrix")?,
            texture_data: v.object(
                Self::TEXTURE_DATA,
                "_animatableTextureDataInfo",
                "AnimatableTextureDataInfo",
            )?,
            start_delay: v.f32(Self::START_DELAY, "_startDelay")?,
        })
    }

    /// `_blendMode`'s code: bits 0-1.
    pub fn blend_code(&self) -> u32 {
        self.properties & 0x3
    }

    /// `_blendMode`, `None` for the one code the EBOOT doesn't name.
    pub fn blend_mode(&self) -> Option<BlendMode> {
        BlendMode::of(self.blend_code())
    }

    /// `_overlayLayerBits`: bits 2-9.
    pub fn overlay_layer(&self) -> u32 {
        (self.properties >> 2) & 0xFF
    }
}

/// `GeneratorInfo`: size 0x54, a sprite generator: its records, each a
/// local object (`_affectorList` stays unfollowed: null on every one of
/// Eruptor's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneratorInfo<'a> {
    pub vfx: VfxGenerator<'a>,
    /// A `GeneratorGenerationFields`.
    pub generation: Option<usize>,
    /// A `GeneratorMovementFields`.
    pub movement: Option<usize>,
    pub affector_list: ObjectRef<'a>,
    /// A `GeneratorAppearanceFields`.
    pub appearance: Option<usize>,
}

impl GeneratorInfo<'_> {
    pub const GENERATION: u32 = 0x44;
    pub const MOVEMENT: u32 = 0x48;
    pub const AFFECTOR_LIST: u32 = 0x4C;
    pub const APPEARANCE: u32 = 0x50;
}

impl<'a> Layout<'a> for GeneratorInfo<'a> {
    const TYPE: &'static str = "GeneratorInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            vfx: VfxGenerator::fields(v)?,
            generation: v.object(
                Self::GENERATION,
                "_generationFields",
                GenerationFields::TYPE,
            )?,
            movement: v.object(Self::MOVEMENT, "_movementFields", MovementFields::TYPE)?,
            affector_list: v.object_ref(Self::AFFECTOR_LIST, "_affectorList")?,
            appearance: v.object(
                Self::APPEARANCE,
                "_appearanceFields",
                AppearanceFields::TYPE,
            )?,
        })
    }
}

/// `GeneratorGenerationFields`: size 0x34, how many particles a sprite
/// generator makes and how long each lives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenerationFields {
    /// Particles a second.
    pub spawn_rate: f32,
    /// The most alive at once (50 by default).
    pub max_alive: u16,
    /// Runtime bookkeeping, 0 in files.
    pub currently_alive: u16,
    /// Seconds, and the most a particle's draws off it.
    pub lifespan: f32,
    pub lifespan_randomness: f32,
    /// Metres (100 ft by default).
    pub max_distance: f32,
    /// Where in a life the middle key sits, percent; negative (the
    /// default, -50) for none.
    pub midpoint_percent: f32,
    pub next_gen_scale_factor: f32,
}

impl GenerationFields {
    pub const SPAWN_RATE: u32 = 0x18;
    pub const MAX_ALIVE: u32 = 0x1C;
    pub const CURRENTLY_ALIVE: u32 = 0x1E;
    pub const LIFESPAN: u32 = 0x20;
    pub const LIFESPAN_RANDOMNESS: u32 = 0x24;
    pub const MAX_DISTANCE: u32 = 0x28;
    pub const MIDPOINT_PERCENT: u32 = 0x2C;
    pub const NEXT_GEN_SCALE_FACTOR: u32 = 0x30;
}

impl<'a> Layout<'a> for GenerationFields {
    const TYPE: &'static str = "GeneratorGenerationFields";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            spawn_rate: v.f32(Self::SPAWN_RATE, "_spawnRate")?,
            max_alive: v.u16(Self::MAX_ALIVE, "_maxAlive")?,
            currently_alive: v.u16(Self::CURRENTLY_ALIVE, "_currentlyAlive")?,
            lifespan: v.f32(Self::LIFESPAN, "_lifespan")?,
            lifespan_randomness: v.f32(Self::LIFESPAN_RANDOMNESS, "_lifespanRandomness")?,
            max_distance: v.f32(Self::MAX_DISTANCE, "_maxDistance")?,
            midpoint_percent: v.f32(Self::MIDPOINT_PERCENT, "_midpointPercent")?,
            next_gen_scale_factor: v.f32(Self::NEXT_GEN_SCALE_FACTOR, "_nextGenScaleFactor")?,
        })
    }
}

/// `GeneratorMovementFields`: size 0x4C, where a particle is born and how
/// it moves: its speed, drag and gravity at the start, middle and end of
/// its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementFields {
    /// Storage for the four bitfields below.
    pub properties: u32,
    /// The spawn volume's size, metres.
    pub spawn_volume: [f32; 3],
    /// Metres a second.
    pub initial_speed: f32,
    pub initial_speed_randomness: f32,
    /// Radians off the direction.
    pub initial_direction_spread: f32,
    pub start_drag: f32,
    pub end_drag: f32,
    /// Metres a second squared, up positive.
    pub start_gravity: f32,
    pub end_gravity: f32,
    pub mid_drag: f32,
    pub mid_gravity: f32,
}

impl MovementFields {
    pub const PROPERTIES: u32 = 0x18;
    pub const SPAWN_VOLUME: u32 = 0x1C;
    pub const INITIAL_SPEED: u32 = 0x28;
    pub const INITIAL_SPEED_RANDOMNESS: u32 = 0x2C;
    pub const INITIAL_DIRECTION_SPREAD: u32 = 0x30;
    pub const START_DRAG: u32 = 0x34;
    pub const END_DRAG: u32 = 0x38;
    pub const START_GRAVITY: u32 = 0x3C;
    pub const END_GRAVITY: u32 = 0x40;
    pub const MID_DRAG: u32 = 0x44;
    pub const MID_GRAVITY: u32 = 0x48;

    pub fn uniform_distribution(&self) -> bool {
        self.properties & 1 != 0
    }

    pub fn inherits_translation(&self) -> bool {
        self.properties & (1 << 1) != 0
    }

    /// `_spawnVolumeType`'s code: bits 2-3.
    pub fn spawn_volume_code(&self) -> u32 {
        (self.properties >> 2) & 0x3
    }

    /// `_initialVelocityDirection`'s code: bits 4-5.
    pub fn velocity_code(&self) -> u32 {
        (self.properties >> 4) & 0x3
    }
}

impl<'a> Layout<'a> for MovementFields {
    const TYPE: &'static str = "GeneratorMovementFields";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            spawn_volume: v.floats(Self::SPAWN_VOLUME, "_spawnVolume")?,
            initial_speed: v.f32(Self::INITIAL_SPEED, "_initialSpeed")?,
            initial_speed_randomness: v
                .f32(Self::INITIAL_SPEED_RANDOMNESS, "_initialSpeedRandomness")?,
            initial_direction_spread: v
                .f32(Self::INITIAL_DIRECTION_SPREAD, "_initialDirectionSpread")?,
            start_drag: v.f32(Self::START_DRAG, "_startDrag")?,
            end_drag: v.f32(Self::END_DRAG, "_endDrag")?,
            start_gravity: v.f32(Self::START_GRAVITY, "_startGravity")?,
            end_gravity: v.f32(Self::END_GRAVITY, "_endGravity")?,
            mid_drag: v.f32(Self::MID_DRAG, "_midDrag")?,
            mid_gravity: v.f32(Self::MID_GRAVITY, "_midGravity")?,
        })
    }
}

/// `GeneratorAppearanceFields`: size 0x9C, how a particle looks over its
/// life: its colour, size and spin at the start, middle and end, each with
/// a randomness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearanceFields {
    pub start_color: Rgba8,
    pub start_color_randomness: Rgba8,
    pub end_color: Rgba8,
    pub end_color_randomness: Rgba8,
    /// Metres: width, height, depth.
    pub start_size: [f32; 3],
    pub start_size_randomness: [f32; 3],
    pub end_size: [f32; 3],
    pub end_size_randomness: [f32; 3],
    /// Radians.
    pub initial_rotation_angle: f32,
    pub initial_rotation_angle_randomness: f32,
    /// Radians a second.
    pub start_rotation_speed: f32,
    pub start_rotation_speed_randomness: f32,
    pub end_rotation_speed: f32,
    pub end_rotation_speed_randomness: f32,
    pub mid_color: Rgba8,
    pub mid_color_randomness: Rgba8,
    pub mid_size: [f32; 3],
    pub mid_size_randomness: [f32; 3],
    pub mid_rotation_speed: f32,
    pub mid_rotation_speed_randomness: f32,
    /// Storage for the three bitfields below.
    pub properties: u32,
}

impl AppearanceFields {
    pub const START_COLOR: u32 = 0x18;
    pub const START_COLOR_RANDOMNESS: u32 = 0x1C;
    pub const END_COLOR: u32 = 0x20;
    pub const END_COLOR_RANDOMNESS: u32 = 0x24;
    pub const START_SIZE: u32 = 0x28;
    pub const START_SIZE_RANDOMNESS: u32 = 0x34;
    pub const END_SIZE: u32 = 0x40;
    pub const END_SIZE_RANDOMNESS: u32 = 0x4C;
    pub const INITIAL_ROTATION_ANGLE: u32 = 0x58;
    pub const INITIAL_ROTATION_ANGLE_RANDOMNESS: u32 = 0x5C;
    pub const START_ROTATION_SPEED: u32 = 0x60;
    pub const START_ROTATION_SPEED_RANDOMNESS: u32 = 0x64;
    pub const END_ROTATION_SPEED: u32 = 0x68;
    pub const END_ROTATION_SPEED_RANDOMNESS: u32 = 0x6C;
    pub const MID_COLOR: u32 = 0x70;
    pub const MID_COLOR_RANDOMNESS: u32 = 0x74;
    pub const MID_SIZE: u32 = 0x78;
    pub const MID_SIZE_RANDOMNESS: u32 = 0x84;
    pub const MID_ROTATION_SPEED: u32 = 0x90;
    pub const MID_ROTATION_SPEED_RANDOMNESS: u32 = 0x94;
    pub const PROPERTIES: u32 = 0x98;

    pub fn random_rotation_direction(&self) -> bool {
        self.properties & 1 != 0
    }

    /// `_viewAlignment`'s code: bits 1-2.
    pub fn view_alignment(&self) -> u32 {
        (self.properties >> 1) & 0x3
    }

    /// `_version`: bit 3.
    pub fn version(&self) -> u32 {
        (self.properties >> 3) & 1
    }
}

impl<'a> Layout<'a> for AppearanceFields {
    const TYPE: &'static str = "GeneratorAppearanceFields";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let colour = |v: &mut View<'_, 'a>, at, field| v.u32(at, field).map(u32::to_be_bytes);
        Ok(Self {
            start_color: colour(v, Self::START_COLOR, "_startColor")?,
            start_color_randomness: colour(
                v,
                Self::START_COLOR_RANDOMNESS,
                "_startColorRandomness",
            )?,
            end_color: colour(v, Self::END_COLOR, "_endColor")?,
            end_color_randomness: colour(v, Self::END_COLOR_RANDOMNESS, "_endColorRandomness")?,
            start_size: v.floats(Self::START_SIZE, "_startSize")?,
            start_size_randomness: v.floats(Self::START_SIZE_RANDOMNESS, "_startSizeRandomness")?,
            end_size: v.floats(Self::END_SIZE, "_endSize")?,
            end_size_randomness: v.floats(Self::END_SIZE_RANDOMNESS, "_endSizeRandomness")?,
            initial_rotation_angle: v.f32(Self::INITIAL_ROTATION_ANGLE, "_initialRotationAngle")?,
            initial_rotation_angle_randomness: v.f32(
                Self::INITIAL_ROTATION_ANGLE_RANDOMNESS,
                "_initialRotationAngleRandomness",
            )?,
            start_rotation_speed: v.f32(Self::START_ROTATION_SPEED, "_startRotationSpeed")?,
            start_rotation_speed_randomness: v.f32(
                Self::START_ROTATION_SPEED_RANDOMNESS,
                "_startRotationSpeedRandomness",
            )?,
            end_rotation_speed: v.f32(Self::END_ROTATION_SPEED, "_endRotationSpeed")?,
            end_rotation_speed_randomness: v.f32(
                Self::END_ROTATION_SPEED_RANDOMNESS,
                "_endRotationSpeedRandomness",
            )?,
            mid_color: colour(v, Self::MID_COLOR, "_midColor")?,
            mid_color_randomness: colour(v, Self::MID_COLOR_RANDOMNESS, "_midColorRandomness")?,
            mid_size: v.floats(Self::MID_SIZE, "_midSize")?,
            mid_size_randomness: v.floats(Self::MID_SIZE_RANDOMNESS, "_midSizeRandomness")?,
            mid_rotation_speed: v.f32(Self::MID_ROTATION_SPEED, "_midRotationSpeed")?,
            mid_rotation_speed_randomness: v.f32(
                Self::MID_ROTATION_SPEED_RANDOMNESS,
                "_midRotationSpeedRandomness",
            )?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
        })
    }
}

/// `RibbonGenerator`: size 0x60, through `RibbonFields` and
/// `SegmentGenerator`: a strip drawn along where its emitter has been.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RibbonGenerator<'a> {
    pub vfx: VfxGenerator<'a>,
    /// Storage for the taper, version and segment count bitfields.
    pub segment_properties: u32,
    pub scroll_uv_percent: f32,
    /// 100 by default.
    pub scale_uv_percent: f32,
    /// Metres (1 by default).
    pub segment_width: f32,
    /// The head's colour.
    pub tint_color: Rgba8,
    /// The tail's.
    pub end_color: Rgba8,
    /// 100 by default.
    pub segment_granularity: f32,
}

impl RibbonGenerator<'_> {
    pub const SEGMENT_PROPERTIES: u32 = 0x44;
    pub const SCROLL_UV_PERCENT: u32 = 0x48;
    pub const SCALE_UV_PERCENT: u32 = 0x4C;
    pub const SEGMENT_WIDTH: u32 = 0x50;
    pub const TINT_COLOR: u32 = 0x54;
    pub const END_COLOR: u32 = 0x58;
    pub const SEGMENT_GRANULARITY: u32 = 0x5C;

    pub fn taper(&self) -> Taper {
        Taper::of(self.segment_properties)
    }

    /// `_version`: bit 2.
    pub fn version(&self) -> u32 {
        (self.segment_properties >> 2) & 1
    }

    /// `_segmentCount`: bits 3-18.
    pub fn segment_count(&self) -> u32 {
        (self.segment_properties >> 3) & 0xFFFF
    }
}

impl<'a> Layout<'a> for RibbonGenerator<'a> {
    const TYPE: &'static str = "RibbonGenerator";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            vfx: VfxGenerator::fields(v)?,
            segment_properties: v.u32(Self::SEGMENT_PROPERTIES, "_properties")?,
            scroll_uv_percent: v.f32(Self::SCROLL_UV_PERCENT, "_scrollUVPercent")?,
            scale_uv_percent: v.f32(Self::SCALE_UV_PERCENT, "_scaleUVPercent")?,
            segment_width: v.f32(Self::SEGMENT_WIDTH, "_segmentWidth")?,
            tint_color: v.u32(Self::TINT_COLOR, "_tintColor")?.to_be_bytes(),
            end_color: v.u32(Self::END_COLOR, "_endColor")?.to_be_bytes(),
            segment_granularity: v.f32(Self::SEGMENT_GRANULARITY, "_segmentGranularity")?,
        })
    }
}

/// `GeneratorInfoList`: an emitter's generators in list order, each kept by
/// its object (a null slot as `None`); their kinds are the reader's to
/// tell apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorInfoList<'a> {
    pub list: DataList<'a>,
    pub generators: Vec<Option<usize>>,
}

impl<'a> Layout<'a> for GeneratorInfoList<'a> {
    const TYPE: &'static str = "GeneratorInfoList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let generators = v.elements(&list, None)?;
        Ok(Self { list, generators })
    }
}
