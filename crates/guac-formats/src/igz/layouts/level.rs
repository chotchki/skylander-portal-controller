//! A level's scene graph (openspec AG.3): `tfbPhysicsWorld._sceneInfo` is an
//! `igSceneInfo` whose `_sceneGraph` is a shallow tree of groups over
//! AC's technique instances (ag-levels: `igGroup` -> `igSpatialNode` ->
//! `tfbRuntimeTechniqueInstance`, with `igTransform` in 10 levels and the
//! sky under `igSimpleUserInfo`). [`crate::level`] walks it.

use super::{Group, Info, Layout, Mat44, ObjectRef, View};
use crate::FormatError;

/// An `igLongMetaField`: big-endian, high word first.
fn i64(v: &mut View<'_, '_>, offset: u32, field: &'static str) -> Result<i64, FormatError> {
    let high = v.u32(offset, field)?;
    let low = v.u32(offset + 4, field)?;
    Ok(((u64::from(high) << 32) | u64::from(low)) as i64)
}

/// `igSceneInfo`: size 0x48, through `igInfo`. One per world: 75 on the
/// disc, each a world's `_sceneInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneInfo<'a> {
    pub info: Info<'a>,
    /// The root node, an `igGroup` on every scene on the disc.
    pub scene_graph: Option<usize>,
    pub textures_deprecated: ObjectRef<'a>,
    /// An `igGraphPathList`.
    pub cameras: ObjectRef<'a>,
    pub animation_begin: i64,
    pub animation_end: i64,
    /// (0, 1, 0) on all 75.
    pub up_vector: [f32; 3],
    /// An `igTextureAttr2List`.
    pub textures: ObjectRef<'a>,
    /// An `igNodeList` of further roots.
    pub scene_graph_list: Option<usize>,
}

impl SceneInfo<'_> {
    pub const SCENE_GRAPH: u32 = 0x14;
    pub const TEXTURES_DEPRECATED: u32 = 0x18;
    pub const CAMERAS: u32 = 0x1C;
    pub const ANIMATION_BEGIN: u32 = 0x20;
    pub const ANIMATION_END: u32 = 0x28;
    pub const UP_VECTOR: u32 = 0x30;
    pub const TEXTURES: u32 = 0x3C;
    pub const SCENE_GRAPH_LIST: u32 = 0x40;
}

impl<'a> Layout<'a> for SceneInfo<'a> {
    const TYPE: &'static str = "igSceneInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: Info::fields(v)?,
            scene_graph: v.object(Self::SCENE_GRAPH, "_sceneGraph", "igNode")?,
            textures_deprecated: v.object_ref(Self::TEXTURES_DEPRECATED, "_texturesDeprecated")?,
            cameras: v.object_ref(Self::CAMERAS, "_cameras")?,
            animation_begin: i64(v, Self::ANIMATION_BEGIN, "_animationBegin")?,
            animation_end: i64(v, Self::ANIMATION_END, "_animationEnd")?,
            up_vector: v.floats(Self::UP_VECTOR, "_upVector")?,
            textures: v.object_ref(Self::TEXTURES, "_textures")?,
            scene_graph_list: v.object(Self::SCENE_GRAPH_LIST, "_sceneGraphList", "igNodeList")?,
        })
    }
}

/// `igSpatialNode`: size 0x28, a group with a culling tree over its
/// children. Bevy culls per entity, so the tree stays unfollowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialNode<'a> {
    pub group: Group<'a>,
    /// An `igSpatialTree`.
    pub spatial_tree: ObjectRef<'a>,
}

impl SpatialNode<'_> {
    pub const SPATIAL_TREE: u32 = 0x24;
}

impl<'a> Layout<'a> for SpatialNode<'a> {
    const TYPE: &'static str = "igSpatialNode";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            group: Group::fields(v)?,
            spatial_tree: v.object_ref(Self::SPATIAL_TREE, "_spatialTree")?,
        })
    }
}

/// `igTransform`: size 0x80, a group whose children sit in its matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform<'a> {
    pub group: Group<'a>,
    /// Row-vector affine, translation in row 3: a child's point p sits at
    /// p * m in the parent's frame.
    pub m: Mat44,
    pub target: i32,
    /// `igTransformSource3` keyframes on an animated one (40 on the disc),
    /// read but not played here.
    pub transform_input: ObjectRef<'a>,
    pub transform_input3: ObjectRef<'a>,
}

impl Transform<'_> {
    pub const M: u32 = 0x30;
    pub const TARGET: u32 = 0x70;
    pub const TRANSFORM_INPUT: u32 = 0x74;
    pub const TRANSFORM_INPUT3: u32 = 0x78;

    /// Either input set, so keyframes move it at runtime; how they combine
    /// with `m` isn't read here.
    pub fn is_animated(&self) -> bool {
        self.transform_input != ObjectRef::Null || self.transform_input3 != ObjectRef::Null
    }
}

impl<'a> Layout<'a> for Transform<'a> {
    const TYPE: &'static str = "igTransform";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            group: Group::fields(v)?,
            m: v.mat44(Self::M, "_m")?,
            target: v.i32(Self::TARGET, "_target")?,
            transform_input: v.object_ref(Self::TRANSFORM_INPUT, "_transformInput")?,
            transform_input3: v.object_ref(Self::TRANSFORM_INPUT3, "_transformInput3")?,
        })
    }
}

/// `igUserInfo`: size 0x28, a group carrying a property list. Every one in
/// a level scene is an `igSimpleUserInfo` (which adds nothing), and level
/// 213's seven are the skydome's groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserInfo<'a> {
    pub group: Group<'a>,
    /// An `igPropertyList`.
    pub property_list: ObjectRef<'a>,
}

impl UserInfo<'_> {
    pub const PROPERTY_LIST: u32 = 0x24;
}

impl<'a> Layout<'a> for UserInfo<'a> {
    const TYPE: &'static str = "igUserInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            group: Group::fields(v)?,
            property_list: v.object_ref(Self::PROPERTY_LIST, "_propertyList")?,
        })
    }
}
