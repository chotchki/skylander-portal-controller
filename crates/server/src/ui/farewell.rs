//! Shutdown farewell screen (PLAN 4.15.11).
//!
//! User asked to quit the launcher (via `POST /api/shutdown`, typically
//! driven by a phone-side menu button). Show a short farewell on the
//! shared round-badge silhouette, count down ~3 seconds, then call
//! `ViewportCommand::Close` (same mechanism the Exit-to-Desktop button
//! uses). Badge spin-in matches every other surface so the goodbye
//! feels deliberate, not a snap-out.

use std::time::Instant;

use super::launch_phase::ScreenIntro;
use super::main_screen::{CARD_SIZE, paint_titled_card, with_alpha};
use crate::{fonts, palette};

/// How long the farewell screen lingers before the launcher issues
/// `ViewportCommand::Close`. Matches the navigation-doc spec (§3.5 —
/// ~2.2s read pause + 1.6s fade-in of the "(launcher will exit)" hint).
/// Kept as a single const so the countdown text and the close trigger
/// can't drift apart.
const FAREWELL_COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(3);

pub(super) fn render(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    started_at: &mut Option<Instant>,
    intro: ScreenIntro,
) {
    // First frame we see Farewell: stamp the start time and ensure we
    // repaint within the countdown window so the remaining-seconds
    // label ticks visibly and the Close command actually fires.
    let start = *started_at.get_or_insert_with(Instant::now);
    let elapsed = start.elapsed();
    let remaining = FAREWELL_COUNTDOWN.saturating_sub(elapsed);

    if remaining.is_zero() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else {
        // egui is lazy by default — without this request_repaint the
        // countdown would only advance on external input.
        let next = remaining.min(std::time::Duration::from_millis(500));
        ctx.request_repaint_after(next);
    }

    let badge_scale = intro.badge_scale();
    let badge_alpha = intro.badge_alpha();
    let text_alpha = intro.content_alpha();

    ui.vertical_centered(|ui| {
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
            // Three lines mirroring the Skylanders "Portal Master"
            // farewell rhythm. Short words let `paint_titled_card`'s
            // auto-scale font fit cleanly inside the inscribed square.
            paint_titled_card(
                ui.painter(),
                badge_rect,
                &["FAREWELL", "PORTAL", "MASTER"],
                badge_alpha,
                text_alpha,
            );
        }

        ui.add_space(24.0);

        let max_w = (ui.available_width() * 0.7).min(900.0);
        // Ceiling division so the first frame shows "3" rather than
        // "2" — the alternative is a 2→1→0 countdown that starts late.
        let secs = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("(launcher will exit)")
                        .size(20.0)
                        .italics()
                        .color(with_alpha(palette::TEXT_DIM, text_alpha)),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(format!("{secs}"))
                        .size(56.0)
                        .color(with_alpha(palette::GOLD_2, text_alpha))
                        .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                );
            },
        );
    });
}
