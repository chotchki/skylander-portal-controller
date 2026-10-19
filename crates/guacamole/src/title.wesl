// The TV's title treatment (openspec AP, design decision 7; `title.rs`):
// design_language.md §2's display treatment built from the coverage of
// glyphs a title camera rendered white into `glyphs`, back to front: the gold
// glow, the blurred drop, the two hard shadows, the gradient fill, then the
// stroke over its edge. The surface is the layout box grown by `margin`
// design px on each side, and `glyphs` is exactly the surface's size, so uv
// maps texel for texel. AP.8's spike proved it (openspec AP's tasks.md).
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
    // The sky behind the title, which the CSS blend is matched on.
    backdrop: vec4<f32>,
    // Physical px per design px: the TV camera's scale factor times UiScale.
    scale: f32,
    // Design px from here on.
    margin: f32,
    stroke_width: f32,
    drop_offset: f32,
    drop_sigma: f32,
    glow_sigma: f32,
}

@group(1) @binding(0) var<uniform> title: TitleParams;
@group(1) @binding(1) var glyphs: texture_2d<f32>;
@group(1) @binding(2) var glyphs_sampler: sampler;

const TAU: f32 = 6.28318530718;
const GOLDEN_ANGLE: f32 = 2.39996322973;

fn coverage(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(glyphs, glyphs_sampler, uv, 0.0).a;
}

// The glyphs grown (or with `grow` false, shrunk) by `r` design px: the most
// (least) coverage on two rings around uv.
fn morphed(uv: vec2<f32>, texel: vec2<f32>, r: f32, grow: bool) -> f32 {
    var a = coverage(uv);
    for (var i = 0; i < 12; i++) {
        let t = f32(i) * TAU / 12.0;
        let d = vec2<f32>(cos(t), sin(t)) * texel;
        let near = coverage(uv + d * (r * 0.5));
        let far = coverage(uv + d * r);
        if grow {
            a = max(a, max(near, far));
        } else {
            a = min(a, min(near, far));
        }
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

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, c / 12.92, c <= vec3<f32>(0.04045));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, c * 12.92, c <= vec3<f32>(0.0031308));
}

// A layer's colour in sRGB, the space the stack is composited in.
fn layer(color: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(to_srgb(color.rgb), color.a);
}

// §2 is CSS, which composites in sRGB, while this target blends in linear
// light, where the same 0.3 glow comes out about three times as bright and
// the black drop barely darkens. So the stack is composited in sRGB, then
// handed over as the colour and alpha that land, blended linearly over the
// backdrop, exactly where a browser's sRGB blend would: the least alpha that
// reaches the browser's result, and never less than the stack's own, so an
// opaque pixel stays opaque. Over any other backdrop the difference is that
// backdrop's, times the transmittance, as for any blend. `surfaces::css_over`
// is the same rule on the CPU, for the halo.
fn as_css(stack: vec4<f32>) -> vec4<f32> {
    let b = title.backdrop.rgb;
    let t = to_linear(mix(to_srgb(b), stack.rgb, stack.a));
    let up = max(t - b, vec3<f32>(0.0)) / max(1.0 - b, vec3<f32>(1e-4));
    let down = max(b - t, vec3<f32>(0.0)) / max(b, vec3<f32>(1e-4));
    let need = max(max(up.r, max(up.g, up.b)), max(down.r, max(down.g, down.b)));
    let a = clamp(max(stack.a, need), 0.0, 1.0);
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(clamp(b + (t - b) / a, vec3<f32>(0.0), vec3<f32>(1.0)), a);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // uv per design px.
    let texel = vec2<f32>(title.scale) / in.size;
    let down = vec2<f32>(0.0, texel.y);
    var out = vec4<f32>(0.0);
    let spin = noise(in.position.xy);
    out = over(out, layer(title.glow), blurred(in.uv, texel, title.glow_sigma, 64, spin));
    out = over(out, layer(title.drop), blurred(in.uv - down * title.drop_offset, texel, title.drop_sigma, 24, spin));
    // The hard shadows are the glyphs' own, not their stroke's, as a browser
    // draws them (Chrome: the stroke's outer half covers the first px of the
    // +2 ledge, so 1 px of --gs and 1 of --gi show under it).
    out = over(out, layer(title.shadow_far), coverage(in.uv - down * 3.0));
    out = over(out, layer(title.shadow_near), coverage(in.uv - down * 2.0));
    // The fill, then the stroke over it (CSS paints them in that order); the
    // stroke is centred on the outline as `-webkit-text-stroke` is, half
    // outside it and half over the fill.
    let top = title.margin * texel.y;
    let t = clamp((in.uv.y - top) / (1.0 - 2.0 * top), 0.0, 1.0);
    let fill = mix(layer(title.fill_top), layer(title.fill_bottom), t);
    out = over(out, fill, coverage(in.uv));
    let half = title.stroke_width * 0.5;
    let outer = morphed(in.uv, texel, half, true);
    if outer > 0.0 {
        let inner = morphed(in.uv, texel, half, false);
        out = over(out, layer(title.stroke), min(outer, 1.0 - inner));
    }
    return as_css(out);
}
