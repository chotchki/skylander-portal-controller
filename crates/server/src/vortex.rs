//! Procedural cloud vortex — PLAN 4.15.5.
//!
//! The canonical design (in `docs/aesthetic/mocks/tv_launcher_v3.html`) is a
//! 5-octave simplex-FBM WebGL fragment shader. Porting that to WGSL and
//! wiring an `egui_wgpu` custom paint callback is the long-term "Path A"
//! from the plan. This module ships the *visual shape* via a cheap
//! polar-mesh approximation driven by egui's native `Painter::add(Mesh)` —
//! no wgpu integration, no shader compilation, no new crate deps. The
//! render cost is ~2k triangles per frame, negligible on the launcher's
//! 250ms repaint cadence.
//!
//! What matches the mock:
//! - concentric density gradient (deep blue at centre → bright blue-white
//!   at edges, modulated by the iris);
//! - 10 rotating spiral arms drifting at `rotation_speed` rad/s;
//! - central hole kept clear for the QR + title;
//! - iris open/close via `iris_radius` knob (0.0 → no clouds, 1.6 →
//!   fills past the screen edges).
//!
//! What doesn't:
//! - simplex-FBM noise texture — replaced with a cheap sin/cos sum that
//!   reads as "bands of density" rather than "organic fluff". The eye
//!   fills in the rest on a 10-foot TV.
//! - "inflow" (radial noise scroll) — the current sin/cos doesn't have a
//!   meaningful analogue; folded into a phase shift on the band noise.
//!
//! Follow-up: real shader port lives in a future `vortex_wgpu` module
//! that swaps `eframe`'s backend to `wgpu` and drops a WGSL port of the
//! HTML mock's FBM. Tracked against 4.15a polish.

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use crate::palette;

/// Tunables for the vortex, mirroring the HTML mock's three knobs
/// (irisRadius / rotationSpeed / inflowSpeed). Defaults approximate the
/// mock's "idle" state — steady slow swirl, clouds fill most of the
/// frame.
#[derive(Debug, Clone, Copy)]
pub struct VortexParams {
    /// 0.0 = no clouds visible (iris fully closed to a dot).
    /// 1.6 = clouds fill past the screen corners.
    pub iris_radius: f32,
    /// Arm rotation in radians per second. Positive = clockwise on screen.
    pub rotation_speed: f32,
    /// Radial inflow speed — folded into the band-noise phase drift.
    pub inflow_speed: f32,
}

impl Default for VortexParams {
    fn default() -> Self {
        Self {
            iris_radius: 1.2,
            rotation_speed: 0.08,
            inflow_speed: 0.18,
        }
    }
}

/// Number of radial bands in the mesh. More = finer density gradient,
/// more triangles. 24 gives a smooth ramp at 10 ft on an 86" TV.
const RADIAL_BANDS: usize = 24;
/// Number of angular segments. Must be a multiple of `ARM_COUNT` for the
/// arm bands to line up cleanly on vertex boundaries.
const ANGULAR_SEGMENTS: usize = 60;
/// Matches the mock's 10-arm iris for visual continuity with the design.
const ARM_COUNT: f32 = 10.0;

/// Paint the cloud vortex into `rect`, filling the background behind the
/// launcher content. Call *before* any other widgets in the same frame
/// so they layer on top. `time_s` is the monotonic animation clock.
pub fn draw(painter: &egui::Painter, rect: Rect, time_s: f32, params: VortexParams) {
    let centre = rect.center();
    // Normalise by the shorter axis so the vortex is round on any aspect
    // ratio, matching the mock's `u_resolution.y` division.
    let radial_scale = rect.height().min(rect.width()) * 0.5;

    let mut mesh = Mesh::default();

    // Generate polar-grid vertices: (ANGULAR_SEGMENTS + 1) × (RADIAL_BANDS + 1).
    // +1 on the angular axis so the seam closes cleanly; +1 on radial so the
    // outer ring reaches the screen edge.
    for ri in 0..=RADIAL_BANDS {
        // r goes 0..iris_radius. We push the outer edge slightly past
        // `iris_radius` so the smoothstep cutoff has room to soften.
        let r_norm = ri as f32 / RADIAL_BANDS as f32;
        let r = r_norm * params.iris_radius;

        for ti in 0..=ANGULAR_SEGMENTS {
            let theta = (ti as f32 / ANGULAR_SEGMENTS as f32) * std::f32::consts::TAU;

            let x = theta.cos() * r * radial_scale;
            let y = theta.sin() * r * radial_scale;
            let pos = Pos2::new(centre.x + x, centre.y + y);

            let colour = sample_cloud_colour(r, theta, time_s, params);
            mesh.vertices.push(Vertex {
                pos,
                uv: egui::epaint::WHITE_UV,
                color: colour,
            });
        }
    }

    // Stitch quads into triangle pairs.
    let stride = (ANGULAR_SEGMENTS + 1) as u32;
    for ri in 0..RADIAL_BANDS as u32 {
        for ti in 0..ANGULAR_SEGMENTS as u32 {
            let a = ri * stride + ti;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            // Triangle 1: a, b, c
            mesh.indices.extend_from_slice(&[a, b, c]);
            // Triangle 2: b, d, c
            mesh.indices.extend_from_slice(&[b, d, c]);
        }
    }

    painter.add(egui::Shape::mesh(mesh));
}

/// Per-vertex cloud colour. Mirrors the structure of the mock's shader —
/// density + arm pattern + iris mask + centre hole + colour ramp — but
/// with a cheap sin/cos "noise" instead of simplex FBM. The three
/// 3-colour-ramp colours are pulled from the mock: deep-blue core,
/// mid-blue body, bright-blue wisps.
fn sample_cloud_colour(r: f32, theta: f32, time_s: f32, params: VortexParams) -> Color32 {
    // Spiral coordinate: same structure as the mock — theta offset grows
    // with r to create swirl arms; time drives rotation at
    // `rotation_speed`. The mock also folds `inflow_speed` into the
    // *radial* noise coord; we fold it into the band phase so there's
    // still perceptible cloud drift.
    let spiral = theta + r * 4.0 + time_s * params.rotation_speed;
    let band_phase = r * 3.5 + time_s * params.inflow_speed;

    // Cheap "band noise" — sum of three sinusoids at different
    // frequencies in spiral+radial space. Reads as banded density rather
    // than organic fluff but captures the right shape.
    let noise = (spiral * 2.3 + band_phase).sin() * 0.35
        + (spiral * 1.1 - band_phase * 0.6).cos() * 0.30
        + (spiral * 4.7 + band_phase * 0.3).sin() * 0.20;
    let cloud = (0.5 + noise * 0.5).clamp(0.0, 1.0);

    // 10 spiral arms, matching the mock. `smoothstep` approximation
    // via a re-map of sin into [0,1] with a contrast curve.
    let arm_raw = (spiral * ARM_COUNT).sin() * 0.5 + 0.5;
    let arm = smoothstep(0.05, 0.95, arm_raw);
    let arm_influence = mix(0.1, 0.38, smoothstep(0.0, 0.6, r));
    let mut cloud = cloud * mix(1.0, arm, arm_influence);

    // Depth / perspective cue — inner clouds dimmer, outer brighter.
    let depth = mix(0.25, 1.0, smoothstep(0.0, 0.65, r).powf(0.9));
    cloud *= depth;

    // Iris mask — 0 past the iris radius with a soft edge. Cheapest
    // smoothstep from `iris_radius + 0.2` → `iris_radius - 0.2`.
    let iris_edge = 0.2;
    let iris = smoothstep(
        params.iris_radius + iris_edge,
        params.iris_radius - iris_edge,
        r,
    );

    // Central hole — keep the very centre clear for the QR / heading.
    let centre_hole = smoothstep(0.05, 0.22, r);

    // Corner vignette — very gentle, only at extreme corners.
    let vignette = 1.0 - smoothstep(1.05, 1.45, r) * 0.45;

    let alpha = (cloud * iris * centre_hole * vignette * 1.25).clamp(0.0, 0.96);

    // 3-stop colour ramp: SF_1 (#0b1e52) → mid-blue (#2d5ab8) → warm
    // bright (#c0d8ff). Matches the mock's `colorDeep` / `colorMid` /
    // `colorBright`.
    let colour_deep = rgba(0x0b, 0x1e, 0x52);
    let colour_mid = rgba(0x2d, 0x5a, 0xb8);
    let colour_bright = rgba(0xc0, 0xd8, 0xff);

    let mix_amount = cloud;
    let lower = lerp_colour(colour_deep, colour_mid, (mix_amount * 2.0).clamp(0.0, 1.0));
    let higher = lerp_colour(
        colour_mid,
        colour_bright,
        ((mix_amount - 0.5) * 2.0).clamp(0.0, 1.0),
    );
    let rgb = if mix_amount < 0.5 { lower } else { higher };

    // Premultiplied alpha — egui::Color32 stores premultiplied values.
    let a = (alpha * 255.0) as u8;
    Color32::from_rgba_premultiplied(
        ((rgb.0 as f32) * alpha) as u8,
        ((rgb.1 as f32) * alpha) as u8,
        ((rgb.2 as f32) * alpha) as u8,
        a,
    )
}

/// Paint the starfield backdrop into `rect`. Sparse procedural stars at
/// deterministic positions (seeded so they don't shimmer position frame-
/// to-frame), with a slow per-star alpha twinkle. Painted *before* the
/// vortex so clouds layer on top — but during the launcher's Startup beat
/// (PLAN 4.19.2a, vortex iris=0) the stars stand alone and the screen
/// reads as the "calm starry sky" the spec calls for.
///
/// Tuned for an 86" TV at ~10ft: ~120 stars distributed across the rect,
/// 1–2.5px equivalent at 1080p (auto-scaled with the rect's shorter
/// axis), three colour tints (white / warm gold / cool blue) so the field
/// has variety without going cartoony.
pub fn paint_starfield(painter: &egui::Painter, rect: Rect, time_s: f32) {
    const NUM_STARS: u32 = 120;
    const SEED: u32 = 0xCAFE_BABE;

    // Reference resolution is 1920×1080; star sizes scale with the shorter
    // rect axis so the density reads similarly on dev windows (900×1000)
    // and the HTPC's 4K (3840×2160).
    let scale = (rect.width().min(rect.height()) / 1080.0).max(0.5);
    let inset = 4.0 * scale; // keep stars off the absolute edge

    for i in 0..NUM_STARS {
        // Four independent hash draws per star: x, y, size+colour pick,
        // twinkle phase. Hashing the seed + index gives a stable layout
        // across frames and across launcher restarts.
        let h1 = star_hash(SEED.wrapping_add(i.wrapping_mul(0x9e37_79b9)));
        let h2 = star_hash(h1);
        let h3 = star_hash(h2);
        let h4 = star_hash(h3);

        let x = rect.left() + inset + (h1 as f32 / u32::MAX as f32) * (rect.width() - 2.0 * inset);
        let y = rect.top() + inset + (h2 as f32 / u32::MAX as f32) * (rect.height() - 2.0 * inset);

        let size_choice = h3 as f32 / u32::MAX as f32;
        // Most stars small (1px), a minority bigger (~2.5px) for the
        // "depth" cue the mock's preview shots have.
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
            // Warm gold tint — picks up the heraldic palette without
            // looking like the gold is leaking out of the bezels.
            (0xff, 0xe6, 0xb4)
        } else {
            // Cool blue tint — keeps the field from looking monotone
            // under a TV's gamma.
            (0xb4, 0xdc, 0xff)
        };

        // Per-star twinkle phase derived from another hash bit so adjacent
        // stars don't pulse in lockstep. Slow cycle (~6s) so it reads as
        // ambient sparkle, not strobe. Alpha bottoms out at ~50% so stars
        // never fully vanish.
        let phase = (h3 as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        let twinkle = 0.5 + 0.5 * (0.5 * (time_s * 1.05 + phase).sin() + 0.5);
        let alpha = (255.0 * twinkle) as u8;

        painter.circle_filled(
            egui::pos2(x, y),
            radius,
            Color32::from_rgba_unmultiplied(base.0, base.1, base.2, alpha),
        );
    }
}

/// Pseudo-random hash used by `paint_starfield` for deterministic star
/// layout. Cheap integer mix from <https://nullprogram.com/blog/2018/07/31/>;
/// we don't need cryptographic strength, just good distribution.
fn star_hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

fn rgba(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    (r, g, b)
}

fn lerp_colour(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as i32 - a.0 as i32) as f32 * t) as u8,
        (a.1 as f32 + (b.1 as i32 - a.1 as i32) as f32 * t) as u8,
        (a.2 as f32 + (b.2 as i32 - a.2 as i32) as f32 * t) as u8,
    )
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    // Standard GLSL smoothstep. Handles edge0 > edge1 (used for the iris
    // mask) via a plain division — no branch needed since the clamp
    // deals with the overshoot either way.
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// Silence unused-import warnings for items we may add later during polish.
#[allow(dead_code)]
fn _unused_refs() -> (Vec2, Stroke, Color32) {
    (Vec2::ZERO, Stroke::NONE, palette::SF_3)
}
