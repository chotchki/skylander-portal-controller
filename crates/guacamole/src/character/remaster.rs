//! The remaster (openspec AE.12, design decision 12; chotchki, 2026-10-18:
//! "let's do the modern bevy look, I'll decide later if there are lighting
//! issues with it"): the character-shading spec worked in linear light,
//! drawn at high dynamic range and tonemapped with TonyMcMapface. The one
//! place that says what the remaster decodes and how its camera draws, so
//! `hero_view`, the GPU lanes and the turntable (AE.13) can't drift apart.
//!
//! What's decoded and what isn't (the remaster-look spec's decoding
//! requirement):
//! - colours the disc stores encoded for display: the lights' ambient,
//!   diffuse and specular, the fog's and the replace colour, each held as
//!   `Srgba::rgb_u8` and converted with `LinearRgba::from` ([`linear`],
//!   [`light`], [`replace`]); the colour, spec and rim maps and the other
//!   layers' colour maps, sampled through `*UnormSrgb` views so the hardware
//!   decodes before it filters ([`is_srgb`], [`texture`]);
//! - as stored: normal maps (`Unorm`), a level's gloss map, which is a
//!   weight ([`samples_srgb`], AG.22), and tints and vertex colours, which
//!   are multipliers centred on half grey (×4·tint·material tint,
//!   ×2·vertex colour), not colours ([`tint`]): decoded, a neutral tint's
//!   0.216 would scale every part by 4 · 0.216² = 0.186 instead of 1.0079.
//!   Weights, exponents, alpha references and fog distances are numbers.
//! - a level's rim that multiplies (AG.79): its map decoded as the other
//!   colour maps are, and its strength raised to 2.2 ([`in_light`]), so
//!   colour × map × strength on stored values comes out as the decoded
//!   product: a grey of one over the strength stays neutral and the map's
//!   highlights keep their reach (the coins' bronze, 2 · 0.82 stored at its
//!   middle, multiplies by 2.9 in linear light, not 1.6 as stored or 1.3 as
//!   decoded at strength 2). A character's rim keeps its strength (AG.95).
//!
//! The camera ([`camera`]) is HDR (an `Rgba16Float` main texture) with 4x
//! MSAA and TonyMcMapface into an 8-bit sRGB image ([`target`]). No
//! exposure lift: the material reads neither Bevy's `Exposure` nor a scale
//! of its own, so its linear result reaches the tonemapper at scale 1,
//! which AE.12's GPU lane checks with tonemapping off. Whether that reads
//! too dark is chotchki's call at AE.18; a lift would be one constant. The
//! game's glow ([`super::glow`]) runs on the linear frame before the
//! tonemapper. Debanding dither is off, so a frame repeats byte for byte and
//! the measured look is the one drawn.

use bevy::asset::RenderAssetUsages;
use bevy::camera::{Camera3d, Hdr};
use bevy::color::{LinearRgba, Srgba};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{
    CompressedImageFormats, Image, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor, ImageType, TextureError,
};
use bevy::math::UVec2;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::Msaa;
use guac_ir::model::{Filter, Material, Sampler, TextureRole, Wrap};
use guac_ir::shading::DirectionalLight;

use super::key::bit;

/// Open question 1's (d): the look ae-color measured at chroma 0.0963 and
/// lightness 0.522 on Eruptor, which AE.12 re-measures.
pub const TONEMAPPING: Tonemapping = Tonemapping::TonyMcMapface;
/// What the camera draws into: 8-bit sRGB, which a card shows as it is.
pub const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
pub const MSAA: Msaa = Msaa::Sample4;

/// A colour the disc stores encoded for display, in linear light.
pub fn linear(code: [u8; 3]) -> [f32; 3] {
    let l = LinearRgba::from(Srgba::rgb_u8(code[0], code[1], code[2]));
    [l.red, l.green, l.blue]
}

/// A directional light from the codes the disc stores: its three colours
/// decoded, its way of travel (unit length, the space the shader lights
/// in) as given.
pub fn light(
    travel: [f32; 3],
    ambient: [u8; 3],
    diffuse: [u8; 3],
    specular: [u8; 3],
) -> DirectionalLight {
    DirectionalLight {
        travel,
        ambient: linear(ambient),
        diffuse: linear(diffuse),
        specular: linear(specular),
    }
}

/// The replace colour decoded, its amount (a number) as given: the scene's
/// `replace`.
pub fn replace(code: [u8; 3], amount: f32) -> [f32; 4] {
    let [r, g, b] = linear(code);
    [r, g, b, amount]
}

/// A placement or material tint as stored: a multiplier of 255, neutral at
/// 128, never decoded.
pub fn tint(code: [u8; 4]) -> [f32; 4] {
    code.map(|c| f32::from(c) / 255.0)
}

/// A placement tint the scripts move, as the shader takes it in linear
/// light (AH.67): each colour channel's ratio to neutral raised to 2.2,
/// since the original multiplies stored values by it, alpha as stored. At
/// neutral it's [`tint`]'s to the bit; the eruption's going-black (34, 20,
/// 20) scales a part by 0.055, 0.016 and 0.016, where as stored it scaled
/// by 0.27, 0.16 and 0.16.
pub fn tint_in_light(code: [u8; 4]) -> [f32; 4] {
    let neutral = 128.0 / 255.0;
    let [r, g, b, a] = tint(code);
    let in_light = |c: f32| neutral * (c / neutral).powf(2.2);
    [in_light(r), in_light(g), in_light(b), a]
}

/// Whether a role's texture samples through an sRGB view: every colour the
/// game paints (colour, spec and rim maps, the layers' colour maps), not
/// the normal maps.
pub fn is_srgb(role: TextureRole) -> bool {
    role.is_color()
}

/// Whether `material` samples its texture in `role` through an sRGB view:
/// [`is_srgb`]'s answer, except that a gloss map (the third layer's texture
/// on a material with the gloss bit, a level's) is a weight the shader
/// reads as stored (character-shading's gloss map).
pub fn samples_srgb(material: &Material, role: TextureRole) -> bool {
    let gloss = material.pixel.base >> bit::GLOSS_MAP & 1 == 1;
    is_srgb(role) && !(gloss && role == TextureRole::Layer3ColorMap)
}

/// What a strength the original multiplies stored colours by multiplies
/// decoded ones by: its 2.2nd power, since (s · c)^2.2 = s^2.2 · c^2.2. A
/// level's multiplying rim takes it (AG.79, [`super::CharacterUniform::multiplies_in_light`]).
pub fn in_light(strength: f32) -> f32 {
    strength.max(0.0).powf(2.2)
}

/// An IR sampler as Bevy's; one without a mipmap filter samples level 0
/// alone, as the original does.
pub fn sampler(s: &Sampler) -> ImageSamplerDescriptor {
    let wrap = |w: Wrap| match w {
        Wrap::Clamp => ImageAddressMode::ClampToEdge,
        Wrap::Repeat => ImageAddressMode::Repeat,
    };
    let filter = |f: Filter| match f {
        Filter::Nearest => ImageFilterMode::Nearest,
        Filter::Linear => ImageFilterMode::Linear,
    };
    ImageSamplerDescriptor {
        address_mode_u: wrap(s.wrap_u),
        address_mode_v: wrap(s.wrap_v),
        mag_filter: filter(s.mag),
        min_filter: filter(s.min),
        mipmap_filter: s.mip.map_or(ImageFilterMode::Nearest, filter),
        lod_max_clamp: if s.mip.is_some() { 32.0 } else { 0.0 },
        ..ImageSamplerDescriptor::default()
    }
}

/// An extract's KTX2 as the role samples it: through an sRGB view when
/// [`is_srgb`], with the IR's sampler, its blocks kept compressed when
/// `formats` takes them.
pub fn texture(
    ktx2: &[u8],
    role: TextureRole,
    s: &Sampler,
    formats: CompressedImageFormats,
) -> Result<Image, TextureError> {
    texture_view(ktx2, is_srgb(role), s, formats)
}

/// An extract's KTX2 through an sRGB view or not, as [`texture`] loads it
/// for a role; a level's loader picks the view by [`samples_srgb`].
pub fn texture_view(
    ktx2: &[u8],
    srgb: bool,
    s: &Sampler,
    formats: CompressedImageFormats,
) -> Result<Image, TextureError> {
    Image::from_buffer(
        ktx2,
        ImageType::Extension("ktx2"),
        formats,
        srgb,
        ImageSampler::Descriptor(sampler(s)),
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The remaster's camera, before its place, projection and target: HDR,
/// 4x MSAA, TonyMcMapface, no dither.
pub fn camera() -> (Camera3d, Hdr, Tonemapping, Msaa, DebandDither) {
    (
        Camera3d::default(),
        Hdr,
        TONEMAPPING,
        MSAA,
        DebandDither::Disabled,
    )
}

/// An image for [`camera`] to draw into, `size` texels, readable by the UI
/// and by a screenshot.
pub fn target(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TARGET_FORMAT, None);
    image.asset_usage = RenderAssetUsages::default();
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::material::{CharacterUniform, ShadowLookup, reference_material};
    use bevy::camera::Exposure;
    use bevy::ecs::world::World;
    use bevy::render::render_resource::TextureUsages;
    use guac_formats::texture::{Format, Texture};
    use guac_ir::model::{self, Blend, Mask, Params, RenderStates};
    use guac_ir::shading::{self, Flat, Fragment, Scene, Textures};

    fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < tolerance)
    }

    #[test]
    fn the_discs_colours_reach_the_uniform_in_linear_light() {
        // The tutorial's key light: (172, 134, 18) lights at its linear
        // value, not at 172/255, 134/255 and 18/255.
        let key = linear([172, 134, 18]);
        assert!(close(key, [0.4125, 0.2384, 0.0060], 5e-5), "{key:?}");
        assert_eq!(linear([255, 255, 255]), [1.0; 3]);
        assert_eq!(linear([0, 0, 0]), [0.0; 3]);
        let want = LinearRgba::from(Srgba::rgb_u8(172, 134, 18));
        assert_eq!(key, [want.red, want.green, want.blue]);

        let mut scene = Scene::new();
        scene.lights[0] = light(
            [0.0, 0.0, -1.0],
            [39, 42, 67],
            [172, 134, 18],
            [75, 85, 107],
        );
        scene.fog = true;
        scene.fog_colour = linear([115, 83, 49]);
        scene.replace = replace([255, 128, 0], 0.25);
        let material = reference_material(&material([128, 128, 128, 255])).unwrap();
        let u = CharacterUniform::new(&material, &scene, ShadowLookup::NONE, 0.0);
        let rgb = |v: bevy::math::Vec4| [v.x, v.y, v.z];
        assert!(close(
            rgb(u.light_diffuse[0]),
            [0.4125, 0.2384, 0.0060],
            5e-5
        ));
        assert_eq!(rgb(u.light_ambient[0]), linear([39, 42, 67]));
        assert_eq!(rgb(u.light_specular[0]), linear([75, 85, 107]));
        assert_eq!(rgb(u.fog_colour), linear([115, 83, 49]));
        let r = linear([255, 128, 0]);
        assert_eq!(u.replace.to_array(), [r[0], r[1], r[2], 0.25]);
        // The travel and the amount are numbers, kept as given.
        assert_eq!(rgb(u.light_travel[0]), [0.0, 0.0, -1.0]);
    }

    /// An IR material with a colour map alone and this tint.
    fn material(tint: [u8; 4]) -> model::Material {
        model::Material {
            name: "made up".into(),
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
                material_tint: tint,
                glow: model::Glow::None,
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

    #[test]
    fn a_neutral_tint_scales_by_1_0079() {
        let neutral = [128, 128, 128, 255];
        let mut scene = Scene::new();
        scene.placement_tint = tint(neutral);
        let m = reference_material(&material(neutral)).unwrap();
        let u = CharacterUniform::new(&m, &scene, ShadowLookup::NONE, 0.0);
        // Each tint centred on 128 of 255, so four times their product is
        // about 1 for two neutral ones.
        let scale = 4.0 * u.placement_tint.x * u.material_tint.x;
        assert!((scale - 1.0079).abs() < 1e-4, "{scale}");
        assert_eq!((u.placement_tint.w, u.material_tint.w), (1.0, 1.0));
        // Decoded, the same tint would darken every part about 5.4 times.
        let decoded = linear([128, 128, 128])[0];
        assert!((4.0 * decoded * decoded - 0.186).abs() < 1e-3);

        // The spec's model on those numbers: an unlit part shows its colour
        // map times 1.0079, the vertex colour's half grey counting twice.
        let colour = Flat([0.25, 0.5, 0.125, 1.0]);
        let fragment = Fragment {
            normal: [0.0, 0.0, 1.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 1.0, 0.0],
            position: [0.0, 0.0, -5.0],
            depth: 5.0,
            eye: [0.0, 0.0, 1.0],
            vertex_colour: shading::HALF_GREY,
            uv: [[0.5; 2]; 3],
            shadow: [0.0; 4],
        };
        let textures = Textures {
            colour: Some(&colour),
            ..Textures::default()
        };
        let shaded = shading::shade(&m, &scene, &fragment, &textures).unwrap();
        for (got, map) in shaded.colour.iter().zip(colour.0) {
            assert!((got - map * scale).abs() < 1e-6, "{got} for {map}");
        }
    }

    /// A 4x4 KTX2 of `format`, every texel the same.
    fn ktx2(format: Format) -> Vec<u8> {
        let block = match format {
            Format::Bc1RgbaUnorm => vec![0xFF, 0xFF, 0x00, 0x00, 0, 0, 0, 0],
            Format::Bc3Unorm => {
                let mut b = vec![0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
                b.extend([0xFF, 0xFF, 0x00, 0x00, 0, 0, 0, 0]);
                b
            }
            Format::R8Unorm => vec![200; 16],
        };
        Texture {
            format,
            width: 4,
            height: 4,
            levels: vec![block],
        }
        .to_ktx2()
    }

    #[test]
    fn each_role_samples_through_its_view() {
        let linear_sampler = Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Clamp,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        };
        let roles = [
            (TextureRole::Layer1ColorMap, true),
            (TextureRole::Layer1BaseNormalMap, false),
            (TextureRole::Layer1BaseSpecMap, true),
            (TextureRole::Layer2ColorMap, true),
            (TextureRole::Layer2BaseNormalMap, false),
            (TextureRole::Layer2BaseSpecMap, true),
            (TextureRole::Layer3ColorMap, true),
            (TextureRole::Layer4ColorMap, true),
        ];
        for (role, srgb) in roles {
            assert_eq!(is_srgb(role), srgb, "{role:?}");
            for (format, want) in [
                (
                    Format::Bc1RgbaUnorm,
                    [TextureFormat::Bc1RgbaUnorm, TextureFormat::Bc1RgbaUnormSrgb],
                ),
                (
                    Format::Bc3Unorm,
                    [TextureFormat::Bc3RgbaUnorm, TextureFormat::Bc3RgbaUnormSrgb],
                ),
            ] {
                let image = texture(
                    &ktx2(format),
                    role,
                    &linear_sampler,
                    CompressedImageFormats::BC,
                )
                .unwrap();
                assert_eq!(
                    image.texture_descriptor.format,
                    want[usize::from(srgb)],
                    "{role:?} {format:?}"
                );
                let ImageSampler::Descriptor(d) = &image.sampler else {
                    panic!("{role:?}: the IR's sampler, not the default");
                };
                assert_eq!(*d, sampler(&linear_sampler));
            }
        }
    }

    /// AE.31: a model's or a level's texture keeps the disc's row order,
    /// bottom row first, the row RSX samples at v = 0 (AC.15, AC.21), so the
    /// disc's UVs read what the original does. Only a portrait's PNG flips.
    #[test]
    fn a_texture_keeps_the_stored_rows() {
        let red = vec![0x00, 0xF8, 0, 0, 0, 0, 0, 0];
        let blue = vec![0x1F, 0x00, 0, 0, 0, 0, 0, 0];
        let stored = [red, blue].concat();
        let ktx2 = Texture {
            format: Format::Bc1RgbaUnorm,
            width: 4,
            height: 8,
            levels: vec![stored.clone()],
        }
        .to_ktx2();
        let s = Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        };
        for srgb in [true, false] {
            let image = texture_view(&ktx2, srgb, &s, CompressedImageFormats::BC).unwrap();
            assert_eq!((image.width(), image.height()), (4, 8));
            assert_eq!(image.data.as_deref(), Some(&stored[..]), "sRGB {srgb}");
        }
    }

    #[test]
    fn a_gloss_map_samples_as_stored() {
        // A third layer's colour map decodes; the same slot holding a gloss
        // map is a weight. Every other role keeps its view either way.
        let mut third = material([128, 128, 128, 255]);
        third.pixel.base = 0x02;
        let mut gloss = material([128, 128, 128, 255]);
        gloss.pixel.base = 0x84;
        for role in [
            TextureRole::Layer1ColorMap,
            TextureRole::Layer1BaseNormalMap,
            TextureRole::Layer1BaseSpecMap,
            TextureRole::Layer4ColorMap,
        ] {
            assert_eq!(samples_srgb(&gloss, role), is_srgb(role), "{role:?}");
            assert_eq!(samples_srgb(&third, role), is_srgb(role), "{role:?}");
        }
        assert!(samples_srgb(&third, TextureRole::Layer3ColorMap));
        assert!(!samples_srgb(&gloss, TextureRole::Layer3ColorMap));
        let s = Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        };
        let image = texture_view(
            &ktx2(Format::Bc1RgbaUnorm),
            samples_srgb(&gloss, TextureRole::Layer3ColorMap),
            &s,
            CompressedImageFormats::BC,
        )
        .unwrap();
        assert_eq!(image.texture_descriptor.format, TextureFormat::Bc1RgbaUnorm);
    }

    /// AH.67: a moved placement tint reaches the shader in light, neutral
    /// untouched to the bit, alpha as stored; the lava's going-black darkens
    /// a part as the original's stored multiply does once decoded.
    #[test]
    fn a_moved_placement_tint_multiplies_in_light() {
        assert_eq!(
            tint_in_light([128, 128, 128, 255]),
            tint([128, 128, 128, 255])
        );
        assert_eq!(tint_in_light([128, 128, 128, 83])[3], 83.0 / 255.0);
        let black = tint_in_light([34, 20, 20, 255]);
        let neutral = 128.0 / 255.0;
        for (c, code) in [(black[0], 34.0), (black[1], 20.0), (black[2], 20.0)] {
            // Four times it and a neutral material tint: the part's scale,
            // the original's stored multiply decoded.
            let scale = 4.0 * c * neutral;
            let stored = 4.0 * code / 255.0 * neutral;
            assert!(
                (scale - f32::powf(stored, 2.2)).abs() < 0.002,
                "{code}: {scale}"
            );
        }
        assert!((4.0 * black[0] * neutral - 0.055).abs() < 0.001);
        assert_eq!(black[3], 1.0);
        // Brighter than neutral brightens more than as stored.
        assert!(tint_in_light([200, 128, 128, 255])[0] > 200.0 / 255.0);
    }

    /// AG.79: a strength on stored values is its 2.2nd power on decoded
    /// ones, so the coins' bronze at strength 2 stays neutral at a grey of
    /// one half and multiplies by 2.9 at its middle (0.82 stored), where as
    /// stored it multiplied by 1.6 and decoded at 2 by 1.3.
    #[test]
    fn a_strength_on_stored_values_is_its_power_on_decoded_ones() {
        assert_eq!(in_light(1.0), 1.0);
        assert_eq!(in_light(0.0), 0.0);
        assert_eq!(in_light(-1.0), 0.0, "no strength");
        let grey = linear([128, 128, 128])[0];
        assert!(
            (in_light(2.0) * grey - 1.0).abs() < 0.02,
            "{}",
            in_light(2.0) * grey
        );
        let middle = linear([209, 209, 209])[0];
        let stored = 2.0 * 209.0 / 255.0;
        assert!((in_light(2.0) * middle - f32::powf(stored, 2.2)).abs() < 0.06);
        assert!(in_light(2.0) * middle > 2.8 && 2.0 * middle < 1.3);
        // Every texture keeps the view it had: a rim map decodes.
        let mut coin = material([128, 128, 128, 255]);
        coin.params.layer4_blend = [0.0, 0.0, 1.0, 0.0];
        assert!(samples_srgb(&coin, TextureRole::Layer4ColorMap));
    }

    #[test]
    fn ir_samplers_map_to_bevys() {
        let d = sampler(&Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Clamp,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        });
        assert_eq!(
            (d.address_mode_u, d.address_mode_v),
            (ImageAddressMode::Repeat, ImageAddressMode::ClampToEdge)
        );
        assert_eq!(
            (d.mag_filter, d.min_filter, d.mipmap_filter, d.lod_max_clamp),
            (
                ImageFilterMode::Linear,
                ImageFilterMode::Linear,
                ImageFilterMode::Linear,
                32.0
            )
        );
        let flat = sampler(&Sampler {
            wrap_u: Wrap::Clamp,
            wrap_v: Wrap::Clamp,
            mag: Filter::Nearest,
            min: Filter::Nearest,
            mip: None,
        });
        assert_eq!(
            (flat.min_filter, flat.lod_max_clamp),
            (ImageFilterMode::Nearest, 0.0)
        );
    }

    #[test]
    fn the_camera_is_hdr_into_tonymcmapface_on_8_bit_srgb() {
        let mut world = World::new();
        let e = world.spawn(camera()).id();
        let e = world.entity(e);
        assert!(e.contains::<Hdr>(), "an Rgba16Float main texture");
        assert_eq!(e.get::<Tonemapping>(), Some(&Tonemapping::TonyMcMapface));
        assert_eq!(e.get::<Msaa>(), Some(&Msaa::Sample4));
        assert_eq!(e.get::<DebandDither>(), Some(&DebandDither::Disabled));
        // No exposure lift: nothing scales the material's result, which
        // doesn't read Bevy's exposure anyway.
        assert!(
            e.get::<Exposure>()
                .is_none_or(|x| x.ev100 == Exposure::default().ev100)
        );

        let image = target(UVec2::new(960, 540));
        let d = &image.texture_descriptor;
        assert_eq!(d.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!((d.size.width, d.size.height), (960, 540));
        assert!(
            d.usage
                .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
        );
        assert_eq!(image.asset_usage, RenderAssetUsages::default());
    }
}
