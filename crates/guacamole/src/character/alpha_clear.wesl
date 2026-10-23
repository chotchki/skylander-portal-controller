// A level frame's alpha cleared to none, colour untouched (openspec AH.65,
// `glow.rs`'s `AlphaClear`): one triangle whose corners cover clip space,
// drawn at every depth, writing alpha alone. What the level's glow masks
// then write into, for the glow chain to read.

struct Vertex {
    @location(0) position: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> @builtin(position) vec4<f32> {
    return vec4<f32>(v.position.xy, 0.0, 1.0);
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
