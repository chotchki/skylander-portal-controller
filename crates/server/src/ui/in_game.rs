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

/// Render the in-game transparent surface. Called from the top-level
/// dispatcher in [`super`] when `LauncherStatus::rpcs3_running` is true
/// (and no Crashed / Farewell screen is active).
pub(super) fn render(
    ui: &mut egui::Ui,
    clients: &Arc<AtomicUsize>,
    qr_texture: Option<&egui::TextureHandle>,
) {
    // Corner reconnect QR: only when every phone has disconnected.
    // Matches the 4.15a.5 refinement — not a persistent overlay.
    if clients.load(Ordering::Relaxed) == 0 {
        reconnect_qr(ui, qr_texture);
    }
}

/// Gold-bezeled round QR coin in the upper-right, with RECONNECT /
/// "scan to rejoin" text above and below. Mirrors the Main surface's
/// QR card scaled for a corner overlay; the texture already carries the
/// circular silhouette via its noise ring + transparent corners.
fn reconnect_qr(ui: &mut egui::Ui, qr_texture: Option<&egui::TextureHandle>) {
    let Some(tex) = qr_texture else { return };

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
        palette::GOLD,
    );

    // Round coin: gold ring → SF_3 screen rim → QR texture. Same
    // layering pattern as the Main surface so the two reads as a single
    // design language, just scaled.
    let coin_center = egui::pos2(
        panel_rect.center().x,
        panel_rect.top() + LABEL_LANE + COIN_DIAMETER * 0.5,
    );
    let coin_r = COIN_DIAMETER * 0.5;
    painter.circle_filled(coin_center, coin_r, palette::GOLD);
    painter.circle_stroke(
        coin_center,
        coin_r - 1.0,
        egui::Stroke::new(1.0, palette::GOLD_SHADOW),
    );
    let screen_r = coin_r - BEZEL_RING_PX;
    painter.circle_filled(coin_center, screen_r, palette::SF_3);
    painter.circle_stroke(
        coin_center,
        screen_r,
        egui::Stroke::new(1.0, palette::GOLD_INK),
    );

    // QR texture sits inside the dark screen rim. Side = inscribed
    // square of the (screen - rim) disc, clamped to the texture aspect.
    let inner_r = screen_r - SCREEN_RIM_PX;
    let side = inner_r * 2.0;
    let qr_rect = egui::Rect::from_center_size(coin_center, egui::vec2(side, side));
    painter.image(
        tex.id(),
        qr_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
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
        palette::TEXT_DIM,
    );
}
