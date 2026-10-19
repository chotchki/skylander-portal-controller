// AP.8's TitleMaterial (openspec AP, design decision 7): design_language.md
// §2's display treatment built from the coverage of glyphs a title camera
// rendered white into `glyphs`, back to front: the gold glow, the blurred
// drop, the two hard shadows, the stroke, then the gradient fill. The surface
// is the layout box grown by `margin` design px on each side, and `glyphs` is
// exactly the surface's size, so uv maps texel for texel.
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct TitleParams {
    // --gb at the top of the layout box, --g at its bottom.
    fill_top: vec4<f32>,
    fill_bottom: vec4<f32>,
    // --gi.
    stroke: vec4<f32>,
    // --gs at +2 px, --gi at +3 px.
    shadow_near: vec4<f32>,
    shadow_far: vec4<f32>,
    // Black at 0.5, +drop_offset px, blurred.
    drop: vec4<f32>,
    // --g at 0.3, blurred wide.
    glow: vec4<f32>,
    // Physical px per design px: the TV camera's scale factor times UiScale.
    scale: f32,
    // Design px from here on.
    margin: f32,
    stroke_width: f32,
    drop_offset: f32,
    drop_sigma: f32,
    glow_sigma: f32,
    // 0 the treatment, 1 the sampled coverage as grey, 2 the sampled colour.
    mode: u32,
}

@group(1) @binding(0) var<uniform> title: TitleParams;
@group(1) @binding(1) var glyphs: texture_2d<f32>;
@group(1) @binding(2) var glyphs_sampler: sampler;

const TAU: f32 = 6.28318530718;
const GOLDEN_ANGLE: f32 = 2.39996322973;

fn coverage(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(glyphs, glyphs_sampler, uv, 0.0).a;
}

// The glyphs grown by the stroke: the most coverage on two rings around uv.
fn stroked(uv: vec2<f32>, texel: vec2<f32>) -> f32 {
    var a = coverage(uv);
    let r = title.stroke_width;
    for (var i = 0; i < 12; i++) {
        let t = f32(i) * TAU / 12.0;
        let d = vec2<f32>(cos(t), sin(t)) * texel;
        a = max(a, coverage(uv + d * r));
        a = max(a, coverage(uv + d * (r * 0.5)));
    }
    return a;
}

// Interleaved gradient noise (Jimenez 2014): a fixed per-pixel value in [0, 1).
fn noise(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// Gaussian coverage: `n` taps spread evenly over a disc out to 2.5 sigma
// (a golden-angle spiral, turned per pixel by `spin` so too few taps show as
// fine grain rather than as rings), each weighted by the Gaussian at its radius.
fn blurred(uv: vec2<f32>, texel: vec2<f32>, sigma: f32, n: i32, spin: f32) -> f32 {
    var sum = coverage(uv);
    var weights = 1.0;
    let radius = 2.5 * sigma;
    for (var i = 1; i < n; i++) {
        let r = radius * sqrt(f32(i) / f32(n));
        let t = f32(i) * GOLDEN_ANGLE + spin * TAU;
        let w = exp(-(r * r) / (2.0 * sigma * sigma));
        sum += w * coverage(uv + vec2<f32>(cos(t), sin(t)) * texel * r);
        weights += w;
    }
    return sum / weights;
}

// `color` at `amount` coverage over `dst`, straight alpha in and out.
fn over(dst: vec4<f32>, color: vec4<f32>, amount: f32) -> vec4<f32> {
    let a = color.a * amount;
    let out_a = a + dst.a * (1.0 - a);
    if out_a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let rgb = (color.rgb * a + dst.rgb * dst.a * (1.0 - a)) / out_a;
    return vec4<f32>(rgb, out_a);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    if title.mode == 1u {
        return vec4<f32>(vec3<f32>(coverage(in.uv)), 1.0);
    }
    if title.mode == 2u {
        return vec4<f32>(textureSampleLevel(glyphs, glyphs_sampler, in.uv, 0.0).rgb, 1.0);
    }
    // uv per design px.
    let texel = vec2<f32>(title.scale) / in.size;
    let down = vec2<f32>(0.0, texel.y);
    var out = vec4<f32>(0.0);
    let spin = noise(in.position.xy);
    out = over(out, title.glow, blurred(in.uv, texel, title.glow_sigma, 64, spin));
    out = over(out, title.drop, blurred(in.uv - down * title.drop_offset, texel, title.drop_sigma, 24, spin));
    out = over(out, title.shadow_far, stroked(in.uv - down * 3.0, texel));
    out = over(out, title.shadow_near, stroked(in.uv - down * 2.0, texel));
    out = over(out, title.stroke, stroked(in.uv, texel));
    let top = title.margin * texel.y;
    let t = clamp((in.uv.y - top) / (1.0 - 2.0 * top), 0.0, 1.0);
    out = over(out, mix(title.fill_top, title.fill_bottom, t), coverage(in.uv));
    return out;
}
