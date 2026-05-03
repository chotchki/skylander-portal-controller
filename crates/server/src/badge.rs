//! 3D rotating badge for the launcher's centre QR card (PLAN 10.7).
//!
//! Replaces the legacy 2D horizontal-scale "coin spin" hack on a flat
//! circle (PLAN 10.7's motivation note in `launch_phase.rs::badge_scale`)
//! with an honest 3D disc rendered through `glow` / `egui_glow` —
//! same plumbing the vortex backdrop already uses
//! (`crates/server/src/vortex.rs`).
//!
//! This file is the **10.7.1 spike**: static (or trivially-rotated)
//! solid-colour disc, no texture, no edge thickness, no lighting.
//! The shape proves out three load-bearing assumptions before the
//! richer 10.7.2–10.7.7 layers land on top:
//!
//!  1. A second `ShaderRig`-shaped GL pipeline can coexist with the
//!     vortex's `ShaderRig` inside `egui_glow`'s shared GL context,
//!     dispatched from a separate `egui::PaintCallback` per frame.
//!  2. A perspective-projected disc renders cleanly inside the
//!     badge's existing `egui::Rect` (the same square the legacy
//!     `qr_card_flip` allocates), with NDC mapping that doesn't
//!     require touching egui's layout.
//!  3. The disc's Y-axis rotation reads as physical motion (front
//!     face → edge → back face → edge → front face) rather than a
//!     2D squish — this is the visual contract the rest of 10.7
//!     builds on.

use std::sync::{Arc, Mutex};

use egui::Rect;
use glow::HasContext;

/// Vertex shader: builds a 64-segment disc procedurally from
/// `gl_VertexID` (no VBO geometry needed — pure shader-side fan).
///
/// Object space is a unit disc in the XY plane, centered at origin.
/// We rotate around the Y axis (the "coin tip" axis) by `u_rotation_y`,
/// translate the camera back along Z, and project with a hardcoded
/// 60° FOV perspective. Aspect is locked to 1.0 because the badge's
/// rect is always square (`CARD_SIZE × CARD_SIZE` per
/// `main_screen.rs`).
///
/// Vertex layout (TRIANGLE_FAN draw mode, vertex count = SEGMENTS + 2):
///   - vid 0: center (0, 0, 0)
///   - vid 1..=SEGMENTS: rim at angle (vid-1) * 2π/SEGMENTS
///   - vid SEGMENTS+1: same as vid 1 (closes the fan)
const VS_SRC: &str = r#"#version 330
const int SEGMENTS = 64;
const float PI = 3.14159265359;

uniform float u_rotation_y;

out vec3 v_obj;
out vec2 v_uv;

void main() {
    int vid = gl_VertexID;
    vec3 obj;
    if (vid == 0) {
        obj = vec3(0.0, 0.0, 0.0);
    } else {
        int rim_idx = (vid - 1) % SEGMENTS;
        float angle = 2.0 * PI * float(rim_idx) / float(SEGMENTS);
        obj = vec3(cos(angle), sin(angle), 0.0);
    }
    v_obj = obj;

    // Map object-space xy ∈ [-1, 1] to UV ∈ [0, 1] for sampling
    // the QR texture. The QR is rendered as a square with the
    // round disc inscribed (corners are transparent), so this
    // mapping picks up the disc's pixels exactly. Flip Y because
    // GL textures origin at bottom-left, our QR raster origin is
    // top-left.
    v_uv = vec2(obj.x * 0.5 + 0.5, 1.0 - (obj.y * 0.5 + 0.5));

    // Rotate around Y axis. With obj.z = 0 the math reduces to:
    //   view.x = cos(theta) * obj.x
    //   view.z = -sin(theta) * obj.x
    //   view.y unchanged
    float c = cos(u_rotation_y);
    float s = sin(u_rotation_y);
    vec3 view = vec3(c * obj.x, obj.y, -s * obj.x);

    // Camera at +Z looking down -Z. Push the disc back so it's in
    // front of the camera at view.z = -CAM_DISTANCE.
    const float CAM_DISTANCE = 2.5;
    view.z -= CAM_DISTANCE;

    // Hardcoded 60° vertical FOV perspective. f = 1/tan(fov/2) ≈
    // 1.732 for 60°. Aspect = 1.0 (badge rect is square).
    const float F = 1.732;
    gl_Position = vec4(view.x * F, view.y * F, view.z, -view.z);
}
"#;

/// Fragment shader: sample the QR texture on the disc's front face.
/// The QR raster (rendered by `crate::round_qr::render`) already
/// has the round shape baked in — the corners outside the
/// inscribed circle are transparent — so a straight texture
/// sample at the polygon-projected UV gives the right pixels for
/// the disc surface, including the bezel ring drawn around the
/// QR data.
const FS_SRC: &str = r#"#version 330
in vec3 v_obj;
in vec2 v_uv;

uniform sampler2D u_qr;

out vec4 frag_color;

void main() {
    vec4 c = texture(u_qr, v_uv);
    // Drop pixels outside the analytic disc. The QR raster's
    // corners are transparent so this is mostly belt-and-
    // suspenders, but the polygon-vs-circle gap on a 64-gon at
    // the edge can pull in a sliver of corner pixel near the
    // 22.5° spokes.
    if (length(v_obj.xy) > 1.0) discard;
    frag_color = c;
}
"#;

/// GL state for the badge shader — lifecycle-managed by `LauncherApp`
/// the same way `vortex::ShaderRig` is. Created lazily on the first
/// frame after the eframe `Frame` hands us a `glow::Context`; reused
/// every frame; dropped in `on_exit`.
///
/// Owns the QR texture too. The same RGBA bytes that power the
/// egui-side `qr_texture` (via `main_screen::render_qr_texture`)
/// also get uploaded to a GL texture here, so the disc's front face
/// renders the round QR pixels at-source rather than re-rasterising.
pub struct BadgeRig {
    program: glow::Program,
    vao: glow::VertexArray,
    qr_texture: glow::Texture,
    u_rotation_y: Option<glow::UniformLocation>,
    u_qr: Option<glow::UniformLocation>,
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

            // Empty VAO — the vertex shader generates positions from
            // `gl_VertexID`, no buffer needed. We still need a bound
            // VAO for the draw call to be legal in core-profile GL.
            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("badge create_vao: {e}"))?;

            // Upload the QR PNG bytes as a GL texture. RGBA8, no
            // mipmaps (the disc fills ~80 % of the badge rect at
            // face-on; minification would only happen at extreme
            // edge-on poses where the texture is barely visible).
            // LINEAR min/mag filter so the rotation reads smoothly
            // without crawling pixel artifacts on the QR's cell
            // boundaries — NEAREST would alias as the disc
            // sub-rotates per frame.
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
            // Spike state set: alpha-blend so the surrounding egui
            // pixels don't get clobbered if the disc doesn't fill
            // the viewport (rotation pose dependent). Disable
            // depth-test (no other 3D in the badge rect to sort
            // against). Enable face-cull so back-of-disc doesn't
            // draw — we'll want this anyway for 10.7.4's lit
            // cylinder. Vortex uses `ONE / ONE_MINUS_SRC_ALPHA`
            // (premultiplied); we match for consistency with the
            // rest of the launcher's GL state convention. The QR
            // PNG is straight (non-premultiplied) sRGB, so we
            // emit straight alpha and let GL premultiply via
            // SRC_ALPHA blending instead — matches the egui-side
            // texture upload's NEAREST + Color32::from_rgba_unmultiplied
            // path's expectations.
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);

            gl.use_program(Some(self.program));
            gl.uniform_1_f32(self.u_rotation_y.as_ref(), rotation_y);

            // Bind QR texture to texture unit 0 + tell the sampler
            // uniform to read from it.
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.qr_texture));
            gl.uniform_1_i32(self.u_qr.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));
            // SEGMENTS + 2 = 66 vertices (center + 64 rim + closing
            // duplicate). TRIANGLE_FAN consumes them as 64 triangles
            // sharing the centre vertex.
            gl.draw_arrays(glow::TRIANGLE_FAN, 0, 66);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);

            // Reset state egui's renderer assumes — it doesn't expect
            // CULL_FACE on coming back to the painter pass. Leave
            // BLEND on (egui wants it). Mirror what vortex.rs does
            // implicitly by not touching state egui owns.
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
