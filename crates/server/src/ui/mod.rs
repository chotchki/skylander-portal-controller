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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::state::{LauncherScreen, LauncherStatus};
use crate::vortex::{self, VortexParams};
use crate::{fonts, palette};

mod crashed;
mod farewell;
mod in_game;
mod launch_phase;
mod main_screen;

use launch_phase::LaunchPhase;

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

        // Launcher start-of-life phasing (PLAN 4.19.2a). Only meaningful
        // for the Main screen — Crashed and Farewell are explicit
        // overrides that should render immediately regardless of how
        // long ago the launcher booted. The phase drives both the
        // vortex's iris and whether the QR/heading layer renders this
        // frame; see `launch_phase.rs` for the full state machine.
        let launch_phase = if matches!(status_snapshot.screen, LauncherScreen::Main) {
            let has_activity =
                status_snapshot.rpcs3_running || self.clients.load(Ordering::Relaxed) > 0;
            LaunchPhase::compute(time_s, has_activity)
        } else {
            LaunchPhase::AwaitingConnect
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.max_rect();

            // Layer 0: soft top + bottom sky-glow ellipses. Static
            // backdrop that gives the dark panel ambient depth — what
            // the mock's `.sky` element does with two CSS radial
            // gradients (tv_launcher_v3.html lines 36-43).
            vortex::paint_sky_background(ui.painter(), rect);

            // Layer 1: starfield. Always painted so it shows through the
            // vortex's iris hole (and stands alone during the launcher's
            // Startup beat when iris_radius=0). Painted before the vortex
            // so clouds layer on top once they're visible. The field
            // slowly drifts diagonally over time so the screen never
            // feels static — see `paint_starfield` for the rate.
            vortex::paint_starfield(ui.painter(), rect, time_s);

            // Layer 2: vortex clouds. Per-screen VortexParams tuning is a
            // 4.15a.7 / 4.19.6 polish item — for now we only override
            // `iris_radius` so the launcher startup sequence (4.19.2a)
            // can hide / ramp / reveal the clouds.
            let vortex_params = VortexParams {
                iris_radius: launch_phase.iris_radius(),
                ..VortexParams::default()
            };
            vortex::draw(ui.painter(), rect, time_s, vortex_params);

            // Layer 3: per-screen content.
            match &status_snapshot.screen {
                LauncherScreen::Main => {
                    if launch_phase.shows_main_content() {
                        self.render_main(ui, ctx, &status_snapshot);
                    } else {
                        // Startup beat: brand title only over the
                        // starfield. Gives the 1.0s (5.0s during 4.19.2a
                        // validation) hold a focal element rather than
                        // an empty calm field.
                        self.render_brand_intro(ui);
                    }
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
