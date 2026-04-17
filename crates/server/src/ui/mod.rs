//! eframe TV launcher — module root.
//!
//! Polls the shared `LauncherStatus` every frame and dispatches to one of
//! three surfaces based on `LauncherStatus.screen`:
//!
//! - [`main_screen`] — default: QR + title + status strip (PLAN 4.15.1–.4).
//! - [`crashed`] — RPCS3 died unexpectedly (PLAN 4.15.10).
//! - [`farewell`] — user asked to quit the launcher (PLAN 4.15.11).
//!
//! The cloud vortex (PLAN 4.15.5) is drawn once per frame as a common
//! backdrop before any screen renders its content, so all three surfaces
//! share the same visual baseline. Per-screen `VortexParams` tuning (urgent
//! iris-close on crash, gentle on farewell) is deferred to 4.15a.7 polish.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use crate::state::{LauncherScreen, LauncherStatus};
use crate::vortex::{self, VortexParams};
use crate::{fonts, palette};

mod crashed;
mod farewell;
mod in_game;
mod main_screen;

pub struct LauncherApp {
    clients: Arc<AtomicUsize>,
    status: Arc<std::sync::Mutex<LauncherStatus>>,
    url: String,
    figure_count: usize,
    qr_texture: Option<egui::TextureHandle>,
    /// Monotonic animation clock for the cloud vortex (PLAN 4.15.5).
    /// `egui::Context::input(|i| i.time)` would work too but is f64 and
    /// resets on Context rebuild; keeping our own `Instant` is simpler.
    started: Instant,
    /// When the farewell countdown started. Set the first frame the UI
    /// observes `LauncherScreen::Farewell`; cleared when the screen flips
    /// back to anything else (future-proofing — we don't currently expose
    /// a "cancel farewell" path). `None` means we haven't rendered the
    /// farewell yet this session.
    farewell_started_at: Option<Instant>,
}

impl LauncherApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        clients: Arc<AtomicUsize>,
        status: Arc<std::sync::Mutex<LauncherStatus>>,
        figure_count: usize,
        url: String,
    ) -> Self {
        // Apply the shared TV-launcher palette + Titan One display face.
        // Both must happen before any widgets render their first frame
        // so colour overrides and named font families take effect
        // immediately (PLAN 4.15.1 / 4.15.2).
        palette::apply(&cc.egui_ctx);
        fonts::register(&cc.egui_ctx);
        let qr_texture = Some(main_screen::render_qr_texture(&cc.egui_ctx, &url));
        Self {
            clients,
            status,
            url,
            figure_count,
            qr_texture,
            started: Instant::now(),
            farewell_started_at: None,
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 60 FPS repaint cadence — the vortex animation needs smooth motion.
        // Before 4.15.5 this was 250ms; the old rate would strobe the arms.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        let status_snapshot = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        let time_s = self.started.elapsed().as_secs_f32();

        // Reset the farewell timer when we're NOT on the farewell surface
        // — so if a future path flips screen out of Farewell (none today)
        // the next time we enter it, the 3s countdown restarts from zero.
        if !matches!(status_snapshot.screen, LauncherScreen::Farewell) {
            self.farewell_started_at = None;
        }

        // In-game transparent surface (PLAN 4.15.8) — when RPCS3 is
        // actually running AND we're not in a crash/farewell override,
        // skip the vortex + Main render and let the game show through
        // the transparent viewport. The CentralPanel uses a fully-clear
        // frame so the OS-level transparency (enabled via
        // `ViewportBuilder::with_transparent(true)`) actually shows RPCS3.
        let in_game =
            status_snapshot.rpcs3_running && matches!(status_snapshot.screen, LauncherScreen::Main);

        if in_game {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    in_game::render(ui, &self.clients, self.qr_texture.as_ref());
                });
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Vortex first so every subsequent widget layers on top of the
            // clouds. Drawn via `ui.painter()` so it fills the whole
            // CentralPanel including padding. All three screens share the
            // same vortex backdrop for visual continuity; per-screen
            // VortexParams tuning is a 4.15a.7 polish item.
            let rect = ui.max_rect();
            vortex::draw(ui.painter(), rect, time_s, VortexParams::default());

            match &status_snapshot.screen {
                LauncherScreen::Main => {
                    self.render_main(ui, ctx, &status_snapshot);
                }
                LauncherScreen::Crashed { message } => {
                    crashed::render(ui, &self.status, message);
                }
                LauncherScreen::Farewell => {
                    farewell::render(ui, ctx, &mut self.farewell_started_at);
                }
            }
        });
    }
}
