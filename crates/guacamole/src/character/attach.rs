//! What a fresh figure carries, on its model (openspec AE.23): each of a
//! model's attachments ([`guac_ir::model::Attachment`]) under the joint or
//! the attachment it hangs on, at its attach point's offset, so it follows
//! the bone through the idle. A static prop draws unskinned; a skinned one
//! (Eye-Brawl's eye) skins over joints of its own, posed by its own idle or
//! held at its reference pose.
//!
//! An attachment's materials draw as a level's do
//! ([`crate::level_scene::drawn_material`]): tfbmulti2 only, so a part on
//! another effect stays off. On the 01.00 disc every attachment's are
//! tfbmulti2.

use bevy::asset::{Assets, Handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::math::{Mat4, Vec3};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{Material, MeshMaterial3d};
use bevy::prelude::{ChildOf, Name, Transform, Visibility};
use guac_ir::clip::Clip;
use guac_ir::model::{self, Attachment, Model};

use super::mesh::{MeshError, inverse_bindposes, joint_transform, pose, spawn_character};
use crate::level_scene::drawn_material;

/// The materials a model draws: its own, then each attachment's drawn
/// ones in attachment order.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub materials: Vec<model::Material>,
    /// Per attachment, its prop's material index to its place in
    /// `materials`; `None` for one that isn't drawn.
    pub of: Vec<Vec<Option<usize>>>,
}

/// `model`'s materials, then each attachment's that draw as tfbmulti2,
/// each attachment's mapped to where its land.
pub fn drawn(model: &Model) -> Drawn {
    let mut materials = model.materials.clone();
    let of = model
        .attachments
        .iter()
        .map(|a| {
            a.model
                .materials
                .iter()
                .map(|m| {
                    drawn_material(m).map(|m| {
                        materials.push(m);
                        materials.len() - 1
                    })
                })
                .collect()
        })
        .collect();
    Drawn { materials, of }
}

/// Each attachment's segments on the drawn materials (`drawn`'s indexes),
/// those on a material that isn't drawn left out.
pub fn segments(model: &Model, drawn: &Drawn) -> Vec<Vec<model::Segment>> {
    model
        .attachments
        .iter()
        .zip(&drawn.of)
        .map(|(a, of)| {
            a.model
                .segments
                .iter()
                .filter_map(|s| {
                    let m = (*of.get(usize::from(s.material))?)?;
                    Some(model::Segment {
                        material: u16::try_from(m).ok()?,
                        ..s.clone()
                    })
                })
                .collect()
        })
        .collect()
}

/// A skinned attachment's idle, or a still one holding its reference pose.
fn idle(a: &Attachment) -> Clip {
    a.model.idle.clone().unwrap_or_else(|| Clip {
        name: String::new(),
        duration: 1.0,
        frame_duration: 1.0,
        frames: 1,
        tracks: Vec::new(),
    })
}

/// A skinned attachment's joints, in its model's space, `seconds` into its
/// own idle; empty for a static one.
pub fn attachment_pose(a: &Attachment, seconds: f64) -> Vec<Transform> {
    let Some(skeleton) = &a.model.skeleton else {
        return Vec::new();
    };
    match &a.model.idle {
        Some(clip) => clip.model_pose(skeleton, seconds),
        None => skeleton.reference_pose(),
    }
    .iter()
    .map(joint_transform)
    .collect()
}

/// Each attachment's frame in the model's space, `seconds` into the idle,
/// as a column-vector matrix: its parent's (the body's origin or the joint
/// it names, or an earlier attachment's frame and the joint of it it names)
/// times its offset.
pub fn frames(model: &Model, seconds: f64) -> Vec<Mat4> {
    let body = pose(model, seconds);
    let mut out: Vec<Mat4> = Vec::with_capacity(model.attachments.len());
    for a in &model.attachments {
        let parent = match a.parent {
            None => a
                .joint
                .and_then(|j| body.get(usize::from(j)))
                .map_or(Mat4::IDENTITY, Transform::to_matrix),
            Some(k) => {
                let k = usize::from(k);
                let at = out.get(k).copied().unwrap_or(Mat4::IDENTITY);
                let joint = a.joint.and_then(|j| {
                    attachment_pose(&model.attachments[k], seconds)
                        .get(usize::from(j))
                        .map(Transform::to_matrix)
                });
                at * joint.unwrap_or(Mat4::IDENTITY)
            }
        };
        out.push(parent * joint_transform(&a.offset).to_matrix());
    }
    out
}

/// Every vertex the attachments draw, in the model's space, `seconds` into
/// the idle: what the turntable's framing sweeps beside the body.
pub fn points(model: &Model, seconds: f64) -> Vec<Vec3> {
    let frames = frames(model, seconds);
    let mut out = Vec::new();
    for (a, frame) in model.attachments.iter().zip(frames) {
        let joints: Vec<Mat4> = match &a.model.skeleton {
            Some(skeleton) => attachment_pose(a, seconds)
                .iter()
                .zip(inverse_bindposes(skeleton))
                .map(|(t, inverse)| t.to_matrix() * inverse)
                .collect(),
            None => Vec::new(),
        };
        for s in &a.model.segments {
            for (i, p) in s.positions.iter().enumerate() {
                let p = Vec3::from_array(*p);
                let local = match (s.joints.get(i), s.weights.get(i)) {
                    (Some(js), Some(ws)) if !joints.is_empty() => {
                        js.iter().zip(ws).fold(Vec3::ZERO, |at, (j, w)| {
                            joints.get(usize::from(*j)).map_or(at, |m| {
                                at + m.transform_point3(p) * (f32::from(*w) / 255.0)
                            })
                        })
                    }
                    _ => p,
                };
                out.push(frame.transform_point3(local));
            }
        }
    }
    out
}

/// One drawn segment of an attachment, as spawned: what the glow pass
/// copies ([`spawn_glow_masks`]).
#[derive(Debug, Clone)]
pub struct Part {
    pub entity: Entity,
    /// Into [`Drawn::materials`].
    pub material: usize,
    pub mesh: Handle<Mesh>,
    /// What it hangs under.
    pub parent: Entity,
    /// Its skin, on a skinned attachment.
    pub skin: Option<SkinnedMesh>,
}

/// Each attachment's root and joints, and every segment spawned.
#[derive(Debug, Clone, Default)]
pub struct SpawnedAttachments {
    /// Each attachment's root, at its offset under what it hangs on.
    pub roots: Vec<Entity>,
    /// Each attachment's joints, empty on a static one: what
    /// [`attachment_pose`] poses.
    pub joints: Vec<Vec<Entity>>,
    pub parts: Vec<Part>,
}

/// Spawns `model`'s attachments under `root` (the body's) and `joints`
/// (its joints, in its skeleton's order), each at its offset under the
/// joint or attachment it hangs on, its segments with `materials` (by
/// [`Drawn::materials`] index). Segments skip frustum culling, as the
/// body's do.
#[allow(clippy::too_many_arguments)]
pub fn spawn_attachments<M: Material>(
    commands: &mut Commands,
    model: &Model,
    drawn: &Drawn,
    root: Entity,
    joints: &[Entity],
    seconds: f64,
    meshes: &mut Assets<Mesh>,
    inverse: &mut Assets<SkinnedMeshInverseBindposes>,
    materials: &[Handle<M>],
) -> Result<SpawnedAttachments, MeshError> {
    let segments = segments(model, drawn);
    let mut out = SpawnedAttachments::default();
    for (i, (a, segments)) in model.attachments.iter().zip(segments).enumerate() {
        let parent = match a.parent {
            None => a
                .joint
                .and_then(|j| joints.get(usize::from(j)).copied())
                .unwrap_or(root),
            Some(k) => {
                let k = usize::from(k);
                a.joint
                    .and_then(|j| out.joints.get(k)?.get(usize::from(j)).copied())
                    .or_else(|| out.roots.get(k).copied())
                    .unwrap_or(root)
            }
        };
        // A joint has no visibility of its own (it draws nothing); what
        // hangs on it inherits through it, so it gets one.
        if parent != root {
            commands.entity(parent).insert_if_new(Visibility::default());
        }
        let offset = joint_transform(&a.offset);
        if let Some(skeleton) = &a.model.skeleton {
            let prop = Model {
                identity: model.identity.clone(),
                load_file: model.load_file.clone(),
                name: a.model.path.clone(),
                skeleton: skeleton.clone(),
                materials: drawn.materials.clone(),
                segments: segments.clone(),
                idle: idle(a),
                attachments: Vec::new(),
                attachments_unsettled: None,
            };
            let spawned =
                spawn_character(commands, &prop, offset, seconds, meshes, inverse, materials)?;
            commands.entity(spawned.root).insert(ChildOf(parent));
            for ((entity, mesh), s) in spawned.segments.iter().zip(&spawned.meshes).zip(&segments) {
                out.parts.push(Part {
                    entity: *entity,
                    material: usize::from(s.material),
                    mesh: mesh.clone(),
                    parent: spawned.root,
                    skin: Some(SkinnedMesh {
                        inverse_bindposes: spawned.skin.clone(),
                        joints: spawned.joints.clone(),
                    }),
                });
            }
            out.roots.push(spawned.root);
            out.joints.push(spawned.joints);
            continue;
        }
        let at = commands
            .spawn((
                Name::new(a.placement.clone()),
                offset,
                Visibility::default(),
                ChildOf(parent),
            ))
            .id();
        for s in &segments {
            let index = usize::from(s.material);
            let material = materials.get(index).ok_or(MeshError::Material {
                segment: i,
                material: s.material,
                materials: materials.len(),
            })?;
            let mesh = meshes.add(super::mesh::static_mesh(s)?);
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    NoFrustumCulling,
                    ChildOf(at),
                ))
                .id();
            out.parts.push(Part {
                entity,
                material: index,
                mesh,
                parent: at,
                skin: None,
            });
        }
        out.roots.push(at);
        out.joints.push(Vec::new());
    }
    Ok(out)
}

/// Each of `spawned`'s parts again on `layer` with its material's glow
/// pass (`masks`, by [`Drawn::materials`] index; `None` leaves the part
/// out), as [`super::spawn_glow_masks`] does the body's.
pub fn spawn_glow_masks<M: Material>(
    commands: &mut Commands,
    spawned: &SpawnedAttachments,
    masks: &[Option<Handle<M>>],
    layer: usize,
) -> Vec<Entity> {
    spawned
        .parts
        .iter()
        .filter_map(|p| Some((p, masks.get(p.material)?.clone()?)))
        .map(|(p, mask)| {
            let mut e = commands.spawn((
                Mesh3d(p.mesh.clone()),
                MeshMaterial3d(mask),
                NoFrustumCulling,
                bevy::camera::visibility::RenderLayers::layer(layer),
                ChildOf(p.parent),
            ));
            if let Some(skin) = &p.skin {
                e.insert(skin.clone());
            }
            e.id()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::app::App;
    use bevy::asset::AssetApp;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::math::Quat;
    use bevy::prelude::{GlobalTransform, MinimalPlugins};
    use guac_ir::clip::{RotationTrack, Track, VecSpline, VecTrack};
    use guac_ir::level::{EffectMaterial, LevelMaterial, Prop};
    use guac_ir::model::{
        Blend, Glow, Joint, Mask, Params, RenderStates, Segment, Skeleton, Transform as IrTransform,
    };

    fn material(name: &str) -> model::Material {
        model::Material {
            name: name.into(),
            pixel: Mask {
                base: 0,
                ignore: 0x2FF,
            },
            vertex: Mask {
                base: 0,
                ignore: 0x1AFF,
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
                glow: Glow::None,
            },
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: false,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        }
    }

    /// A unit square at `z` on `material`, unskinned.
    fn square(material: u16, z: f32) -> Segment {
        Segment {
            material,
            source: 0,
            positions: vec![[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z], [1.0, 1.0, z]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: None,
            uvs: vec![vec![[0.0, 0.0]; 4]],
            colors: None,
            joints: Vec::new(),
            weights: Vec::new(),
            indices: vec![0, 1, 2, 2, 1, 3],
        }
    }

    fn offset(translation: [f32; 3], rotation: Quat) -> IrTransform {
        IrTransform {
            translation,
            rotation: rotation.to_array(),
            scale: [1.0; 3],
        }
    }

    /// A column two high whose head (joint 1) slides 0 → 1 → 0 along X over
    /// its 2 s idle; a knife on the head a quarter out and turned about Y,
    /// a jewel on the knife's root a half further, and a sparkle (another
    /// effect) beside the knife's own material.
    fn carrier() -> Model {
        let joint = |name: &str, parent, y: f32| Joint {
            name: name.into(),
            parent,
            reference: IrTransform {
                translation: [0.0, y, 0.0],
                ..IrTransform::IDENTITY
            },
            inverse_bind: Mat4::from_translation(Vec3::new(0.0, -y, 0.0)).to_cols_array_2d(),
        };
        let head = Track {
            joint: 1,
            translation: VecTrack {
                constant: [0.0, 2.0, 0.0],
                spline: Some(VecSpline {
                    channels: [true, false, false],
                    degree: 1,
                    knots: vec![0, 0, 30, 60, 60],
                    points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0; 3]],
                }),
            },
            rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
            scale: VecTrack {
                constant: [1.0; 3],
                spline: None,
            },
        };
        let sparkle = LevelMaterial::Effect(EffectMaterial {
            name: "sparkle".into(),
            effect: 0xAA9,
            pixel: None,
            vertex: None,
            textures: Vec::new(),
            states: material("x").states,
            scrolls: Vec::new(),
            tracks: Vec::new(),
            unread: Vec::new(),
        });
        let prop = |path: &str, materials: Vec<LevelMaterial>, segments| Prop {
            path: path.into(),
            skeleton: None,
            materials,
            segments,
            idle: None,
        };
        let mut body = square(0, 0.0);
        body.joints = vec![[0; 4], [0; 4], [1, 0, 0, 0], [1, 0, 0, 0]];
        body.weights = vec![[255, 0, 0, 0]; 4];
        Model {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([2; 32]),
            },
            load_file: "000_Carrier".into(),
            name: "Carrier.mdl".into(),
            skeleton: Skeleton {
                joints: vec![joint("foot", None, 0.0), joint("head", Some(0), 2.0)],
            },
            materials: vec![material("body")],
            segments: vec![body],
            idle: Clip {
                name: "Carrier_Idle.ma".into(),
                duration: 2.0,
                frame_duration: 1.0 / 30.0,
                frames: 61,
                tracks: vec![head],
            },
            attachments: vec![
                Attachment {
                    placement: "000_Knife".into(),
                    parent: None,
                    bone: Some("head".into()),
                    joint: Some(1),
                    offset: offset([0.25, 0.0, 0.0], Quat::from_rotation_y(0.5)),
                    model: prop(
                        "Knife.mdl",
                        vec![LevelMaterial::Multi2(material("knife")), sparkle],
                        vec![square(0, 0.0), square(1, 0.1)],
                    ),
                },
                Attachment {
                    placement: "000_Jewel".into(),
                    parent: Some(0),
                    bone: Some("*root*".into()),
                    joint: None,
                    offset: offset([0.0, 0.0, 0.5], Quat::IDENTITY),
                    model: prop(
                        "Jewel.mdl",
                        vec![LevelMaterial::Multi2(material("jewel"))],
                        vec![square(0, 0.0)],
                    ),
                },
            ],
            attachments_unsettled: None,
        }
    }

    fn close(a: Mat4, b: Mat4) -> bool {
        a.abs_diff_eq(b, 1e-5)
    }

    #[test]
    fn an_attachment_follows_its_bone_through_the_idle() {
        let m = carrier();
        let knife = joint_transform(&m.attachments[0].offset).to_matrix();
        let jewel = joint_transform(&m.attachments[1].offset).to_matrix();
        for t in [0.0, 0.5, 1.0, 1.5, 2.5] {
            let head = pose(&m, t)[1].to_matrix();
            let f = frames(&m, t);
            assert!(close(f[0], head * knife), "the knife at {t}");
            assert!(close(f[1], head * knife * jewel), "the jewel at {t}");
        }
        // The head slides 1 along X by half the loop, and the knife with it.
        let at = |t| frames(&m, t)[0].w_axis.truncate();
        assert!((at(1.0) - at(0.0) - Vec3::X).length() < 1e-5, "{}", at(1.0));
        assert!((at(2.0) - at(0.0)).length() < 1e-5);
    }

    #[test]
    fn only_tfbmulti2_draws_and_the_points_sweep_what_does() {
        let m = carrier();
        let d = drawn(&m);
        let names: Vec<&str> = d.materials.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["body", "knife", "jewel"]);
        assert_eq!(d.of, [vec![Some(1), None], vec![Some(2)]]);
        let s = segments(&m, &d);
        assert_eq!(s[0].len(), 1, "the sparkle's square is left out");
        assert_eq!((s[0][0].material, s[1][0].material), (1, 2));
        // The jewel's far corner, a quarter, a half and a unit square out
        // along the knife's turned axes from the head at rest.
        let points = points(&m, 0.0);
        assert_eq!(points.len(), 4 + 4 + 4);
        let want = frames(&m, 0.0)[1].transform_point3(Vec3::new(1.0, 1.0, 0.0));
        assert!(points.iter().any(|p| (*p - want).length() < 1e-5));
    }

    #[test]
    fn spawned_attachments_hang_under_their_joint_and_parent() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .init_asset::<bevy::pbr::StandardMaterial>();
        let m = carrier();
        let d = drawn(&m);
        let world = app.world_mut();
        let handles: Vec<Handle<bevy::pbr::StandardMaterial>> = {
            let mut assets = world.resource_mut::<Assets<bevy::pbr::StandardMaterial>>();
            d.materials
                .iter()
                .map(|_| assets.add(bevy::pbr::StandardMaterial::default()))
                .collect()
        };
        let spawned =
            world
                .run_system_once(
                    move |mut commands: Commands,
                          mut meshes: bevy::prelude::ResMut<Assets<Mesh>>,
                          mut inverse: bevy::prelude::ResMut<
                        Assets<SkinnedMeshInverseBindposes>,
                    >| {
                        let m = carrier();
                        let body = spawn_character(
                            &mut commands,
                            &m,
                            Transform::IDENTITY,
                            0.5,
                            &mut meshes,
                            &mut inverse,
                            &handles,
                        )
                        .unwrap();
                        let carried = spawn_attachments(
                            &mut commands,
                            &m,
                            &drawn(&m),
                            body.root,
                            &body.joints,
                            0.5,
                            &mut meshes,
                            &mut inverse,
                            &handles,
                        )
                        .unwrap();
                        (body, carried)
                    },
                )
                .unwrap();
        app.update();
        let (body, carried) = spawned;
        let world = app.world();
        assert_eq!(carried.roots.len(), 2);
        assert_eq!(carried.parts.len(), 2, "the sparkle's square isn't spawned");
        assert_eq!(
            world.get::<ChildOf>(carried.roots[0]).unwrap().parent(),
            body.joints[1]
        );
        // The joint a part hangs on passes visibility down (Bevy's B0004).
        assert!(
            world
                .get::<bevy::prelude::InheritedVisibility>(body.joints[1])
                .is_some()
        );
        assert!(
            world
                .get::<bevy::prelude::InheritedVisibility>(body.joints[0])
                .is_none()
        );
        assert_eq!(
            world.get::<ChildOf>(carried.roots[1]).unwrap().parent(),
            carried.roots[0]
        );
        let f = frames(&m, 0.5);
        for (root, want) in carried.roots.iter().zip(f) {
            let got = world.get::<GlobalTransform>(*root).unwrap().to_matrix();
            assert!(close(got, want), "{got} against {want}");
        }
        // The glow pass copies a part whose material has a mask.
        let masks = vec![
            None,
            Some(Handle::<bevy::pbr::StandardMaterial>::default()),
            None,
        ];
        let n = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_glow_masks(&mut commands, &carried, &masks, 9).len()
            })
            .unwrap();
        assert_eq!(n, 1);
    }
}
