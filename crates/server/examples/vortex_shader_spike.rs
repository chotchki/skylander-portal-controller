//! Spike: GPU fragment-shader vortex.
//!
//! Goal: get the rich cloud-textured swirl from `docs/aesthetic/loading_screen.png`
//! and the project mock without prerendered video. The current production
//! vortex (`crates/server/src/vortex.rs`) uses a cheap polar-mesh
//! approximation that reads as flat. This spike drops into raw GLSL via
//! `egui::PaintCallback` + the glow backend to render domain-warped FBM
//! noise in polar+spiral coordinates, with a smoothstep iris mask for
//! the dramatic Booting/Crash close animation.
//!
//! Run with: `cargo run --example vortex_shader_spike`

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui_glow::glow::{self, HasContext};
use serde::{Deserialize, Serialize};

/// Where the spike persists its parameter state between runs.
/// `target/` is gitignored and always exists when launched via cargo,
/// so it's a safe scratch location that doesn't pollute the repo or
/// require us to create new directories.
fn state_path() -> PathBuf {
    PathBuf::from("target").join("vortex_spike_state.json")
}

/// Versioned preset directory. Files here are committed to the repo
/// so production code can `include_str!` them at compile time once
/// we wire the shader into `vortex.rs`. Save named presets via the
/// spike's Presets panel; load + tweak + re-save as the look evolves.
fn presets_dir() -> PathBuf {
    PathBuf::from("crates")
        .join("server")
        .join("src")
        .join("vortex_presets")
}

fn list_presets() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(presets_dir())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            (path.extension().and_then(|x| x.to_str()) == Some("json"))
                .then(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

fn save_preset(name: &str, params: &Params) -> std::io::Result<()> {
    let dir = presets_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(params).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(format!("{name}.json")), json)
}

fn load_preset(name: &str) -> Option<Params> {
    let path = presets_dir().join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn delete_preset(name: &str) -> std::io::Result<()> {
    std::fs::remove_file(presets_dir().join(format!("{name}.json")))
}

const VS_SRC: &str = r#"#version 140
in vec2 a_pos;
out vec2 v_uv;
void main() {
    v_uv = a_pos * 0.5 + 0.5;
    gl_Position = vec4(a_pos, 0.0, 1.0);
}
"#;

const FS_SRC: &str = r#"#version 140
precision highp float;
in vec2 v_uv;
out vec4 frag_color;

uniform vec2  u_resolution;
uniform float u_time;
uniform float u_iris_radius;
uniform float u_iris_softness;
uniform int   u_iris_mode;       // 0 = mask center (dark hole), 1 = reveal from center
uniform float u_rotation_speed;
uniform float u_inflow_speed;
uniform float u_spiral_tightness;
uniform float u_cloud_brightness;
uniform float u_cloud_bias;       // 0 = full noise; 1 = only brightest peaks become wisps
uniform float u_radial_freq;
uniform float u_angular_freq;
uniform float u_thickness;       // higher = thicker arms (samples noise more slowly)
uniform int   u_octaves;
uniform float u_persistence;
uniform float u_layer2_strength; // 0 = single layer; 1 = full counter-rotating overlay
uniform float u_streak_strength; // 0 = no streaks; 1 = strong rain-line overlay
uniform float u_streak_freq;     // streak count across the spiral (rounded to int)
uniform float u_streak_speed;    // streak scroll velocity (signed: +inward, -outward)
uniform vec3  u_color_deep;
uniform vec3  u_color_mid;
uniform vec3  u_color_wisp;
uniform float u_star_brightness;  // 0 = no stars, 1 = subtle, 2+ = punchy
uniform float u_star_density;     // cells per shorter axis (~24 reasonable)
uniform float u_iris_glow;        // brightness of the wave-front ring at iris boundary
uniform int   u_transparent;      // 0 = opaque, 1 = alpha-out the iris area

// Hash + value noise (iquilezles style, fast).
float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

float vnoise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    float total = 0.0;
    for (int i = 0; i < 8; i++) {
        if (i >= u_octaves) break;
        v += a * vnoise(p);
        total += a;
        p *= 2.0;
        a *= u_persistence;
    }
    return v / max(total, 1e-4);
}

// Procedural starfield: divide UV into a grid, hash each cell, light
// up cells whose hash lands above a threshold. Brightness varies with
// the hash spillover so denser cells get brighter stars; gentle
// twinkle modulated by per-cell phase.
float starfield(vec2 uv, float density) {
    vec2 cell = floor(uv * density);
    float h = hash(cell);
    if (h < 0.985) return 0.0;
    vec2 sub = fract(uv * density);
    vec2 star_pos = vec2(hash(cell + 11.0), hash(cell + 17.0));
    float d = length(sub - star_pos);
    float brightness = (h - 0.985) / 0.015; // 0..1 across the threshold tail
    float twinkle = 0.7 + 0.3 * sin(u_time * 1.5 + h * 100.0);
    return smoothstep(0.06, 0.0, d) * brightness * twinkle;
}

void main() {
    // Aspect-corrected centred UV, range roughly [-1, 1] on the
    // shorter axis — keeps the vortex circular regardless of window.
    //
    // **Why v_uv and not gl_FragCoord?** `gl_FragCoord` is in WINDOW
    // pixel coords (always relative to framebuffer 0,0). The vertex
    // shader emits `v_uv` in [0, 1] across the visible viewport, so
    // it's correctly viewport-relative even when the GL viewport is
    // offset (e.g. central panel sitting to the right of a side
    // panel). Using `gl_FragCoord` here would centre the vortex on
    // the WINDOW centre instead of the panel centre, producing a
    // visible seam at the side-panel boundary.
    vec2 uv = (v_uv - 0.5) * u_resolution / min(u_resolution.x, u_resolution.y);

    float radius = length(uv);
    float angle  = atan(uv.y, uv.x);

    // Spiral warp — angle gets pulled around as you move outward,
    // plus a global rotation over time. `spiral_tightness` controls
    // arm count + curl.
    float spiral_angle = angle
        + radius * u_spiral_tightness
        + u_time * u_rotation_speed;

    // Sample FBM in spiral coords. log(radius) gives a self-similar
    // zoom into the centre — same noise structure no matter how far
    // in you look. Time on the radial axis = inflow.
    //
    // **Why cos+sin and not `spiral_angle` directly?** `atan(uv.y,
    // uv.x)` returns values in [−π, π] with a discontinuous wrap at
    // the −x axis (jumps from +π to −π in adjacent pixels). Feeding
    // that into `vnoise` made the sampler see a step discontinuity
    // along the −x ray — the persistent horizontal seam Chris
    // flagged 2026-04-19 ("out of phase by a wavelength"). Mapping
    // `spiral_angle` through (cos, sin) puts the angular axis on a
    // smooth periodic circle in noise-space — the wrap point is
    // continuous because cos(π) = cos(−π), sin(π) = sin(−π). The
    // radial coord becomes a translation of that circle through
    // noise-space, so different radii still see different patches
    // (preserving the inward zoom + inflow feel).
    float r_clamped = max(radius, 0.001);
    float radial = -log(r_clamped) * u_radial_freq + u_time * u_inflow_speed;
    // Circle radius in noise-space. Multiplied by π so that one
    // revolution traces a circumference of 2π² · angular_freq ≈ 23.7
    // at the default 1.2 slider — same order of magnitude as the
    // original strip's angular extent, so the FBM sees comparable
    // detail per revolution instead of a tiny smooth loop.
    //
    // The (1 + radial · k) modulation grows the circle as you move
    // inward, so different radii trace different-sized loops → they
    // sample independent noise patterns → the spiral arms come back.
    // Without this, every radius would see a translated copy of the
    // same loop, producing the concentric-smear look.
    float circle_r = u_angular_freq * 3.14159 * (1.0 + 0.18 * radial);
    vec2 nuv1 = vec2(cos(spiral_angle), sin(spiral_angle)) * circle_r;
    // Translation along the radial axis. The magnitude (~2 units
    // per radial unit) is sized to match the circle's angular
    // gradient — at the default angular_freq the per-pixel noise
    // gradient is roughly isotropic in (angle, radius), so noise
    // features read as blobs rather than radial rays.
    //
    // Earlier versions used (0.3, 0.5) which under-walked the
    // radial axis — the angular gradient dominated by ~2.5× and
    // every blob stretched into a long thin radial streak (Chris
    // flagged 2026-04-19). This also serves the inward "drilling"
    // motion when inflow ≠ 0; a too-small magnitude there meant
    // even with inflow on, the field barely shifted radially.
    nuv1 += vec2(radial * 1.0, radial * 1.7);
    // Thickness divides the entire noise input. Larger value =
    // sample the FBM more slowly = bigger blobs = thicker arms.
    // Decoupled from angular_freq (which controls how the circle
    // wraps) and octaves (which controls fine detail) so the user
    // can dial arm width independently of arm count + detail.
    nuv1 /= max(u_thickness, 0.05);
    float n = fbm(nuv1);

    // Optional second layer: counter-rotating, slightly different
    // tightness/scale. Blended in via max() (peak-take) so the
    // brightest of either layer wins, giving an interleaved look
    // rather than washing the contrast out.
    if (u_layer2_strength > 0.001) {
        float spiral_angle_2 = -angle
            + radius * u_spiral_tightness * 1.3
            - u_time * u_rotation_speed * 1.4;
        float circle_r_2 = u_angular_freq * 3.14159 * (1.0 + 0.18 * radial) * 1.3;
        vec2 nuv2 = vec2(cos(spiral_angle_2), sin(spiral_angle_2)) * circle_r_2;
        nuv2 += vec2(radial * 0.5, radial * 0.3);
        nuv2 /= max(u_thickness, 0.05);
        float n2 = fbm(nuv2);
        n = mix(n, max(n, n2), u_layer2_strength);
    }

    // Cloud bias via smoothstep, not a hard clip. The hard-clip
    // form (max(n − bias, 0) / (1 − bias)) made any region whose
    // FBM happened to fall just below the bias disappear entirely;
    // combined with value-noise's natural clumpiness this read as
    // "one half of the spiral has clouds, the other half is bare"
    // — the asymmetry Chris flagged 2026-04-19. Smoothstep instead
    // maps [bias − 0.2, bias + 0.7] → [0, 1] so dim regions retain
    // a faint presence and structure stays continuous around the
    // full spiral, while the bias still pulls the white–blue ratio
    // toward blue at higher slider values.
    n = smoothstep(u_cloud_bias - 0.2, u_cloud_bias + 0.7, n);

    // Streak overlay: bright dashes that lie exactly on the spiral
    // arms and slide inward over time, like clouds streaming past
    // the camera.
    //
    // The earlier form `cos(spiral_angle · freq + radial · 4)` was
    // an *additive* phase combination, which tilts the streak peaks
    // away from the arm tangent by an amount proportional to 1/r —
    // the "fan shadow" look Chris flagged 2026-04-19, where streaks
    // pointed straight outward instead of following the curve.
    //
    // Multiplicative form fixes that. `across` picks which arms get
    // a streak based on spiral_angle alone, so the value is constant
    // along an entire arm and the streak follows the arm exactly.
    // `along` is an independent radial wave that scrolls inward via
    // `−time · speed`, lighting up segments at different positions
    // on each arm. Multiplying the two confines bright pixels to
    // (lit arm) × (current segment) → discrete dashes flying along
    // the arm rather than continuous radial fans.
    //
    // Round freq to int so cos(freq · spiral_angle) is fully
    // periodic across the atan2 wrap at the −x axis.
    float streak_freq_i = floor(u_streak_freq + 0.5);
    float across = cos(spiral_angle * streak_freq_i) * 0.5 + 0.5;
    across = pow(across, 2.5);
    // Bias the along trough up (·0.45 + 0.55) so the dim half of
    // the cosine still contributes ~10% streak presence after the
    // pow shaper. Without the floor, when streak_speed = 0 the
    // cosine freezes and any radius that lands in its trough loses
    // streaks entirely → the "missing annular band" look Chris
    // flagged 2026-04-19. Bright dashes are still ~10× brighter
    // than the floor, so dash motion stays clearly visible when
    // streak_speed > 0.
    float along = cos(radial * 1.2 - u_time * u_streak_speed * 4.0) * 0.45 + 0.55;
    along = pow(along, 1.5);
    float streak = across * along;
    n = mix(n, n * (0.6 + streak * 1.8), u_streak_strength);

    // Background gradient: mid blue at the rim, fading toward deep
    // navy as you approach the centre. Gives the swirl a sense of
    // depth before any clouds are drawn on top.
    // Cloud density drives ALPHA, not color mixing. The wisp tint is
    // the only color the shader contributes; the deep blue / mid blue
    // background is provided by `paint_sky_background` underneath
    // (production) or by the spike's eframe panel fill, so it shows
    // through wherever cloud density is low. Result: white wisps
    // against a blue gradient with starfield visible through gaps —
    // matches the pre-shader design (Chris 2026-04-19).
    float cloud_falloff = smoothstep(0.0, 0.6, radius);
    float cloud_density = n * u_cloud_brightness * cloud_falloff;

    // Iris mask. Two modes:
    //  0 = dark-hole mode (close transition).
    //  1 = reveal-from-centre (intro).
    float edge = smoothstep(
        u_iris_radius - u_iris_softness,
        u_iris_radius,
        radius
    );
    float iris = (u_iris_mode == 1) ? (1.0 - edge) : edge;
    float cloud_alpha = cloud_density * iris;

    // Iris glow ring — bright wave-front at the iris boundary,
    // contributes to alpha so it shines through regardless of cloud
    // density. Sells the "portal opening" feel during transitions.
    float boundary_dist = abs(radius - u_iris_radius);
    float ring = exp(-boundary_dist * 30.0 / max(u_iris_softness, 0.05));
    float glow_alpha = ring * u_iris_glow;

    float a = clamp(cloud_alpha + glow_alpha, 0.0, 1.0);

    // Premultiplied alpha (egui blend func is ONE / ONE_MINUS_SRC_ALPHA).
    vec3 col = u_color_wisp * a;
    frag_color = vec4(col, a);
}
"#;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum IrisMode {
    /// Dark-hole-grows (used for close transitions: crash, server-error, booting).
    DarkHole,
    /// Vortex-reveals-from-centre (used for the startup → idle reveal).
    Reveal,
}

/// `#[serde(default)]` at struct level: any field missing from the
/// saved JSON falls back to its value in `Params::default()`. So a
/// state file written by an older build (lacking newer knobs like
/// `streak_speed`) deserialises cleanly with sane defaults instead
/// of erroring — exactly the "variables it doesn't know yet just be
/// set to defaults" requirement.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Params {
    iris_radius: f32,
    iris_softness: f32,
    iris_mode: IrisMode,
    rotation_speed: f32,
    /// Flips the streak dash flow direction. When true the bright
    /// dashes scroll outward (clouds coming OUT of the vortex)
    /// instead of inward (clouds drawn IN). Applied as a sign
    /// multiplier on `streak_speed` at upload time so the slider
    /// can stay positive-only and the dialled-in speed magnitude
    /// is preserved across direction flips.
    streak_outward: bool,
    inflow_speed: f32,
    spiral_tightness: f32,
    cloud_brightness: f32,
    cloud_bias: f32,
    radial_freq: f32,
    angular_freq: f32,
    thickness: f32,
    octaves: i32,
    persistence: f32,
    layer2_strength: f32,
    streak_strength: f32,
    streak_freq: f32,
    streak_speed: f32,
    /// Animation clock offset, in seconds. The shader's `u_time`
    /// drives rotation, inflow, and streak motion; the same params
    /// at different `u_time` values produce visually different
    /// snapshots. This offset becomes the initial elapsed time on
    /// app launch (and on slider scrub) so a tuned look can be
    /// pinned to a known point in the animation cycle — without it,
    /// re-opening the spike would drop you at a random phase of the
    /// motion (Chris flagged 2026-04-19, "time is the hidden
    /// parameter"). Production ignores this field; the launcher's
    /// own clock starts from 0 every boot.
    time_offset: f32,
    color_deep: [f32; 3],
    color_mid: [f32; 3],
    color_wisp: [f32; 3],
    star_brightness: f32,
    star_density: f32,
    iris_glow: f32,
    transparent: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            // 0 = no iris hole (full vortex), larger = bigger dark hole.
            // ~0.35 is a good "AwaitingConnect" idle look.
            iris_radius: 0.35,
            iris_softness: 0.25,
            iris_mode: IrisMode::DarkHole,
            rotation_speed: 0.08,
            streak_outward: false,
            inflow_speed: 0.20,
            spiral_tightness: 4.0,
            cloud_brightness: 1.0,
            cloud_bias: 0.15,
            radial_freq: 1.5,
            angular_freq: 1.2,
            thickness: 1.0,
            octaves: 5,
            persistence: 0.55,
            layer2_strength: 0.0,
            streak_strength: 0.5,
            streak_freq: 8.0,
            streak_speed: 1.0,
            time_offset: 0.0,
            // Match crate palette: SF_3 / SF_1 + a near-white wisp.
            color_deep: [0.008, 0.031, 0.094], // SF_3
            color_mid: [0.043, 0.118, 0.322],  // SF_1
            color_wisp: [0.85, 0.92, 1.0],
            star_brightness: 0.9,
            star_density: 24.0,
            iris_glow: 0.4,
            transparent: true,
        }
    }
}

struct ShaderRig {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    u_resolution: Option<glow::UniformLocation>,
    u_time: Option<glow::UniformLocation>,
    u_iris_radius: Option<glow::UniformLocation>,
    u_iris_softness: Option<glow::UniformLocation>,
    u_iris_mode: Option<glow::UniformLocation>,
    u_rotation_speed: Option<glow::UniformLocation>,
    u_inflow_speed: Option<glow::UniformLocation>,
    u_spiral_tightness: Option<glow::UniformLocation>,
    u_cloud_brightness: Option<glow::UniformLocation>,
    u_cloud_bias: Option<glow::UniformLocation>,
    u_radial_freq: Option<glow::UniformLocation>,
    u_angular_freq: Option<glow::UniformLocation>,
    u_thickness: Option<glow::UniformLocation>,
    u_octaves: Option<glow::UniformLocation>,
    u_persistence: Option<glow::UniformLocation>,
    u_layer2_strength: Option<glow::UniformLocation>,
    u_streak_strength: Option<glow::UniformLocation>,
    u_streak_freq: Option<glow::UniformLocation>,
    u_streak_speed: Option<glow::UniformLocation>,
    u_color_deep: Option<glow::UniformLocation>,
    u_color_mid: Option<glow::UniformLocation>,
    u_color_wisp: Option<glow::UniformLocation>,
    u_star_brightness: Option<glow::UniformLocation>,
    u_star_density: Option<glow::UniformLocation>,
    u_iris_glow: Option<glow::UniformLocation>,
    u_transparent: Option<glow::UniformLocation>,
}

impl ShaderRig {
    fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let program = gl
                .create_program()
                .map_err(|e| format!("create_program: {e}"))?;

            let vs = compile_shader(gl, glow::VERTEX_SHADER, VS_SRC)?;
            let fs = compile_shader(gl, glow::FRAGMENT_SHADER, FS_SRC)?;
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_shader(vs);
                gl.delete_shader(fs);
                gl.delete_program(program);
                return Err(format!("program link: {log}"));
            }
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            // Real vertex buffer with explicit fullscreen-triangle
            // positions. We could use the attributeless `gl_VertexID`
            // trick (`const vec2 verts[3]` indexed in the vertex
            // shader) and skip the VBO entirely, but a real VBO is
            // the conventional path that works the same on every GL
            // driver — keeps the spike portable when we lift it into
            // production.
            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("create_vao: {e}"))?;
            let vbo = gl.create_buffer().map_err(|e| format!("create_vbo: {e}"))?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            // Fullscreen triangle covers clip-space [-1,1]² with one
            // triangle (cheaper than a 6-vertex quad). Vertices at
            // (-1,-1), (3,-1), (-1,3) — the visible viewport portion
            // is fully inside this triangle.
            let verts: [f32; 6] = [-1.0, -1.0, 3.0, -1.0, -1.0, 3.0];
            let bytes = std::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                std::mem::size_of_val(&verts),
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            let pos_loc = gl
                .get_attrib_location(program, "a_pos")
                .ok_or("missing a_pos attribute")?;
            gl.enable_vertex_attrib_array(pos_loc);
            gl.vertex_attrib_pointer_f32(pos_loc, 2, glow::FLOAT, false, 0, 0);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);

            Ok(Self {
                u_resolution: gl.get_uniform_location(program, "u_resolution"),
                u_time: gl.get_uniform_location(program, "u_time"),
                u_iris_radius: gl.get_uniform_location(program, "u_iris_radius"),
                u_iris_softness: gl.get_uniform_location(program, "u_iris_softness"),
                u_iris_mode: gl.get_uniform_location(program, "u_iris_mode"),
                u_rotation_speed: gl.get_uniform_location(program, "u_rotation_speed"),
                u_inflow_speed: gl.get_uniform_location(program, "u_inflow_speed"),
                u_spiral_tightness: gl.get_uniform_location(program, "u_spiral_tightness"),
                u_cloud_brightness: gl.get_uniform_location(program, "u_cloud_brightness"),
                u_cloud_bias: gl.get_uniform_location(program, "u_cloud_bias"),
                u_radial_freq: gl.get_uniform_location(program, "u_radial_freq"),
                u_angular_freq: gl.get_uniform_location(program, "u_angular_freq"),
                u_thickness: gl.get_uniform_location(program, "u_thickness"),
                u_octaves: gl.get_uniform_location(program, "u_octaves"),
                u_persistence: gl.get_uniform_location(program, "u_persistence"),
                u_layer2_strength: gl.get_uniform_location(program, "u_layer2_strength"),
                u_streak_strength: gl.get_uniform_location(program, "u_streak_strength"),
                u_streak_freq: gl.get_uniform_location(program, "u_streak_freq"),
                u_streak_speed: gl.get_uniform_location(program, "u_streak_speed"),
                u_color_deep: gl.get_uniform_location(program, "u_color_deep"),
                u_color_mid: gl.get_uniform_location(program, "u_color_mid"),
                u_color_wisp: gl.get_uniform_location(program, "u_color_wisp"),
                u_star_brightness: gl.get_uniform_location(program, "u_star_brightness"),
                u_star_density: gl.get_uniform_location(program, "u_star_density"),
                u_iris_glow: gl.get_uniform_location(program, "u_iris_glow"),
                u_transparent: gl.get_uniform_location(program, "u_transparent"),
                program,
                vao,
                vbo,
            })
        }
    }

    fn paint(&self, gl: &glow::Context, params: Params, time: f32, viewport_px: [i32; 4]) {
        unsafe {
            // Confine drawing to the painted region (egui's central
            // panel rect, in pixels). The shader uses v_uv (vertex-
            // emitted, viewport-relative) so the centre + scale match
            // the viewport regardless of where it sits in the window.
            gl.viewport(
                viewport_px[0],
                viewport_px[1],
                viewport_px[2],
                viewport_px[3],
            );
            // Always alpha-blend — shader outputs premultiplied RGBA
            // where alpha = cloud_density * iris (+ glow ring), so
            // the panel/sky underneath shows through dim regions.
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);

            gl.use_program(Some(self.program));
            gl.uniform_2_f32(
                self.u_resolution.as_ref(),
                viewport_px[2] as f32,
                viewport_px[3] as f32,
            );
            gl.uniform_1_f32(self.u_time.as_ref(), time);
            gl.uniform_1_f32(self.u_iris_radius.as_ref(), params.iris_radius);
            gl.uniform_1_f32(self.u_iris_softness.as_ref(), params.iris_softness);
            let mode_int = match params.iris_mode {
                IrisMode::DarkHole => 0,
                IrisMode::Reveal => 1,
            };
            gl.uniform_1_i32(self.u_iris_mode.as_ref(), mode_int);
            gl.uniform_1_f32(self.u_rotation_speed.as_ref(), params.rotation_speed);
            gl.uniform_1_f32(self.u_inflow_speed.as_ref(), params.inflow_speed);
            gl.uniform_1_f32(self.u_spiral_tightness.as_ref(), params.spiral_tightness);
            gl.uniform_1_f32(self.u_cloud_brightness.as_ref(), params.cloud_brightness);
            gl.uniform_1_f32(self.u_cloud_bias.as_ref(), params.cloud_bias);
            gl.uniform_1_f32(self.u_radial_freq.as_ref(), params.radial_freq);
            gl.uniform_1_f32(self.u_angular_freq.as_ref(), params.angular_freq);
            gl.uniform_1_f32(self.u_thickness.as_ref(), params.thickness);
            gl.uniform_1_i32(self.u_octaves.as_ref(), params.octaves);
            gl.uniform_1_f32(self.u_persistence.as_ref(), params.persistence);
            gl.uniform_1_f32(self.u_layer2_strength.as_ref(), params.layer2_strength);
            gl.uniform_1_f32(self.u_streak_strength.as_ref(), params.streak_strength);
            gl.uniform_1_f32(self.u_streak_freq.as_ref(), params.streak_freq);
            let streak_speed = if params.streak_outward {
                -params.streak_speed
            } else {
                params.streak_speed
            };
            gl.uniform_1_f32(self.u_streak_speed.as_ref(), streak_speed);
            gl.uniform_3_f32_slice(self.u_color_deep.as_ref(), &params.color_deep);
            gl.uniform_3_f32_slice(self.u_color_mid.as_ref(), &params.color_mid);
            gl.uniform_3_f32_slice(self.u_color_wisp.as_ref(), &params.color_wisp);
            gl.uniform_1_f32(self.u_star_brightness.as_ref(), params.star_brightness);
            gl.uniform_1_f32(self.u_star_density.as_ref(), params.star_density);
            gl.uniform_1_f32(self.u_iris_glow.as_ref(), params.iris_glow);
            gl.uniform_1_i32(
                self.u_transparent.as_ref(),
                if params.transparent { 1 } else { 0 },
            );

            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
        }
    }
}

unsafe fn compile_shader(gl: &glow::Context, kind: u32, src: &str) -> Result<glow::Shader, String> {
    unsafe {
        let s = gl
            .create_shader(kind)
            .map_err(|e| format!("create_shader: {e}"))?;
        gl.shader_source(s, src);
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            let log = gl.get_shader_info_log(s);
            gl.delete_shader(s);
            return Err(format!("compile: {log}"));
        }
        Ok(s)
    }
}

struct App {
    params: Params,
    /// Snapshot of last-persisted params; used to detect changes so
    /// we only write when something actually changed.
    last_saved: Params,
    /// When the most-recent param change happened. Save fires once
    /// `SAVE_DEBOUNCE` has elapsed without further changes — avoids
    /// hammering the disk while the user drags a slider.
    dirty_since: Option<std::time::Instant>,
    /// Text-input buffer for the "save preset" UI.
    preset_name: String,
    rig: Arc<Mutex<Option<ShaderRig>>>,
    started_at: std::time::Instant,
    /// Animated iris-close demo trigger. When set, iris_radius is
    /// driven over time to demonstrate the open/close transitions.
    iris_anim: Option<IrisAnim>,
}

const SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(750);

#[derive(Clone, Copy)]
enum IrisAnim {
    /// Crash-style: iris closes fast over ~1.0s.
    Close { started_at: f32 },
    /// Booting-style: iris opens slowly over ~2.5s.
    Open { started_at: f32 },
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Try to restore the previous run's params. Read failure or
        // parse error → silently fall back to defaults; the spike is
        // a dev tool and a stale/missing/corrupt state file shouldn't
        // block startup.
        let params = std::fs::read_to_string(state_path())
            .ok()
            .and_then(|s| serde_json::from_str::<Params>(&s).ok())
            .unwrap_or_default();
        let started_at = clock_at_offset(params.time_offset);
        Self {
            params,
            last_saved: params,
            dirty_since: None,
            preset_name: String::new(),
            rig: Arc::new(Mutex::new(None)),
            started_at,
            iris_anim: None,
        }
    }

    /// Reset the animation clock so the next frame's `elapsed` equals
    /// `params.time_offset`. Called on app launch, on time-offset
    /// slider edits, and whenever an iris animation is triggered so
    /// every play of the open/close lands on the same visual snapshot
    /// rather than wherever the wall-clock happens to be.
    fn reset_clock(&mut self) {
        self.started_at = clock_at_offset(self.params.time_offset);
    }

    fn save(&mut self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.params) {
            let _ = std::fs::write(state_path(), json);
        }
        self.last_saved = self.params;
        self.dirty_since = None;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // Lazy shader init on the first frame (need the gl context
        // from the eframe Frame; not available before the app starts
        // updating).
        if self.rig.lock().unwrap().is_none()
            && let Some(gl) = frame.gl()
        {
            match ShaderRig::new(gl) {
                Ok(rig) => *self.rig.lock().unwrap() = Some(rig),
                Err(e) => eprintln!("shader init failed: {e}"),
            }
        }

        let time = self.started_at.elapsed().as_secs_f32();

        // Drive the iris animation if active.
        if let Some(anim) = self.iris_anim {
            match anim {
                IrisAnim::Close { started_at } => {
                    let t = ((time - started_at) / 1.0).clamp(0.0, 1.0);
                    let eased = ease_in_quad(t);
                    self.params.iris_radius = lerp(0.0, 1.4, eased);
                    if t >= 1.0 {
                        self.iris_anim = None;
                    }
                }
                IrisAnim::Open { started_at } => {
                    let t = ((time - started_at) / 2.5).clamp(0.0, 1.0);
                    let eased = ease_out_cubic(t);
                    self.params.iris_radius = lerp(1.4, 0.35, eased);
                    if t >= 1.0 {
                        self.iris_anim = None;
                    }
                }
            }
        }

        egui::SidePanel::left("controls")
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading("Vortex Shader Spike");
                ui.separator();

                ui.collapsing("Presets", |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.preset_name)
                                .hint_text("name (e.g. idle, booting)")
                                .desired_width(160.0),
                        );
                        let name = self.preset_name.trim().to_string();
                        let can_save = !name.is_empty();
                        if ui
                            .add_enabled(can_save, egui::Button::new("Save"))
                            .clicked()
                        {
                            let _ = save_preset(&name, &self.params);
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            for name in list_presets() {
                                ui.horizontal(|ui| {
                                    if ui.button("Load").clicked()
                                        && let Some(p) = load_preset(&name)
                                    {
                                        self.params = p;
                                        self.preset_name = name.clone();
                                    }
                                    if ui.button("Del").clicked() {
                                        let _ = delete_preset(&name);
                                    }
                                    ui.label(&name);
                                });
                            }
                        });
                });
                ui.separator();

                ui.label("Iris");
                ui.add(egui::Slider::new(&mut self.params.iris_radius, 0.0..=1.5).text("radius"));
                ui.add(
                    egui::Slider::new(&mut self.params.iris_softness, 0.0..=1.0).text("softness"),
                );
                ui.add(egui::Slider::new(&mut self.params.iris_glow, 0.0..=1.5).text("glow ring"));
                ui.horizontal(|ui| {
                    ui.label("mode");
                    ui.radio_value(&mut self.params.iris_mode, IrisMode::DarkHole, "dark hole");
                    ui.radio_value(&mut self.params.iris_mode, IrisMode::Reveal, "reveal");
                });
                ui.horizontal(|ui| {
                    if ui.button("Open (2.5s)").clicked() {
                        // Reset the animation clock so the open
                        // animation always plays from the same noise
                        // snapshot (the configured time_offset),
                        // regardless of how long the spike's been
                        // running. Iris-anim's `started_at` is the
                        // post-reset elapsed value (= time_offset).
                        self.reset_clock();
                        self.iris_anim = Some(IrisAnim::Open {
                            started_at: self.params.time_offset,
                        });
                    }
                    if ui.button("Close (1.0s)").clicked() {
                        self.reset_clock();
                        self.iris_anim = Some(IrisAnim::Close {
                            started_at: self.params.time_offset,
                        });
                    }
                });
                ui.separator();

                ui.label("Motion");
                ui.add(
                    egui::Slider::new(&mut self.params.rotation_speed, -1.0..=1.0).text("rotation"),
                );
                ui.add(egui::Slider::new(&mut self.params.inflow_speed, -2.0..=2.0).text("inflow"));
                ui.add(
                    egui::Slider::new(&mut self.params.spiral_tightness, 0.0..=12.0)
                        .text("spiral tightness"),
                );
                // Time offset (seconds). Scrubs the animation clock —
                // the same params at different `u_time` values look
                // visually different because rotation / inflow /
                // streaks all advect the noise field. Pinning a
                // value here makes the saved look reproducible:
                // every app launch (and every iris Open/Close) starts
                // the clock at exactly this offset.
                let prev_offset = self.params.time_offset;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.params.time_offset)
                            .speed(0.1)
                            .min_decimals(1)
                            .max_decimals(2),
                    );
                    ui.label("time offset (s)");
                });
                if (self.params.time_offset - prev_offset).abs() > 1e-3 {
                    self.reset_clock();
                    self.iris_anim = None;
                }
                ui.separator();

                ui.label("Noise");
                ui.add(
                    egui::Slider::new(&mut self.params.cloud_brightness, 0.0..=2.5)
                        .text("cloud brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.cloud_bias, 0.0..=0.9)
                        .text("cloud bias (less white)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.radial_freq, 0.1..=4.0).text("radial freq"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.angular_freq, 0.1..=4.0)
                        .text("angular freq"),
                );
                ui.add(egui::Slider::new(&mut self.params.thickness, 0.2..=4.0).text("thickness"));
                ui.add(egui::Slider::new(&mut self.params.octaves, 1..=8).text("octaves"));
                ui.add(
                    egui::Slider::new(&mut self.params.persistence, 0.2..=0.8).text("persistence"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.layer2_strength, 0.0..=1.0)
                        .text("layer 2 (counter-spiral)"),
                );
                ui.separator();

                ui.label("Streaks (clouds flying past)");
                ui.add(
                    egui::Slider::new(&mut self.params.streak_strength, 0.0..=1.0).text("strength"),
                );
                ui.add(egui::Slider::new(&mut self.params.streak_freq, 2.0..=20.0).text("count"));
                ui.add(egui::Slider::new(&mut self.params.streak_speed, 0.0..=4.0).text("speed"));
                ui.checkbox(
                    &mut self.params.streak_outward,
                    "flow outward (clouds out vs in)",
                );
                ui.separator();

                ui.label("Colors");
                ui.horizontal(|ui| {
                    ui.label("deep");
                    ui.color_edit_button_rgb(&mut self.params.color_deep);
                });
                ui.horizontal(|ui| {
                    ui.label("mid");
                    ui.color_edit_button_rgb(&mut self.params.color_mid);
                });
                ui.horizontal(|ui| {
                    ui.label("wisp");
                    ui.color_edit_button_rgb(&mut self.params.color_wisp);
                });
                ui.separator();

                ui.label("Stars");
                ui.add(
                    egui::Slider::new(&mut self.params.star_brightness, 0.0..=2.0)
                        .text("brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut self.params.star_density, 8.0..=64.0).text("density"),
                );
                ui.separator();

                ui.checkbox(
                    &mut self.params.transparent,
                    "transparent iris (in-game demo)",
                );
                ui.label(
                    egui::RichText::new(
                        "When on, the iris area becomes alpha=0 — desktop \
                         behind the window shows through. Use with iris radius \
                         animated open to demo the in-game reveal.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();

                if ui.button("Reset to defaults").clicked() {
                    self.params = Params::default();
                }
            });

        // Detect param changes and debounce a write. We compare
        // against the last-saved snapshot instead of frame-to-frame
        // so a quick wiggle that returns to the same value doesn't
        // spuriously trigger a write. While an iris animation is
        // running, exclude iris_radius from the comparison so we
        // don't queue a save every frame of the open/close playback;
        // when the user is manually dragging the slider the field
        // is included normally.
        let mut compare = self.params;
        if self.iris_anim.is_some() {
            compare.iris_radius = self.last_saved.iris_radius;
        }
        if compare != self.last_saved {
            self.dirty_since = Some(std::time::Instant::now());
            self.last_saved = compare;
        }
        if let Some(t) = self.dirty_since
            && t.elapsed() >= SAVE_DEBOUNCE
            && self.iris_anim.is_none()
        {
            self.save();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let pixels_per_point = ctx.pixels_per_point();
                let rig = self.rig.clone();
                let params = self.params;

                let cb = egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                        // viewport_in_pixels gives [x, y, w, h] in
                        // physical pixels with origin bottom-left
                        // (GL convention) — exactly what gl.viewport
                        // wants.
                        let vp = info.viewport_in_pixels();
                        let viewport_px =
                            [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
                        if let Some(rig) = rig.lock().unwrap().as_ref() {
                            rig.paint(painter.gl(), params, time, viewport_px);
                        }
                    })),
                };
                let _ = pixels_per_point;
                ui.painter().add(cb);
            });
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        // Final save — catches any pending dirty state and the
        // iris_radius value if the user closed mid-animation.
        self.save();
        if let (Some(gl), Some(rig)) = (gl, self.rig.lock().unwrap().as_ref()) {
            rig.destroy(gl);
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Compute an `Instant` such that `Instant::now() - returned` equals
/// `offset_s` (clamped non-negative). Used to seed the animation
/// clock so the next-frame's `elapsed` value matches the saved
/// time offset — both at app launch and whenever an animation is
/// re-triggered (Chris 2026-04-19, "the offset should be reused
/// whenever the animation is triggered").
fn clock_at_offset(offset_s: f32) -> std::time::Instant {
    let secs = offset_s.max(0.0);
    std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs_f32(secs))
        .unwrap_or_else(std::time::Instant::now)
}

fn ease_in_quad(t: f32) -> f32 {
    t * t
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vortex Shader Spike")
            .with_inner_size([1600.0, 1000.0])
            // Transparent so the "transparent iris" toggle can show
            // the desktop through the open iris — same machinery the
            // launcher's in-game transition will use to reveal RPCS3.
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "Vortex Shader Spike",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
