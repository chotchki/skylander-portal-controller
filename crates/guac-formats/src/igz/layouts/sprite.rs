//! Sprites (openspec AE.1): `tfbSpriteInfo` is a placement in a UI layout,
//! and an image sprite's `_contextDataInfo` is an `AnimatableTextureDataInfo`
//! whose `_texAttr` binds the texture. The collection screen's portraits are
//! `tfbSpriteInfo` -> `AnimatableTextureDataInfo` -> `tfbAlphaTextureUnitIDAttr`
//! -> `igTextureAttr2` -> `igImage2`.

use super::script::{EulerTransform, Rgba8};
use super::{BitField, BuildDataInfo, DataList, Layout, ObjectRef, View};
use crate::FormatError;

/// `AbstractPlacement`'s fields (size 0xA0, through `AbstractAttachNode`,
/// whose `_tagList` it carries). The refs are runtime links and stay
/// unfollowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbstractPlacement<'a> {
    pub name: Option<&'a str>,
    pub tag_list: ObjectRef<'a>,
    /// Storage for `_explicitAttachment`.
    pub flag_pool: u32,
    pub id: i32,
    pub init_matrix: EulerTransform,
    pub current_matrix: EulerTransform,
    /// An enum, kept raw.
    pub activity_state: u32,
    pub clones: ObjectRef<'a>,
    pub parent: ObjectRef<'a>,
    pub activator: ObjectRef<'a>,
    pub move_object: ObjectRef<'a>,
    pub move_index: i32,
    pub attach_node: ObjectRef<'a>,
    pub attach_node_owner: ObjectRef<'a>,
    pub attached_objects: ObjectRef<'a>,
    pub attach_xform: EulerTransform,
    pub sound_list: ObjectRef<'a>,
    pub animation_list: ObjectRef<'a>,
    pub reset_template: ObjectRef<'a>,
    pub active_time_layer: u32,
}

impl<'a> AbstractPlacement<'a> {
    pub const NAME: u32 = 0x08;
    pub const TAG_LIST: u32 = 0x18;
    pub const FLAG_POOL: u32 = 0x1C;
    pub const ID: u32 = 0x20;
    pub const INIT_MATRIX: u32 = 0x24;
    pub const CURRENT_MATRIX: u32 = 0x3C;
    pub const ACTIVITY_STATE: u32 = 0x54;
    pub const CLONES: u32 = 0x58;
    pub const PARENT: u32 = 0x5C;
    pub const ACTIVATOR: u32 = 0x60;
    pub const MOVE_OBJECT: u32 = 0x64;
    pub const MOVE_INDEX: u32 = 0x68;
    pub const ATTACH_NODE: u32 = 0x6C;
    pub const ATTACH_NODE_OWNER: u32 = 0x70;
    pub const ATTACHED_OBJECTS: u32 = 0x74;
    pub const ATTACH_XFORM: u32 = 0x78;
    pub const SOUND_LIST: u32 = 0x90;
    pub const ANIMATION_LIST: u32 = 0x94;
    pub const RESET_TEMPLATE: u32 = 0x98;
    pub const ACTIVE_TIME_LAYER: u32 = 0x9C;

    pub(super) fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            tag_list: v.object_ref(Self::TAG_LIST, "_tagList")?,
            flag_pool: v.u32(Self::FLAG_POOL, "_flagPool")?,
            id: v.i32(Self::ID, "_ID")?,
            init_matrix: EulerTransform::read(v, Self::INIT_MATRIX, "_initMatrix")?,
            current_matrix: EulerTransform::read(v, Self::CURRENT_MATRIX, "_currentMatrix")?,
            activity_state: v.u32(Self::ACTIVITY_STATE, "_activityState")?,
            clones: v.object_ref(Self::CLONES, "_clones")?,
            parent: v.object_ref(Self::PARENT, "_parent")?,
            activator: v.object_ref(Self::ACTIVATOR, "_activator")?,
            move_object: v.object_ref(Self::MOVE_OBJECT, "_moveObject")?,
            move_index: v.i32(Self::MOVE_INDEX, "_moveIndex")?,
            attach_node: v.object_ref(Self::ATTACH_NODE, "_attachNode")?,
            attach_node_owner: v.object_ref(Self::ATTACH_NODE_OWNER, "_attachNodeOwner")?,
            attached_objects: v.object_ref(Self::ATTACHED_OBJECTS, "_attachedObjects")?,
            attach_xform: EulerTransform::read(v, Self::ATTACH_XFORM, "_attachXform")?,
            sound_list: v.object_ref(Self::SOUND_LIST, "_soundList")?,
            animation_list: v.object_ref(Self::ANIMATION_LIST, "_animationList")?,
            reset_template: v.object_ref(Self::RESET_TEMPLATE, "_resetTemplate")?,
            active_time_layer: v.u32(Self::ACTIVE_TIME_LAYER, "_activeTimeLayer")?,
        })
    }
}

/// What a sprite draws: `_visualType`'s values, in the order the engine's
/// `spriteinfo.type` member names them.
pub const VISUAL_TYPES: [&str; 5] = ["art", "text", "model", "container", "particles"];

/// The nine anchors `_alignment` and `_alignToParent` name, top-left first,
/// row by row.
pub const ALIGNMENTS: [&str; 9] = [
    "top-left",
    "top-middle",
    "top-right",
    "center-left",
    "center",
    "center-right",
    "bottom-left",
    "bottom-middle",
    "bottom-right",
];

/// `_visualType`'s value as [`VISUAL_TYPES`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualType {
    Art,
    Text,
    Model,
    Container,
    Particles,
    /// 5-7: none on the disc.
    Other(u8),
}

impl VisualType {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Art,
            1 => Self::Text,
            2 => Self::Model,
            3 => Self::Container,
            4 => Self::Particles,
            other => Self::Other(other),
        }
    }
}

/// `tfbSpriteInfo._properties`' nine bitfields (AT.1), read off the class's
/// field registration function at 0xB0CD8: each field's shift and width,
/// from the word's low end, and the registered defaults (blend mode 1,
/// visible and dirty on, the rest 0). The engine's `spriteinfo` members read
/// and write the same bits (`type` bits 0-2, `blend mode` 3-5, `justify`
/// 6-7, `alignment` 8-11, `align to parent` 12-15, `visible` 16, `point
/// size` 17-28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBits {
    /// [`VisualType`]'s raw value.
    pub visual_type: u8,
    pub blend_mode: u8,
    pub justification: u8,
    /// An [`ALIGNMENTS`] index: the sprite's own anchor.
    pub alignment: u8,
    /// An [`ALIGNMENTS`] index: the parent's anchor it sits on.
    pub align_to_parent: u8,
    pub visible: bool,
    pub point_size: u16,
    pub dirty: bool,
    pub string_width_valid: bool,
}

impl SpriteBits {
    pub const VISUAL_TYPE: BitField = BitField::new(0, 3);
    pub const BLEND_MODE: BitField = BitField::new(3, 3);
    pub const JUSTIFICATION: BitField = BitField::new(6, 2);
    pub const ALIGNMENT: BitField = BitField::new(8, 4);
    pub const ALIGN_TO_PARENT: BitField = BitField::new(12, 4);
    pub const VISIBLE: BitField = BitField::new(16, 1);
    pub const POINT_SIZE: BitField = BitField::new(17, 12);
    pub const DIRTY: BitField = BitField::new(29, 1);
    pub const STRING_WIDTH_VALID: BitField = BitField::new(30, 1);

    /// Every field's width fits its type, so the casts are lossless.
    pub fn from_word(word: u32) -> Self {
        Self {
            visual_type: Self::VISUAL_TYPE.get(word) as u8,
            blend_mode: Self::BLEND_MODE.get(word) as u8,
            justification: Self::JUSTIFICATION.get(word) as u8,
            alignment: Self::ALIGNMENT.get(word) as u8,
            align_to_parent: Self::ALIGN_TO_PARENT.get(word) as u8,
            visible: Self::VISIBLE.get(word) != 0,
            point_size: Self::POINT_SIZE.get(word) as u16,
            dirty: Self::DIRTY.get(word) != 0,
            string_width_valid: Self::STRING_WIDTH_VALID.get(word) != 0,
        }
    }

    /// The word these fields make, bit 31 (no field's) clear; each value
    /// past its width keeps its low bits.
    pub fn to_word(&self) -> u32 {
        [
            (Self::VISUAL_TYPE, u32::from(self.visual_type)),
            (Self::BLEND_MODE, u32::from(self.blend_mode)),
            (Self::JUSTIFICATION, u32::from(self.justification)),
            (Self::ALIGNMENT, u32::from(self.alignment)),
            (Self::ALIGN_TO_PARENT, u32::from(self.align_to_parent)),
            (Self::VISIBLE, u32::from(self.visible)),
            (Self::POINT_SIZE, u32::from(self.point_size)),
            (Self::DIRTY, u32::from(self.dirty)),
            (Self::STRING_WIDTH_VALID, u32::from(self.string_width_valid)),
        ]
        .into_iter()
        .fold(0, |word, (field, value)| field.set(word, value))
    }

    pub fn kind(&self) -> VisualType {
        VisualType::from_raw(self.visual_type)
    }
}

/// `tfbSpriteInfo`: size 0xE4, one UI sprite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteInfo<'a> {
    pub placement: AbstractPlacement<'a>,
    pub flags: i32,
    /// Screen position, 12.4 fixed point (aj-vm).
    pub position: [i16; 2],
    /// Storage for the visual type, blend mode, alignment and visibility
    /// bitfields.
    pub properties: u32,
    pub z_depth: f32,
    pub priority: f32,
    pub width: f32,
    pub height: f32,
    pub scale: [f32; 3],
    pub color: Rgba8,
    pub replace_color: Rgba8,
    /// What the sprite draws: an `AnimatableTextureDataInfo` on an image
    /// sprite, left unfollowed here ([`SpriteInfo::texture_data`]).
    pub context_data_info: ObjectRef<'a>,
    pub string_info: ObjectRef<'a>,
    pub num_value: i32,
    pub depth: f32,
    pub game_trans: ObjectRef<'a>,
}

impl SpriteInfo<'_> {
    pub const FLAGS: u32 = 0xA0;
    pub const POSITION: u32 = 0xA4;
    pub const PROPERTIES: u32 = 0xA8;
    pub const Z_DEPTH: u32 = 0xAC;
    pub const PRIORITY: u32 = 0xB0;
    pub const WIDTH: u32 = 0xB4;
    pub const HEIGHT: u32 = 0xB8;
    pub const SCALE: u32 = 0xBC;
    pub const COLOR: u32 = 0xC8;
    pub const REPLACE_COLOR: u32 = 0xCC;
    pub const CONTEXT_DATA_INFO: u32 = 0xD0;
    pub const STRING_INFO: u32 = 0xD4;
    pub const NUM_VALUE: u32 = 0xD8;
    pub const DEPTH: u32 = 0xDC;
    pub const GAME_TRANS: u32 = 0xE0;

    /// `_properties`' bitfields.
    pub fn bits(&self) -> SpriteBits {
        SpriteBits::from_word(self.properties)
    }

    /// The local `AnimatableTextureDataInfo` an image sprite draws, if its
    /// context is one.
    pub fn texture_data(&self, igz: &crate::igz::Igz<'_>) -> Option<usize> {
        let ObjectRef::Object(i) = self.context_data_info else {
            return None;
        };
        let o = igz.objects().get(i)?;
        super::is_a(igz.type_name(o), AnimatableTextureDataInfo::TYPE).then_some(i)
    }
}

impl<'a> Layout<'a> for SpriteInfo<'a> {
    const TYPE: &'static str = "tfbSpriteInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let placement = AbstractPlacement::fields(v)?;
        Ok(Self {
            placement,
            flags: v.i32(Self::FLAGS, "_flags")?,
            position: [
                v.i16(Self::POSITION, "_position")?,
                v.i16(Self::POSITION + 2, "_position")?,
            ],
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            z_depth: v.f32(Self::Z_DEPTH, "_zDepth")?,
            priority: v.f32(Self::PRIORITY, "_priority")?,
            width: v.f32(Self::WIDTH, "_width")?,
            height: v.f32(Self::HEIGHT, "_height")?,
            scale: v.floats(Self::SCALE, "_scaleVec")?,
            color: v.u32(Self::COLOR, "_color")?.to_be_bytes(),
            replace_color: v.u32(Self::REPLACE_COLOR, "_replaceColor")?.to_be_bytes(),
            context_data_info: v.object_ref(Self::CONTEXT_DATA_INFO, "_contextDataInfo")?,
            string_info: v.object_ref(Self::STRING_INFO, "_stringInfo")?,
            num_value: v.i32(Self::NUM_VALUE, "_numValue")?,
            depth: v.f32(Self::DEPTH, "_depth")?,
            game_trans: v.object_ref(Self::GAME_TRANS, "_gameTrans")?,
        })
    }
}

/// `AnimatableTextureDataInfo`: size 0x34, through `tfbBuildDataInfo` (its
/// name is the source image's path). `_texAttr` is the texture bind: a
/// `tfbAlphaTextureUnitIDAttr` on every one on the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimatableTextureDataInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    pub loader: ObjectRef<'a>,
    pub desired_width: i32,
    pub desired_height: i32,
    pub updated_count: i32,
    /// An `igTextureBindAttr2`.
    pub tex_attr: Option<usize>,
}

impl AnimatableTextureDataInfo<'_> {
    pub const LOADER: u32 = 0x20;
    pub const DESIRED_WIDTH: u32 = 0x24;
    pub const DESIRED_HEIGHT: u32 = 0x28;
    pub const UPDATED_COUNT: u32 = 0x2C;
    pub const TEX_ATTR: u32 = 0x30;
}

impl<'a> Layout<'a> for AnimatableTextureDataInfo<'a> {
    const TYPE: &'static str = "AnimatableTextureDataInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            loader: v.object_ref(Self::LOADER, "_loader")?,
            desired_width: v.i32(Self::DESIRED_WIDTH, "_desiredWidth")?,
            desired_height: v.i32(Self::DESIRED_HEIGHT, "_desiredHeight")?,
            updated_count: v.i32(Self::UPDATED_COUNT, "_updatedCount")?,
            tex_attr: v.object(Self::TEX_ATTR, "_texAttr", "igTextureBindAttr2")?,
        })
    }
}

/// `SpriteInfoList`: size 0x18, through `ScriptObjectList`, a template's
/// `_attachedObjects`. Empty in every file on the disc (AT.1): the scripts
/// build every tree at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteInfoList<'a> {
    pub list: DataList<'a>,
    /// The attached objects, unfollowed; `None` for a null slot.
    pub elements: Vec<Option<usize>>,
}

impl<'a> Layout<'a> for SpriteInfoList<'a> {
    const TYPE: &'static str = "SpriteInfoList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let elements = v.elements(&list, None)?;
        Ok(Self { list, elements })
    }
}
