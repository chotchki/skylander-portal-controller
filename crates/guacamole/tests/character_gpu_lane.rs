// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! The character shader against the character-shading spec's CPU model
//! (openspec AE.8, design decision 10): with the look a remaster, this
//! lane is the only check that pins each shading term on the GPU's output.
//!
//! Every material key AE.5 counted on the roster's 67 character models is
//! drawn through the real pipeline (the IR's mesh, Bevy's GPU skinning on
//! one identity joint, the WESL shader specialized for the key) into an
//! `Rgba16Float` target with tonemapping off, and every channel of every
//! pixel is compared with [`guac_ir::shading`] fed the same inputs: light,
//! fog and replace colours decoded through `bevy_color`, tints and vertex
//! colours as stored.
//!
//! Three passes share one set of random inputs:
//! 1. half-float textures holding the exact decodes, so the shader and the
//!    model read the same texels: every channel within one half step of
//!    the model's value as the target stores it (the M3 Max truncates to
//!    half on store, so that's the value truncated);
//! 2. the decode probes: what the sampler hands the shader for each 8-bit
//!    code, against the exact decode, inside the envelope AE.8 measured;
//! 3. the same inputs on 8-bit textures through the remaster's own views
//!    (`Rgba8UnormSrgb` for colour, spec, rim and layer maps, `Rgba8Unorm`
//!    for normal and shadow maps), against the model on exact decodes:
//!    recorded, since the hardware hands some samples back at its reduced
//!    precision and some at full, as the shader compiles them.
//!
//! Each pixel is its own quad: corners on the pixel's corners under an
//! orthographic camera of one unit per pixel, so the pixel centre sits on
//! the quad's diagonal and takes exactly the mean of the diagonal's two
//! vertices (the other two weigh nothing). Those two carry different
//! normals, tangents, vertex colours and eye directions, so the spec's
//! "as interpolated, not renormalized" is exercised. Textures are 4x4
//! blocks of equal texels sampled nearest; texture-coordinate sets land on
//! texel centres and rim lookups well inside a block.
//!
//! `cargo test -p guacamole --release --test character_gpu_lane -- --ignored --nocapture --test-threads 1`
//! (`GUAC_LANE_PROBE=1` prints each code's decode, `GUAC_LANE_DEBUG=<key>`
//! every input of that key's worst misses).

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    ClearColorConfig, Hdr, OrthographicProjection, Projection, RenderTarget, ScalingMode,
};
use bevy::color::{LinearRgba, Srgba};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::log::LogPlugin;
use bevy::math::Vec4;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::{
    App, AppExit, Assets, Camera, Camera3d, Color, Commands, DefaultPlugins, Handle, Image, Local,
    Mesh, MessageWriter, On, PluginGroup, Res, ResMut, Resource, Startup, Transform, Update,
    WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_ir::clip::Clip;
use guac_ir::model::{self, Blend, Joint, Mask, Model, Segment, Skeleton};
use guac_ir::shading::{
    self, DirectionalLight, Fixed, Fragment, Layer, PointLight, Rim, Scene, SecondLayer,
    ThirdLayer, UvTransform, Weights,
};
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, CharacterTextures, MaterialKey, SceneBits,
    ShaderKey, ShadowLookup, StateKey, spawn_character,
};

/// The decode probes ahead of the keys' cases.
const PROBES: usize = 2;
/// The target, in pixels: four materials of 16x16 quads each.
const SIZE: u32 = 32;
const REGION: u32 = 16;
/// Texels a side of every test texture but the probe's and the shadow's,
/// in 4x4 blocks of equal texels.
const TEXELS: u32 = 16;
const BLOCK: u32 = 4;
/// The clear colour, exact in half precision.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.125, 0.75];
/// How far, in texels, a lookup keeps from a block's edge past the half
/// texel bilinear filtering reaches.
const MARGIN: f32 = 0.25;

// ---- AE.5's census: every stored key on the roster's character models ----

#[derive(Debug, Clone, Copy)]
struct Stored {
    pixel: Mask,
    vertex: Mask,
    states: StateKey,
}

const fn stored(
    pixel: u32,
    pixel_ignore: u32,
    vertex: u32,
    vertex_ignore: u32,
    blend: Blend,
    depth_write: bool,
    two_sided: bool,
    alpha_test: bool,
) -> Stored {
    Stored {
        pixel: Mask {
            base: pixel,
            ignore: pixel_ignore,
        },
        vertex: Mask {
            base: vertex,
            ignore: vertex_ignore,
        },
        states: StateKey {
            blend,
            depth_write,
            two_sided,
            alpha_test,
        },
    }
}

/// The 37 distinct stored keys of the 237 character materials (AE.5's
/// census: both masks, both ignores and the render states).
const CENSUS: [Stored; 37] = {
    use Blend::{Additive as Add, Alpha as A, Opaque as O};
    const P: u32 = 0x2FF;
    const V: u32 = 0x1AFF;
    const X: u32 = 0x3AFF;
    [
        stored(0x3C, P, 0x3C, V, O, true, false, false),
        stored(0x24, P, 0x24, V, O, true, false, false),
        stored(0x3C, X, 0x3C, X, O, true, false, false),
        stored(0x3C, P, 0x103C, V, O, true, false, false),
        stored(0x04, P, 0x04, V, O, true, false, false),
        stored(0x3C, P, 0x103C, V, A, true, false, false),
        stored(0x38, P, 0x38, V, O, true, false, false),
        stored(0x2C, P, 0x2C, V, O, true, false, false),
        stored(0x1C, P, 0x1C, V, O, true, false, false),
        stored(0x04, P, 0x1004, V, A, false, false, false),
        stored(0x38, P, 0x1038, V, O, true, false, false),
        stored(0x00, P, 0x00, V, O, true, false, false),
        stored(0x3C, P, 0x3C, V, O, true, false, true),
        stored(0x2C, P, 0x102C, V, O, true, false, false),
        stored(0x24, P, 0x24, V, O, true, false, true),
        stored(0x3C, X, 0x3C, X, O, true, false, true),
        stored(0x3C, P, 0x3C, V, O, true, true, false),
        stored(0x3C, P, 0x3C, V, A, true, true, false),
        stored(0x3C, P, 0x3C, V, A, true, false, true),
        stored(0x28, P, 0x28, V, O, true, false, false),
        stored(0x1C, X, 0x1C, X, O, true, false, false),
        stored(0x04, P, 0x04, V, Add, false, false, false),
        stored(0x3C, P, 0x3C, V, O, true, true, true),
        stored(0x3C, P, 0x3C, V, A, true, true, true),
        stored(0x3C, P, 0x3C, V, A, true, false, false),
        stored(0x3C, P, 0x3C, V, A, false, true, false),
        stored(0x38, P, 0x38, V, O, true, true, true),
        stored(0x38, P, 0x38, V, O, true, true, false),
        stored(0x24, P, 0x24, V, O, true, true, true),
        stored(0x24, P, 0x24, V, A, true, true, true),
        stored(0x20, P, 0x20, V, O, true, false, false),
        stored(0x1C, P, 0x101C, V, O, true, false, false),
        stored(0x1C, P, 0x1C, V, O, true, false, true),
        stored(0x18, P, 0x18, V, O, true, false, false),
        stored(0x04, X, 0x04, X, O, true, false, false),
        stored(0x04, P, 0x1004, V, O, true, false, false),
        stored(0x04, P, 0x1004, V, A, false, true, false),
    ]
};

/// The keys `scenes` draw the census with, each once.
fn drawn(scenes: &[SceneBits]) -> Vec<MaterialKey> {
    let mut keys = HashSet::new();
    let mut out = Vec::new();
    for scene in scenes {
        for s in &CENSUS {
            let shader = ShaderKey::resolve(s.pixel, s.vertex, *scene).expect("a census key");
            let k = MaterialKey {
                shader,
                states: s.states,
            };
            if keys.insert(k) {
                out.push(k);
            }
        }
    }
    out
}

/// Every scene the runtime can set.
fn every_scene() -> Vec<SceneBits> {
    let mut v = Vec::new();
    for fog in [true, false] {
        for shadows in [false, true] {
            for point_lights in 0..=3 {
                v.push(SceneBits {
                    shadows,
                    fog,
                    point_lights,
                });
            }
        }
    }
    v
}

/// Keys no character has: the second and third layers the spec defines,
/// alone, together and with everything else, so their terms are drawn too.
fn layer_keys() -> Vec<MaterialKey> {
    [
        (0x401, 0x401),
        (0x402, 0x402),
        (0x403, 0x403),
        (0x41B, 0x141B),
        (0x43F, 0x143F),
        (0x3D3F, 0x253F),
    ]
    .map(|(p, v)| MaterialKey {
        shader: ShaderKey::new(p, v).expect("a layer key"),
        states: StateKey::OPAQUE,
    })
    .to_vec()
}

// ---- numbers ----

/// SplitMix64: the lane's inputs are reproducible from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next() % u64::from(n)) as u32
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    fn direction(&mut self) -> [f32; 3] {
        loop {
            let v = [
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            ];
            let l = dot(v, v).sqrt();
            if (0.2..=1.0).contains(&l) {
                return v.map(|c| c / l);
            }
        }
    }

    /// A colour as the disc stores one, decoded as the remaster does.
    fn colour(&mut self, max: u8) -> [f32; 3] {
        let mut c = || (u32::from(self.byte()) * u32::from(max) / 255) as u8;
        let l = LinearRgba::from(Srgba::rgb_u8(c(), c(), c()));
        [l.red, l.green, l.blue]
    }

    fn weights(&mut self) -> Weights {
        Weights::new(
            self.range(0.0, 1.0),
            self.range(0.0, 1.0),
            self.range(0.0, 1.0),
        )
    }

    /// A fixed alpha: the colour map's (-1), its own, or partway up the ramp.
    fn fixed_alpha(&mut self) -> f32 {
        match self.below(4) {
            0 | 1 => -1.0,
            2 => self.range(0.0, 1.0),
            _ => -0.00095,
        }
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    a.map(|c| c / l)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn mean<const N: usize>(a: [f32; N], b: [f32; N]) -> [f32; N] {
    std::array::from_fn(|i| 0.5 * a[i] + 0.5 * b[i])
}

/// sRGB's decode, in f64 then rounded once.
fn srgb(code: u8) -> f32 {
    let v = f64::from(code) / 255.0;
    let l = if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    };
    l as f32
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = i32::from(h >> 10 & 0x1F);
    let frac = f32::from(h & 0x3FF);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The half nearest `v`, as its bits; `v` in [0, 65504].
fn f32_to_f16(v: f32) -> u16 {
    let v = round_half(v);
    if v == 0.0 {
        return 0;
    }
    let e = v.log2().floor() as i32;
    if e < -14 {
        return (v / 2f32.powi(-24)) as u16;
    }
    let mantissa = ((v / 2f32.powi(e) - 1.0) * 1024.0).round() as u16;
    (((e + 15) as u16) << 10) | mantissa
}

/// One half-precision step at `v`: the spacing of the halves around it.
fn half_step(v: f32) -> f32 {
    let a = v.abs();
    if a < 2f32.powi(-14) {
        2f32.powi(-24)
    } else {
        2f32.powi(a.log2().floor() as i32 - 10)
    }
}

/// The half nearest `v` (ties to even), as f32.
fn round_half(v: f32) -> f32 {
    let step = half_step(v);
    let q = v / step;
    let r = q.round();
    let r = if (q - q.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - q.signum()
    } else {
        r
    };
    r * step
}

// ---- textures ----

/// What a texel's 8-bit codes come back as from the sampler: the exact
/// decode (sRGB's curve, or code / 255), or what the hardware returned for
/// each code in the probe.
#[derive(Clone)]
struct Decode {
    srgb_rgb: [f32; 256],
    srgb_alpha: [f32; 256],
    unorm: [f32; 256],
}

impl Decode {
    fn exact() -> Self {
        let unorm = std::array::from_fn(|k| k as f32 / 255.0);
        Self {
            srgb_rgb: std::array::from_fn(|k| srgb(k as u8)),
            srgb_alpha: unorm,
            unorm,
        }
    }
}

/// A texture of 4x4 blocks of equal texels, sampled nearest: a texture-
/// coordinate set lands on a texel's centre, a rim lookup anywhere well
/// inside a block, so the sampler returns one texel's value.
#[derive(Clone)]
struct Blocks {
    size: u32,
    srgb: bool,
    /// Stored as half floats holding the exact decode, rounded to half: the
    /// shader and the model then read the same texels whatever precision
    /// the sampler hands back.
    half: bool,
    /// RGBA codes per block, row by row.
    codes: Vec<[u8; 4]>,
}

impl Blocks {
    fn random(rng: &mut Rng, size: u32, srgb: bool) -> Self {
        let n = (size / BLOCK) * (size / BLOCK);
        Self {
            size,
            srgb,
            half: false,
            codes: (0..n)
                .map(|_| [rng.byte(), rng.byte(), rng.byte(), rng.byte()])
                .collect(),
        }
    }

    /// A normal map whose texels keep X² + Y² off 1 by 0.05: right at the
    /// unit circle Z is the square root of a near-zero difference, where a
    /// texel's last bit, or f32 rounding, moves the normal by degrees.
    /// Texels past the circle (the spec's absolute value) stay in.
    fn random_normal_map(rng: &mut Rng) -> Self {
        let mut b = Self::random(rng, TEXELS, false);
        for code in &mut b.codes {
            loop {
                let x = 2.0 * f32::from(code[3]) / 255.0 - 1.0;
                let y = 2.0 * f32::from(code[1]) / 255.0 - 1.0;
                if (1.0 - x * x - y * y).abs() >= 0.05 {
                    break;
                }
                code[1] = rng.byte();
                code[3] = rng.byte();
            }
        }
        b
    }

    fn blocks(&self) -> u32 {
        self.size / BLOCK
    }

    fn decoded(&self, code: [u8; 4], decode: &Decode) -> [f32; 4] {
        if self.half {
            let exact = Self {
                half: false,
                ..self.clone()
            };
            return exact.decoded(code, &Decode::exact()).map(round_half);
        }
        let at = |table: &[f32; 256], c: u8| table[usize::from(c)];
        if self.srgb {
            [
                at(&decode.srgb_rgb, code[0]),
                at(&decode.srgb_rgb, code[1]),
                at(&decode.srgb_rgb, code[2]),
                at(&decode.srgb_alpha, code[3]),
            ]
        } else {
            code.map(|c| at(&decode.unorm, c))
        }
    }

    /// The block a coordinate repeats into, if it lands well inside it.
    fn block(&self, uv: [f32; 2]) -> Option<usize> {
        let mut index = [0u32; 2];
        for (axis, c) in uv.into_iter().enumerate() {
            let t = (c - c.floor()) * self.size as f32;
            let within = t % BLOCK as f32;
            let edge = within.min(BLOCK as f32 - within);
            if edge < 0.5 + MARGIN || !t.is_finite() {
                return None;
            }
            index[axis] = (t / BLOCK as f32) as u32 % self.blocks();
        }
        Some((index[1] * self.blocks() + index[0]) as usize)
    }

    fn sample(&self, uv: [f32; 2], decode: &Decode) -> Option<[f32; 4]> {
        self.block(uv).map(|b| self.decoded(self.codes[b], decode))
    }

    /// The centre of one of a random block's two inner texels a side.
    fn point(&self, rng: &mut Rng) -> [f32; 2] {
        let pick = |rng: &mut Rng| {
            let b = rng.below(self.blocks()) as f32;
            (b * BLOCK as f32 + 1.5 + rng.below(2) as f32) / self.size as f32
        };
        [pick(rng), pick(rng)]
    }

    fn image(&self) -> Image {
        let mut data = Vec::new();
        for y in 0..self.size {
            for x in 0..self.size {
                let code = self.codes[((y / BLOCK) * self.blocks() + x / BLOCK) as usize];
                if self.half {
                    for v in self.decoded(code, &Decode::exact()) {
                        data.extend_from_slice(&f32_to_f16(v).to_le_bytes());
                    }
                } else {
                    data.extend_from_slice(&code);
                }
            }
        }
        let mut image = Image::new(
            Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            if self.half {
                TextureFormat::Rgba16Float
            } else if self.srgb {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            },
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Nearest,
            min_filter: ImageFilterMode::Nearest,
            ..ImageSamplerDescriptor::default()
        });
        image
    }
}

/// The textures one material reads, by the reference model's roles.
#[derive(Clone)]
struct Maps {
    colour: Blocks,
    normal_map: Blocks,
    spec_map: Blocks,
    second_colour: Blocks,
    second_normal_map: Blocks,
    second_spec_map: Blocks,
    third: Blocks,
    rim: Blocks,
    /// One block per pixel of the target ([`shadow_lookup`]).
    shadow: Blocks,
}

impl Maps {
    fn halved(&self) -> Self {
        let mut m = self.clone();
        for b in [
            &mut m.colour,
            &mut m.normal_map,
            &mut m.spec_map,
            &mut m.second_colour,
            &mut m.second_normal_map,
            &mut m.second_spec_map,
            &mut m.third,
            &mut m.rim,
            &mut m.shadow,
        ] {
            b.half = true;
        }
        m
    }

    fn random(rng: &mut Rng) -> Self {
        Self {
            colour: Blocks::random(rng, TEXELS, true),
            normal_map: Blocks::random_normal_map(rng),
            spec_map: Blocks::random(rng, TEXELS, true),
            second_colour: Blocks::random(rng, TEXELS, true),
            second_normal_map: Blocks::random_normal_map(rng),
            second_spec_map: Blocks::random(rng, TEXELS, true),
            third: Blocks::random(rng, TEXELS, true),
            rim: Blocks::random(rng, TEXELS, true),
            shadow: Blocks::random(rng, SIZE * BLOCK, false),
        }
    }
}

/// Where the shader looks a view-space position up in the shadow map.
fn shadow_uv(position: [f32; 3]) -> [f32; 2] {
    let p = Vec4::new(position[0], position[1], position[2], 1.0);
    let l = shadow_lookup();
    [p.dot(l.u), p.dot(l.v)]
}

/// The shadow map's lookup: each pixel's centre lands on the centre of a
/// block of its own (`SIZE` pixels across `SIZE` blocks).
fn shadow_lookup() -> ShadowLookup {
    let s = 1.0 / SIZE as f32;
    ShadowLookup {
        u: Vec4::new(s, 0.0, 0.0, 0.5),
        v: Vec4::new(0.0, -s, 0.0, 0.5),
    }
}

// ---- one material's quads and what the model makes of them ----

/// One pixel's quad: its diagonal's two vertices carry these, the other
/// two copy the first (they weigh nothing at the pixel's centre).
#[derive(Debug, Clone, Copy)]
struct Quad {
    pixel: [u32; 2],
    depth: f32,
    normal: [[f32; 3]; 2],
    tangent: [[f32; 3]; 2],
    handedness: f32,
    colour: [[u8; 4]; 2],
    /// Before each set's transform.
    uv: [[f32; 2]; 3],
    front: bool,
}

impl Quad {
    /// The corners in view space: x right, y up, one unit a pixel.
    fn corners(&self) -> [[f32; 3]; 4] {
        let x0 = self.pixel[0] as f32 - SIZE as f32 / 2.0;
        let y0 = SIZE as f32 / 2.0 - self.pixel[1] as f32 - 1.0;
        let z = -self.depth;
        [
            [x0, y0, z],
            [x0 + 1.0, y0, z],
            [x0 + 1.0, y0 + 1.0, z],
            [x0, y0 + 1.0, z],
        ]
    }

    /// What the vertex stage interpolates to the pixel's centre: the mean
    /// of the diagonal's vertices as the shader writes them.
    fn fragment(&self, shadow: [f32; 4]) -> Fragment {
        let c = self.corners();
        let n = self.normal.map(|n| normalize(normalize(n)));
        let t = self.tangent.map(|t| normalize(normalize(t)));
        let b = [0, 1].map(|i| cross(n[i], t[i]).map(|v| v * self.handedness));
        let eye = [c[0], c[2]].map(|p| normalize(p.map(|v| -v)));
        let colour = self.colour.map(|c| c.map(|v| f32::from(v) / 255.0));
        Fragment {
            normal: mean(n[0], n[1]),
            tangent: mean(t[0], t[1]),
            bitangent: mean(b[0], b[1]),
            position: mean(c[0], c[2]),
            depth: self.depth,
            eye: mean(eye[0], eye[1]),
            vertex_colour: mean(colour[0], colour[1]),
            uv: self.uv,
            shadow,
        }
    }
}

/// A pixel's expected value: the model's colour and the alpha the
/// pipeline stores, or the clear colour where nothing draws (culled or
/// alpha-tested away).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Want {
    Colour([f32; 4]),
    Clear,
}

#[derive(Clone)]
struct Variant {
    material: shading::Material,
    scene: Scene,
    maps: Maps,
    alpha_reference: f32,
    quads: Vec<Quad>,
    fragments: Vec<Fragment>,
    /// Under the exact decode: what tells the capture loop a pixel drew.
    wants: Vec<Want>,
}

impl Variant {
    fn wants(&self, key: &MaterialKey, decode: &Decode) -> Vec<Want> {
        self.quads
            .iter()
            .zip(&self.fragments)
            .map(|(q, f)| {
                let m = &self.material;
                want(
                    key,
                    m,
                    &self.scene,
                    &self.maps,
                    self.alpha_reference,
                    q,
                    f,
                    decode,
                )
                .0
            })
            .collect()
    }
}

#[derive(Clone)]
struct Case {
    name: String,
    key: MaterialKey,
    variants: Vec<Variant>,
}

/// A material with `key`'s switches, every number random, owning each scene
/// switch so the model resolves exactly `key` under any scene.
fn random_material(rng: &mut Rng, key: &ShaderKey) -> shading::Material {
    let s = key.switches();
    let layer = |rng: &mut Rng| Layer {
        exponent: 2f32.powf(rng.range(-1.6, 4.0)),
        detail_weight: rng.range(0.0, 1.5),
        fixed_alpha: rng.fixed_alpha(),
    };
    let first = layer(rng);
    let second = s.second.then(|| SecondLayer {
        layer: layer(rng),
        weights: rng.weights(),
    });
    let third = s.third.then(|| ThirdLayer {
        strength: rng.range(0.0, 2.0),
        weights: rng.weights(),
    });
    let rim = s.rim.then(|| Rim {
        scale: rng.range(0.5, 1.5),
        strength: rng.range(0.0, 2.0),
        weights: rng.weights(),
        split: if rng.chance(0.3) {
            0.0
        } else {
            rng.range(0.0, 1.0)
        },
    });
    let uv = |rng: &mut Rng| loop {
        let t = UvTransform {
            u: [
                rng.range(-2.0, 2.0),
                rng.range(-2.0, 2.0),
                rng.range(-1.0, 1.0),
            ],
            v: [
                rng.range(-2.0, 2.0),
                rng.range(-2.0, 2.0),
                rng.range(-1.0, 1.0),
            ],
        };
        if (t.u[0] * t.v[1] - t.u[1] * t.v[0]).abs() > 0.3 {
            return t;
        }
    };
    shading::Material {
        normal_map: s.normal_map,
        spec_map: s.spec_map,
        directional: s.directional,
        vertex_colour: s.vertex_colour,
        first,
        second,
        third,
        rim,
        tint: [rng.byte(), rng.byte(), rng.byte(), rng.byte()].map(|c| f32::from(c) / 255.0),
        glow: None,
        uv_transforms: [uv(rng), uv(rng), uv(rng)],
        fixed: Fixed {
            shadows: Some(s.shadows),
            fog: Some(s.fog),
            point_lights: Some(s.point_lights),
        },
    }
}

fn random_scene(rng: &mut Rng) -> Scene {
    let light = |rng: &mut Rng| DirectionalLight {
        travel: rng.direction(),
        ambient: rng.colour(80),
        diffuse: rng.colour(255),
        specular: rng.colour(255),
    };
    let fog_near = rng.range(5.0, 20.0);
    Scene {
        lights: [light(rng), light(rng), light(rng)],
        shadows: true,
        shadow_mode: if rng.chance(0.5) { 1.0 } else { 2.0 },
        fog: true,
        fog_colour: rng.colour(255),
        fog_near,
        fog_far: rng.range(fog_near + 10.0, 60.0),
        point_lights: (0..3)
            .map(|_| PointLight {
                position: [
                    rng.range(-20.0, 20.0),
                    rng.range(-20.0, 20.0),
                    rng.range(-80.0, 0.0),
                ],
                colour: rng.colour(255),
                linear: rng.range(0.0, 5.0),
                quadratic: rng.range(0.0, 20.0),
            })
            .collect(),
        placement_tint: [0, 1, 2, 3].map(|_| f32::from(64 + rng.byte() / 2) / 255.0),
        replace: {
            let c = rng.colour(255);
            let amount = if rng.chance(0.5) {
                0.0
            } else {
                rng.range(0.0, 0.6)
            };
            [c[0], c[1], c[2], amount]
        },
    }
}

/// The model's textures for `maps`, each a lookup into its blocks that
/// the quad was drawn to keep safe.
fn model_textures<'a>(
    maps: &'a Maps,
    decode: &'a Decode,
) -> [Box<dyn Fn([f32; 2]) -> [f32; 4] + 'a>; 8] {
    let lookup = |b: &'a Blocks| -> Box<dyn Fn([f32; 2]) -> [f32; 4] + 'a> {
        Box::new(move |uv| b.sample(uv, decode).unwrap_or([f32::NAN; 4]))
    };
    [
        lookup(&maps.colour),
        lookup(&maps.normal_map),
        lookup(&maps.spec_map),
        lookup(&maps.second_colour),
        lookup(&maps.second_normal_map),
        lookup(&maps.second_spec_map),
        lookup(&maps.third),
        lookup(&maps.rim),
    ]
}

/// The model's value for a quad's pixel under `decode`, drawn over the
/// clear colour as the key's states draw it.
fn want(
    key: &MaterialKey,
    m: &shading::Material,
    scene: &Scene,
    maps: &Maps,
    alpha_reference: f32,
    quad: &Quad,
    fragment: &Fragment,
    decode: &Decode,
) -> (Want, shading::Shaded) {
    // The shadow map's sample at this pixel, as this pass's texels hold it.
    let fragment = &Fragment {
        shadow: maps
            .shadow
            .sample(shadow_uv(fragment.position), decode)
            .unwrap_or([f32::NAN; 4]),
        ..*fragment
    };
    let [c, nm, sm, c2, n2, s2, t3, r] = model_textures(maps, decode);
    let textures = shading::Textures {
        colour: Some(&c),
        normal_map: Some(&nm),
        spec_map: Some(&sm),
        second_colour: Some(&c2),
        second_normal_map: Some(&n2),
        second_spec_map: Some(&s2),
        third: Some(&t3),
        rim: Some(&r),
    };
    let shaded = shading::shade(m, scene, fragment, &textures).expect("the model shades");
    let culled = !quad.front && !key.states.two_sided;
    let discarded = key.states.alpha_test && shaded.alpha <= alpha_reference;
    let want = if culled || discarded {
        Want::Clear
    } else {
        let pixel = shading::draw(
            shading::Pixel {
                colour: [CLEAR[0], CLEAR[1], CLEAR[2]],
                depth: 0.0,
            },
            &shading::Arriving {
                shaded,
                depth: 1.0,
                front_facing: quad.front,
            },
            &model::RenderStates {
                blend: key.states.blend,
                depth_write: key.states.depth_write,
                two_sided: key.states.two_sided,
                alpha_test: None,
            },
        );
        // The stored alpha is the pipeline's: replaced, blended "over", or
        // left as it was by an additive part.
        let alpha = shaded.alpha;
        let stored = match key.states.blend {
            Blend::Opaque => alpha,
            Blend::Alpha => alpha + CLEAR[3] * (1.0 - alpha),
            Blend::Additive => CLEAR[3],
        };
        Want::Colour([pixel.colour[0], pixel.colour[1], pixel.colour[2], stored])
    };
    (want, shaded)
}

/// A quad and what the pixel should hold, or `None` when an input lands
/// where the GPU and the model could part ways for reasons that aren't
/// shading (a filter footprint straddling blocks, an alpha within a hair of
/// its test, a light on the surface).
fn random_quad(
    rng: &mut Rng,
    pixel: [u32; 2],
    key: &MaterialKey,
    m: &shading::Material,
    scene: &Scene,
    maps: &Maps,
    alpha_reference: f32,
) -> Option<(Quad, Fragment)> {
    let exact = Decode::exact();
    let n0 = rng.direction();
    let n1 = normalize(std::array::from_fn(|i| n0[i] + rng.range(-0.6, 0.6)));
    let tangent = |rng: &mut Rng, n: [f32; 3]| {
        let r = rng.direction();
        let d = dot(r, n);
        normalize(std::array::from_fn(|i| {
            r[i] - d * n[i] + rng.range(-0.2, 0.2)
        }))
    };
    let t = [tangent(rng, n0), tangent(rng, n1)];
    // Each set's coordinates: a safe point, sent back through its transform.
    let uv = std::array::from_fn(|set| {
        let target = maps.colour.point(rng);
        let tr = m.uv_transforms[set];
        let (a, b, c, d) = (tr.u[0], tr.u[1], tr.v[0], tr.v[1]);
        let det = a * d - b * c;
        let (x, y) = (target[0] - tr.u[2], target[1] - tr.v[2]);
        [(d * x - b * y) / det, (a * y - c * x) / det]
    });
    let quad = Quad {
        pixel,
        depth: rng.range(1.0, 70.0),
        normal: [n0, n1],
        tangent: t,
        handedness: if rng.chance(0.5) { 1.0 } else { -1.0 },
        colour: [0, 1].map(|_| [rng.byte(), rng.byte(), rng.byte(), rng.byte()]),
        uv,
        front: !rng.chance(0.125),
    };

    // Every lookup the shader makes must land safely.
    let sets = shading::uv_sets(m);
    for set in [Some(sets.first), sets.second, sets.third]
        .into_iter()
        .flatten()
    {
        maps.colour
            .block(m.uv_transforms[set].apply(quad.uv[set]))?;
    }
    let shadow = maps
        .shadow
        .sample(shadow_uv(quad.fragment([0.0; 4]).position), &exact)?;
    let fragment = quad.fragment(shadow);
    if dot(fragment.normal, fragment.normal) < 0.3 * 0.3 {
        return None;
    }
    let on = shading::Switches::resolve(m, scene);
    // A mapped normal, unless the vector it normalizes is near zero.
    // A mapped normal, from the texel exact or rounded to half (the
    // half-float pass's), unless the vector it normalizes is near zero.
    let mapped = |blocks: &Blocks, set: usize, half: bool| -> Option<[f32; 3]> {
        let mut texel = blocks.sample(m.uv_transforms[set].apply(quad.uv[set]), &exact)?;
        if half {
            texel = texel.map(round_half);
        }
        let (x, y) = (2.0 * texel[3] - 1.0, 2.0 * texel[1] - 1.0);
        let z = (1.0 - x * x - y * y).abs().sqrt();
        let raw: [f32; 3] = std::array::from_fn(|i| {
            y * fragment.bitangent[i] - x * fragment.tangent[i] + z * fragment.normal[i]
        });
        (dot(raw, raw) >= 0.3 * 0.3).then(|| shading::mapped_normal(texel, &fragment))
    };
    // Kinks the spec puts in a term (a facing clamped at zero, specular's
    // floor) turn the f32 rounding the two evaluations differ by into a
    // jump in the result; a quad sitting within a hair of one is redrawn.
    let eye = if on.eye_direction() {
        fragment.eye.map(|c| c.max(0.0))
    } else {
        [0.0; 3]
    };
    for half in [false, true] {
        let first_normal = if m.normal_map {
            // The second layer's normal map shares the frame.
            if let (Some(set), Some(_)) = (sets.second, &m.second) {
                mapped(&maps.second_normal_map, set, half)?;
            }
            mapped(&maps.normal_map, sets.first, half)?
        } else {
            fragment.normal
        };
        if let Some(rim) = &m.rim {
            maps.rim.block(shading::rim_uv(first_normal, rim.scale))?;
        }
        for light in scene.point_lights.iter().take(usize::from(on.point_lights)) {
            let to: [f32; 3] = std::array::from_fn(|i| light.position[i] - fragment.position[i]);
            let d = dot(to, to).sqrt();
            // Two units off keeps the falloff (up to 5/d + 20/d^2) from
            // multiplying that rounding by 25.
            if d < 2.0 || (dot(first_normal, to) / d).abs() < 1e-3 {
                return None;
            }
        }
        if m.spec_map {
            let normal = (m.normal_map || on.geometric_normal()).then_some(first_normal);
            for light in &scene.lights {
                let x = dot(eye, shading::reflection(light.travel, normal));
                if x > 0.0 && x < 1e-3 {
                    return None;
                }
            }
        }
    }

    let (_, shaded) = want(
        key,
        m,
        scene,
        maps,
        alpha_reference,
        &quad,
        &fragment,
        &exact,
    );
    let all = [
        shaded.colour[0],
        shaded.colour[1],
        shaded.colour[2],
        shaded.alpha,
    ];
    if all.iter().any(|v| !v.is_finite()) {
        return None;
    }
    // The hardware's texels differ from the exact decode by up to a few
    // thousandths, so a test this close could go either way.
    if key.states.alpha_test && (shaded.alpha - alpha_reference).abs() < 5e-3 {
        return None;
    }
    Some((quad, fragment))
}

fn build_case(rng: &mut Rng, name: String, key: MaterialKey) -> Case {
    let variants = (0..4)
        .map(|v| {
            let material = random_material(rng, &key.shader);
            let scene = random_scene(rng);
            let maps = Maps::random(rng);
            let alpha_reference = rng.range(0.1, 0.9);
            let (ox, oy) = ((v % 2) * REGION, (v / 2) * REGION);
            let mut quads = Vec::new();
            let mut fragments = Vec::new();
            for j in 0..REGION {
                for i in 0..REGION {
                    let pixel = [ox + i, oy + j];
                    let (q, f) = (0..1000)
                        .find_map(|_| {
                            random_quad(rng, pixel, &key, &material, &scene, &maps, alpha_reference)
                        })
                        .expect("a safe quad within 1000 tries");
                    quads.push(q);
                    fragments.push(f);
                }
            }
            let mut v = Variant {
                material,
                scene,
                maps,
                alpha_reference,
                quads,
                fragments,
                wants: Vec::new(),
            };
            v.wants = v.wants(&key, &Decode::exact());
            v
        })
        .collect();
    Case {
        name,
        key,
        variants,
    }
}

/// The joint's pose and bind: the segment's positions sit at
/// `BIND - JOINT` from where they draw, so only the skin (the joint's pose
/// times its inverse joint) puts them on their pixels. Integers, so the
/// round trip is exact in f32.
const JOINT: [f32; 3] = [5.0, -3.0, 2.0];
const BIND: [f32; 3] = [-2.0, 4.0, -1.0];

/// A variant's quads as one IR segment, skinned to joint 0 at full weight.
fn segment(quads: &[Quad], material: u16) -> Segment {
    let mut s = Segment {
        material,
        source: 0,
        positions: Vec::new(),
        normals: Vec::new(),
        tangents: Some(Vec::new()),
        uvs: vec![Vec::new(), Vec::new(), Vec::new()],
        colors: Some(Vec::new()),
        joints: Vec::new(),
        weights: Vec::new(),
        indices: Vec::new(),
    };
    for q in quads {
        let base = s.positions.len() as u16;
        // v0 and v2 are the diagonal; v1 and v3 copy v0.
        for (corner, side) in q.corners().into_iter().zip([0, 0, 1, 0]) {
            s.positions
                .push(std::array::from_fn(|i| corner[i] + BIND[i] - JOINT[i]));
            s.normals.push(q.normal[side]);
            let t = q.tangent[side];
            s.tangents
                .as_mut()
                .unwrap()
                .push([t[0], t[1], t[2], q.handedness]);
            for (set, uv) in s.uvs.iter_mut().enumerate() {
                uv.push(q.uv[set]);
            }
            s.colors.as_mut().unwrap().push(q.colour[side]);
            s.joints.push([0; 4]);
            s.weights.push([255, 0, 0, 0]);
        }
        let tri: [u16; 6] = if q.front {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        s.indices.extend(tri.map(|i| base + i));
    }
    s
}

fn quad_model(case: &Case) -> Model {
    Model {
        identity: guac_ir::DumpIdentity {
            title_id: "LANE00000".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        },
        load_file: case.name.clone(),
        name: case.name.clone(),
        skeleton: Skeleton {
            joints: vec![Joint {
                name: "root".into(),
                parent: None,
                reference: model::Transform {
                    translation: JOINT,
                    ..model::Transform::IDENTITY
                },
                inverse_bind: bevy::math::Mat4::from_translation(-bevy::math::Vec3::from(BIND))
                    .to_cols_array_2d(),
            }],
        },
        materials: Vec::new(),
        segments: case
            .variants
            .iter()
            .enumerate()
            .map(|(i, v)| segment(&v.quads, i as u16))
            .collect(),
        idle: Clip {
            name: "still".into(),
            duration: 1.0,
            frame_duration: 1.0 / 30.0,
            frames: 31,
            tracks: Vec::new(),
        },
    }
}

// ---- the decode probe ----

/// A key-0 material whose output is exactly its colour map's sample: tints
/// multiplying to 1, half grey times two, no other term.
fn probe_case(srgb: bool) -> Case {
    let key = MaterialKey {
        shader: ShaderKey::new(0, 0).unwrap(),
        states: StateKey::OPAQUE,
    };
    // 256 blocks, one per code; the channels run through the codes apart.
    let codes: Vec<[u8; 4]> = (0..=255u8)
        .map(|k| [k, 255 - k, k.wrapping_mul(7), k.wrapping_mul(13)])
        .collect();
    let colour = Blocks {
        size: 16 * BLOCK,
        srgb,
        half: false,
        codes,
    };
    let mut rng = Rng(0xDEC0DE);
    let mut maps = Maps::random(&mut rng);
    maps.colour = colour.clone();
    let mut material = shading::Material::new();
    material.tint = [0.5, 0.5, 0.5, 1.0];
    material.fixed = Fixed {
        shadows: Some(false),
        fog: Some(false),
        point_lights: Some(0),
    };
    let mut scene = Scene::new();
    scene.placement_tint = [0.5, 0.5, 0.5, 1.0];
    let mut quads = Vec::new();
    let mut wants = Vec::new();
    let mut fragments = Vec::new();
    for j in 0..SIZE {
        for i in 0..SIZE {
            let k = (j * SIZE + i) % 256;
            let (bx, by) = ((k % 16) as f32, (k / 16) as f32);
            let uv = [
                (bx * BLOCK as f32 + 2.0) / colour.size as f32,
                (by * BLOCK as f32 + 2.0) / colour.size as f32,
            ];
            let q = Quad {
                pixel: [i, j],
                depth: 10.0,
                normal: [[0.0, 0.0, 1.0]; 2],
                tangent: [[1.0, 0.0, 0.0]; 2],
                handedness: 1.0,
                colour: [[255; 4]; 2],
                uv: [uv; 3],
                front: true,
            };
            fragments.push(q.fragment([0.0; 4]));
            quads.push(q);
            let code = colour.codes[k as usize];
            wants.push(Want::Colour(colour.decoded(code, &Decode::exact())));
        }
    }
    let variant = Variant {
        material,
        scene,
        maps,
        alpha_reference: 0.0,
        quads,
        fragments,
        wants,
    };
    Case {
        name: format!("decode probe ({})", if srgb { "sRGB" } else { "unorm" }),
        key,
        variants: vec![variant],
    }
}

// ---- the Bevy side ----

#[derive(Resource)]
struct Plan {
    cases: Arc<Vec<Case>>,
}

#[derive(Resource, Default)]
struct Targets(Vec<Handle<Image>>);

#[derive(Resource, Clone, Default)]
struct Captured(Arc<Mutex<Vec<Option<Vec<[f32; 4]>>>>>);

#[derive(Resource, Clone, Default)]
struct Outcome(Arc<Mutex<Option<Result<Vec<Vec<[f32; 4]>>, String>>>>);

fn setup(
    mut commands: Commands,
    plan: Res<Plan>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut targets: ResMut<Targets>,
) {
    for (n, case) in plan.cases.iter().enumerate() {
        let layer = RenderLayers::from_layers(&[n + 1]);
        let handles: Vec<Handle<CharacterMaterial>> = case
            .variants
            .iter()
            .map(|v| {
                let mut add = |b: &Blocks| Some(images.add(b.image()));
                let textures = CharacterTextures {
                    colour: add(&v.maps.colour),
                    normal_map: add(&v.maps.normal_map),
                    spec_map: add(&v.maps.spec_map),
                    second_colour: add(&v.maps.second_colour),
                    second_normal_map: add(&v.maps.second_normal_map),
                    second_spec_map: add(&v.maps.second_spec_map),
                    third: add(&v.maps.third),
                    rim: add(&v.maps.rim),
                    shadow: add(&v.maps.shadow),
                };
                let m = CharacterMaterial::new(
                    case.key,
                    &v.material,
                    &v.scene,
                    shadow_lookup(),
                    v.alpha_reference,
                    textures,
                )
                .expect("a material");
                materials.add(m)
            })
            .collect();
        let spawned = spawn_character(
            &mut commands,
            &quad_model(case),
            Transform::IDENTITY,
            0.0,
            &mut meshes,
            &mut bindposes,
            &handles,
        )
        .expect("the quads spawn");
        for e in spawned.segments {
            commands.entity(e).insert(layer.clone());
        }

        let mut target = Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba16Float, None);
        target.asset_usage = RenderAssetUsages::default();
        let handle = images.add(target);
        commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::LinearRgba(LinearRgba::new(
                    CLEAR[0], CLEAR[1], CLEAR[2], CLEAR[3],
                ))),
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: SIZE as f32,
                    height: SIZE as f32,
                },
                near: 0.0,
                far: 1000.0,
                ..OrthographicProjection::default_3d()
            }),
            Transform::IDENTITY,
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            layer,
        ));
        targets.0.push(handle);
    }
}

/// How many frames between captures, and the longest to wait for every
/// pipeline to compile.
const EVERY: u32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(180);
/// Captures in a row that have to agree before a target that has drawn
/// counts as done though pixels the model draws are still clear: each
/// target is one key, so one pipeline, and once it draws anything it has
/// compiled. Those pixels then fail as misses (a shader dropping them, a
/// wrong alpha test or cull) instead of waiting out [`MAX_WAIT`] as if a
/// pipeline were still compiling.
const SETTLED: u32 = 3;

fn capture(
    mut commands: Commands,
    plan: Res<Plan>,
    targets: Res<Targets>,
    captured: Res<Captured>,
    outcome: Res<Outcome>,
    mut frame: Local<u32>,
    mut started: Local<Option<Instant>>,
    mut previous: Local<Option<Vec<Vec<[f32; 4]>>>>,
    mut agreeing: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    let started = *started.get_or_insert_with(Instant::now);
    let done = {
        let c = captured.0.lock().unwrap();
        !c.is_empty() && c.iter().all(Option::is_some)
    };
    if done {
        let shots: Vec<Vec<[f32; 4]>> = std::mem::take(&mut *captured.0.lock().unwrap())
            .into_iter()
            .map(Option::unwrap)
            .collect();
        // Ready once every pixel that should draw has, and two captures in
        // a row agree (no pipeline still compiling).
        let waiting: Vec<(&str, usize)> = plan
            .cases
            .iter()
            .zip(&shots)
            .filter_map(|(case, shot)| {
                let undrawn = case
                    .variants
                    .iter()
                    .flat_map(|v| v.quads.iter().zip(&v.wants))
                    .filter(|(q, w)| match w {
                        Want::Clear => false,
                        Want::Colour(want) => visible(*want) && shot[index(q.pixel)] == CLEAR,
                    })
                    .count();
                (undrawn > 0).then_some((case.name.as_str(), undrawn))
            })
            .collect();
        let stable = previous.as_ref() == Some(&shots);
        *agreeing = if stable { *agreeing + 1 } else { 0 };
        let every_target_drew = shots.iter().all(|s| s.iter().any(|p| *p != CLEAR));
        let settled = every_target_drew && *agreeing >= SETTLED;
        if !waiting.is_empty() && *frame > 10 * EVERY && !settled {
            eprintln!(
                "frame {}: {} of {} targets still undrawn {:?}",
                *frame,
                waiting.len(),
                shots.len(),
                &waiting[..waiting.len().min(3)]
            );
        }
        if (waiting.is_empty() && stable) || settled {
            if !waiting.is_empty() {
                eprintln!(
                    "frame {}: every target drew and {} captures agree, but {} still leave pixels the model draws clear, compared as misses: {:?}",
                    *frame,
                    *agreeing + 1,
                    waiting.len(),
                    &waiting[..waiting.len().min(3)]
                );
            }
            *outcome.0.lock().unwrap() = Some(Ok(shots));
            exit.write(AppExit::Success);
            return;
        }
        *previous = Some(shots);
    }
    if started.elapsed() > MAX_WAIT {
        *outcome.0.lock().unwrap() = Some(Err(format!(
            "pipelines not ready after {:?} ({} frames)",
            MAX_WAIT, *frame
        )));
        exit.write(AppExit::error());
        return;
    }
    if (*frame).is_multiple_of(EVERY) && captured.0.lock().unwrap().is_empty() {
        *captured.0.lock().unwrap() = vec![None; targets.0.len()];
        for (i, handle) in targets.0.iter().enumerate() {
            let store = captured.0.clone();
            commands.spawn(Screenshot::image(handle.clone())).observe(
                move |c: On<ScreenshotCaptured>| {
                    let data = c.image.data.as_ref().expect("pixels");
                    let px: Vec<[f32; 4]> = data
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|p| {
                            std::array::from_fn(|ch| {
                                f16_to_f32(u16::from_le_bytes([p[2 * ch], p[2 * ch + 1]]))
                            })
                        })
                        .collect();
                    store.lock().unwrap()[i] = Some(px);
                },
            );
        }
    }
}

/// Whether a drawn pixel shows: a blended part at an alpha near zero
/// stores the clear colour itself.
fn visible(want: [f32; 4]) -> bool {
    (0..4).any(|ch| (want[ch] - CLEAR[ch]).abs() > 4.0 * half_step(CLEAR[ch]))
}

fn index(pixel: [u32; 2]) -> usize {
    (pixel[1] * SIZE + pixel[0]) as usize
}

/// Renders every case and returns each target's pixels.
fn render(cases: Arc<Vec<Case>>) -> Vec<Vec<[f32; 4]>> {
    let outcome = Outcome::default();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(LogPlugin {
                filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_shader=info".into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
        ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)),
        CharacterShadingPlugin,
    ))
    .insert_resource(Plan { cases })
    .insert_resource(Targets::default())
    .insert_resource(Captured::default())
    .insert_resource(outcome.clone())
    .add_systems(Startup, setup)
    .add_systems(Update, capture);
    app.run();
    outcome
        .0
        .lock()
        .unwrap()
        .take()
        .expect("an outcome")
        .expect("every pipeline drew")
}

// ---- comparing ----

#[derive(Debug, Clone)]
struct Worst {
    steps: f32,
    pixel: [u32; 2],
    channel: usize,
    want: f32,
    got: f32,
}

struct Compared {
    pixels: usize,
    cleared: usize,
    worst: Option<Worst>,
    /// Channels past `bound` half-precision steps of the model's value as
    /// the target stores it.
    misses: Vec<Worst>,
    /// The distance from the model's own value, in half steps: the worst,
    /// and how many channels sit past one step.
    raw: f32,
    raw_past_one: usize,
    /// Channels stored further from zero than the model's value.
    away_from_zero: usize,
    /// Channels under [`FLOOR`] past their own half step, and the worst
    /// absolute distance among them.
    floored: usize,
    floored_worst: f32,
}

/// Below this the bound is half's step here, 2^-20: a value under a
/// thousandth is mostly terms of order one cancelling, where f32 rounding
/// of the terms (which the two evaluations do in different orders and
/// Metal with its own pow, exp2 and rsqrt) outruns half's grid.
const FLOOR: f32 = 1.0 / 1024.0;

/// The half toward zero from `v`: how the M3 Max stores a half-float
/// target (no channel of the half-float pass lands further from zero than
/// the model's value, where rounding to nearest would put about half of
/// them there). A GPU value within one step of the model's then stores
/// within one step of this.
fn truncate_half(v: f32) -> f32 {
    let step = half_step(v);
    (v / step).trunc() * step
}

fn compare(case: &Case, shot: &[[f32; 4]], bound: f32, decode: &Decode) -> Compared {
    let mut out = Compared {
        pixels: 0,
        cleared: 0,
        worst: None,
        misses: Vec::new(),
        raw: 0.0,
        raw_past_one: 0,
        away_from_zero: 0,
        floored: 0,
        floored_worst: 0.0,
    };
    for v in &case.variants {
        for (q, w) in v.quads.iter().zip(&v.wants(&case.key, decode)) {
            let got = shot[index(q.pixel)];
            out.pixels += 1;
            let want = match w {
                Want::Clear => {
                    out.cleared += 1;
                    CLEAR
                }
                Want::Colour(c) => *c,
            };
            for ch in 0..4 {
                let step = half_step(want[ch].abs().max(FLOOR));
                let raw = (got[ch] - want[ch]).abs() / half_step(want[ch]);
                out.raw = out.raw.max(raw);
                if raw > 1.0 {
                    out.raw_past_one += 1;
                }
                if got[ch].abs() > want[ch].abs() {
                    out.away_from_zero += 1;
                }
                let steps = (got[ch] - truncate_half(want[ch])).abs() / step;
                if want[ch].abs() < FLOOR
                    && (got[ch] - truncate_half(want[ch])).abs() > half_step(want[ch])
                {
                    out.floored += 1;
                    out.floored_worst = out.floored_worst.max((got[ch] - want[ch]).abs());
                }
                let this = Worst {
                    steps: if steps.is_nan() { f32::INFINITY } else { steps },
                    pixel: q.pixel,
                    channel: ch,
                    want: want[ch],
                    got: got[ch],
                };
                if this.steps > bound {
                    out.misses.push(this.clone());
                }
                if out.worst.as_ref().is_none_or(|w| this.steps > w.steps) {
                    out.worst = Some(this);
                }
            }
        }
    }
    out
}

/// Everything the model saw at one pixel, for a miss.
fn debug_pixel(case: &Case, pixel: [u32; 2], shot: &[[f32; 4]], decode: &Decode) {
    for v in &case.variants {
        let Some(i) = v.quads.iter().position(|q| q.pixel == pixel) else {
            continue;
        };
        let (q, f, m) = (&v.quads[i], &v.fragments[i], &v.material);
        println!("    pixel {pixel:?}: got {:?}", shot[index(pixel)]);
        println!("      quad {q:?}");
        println!("      fragment {f:?}");
        println!("      material {m:?}");
        println!("      scene lights {:?}", v.scene.lights);
        let sets = shading::uv_sets(m);
        for set in [Some(sets.first), sets.second, sets.third]
            .into_iter()
            .flatten()
        {
            let uv = m.uv_transforms[set].apply(q.uv[set]);
            println!(
                "      set {set}: uv {uv:?} colour block {:?}",
                v.maps.colour.block(uv).map(|b| v.maps.colour.codes[b])
            );
            println!(
                "        normal block {:?} spec block {:?}",
                v.maps
                    .normal_map
                    .block(uv)
                    .map(|b| v.maps.normal_map.codes[b]),
                v.maps.spec_map.block(uv).map(|b| v.maps.spec_map.codes[b])
            );
        }
        for (name, d) in [("exact", &Decode::exact()), ("given", decode)] {
            let (w, sh) = want(&case.key, m, &v.scene, &v.maps, v.alpha_reference, q, f, d);
            println!("      model ({name}): {w:?} shaded {sh:?}");
            let uv = m.uv_transforms[0].apply(q.uv[0]);
            println!(
                "        colour {:?} normal {:?} spec {:?}",
                v.maps.colour.sample(uv, d),
                v.maps.normal_map.sample(uv, d),
                v.maps.spec_map.sample(uv, d)
            );
        }
    }
}

fn describe(key: &MaterialKey) -> String {
    let s = key.states;
    format!(
        "ps {:#06x} vs {:#06x} {:?}{}{}{}",
        key.shader.pixel(),
        key.shader.vertex(),
        s.blend,
        if s.depth_write { "" } else { " no-depth-write" },
        if s.two_sided { " two-sided" } else { "" },
        if s.alpha_test { " alpha-test" } else { "" },
    )
}

/// Runs the lane over `keys` in three passes on one render:
/// 1. half-float textures holding the exact decodes, so the shader and the
///    model read the same texels: every channel within `bound` half steps;
/// 2. the decode probes: what the sampler hands back for each 8-bit code;
/// 3. the same inputs on 8-bit textures through the remaster's sRGB views,
///    against the model on exact decodes: the end-to-end distance, which
///    the decode accounts for, recorded.
fn lane(label: &str, keys: Vec<MaterialKey>, seed: u64, bound: f32) {
    let start = Instant::now();
    let mut rng = Rng(seed);
    let mut shading: Option<Summary> = None;
    let mut eight_bit: Option<EndToEnd> = None;
    // A few dozen keys an app: hundreds of cameras at once lose the device.
    for chunk in keys.chunks(BATCH) {
        let eight: Vec<Case> = chunk
            .iter()
            .map(|k| build_case(&mut rng, describe(k), *k))
            .collect();
        let halved: Vec<Case> = eight
            .iter()
            .map(|c| Case {
                variants: c
                    .variants
                    .iter()
                    .map(|v| Variant {
                        maps: v.maps.halved(),
                        ..v.clone()
                    })
                    .collect(),
                ..c.clone()
            })
            .collect();
        let mut cases = vec![probe_case(true), probe_case(false)];
        cases.extend(halved);
        cases.extend(eight);
        let cases = Arc::new(cases);
        let shots = render(cases.clone());
        let n = chunk.len();
        let pass = |from: usize| -> Vec<(&Case, &Vec<[f32; 4]>)> {
            cases[from..from + n]
                .iter()
                .zip(&shots[from..from + n])
                .collect()
        };
        let s = report(label, &pass(PROBES), bound, "half-float texels");
        shading = Some(match shading {
            None => s,
            Some(t) => t.merge(s),
        });
        probed(&cases, &shots);
        let e = end_to_end(label, &pass(PROBES + n));
        eight_bit = Some(match eight_bit {
            None => e,
            Some(t) => t.merge(e),
        });
    }
    let shading = shading.expect("keys");
    println!(
        "{label}: {} keys in {:.1} s",
        keys.len(),
        start.elapsed().as_secs_f64()
    );
    println!(
        "{label}, the model on half-float texels: {} channels, worst {:.3} half-precision steps from the model's value as stored (truncated), on {} (ch {} at {:?}: want {:.6e}, got {:.6e}); {} past {bound}; {} under 2^-10 lean on its step, at most {:.3e} off; from the model's own value the worst is {:.3} steps and {} channels sit past one; {} stored further from zero than the model",
        shading.channels,
        shading.worst.1.steps,
        shading.worst.0,
        shading.worst.1.channel,
        shading.worst.1.pixel,
        shading.worst.1.want,
        shading.worst.1.got,
        shading.misses,
        shading.floored,
        shading.floored_worst,
        shading.raw,
        shading.raw_past,
        shading.away
    );
    println!("{label}: {}", eight_bit.expect("keys"));
    assert_eq!(
        shading.misses, 0,
        "{label}: {} channels past {bound} steps",
        shading.misses
    );
}

/// Keys an app renders at once.
const BATCH: usize = 40;

/// The half-float pass over some keys.
struct Summary {
    channels: usize,
    worst: (String, Worst),
    misses: usize,
    raw: f32,
    raw_past: usize,
    away: usize,
    floored: usize,
    floored_worst: f32,
}

impl Summary {
    fn merge(self, o: Self) -> Self {
        Self {
            channels: self.channels + o.channels,
            worst: if o.worst.1.steps > self.worst.1.steps {
                o.worst
            } else {
                self.worst
            },
            misses: self.misses + o.misses,
            raw: self.raw.max(o.raw),
            raw_past: self.raw_past + o.raw_past,
            away: self.away + o.away,
            floored: self.floored + o.floored,
            floored_worst: self.floored_worst.max(o.floored_worst),
        }
    }
}

/// How far the 8-bit pass sits from the model on exact decodes.
struct EndToEnd {
    worst_steps: (f32, String, f32, f32),
    worst_absolute: (f32, String, f32),
    /// Over channels at or above 2^-8, where the decode's absolute grid
    /// near black no longer dominates.
    worst_relative: (f32, String, f32),
    channels: usize,
    past_one_step: usize,
}

impl EndToEnd {
    fn merge(self, o: Self) -> Self {
        Self {
            worst_steps: if o.worst_steps.0 > self.worst_steps.0 {
                o.worst_steps
            } else {
                self.worst_steps
            },
            worst_absolute: if o.worst_absolute.0 > self.worst_absolute.0 {
                o.worst_absolute
            } else {
                self.worst_absolute
            },
            worst_relative: if o.worst_relative.0 > self.worst_relative.0 {
                o.worst_relative
            } else {
                self.worst_relative
            },
            channels: self.channels + o.channels,
            past_one_step: self.past_one_step + o.past_one_step,
        }
    }
}

impl std::fmt::Display for EndToEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "8-bit textures against exact decodes, {} channels: {} past one half step; worst {:.3} steps ({}: want {:.6e}, got {:.6e}); worst absolute {:.3e} ({}, at {:.6e}); worst relative at 2^-8 and up {:.3}% ({}, at {:.6e})",
            self.channels,
            self.past_one_step,
            self.worst_steps.0,
            self.worst_steps.1,
            self.worst_steps.2,
            self.worst_steps.3,
            self.worst_absolute.0,
            self.worst_absolute.1,
            self.worst_absolute.2,
            100.0 * self.worst_relative.0,
            self.worst_relative.1,
            self.worst_relative.2,
        )
    }
}

fn end_to_end(label: &str, pass: &[(&Case, &Vec<[f32; 4]>)]) -> EndToEnd {
    let exact = Decode::exact();
    let mut out = EndToEnd {
        worst_steps: (0.0, String::new(), 0.0, 0.0),
        worst_absolute: (0.0, String::new(), 0.0),
        worst_relative: (0.0, String::new(), 0.0),
        channels: 0,
        past_one_step: 0,
    };
    println!("{label}, 8-bit textures against exact decodes:");
    for (case, shot) in pass {
        let mut key_worst = 0.0f32;
        for v in &case.variants {
            for (q, w) in v.quads.iter().zip(&v.wants(&case.key, &exact)) {
                let got = shot[index(q.pixel)];
                let want = match w {
                    Want::Clear => CLEAR,
                    Want::Colour(c) => *c,
                };
                for ch in 0..4 {
                    let (g, w) = (got[ch], want[ch]);
                    let e = (g - w).abs();
                    let at = format!("{} ch {ch} at {:?}", case.name, q.pixel);
                    let steps = e / half_step(w);
                    out.channels += 1;
                    if steps > 1.0 {
                        out.past_one_step += 1;
                    }
                    key_worst = key_worst.max(steps);
                    if steps > out.worst_steps.0 {
                        out.worst_steps = (steps, at.clone(), w, g);
                    }
                    if e > out.worst_absolute.0 {
                        out.worst_absolute = (e, at.clone(), w);
                    }
                    if w.abs() >= 2f32.powi(-8) && e / w.abs() > out.worst_relative.0 {
                        out.worst_relative = (e / w.abs(), at, w);
                    }
                }
            }
        }
        println!("  {}: worst {:.3} steps", case.name, key_worst);
    }
    out
}

/// The hardware's decode of every code, off the two probes: each pixel's
/// output is its texel as sampled ([`probe_case`]), exact in half. Every
/// pixel reading a code in any channel has to agree; how far each table
/// sits from the exact decode is printed.
fn probed(cases: &[Case], shots: &[Vec<[f32; 4]>]) -> Decode {
    fn put(table: &mut [f32; 256], code: u8, v: f32, disagree: &mut usize) {
        let slot = &mut table[usize::from(code)];
        if slot.is_nan() {
            *slot = v;
        } else if *slot != v {
            *disagree += 1;
        }
    }
    let mut d = Decode {
        srgb_rgb: [f32::NAN; 256],
        srgb_alpha: [f32::NAN; 256],
        unorm: [f32::NAN; 256],
    };
    let mut disagree = 0;
    for (probe, (case, shot)) in cases.iter().zip(shots).take(PROBES).enumerate() {
        let v = &case.variants[0];
        for q in &v.quads {
            let got = shot[index(q.pixel)];
            let code = v.maps.colour.codes[((q.pixel[1] * SIZE + q.pixel[0]) % 256) as usize];
            for ch in 0..4 {
                let table = match (probe, ch) {
                    (0, 3) => &mut d.srgb_alpha,
                    (0, _) => &mut d.srgb_rgb,
                    _ => &mut d.unorm,
                };
                put(table, code[ch], got[ch], &mut disagree);
            }
        }
    }
    assert_eq!(disagree, 0, "a code sampled to two values");
    let exact = Decode::exact();
    for (name, table, truth) in [
        ("sRGB colour", &d.srgb_rgb, &exact.srgb_rgb),
        ("sRGB alpha", &d.srgb_alpha, &exact.srgb_alpha),
        ("unorm", &d.unorm, &exact.unorm),
    ] {
        assert!(
            table.iter().all(|v| v.is_finite()),
            "{name}: a code never sampled"
        );
        let mut worst_abs = (0.0f32, 0usize);
        let mut worst_steps = (0.0f32, 0usize);
        let mut worst_rel = (0.0f32, 0usize);
        let mut off = 0;
        for k in 0..256 {
            let (got, want) = (table[k], truth[k]);
            let e = (got - want).abs();
            if got != round_half(want) {
                off += 1;
            }
            if e > worst_abs.0 {
                worst_abs = (e, k);
            }
            let steps = e / half_step(want);
            if steps > worst_steps.0 {
                worst_steps = (steps, k);
            }
            if want > 0.0 && e / want > worst_rel.0 {
                worst_rel = (e / want, k);
            }
        }
        println!(
            "  decode, {name}: {off} of 256 codes off the exact value rounded to half; worst {:.3e} absolute (code {}), {:.3} half steps (code {}), {:.2}% (code {})",
            worst_abs.0,
            worst_abs.1,
            worst_steps.0,
            worst_steps.1,
            100.0 * worst_rel.0,
            worst_rel.1
        );
        if std::env::var_os("GUAC_LANE_PROBE").is_some() {
            for k in 0..256 {
                println!(
                    "    {name} {k}: exact {:.9e} sampled {:.9e}",
                    truth[k], table[k]
                );
            }
        }
        // The decode's envelope on the M3 Max (AE.8): sRGB colour measured
        // up to 1.44 x max(2^-13, one half step) off the exact decode (on a
        // 2^-12 grid near black) and asserted within 1.5x, linear 8-bit
        // truncated to the half below it.
        for k in 0..256 {
            let (got, want) = (table[k], truth[k]);
            let e = (got - want).abs();
            if name == "sRGB colour" {
                let envelope = 1.5 * half_step(want).max(2f32.powi(-13));
                assert!(e <= envelope, "{name} {k}: {got} for {want}");
            } else {
                assert!(
                    got <= want && e <= half_step(want),
                    "{name} {k}: {got} for {want}"
                );
            }
        }
    }
    d
}

/// Compares every key's target with the model under `decode` and prints
/// each key's worst channel; the overall worst and the channels past
/// `bound` come back.
fn report(label: &str, pass: &[(&Case, &Vec<[f32; 4]>)], bound: f32, what: &str) -> Summary {
    println!("{label}, the model on {what}:");
    let decode = &Decode::exact();
    let mut overall: Option<(String, Worst)> = None;
    let mut misses = 0;
    let (mut channels, mut raw, mut raw_past, mut away) = (0, 0.0f32, 0, 0);
    let (mut floored, mut floored_worst) = (0, 0.0f32);
    for (case, shot) in pass {
        let c = compare(case, shot, bound, decode);
        channels += 4 * c.pixels;
        raw = raw.max(c.raw);
        raw_past += c.raw_past_one;
        away += c.away_from_zero;
        floored += c.floored;
        floored_worst = floored_worst.max(c.floored_worst);
        let w = c.worst.clone().expect("pixels");
        println!(
            "  {}: {} px ({} clear), worst {:.3} steps (ch {} at {:?}: want {:.6e}, got {:.6e}){}",
            case.name,
            c.pixels,
            c.cleared,
            w.steps,
            w.channel,
            w.pixel,
            w.want,
            w.got,
            if c.misses.is_empty() {
                String::new()
            } else {
                format!(", {} channels past {bound}", c.misses.len())
            }
        );
        for m in c.misses.iter().take(3) {
            println!(
                "    miss: ch {} at {:?}: want {:.6e}, got {:.6e} ({:.3} steps)",
                m.channel, m.pixel, m.want, m.got, m.steps
            );
        }
        if let Some(filter) = std::env::var_os("GUAC_LANE_DEBUG")
            && case.name.contains(filter.to_str().unwrap_or(""))
        {
            let mut misses = c.misses.clone();
            misses.sort_by(|a, b| b.steps.total_cmp(&a.steps));
            for m in misses.iter().take(4) {
                debug_pixel(case, m.pixel, shot, decode);
            }
        }
        misses += c.misses.len();
        if overall.as_ref().is_none_or(|(_, o)| w.steps > o.steps) {
            overall = Some((case.name.clone(), w));
        }
    }
    Summary {
        channels,
        worst: overall.expect("cases"),
        misses,
        raw,
        raw_past,
        away,
        floored,
        floored_worst,
    }
}

#[test]
#[ignore = "GPU lane: run on the Mac (AE.8)"]
fn the_turntables_keys_match_the_reference_model() {
    let keys = drawn(&[SceneBits::TURNTABLE]);
    assert_eq!(
        keys.len(),
        33,
        "AE.5 counted 33 keys as the turntable draws them"
    );
    lane("turntable", keys, 0xAE08_0001, 1.0);
}

#[test]
#[ignore = "GPU lane: run on the Mac (AE.8)"]
fn every_scene_and_layer_matches_the_reference_model() {
    let mut keys = drawn(&every_scene());
    keys.extend(layer_keys());
    lane("every scene", keys, 0xAE08_0002, 1.0);
}

#[test]
fn the_census_draws_33_keys_on_the_turntable() {
    // No GPU: the key table itself, so a typo in it fails in `cargo test`.
    assert_eq!(drawn(&[SceneBits::TURNTABLE]).len(), 33);
    let pixels: BTreeSet<u32> = drawn(&[SceneBits::TURNTABLE])
        .iter()
        .map(|k| k.shader.pixel())
        .collect();
    assert_eq!(
        pixels,
        BTreeSet::from([
            0x400, 0x404, 0x418, 0x41C, 0x420, 0x424, 0x428, 0x42C, 0x438, 0x43C
        ])
    );
}

#[test]
fn half_steps_and_rounding() {
    assert_eq!(half_step(1.0), 2f32.powi(-10));
    assert_eq!(half_step(1.5), 2f32.powi(-10));
    assert_eq!(half_step(0.75), 2f32.powi(-11));
    assert_eq!(half_step(0.0), 2f32.powi(-24));
    assert_eq!(f16_to_f32(0x3C00), 1.0);
    assert_eq!(f16_to_f32(0xC000), -2.0);
    assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
    assert_eq!(round_half(1.0 + 2f32.powi(-11)), 1.0);
    assert_eq!(
        round_half(1.0 + 3.0 * 2f32.powi(-11)),
        1.0 + 2.0 * 2f32.powi(-10)
    );
    assert_eq!(srgb(255), 1.0);
    assert_eq!(srgb(0), 0.0);
    assert_eq!(
        truncate_half(1.0 + 1.9 * 2f32.powi(-10)),
        1.0 + 2f32.powi(-10)
    );
    assert_eq!(
        truncate_half(-(1.0 + 1.9 * 2f32.powi(-10))),
        -(1.0 + 2f32.powi(-10))
    );
    assert_eq!(truncate_half(0.25), 0.25);
    for v in [0.0, 0.25, 0.5, 1.0, 0.123, 3.75, 1e-6] {
        assert_eq!(f16_to_f32(f32_to_f16(v)), round_half(v), "{v}");
    }
}
