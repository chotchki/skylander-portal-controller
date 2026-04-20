//! GPU vortex shader (PLAN 4.19.6).
//!
//! Replaces the earlier polar-mesh approximation with a real fragment
//! shader. Domain-warped FBM in spiral+log-radial coords, with a
//! streak overlay for "clouds flying past" motion and an iris mask
//! the launch-phase machinery uses for intro reveal + close-to-
//! in-game transitions.
//!
//! The shader was developed in `examples/vortex_shader_spike.rs` —
//! that file remains as the iteration playground (live sliders, save/
//! load presets). This module is the production embedding: same
//! shader source, same `Params` shape, with the canonical "look"
//! baked into a JSON preset committed at `vortex_presets/idle.json`.
//! Re-tune in the spike → save preset → rebuild to update the
//! production look.
//!
//! The non-shader parts (sky background, starfield, helper paint
//! primitives) are unchanged from the polar-mesh era.

use std::sync::{Arc, Mutex};

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Pos2, Rect};
use egui_glow::glow::{self, HasContext};
use serde::{Deserialize, Serialize};

use crate::palette;

/// Which side of the iris boundary is opaque. The intro reveal grows
/// the visible region (Reveal); the in-game close grows a dark hole
/// that pushes the vortex outward (DarkHole).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrisMode {
    /// `iris_radius` is the INNER edge of a dark hole. Dark inside the
    /// radius, clouds visible outside. Used during the close-to-in-game
    /// transition.
    DarkHole,
    /// `iris_radius` is the OUTER edge of cloud visibility. Clouds
    /// visible inside the radius, dark outside. Used for the intro
    /// reveal (Startup → AwaitingConnect).
    Reveal,
}

/// Full parameter set for the vortex shader. Mirrors the spike's
/// `Params` exactly so JSON presets serialise/deserialise across the
/// two contexts. `#[serde(default)]` at struct level lets older preset
/// files deserialise cleanly when new fields are added — unknown
/// fields fall back to `VortexParams::default()`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct VortexParams {
    pub iris_radius: f32,
    pub iris_softness: f32,
    pub iris_mode: IrisMode,
    pub rotation_speed: f32,
    pub streak_outward: bool,
    pub inflow_speed: f32,
    pub spiral_tightness: f32,
    pub cloud_brightness: f32,
    pub cloud_bias: f32,
    pub radial_freq: f32,
    pub angular_freq: f32,
    pub thickness: f32,
    pub octaves: i32,
    pub persistence: f32,
    pub layer2_strength: f32,
    pub streak_strength: f32,
    pub streak_freq: f32,
    pub streak_speed: f32,
    /// Animation clock offset, in seconds. Added to the launcher's
    /// elapsed time when feeding `u_time` to the shader. Lets the
    /// preset pin a known starting point in the noise/motion cycle
    /// so the boot-up frame matches what was tuned in the spike —
    /// without it, every launcher start shows a different snapshot
    /// because rotation / inflow / streak motion all advect the
    /// noise field over time. Saved by the spike (see "time offset
    /// (s)" in its Motion section).
    pub time_offset: f32,
    pub color_deep: [f32; 3],
    pub color_mid: [f32; 3],
    pub color_wisp: [f32; 3],
    pub star_brightness: f32,
    pub star_density: f32,
    pub iris_glow: f32,
    pub transparent: bool,
}

impl Default for VortexParams {
    fn default() -> Self {
        // Conservative defaults matching the spike's `Default` impl.
        // Production normally uses `idle_params()` (loaded from the
        // preset JSON); this Default serves as a fallback if the
        // preset fails to parse.
        Self {
            iris_radius: 1.5,
            iris_softness: 0.25,
            iris_mode: IrisMode::Reveal,
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
            color_deep: [0.008, 0.031, 0.094], // SF_3
            color_mid: [0.043, 0.118, 0.322],  // SF_1
            color_wisp: [0.85, 0.92, 1.0],
            star_brightness: 0.9,
            star_density: 24.0,
            iris_glow: 0.4,
            transparent: false,
        }
    }
}

/// Production vortex preset, bundled into the binary at compile time.
/// Update by re-saving from the spike's Presets panel and rebuilding.
const IDLE_PRESET_JSON: &str = include_str!("vortex_presets/idle.json");

/// Load the canonical "idle" vortex look from the bundled preset JSON.
/// Falls back to `VortexParams::default()` if the file is malformed —
/// the launcher should still boot rather than panic over a bad preset.
pub fn idle_params() -> VortexParams {
    serde_json::from_str(IDLE_PRESET_JSON).unwrap_or_else(|err| {
        tracing::warn!("vortex preset parse failed, using defaults: {err}");
        VortexParams::default()
    })
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
uniform float u_cloud_bias;
uniform float u_radial_freq;
uniform float u_angular_freq;
uniform float u_thickness;
uniform int   u_octaves;
uniform float u_persistence;
uniform float u_layer2_strength;
uniform float u_streak_strength;
uniform float u_streak_freq;
uniform float u_streak_speed;
uniform vec3  u_color_deep;
uniform vec3  u_color_mid;
uniform vec3  u_color_wisp;
uniform float u_star_brightness;
uniform float u_star_density;
uniform float u_iris_glow;
uniform int   u_transparent;

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

float starfield(vec2 uv, float density) {
    vec2 cell = floor(uv * density);
    float h = hash(cell);
    if (h < 0.985) return 0.0;
    vec2 sub = fract(uv * density);
    vec2 star_pos = vec2(hash(cell + 11.0), hash(cell + 17.0));
    float d = length(sub - star_pos);
    float brightness = (h - 0.985) / 0.015;
    float twinkle = 0.7 + 0.3 * sin(u_time * 1.5 + h * 100.0);
    return smoothstep(0.06, 0.0, d) * brightness * twinkle;
}

void main() {
    vec2 uv = (v_uv - 0.5) * u_resolution / min(u_resolution.x, u_resolution.y);
    float radius = length(uv);
    float angle  = atan(uv.y, uv.x);

    float spiral_angle = angle
        + radius * u_spiral_tightness
        + u_time * u_rotation_speed;

    float r_clamped = max(radius, 0.001);
    float radial = -log(r_clamped) * u_radial_freq + u_time * u_inflow_speed;
    // cos+sin parametrisation avoids the atan2 wrap discontinuity at
    // the −x axis. Translation magnitude (~2 per radial unit) gives
    // roughly isotropic noise gradient so features read as blobs
    // rather than radial rays — see spike comments for the analysis.
    float circle_r = u_angular_freq * 3.14159 * (1.0 + 0.18 * radial);
    vec2 nuv1 = vec2(cos(spiral_angle), sin(spiral_angle)) * circle_r;
    nuv1 += vec2(radial * 1.0, radial * 1.7);
    nuv1 /= max(u_thickness, 0.05);
    float n = fbm(nuv1);

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

    // Soft cloud bias — see spike comments for why smoothstep over
    // hard clip (the latter produced "missing arm" asymmetry).
    n = smoothstep(u_cloud_bias - 0.2, u_cloud_bias + 0.7, n);

    // Streak overlay: multiplicative across × along, so streaks lie
    // exactly on the spiral arms and bright dashes scroll along them.
    float streak_freq_i = floor(u_streak_freq + 0.5);
    float across = cos(spiral_angle * streak_freq_i) * 0.5 + 0.5;
    across = pow(across, 2.5);
    float along = cos(radial * 1.2 - u_time * u_streak_speed * 4.0) * 0.45 + 0.55;
    along = pow(along, 1.5);
    float streak = across * along;
    n = mix(n, n * (0.6 + streak * 1.8), u_streak_strength);

    // Cloud density drives ALPHA, not color mixing. The wisp tint is
    // the only color the shader contributes; the deep blue / mid blue
    // background that used to live in the shader is now provided by
    // `paint_sky_background` underneath, so it shows through wherever
    // the cloud density is low. Result: white clouds against the
    // production sky gradient, with starfield visible through gaps —
    // matches the pre-shader design (Chris 2026-04-19).
    float cloud_falloff = smoothstep(0.0, 0.6, radius);
    float cloud_density = n * u_cloud_brightness * cloud_falloff;

    float edge = smoothstep(
        u_iris_radius - u_iris_softness,
        u_iris_radius,
        radius
    );
    float iris = (u_iris_mode == 1) ? (1.0 - edge) : edge;
    float cloud_alpha = cloud_density * iris;

    // Iris glow ring — bright wave-front at the iris boundary,
    // contributes to alpha so it shines through regardless of the
    // local cloud density. Used by the close transition's portal-
    // opening pulse.
    float boundary_dist = abs(radius - u_iris_radius);
    float ring = exp(-boundary_dist * 30.0 / max(u_iris_softness, 0.05));
    float glow_alpha = ring * u_iris_glow;

    float a = clamp(cloud_alpha + glow_alpha, 0.0, 1.0);

    // Premultiplied alpha — egui blends with `glBlendFunc(ONE, ONE_MINUS_SRC_ALPHA)`,
    // so RGB is pre-multiplied by alpha at the shader output.
    vec3 col = u_color_wisp * a;
    frag_color = vec4(col, a);
}
"#;

/// GL state for one shader program — created once on first paint
/// (when the eframe `Frame` first hands us a `glow::Context`) and
/// reused every frame. Owned by the `LauncherApp` via
/// `Arc<Mutex<Option<ShaderRig>>>` so the `egui::PaintCallback`
/// closure can capture it across the immediate-mode boundary.
pub struct ShaderRig {
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
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
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

            // Fullscreen triangle covers clip-space [-1,1]² with one
            // triangle (cheaper than a 6-vertex quad). The visible
            // viewport is fully inside this triangle.
            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("create_vao: {e}"))?;
            let vbo = gl.create_buffer().map_err(|e| format!("create_vbo: {e}"))?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
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

    pub fn paint(
        &self,
        gl: &glow::Context,
        params: VortexParams,
        time: f32,
        viewport_px: [i32; 4],
    ) {
        unsafe {
            gl.viewport(viewport_px[0], viewport_px[1], viewport_px[2], viewport_px[3]);
            // Always alpha-blend. The shader outputs premultiplied
            // RGBA where alpha = cloud_density * iris (+ glow ring),
            // so the sky/starfield layers painted underneath show
            // through the dim/no-cloud regions — that's what gives
            // the deep-blue gradient + tuned stars visibility behind
            // the white wisps. Disabling blend would overwrite the
            // sky with the shader's transparent-by-design output.
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

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
        }
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    src: &str,
) -> Result<glow::Shader, String> {
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

/// Paint the GPU vortex into `rect` via an `egui::PaintCallback`. The
/// `rig` is captured by the callback closure (Arc-shared) so it can
/// outlive this function's stack frame and be dispatched from egui's
/// paint pass — same pattern as the spike.
///
/// The rig may be `None` on the first frame before the eframe `Frame`
/// has handed us a `glow::Context`; in that case the callback is a
/// no-op and the screen renders without the vortex layer for one
/// frame. Subsequent frames pick up the initialised rig.
pub fn paint_vortex(
    painter: &egui::Painter,
    rect: Rect,
    rig: Arc<Mutex<Option<ShaderRig>>>,
    params: VortexParams,
    time_s: f32,
) {
    let cb = egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let viewport_px = [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
            if let Some(rig) = rig.lock().unwrap().as_ref() {
                rig.paint(painter.gl(), params, time_s, viewport_px);
            }
        })),
    };
    painter.add(cb);
}

// -- Sky background, starfield, and primitive helpers ----------------
// Unchanged from the polar-mesh era — these layer beneath the GPU
// vortex (sky behind, starfield above sky but logically beneath the
// shader output too — the shader has its own internal starfield;
// this CPU one is only used during the Startup beat where the vortex
// shader is gated off by iris_radius=0 in Reveal mode).

/// Paint the full sky backdrop: vertical base gradient + top/bottom
/// hue ellipses. Always-on; the shader vortex layers on top.
pub fn paint_sky_background(painter: &egui::Painter, rect: Rect) {
    paint_vertical_gradient(
        painter,
        rect,
        palette::SF_1,
        0.85,
        palette::SF_2,
        palette::SF_3,
    );

    paint_radial_ellipse(
        painter,
        Pos2::new(rect.center().x, rect.top()),
        rect.width() * 0.6,
        rect.height() * 0.3,
        Color32::from_rgba_unmultiplied(0x1a, 0x3a, 0x8a, 70),
    );

    paint_radial_ellipse(
        painter,
        Pos2::new(rect.center().x, rect.bottom()),
        rect.width() * 0.6,
        rect.height() * 0.25,
        Color32::from_rgba_unmultiplied(0x0e, 0x24, 0x64, 60),
    );
}

/// Paint a vertical 3-stop linear gradient as a rect-filling mesh.
pub fn paint_vertical_gradient(
    painter: &egui::Painter,
    rect: Rect,
    top_color: Color32,
    mid_pos: f32,
    mid_color: Color32,
    bot_color: Color32,
) {
    use egui::epaint::WHITE_UV;

    let mid_y = rect.top() + rect.height() * mid_pos.clamp(0.0, 1.0);
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.left(), rect.top()),
        uv: WHITE_UV,
        color: top_color,
    });
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.right(), rect.top()),
        uv: WHITE_UV,
        color: top_color,
    });
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.left(), mid_y),
        uv: WHITE_UV,
        color: mid_color,
    });
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.right(), mid_y),
        uv: WHITE_UV,
        color: mid_color,
    });
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.left(), rect.bottom()),
        uv: WHITE_UV,
        color: bot_color,
    });
    mesh.vertices.push(Vertex {
        pos: Pos2::new(rect.right(), rect.bottom()),
        uv: WHITE_UV,
        color: bot_color,
    });
    mesh.indices.extend([0, 1, 2, 1, 3, 2]);
    mesh.indices.extend([2, 3, 4, 3, 5, 4]);
    painter.add(egui::Shape::mesh(mesh));
}

/// Paint a single soft ellipse that fades from `center_color` to
/// fully transparent at the rim.
pub fn paint_radial_ellipse(
    painter: &egui::Painter,
    center: Pos2,
    half_width: f32,
    half_height: f32,
    center_color: Color32,
) {
    use egui::epaint::WHITE_UV;

    const SEGMENTS: usize = 48;
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: center_color,
    });
    for i in 0..SEGMENTS {
        let theta = (i as f32) * std::f32::consts::TAU / SEGMENTS as f32;
        let rim = Pos2::new(
            center.x + theta.cos() * half_width,
            center.y + theta.sin() * half_height,
        );
        mesh.vertices.push(Vertex {
            pos: rim,
            uv: WHITE_UV,
            color: Color32::TRANSPARENT,
        });
    }
    for i in 0..SEGMENTS {
        let next = (i + 1) % SEGMENTS;
        mesh.indices.push(0);
        mesh.indices.push(1 + i as u32);
        mesh.indices.push(1 + next as u32);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Paint the CPU starfield — the production starfield, with three
/// colour tints (white / warm gold / cool blue), radial outward
/// drift with per-star fade-in/-out at the wrap, and per-star
/// twinkle. The shader has its own internal `starfield()` function
/// but it's spike-only (used while tuning); production sets
/// `star_brightness = 0` on the shader and renders this CPU field
/// AFTER the vortex so the stars sit visibly on top of the clouds.
pub fn paint_starfield(painter: &egui::Painter, rect: Rect, time_s: f32) {
    const NUM_STARS: u32 = 36;
    const SEED: u32 = 0xCAFE_BABE;
    const DRIFT_PX_PER_SEC: f32 = 5.0;
    const FADE_FRAC: f32 = 0.15;

    let scale = (rect.width().min(rect.height()) / 1080.0).max(0.5);
    let centre = rect.center();
    let max_radius = ((rect.width() * 0.5).powi(2) + (rect.height() * 0.5).powi(2))
        .sqrt()
        .max(1.0);
    let cycle_offset = (time_s * DRIFT_PX_PER_SEC / max_radius).rem_euclid(1.0);

    for i in 0..NUM_STARS {
        let h1 = star_hash(SEED.wrapping_add(i.wrapping_mul(0x9e37_79b9)));
        let h2 = star_hash(h1);
        let h3 = star_hash(h2);
        let h4 = star_hash(h3);

        let theta = (h1 as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        let base_phase = h2 as f32 / u32::MAX as f32;
        let depth = (base_phase + cycle_offset).rem_euclid(1.0);

        let r = depth * max_radius;
        let x = centre.x + theta.cos() * r;
        let y = centre.y + theta.sin() * r;

        if x < rect.left() || x > rect.right() || y < rect.top() || y > rect.bottom() {
            continue;
        }

        let fade_in = (depth / FADE_FRAC).clamp(0.0, 1.0);
        let fade_out = ((1.0 - depth) / FADE_FRAC).clamp(0.0, 1.0);
        let life_alpha = fade_in.min(fade_out);

        let size_choice = h3 as f32 / u32::MAX as f32;
        let radius = if size_choice < 0.7 {
            1.0 * scale
        } else if size_choice < 0.92 {
            1.6 * scale
        } else {
            2.4 * scale
        };

        let colour_choice = h4 as f32 / u32::MAX as f32;
        let base = if colour_choice < 0.6 {
            (0xff, 0xff, 0xff)
        } else if colour_choice < 0.85 {
            (0xff, 0xe6, 0xb4)
        } else {
            (0xb4, 0xdc, 0xff)
        };

        let phase = (h3 as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        let twinkle = 0.5 + 0.5 * (0.5 * (time_s * 1.05 + phase).sin() + 0.5);
        let alpha = (255.0 * twinkle * life_alpha) as u8;

        painter.circle_filled(
            egui::pos2(x, y),
            radius,
            Color32::from_rgba_unmultiplied(base.0, base.1, base.2, alpha),
        );
    }
}

fn star_hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}
