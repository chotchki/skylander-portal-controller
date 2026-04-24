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

/// How long the farewell message lingers before the fade-to-black
/// overlay begins. Matches the navigation-doc spec (§3.5 — ~2.2s read
/// pause + 1.6s fade-in of the "(launcher will exit)" hint).
const FAREWELL_COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(3);

/// Fade-to-black duration AFTER the countdown hits zero (PLAN 4.19.15).
/// A full-viewport black rect ramps from 0 → fully-opaque alpha over
/// this window, then `ViewportCommand::Close` fires. Gives the exit
/// a deliberate "the lights dim" beat instead of popping the window
/// out the instant the countdown ends.
const FAREWELL_FADE: std::time::Duration = std::time::Duration::from_millis(800);

/// Farewell heading breathe pulse (PLAN 4.19.14). Period of the
/// combined scale + opacity wobble — matches `navigation.md` §3.5 spec.
/// The badge pulses during the 3s countdown beat only; the fade-to-
/// black window freezes it at the last state so the engulfing black
/// doesn't fight a moving badge.
const BREATHE_PERIOD_S: f32 = 2.4;
/// Scale amplitude — the badge pulses between 0.975 × and 1.025 × its
/// steady size. Subtle enough to read as "the portal is still alive"
/// without looking like a graphical glitch.
const BREATHE_SCALE_AMP: f32 = 0.025;
/// Opacity amplitude — at sin-phase -1 the badge sits at 0.975 × its
/// steady alpha; at phase +1 it's fully opaque. Half-rectified so
/// the pulse only dips (never overshoots) full opacity.
const BREATHE_OPACITY_AMP: f32 = 0.025;

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

    // Fade phase elapsed — 0 before the countdown ends, grows up to
    // FAREWELL_FADE after. Converted to a 0..=1 progress the alpha
    // curve multiplies against below.
    let fade_elapsed = elapsed.saturating_sub(FAREWELL_COUNTDOWN);
    let fade_progress = (fade_elapsed.as_secs_f32() / FAREWELL_FADE.as_secs_f32())
        .clamp(0.0, 1.0);

    if fade_elapsed >= FAREWELL_FADE {
        tracing::info!("farewell fade complete — sending viewport close");
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else {
        // Tight repaint throughout farewell — the breathe pulse (60fps
        // sin modulation) and the fade-to-black overlay both need
        // smooth frames. The countdown text only changes once/sec but
        // the GPU was already painting the vortex at 60fps anyway, so
        // the extra frames are free.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    // Breathe pulse (PLAN 4.19.14). Active during the 3s countdown
    // beat only — freezes the moment the fade starts so the engulfing
    // black doesn't fight a moving badge. sin-wave over
    // BREATHE_PERIOD_S; scale uses the raw phase (±2.5% around 1.0),
    // opacity half-rectifies so it only ever dips (never overshoots
    // full opacity, which would be invisible anyway at alpha=1).
    let (breathe_scale, breathe_alpha) = if fade_progress > 0.0 {
        (1.0, 1.0)
    } else {
        let phase = (elapsed.as_secs_f32() * std::f32::consts::TAU / BREATHE_PERIOD_S).sin();
        let t = (phase + 1.0) * 0.5; // 0..1
        (1.0 + BREATHE_SCALE_AMP * phase, 1.0 - BREATHE_OPACITY_AMP + BREATHE_OPACITY_AMP * t)
    };

    let badge_scale = intro.badge_scale() * breathe_scale;
    let badge_alpha = intro.badge_alpha() * breathe_alpha;
    let text_alpha = intro.content_alpha() * breathe_alpha;

    // Grab the full panel rect before the centered layout consumes it
    // — we use it below to paint the fade-to-black overlay over
    // everything else (badge + countdown + vortex).
    let panel_rect = ui.max_rect();

    ui.vertical_centered(|ui| {
        let avail = ui.available_height();
        ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

        let (full_rect, _) =
            ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());
        // Horizontal scales via `badge_scale` (carries the intro coin-
        // flip + the breathe pulse). Vertical scales via the breathe
        // only — coin-flip is a horizontal-only spin axis. On landed
        // intro + no-fade, both collapse to `breathe_scale`.
        let half_w = (full_rect.width() * badge_scale) * 0.5;
        let height = full_rect.height() * breathe_scale;
        let badge_rect = egui::Rect::from_center_size(
            full_rect.center(),
            egui::vec2(half_w * 2.0, height),
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
                        .size(palette::SUBHEAD)
                        .italics()
                        .color(with_alpha(palette::TEXT_DIM, text_alpha)),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(format!("{secs}"))
                        .size(palette::COUNTDOWN)
                        .color(with_alpha(palette::GOLD_2, text_alpha))
                        .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                );
            },
        );
    });

    // Fade-to-black overlay (PLAN 4.19.15). Painted last so it layers
    // over the badge + countdown + vortex backdrop. `fade_progress`
    // stays at 0 during the 3s read beat, then ramps 0 → 1 over the
    // 0.8s fade window; ease-in cubic so the dim starts gentle and
    // accelerates into full black. At progress=1 the next frame's
    // ViewportCommand::Close fires.
    if fade_progress > 0.0 {
        let eased = {
            let t = fade_progress;
            t * t * t
        };
        let alpha = (255.0 * eased).round().clamp(0.0, 255.0) as u8;
        ui.painter().rect_filled(
            panel_rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
        );
    }
}
