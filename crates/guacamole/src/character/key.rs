//! The material key (design decision 9): a part's pixel and vertex masks as
//! the original draws them, plus its render states. The masks become the
//! shader defs the WESL source tests with `@if`, one per feature bit; the
//! states become the pipeline's blend, depth write and culling.
//!
//! A drawn mask is the material's base bits plus the scene's runtime bits
//! outside its ignore mask (ae-materials' selector): shadows (8), fog (10)
//! and point lights (11-13, thermometer-coded) on the pixel side, the same
//! two and "point lit" (13) on the vertex side. Bits 0-10 mean the same on
//! both sides; vertex bit 12 is the vertex colour.

use bevy::material::AlphaMode;
use bevy::render::render_resource::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, Face, RenderPipelineDescriptor,
};
use bevy::shader::ShaderDefVal;
use guac_ir::model::{Blend, Mask, RenderStates};
use guac_ir::shading::Switches;

/// tfbmulti2's feature bits (ae-shader section 4), pixel side unless named.
pub mod bit {
    pub const LAYER2: u32 = 0;
    pub const LAYER3: u32 = 1;
    /// Layer 4, the rim's sphere map.
    pub const RIM: u32 = 2;
    pub const NORMAL_MAP: u32 = 3;
    pub const SPEC_MAP: u32 = 4;
    pub const DIRECTIONAL: u32 = 5;
    pub const RIM_BY_POSITION: u32 = 6;
    pub const GLOSS_MAP: u32 = 7;
    pub const SHADOWS: u32 = 8;
    pub const REFLECTIONS: u32 = 9;
    pub const FOG: u32 = 10;
    /// At least one point light; 12 at least two, 13 three.
    pub const POINT_LIGHTS: u32 = 11;
    /// Vertex side: skinning in the shader, never compiled (the SPU skins).
    pub const VERTEX_SKINNING: u32 = 11;
    /// Vertex side.
    pub const VERTEX_COLOUR: u32 = 12;
    /// Vertex side: a point light reaches the part.
    pub const VERTEX_POINT_LIT: u32 = 13;
}

/// Both masks' width: tfbmulti2 has 14 feature bits.
const BITS: u32 = 14;
/// Bits 0-10, which mean the same on both sides.
const SHARED: u32 = (1 << 11) - 1;

/// The shader defs, one per feature bit, and the ones derived from them.
pub mod def {
    pub const LAYER2: &str = "CHAR_LAYER2";
    pub const LAYER3: &str = "CHAR_LAYER3";
    pub const RIM: &str = "CHAR_RIM";
    pub const NORMAL_MAP: &str = "CHAR_NORMAL_MAP";
    pub const SPEC_MAP: &str = "CHAR_SPEC_MAP";
    pub const DIRECTIONAL: &str = "CHAR_DIRECTIONAL";
    pub const SHADOWS: &str = "CHAR_SHADOWS";
    pub const FOG: &str = "CHAR_FOG";
    pub const POINT_LIGHTS: [&str; 3] = [
        "CHAR_POINT_LIGHTS_1",
        "CHAR_POINT_LIGHTS_2",
        "CHAR_POINT_LIGHTS_3",
    ];
    pub const VERTEX_COLOUR: &str = "CHAR_VERTEX_COLOUR";
    /// Derived: the vertex stage writes the eye direction.
    pub const EYE: &str = "CHAR_EYE";
    /// Derived: the part has a geometric normal.
    pub const GEOMETRIC_NORMAL: &str = "CHAR_GEOMETRIC_NORMAL";
    /// Derived: the second texture-coordinate set is read.
    pub const UV1: &str = "CHAR_UV1";
    /// Derived: the third texture-coordinate set is read.
    pub const UV2: &str = "CHAR_UV2";
    /// From the render states.
    pub const ALPHA_TEST: &str = "CHAR_ALPHA_TEST";
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("{side} bit {bit} isn't one of tfbmulti2's")]
    Unknown { side: Side, bit: u32 },
    #[error("{side} bit {bit} is {name}, which no character has")]
    Feature {
        side: Side,
        bit: u32,
        name: &'static str,
    },
    #[error("pixel bit {bit} needs bit {needs}")]
    Requires { bit: u32, needs: u32 },
    #[error("the pixel ({pixel:#06x}) and vertex ({vertex:#06x}) masks disagree on bits 0-10")]
    Disagree { pixel: u32, vertex: u32 },
    #[error("vertex bit 13 (point lit) is {vertex} where the pixel side has {pixel} point lights")]
    PointLit { vertex: bool, pixel: u8 },
    #[error("the {side} ignore mask {ignore:#06x} owns some of the point-light bits but not all")]
    SplitPointLights { side: Side, ignore: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Pixel,
    Vertex,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pixel => "pixel",
            Self::Vertex => "vertex",
        })
    }
}

/// The scene's runtime switches: what it turns on for every part that
/// leaves them to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneBits {
    pub shadows: bool,
    pub fog: bool,
    /// Up to three.
    pub point_lights: u8,
}

impl SceneBits {
    /// The turntable's (design decision 14): fog on so a spec-mapped part
    /// keeps its sheen, no shadows or point lights.
    pub const TURNTABLE: Self = Self {
        shadows: false,
        fog: true,
        point_lights: 0,
    };

    fn pixel(self) -> u32 {
        u32::from(self.shadows) << bit::SHADOWS
            | u32::from(self.fog) << bit::FOG
            | thermometer(self.point_lights) << bit::POINT_LIGHTS
    }

    fn vertex(self) -> u32 {
        u32::from(self.shadows) << bit::SHADOWS
            | u32::from(self.fog) << bit::FOG
            | u32::from(self.point_lights > 0) << bit::VERTEX_POINT_LIT
    }
}

/// 1, 2 or 3 lights as 0b1, 0b11 or 0b111.
fn thermometer(lights: u8) -> u32 {
    (1u32 << lights.min(3)) - 1
}

/// The masks a part draws with, checked: every bit one this shader has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShaderKey {
    pixel: u16,
    vertex: u16,
}

impl ShaderKey {
    /// Checks a drawn pair of masks.
    pub fn new(pixel: u32, vertex: u32) -> Result<Self, KeyError> {
        for (side, mask) in [(Side::Pixel, pixel), (Side::Vertex, vertex)] {
            if let Some(bit) = (BITS..32).find(|b| mask >> b & 1 == 1) {
                return Err(KeyError::Unknown { side, bit });
            }
            for (bit, name) in [
                (bit::RIM_BY_POSITION, "rim by position"),
                (bit::GLOSS_MAP, "a gloss map"),
                (bit::REFLECTIONS, "reflections"),
            ] {
                if mask >> bit & 1 == 1 {
                    return Err(KeyError::Feature { side, bit, name });
                }
            }
        }
        if vertex >> bit::VERTEX_SKINNING & 1 == 1 {
            return Err(KeyError::Feature {
                side: Side::Vertex,
                bit: bit::VERTEX_SKINNING,
                name: "skinning in the shader",
            });
        }
        for (bit, needs) in [(12, 11), (13, 12)] {
            if pixel >> bit & 1 == 1 && pixel >> needs & 1 == 0 {
                return Err(KeyError::Requires { bit, needs });
            }
        }
        if pixel & SHARED != vertex & SHARED {
            return Err(KeyError::Disagree { pixel, vertex });
        }
        let lit = vertex >> bit::VERTEX_POINT_LIT & 1 == 1;
        let lights = (pixel >> bit::POINT_LIGHTS & 0b111).count_ones() as u8;
        if lit != (lights > 0) {
            return Err(KeyError::PointLit {
                vertex: lit,
                pixel: lights,
            });
        }
        Ok(Self {
            pixel: pixel as u16,
            vertex: vertex as u16,
        })
    }

    /// What the original draws a material with in `scene`: its base bits
    /// plus the scene's outside its ignore masks.
    pub fn resolve(pixel: Mask, vertex: Mask, scene: SceneBits) -> Result<Self, KeyError> {
        for (side, ignore, lights) in [
            (Side::Pixel, pixel.ignore, 0b111 << bit::POINT_LIGHTS),
            (Side::Vertex, vertex.ignore, 1 << bit::VERTEX_POINT_LIT),
        ] {
            let owned = ignore & lights;
            if owned != 0 && owned != lights {
                return Err(KeyError::SplitPointLights { side, ignore });
            }
        }
        Self::new(
            pixel.base | (scene.pixel() & !pixel.ignore),
            vertex.base | (scene.vertex() & !vertex.ignore),
        )
    }

    /// The masks for a set of switches, the reverse of [`Self::switches`].
    pub fn from_switches(s: &Switches) -> Result<Self, KeyError> {
        let shared = u32::from(s.second) << bit::LAYER2
            | u32::from(s.third) << bit::LAYER3
            | u32::from(s.rim) << bit::RIM
            | u32::from(s.normal_map) << bit::NORMAL_MAP
            | u32::from(s.spec_map) << bit::SPEC_MAP
            | u32::from(s.directional) << bit::DIRECTIONAL
            | u32::from(s.shadows) << bit::SHADOWS
            | u32::from(s.fog) << bit::FOG;
        if s.point_lights > 3 {
            return Err(KeyError::Unknown {
                side: Side::Pixel,
                bit: bit::POINT_LIGHTS + u32::from(s.point_lights) - 1,
            });
        }
        Self::new(
            shared | thermometer(s.point_lights) << bit::POINT_LIGHTS,
            shared
                | u32::from(s.vertex_colour) << bit::VERTEX_COLOUR
                | u32::from(s.point_lights > 0) << bit::VERTEX_POINT_LIT,
        )
    }

    pub fn pixel(&self) -> u32 {
        u32::from(self.pixel)
    }

    pub fn vertex(&self) -> u32 {
        u32::from(self.vertex)
    }

    fn pixel_bit(&self, b: u32) -> bool {
        self.pixel() >> b & 1 == 1
    }

    /// The character-shading spec's switches these masks turn on.
    pub fn switches(&self) -> Switches {
        Switches {
            normal_map: self.pixel_bit(bit::NORMAL_MAP),
            spec_map: self.pixel_bit(bit::SPEC_MAP),
            directional: self.pixel_bit(bit::DIRECTIONAL),
            vertex_colour: self.vertex() >> bit::VERTEX_COLOUR & 1 == 1,
            second: self.pixel_bit(bit::LAYER2),
            third: self.pixel_bit(bit::LAYER3),
            rim: self.pixel_bit(bit::RIM),
            shadows: self.pixel_bit(bit::SHADOWS),
            fog: self.pixel_bit(bit::FOG),
            point_lights: (self.pixel() >> bit::POINT_LIGHTS & 0b111).count_ones() as u8,
        }
    }

    fn vertex_bit(&self, b: u32) -> bool {
        self.vertex() >> b & 1 == 1
    }

    /// Whether the vertex stage writes the eye direction: a spec map with
    /// fog or a point light, read off the vertex mask as the original's
    /// vertex program decides it rather than from the reference model's
    /// [`Switches::eye_direction`], so the GPU lane still checks the
    /// model's reading of the condition.
    pub fn writes_eye(&self) -> bool {
        self.vertex_bit(bit::SPEC_MAP)
            && (self.vertex_bit(bit::FOG) || self.vertex_bit(bit::VERTEX_POINT_LIT))
    }

    /// Whether the part has a geometric normal for specular to reflect
    /// about without a normal map: a rim, directional lighting or a point
    /// light, off the vertex mask as [`Self::writes_eye`] is.
    pub fn writes_normal(&self) -> bool {
        self.vertex_bit(bit::RIM)
            || self.vertex_bit(bit::DIRECTIONAL)
            || self.vertex_bit(bit::VERTEX_POINT_LIT)
    }

    /// How many texture-coordinate sets the part reads: the first layer's,
    /// then one each for a second and a third layer.
    pub fn uv_sets(&self) -> usize {
        let s = self.switches();
        1 + usize::from(s.second) + usize::from(s.third)
    }

    /// One def per bit set, then the derived ones.
    pub fn shader_defs(&self) -> Vec<ShaderDefVal> {
        let s = self.switches();
        let mut defs: Vec<&'static str> = [
            (s.second, def::LAYER2),
            (s.third, def::LAYER3),
            (s.rim, def::RIM),
            (s.normal_map, def::NORMAL_MAP),
            (s.spec_map, def::SPEC_MAP),
            (s.directional, def::DIRECTIONAL),
            (s.shadows, def::SHADOWS),
            (s.fog, def::FOG),
            (s.vertex_colour, def::VERTEX_COLOUR),
            (self.writes_eye(), def::EYE),
            (self.writes_normal(), def::GEOMETRIC_NORMAL),
            (self.uv_sets() > 1, def::UV1),
            (self.uv_sets() > 2, def::UV2),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        defs.extend(&def::POINT_LIGHTS[..usize::from(s.point_lights)]);
        defs.into_iter().map(ShaderDefVal::from).collect()
    }
}

/// A part's render states as the pipeline takes them; the alpha test's
/// reference is a uniform, so only whether there is one keys a pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateKey {
    pub blend: Blend,
    pub depth_write: bool,
    pub two_sided: bool,
    pub alpha_test: bool,
}

impl From<&RenderStates> for StateKey {
    fn from(s: &RenderStates) -> Self {
        Self {
            blend: s.blend,
            depth_write: s.depth_write,
            two_sided: s.two_sided,
            alpha_test: s.alpha_test.is_some(),
        }
    }
}

impl StateKey {
    /// An opaque, depth-writing, one-sided part, the 199 of 237 AE.5 counted.
    pub const OPAQUE: Self = Self {
        blend: Blend::Opaque,
        depth_write: true,
        two_sided: false,
        alpha_test: false,
    };

    /// The colour blend: by source alpha, or source alpha plus what's there,
    /// or what's there times the part's colour (a level's, AG.15). Alpha
    /// keeps Bevy's "over" when blending and stays what's there when adding
    /// or multiplying, since those parts tint light and cover nothing; the
    /// spec says nothing of the stored alpha.
    pub fn blend_state(&self) -> Option<BlendState> {
        let colour = |dst_factor| BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor,
            operation: BlendOperation::Add,
        };
        match self.blend {
            Blend::Opaque => None,
            Blend::Alpha => Some(BlendState {
                color: colour(BlendFactor::OneMinusSrcAlpha),
                alpha: BlendComponent::OVER,
            }),
            Blend::Additive => Some(BlendState {
                color: colour(BlendFactor::One),
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
            Blend::Multiply => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Zero,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
        }
    }

    /// The game's winding matches Bevy's (counter-clockwise in front) and
    /// it culls back faces too (AC.16), so only a two-sided part keeps them.
    pub fn cull_mode(&self) -> Option<Face> {
        (!self.two_sided).then_some(Face::Back)
    }

    /// The Bevy phase the part draws in: blended parts sort back to front,
    /// alpha-tested ones draw with the parts that may discard.
    pub fn alpha_mode(&self, reference: f32) -> AlphaMode {
        match self.blend {
            Blend::Alpha => AlphaMode::Blend,
            Blend::Additive => AlphaMode::Add,
            Blend::Multiply => AlphaMode::Multiply,
            Blend::Opaque if self.alpha_test => AlphaMode::Mask(reference),
            Blend::Opaque => AlphaMode::Opaque,
        }
    }

    /// Sets the descriptor's blend on every colour target, its depth write
    /// and its culling. The depth test stays the pipeline's.
    pub fn apply(&self, descriptor: &mut RenderPipelineDescriptor) {
        descriptor.primitive.cull_mode = self.cull_mode();
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(self.depth_write);
        }
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = self.blend_state();
            }
        }
    }
}

/// Which pass a part draws in: the frame's colour, or the glow pass's mask
/// (the character-shading spec's glow mask, AE.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Pass {
    #[default]
    Colour,
    GlowMask,
}

/// What a pipeline is specialized on: the masks, the states and the pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialKey {
    pub shader: ShaderKey,
    pub states: StateKey,
    pub pass: Pass,
}

impl MaterialKey {
    pub fn shader_defs(&self) -> Vec<ShaderDefVal> {
        let mut defs = self.shader.shader_defs();
        if self.states.alpha_test {
            defs.push(def::ALPHA_TEST.into());
        }
        defs
    }

    /// The fragment entry point: the shader's `fragment`, or `glow_mask`
    /// in the glow pass.
    pub fn entry_point(&self) -> &'static str {
        match self.pass {
            Pass::Colour => "fragment",
            Pass::GlowMask => "glow_mask",
        }
    }

    /// The pipeline's states and entry point: the part's own states in the
    /// colour pass; in the glow pass its culling, depth write and alpha
    /// test, never blended.
    pub fn apply(&self, descriptor: &mut RenderPipelineDescriptor) {
        self.states.apply(descriptor);
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.entry_point = Some(self.entry_point().into());
            if self.pass == Pass::GlowMask {
                for target in fragment.targets.iter_mut().flatten() {
                    target.blend = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{
        ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, FragmentState,
        TextureFormat,
    };

    fn names(key: &MaterialKey) -> Vec<String> {
        key.shader_defs()
            .into_iter()
            .map(|d| match d {
                ShaderDefVal::Bool(name, true) => name.into_owned(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn key(pixel: u32, vertex: u32) -> MaterialKey {
        MaterialKey {
            shader: ShaderKey::new(pixel, vertex).expect("a valid key"),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        }
    }

    fn mask(base: u32, ignore: u32) -> Mask {
        Mask { base, ignore }
    }

    #[test]
    fn each_character_bit_sets_its_own_def() {
        for (pixel, vertex, want) in [
            (0x0001, 0x0001, vec![def::LAYER2, def::UV1]),
            (0x0002, 0x0002, vec![def::LAYER3, def::UV1]),
            (0x0004, 0x0004, vec![def::RIM, def::GEOMETRIC_NORMAL]),
            (0x0008, 0x0008, vec![def::NORMAL_MAP]),
            (0x0010, 0x0010, vec![def::SPEC_MAP]),
            (
                0x0020,
                0x0020,
                vec![def::DIRECTIONAL, def::GEOMETRIC_NORMAL],
            ),
            (0x0100, 0x0100, vec![def::SHADOWS]),
            (0x0400, 0x0400, vec![def::FOG]),
            (
                0x0800,
                0x2000,
                vec![def::GEOMETRIC_NORMAL, def::POINT_LIGHTS[0]],
            ),
            (0x0000, 0x1000, vec![def::VERTEX_COLOUR]),
            (0x0000, 0x0000, vec![]),
        ] {
            assert_eq!(names(&key(pixel, vertex)), want, "{pixel:#x}/{vertex:#x}");
        }
    }

    #[test]
    fn point_lights_count_up_the_thermometer() {
        for (pixel, n) in [(0x0800, 1), (0x1800, 2), (0x3800, 3)] {
            let k = key(pixel, 0x2000);
            assert_eq!(k.shader.switches().point_lights, n);
            let lights: Vec<String> = names(&k)
                .into_iter()
                .filter(|d| d.starts_with("CHAR_POINT"))
                .collect();
            assert_eq!(lights, def::POINT_LIGHTS[..usize::from(n)]);
        }
    }

    #[test]
    fn the_derived_defs_follow_the_spec() {
        // The eye direction needs a spec map and fog or a point light.
        assert!(names(&key(0x0410, 0x0410)).contains(&def::EYE.to_string()));
        assert!(names(&key(0x0810, 0x2010)).contains(&def::EYE.to_string()));
        assert!(!names(&key(0x0010, 0x0010)).contains(&def::EYE.to_string()));
        assert!(!names(&key(0x0408, 0x0408)).contains(&def::EYE.to_string()));
        // A third layer without a second reads the second set; with one,
        // the third.
        assert_eq!(key(0x0002, 0x0002).shader.uv_sets(), 2);
        assert_eq!(key(0x0003, 0x0003).shader.uv_sets(), 3);
        assert!(names(&key(0x0003, 0x0003)).contains(&def::UV2.to_string()));
        // Eruptor's body as the turntable draws it.
        assert_eq!(
            names(&key(0x041C, 0x041C)),
            [
                def::RIM,
                def::NORMAL_MAP,
                def::SPEC_MAP,
                def::FOG,
                def::EYE,
                def::GEOMETRIC_NORMAL
            ]
        );
    }

    #[test]
    fn the_masks_and_the_model_agree_on_the_eye_direction_and_the_normal() {
        // The shader's two derived defs come off the vertex mask, the
        // model's off its switches: two readings of the spec's conditions,
        // equal on every key (64 layer and lighting sets, shadows, fog and
        // 0-3 point lights, with and without vertex colour).
        let mut keys = 0;
        for pixel in 0u32..1 << 14 {
            let lit = u32::from(pixel >> 11 & 0b111 != 0) << 13;
            for colour in [0, 1 << 12] {
                let Ok(k) = ShaderKey::new(pixel, pixel & SHARED | lit | colour) else {
                    continue;
                };
                let s = k.switches();
                assert_eq!(k.writes_eye(), s.eye_direction(), "{pixel:#06x}");
                assert_eq!(k.writes_normal(), s.geometric_normal(), "{pixel:#06x}");
                keys += 1;
            }
        }
        assert_eq!(keys, 64 * 2 * 2 * 4 * 2);
    }

    #[test]
    fn the_alpha_test_is_a_def_of_its_own() {
        let mut k = key(0x0424, 0x0424);
        assert!(!names(&k).contains(&def::ALPHA_TEST.to_string()));
        k.states.alpha_test = true;
        assert!(names(&k).contains(&def::ALPHA_TEST.to_string()));
    }

    #[test]
    fn the_point_light_bits_count_up_from_eleven() {
        assert_eq!(
            ShaderKey::new(0x1000, 0x2000),
            Err(KeyError::Requires { bit: 12, needs: 11 })
        );
        assert_eq!(
            ShaderKey::new(0x2800, 0x2000),
            Err(KeyError::Requires { bit: 13, needs: 12 })
        );
        assert_eq!(
            ShaderKey::new(0x0800, 0x0000),
            Err(KeyError::PointLit {
                vertex: false,
                pixel: 1
            })
        );
        assert_eq!(
            ShaderKey::new(0x0000, 0x2000),
            Err(KeyError::PointLit {
                vertex: true,
                pixel: 0
            })
        );
    }

    #[test]
    fn features_no_character_has_are_refused_by_name() {
        for (bit, name) in [
            (6, "rim by position"),
            (7, "a gloss map"),
            (9, "reflections"),
        ] {
            let m = 1 << bit | 0x4;
            assert_eq!(
                ShaderKey::new(m, m),
                Err(KeyError::Feature {
                    side: Side::Pixel,
                    bit,
                    name
                })
            );
        }
        assert_eq!(
            ShaderKey::new(0, 1 << 11),
            Err(KeyError::Feature {
                side: Side::Vertex,
                bit: 11,
                name: "skinning in the shader"
            })
        );
        assert_eq!(
            ShaderKey::new(1 << 14, 0),
            Err(KeyError::Unknown {
                side: Side::Pixel,
                bit: 14
            })
        );
        assert_eq!(
            ShaderKey::new(0x3C, 0x1C),
            Err(KeyError::Disagree {
                pixel: 0x3C,
                vertex: 0x1C
            })
        );
    }

    #[test]
    fn a_drawn_mask_takes_the_scenes_bits_outside_the_ignore_mask() {
        // Eruptor's body (AE.5): 0x1C / 0x2FF and 0x1C / 0x1AFF.
        let (p, v) = (mask(0x1C, 0x2FF), mask(0x1C, 0x1AFF));
        let turntable = ShaderKey::resolve(p, v, SceneBits::TURNTABLE).unwrap();
        assert_eq!((turntable.pixel(), turntable.vertex()), (0x41C, 0x41C));
        let everything = SceneBits {
            shadows: true,
            fog: true,
            point_lights: 3,
        };
        let lit = ShaderKey::resolve(p, v, everything).unwrap();
        assert_eq!((lit.pixel(), lit.vertex()), (0x3D1C, 0x251C));
        // 0x3AFF keeps point lights off whatever the scene has.
        let owned = ShaderKey::resolve(mask(0x3C, 0x3AFF), mask(0x3C, 0x3AFF), everything).unwrap();
        assert_eq!((owned.pixel(), owned.vertex()), (0x53C, 0x53C));
        // A vertex colour is the material's own.
        let coloured = ShaderKey::resolve(
            mask(0x3C, 0x2FF),
            mask(0x103C, 0x1AFF),
            SceneBits::TURNTABLE,
        )
        .unwrap();
        assert_eq!(coloured.vertex(), 0x143C);
        assert!(coloured.switches().vertex_colour);
        assert_eq!(
            ShaderKey::resolve(mask(0, 0x0AFF), mask(0, 0x1AFF), everything),
            Err(KeyError::SplitPointLights {
                side: Side::Pixel,
                ignore: 0x0AFF
            })
        );
    }

    #[test]
    fn switches_and_masks_round_trip() {
        for pixel in 0u32..1 << 14 {
            let vertex_shared = pixel & SHARED;
            let lit = u32::from(pixel >> 11 & 0b111 != 0) << 13;
            for colour in [0, 1 << 12] {
                let Ok(k) = ShaderKey::new(pixel, vertex_shared | lit | colour) else {
                    continue;
                };
                assert_eq!(ShaderKey::from_switches(&k.switches()), Ok(k));
            }
        }
    }

    fn descriptor() -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            fragment: Some(FragmentState {
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn render_states_set_the_pipeline() {
        let target = |d: &RenderPipelineDescriptor| {
            d.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
        };
        let depth =
            |d: &RenderPipelineDescriptor| d.depth_stencil.as_ref().unwrap().depth_write_enabled;

        let mut d = descriptor();
        StateKey::OPAQUE.apply(&mut d);
        assert_eq!(target(&d), None);
        assert_eq!(depth(&d), Some(true));
        assert_eq!(d.primitive.cull_mode, Some(Face::Back));
        assert_eq!(StateKey::OPAQUE.alpha_mode(0.5), AlphaMode::Opaque);

        let blended = StateKey {
            blend: Blend::Alpha,
            depth_write: true,
            two_sided: true,
            alpha_test: true,
        };
        let mut d = descriptor();
        blended.apply(&mut d);
        let b = target(&d).unwrap();
        assert_eq!(
            (b.color.src_factor, b.color.dst_factor),
            (BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha)
        );
        assert_eq!(b.alpha, BlendComponent::OVER);
        assert_eq!(depth(&d), Some(true));
        assert_eq!(d.primitive.cull_mode, None);
        assert_eq!(blended.alpha_mode(0.5), AlphaMode::Blend);

        let additive = StateKey {
            blend: Blend::Additive,
            depth_write: false,
            two_sided: false,
            alpha_test: false,
        };
        let mut d = descriptor();
        additive.apply(&mut d);
        let b = target(&d).unwrap();
        assert_eq!(
            (b.color.src_factor, b.color.dst_factor),
            (BlendFactor::SrcAlpha, BlendFactor::One)
        );
        assert_eq!(
            (b.alpha.src_factor, b.alpha.dst_factor),
            (BlendFactor::Zero, BlendFactor::One)
        );
        assert_eq!(depth(&d), Some(false));
        assert_eq!(d.primitive.cull_mode, Some(Face::Back));
        assert_eq!(additive.alpha_mode(0.5), AlphaMode::Add);

        let tested = StateKey {
            alpha_test: true,
            ..StateKey::OPAQUE
        };
        assert_eq!(tested.alpha_mode(0.25), AlphaMode::Mask(0.25));
    }

    /// A level's multiplied part (AG.15) through a material key (AE.11's
    /// passes): DESTINATION_COLOR, ZERO with the alpha kept in the colour
    /// pass, never blended in the glow pass.
    #[test]
    fn a_multiplied_part_scales_whats_there_and_its_glow_pass_doesnt() {
        let target = |d: &RenderPipelineDescriptor| {
            d.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
        };
        let colour = MaterialKey {
            states: StateKey {
                blend: Blend::Multiply,
                depth_write: false,
                two_sided: false,
                alpha_test: false,
            },
            ..key(0, 0)
        };
        let mut d = descriptor();
        colour.apply(&mut d);
        let b = target(&d).unwrap();
        assert_eq!(
            (b.color.src_factor, b.color.dst_factor),
            (BlendFactor::Dst, BlendFactor::Zero)
        );
        assert_eq!(
            (b.alpha.src_factor, b.alpha.dst_factor),
            (BlendFactor::Zero, BlendFactor::One)
        );
        assert_eq!(colour.states.alpha_mode(0.5), AlphaMode::Multiply);
        let glow = MaterialKey {
            pass: Pass::GlowMask,
            ..colour
        };
        let mut d = descriptor();
        glow.apply(&mut d);
        assert_eq!(target(&d), None);
        assert_eq!(
            d.fragment.as_ref().unwrap().entry_point.as_deref(),
            Some("glow_mask")
        );
        assert_eq!(
            d.depth_stencil.as_ref().unwrap().depth_write_enabled,
            Some(false)
        );
    }

    #[test]
    fn the_irs_states_become_the_key() {
        let s = RenderStates {
            blend: Blend::Alpha,
            depth_write: false,
            two_sided: true,
            alpha_test: Some(0.1),
        };
        assert_eq!(
            StateKey::from(&s),
            StateKey {
                blend: Blend::Alpha,
                depth_write: false,
                two_sided: true,
                alpha_test: true
            }
        );
    }
}
