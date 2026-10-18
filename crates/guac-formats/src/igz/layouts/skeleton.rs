//! The Alchemy skeleton the skin palette indexes, and the animation system
//! (`tfbPhysicsCombinerLink` on heroes) that holds it.

use super::{DataList, Layout, Mat44, View, be_f32s};
use crate::igz::MemoryRef;
use crate::{ErrorKind, FormatError};

/// `igSkeleton2`: size 0x18, parent `igNamedObject`.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton2<'a> {
    pub name: Option<&'a str>,
    /// An `igSkeletonBoneList`.
    pub bone_list: Option<usize>,
    pub inverse_joint_array: Option<MemoryRef<'a>>,
    /// The inverse joint matrices, indexed by `_blendMatrixIndex`.
    pub inverse_joints: Vec<Mat44>,
}

impl Skeleton2<'_> {
    pub const NAME: u32 = 0x08;
    pub const BONE_LIST: u32 = 0x0C;
    pub const INVERSE_JOINT_ARRAY: u32 = 0x10;
}

impl<'a> Layout<'a> for Skeleton2<'a> {
    const TYPE: &'static str = "igSkeleton2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let name = v.string(Self::NAME, "_name")?;
        let bone_list = v.object(Self::BONE_LIST, "_boneList", "igSkeletonBoneList")?;
        let inverse_joint_array = v.memory_ref(Self::INVERSE_JOINT_ARRAY, "_inverseJointArray")?;
        let bytes = inverse_joint_array.map_or(&[][..], |m| m.data);
        if !bytes.len().is_multiple_of(64) {
            return Err(v.igz().err_at(
                inverse_joint_array.map_or_else(|| v.igz().objects()[v.index()].at, |m| m.at),
                "_inverseJointArray",
                ErrorKind::Unsupported {
                    value: bytes.len() as u64,
                },
            ));
        }
        let floats: Vec<f32> = be_f32s(bytes).collect();
        let inverse_joints = floats
            .as_chunks::<16>()
            .0
            .iter()
            .map(|m| {
                let row = |r: usize| [m[4 * r], m[4 * r + 1], m[4 * r + 2], m[4 * r + 3]];
                [row(0), row(1), row(2), row(3)]
            })
            .collect();
        Ok(Self {
            name,
            bone_list,
            inverse_joint_array,
            inverse_joints,
        })
    }
}

/// `igSkeletonBoneList`: an object list of bones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonBoneList<'a> {
    pub list: DataList<'a>,
    /// Indexes of the `igSkeletonBone` objects, in bone order.
    pub bones: Vec<usize>,
}

impl<'a> Layout<'a> for SkeletonBoneList<'a> {
    const TYPE: &'static str = "igSkeletonBoneList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let bones = v
            .elements(&list, Some("igSkeletonBone"))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { list, bones })
    }
}

/// `igSkeletonBone`: size 0x20, parent `igNamedObject`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkeletonBone<'a> {
    pub name: Option<&'a str>,
    /// -1 on the root.
    pub parent_index: i32,
    /// Index into `_inverseJointArray`, -1 for an unskinned bone.
    pub blend_matrix_index: i32,
    /// Parent-local.
    pub translation: [f32; 3],
}

impl SkeletonBone<'_> {
    pub const NAME: u32 = 0x08;
    pub const PARENT_INDEX: u32 = 0x0C;
    pub const BLEND_MATRIX_INDEX: u32 = 0x10;
    pub const TRANSLATION: u32 = 0x14;
}

impl<'a> Layout<'a> for SkeletonBone<'a> {
    const TYPE: &'static str = "igSkeletonBone";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            parent_index: v.i32(Self::PARENT_INDEX, "_parentIndex")?,
            blend_matrix_index: v.i32(Self::BLEND_MATRIX_INDEX, "_blendMatrixIndex")?,
            translation: v.floats(Self::TRANSLATION, "_translation")?,
        })
    }
}

/// `igAnimationSystem2`: size 0x24, parent `igNamedObject`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationSystem2<'a> {
    pub name: Option<&'a str>,
    /// An `igSkeleton2`.
    pub skeleton: Option<usize>,
    /// Runtime; null in files.
    pub concatenated_bone_matrix_array: Option<MemoryRef<'a>>,
    /// Runtime; null in files.
    pub concatenated_blend_matrix_array: Option<MemoryRef<'a>>,
    /// INT_MIN in files.
    pub matrix_array_time: i32,
}

impl<'a> AnimationSystem2<'a> {
    pub const NAME: u32 = 0x08;
    pub const SKELETON: u32 = 0x0C;
    pub const CONCATENATED_BONE_MATRIX_ARRAY: u32 = 0x10;
    pub const CONCATENATED_BLEND_MATRIX_ARRAY: u32 = 0x18;
    pub const MATRIX_ARRAY_TIME: u32 = 0x20;
}

impl<'a> Layout<'a> for AnimationSystem2<'a> {
    const TYPE: &'static str = "igAnimationSystem2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            skeleton: v.object(Self::SKELETON, "_skeleton", "igSkeleton2")?,
            concatenated_bone_matrix_array: v.memory_ref(
                Self::CONCATENATED_BONE_MATRIX_ARRAY,
                "_concatenatedBoneMatrixArray",
            )?,
            concatenated_blend_matrix_array: v.memory_ref(
                Self::CONCATENATED_BLEND_MATRIX_ARRAY,
                "_concatenatedBlendMatrixArray",
            )?,
            matrix_array_time: v.i32(Self::MATRIX_ARRAY_TIME, "_matrixArrayTime")?,
        })
    }
}

/// `tfbPhysicsCombiner`: size 0x38, parent `igAnimationSystem2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsCombiner<'a> {
    pub system: AnimationSystem2<'a>,
    /// i32 elements; null in files.
    pub joint_index_array: Option<MemoryRef<'a>>,
    /// Matrix elements; null in files.
    pub anim_array: Option<MemoryRef<'a>>,
    pub id_code_padding: u32,
}

impl PhysicsCombiner<'_> {
    pub const JOINT_INDEX_ARRAY: u32 = 0x24;
    pub const ANIM_ARRAY: u32 = 0x2C;
    pub const ID_CODE_PADDING: u32 = 0x34;
}

impl<'a> Layout<'a> for PhysicsCombiner<'a> {
    const TYPE: &'static str = "tfbPhysicsCombiner";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            system: AnimationSystem2::fields(v)?,
            joint_index_array: v.memory_ref(Self::JOINT_INDEX_ARRAY, "_jointIndexArray")?,
            anim_array: v.memory_ref(Self::ANIM_ARRAY, "_animArray")?,
            id_code_padding: v.u32(Self::ID_CODE_PADDING, "_idCodePadding")?,
        })
    }
}

/// `tfbPhysicsCombinerLink`: size 0x3C, the hero's `_combinerPrototype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsCombinerLink<'a> {
    pub combiner: PhysicsCombiner<'a>,
    /// A 4-byte runtime id struct.
    pub id_code: u32,
}

impl PhysicsCombinerLink<'_> {
    pub const ID_CODE: u32 = 0x38;
}

impl<'a> Layout<'a> for PhysicsCombinerLink<'a> {
    const TYPE: &'static str = "tfbPhysicsCombinerLink";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            combiner: PhysicsCombiner::fields(v)?,
            id_code: v.u32(Self::ID_CODE, "_idCode")?,
        })
    }
}
