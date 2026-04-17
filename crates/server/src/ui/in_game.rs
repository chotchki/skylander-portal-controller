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

/// Small gold-bezeled QR in the upper-right. Includes a one-line
/// "reconnect a phone" hint in dim text. Mirrors what the phone SPA
/// would show if it had a connection-lost overlay — this is the
/// TV-side equivalent that guides a family member back in.
fn reconnect_qr(ui: &mut egui::Ui, qr_texture: Option<&egui::TextureHandle>) {
    let Some(tex) = qr_texture else { return };

    let full = ui.max_rect();
    // Reconnect overlay panel: ~200px QR + label, anchored 32px inside
    // the upper-right corner.
    const PANEL_W: f32 = 260.0;
    const PANEL_H: f32 = 300.0;
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(full.right() - PANEL_W - 32.0, full.top() + 32.0),
        egui::vec2(PANEL_W, PANEL_H),
    );

    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(panel_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            egui::Frame::none()
                .fill(palette::SF_3)
                .stroke(egui::Stroke::new(2.0, palette::GOLD_INK))
                .inner_margin(egui::Margin::same(12.0))
                .rounding(egui::Rounding::same(10.0))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 4.0),
                    blur: 14.0,
                    spread: 0.0,
                    color: egui::Color32::from_black_alpha(180),
                })
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("RECONNECT")
                            .size(14.0)
                            .color(palette::GOLD)
                            .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                    );
                    ui.add_space(6.0);
                    // QR scales down to ~180px to fit the corner panel.
                    // `size_vec2()` returns the native (10×module) size;
                    // we let egui clamp via `.max_size`.
                    let tex_size = tex.size_vec2();
                    let max = 180.0;
                    let scale = (max / tex_size.x).min(max / tex_size.y).min(1.0);
                    let display_size = tex_size * scale;
                    ui.image((tex.id(), display_size));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("scan to rejoin")
                            .size(11.0)
                            .italics()
                            .color(palette::TEXT_DIM),
                    );
                });
        },
    );
}
