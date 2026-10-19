//! What a character model's draws compile (openspec AE.14, design decision
//! 15): a render pipeline is a material's key (its masks as the scene draws
//! them, its render states, the pass) on the vertex layout its segment's
//! mesh gives (how many texture-coordinate sets, tangents, vertex colours).
//! A [`WarmKey`] per (material key, mesh shape) a model draws names every
//! pipeline its colour pass and its glow pass need on a turntable, so the
//! TV can draw each once offscreen before a model needs it: macOS compiles
//! a pipeline inside the frame that first draws with it.

use bevy::asset::Handle;
use guac_ir::model::{self, Model, Segment};
use guac_ir::shading::Scene;

use super::key::MaterialKey;
use super::material::{CharacterMaterial, MaterialError, ShadowLookup};

/// What a segment's mesh gives the vertex layout: [`super::segment_mesh`]
/// always writes positions, normals, joints and weights, and these vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshShape {
    pub uv_sets: u8,
    pub tangents: bool,
    pub colours: bool,
}

impl MeshShape {
    pub fn of(segment: &Segment) -> Self {
        Self {
            uv_sets: u8::try_from(segment.uvs.len()).unwrap_or(u8::MAX),
            tangents: segment.tangents.is_some(),
            colours: segment.colors.is_some(),
        }
    }

    /// One triangle of this shape on joint 0, facing +Z, its colours half
    /// grey: what the warm draws a key with.
    pub fn triangle(&self, material: u16) -> Segment {
        let n = 3;
        Segment {
            material,
            source: 0,
            positions: vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; n],
            tangents: self.tangents.then(|| vec![[1.0, 0.0, 0.0, 1.0]; n]),
            uvs: vec![vec![[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]]; usize::from(self.uv_sets)],
            colors: self.colours.then(|| vec![[128, 128, 128, 255]; n]),
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

/// Every key `model` draws in `scene`, each once, in segment order, with
/// the material that first draws it. Built through
/// [`CharacterMaterial::from_ir`], as the turntable builds the model's
/// materials, so a key here is the key there and a model it refuses has no
/// keys.
pub fn keys(model: &Model, scene: &Scene) -> Result<Vec<(WarmKey, usize)>, WarmError> {
    let materials = model
        .materials
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
    for (i, s) in model.segments.iter().enumerate() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::key::{Pass, SceneBits, ShaderKey, StateKey};
    use crate::character::mesh::segment_mesh;
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
            },
        );
        m.materials = materials;
        m.segments = segments;
        m
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
                    let shape = MeshShape {
                        uv_sets,
                        tangents,
                        colours,
                    };
                    let t = shape.triangle(0);
                    assert_eq!(MeshShape::of(&t), shape);
                    segment_mesh(&t).expect("a mesh");
                }
            }
        }
    }
}
