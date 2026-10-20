// AE.12's GPU lane: what the character shader's fragment stage reads at each
// pixel, so the character-shading spec's CPU model can shade the same inputs.
// The vertex stage is the character shader's own (same source, same defs), so
// every probe pass rasterizes and interpolates as the colour pass does.
//
// `constants::PROBE` picks what a pass writes: two f32 values, each as a
// half-float pair (hi, the value with its low 13 mantissa bits cleared, which
// a half holds exactly; lo, what's left times 4096), or a texture's sample as
// the colour pass takes it. The mask is bit arithmetic on purpose: a
// pack2x16float round trip gets folded away by Metal's compiler (ae-color's
// research viewer, renders/README.md).

import guacamole::character::shading::{
    Interpolants, character, colour_map, colour_sampler, normal_map, normal_sampler,
    spec_map, spec_sampler, mapped_normal,
};

fn split(v: f32) -> vec2<f32> {
    let hi = bitcast<f32>(bitcast<u32>(v) & 0xFFFFE000u);
    return vec2<f32>(hi, (v - hi) * 4096.0);
}

fn pair(a: f32, b: f32) -> vec4<f32> {
    return vec4<f32>(split(a), split(b));
}

@fragment
fn fragment(in: Interpolants) -> @location(0) vec4<f32> {
    let p = in.position;
    let n = in.normal;
    var t = vec3<f32>(0.0);
    var b = vec3<f32>(0.0);
@if(CHAR_NORMAL_MAP) {
    t = in.tangent;
    b = in.bitangent;
}
    var e = vec3<f32>(0.0);
@if(CHAR_EYE)
    e = in.eye;
    let uv = in.uv01.xy;
    let c = textureSample(colour_map, colour_sampler, uv);
    var nm = vec4<f32>(0.0);
@if(CHAR_NORMAL_MAP)
    nm = textureSample(normal_map, normal_sampler, uv);
    var sp = vec4<f32>(0.0);
@if(CHAR_SPEC_MAP)
    sp = textureSample(spec_map, spec_sampler, uv);
    // Where the colour pass looks the rim up, for telling a texel flip from a
    // shading miss.
    var rn = n;
@if(CHAR_NORMAL_MAP)
    rn = mapped_normal(nm, n, t, b);
    let scale = character.rim.x;
    let rim_uv = vec2<f32>((0.5 + 0.5 * rn.x) * scale, (0.5 - 0.5 * rn.y) * scale);

    let probe = constants::PROBE;
    if probe == 0 {
        return pair(p.x, p.y);
    }
    // The material's index in its model plus one, which the lane puts in
    // the uniform's spare word; 0 is the clear colour.
    if probe == 1 {
        return pair(p.z, character.misc.w);
    }
    if probe == 2 {
        return pair(n.x, n.y);
    }
    if probe == 3 {
        return pair(n.z, e.x);
    }
    if probe == 4 {
        return pair(e.y, e.z);
    }
    if probe == 5 {
        return pair(t.x, t.y);
    }
    if probe == 6 {
        return pair(t.z, b.x);
    }
    if probe == 7 {
        return pair(b.y, b.z);
    }
    if probe == 8 {
        return pair(uv.x, uv.y);
    }
    if probe == 9 {
        return pair(rim_uv.x, rim_uv.y);
    }
    if probe == 10 {
        return c;
    }
    if probe == 11 {
        return nm;
    }
    return sp;
}
