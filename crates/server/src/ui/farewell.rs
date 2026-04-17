//! Shutdown farewell screen (PLAN 4.15.11).
//!
//! User asked to quit the launcher (via `POST /api/shutdown`, typically
//! driven by a phone-side menu button). Show a short farewell, count down
//! ~3 seconds, then call `ViewportCommand::Close` (same mechanism the
//! Exit-to-Desktop button uses).
//!
//! The cloud vortex backdrop (4.15.5) is drawn by the top-level dispatcher
//! before this surface renders; per-screen `VortexParams` tuning (gentle
//! iris-close matching §3.5 of `docs/aesthetic/navigation.md`) is a
//! 4.15a.7 polish item.

use std::time::Instant;

use crate::{fonts, palette};

/// How long the farewell screen lingers before the launcher issues
/// `ViewportCommand::Close`. Matches the navigation-doc spec (§3.5 —
/// ~2.2s read pause + 1.6s fade-in of the "(launcher will exit)" hint).
/// Kept as a single const so the countdown text and the close trigger
/// can't drift apart.
const FAREWELL_COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(3);

/// Render the farewell surface + drive the countdown. The `started_at`
/// slot lives on `LauncherApp` so it survives across frames; the first
/// frame we see Farewell stamps the start time, and we re-request
/// repaints so the timer ticks visibly (egui is lazy by default and would
/// otherwise only advance on external input).
pub(super) fn render(ui: &mut egui::Ui, ctx: &egui::Context, started_at: &mut Option<Instant>) {
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
        // countdown would only advance on external input. Repaint at
        // the smaller of "half a second" and "however long is left".
        let next = remaining.min(std::time::Duration::from_millis(500));
        ctx.request_repaint_after(next);
    }

    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.22);
        ui.heading(
            egui::RichText::new("SEE YOU NEXT TIME, PORTAL MASTER")
                .size(72.0)
                .color(palette::GOLD)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        );
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("(launcher will exit)")
                .size(24.0)
                .italics()
                .color(palette::TEXT_DIM),
        );
        ui.add_space(48.0);
        // Ceiling division so the first frame shows "3" rather than
        // "2" — subtle, but the alternative is a 2→1→0 countdown that
        // starts late.
        let secs = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        ui.label(
            egui::RichText::new(format!("{secs}"))
                .size(56.0)
                .color(palette::GOLD_2)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        );
    });
}
