//! Main launcher surface — QR + heading + status strip (PLAN 4.15.1–.4).
//!
//! This is the default screen the launcher boots into: a starfield backdrop
//! (provided by [`crate::vortex`]), a gold-bezeled QR for the phone pairing
//! URL, a `SKYLANDER PORTAL` heading in Titan One, and a status strip with
//! the RPCS3 connection dot + current-game label.
//!
//! PLAN 4.15.6 bolts on a Y-axis card-flip that covers the QR with a
//! `MAXIMUM PLAYERS REACHED` back face when the session registry hits
//! `MAX_SESSIONS`. PLAN 4.15.7 paints a slow ellipse of gold-bezeled pips
//! around the QR — one per connected phone session, each tinted with the
//! session's unlocked-profile colour + initial.

use std::f32::consts::TAU;
use std::sync::atomic::Ordering;

use super::LauncherApp;
use crate::state::{LauncherStatus, SessionPip};
use crate::{fonts, palette};

/// Fixed square edge for the QR card. Matches the mock's `.qr-flipper`
/// (280px) scaled up for a 10 ft TV read. The QR image itself is generated
/// at whatever size `render_qr_texture` produces; here we just reserve the
/// frame so the card-flip animation has a stable rect to shrink/grow into.
const CARD_SIZE: f32 = 320.0;

/// How long the QR → back-face flip animation takes, in seconds. `egui`'s
/// `animate_bool_with_time` uses this as the full 0→1 transition; we split
/// it into two halves (front scaling to 0, then back scaling from 0).
const FLIP_DURATION: f32 = 0.5;

/// Pip diameter in screen pixels. The mock uses 84px; we keep the same
/// physical size so the pip text is legible at 10 ft.
const PIP_DIAMETER: f32 = 84.0;

/// Orbit rotation rate (rad/s). Quiet idle motion — matches the vortex's
/// 0.08 rad/s so the two ambient animations don't compete for attention.
const ORBIT_SPEED: f32 = 0.10;

impl LauncherApp {
    /// Render the gold "STARTING" title, centred on the panel. Used by
    /// the launcher Startup beat (PLAN 4.19.2a) to give the calm-starfield
    /// window a focal element — without it the screen reads as broken for
    /// the first second(s). The copy matches the spec's "state title"
    /// pattern (§3.1 state 1 reads as the launcher waking up); the full
    /// Main surface (QR + status + orbit pips) takes over after the
    /// transition completes.
    ///
    /// Painted via direct `Painter::text` calls (not `ui.heading`) so the
    /// embossed shadow stack + outer glow can land — `RichText` only
    /// supports a single solid colour, which produced the flat sticker
    /// look Chris flagged 2026-04-19. See `paint_heraldic_title`.
    pub(super) fn render_brand_intro(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let pos = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.5);
        // Spec §3.7 calls 96px the "TV display hero" floor for state
        // titles. The mock's actual rendered title is visibly larger —
        // takes ~50% of panel width vs ours at 25%. Bumped to 140px to
        // match the mock's on-screen presence (Chris's screenshot
        // 2026-04-19). render_main's "SKYLANDER PORTAL" steady-state
        // title is a separate drift item (4.19.19) still on 80px.
        paint_heraldic_title(ui.painter(), pos, "STARTING", 140.0);
    }

    /// Render the Main surface. Called from the top-level dispatcher in
    /// [`super`] when `LauncherStatus::screen == LauncherScreen::Main`.
    pub(super) fn render_main(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        status_snapshot: &LauncherStatus,
    ) {
        // Paint the orbit pips *before* any widgets lay out. Immediate-mode
        // egui paints shapes in submission order, so these land above the
        // vortex (already drawn by the dispatcher) but below the heading
        // and QR rendered below — matching the mock's z-index 9 orbit
        // passing behind z-index 10 text.
        let panel_rect = ui.max_rect();
        paint_player_orbit(
            ui.painter(),
            panel_rect,
            ctx.input(|i| i.time) as f32,
            &status_snapshot.session_profiles,
        );

        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            status_strip(ui, status_snapshot);
            ui.add_space(8.0);
            ui.heading(
                egui::RichText::new("SKYLANDER PORTAL")
                    .size(80.0)
                    .color(palette::GOLD)
                    .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
            );
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("Scan to connect:")
                    .size(36.0)
                    .color(palette::TEXT_DIM),
            );
            ui.add_space(24.0);
            if let Some(tex) = &self.qr_texture {
                // The card-flip helper reserves a fixed-size square and
                // paints the front / back face directly via Painter so the
                // Y-axis rotation reads as a horizontal scale without any
                // custom matrix math — egui-native, see the helper doc.
                qr_card_flip(ui, ctx, tex, status_snapshot.session_slots_full);
            }
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new(&self.url)
                    .size(32.0)
                    .monospace()
                    .color(palette::GOLD),
            );
            ui.add_space(16.0);

            let n = self.clients.load(Ordering::Relaxed);
            let status = if n == 0 {
                "Waiting for phone…".to_string()
            } else if n == 1 {
                "1 device connected".to_string()
            } else {
                format!("{n} devices connected")
            };
            ui.label(egui::RichText::new(status).size(40.0).color(palette::TEXT));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{} figures indexed", self.figure_count))
                    .size(24.0)
                    .color(palette::TEXT_DIM),
            );

            ui.add_space(32.0);
            let btn = egui::Button::new(
                egui::RichText::new("Exit to Desktop")
                    .size(28.0)
                    .color(palette::TEXT),
            )
            .fill(palette::DANGER)
            .rounding(egui::Rounding::same(16.0))
            .min_size(egui::vec2(260.0, 60.0));
            if ui.add(btn).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

/// Header strip: RPCS3 connection dot + current-game label (PLAN 4.15.4).
/// Absorbs the 2.8.4 deferral — a steady green dot while the emulator is
/// running, dim grey otherwise. The current-game name renders in Titan
/// One gold when a game is booted; blank otherwise.
fn status_strip(ui: &mut egui::Ui, status: &LauncherStatus) {
    const DOT_RADIUS: f32 = 10.0;
    let (dot_colour, tooltip) = if status.rpcs3_running {
        (palette::SUCCESS_GLOW, "RPCS3 running")
    } else {
        (palette::TEXT_DIM, "RPCS3 idle")
    };

    ui.horizontal(|ui| {
        // Let the strip grow to the panel width so `with_layout` centering
        // inside `vertical_centered` gives us the full row to work with.
        ui.set_min_width(ui.available_width());
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space(24.0);
            // Dot — allocate a small square and paint a circle in its centre.
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(DOT_RADIUS * 2.0 + 4.0, DOT_RADIUS * 2.0 + 4.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .circle_filled(rect.center(), DOT_RADIUS, dot_colour);
            // Subtle outer ring for contrast against the starfield background.
            ui.painter().circle_stroke(
                rect.center(),
                DOT_RADIUS,
                egui::Stroke::new(1.5, palette::GOLD_INK),
            );
            response.on_hover_text(tooltip);

            ui.add_space(12.0);
            match &status.current_game {
                Some(name) => {
                    ui.label(
                        egui::RichText::new(name)
                            .size(26.0)
                            .color(palette::GOLD)
                            .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new("no game running")
                            .size(22.0)
                            .italics()
                            .color(palette::TEXT_DIM),
                    );
                }
            }
        });
    });
}

/// Card-flip container (PLAN 4.15.6). Reserves a `CARD_SIZE × CARD_SIZE`
/// square and paints either the QR front face or the `MAXIMUM PLAYERS
/// REACHED` back face, with a tent-wave horizontal scale to simulate a
/// Y-axis rotation.
///
/// egui doesn't ship 3D transforms, but a Y-axis rotation in 2D reads
/// identically to an X-axis scale (the vertical edges stay stationary;
/// the face just compresses horizontally, crosses zero at the midpoint,
/// and expands back). `animate_bool_with_time` drives the 0→1 progress;
/// the scale is `|2·progress − 1|`, peaking at 0 in the middle where we
/// swap content. `FLIP_DURATION` sets the overall transition time.
fn qr_card_flip(ui: &mut egui::Ui, ctx: &egui::Context, tex: &egui::TextureHandle, flipped: bool) {
    // `animate_bool_with_time` interpolates 0.0 → 1.0 when `flipped` goes
    // true, and 1.0 → 0.0 when it goes false. Unique id so multiple
    // instances don't alias animations.
    let id = egui::Id::new("launcher_qr_card_flip");
    let progress = ctx.animate_bool_with_time(id, flipped, FLIP_DURATION);

    // Tent wave: 1 at the edges, 0 in the middle. Represents the x-scale
    // of the visible face; hits 0 at the flip midpoint.
    let scale = (progress * 2.0 - 1.0).abs();
    let show_back = progress > 0.5;

    // Reserve the full square so layout below doesn't shift during the
    // animation.
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());

    // Squish horizontally, keep vertical alignment.
    let half_w = (rect.width() * scale) * 0.5;
    let inner =
        egui::Rect::from_center_size(rect.center(), egui::vec2(half_w * 2.0, rect.height()));

    // Guard against zero-width rendering — egui's rounding + stroke paint
    // gets messy at 0 extent, and nothing's visible anyway at the midpoint.
    if inner.width() < 1.0 {
        return;
    }

    let painter = ui.painter();
    if show_back {
        paint_back_face(painter, inner);
    } else {
        paint_qr_front(painter, inner, tex);
    }
}

/// Front face — gold bezel framing the QR. Equivalent to the stacked
/// `egui::Frame` version in the pre-4.15.6 code, but painted directly via
/// the `Painter` API so we can drive a per-frame horizontal scale.
fn paint_qr_front(painter: &egui::Painter, rect: egui::Rect, tex: &egui::TextureHandle) {
    // Outer gold body with a `GOLD_INK` hairline stroke.
    painter.rect(
        rect,
        egui::Rounding::same(14.0),
        palette::GOLD,
        egui::Stroke::new(2.0, palette::GOLD_INK),
    );
    // Inner bezel plate — the darker `SF_3` rim that frames the QR itself.
    let plate = rect.shrink(18.0);
    painter.rect(
        plate,
        egui::Rounding::same(8.0),
        palette::SF_3,
        egui::Stroke::new(1.0, palette::GOLD_SHADOW),
    );
    // The QR texture. Fill the plate minus a small quiet-zone margin so
    // the dark plate peeks through as a contrast rim.
    let qr_rect = plate.shrink(10.0);
    painter.image(
        tex.id(),
        qr_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

/// Back face — same bezel geometry, Titan One gold text reading
/// `MAXIMUM PLAYERS REACHED`. Matches the mock's state 5 copy split
/// across three lines so the letters stay big at 10 ft.
fn paint_back_face(painter: &egui::Painter, rect: egui::Rect) {
    // Gold bezel body (identical to the front face so the flip has a
    // stable silhouette).
    painter.rect(
        rect,
        egui::Rounding::same(14.0),
        palette::GOLD,
        egui::Stroke::new(2.0, palette::GOLD_INK),
    );
    // Plate with a warmer blue fill than `SF_3` — the mock uses a
    // downward blue gradient. We approximate with a flat `SF_1` which is
    // the brightest of the three starfield blues.
    let plate = rect.shrink(18.0);
    painter.rect(
        plate,
        egui::Rounding::same(8.0),
        palette::SF_1,
        egui::Stroke::new(1.0, palette::GOLD_SHADOW),
    );
    // Three-line title. Sized to fill the plate vertically; the mock
    // splits at "MAXIMUM / PLAYERS / REACHED" so each word gets its own
    // line and the letter-spacing stays readable.
    let lines = ["MAXIMUM", "PLAYERS", "REACHED"];
    let line_h = plate.height() / lines.len() as f32;
    for (i, word) in lines.iter().enumerate() {
        let y = plate.top() + line_h * (i as f32 + 0.5);
        painter.text(
            egui::pos2(plate.center().x, y),
            egui::Align2::CENTER_CENTER,
            word,
            egui::FontId::new(44.0, egui::FontFamily::Name(fonts::TITAN_ONE.into())),
            palette::GOLD_BRIGHT,
        );
    }
}

/// Paint the player-orbit indicators (PLAN 4.15.7). Up to `MAX_SESSIONS`
/// gold-bezeled pips ride a slow ellipse centred on the panel; each pip is
/// tinted with the owning profile's colour and shows the profile's
/// initial. Called *before* the heading widget lays out so the pips sit
/// behind the text when their orbit intersects it (per the mock's
/// z-index 9 vs 10 layering).
///
/// The mock hard-codes `rx=560, ry=400` against a 1920×1080 surface. We
/// express the ellipse as a fraction of the panel's shorter axis so it
/// scales with the launcher's actual rect — dev builds are 900×1000, the
/// HTPC is 3840×2160, and both should look proportionate.
fn paint_player_orbit(painter: &egui::Painter, rect: egui::Rect, time_s: f32, pips: &[SessionPip]) {
    if pips.is_empty() {
        return;
    }
    let centre = rect.center();
    // Ellipse proportions matched to the mock (560×400 on a 1920×1080
    // frame → rx = 29% of width, ry = 37% of height). Clamped against
    // the shorter axis so portrait and landscape surfaces both stay
    // sensible.
    let rx = (rect.width() * 0.29).min(rect.width() * 0.45);
    let ry = (rect.height() * 0.37).min(rect.height() * 0.45);

    let base_phase = time_s * ORBIT_SPEED;
    for (i, pip) in pips.iter().enumerate().take(crate::profiles::MAX_SESSIONS) {
        // Distribute pips evenly around the orbit — 2 pips → 180° apart.
        let offset = TAU * (i as f32) / (crate::profiles::MAX_SESSIONS as f32);
        let t = base_phase + offset;
        let x = centre.x + t.cos() * rx;
        let y = centre.y + t.sin() * ry;
        let pip_rect =
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(PIP_DIAMETER, PIP_DIAMETER));
        paint_pip(painter, pip_rect, pip);
    }
}

/// Single pip — a gold-bezeled circle with the profile's colour as fill
/// and the initial rendered in Titan One. Unknown profile (None fields)
/// falls back to neutral gold + a small dot.
fn paint_pip(painter: &egui::Painter, rect: egui::Rect, pip: &SessionPip) {
    let centre = rect.center();
    let radius = rect.width() * 0.5;
    // Dark hairline well outside the bezel — matches the mock's
    // `0 0 0 2px #000` outer shadow, gives the pip a clean silhouette
    // against either the vortex or the starfield.
    painter.circle_stroke(
        centre,
        radius + 2.0,
        egui::Stroke::new(2.0, egui::Color32::BLACK),
    );
    // Gold bezel body.
    painter.circle_filled(centre, radius, palette::GOLD);
    // Inner gold-ink ring (the mock's `inset 0 0 0 3px var(--gi)`).
    painter.circle_stroke(
        centre,
        radius - 4.0,
        egui::Stroke::new(3.0, palette::GOLD_INK),
    );

    // Profile-colour fill inside the ring.
    let fill_radius = radius - 10.0;
    let fill = pip
        .color
        .as_deref()
        .and_then(parse_hex_color)
        .unwrap_or(palette::GOLD_BRIGHT);
    painter.circle_filled(centre, fill_radius, fill);

    // Initial (or a small dot if unknown).
    match pip.initial.as_deref() {
        Some(ch) if !ch.is_empty() => {
            painter.text(
                centre,
                egui::Align2::CENTER_CENTER,
                ch,
                egui::FontId::new(36.0, egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                palette::GOLD_INK,
            );
        }
        _ => {
            painter.circle_filled(centre, 6.0, palette::GOLD_INK);
        }
    }
}

/// Parse `#rrggbb` / `#rgb` into an `egui::Color32`. Profiles stored in
/// SQLite are validated to one of those two shapes, but be defensive —
/// a garbage value just falls through to `None` and the caller picks a
/// neutral default.
fn parse_hex_color(s: &str) -> Option<egui::Color32> {
    let s = s.strip_prefix('#')?;
    let (r, g, b) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ),
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            (r * 17, g * 17, b * 17)
        }
        _ => return None,
    };
    Some(egui::Color32::from_rgb(r, g, b))
}

/// Paint a Titan-One title with the heraldic embossed treatment from
/// `docs/aesthetic/design_language.md` §1: stacked shadow layers reading
/// as carved-into-the-bezel depth, plus a strong gold outer halo. egui's
/// `RichText` only ships a single solid colour per glyph, so the effect
/// is built by stacking multiple `Painter::text` calls in z-order. The
/// CSS spec the visual mimics:
///
///   text-shadow:
///     0 2px 0 var(--gs),                        ← carve, sharp
///     0 3px 0 var(--gi),                        ← carve, sharp
///     0 5px 10px rgba(0,0,0,0.5),               ← lift, blurred
///     0 0 24px rgba(245,198,52,0.25);           ← halo, wide blur
///
/// We can't do real Gaussian blur in the painter, so the blurred layers
/// (drop shadow + halo) are approximated by painting multiple copies on
/// concentric rings with falling alpha — accumulated additive paint
/// fakes the bloom. Tuned per Chris's feedback 2026-04-19 ("missing the
/// gold glow") — earlier single-ring 8-copy halo was too tight + too dim
/// to register against the dark starfield panel.
///
/// Layer order (bottom → top):
///   1. Outer gold halo (3 rings × 12 angular steps = 36 copies)
///   2. Soft black drop shadow (2 layers at +5/+7 px)
///   3. Carve under-layers (GOLD_SHADOW +2.5px, GOLD_INK +3.5px)
///   4. Bright body (GOLD_BRIGHT) on top
///
/// Called by `render_brand_intro` for the Startup beat. Render_main's
/// title still uses `ui.heading(...)` flat — switching that over is
/// 4.19.19's territory and would mean restructuring the layout stack.
fn paint_heraldic_title(painter: &egui::Painter, pos: egui::Pos2, text: &str, size: f32) {
    let font = egui::FontId::new(size, egui::FontFamily::Name(fonts::TITAN_ONE.into()));

    // 1. Outer gold halo. The first cut painted 36 offset copies of
    // the text glyph — but you could SEE the ghost copies (Chris
    // flagged 2026-04-19). The mock's CSS `text-shadow: 0 0 24px gold`
    // is a real Gaussian blur, which loses glyph detail and becomes a
    // soft blob roughly tracing the word's bounding box.
    //
    // We approximate that blob with a smooth radial-gradient ellipse
    // sized to the text bounds + halo padding. No glyph shape — just
    // a soft gold backlight. Looks like a real blur because it IS
    // smooth (vs. 36 offset copies that read as ghost letters).
    let galley = painter
        .ctx()
        .fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), palette::GOLD_BRIGHT));
    let text_size = galley.size();
    // Two stacked ellipses for a softer Gaussian-style falloff: wide
    // outer at low alpha (the diffuse outer glow), tighter inner at
    // medium alpha (the focal warmth around the text). A single
    // ellipse has a linear-ramp falloff that reads as harder-edged
    // than the mock's ~24px-blur shadow — the two-layer overpaint
    // approximates the soft Gaussian shoulder.
    // Outer ellipse uses pale GOLD_BRIGHT (#ffe58a) — fades to pale
    // gold at the rim. Inner uses richer GOLD (#f5c634) — keeps the
    // centre warm/honey instead of washing to white when the alphas
    // stack (Chris flagged 2026-04-19: pure GOLD_BRIGHT in both
    // layers reads white-hot at the centre, loses the gold tint).
    let outer = palette::GOLD_BRIGHT;
    let inner = palette::GOLD;
    let outer_pad = size * 1.45;
    let inner_pad = size * 0.75;
    crate::vortex::paint_radial_ellipse(
        painter,
        pos,
        text_size.x * 0.5 + outer_pad,
        text_size.y * 0.5 + outer_pad,
        egui::Color32::from_rgba_unmultiplied(outer.r(), outer.g(), outer.b(), 5),
    );
    crate::vortex::paint_radial_ellipse(
        painter,
        pos,
        text_size.x * 0.5 + inner_pad,
        text_size.y * 0.5 + inner_pad,
        egui::Color32::from_rgba_unmultiplied(inner.r(), inner.g(), inner.b(), 15),
    );

    // 2. Soft drop shadow. Two layers at +5/+7px below — the lower one
    // half-alpha — approximates a 10px blur. Title lifts off the
    // starfield instead of sitting flat on it.
    painter.text(
        pos + egui::vec2(0.0, (size * 0.06).max(4.0)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 130),
    );
    painter.text(
        pos + egui::vec2(0.0, (size * 0.085).max(6.0)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 70),
    );

    // 3. Carve under-layers. Two stacked offsets in deepening gold
    // tones produce the engraved-into-the-bezel depth the heraldic
    // treatment calls for. Offsets scale with the size so the depth
    // reads at any resolution. Sharp (no spread) — the engraving
    // shouldn't blur.
    painter.text(
        pos + egui::vec2(0.0, (size * 0.035).max(2.5)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        palette::GOLD_INK,
    );
    painter.text(
        pos + egui::vec2(0.0, (size * 0.022).max(1.5)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        palette::GOLD_SHADOW,
    );

    // 4. Bright body on top.
    painter.text(
        pos,
        egui::Align2::CENTER_CENTER,
        text,
        font,
        palette::GOLD_BRIGHT,
    );
}

/// Rasterise the pairing URL into a QR texture. Called once from
/// [`LauncherApp::new`]; the texture is cached on the app for the life of
/// the viewport.
pub(super) fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    let code = qrcode::QrCode::new(url).expect("qr encode");
    // QR renders in starfield-blue-on-white for readability. Matches the
    // phone's selection-on-dark treatment (dark modules on a white
    // quiet-zone background is what most QR scanners expect).
    let dark = palette::SF_2;
    let light = egui::Color32::WHITE;
    let scale = 10usize;
    let modules: Vec<Vec<bool>> = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build()
        .lines()
        .map(|l| l.chars().map(|c| c != ' ').collect())
        .collect();
    let h = modules.len();
    let w = modules.first().map(|r| r.len()).unwrap_or(0);
    let img_w = w * scale;
    let img_h = h * scale;
    let mut pixels = Vec::with_capacity(img_w * img_h);
    for y in 0..img_h {
        for x in 0..img_w {
            let b = modules[y / scale][x / scale];
            pixels.push(if b { dark } else { light });
        }
    }
    let color_image = egui::ColorImage {
        size: [img_w, img_h],
        pixels,
    };
    ctx.load_texture("qr", color_image, egui::TextureOptions::NEAREST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_six_char() {
        let c = parse_hex_color("#ff00aa").unwrap();
        assert_eq!(c, egui::Color32::from_rgb(0xff, 0x00, 0xaa));
    }

    #[test]
    fn hex_color_three_char_expands() {
        let c = parse_hex_color("#f0a").unwrap();
        // 0xf => 0xff, 0x0 => 0x00, 0xa => 0xaa (nibble * 17).
        assert_eq!(c, egui::Color32::from_rgb(0xff, 0x00, 0xaa));
    }

    #[test]
    fn hex_color_rejects_nonsense() {
        assert!(parse_hex_color("red").is_none());
        assert!(parse_hex_color("#xyz").is_none());
        assert!(parse_hex_color("").is_none());
        assert!(parse_hex_color("#ff00aabb").is_none());
    }
}
