//! The NV4097 method registers the replay reads, as register indices (byte offset / 4).
//! Offsets match RPCS3's `gcm_enums.h` at the vendored pin.

pub const NO_OPERATION: u32 = 0x0100 >> 2;
pub const SET_CONTEXT_DMA_COLOR_B: u32 = 0x018c >> 2;
pub const SET_CONTEXT_DMA_COLOR_A: u32 = 0x0194 >> 2;
pub const SET_CONTEXT_DMA_ZETA: u32 = 0x0198 >> 2;
pub const SET_CONTEXT_DMA_COLOR_C: u32 = 0x01b4 >> 2;
pub const SET_CONTEXT_DMA_COLOR_D: u32 = 0x01b8 >> 2;
pub const SET_SURFACE_CLIP_HORIZONTAL: u32 = 0x0200 >> 2;
pub const SET_SURFACE_CLIP_VERTICAL: u32 = 0x0204 >> 2;
pub const SET_SURFACE_FORMAT: u32 = 0x0208 >> 2;
pub const SET_SURFACE_PITCH_A: u32 = 0x020c >> 2;
pub const SET_SURFACE_COLOR_AOFFSET: u32 = 0x0210 >> 2;
pub const SET_SURFACE_ZETA_OFFSET: u32 = 0x0214 >> 2;
pub const SET_SURFACE_COLOR_BOFFSET: u32 = 0x0218 >> 2;
pub const SET_SURFACE_PITCH_B: u32 = 0x021c >> 2;
pub const SET_SURFACE_COLOR_TARGET: u32 = 0x0220 >> 2;
pub const SET_SURFACE_PITCH_Z: u32 = 0x022c >> 2;
pub const SET_SURFACE_PITCH_C: u32 = 0x0280 >> 2;
pub const SET_SURFACE_PITCH_D: u32 = 0x0284 >> 2;
pub const SET_SURFACE_COLOR_COFFSET: u32 = 0x0288 >> 2;
pub const SET_SURFACE_COLOR_DOFFSET: u32 = 0x028c >> 2;
pub const SET_WINDOW_OFFSET: u32 = 0x02b8 >> 2;
pub const SET_DITHER_ENABLE: u32 = 0x0300 >> 2;
pub const SET_ALPHA_TEST_ENABLE: u32 = 0x0304 >> 2;
pub const SET_ALPHA_FUNC: u32 = 0x0308 >> 2;
pub const SET_ALPHA_REF: u32 = 0x030c >> 2;
pub const SET_BLEND_ENABLE: u32 = 0x0310 >> 2;
pub const SET_BLEND_FUNC_SFACTOR: u32 = 0x0314 >> 2;
pub const SET_BLEND_FUNC_DFACTOR: u32 = 0x0318 >> 2;
pub const SET_BLEND_COLOR: u32 = 0x031c >> 2;
pub const SET_BLEND_EQUATION: u32 = 0x0320 >> 2;
pub const SET_COLOR_MASK: u32 = 0x0324 >> 2;
pub const SET_BLEND_ENABLE_MRT: u32 = 0x036c >> 2;
pub const SET_COLOR_MASK_MRT: u32 = 0x0370 >> 2;
pub const SET_BLEND_COLOR2: u32 = 0x037c >> 2;
pub const SET_SCISSOR_HORIZONTAL: u32 = 0x08c0 >> 2;
pub const SET_SCISSOR_VERTICAL: u32 = 0x08c4 >> 2;
pub const SET_SHADER_PROGRAM: u32 = 0x08e4 >> 2;
pub const SET_VIEWPORT_HORIZONTAL: u32 = 0x0a00 >> 2;
pub const SET_VIEWPORT_VERTICAL: u32 = 0x0a04 >> 2;
pub const SET_VIEWPORT_OFFSET: u32 = 0x0a20 >> 2;
pub const SET_VIEWPORT_SCALE: u32 = 0x0a30 >> 2;
pub const SET_DEPTH_FUNC: u32 = 0x0a6c >> 2;
pub const SET_DEPTH_MASK: u32 = 0x0a70 >> 2;
pub const SET_DEPTH_TEST_ENABLE: u32 = 0x0a74 >> 2;
pub const SET_TRANSFORM_PROGRAM: u32 = 0x0b80 >> 2;
pub const SET_VERTEX_DATA_ARRAY_OFFSET: u32 = 0x1680 >> 2;
pub const SET_VERTEX_DATA_BASE_OFFSET: u32 = 0x1738 >> 2;
pub const SET_VERTEX_DATA_BASE_INDEX: u32 = 0x173c >> 2;
pub const SET_VERTEX_DATA_ARRAY_FORMAT: u32 = 0x1740 >> 2;
pub const SET_BEGIN_END: u32 = 0x1808 >> 2;
pub const SET_CULL_FACE: u32 = 0x1830 >> 2;
pub const SET_FRONT_FACE: u32 = 0x1834 >> 2;
pub const SET_CULL_FACE_ENABLE: u32 = 0x183c >> 2;
pub const DRAW_ARRAYS: u32 = 0x1814 >> 2;
pub const SET_INDEX_ARRAY_ADDRESS: u32 = 0x181c >> 2;
pub const SET_INDEX_ARRAY_DMA: u32 = 0x1820 >> 2;
pub const DRAW_INDEX_ARRAY: u32 = 0x1824 >> 2;
pub const SET_TEXTURE_CONTROL3: u32 = 0x1840 >> 2;
pub const SET_TEXTURE_OFFSET: u32 = 0x1a00 >> 2;
pub const SET_TEXTURE_FORMAT: u32 = 0x1a04 >> 2;
pub const SET_TEXTURE_ADDRESS: u32 = 0x1a08 >> 2;
pub const SET_TEXTURE_CONTROL0: u32 = 0x1a0c >> 2;
pub const SET_TEXTURE_CONTROL1: u32 = 0x1a10 >> 2;
pub const SET_TEXTURE_FILTER: u32 = 0x1a14 >> 2;
pub const SET_TEXTURE_IMAGE_RECT: u32 = 0x1a18 >> 2;
pub const SET_TEXTURE_BORDER_COLOR: u32 = 0x1a1c >> 2;
pub const SET_SHADER_CONTROL: u32 = 0x1d60 >> 2;
pub const SET_ZSTENCIL_CLEAR_VALUE: u32 = 0x1d8c >> 2;
pub const SET_COLOR_CLEAR_VALUE: u32 = 0x1d90 >> 2;
pub const CLEAR_SURFACE: u32 = 0x1d94 >> 2;
pub const SET_CLEAR_RECT_HORIZONTAL: u32 = 0x1d98 >> 2;
pub const SET_CLEAR_RECT_VERTICAL: u32 = 0x1d9c >> 2;
pub const SET_RESTART_INDEX_ENABLE: u32 = 0x1dac >> 2;
pub const SET_RESTART_INDEX: u32 = 0x1db0 >> 2;
pub const SET_TRANSFORM_PROGRAM_LOAD: u32 = 0x1e9c >> 2;
pub const SET_TRANSFORM_PROGRAM_START: u32 = 0x1ea0 >> 2;
pub const SET_TRANSFORM_CONSTANT_LOAD: u32 = 0x1efc >> 2;
pub const SET_TRANSFORM_CONSTANT: u32 = 0x1f00 >> 2;
pub const SET_SHADER_PACKER: u32 = 0x1fec >> 2;
pub const SET_VERTEX_ATTRIB_INPUT_MASK: u32 = 0x1ff0 >> 2;
pub const SET_VERTEX_ATTRIB_OUTPUT_MASK: u32 = 0x1ff4 >> 2;

/// Register-index stride between consecutive fragment texture units (0x20 bytes).
pub const TEXTURE_UNIT_STRIDE: u32 = 8;
pub const FRAGMENT_TEXTURE_UNITS: u32 = 16;
pub const VERTEX_ARRAYS: u32 = 16;
/// Words in one constant or program upload window.
pub const UPLOAD_WINDOW: u32 = 32;
/// Instruction slots of transform-program memory, four words each.
pub const TRANSFORM_PROGRAM_SLOTS: usize = 544;
/// Constants the vertex program can address (RPCS3 ignores writes past 467).
pub const TRANSFORM_CONSTANTS: usize = 468;

/// Header bits of a recorded command.
pub const HEADER_METHOD_MASK: u32 = 0x0000_fffc;
pub const HEADER_COUNT_SHIFT: u32 = 18;
pub const HEADER_COUNT_MASK: u32 = 0x7ff;
pub const HEADER_NON_INCREMENT: u32 = 0x4000_0000;

/// `SET_TEXTURE_FORMAT` format-byte flags.
pub const TEXTURE_LN: u32 = 0x20;
pub const TEXTURE_UN: u32 = 0x40;

/// `SET_INDEX_ARRAY_ADDRESS` keeps 29 bits of offset (RPCS3 decodes it as `bf_decoder<0, 29>`).
pub const INDEX_ARRAY_OFFSET_MASK: u32 = 0x1fff_ffff;

/// Base formats RPCS3 never Morton-orders: DXT1, DXT23, DXT45, B8R8_G8R8 and R8B8_R8G8.
/// For these a clear `LN` bit only means packed rows (pitch ignored, layers 128-byte aligned);
/// 3D power-of-two DXT is VTC-tiled instead.
pub fn never_swizzled(base: u32) -> bool {
    matches!(base, 0x86 | 0x87 | 0x88 | 0x8d | 0x8e)
}

pub fn primitive_name(value: u32) -> &'static str {
    match value {
        1 => "points",
        2 => "lines",
        3 => "line_loop",
        4 => "line_strip",
        5 => "triangles",
        6 => "triangle_strip",
        7 => "triangle_fan",
        8 => "quads",
        9 => "quad_strip",
        10 => "polygon",
        _ => "unknown",
    }
}

/// Vertex attribute base type (bits 0..3 of the array format) as (name, bytes per component).
pub fn vertex_type(raw: u32) -> (&'static str, u32) {
    match raw {
        1 => ("snorm16", 2),
        2 => ("f32", 4),
        3 => ("f16", 2),
        4 => ("unorm8", 1),
        5 => ("sint16", 2),
        6 => ("cmp32", 4),
        7 => ("uint8", 1),
        _ => ("undefined", 0),
    }
}

/// Bytes one vertex of an array occupies in RSX memory (cmp32 packs all components in one word).
pub fn vertex_element_size(type_raw: u32, components: u32) -> u32 {
    match type_raw {
        6 => 4,
        t => vertex_type(t).1 * components,
    }
}

/// Base texture format (format byte without the LN/UN flags) as a GCM name.
pub fn texture_format_name(base: u32) -> &'static str {
    match base {
        0x81 => "B8",
        0x82 => "A1R5G5B5",
        0x83 => "A4R4G4B4",
        0x84 => "R5G6B5",
        0x85 => "A8R8G8B8",
        0x86 => "COMPRESSED_DXT1",
        0x87 => "COMPRESSED_DXT23",
        0x88 => "COMPRESSED_DXT45",
        0x8b => "G8B8",
        0x8d => "COMPRESSED_B8R8_G8R8",
        0x8e => "COMPRESSED_R8B8_R8G8",
        0x8f => "R6G5B5",
        0x90 => "DEPTH24_D8",
        0x91 => "DEPTH24_D8_FLOAT",
        0x92 => "DEPTH16",
        0x93 => "DEPTH16_FLOAT",
        0x94 => "X16",
        0x95 => "Y16_X16",
        0x97 => "R5G5B5A1",
        0x98 => "COMPRESSED_HILO8",
        0x99 => "COMPRESSED_HILO_S8",
        0x9a => "W16_Z16_Y16_X16_FLOAT",
        0x9b => "W32_Z32_Y32_X32_FLOAT",
        0x9c => "X32_FLOAT",
        0x9d => "D1R5G5B5",
        0x9e => "D8R8G8B8",
        0x9f => "Y16_X16_FLOAT",
        _ => "unknown",
    }
}

/// Folds the DMA-handle spellings of local/main memory onto locations 0 and 1.
pub fn normalize_location(location: u32) -> u32 {
    match location {
        0xfeed_0000 => 0,
        0xfeed_0001 => 1,
        other => other,
    }
}
