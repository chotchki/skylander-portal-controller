//! 3D rotating badge for the launcher's centre QR card (PLAN 10.7).
//!
//! Replaces the legacy 2D horizontal-scale "coin spin" hack on a flat
//! circle (PLAN 10.7's motivation note in `launch_phase.rs::badge_scale`)
//! with an honest 3D disc rendered through `glow` / `egui_glow` —
//! same plumbing the vortex backdrop already uses
//! (`crates/server/src/vortex.rs`).
//!
//! 10.7.1 (spike): solid-colour textureless disc proved the GL +
//! `egui::PaintCallback` pipeline.
//!
//! 10.7.2: front-face QR texture sample.
//!
//! 10.7.3: rotation driven by `LaunchPhase::badge_rotation_y`.
//!
//! 10.7.4 (this iteration): cylinder-cap geometry so the disc has
//! visible thickness during the edge-on phase, and directional
//! Lambert lighting so the rotation is shaded as a physical object
//! instead of reading as a flat decal that vanishes at 90°. Three
//! draw calls per frame share one shader program, dispatched via the
//! `u_face` uniform:
//!
//!   - **Face 0 — front:** textured QR fan at z = +HALF_THICKNESS,
//!     standard CCW winding so the front face is `front` to GL when
//!     the +Z side of the disc faces the camera.
//!   - **Face 1 — back:** solid gold fan at z = -HALF_THICKNESS,
//!     **reversed** rim winding (negative angle step) so the back
//!     face is `front` to GL only when the disc has rotated past
//!     edge-on and the original back side is now facing the camera.
//!     Without the reversal both fans would draw at θ=0 and the
//!     no-depth-test paint would race for the centre pixels.
//!   - **Face 2 — side wall:** gold cylinder triangle-strip
//!     connecting the two rims. Outward normal is radial in the
//!     XY plane, so the lit gold reads as a coin's milled edge.
//!
//! The `front` vs `back` selection above is a normal-direction
//! statement: GL's `CULL_FACE BACK` actually keys off the projected
//! winding, but with `HALF_THICKNESS` small relative to
//! `CAM_DISTANCE` the projection preserves the object-space winding
//! of each face, so the two are interchangeable here.

use std::sync::{Arc, Mutex};

use egui::Rect;
use glow::HasContext;

/// Number of rim segments in the disc fan / cylinder strip. Mirrors
/// `SEGMENTS` in the GLSL source — keep in sync. 64 reads as smooth
/// at the badge's typical on-screen size (~`CARD_SIZE` of
/// `crate::ui::main_screen`); polygon-vs-circle gap at the rim is
/// below 1 px when the badge fills the card.
///
/// Disc thickness is set in GLSL as `HALF_THICKNESS = 0.04` (8 %
/// of the disc's diameter — coin / poker-chip aspect). Not mirrored
/// in Rust because no Rust code consumes it.
const SEGMENTS: i32 = 64;

/// Vertex shader: branches on `u_face` to emit one of three pieces
/// of geometry, all rotated together around the Y axis by
/// `u_rotation_y` and projected with a shared 60° FOV perspective.
/// Object space is a unit disc in the XY plane centred at origin
/// (Z is thickness). Geometry is generated procedurally from
/// `gl_VertexID` — no VBO needed.
///
/// Faces:
///   - **0 (front):** TRIANGLE_FAN, SEGMENTS+2 verts, z=+HALF_THICKNESS,
///     standard CCW rim. Carries the QR texture.
///   - **1 (back):** TRIANGLE_FAN, SEGMENTS+2 verts, z=-HALF_THICKNESS,
///     reversed rim (negative angle step) so screen-space winding
///     comes out CW when the disc's +Z side faces camera — GL's
///     CULL_FACE BACK keeps it hidden until rotation flips the disc
///     past edge-on. Solid gold.
///   - **2 (side wall):** TRIANGLE_STRIP, 2*(SEGMENTS+1) verts,
///     alternating top/bottom rim. Outward normal radial in XY.
///     Solid gold; visible as the disc rotates through edge-on.
///
/// Per-vertex normal in object space is then rotated through the
/// same Y-rotation matrix as the position, and passed to the
/// fragment shader for Lambert shading.
const VS_SRC: &str = r#"#version 330
const int SEGMENTS = 64;
const float PI = 3.14159265359;
const float HALF_THICKNESS = 0.04;

uniform float u_rotation_y;
uniform int u_face;

out vec3 v_obj;
out vec2 v_uv;
out vec3 v_normal_view;

void main() {
    int vid = gl_VertexID;
    vec3 obj;
    vec3 obj_normal;

    if (u_face == 0) {
        // Front face fan: vid 0 = centre, vid 1..=SEGMENTS = rim,
        // vid SEGMENTS+1 = duplicate first rim vert (closes the fan).
        if (vid == 0) {
            obj = vec3(0.0, 0.0, HALF_THICKNESS);
        } else {
            int rim_idx = (vid - 1) % SEGMENTS;
            float angle = 2.0 * PI * float(rim_idx) / float(SEGMENTS);
            obj = vec3(cos(angle), sin(angle), HALF_THICKNESS);
        }
        obj_normal = vec3(0.0, 0.0, 1.0);
    } else if (u_face == 1) {
        // Back face fan: same vert layout, z negated, AND angle
        // negated so the rim winds CW from +Z view. Without the
        // angle flip both fans would have CCW screen-space winding
        // at θ=0 and CULL_FACE BACK couldn't hide the back face
        // behind the front (no depth test in this pass).
        if (vid == 0) {
            obj = vec3(0.0, 0.0, -HALF_THICKNESS);
        } else {
            int rim_idx = (vid - 1) % SEGMENTS;
            float angle = -2.0 * PI * float(rim_idx) / float(SEGMENTS);
            obj = vec3(cos(angle), sin(angle), -HALF_THICKNESS);
        }
        obj_normal = vec3(0.0, 0.0, -1.0);
    } else {
        // Side-wall strip: 2*(SEGMENTS+1) verts; even vid = top
        // rim (+H), odd vid = bottom rim (-H), rim_idx = vid/2.
        // The closing vert at rim_idx == SEGMENTS wraps back to
        // angle 0 so the strip seams cleanly. GL_TRIANGLE_STRIP
        // alternates triangle orientation per vert; the resulting
        // outward normal of each tri is radial (+XY plane), which
        // is what we compute below.
        int rim_idx = vid / 2;
        float z_sign = (vid % 2 == 0) ? 1.0 : -1.0;
        float angle = 2.0 * PI * float(rim_idx) / float(SEGMENTS);
        float ca = cos(angle);
        float sa = sin(angle);
        obj = vec3(ca, sa, z_sign * HALF_THICKNESS);
        obj_normal = vec3(ca, sa, 0.0);
    }

    v_obj = obj;
    // Front-face QR sample maps obj.xy ∈ [-1,1] to UV ∈ [0,1] with
    // V flipped for GL's bottom-left texture origin. Other faces
    // ignore v_uv (they use solid material) so the back-face's
    // negative-angle path can leave this unconditional.
    v_uv = vec2(obj.x * 0.5 + 0.5, 1.0 - (obj.y * 0.5 + 0.5));

    // Y-axis rotation applied to position. Reduces to the
    // 10.7.1/10.7.2 form when obj.z = 0; the +obj.z * sin term
    // is what makes the side wall and back face translate
    // correctly as the disc tips.
    float c = cos(u_rotation_y);
    float s = sin(u_rotation_y);
    vec3 view = vec3(
        c * obj.x + s * obj.z,
        obj.y,
        -s * obj.x + c * obj.z
    );

    // Same rotation applied to the normal. For a rigid Y rotation
    // (no scale, no shear) the normal matrix is just the rotation
    // itself, so we don't need a transpose-inverse. View space
    // here = world space — there's no separate model/view split,
    // the camera is fixed at +Z looking down -Z.
    v_normal_view = vec3(
        c * obj_normal.x + s * obj_normal.z,
        obj_normal.y,
        -s * obj_normal.x + c * obj_normal.z
    );

    const float CAM_DISTANCE = 2.5;
    view.z -= CAM_DISTANCE;

    const float F = 1.732;
    gl_Position = vec4(view.x * F, view.y * F, view.z, -view.z);
}
"#;

/// Fragment shader: per-face material × Lambert-diffuse intensity.
/// Light is a hardcoded directional source from upper-right and
/// slightly toward camera (`normalize(0.3, 0.5, 1.0)`) so the
/// face-on pose (normal = +Z) gets the brightest reading and the
/// edge-on side wall picks up a moving highlight as the disc
/// rotates. Ambient term keeps the unlit side legible — without it
/// the back face would go fully black at θ=π and read as a hole.
const FS_SRC: &str = r#"#version 330
in vec3 v_obj;
in vec2 v_uv;
in vec3 v_normal_view;

uniform sampler2D u_qr;
uniform int u_face;

out vec4 frag_color;

const vec3 LIGHT_DIR = normalize(vec3(0.3, 0.5, 1.0));
const float AMBIENT = 0.4;
// Skylanders-y warm gold for the back face + milled edge. Matches
// the bezel ring baked into the QR texture closely enough that the
// disc reads as one material when rotating between front and back.
const vec3 GOLD = vec3(0.82, 0.62, 0.20);

void main() {
    float lambert = max(0.0, dot(normalize(v_normal_view), LIGHT_DIR));
    float intensity = AMBIENT + (1.0 - AMBIENT) * lambert;

    if (u_face == 0) {
        // Front face: QR texture, with the analytic disc clip kept
        // as belt-and-suspenders against the polygon-vs-circle
        // sliver at the 64-gon rim spokes. (See 10.7.2 note.)
        vec4 c = texture(u_qr, v_uv);
        if (length(v_obj.xy) > 1.0) discard;
        frag_color = vec4(c.rgb * intensity, c.a);
    } else if (u_face == 1) {
        // Back face: solid lit gold. Same disc clip.
        if (length(v_obj.xy) > 1.0) discard;
        frag_color = vec4(GOLD * intensity, 1.0);
    } else {
        // Side wall: solid lit gold. No disc clip — the wall lives
        // exactly on the unit cylinder, no overshoot to trim.
        frag_color = vec4(GOLD * intensity, 1.0);
    }
}
"#;

/// GL state for the badge shader — lifecycle-managed by `LauncherApp`
/// the same way `vortex::ShaderRig` is. Created lazily on the first
/// frame after the eframe `Frame` hands us a `glow::Context`; reused
/// every frame; dropped in `on_exit`.
///
/// Owns the QR texture too. The same RGBA bytes that power the
/// egui-side `qr_texture` (via `main_screen::render_qr_pixels`) get
/// uploaded to a GL texture here, so the disc's front face renders
/// the round QR pixels at-source rather than re-rasterising.
pub struct BadgeRig {
    program: glow::Program,
    vao: glow::VertexArray,
    qr_texture: glow::Texture,
    u_rotation_y: Option<glow::UniformLocation>,
    u_qr: Option<glow::UniformLocation>,
    u_face: Option<glow::UniformLocation>,
}

impl BadgeRig {
    /// Build the rig, compile shaders, and upload the QR pixels as
    /// a GL texture in one go. `qr_pixels` is the same buffer
    /// `crate::round_qr::render` produces; bytes layout is RGBA8
    /// row-major from the top-left.
    pub fn new(
        gl: &glow::Context,
        qr_pixels: &crate::round_qr::RoundQrPixels,
    ) -> Result<Self, String> {
        unsafe {
            let program = gl
                .create_program()
                .map_err(|e| format!("badge create_program: {e}"))?;
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
                return Err(format!("badge program link: {log}"));
            }
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("badge create_vao: {e}"))?;

            let qr_texture = gl
                .create_texture()
                .map_err(|e| format!("badge create_texture: {e}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(qr_texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                qr_pixels.width as i32,
                qr_pixels.height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(qr_pixels.rgba.as_slice()),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);

            Ok(Self {
                u_rotation_y: gl.get_uniform_location(program, "u_rotation_y"),
                u_qr: gl.get_uniform_location(program, "u_qr"),
                u_face: gl.get_uniform_location(program, "u_face"),
                program,
                vao,
                qr_texture,
            })
        }
    }

    pub fn paint(&self, gl: &glow::Context, rotation_y: f32, viewport_px: [i32; 4]) {
        unsafe {
            gl.viewport(
                viewport_px[0],
                viewport_px[1],
                viewport_px[2],
                viewport_px[3],
            );
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);

            gl.use_program(Some(self.program));
            gl.uniform_1_f32(self.u_rotation_y.as_ref(), rotation_y);

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.qr_texture));
            gl.uniform_1_i32(self.u_qr.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));

            // Front face fan (textured QR).
            gl.uniform_1_i32(self.u_face.as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLE_FAN, 0, SEGMENTS + 2);

            // Back face fan (gold).
            gl.uniform_1_i32(self.u_face.as_ref(), 1);
            gl.draw_arrays(glow::TRIANGLE_FAN, 0, SEGMENTS + 2);

            // Side-wall cylinder strip (gold).
            gl.uniform_1_i32(self.u_face.as_ref(), 2);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 2 * (SEGMENTS + 1));

            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);

            gl.disable(glow::CULL_FACE);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_texture(self.qr_texture);
        }
    }
}

unsafe fn compile_shader(gl: &glow::Context, kind: u32, src: &str) -> Result<glow::Shader, String> {
    unsafe {
        let s = gl
            .create_shader(kind)
            .map_err(|e| format!("badge create_shader: {e}"))?;
        gl.shader_source(s, src);
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            let log = gl.get_shader_info_log(s);
            gl.delete_shader(s);
            return Err(format!("badge compile: {log}"));
        }
        Ok(s)
    }
}

/// Paint the 3D badge into `rect` via an `egui::PaintCallback`.
/// Mirrors `vortex::paint_vortex`'s shape — the rig is captured
/// `Arc`-shared so the callback closure can outlive this stack
/// frame, and a `None` rig (first-frame race) silently no-ops.
pub fn paint_badge(
    painter: &egui::Painter,
    rect: Rect,
    rig: Arc<Mutex<Option<BadgeRig>>>,
    rotation_y: f32,
) {
    let cb = egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let viewport_px = [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
            if let Some(rig) = rig.lock().unwrap().as_ref() {
                rig.paint(painter.gl(), rotation_y, viewport_px);
            }
        })),
    };
    painter.add(cb);
}
