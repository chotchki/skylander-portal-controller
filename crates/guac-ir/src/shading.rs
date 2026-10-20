//! The character-shading spec as a CPU reference model (openspec AE.7,
//! design decision 10): the second implementation AE.8's shader is checked
//! against, written from the spec's text
//! (`openspec/changes/ae-animated-turntable/specs/guacamole/character-shading/spec.md`),
//! not from the game's shader or the research one, so a term both read
//! wrong can't pass unseen.
//!
//! It computes in f32 on whatever values it's given. AE.8's and AE.12's GPU
//! lanes hand it the remaster's: colours decoded into linear light, tints
//! and vertex colours as stored, textures as the GPU samples them. Nothing
//! is clamped on the way out, so a colour past full reaches the tonemapper
//! as it is (the remaster-look spec). Vectors are view space, and a
//! directional light is given by the way it travels.
//!
//! Left to others: the glow's extract, blur and combine ([`crate::glow`],
//! from the mask [`glow_mask`] gives), a scroll track's motion (AE.21; a
//! set's transform is taken as given) and the depth test, which is the
//! pipeline's, not a material's.

use crate::model::{Blend, RenderStates};

pub type Vec3 = [f32; 3];
pub type Rgba = [f32; 4];
pub type Uv = [f32; 2];

/// 128 of 255: a neutral tint.
pub const NEUTRAL: f32 = 128.0 / 255.0;
/// What a part without vertex colours takes in their place.
pub const HALF_GREY: Rgba = [0.5, 0.5, 0.5, 1.0];
/// Specular's floor, under the exponent.
pub const SPECULAR_FLOOR: f32 = 0.00001;

/// A texture as the shader samples it.
pub trait Texture {
    fn sample(&self, uv: Uv) -> Rgba;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flat(pub Rgba);

impl Texture for Flat {
    fn sample(&self, _: Uv) -> Rgba {
        self.0
    }
}

impl<F: Fn(Uv) -> Rgba> Texture for F {
    fn sample(&self, uv: Uv) -> Rgba {
        self(uv)
    }
}

/// A blend's weights for its three ways: over, add and multiply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub over: f32,
    pub add: f32,
    pub multiply: f32,
}

impl Weights {
    pub const OVER: Self = Self::new(1.0, 0.0, 0.0);
    pub const ADD: Self = Self::new(0.0, 1.0, 0.0);
    pub const MULTIPLY: Self = Self::new(0.0, 0.0, 1.0);

    pub const fn new(over: f32, add: f32, multiply: f32) -> Self {
        Self {
            over,
            add,
            multiply,
        }
    }

    fn blend<const N: usize>(self, over: [f32; N], add: [f32; N], multiply: [f32; N]) -> [f32; N] {
        std::array::from_fn(|i| {
            self.over * over[i] + self.add * add[i] + self.multiply * multiply[i]
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    /// Specular's exponent; can be under 1.
    pub exponent: f32,
    /// How much of the normal map's difference lighting shows.
    pub detail_weight: f32,
    /// From -0.0009 up it replaces the colour map's alpha (the ramp in
    /// [`layer_alpha`]).
    pub fixed_alpha: f32,
}

impl Layer {
    /// The original's defaults. The spec names no fixed alpha's default;
    /// -1 leaves the colour map's alpha, the tutorial's value AE.6 takes.
    pub const DEFAULT: Self = Self {
        exponent: 1.0,
        detail_weight: 0.25,
        fixed_alpha: -1.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondLayer {
    pub layer: Layer,
    pub weights: Weights,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThirdLayer {
    /// The material's fixed alpha for the third layer.
    pub strength: f32,
    pub weights: Weights,
}

/// The rim layer: a sphere map looked up by the view-space normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rim {
    /// The lookup's scale.
    pub scale: f32,
    /// The material's fixed alpha for the rim.
    pub strength: f32,
    pub weights: Weights,
    /// 0 puts the blend's result all into the colour, 1 all into the alpha.
    pub split: f32,
}

impl Rim {
    /// A rim at `strength` with the original's defaults: added, at a lookup
    /// scale of 1, all of it into the colour.
    pub const fn new(strength: f32) -> Self {
        Self {
            scale: 1.0,
            strength,
            weights: Weights::ADD,
            split: 0.0,
        }
    }
}

/// A texture-coordinate set's 2x3 transform: u' = u[0]·u + u[1]·v + u[2],
/// v' alike.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvTransform {
    pub u: [f32; 3],
    pub v: [f32; 3],
}

impl UvTransform {
    pub const IDENTITY: Self = Self {
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    };

    pub fn apply(&self, uv: Uv) -> Uv {
        [
            self.u[0] * uv[0] + self.u[1] * uv[1] + self.u[2],
            self.v[0] * uv[0] + self.v[1] * uv[1] + self.v[2],
        ]
    }
}

/// What a part writes into the glow mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Glow {
    /// Nothing: a part without the glow attribute.
    None,
    /// The amount, of 1.
    Amount(f32),
    /// The first layer's alpha, switched hard at a fixed alpha of 0, times
    /// the vertex and both tints' alphas.
    LayerAlpha,
}

/// The scene's switches a material owns, if any; `None` leaves one to the
/// scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fixed {
    pub shadows: Option<bool>,
    pub fog: Option<bool>,
    pub point_lights: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub normal_map: bool,
    pub spec_map: bool,
    pub directional: bool,
    pub vertex_colour: bool,
    pub first: Layer,
    pub second: Option<SecondLayer>,
    pub third: Option<ThirdLayer>,
    pub rim: Option<Rim>,
    /// The material tint, RGBA of 1 ([`unpack_tint`] for a packed one).
    pub tint: Rgba,
    pub glow: Glow,
    /// One per texture-coordinate set, numbered as the set.
    pub uv_transforms: [UvTransform; 3],
    pub fixed: Fixed,
}

impl Material {
    /// A colour map and nothing else, the original's defaults everywhere.
    pub fn new() -> Self {
        Self {
            normal_map: false,
            spec_map: false,
            directional: false,
            vertex_colour: false,
            first: Layer::DEFAULT,
            second: None,
            third: None,
            rim: None,
            tint: [NEUTRAL, NEUTRAL, NEUTRAL, 1.0],
            glow: Glow::None,
            uv_transforms: [UvTransform::IDENTITY; 3],
            fixed: Fixed::default(),
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new()
    }
}

/// A packed material tint, red in its lowest byte.
pub fn unpack_tint(packed: u32) -> Rgba {
    packed.to_le_bytes().map(|b| f32::from(b) / 255.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    /// The way the light travels, unit length.
    pub travel: Vec3,
    pub ambient: Vec3,
    pub diffuse: Vec3,
    pub specular: Vec3,
}

impl DirectionalLight {
    /// A black light, for a rig's empty slots.
    pub const OFF: Self = Self {
        travel: [0.0, 0.0, -1.0],
        ambient: [0.0; 3],
        diffuse: [0.0; 3],
        specular: [0.0; 3],
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    pub colour: Vec3,
    /// The falloff's term over (d + 0.001).
    pub linear: f32,
    /// The falloff's term over (d + 0.001)².
    pub quadratic: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub lights: [DirectionalLight; 3],
    pub shadows: bool,
    /// k in the shadow's 2·(a + (2 − k)·(rgb − a)).
    pub shadow_mode: f32,
    pub fog: bool,
    pub fog_colour: Vec3,
    pub fog_near: f32,
    pub fog_far: f32,
    /// Up to three.
    pub point_lights: Vec<PointLight>,
    pub placement_tint: Rgba,
    /// RGB, and its amount in alpha.
    pub replace: Rgba,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            lights: [DirectionalLight::OFF; 3],
            shadows: false,
            shadow_mode: 2.0,
            fog: false,
            fog_colour: [0.0; 3],
            fog_near: 0.0,
            fog_far: 1.0,
            point_lights: Vec::new(),
            placement_tint: [NEUTRAL, NEUTRAL, NEUTRAL, 1.0],
            replace: [0.0; 4],
        }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

/// What the vertex stage interpolates to a pixel, and its shadow sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fragment {
    /// The geometric normal as interpolated, not renormalized.
    pub normal: Vec3,
    pub tangent: Vec3,
    pub bitangent: Vec3,
    pub position: Vec3,
    /// The distance along the view direction.
    pub depth: f32,
    /// Toward the eye, unit length at each vertex, interpolated.
    pub eye: Vec3,
    /// Read only when the material has vertex colours.
    pub vertex_colour: Rgba,
    /// The part's texture-coordinate sets, before their transforms.
    pub uv: [Uv; 3],
    pub shadow: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Colour,
    NormalMap,
    SpecMap,
    SecondColour,
    SecondNormalMap,
    SecondSpecMap,
    Third,
    Rim,
}

/// The textures by role; the ones the material doesn't switch on are
/// never sampled.
#[derive(Clone, Copy, Default)]
pub struct Textures<'a> {
    pub colour: Option<&'a dyn Texture>,
    pub normal_map: Option<&'a dyn Texture>,
    pub spec_map: Option<&'a dyn Texture>,
    pub second_colour: Option<&'a dyn Texture>,
    pub second_normal_map: Option<&'a dyn Texture>,
    pub second_spec_map: Option<&'a dyn Texture>,
    pub third: Option<&'a dyn Texture>,
    pub rim: Option<&'a dyn Texture>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("the material needs a texture in its {0:?} role")]
    Missing(Role),
    #[error("{0} point lights in the scene; three at most")]
    TooManyPointLights(usize),
}

/// Which terms are on for a part: the material's switches, and the
/// scene's except where the material owns one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Switches {
    pub normal_map: bool,
    pub spec_map: bool,
    pub directional: bool,
    pub vertex_colour: bool,
    pub second: bool,
    pub third: bool,
    pub rim: bool,
    pub shadows: bool,
    pub fog: bool,
    pub point_lights: u8,
}

impl Switches {
    pub fn resolve(material: &Material, scene: &Scene) -> Self {
        let lights = u8::try_from(scene.point_lights.len()).unwrap_or(u8::MAX);
        Self {
            normal_map: material.normal_map,
            spec_map: material.spec_map,
            directional: material.directional,
            vertex_colour: material.vertex_colour,
            second: material.second.is_some(),
            third: material.third.is_some(),
            rim: material.rim.is_some(),
            shadows: material.fixed.shadows.unwrap_or(scene.shadows),
            fog: material.fixed.fog.unwrap_or(scene.fog),
            point_lights: material.fixed.point_lights.unwrap_or(lights).min(lights),
        }
    }

    /// Without it the vertex stage writes a zero eye direction, which
    /// leaves specular at its floor.
    pub fn eye_direction(&self) -> bool {
        self.spec_map && (self.fog || self.point_lights > 0)
    }

    /// A part has a geometric normal with a rim layer, directional
    /// lighting or a point light reaching it.
    pub fn geometric_normal(&self) -> bool {
        self.rim || self.directional || self.point_lights > 0
    }
}

/// The texture-coordinate set each layer reads, handed out in order of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UvSets {
    pub first: usize,
    pub second: Option<usize>,
    pub third: Option<usize>,
}

pub fn uv_sets(material: &Material) -> UvSets {
    let second = material.second.map(|_| 1);
    let third = material.third.map(|_| second.map_or(1, |s| s + 1));
    UvSets {
        first: 0,
        second,
        third,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaded {
    pub colour: Vec3,
    pub alpha: f32,
}

/// The mapped normal: X from the texel's alpha, Y from its green, Z from
/// both, over the interpolated frame, none of it renormalized first.
pub fn mapped_normal(texel: Rgba, fragment: &Fragment) -> Vec3 {
    let x = 2.0 * texel[3] - 1.0;
    let y = 2.0 * texel[1] - 1.0;
    let z = (1.0 - x * x - y * y).abs().sqrt();
    normalize(add(
        sub(scale(fragment.bitangent, y), scale(fragment.tangent, x)),
        scale(fragment.normal, z),
    ))
}

/// A layer's alpha: the colour map's, mixed toward the fixed alpha by
/// (fixed + 0.001) × 10,000 clamped to 0..1.
pub fn layer_alpha(colour_alpha: f32, fixed_alpha: f32) -> f32 {
    let k = ((fixed_alpha + 0.001) * 10_000.0).clamp(0.0, 1.0);
    colour_alpha + k * (fixed_alpha - colour_alpha)
}

/// Where the rim map is looked up for a view-space normal.
pub fn rim_uv(normal: Vec3, scale: f32) -> Uv {
    [
        (0.5 + 0.5 * normal[0]) * scale,
        (0.5 - 0.5 * normal[1]) * scale,
    ]
}

/// How far fog has mixed in at `depth`.
pub fn fog_factor(depth: f32, near: f32, far: f32) -> f32 {
    ((depth - near) / (far - near)).clamp(0.0, 1.0)
}

/// A light's reflection about `normal`, or the way it travels without one.
pub fn reflection(travel: Vec3, normal: Option<Vec3>) -> Vec3 {
    match normal {
        Some(n) => normalize(sub(travel, scale(n, 2.0 * dot(travel, n)))),
        None => travel,
    }
}

/// A normal's facing to a directional light: its dot with the direction
/// toward the light.
pub fn facing(normal: Vec3, travel: Vec3) -> f32 {
    -dot(normal, travel)
}

/// A part's glow alpha: its first layer's alpha switched hard at a fixed
/// alpha of 0 (not the colour pass's ramp), times the vertex and both
/// tints' alphas. The glow pass's alpha test runs on it.
pub fn glow_alpha(
    material: &Material,
    scene: &Scene,
    fragment: &Fragment,
    colour_alpha: f32,
) -> f32 {
    let first = if material.first.fixed_alpha >= 0.0 {
        material.first.fixed_alpha
    } else {
        colour_alpha
    };
    first * vertex_colour(material, fragment)[3] * scene.placement_tint[3] * material.tint[3]
}

/// The glow pass's mask for a pixel: nothing without a glow, else the
/// amount or the glow alpha.
pub fn glow_mask(
    material: &Material,
    scene: &Scene,
    fragment: &Fragment,
    colour_alpha: f32,
) -> f32 {
    match material.glow {
        Glow::None => 0.0,
        Glow::Amount(amount) => amount,
        Glow::LayerAlpha => glow_alpha(material, scene, fragment, colour_alpha),
    }
}

/// Shades one pixel of a part, every term in the spec's order.
pub fn shade(
    material: &Material,
    scene: &Scene,
    fragment: &Fragment,
    textures: &Textures,
) -> Result<Shaded, Error> {
    if scene.point_lights.len() > 3 {
        return Err(Error::TooManyPointLights(scene.point_lights.len()));
    }
    let on = Switches::resolve(material, scene);
    let eye = if on.eye_direction() {
        fragment.eye.map(|c| c.max(0.0))
    } else {
        [0.0; 3]
    };
    let ctx = Context {
        scene,
        fragment,
        on,
        vertex: vertex_colour(material, fragment),
        eye,
        geometric: on.geometric_normal().then_some(fragment.normal),
    };
    let sets = uv_sets(material);
    let uv = |set: usize| material.uv_transforms[set].apply(fragment.uv[set]);

    // Each layer's lighting and specular.
    let first = ctx.lit(
        &Maps {
            colour: need(textures.colour, Role::Colour)?,
            normal_map: on
                .normal_map
                .then(|| need(textures.normal_map, Role::NormalMap))
                .transpose()?,
            spec_map: on
                .spec_map
                .then(|| need(textures.spec_map, Role::SpecMap))
                .transpose()?,
        },
        &material.first,
        uv(sets.first),
    );
    let mut lit = first.lit;

    // The second layer's blend.
    if let (Some(second), Some(set)) = (&material.second, sets.second) {
        let layer = ctx.lit(
            &Maps {
                colour: need(textures.second_colour, Role::SecondColour)?,
                normal_map: on
                    .normal_map
                    .then(|| need(textures.second_normal_map, Role::SecondNormalMap))
                    .transpose()?,
                spec_map: on
                    .spec_map
                    .then(|| need(textures.second_spec_map, Role::SecondSpecMap))
                    .transpose()?,
            },
            &second.layer,
            uv(set),
        );
        let coverage = ctx.vertex[3]
            * layer_alpha(layer.colour_alpha, second.layer.fixed_alpha)
            * material.tint[3];
        let covered = scale(layer.lit, coverage);
        let rest = scale(lit, 1.0 - coverage);
        lit = second.weights.blend(
            add(covered, rest),
            add(lit, covered),
            add(mul(lit, covered), rest),
        );
    }

    // Tints.
    let tint: Vec3 = std::array::from_fn(|i| 4.0 * scene.placement_tint[i] * material.tint[i]);
    let mut colour = mul(lit, tint);
    let mut alpha = layer_alpha(first.colour_alpha, material.first.fixed_alpha)
        * ctx.vertex[3]
        * scene.placement_tint[3]
        * material.tint[3];

    // The third layer.
    if let (Some(third), Some(set)) = (&material.third, sets.third) {
        let layer = scale(
            rgb(need(textures.third, Role::Third)?.sample(uv(set))),
            third.strength,
        );
        colour = third.weights.blend(
            add(layer, scale(colour, 1.0 - third.strength)),
            add(colour, layer),
            mul(colour, layer),
        );
    }

    // Shadow.
    if on.shadows {
        let s = fragment.shadow;
        let k = scene.shadow_mode;
        colour = std::array::from_fn(|i| colour[i] * 2.0 * (s[3] + (2.0 - k) * (s[i] - s[3])));
    }

    // The normal the rim and point lights take: either one switches the
    // geometric normal on.
    let normal = first.mapped.or(ctx.geometric).unwrap_or(fragment.normal);

    // Rim.
    if let Some(rim) = &material.rim {
        let sample = need(textures.rim, Role::Rim)?.sample(rim_uv(normal, rim.scale));
        let t = sample.map(|c| c * rim.strength);
        let base = [colour[0], colour[1], colour[2], alpha];
        let blended = rim.weights.blend(
            std::array::from_fn(|i| t[i] + base[i] * (1.0 - rim.strength)),
            std::array::from_fn(|i| base[i] + t[i]),
            std::array::from_fn(|i| base[i] * t[i]),
        );
        colour = mix(colour, rgb(blended), 1.0 - rim.split);
        alpha += (blended[3] - alpha) * rim.split;
    }

    // Point lights.
    for light in scene.point_lights.iter().take(usize::from(on.point_lights)) {
        let to = sub(light.position, fragment.position);
        let d = length(to);
        let f = (dot(normal, to) / d).max(0.0);
        let de = d + 0.001;
        let falloff = light.linear / de + light.quadratic / (de * de);
        colour = add(colour, scale(light.colour, f * falloff));
    }

    // Replace colour, then fog.
    colour = mix(colour, rgb(scene.replace), scene.replace[3]);
    if on.fog {
        let f = fog_factor(fragment.depth, scene.fog_near, scene.fog_far);
        colour = mix(colour, scene.fog_colour, f);
    }
    Ok(Shaded { colour, alpha })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    pub colour: Vec3,
    pub depth: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arriving {
    pub shaded: Shaded,
    pub depth: f32,
    pub front_facing: bool,
}

/// Draws a fragment into a pixel as its part's render states say: culled,
/// alpha-tested, blended and depth-written. The depth test is the
/// pipeline's, so it isn't here.
pub fn draw(pixel: Pixel, arriving: &Arriving, states: &RenderStates) -> Pixel {
    if !arriving.front_facing && !states.two_sided {
        return pixel;
    }
    let Shaded { colour, alpha } = arriving.shaded;
    if states
        .alpha_test
        .is_some_and(|reference| alpha <= reference)
    {
        return pixel;
    }
    let colour = match states.blend {
        Blend::Opaque => colour,
        Blend::Alpha => add(scale(colour, alpha), scale(pixel.colour, 1.0 - alpha)),
        Blend::Additive => add(scale(colour, alpha), pixel.colour),
        // A level's (AG.15), never a character's: DESTINATION_COLOR, ZERO.
        Blend::Multiply => mul(colour, pixel.colour),
    };
    Pixel {
        colour,
        depth: if states.depth_write {
            arriving.depth
        } else {
            pixel.depth
        },
    }
}

struct Context<'a> {
    scene: &'a Scene,
    fragment: &'a Fragment,
    on: Switches,
    vertex: Rgba,
    eye: Vec3,
    geometric: Option<Vec3>,
}

struct Maps<'a> {
    colour: &'a dyn Texture,
    normal_map: Option<&'a dyn Texture>,
    spec_map: Option<&'a dyn Texture>,
}

struct LitLayer {
    lit: Vec3,
    colour_alpha: f32,
    mapped: Option<Vec3>,
}

impl Context<'_> {
    fn lit(&self, maps: &Maps, layer: &Layer, uv: Uv) -> LitLayer {
        let lights = &self.scene.lights;
        let c = maps.colour.sample(uv);
        let mapped = maps
            .normal_map
            .map(|t| mapped_normal(t.sample(uv), self.fragment));
        let normal = mapped.or(self.geometric);
        let base = if self.on.directional {
            let n = normal.unwrap_or(self.fragment.normal);
            let sum = lights.iter().fold([0.0; 3], |sum, l| {
                add(
                    sum,
                    add(l.ambient, scale(l.diffuse, facing(n, l.travel).max(0.0))),
                )
            });
            scale(mul(rgb(c), sum), 2.0)
        } else if let Some(m) = mapped {
            let g = self.fragment.normal;
            let sum = lights.iter().fold([0.0; 3], |sum, l| {
                let difference = facing(m, l.travel).max(0.0) - facing(g, l.travel).max(0.0);
                add(sum, scale(l.diffuse, difference))
            });
            add(rgb(c), scale(sum, layer.detail_weight))
        } else {
            rgb(c)
        };
        let mut lit = scale(mul(base, rgb(self.vertex)), 2.0);
        if let Some(spec) = maps.spec_map {
            let sum = lights.iter().fold([0.0; 3], |sum, l| {
                let r = reflection(l.travel, normal);
                let s = dot(self.eye, r).max(SPECULAR_FLOOR).powf(layer.exponent);
                add(sum, scale(l.specular, s))
            });
            lit = add(lit, mul(rgb(spec.sample(uv)), sum));
        }
        LitLayer {
            lit,
            colour_alpha: c[3],
            mapped,
        }
    }
}

fn vertex_colour(material: &Material, fragment: &Fragment) -> Rgba {
    if material.vertex_colour {
        fragment.vertex_colour
    } else {
        HALF_GREY
    }
}

fn need(texture: Option<&dyn Texture>, role: Role) -> Result<&dyn Texture, Error> {
    texture.ok_or(Error::Missing(role))
}

fn rgb(c: Rgba) -> Vec3 {
    [c[0], c[1], c[2]]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] + b[i])
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] - b[i])
}

fn mul(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] * b[i])
}

fn scale(a: Vec3, s: f32) -> Vec3 {
    a.map(|c| c * s)
}

fn mix(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: Vec3) -> Vec3 {
    scale(a, 1.0 / length(a))
}

#[cfg(test)]
mod tests {
    //! One or more tests per requirement of the character-shading spec,
    //! each expectation worked from the spec's own arithmetic.

    use super::*;

    /// Neutral tints' scale, 4 · (128/255)².
    const NEUTRAL_SCALE: f32 = 4.0 * NEUTRAL * NEUTRAL;
    const FOG_NEAR: f32 = 30.48;
    const FOG_FAR: f32 = 335.28;

    /// A pixel on a unit frame facing the eye, 5 in front of it.
    fn frag() -> Fragment {
        Fragment {
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 1.0, 0.0],
            position: [0.0, 0.0, -5.0],
            depth: 5.0,
            eye: [0.0, 0.0, 1.0],
            vertex_colour: [1.0; 4],
            uv: [[0.25, 0.75], [0.5, 0.5], [0.5, 0.5]],
            shadow: [0.5; 4],
        }
    }

    /// Tints whose product is exactly 1 (4 · 0.5 · 0.5), so a test reads
    /// the lit colour itself.
    fn unit_tints(m: &mut Material, s: &mut Scene) {
        m.tint = [0.5, 0.5, 0.5, 1.0];
        s.placement_tint = [0.5, 0.5, 0.5, 1.0];
    }

    fn fogged() -> Scene {
        Scene {
            fog: true,
            fog_colour: [0.3, 0.2, 0.1],
            fog_near: FOG_NEAR,
            fog_far: FOG_FAR,
            ..Scene::new()
        }
    }

    fn light(travel: Vec3) -> DirectionalLight {
        DirectionalLight {
            travel,
            ..DirectionalLight::OFF
        }
    }

    /// A normal-map texel whose mapped X and Y are `x` and `y`.
    fn texel(x: f32, y: f32) -> Flat {
        Flat([0.0, (y + 1.0) / 2.0, 0.0, (x + 1.0) / 2.0])
    }

    /// Eruptor's body as AE.6 reads it: normal map, spec map, the rim
    /// added at full strength, exponent 1, detail weight 1, glow 64.
    fn eruptor_body() -> Material {
        Material {
            normal_map: true,
            spec_map: true,
            first: Layer {
                exponent: 1.0,
                detail_weight: 1.0,
                fixed_alpha: -1.0,
            },
            rim: Some(Rim::new(1.0)),
            glow: Glow::Amount(64.0 / 255.0),
            ..Material::new()
        }
    }

    fn shaded(m: &Material, s: &Scene, f: &Fragment, t: &Textures) -> Shaded {
        shade(m, s, f, t).expect("shades")
    }

    #[track_caller]
    fn near(got: &[f32], want: &[f32], tol: f32) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() <= tol, "got {got:?}, want {want:?}");
        }
    }

    // The material comes off the disc.

    #[test]
    fn a_material_takes_the_originals_defaults() {
        let m = Material::new();
        assert_eq!(m.first.exponent, 1.0);
        assert_eq!(m.first.detail_weight, 0.25);
        let rim = Rim::new(0.7);
        assert_eq!(
            (rim.scale, rim.weights, rim.split),
            (1.0, Weights::ADD, 0.0)
        );
        assert_eq!(m.tint, unpack_tint(0xFF80_8080));
        assert_eq!(Scene::new().placement_tint, unpack_tint(0xFF80_8080));
        assert_eq!(m.tint[0], 128.0 / 255.0);
    }

    #[test]
    fn eruptors_body_reads_each_map_in_its_role() {
        let m = eruptor_body();
        let mut s = fogged();
        let (d, sp) = ([0.3, 0.2, 0.1], [0.5, 0.4, 0.3]);
        s.lights[0] = DirectionalLight {
            travel: [0.0, 0.0, -1.0],
            ambient: [0.0; 3],
            diffuse: d,
            specular: sp,
        };
        let (c, n, sm, r) = (
            Flat([0.4, 0.2, 0.1, 0.8]),
            texel(0.0, 0.0),
            Flat([0.2, 0.3, 0.4, 1.0]),
            Flat([0.1, 0.05, 0.02, 0.6]),
        );
        let t = Textures {
            colour: Some(&c),
            normal_map: Some(&n),
            spec_map: Some(&sm),
            rim: Some(&r),
            ..Default::default()
        };
        // Flat map, unit normal: no difference lighting. The light comes
        // from the eye and reflects back into it: specular 1^1. Twice half
        // grey is 1, then the neutral tints, then the rim added; fog is 0
        // this near.
        let got = shaded(&m, &s, &frag(), &t);
        let want: Vec3 =
            std::array::from_fn(|i| (c.0[i] + sm.0[i] * sp[i]) * NEUTRAL_SCALE + r.0[i]);
        near(&got.colour, &want, 1e-6);
        assert_eq!(
            got.alpha, 0.8,
            "the colour map's alpha; the rim's goes to colour"
        );

        for (role, textures) in [
            (
                Role::NormalMap,
                Textures {
                    normal_map: None,
                    ..t
                },
            ),
            (
                Role::SpecMap,
                Textures {
                    spec_map: None,
                    ..t
                },
            ),
            (Role::Rim, Textures { rim: None, ..t }),
            (Role::Colour, Textures { colour: None, ..t }),
        ] {
            assert_eq!(shade(&m, &s, &frag(), &textures), Err(Error::Missing(role)));
        }
    }

    // The scene sets its own switches.

    #[test]
    fn the_scene_sets_what_the_material_leaves_it() {
        assert_eq!(
            Switches::resolve(&eruptor_body(), &fogged()),
            Switches {
                normal_map: true,
                spec_map: true,
                rim: true,
                fog: true,
                ..Switches::default()
            }
        );

        let lamp = PointLight {
            position: [0.0, 0.0, -3.0],
            colour: [1.0, 0.5, 0.25],
            linear: 1.0,
            quadratic: 0.0,
        };
        let lit = Scene {
            shadows: true,
            shadow_mode: 1.0,
            point_lights: vec![lamp; 3],
            ..fogged()
        };
        assert_eq!(Switches::resolve(&Material::new(), &lit).point_lights, 3);
        assert!(Switches::resolve(&Material::new(), &lit).shadows);

        let mut m = Material::new();
        m.fixed = Fixed {
            shadows: Some(false),
            fog: None,
            point_lights: Some(0),
        };
        let on = Switches::resolve(&m, &lit);
        assert_eq!((on.point_lights, on.shadows, on.fog), (0, false, true));
        let c = Flat([0.4, 0.2, 0.1, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let dark = Scene {
            shadows: false,
            point_lights: Vec::new(),
            ..lit.clone()
        };
        assert_eq!(
            shaded(&m, &lit, &frag(), &t),
            shaded(&m, &dark, &frag(), &t)
        );
    }

    #[test]
    fn more_than_three_point_lights_are_refused() {
        let lamp = PointLight {
            position: [0.0; 3],
            colour: [1.0; 3],
            linear: 1.0,
            quadratic: 0.0,
        };
        let s = Scene {
            point_lights: vec![lamp; 4],
            ..Scene::new()
        };
        let c = Flat([0.0; 4]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        assert_eq!(
            shade(&Material::new(), &s, &frag(), &t),
            Err(Error::TooManyPointLights(4))
        );
    }

    // Normal maps.

    #[test]
    fn a_flat_texel_gives_the_interpolated_normal_normalized() {
        let f = Fragment {
            normal: [0.0, 0.0, 0.8],
            tangent: [0.9, 0.0, 0.0],
            bitangent: [0.0, 1.1, 0.0],
            ..frag()
        };
        near(
            &mapped_normal(texel(0.0, 0.0).0, &f),
            &[0.0, 0.0, 1.0],
            1e-7,
        );
    }

    #[test]
    fn x_comes_from_alpha_against_the_tangent_and_y_from_green() {
        let f = frag();
        near(
            &mapped_normal(texel(1.0, 0.0).0, &f),
            &[-1.0, 0.0, 0.0],
            1e-6,
        );
        near(
            &mapped_normal(texel(0.0, 1.0).0, &f),
            &[0.0, 1.0, 0.0],
            1e-6,
        );
        near(
            &mapped_normal(texel(0.6, 0.0).0, &f),
            &[-0.6, 0.0, 0.8],
            1e-6,
        );
        // Red and blue are never read.
        let noisy = Flat([0.9, 0.5, 0.1, 0.8]);
        near(&mapped_normal(noisy.0, &f), &[-0.6, 0.0, 0.8], 1e-6);
    }

    #[test]
    fn z_takes_the_absolute_value_past_the_unit_circle() {
        // X = Y = 0.9: 1 − 0.81 − 0.81 = −0.62, so Z = √0.62, not 0.
        let n = mapped_normal(texel(0.9, 0.9).0, &frag());
        let len = (0.81f32 + 0.81 + 0.62).sqrt();
        near(&n, &[-0.9 / len, 0.9 / len, 0.62f32.sqrt() / len], 1e-5);
    }

    #[test]
    fn the_frame_is_not_renormalized_first() {
        let f = Fragment {
            bitangent: [0.0, 2.0, 0.0],
            ..frag()
        };
        // Y 0.6 on a bitangent of 2, Z 0.8: (0, 1.2, 0.8) normalized, not
        // the (0, 0.6, 0.8) a renormalized bitangent gives.
        let len = (1.44f32 + 0.64).sqrt();
        near(
            &mapped_normal(texel(0.0, 0.6).0, &f),
            &[0.0, 1.2 / len, 0.8 / len],
            1e-6,
        );
    }

    // The geometric normal as interpolated.

    #[test]
    fn a_normal_shortened_by_interpolation_lights_short() {
        let mut m = Material {
            directional: true,
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        s.lights[0] = DirectionalLight {
            diffuse: [1.0; 3],
            ..light([0.0, 0.0, -1.0])
        };
        let f = Fragment {
            normal: [0.0, 0.0, 0.9],
            ..frag()
        };
        let c = Flat([1.0; 4]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        // 4 · colour · half grey · 0.9: the light at 0.9, not 1.
        near(&shaded(&m, &s, &f, &t).colour, &[1.8; 3], 1e-6);
    }

    // Facing a light.

    #[test]
    fn facing_is_the_dot_toward_the_light() {
        assert_eq!(facing([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]), 1.0);
        assert_eq!(facing([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]), -1.0);
        assert!((facing([1.0, 0.0, 0.0], [-0.6, 0.0, -0.8]) - 0.6).abs() < 1e-7);
    }

    // Parts without directional lighting.

    /// An unlit part with a normal map, at unit tints.
    fn normal_mapped() -> (Material, Scene) {
        let mut m = Material {
            normal_map: true,
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        (m, s)
    }

    #[test]
    fn a_flat_normal_map_shows_the_colour_map_on_the_unlit_path() {
        let n = texel(0.0, 0.0);
        let (m, mut s) = normal_mapped();
        s.lights = [
            DirectionalLight {
                diffuse: [0.7; 3],
                ..light([0.0, 0.0, -1.0])
            },
            DirectionalLight {
                diffuse: [0.3; 3],
                ..light([0.6, 0.0, -0.8])
            },
            DirectionalLight::OFF,
        ];
        let c = Flat([0.4, 0.3, 0.2, 0.9]);
        let t = Textures {
            colour: Some(&c),
            normal_map: Some(&n),
            ..Default::default()
        };
        let got = shaded(&m, &s, &frag(), &t);
        assert_eq!(got.colour, [0.4, 0.3, 0.2]);
        assert_eq!(got.alpha, 0.9);
    }

    #[test]
    fn the_difference_lighting_adds_what_the_map_turns_toward_a_light() {
        // Mapped normal (−0.6, 0, 0.8) on a geometric (0, 0, 1).
        let n = texel(0.6, 0.0);
        let (m, mut s) = normal_mapped();
        let (d, e) = ([0.5, 0.25, 1.0], [0.2, 0.4, 0.8]);
        s.lights[0] = DirectionalLight {
            diffuse: d,
            ..light([1.0, 0.0, 0.0])
        };
        s.lights[1] = DirectionalLight {
            diffuse: e,
            ..light([0.0, 0.0, -1.0])
        };
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            normal_map: Some(&n),
            ..Default::default()
        };
        // Light 0 from the side: mapped facing 0.6, geometric 0. Light 1
        // head on: mapped 0.8, geometric 1, a difference of −0.2. Detail
        // weight 0.25 (the default); twice half grey is 1.
        let want: Vec3 = std::array::from_fn(|i| c.0[i] + 0.25 * (0.6 * d[i] - 0.2 * e[i]));
        near(&shaded(&m, &s, &frag(), &t).colour, &want, 1e-6);
    }

    #[test]
    fn each_facing_is_clamped_before_the_difference() {
        let n = texel(0.6, 0.0);
        let (m, mut s) = normal_mapped();
        // Behind the geometric normal, in front of the mapped one.
        let k = 1.0f32 / 1.0625f32.sqrt();
        s.lights[0] = DirectionalLight {
            diffuse: [1.0; 3],
            ..light([k, 0.0, 0.25 * k])
        };
        let c = Flat([0.0, 0.0, 0.0, 1.0]);
        let t = Textures {
            colour: Some(&c),
            normal_map: Some(&n),
            ..Default::default()
        };
        // Mapped facing 0.6k − 0.2k = 0.4k, geometric −0.25k clamped to 0.
        let got = shaded(&m, &s, &frag(), &t).colour[0];
        assert!((got - 0.25 * 0.4 * k).abs() < 1e-6, "{got}");
        assert!(
            (got - 0.25 * 0.65 * k).abs() > 1e-2,
            "the geometric facing was clamped"
        );
    }

    #[test]
    fn vertex_colours_count_twice_and_half_grey_stands_in() {
        let mut m = Material::new();
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let f = Fragment {
            vertex_colour: [0.25, 0.5, 1.0, 1.0],
            ..frag()
        };
        // Without vertex colours the fragment's are never read.
        assert_eq!(shaded(&m, &s, &f, &t).colour, [0.4, 0.3, 0.2]);
        m.vertex_colour = true;
        near(&shaded(&m, &s, &f, &t).colour, &[0.2, 0.3, 0.4], 1e-7);
    }

    // Directionally lit parts.

    #[test]
    fn facing_away_from_every_light_leaves_twice_the_colour_map_times_the_ambients() {
        let mut m = Material {
            directional: true,
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let ambients = [[0.1, 0.2, 0.3], [0.05, 0.05, 0.05], [0.2, 0.0, 0.1]];
        for (l, a) in s.lights.iter_mut().zip(ambients) {
            *l = DirectionalLight {
                travel: [0.0, 0.0, 1.0],
                ambient: a,
                diffuse: [0.9; 3],
                specular: [0.0; 3],
            };
        }
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let want: Vec3 = std::array::from_fn(|i| {
            2.0 * c.0[i] * (ambients[0][i] + ambients[1][i] + ambients[2][i])
        });
        near(&shaded(&m, &s, &frag(), &t).colour, &want, 1e-6);
    }

    #[test]
    fn a_lit_part_takes_four_times_colour_and_vertex_colour_without_difference_lighting() {
        let n = texel(0.6, 0.0);
        let mut m = Material {
            directional: true,
            normal_map: true,
            vertex_colour: true,
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let (a, d) = ([0.1, 0.2, 0.05], [0.5, 0.25, 1.0]);
        s.lights[0] = DirectionalLight {
            travel: [1.0, 0.0, 0.0],
            ambient: a,
            diffuse: d,
            specular: [0.0; 3],
        };
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            normal_map: Some(&n),
            ..Default::default()
        };
        let f = Fragment {
            vertex_colour: [0.25, 0.5, 0.75, 1.0],
            ..frag()
        };
        // The mapped normal faces the light at 0.6; nothing else is added.
        let want: Vec3 =
            std::array::from_fn(|i| 4.0 * c.0[i] * f.vertex_colour[i] * (a[i] + 0.6 * d[i]));
        near(&shaded(&m, &s, &f, &t).colour, &want, 1e-6);
    }

    // Specular, the eye direction, the reflection, no eye direction.

    /// A black part with a white spec map and fog on, so specular alone
    /// shows, at unit tints.
    fn sheen(m: Material) -> (Material, Scene) {
        let mut m = Material {
            spec_map: true,
            ..m
        };
        let mut s = fogged();
        unit_tints(&mut m, &mut s);
        (m, s)
    }

    fn sheen_textures<'a>(black: &'a Flat, white: &'a Flat) -> Textures<'a> {
        Textures {
            colour: Some(black),
            spec_map: Some(white),
            rim: Some(black),
            ..Default::default()
        }
    }

    const BLACK: Flat = Flat([0.0, 0.0, 0.0, 1.0]);
    const WHITE: Flat = Flat([1.0; 4]);

    #[test]
    fn the_eye_direction_is_clamped_per_component() {
        let (m, mut s) = sheen(Material::new());
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([0.6, 0.8, 0.0])
        };
        let f = Fragment {
            eye: [-0.6, 0.8, 0.0],
            ..frag()
        };
        // (0, 0.8, 0) · (0.6, 0.8, 0) = 0.64; unclamped it'd be 0.28.
        let got = shaded(&m, &s, &f, &sheen_textures(&BLACK, &WHITE));
        near(&got.colour, &[0.64; 3], 1e-6);
    }

    #[test]
    fn without_a_normal_specular_takes_the_way_the_light_travels() {
        let (m, mut s) = sheen(Material::new());
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([0.0, 0.0, 1.0])
        };
        let t = sheen_textures(&BLACK, &WHITE);
        assert!(!Switches::resolve(&m, &s).geometric_normal());
        near(&shaded(&m, &s, &frag(), &t).colour, &[1.0; 3], 1e-6);
        // A rim that adds nothing switches the geometric normal on, and the
        // light then reflects away from the eye: the floor.
        let (rimmed, _) = sheen(Material {
            rim: Some(Rim::new(0.0)),
            ..Material::new()
        });
        near(
            &shaded(&rimmed, &s, &frag(), &t).colour,
            &[SPECULAR_FLOOR; 3],
            1e-9,
        );
    }

    #[test]
    fn the_reflection_mirrors_about_the_normal_as_interpolated() {
        let (m, mut s) = sheen(Material {
            directional: true,
            ..Material::new()
        });
        let h = std::f32::consts::FRAC_1_SQRT_2;
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([h, 0.0, -h])
        };
        let f = Fragment {
            normal: [0.0, 0.0, 0.9],
            ..frag()
        };
        // t − 2(N·t)N with N 0.9 long: (h, 0, −h + 1.62h) normalized; its z
        // meets the eye at 0.62/√(1 + 0.62²), where a unit N gives h.
        let want = 0.62 / (1.0f32 + 0.62 * 0.62).sqrt();
        let got = shaded(&m, &s, &f, &sheen_textures(&BLACK, &WHITE)).colour;
        near(&got, &[want; 3], 1e-6);
        assert!((got[0] - h).abs() > 0.1);
    }

    #[test]
    fn an_exponent_under_one_widens_the_sheen() {
        let (mut m, mut s) = sheen(Material::new());
        m.first.exponent = 0.34;
        // Travelling at 60° off the eye: a dot of 0.5.
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([0.0, 0.866_025_4, 0.5])
        };
        let got = shaded(&m, &s, &frag(), &sheen_textures(&BLACK, &WHITE)).colour;
        near(&got, &[0.790_04; 3], 1e-4);
    }

    #[test]
    fn the_dot_is_floored_before_the_exponent() {
        let (mut m, mut s) = sheen(Material::new());
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([0.0, 0.0, -1.0])
        };
        let t = sheen_textures(&BLACK, &WHITE);
        // The dot is −1: the floor, raised to the exponent.
        near(&shaded(&m, &s, &frag(), &t).colour, &[0.00001; 3], 1e-10);
        m.first.exponent = 0.34;
        near(&shaded(&m, &s, &frag(), &t).colour, &[0.019_952_6; 3], 1e-6);
    }

    #[test]
    fn specular_skips_the_vertex_colour_and_takes_the_tints() {
        let (mut m, mut s) = sheen(Material {
            vertex_colour: true,
            ..Material::new()
        });
        m.tint = [1.0, 1.0, 1.0, 1.0];
        s.lights[0] = DirectionalLight {
            specular: [1.0; 3],
            ..light([0.0, 0.0, 1.0])
        };
        let f = Fragment {
            vertex_colour: [0.25, 0.25, 0.25, 1.0],
            ..frag()
        };
        // Tints 4 · 0.5 · 1 = 2; the vertex colour leaves specular alone.
        let got = shaded(&m, &s, &f, &sheen_textures(&BLACK, &WHITE)).colour;
        near(&got, &[2.0; 3], 1e-6);
    }

    #[test]
    fn without_fog_or_a_point_light_specular_is_only_the_floor() {
        let m = eruptor_body();
        let sp = [0.5, 0.4, 0.3];
        let key = DirectionalLight {
            specular: sp,
            ..light([0.0, 0.0, -1.0])
        };
        let (c, n, r) = (Flat([0.4, 0.2, 0.1, 1.0]), texel(0.0, 0.0), Flat([0.0; 4]));
        let (s_map, none) = (Flat([0.2, 0.3, 0.4, 1.0]), Flat([0.0; 4]));
        let colour = |scene: &Scene, spec: &Flat| {
            let t = Textures {
                colour: Some(&c),
                normal_map: Some(&n),
                spec_map: Some(spec),
                rim: Some(&r),
                ..Default::default()
            };
            shaded(&m, scene, &frag(), &t).colour
        };
        let mut dark = Scene::new();
        dark.lights[0] = key;
        assert!(!Switches::resolve(&m, &dark).eye_direction());
        let floor: Vec3 =
            std::array::from_fn(|i| SPECULAR_FLOOR * s_map.0[i] * sp[i] * NEUTRAL_SCALE);
        let added: Vec<f32> = (0..3)
            .map(|i| colour(&dark, &s_map)[i] - colour(&dark, &none)[i])
            .collect();
        near(&added, &floor, 1e-8);

        // With fog the eye direction is back: the light reflects into it.
        let mut lit = fogged();
        lit.lights[0] = key;
        let added: Vec<f32> = (0..3)
            .map(|i| colour(&lit, &s_map)[i] - colour(&lit, &none)[i])
            .collect();
        let sheen: Vec3 = std::array::from_fn(|i| s_map.0[i] * sp[i] * NEUTRAL_SCALE);
        near(&added, &sheen, 1e-6);

        // A point light brings it too.
        let lamp = Scene {
            point_lights: vec![PointLight {
                position: [0.0; 3],
                colour: [0.0; 3],
                linear: 0.0,
                quadratic: 0.0,
            }],
            ..dark
        };
        assert!(Switches::resolve(&m, &lamp).eye_direction());
    }

    // Tints.

    #[test]
    fn neutral_tints_scale_by_1_0079() {
        assert_eq!((NEUTRAL_SCALE * 10_000.0).round(), 10_079.0);
        let m = Material::new();
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let want = c.0.map(|v| v * NEUTRAL_SCALE);
        near(
            &shaded(&m, &Scene::new(), &frag(), &t).colour,
            &want[..3],
            1e-7,
        );
    }

    #[test]
    fn a_packed_tint_reads_red_from_its_low_byte() {
        let yellow = unpack_tint(0xFF00_F0F0);
        assert_eq!(yellow, [240.0 / 255.0, 240.0 / 255.0, 0.0, 1.0]);
        assert_eq!(
            unpack_tint(0x8040_2010),
            [16.0 / 255.0, 32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0]
        );
        let m = Material {
            tint: yellow,
            ..Material::new()
        };
        let c = Flat([1.0; 4]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let got = shaded(&m, &Scene::new(), &frag(), &t).colour;
        assert!(got[0] > 0.9 && got[0] == got[1] && got[2] == 0.0, "{got:?}");
    }

    // Alpha.

    #[test]
    fn the_alpha_ramp_starts_at_minus_0_001() {
        assert_eq!(layer_alpha(0.2, -1.0), 0.2);
        assert_eq!(layer_alpha(0.2, -0.001), 0.2);
        assert!((layer_alpha(0.2, -0.00095) - (0.2 - 0.00095) / 2.0).abs() < 1e-4);
        assert!((layer_alpha(0.2, -0.0009) + 0.0009).abs() < 1e-5);
        assert_eq!(layer_alpha(0.2, 0.0), 0.0);
        assert_eq!(layer_alpha(0.2, 0.5), 0.5);
    }

    #[test]
    fn alpha_takes_the_vertex_and_tint_alphas_without_the_four() {
        let mut m = Material {
            vertex_colour: true,
            ..Material::new()
        };
        m.first.fixed_alpha = 0.5;
        m.tint[3] = 0.5;
        let s = Scene {
            placement_tint: [NEUTRAL, NEUTRAL, NEUTRAL, 0.8],
            ..Scene::new()
        };
        let c = Flat([0.4, 0.3, 0.2, 0.9]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let f = Fragment {
            vertex_colour: [1.0, 1.0, 1.0, 0.5],
            ..frag()
        };
        assert!((shaded(&m, &s, &f, &t).alpha - 0.5 * 0.5 * 0.8 * 0.5).abs() < 1e-7);
    }

    // The rim lookup and blend.

    #[test]
    fn the_rim_is_looked_up_by_the_view_space_normal() {
        assert_eq!(rim_uv([0.0, 0.0, 1.0], 1.0), [0.5, 0.5]);
        near(&rim_uv([0.6, 0.8, 0.0], 2.0), &[1.6, 0.2], 1e-6);

        let at = |uv: Uv| [uv[0], uv[1], 0.0, 0.0];
        let mut m = Material {
            rim: Some(Rim::new(1.0)),
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let t = Textures {
            colour: Some(&BLACK),
            rim: Some(&at),
            ..Default::default()
        };
        // The geometric normal as interpolated, (0.3, 0.4, 0).
        let f = Fragment {
            normal: [0.3, 0.4, 0.0],
            ..frag()
        };
        near(&shaded(&m, &s, &f, &t).colour, &[0.65, 0.3, 0.0], 1e-6);
        // With a normal map, the mapped normal (−0.6, 0, 0.8).
        m.normal_map = true;
        let n = texel(0.6, 0.0);
        let t = Textures {
            normal_map: Some(&n),
            ..t
        };
        near(&shaded(&m, &s, &frag(), &t).colour, &[0.2, 0.5, 0.0], 1e-6);
    }

    /// An unlit part at unit tints showing `c` (alpha 0.5) under a rim of
    /// `r` with `rim`'s blend.
    fn rimmed(rim: Rim) -> Shaded {
        let mut m = Material {
            rim: Some(rim),
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let (c, r) = (Flat([0.4, 0.6, 0.8, 0.5]), Flat([0.5, 0.25, 1.0, 0.8]));
        let t = Textures {
            colour: Some(&c),
            rim: Some(&r),
            ..Default::default()
        };
        shaded(&m, &s, &frag(), &t)
    }

    #[test]
    fn the_rim_weighs_over_add_and_multiply() {
        // Strength 0.5: the sample scaled to (0.25, 0.125, 0.5, 0.4).
        let over = [0.45, 0.425, 0.9, 0.65];
        let add = [0.65, 0.725, 1.3, 0.9];
        let multiply = [0.1, 0.075, 0.4, 0.2];
        for (weights, want) in [
            (Weights::OVER, over),
            (Weights::ADD, add),
            (Weights::MULTIPLY, multiply),
        ] {
            let got = rimmed(Rim {
                strength: 0.5,
                weights,
                ..Rim::new(0.5)
            });
            near(&got.colour, &want[..3], 1e-6);
            assert_eq!(got.alpha, 0.5, "no split: alpha stays");
        }
        let w = Weights::new(0.2, 0.3, 0.5);
        let want: Vec<f32> = (0..3)
            .map(|i| 0.2 * over[i] + 0.3 * add[i] + 0.5 * multiply[i])
            .collect();
        let got = rimmed(Rim {
            weights: w,
            ..Rim::new(0.5)
        });
        near(&got.colour, &want, 1e-6);
    }

    #[test]
    fn the_split_moves_colour_and_alpha_opposite_ways() {
        let base = [0.4, 0.6, 0.8];
        let add = [0.65, 0.725, 1.3, 0.9];
        let all_alpha = rimmed(Rim {
            split: 1.0,
            ..Rim::new(0.5)
        });
        near(&all_alpha.colour, &base, 1e-7);
        assert!((all_alpha.alpha - add[3]).abs() < 1e-7);

        let quarter = rimmed(Rim {
            split: 0.25,
            ..Rim::new(0.5)
        });
        let want: Vec<f32> = (0..3)
            .map(|i| base[i] + (add[i] - base[i]) * 0.75)
            .collect();
        near(&quarter.colour, &want, 1e-6);
        assert!((quarter.alpha - (0.5 + (0.9 - 0.5) * 0.25)).abs() < 1e-6);
    }

    #[test]
    fn eruptors_spikes_multiply_by_twice_the_rim() {
        let got = rimmed(Rim {
            weights: Weights::MULTIPLY,
            ..Rim::new(2.0)
        });
        near(
            &got.colour,
            &[0.4 * 0.5 * 2.0, 0.6 * 0.25 * 2.0, 0.8 * 1.0 * 2.0],
            1e-6,
        );
        assert_eq!(got.alpha, 0.5);
    }

    // A second layer and its blends.

    fn two_layers(weights: Weights, fixed_alpha: f32) -> Shaded {
        let mut m = Material {
            second: Some(SecondLayer {
                layer: Layer {
                    fixed_alpha,
                    ..Layer::DEFAULT
                },
                weights,
            }),
            ..Material::new()
        };
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        // Tints of 2, to show the blend comes before them.
        m.tint = [1.0, 1.0, 1.0, 1.0];
        let (c1, c2) = (Flat([0.4, 0.6, 0.8, 1.0]), Flat([0.5, 0.25, 0.1, 1.0]));
        let t = Textures {
            colour: Some(&c1),
            second_colour: Some(&c2),
            ..Default::default()
        };
        shaded(&m, &s, &frag(), &t)
    }

    #[test]
    fn a_second_layer_blends_by_its_coverage_before_the_tints() {
        let (c1, c2) = ([0.4, 0.6, 0.8], [0.5, 0.25, 0.1]);
        let over: Vec<f32> = (0..3)
            .map(|i| 2.0 * (0.25 * c2[i] + 0.75 * c1[i]))
            .collect();
        near(&two_layers(Weights::OVER, 0.25).colour, &over, 1e-6);
        let add: Vec<f32> = (0..3).map(|i| 2.0 * (c1[i] + 0.25 * c2[i])).collect();
        near(&two_layers(Weights::ADD, 0.25).colour, &add, 1e-6);
        let multiply: Vec<f32> = (0..3).map(|i| 2.0 * c1[i] * c2[i]).collect();
        near(&two_layers(Weights::MULTIPLY, 1.0).colour, &multiply, 1e-6);
        // At a quarter: the first times the covered second, plus the first
        // times three quarters.
        let partial: Vec<f32> = (0..3)
            .map(|i| 2.0 * (c1[i] * 0.25 * c2[i] + 0.75 * c1[i]))
            .collect();
        near(&two_layers(Weights::MULTIPLY, 0.25).colour, &partial, 1e-6);
    }

    #[test]
    fn a_second_layer_needs_its_own_maps() {
        let m = Material {
            normal_map: true,
            second: Some(SecondLayer {
                layer: Layer::DEFAULT,
                weights: Weights::ADD,
            }),
            ..Material::new()
        };
        let n = texel(0.0, 0.0);
        let t = Textures {
            colour: Some(&BLACK),
            normal_map: Some(&n),
            second_colour: Some(&BLACK),
            ..Default::default()
        };
        assert_eq!(
            shade(&m, &Scene::new(), &frag(), &t),
            Err(Error::Missing(Role::SecondNormalMap))
        );
    }

    // A third layer and the texture-coordinate sets.

    fn third(strength: f32, weights: Weights) -> Material {
        Material {
            third: Some(ThirdLayer { strength, weights }),
            ..Material::new()
        }
    }

    #[test]
    fn a_third_layer_blends_after_the_tints_and_leaves_alpha() {
        let (c, l3) = (Flat([0.4, 0.6, 0.8, 0.7]), Flat([0.5, 0.25, 0.1, 0.2]));
        let t = Textures {
            colour: Some(&c),
            third: Some(&l3),
            ..Default::default()
        };
        let s = Scene::new();
        let tinted = c.0.map(|v| v * NEUTRAL_SCALE);
        let got = shaded(&third(1.0, Weights::ADD), &s, &frag(), &t);
        let want: Vec<f32> = (0..3).map(|i| tinted[i] + l3.0[i]).collect();
        near(&got.colour, &want, 1e-6);
        assert!(
            (got.alpha - 0.7).abs() < 1e-7,
            "the third layer leaves alpha"
        );
        let got = shaded(&third(0.25, Weights::OVER), &s, &frag(), &t).colour;
        let want: Vec<f32> = (0..3).map(|i| 0.25 * l3.0[i] + 0.75 * tinted[i]).collect();
        near(&got, &want, 1e-6);
        let got = shaded(&third(0.5, Weights::MULTIPLY), &s, &frag(), &t).colour;
        let want: Vec<f32> = (0..3).map(|i| tinted[i] * 0.5 * l3.0[i]).collect();
        near(&got, &want, 1e-6);
    }

    #[test]
    fn a_third_layer_without_a_second_reads_the_second_set() {
        let mut m = third(1.0, Weights::ADD);
        assert_eq!(
            uv_sets(&m),
            UvSets {
                first: 0,
                second: None,
                third: Some(1)
            }
        );
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        m.uv_transforms[1] = UvTransform {
            u: [2.0, 0.0, 0.1],
            v: [0.0, 1.0, 0.0],
        };
        let at = |uv: Uv| [uv[0], uv[1], 0.0, 0.0];
        let t = Textures {
            colour: Some(&BLACK),
            second_colour: Some(&BLACK),
            third: Some(&at),
            ..Default::default()
        };
        let f = Fragment {
            uv: [[0.0, 0.0], [0.3, 0.7], [0.9, 0.1]],
            ..frag()
        };
        // The second set through the second transform: (2·0.3 + 0.1, 0.7).
        near(&shaded(&m, &s, &f, &t).colour, &[0.7, 0.7, 0.0], 1e-6);

        m.second = Some(SecondLayer {
            layer: Layer::DEFAULT,
            weights: Weights::ADD,
        });
        assert_eq!(
            uv_sets(&m),
            UvSets {
                first: 0,
                second: Some(1),
                third: Some(2)
            }
        );
        near(&shaded(&m, &s, &f, &t).colour, &[0.9, 0.1, 0.0], 1e-6);
    }

    #[test]
    fn each_set_goes_through_its_own_transform() {
        let x = UvTransform {
            u: [0.5, 0.25, 0.1],
            v: [-1.0, 2.0, 0.3],
        };
        near(
            &x.apply([0.4, 0.8]),
            &[0.5 * 0.4 + 0.25 * 0.8 + 0.1, -0.4 + 1.6 + 0.3],
            1e-6,
        );
        assert_eq!(UvTransform::IDENTITY.apply([0.4, 0.8]), [0.4, 0.8]);
        // The first layer reads the first set through the first transform.
        let mut m = Material::new();
        m.uv_transforms[0] = x;
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let at = |uv: Uv| [uv[0], uv[1], 0.0, 1.0];
        let t = Textures {
            colour: Some(&at),
            ..Default::default()
        };
        let f = Fragment {
            uv: [[0.4, 0.8], [0.0; 2], [0.0; 2]],
            ..frag()
        };
        let want = x.apply([0.4, 0.8]);
        near(
            &shaded(&m, &s, &f, &t).colour,
            &[want[0], want[1], 0.0],
            1e-6,
        );
    }

    // The order of the terms, shadows, point lights.

    #[test]
    fn a_shadow_scales_by_its_sample() {
        let mut m = Material::new();
        let mut s = Scene {
            shadows: true,
            ..Scene::new()
        };
        unit_tints(&mut m, &mut s);
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        let f = Fragment {
            shadow: [0.2, 0.4, 0.6, 0.5],
            ..frag()
        };
        // Mode 2: 2·a, so alpha 0.5 is unshadowed.
        near(&shaded(&m, &s, &f, &t).colour, &[0.4, 0.3, 0.2], 1e-7);
        // Mode 1: 2·rgb.
        s.shadow_mode = 1.0;
        near(
            &shaded(&m, &s, &f, &t).colour,
            &[0.4 * 0.4, 0.3 * 0.8, 0.2 * 1.2],
            1e-6,
        );
    }

    fn lamp(position: Vec3, linear: f32, quadratic: f32) -> PointLight {
        PointLight {
            position,
            colour: [1.0, 0.5, 0.25],
            linear,
            quadratic,
        }
    }

    #[test]
    fn a_point_light_lights_a_black_part() {
        let mut m = Material::new();
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        let t = Textures {
            colour: Some(&BLACK),
            ..Default::default()
        };
        // 2 in front of the part, head on.
        s.point_lights = vec![lamp([0.0, 0.0, -3.0], 1.0, 0.0)];
        let d = 2.001;
        near(
            &shaded(&m, &s, &frag(), &t).colour,
            &[1.0 / d, 0.5 / d, 0.25 / d],
            1e-6,
        );
        s.point_lights = vec![lamp([0.0, 0.0, -3.0], 0.0, 1.0)];
        let d2 = d * d;
        near(
            &shaded(&m, &s, &frag(), &t).colour,
            &[1.0 / d2, 0.5 / d2, 0.25 / d2],
            1e-6,
        );
        // Edge on, nothing; on a normal half as long, half.
        s.point_lights = vec![lamp([2.0, 0.0, -5.0], 1.0, 0.0)];
        near(&shaded(&m, &s, &frag(), &t).colour, &[0.0; 3], 1e-7);
        s.point_lights = vec![lamp([0.0, 0.0, -3.0], 1.0, 0.0)];
        let f = Fragment {
            normal: [0.0, 0.0, 0.5],
            ..frag()
        };
        near(
            &shaded(&m, &s, &f, &t).colour,
            &[0.5 / d, 0.25 / d, 0.125 / d],
            1e-6,
        );
        // The colour map doesn't scale it: white adds itself, no more.
        let white = Textures {
            colour: Some(&WHITE),
            ..t
        };
        near(
            &shaded(&m, &s, &frag(), &white).colour,
            &[1.0 + 1.0 / d, 1.0 + 0.5 / d, 1.0 + 0.25 / d],
            1e-6,
        );
    }

    #[test]
    fn the_terms_apply_in_order() {
        let mut s = Scene {
            shadows: true,
            ..Scene::new()
        };
        let mut m = Material {
            rim: Some(Rim::new(1.0)),
            ..Material::new()
        };
        unit_tints(&mut m, &mut s);
        let c = Flat([0.4, 0.3, 0.2, 1.0]);
        let r = Flat([0.1, 0.2, 0.3, 0.0]);
        let t = Textures {
            colour: Some(&c),
            rim: Some(&r),
            ..Default::default()
        };
        // A full shadow (mode 2, alpha 0) zeroes the colour; the rim comes
        // after, so it shows.
        let f = Fragment {
            shadow: [0.0; 4],
            ..frag()
        };
        near(&shaded(&m, &s, &f, &t).colour, &[0.1, 0.2, 0.3], 1e-7);

        // A black rim multiplied zeroes it; the point light after adds.
        m.rim = Some(Rim {
            weights: Weights::MULTIPLY,
            ..Rim::new(1.0)
        });
        s.shadows = false;
        s.point_lights = vec![lamp([0.0, 0.0, -3.0], 1.0, 0.0)];
        let black_rim = Textures {
            rim: Some(&BLACK),
            ..t
        };
        let d = 2.001;
        near(
            &shaded(&m, &s, &f, &black_rim).colour,
            &[1.0 / d, 0.5 / d, 0.25 / d],
            1e-6,
        );

        // The replace colour covers the point light; fog comes after it.
        s.replace = [0.9, 0.8, 0.7, 1.0];
        near(
            &shaded(&m, &s, &f, &black_rim).colour,
            &[0.9, 0.8, 0.7],
            1e-6,
        );
        s.fog = true;
        s.fog_colour = [0.1, 0.2, 0.3];
        s.fog_near = 0.0;
        s.fog_far = 10.0;
        near(
            &shaded(&m, &s, &f, &black_rim).colour,
            &[0.5, 0.5, 0.5],
            1e-6,
        );
    }

    // Replace colour and fog.

    #[test]
    fn the_replace_colour_mixes_by_its_amount() {
        let mut m = Material::new();
        let mut s = Scene::new();
        unit_tints(&mut m, &mut s);
        s.replace = [1.0, 0.0, 0.5, 0.25];
        let c = Flat([0.4, 0.8, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        near(
            &shaded(&m, &s, &frag(), &t).colour,
            &[0.55, 0.6, 0.275],
            1e-6,
        );
    }

    #[test]
    fn fog_mixes_by_depth_clamped() {
        assert_eq!(fog_factor(5.0, FOG_NEAR, FOG_FAR), 0.0);
        assert_eq!(fog_factor(400.0, FOG_NEAR, FOG_FAR), 1.0);
        assert!((fog_factor((FOG_NEAR + FOG_FAR) / 2.0, FOG_NEAR, FOG_FAR) - 0.5).abs() < 1e-6);

        let mut m = Material::new();
        let mut s = fogged();
        unit_tints(&mut m, &mut s);
        let c = Flat([0.4, 0.8, 0.2, 1.0]);
        let t = Textures {
            colour: Some(&c),
            ..Default::default()
        };
        near(&shaded(&m, &s, &frag(), &t).colour, &[0.4, 0.8, 0.2], 1e-7);
        let far = Fragment {
            depth: 400.0,
            ..frag()
        };
        near(&shaded(&m, &s, &far, &t).colour, &s.fog_colour, 1e-7);
        s.fog = false;
        near(&shaded(&m, &s, &far, &t).colour, &[0.4, 0.8, 0.2], 1e-7);
    }

    // Render states.

    fn arriving(colour: Vec3, alpha: f32) -> Arriving {
        Arriving {
            shaded: Shaded { colour, alpha },
            depth: 0.25,
            front_facing: true,
        }
    }

    const OPAQUE: RenderStates = RenderStates {
        blend: Blend::Opaque,
        depth_write: true,
        two_sided: false,
        alpha_test: None,
    };

    const CLEAR: Pixel = Pixel {
        colour: [0.0; 3],
        depth: 1.0,
    };

    #[test]
    fn two_additive_parts_add() {
        let additive = RenderStates {
            blend: Blend::Additive,
            depth_write: false,
            ..OPAQUE
        };
        let p = draw(CLEAR, &arriving([0.5, 0.25, 0.1], 1.0), &additive);
        let p = draw(p, &arriving([0.4, 0.4, 0.4], 0.5), &additive);
        near(&p.colour, &[0.7, 0.45, 0.3], 1e-7);
    }

    /// A level's multiplied decal (AG.15) scales what's there by its
    /// colour, whatever its alpha.
    #[test]
    fn a_multiplied_part_scales_what_is_there() {
        let multiply = RenderStates {
            blend: Blend::Multiply,
            depth_write: false,
            ..OPAQUE
        };
        let under = Pixel {
            colour: [0.8, 0.5, 0.25],
            depth: 1.0,
        };
        let p = draw(under, &arriving([0.5, 1.0, 2.0], 0.25), &multiply);
        near(&p.colour, &[0.4, 0.5, 0.5], 1e-7);
        assert_eq!(p.depth, 1.0);
    }

    #[test]
    fn a_blended_part_mixes_by_its_alpha() {
        let blended = RenderStates {
            blend: Blend::Alpha,
            ..OPAQUE
        };
        let under = Pixel {
            colour: [1.0, 0.0, 0.0],
            depth: 1.0,
        };
        let p = draw(under, &arriving([0.0, 1.0, 0.0], 0.25), &blended);
        near(&p.colour, &[0.75, 0.25, 0.0], 1e-7);
        // Opaque replaces whatever its alpha.
        assert_eq!(
            draw(under, &arriving([0.0, 1.0, 0.0], 0.25), &OPAQUE).colour,
            [0.0, 1.0, 0.0]
        );
    }

    #[test]
    fn an_alpha_test_passes_only_above_its_reference() {
        let tested = RenderStates {
            alpha_test: Some(0.5),
            ..OPAQUE
        };
        assert_eq!(draw(CLEAR, &arriving([1.0; 3], 0.5), &tested), CLEAR);
        assert_eq!(
            draw(CLEAR, &arriving([1.0; 3], 0.51), &tested).colour,
            [1.0; 3]
        );
    }

    #[test]
    fn back_faces_are_culled_unless_two_sided() {
        let back = Arriving {
            front_facing: false,
            ..arriving([1.0; 3], 1.0)
        };
        assert_eq!(draw(CLEAR, &back, &OPAQUE), CLEAR);
        let two_sided = RenderStates {
            two_sided: true,
            ..OPAQUE
        };
        assert_eq!(draw(CLEAR, &back, &two_sided).colour, [1.0; 3]);
    }

    #[test]
    fn a_part_not_writing_depth_leaves_it() {
        let a = arriving([1.0; 3], 1.0);
        assert_eq!(draw(CLEAR, &a, &OPAQUE).depth, 0.25);
        let no_write = RenderStates {
            depth_write: false,
            ..OPAQUE
        };
        assert_eq!(draw(CLEAR, &a, &no_write).depth, 1.0);
    }

    // Glow.

    #[test]
    fn eruptors_body_glows_by_its_amount_and_his_eyes_not_at_all() {
        let s = Scene::new();
        assert_eq!(glow_mask(&eruptor_body(), &s, &frag(), 0.7), 64.0 / 255.0);
        let eyes = Material {
            glow: Glow::None,
            ..eruptor_body()
        };
        assert_eq!(glow_mask(&eyes, &s, &frag(), 0.7), 0.0);
    }

    #[test]
    fn the_glow_switches_to_the_fixed_alpha_hard_at_zero() {
        let s = Scene {
            placement_tint: [NEUTRAL, NEUTRAL, NEUTRAL, 0.8],
            ..Scene::new()
        };
        let mut m = Material {
            glow: Glow::LayerAlpha,
            ..Material::new()
        };
        m.first.fixed_alpha = -0.0005;
        // Past the main pass's ramp, which takes the fixed alpha from
        // -0.0009, the glow's switch at 0 still takes the colour map's.
        assert_eq!(glow_mask(&m, &Scene::new(), &frag(), 0.7), 0.7);
        assert!((layer_alpha(0.7, -0.0005) + 0.0005).abs() < 1e-7);
        m.first.fixed_alpha = 0.0;
        assert_eq!(glow_mask(&m, &Scene::new(), &frag(), 0.7), 0.0);
        m.first.fixed_alpha = 0.3;
        m.vertex_colour = true;
        m.tint[3] = 0.5;
        let f = Fragment {
            vertex_colour: [1.0, 1.0, 1.0, 0.5],
            ..frag()
        };
        assert!((glow_mask(&m, &s, &f, 0.7) - 0.3 * 0.5 * 0.8 * 0.5).abs() < 1e-7);
        // An amount doesn't touch the glow alpha the alpha test reads.
        m.glow = Glow::Amount(0.25);
        assert_eq!(glow_mask(&m, &s, &f, 0.7), 0.25);
        assert!((glow_alpha(&m, &s, &f, 0.7) - 0.3 * 0.5 * 0.8 * 0.5).abs() < 1e-7);
    }

    // The remaster: nothing clamped before the tonemapper.

    #[test]
    fn an_orange_past_full_stays_past_full_in_every_channel() {
        let m = Material {
            rim: Some(Rim::new(1.0)),
            ..Material::new()
        };
        let (c, r) = (Flat([0.9, 0.45, 0.05, 1.0]), Flat([0.6, 0.3, 0.02, 0.0]));
        let t = Textures {
            colour: Some(&c),
            rim: Some(&r),
            ..Default::default()
        };
        let mut s = Scene::new();
        let orange = shaded(&m, &s, &frag(), &t).colour;
        assert!(orange[0] > 1.5, "{orange:?}");
        // Red stays twice green: no channel was clipped on its own.
        assert!((orange[0] - 2.0 * orange[1]).abs() < 1e-6, "{orange:?}");
        // A point light adding 4 to every channel (falloff 2.001 / 2.001)
        // takes them all past full, none held back.
        s.point_lights = vec![PointLight {
            position: [0.0, 0.0, -3.0],
            colour: [4.0, 4.0, 4.0],
            linear: 2.001,
            quadratic: 0.0,
        }];
        let got = shaded(&m, &s, &frag(), &t).colour;
        near(&got, &orange.map(|v| v + 4.0), 1e-5);
    }
}
