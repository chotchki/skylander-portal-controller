//! What a character model's draws compile (openspec AE.14, design decision
//! 15): a render pipeline is a material's key (its masks as the scene draws
//! them, its render states, the pass) on the vertex layout its segment's
//! mesh gives (how many texture-coordinate sets, tangents, vertex colours).
//! A [`WarmKey`] per (material key, mesh shape) a model draws names every
//! pipeline its colour pass and its glow pass need on a turntable, so the
//! TV can draw each once offscreen before a model needs it: macOS compiles
//! a pipeline inside the frame that first draws with it. A level's draws
//! (openspec AG.19) are keys of the same kind on unskinned meshes
//! ([`segment_keys`], [`MeshShape::skinned`]).

use bevy::asset::Handle;
use guac_ir::model::{self, Model, Segment};
use guac_ir::shading::Scene;

use super::key::{MaterialKey, Pass};
use super::material::{CharacterMaterial, MaterialError, ShadowLookup};

/// What a segment's mesh gives the vertex layout: [`super::segment_mesh`]
/// always writes positions and normals, and these vary. A skinned one (a
/// character's) also has joints and weights; a static one (a level's,
/// [`super::static_mesh`]) has neither, which is another pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshShape {
    pub uv_sets: u8,
    pub tangents: bool,
    pub colours: bool,
    pub skinned: bool,
}

impl MeshShape {
    pub fn of(segment: &Segment) -> Self {
        Self {
            uv_sets: u8::try_from(segment.uvs.len()).unwrap_or(u8::MAX),
            tangents: segment.tangents.is_some(),
            colours: segment.colors.is_some(),
            skinned: !segment.joints.is_empty(),
        }
    }

    /// One triangle of this shape facing +Z, its colours half grey, on
    /// joint 0 when it's skinned: what the warm draws a key with.
    pub fn triangle(&self, material: u16) -> Segment {
        let n = if self.skinned { 3 } else { 0 };
        Segment {
            material,
            source: 0,
            positions: vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: self.tangents.then(|| vec![[1.0, 0.0, 0.0, 1.0]; 3]),
            uvs: vec![vec![[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]]; usize::from(self.uv_sets)],
            colors: self.colours.then(|| vec![[128, 128, 128, 255]; 3]),
            joints: vec![[0; 4]; n],
            weights: vec![[255, 0, 0, 0]; n],
            indices: vec![0, 1, 2],
        }
    }
}

/// A pipeline pair a model draws: its colour pass's key on this mesh shape,
/// and the same key in the glow pass ([`CharacterMaterial::glow_mask`]),
/// unless that pass leaves the part out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WarmKey {
    pub material: MaterialKey,
    pub mesh: MeshShape,
}

/// Why a model has no keys: the turntable can't show it either.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WarmError {
    #[error("material {name}: {error}")]
    Material { name: String, error: MaterialError },
    #[error("segment {segment} names material {material}, past the model's {materials}")]
    Segment {
        segment: usize,
        material: u16,
        materials: usize,
    },
}

/// Every key `model` draws in `scene`, its attachments' (AE.23) after its
/// own, each once, in segment order, with the index of the material that
/// first draws it in [`super::attach::drawn`]'s list. Built through
/// [`CharacterMaterial::from_ir`], as the turntable builds the model's
/// materials, so a key here is the key there and a model it refuses has no
/// keys.
pub fn keys(model: &Model, scene: &Scene) -> Result<Vec<(WarmKey, usize)>, WarmError> {
    let drawn = super::attach::drawn(model);
    let mut segments = model.segments.clone();
    segments.extend(super::attach::segments(model, &drawn).into_iter().flatten());
    segment_keys(&drawn.materials, &segments, scene)
}

/// Every key `segments` draw with `materials` in `scene`, each once, in
/// segment order, with the index of the material that first draws it: a
/// model's ([`keys`]) or a level's drawn segments (AG.19).
pub fn segment_keys(
    materials: &[model::Material],
    segments: &[Segment],
    scene: &Scene,
) -> Result<Vec<(WarmKey, usize)>, WarmError> {
    let materials = materials
        .iter()
        .map(|m| {
            CharacterMaterial::from_ir(m, scene, ShadowLookup::NONE, |_| Handle::default())
                .map(|c| c.key)
                .map_err(|error| WarmError::Material {
                    name: m.name.clone(),
                    error,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut keys: Vec<(WarmKey, usize)> = Vec::new();
    for (i, s) in segments.iter().enumerate() {
        let index = usize::from(s.material);
        let material = *materials.get(index).ok_or(WarmError::Segment {
            segment: i,
            material: s.material,
            materials: materials.len(),
        })?;
        let key = WarmKey {
            material,
            mesh: MeshShape::of(s),
        };
        if !keys.iter().any(|(k, _)| *k == key) {
            keys.push((key, index));
        }
    }
    Ok(keys)
}

/// `keys` and, after them, each one whose material glows again in a level's
/// glow pass ([`Pass::LevelGlow`], openspec AH.65), each once, with the
/// material's index: what a level's parts, its props, its clones and its
/// hero draw, their glowing parts' masks laid into the frame's alpha.
pub fn with_level_glow(
    materials: &[model::Material],
    mut keys: Vec<(WarmKey, usize)>,
) -> Vec<(WarmKey, usize)> {
    let glows: Vec<(WarmKey, usize)> = keys
        .iter()
        .filter(|(_, m)| {
            materials
                .get(*m)
                .is_some_and(|m| m.params.glow != model::Glow::None)
        })
        .map(|(k, m)| {
            let mut k = *k;
            k.material.pass = Pass::LevelGlow;
            (k, *m)
        })
        .collect();
    for g in glows {
        if !keys.iter().any(|(k, _)| *k == g.0) {
            keys.push(g);
        }
    }
    keys
}

/// A one-triangle model drawing `material` on `shape`, under one joint at
/// rest with an idle that holds it: what the warm spawns for a key.
pub fn triangle_model(
    identity: &guac_ir::DumpIdentity,
    material: &model::Material,
    shape: MeshShape,
) -> Model {
    Model {
        identity: identity.clone(),
        load_file: "warm".into(),
        name: format!("warm {}", material.name),
        skeleton: model::Skeleton {
            joints: vec![model::Joint {
                name: "root".into(),
                parent: None,
                reference: model::Transform::IDENTITY,
                inverse_bind: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
            }],
        },
        materials: vec![material.clone()],
        segments: vec![shape.triangle(0)],
        idle: guac_ir::clip::Clip {
            name: "warm".into(),
            duration: 0.0,
            frame_duration: 1.0 / 30.0,
            frames: 1,
            tracks: Vec::new(),
        },
        attachments: Vec::new(),
        attachments_unsettled: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::key::{Pass, SceneBits, ShaderKey, StateKey};
    use crate::character::mesh::{segment_mesh, static_mesh};
    use guac_ir::model::{
        Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, TextureRole, TextureSlot, Wrap,
    };

    fn slot(role: TextureRole) -> TextureSlot {
        TextureSlot {
            role,
            texture: "textures/x.ktx2".into(),
            image: "x".into(),
            sampler: Sampler {
                wrap_u: Wrap::Repeat,
                wrap_v: Wrap::Repeat,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: None,
            },
        }
    }

    /// A material with `base` on both sides (Eruptor's ignore masks), its
    /// maps by role and `states`.
    fn material(name: &str, base: u32, states: RenderStates) -> model::Material {
        let mut textures = vec![slot(TextureRole::Layer1ColorMap)];
        for (bit, role) in [
            (3, TextureRole::Layer1BaseNormalMap),
            (4, TextureRole::Layer1BaseSpecMap),
            (2, TextureRole::Layer4ColorMap),
        ] {
            if base >> bit & 1 == 1 {
                textures.push(slot(role));
            }
        }
        model::Material {
            name: name.into(),
            pixel: Mask {
                base,
                ignore: 0x2FF,
            },
            vertex: Mask {
                base,
                ignore: 0x1AFF,
            },
            textures,
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
            states,
            scrolls: Vec::new(),
            tracks: Vec::new(),
        }
    }

    const OPAQUE: RenderStates = RenderStates {
        blend: Blend::Opaque,
        depth_write: true,
        two_sided: false,
        alpha_test: None,
    };

    fn fogged() -> Scene {
        Scene {
            fog: true,
            ..Scene::new()
        }
    }

    fn model(materials: Vec<model::Material>, segments: Vec<Segment>) -> Model {
        let mut m = triangle_model(
            &guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([1; 32]),
            },
            &materials[0],
            MeshShape {
                uv_sets: 1,
                tangents: true,
                colours: false,
                skinned: true,
            },
        );
        m.materials = materials;
        m.segments = segments;
        m
    }

    /// AH.65: a glowing material's keys come again in a level's glow pass,
    /// each once, after the colour pass's, with its material's index; a
    /// material without a glow attribute keys none.
    #[test]
    fn the_glowing_parts_keys_come_again_in_the_level_glow_pass() {
        let shape = MeshShape {
            uv_sets: 1,
            tangents: true,
            colours: false,
            skinned: false,
        };
        let key = |pixel: u32, pass: Pass| WarmKey {
            material: MaterialKey {
                shader: super::super::key::ShaderKey::new(pixel, pixel).unwrap(),
                states: super::super::key::StateKey::OPAQUE,
                pass,
            },
            mesh: shape,
        };
        let material = |glow: model::Glow| model::Material {
            name: "m".into(),
            pixel: model::Mask { base: 0, ignore: 0 },
            vertex: model::Mask { base: 0, ignore: 0 },
            textures: Vec::new(),
            params: model::Params {
                fixed_alpha: [-1.0; 4],
                layer1_extra: [1.0; 4],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: None,
                layer4_blend: [0.0; 4],
                layer4_values: [0.0; 4],
                material_tint: [128; 4],
                glow,
            },
            states: model::RenderStates {
                blend: model::Blend::Opaque,
                depth_write: true,
                two_sided: false,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        };
        let materials = [
            material(model::Glow::Amount(64)),
            material(model::Glow::None),
            material(model::Glow::LayerAlpha),
        ];
        let keys = vec![
            (key(0x400, Pass::Colour), 0),
            (key(0x404, Pass::Colour), 1),
            (key(0x408, Pass::Colour), 2),
            (key(0x400, Pass::Colour), 0),
        ];
        let out = with_level_glow(&materials, keys.clone());
        assert_eq!(out[..4], keys[..]);
        assert_eq!(
            out[4..],
            [
                (key(0x400, Pass::LevelGlow), 0),
                (key(0x408, Pass::LevelGlow), 2)
            ]
        );
    }

    #[test]
    fn a_models_keys_are_its_materials_as_the_scene_draws_them_on_each_mesh_shape() {
        let blended = RenderStates {
            blend: Blend::Alpha,
            depth_write: false,
            two_sided: true,
            alpha_test: Some(0.5),
        };
        let plain = MeshShape {
            uv_sets: 1,
            tangents: true,
            colours: false,
            skinned: true,
        };
        let coloured = MeshShape {
            colours: true,
            ..plain
        };
        let m = model(
            vec![
                material("body", 0x1C, OPAQUE),
                material("eyes", 0x1C, OPAQUE),
                material("fins", 0x18, blended),
            ],
            vec![
                plain.triangle(0),
                // Another material with the same key on the same shape: one.
                plain.triangle(1),
                coloured.triangle(0),
                plain.triangle(2),
            ],
        );
        let keys = keys(&m, &fogged()).unwrap();
        let body = MaterialKey {
            // Fog (bit 10) is the scene's, outside both ignore masks.
            shader: ShaderKey::resolve(
                m.materials[0].pixel,
                m.materials[0].vertex,
                SceneBits::TURNTABLE,
            )
            .unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        assert_eq!(body.shader.pixel(), 0x41C);
        let fins = MaterialKey {
            shader: ShaderKey::new(0x418, 0x418).unwrap(),
            states: StateKey::from(&blended),
            pass: Pass::Colour,
        };
        assert_eq!(
            keys,
            vec![
                (
                    WarmKey {
                        material: body,
                        mesh: plain
                    },
                    0
                ),
                (
                    WarmKey {
                        material: body,
                        mesh: coloured
                    },
                    0
                ),
                (
                    WarmKey {
                        material: fins,
                        mesh: plain
                    },
                    2
                ),
            ]
        );
        // The shape is the mesh's: the same attributes, the same layout.
        for (s, shape) in [(&m.segments[0], plain), (&m.segments[2], coloured)] {
            let mesh = segment_mesh(s).unwrap();
            assert_eq!(MeshShape::of(s), shape);
            assert_eq!(
                mesh.attribute(bevy::mesh::Mesh::ATTRIBUTE_COLOR).is_some(),
                shape.colours
            );
        }
    }

    #[test]
    fn a_model_the_turntable_refuses_has_no_keys() {
        // A spec map with no spec map texture: `from_ir` refuses it.
        let mut bad = material("bad", 0x10, OPAQUE);
        bad.textures
            .retain(|t| t.role != TextureRole::Layer1BaseSpecMap);
        let shape = MeshShape {
            uv_sets: 1,
            tangents: true,
            colours: false,
            skinned: true,
        };
        let m = model(
            vec![material("good", 0, OPAQUE), bad],
            vec![shape.triangle(0)],
        );
        assert!(matches!(
            keys(&m, &fogged()),
            Err(WarmError::Material { name, .. }) if name == "bad"
        ));
        let m = model(vec![material("good", 0, OPAQUE)], vec![shape.triangle(3)]);
        assert_eq!(
            keys(&m, &fogged()),
            Err(WarmError::Segment {
                segment: 0,
                material: 3,
                materials: 1
            })
        );
    }

    #[test]
    fn the_warm_triangle_takes_each_shape() {
        for uv_sets in 1..=3 {
            for tangents in [false, true] {
                for colours in [false, true] {
                    for skinned in [false, true] {
                        let shape = MeshShape {
                            uv_sets,
                            tangents,
                            colours,
                            skinned,
                        };
                        let t = shape.triangle(0);
                        assert_eq!(MeshShape::of(&t), shape);
                        let mesh = if skinned {
                            segment_mesh(&t)
                        } else {
                            static_mesh(&t)
                        };
                        mesh.expect("a mesh");
                    }
                }
            }
        }
    }
}
