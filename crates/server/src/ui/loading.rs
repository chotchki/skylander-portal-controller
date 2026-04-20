//! Game-loading screen.
//!
//! Renders between the close-to-in-game animation and RPCS3 actually
//! being ready to display the game. Without this beat the user sees
//! ~30s of dark vortex with no feedback after picking a game on the
//! phone — the launch handler doesn't set `rpcs3_running = true`
//! until the UIA-boot completes, so the launcher would otherwise sit
//! on the post-close empty state until then.
//!
//! Layout: round-badge "LOADING" card matching the rest of the
//! launcher's transition surfaces, with the chosen game's display
//! name as a heraldic subtitle below. The badge pulses gently so
//! the screen doesn't feel frozen during long boots.

use super::main_screen::{CARD_SIZE, paint_heraldic_title, paint_titled_card};
use crate::palette;

pub(super) fn render(ui: &mut egui::Ui, game_name: &str) {
    // Pulse the badge alpha 0.65 ↔ 1.0 over a ~1.6s cycle. Slow
    // enough to feel like breathing, not flashing — and the alpha
    // floor keeps the badge clearly visible at the dim end.
    let time_s = ui.ctx().input(|i| i.time) as f32;
    let pulse = 0.825 + 0.175 * (time_s * (std::f32::consts::TAU / 1.6)).sin();

    ui.vertical_centered(|ui| {
        // Same vertical centring as render_main / server_error so the
        // badge sits on the vortex iris.
        let avail = ui.available_height();
        ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());
        // Single-word title — paint_titled_card auto-scales the font
        // to fill the inscribed square, so "LOADING" reads big at 10ft.
        paint_titled_card(ui.painter(), rect, &["LOADING"], pulse, pulse);

        ui.add_space(24.0);

        // Game name in heraldic style below. 48px is the same scale
        // the QR card uses for its "SCAN TO CONNECT" subtitle.
        let label_height = 96.0;
        let label_rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), label_height),
                egui::Sense::hover(),
            )
            .0;
        paint_heraldic_title(ui.painter(), label_rect.center(), game_name, 48.0, pulse);

        // Spacer below the title so anything appended later (progress
        // dots, hint text) doesn't crowd the bottom edge. `palette`
        // import is present even though unused so future polish can
        // grab `palette::TEXT_DIM` for hint text without re-importing.
        let _ = palette::TEXT_DIM;
        let remaining = ui.available_height();
        ui.add_space((remaining * 0.55).max(48.0));
    });
}
