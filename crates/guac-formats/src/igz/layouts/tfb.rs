//! The Havok wrappers and the hero's model: `tfbBuildDataInfo._data` holds a
//! whole Havok packfile (skeleton on `tfbPhysicsBody`, clip on
//! `tfbPhysicsAnimation`) or a lip-sync block, and `tfbPhysicsModel` ties the
//! body to the clip table.

use super::{DataList, Layout, ObjectRef, View};
use crate::FormatError;
use crate::igz::MemoryRef;

/// A Havok packfile's first 8 bytes.
pub const HAVOK_MAGIC: [u8; 8] = [0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10];
/// A voice line's lip-sync block starts with 0xDEADBEEF.
pub const LIP_SYNC_MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

/// `igInfo`'s fields (through `igReferenceResolver`, which adds none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info<'a> {
    pub name: Option<&'a str>,
    pub directory: ObjectRef<'a>,
    pub resolve_state: bool,
}

impl<'a> Info<'a> {
    pub const NAME: u32 = 0x08;
    pub const DIRECTORY: u32 = 0x0C;
    pub const RESOLVE_STATE: u32 = 0x10;

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            directory: v.object_ref(Self::DIRECTORY, "_directory")?,
            resolve_state: v.bool(Self::RESOLVE_STATE, "_resolveState")?,
        })
    }
}

/// What a `_data` block holds, by its magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload<'a> {
    Havok(&'a [u8]),
    LipSync(&'a [u8]),
    Other(&'a [u8]),
}

/// `tfbBuildDataInfo`: size 0x20, through `tfbComplexDataInfo` and `tfbInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildDataInfo<'a> {
    pub info: Info<'a>,
    /// u8 elements, 16-aligned.
    pub data: Option<MemoryRef<'a>>,
    pub stream_archive_hash: i32,
}

impl<'a> BuildDataInfo<'a> {
    pub const DATA: u32 = 0x14;
    pub const STREAM_ARCHIVE_HASH: u32 = 0x1C;

    pub fn payload(&self) -> Option<Payload<'a>> {
        let bytes = self.data?.data;
        Some(if bytes.starts_with(&HAVOK_MAGIC) {
            Payload::Havok(bytes)
        } else if bytes.starts_with(&LIP_SYNC_MAGIC) {
            Payload::LipSync(bytes)
        } else {
            Payload::Other(bytes)
        })
    }
}

impl<'a> Layout<'a> for BuildDataInfo<'a> {
    const TYPE: &'static str = "tfbBuildDataInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: Info::fields(v)?,
            data: v.memory_ref(Self::DATA, "_data")?,
            stream_archive_hash: v.i32(Self::STREAM_ARCHIVE_HASH, "_streamArchiveHash")?,
        })
    }
}

/// `tfbAnimationInfo`: size 0x28, a build-data block plus clip flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TfbAnimationInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    /// Storage for the bitfields below.
    pub properties: u32,
    pub animation_slot_index: i32,
}

impl TfbAnimationInfo<'_> {
    pub const PROPERTIES: u32 = 0x20;
    pub const ANIMATION_SLOT_INDEX: u32 = 0x24;

    pub fn motion_extraction_flags(&self) -> u32 {
        self.properties & 0xF
    }

    pub fn additive_blend(&self) -> bool {
        self.properties & (1 << 4) != 0
    }

    pub fn partial_animation(&self) -> bool {
        self.properties & (1 << 5) != 0
    }

    pub fn layered(&self) -> bool {
        self.properties & (1 << 6) != 0
    }

    /// 4 by default.
    pub fn compression_quality(&self) -> u32 {
        (self.properties >> 7) & 0x7
    }

    pub fn status_fields_set(&self) -> bool {
        self.properties & (1 << 10) != 0
    }
}

impl<'a> Layout<'a> for TfbAnimationInfo<'a> {
    const TYPE: &'static str = "tfbAnimationInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            animation_slot_index: v.i32(Self::ANIMATION_SLOT_INDEX, "_animationSlotIndex")?,
        })
    }
}

/// `tfbPhysicsAnimation`: size 0x2C, the clip wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsAnimation<'a> {
    pub info: TfbAnimationInfo<'a>,
    /// A 4-byte runtime id struct.
    pub id_code: u32,
}

impl PhysicsAnimation<'_> {
    pub const ID_CODE: u32 = 0x28;
}

impl<'a> Layout<'a> for PhysicsAnimation<'a> {
    const TYPE: &'static str = "tfbPhysicsAnimation";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: TfbAnimationInfo::fields(v)?,
            id_code: v.u32(Self::ID_CODE, "_idCode")?,
        })
    }
}

/// `tfbBodyInfo`: size 0x3C, a build-data block (the Havok skeleton) plus
/// the scene graph and the combiner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    /// An `igNode`; the hero's is an `igActor2`.
    pub node: Option<usize>,
    /// A `tfbPhysicsCombiner`; null on props.
    pub combiner_prototype: Option<usize>,
    /// i32 elements.
    pub bone_mapping_table: Option<MemoryRef<'a>>,
    pub viseme_set: ObjectRef<'a>,
    pub eye_blink: ObjectRef<'a>,
    pub footprint_data: ObjectRef<'a>,
}

impl BodyInfo<'_> {
    pub const NODE: u32 = 0x20;
    pub const COMBINER_PROTOTYPE: u32 = 0x24;
    pub const BONE_MAPPING_TABLE: u32 = 0x28;
    pub const VISEME_SET: u32 = 0x30;
    pub const EYE_BLINK: u32 = 0x34;
    pub const FOOTPRINT_DATA: u32 = 0x38;
}

impl<'a> Layout<'a> for BodyInfo<'a> {
    const TYPE: &'static str = "tfbBodyInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            node: v.object(Self::NODE, "_node", "igNode")?,
            combiner_prototype: v.object(
                Self::COMBINER_PROTOTYPE,
                "_combinerPrototype",
                "tfbPhysicsCombiner",
            )?,
            bone_mapping_table: v.memory_ref(Self::BONE_MAPPING_TABLE, "_boneMappingTable")?,
            viseme_set: v.object_ref(Self::VISEME_SET, "_visemeSet")?,
            eye_blink: v.object_ref(Self::EYE_BLINK, "_eyeBlink")?,
            footprint_data: v.object_ref(Self::FOOTPRINT_DATA, "_footprintData")?,
        })
    }
}

/// `tfbPhysicsBody`: size 0x40, the skeleton wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsBody<'a> {
    pub body: BodyInfo<'a>,
    /// A 4-byte runtime id struct.
    pub id_code: u32,
}

impl PhysicsBody<'_> {
    pub const ID_CODE: u32 = 0x3C;
}

impl<'a> Layout<'a> for PhysicsBody<'a> {
    const TYPE: &'static str = "tfbPhysicsBody";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            body: BodyInfo::fields(v)?,
            id_code: v.u32(Self::ID_CODE, "_idCode")?,
        })
    }
}

/// `tfbModelInfo`: size 0x2C, through `tfbComplexDataInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo<'a> {
    pub info: Info<'a>,
    /// A `tfbBodyInfo`.
    pub tfb_body: Option<usize>,
    pub model_parameters: ObjectRef<'a>,
    /// An `AnimationInfoList`, the clip table.
    pub animation_set: Option<usize>,
    pub collision: ObjectRef<'a>,
    pub attach_list: ObjectRef<'a>,
    pub template_path: Option<&'a str>,
}

impl ModelInfo<'_> {
    pub const TFB_BODY: u32 = 0x14;
    pub const MODEL_PARAMETERS: u32 = 0x18;
    pub const ANIMATION_SET: u32 = 0x1C;
    pub const COLLISION: u32 = 0x20;
    pub const ATTACH_LIST: u32 = 0x24;
    pub const TEMPLATE_PATH: u32 = 0x28;
}

impl<'a> Layout<'a> for ModelInfo<'a> {
    const TYPE: &'static str = "tfbModelInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: Info::fields(v)?,
            tfb_body: v.object(Self::TFB_BODY, "_tfbBody", "tfbBodyInfo")?,
            model_parameters: v.object_ref(Self::MODEL_PARAMETERS, "_modelParameters")?,
            animation_set: v.object(Self::ANIMATION_SET, "_animationSet", "AnimationInfoList")?,
            collision: v.object_ref(Self::COLLISION, "_collision")?,
            attach_list: v.object_ref(Self::ATTACH_LIST, "_attachList")?,
            template_path: v.string(Self::TEMPLATE_PATH, "_templatePath")?,
        })
    }
}

/// `tfbPhysicsModel`: size 0x30, what `tfbActorInfo._model` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsModel<'a> {
    pub model: ModelInfo<'a>,
    /// A 4-byte runtime id struct.
    pub id_code: u32,
}

impl PhysicsModel<'_> {
    pub const ID_CODE: u32 = 0x2C;
}

impl<'a> Layout<'a> for PhysicsModel<'a> {
    const TYPE: &'static str = "tfbPhysicsModel";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            model: ModelInfo::fields(v)?,
            id_code: v.u32(Self::ID_CODE, "_idCode")?,
        })
    }
}

/// `AnimationInfoList`: the clip table, an object list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationInfoList<'a> {
    pub list: DataList<'a>,
    /// Indexes of the `AnimationInfo` objects; a slot can be empty.
    pub entries: Vec<Option<usize>>,
}

impl<'a> Layout<'a> for AnimationInfoList<'a> {
    const TYPE: &'static str = "AnimationInfoList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let entries = v.elements(&list, None)?;
        for &e in entries.iter().flatten() {
            v.check_type(e, AnimationInfo::TYPE, "element")?;
        }
        Ok(Self { list, entries })
    }
}

/// `AnimationInfo`: size 0x44, one clip-table entry. Its parents' script
/// fields (0x0C-0x30) aren't recovered and are skipped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationInfo<'a> {
    pub name: Option<&'a str>,
    /// Bitfields as `tfbAnimationInfo`'s, without `_statusFieldsSet`.
    pub properties: u32,
    pub media_name: Option<&'a str>,
    pub blend_factor: f32,
    /// A `tfbAnimationInfo`: a `tfbPhysicsAnimation` or an animation network.
    pub tfb_animation: Option<usize>,
    pub blend_list: ObjectRef<'a>,
}

impl AnimationInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const PROPERTIES: u32 = 0x30;
    pub const MEDIA_NAME: u32 = 0x34;
    pub const BLEND_FACTOR: u32 = 0x38;
    pub const TFB_ANIMATION: u32 = 0x3C;
    pub const BLEND_LIST: u32 = 0x40;
}

impl<'a> Layout<'a> for AnimationInfo<'a> {
    const TYPE: &'static str = "AnimationInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        v.skip(0x0C, Self::PROPERTIES);
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            media_name: v.string(Self::MEDIA_NAME, "_mediaName")?,
            blend_factor: v.f32(Self::BLEND_FACTOR, "_blendFactor")?,
            tfb_animation: v.object(Self::TFB_ANIMATION, "_tfbAnimation", "tfbAnimationInfo")?,
            blend_list: v.object_ref(Self::BLEND_LIST, "_blendList")?,
        })
    }
}
