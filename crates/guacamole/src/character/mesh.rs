//! A character model's meshes and skin from IR v2 (design decision 8):
//! each segment a Bevy `Mesh` with every attribute the shader reads, the
//! skeleton's joints as entities Bevy's GPU skinning reads, each skinned
//! with Alchemy's inverse joint (AC.17), the pose evaluated from the clip
//! on the CPU.

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::math::{Mat4, Quat, Vec3};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{
    Indices, Mesh, Mesh3d, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues,
    VertexFormat,
};
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::{ChildOf, Name, Transform, Visibility};
use guac_ir::model::{self, Model, Segment, Skeleton};

use super::material::CharacterMaterial;

/// The third texture-coordinate set, which Bevy has no attribute for.
pub const ATTRIBUTE_UV_2: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_Uv_2", 0x6775_6163_0002, VertexFormat::Float32x2);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("{what} has {got} entries for {vertices} vertices")]
    Length {
        what: &'static str,
        got: usize,
        vertices: usize,
    },
    #[error("the segment has no texture-coordinate set")]
    NoUvs,
    #[error("{count} texture-coordinate sets; the shader reads three at most")]
    TooManyUvs { count: usize },
    #[error("index {index} past the segment's {vertices} vertices")]
    Index { index: u16, vertices: usize },
    #[error("joint {joint} past the skeleton's {joints}")]
    Joint { joint: u16, joints: usize },
    #[error("segment {segment} names material {material}, past the model's {materials}")]
    Material {
        segment: usize,
        material: u16,
        materials: usize,
    },
}

/// A segment as a Bevy mesh: positions, normals, tangents with their
/// handedness, each texture-coordinate set, vertex colours as stored (a
/// multiplier centred on half grey, never decoded), four joints and their
/// weights of 255 as fractions, the same blend the SPU does.
pub fn segment_mesh(segment: &Segment) -> Result<Mesh, MeshError> {
    let n = segment.positions.len();
    let check = |what, got| {
        (got == n).then_some(()).ok_or(MeshError::Length {
            what,
            got,
            vertices: n,
        })
    };
    check("normals", segment.normals.len())?;
    check("joints", segment.joints.len())?;
    check("weights", segment.weights.len())?;
    if let Some(t) = &segment.tangents {
        check("tangents", t.len())?;
    }
    if let Some(c) = &segment.colors {
        check("colours", c.len())?;
    }
    match segment.uvs.len() {
        0 => return Err(MeshError::NoUvs),
        1..=3 => {}
        count => return Err(MeshError::TooManyUvs { count }),
    }
    for uv in &segment.uvs {
        check("a texture-coordinate set", uv.len())?;
    }
    if let Some(&index) = segment.indices.iter().find(|&&i| usize::from(i) >= n) {
        return Err(MeshError::Index { index, vertices: n });
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, segment.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, segment.normals.clone());
    let sets = [Mesh::ATTRIBUTE_UV_0, Mesh::ATTRIBUTE_UV_1, ATTRIBUTE_UV_2];
    for (attribute, uv) in sets.into_iter().zip(&segment.uvs) {
        mesh.insert_attribute(attribute, uv.clone());
    }
    if let Some(t) = &segment.tangents {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t.clone());
    }
    if let Some(c) = &segment.colors {
        let colours: Vec<[f32; 4]> = c.iter().map(|c| c.map(|v| f32::from(v) / 255.0)).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(segment.joints.clone()),
    );
    let weights: Vec<[f32; 4]> = segment
        .weights
        .iter()
        .map(|w| w.map(|v| f32::from(v) / 255.0))
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    mesh.insert_indices(Indices::U16(segment.indices.clone()));
    Ok(mesh)
}

/// Each joint's inverse joint, in joint order: what a skin influence's
/// joint index picks.
pub fn inverse_bindposes(skeleton: &Skeleton) -> Vec<Mat4> {
    skeleton
        .joints
        .iter()
        .map(|j| Mat4::from_cols_array_2d(&j.inverse_bind))
        .collect()
}

/// A model-space IR transform as Bevy's: translation, rotation, then
/// per-axis scale, the same matrix Havok's composes to.
pub fn joint_transform(t: &model::Transform) -> Transform {
    Transform {
        translation: Vec3::from_array(t.translation),
        rotation: Quat::from_array(t.rotation),
        scale: Vec3::from_array(t.scale),
    }
}

/// Every joint's model-space transform `seconds` into the idle, looped.
/// Joints are flat children of the model's root: Havok composes scale
/// differently from Bevy's transform hierarchy, so the clip evaluator
/// composes and Bevy only places.
pub fn pose(model: &Model, seconds: f64) -> Vec<Transform> {
    model
        .idle
        .model_pose(&model.skeleton, seconds)
        .iter()
        .map(joint_transform)
        .collect()
}

/// What [`spawn_character`] made.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnedCharacter {
    pub root: Entity,
    /// In the skeleton's order, which the skin's joint indices name.
    pub joints: Vec<Entity>,
    pub segments: Vec<Entity>,
}

/// Spawns `model` under a new root at `root_transform`: its joints posed
/// `seconds` into the idle, and each segment skinned over them with the
/// material its segment names (`materials` in the model's material order).
/// Segments skip frustum culling, since their bounds are the bind pose's.
pub fn spawn_character(
    commands: &mut Commands,
    model: &Model,
    root_transform: Transform,
    seconds: f64,
    meshes: &mut Assets<Mesh>,
    inverse: &mut Assets<SkinnedMeshInverseBindposes>,
    materials: &[Handle<CharacterMaterial>],
) -> Result<SpawnedCharacter, MeshError> {
    let joints = model.skeleton.joints.len();
    for (i, s) in model.segments.iter().enumerate() {
        if usize::from(s.material) >= materials.len() {
            return Err(MeshError::Material {
                segment: i,
                material: s.material,
                materials: materials.len(),
            });
        }
        if let Some(&joint) = s
            .joints
            .iter()
            .flatten()
            .find(|&&j| usize::from(j) >= joints)
        {
            return Err(MeshError::Joint { joint, joints });
        }
    }
    let built = model
        .segments
        .iter()
        .map(segment_mesh)
        .collect::<Result<Vec<_>, _>>()?;

    let root = commands
        .spawn((
            Name::new(model.name.clone()),
            root_transform,
            Visibility::default(),
        ))
        .id();
    let joint_entities: Vec<Entity> = model
        .skeleton
        .joints
        .iter()
        .zip(pose(model, seconds))
        .map(|(j, t)| {
            commands
                .spawn((Name::new(j.name.clone()), t, ChildOf(root)))
                .id()
        })
        .collect();
    let skin = inverse.add(SkinnedMeshInverseBindposes::from(inverse_bindposes(
        &model.skeleton,
    )));
    let segments = model
        .segments
        .iter()
        .zip(built)
        .map(|(s, mesh)| {
            commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials[usize::from(s.material)].clone()),
                    SkinnedMesh {
                        inverse_bindposes: skin.clone(),
                        joints: joint_entities.clone(),
                    },
                    NoFrustumCulling,
                    ChildOf(root),
                ))
                .id()
        })
        .collect();
    Ok(SpawnedCharacter {
        root,
        joints: joint_entities,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_ir::clip::Clip;
    use guac_ir::model::Joint;

    fn segment() -> Segment {
        Segment {
            material: 0,
            source: 122,
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 3]),
            uvs: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]],
            colors: Some(vec![[255, 128, 0, 51]; 3]),
            joints: vec![[0, 1, 0, 0]; 3],
            weights: vec![[204, 51, 0, 0]; 3],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn a_segment_becomes_a_mesh_with_every_attribute() {
        let mesh = segment_mesh(&segment()).unwrap();
        let formats: Vec<(&str, VertexFormat)> =
            mesh.attributes().map(|(a, _)| (a.name, a.format)).collect();
        assert_eq!(
            formats,
            [
                ("Vertex_Position", VertexFormat::Float32x3),
                ("Vertex_Normal", VertexFormat::Float32x3),
                ("Vertex_Uv", VertexFormat::Float32x2),
                ("Vertex_Tangent", VertexFormat::Float32x4),
                ("Vertex_Color", VertexFormat::Float32x4),
                ("Vertex_JointWeight", VertexFormat::Float32x4),
                ("Vertex_JointIndex", VertexFormat::Uint16x4),
            ]
        );
        // Colours and weights as stored, out of 255.
        let Some(VertexAttributeValues::Float32x4(c)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colours")
        };
        assert_eq!(c[0], [1.0, 128.0 / 255.0, 0.0, 0.2]);
        let Some(VertexAttributeValues::Float32x4(w)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
        else {
            panic!("weights")
        };
        assert_eq!(w[0], [0.8, 0.2, 0.0, 0.0]);
        assert_eq!(mesh.indices().unwrap().len(), 3);
    }

    #[test]
    fn three_texture_coordinate_sets_take_three_attributes() {
        let mut s = segment();
        s.uvs = vec![s.uvs[0].clone(); 3];
        let mesh = segment_mesh(&s).unwrap();
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
        assert!(mesh.attribute(ATTRIBUTE_UV_2).is_some());
        s.uvs.push(s.uvs[0].clone());
        assert_eq!(segment_mesh(&s), Err(MeshError::TooManyUvs { count: 4 }));
        s.uvs.clear();
        assert_eq!(segment_mesh(&s), Err(MeshError::NoUvs));
    }

    #[test]
    fn a_malformed_segment_is_refused() {
        let mut s = segment();
        s.normals.pop();
        assert_eq!(
            segment_mesh(&s),
            Err(MeshError::Length {
                what: "normals",
                got: 2,
                vertices: 3
            })
        );
        let mut s = segment();
        s.indices.push(3);
        assert_eq!(
            segment_mesh(&s),
            Err(MeshError::Index {
                index: 3,
                vertices: 3
            })
        );
    }

    fn model() -> Model {
        let joint = |name: &str, parent, x| Joint {
            name: name.into(),
            parent,
            reference: model::Transform {
                translation: [x, 0.0, 0.0],
                ..model::Transform::IDENTITY
            },
            inverse_bind: Mat4::from_translation(Vec3::new(-x, 0.0, 0.0)).to_cols_array_2d(),
        };
        Model {
            identity: guac_ir::DumpIdentity {
                title_id: "TEST00000".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            load_file: "000_Test".into(),
            name: "Test.mdl".into(),
            skeleton: Skeleton {
                joints: vec![joint("root", None, 0.0), joint("tip", Some(0), 1.0)],
            },
            materials: Vec::new(),
            segments: vec![segment()],
            idle: Clip {
                name: "Test_Idle.ma".into(),
                duration: 1.0,
                frame_duration: 1.0 / 30.0,
                frames: 31,
                tracks: Vec::new(),
            },
        }
    }

    #[test]
    fn the_skin_takes_each_joints_inverse_in_joint_order() {
        let m = model();
        let inv = inverse_bindposes(&m.skeleton);
        assert_eq!(inv.len(), 2);
        assert_eq!(
            inv[1].transform_point3(Vec3::new(1.0, 0.0, 0.0)),
            Vec3::ZERO
        );
        // Posed at reference, each joint's model transform undoes its
        // inverse joint: the bind pose skins to itself.
        let posed = pose(&m, 0.0);
        assert_eq!(posed[1].translation, Vec3::new(1.0, 0.0, 0.0));
        for (t, i) in posed.iter().zip(&inv) {
            assert!(
                (t.to_matrix() * *i).abs_diff_eq(Mat4::IDENTITY, 1e-6),
                "{t:?}"
            );
        }
    }

    #[test]
    fn a_joint_transform_keeps_translation_rotation_and_scale() {
        let t = joint_transform(&model::Transform {
            translation: [1.0, 2.0, 3.0],
            rotation: [
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            scale: [2.0, 1.0, 1.0],
        });
        // Scale first, then rotate (90° about Z), then translate.
        let p = t.transform_point(Vec3::new(1.0, 0.0, 0.0));
        assert!(p.abs_diff_eq(Vec3::new(1.0, 4.0, 3.0), 1e-6), "{p}");
    }
}
