//! A draw's pipeline state read back from the NV4097 registers: the colour and depth targets it
//! renders into, viewport and scissor, blend, write masks, depth, alpha test and culling, plus
//! each texture unit's sampler (wrap, filter, LOD, border colour). Raw words ride beside every
//! decoded field, so a value this decoder names wrongly can still be read off the dump.
//! Layouts are RPCS3's `rsx_decode.h`, `RSXTexture.cpp` and `gcm_enums.h` at the vendored pin.

use serde::Serialize;

use crate::nv4097 as m;

#[derive(Debug, Clone, Serialize)]
pub struct ColorTarget {
    /// `a`, `b`, `c` or `d`: the surface slot, which is also the fragment output it takes
    /// (`a` is output 0).
    pub surface: char,
    pub offset: u32,
    pub location: u32,
    pub pitch: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Surface {
    pub format_raw: u32,
    pub color_format: &'static str,
    pub depth_format: &'static str,
    /// `pitch` (linear rows) or `swizzle`.
    pub layout: &'static str,
    pub antialias: &'static str,
    /// The surface's size as a power of two (swizzled surfaces only; 0 on pitch surfaces).
    pub log2_width: u32,
    pub log2_height: u32,
    pub color_target_raw: u32,
    pub color_targets: Vec<ColorTarget>,
    pub zeta_offset: u32,
    pub zeta_location: u32,
    pub zeta_pitch: u32,
    /// The surface clip: where on the surface the draw may land, and its size.
    pub clip: Rect,
    pub window_offset: [u32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Viewport {
    pub rect: Rect,
    /// `SET_VIEWPORT_SCALE` and `SET_VIEWPORT_OFFSET`: clip space to window space.
    pub scale: [f32; 4],
    pub offset: [f32; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct Blend {
    pub enabled: bool,
    /// `SET_BLEND_ENABLE_MRT` bits 1-3: blending on surfaces b, c and d.
    pub enabled_mrt: [bool; 3],
    pub src_rgb: &'static str,
    pub src_alpha: &'static str,
    pub dst_rgb: &'static str,
    pub dst_alpha: &'static str,
    pub equation_rgb: &'static str,
    pub equation_alpha: &'static str,
    /// `SET_BLEND_COLOR`: A8R8G8B8 on an 8-bit surface, red and green as halves on a float one.
    pub color_raw: u32,
    /// `SET_BLEND_COLOR2`: blue and alpha as halves on a float surface.
    pub color2_raw: u32,
    pub sfactor_raw: u32,
    pub dfactor_raw: u32,
    pub equation_raw: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChannelMask {
    pub r: bool,
    pub g: bool,
    pub b: bool,
    pub a: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteMasks {
    /// Surface a's channels (`SET_COLOR_MASK`).
    pub color: ChannelMask,
    pub color_raw: u32,
    /// Surfaces b, c and d (`SET_COLOR_MASK_MRT`, a nibble each).
    pub color_mrt: [ChannelMask; 3],
    pub color_mrt_raw: u32,
    pub depth: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Depth {
    pub test: bool,
    pub func: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlphaTest {
    pub enabled: bool,
    pub func: &'static str,
    /// The raw reference: its low byte / 255 on an 8-bit surface, a half on a float one.
    pub reference_raw: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cull {
    pub enabled: bool,
    pub face: &'static str,
    pub front_face: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderState {
    pub surface: Surface,
    pub viewport: Viewport,
    pub scissor: Rect,
    pub blend: Blend,
    pub write_masks: WriteMasks,
    pub depth: Depth,
    pub alpha_test: AlphaTest,
    pub cull: Cull,
    pub dither: bool,
    /// `SET_SHADER_PACKER`: the fragment output is sRGB-encoded on store.
    pub srgb_output: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Sampler {
    pub wrap_s: &'static str,
    pub wrap_t: &'static str,
    pub wrap_r: &'static str,
    pub min_filter: &'static str,
    pub mag_filter: &'static str,
    /// `SET_TEXTURE_FILTER` bits 13-15 (1 quincunx, 2 gaussian, 3 quincunx alt).
    pub convolution: u32,
    /// LOD bias, signed 4.8 fixed point.
    pub lod_bias: f32,
    /// Unsigned 4.8 fixed point, from `SET_TEXTURE_CONTROL0`.
    pub min_lod: f32,
    pub max_lod: f32,
    /// `SET_TEXTURE_CONTROL0` bits 4-6 as the anisotropy it caps at.
    pub max_aniso: u32,
    /// `SET_TEXTURE_ADDRESS` bits 20-23: channels the unit decodes from sRGB, as RPCS3's
    /// `gamma()` reorders them, a bit each for A, R, G, B from the top (0 = none).
    pub gamma: u32,
    pub address_raw: u32,
    pub filter_raw: u32,
    pub control0_raw: u32,
    pub border_color_raw: u32,
}

pub fn color_format_name(raw: u32) -> &'static str {
    match raw {
        1 => "X1R5G5B5_Z1R5G5B5",
        2 => "X1R5G5B5_O1R5G5B5",
        3 => "R5G6B5",
        4 => "X8R8G8B8_Z8R8G8B8",
        5 => "X8R8G8B8_O8R8G8B8",
        8 => "A8R8G8B8",
        9 => "B8",
        10 => "G8B8",
        11 => "F_W16Z16Y16X16",
        12 => "F_W32Z32Y32X32",
        13 => "F_X32",
        14 => "X8B8G8R8_Z8B8G8R8",
        15 => "X8B8G8R8_O8B8G8R8",
        16 => "A8B8G8R8",
        _ => "unknown",
    }
}

fn depth_format_name(raw: u32) -> &'static str {
    match raw {
        0 => "none",
        1 => "Z16",
        2 => "Z24S8",
        // RPCS3 treats 3 and 4 as the float variants.
        3 => "Z16_FLOAT",
        4 => "Z24S8_FLOAT",
        _ => "unknown",
    }
}

fn surface_layout_name(raw: u32) -> &'static str {
    match raw {
        1 => "pitch",
        2 => "swizzle",
        _ => "unknown",
    }
}

fn antialias_name(raw: u32) -> &'static str {
    match raw {
        0 => "center_1",
        3 => "diagonal_centered_2",
        4 => "square_centered_4",
        5 => "square_rotated_4",
        _ => "unknown",
    }
}

/// The surfaces `SET_SURFACE_COLOR_TARGET` enables, in output order.
pub fn color_target_surfaces(raw: u32) -> &'static [char] {
    match raw {
        1 => &['a'],
        2 => &['b'],
        0x13 => &['a', 'b'],
        0x17 => &['a', 'b', 'c'],
        0x1f => &['a', 'b', 'c', 'd'],
        _ => &[],
    }
}

pub fn blend_factor_name(raw: u32) -> &'static str {
    match raw {
        0 => "zero",
        1 => "one",
        0x300 => "src_color",
        0x301 => "one_minus_src_color",
        0x302 => "src_alpha",
        0x303 => "one_minus_src_alpha",
        0x304 => "dst_alpha",
        0x305 => "one_minus_dst_alpha",
        0x306 => "dst_color",
        0x307 => "one_minus_dst_color",
        0x308 => "src_alpha_saturate",
        0x8001 => "constant_color",
        0x8002 => "one_minus_constant_color",
        0x8003 => "constant_alpha",
        0x8004 => "one_minus_constant_alpha",
        _ => "unknown",
    }
}

pub fn blend_equation_name(raw: u32) -> &'static str {
    match raw {
        0x8006 => "add",
        0x8007 => "min",
        0x8008 => "max",
        0x800a => "subtract",
        0x800b => "reverse_subtract",
        0xf005 => "reverse_subtract_signed",
        0xf006 => "add_signed",
        0xf007 => "reverse_add_signed",
        _ => "unknown",
    }
}

pub fn compare_name(raw: u32) -> &'static str {
    match raw {
        0x200 => "never",
        0x201 => "less",
        0x202 => "equal",
        0x203 => "lequal",
        0x204 => "greater",
        0x205 => "notequal",
        0x206 => "gequal",
        0x207 => "always",
        _ => "unknown",
    }
}

fn cull_face_name(raw: u32) -> &'static str {
    match raw {
        0x404 => "front",
        0x405 => "back",
        0x408 => "front_and_back",
        _ => "unknown",
    }
}

fn front_face_name(raw: u32) -> &'static str {
    match raw {
        0x900 => "cw",
        0x901 => "ccw",
        _ => "unknown",
    }
}

pub fn wrap_name(raw: u32) -> &'static str {
    match raw {
        1 => "wrap",
        2 => "mirror",
        3 => "clamp_to_edge",
        4 => "border",
        5 => "clamp",
        6 => "mirror_once_clamp_to_edge",
        7 => "mirror_once_border",
        8 => "mirror_once_clamp",
        _ => "unknown",
    }
}

pub fn min_filter_name(raw: u32) -> &'static str {
    match raw {
        1 => "nearest",
        2 => "linear",
        3 => "nearest_mip_nearest",
        4 => "linear_mip_nearest",
        5 => "nearest_mip_linear",
        6 => "linear_mip_linear",
        7 => "convolution",
        _ => "unknown",
    }
}

pub fn mag_filter_name(raw: u32) -> &'static str {
    match raw {
        1 => "nearest",
        2 => "linear",
        4 => "convolution",
        _ => "unknown",
    }
}

fn ufixed(raw: u32, frac: u32) -> f32 {
    raw as f32 / (1u32 << frac) as f32
}

/// Signed (two's complement) fixed point over `bits` bits with `frac` fraction bits.
fn sfixed(raw: u32, bits: u32, frac: u32) -> f32 {
    let shift = 32 - bits;
    ((raw << shift) as i32 >> shift) as f32 / (1u32 << frac) as f32
}

fn rect(horizontal: u32, vertical: u32) -> Rect {
    Rect {
        x: horizontal & 0xffff,
        y: vertical & 0xffff,
        width: horizontal >> 16,
        height: vertical >> 16,
    }
}

fn floats(regs: impl Fn(u32) -> u32, first: u32) -> [f32; 4] {
    std::array::from_fn(|k| f32::from_bits(regs(first + k as u32)))
}

/// Reads the render state out of a register file indexed by `method_offset >> 2`.
pub fn render_state(reg: impl Fn(u32) -> u32) -> RenderState {
    let format = reg(m::SET_SURFACE_FORMAT);
    let target_raw = reg(m::SET_SURFACE_COLOR_TARGET);
    let color_targets = color_target_surfaces(target_raw)
        .iter()
        .map(|&surface| {
            let (offset, dma, pitch) = match surface {
                'a' => (
                    m::SET_SURFACE_COLOR_AOFFSET,
                    m::SET_CONTEXT_DMA_COLOR_A,
                    m::SET_SURFACE_PITCH_A,
                ),
                'b' => (
                    m::SET_SURFACE_COLOR_BOFFSET,
                    m::SET_CONTEXT_DMA_COLOR_B,
                    m::SET_SURFACE_PITCH_B,
                ),
                'c' => (
                    m::SET_SURFACE_COLOR_COFFSET,
                    m::SET_CONTEXT_DMA_COLOR_C,
                    m::SET_SURFACE_PITCH_C,
                ),
                _ => (
                    m::SET_SURFACE_COLOR_DOFFSET,
                    m::SET_CONTEXT_DMA_COLOR_D,
                    m::SET_SURFACE_PITCH_D,
                ),
            };
            ColorTarget {
                surface,
                offset: reg(offset),
                location: m::normalize_location(reg(dma)),
                pitch: reg(pitch),
            }
        })
        .collect();
    let mask = reg(m::SET_COLOR_MASK);
    let mrt = reg(m::SET_COLOR_MASK_MRT);
    let blend_mrt = reg(m::SET_BLEND_ENABLE_MRT);
    let sfactor = reg(m::SET_BLEND_FUNC_SFACTOR);
    let dfactor = reg(m::SET_BLEND_FUNC_DFACTOR);
    let equation = reg(m::SET_BLEND_EQUATION);
    RenderState {
        surface: Surface {
            format_raw: format,
            color_format: color_format_name(format & 0x1f),
            depth_format: depth_format_name((format >> 5) & 0x7),
            layout: surface_layout_name((format >> 8) & 0xf),
            antialias: antialias_name((format >> 12) & 0xf),
            log2_width: (format >> 16) & 0xff,
            log2_height: format >> 24,
            color_target_raw: target_raw,
            color_targets,
            zeta_offset: reg(m::SET_SURFACE_ZETA_OFFSET),
            zeta_location: m::normalize_location(reg(m::SET_CONTEXT_DMA_ZETA)),
            zeta_pitch: reg(m::SET_SURFACE_PITCH_Z),
            clip: rect(
                reg(m::SET_SURFACE_CLIP_HORIZONTAL),
                reg(m::SET_SURFACE_CLIP_VERTICAL),
            ),
            window_offset: {
                let w = reg(m::SET_WINDOW_OFFSET);
                [w & 0xffff, w >> 16]
            },
        },
        viewport: Viewport {
            rect: rect(
                reg(m::SET_VIEWPORT_HORIZONTAL),
                reg(m::SET_VIEWPORT_VERTICAL),
            ),
            scale: floats(&reg, m::SET_VIEWPORT_SCALE),
            offset: floats(&reg, m::SET_VIEWPORT_OFFSET),
        },
        scissor: rect(reg(m::SET_SCISSOR_HORIZONTAL), reg(m::SET_SCISSOR_VERTICAL)),
        blend: Blend {
            enabled: reg(m::SET_BLEND_ENABLE) != 0,
            enabled_mrt: std::array::from_fn(|k| blend_mrt >> (k + 1) & 1 == 1),
            src_rgb: blend_factor_name(sfactor & 0xffff),
            src_alpha: blend_factor_name(sfactor >> 16),
            dst_rgb: blend_factor_name(dfactor & 0xffff),
            dst_alpha: blend_factor_name(dfactor >> 16),
            equation_rgb: blend_equation_name(equation & 0xffff),
            equation_alpha: blend_equation_name(equation >> 16),
            color_raw: reg(m::SET_BLEND_COLOR),
            color2_raw: reg(m::SET_BLEND_COLOR2),
            sfactor_raw: sfactor,
            dfactor_raw: dfactor,
            equation_raw: equation,
        },
        write_masks: WriteMasks {
            color: ChannelMask {
                r: (mask >> 16) & 0xff != 0,
                g: (mask >> 8) & 0xff != 0,
                b: mask & 0xff != 0,
                a: mask >> 24 != 0,
            },
            color_raw: mask,
            color_mrt: std::array::from_fn(|k| {
                let n = mrt >> ((k + 1) * 4);
                ChannelMask {
                    a: n & 1 != 0,
                    r: n & 2 != 0,
                    g: n & 4 != 0,
                    b: n & 8 != 0,
                }
            }),
            color_mrt_raw: mrt,
            depth: reg(m::SET_DEPTH_MASK) != 0,
        },
        depth: Depth {
            test: reg(m::SET_DEPTH_TEST_ENABLE) != 0,
            func: compare_name(reg(m::SET_DEPTH_FUNC)),
        },
        alpha_test: AlphaTest {
            enabled: reg(m::SET_ALPHA_TEST_ENABLE) != 0,
            func: compare_name(reg(m::SET_ALPHA_FUNC)),
            reference_raw: reg(m::SET_ALPHA_REF),
        },
        cull: Cull {
            enabled: reg(m::SET_CULL_FACE_ENABLE) != 0,
            face: cull_face_name(reg(m::SET_CULL_FACE)),
            front_face: front_face_name(reg(m::SET_FRONT_FACE)),
        },
        dither: reg(m::SET_DITHER_ENABLE) != 0,
        srgb_output: reg(m::SET_SHADER_PACKER) != 0,
    }
}

/// A unit's registers sit `TEXTURE_UNIT_STRIDE` apart, so `unit` picks its copy of each.
pub fn sampler(reg: impl Fn(u32) -> u32, unit: u32) -> Sampler {
    let r = |base: u32| reg(base + unit * m::TEXTURE_UNIT_STRIDE);
    let address = r(m::SET_TEXTURE_ADDRESS);
    let filter = r(m::SET_TEXTURE_FILTER);
    let control0 = r(m::SET_TEXTURE_CONTROL0);
    let rgba = (address >> 20) & 0xf;
    Sampler {
        wrap_s: wrap_name(address & 0xf),
        wrap_t: wrap_name((address >> 8) & 0xf),
        wrap_r: wrap_name((address >> 16) & 0xf),
        min_filter: min_filter_name((filter >> 16) & 0x7),
        mag_filter: mag_filter_name((filter >> 24) & 0x7),
        convolution: (filter >> 13) & 0x7,
        lod_bias: sfixed(filter & 0x1fff, 13, 8),
        min_lod: ufixed((control0 >> 19) & 0xfff, 8),
        max_lod: ufixed((control0 >> 7) & 0xfff, 8),
        max_aniso: [1, 2, 4, 6, 8, 10, 12, 16][((control0 >> 4) & 0x7) as usize],
        gamma: ((rgba << 1) & 0xf) | (rgba >> 3),
        address_raw: address,
        filter_raw: filter,
        control0_raw: control0,
        border_color_raw: r(m::SET_TEXTURE_BORDER_COLOR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_lods_and_bias_decode_as_rpcs3_does() {
        assert_eq!(ufixed(0x100, 8), 1.0);
        assert_eq!(ufixed(0xc80, 8), 12.5);
        assert_eq!(sfixed(0x80, 13, 8), 0.5);
        assert_eq!(sfixed(0x1f80, 13, 8), -0.5);
        assert_eq!(sfixed(0x1000, 13, 8), -16.0);
    }

    #[test]
    fn mrt_masks_take_a_nibble_per_surface_alpha_lowest() {
        let mut regs = vec![0u32; crate::capture::REGISTER_WORDS];
        // Surface b: alpha only; c: red and blue; d: everything.
        regs[m::SET_COLOR_MASK_MRT as usize] = 0x1 << 4 | 0xa << 8 | 0xf << 12;
        let s = render_state(|r| regs[r as usize]);
        let [b, c, d] = s.write_masks.color_mrt;
        assert_eq!(
            b,
            ChannelMask {
                r: false,
                g: false,
                b: false,
                a: true
            }
        );
        assert_eq!(
            c,
            ChannelMask {
                r: true,
                g: false,
                b: true,
                a: false
            }
        );
        assert!(d.r && d.g && d.b && d.a);
    }
}
