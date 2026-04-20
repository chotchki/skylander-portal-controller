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
use std::sync::Mutex;
use std::time::Instant;

use crate::state::{LauncherScreen, LauncherStatus};
use crate::vortex::{self, ShaderRig, VortexParams};
use crate::{fonts, palette};

mod crashed;
mod farewell;
mod in_game;
mod launch_phase;
mod main_screen;
mod server_error;

use launch_phase::LaunchPhase;

pub struct LauncherApp {
    clients: Arc<AtomicUsize>,
    status: Arc<std::sync::Mutex<LauncherStatus>>,
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
    /// When the close-to-in-game animation started. Set the first frame
    /// the dispatcher detects RPCS3-running while on the Main screen
    /// (transition from "awaiting connect" → "game running"). Drives
    /// the badge spin-out + dark-hole iris growth via `LaunchPhase::
    /// ClosingToInGame`. Cleared if RPCS3 stops before the animation
    /// completes (game crash mid-launch). After the animation finishes
    /// (`progress >= 1.0`) the dispatcher routes rendering to
    /// `in_game::render` and the transparent panel reveals RPCS3.
    closing_to_in_game_at: Option<Instant>,
    /// GPU shader rig for the vortex (PLAN 4.19.6). Initialised lazily
    /// on the first frame because the eframe `Frame::gl()` context
    /// isn't available until `update()` is called. `Arc<Mutex<…>>` so
    /// the `egui::PaintCallback` closure can capture a shared handle
    /// across the immediate-mode boundary.
    vortex_rig: Arc<Mutex<Option<ShaderRig>>>,
    /// Vortex look (noise + colors + motion), loaded once at startup
    /// from the bundled `vortex_presets/idle.json`. Per-frame
    /// `iris_radius` / `iris_mode` / `transparent` overrides are
    /// applied at draw time based on the launch phase; the rest of
    /// the params come from this struct unchanged.
    vortex_idle: VortexParams,
}

impl LauncherApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        clients: Arc<AtomicUsize>,
        status: Arc<std::sync::Mutex<LauncherStatus>>,
        url: String,
    ) -> Self {
        // Apply the shared TV-launcher palette + Titan One display face.
        // Both must happen before any widgets render their first frame
        // so colour overrides and named font families take effect
        // immediately (PLAN 4.15.1 / 4.15.2).
        palette::apply(&cc.egui_ctx);
        fonts::register(&cc.egui_ctx);
        // QR texture is generated once at startup and cached. We
        // consume the URL here — render_main no longer needs it as a
        // field after 4.19.10a / 4.19.22 dropped the on-screen URL
        // text and brand heading. `figure_count` previously rode along
        // for the "504 figures indexed" debug counter; same drop.
        let qr_texture = Some(main_screen::render_qr_texture(&cc.egui_ctx, &url));
        Self {
            clients,
            status,
            qr_texture,
            started: Instant::now(),
            farewell_started_at: None,
            closing_to_in_game_at: None,
            vortex_rig: Arc::new(Mutex::new(None)),
            vortex_idle: vortex::idle_params(),
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 60 FPS repaint cadence — the vortex animation needs smooth motion.
        // Before 4.15.5 this was 250ms; the old rate would strobe the arms.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // Lazy-init the vortex shader rig on the first frame. The eframe
        // glow context isn't exposed until `update()` runs, so we can't
        // do this in `LauncherApp::new`. On init failure (driver bug,
        // unsupported GLSL version) we log and continue with the rig
        // stuck at None — the launcher renders without the vortex
        // layer rather than crashing.
        if self.vortex_rig.lock().unwrap().is_none() {
            if let Some(gl) = frame.gl() {
                match ShaderRig::new(gl) {
                    Ok(rig) => *self.vortex_rig.lock().unwrap() = Some(rig),
                    Err(e) => tracing::error!("vortex shader init failed: {e}"),
                }
            }
        }

        let status_snapshot = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        let time_s = self.started.elapsed().as_secs_f32();

        // Reset the farewell timer when we're NOT on the farewell surface
        // — so if a future path flips screen out of Farewell (none today)
        // the next time we enter it, the 3s countdown restarts from zero.
        if !matches!(status_snapshot.screen, LauncherScreen::Farewell) {
            self.farewell_started_at = None;
        }

        // In-game transition (PLAN 4.15.8 + close animation):
        //
        //   1. RPCS3 just started while we're on Main → kick off the
        //      ClosingToInGame animation (badge spins out, dark-hole
        //      iris grows).
        //   2. Animation runs as a Main render with the closing phase.
        //   3. Once `close_complete()`, flip to the transparent in-game
        //      surface so RPCS3 shows through.
        //   4. If RPCS3 stops before the animation finishes (rare —
        //      game crashes mid-launch), reset the timer so we don't
        //      finish a close that no longer applies.
        let want_in_game =
            status_snapshot.rpcs3_running && matches!(status_snapshot.screen, LauncherScreen::Main);
        if want_in_game && self.closing_to_in_game_at.is_none() {
            self.closing_to_in_game_at = Some(Instant::now());
        }
        if !want_in_game {
            self.closing_to_in_game_at = None;
        }
        let closing_elapsed_s = self
            .closing_to_in_game_at
            .map(|t| t.elapsed().as_secs_f32());

        // Launcher start-of-life phasing (PLAN 4.19.2a). Only meaningful
        // for the Main screen — Crashed and Farewell are explicit
        // overrides that should render immediately regardless of how
        // long ago the launcher booted. The phase drives the vortex
        // iris, the badge spin scale + alpha, and the text fade — see
        // `launch_phase.rs` for the choreography.
        let launch_phase = if matches!(status_snapshot.screen, LauncherScreen::Main) {
            let has_activity =
                status_snapshot.rpcs3_running || self.clients.load(Ordering::Relaxed) > 0;
            LaunchPhase::compute(time_s, closing_elapsed_s, has_activity)
        } else {
            LaunchPhase::AwaitingConnect
        };

        // Close animation has finished — hand off to the transparent
        // in-game surface so RPCS3 is visible.
        if launch_phase.close_complete() {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    in_game::render(ui, &self.clients, self.qr_texture.as_ref());
                });
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.max_rect();

            // Layer 0: soft top + bottom sky-glow ellipses. Static
            // backdrop that gives the dark panel ambient depth — what
            // the mock's `.sky` element does with two CSS radial
            // gradients (tv_launcher_v3.html lines 36-43).
            vortex::paint_sky_background(ui.painter(), rect);

            // Layer 1: tuned CPU starfield (gold/blue tints, radial
            // outward drift, twinkle). Painted before the vortex so
            // the shader's iris alpha mask determines whether stars
            // show: alpha=0 (Startup, outside iris during Intro,
            // dark hole during Close) → stars visible through;
            // alpha=1 (AwaitingConnect interior) → vortex covers
            // them. Matches the pre-shader rendering order Chris
            // settled on, ref `Screenshot 2026-04-19 093358.png`.
            vortex::paint_starfield(ui.painter(), rect, time_s);

            // Layer 2: GPU vortex shader. Most params (noise, colors,
            // motion, streaks) come from the bundled `idle.json`
            // preset. The launch phase overrides:
            //
            //   - `iris_radius` — animates 0 → 1.5 during intro/close
            //   - `iris_mode` — Reveal during intro, DarkHole during close
            //
            // The shader now outputs premultiplied alpha based on
            // cloud density × iris, so sky + starfield underneath
            // show through dim regions naturally without any extra
            // mode flag. `star_brightness` is forced to 0 because
            // the shader's internal starfield was a spike-only
            // tuning aid; production uses the CPU starfield (Layer 1).
            let mut vortex_params = self.vortex_idle;
            vortex_params.iris_radius = launch_phase.iris_radius();
            vortex_params.iris_mode = launch_phase.iris_mode();
            vortex_params.star_brightness = 0.0;
            // Add the preset's `time_offset` to the launcher's
            // elapsed time so the very first frame's `u_time`
            // matches the spike-tuned starting snapshot. Without
            // this, every launcher boot shows the noise field at
            // wall-clock-zero (uninteresting flat-looking phase);
            // with it, the visible vortex matches what was dialled
            // in the spike at the moment the preset was saved.
            let vortex_time_s = time_s + self.vortex_idle.time_offset;
            vortex::paint_vortex(
                ui.painter(),
                rect,
                self.vortex_rig.clone(),
                vortex_params,
                vortex_time_s,
            );

            // Layer 2: CPU starfield. Painted AFTER the shader so the
            // tuned stars (gold + blue tints, radial outward drift,
            // per-star twinkle) sit on top of the vortex clouds
            // rather than being obscured by the shader's opaque
            // output. Reads as "stars in space, with clouds drifting
            // among them" — the design language the launcher's been
            // tuned to.
            vortex::paint_starfield(ui.painter(), rect, time_s);

            // Layer 3: per-screen content.
            match &status_snapshot.screen {
                LauncherScreen::Main => {
                    // Both layers can render in the same frame during
                    // the intro hand-off: main content fades in via
                    // its own per-element alpha curves while the
                    // brand intro fades out via `brand_intro_alpha`.
                    // Painting the brand AFTER main content puts the
                    // title on top during the early intro window
                    // when both are visible — the title is the focal
                    // element until ~30% into the transition.
                    if launch_phase.shows_main_content() {
                        self.render_main(ui, ctx, &status_snapshot, launch_phase);
                    }
                    let brand_alpha = launch_phase.brand_intro_alpha();
                    if brand_alpha > 0.001 {
                        self.render_brand_intro(ui, brand_alpha);
                    }
                }
                LauncherScreen::Crashed { message } => {
                    crashed::render(ui, &self.status, message);
                }
                LauncherScreen::Farewell => {
                    farewell::render(ui, ctx, &mut self.farewell_started_at);
                }
                LauncherScreen::ServerError { message } => {
                    server_error::render(ui, ctx, message, launch_phase);
                }
            }
        });
    }

    fn on_exit(&mut self, gl: Option<&egui_glow::glow::Context>) {
        // Release GL resources (program / VBO / VAO) cleanly. eframe
        // would tear them down anyway on context drop, but doing it
        // explicitly avoids spurious "leaked GL handle" warnings on
        // some drivers.
        if let (Some(gl), Some(rig)) = (gl, self.vortex_rig.lock().unwrap().as_ref()) {
            rig.destroy(gl);
        }
    }
}
