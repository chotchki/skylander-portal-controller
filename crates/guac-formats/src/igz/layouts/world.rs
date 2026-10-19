//! A level's world (openspec AG.2): `tfbPhysicsWorld`, through
//! `tfbWorldInfo`, is a build-data block whose `_data` is the scene's
//! collision as a Havok packfile ([`crate::collision`] reads it), beside
//! the scene graph and the level's texture table, both unfollowed here.

use super::{BuildDataInfo, Layout, ObjectRef, View};
use crate::FormatError;

/// `tfbWorldInfo`: size 0x28, a build-data block plus the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    /// An `igSceneInfo` (AG.3 follows it).
    pub scene_info: ObjectRef<'a>,
    pub global_level_texture_info: ObjectRef<'a>,
}

impl WorldInfo<'_> {
    pub const SCENE_INFO: u32 = 0x20;
    pub const GLOBAL_LEVEL_TEXTURE_INFO: u32 = 0x24;
}

impl<'a> Layout<'a> for WorldInfo<'a> {
    const TYPE: &'static str = "tfbWorldInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            scene_info: v.object_ref(Self::SCENE_INFO, "_sceneInfo")?,
            global_level_texture_info: v
                .object_ref(Self::GLOBAL_LEVEL_TEXTURE_INFO, "_globalLevelTextureInfo")?,
        })
    }
}

/// `tfbPhysicsWorld`: size 0x2C, one per scene of a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsWorld<'a> {
    pub world: WorldInfo<'a>,
    /// A 4-byte runtime id struct.
    pub id_code: u32,
}

impl PhysicsWorld<'_> {
    pub const ID_CODE: u32 = 0x28;
}

impl<'a> Layout<'a> for PhysicsWorld<'a> {
    const TYPE: &'static str = "tfbPhysicsWorld";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            world: WorldInfo::fields(v)?,
            id_code: v.u32(Self::ID_CODE, "_idCode")?,
        })
    }
}
