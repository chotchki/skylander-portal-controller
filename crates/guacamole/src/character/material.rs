//! The character material (design decision 9): one WESL shader
//! ([`shading.wesl`](shading.wesl)) specialized per [`MaterialKey`], its
//! uniform packed from the reference model's own types
//! ([`guac_ir::shading`]), so the GPU lane feeds the shader and the model
//! the same numbers.
//!
//! The scene rides in each material's uniform: a turntable's rig is fixed
//! to its camera (design decision 14), so its lights are constants in view
//! space, the space the shader lights in.

use bevy::asset::{Asset, Handle, uuid_handle};
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::math::Vec4;
use bevy::mesh::{
    Mesh, MeshVertexBufferLayoutRef, MissingVertexAttributeError, VertexAttributeDescriptor,
};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use guac_ir::model::{self, Mask, TextureRole};
use guac_ir::shading::{
    Fixed, Glow, Layer, Rim, Scene, SecondLayer, ThirdLayer, UvTransform, Weights,
};

use super::key::{KeyError, MaterialKey, Pass, SceneBits, ShaderKey, StateKey, bit};
use super::mesh::ATTRIBUTE_UV_2;

/// The shader, embedded by [`super::CharacterShadingPlugin`].
pub const SHADER: &str = "embedded://guacamole/character/shading.wesl";

/// `misc.w` for lights in view space: a turntable's rig, fixed to its camera,
/// as the scene gives them.
pub const LIGHTS_IN_VIEW: f32 = 0.0;
/// `misc.w` for lights in the world's frame, which the shader turns by the
/// view it draws for: a level's (AG.21), so a camera that moves rewrites no
/// material.
pub const LIGHTS_IN_WORLD: f32 = 1.0;
/// `misc.w` for lights in the world's frame with the point lights read off
/// the scene's buffer ([`CharacterMaterial::scene_lights`]) rather than the
/// uniform (openspec AH.65): a level's, whose point lights the scripts move
/// every frame, so a light that moves rewrites one buffer, no material.
pub const LIGHTS_FROM_SCENE: f32 = 2.0;

/// The scene buffer of a material that reads none ([`LIGHTS_FROM_SCENE`]
/// off): every point light dark. Inserted by
/// [`super::CharacterShadingPlugin`].
pub const NO_SCENE_LIGHTS: Handle<ShaderBuffer> =
    uuid_handle!("6c0d6f3e-3a4b-4f0e-9a7c-6a1f2d5b7e01");
/// The level's scene buffer (openspec AH.65): its placed point lights and
/// those the scripts light, the three a frame draws, in the world's frame.
pub const LEVEL_LIGHTS: Handle<ShaderBuffer> = uuid_handle!("6c0d6f3e-3a4b-4f0e-9a7c-6a1f2d5b7e02");

/// A scene's three point lights as the shader's `SceneLights` reads them
/// off the buffer: the uniform's three arrays, packed as it packs them.
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct ScenePointLights {
    pub position: [Vec4; 3],
    pub colour: [Vec4; 3],
    /// x over (d + 0.001), y over (d + 0.001)².
    pub falloff: [Vec4; 3],
}

impl ScenePointLights {
    /// Up to three of `points`, the rest dark.
    pub fn new(points: &[guac_ir::shading::PointLight]) -> Self {
        let point = |i: usize, f: fn(&guac_ir::shading::PointLight) -> Vec4| {
            points.get(i).map_or(Vec4::ZERO, f)
        };
        Self {
            position: [0, 1, 2]
                .map(|i| point(i, |p| bevy::math::Vec3::from_array(p.position).extend(0.0))),
            colour: [0, 1, 2]
                .map(|i| point(i, |p| bevy::math::Vec3::from_array(p.colour).extend(0.0))),
            falloff: [0, 1, 2].map(|i| point(i, |p| Vec4::new(p.linear, p.quadratic, 0.0, 0.0))),
        }
    }

    /// The buffer's words: positions, colours, then falloffs.
    pub fn words(&self) -> Vec<[f32; 4]> {
        self.position
            .iter()
            .chain(&self.colour)
            .chain(&self.falloff)
            .map(|v| v.to_array())
            .collect()
    }

    /// The buffer itself, kept in both worlds so it can be rewritten.
    pub fn buffer(&self) -> ShaderBuffer {
        let mut b = ShaderBuffer::new(self.words(), bevy::asset::RenderAssetUsages::default());
        b.label = "scene point lights".into();
        b
    }
}

/// The material's uniform, in the shader's `Character` layout.
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct CharacterUniform {
    /// Each directional light's way of travel, view space (the world's
    /// when `misc.w` is [`LIGHTS_IN_WORLD`]).
    pub light_travel: [Vec4; 3],
    pub light_ambient: [Vec4; 3],
    pub light_diffuse: [Vec4; 3],
    pub light_specular: [Vec4; 3],
    /// View space, as `light_travel` is.
    pub point_position: [Vec4; 3],
    pub point_colour: [Vec4; 3],
    /// x the falloff's term over (d + 0.001), y its term over (d + 0.001)².
    pub point_falloff: [Vec4; 3],
    pub fog_colour: Vec4,
    /// x near, y far.
    pub fog_range: Vec4,
    pub placement_tint: Vec4,
    /// RGB and its amount.
    pub replace: Vec4,
    /// A view-space position's shadow-map coordinates: u = (P, 1)·shadow_u.
    pub shadow_u: Vec4,
    pub shadow_v: Vec4,
    /// x the shadow mode, y the alpha test's reference, z the glow: its
    /// amount, -1 for layer 1's alpha, 0 for none (as the original's glow
    /// shader takes it), w the lights' frame ([`LIGHTS_IN_VIEW`] or
    /// [`LIGHTS_IN_WORLD`]).
    pub misc: Vec4,
    /// x layer 1's fixed alpha, y layer 2's, z layer 3's strength (or the
    /// gloss map's, which takes its place), w the rim's.
    pub fixed_alpha: Vec4,
    /// x the exponent, y the detail weight.
    pub layer1: Vec4,
    pub layer2: Vec4,
    /// Over, add and multiply.
    pub layer2_weights: Vec4,
    pub layer3_weights: Vec4,
    pub rim_weights: Vec4,
    /// x the lookup's scale, y the colour/alpha split, z what a multiplying
    /// rim's colour takes for its strength: the strength itself, or on a
    /// level's material its power in linear light
    /// ([`Self::multiplies_in_light`]).
    pub rim: Vec4,
    pub material_tint: Vec4,
    /// Each texture-coordinate set's transform, numbered as the set:
    /// u' = (u, v, 1)·uv_u, v' = (u, v, 1)·uv_v.
    pub uv_u: [Vec4; 3],
    pub uv_v: [Vec4; 3],
}

/// Where the shader looks a view-space position up in the shadow map. The
/// reference model takes the sample as given, so this is the GPU's alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLookup {
    pub u: Vec4,
    pub v: Vec4,
}

impl ShadowLookup {
    /// Nothing renders a shadow map in AE: the turntable has shadows off.
    pub const NONE: Self = Self {
        u: Vec4::ZERO,
        v: Vec4::ZERO,
    };
}

impl CharacterUniform {
    pub fn new(
        material: &guac_ir::shading::Material,
        scene: &Scene,
        shadow: ShadowLookup,
        alpha_reference: f32,
    ) -> Self {
        fn v3(c: [f32; 3]) -> Vec4 {
            Vec4::new(c[0], c[1], c[2], 0.0)
        }
        let v4 = Vec4::from_array;
        let weights = |w: Weights| Vec4::new(w.over, w.add, w.multiply, 0.0);
        let layer = |l: &Layer| Vec4::new(l.exponent, l.detail_weight, 0.0, 0.0);
        let lights = &scene.lights;
        let point = |i: usize, f: fn(&guac_ir::shading::PointLight) -> Vec4| {
            scene.point_lights.get(i).map_or(Vec4::ZERO, f)
        };
        let unset = Weights::new(0.0, 0.0, 0.0);
        let second = material.second.as_ref();
        let third = material.third.as_ref();
        let rim = material.rim.as_ref();
        let (uv_u, uv_v) = Self::uv_rows(&material.uv_transforms);
        Self {
            light_travel: lights.map(|l| v3(l.travel)),
            light_ambient: lights.map(|l| v3(l.ambient)),
            light_diffuse: lights.map(|l| v3(l.diffuse)),
            light_specular: lights.map(|l| v3(l.specular)),
            point_position: [0, 1, 2].map(|i| point(i, |p| v3(p.position))),
            point_colour: [0, 1, 2].map(|i| point(i, |p| v3(p.colour))),
            point_falloff: [0, 1, 2]
                .map(|i| point(i, |p| Vec4::new(p.linear, p.quadratic, 0.0, 0.0))),
            fog_colour: v3(scene.fog_colour),
            fog_range: Vec4::new(scene.fog_near, scene.fog_far, 0.0, 0.0),
            placement_tint: v4(scene.placement_tint),
            replace: v4(scene.replace),
            shadow_u: shadow.u,
            shadow_v: shadow.v,
            misc: Vec4::new(
                scene.shadow_mode,
                alpha_reference,
                match material.glow {
                    Glow::None => 0.0,
                    Glow::Amount(a) => a,
                    Glow::LayerAlpha => -1.0,
                },
                LIGHTS_IN_VIEW,
            ),
            fixed_alpha: Vec4::new(
                material.first.fixed_alpha,
                second.map_or(-1.0, |s| s.layer.fixed_alpha),
                third
                    .map(|t| t.strength)
                    .or(rim.and_then(|r| r.gloss))
                    .unwrap_or(0.0),
                rim.map_or(0.0, |r| r.strength),
            ),
            layer1: layer(&material.first),
            layer2: second.map_or(Vec4::ZERO, |s| layer(&s.layer)),
            layer2_weights: weights(second.map_or(unset, |s| s.weights)),
            layer3_weights: weights(third.map_or(unset, |t| t.weights)),
            rim_weights: weights(rim.map_or(unset, |r| r.weights)),
            rim: rim.map_or(Vec4::ZERO, |r| Vec4::new(r.scale, r.split, r.strength, 0.0)),
            material_tint: v4(material.tint),
            uv_u,
            uv_v,
        }
    }

    /// The uniform with its lights read in the world's frame
    /// ([`LIGHTS_IN_WORLD`]): for a scene whose light travels and point-light
    /// positions are the world's.
    pub fn lit_in_world(mut self) -> Self {
        self.misc.w = LIGHTS_IN_WORLD;
        self
    }

    /// The uniform with its lights read in the world's frame and its point
    /// lights off the scene's buffer ([`LIGHTS_FROM_SCENE`]).
    pub fn lit_by_scene(mut self) -> Self {
        self.misc.w = LIGHTS_FROM_SCENE;
        self
    }

    /// The uniform with a multiplying rim's colour scaled by its strength
    /// in linear light ([`super::remaster::in_light`]), as a level's
    /// materials draw (openspec AG.79): the original multiplies stored
    /// values, and its strength on decoded ones is its 2.2nd power. Its
    /// alpha keeps the strength; a gloss map's rim reads its own.
    pub fn multiplies_in_light(mut self) -> Self {
        self.rim.z = super::remaster::in_light(self.fixed_alpha.w);
        self
    }

    /// The sets' transforms as the shader's `uv_u` and `uv_v` rows; a scroll
    /// writes these each frame ([`guac_ir::scroll::transforms`]).
    pub fn uv_rows(transforms: &[UvTransform; 3]) -> ([Vec4; 3], [Vec4; 3]) {
        let row = |r: [f32; 3]| Vec4::new(r[0], r[1], r[2], 0.0);
        (transforms.map(|t| row(t.u)), transforms.map(|t| row(t.v)))
    }
}

/// A material's textures by role, each loaded with the view its role
/// samples through: colour, spec, rim and the layers' colour maps as sRGB
/// (the remaster decodes them), normal maps and the shadow map as stored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterTextures {
    pub colour: Option<Handle<Image>>,
    pub normal_map: Option<Handle<Image>>,
    pub spec_map: Option<Handle<Image>>,
    pub second_colour: Option<Handle<Image>>,
    pub second_normal_map: Option<Handle<Image>>,
    pub second_spec_map: Option<Handle<Image>>,
    pub third: Option<Handle<Image>>,
    pub rim: Option<Handle<Image>>,
    pub shadow: Option<Handle<Image>>,
}

impl CharacterTextures {
    /// The slot an IR role fills.
    pub fn slot(&mut self, role: TextureRole) -> &mut Option<Handle<Image>> {
        match role {
            TextureRole::Layer1ColorMap => &mut self.colour,
            TextureRole::Layer1BaseNormalMap => &mut self.normal_map,
            TextureRole::Layer1BaseSpecMap => &mut self.spec_map,
            TextureRole::Layer2ColorMap => &mut self.second_colour,
            TextureRole::Layer2BaseNormalMap => &mut self.second_normal_map,
            TextureRole::Layer2BaseSpecMap => &mut self.second_spec_map,
            TextureRole::Layer3ColorMap => &mut self.third,
            TextureRole::Layer4ColorMap => &mut self.rim,
        }
    }

    /// The first texture `key` samples that isn't here.
    fn missing(&self, key: &ShaderKey) -> Option<&'static str> {
        let s = key.switches();
        [
            (true, self.colour.is_some(), "colour map"),
            (s.normal_map, self.normal_map.is_some(), "normal map"),
            (s.spec_map, self.spec_map.is_some(), "spec map"),
            (s.second, self.second_colour.is_some(), "second colour map"),
            (
                s.second && s.normal_map,
                self.second_normal_map.is_some(),
                "second normal map",
            ),
            (
                s.second && s.spec_map,
                self.second_spec_map.is_some(),
                "second spec map",
            ),
            (s.third, self.third.is_some(), "third layer's map"),
            (s.gloss, self.third.is_some(), "gloss map"),
            (s.rim, self.rim.is_some(), "rim map"),
            (s.shadows, self.shadow.is_some(), "shadow map"),
        ]
        .into_iter()
        .find_map(|(needed, there, name)| (needed && !there).then_some(name))
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MaterialError {
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error("the material's {0} isn't there")]
    MissingTexture(&'static str),
    #[error("the material has a {0} but not its values")]
    MissingValues(&'static str),
    #[error("the base mask turns on point lights the material doesn't own: {0:#06x}")]
    ForcedPointLights(u32),
}

/// The tfbmulti2 character material: the key the pipeline specializes on,
/// the uniform and the textures.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(MaterialKey)]
pub struct CharacterMaterial {
    #[uniform(0)]
    pub uniform: CharacterUniform,
    #[texture(1, visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    pub colour: Option<Handle<Image>>,
    #[texture(3, visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    pub normal_map: Option<Handle<Image>>,
    #[texture(5, visibility(fragment))]
    #[sampler(6, visibility(fragment))]
    pub spec_map: Option<Handle<Image>>,
    #[texture(7, visibility(fragment))]
    #[sampler(8, visibility(fragment))]
    pub second_colour: Option<Handle<Image>>,
    #[texture(9, visibility(fragment))]
    #[sampler(10, visibility(fragment))]
    pub second_normal_map: Option<Handle<Image>>,
    #[texture(11, visibility(fragment))]
    #[sampler(12, visibility(fragment))]
    pub second_spec_map: Option<Handle<Image>>,
    #[texture(13, visibility(fragment))]
    #[sampler(14, visibility(fragment))]
    pub third: Option<Handle<Image>>,
    #[texture(15, visibility(fragment))]
    #[sampler(16, visibility(fragment))]
    pub rim: Option<Handle<Image>>,
    #[texture(17, visibility(fragment))]
    #[sampler(18, visibility(fragment))]
    pub shadow: Option<Handle<Image>>,
    /// The scene's point lights when the uniform says to read them here
    /// ([`LIGHTS_FROM_SCENE`]); [`NO_SCENE_LIGHTS`] otherwise.
    #[storage(19, read_only, visibility(fragment))]
    pub scene_lights: Handle<ShaderBuffer>,
    pub key: MaterialKey,
    /// Whether the part has a glow attribute (its glow is an amount or
    /// layer 1's alpha, not none): what [`Self::glow_mask`] reads.
    pub glows: bool,
    /// Added to a blended part's view depth when Bevy sorts its pass (its
    /// [`Material::depth_bias`]): 0 but for a level's sky, which draws
    /// first in the scene's order (AG.79, `level_scene::sky_bias`).
    pub depth_bias: f32,
}

impl From<&CharacterMaterial> for MaterialKey {
    fn from(m: &CharacterMaterial) -> Self {
        m.key
    }
}

impl CharacterMaterial {
    /// A material for `key`, its numbers from the reference model's types.
    /// Refuses a key whose textures aren't all there, since an unbound slot
    /// would sample Bevy's white fallback and draw something wrong quietly.
    pub fn new(
        key: MaterialKey,
        material: &guac_ir::shading::Material,
        scene: &Scene,
        shadow: ShadowLookup,
        alpha_reference: f32,
        textures: CharacterTextures,
    ) -> Result<Self, MaterialError> {
        if let Some(name) = textures.missing(&key.shader) {
            return Err(MaterialError::MissingTexture(name));
        }
        Ok(Self {
            uniform: CharacterUniform::new(material, scene, shadow, alpha_reference),
            colour: textures.colour,
            normal_map: textures.normal_map,
            spec_map: textures.spec_map,
            second_colour: textures.second_colour,
            second_normal_map: textures.second_normal_map,
            second_spec_map: textures.second_spec_map,
            third: textures.third,
            rim: textures.rim,
            shadow: textures.shadow,
            scene_lights: NO_SCENE_LIGHTS,
            key,
            glows: material.glow != Glow::None,
            depth_bias: 0.0,
        })
    }

    /// The same part in the glow pass, drawing its glow mask; `None` for a
    /// part without a glow attribute that writes no depth. The original
    /// draws only the parts with the attribute into the mask, tested against
    /// the colour pass's depth, so a part without one hides glow behind it
    /// only where it wrote depth (the character-shading spec's glow mask).
    pub fn glow_mask(&self) -> Option<Self> {
        if !self.glows && !self.key.states.depth_write {
            return None;
        }
        let mut m = self.clone();
        m.key.pass = Pass::GlowMask;
        Some(m)
    }

    /// The same part's glow mask in a level ([`Pass::LevelGlow`], openspec
    /// AH.65), into the frame's alpha after everything else draws; `None`
    /// for a part without a glow attribute, which writes none (the level's
    /// depth hides what's behind it, and its alpha was cleared to none).
    pub fn level_glow(&self) -> Option<Self> {
        if !self.glows {
            return None;
        }
        let mut m = self.clone();
        m.key.pass = Pass::LevelGlow;
        m.depth_bias = super::glow::LEVEL_GLOW_BIAS;
        Some(m)
    }

    /// An IR material in `scene`: the masks it draws there, its numbers and
    /// its textures, each found by `texture`.
    pub fn from_ir(
        ir: &model::Material,
        scene: &Scene,
        shadow: ShadowLookup,
        mut texture: impl FnMut(&model::TextureSlot) -> Handle<Image>,
    ) -> Result<Self, MaterialError> {
        let bits = SceneBits {
            shadows: scene.shadows,
            fog: scene.fog,
            point_lights: u8::try_from(scene.point_lights.len()).unwrap_or(u8::MAX),
        };
        let key = MaterialKey {
            shader: ShaderKey::resolve(ir.pixel, ir.vertex, bits)?,
            states: StateKey::from(&ir.states),
            pass: Pass::Colour,
        };
        let mut textures = CharacterTextures::default();
        for slot in &ir.textures {
            *textures.slot(slot.role) = Some(texture(slot));
        }
        Self::new(
            key,
            &reference_material(ir)?,
            scene,
            shadow,
            ir.states.alpha_test.unwrap_or(0.0),
            textures,
        )
    }
}

fn mask_bit(mask: u32, b: u32) -> bool {
    mask >> b & 1 == 1
}

/// A scene switch the material owns: its base bit when the ignore mask
/// holds the bit, on when the base sets one it leaves to the scene (the
/// selector ORs the scene's onto it), else the scene's.
fn owned(mask: Mask, b: u32) -> Option<bool> {
    if mask_bit(mask.ignore, b) {
        Some(mask_bit(mask.base, b))
    } else {
        mask_bit(mask.base, b).then_some(true)
    }
}

/// The reference model's material for an IR one: the switches its base
/// masks set, the scene switches its ignore masks own, and its numbers
/// (the character-shading spec's material requirement), each set's
/// transform where its scroll track starts: the turntable moves it from
/// there each frame.
pub fn reference_material(
    ir: &model::Material,
) -> Result<guac_ir::shading::Material, MaterialError> {
    let (p, v) = (ir.pixel, ir.vertex);
    let pr = &ir.params;
    let lights = 0b111 << bit::POINT_LIGHTS;
    let point_lights = if p.ignore & lights == lights {
        Some((p.base & lights).count_ones() as u8)
    } else if p.base & lights != 0 {
        return Err(MaterialError::ForcedPointLights(p.base));
    } else {
        None
    };
    let weights = |w: [f32; 4]| Weights::new(w[0], w[1], w[2]);
    let second = if mask_bit(p.base, bit::LAYER2) {
        let extra = pr
            .layer2_extra
            .ok_or(MaterialError::MissingValues("second layer"))?;
        let blend = pr
            .layer2_blend
            .ok_or(MaterialError::MissingValues("second layer's blend"))?;
        Some(SecondLayer {
            layer: Layer {
                exponent: extra[2],
                detail_weight: extra[3],
                fixed_alpha: pr.fixed_alpha[1],
            },
            weights: weights(blend),
        })
    } else {
        None
    };
    let third = if mask_bit(p.base, bit::LAYER3) {
        let blend = pr
            .layer3_blend
            .ok_or(MaterialError::MissingValues("third layer's blend"))?;
        Some(ThirdLayer {
            strength: pr.fixed_alpha[2],
            weights: weights(blend),
        })
    } else {
        None
    };
    Ok(guac_ir::shading::Material {
        normal_map: mask_bit(p.base, bit::NORMAL_MAP),
        spec_map: mask_bit(p.base, bit::SPEC_MAP),
        directional: mask_bit(p.base, bit::DIRECTIONAL),
        vertex_colour: mask_bit(v.base, bit::VERTEX_COLOUR),
        first: Layer {
            exponent: pr.layer1_extra[2],
            detail_weight: pr.layer1_extra[3],
            fixed_alpha: pr.fixed_alpha[0],
        },
        second,
        third,
        rim: mask_bit(p.base, bit::RIM).then(|| Rim {
            scale: pr.layer4_values[1],
            strength: pr.fixed_alpha[3],
            weights: weights(pr.layer4_blend),
            split: pr.layer4_values[3],
            // A gloss map's strength is the third layer's fixed alpha.
            gloss: mask_bit(p.base, bit::GLOSS_MAP).then_some(pr.fixed_alpha[2]),
        }),
        tint: super::remaster::tint(pr.material_tint),
        glow: match pr.glow {
            model::Glow::None => Glow::None,
            model::Glow::Amount(a) => Glow::Amount(f32::from(a) / 255.0),
            model::Glow::LayerAlpha => Glow::LayerAlpha,
        },
        uv_transforms: guac_ir::scroll::transforms(&ir.scrolls, 0.0),
        fixed: Fixed {
            shadows: owned(p, bit::SHADOWS),
            fog: owned(p, bit::FOG),
            point_lights,
        },
    })
}

/// The vertex buffer's attributes for `key` on a mesh of this layout, at
/// the shader's locations; a mesh missing one the key reads is refused.
pub fn vertex_attributes(
    key: &ShaderKey,
    layout: &MeshVertexBufferLayoutRef,
) -> Vec<VertexAttributeDescriptor> {
    let s = key.switches();
    let mut attributes = vec![
        Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
        Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
    ];
    if key.uv_sets() > 1 {
        attributes.push(Mesh::ATTRIBUTE_UV_1.at_shader_location(3));
    }
    if s.normal_map {
        attributes.push(Mesh::ATTRIBUTE_TANGENT.at_shader_location(4));
    }
    if s.vertex_colour {
        attributes.push(Mesh::ATTRIBUTE_COLOR.at_shader_location(5));
    }
    // Bevy's mesh pipeline turns skinning on (the SKINNED def, the joint
    // palette's bind group) when the mesh carries both.
    if layout.0.contains(Mesh::ATTRIBUTE_JOINT_INDEX)
        && layout.0.contains(Mesh::ATTRIBUTE_JOINT_WEIGHT)
    {
        attributes.push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(6));
        attributes.push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(7));
    }
    if key.uv_sets() > 2 {
        attributes.push(ATTRIBUTE_UV_2.at_shader_location(8));
    }
    attributes
}

/// Sets `descriptor` up for `key`: the shader defs on both stages, the
/// vertex buffer and the render states.
pub fn specialize_for(
    key: &MaterialKey,
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
) -> Result<(), MissingVertexAttributeError> {
    let defs = key.shader_defs();
    descriptor.vertex.shader_defs.extend(defs.iter().cloned());
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.shader_defs.extend(defs);
    }
    descriptor.vertex.buffers = vec![
        layout
            .0
            .get_layout(&vertex_attributes(&key.shader, layout))?,
    ];
    key.apply(descriptor);
    Ok(())
}

impl Material for CharacterMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // A level's glow mask sorts with the blended parts, after them all
        // ([`super::glow::LEVEL_GLOW_BIAS`]).
        if self.key.pass == Pass::LevelGlow {
            return AlphaMode::Blend;
        }
        self.key.states.alpha_mode(self.uniform.misc.y)
    }

    fn depth_bias(&self) -> f32 {
        self.depth_bias
    }

    // Bevy's prepass and shadow shaders read its own vertex layout and
    // material, not this one; nothing in AE needs either.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        specialize_for(&key.bind_group_data, descriptor, layout)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::mesh::segment_mesh;
    use bevy::mesh::MeshVertexBufferLayouts;
    use bevy::render::render_resource::{
        ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Face, FragmentState,
        TextureFormat,
    };
    use guac_ir::model::{Blend, Filter, Params, RenderStates, Sampler, TextureSlot, Wrap};
    use guac_ir::shading::{DirectionalLight, PointLight};

    fn slot(role: TextureRole) -> TextureSlot {
        TextureSlot {
            role,
            texture: format!("textures/{role:?}.ktx2"),
            image: format!("{role:?}"),
            sampler: Sampler {
                wrap_u: Wrap::Repeat,
                wrap_v: Wrap::Repeat,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: Some(Filter::Linear),
            },
        }
    }

    /// Eruptor's body as AE.6 reads it off the disc (ae-shader section 2).
    fn eruptor_body() -> model::Material {
        model::Material {
            name: "EruptorBody".into(),
            pixel: Mask {
                base: 0x1C,
                ignore: 0x2FF,
            },
            vertex: Mask {
                base: 0x1C,
                ignore: 0x1AFF,
            },
            textures: vec![
                slot(TextureRole::Layer1BaseNormalMap),
                slot(TextureRole::Layer1BaseSpecMap),
                slot(TextureRole::Layer1ColorMap),
                slot(TextureRole::Layer4ColorMap),
            ],
            params: Params {
                fixed_alpha: [-1.0, -1.0, -1.0, 1.0],
                layer1_extra: [1.0, 1.0, 1.0, 1.0],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: None,
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: model::Glow::Amount(64),
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

    fn fogged() -> Scene {
        Scene {
            fog: true,
            ..Scene::new()
        }
    }

    #[test]
    fn eruptors_body_becomes_the_spec_material() {
        let m = reference_material(&eruptor_body()).unwrap();
        assert!(m.normal_map && m.spec_map && !m.directional && !m.vertex_colour);
        assert_eq!(
            (m.first.exponent, m.first.detail_weight, m.first.fixed_alpha),
            (1.0, 1.0, -1.0)
        );
        assert_eq!(m.rim, Some(Rim::new(1.0)));
        assert_eq!(m.tint, [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0]);
        assert_eq!(m.glow, Glow::Amount(64.0 / 255.0));
        assert_eq!(m.second, None);
        assert_eq!(m.third, None);
        // 0x2FF leaves shadows, fog and point lights to the scene.
        assert_eq!(m.fixed, Fixed::default());
    }

    #[test]
    fn the_spikes_multiply_their_rim_at_twice_the_strength() {
        let mut ir = eruptor_body();
        ir.pixel.base = 0x24;
        ir.vertex.base = 0x24;
        ir.params.layer4_blend = [0.0, 0.0, 1.0, 0.0];
        ir.params.fixed_alpha = [-1.0, -1.0, -1.0, 2.0];
        ir.params.material_tint = [0x91, 0x91, 0x91, 0xFF];
        let m = reference_material(&ir).unwrap();
        assert!(m.directional && !m.normal_map && !m.spec_map);
        let rim = m.rim.unwrap();
        assert_eq!((rim.strength, rim.weights), (2.0, Weights::MULTIPLY));
        assert_eq!(m.tint[0], 145.0 / 255.0);
    }

    #[test]
    fn the_model_and_the_selector_agree_on_what_a_scene_draws() {
        // Every base mask AE.5 counted, under both ignore pairs, in every
        // scene the runtime can set: the model's switches resolved from
        // the material's ownership equal the masks the selector draws.
        let bases = [0x00, 0x04, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C, 0x38, 0x3C];
        for base in bases {
            for (pi, vi) in [(0x2FF, 0x1AFF), (0x3AFF, 0x3AFF)] {
                for colour in [0, 0x1000] {
                    let mut ir = eruptor_body();
                    ir.pixel = Mask { base, ignore: pi };
                    ir.vertex = Mask {
                        base: base | colour,
                        ignore: vi,
                    };
                    let m = reference_material(&ir).unwrap();
                    for shadows in [false, true] {
                        for fog in [false, true] {
                            for lights in 0..=3u8 {
                                let mut scene = Scene::new();
                                scene.shadows = shadows;
                                scene.fog = fog;
                                scene.point_lights = vec![
                                    PointLight {
                                        position: [0.0; 3],
                                        colour: [0.0; 3],
                                        linear: 0.0,
                                        quadratic: 0.0,
                                    };
                                    usize::from(lights)
                                ];
                                let bits = SceneBits {
                                    shadows,
                                    fog,
                                    point_lights: lights,
                                };
                                let key = ShaderKey::resolve(ir.pixel, ir.vertex, bits).unwrap();
                                let model = guac_ir::shading::Switches::resolve(&m, &scene);
                                assert_eq!(key.switches(), model, "{base:#x}/{pi:#x} {bits:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// Chapter 1's one gloss-mapped material, "leaves", as the extract
    /// reads it (AG.22): normal, spec and colour maps, an ambient-occlusion
    /// map in the third layer's slot as the gloss map at strength 1, the rim
    /// added at 0.125, vertex colours, alpha-tested.
    fn leaves() -> model::Material {
        let mut m = eruptor_body();
        m.name = "leaves".into();
        m.pixel = Mask {
            base: 0x9C,
            ignore: 0x3FF,
        };
        m.vertex = Mask {
            base: 0x109C,
            ignore: 0x1BFF,
        };
        m.textures.push(slot(TextureRole::Layer3ColorMap));
        m.params.fixed_alpha = [-1.0, -1.0, 1.0, 0.125];
        m.params.layer1_extra = [1.0, 1.0, 1.0, 0.25];
        m.params.layer4_values = [0.0, 1.0, 1.0, 0.0];
        m.params.glow = model::Glow::None;
        m.states.alpha_test = Some(0.964706);
        m
    }

    #[test]
    fn the_leaves_gloss_map_weighs_their_rim() {
        let m = reference_material(&leaves()).unwrap();
        let rim = m.rim.unwrap();
        assert_eq!(
            (rim.strength, rim.weights, rim.gloss, rim.split),
            (0.125, Weights::ADD, Some(1.0), 0.0)
        );
        assert_eq!(m.third, None, "the gloss map is no third layer");
        assert!(m.vertex_colour && m.normal_map && m.spec_map);
        // Fog and the point light from the level's scene; shadows owned off.
        let mut scene = fogged();
        scene.point_lights.push(PointLight {
            position: [0.0; 3],
            colour: [0.0; 3],
            linear: 0.0,
            quadratic: 0.0,
        });
        let u = CharacterUniform::new(&m, &scene, ShadowLookup::NONE, 0.964706);
        assert_eq!(u.fixed_alpha, Vec4::new(-1.0, -1.0, 1.0, 0.125));
        let mut images = bevy::asset::Assets::<Image>::default();
        let mut seen = Vec::new();
        let material = CharacterMaterial::from_ir(&leaves(), &scene, ShadowLookup::NONE, |s| {
            seen.push(s.role);
            images.add(Image::default())
        })
        .unwrap();
        assert_eq!(
            (material.key.shader.pixel(), material.key.shader.vertex()),
            (0xC9C, 0x349C)
        );
        assert!(material.key.shader.switches().gloss);
        assert!(material.third.is_some() && seen.contains(&TextureRole::Layer3ColorMap));
        assert_eq!(material.alpha_mode(), AlphaMode::Mask(0.964706));
        // Without its map it's refused by name.
        let mut bare = leaves();
        bare.textures
            .retain(|t| t.role != TextureRole::Layer3ColorMap);
        let got = CharacterMaterial::from_ir(&bare, &scene, ShadowLookup::NONE, |_| {
            images.add(Image::default())
        });
        assert_eq!(got.unwrap_err(), MaterialError::MissingTexture("gloss map"));
    }

    #[test]
    fn a_layer_without_its_values_is_refused() {
        let mut ir = eruptor_body();
        ir.pixel.base |= 1;
        ir.vertex.base |= 1;
        assert_eq!(
            reference_material(&ir),
            Err(MaterialError::MissingValues("second layer"))
        );
        ir.params.layer2_extra = Some([1.0, 1.0, 3.0, 0.5]);
        assert_eq!(
            reference_material(&ir),
            Err(MaterialError::MissingValues("second layer's blend"))
        );
        ir.params.layer2_blend = Some([0.0, 0.0, 1.0, 0.0]);
        let second = reference_material(&ir).unwrap().second.unwrap();
        assert_eq!(
            (
                second.layer.exponent,
                second.layer.detail_weight,
                second.weights
            ),
            (3.0, 0.5, Weights::MULTIPLY)
        );
    }

    #[test]
    fn the_uniform_packs_the_models_numbers() {
        let mut m = reference_material(&eruptor_body()).unwrap();
        m.uv_transforms[1] = UvTransform {
            u: [2.0, 0.0, 0.5],
            v: [0.0, 3.0, 0.25],
        };
        let mut scene = fogged();
        scene.fog_near = 30.48;
        scene.fog_far = 335.28;
        scene.lights[1] = DirectionalLight {
            travel: [0.0, -1.0, 0.0],
            ambient: [0.1, 0.2, 0.3],
            diffuse: [0.4, 0.5, 0.6],
            specular: [0.7, 0.8, 0.9],
        };
        scene.point_lights.push(PointLight {
            position: [1.0, 2.0, -3.0],
            colour: [0.5, 0.25, 0.125],
            linear: 1.5,
            quadratic: 0.75,
        });
        let u = CharacterUniform::new(&m, &scene, ShadowLookup::NONE, 0.3);
        assert_eq!(u.light_travel[1], Vec4::new(0.0, -1.0, 0.0, 0.0));
        assert_eq!(u.light_specular[1], Vec4::new(0.7, 0.8, 0.9, 0.0));
        assert_eq!(u.point_falloff[0], Vec4::new(1.5, 0.75, 0.0, 0.0));
        assert_eq!(u.point_colour[1], Vec4::ZERO);
        assert_eq!(u.fog_range, Vec4::new(30.48, 335.28, 0.0, 0.0));
        assert_eq!(u.misc, Vec4::new(2.0, 0.3, 64.0 / 255.0, 0.0));
        // The glow as the original's glow shader takes it.
        for (glow, z) in [(Glow::None, 0.0), (Glow::LayerAlpha, -1.0)] {
            m.glow = glow;
            let u = CharacterUniform::new(&m, &scene, ShadowLookup::NONE, 0.3);
            assert_eq!(u.misc.z, z, "{glow:?}");
        }
        assert_eq!(u.fixed_alpha, Vec4::new(-1.0, -1.0, 0.0, 1.0));
        assert_eq!(u.layer1, Vec4::new(1.0, 1.0, 0.0, 0.0));
        assert_eq!(u.rim_weights, Vec4::new(0.0, 1.0, 0.0, 0.0));
        assert_eq!(u.rim, Vec4::new(1.0, 0.0, 1.0, 0.0));
        assert_eq!(u.uv_u[1], Vec4::new(2.0, 0.0, 0.5, 0.0));
        assert_eq!(u.uv_v[1], Vec4::new(0.0, 3.0, 0.25, 0.0));
        assert_eq!(u.uv_u[0], Vec4::new(1.0, 0.0, 0.0, 0.0));
        // The lights' frame: the scene's view space unless read in the
        // world's (a level's, AG.21), which changes nothing else.
        let world = u.lit_in_world();
        assert_eq!((u.misc.w, world.misc.w), (LIGHTS_IN_VIEW, LIGHTS_IN_WORLD));
        assert_eq!(world.misc.truncate(), u.misc.truncate());
        assert_eq!(
            CharacterUniform {
                misc: u.misc,
                ..world
            },
            u
        );
    }

    /// AG.79: a multiplying rim's colour takes its strength as is on a
    /// character and its 2.2nd power on a level's material (the coins'
    /// bronze at 2 multiplies by 4.6 times the decoded map), nothing else
    /// moving; with no rim there's nothing to scale.
    #[test]
    fn a_levels_multiplying_rim_takes_its_strength_in_light() {
        let mut ir = eruptor_body();
        ir.pixel.base = 0x24;
        ir.vertex.base = 0x24;
        ir.params.layer4_blend = [0.0, 0.0, 1.0, 0.0];
        ir.params.fixed_alpha = [-1.0, -1.0, -1.0, 2.0];
        let m = reference_material(&ir).unwrap();
        let u = CharacterUniform::new(&m, &Scene::new(), ShadowLookup::NONE, 0.0);
        assert_eq!((u.rim.z, u.fixed_alpha.w), (2.0, 2.0), "a character's");
        let level = u.lit_in_world().multiplies_in_light();
        assert!(
            (level.rim.z - 2.0f32.powf(2.2)).abs() < 1e-6,
            "{}",
            level.rim.z
        );
        assert_eq!(
            CharacterUniform {
                rim: u.rim,
                ..level
            },
            u.lit_in_world()
        );
        let mut plain = eruptor_body();
        plain.pixel.base = 0x20;
        let p = reference_material(&plain).unwrap();
        let none = CharacterUniform::new(&p, &Scene::new(), ShadowLookup::NONE, 0.0);
        assert_eq!(none.multiplies_in_light().rim, Vec4::ZERO);
    }

    #[test]
    fn a_scroll_track_moves_the_set_its_layer_names_from_its_start() {
        let mut ir = eruptor_body();
        ir.scrolls = vec![model::Scroll {
            layer: 1,
            scale: [2.0, 1.0],
            velocity: [0.0, 0.75],
            amplitude: [0.25, 0.0],
            frequency: [0.0, 0.0],
            phase: [std::f32::consts::FRAC_PI_2, 0.0],
            rotation_phase: 0.0,
        }];
        // At its start: u doubled and swung by 0.25 sin(pi/2), v as stored;
        // the other sets untouched.
        let m = reference_material(&ir).unwrap();
        let start = m.uv_transforms[1];
        assert_eq!(
            (start.u[0], start.u[1], start.v),
            (2.0, 0.0, [0.0, 1.0, 0.0])
        );
        assert!((start.u[2] - 0.25).abs() < 1e-6, "{start:?}");
        assert_eq!(m.uv_transforms[0], UvTransform::IDENTITY);
        assert_eq!(m.uv_transforms[2], UvTransform::IDENTITY);
        // Two seconds in, v has slid 1.5 textures, so the shader's row reads
        // half of one; u's swing is where it started (no frequency).
        let (u, v) = CharacterUniform::uv_rows(&guac_ir::scroll::transforms(&ir.scrolls, 2.0));
        assert_eq!(v[1], Vec4::new(0.0, 1.0, 0.5, 0.0));
        assert!((u[1] - Vec4::new(2.0, 0.0, 0.25, 0.0)).abs().max_element() < 1e-6);
        assert_eq!((u[0], v[0]), (Vec4::X, Vec4::Y));
    }

    #[test]
    fn an_ir_material_finds_each_texture_in_its_role() {
        let ir = eruptor_body();
        let mut images = bevy::asset::Assets::<Image>::default();
        let mut seen = Vec::new();
        let m = CharacterMaterial::from_ir(&ir, &fogged(), ShadowLookup::NONE, |slot| {
            seen.push(slot.role);
            images.add(Image::default())
        })
        .unwrap();
        assert_eq!(seen.len(), 4);
        assert_eq!(
            (m.key.shader.pixel(), m.key.shader.vertex()),
            (0x41C, 0x41C)
        );
        assert_eq!(m.key.states, StateKey::OPAQUE);
        assert!(m.colour.is_some() && m.normal_map.is_some() && m.spec_map.is_some());
        assert!(m.rim.is_some() && m.second_colour.is_none() && m.shadow.is_none());
        assert_eq!(m.alpha_mode(), AlphaMode::Opaque);
    }

    #[test]
    fn a_missing_texture_is_refused() {
        let mut ir = eruptor_body();
        ir.textures
            .retain(|t| t.role != TextureRole::Layer4ColorMap);
        let mut images = bevy::asset::Assets::<Image>::default();
        let got = CharacterMaterial::from_ir(&ir, &fogged(), ShadowLookup::NONE, |_| {
            images.add(Image::default())
        });
        assert_eq!(got.unwrap_err(), MaterialError::MissingTexture("rim map"));
        // Shadows need a shadow map the IR never names.
        let mut shadowed = fogged();
        shadowed.shadows = true;
        let got =
            CharacterMaterial::from_ir(&eruptor_body(), &shadowed, ShadowLookup::NONE, |_| {
                images.add(Image::default())
            });
        assert_eq!(
            got.unwrap_err(),
            MaterialError::MissingTexture("shadow map")
        );
    }

    fn segment(uvs: usize, colours: bool) -> model::Segment {
        model::Segment {
            material: 0,
            source: 0,
            positions: vec![[0.0; 3]; 3],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 3]),
            uvs: vec![vec![[0.0; 2]; 3]; uvs],
            colors: colours.then(|| vec![[255; 4]; 3]),
            joints: vec![[0; 4]; 3],
            weights: vec![[255, 0, 0, 0]; 3],
            indices: vec![0, 1, 2],
        }
    }

    fn layout(s: &model::Segment) -> MeshVertexBufferLayoutRef {
        segment_mesh(s)
            .unwrap()
            .get_mesh_vertex_buffer_layout(&mut MeshVertexBufferLayouts::default())
    }

    fn locations(key: &ShaderKey, s: &model::Segment) -> Vec<u32> {
        let layout = layout(s);
        vertex_attributes(key, &layout)
            .iter()
            .map(|a| a.shader_location)
            .collect()
    }

    #[test]
    fn the_key_picks_the_vertex_attributes() {
        let full = segment(3, true);
        let key = |p: u32, v: u32| ShaderKey::new(p, v).unwrap();
        // Eruptor's body: no second set, no colour.
        assert_eq!(locations(&key(0x41C, 0x41C), &full), [0, 1, 2, 4, 6, 7]);
        // Vertex colour.
        assert_eq!(locations(&key(0x404, 0x1404), &full), [0, 1, 2, 5, 6, 7]);
        // A third layer alone reads the second set; both read the third.
        assert_eq!(locations(&key(0x002, 0x002), &full), [0, 1, 2, 3, 6, 7]);
        assert_eq!(locations(&key(0x003, 0x003), &full), [0, 1, 2, 3, 6, 7, 8]);
        // A gloss map reads the third layer's set: the second here.
        assert_eq!(locations(&key(0x084, 0x084), &full), [0, 1, 2, 3, 6, 7]);
        assert_eq!(locations(&key(0x085, 0x085), &full), [0, 1, 2, 3, 6, 7, 8]);
    }

    #[test]
    fn a_mesh_missing_what_the_key_reads_is_refused() {
        let one_set = segment(1, false);
        let layout = layout(&one_set);
        let key = MaterialKey {
            shader: ShaderKey::new(0x003, 0x003).unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        let mut d = RenderPipelineDescriptor::default();
        assert!(specialize_for(&key, &mut d, &layout).is_err());
        let colour = MaterialKey {
            shader: ShaderKey::new(0x404, 0x1404).unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        assert!(specialize_for(&colour, &mut d, &layout).is_err());
    }

    /// AH.65: a level's glowing part draws its glow mask into the frame's
    /// alpha alone, unblended, tested against the level's depth and
    /// writing none, sorting with the blended parts after the alpha clear
    /// (`level_glow`'s entry, the level's glow pass); its other states and
    /// uniform are the part's. A part without a glow attribute draws none.
    #[test]
    fn a_level_glow_mask_writes_alpha_alone_after_the_blended_parts() {
        let s = segment(1, false);
        let layout = layout(&s);
        let colour = MaterialKey {
            shader: ShaderKey::new(0x41C, 0x41C).unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        let textures = || CharacterTextures {
            colour: Some(Handle::default()),
            normal_map: Some(Handle::default()),
            spec_map: Some(Handle::default()),
            rim: Some(Handle::default()),
            ..Default::default()
        };
        let m = CharacterMaterial::new(
            colour,
            &reference_material(&eruptor_body()).unwrap(),
            &fogged(),
            ShadowLookup::NONE,
            0.5,
            textures(),
        )
        .unwrap();
        let twin = m.level_glow().expect("a glowing part draws its mask");
        assert_eq!(
            twin.key,
            MaterialKey {
                pass: Pass::LevelGlow,
                ..colour
            }
        );
        assert_eq!(twin.uniform, m.uniform);
        assert_eq!(twin.depth_bias, super::super::glow::LEVEL_GLOW_BIAS);
        const { assert!(super::super::glow::LEVEL_GLOW_BIAS > super::super::glow::CLEAR_BIAS) };
        // Opaque as its part is, it sorts with the blended ones.
        assert_eq!(m.alpha_mode(), AlphaMode::Opaque);
        assert_eq!(twin.alpha_mode(), AlphaMode::Blend);
        let mut d = RenderPipelineDescriptor {
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
                    blend: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        };
        specialize_for(&twin.key, &mut d, &layout).unwrap();
        let fragment = d.fragment.as_ref().unwrap();
        assert_eq!(fragment.entry_point, Some("level_glow".into()));
        let target = fragment.targets[0].as_ref().unwrap();
        assert_eq!(
            (target.blend, target.write_mask),
            (None, ColorWrites::ALPHA)
        );
        let depth = d.depth_stencil.as_ref().unwrap();
        assert_eq!(
            (depth.depth_write_enabled, depth.depth_compare),
            (Some(false), Some(CompareFunction::GreaterEqual)),
            "tested, never written"
        );
        assert_eq!(d.primitive.cull_mode, Some(Face::Back));
        // No glow attribute, no mask.
        let plain = CharacterMaterial::new(
            colour,
            &guac_ir::shading::Material {
                glow: Glow::None,
                ..reference_material(&eruptor_body()).unwrap()
            },
            &fogged(),
            ShadowLookup::NONE,
            0.5,
            textures(),
        )
        .unwrap();
        assert!(plain.level_glow().is_none());
    }

    /// AH.65: the scene's buffer packs three point lights as the uniform
    /// packs them, positions, colours, then falloffs, the slots past the
    /// lights given dark; a material reads it once its uniform says so,
    /// and binds the dark one until then.
    #[test]
    fn the_scene_buffer_packs_its_point_lights_as_the_uniform_does() {
        let p = PointLight {
            position: [1.0, 2.0, 3.0],
            colour: [0.5, 0.25, 0.125],
            linear: 1.5,
            quadratic: 0.25,
        };
        let packed = ScenePointLights::new(&[p]);
        let scene = guac_ir::shading::Scene {
            point_lights: vec![p],
            ..fogged()
        };
        let u = CharacterUniform::new(
            &reference_material(&eruptor_body()).unwrap(),
            &scene,
            ShadowLookup::NONE,
            0.5,
        );
        assert_eq!(packed.position, u.point_position);
        assert_eq!(packed.colour, u.point_colour);
        assert_eq!(packed.falloff, u.point_falloff);
        assert_eq!(packed.position[1], Vec4::ZERO);
        let words = packed.words();
        assert_eq!(words.len(), 9);
        assert_eq!(
            (words[0], words[3], words[6]),
            (
                [1.0, 2.0, 3.0, 0.0],
                [0.5, 0.25, 0.125, 0.0],
                [1.5, 0.25, 0.0, 0.0]
            )
        );
        assert_eq!(u.misc.w, LIGHTS_IN_VIEW);
        assert_eq!(u.lit_by_scene().misc.w, LIGHTS_FROM_SCENE);
    }

    #[test]
    fn the_glow_pass_writes_the_mask_unblended_with_the_parts_other_states() {
        let s = segment(1, false);
        let layout = layout(&s);
        let colour = MaterialKey {
            shader: ShaderKey::new(0x41C, 0x41C).unwrap(),
            states: StateKey {
                blend: Blend::Additive,
                depth_write: false,
                two_sided: true,
                alpha_test: true,
            },
            pass: Pass::Colour,
        };
        let glow = MaterialKey {
            pass: Pass::GlowMask,
            ..colour
        };
        assert_eq!(glow.shader_defs(), colour.shader_defs());
        let descriptor = || RenderPipelineDescriptor {
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
        };
        let (mut c, mut g) = (descriptor(), descriptor());
        specialize_for(&colour, &mut c, &layout).unwrap();
        specialize_for(&glow, &mut g, &layout).unwrap();
        let blend = |d: &RenderPipelineDescriptor| {
            d.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
        };
        let entry = |d: &RenderPipelineDescriptor| d.fragment.as_ref().unwrap().entry_point.clone();
        assert_eq!(entry(&c), Some("fragment".into()));
        assert_eq!(entry(&g), Some("glow_mask".into()));
        assert!(blend(&c).is_some());
        assert_eq!(blend(&g), None, "never blended");
        assert_eq!(g.primitive.cull_mode, c.primitive.cull_mode);
        assert_eq!(
            g.depth_stencil.as_ref().unwrap().depth_write_enabled,
            Some(false)
        );
        // The same material, its pass flipped.
        let textures = CharacterTextures {
            colour: Some(Handle::default()),
            normal_map: Some(Handle::default()),
            spec_map: Some(Handle::default()),
            rim: Some(Handle::default()),
            ..Default::default()
        };
        let m = CharacterMaterial::new(
            colour,
            &reference_material(&eruptor_body()).unwrap(),
            &fogged(),
            ShadowLookup::NONE,
            0.5,
            textures,
        )
        .unwrap();
        let mask = m.glow_mask().expect("a glowing part draws its mask");
        assert_eq!(mask.key, glow);
        assert_eq!(mask.uniform, m.uniform);
        assert_eq!(mask.colour, m.colour);
        // Without a glow attribute: the mask's zero where the part writes
        // depth, and no mask draw at all where it doesn't (review of AE.11).
        let none = |depth_write| {
            let key = MaterialKey {
                states: StateKey {
                    depth_write,
                    ..colour.states
                },
                ..colour
            };
            let material = guac_ir::shading::Material {
                glow: Glow::None,
                ..reference_material(&eruptor_body()).unwrap()
            };
            CharacterMaterial::new(
                key,
                &material,
                &fogged(),
                ShadowLookup::NONE,
                0.5,
                CharacterTextures {
                    colour: Some(Handle::default()),
                    normal_map: Some(Handle::default()),
                    spec_map: Some(Handle::default()),
                    rim: Some(Handle::default()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let occluder = none(true).glow_mask().expect("it hides glow behind it");
        assert_eq!(occluder.uniform.misc.z, 0.0);
        assert!(none(false).glow_mask().is_none());
        // With the attribute, a part writing no depth still draws its mask.
        assert!(!colour.states.depth_write && m.glow_mask().is_some());
    }

    #[test]
    fn specialize_sets_defs_buffers_and_states() {
        let s = segment(1, false);
        let layout = layout(&s);
        let key = MaterialKey {
            shader: ShaderKey::new(0x41C, 0x41C).unwrap(),
            states: StateKey {
                blend: Blend::Alpha,
                depth_write: false,
                two_sided: true,
                alpha_test: true,
            },
            pass: Pass::Colour,
        };
        let mut d = RenderPipelineDescriptor {
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
        };
        d.primitive.cull_mode = Some(Face::Back);
        specialize_for(&key, &mut d, &layout).unwrap();
        assert_eq!(d.vertex.shader_defs, key.shader_defs());
        assert_eq!(d.fragment.as_ref().unwrap().shader_defs, key.shader_defs());
        let buffer = &d.vertex.buffers[0];
        let locations: Vec<u32> = buffer
            .attributes
            .iter()
            .map(|a| a.shader_location)
            .collect();
        assert_eq!(locations, [0, 1, 2, 4, 6, 7]);
        assert_eq!(d.primitive.cull_mode, None);
        assert_eq!(d.depth_stencil.unwrap().depth_write_enabled, Some(false));
        assert!(
            d.fragment.unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
                .is_some()
        );
    }
}
