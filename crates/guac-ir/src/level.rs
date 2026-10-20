//! A level as guacamole plays it (openspec AG.15, design decision 8), in
//! five parts, each a file of its own under `levels/<archive>/` so a part
//! the extract lacks is extracted alone (the manifest names each level's
//! parts):
//!
//! - [`Scene`]: every scene's segments, already where the level has them
//!   (no transforms on level 213's), on AE's segment and material records,
//!   the sky's groups beside them;
//! - [`Props`]: the models the placements draw, on AE's records too;
//! - [`Collision`]: each world's triangle mesh, in the level's frame;
//! - [`Placements`]: every actor with its transform, solid, movement,
//!   waypoints and activity state;
//! - [`Lights`]: the directional rig and the placed lights.
//!
//! Nothing is converted: positions in metres, angles in degrees, scales in
//! percent, solids and movement in the disc's units ([`crate::actor`]).
//! What exists at the start, what's solid and how it's drawn are the
//! consumers' (AG.7's rule, AG.19, AG.25).

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::actor::{Movement, Solid};
use crate::clip::Clip;
use crate::model::{
    Mask, Material, MaterialTrack, RenderStates, Sampler, Scroll, Segment, Skeleton,
};
use crate::rig::RigLight;

/// A technique's material: tfbmulti2's on AE's record, or another
/// effect's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LevelMaterial {
    Multi2(Material),
    Effect(EffectMaterial),
}

/// A material of an effect other than tfbmulti2 as stored: the grass's
/// (`0xAA6`), `0xAA8`'s (`player_shadow` on level 213) or tfbwarp's
/// (`0xAA9`). What each effect draws is AG.24's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectMaterial {
    /// The technique's name (`grass_HIRES`).
    pub name: String,
    /// The effect's export id in `global.bld`.
    pub effect: u32,
    /// The variant masks, on an effect that has them (the grass's).
    pub pixel: Option<Mask>,
    pub vertex: Option<Mask>,
    /// In list order, each by its texture unit.
    pub textures: Vec<UnitTexture>,
    pub states: RenderStates,
    /// Its material animation, as a tfbmulti2 material's (level 213's
    /// water warp scrolls).
    pub scrolls: Vec<Scroll>,
    pub tracks: Vec<MaterialTrack>,
    /// Attributes kept by type and left unread (`igShaderParametersAttr`,
    /// the shader's own constants).
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitTexture {
    pub unit: i16,
    /// `textures/<sha256 of the file>.ktx2`, shared with the characters'.
    pub texture: String,
    pub image: String,
    pub sampler: Sampler,
}

/// The scene part: `levels/<archive>/scene.<ron|postcard>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub identity: DumpIdentity,
    /// The level's archive (`level_213_tutotial`).
    pub level: String,
    pub scenes: Vec<SceneGraph>,
}

/// One `igSceneInfo` and the physics world that names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneGraph {
    /// The world's Maya file (`Levels/Level_213/Level_213_ART_MASTER.ma`).
    pub world: String,
    /// One per technique, in scene-graph order; segments index it.
    pub materials: Vec<LevelMaterial>,
    /// Where the level has them: no transform to apply.
    pub segments: Vec<Segment>,
    /// The groups the sky hangs under (`igUserInfo`), each with its segments.
    pub sky: Vec<SkyGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkyGroup {
    pub name: String,
    /// Into [`SceneGraph::segments`].
    pub segments: Vec<u32>,
}

/// The props part: `levels/<archive>/props.<ron|postcard>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Props {
    pub identity: DumpIdentity,
    pub level: String,
    /// Every model a placement names that reads, in object order.
    pub props: Vec<Prop>,
    /// Those that don't, each with why: its placements draw nothing.
    pub refused: Vec<RefusedProp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedProp {
    pub path: String,
    pub reason: String,
}

/// A placed model, on AE's records: AE's [`crate::Model`] carries a
/// character's skeleton and idle, which a prop may lack (183 of level 213's
/// 219 have neither).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    /// The model's path as the level names it, the key placements use
    /// (`C:/tfb/Content/Models/Objects/gourd_01.mdl`).
    pub path: String,
    /// A skinned prop's joints; `None` on a static one, whose segments
    /// carry no skin.
    pub skeleton: Option<Skeleton>,
    pub materials: Vec<LevelMaterial>,
    /// In the model's frame.
    pub segments: Vec<Segment>,
    /// A skinned prop's slot 0, when that is a clip posing its skeleton.
    pub idle: Option<Clip>,
}

/// The collision part: `levels/<archive>/collision.<ron|postcard>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collision {
    pub identity: DumpIdentity,
    pub level: String,
    /// One per world with a body (level 213: the art master's; its grass
    /// world has none).
    pub meshes: Vec<CollisionMesh>,
}

/// A world's triangle mesh where the level has it: its body's translation
/// added, its rotation identity. The welding words are dropped (the
/// physics rebuilds edge handling from adjacency) and the MOPP skipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollisionMesh {
    pub world: String,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
    /// One per triangle; what each value means is open (ag-motion's open
    /// question 4).
    pub materials: Vec<u8>,
    /// The shape's convex radius (0.05 m on the disc).
    pub radius: f32,
    /// `weldingType` as stored (4 on the disc): which side a triangle's
    /// edges weld on, for the physics to choose its edge fixing by.
    pub welding: u8,
}

/// The placements part: `levels/<archive>/placements.<ron|postcard>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placements {
    pub identity: DumpIdentity,
    pub level: String,
    /// Every actor, in object order.
    pub actors: Vec<Placement>,
    /// Their waypoint lists, each in path order.
    pub waypoints: Vec<Vec<Waypoint>>,
}

/// One actor (`tfbActorInfo`) as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Its object index in `level.bld`: unique, as names aren't.
    pub object: u32,
    /// `_ID`: a camera volume's camera, among others.
    pub id: i32,
    pub name: String,
    /// Its script's source path (`.../Scripts/Start_Level.ai`); scripts
    /// aren't run (AJ).
    pub script: Option<String>,
    /// Where it starts, and its Euler angles in degrees.
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    /// Per axis, in percent.
    pub scale: [f32; 3],
    /// A [`Prop::path`].
    pub model: Option<String>,
    pub solid: Option<Solid>,
    pub movement: Option<Movement>,
    /// Into [`Placements::waypoints`].
    pub waypoints: Option<u32>,
    /// As stored (level 213: 4 on 584, 5 on 413; what each means is
    /// AG.7's).
    pub activity_state: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub height: f32,
}

/// The lights part: `levels/<archive>/lights.<ron|postcard>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lights {
    pub identity: DumpIdentity,
    pub level: String,
    /// In object order: `_worldLight1`-`3`, which the materials read, and
    /// `_causticLight` on level 213.
    pub directional: Vec<RigLight>,
    pub placed: Vec<PlacedLight>,
}

/// A placed light (`tfbLightInfo`) as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedLight {
    pub name: String,
    /// The kind, raw (1 on level 213's one, a point light).
    pub light_type: u32,
    pub visible: bool,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub falloff_rate: f32,
    pub cutoff_angle: f32,
    /// Constant, linear and quadratic.
    pub attenuation: [f32; 3],
    pub diffuse: [u8; 4],
    pub ambient: [u8; 4],
    pub specular: [u8; 4],
    pub multiplier: f32,
}

impl SceneGraph {
    pub fn vertices(&self) -> usize {
        self.segments.iter().map(|s| s.positions.len()).sum()
    }

    pub fn triangles(&self) -> usize {
        self.segments.iter().map(|s| s.indices.len() / 3).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Blend, Filter, Glow, Params, Wrap};
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([3; 32]),
        }
    }

    fn both<T>(value: &T)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        assert_eq!(&from_ron::<T>(&to_ron(value).unwrap()).unwrap(), value);
        assert_eq!(
            &from_postcard::<T>(&to_postcard(value).unwrap()).unwrap(),
            value
        );
    }

    #[test]
    fn every_part_reads_back_in_both_formats() {
        let sampler = Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Clamp,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        };
        let grass = EffectMaterial {
            name: "grass_HIRES".into(),
            effect: 0xAA6,
            pixel: Some(Mask {
                base: 0x50,
                ignore: 0x50,
            }),
            vertex: None,
            textures: vec![UnitTexture {
                unit: 1,
                texture: "textures/ab.ktx2".into(),
                image: "grass_hieght.png".into(),
                sampler,
            }],
            states: RenderStates {
                blend: Blend::Multiply,
                depth_write: false,
                two_sided: true,
                alpha_test: Some(0.023529),
            },
            scrolls: Vec::new(),
            tracks: vec![MaterialTrack::FlipBook {
                layer: 0,
                style: 1,
                rows: 3,
                columns: 1,
                rate: 1.0 / 6.0,
                cells: Vec::new(),
            }],
            unread: vec!["igShaderParametersAttr".into()],
        };
        // A tfbmulti2 material carries the glow (AE.11) beside the tracks and
        // the multiply (AG.15): IR v4 has both.
        let cave = Material {
            name: "cave_painting".into(),
            pixel: Mask {
                base: 0,
                ignore: 0x3FF,
            },
            vertex: Mask {
                base: 0x1000,
                ignore: 0x1BFF,
            },
            textures: Vec::new(),
            params: Params {
                fixed_alpha: [-1.0; 4],
                layer1_extra: [1.0, 1.0, 1.0, 0.25],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: None,
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: Glow::LayerAlpha,
            },
            states: RenderStates {
                blend: Blend::Multiply,
                depth_write: false,
                two_sided: false,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: vec![MaterialTrack::Tint {
                style: 1,
                rate: 1.0,
                frames: vec![0, 2000],
                colors: vec![[0x6E, 0x6D, 0x70, 0xFF], [0x6E, 0x6D, 0x70, 0x96]],
            }],
        };
        let segment = Segment {
            material: 0,
            source: 9,
            positions: vec![[0.0, 1.0, 2.0]; 3],
            normals: Vec::new(),
            tangents: None,
            uvs: vec![vec![[0.5, 0.25]; 3]],
            colors: Some(vec![[1, 2, 3, 4]; 3]),
            joints: Vec::new(),
            weights: Vec::new(),
            indices: vec![0, 1, 2],
        };
        let scene = Scene {
            identity: identity(),
            level: "level_213_tutotial".into(),
            scenes: vec![SceneGraph {
                world: "Levels/Level_213/Level_213_GRASS.ma".into(),
                materials: vec![LevelMaterial::Effect(grass), LevelMaterial::Multi2(cave)],
                segments: vec![segment.clone()],
                sky: vec![SkyGroup {
                    name: "|Sky_morning_213:SKYDOME".into(),
                    segments: vec![0],
                }],
            }],
        };
        both(&scene);
        assert_eq!(
            (scene.scenes[0].vertices(), scene.scenes[0].triangles()),
            (3, 1)
        );
        both(&Props {
            identity: identity(),
            level: "level_213_tutotial".into(),
            props: vec![Prop {
                path: "C:/tfb/Content/Models/Objects/gourd_01.mdl".into(),
                skeleton: None,
                materials: Vec::new(),
                segments: vec![segment],
                idle: None,
            }],
            refused: vec![RefusedProp {
                path: "C:/tfb/Content/Models/Objects/fairy_upgrade_gem.mdl".into(),
                reason: "an animated transform".into(),
            }],
        });
        both(&Collision {
            identity: identity(),
            level: "level_213_tutotial".into(),
            meshes: vec![CollisionMesh {
                world: "Levels/Level_213/Level_213_ART_MASTER.ma".into(),
                vertices: vec![[-425.5, -37.625, -16.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
                triangles: vec![[0, 2, 1]],
                materials: vec![188],
                radius: 0.05,
                welding: 4,
            }],
        });
        let solid = Solid {
            shape: crate::actor::Shape::Box,
            solid: true,
            world: true,
            actor_surface: false,
            offset: [0.0, 1.5, 0.0],
            rotation: [0.0, 90.0, 0.0],
            width: 4.0,
            height: 21.0,
            length: 34.0,
            weight: 10.0,
            rebound: 25.0,
        };
        both(&Placements {
            identity: identity(),
            level: "level_213_tutotial".into(),
            actors: vec![Placement {
                object: 41,
                id: 7,
                name: "213_Init_Location".into(),
                script: Some("Levels/Level_213/Scripts/InitializeLevel.ai".into()),
                position: [10.191, 43.379, -93.394],
                rotation: [0.0, 180.0, 0.0],
                scale: [150.0, 125.0, 100.0],
                model: None,
                solid: Some(solid),
                movement: None,
                waypoints: Some(0),
                activity_state: 4,
            }],
            waypoints: vec![vec![Waypoint {
                name: "wp0".into(),
                position: [10.191, 43.379, -93.394],
                rotation: [0.0; 3],
                height: 2.5,
            }]],
        });
        both(&Lights {
            identity: identity(),
            level: "level_213_tutotial".into(),
            directional: vec![RigLight {
                name: "_worldLight1".into(),
                diffuse: [200, 190, 170, 255],
                ambient: [40, 45, 60, 255],
                specular: [255, 250, 240, 255],
                multiplier: 1.0,
                position: [0.0; 3],
                rotation: [-45.0, 30.0, 0.0],
            }],
            placed: vec![PlacedLight {
                name: "213_EvilRobot_Beam_light".into(),
                light_type: 1,
                visible: true,
                position: [1.0, 2.0, 3.0],
                rotation: [0.0; 3],
                falloff_rate: 0.25,
                cutoff_angle: 30.0,
                attenuation: [1.0, 0.5, 0.125],
                diffuse: [0x55, 0x2A, 0x11, 0xFF],
                ambient: [0, 0, 0, 0xFF],
                specular: [0x2C, 0x1F, 0x13, 0xFF],
                multiplier: 1.5,
            }],
        });
    }
}
