// The game's glow chain (openspec AE.11; the character-shading spec's glow
// requirements), run on the remaster's linear frame before tonemapping:
// extract the frame times the glow mask into a smaller buffer, blur it
// across and then down, add it back twice over. `glow.rs` draws one
// fullscreen triangle per step.
//
// Every sample is a linear filter worked here from `textureLoad`s, clamped
// to the edge texel, not left to a sampler, so the CPU reference
// (`guac_ir::glow`) can do the same arithmetic in the same order.

struct Glow {
    // The frame's size and the glow buffer's, in texels.
    frame: vec2<u32>,
    buffer: vec2<u32>,
    // The taps' weights, tap 0 (the centre) through tap 7.
    weights: array<vec4<f32>, 2>,
}

// The step's input: the frame (extract), the buffer (the blurs, the add-back).
@group(0) @binding(0) var source: texture_2d<f32>;
// The glow mask (extract; the other steps bind the source here again).
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var<uniform> glow: Glow;

// The texel at p, or the nearest edge texel past the edge.
fn load(t: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    let last = vec2<i32>(textureDimensions(t)) - vec2<i32>(1);
    return textureLoad(t, clamp(p, vec2<i32>(0), last), 0);
}

fn lerp4(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    return a + (b - a) * t;
}

// A linear filter at p in texel units, texel i's centre at i.
fn filtered(t: texture_2d<f32>, p: vec2<f32>) -> vec4<f32> {
    let p0 = floor(p);
    let f = p - p0;
    let i = vec2<i32>(p0);
    let top = lerp4(load(t, i), load(t, i + vec2<i32>(1, 0)), f.x);
    let bottom = lerp4(load(t, i + vec2<i32>(0, 1)), load(t, i + vec2<i32>(1, 1)), f.x);
    return lerp4(top, bottom, f.y);
}

// Where the pixel whose centre is `at` (texel i's centre is i + 0.5) on an
// image `dst` texels across samples one `src` across, both spanning the
// frame, in the other's texel units.
fn centre(at: vec2<f32>, dst: vec2<u32>, src: vec2<u32>) -> vec2<f32> {
    return at * vec2<f32>(src) / vec2<f32>(dst) - vec2<f32>(0.5);
}

fn weight(k: u32) -> f32 {
    return glow.weights[k / 4u][k % 4u];
}

// Tap k's offset in texels (k from -7 to 7): 0 at the centre, else
// 2|k| - 0.5 to its side, halfway between two texels.
fn offset(k: i32) -> f32 {
    if k == 0 {
        return 0.0;
    }
    return sign(f32(k)) * (2.0 * f32(abs(k)) - 0.5);
}

@fragment
fn extract(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let c = filtered(source, centre(at.xy, glow.buffer, glow.frame));
    let m = filtered(mask, centre(at.xy, glow.buffer, textureDimensions(mask))).r;
    return c * m;
}

// The taps in the CPU reference's order, -7 to 7.
fn blur(at: vec2<f32>, axis: vec2<f32>) -> vec4<f32> {
    let i = floor(at);
    var sum = vec4<f32>(0.0);
    for (var k = -7; k <= 7; k++) {
        sum += filtered(source, i + axis * offset(k)) * weight(u32(abs(k)));
    }
    return sum;
}

@fragment
fn across(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return blur(at.xy, vec2<f32>(1.0, 0.0));
}

@fragment
fn down(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return blur(at.xy, vec2<f32>(0.0, 1.0));
}

// Blended onto the frame by adding colour and leaving its alpha.
@fragment
fn add_back(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let g = filtered(source, centre(at.xy, glow.frame, glow.buffer));
    return vec4<f32>(2.0 * g.rgb, 0.0);
}
