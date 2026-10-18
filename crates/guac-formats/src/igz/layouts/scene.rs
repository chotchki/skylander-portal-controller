//! The scene-graph types between `tfbPhysicsBody._node` and the meshes.

use super::{DataList, Layout, Mat44, ObjectRef, View};
use crate::FormatError;

/// `igNode`: size 0x20, parent `igNamedObject`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node<'a> {
    pub name: Option<&'a str>,
    pub bound: ObjectRef<'a>,
    pub parent_list: ObjectRef<'a>,
    pub traversal_weight: i16,
    pub traversal_spu_weight: i16,
    pub handle: i32,
    pub flags: i32,
}

impl<'a> Node<'a> {
    pub const NAME: u32 = 0x08;
    pub const BOUND: u32 = 0x0C;
    pub const PARENT_LIST: u32 = 0x10;
    pub const TRAVERSAL_WEIGHT: u32 = 0x14;
    pub const TRAVERSAL_SPU_WEIGHT: u32 = 0x16;
    pub const HANDLE: u32 = 0x18;
    pub const FLAGS: u32 = 0x1C;
}

impl<'a> Layout<'a> for Node<'a> {
    const TYPE: &'static str = "igNode";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            bound: v.object_ref(Self::BOUND, "_bound")?,
            parent_list: v.object_ref(Self::PARENT_LIST, "_parentList")?,
            traversal_weight: v.i16(Self::TRAVERSAL_WEIGHT, "_traversalWeight")?,
            traversal_spu_weight: v.i16(Self::TRAVERSAL_SPU_WEIGHT, "_traversalSpuWeight")?,
            handle: v.i32(Self::HANDLE, "_handle")?,
            flags: v.i32(Self::FLAGS, "_flags")?,
        })
    }
}

/// `igGroup`: size 0x24, a node with children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group<'a> {
    pub node: Node<'a>,
    /// An `igNodeList`.
    pub child_list: Option<usize>,
}

impl Group<'_> {
    pub const CHILD_LIST: u32 = 0x20;
}

impl<'a> Layout<'a> for Group<'a> {
    const TYPE: &'static str = "igGroup";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            node: Node::fields(v)?,
            child_list: v.object(Self::CHILD_LIST, "_childList", "igNodeList")?,
        })
    }
}

/// `igNodeList`: an object list of nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeList<'a> {
    pub list: DataList<'a>,
    /// Indexes of the `igNode` objects.
    pub nodes: Vec<usize>,
}

impl<'a> Layout<'a> for NodeList<'a> {
    const TYPE: &'static str = "igNodeList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let nodes = v
            .elements(&list, Some("igNode"))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { list, nodes })
    }
}

/// `igAttrSet`: size 0x2C, a group with attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttrSet<'a> {
    pub group: Group<'a>,
    pub attributes: ObjectRef<'a>,
    pub trigger: bool,
}

impl AttrSet<'_> {
    pub const ATTRIBUTES: u32 = 0x24;
    pub const TRIGGER: u32 = 0x28;
}

impl<'a> Layout<'a> for AttrSet<'a> {
    const TYPE: &'static str = "igAttrSet";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            group: Group::fields(v)?,
            attributes: v.object_ref(Self::ATTRIBUTES, "_attributes")?,
            trigger: v.bool(Self::TRIGGER, "_trigger")?,
        })
    }
}

/// `igBlendMatrixSelect`: size 0xC0, the skin palette over its children.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendMatrixSelect<'a> {
    pub attr_set: AttrSet<'a>,
    pub skeleton_transform: Mat44,
    pub skeleton_inverse_transform: Mat44,
    pub use_previous: bool,
    /// An `igIntList`: palette slot to skeleton blend-matrix index.
    pub blend_matrix_indices: Option<usize>,
}

impl BlendMatrixSelect<'_> {
    pub const SKELETON_TRANSFORM: u32 = 0x30;
    pub const SKELETON_INVERSE_TRANSFORM: u32 = 0x70;
    pub const USE_PREVIOUS: u32 = 0xB0;
    pub const BLEND_MATRIX_INDICES: u32 = 0xB4;
}

impl<'a> Layout<'a> for BlendMatrixSelect<'a> {
    const TYPE: &'static str = "igBlendMatrixSelect";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr_set: AttrSet::fields(v)?,
            skeleton_transform: v.mat44(Self::SKELETON_TRANSFORM, "_skeletonTransform")?,
            skeleton_inverse_transform: v.mat44(
                Self::SKELETON_INVERSE_TRANSFORM,
                "_skeletonInverseTransform",
            )?,
            use_previous: v.bool(Self::USE_PREVIOUS, "_usePrevious")?,
            blend_matrix_indices: v.object(
                Self::BLEND_MATRIX_INDICES,
                "_blendMatrixIndices",
                "igIntList",
            )?,
        })
    }
}

/// `igIntList`: a data list of i32.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntList<'a> {
    pub list: DataList<'a>,
    pub values: Vec<i32>,
}

impl<'a> Layout<'a> for IntList<'a> {
    const TYPE: &'static str = "igIntList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let values = v
            .list_bytes(&list, 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_be_bytes(*c))
            .collect();
        Ok(Self { list, values })
    }
}

/// `igActor2`: size 0x28, the hero's scene-graph root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor2<'a> {
    pub group: Group<'a>,
    /// An `igAnimationSystem2`.
    pub animation_system: Option<usize>,
}

impl Actor2<'_> {
    pub const ANIMATION_SYSTEM: u32 = 0x24;
}

impl<'a> Layout<'a> for Actor2<'a> {
    const TYPE: &'static str = "igActor2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            group: Group::fields(v)?,
            animation_system: v.object(
                Self::ANIMATION_SYSTEM,
                "_animationSystem",
                "igAnimationSystem2",
            )?,
        })
    }
}

/// `tfbRuntimeTechniqueInstance`: size 0x38, a drawable leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeTechniqueInstance<'a> {
    pub node: Node<'a>,
    pub ti_flags: u32,
    /// Holds the `_techniqueIndex` bitfield.
    pub properties: u32,
    /// An `igEdgeGeometryAttr` (2,243 of 2,243 in the character files).
    pub geom_attr: Option<usize>,
    pub material_anim: ObjectRef<'a>,
    pub material_pass: ObjectRef<'a>,
    /// An import in files.
    pub effect: ObjectRef<'a>,
}

impl RuntimeTechniqueInstance<'_> {
    pub const TI_FLAGS: u32 = 0x20;
    pub const PROPERTIES: u32 = 0x24;
    pub const GEOM_ATTR: u32 = 0x28;
    pub const MATERIAL_ANIM: u32 = 0x2C;
    pub const MATERIAL_PASS: u32 = 0x30;
    pub const EFFECT: u32 = 0x34;
}

impl<'a> Layout<'a> for RuntimeTechniqueInstance<'a> {
    const TYPE: &'static str = "tfbRuntimeTechniqueInstance";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            node: Node::fields(v)?,
            ti_flags: v.u32(Self::TI_FLAGS, "_tiFlags")?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            geom_attr: v.object(Self::GEOM_ATTR, "_geomAttr", "igEdgeGeometryAttr")?,
            material_anim: v.object_ref(Self::MATERIAL_ANIM, "_materialAnim")?,
            material_pass: v.object_ref(Self::MATERIAL_PASS, "_materialPass")?,
            effect: v.object_ref(Self::EFFECT, "_effect")?,
        })
    }
}
