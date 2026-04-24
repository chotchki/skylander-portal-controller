//! In-game transparent surface (PLAN 4.15.8).
//!
//! When an RPCS3 game is running, the launcher's egui viewport becomes
//! see-through so the game fills the TV. The launcher still owns the
//! topmost layer (`with_always_on_top` in `main.rs`) so corner overlays
//! can render over the game — the one that ships now is the reconnect
//! QR, shown only when *every* phone has disconnected.
//!
//! How transparency works:
//! - `ViewportBuilder::with_transparent(true)` enables OS-level window
//!   transparency (Win32 `WS_EX_LAYERED`).
//! - This screen paints the CentralPanel with `Color32::TRANSPARENT`
//!   fill, so pixels not explicitly drawn let the game through. The
//!   cloud vortex is deliberately skipped — it would defeat the point.
//!
//! Per PLAN 4.15a.5 (aesthetic refinement): the reconnect QR is *not*
//! always-on. It surfaces only when connected-clients drops to zero —
//! the "everyone left, anyone come back" cue — so gameplay isn't
//! cluttered with a persistent overlay.
//!
//! Visual form: gold-bezel coin matching the launcher's main QR card
//! (`ui/main_screen.rs::paint_qr_front`) but shrunk. The round noise
//! ring inside the `qr_texture` carries the circular silhouette on its
//! own; this screen just wraps it in a smaller gold ring + SF_3 screen
//! rim so it reads as a scaled-down version of the Main surface QR.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{fonts, palette};

/// Duration of the reconnect QR's ease-out fade-in (PLAN 4.19.12).
/// Measured from the moment `clients` hits 0 while the in-game
/// surface is being rendered.
const RECONNECT_FADE_IN_S: f32 = 1.0;

/// Render the in-game transparent surface. Called from the top-level
/// dispatcher in [`super`] when `LauncherStatus::rpcs3_running` is true
/// (and no Crashed / Farewell screen is active).
///
/// `reconnect_fade_elapsed_s` is the wall-clock time since the
/// reconnect-QR state became eligible (phones all left). 0.0 when
/// any phone is still connected.
pub(super) fn render(
    ui: &mut egui::Ui,
    clients: &Arc<AtomicUsize>,
    qr_texture: Option<&egui::TextureHandle>,
    reconnect_fade_elapsed_s: f32,
) {
    // Corner reconnect QR: only when every phone has disconnected.
    // Matches the 4.15a.5 refinement — not a persistent overlay.
    if clients.load(Ordering::Relaxed) == 0 {
        let progress = (reconnect_fade_elapsed_s / RECONNECT_FADE_IN_S).clamp(0.0, 1.0);
        let alpha = ease_out_cubic(progress);
        reconnect_qr(ui, qr_texture, alpha);
    }
}

/// Ease-out cubic: fast in, gentle landing. Matches the curve shape
/// used for launch_phase intro reveals so the reconnect QR fade has
/// the same rhythm as the main launcher's iris-reveal.
fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// Scale a Color32's alpha by `a` (0.0–1.0). Honors the existing
/// alpha — for opaque input colours this is equivalent to fading
/// from transparent to fully opaque.
fn with_alpha(c: egui::Color32, a: f32) -> egui::Color32 {
    let scaled = (c.a() as f32 * a.clamp(0.0, 1.0)).round() as u8;
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), scaled)
}

/// Gold-bezeled round QR coin in the upper-right, with RECONNECT /
/// "scan to rejoin" text above and below. Mirrors the Main surface's
/// QR card scaled for a corner overlay; the texture already carries the
/// circular silhouette via its noise ring + transparent corners.
///
/// `alpha` is the ease-out fade-in factor (0.0 → transparent, 1.0 →
/// fully visible). Multiplied uniformly across every paint so the
/// overlay reveals as a single piece rather than layered pops.
/// PLAN 4.19.12 — previously rendered at full opacity instantly.
fn reconnect_qr(ui: &mut egui::Ui, qr_texture: Option<&egui::TextureHandle>, alpha: f32) {
    let Some(tex) = qr_texture else { return };
    if alpha <= 0.001 {
        return;
    }

    // Visible diameter of the round QR coin. Larger than the prior
    // square panel's 180px because a circle reads smaller than the same
    // bounding-box square (same tradeoff PLAN 4.19.6 hit on the main
    // QR), and the coin replaces the panel outright here.
    const COIN_DIAMETER: f32 = 220.0;
    // Gold ring + dark screen rim thicknesses. Proportionally lighter
    // than the main screen's 24/14 so the small coin doesn't look
    // overbuilt at TV distance from across the room.
    const BEZEL_RING_PX: f32 = 14.0;
    const SCREEN_RIM_PX: f32 = 8.0;
    // Margin from the viewport corners to the coin's bounding box.
    const CORNER_MARGIN: f32 = 32.0;
    // Extra vertical room for the RECONNECT title + "scan to rejoin"
    // subtitle rendered above / below the coin.
    const LABEL_LANE: f32 = 44.0;

    let full = ui.max_rect();
    let panel_w = COIN_DIAMETER;
    let panel_h = COIN_DIAMETER + LABEL_LANE * 2.0;
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(
            full.right() - panel_w - CORNER_MARGIN,
            full.top() + CORNER_MARGIN,
        ),
        egui::vec2(panel_w, panel_h),
    );
    let painter = ui.painter();

    // Title — Titan One gold caption above the coin.
    painter.text(
        egui::pos2(
            panel_rect.center().x,
            panel_rect.top() + LABEL_LANE * 0.5,
        ),
        egui::Align2::CENTER_CENTER,
        "RECONNECT",
        egui::FontId::new(
            palette::CAPTION,
            egui::FontFamily::Name(fonts::TITAN_ONE.into()),
        ),
        with_alpha(palette::GOLD, alpha),
    );

    // Round coin: gold ring → SF_3 screen rim → QR texture. Same
    // layering pattern as the Main surface so the two reads as a single
    // design language, just scaled.
    let coin_center = egui::pos2(
        panel_rect.center().x,
        panel_rect.top() + LABEL_LANE + COIN_DIAMETER * 0.5,
    );
    let coin_r = COIN_DIAMETER * 0.5;
    painter.circle_filled(coin_center, coin_r, with_alpha(palette::GOLD, alpha));
    painter.circle_stroke(
        coin_center,
        coin_r - 1.0,
        egui::Stroke::new(1.0, with_alpha(palette::GOLD_SHADOW, alpha)),
    );
    let screen_r = coin_r - BEZEL_RING_PX;
    painter.circle_filled(coin_center, screen_r, with_alpha(palette::SF_3, alpha));
    painter.circle_stroke(
        coin_center,
        screen_r,
        egui::Stroke::new(1.0, with_alpha(palette::GOLD_INK, alpha)),
    );

    // QR texture sits inside the dark screen rim. Side = inscribed
    // square of the (screen - rim) disc, clamped to the texture aspect.
    let inner_r = screen_r - SCREEN_RIM_PX;
    let side = inner_r * 2.0;
    let qr_rect = egui::Rect::from_center_size(coin_center, egui::vec2(side, side));
    // Fade the QR texture in via a white tint with the same alpha —
    // egui's `painter.image` multiplies the texture sample by this
    // tint, so alpha scaling carries through to the pixel output.
    let image_tint = egui::Color32::from_rgba_unmultiplied(
        255, 255, 255,
        (255.0 * alpha.clamp(0.0, 1.0)).round() as u8,
    );
    painter.image(
        tex.id(),
        qr_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        image_tint,
    );

    // Subtitle — italic dim caption below the coin.
    painter.text(
        egui::pos2(
            panel_rect.center().x,
            panel_rect.bottom() - LABEL_LANE * 0.5,
        ),
        egui::Align2::CENTER_CENTER,
        "scan to rejoin",
        egui::FontId::new(palette::CAPTION_SM, egui::FontFamily::Proportional),
        with_alpha(palette::TEXT_DIM, alpha),
    );
}
