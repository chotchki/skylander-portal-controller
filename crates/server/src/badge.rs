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
//! 10.7.4: cylinder-cap geometry so the disc has visible thickness
//! during the edge-on phase, and directional Lambert lighting.
//!
//! 10.7.5 (this iteration): a flatter torus around the disc as a
//! decorative gold ring — Skylanders' visual language leans hard on
//! framed-medallion shapes and the bare cylinder reads naked
//! without one. The torus has an elliptical cross-section (radial
//! half-width > z half-width) so it sits like a flat band rather
//! than a donut, and overlaps the disc's body in radius (its inner
//! cross-section is inside the disc rim). To resolve the overlap
//! we enable `DEPTH_TEST` and clear the depth buffer at the start
//! of each paint — the disc's solid interior occludes the torus
//! inner surface cleanly, so only the visually-meaningful outer
//! ring renders. With depth test on, the back-face winding flip
//! from 10.7.4 becomes load-bearing only at θ near 0 (where the
//! back fan and front fan share screen pixels but differ in z) —
//! depth would also resolve it, but cull-face is cheaper.
//!
//! Four draw calls per frame share one shader program, dispatched
//! via the `u_face` uniform:
//!
//!   - **Face 0 — front:** textured QR fan at z = +HALF_THICKNESS,
//!     standard CCW winding so the front face is `front` to GL when
//!     the +Z side of the disc faces the camera.
//!   - **Face 1 — back:** solid gold fan at z = -HALF_THICKNESS,
//!     **reversed** rim winding (negative angle step) so the back
//!     face is `front` to GL only when the disc has rotated past
//!     edge-on and the original back side is now facing the camera.
//!   - **Face 2 — side wall:** gold cylinder triangle-strip
//!     connecting the two rims. Outward normal is radial in the
//!     XY plane, so the lit gold reads as a coin's milled edge.
//!   - **Face 3 — torus ring:** flatter elliptical torus wrapping
//!     the disc rim (TRIANGLES, generated procedurally from a
//!     `(u, v)` grid via `gl_VertexID`). Same gold material as the
//!     other backing faces. Inner half is occluded by the disc body
//!     via depth test; only the outer half + the rim's "lip" above
//!     and below the disc faces is visible.

use std::collections::HashMap;
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

/// Torus tessellation: `TORUS_MAJOR` segments around the disc rim,
/// `TORUS_MINOR` segments around each cross-section. Mirror
/// `TORUS_MAJOR` / `TORUS_MINOR` in the GLSL source — keep in sync.
/// Total triangle count = 2 × MAJOR × MINOR = 2048 tris (6144
/// verts at 6 per quad). Cheap enough for a per-frame draw.
///
/// Cross-section geometry (`TORUS_R`, `TORUS_W_RAD`, `TORUS_W_Z`)
/// is GLSL-only — see the VS_SRC source for the elliptical-band
/// values.
const TORUS_MAJOR: i32 = 64;
const TORUS_MINOR: i32 = 16;

/// Vertex shader: branches on `u_face` to emit one of four pieces
/// of geometry (disc front, disc back, disc side wall, surrounding
/// torus ring), all rotated together around the Y axis by
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
///   - **3 (torus ring):** TRIANGLES, 6*TORUS_MAJOR*TORUS_MINOR
///     verts. Procedural (u, v) grid quad-fan: each `tri_idx =
///     vid / 6` picks one quad of the torus surface, `vert_in_tri
///     = vid % 6` picks one of the two triangles' three corners.
///     Cross-section is an ellipse with `TORUS_W_RAD` radial half-
///     width and `TORUS_W_Z` z half-width — flatter than tall, so
///     the ring reads as a band rather than a donut. The torus
///     overlaps the disc body radially (its inner cross-section is
///     inside the disc rim); depth test handles the occlusion.
///
/// Per-vertex normal in object space is then rotated through the
/// same Y-rotation matrix as the position, and passed to the
/// fragment shader for Lambert shading.
const VS_SRC: &str = r#"#version 330
const int SEGMENTS = 64;
const int TORUS_MAJOR = 64;
const int TORUS_MINOR = 16;
const float PI = 3.14159265359;
const float HALF_THICKNESS = 0.04;
// Disc geometry. The QR raster fills the *inscribed* unit disc
// (`QR_RADIUS = 1.0`) but the disc body itself extends to
// `OUTER_RADIUS = 1.04` — the extra annulus is painted as solid
// lit gold in the fragment shader, bridging the QR's outer edge
// to the torus's inner edge so the disc reads as one continuous
// gold-framed medallion. Started at 0.08 thick; ChrisCheck →
// "cut the gold ring thickness in half" → 0.04.
const float QR_RADIUS = 1.0;
const float OUTER_RADIUS = 1.04;
// Torus geometry. R is the major radius (centre of cross-section
// to disc centre); W_RAD/W_Z are the cross-section half-widths.
// W_RAD > W_Z gives the "flatter band" feel Chris asked for at
// 10.7.5. Inner cross-section at R - W_RAD = 1.03 sits *just
// inside* OUTER_RADIUS so the torus's interior overlaps the disc
// body by ~0.01 — depth test occludes the inner sliver, and the
// visible torus reads as starting flush against the disc's gold
// rim with no dark gap. Cross-section shape restored after
// shrinking it lost the chunky-band feel; only the major radius
// was actually too far out — pulled R from 1.20 → 1.16 to seat
// against the now-thinner 0.04 gold ring.
const float TORUS_R = 1.16;
const float TORUS_W_RAD = 0.13;
const float TORUS_W_Z = 0.05;

uniform float u_rotation_y;
uniform int u_face;
// Whole-disc 2D scale (PLAN 10.7.6). Applied to gl_Position.xy
// after the perspective projection so the disc inflates from
// `SCALE_MIN_3D` to 1.0 over the spin window — independent of
// rotation, so the multi-turn coin-spin's natural cosine cycles
// don't pulse the envelope. Scaling clip-space rather than
// object/view-space leaves perspective and depth untouched.
uniform float u_scale;

out vec3 v_obj;
out vec2 v_uv;
out vec3 v_normal_view;
// View-space position (camera at origin, looking down -Z) so the
// fragment shader can build a per-pixel view direction for the
// Blinn-Phong specular highlight on the gold faces. Pass it
// after the CAM_DISTANCE shift so `-v_view_pos` is the camera-
// to-fragment vector and `normalize(-v_view_pos)` is V.
out vec3 v_view_pos;

void main() {
    int vid = gl_VertexID;
    vec3 obj;
    vec3 obj_normal;

    if (u_face == 0) {
        // Front face fan: vid 0 = centre, vid 1..=SEGMENTS = rim,
        // vid SEGMENTS+1 = duplicate first rim vert (closes the
        // fan). Rim is at OUTER_RADIUS, not 1.0 — the disc body
        // extends past the QR's QR_RADIUS to fill the gap to the
        // torus's inner edge with a lit-gold annulus.
        if (vid == 0) {
            obj = vec3(0.0, 0.0, HALF_THICKNESS);
        } else {
            int rim_idx = (vid - 1) % SEGMENTS;
            float angle = 2.0 * PI * float(rim_idx) / float(SEGMENTS);
            obj = vec3(cos(angle) * OUTER_RADIUS, sin(angle) * OUTER_RADIUS, HALF_THICKNESS);
        }
        obj_normal = vec3(0.0, 0.0, 1.0);
    } else if (u_face == 1) {
        // Back face fan: same vert layout, z negated, AND angle
        // negated so the rim winds CW from +Z view. Cull-face
        // hides it before edge-on; depth test (10.7.5) would also
        // resolve front-vs-back at θ=0, but cull is cheaper.
        if (vid == 0) {
            obj = vec3(0.0, 0.0, -HALF_THICKNESS);
        } else {
            int rim_idx = (vid - 1) % SEGMENTS;
            float angle = -2.0 * PI * float(rim_idx) / float(SEGMENTS);
            obj = vec3(cos(angle) * OUTER_RADIUS, sin(angle) * OUTER_RADIUS, -HALF_THICKNESS);
        }
        obj_normal = vec3(0.0, 0.0, -1.0);
    } else if (u_face == 2) {
        // Side-wall strip at OUTER_RADIUS: 2*(SEGMENTS+1) verts;
        // even vid = top rim (+H), odd vid = bottom rim (-H),
        // rim_idx = vid/2. The closing vert at rim_idx == SEGMENTS
        // wraps back to angle 0 so the strip seams cleanly.
        // GL_TRIANGLE_STRIP alternates triangle orientation per
        // vert; the resulting outward normal of each tri is radial
        // (+XY plane), which is what we compute below.
        int rim_idx = vid / 2;
        float z_sign = (vid % 2 == 0) ? 1.0 : -1.0;
        float angle = 2.0 * PI * float(rim_idx) / float(SEGMENTS);
        float ca = cos(angle);
        float sa = sin(angle);
        obj = vec3(ca * OUTER_RADIUS, sa * OUTER_RADIUS, z_sign * HALF_THICKNESS);
        obj_normal = vec3(ca, sa, 0.0);
    } else {
        // Torus ring (u_face == 3). Procedural (u, v) grid as
        // independent triangles: each "quad" of the surface is two
        // triangles, 6 verts. vid layout:
        //   tri_idx     = vid / 6      → which quad
        //   vert_in_tri = vid % 6      → which corner of that quad
        //   u_idx       = tri_idx % TORUS_MAJOR
        //   v_idx       = tri_idx / TORUS_MAJOR
        // The two triangles of each quad use vertices (du, dv) ∈
        // {(0,0),(1,0),(0,1)} and {(1,0),(1,1),(0,1)} so winding
        // comes out CCW from outside (parametric outward).
        int tri_idx = vid / 6;
        int vert_in_tri = vid % 6;
        int u_idx = tri_idx % TORUS_MAJOR;
        int v_idx = tri_idx / TORUS_MAJOR;

        int du, dv;
        if (vert_in_tri == 0)      { du = 0; dv = 0; }
        else if (vert_in_tri == 1) { du = 1; dv = 0; }
        else if (vert_in_tri == 2) { du = 0; dv = 1; }
        else if (vert_in_tri == 3) { du = 1; dv = 0; }
        else if (vert_in_tri == 4) { du = 1; dv = 1; }
        else                       { du = 0; dv = 1; }

        float u = 2.0 * PI * float(u_idx + du) / float(TORUS_MAJOR);
        float v = 2.0 * PI * float(v_idx + dv) / float(TORUS_MINOR);
        float cu = cos(u);
        float su = sin(u);
        float cv = cos(v);
        float sv = sin(v);

        obj = vec3(
            (TORUS_R + TORUS_W_RAD * cv) * cu,
            (TORUS_R + TORUS_W_RAD * cv) * su,
            TORUS_W_Z * sv
        );
        // Outward normal of an elliptical-cross-section torus —
        // derived from ∂P/∂u × ∂P/∂v then normalized. For a
        // circular cross-section (W_RAD == W_Z) this collapses to
        // the familiar (cv*cu, cv*su, sv); when W_RAD ≠ W_Z each
        // component is scaled by the *other* axis's half-width
        // (so the ellipse's normal is steeper at the wide axis,
        // shallower at the narrow one).
        obj_normal = normalize(vec3(
            TORUS_W_Z * cv * cu,
            TORUS_W_Z * cv * su,
            TORUS_W_RAD * sv
        ));
    }

    v_obj = obj;
    // Front-face QR sample maps obj.xy ∈ [-1,1] to UV ∈ [0,1] with
    // V flipped for GL's bottom-left texture origin. The QR raster
    // is rendered at the unit disc inscribed in a unit square — so
    // this mapping picks up the QR pixels exactly for radius ≤
    // QR_RADIUS = 1.0. For OUTER_RADIUS ≥ radius > QR_RADIUS (the
    // gold-ring annulus) the UV exceeds [0, 1] and would clamp to
    // the texture edge, but the fragment shader overrides with
    // solid lit gold for r > QR_RADIUS so the bogus UV never gets
    // sampled. Other faces ignore v_uv (solid material).
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

    // Perspective parameters. CAM_DISTANCE bumped 2.5 → 3.5 and F
    // bumped 1.732 → 2.425 (tighter ~45° FOV) so the face-on disc
    // projects to the same NDC size (1.04 * F / CAM_DISTANCE ≈
    // 0.72) but the rotation-asymmetry from perspective is gentler
    // — the torus's near rim at θ ≈ 45° was reaching NDC.x ≈
    // -0.995 with the old perspective and clipping the rect on
    // its left edge for slightly larger angles. The new params
    // give ~15% margin from the clip plane across the whole
    // rotation arc.
    //
    // Depth mapping: pre-10.7.5e gl_Position.z was just view.z,
    // which made NDC.z = view.z / -view.z = -1 for *every*
    // fragment — depth_test had nothing to compare. Proper map:
    //   NDC.z = (A * view.z + B) / -view.z
    // with A = -(F+N)/(F-N) and B = -2*F*N/(F-N) so view.z = -N
    // → NDC.z = -1 and view.z = -F → NDC.z = +1. With NEAR=1.0
    // and FAR=6.0 our geometry (view.z ranging roughly -2.21 to
    // -4.79 across all rotations + faces) sits comfortably inside
    // the mapped [-1, +1] NDC.z band, so the depth test now
    // actually sorts the torus inner sliver against the disc
    // body the way the comments have always claimed it did.
    const float CAM_DISTANCE = 3.5;
    const float F = 2.425;
    view.z -= CAM_DISTANCE;
    v_view_pos = view;

    const float NEAR = 1.0;
    const float FAR = 6.0;
    const float DEPTH_A = -(FAR + NEAR) / (FAR - NEAR);
    const float DEPTH_B = -2.0 * FAR * NEAR / (FAR - NEAR);
    gl_Position = vec4(
        view.x * F * u_scale,
        view.y * F * u_scale,
        DEPTH_A * view.z + DEPTH_B,
        -view.z
    );
}
"#;

/// Fragment shader: per-face material × Lambert diffuse +
/// Blinn-Phong specular on the gold faces. Light is a hardcoded
/// directional source from upper-right and slightly toward camera
/// (`normalize(0.3, 0.5, 1.0)`); ambient term keeps the unlit side
/// legible.
///
/// The QR front face stays Lambert-only — printed paper (or the
/// in-app raster equivalent) shouldn't catch a specular highlight,
/// and adding one washed the QR data out under the highlight band
/// when the disc was face-on. Gold faces (back, side wall, torus)
/// get a Blinn-Phong specular term using the half-vector between
/// `LIGHT_DIR` and the per-fragment view direction so the highlight
/// sweeps physically as the disc rotates and reads as polished
/// metal rather than a flat gold sticker.
const FS_SRC: &str = r#"#version 330
in vec3 v_obj;
in vec2 v_uv;
in vec3 v_normal_view;
in vec3 v_view_pos;

uniform sampler2D u_qr;
uniform int u_face;
// Whole-badge opacity (PLAN 10.7.6). Multiplied against every
// fragment's output alpha so the disc fades in/out coherently
// during intro / close transitions instead of popping in once
// the iris reveals it.
uniform float u_alpha;
// Whether to add the Blinn-Phong specular term to the texture-
// sampled disc area on the front face. False for the QR raster
// (printed-paper look — spec washes out the QR data); true for the
// back-face text textures (gold material — spec reads as polished
// metal under the same highlight that sweeps the side wall +
// torus). 0 / 1 used in lieu of `bool` for older driver mileage.
uniform int u_apply_texture_spec;

out vec4 frag_color;

const vec3 LIGHT_DIR = normalize(vec3(0.3, 0.5, 1.0));
const float AMBIENT = 0.4;
// Skylanders-y warm gold for the back face + milled edge + torus.
const vec3 GOLD = vec3(0.82, 0.62, 0.20);
// Specular highlight: warm-white tint (gold reflects whitish at
// grazing angles) with a moderately tight power so the highlight
// reads as a discrete sweep rather than a diffuse sheen.
const vec3 SPEC_TINT = vec3(1.0, 0.92, 0.65);
const float SHININESS = 48.0;
const float SPEC_STRENGTH = 0.85;
// Mirror of the VS_SRC constants — keep in sync. Used by the
// front-face's gold-vs-QR split and by the disc clip on the
// fan faces (front + back).
const float QR_RADIUS = 1.0;
const float OUTER_RADIUS = 1.04;

void main() {
    vec3 N = normalize(v_normal_view);
    float lambert = max(0.0, dot(N, LIGHT_DIR));
    float intensity = AMBIENT + (1.0 - AMBIENT) * lambert;

    // Blinn-Phong specular. View vector is from fragment to
    // camera; camera sits at view-space origin so V = -view_pos
    // normalized. Computed once and reused across all gold faces
    // (back fan, side wall, torus, gold-ring annulus on the
    // front face) so the highlight tracks the same physical
    // light source consistently across the whole "frame" of the
    // medallion.
    vec3 V = normalize(-v_view_pos);
    vec3 H = normalize(LIGHT_DIR + V);
    float spec = pow(max(0.0, dot(N, H)), SHININESS) * SPEC_STRENGTH;
    vec3 gold_lit = GOLD * intensity + SPEC_TINT * spec;

    if (u_face == 0) {
        // Front face: bound texture inside QR_RADIUS, lit gold ring
        // for QR_RADIUS < r ≤ OUTER_RADIUS, discard outside. The
        // gold ring fills the gap between the texture's outer edge
        // and the torus's inner cross-section (10.7.5). Texture
        // sample picks up either the QR raster or one of the back-
        // face text textures depending on what the call site bound.
        float r = length(v_obj.xy);
        if (r > OUTER_RADIUS) discard;
        if (r > QR_RADIUS) {
            frag_color = vec4(gold_lit, 1.0);
        } else {
            vec4 c = texture(u_qr, v_uv);
            // QR raster: Lambert-only — spec washes out the QR
            // data and the printed-paper look is the right
            // material analogy. Back-face text on gold: same spec
            // sweep as the surrounding ring/torus so the metal
            // reads as one polished material.
            vec3 lit = c.rgb * intensity;
            if (u_apply_texture_spec == 1) {
                lit += SPEC_TINT * spec;
            }
            frag_color = vec4(lit, c.a);
        }
    } else if (u_face == 1) {
        // Back face: lit gold across the whole disc body. Same
        // OUTER_RADIUS clip as the front so the back matches.
        if (length(v_obj.xy) > OUTER_RADIUS) discard;
        frag_color = vec4(gold_lit, 1.0);
    } else {
        // Side wall + torus ring: same gold material — both are
        // "frame" geometry. No analytic clip — their geometry is
        // exact (no overshoot to trim).
        frag_color = vec4(gold_lit, 1.0);
    }
    // Whole-badge fade. Multiplies *after* the per-face material
    // pick so QR + gold + spec all fade in lockstep — no risk of
    // the QR briefly outpacing the gold ring during the ramp.
    frag_color.a *= u_alpha;
}
"#;

/// GL state for the badge shader — lifecycle-managed by `LauncherApp`
/// the same way `vortex::ShaderRig` is. Created lazily on the first
/// frame after the eframe `Frame` hands us a `glow::Context`; reused
/// every frame; dropped in `on_exit`.
///
/// Owns the bound textures: index 0 is always the QR raster (same
/// bytes that power the egui-side `qr_texture` via
/// `main_screen::render_qr_pixels`); indices 1.. are the
/// `badge_text`-rasterised back-face strings (PLAN 10.7.6b), one
/// per `BackFace` enum variant in the order the call site uploads
/// them. The shader binds whichever index `paint` is told to so the
/// flip-on-state-change animation (intro/close-style coin spin
/// triggered when the displayed text would change) lands face-on
/// with the new text already on the disc, not popping in.
/// Cache key for the pip-face textures rendered through `paint_pip`
/// (PLAN 10.7.8). Encodes everything that distinguishes one pip
/// from another visually — colour packed into a `u32`, profile
/// initial, ghost-state flag — so identical pips share one GL
/// texture instead of re-uploading per frame.
pub type PipKey = (u32, char, bool);

pub struct BadgeRig {
    program: glow::Program,
    vao: glow::VertexArray,
    textures: Vec<glow::Texture>,
    /// Cache of pip-face textures lazily uploaded on first
    /// `paint_pip` for each `(color, initial, ghost)` triple. Maps
    /// to the `textures` index — we never evict, since profile
    /// count stays small (`profiles::MAX_SESSIONS`) and the cost
    /// of holding ~8 small RGBA textures is trivial.
    pip_cache: HashMap<PipKey, usize>,
    /// Pixel size used for newly rasterised pip-face textures.
    /// Matches the QR's per-axis size so all front-face textures
    /// upload at the same resolution and the LINEAR mag filter
    /// gives consistent on-screen sharpness across QR / back-face
    /// / pip.
    pip_texture_size: u32,
    u_rotation_y: Option<glow::UniformLocation>,
    u_qr: Option<glow::UniformLocation>,
    u_face: Option<glow::UniformLocation>,
    u_alpha: Option<glow::UniformLocation>,
    u_scale: Option<glow::UniformLocation>,
    u_apply_texture_spec: Option<glow::UniformLocation>,
}

/// One texture's worth of RGBA pixels destined for `BadgeRig`.
/// `width = height` (the disc geometry is square in object space
/// and the shader UV mapping assumes square textures). Carried as a
/// plain struct so callers can build a heterogeneous list without
/// the QR / back-face textures sharing a wrapper type.
pub struct TextureSource<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> From<&'a crate::round_qr::RoundQrPixels> for TextureSource<'a> {
    fn from(p: &'a crate::round_qr::RoundQrPixels) -> Self {
        Self {
            width: p.width,
            height: p.height,
            rgba: &p.rgba,
        }
    }
}

impl BadgeRig {
    /// Build the rig, compile shaders, and upload all the disc
    /// front-face textures as a single contiguous GL texture array
    /// (one entry per call-site-supplied source). Index 0 is bound
    /// when `paint` is called with `texture_index = 0`, etc. Caller
    /// is expected to pass the QR raster at index 0 followed by the
    /// back-face text rasters in `BackFace`-variant order so the
    /// `main_screen` flip-state can map to indices statically.
    pub fn new(gl: &glow::Context, sources: &[TextureSource<'_>]) -> Result<Self, String> {
        if sources.is_empty() {
            return Err("badge: at least one texture source required".into());
        }
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

            let mut textures = Vec::with_capacity(sources.len());
            for source in sources {
                let tex = gl
                    .create_texture()
                    .map_err(|e| format!("badge create_texture: {e}"))?;
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
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
                    source.width as i32,
                    source.height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    Some(source.rgba),
                );
                textures.push(tex);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);

            // Pip textures get lazily uploaded at the same per-axis
            // resolution as the QR raster (index 0) so every front-
            // face texture has matching pixel density.
            let pip_texture_size = sources[0].width.max(sources[0].height);
            Ok(Self {
                u_rotation_y: gl.get_uniform_location(program, "u_rotation_y"),
                u_qr: gl.get_uniform_location(program, "u_qr"),
                u_face: gl.get_uniform_location(program, "u_face"),
                u_alpha: gl.get_uniform_location(program, "u_alpha"),
                u_scale: gl.get_uniform_location(program, "u_scale"),
                u_apply_texture_spec: gl.get_uniform_location(program, "u_apply_texture_spec"),
                program,
                vao,
                textures,
                pip_cache: HashMap::new(),
                pip_texture_size,
            })
        }
    }

    pub fn paint(
        &self,
        gl: &glow::Context,
        rotation_y: f32,
        scale: f32,
        alpha: f32,
        texture_index: usize,
        apply_texture_spec: bool,
        viewport_px: [i32; 4],
    ) {
        // Skip the whole draw when the badge is functionally
        // invisible. Covers the early startup-hold beat (alpha=0)
        // and any pose where the disc has shrunk to a sub-pixel
        // mote — saves shading the ~6000 torus + cylinder verts
        // when there's nothing to show.
        if alpha < 0.001 || scale < 0.001 {
            return;
        }
        let texture = match self.textures.get(texture_index) {
            Some(t) => *t,
            // Out-of-range index is a programming error in the call
            // site; falling back to index 0 (QR) keeps the launcher
            // visible rather than blanking the badge while a logged
            // warning surfaces the bug.
            None => {
                tracing::warn!(
                    "badge paint with out-of-range texture_index {texture_index} (have {})",
                    self.textures.len()
                );
                self.textures[0]
            }
        };
        unsafe {
            gl.viewport(
                viewport_px[0],
                viewport_px[1],
                viewport_px[2],
                viewport_px[3],
            );
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

            // Depth test on so the torus's inner half (which dives
            // into the disc body) gets occluded by the disc faces
            // without any sort-order tricks. The depth buffer state
            // at callback entry could be anything (egui doesn't use
            // it, doesn't clear it), so we clear it ourselves to
            // 1.0 first. Egui doesn't read depth, so we don't have
            // to restore the previous depth contents.
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.depth_mask(true);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);

            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);

            gl.use_program(Some(self.program));
            gl.uniform_1_f32(self.u_rotation_y.as_ref(), rotation_y);
            gl.uniform_1_f32(self.u_alpha.as_ref(), alpha);
            gl.uniform_1_f32(self.u_scale.as_ref(), scale);
            gl.uniform_1_i32(
                self.u_apply_texture_spec.as_ref(),
                if apply_texture_spec { 1 } else { 0 },
            );

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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

            // Torus ring (gold). 6 verts per (u, v) quad ×
            // TORUS_MAJOR × TORUS_MINOR quads = 6144 verts at
            // 64 × 16. Independent triangles, not strips, because
            // the procedural vid → quad mapping is simpler than
            // strip-with-degenerates and the triangle count is
            // small enough that the redundant-vertex overhead is
            // immaterial.
            gl.uniform_1_i32(self.u_face.as_ref(), 3);
            gl.draw_arrays(glow::TRIANGLES, 0, 6 * TORUS_MAJOR * TORUS_MINOR);

            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);

            // Restore egui's expected state: cull-face off (egui
            // assumes none); depth test off (egui doesn't sort).
            // Don't bother restoring depth_mask — egui doesn't
            // touch the depth buffer either way.
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::DEPTH_TEST);
        }
    }

    /// Look up (or rasterise + upload, on cache miss) the pip-face
    /// texture for a `(color, initial, ghost)` triple and return its
    /// `textures` index. Called from `paint_pip` so the upload
    /// happens on the GL thread inside a `PaintCallback`. Cache is
    /// keyed on the same triple so repeated frames hit it cleanly.
    fn ensure_pip_texture(
        &mut self,
        gl: &glow::Context,
        color: [u8; 3],
        initial: char,
        ghost: bool,
    ) -> usize {
        let key = (
            u32::from_be_bytes([color[0], color[1], color[2], 0]),
            initial,
            ghost,
        );
        if let Some(&idx) = self.pip_cache.get(&key) {
            return idx;
        }
        let pixels =
            crate::badge_text::render_pip(color, initial, ghost, self.pip_texture_size);
        unsafe {
            let tex = match gl.create_texture() {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("badge: pip texture create failed: {e}");
                    return 0;
                }
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
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
                self.pip_texture_size as i32,
                self.pip_texture_size as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(pixels.as_slice()),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
            let idx = self.textures.len();
            self.textures.push(tex);
            self.pip_cache.insert(key, idx);
            // Diagnostic for PLAN 10.7.9 (GPU usage growth): every
            // pip texture upload logs once. Steady-state gameplay
            // should bottom out at one entry per (active profile,
            // ghost-state) seen — repeated logs at the same key
            // means the cache isn't holding and we're churning
            // GPU memory.
            tracing::info!(
                "pip texture uploaded color={:02x}{:02x}{:02x} initial={initial:?} ghost={ghost} (cache={} textures={})",
                color[0],
                color[1],
                color[2],
                self.pip_cache.len(),
                self.textures.len(),
            );
            idx
        }
    }

    /// Paint a single pip — same coin geometry as the main badge,
    /// just with a profile-coloured + initial-glyph front face
    /// instead of the QR. Caller positions the pip via the
    /// PaintCallback's viewport (`viewport_px`) and supplies a
    /// rotation if the orbit should tilt the disc; first cut keeps
    /// pips face-on (rotation=0, scale=1).
    pub fn paint_pip(
        &mut self,
        gl: &glow::Context,
        rotation_y: f32,
        scale: f32,
        alpha: f32,
        color: [u8; 3],
        initial: char,
        ghost: bool,
        viewport_px: [i32; 4],
    ) {
        if alpha < 0.001 || scale < 0.001 {
            return;
        }
        let idx = self.ensure_pip_texture(gl, color, initial, ghost);
        // `apply_texture_spec = true` so the pip's profile-coloured
        // disc picks up the same Blinn-Phong sweep as the
        // surrounding gold ring + side wall + torus — reads as
        // "coloured face on a polished gold coin", not a flat dot.
        self.paint(gl, rotation_y, scale, alpha, idx, true, viewport_px);
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            for tex in &self.textures {
                gl.delete_texture(*tex);
            }
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
///
/// `rotation_y` / `scale` / `alpha` are the three animation
/// outputs from `LaunchPhase::badge_*_3d` (PLAN 10.7.6) —
/// rotation drives the multi-turn coin spin, scale inflates the
/// disc from 10% to full size over the spin, alpha handles the
/// quick fade-in at the start of intro and the post-spin fade-out
/// at the end of close. `texture_index` selects which of the
/// `BadgeRig`'s pre-uploaded textures to bind (0 = QR, 1.. =
/// back-face strings); `apply_texture_spec` toggles the
/// Blinn-Phong term on the texture sample (off for QR's printed-
/// paper look, on for back-face metal). `paint` short-circuits the
/// draw entirely when either scale or alpha is negligible.
pub fn paint_badge(
    painter: &egui::Painter,
    rect: Rect,
    rig: Arc<Mutex<Option<BadgeRig>>>,
    rotation_y: f32,
    scale: f32,
    alpha: f32,
    texture_index: usize,
    apply_texture_spec: bool,
) {
    let cb = egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let viewport_px = [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
            if let Some(rig) = rig.lock().unwrap().as_ref() {
                rig.paint(
                    painter.gl(),
                    rotation_y,
                    scale,
                    alpha,
                    texture_index,
                    apply_texture_spec,
                    viewport_px,
                );
            }
        })),
    };
    painter.add(cb);
}

/// Paint a single pip into `rect` via an `egui::PaintCallback`
/// (PLAN 10.7.8). Pip = a shrunken coin reusing the badge's GL
/// pipeline (gold ring + side wall + torus + Lambert + spec) with
/// a per-profile front-face texture (coloured disc + centred
/// initial). Texture is rasterised + uploaded on cache miss inside
/// the callback so call sites don't need GL access — they just
/// supply the profile spec, the pip rect, and the alpha curve.
#[allow(clippy::too_many_arguments)]
pub fn paint_pip(
    painter: &egui::Painter,
    rect: Rect,
    rig: Arc<Mutex<Option<BadgeRig>>>,
    rotation_y: f32,
    scale: f32,
    alpha: f32,
    color: [u8; 3],
    initial: char,
    ghost: bool,
) {
    let cb = egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let viewport_px = [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px];
            if let Some(rig) = rig.lock().unwrap().as_mut() {
                rig.paint_pip(
                    painter.gl(),
                    rotation_y,
                    scale,
                    alpha,
                    color,
                    initial,
                    ghost,
                    viewport_px,
                );
            }
        })),
    };
    painter.add(cb);
}
