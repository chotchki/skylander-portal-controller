//! Field layouts of the decode gate's Alchemy types (openspec AC.4), of
//! the script, sprite and language types the roster reads (AE.1) and of the
//! material attributes the model stage reads (AE.6), tagged [`TAG`], and
//! typed readers over [`Igz`]'s object graph.
//!
//! Method: static EBOOT registry tables cross-checked against the disc; no
//! statistical fallback ([`METHOD`]). The registry is the source of every
//! offset here; the disc checks were MTSZ sizes, which words the fixups
//! mark, object-ref targets and value relations (sizes vs buffers, bind pose
//! vs inverse joints) over the 158 character `level.bld` files.
//!
//! A reader refuses an object that isn't its type or a subtype listed in
//! [`TYPES`], one whose MTSZ size differs from the registry's, and one with a
//! fixup on a word its layout doesn't declare as a reference. Fields that
//! point outside the gate's types stay unfollowed as [`ObjectRef`].

mod attrs;
mod edge;
mod hero;
mod image;
mod level;
mod material;
mod ops;
mod placement;
mod scene;
mod script;
mod skeleton;
mod sprite;
mod tfb;
mod world;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

pub use attrs::{
    AlphaFunctionAttr, AlphaStateAttr, BlendFunctionAttr, BlendStateAttr, CullFaceAttr,
    DepthWriteStateAttr, DestinationAlphaAttr, FlipBookUvAnimData, MaterialAnim, MaterialAnimTrack,
    MaterialAnimTrackList, MaterialTintAnimData, MaterialUvAnimTrack, PixelShaderVariantMaskAttr,
    ScrollUvAnimData, ShaderConstantIntAttr, ShaderConstantVec4fAttr, TransformUvAnimData,
    UnsignedIntList, VariantMask, VertexShaderVariantMaskAttr,
};
pub use edge::{Attr, EdgeGeometry, EdgeGeometryAttr, EdgeGeometrySegment};
pub use hero::{Clip, Hero, MaterialLists, Mesh, Palette, Passes};
pub use image::{Image2, KNOWN_FORMATS, METAIMAGES};
pub use level::{SceneInfo, SpatialNode, Transform, UserInfo};
pub use material::{AlphaTextureUnitIdAttr, AttrList, TextureAttr2, TextureBindAttr2};
pub use ops::{
    BRANCH_END, NO_OPERATOR, OpChangeMembership, OpCode, OpCodeList, OpDefineMacro, OpLoopValue,
    OpMacroInterface, OpMacroParameter, OpUseMacro, RhsReferenceStack, RhsValueStack, ScriptInfo,
    Stack, ValueRhsVariant, Variable,
};
pub use placement::{
    ActorInfo, ActorParameters, ActorWaypoint, ActorWaypointList, BoneAttachInfo,
    BoneAttachInfoList, CollisionInfo, LightInfo, ModelParameters, ParticleInfo, Placement, Shape,
};
pub use scene::{
    Actor2, AttrSet, BlendMatrixSelect, Group, IntList, Node, NodeList, RuntimeTechniqueInstance,
};
pub use script::{
    DirectionalLightInfo, EulerTransform, LanguagePackInfo, Rgba8, RootList, ScriptSet,
    ScriptSetList, StringInfo, ValueInfo,
};
pub use skeleton::{
    AnimationSystem2, PhysicsCombiner, PhysicsCombinerLink, Skeleton2, SkeletonBone,
    SkeletonBoneList,
};
pub use sprite::{AbstractPlacement, AnimatableTextureDataInfo, SpriteInfo};
pub use tfb::{
    AnimationInfo, AnimationInfoList, BodyInfo, BuildDataInfo, HAVOK_MAGIC, Info, LIP_SYNC_MAGIC,
    ModelInfo, Payload, PhysicsAnimation, PhysicsBody, PhysicsModel, TfbAnimationInfo,
};
pub use world::{PhysicsWorld, WorldInfo};

use super::{ExternalId, ExternalName, Igz, Location, MemoryHandle, MemoryRef, Object, Ref};
use crate::{ErrorKind, FormatError};

/// The build these layouts were recovered from.
pub const TAG: &str = "IGZ v6 / BLUS30968 01.00";
/// How they were recovered (AC.4, design decision 4).
pub const METHOD: &str =
    "static EBOOT registry tables cross-checked against the disc; no statistical fallback";

/// A 4x4 f32 matrix as stored: rows, row-vector affine (translation in row 3).
pub type Mat44 = [[f32; 4]; 4];

/// A registered type: its parent and its instance size, per the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeDef {
    pub name: &'static str,
    pub parent: Option<&'static str>,
    pub size: u32,
}

const fn def(name: &'static str, parent: &'static str, size: u32) -> TypeDef {
    TypeDef {
        name,
        parent: Some(parent),
        size,
    }
}

/// The gate's types, the roster's (AE.1) and every type on their parent
/// chains.
pub const TYPES: &[TypeDef] = &[
    TypeDef {
        name: "igObject",
        parent: None,
        size: 0x8,
    },
    def("igNamedObject", "igObject", 0xC),
    def("igContainer", "igObject", 0x8),
    def("igDataList", "igContainer", 0x18),
    def("igObjectList", "igDataList", 0x18),
    def("igIntList", "igDataList", 0x18),
    def("igUnsignedIntList", "igDataList", 0x18),
    // Textures.
    def("igImage2", "igNamedObject", 0x60),
    // Edge geometry.
    def("igPS3EdgeGeometrySegment", "igObject", 0xB0),
    def("igPS3EdgeGeometrySegmentList", "igObjectList", 0x18),
    def("igPS3EdgeGeometry", "igPS3EdgeGeometrySegmentList", 0x1C),
    def("igAttr", "igObject", 0x10),
    def("igVisualAttribute", "igAttr", 0x10),
    def("igDrawableAttr", "igVisualAttribute", 0x10),
    def("igEdgeGeometryAttr", "igDrawableAttr", 0x14),
    // Materials.
    def("igAttrList", "igObjectList", 0x18),
    def("igTextureBindAttr2", "igVisualAttribute", 0x18),
    def("tfbAlphaTextureUnitIDAttr", "igTextureBindAttr2", 0x20),
    def("igTextureAttr2", "igVisualAttribute", 0x40),
    // Skeleton and skin palette.
    def("igSkeleton2", "igNamedObject", 0x18),
    def("igSkeletonBoneList", "igObjectList", 0x18),
    def("igSkeletonBone", "igNamedObject", 0x20),
    def("igAnimationSystem2", "igNamedObject", 0x24),
    def("tfbPhysicsCombiner", "igAnimationSystem2", 0x38),
    def("tfbPhysicsCombinerLink", "tfbPhysicsCombiner", 0x3C),
    // Scene graph.
    def("igNode", "igNamedObject", 0x20),
    def("igGroup", "igNode", 0x24),
    def("igAttrSet", "igGroup", 0x2C),
    def("igBlendMatrixSelect", "igAttrSet", 0xC0),
    def("igActor2", "igGroup", 0x28),
    def("igNodeList", "igObjectList", 0x18),
    def("tfbRuntimeTechniqueInstance", "igNode", 0x38),
    // Groups the character files hold, read in levels' scenes (AG.3).
    def("igTransform", "igGroup", 0x80),
    def("igSpatialNode", "igGroup", 0x28),
    def("igUserInfo", "igGroup", 0x28),
    def("igSimpleUserInfo", "igUserInfo", 0x28),
    // Havok wrappers and the hero's model.
    def("igReferenceResolver", "igNamedObject", 0xC),
    def("igInfo", "igReferenceResolver", 0x14),
    def("tfbInfo", "igInfo", 0x14),
    def("tfbComplexDataInfo", "tfbInfo", 0x14),
    def("tfbBuildDataInfo", "tfbComplexDataInfo", 0x20),
    def("tfbAnimationInfo", "tfbBuildDataInfo", 0x28),
    def("tfbPhysicsAnimation", "tfbAnimationInfo", 0x2C),
    // Clip choosers `_tfbAnimation` can name, listed for is_a only.
    def("animationNetwork", "tfbAnimationInfo", 0x2C),
    def("randomAnimationNetwork", "animationNetwork", 0x38),
    def(
        "physicsRandomAnimationNetwork",
        "randomAnimationNetwork",
        0x3C,
    ),
    def("tfbBodyInfo", "tfbBuildDataInfo", 0x3C),
    def("tfbPhysicsBody", "tfbBodyInfo", 0x40),
    def("tfbModelInfo", "tfbComplexDataInfo", 0x2C),
    def("tfbPhysicsModel", "tfbModelInfo", 0x30),
    // A level's world: its collision packfile and its scene (AG.2).
    def("tfbWorldInfo", "tfbBuildDataInfo", 0x28),
    def("tfbPhysicsWorld", "tfbWorldInfo", 0x2C),
    // A level's scene and placements (AG.3).
    def("igSceneInfo", "igInfo", 0x48),
    def("Placement", "AbstractPlacement", 0xD8),
    def("tfbActorInfo", "Placement", 0xF8),
    def("tfbParticleInfo", "Placement", 0xE0),
    def("tfbLightInfo", "Placement", 0x10C),
    def("CollisionInfo", "MatrixMeasurement", 0x58),
    def("ModelPhysics", "igObject", 0xC),
    def("ModelParameters", "ModelPhysics", 0x48),
    def("ActorParameters", "ModelParameters", 0x4C),
    def("ActorWaypoint", "AbstractAttachNode", 0x38),
    def("ActorWaypointList", "ScriptObjectList", 0x18),
    // The clip table; only AnimationInfo's own fields are read.
    def("ScriptObjectList", "igObjectList", 0x18),
    def("AnimationStack", "ScriptObjectList", 0x18),
    def("AnimationInfoList", "AnimationStack", 0x18),
    def("tfbScriptObject", "igNamedObject", 0x18),
    def("Sequence", "tfbScriptObject", 0x28),
    def("ClonedSequence", "Sequence", 0x30),
    def("AbstractAnimSequence", "ClonedSequence", 0x30),
    def("AnimationInfo", "AbstractAnimSequence", 0x44),
    // The roster's script objects (AE.1): names, numbers and sets.
    def("StringInfo", "tfbScriptObject", 0x28),
    def("AbstractScriptVariant", "tfbScriptObject", 0x18),
    def("AbstractValueMeasurement", "AbstractScriptVariant", 0x18),
    def(
        "AbstractNumberMeasurement",
        "AbstractValueMeasurement",
        0x18,
    ),
    def("ValueInfo", "AbstractNumberMeasurement", 0x24),
    def("SetVariant", "AbstractScriptVariant", 0x18),
    def("ScriptSet", "SetVariant", 0x1C),
    def("ScriptGroupStack", "ScriptObjectList", 0x1C),
    def("ScriptSetList", "ScriptGroupStack", 0x24),
    def("DirectionalLightInfo", "tfbScriptObject", 0x40),
    def("LanguagePackInfo", "igInfo", 0x20),
    // Sprites and their textures.
    def("VectorMeasurement", "tfbScriptObject", 0x18),
    def("PositionMeasurement", "VectorMeasurement", 0x18),
    def("MatrixMeasurement", "PositionMeasurement", 0x18),
    def("AbstractAttachNode", "MatrixMeasurement", 0x1C),
    def("AbstractPlacement", "AbstractAttachNode", 0xA0),
    // A model's attach points (AE.23).
    def("BoneAttachInfo", "AbstractAttachNode", 0x3C),
    def("BoneAttachInfoList", "ScriptObjectList", 0x18),
    def("tfbSpriteInfo", "AbstractPlacement", 0xE4),
    def("AnimatableTextureDataInfo", "tfbBuildDataInfo", 0x34),
    // Script ops and operand stacks, what the toy table's reader walks (AE.2).
    def("tfbScriptInfo", "tfbComplexDataInfo", 0x1C),
    def("OpCodeList", "ScriptObjectList", 0x18),
    // A script's `_masterVarList`: the variables a placement's locals are
    // built from (AH.16).
    def("OpCreateVariableList", "igObjectList", 0x18),
    def("OpCode", "AbstractScriptVariant", 0x1C),
    def("OpBranch", "OpCode", 0x1C),
    def("OpAbstractBehavior", "OpBranch", 0x1C),
    def("OpMacroInterface", "OpAbstractBehavior", 0x20),
    def("OpAbstractFlow", "OpBranch", 0x1C),
    def("OpFlowMacro", "OpAbstractFlow", 0x1C),
    def("OpFlowBuiltInBehavior", "OpAbstractFlow", 0x1C),
    def("OpUseMacro", "OpBranch", 0x24),
    def("OpAbstractDefinition", "OpBranch", 0x30),
    def("OpDefineMacro", "OpAbstractDefinition", 0x38),
    def("OpDefineMacroSpecialization", "OpDefineMacro", 0x40),
    def("OpAbstractCreateVariable", "OpCode", 0x2C),
    def("OpCreateVariable", "OpAbstractCreateVariable", 0x2C),
    def("OpParameter", "OpAbstractCreateVariable", 0x38),
    def("OpMacroParameter", "OpParameter", 0x48),
    def("OpChangeMembership", "OpCode", 0x28),
    def("OpLoop", "OpBranch", 0x20),
    def("OpLoopValue", "OpLoop", 0x28),
    def("ValueStack", "ScriptObjectList", 0x1C),
    def("RHSValueStack", "ValueStack", 0x28),
    def("RHSReferenceStack", "RHSValueStack", 0x2C),
    def("SetStack", "ScriptObjectList", 0x18),
    def("ReferenceStack", "ScriptObjectList", 0x18),
    TypeDef {
        name: "ValueRHSVariant",
        parent: Some("igObject"),
        size: 0x1C,
    },
    // The ops and the save param class the progression readers follow (AH.2).
    def("OpSetValue", "OpCode", 0x24),
    def("OpIncValue", "OpCode", 0x20),
    def("OpAbstractCheckValue", "OpBranch", 0x28),
    def("OpCheckValue", "OpAbstractCheckValue", 0x28),
    def("OpSetReference", "OpCode", 0x28),
    def("OpDefineMacroStack", "ScriptObjectList", 0x18),
    def("StructureVariant", "AbstractScriptVariant", 0x18),
    def("ScriptStructure", "StructureVariant", 0x20),
    // The rest of the 44 op classes on the disc and their operand stacks
    // (AH.13, `crate::script`).
    def("OpTopLevelBehavior", "OpAbstractBehavior", 0x1C),
    def("OpPreScript", "OpTopLevelBehavior", 0x1C),
    def("OpUserBehavior", "OpTopLevelBehavior", 0x24),
    def("OpStartUp", "OpAbstractBehavior", 0x1C),
    def("OpShutDown", "OpAbstractBehavior", 0x1C),
    def("OpMacroSection", "OpAbstractBehavior", 0x1C),
    def("OpStructureSection", "OpAbstractBehavior", 0x1C),
    def("OpFlowBehavior", "OpAbstractFlow", 0x1C),
    def("OpFlowLoop", "OpAbstractFlow", 0x1C),
    def("OpAbstractControl", "OpBranch", 0x28),
    def("OpSpawn", "OpAbstractControl", 0x30),
    def("OpControl", "OpAbstractControl", 0x2C),
    def("OpCheckReference", "OpBranch", 0x2C),
    def("OpSlideValue", "OpBranch", 0x34),
    def("OpRemove", "OpCode", 0x20),
    def("OpDisplace", "OpCode", 0x30),
    def("OpStartSequence", "OpBranch", 0x28),
    def("OpStopSequence", "OpCode", 0x20),
    def("OpFindVariable", "OpBranch", 0x3C),
    def("OpSetBehavior", "OpCode", 0x24),
    def("OpForEach", "OpLoop", 0x34),
    def("OpFindSubSet", "OpAbstractCheckValue", 0x30),
    def("OpCheckMembership", "OpBranch", 0x30),
    def("OpDefineStructure", "OpAbstractDefinition", 0x30),
    def("OpReset", "OpCode", 0x20),
    def("OpCheckFOV", "OpBranch", 0x3C),
    def("OpDecValue", "OpCode", 0x20),
    def("OpTurnTo", "OpBranch", 0x28),
    def("OpAbstractMove", "OpBranch", 0x30),
    def("OpMoveTo", "OpAbstractMove", 0x30),
    def("OpMoveFrom", "OpAbstractMove", 0x30),
    def("OpTeleportTo", "OpCode", 0x28),
    def("PositionStack", "ScriptObjectList", 0x18),
    def("VectorStack", "ScriptObjectList", 0x18),
    def("AbstractPlacementList", "ScriptObjectList", 0x18),
    // A tfbmulti2 material's attributes and its UV animation (AE.6).
    def("igPixelShaderVariantMaskAttr", "igVisualAttribute", 0x18),
    def("igVertexShaderVariantMaskAttr", "igVisualAttribute", 0x18),
    def("tfbShaderConstantAttr", "igVisualAttribute", 0x10),
    def("tfbShaderConstantVec4fAttr", "tfbShaderConstantAttr", 0x20),
    def(
        "tfbMulti2FixedAlphaValuesAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer1ExtraValuesAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer2ExtraValuesAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer2BlendFactorsAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer3BlendFactorsAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer4BlendFactorsAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def(
        "tfbMulti2Layer4ValuesAttr",
        "tfbShaderConstantVec4fAttr",
        0x20,
    ),
    def("tfbShaderConstantIntAttr", "tfbShaderConstantAttr", 0x14),
    def("tfbMaterialTintAttr", "tfbShaderConstantIntAttr", 0x14),
    def("igBlendStateAttr", "igVisualAttribute", 0x10),
    def("igBlendFunctionAttr", "igVisualAttribute", 0x34),
    def("igDepthWriteStateAttr", "igVisualAttribute", 0x10),
    def("igCullFaceAttr", "igVisualAttribute", 0x14),
    def("igAlphaStateAttr", "igVisualAttribute", 0x10),
    def("igAlphaFunctionAttr", "igVisualAttribute", 0x18),
    def("tfbDestinationAlphaAttr", "igVisualAttribute", 0x18),
    def("tfbMaterialAnim", "igObject", 0x10),
    def("tfbMaterialAnimTrackList", "igObjectList", 0x18),
    def("tfbMaterialAnimTrack", "igObject", 0x24),
    def("tfbMaterialUVAnimTrack", "tfbMaterialAnimTrack", 0x28),
    def("tfbScrollUVAnimTrack", "tfbMaterialUVAnimTrack", 0x28),
    def("tfbTransformUVAnimTrack", "tfbMaterialUVAnimTrack", 0x28),
    def("tfbFlipBookUVAnimTrack", "tfbMaterialUVAnimTrack", 0x28),
    def("tfbMaterialTintAnimTrack", "tfbMaterialAnimTrack", 0x24),
    def("tfbMaterialAnimData", "igObject", 0x8),
    def("tfbMaterialUVAnimData", "tfbMaterialAnimData", 0x8),
    def("tfbScrollUVAnimData", "tfbMaterialUVAnimData", 0x34),
    // The other kinds' data: a character's reader refuses them, a level's
    // reads them (AG.15).
    def("tfbTransformUVAnimData", "tfbMaterialUVAnimData", 0x40),
    def("tfbFlipBookUVAnimData", "tfbMaterialUVAnimData", 0x1C),
    def("tfbMaterialTintAnimData", "tfbMaterialAnimData", 0x18),
];

/// Types whose files carry an MTSZ 4 over the registry size (AC.4's open
/// question 2, settled by AC.14: the loader reads sections in place, so the
/// 4 bytes sit in memory as on disc, past the layout and never marked): the
/// script stack AE.2 reads and the four render states AE.6 reads.
pub const FILE_PADDED: &[(&str, u32)] = &[
    ("RHSReferenceStack", 0x30),
    ("igBlendStateAttr", 0x14),
    ("igDepthWriteStateAttr", 0x14),
    ("igCullFaceAttr", 0x18),
    ("igAlphaStateAttr", 0x14),
];

/// The MTSZ a file gives `name`: its registry size, or the padded one.
pub fn file_size(name: &str) -> Option<u32> {
    let registered = type_def(name)?.size;
    Some(
        FILE_PADDED
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(registered, |(_, size)| *size),
    )
}

pub fn type_def(name: &str) -> Option<&'static TypeDef> {
    TYPES.iter().find(|t| t.name == name)
}

/// Whether `name` is `base` or a subtype of it, per [`TYPES`].
pub fn is_a(name: &str, base: &str) -> bool {
    let mut current = type_def(name);
    // Bounded by the table, so a cycle can't hang it.
    for _ in 0..TYPES.len() {
        let Some(t) = current else { return false };
        if t.name == base {
            return true;
        }
        current = t.parent.and_then(type_def);
    }
    false
}

/// A type with a recovered layout, read from one object of an [`Igz`].
pub trait Layout<'a>: Sized {
    const TYPE: &'static str;

    /// Reads this type's fields, parents' first. Use [`Layout::read`].
    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError>;

    /// Reads `igz.objects()[object]`, which must be a [`Self::TYPE`].
    fn read(igz: &Igz<'a>, object: usize) -> Result<Self, FormatError> {
        let mut v = View::new(igz, object, Self::TYPE)?;
        let out = Self::fields(&mut v)?;
        v.finish()?;
        Ok(out)
    }
}

/// Every object that is a `T`, with its index, read.
pub fn read_all<'s, 'a, T: Layout<'a> + 's>(
    igz: &'s Igz<'a>,
) -> impl Iterator<Item = (usize, Result<T, FormatError>)> + 's {
    igz.objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| is_a(igz.type_name(o), T::TYPE))
        .map(|(i, _)| (i, T::read(igz, i)))
}

/// An EXID import, as `_format` and `_effect` hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import {
    pub index: u32,
    pub id: ExternalId,
}

/// A memory handle field: its TMHN index and block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle<'a> {
    pub index: u32,
    pub block: MemoryHandle<'a>,
}

/// An object ref this crate doesn't follow: its target type is outside the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRef<'a> {
    Null,
    Object(usize),
    Import(Import),
    /// A kind-0x10 named external ref.
    Named {
        index: u32,
        name: ExternalName<'a>,
    },
}

/// A script object's type field (`_type`, `_editType`, a variable's
/// container and contents types): an object ref that can also be a kind-0x10
/// word without the index flag, which on Giants indexes the file's EXNM
/// table, every such entry in the `metaobject` namespace (aj-vm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRef<'a> {
    Null,
    Object(usize),
    Import(Import),
    /// A flagged kind-0x10 word.
    Named {
        index: u32,
        name: ExternalName<'a>,
    },
    /// An unflagged kind-0x10 word and the EXNM entry it indexes, `None`
    /// past the table's end.
    Indexed {
        index: u32,
        name: Option<ExternalName<'a>>,
    },
}

impl TypeRef<'_> {
    /// The `metaobject` class it names, if it names one.
    pub fn metaobject(&self) -> Option<&str> {
        let name = match self {
            TypeRef::Named { name, .. } => name,
            TypeRef::Indexed {
                name: Some(name), ..
            } => name,
            _ => return None,
        };
        (name.namespace == "metaobject").then_some(name.name)
    }
}

/// `igVectorMetaField`: a count, then a memory ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vector<'a> {
    pub count: u32,
    pub data: Option<MemoryRef<'a>>,
}

/// `igDataList`: count, capacity, then the elements' memory ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataList<'a> {
    pub count: i32,
    pub capacity: i32,
    pub data: Option<MemoryRef<'a>>,
}

impl<'a> DataList<'a> {
    pub const COUNT: u32 = 0x08;
    pub const CAPACITY: u32 = 0x0C;
    pub const DATA: u32 = 0x10;

    pub(crate) fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            count: v.i32(Self::COUNT, "_count")?,
            capacity: v.i32(Self::CAPACITY, "_capacity")?,
            data: v.memory_ref(Self::DATA, "_data")?,
        })
    }
}

/// `igObjectList`, elements unfollowed (`None` for a null slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectList<'a> {
    pub list: DataList<'a>,
    pub elements: Vec<Option<usize>>,
}

impl<'a> Layout<'a> for ObjectList<'a> {
    const TYPE: &'static str = "igObjectList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let elements = v.elements(&list, None)?;
        Ok(Self { list, elements })
    }
}

/// A view of one object, reading fields at registry offsets and recording
/// which words it read as references.
pub struct View<'f, 'a> {
    igz: &'f Igz<'a>,
    index: usize,
    object: Object,
    /// The layout's size: fields and the fixup check stop here.
    extent: u32,
    claimed: Vec<u32>,
}

impl<'f, 'a> View<'f, 'a> {
    fn new(igz: &'f Igz<'a>, index: usize, ty: &'static str) -> Result<Self, FormatError> {
        let Some(&object) = igz.objects().get(index) else {
            return Err(FormatError::new(
                "IGZ",
                ty,
                0,
                ErrorKind::OutOfRange {
                    value: index as u64,
                    limit: igz.objects().len() as u64,
                },
            ));
        };
        let found = igz.type_name(&object);
        if !is_a(found, ty) {
            return Err(igz.err_at(
                object.at,
                ty,
                ErrorKind::WrongType {
                    expected: ty,
                    found: found.to_owned(),
                },
            ));
        }
        // is_a found it, so the object's type is in the table.
        let registered = type_def(found).map_or(0, |t| t.size);
        if object.size != file_size(found).unwrap_or(registered) {
            return Err(igz.err_at(
                object.at,
                "MTSZ",
                ErrorKind::Unsupported {
                    value: object.size.into(),
                },
            ));
        }
        // A padded type's extra bytes hold no fixup.
        if let Some(pad) = object.at.offset_by(registered)
            && let Some(&(at, marker)) = igz.markers_in(pad, object.size - registered).first()
        {
            return Err(igz.err_at(
                at,
                marker.field(),
                ErrorKind::Unsupported {
                    value: registered.into(),
                },
            ));
        }
        let extent = type_def(ty).map_or(0, |t| t.size);
        Ok(Self {
            igz,
            index,
            object,
            extent,
            claimed: Vec::new(),
        })
    }

    pub fn igz(&self) -> &'f Igz<'a> {
        self.igz
    }

    /// The object's index in [`Igz::objects`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// Every fixup inside the layout must sit on a word read as a reference.
    fn finish(self) -> Result<(), FormatError> {
        for &(at, marker) in self.igz.markers_in(self.object.at, self.extent) {
            let offset = at.offset() - self.object.at.offset();
            if !self.claimed.contains(&offset) {
                return Err(self.igz.err_at(
                    at,
                    marker.field(),
                    ErrorKind::Unsupported {
                        value: offset.into(),
                    },
                ));
            }
        }
        Ok(())
    }

    fn at(&self, offset: u32, len: u32, field: &'static str) -> Result<Location, FormatError> {
        let inside = offset
            .checked_add(len)
            .is_some_and(|end| end <= self.extent);
        match self.object.field(offset) {
            Some(at) if inside => Ok(at),
            _ => Err(self.igz.err_at(
                self.object.at,
                field,
                ErrorKind::OutOfRange {
                    value: offset.into(),
                    limit: self.extent.into(),
                },
            )),
        }
    }

    fn claim(&mut self, offset: u32) {
        self.claimed.push(offset);
    }

    pub fn u8(&mut self, offset: u32, field: &'static str) -> Result<u8, FormatError> {
        let at = self.at(offset, 1, field)?;
        Ok(self.igz.bytes(at, 1, field)?[0])
    }

    /// `igBoolMetaField`: one byte, 0 or 1.
    pub fn bool(&mut self, offset: u32, field: &'static str) -> Result<bool, FormatError> {
        match self.u8(offset, field)? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(self.igz.err_at(
                self.at(offset, 1, field)?,
                field,
                ErrorKind::Unsupported {
                    value: value.into(),
                },
            )),
        }
    }

    pub fn u16(&mut self, offset: u32, field: &'static str) -> Result<u16, FormatError> {
        let at = self.at(offset, 2, field)?;
        self.igz.u16_at(at, field)
    }

    pub fn i16(&mut self, offset: u32, field: &'static str) -> Result<i16, FormatError> {
        self.u16(offset, field).map(|v| v as i16)
    }

    pub fn u32(&mut self, offset: u32, field: &'static str) -> Result<u32, FormatError> {
        let at = self.at(offset, 4, field)?;
        self.igz.u32_at(at, field)
    }

    pub fn i32(&mut self, offset: u32, field: &'static str) -> Result<i32, FormatError> {
        self.u32(offset, field).map(|v| v as i32)
    }

    pub fn f32(&mut self, offset: u32, field: &'static str) -> Result<f32, FormatError> {
        self.u32(offset, field).map(f32::from_bits)
    }

    pub fn floats<const N: usize>(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<[f32; N], FormatError> {
        let mut out = [0.0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32(offset + 4 * i as u32, field)?;
        }
        Ok(out)
    }

    pub fn mat44(&mut self, offset: u32, field: &'static str) -> Result<Mat44, FormatError> {
        let mut out = [[0.0; 4]; 4];
        for (i, row) in out.iter_mut().enumerate() {
            *row = self.floats(offset + 16 * i as u32, field)?;
        }
        Ok(out)
    }

    /// `igUnsignedShortArrayMetaField` of `N` entries.
    pub fn u16s<const N: usize>(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<[u16; N], FormatError> {
        let mut out = [0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.u16(offset + 2 * i as u32, field)?;
        }
        Ok(out)
    }

    pub fn string(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<Option<&'a str>, FormatError> {
        let at = self.at(offset, 4, field)?;
        self.claim(offset);
        self.igz.string(at, field)
    }

    pub fn memory_ref(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<Option<MemoryRef<'a>>, FormatError> {
        let at = self.at(offset, 8, field)?;
        self.claim(offset);
        self.claim(offset + 4);
        self.igz.memory_ref(at, field)
    }

    pub fn vector(&mut self, offset: u32, field: &'static str) -> Result<Vector<'a>, FormatError> {
        let count = self.u32(offset, field)?;
        let data = self.memory_ref(offset + 4, field)?;
        if data.is_none() && count != 0 {
            return Err(self.igz.err_at(
                self.at(offset, 4, field)?,
                field,
                ErrorKind::Unsupported {
                    value: count.into(),
                },
            ));
        }
        Ok(Vector { count, data })
    }

    fn reference(&mut self, offset: u32, field: &'static str) -> Result<Ref<'a>, FormatError> {
        let at = self.at(offset, 4, field)?;
        self.claim(offset);
        self.igz.reference(at, field)
    }

    pub(crate) fn refused(&self, offset: u32, field: &'static str) -> FormatError {
        let at = self.object.field(offset).unwrap_or(self.object.at);
        let value = self.igz.u32_at(at, field).unwrap_or_default();
        self.igz.err_at(
            at,
            field,
            ErrorKind::Unsupported {
                value: value.into(),
            },
        )
    }

    /// `igMemoryRefHandleMetaField`: a TMHN index, `None` when zero and unmarked.
    pub fn handle(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<Option<Handle<'a>>, FormatError> {
        match self.reference(offset, field)? {
            Ref::Null => Ok(None),
            Ref::Handle { index, handle } => Ok(Some(Handle {
                index,
                block: handle,
            })),
            _ => Err(self.refused(offset, field)),
        }
    }

    /// An object ref that must be an import.
    pub fn import(&mut self, offset: u32, field: &'static str) -> Result<Import, FormatError> {
        match self.reference(offset, field)? {
            Ref::Import { index, id } => Ok(Import { index, id }),
            _ => Err(self.refused(offset, field)),
        }
    }

    /// An object ref this crate follows: null, or a local `target`.
    pub fn object(
        &mut self,
        offset: u32,
        field: &'static str,
        target: &'static str,
    ) -> Result<Option<usize>, FormatError> {
        match self.reference(offset, field)? {
            Ref::Null => Ok(None),
            Ref::Object(i) => {
                self.check_type(i, target, field)?;
                Ok(Some(i))
            }
            _ => Err(self.refused(offset, field)),
        }
    }

    /// An object ref this crate doesn't follow.
    pub fn object_ref(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<ObjectRef<'a>, FormatError> {
        match self.reference(offset, field)? {
            Ref::Null => Ok(ObjectRef::Null),
            Ref::Object(i) => Ok(ObjectRef::Object(i)),
            Ref::Import { index, id } => Ok(ObjectRef::Import(Import { index, id })),
            Ref::MetaObject { index, name } => Ok(ObjectRef::Named { index, name }),
            _ => Err(self.refused(offset, field)),
        }
    }

    /// A script object's type field ([`TypeRef`]).
    pub fn type_ref(
        &mut self,
        offset: u32,
        field: &'static str,
    ) -> Result<TypeRef<'a>, FormatError> {
        match self.reference(offset, field)? {
            Ref::Null => Ok(TypeRef::Null),
            Ref::Object(i) => Ok(TypeRef::Object(i)),
            Ref::Import { index, id } => Ok(TypeRef::Import(Import { index, id })),
            Ref::MetaObject { index, name } => Ok(TypeRef::Named { index, name }),
            Ref::UnresolvedMeta(index) => Ok(TypeRef::Indexed {
                index,
                name: self.igz.external_names().get(index as usize).copied(),
            }),
            _ => Err(self.refused(offset, field)),
        }
    }

    pub(crate) fn check_type(
        &self,
        object: usize,
        target: &'static str,
        field: &'static str,
    ) -> Result<(), FormatError> {
        let o = &self.igz.objects()[object];
        let found = self.igz.type_name(o);
        if is_a(found, target) {
            Ok(())
        } else {
            Err(self.igz.err_at(
                o.at,
                field,
                ErrorKind::WrongType {
                    expected: target,
                    found: found.to_owned(),
                },
            ))
        }
    }

    /// A data list's elements as object pointers. With `target`, each must
    /// be a non-null `target`.
    pub(crate) fn elements(
        &self,
        list: &DataList<'a>,
        target: Option<&'static str>,
    ) -> Result<Vec<Option<usize>>, FormatError> {
        let bytes = self.list_bytes(list, 4)?;
        let Some(data) = list.data else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for i in 0..bytes.len() as u32 / 4 {
            let at = data.at.offset_by(4 * i).unwrap_or(data.at);
            match (self.igz.reference(at, "element")?, target) {
                (Ref::Null, None) => out.push(None),
                (Ref::Object(o), None) => out.push(Some(o)),
                (Ref::Object(o), Some(t)) => {
                    self.check_type(o, t, "element")?;
                    out.push(Some(o));
                }
                _ => {
                    let value = self.igz.u32_at(at, "element")?;
                    return Err(self.igz.err_at(
                        at,
                        "element",
                        ErrorKind::Unsupported {
                            value: value.into(),
                        },
                    ));
                }
            }
        }
        Ok(out)
    }

    /// A data list's elements as unfollowed refs: null, local, imported or
    /// named. Script lists hold all four.
    pub(crate) fn element_refs(
        &self,
        list: &DataList<'a>,
    ) -> Result<Vec<ObjectRef<'a>>, FormatError> {
        let bytes = self.list_bytes(list, 4)?;
        let Some(data) = list.data else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for i in 0..bytes.len() as u32 / 4 {
            let at = data.at.offset_by(4 * i).unwrap_or(data.at);
            out.push(match self.igz.reference(at, "element")? {
                Ref::Null => ObjectRef::Null,
                Ref::Object(o) => ObjectRef::Object(o),
                Ref::Import { index, id } => ObjectRef::Import(Import { index, id }),
                Ref::MetaObject { index, name } => ObjectRef::Named { index, name },
                _ => {
                    let value = self.igz.u32_at(at, "element")?;
                    return Err(self.igz.err_at(
                        at,
                        "element",
                        ErrorKind::Unsupported {
                            value: value.into(),
                        },
                    ));
                }
            });
        }
        Ok(out)
    }

    /// The first `count` elements' bytes, `width` each. A negative count,
    /// or one the data can't hold, is refused.
    pub(crate) fn list_bytes(
        &self,
        list: &DataList<'a>,
        width: usize,
    ) -> Result<&'a [u8], FormatError> {
        let data = list.data.map_or(&[][..], |d| d.data);
        let needed = usize::try_from(list.count)
            .ok()
            .and_then(|c| c.checked_mul(width))
            .filter(|n| *n <= data.len());
        match needed {
            Some(n) => Ok(&data[..n]),
            None => Err(self.igz.err_at(
                self.object.field(DataList::COUNT).unwrap_or(self.object.at),
                "_count",
                ErrorKind::OutOfRange {
                    value: list.count as i64 as u64,
                    limit: (data.len() / width) as u64,
                },
            )),
        }
    }
}

/// Big-endian f32s from a byte run whose length is a multiple of 4.
pub(crate) fn be_f32s(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_bits(u32::from_be_bytes(*c)))
}

#[cfg(test)]
mod tests;
