//! Crash-recovery screen (PLAN 4.15.10).
//!
//! RPCS3 died unexpectedly — the `spawn_crash_watchdog` task in
//! [`crate::state`] flipped `LauncherStatus.screen` to
//! `LauncherScreen::Crashed { message }` and broadcast `Event::GameCrashed`
//! to phones. The launcher shows a full-bleed "SOMETHING WENT WRONG"
//! surface with the watchdog's message and a gold RESTART button.
//!
//! The cloud vortex backdrop (4.15.5) is drawn by the top-level dispatcher
//! before this surface renders; per-screen `VortexParams` tuning (urgent
//! iris-close matching §3.8 of `docs/aesthetic/navigation.md`) is a
//! 4.15a.7 polish item.

use std::sync::Arc;

use crate::state::{LauncherScreen, LauncherStatus};
use crate::{fonts, palette};

/// Render the crash-recovery surface. RESTART flips the screen back to
/// `Main`; actual RPCS3 respawn is a follow-up (see the TODO inline — the
/// `/api/launch` endpoint currently expects the phone to pick a serial,
/// and the server has no "last booted game" memory yet).
pub(super) fn render(
    ui: &mut egui::Ui,
    status: &Arc<std::sync::Mutex<LauncherStatus>>,
    message: &str,
) {
    ui.vertical_centered(|ui| {
        // Vertical centering on a full-screen panel: use a chunk of the
        // available height as top padding so the block sits roughly
        // between 1/4 and 3/4 of the viewport height.
        ui.add_space(ui.available_height() * 0.18);
        ui.heading(
            egui::RichText::new("SOMETHING WENT WRONG")
                .size(80.0)
                .color(palette::GOLD)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        );
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new(message)
                .size(26.0)
                .italics()
                .color(palette::TEXT_DIM),
        );
        ui.add_space(56.0);

        let btn = egui::Button::new(
            egui::RichText::new("RESTART")
                .size(40.0)
                .color(palette::GOLD_INK)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        )
        .fill(palette::GOLD)
        .stroke(egui::Stroke::new(2.0, palette::GOLD_SHADOW))
        .rounding(egui::Rounding::same(20.0))
        .min_size(egui::vec2(320.0, 96.0));
        if ui.add(btn).clicked() {
            // TODO(4.15.10 follow-up): respawn RPCS3 with the last
            // booted game's serial once we persist a "last game"
            // memory. Today `/api/launch` expects the phone to pick
            // the serial, so a real restart requires either (a) the
            // phone driving this button via a REST endpoint, or (b)
            // a new server-side "last serial" cache. Flipping back to
            // Main is sufficient for MVP — the user sees the QR again
            // and can reconnect the phone + re-pick the game.
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
