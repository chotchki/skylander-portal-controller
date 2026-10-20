// The character-shading spec (openspec AE, specs/guacamole/character-shading)
// as one shader: each `@if` is one of tfbmulti2's feature bits, set per
// material key by `material.rs`'s specialize (`key.rs` names the defs). The
// remaster works it in linear light: the disc's colours arrive decoded (sRGB
// texture views, colours converted on the CPU), tints and vertex colours as
// stored, and nothing is clamped on the way out.
//
// Lighting is in view space: the eye direction and the rim lookup are
// view-space by construction, and a turntable's rig is fixed to its camera.
// Every vector the uniform holds is view space, but for a level's lights
// (AG.21): with `misc.w` 1 the directional lights' travels and the point
// lights' positions are the world's, turned into view space by the view
// drawn for, so a camera that moves rewrites no material.
//
// WESL flattens a block an `@if` holds into the block around it, so each
// such block's names are its own.

import bevy_pbr::render::mesh_view_bindings::view;
import bevy_pbr::render::mesh_functions;
import bevy_pbr::render::skinning;

struct Character {
    light_travel: array<vec4<f32>, 3>,
    light_ambient: array<vec4<f32>, 3>,
    light_diffuse: array<vec4<f32>, 3>,
    light_specular: array<vec4<f32>, 3>,
    point_position: array<vec4<f32>, 3>,
    point_colour: array<vec4<f32>, 3>,
    // x over (d + 0.001), y over (d + 0.001)^2.
    point_falloff: array<vec4<f32>, 3>,
    fog_colour: vec4<f32>,
    // x near, y far.
    fog_range: vec4<f32>,
    placement_tint: vec4<f32>,
    // RGB and its amount.
    replace: vec4<f32>,
    shadow_u: vec4<f32>,
    shadow_v: vec4<f32>,
    // x the shadow mode, y the alpha test's reference.
    misc: vec4<f32>,
    // x layer 1's fixed alpha, y layer 2's, z layer 3's strength, w the rim's.
    fixed_alpha: vec4<f32>,
    // x the spec exponent, y the detail weight.
    layer1: vec4<f32>,
    layer2: vec4<f32>,
    // Over, add and multiply.
    layer2_weights: vec4<f32>,
    layer3_weights: vec4<f32>,
    rim_weights: vec4<f32>,
    // x the lookup's scale, y the colour/alpha split.
    rim: vec4<f32>,
    material_tint: vec4<f32>,
    uv_u: array<vec4<f32>, 3>,
    uv_v: array<vec4<f32>, 3>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> character: Character;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var colour_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var colour_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var normal_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var spec_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var spec_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var second_colour_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var second_colour_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var second_normal_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var second_normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var second_spec_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var second_spec_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(13) var third_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(14) var third_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(15) var rim_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(16) var rim_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(17) var shadow_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(18) var shadow_sampler: sampler;

// What a part without vertex colours takes in their place.
const HALF_GREY: vec4<f32> = vec4<f32>(0.5, 0.5, 0.5, 1.0);
// Specular's floor, under the exponent.
const SPECULAR_FLOOR: f32 = 0.00001;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @if(CHAR_UV1)
    @location(3) uv1: vec2<f32>,
    @if(CHAR_NORMAL_MAP)
    @location(4) tangent: vec4<f32>,
    @if(CHAR_VERTEX_COLOUR)
    @location(5) colour: vec4<f32>,
    @if(SKINNED)
    @location(6) joint_indices: vec4<u32>,
    @if(SKINNED)
    @location(7) joint_weights: vec4<f32>,
    @if(CHAR_UV2)
    @location(8) uv2: vec2<f32>,
}

// What the vertex stage interpolates to a pixel: only what the spec reads.
struct Interpolants {
    @builtin(position) clip_position: vec4<f32>,
    // View space.
    @location(0) position: vec3<f32>,
    // Unit length at each vertex; the fragment stage takes it as interpolated.
    @location(1) normal: vec3<f32>,
    // Sets 0 and 1 after their transforms.
    @location(2) uv01: vec4<f32>,
    @if(CHAR_NORMAL_MAP)
    @location(3) tangent: vec3<f32>,
    @if(CHAR_NORMAL_MAP)
    @location(4) bitangent: vec3<f32>,
    @if(CHAR_VERTEX_COLOUR)
    @location(5) colour: vec4<f32>,
    @if(CHAR_EYE)
    @location(6) eye: vec3<f32>,
    @if(CHAR_UV2)
    @location(7) uv2: vec2<f32>,
}

// A texture-coordinate set through its own 2x3 transform, numbered as the set.
fn uv_transform(index: u32, uv: vec2<f32>) -> vec2<f32> {
    let p = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(character.uv_u[index].xyz, p), dot(character.uv_v[index].xyz, p));
}

@vertex
fn vertex(v: Vertex) -> Interpolants {
    var out: Interpolants;
@if(SKINNED)
    let world_from_local = skinning::skin_model(v.joint_indices, v.joint_weights, v.instance_index);
@else
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);

    let world = world_from_local * vec4<f32>(v.position, 1.0);
    out.clip_position = view.clip_from_world * world;
    out.position = (view.view_from_world * world).xyz;

@if(SKINNED)
    let world_normal = skinning::skin_normals(world_from_local, v.normal);
@else
    let world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
    out.normal = normalize((view.view_from_world * vec4<f32>(world_normal, 0.0)).xyz);

@if(CHAR_NORMAL_MAP) {
    let world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        v.tangent,
        v.instance_index,
    );
    out.tangent = normalize((view.view_from_world * vec4<f32>(world_tangent.xyz, 0.0)).xyz);
    out.bitangent = cross(out.normal, out.tangent) * world_tangent.w;
}

    var uv1 = vec2<f32>(0.0);
@if(CHAR_UV1)
    uv1 = uv_transform(1u, v.uv1);
    out.uv01 = vec4<f32>(uv_transform(0u, v.uv0), uv1);
@if(CHAR_UV2)
    out.uv2 = uv_transform(2u, v.uv2);
@if(CHAR_VERTEX_COLOUR)
    out.colour = v.colour;
    // Toward the eye, unit length at each vertex; only with a spec map and
    // fog or a point light on (no eye direction, no specular).
@if(CHAR_EYE)
    out.eye = normalize(-out.position);
    return out;
}

// a + (b - a) t: the spec's mixes, in the order the CPU model computes them.
fn lerp3(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return a + (b - a) * t;
}

// A normal map's normal: X from alpha against the tangent, Y from green
// along the bitangent, Z from both, over the frame as interpolated.
fn mapped_normal(texel: vec4<f32>, normal: vec3<f32>, tangent: vec3<f32>, bitangent: vec3<f32>) -> vec3<f32> {
    let x = 2.0 * texel.a - 1.0;
    let y = 2.0 * texel.g - 1.0;
    let z = sqrt(abs(1.0 - x * x - y * y));
    return normalize(y * bitangent - x * tangent + z * normal);
}

// Directional light `i`'s travel in view space ([`light_travel`] and
// [`point_position`] read the uniform's frame off `misc.w`).
fn light_travel(i: u32) -> vec3<f32> {
    let travel = character.light_travel[i].xyz;
    if character.misc.w > 0.5 {
        return (view.view_from_world * vec4<f32>(travel, 0.0)).xyz;
    }
    return travel;
}

// Point light `j`'s position in view space.
fn point_position(j: u32) -> vec3<f32> {
    let at = character.point_position[j].xyz;
    if character.misc.w > 0.5 {
        return (view.view_from_world * vec4<f32>(at, 1.0)).xyz;
    }
    return at;
}

// The dot with the direction toward the light, the reverse of its travel.
fn facing(n: vec3<f32>, travel: vec3<f32>) -> f32 {
    return -dot(n, travel);
}

// A layer's alpha: the colour map's, ramping to the fixed alpha from -0.001.
fn layer_alpha(colour_alpha: f32, fixed_alpha: f32) -> f32 {
    let k = clamp((fixed_alpha + 0.001) * 10000.0, 0.0, 1.0);
    return colour_alpha + k * (fixed_alpha - colour_alpha);
}

// One layer's lighting and specular. `mapped` and `spec` are its normal and
// spec maps' samples (ignored without those features), `layer` its exponent
// and detail weight, `g` the geometric normal as interpolated.
fn lit_layer(
    c: vec4<f32>,
    mapped: vec3<f32>,
    spec: vec3<f32>,
    layer: vec4<f32>,
    g: vec3<f32>,
    vertex_colour: vec4<f32>,
    eye: vec3<f32>,
) -> vec3<f32> {
    var n = g;
@if(CHAR_NORMAL_MAP)
    n = mapped;

    var base = c.rgb;
@if(CHAR_DIRECTIONAL) {
    var lights = vec3<f32>(0.0);
    for (var i = 0u; i < 3u; i++) {
        let diffuse = character.light_diffuse[i].rgb * max(facing(n, light_travel(i)), 0.0);
        lights += character.light_ambient[i].rgb + diffuse;
    }
    base = 2.0 * (c.rgb * lights);
}
    // Unlit parts show the colour map at full strength, plus what the normal
    // map turns toward or away from each light.
@if(!CHAR_DIRECTIONAL && CHAR_NORMAL_MAP) {
    var detail = vec3<f32>(0.0);
    for (var i = 0u; i < 3u; i++) {
        let travel = light_travel(i);
        let difference = max(facing(mapped, travel), 0.0) - max(facing(g, travel), 0.0);
        detail += character.light_diffuse[i].rgb * difference;
    }
    base = c.rgb + detail * layer.y;
}
    var lit = 2.0 * (base * vertex_colour.rgb);

    // Specular after the vertex colour, so it doesn't scale it.
@if(CHAR_SPEC_MAP) {
    var sheen = vec3<f32>(0.0);
    for (var i = 0u; i < 3u; i++) {
        let travel = light_travel(i);
        var r = travel;
@if(CHAR_NORMAL_MAP || CHAR_GEOMETRIC_NORMAL)
        r = normalize(travel - n * (2.0 * dot(travel, n)));
        let s = pow(max(dot(eye, r), SPECULAR_FLOOR), layer.x);
        sheen += character.light_specular[i].rgb * s;
    }
    lit += spec * sheen;
}
    return lit;
}

// Point light `j`: its colour by the normal's facing to it and its falloff,
// unscaled by the colour map.
fn point_light(j: u32, n: vec3<f32>, position: vec3<f32>) -> vec3<f32> {
    let to = point_position(j) - position;
    let d = length(to);
    let f = max(dot(n, to) / d, 0.0);
    let de = d + 0.001;
    let falloff = character.point_falloff[j].x / de + character.point_falloff[j].y / (de * de);
    return character.point_colour[j].rgb * (f * falloff);
}

@fragment
fn fragment(in: Interpolants) -> @location(0) vec4<f32> {
    let g = in.normal;
    var vertex_colour = HALF_GREY;
@if(CHAR_VERTEX_COLOUR)
    vertex_colour = in.colour;
    // Each component clamped at zero.
    var eye = vec3<f32>(0.0);
@if(CHAR_EYE)
    eye = max(in.eye, vec3<f32>(0.0));

    // The first layer.
    let uv0 = in.uv01.xy;
    let c1 = textureSample(colour_map, colour_sampler, uv0);
    var mapped1 = vec3<f32>(0.0);
@if(CHAR_NORMAL_MAP)
    mapped1 = mapped_normal(textureSample(normal_map, normal_sampler, uv0), g, in.tangent, in.bitangent);
    var spec1 = vec3<f32>(0.0);
@if(CHAR_SPEC_MAP)
    spec1 = textureSample(spec_map, spec_sampler, uv0).rgb;
    var lit = lit_layer(c1, mapped1, spec1, character.layer1, g, vertex_colour, eye);

    // A second layer, lit as the first, blended by its coverage.
@if(CHAR_LAYER2) {
    let uv1 = in.uv01.zw;
    let c2 = textureSample(second_colour_map, second_colour_sampler, uv1);
    var mapped2 = vec3<f32>(0.0);
@if(CHAR_NORMAL_MAP)
    mapped2 = mapped_normal(textureSample(second_normal_map, second_normal_sampler, uv1), g, in.tangent, in.bitangent);
    var spec2 = vec3<f32>(0.0);
@if(CHAR_SPEC_MAP)
    spec2 = textureSample(second_spec_map, second_spec_sampler, uv1).rgb;
    let lit2 = lit_layer(c2, mapped2, spec2, character.layer2, g, vertex_colour, eye);
    let coverage = vertex_colour.a * layer_alpha(c2.a, character.fixed_alpha.y) * character.material_tint.a;
    let covered = lit2 * coverage;
    let rest = lit * (1.0 - coverage);
    let w2 = character.layer2_weights;
    lit = w2.x * (covered + rest) + w2.y * (lit + covered) + w2.z * (lit * covered + rest);
}

    // Tints: four times the placement's and the material's, neutral at 128.
    let tint = 4.0 * character.placement_tint.rgb * character.material_tint.rgb;
    var colour = lit * tint;
    var alpha = layer_alpha(c1.a, character.fixed_alpha.x) * vertex_colour.a * character.placement_tint.a * character.material_tint.a;

    // A third layer, after the tints; alpha stays. It reads the second set
    // when there's no second layer.
@if(CHAR_LAYER3) {
    var uv3 = in.uv01.zw;
@if(CHAR_UV2)
    uv3 = in.uv2;
    let strength3 = character.fixed_alpha.z;
    let third = textureSample(third_map, third_sampler, uv3).rgb * strength3;
    let w3 = character.layer3_weights;
    colour = w3.x * (third + colour * (1.0 - strength3)) + w3.y * (colour + third) + w3.z * (colour * third);
}

@if(CHAR_SHADOWS) {
    let at = vec4<f32>(in.position, 1.0);
    let shadow = textureSample(shadow_map, shadow_sampler, vec2<f32>(dot(at, character.shadow_u), dot(at, character.shadow_v)));
    let mode = character.misc.x;
    colour = colour * 2.0 * (vec3<f32>(shadow.a) + (2.0 - mode) * (shadow.rgb - vec3<f32>(shadow.a)));
}

    // The rim and point lights take the mapped normal, else the geometric.
    var n = g;
@if(CHAR_NORMAL_MAP)
    n = mapped1;

@if(CHAR_RIM) {
    let rim_scale = character.rim.x;
    let split = character.rim.y;
    let rim_strength = character.fixed_alpha.w;
    let rim_uv = vec2<f32>((0.5 + 0.5 * n.x) * rim_scale, (0.5 - 0.5 * n.y) * rim_scale);
    let rim_sample = textureSample(rim_map, rim_sampler, rim_uv);
    let under = vec4<f32>(colour, alpha);
    let w4 = character.rim_weights;
    var blended = vec4<f32>(0.0);
@if(!CHAR_GLOSS_MAP) {
    let rim = rim_sample * rim_strength;
    blended = w4.x * (rim + under * (1.0 - rim_strength)) + w4.y * (under + rim) + w4.z * (under * rim);
}
    // A gloss map (AG.22): the third layer's texture, at its set, read as
    // stored; its red times the gloss strength is the share of the rim's
    // strength each pixel takes, and k the strength it leaves.
@if(CHAR_GLOSS_MAP) {
    var gloss_uv = in.uv01.zw;
@if(CHAR_UV2)
    gloss_uv = in.uv2;
    let gloss = textureSample(third_map, third_sampler, gloss_uv).r * character.fixed_alpha.z;
    let glossed = rim_sample * (gloss * rim_strength);
    let k = rim_strength * (1.0 - gloss);
    blended = w4.x * (under * k + glossed) + w4.y * (under + glossed) + w4.z * (under * (k / 2.0 + glossed));
}
    colour = lerp3(colour, blended.rgb, 1.0 - split);
    alpha = alpha + (blended.a - alpha) * split;
}

@if(CHAR_POINT_LIGHTS_1)
    colour += point_light(0u, n, in.position);
@if(CHAR_POINT_LIGHTS_2)
    colour += point_light(1u, n, in.position);
@if(CHAR_POINT_LIGHTS_3)
    colour += point_light(2u, n, in.position);

    // The replace colour by its amount, then fog by depth along the view.
    colour = lerp3(colour, character.replace.rgb, character.replace.a);
@if(CHAR_FOG) {
    let depth = -in.position.z;
    let range = character.fog_range;
    let fog = clamp((depth - range.x) / (range.y - range.x), 0.0, 1.0);
    colour = lerp3(colour, character.fog_colour.rgb, fog);
}

    // An alpha test passes only above its reference.
@if(CHAR_ALPHA_TEST) {
    if alpha <= character.misc.y {
        discard;
    }
}
    return vec4<f32>(colour, alpha);
}

// The glow pass (AE.11): the part's glow alpha, layer 1's alpha switched
// hard at a fixed alpha of 0 (not the colour pass's ramp) times the vertex
// and both tints' alphas, and its mask: the glow amount, that alpha when the
// amount is -1, nothing at 0. The alpha test runs on the glow alpha; the
// mask goes in red, green and blue, the glow alpha in alpha, as the
// original's glow shader writes them.
@fragment
fn glow_mask(in: Interpolants) -> @location(0) vec4<f32> {
    var vertex_colour = HALF_GREY;
@if(CHAR_VERTEX_COLOUR)
    vertex_colour = in.colour;
    var first = textureSample(colour_map, colour_sampler, in.uv01.xy).a;
    if character.fixed_alpha.x >= 0.0 {
        first = character.fixed_alpha.x;
    }
    let glow_alpha = first * vertex_colour.a * character.placement_tint.a * character.material_tint.a;
    var mask = character.misc.z;
    if mask < 0.0 {
        mask = glow_alpha;
    }
@if(CHAR_ALPHA_TEST) {
    if glow_alpha <= character.misc.y {
        discard;
    }
}
    return vec4<f32>(mask, mask, mask, glow_alpha);
}
