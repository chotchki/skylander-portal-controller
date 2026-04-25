//! Server-error screen (PLAN 4.19.23).
//!
//! Backend startup failed — the tokio thread couldn't construct the
//! driver, open the profile DB, bind the listener, etc. Surface the
//! failure on the round card silhouette the QR-front and back-face
//! share, so the design language carries across screens. Layout
//! mirrors `render_main`: centred card + diagnostic detail + Exit
//! button at the bottom.
//!
//! Why no "Retry" button (unlike `crashed.rs` which has RESTART)?
//! Server-startup errors are infrastructure problems the launcher
//! can't fix from inside the process — port already in use needs
//! the conflicting process killed; corrupt SQLite needs the file
//! repaired or deleted; missing firmware-pack needs a re-config.
//! Re-running the same code in the same process won't help. Exit
//! gives the user a clean way out so they can address the root
//! cause and relaunch.

use super::launch_phase::ScreenIntro;
use super::main_screen::{paint_centered_back_card, with_alpha};
use crate::palette;

pub(super) fn render(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    message: &str,
    intro: ScreenIntro,
) {
    let badge_scale = intro.badge_scale();
    let badge_alpha = intro.badge_alpha();
    let text_alpha = intro.content_alpha();

    ui.vertical_centered(|ui| {
        paint_centered_back_card(
            ui,
            &["SERVER", "FAILED", "TO START"],
            badge_scale,
            1.0,
            badge_alpha,
            text_alpha,
        );

        ui.add_space(24.0);

        // Technical error message — italic dim. Width-constrained so
        // long error strings (Windows formats `bind` errors as long
        // sentences) wrap instead of overflowing the panel.
        let max_w = (ui.available_width() * 0.7).min(900.0);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let dim_alpha = with_alpha(palette::TEXT_DIM, text_alpha);
                ui.label(
                    egui::RichText::new(message)
                        .size(palette::SUBHEAD)
                        .italics()
                        .color(dim_alpha),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(
                        "Check the launcher log for details, then exit and try again.",
                    )
                    .size(palette::BODY)
                    .color(dim_alpha),
                );
            },
        );

        // Push the exit button toward the bottom, same pattern as
        // render_main's Exit-to-Desktop placement.
        let remaining = ui.available_height();
        ui.add_space((remaining * 0.55).max(48.0));

        let btn = egui::Button::new(
            egui::RichText::new("Exit to Desktop")
                .size(palette::HEADING)
                .color(egui::Color32::WHITE),
        )
        .fill(palette::DANGER)
        .rounding(egui::Rounding::same(16.0))
        .min_size(egui::vec2(260.0, 60.0));
        if ui.add(btn).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
