//! The pipelines the TV draws Giants 01.00 with, as numbers (openspec AE.25,
//! AG.43): every key the roster's figures draw on a card (AE.5's census of
//! the character models, with what each carries fresh since AE.23) and
//! every key chapter 1 draws (AG.19's), so a first start can compile them
//! before any extraction, when nothing of the disc is read yet.
//!
//! Which of the code's keys a part draws is the disc's: a key is a material
//! key ([`MaterialKey`]: the masks as drawn, the render states) on a mesh
//! shape ([`MeshShape`]), and the masks, states and vertex layouts the
//! character shader allows run to tens of thousands of pipelines, where the
//! 01.00 disc's figures draw 57 on a card (33 on skinned meshes, 24 on the
//! static parts they carry) and its chapter 1 another 55. So the tables
//! hold the census as numbers (no game bytes: a mask, a blend, a count of
//! texture-coordinate sets), and `tests/giants_census.rs` (`#[ignore]`d, on
//! `GUAC_GIANTS_DUMP`) holds them equal to what the extractor's readers and
//! the TV's own key derivation give off the dump, so a change to either
//! shows there. A key outside them still compiles when a shelf or a level
//! lands (AE.14's warm), as it did before.

use guac_ir::model::{
    self, Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, TextureRole, TextureSlot, Wrap,
};

use crate::character::key::{KeyError, MaterialKey, Pass, ShaderKey, StateKey};
use crate::character::warm::{MeshShape, WarmKey};

/// A drawn key as the tables keep it: the masks as drawn, the render
/// states, and the mesh's texture-coordinate sets, tangents and vertex
/// colours. Whether it's skinned is the table's: [`TURNTABLE`]'s are,
/// [`TURNTABLE_STATIC`]'s and [`LEVEL`]'s aren't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Row {
    pub pixel: u32,
    pub vertex: u32,
    pub blend: u8,
    pub depth_write: bool,
    pub two_sided: bool,
    pub alpha_test: bool,
    pub uv_sets: u8,
    pub tangents: bool,
    pub colours: bool,
}

/// [`Row::blend`]'s codes, in [`Blend`]'s order.
const BLENDS: [Blend; 4] = [
    Blend::Opaque,
    Blend::Alpha,
    Blend::Additive,
    Blend::Multiply,
];

const O: u8 = 0;
const A: u8 = 1;
const ADD: u8 = 2;
const MUL: u8 = 3;

#[allow(clippy::too_many_arguments)]
const fn row(
    pixel: u32,
    vertex: u32,
    blend: u8,
    depth_write: bool,
    two_sided: bool,
    alpha_test: bool,
    uv_sets: u8,
    tangents: bool,
    colours: bool,
) -> Row {
    Row {
        pixel,
        vertex,
        blend,
        depth_write,
        two_sided,
        alpha_test,
        uv_sets,
        tangents,
        colours,
    }
}

impl Row {
    /// The row's key, on a skinned mesh or a static one.
    pub fn key(&self, skinned: bool) -> Result<WarmKey, KeyError> {
        Ok(WarmKey {
            material: MaterialKey {
                shader: ShaderKey::new(self.pixel, self.vertex)?,
                states: StateKey {
                    blend: BLENDS[usize::from(self.blend)],
                    depth_write: self.depth_write,
                    two_sided: self.two_sided,
                    alpha_test: self.alpha_test,
                },
                pass: Pass::Colour,
            },
            mesh: MeshShape {
                uv_sets: self.uv_sets,
                tangents: self.tangents,
                colours: self.colours,
                skinned,
            },
        })
    }

    /// `key` as a row (its pass and whether it's skinned left out).
    pub fn of(key: &WarmKey) -> Self {
        let s = &key.material.states;
        Self {
            pixel: key.material.shader.pixel(),
            vertex: key.material.shader.vertex(),
            blend: BLENDS.iter().position(|b| *b == s.blend).unwrap_or(0) as u8,
            depth_write: s.depth_write,
            two_sided: s.two_sided,
            alpha_test: s.alpha_test,
            uv_sets: key.mesh.uv_sets,
            tangents: key.mesh.tangents,
            colours: key.mesh.colours,
        }
    }

    /// The row as the tables write it.
    pub fn source(&self) -> String {
        let blend = ["O", "A", "ADD", "MUL"][usize::from(self.blend)];
        format!(
            "row({:#06x}, {:#06x}, {blend}, {}, {}, {}, {}, {}, {}),",
            self.pixel,
            self.vertex,
            self.depth_write,
            self.two_sided,
            self.alpha_test,
            self.uv_sets,
            self.tangents,
            self.colours
        )
    }
}

/// Every key the 01.00 roster's figures draw on a card on a skinned mesh
/// (the turntable's scene: fog on, nothing else), each once: AE.5's census
/// of the 67 character models as the warm keys it, with the skinned parts
/// they carry fresh (AE.23).
pub const TURNTABLE: &[Row] = &[
    row(0x0400, 0x0400, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, ADD, false, false, false, 1, true, false),
    row(0x0404, 0x1404, O, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, true, false, 1, true, true),
    row(0x0418, 0x0418, O, true, false, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, true, 1, true, false),
    row(0x041c, 0x141c, O, true, false, false, 1, true, true),
    row(0x0420, 0x0420, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, true, 1, true, false),
    row(0x0424, 0x0424, O, true, true, true, 1, true, false),
    row(0x0424, 0x0424, A, true, true, true, 1, true, false),
    row(0x0428, 0x0428, O, true, false, false, 1, true, false),
    row(0x042c, 0x042c, O, true, false, false, 1, true, false),
    row(0x042c, 0x142c, O, true, false, false, 1, true, true),
    row(0x0438, 0x0438, O, true, false, false, 1, true, false),
    row(0x0438, 0x0438, O, true, true, false, 1, true, false),
    row(0x0438, 0x0438, O, true, true, true, 1, true, false),
    row(0x0438, 0x1438, O, true, false, false, 1, true, true),
    row(0x043c, 0x043c, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, false, true, 1, true, false),
    row(0x043c, 0x043c, O, true, true, false, 1, true, false),
    row(0x043c, 0x043c, O, true, true, true, 1, true, false),
    row(0x043c, 0x043c, A, false, true, false, 1, true, false),
    row(0x043c, 0x043c, A, true, false, false, 1, true, false),
    row(0x043c, 0x043c, A, true, false, true, 1, true, false),
    row(0x043c, 0x043c, A, true, true, false, 1, true, false),
    row(0x043c, 0x043c, A, true, true, true, 1, true, false),
    row(0x043c, 0x143c, O, true, false, false, 1, true, true),
    row(0x043c, 0x143c, A, true, false, false, 1, true, true),
];

/// Every key chapter 1 (`level_213_tutotial`) draws at AF's fresh state in
/// the level's scene (fog on but on the sky, which takes none of its
/// start's fog (AG.41); its one point light unless a material owns its
/// point lights off; no shadows until AG.23), each once: AG.19's 55, the
/// sky's 7 unfogged since AG.41.
pub const LEVEL: &[Row] = &[
    row(0x0000, 0x0000, O, true, false, false, 1, true, false),
    row(0x0000, 0x1000, O, true, false, false, 1, true, true),
    row(0x0000, 0x1000, A, true, false, false, 1, true, true),
    row(0x0000, 0x1000, A, true, false, true, 1, true, true),
    row(0x0001, 0x1001, A, false, true, false, 2, true, true),
    row(0x0001, 0x1001, A, false, true, true, 2, true, true),
    row(0x0001, 0x1001, ADD, true, true, false, 2, true, true),
    row(0x0c00, 0x2400, MUL, true, true, false, 1, true, false),
    row(0x0c00, 0x3400, O, true, true, false, 1, true, true),
    row(0x0c00, 0x3400, A, false, true, false, 1, true, true),
    row(0x0c00, 0x3400, A, true, true, false, 1, true, true),
    row(0x0c01, 0x3401, A, true, true, false, 2, true, true),
    row(0x0c01, 0x3401, ADD, false, false, false, 2, true, true),
    row(0x0c02, 0x3402, O, true, true, false, 2, true, true),
    row(0x0c02, 0x3402, O, true, true, true, 2, true, true),
    row(0x0c02, 0x3402, A, false, false, false, 2, true, true),
    row(0x0c02, 0x3402, A, false, true, false, 2, true, true),
    row(0x0c02, 0x3402, A, true, true, false, 2, true, true),
    row(0x0c04, 0x3404, O, true, true, false, 1, true, true),
    row(0x0c04, 0x3404, A, true, true, false, 1, true, true),
    row(0x0c08, 0x3408, O, true, true, false, 1, true, true),
    row(0x0c0a, 0x340a, O, true, true, false, 2, true, true),
    row(0x0c18, 0x2418, O, true, true, false, 1, true, false),
    row(0x0c18, 0x3418, O, true, false, false, 1, true, true),
    row(0x0c18, 0x3418, O, true, false, true, 1, true, true),
    row(0x0c18, 0x3418, O, true, true, false, 1, true, true),
    row(0x0c18, 0x3418, A, false, false, false, 1, true, true),
    row(0x0c18, 0x3418, A, false, true, false, 1, true, true),
    row(0x0c18, 0x3418, A, true, false, false, 1, true, true),
    row(0x0c18, 0x3418, A, true, false, true, 1, true, true),
    row(0x0c18, 0x3418, A, true, true, false, 1, true, true),
    row(0x0c18, 0x3418, A, true, true, true, 1, true, true),
    row(0x0c19, 0x2419, O, true, true, false, 2, true, false),
    row(0x0c19, 0x3419, O, true, false, false, 2, true, true),
    row(0x0c19, 0x3419, O, true, true, false, 2, true, true),
    row(0x0c1a, 0x341a, O, true, false, false, 2, true, true),
    row(0x0c1a, 0x341a, O, true, true, false, 2, true, true),
    row(0x0c1a, 0x341a, A, false, true, false, 2, true, true),
    row(0x0c1a, 0x341a, A, true, false, false, 2, true, true),
    row(0x0c1a, 0x341a, A, true, true, false, 2, true, true),
    row(0x0c1b, 0x341b, O, true, false, false, 3, true, true),
    row(0x0c1b, 0x341b, O, true, true, false, 3, true, true),
    row(0x0c1c, 0x341c, O, true, false, false, 1, true, true),
    row(0x0c1c, 0x341c, O, true, true, false, 1, true, true),
    row(0x0c1d, 0x341d, O, true, true, false, 2, true, true),
    row(0x0c1e, 0x341e, O, true, false, false, 2, true, true),
    row(0x0c1e, 0x341e, O, true, true, false, 2, true, true),
    row(0x0c1f, 0x341f, O, true, true, false, 3, true, true),
    row(0x0c38, 0x3438, O, true, true, false, 1, true, true),
    row(0x0c3a, 0x343a, O, true, true, false, 2, true, true),
    row(0x0c3a, 0x343a, A, true, true, false, 2, true, true),
    row(0x0c3b, 0x343b, O, true, true, false, 3, true, true),
    row(0x0c3e, 0x343e, O, true, true, false, 2, true, true),
    row(0x0c3e, 0x343e, A, true, true, false, 2, true, true),
    row(0x0c9c, 0x349c, O, true, false, true, 2, true, true),
];

/// Every key the 01.00 roster's figures draw on a card on a static mesh
/// (the turntable's scene), each once: the unskinned parts they carry
/// fresh (AE.23), each drawn under its bone or its parent part.
pub const TURNTABLE_STATIC: &[Row] = &[
    row(0x0400, 0x0400, O, true, false, false, 1, true, false),
    row(0x0400, 0x0400, A, false, true, false, 1, true, false),
    row(0x0404, 0x0404, O, true, false, false, 1, true, false),
    row(0x0404, 0x0404, A, false, false, false, 1, true, false),
    row(0x0404, 0x0404, A, true, false, false, 1, true, false),
    row(0x0404, 0x1404, O, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, false, false, false, 1, true, true),
    row(0x0404, 0x1404, A, true, false, false, 1, true, true),
    row(0x0404, 0x1404, A, true, true, false, 1, true, true),
    row(0x0408, 0x0408, O, true, false, false, 1, true, false),
    row(0x040c, 0x040c, O, true, false, false, 1, true, false),
    row(0x040c, 0x040c, O, true, true, false, 1, true, false),
    row(0x041c, 0x041c, O, true, false, false, 1, true, false),
    row(0x0420, 0x0420, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, O, true, false, false, 1, true, false),
    row(0x0424, 0x0424, A, true, true, false, 1, true, false),
    row(0x0424, 0x0424, ADD, true, false, false, 1, true, false),
    row(0x0424, 0x1424, A, true, false, false, 1, true, true),
    row(0x042c, 0x042c, O, true, false, false, 1, true, false),
    row(0x0438, 0x0438, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, false, false, 1, true, false),
    row(0x043c, 0x043c, O, true, true, false, 1, true, false),
    row(0x043c, 0x143c, O, true, false, false, 1, true, true),
    row(0x043c, 0x143c, A, true, false, false, 1, true, true),
];

/// Every key a card draws: [`TURNTABLE`]'s on skinned meshes, then
/// [`TURNTABLE_STATIC`]'s on static ones.
pub fn turntable_keys() -> Vec<WarmKey> {
    let mut out = keys(TURNTABLE, true);
    out.extend(keys(TURNTABLE_STATIC, false));
    out
}

/// [`LEVEL`]'s keys: a level's segments are static, in world space.
pub fn level_keys() -> Vec<WarmKey> {
    keys(LEVEL, false)
}

fn keys(rows: &[Row], skinned: bool) -> Vec<WarmKey> {
    rows.iter()
        // A row that isn't a key is a typo `cargo test` catches.
        .filter_map(|r| r.key(skinned).ok())
        .collect()
}

/// Bits the scene could turn on that a material owns once its ignore mask
/// holds them: every one of the 14, so the masks are drawn as given in any
/// scene.
const OWN_EVERYTHING: u32 = (1 << 14) - 1;

/// An IR material that draws `key`'s material key in any scene, through
/// [`crate::character::CharacterMaterial::from_ir`] as a model's or a
/// level's does: the masks owned whole, every texture role filled, every
/// layer's values set, a glow amount (so its glow pass draws too) and the
/// key's states. What the warm draws a census key with.
pub fn material_for(key: &WarmKey) -> model::Material {
    let s = &key.material.states;
    let mask = |base| Mask {
        base,
        ignore: OWN_EVERYTHING,
    };
    let slot = |role| TextureSlot {
        role,
        texture: "census".into(),
        image: "census".into(),
        sampler: Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        },
    };
    model::Material {
        name: format!(
            "census {:#06x}/{:#06x}",
            key.material.shader.pixel(),
            key.material.shader.vertex()
        ),
        pixel: mask(key.material.shader.pixel()),
        vertex: mask(key.material.shader.vertex()),
        textures: [
            TextureRole::Layer1ColorMap,
            TextureRole::Layer1BaseNormalMap,
            TextureRole::Layer1BaseSpecMap,
            TextureRole::Layer2ColorMap,
            TextureRole::Layer2BaseNormalMap,
            TextureRole::Layer2BaseSpecMap,
            TextureRole::Layer3ColorMap,
            TextureRole::Layer4ColorMap,
        ]
        .into_iter()
        .map(slot)
        .collect(),
        params: Params {
            fixed_alpha: [-1.0, -1.0, 1.0, 1.0],
            layer1_extra: [1.0, 1.0, 1.0, 0.25],
            layer2_extra: Some([1.0, 1.0, 1.0, 0.25]),
            layer2_blend: Some([1.0, 0.0, 0.0, 0.0]),
            layer3_blend: Some([0.0, 0.0, 1.0, 0.0]),
            layer4_blend: [0.0, 1.0, 0.0, 0.0],
            layer4_values: [0.0, 1.0, 0.0, 0.0],
            material_tint: [128, 128, 128, 255],
            glow: Glow::Amount(255),
        },
        states: RenderStates {
            blend: s.blend,
            depth_write: s.depth_write,
            two_sided: s.two_sided,
            alpha_test: s.alpha_test.then_some(0.5),
        },
        scrolls: Vec::new(),
        tracks: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::mesh::{segment_mesh, static_mesh};
    use crate::character::{CharacterMaterial, ShadowLookup};
    use bevy::asset::Handle;
    use guac_ir::shading::{PointLight, Scene};
    use std::collections::HashSet;

    /// Every scene a material could be drawn in: the masks are owned whole,
    /// so none of them moves the key.
    fn scenes() -> Vec<Scene> {
        let mut out = Vec::new();
        for fog in [false, true] {
            for shadows in [false, true] {
                for lights in 0..=3 {
                    out.push(Scene {
                        fog,
                        shadows,
                        point_lights: vec![
                            PointLight {
                                position: [0.0, 0.0, 0.0],
                                colour: [1.0; 3],
                                linear: 1.0,
                                quadratic: 0.0,
                            };
                            lights
                        ],
                        ..Scene::new()
                    });
                }
            }
        }
        out
    }

    #[test]
    fn every_row_is_a_key_and_none_twice() {
        let mut every = HashSet::new();
        for (rows, skinned, n) in [
            (TURNTABLE, true, 33),
            (TURNTABLE_STATIC, false, 24),
            (LEVEL, false, 55),
        ] {
            assert_eq!(rows.len(), n);
            let keys: Vec<WarmKey> = rows
                .iter()
                .map(|r| r.key(skinned).unwrap_or_else(|e| panic!("{r:?}: {e}")))
                .collect();
            let unique: HashSet<WarmKey> = keys.iter().copied().collect();
            assert_eq!(unique.len(), n, "a row twice");
            for (r, k) in rows.iter().zip(&keys) {
                assert_eq!(Row::of(k), *r);
                assert_eq!(k.material.pass, Pass::Colour);
            }
            every.extend(unique);
        }
        // No key in two tables: the warm draws a key once, a card's with
        // its glow pass and a level's without.
        assert_eq!(every.len(), 33 + 24 + 55);
        assert_eq!(
            turntable_keys().len(),
            TURNTABLE.len() + TURNTABLE_STATIC.len()
        );
        assert_eq!(level_keys().len(), LEVEL.len());
    }

    /// The tables as the scenes they were counted in draw them: the cards'
    /// keys (a body's and what it carries) carry the turntable's fog and
    /// nothing else, the level's its fog (but the sky's, which own it off
    /// and their point lights with it) and at most its one point light.
    #[test]
    fn each_table_is_its_scenes() {
        let cards = turntable_keys();
        for k in &cards {
            let s = k.material.shader.switches();
            assert!(s.fog && !s.shadows && s.point_lights == 0, "{k:?}");
            assert_eq!(
                (k.mesh.uv_sets, k.mesh.tangents),
                (1, true),
                "a character's or a carried part's segment: one set and a tangent"
            );
        }
        let static_parts = cards.iter().filter(|k| !k.mesh.skinned).count();
        assert_eq!(static_parts, TURNTABLE_STATIC.len());
        // A level material that owns its point lights draws none.
        let mut sky = 0;
        for k in level_keys() {
            let s = k.material.shader.switches();
            assert!(!s.shadows && s.point_lights <= 1, "{k:?}");
            if !s.fog {
                assert_eq!(s.point_lights, 0, "an unfogged key is the sky's: {k:?}");
                sky += 1;
            }
        }
        assert_eq!(sky, 7);
    }

    /// The warm draws a census key through the constructor the turntable and
    /// the level build their materials with: in every scene it builds the
    /// key the row names, its glow pass too, on a triangle of the row's shape.
    #[test]
    fn every_census_key_builds_through_the_turntables_constructor() {
        let scenes = scenes();
        for key in turntable_keys().into_iter().chain(level_keys()) {
            let ir = material_for(&key);
            for scene in &scenes {
                let m = CharacterMaterial::from_ir(&ir, scene, ShadowLookup::NONE, |_| {
                    Handle::default()
                })
                .unwrap_or_else(|e| panic!("{key:?}: {e}"));
                assert_eq!(m.key, key.material, "{scene:?}");
                let glow = m.glow_mask().expect("its glow pass draws");
                assert_eq!(glow.key.pass, Pass::GlowMask);
            }
            let triangle = key.mesh.triangle(0);
            assert_eq!(MeshShape::of(&triangle), key.mesh);
            let mesh = if key.mesh.skinned {
                segment_mesh(&triangle)
            } else {
                static_mesh(&triangle)
            };
            mesh.unwrap_or_else(|e| panic!("{key:?}: {e}"));
        }
    }

    #[test]
    fn a_row_reads_back_as_written() {
        let r = row(0x41C, 0x41C, O, true, false, true, 1, true, false);
        assert_eq!(
            r.source(),
            "row(0x041c, 0x041c, O, true, false, true, 1, true, false),"
        );
        let k = r.key(true).unwrap();
        assert_eq!(k.material.states.blend, Blend::Opaque);
        assert!(k.material.states.alpha_test);
        for (code, blend) in [
            (A, Blend::Alpha),
            (ADD, Blend::Additive),
            (MUL, Blend::Multiply),
        ] {
            let k = Row { blend: code, ..r }.key(false).unwrap();
            assert_eq!(k.material.states.blend, blend);
            assert!(!k.mesh.skinned);
        }
        // Bit 6 isn't a key the shader draws.
        assert!(
            row(0x40, 0x40, O, true, false, false, 1, true, false)
                .key(true)
                .is_err()
        );
    }
}
