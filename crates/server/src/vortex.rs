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

/// Paint the full sky backdrop: vertical base gradient + top/bottom
/// hue ellipses. Three layers, matching the mock's `.sky` element
/// (`tv_launcher_v3.html` lines 36-43):
///
///   1. **Vertical gradient** (`SF_1` at top → `SF_2` at 60% down →
///      `SF_3` at bottom) — the base. The launcher's `panel_fill` is
///      flat `SF_3`, so without this layer everything reads "all dark
///      bottom blue" and the upper half loses depth. We paint over the
///      panel fill rather than reach into egui's Visuals to make the
///      panel transparent — slightly redundant but simple.
///   2. **Top ellipse** — `#1a3a8a`-tinted, wide + relatively short
///      (mock: 120% × 60%), centred at top-middle. Brightens the upper
///      portion further; reads as the "sky lifting" toward where the
///      logo / QR will be.
///   3. **Bottom ellipse** — `#0e2464`-tinted, narrower + shorter
///      (mock: 80% × 50%), centred at bottom-middle. Adds a subtle
///      foreground depth without competing with the top.
///
/// Painted *before* `paint_starfield` + `vortex::draw` so stars + clouds
/// layer on top. Always-on backdrop — no time argument because the
/// gradient + glows are static (moving elements are stars + vortex).
pub fn paint_sky_background(painter: &egui::Painter, rect: Rect) {
    // 1. Vertical gradient base. Mock's CSS spec is mid-stop at 60%
    // (`linear-gradient(180deg, var(--sf-1), var(--sf-2) 60%,
    // var(--sf-3))`) but on a real fullscreen render that pulls the
    // near-black SF_3 too high up the panel — the bottom 40% reads
    // as ink rather than ocean. Pushing the mid-stop to 85% keeps
    // most of the panel in the SF_1→SF_2 range and reserves SF_3 for
    // the very bottom edge, which matches the mock's actual on-TV
    // look (Chris's screenshot 2026-04-19).
    paint_vertical_gradient(
        painter,
        rect,
        palette::SF_1,
        0.85,
        palette::SF_2,
        palette::SF_3,
    );

    // 2. Top ellipse. Mock spec proportions (60% × 30% half-w/h)
    // produce a hotspot too bright when overlaid on the gradient
    // base; alpha dialed down to 70 (from the original 130) so it
    // reads as ambient depth, not a spotlight.
    paint_radial_ellipse(
        painter,
        Pos2::new(rect.center().x, rect.top()),
        rect.width() * 0.6,
        rect.height() * 0.3,
        Color32::from_rgba_unmultiplied(0x1a, 0x3a, 0x8a, 70),
    );

    // 3. Bottom ellipse. Same alpha-down treatment — ambient depth,
    // not a focal element. Half-width pushed past the panel edges
    // (0.6 instead of mock-spec 0.4) so the bottom corners get tinted
    // too — Chris flagged 2026-04-19 that the corners were reading as
    // dark voids when only the centre band was lit. Rim alpha is 0
    // either way, so painting past the edges costs nothing visible
    // beyond what gets clipped.
    paint_radial_ellipse(
        painter,
        Pos2::new(rect.center().x, rect.bottom()),
        rect.width() * 0.6,
        rect.height() * 0.25,
        Color32::from_rgba_unmultiplied(0x0e, 0x24, 0x64, 60),
    );
}

/// Paint a vertical 3-stop linear gradient as a rect-filling mesh.
/// The colour at `mid_pos` (0.0..=1.0 from top) is `mid_color`; top is
/// `top_color`, bottom is `bot_color`. egui smooth-shades vertex colours
/// so the result reads as a continuous gradient. Sharp corners — caller
/// is responsible for hiding them under a rounded layer if needed.
/// Used by `paint_sky_background` for the SF_1 → SF_2 → SF_3 base, and
/// by `paint_bezel` for the gold-bezel body's vertical light → dark
/// ramp.
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
    // 6 vertices: 3 rows × 2 columns. Order: TL, TR, ML, MR, BL, BR.
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
    // Top quad (TL, TR, ML, MR) → 2 triangles.
    mesh.indices.extend([0, 1, 2, 1, 3, 2]);
    // Bottom quad (ML, MR, BL, BR) → 2 triangles.
    mesh.indices.extend([2, 3, 4, 3, 5, 4]);
    painter.add(egui::Shape::mesh(mesh));
}

/// Paint a single soft ellipse that fades from `center_color` at the
/// centre to fully transparent at the rim. Triangle-fan with one
/// centre vertex + N rim vertices; egui smooth-shades the alpha
/// between them, giving a passable radial-gradient look without
/// needing a custom shader. Useful as a general-purpose glow primitive
/// — sky background uses it for the top + bottom hue washes, the
/// heraldic title (`paint_heraldic_title`) uses it for the soft gold
/// halo behind the text.
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

/// Paint the starfield backdrop into `rect`. Sparse procedural stars
/// at deterministic *angular* positions (seeded so the field doesn't
/// shimmer frame-to-frame), with two animations:
///
///   1. **Radial outward drift** — each star travels from near the
///      panel centre toward the edges along a fixed bearing, then
///      wraps back to centre. Reads as "coming out of the screen,"
///      which doesn't conflict with the vortex's own rotation (the
///      first cut used a diagonal pan that fought the iris). Per-star
///      fade-in near centre + fade-out near edge hides the wrap so
///      no teleport is visible. Rate: ~5 px/s — half the previous
///      diagonal speed per Chris's feedback 2026-04-19, since the
///      effect is meant to be subtle.
///   2. **Per-star alpha twinkle** — slow ~6s cycle, alpha bottoms at
///      ~50%. Phase hashed per-star so neighbours don't pulse in
///      lockstep.
///
/// Density: ~36 stars across the rect. Three colour tints (white,
/// warm gold, cool blue) for variety. Painted *before* the vortex so
/// clouds layer on top during Awaiting Connect; during Startup
/// (vortex iris=0) the stars stand alone.
pub fn paint_starfield(painter: &egui::Painter, rect: Rect, time_s: f32) {
    const NUM_STARS: u32 = 36;
    const SEED: u32 = 0xCAFE_BABE;
    /// Pixels per second along the radial outward bearing. Subtle —
    /// over 30s a star travels ~150px, which on a 1080p panel is
    /// visible motion without dominating the visual frame.
    const DRIFT_PX_PER_SEC: f32 = 5.0;
    /// Fraction of the drift cycle at each end where stars fade
    /// (in near centre, out near edge). 0.15 gives a smooth handoff
    /// without making the visible-motion middle too short.
    const FADE_FRAC: f32 = 0.15;

    // Reference resolution is 1920×1080; star sizes scale with the shorter
    // rect axis so the density reads similarly on dev windows (900×1000)
    // and the HTPC's 4K (3840×2160).
    let scale = (rect.width().min(rect.height()) / 1080.0).max(0.5);
    let centre = rect.center();
    // Max radius = the distance from centre to the panel corner. Stars
    // disappear (fade) before they reach this, so they're never painted
    // outside the rect.
    let max_radius = ((rect.width() * 0.5).powi(2) + (rect.height() * 0.5).powi(2))
        .sqrt()
        .max(1.0);
    // Drift advances the cycle phase. One full cycle (centre → edge →
    // wrap) = max_radius / DRIFT_PX_PER_SEC seconds.
    let cycle_offset = (time_s * DRIFT_PX_PER_SEC / max_radius).rem_euclid(1.0);

    for i in 0..NUM_STARS {
        // Four independent hash draws per star: bearing (theta), initial
        // cycle phase, size+colour pick, twinkle phase. Hashing the seed
        // + index gives a stable layout across frames and restarts.
        let h1 = star_hash(SEED.wrapping_add(i.wrapping_mul(0x9e37_79b9)));
        let h2 = star_hash(h1);
        let h3 = star_hash(h2);
        let h4 = star_hash(h3);

        // Bearing — the fixed angle at which this star travels outward.
        let theta = (h1 as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        // Initial phase along the cycle in [0, 1). Plus the global
        // cycle_offset, modulo 1.0, so stars are spread along the
        // cycle at any moment (some near centre, some near edge).
        let base_phase = h2 as f32 / u32::MAX as f32;
        let depth = (base_phase + cycle_offset).rem_euclid(1.0);

        // Position: centre + (cos, sin) * depth*max_radius.
        let r = depth * max_radius;
        let x = centre.x + theta.cos() * r;
        let y = centre.y + theta.sin() * r;

        // Skip stars whose bearing happens to land outside the rect
        // before they reach max_radius (panel isn't square; corners
        // are farther than the cardinal sides). Cheap rect-contains
        // check, no allocation.
        if x < rect.left() || x > rect.right() || y < rect.top() || y > rect.bottom() {
            continue;
        }

        // Smooth fade in (near centre) + out (near edge) so the wrap
        // is invisible. life_alpha goes 0→1 over [0..FADE_FRAC], stays
        // 1 in the middle, then 1→0 over [1-FADE_FRAC..1].
        let fade_in = (depth / FADE_FRAC).clamp(0.0, 1.0);
        let fade_out = ((1.0 - depth) / FADE_FRAC).clamp(0.0, 1.0);
        let life_alpha = fade_in.min(fade_out);

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
        // Final alpha = twinkle * life_alpha. The life_alpha multiplier
        // is the in-out fade that hides the centre→edge wrap.
        let alpha = (255.0 * twinkle * life_alpha) as u8;

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
