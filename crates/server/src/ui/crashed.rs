//! Crash-recovery screen (PLAN 4.15.10).
//!
//! RPCS3 died unexpectedly — the `spawn_crash_watchdog` task in
//! [`crate::state`] flipped `LauncherStatus.screen` to
//! `LauncherScreen::Crashed { message }` and broadcast `Event::GameCrashed`
//! to phones. The launcher shows a round-badge "SOMETHING / WENT /
//! WRONG" surface with the watchdog's diagnostic message and a gold
//! RESTART button.
//!
//! The badge spin-in is driven by the dispatcher's `ScreenIntro` so
//! the crash card lands the same way the QR card does on a fresh
//! intro — visual continuity across surfaces. When the crash arrives
//! from an in-game session the vortex wasn't visible at all; the
//! dispatcher pairs the badge spin with an iris reveal so the
//! launcher reasserts itself rather than snapping in opaque.

use std::sync::Arc;

use super::launch_phase::ScreenIntro;
use super::main_screen::{CARD_SIZE, paint_titled_card, with_alpha};
use crate::state::{LauncherScreen, LauncherStatus};
use crate::{fonts, palette};

pub(super) fn render(
    ui: &mut egui::Ui,
    status: &Arc<std::sync::Mutex<LauncherStatus>>,
    message: &str,
    intro: ScreenIntro,
) {
    let badge_scale = intro.badge_scale();
    let badge_alpha = intro.badge_alpha();
    let text_alpha = intro.content_alpha();

    ui.vertical_centered(|ui| {
        // Same vertical centring as `render_main` and `server_error`
        // so the badge sits on the vortex iris regardless of which
        // surface the user lands on.
        let avail = ui.available_height();
        ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

        let (full_rect, _) =
            ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());
        let half_w = (full_rect.width() * badge_scale) * 0.5;
        let badge_rect = egui::Rect::from_center_size(
            full_rect.center(),
            egui::vec2(half_w * 2.0, full_rect.height()),
        );
        if badge_rect.width() >= 1.0 {
            paint_titled_card(
                ui.painter(),
                badge_rect,
                &["SOMETHING", "WENT", "WRONG"],
                badge_alpha,
                text_alpha,
            );
        }

        ui.add_space(24.0);

        // Diagnostic message — italic dim, fades in with the rest of
        // the card content so it isn't readable mid-spin.
        let max_w = (ui.available_width() * 0.7).min(900.0);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(message)
                        .size(20.0)
                        .italics()
                        .color(with_alpha(palette::TEXT_DIM, text_alpha)),
                );
            },
        );

        // Push the RESTART button toward the bottom — same placement
        // pattern as render_main's Exit button + server_error's exit.
        let remaining = ui.available_height();
        ui.add_space((remaining * 0.55).max(48.0));

        let btn = egui::Button::new(
            egui::RichText::new("RESTART")
                .size(28.0)
                .color(with_alpha(palette::GOLD_INK, text_alpha))
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        )
        .fill(with_alpha(palette::GOLD, text_alpha))
        .rounding(egui::Rounding::same(16.0))
        .min_size(egui::vec2(260.0, 60.0));
        if ui.add(btn).clicked() {
            // TODO(4.15.10 follow-up): respawn RPCS3 with the last
            // booted game's serial once we persist a "last game"
            // memory. For now flipping back to Main is sufficient —
            // the user sees the QR again and can reconnect + re-pick.
            if let Ok(mut st) = status.lock() {
                st.screen = LauncherScreen::Main;
                // The watchdog already cleared these, but be
                // defensive — the restart-while-still-running case
                // becomes possible once auto-respawn lands.
                st.rpcs3_running = false;
                st.current_game = None;
            }
        }
    });
}
