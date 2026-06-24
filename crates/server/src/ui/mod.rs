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
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::badge::BadgeRig;
use crate::state::{LauncherScreen, LauncherStatus};
use crate::vortex::{self, ShaderRig, VortexParams};
use crate::{fonts, palette};

mod crashed;
mod farewell;
mod in_game;
mod launch_phase;
mod main_screen;
mod sequencer;
mod server_error;

use launch_phase::{LaunchPhase, ScreenIntro};
use sequencer::{CloseTimers, detect_returning_from_game};

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
    /// Close-animation timers (in-game and shutdown). See
    /// [`sequencer::CloseTimers`] for the lifecycle rules — they're
    /// extracted into the sequencer module so the dispatcher's
    /// state-machine logic is unit-testable without an eframe context.
    close_timers: CloseTimers,
    /// When the launcher first observed `LauncherStatus.server_ready =
    /// true`. The launch-phase elapsed clock starts here, NOT at app
    /// mount — so the intro animations (iris reveal, badge spin) only
    /// fire once the server is healthy. If the server fails to start,
    /// this stays `None`, the launcher holds in the calm-starfield
    /// Startup beat, and `ServerError` takes over without the user
    /// ever seeing a partially-played spin animation.
    server_ready_at: Option<Instant>,
    /// Discriminant of the last-rendered `LauncherScreen` variant +
    /// when this variant first became active. Drives the per-screen
    /// `ScreenIntro` animation: each non-Main screen plays a
    /// badge-spin-in on its first ~1.2s of being shown. Reset
    /// whenever the screen variant changes (compared via
    /// `mem::discriminant` so e.g. `Crashed { msg }` with a different
    /// message doesn't trigger a re-entry).
    current_screen: LauncherScreen,
    screen_entered_at: Instant,
    /// Whether the previous frame routed to `in_game::render` (RPCS3
    /// running + screen=Main + close-complete). Used by the dispatcher
    /// to detect the "game just ended" transition: if last frame was
    /// in-game and this frame isn't, kick off a return animation
    /// (vortex iris reveal + badge spin-in via `LaunchPhase::
    /// ReturnFromGame`, OR, if entering Crashed instead of Main,
    /// drive the iris reveal off `ScreenIntro`).
    was_in_game: bool,
    /// Last-applied always-on-top state. `None` until the first frame
    /// sends a `WindowLevel` command so we re-assert on startup; then
    /// only on transitions. In release the target is always
    /// `AlwaysOnTop` (matches the viewport-creation setting). In dev
    /// the target is `AlwaysOnTop` only while RPCS3 is running so the
    /// launcher overlays the game window for in-game testing —
    /// otherwise `Normal`, so alt-tab works during code iteration.
    window_on_top_state: Option<bool>,
    /// `true` while we've minimised the launcher window for the RPCS3 settings
    /// GUI (PLAN 16.9.3). The Qt settings window needs the whole TV + input, so
    /// the always-on-top launcher steps fully aside while `config_gui_open`, then
    /// restores + refocuses when the user closes RPCS3. Tracked so the
    /// `Minimized` viewport command only fires on the transitions, not per frame.
    config_gui_minimized: bool,
    /// When the launcher started returning from an in-game session.
    /// Drives `LaunchPhase::ReturnFromGame` (skips the Startup beat,
    /// no brand intro). Cleared once the animation completes.
    returning_from_game_at: Option<Instant>,
    /// When the reconnect QR first became eligible to render (i.e.
    /// the moment `clients` transitioned to 0 while the launcher was
    /// on the in-game transparent surface). Drives `in_game::render`'s
    /// 1.0s ease-out fade-in per PLAN 4.19.12 — instead of popping
    /// into view the second the last phone drops, the overlay
    /// smooths in. Cleared whenever `clients > 0` so a subsequent
    /// drop starts a fresh fade.
    reconnect_qr_shown_at: Option<Instant>,
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
    /// 3D badge shader rig (PLAN 10.7.1 spike). Coexists with
    /// `vortex_rig` in the same shared GL context — separate program
    /// and VAO, separate `egui::PaintCallback` per frame. Lazy-init
    /// alongside the vortex so first-frame ordering matches.
    badge_rig: Arc<Mutex<Option<BadgeRig>>>,
    /// Round-QR raster bytes, generated once at startup. Same buffer
    /// `main_screen::render_qr_texture` consumes for the egui-side
    /// texture; held here so the GL-side `BadgeRig` lazy-init in
    /// `update()` can upload them without re-rasterising. PLAN 10.7.2.
    qr_pixels: crate::round_qr::RoundQrPixels,
    /// The phone join URL (same one the QR encodes). Kept so the Main screen's
    /// "Open in Browser" button can launch it in the PC's default browser as a
    /// QR-scan fallback (user request 2026-05-30).
    url: String,
    /// The raw-IP form of the join URL (`http://<ip>:<port>/?k=…`), shown by the
    /// PLAN 17.1 "Trouble connecting?" card as a fallback when the `.local` URL
    /// the QR usually encodes doesn't resolve on the phone (Android mDNS). Also
    /// the payload of the 17.4 raw-IP QR. Equals `url` when mDNS wasn't used.
    raw_ip_url: String,
    /// Pre-rendered round-QR pixels for `raw_ip_url` (PLAN 17.4), uploaded to an
    /// egui texture lazily on first use so the connectivity card can show a
    /// scannable raw-IP code.
    raw_ip_qr_texture: Option<egui::TextureHandle>,
    raw_ip_qr_pixels: crate::round_qr::RoundQrPixels,
    /// Bind port (PLAN 17.3) — the connectivity card's "Fix Firewall" button
    /// passes it to the elevated `netsh` rule-add.
    bind_port: u16,
    /// PLAN 20 launcher window mode. In `Desktop` the launcher is a resizable
    /// window, so the RPCS3 game window is *fitted* to our client rect (20.4)
    /// rather than just z-ordered; `Tv` keeps the fullscreen behaviour.
    window_mode: crate::config::WindowMode,
    /// Channel to the driver worker (PLAN B.2). On macOS the launcher can't move
    /// another app's window via Win32, so the Desktop-mode game-window fit is
    /// dispatched as a `DriverJob::WindowSet` that the worker routes over the IPC
    /// P7 command. `update()` is sync, so we `try_send` (never `.await`).
    driver_tx: tokio::sync::mpsc::Sender<crate::state::DriverJob>,
    /// Last game-window rect (screen coords) we sent over IPC, to suppress
    /// per-frame `WindowSet` spam — only re-send when the launcher's content rect
    /// actually changes. `None` until the first fit. macOS-only path (the Win32
    /// fit on Windows re-applies every frame and tracks its own state).
    last_window_set: Option<(i32, i32, u32, u32)>,
    /// Wall-clock (egui `input.time`, seconds) of the last `WindowSet` send. Drives
    /// a low-rate re-assert (≥0.5s) on top of the change-detection above: RPCS3
    /// resizes its own window during boot, and a stale `last_window_set` would
    /// otherwise leave the game stranded outside the launcher pane until the next
    /// genuine launcher-rect change. macOS-only path. `None` until the first send.
    last_window_set_at: Option<f64>,
    /// macOS surface-embed host (P8 / Phase C). When the driver publishes the
    /// game's `CAContextID` (`status.game_surface_context_id`), the launcher
    /// hosts that render-layer tree INSIDE its own egui view via `CALayerHost`
    /// — compositing the game behind egui's chrome — rather than tiling a
    /// second top-level window beneath itself (the `WindowSet` fallback). A
    /// no-op stub on non-macOS targets, so this field exists on every platform.
    compositor: crate::compositor::CompositorHost,
}

impl LauncherApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        clients: Arc<AtomicUsize>,
        status: Arc<std::sync::Mutex<LauncherStatus>>,
        url: String,
        raw_ip_url: String,
        bind_port: u16,
        window_mode: crate::config::WindowMode,
        driver_tx: tokio::sync::mpsc::Sender<crate::state::DriverJob>,
    ) -> Self {
        // Apply the shared TV-launcher palette + Titan One display face.
        // Both must happen before any widgets render their first frame
        // so colour overrides and named font families take effect
        // immediately (PLAN 4.15.1 / 4.15.2).
        palette::apply(&cc.egui_ctx);
        fonts::register(&cc.egui_ctx);
        // Log the OpenGL renderer eframe/glow actually bound to — proves
        // whether we're on hardware GL or the bundled Mesa software fallback
        // (PLAN 19) and is a handy support breadcrumb for "won't launch" reports.
        if let Some(gl) = &cc.gl {
            use glow::HasContext as _;
            // SAFETY: `gl` is eframe's current glow context for this thread.
            let (renderer, version) = unsafe {
                (
                    gl.get_parameter_string(glow::RENDERER),
                    gl.get_parameter_string(glow::VERSION),
                )
            };
            tracing::info!(%renderer, %version, "launcher OpenGL context");
        }
        // QR texture is generated once at startup and cached. The URL is also
        // retained on the struct (below) for the Main screen's "Open in Browser"
        // fallback button (user request 2026-05-30). `figure_count` previously
        // rode along for the "504 figures indexed" debug counter; that was dropped.
        // Render once into raw RGBA pixels; the egui-side texture and the GL-side
        // BadgeRig texture (PLAN 10.7.2) both consume the same buffer so they stay
        // byte-identical.
        let qr_pixels = main_screen::render_qr_pixels(&url);
        let qr_texture = Some(main_screen::pixels_to_egui_texture(
            &cc.egui_ctx,
            &qr_pixels,
        ));
        // PLAN 17.4: pre-render the raw-IP QR pixels too (texture uploads lazily
        // in the connectivity card, only when it actually shows).
        let raw_ip_qr_pixels = main_screen::render_qr_pixels(&raw_ip_url);
        Self {
            clients,
            status,
            qr_texture,
            started: Instant::now(),
            farewell_started_at: None,
            close_timers: CloseTimers::default(),
            server_ready_at: None,
            current_screen: LauncherScreen::default(),
            screen_entered_at: Instant::now(),
            was_in_game: false,
            returning_from_game_at: None,
            reconnect_qr_shown_at: None,
            window_on_top_state: None,
            config_gui_minimized: false,
            vortex_rig: Arc::new(Mutex::new(None)),
            vortex_idle: vortex::idle_params(),
            badge_rig: Arc::new(Mutex::new(None)),
            qr_pixels,
            url,
            raw_ip_url,
            raw_ip_qr_texture: None,
            raw_ip_qr_pixels,
            bind_port,
            window_mode,
            driver_tx,
            last_window_set: None,
            last_window_set_at: None,
            compositor: crate::compositor::CompositorHost::new(),
        }
    }

    /// macOS surface-embed (P8 / Phase C). If the driver has published the
    /// game's `CAContextID` (`status.game_surface_context_id`), host that
    /// render-layer tree inside THIS launcher window via `CALayerHost` and fit
    /// it to the same 16:9 sub-rect the `WindowSet` fallback would have tiled a
    /// second window into — but **view-relative** (origin at the launcher's
    /// content, not screen-global). Returns `true` when the surface is hosted
    /// (so the caller skips the `WindowSet` fallback).
    ///
    /// Attaches once (the contextId is stable for the session); subsequent
    /// frames only re-fit. Detaches when the contextId goes away (game stopped
    /// / recovering) so a fresh boot re-attaches on its new id.
    #[cfg(target_os = "macos")]
    fn try_embed_game_surface(
        &mut self,
        frame: &eframe::Frame,
        ctx: &egui::Context,
        status: &LauncherStatus,
    ) -> bool {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        // No published surface → detach any prior host and let the WindowSet
        // fallback take over.
        let Some(context_id) = status.game_surface_context_id.filter(|c| *c != 0) else {
            self.compositor.detach();
            return false;
        };

        // Only embed in Desktop window mode — TV mode is fullscreen and the
        // game/launcher coexist as plain siblings (window coordination is a
        // Desktop-mode concern). In TV mode, fall back to the sibling behaviour.
        if !matches!(self.window_mode, crate::config::WindowMode::Desktop) {
            self.compositor.detach();
            return false;
        }

        // Pull the launcher window's NSView* out of raw-window-handle (the
        // AppKit equivalent of the Win32 HWND path used elsewhere in this file).
        let Ok(handle) = frame.window_handle() else {
            return false;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return false;
        };
        let ns_view: *mut std::ffi::c_void = appkit.ns_view.as_ptr();

        // Attach once — idempotent if already on this contextId (stable for the
        // session).
        if !self.compositor.is_attached_to(context_id) {
            // SAFETY: `ns_view` is eframe's owned AppKit window handle for this
            // frame — a valid live NSView*. The compositor retains it.
            unsafe { self.compositor.attach(ns_view, context_id) };
        }

        // Fit the hosted layer to the same largest-16:9-centered sub-rect the
        // WindowSet fallback computes, but VIEW-RELATIVE: the egui inner_rect is
        // screen-global, so subtract its own min to get a (0,0)-origin pane
        // inside the launcher's content. (The compositor's set_frame then
        // applies the AppKit flip — see LIVE-TWEAK SPOT #2 there.)
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let pane_w = rect.width();
            let pane_h = rect.height();
            let ar = 16.0_f32 / 9.0;
            let (gw, gh) = if pane_w / pane_h > ar {
                (pane_h * ar, pane_h)
            } else {
                (pane_w, pane_w / ar)
            };
            // View-relative origin: centered within the (0,0)-anchored content
            // rect. Note inner_rect's own min is the window's screen position,
            // which we deliberately drop here (the hosted layer lives in the
            // view's coordinate space, not the screen's).
            let gx = (pane_w - gw) / 2.0;
            let gy = (pane_h - gh) / 2.0;

            self.compositor
                .set_frame(gx as f64, gy as f64, gw.max(0.0) as f64, gh.max(0.0) as f64);
        }

        true
    }

    /// "Trouble connecting?" diagnostic card (PLAN 17.1/17.3/17.4). Rendered as a
    /// centered floating window over the Main screen when the connectivity
    /// watchdog has raised `connectivity_warning` (no phone connected within the
    /// grace window). Shows the **raw-IP** join URL + a scannable raw-IP QR (the
    /// `.local`-mDNS fallback), a same-Wi-Fi reminder, and a firewall line that
    /// specialises on `firewall_status` — including a one-click "FIX FIREWALL"
    /// button that adds the inbound rule via an elevated `netsh` (17.3) when a
    /// rule is missing.
    fn paint_connectivity_card(&mut self, ctx: &egui::Context, status: &LauncherStatus) {
        use crate::firewall::FirewallStatus;

        // Upload the raw-IP QR texture lazily, the first frame the card shows.
        if self.raw_ip_qr_texture.is_none() {
            self.raw_ip_qr_texture = Some(main_screen::pixels_to_egui_texture(
                ctx,
                &self.raw_ip_qr_pixels,
            ));
        }
        let firewall = status.firewall_status;
        let gold = egui::Color32::from_rgb(245, 198, 52);
        let body = egui::Color32::from_rgb(220, 228, 245);

        egui::Window::new("trouble_connecting")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(8, 14, 36))
                    .stroke(egui::Stroke::new(2.0, gold))
                    .rounding(egui::Rounding::same(16.0))
                    .inner_margin(egui::Margin::same(28.0)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(540.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("TROUBLE CONNECTING?")
                            .size(34.0)
                            .strong()
                            .color(gold),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "No phone has joined yet. On your phone, make sure you're on \
                             the same Wi-Fi, then open this address:",
                        )
                        .size(18.0)
                        .color(body),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(&self.raw_ip_url)
                            .size(22.0)
                            .monospace()
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(14.0);
                    if let Some(tex) = &self.raw_ip_qr_texture {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(180.0, 180.0), egui::Sense::hover());
                        ui.painter().image(
                            tex.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    ui.add_space(14.0);
                    match firewall {
                        FirewallStatus::RuleMissing => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Windows Firewall is blocking port {}.",
                                    self.bind_port
                                ))
                                .size(17.0)
                                .color(egui::Color32::from_rgb(255, 180, 120)),
                            );
                            ui.add_space(8.0);
                            let btn = egui::Button::new(
                                egui::RichText::new("FIX FIREWALL (ALLOW PORT)")
                                    .size(18.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(42, 26, 0)),
                            )
                            .fill(gold)
                            .min_size(egui::vec2(300.0, 46.0));
                            if ui.add(btn).clicked() {
                                // Elevated netsh runs off the UI thread (UAC
                                // prompt can block); on success re-check and
                                // update the status so the card flips to Healthy.
                                let status_arc = self.status.clone();
                                let port = self.bind_port;
                                std::thread::spawn(move || {
                                    match crate::firewall::add_inbound_rule_elevated(port) {
                                        Ok(()) => {
                                            let re = crate::firewall::check_inbound_rule(port);
                                            if let Ok(mut st) = status_arc.lock() {
                                                st.firewall_status = re;
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!("firewall one-click fix failed: {e}")
                                        }
                                    }
                                });
                            }
                        }
                        FirewallStatus::Healthy => {
                            ui.label(
                                egui::RichText::new(
                                    "\u{2713} Firewall rule is in place \u{2014} \
                                     double-check the phone is on the same Wi-Fi.",
                                )
                                .size(16.0)
                                .color(egui::Color32::from_rgb(150, 220, 160)),
                            );
                        }
                        FirewallStatus::FirewallOff => {
                            ui.label(
                                egui::RichText::new(
                                    "Firewall is off \u{2014} not the problem. \
                                     Make sure the phone is on the same Wi-Fi network.",
                                )
                                .size(16.0)
                                .color(body),
                            );
                        }
                        FirewallStatus::Unknown => {
                            ui.label(
                                egui::RichText::new(
                                    "If it still won't connect, allow this app through \
                                     Windows Firewall and confirm the phone shares this Wi-Fi.",
                                )
                                .size(15.0)
                                .color(egui::Color32::from_rgb(180, 190, 210)),
                            );
                        }
                    }
                });
            });
    }
}

impl eframe::App for LauncherApp {
    /// Fully transparent GL clear every frame. eframe's default pulls
    /// from `visuals.window_fill` (dark grey in the default dark theme),
    /// which painted a dim grey-black over RPCS3 during the in-game
    /// transparent surface — the `Frame::none().fill(TRANSPARENT)` on
    /// the CentralPanel only skips the panel's own paint, it does not
    /// change the pre-panel GL clear. Main / Crashed / Farewell are
    /// unaffected because they paint opaque `paint_sky_background` +
    /// starfield + vortex on top of the clear; only in-game, which
    /// deliberately paints nothing below the reconnect QR, exposes the
    /// clear color to the compositor. Chris flagged 2026-04-24.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 60 FPS repaint cadence is requested *per branch* below
        // rather than unconditionally here (PLAN 10.7.9). The
        // launcher's animated surfaces (Main vortex + 3D badge,
        // Crashed / Farewell / ServerError intros) want smooth
        // 60fps; the in-game surface paints next to nothing
        // (transparent panel + occasional reconnect-QR fade-in)
        // and was burning constant CPU + GPU on empty redraws.
        // Empirically that idled the laptop in the high-80s%
        // CPU even with the launcher window offscreen and the
        // game (RPCS3) on top. Anything that mutates server
        // status from a background task already wakes egui via
        // its own `Context::request_repaint`, so this branch-
        // local approach doesn't miss state transitions.

        // Lazy-init the vortex shader rig on the first frame. The eframe
        // glow context isn't exposed until `update()` runs, so we can't
        // do this in `LauncherApp::new`. On init failure (driver bug,
        // unsupported GLSL version) we log and continue with the rig
        // stuck at None — the launcher renders without the vortex
        // layer rather than crashing.
        if self.vortex_rig.lock().unwrap().is_none()
            && let Some(gl) = frame.gl()
        {
            match ShaderRig::new(gl) {
                Ok(rig) => *self.vortex_rig.lock().unwrap() = Some(rig),
                Err(e) => tracing::error!("vortex shader init failed: {e}"),
            }
        }
        // Same lazy-init for the badge rig (PLAN 10.7.1). Hands the
        // pre-rendered QR pixels in plus a per-`BackFace`
        // text-on-gold raster (PLAN 10.7.6b) so the GL texture
        // uploads happen here, on-thread, with the GL context
        // available — they stay alive until on_exit.
        if self.badge_rig.lock().unwrap().is_none()
            && let Some(gl) = frame.gl()
        {
            // Pre-rasterise back-face texts at the QR's pixel
            // resolution so the `BadgeRig`'s LINEAR mag filter
            // gives both the same on-screen sharpness when bound.
            let size = self.qr_pixels.width.max(self.qr_pixels.height);
            let back_face_pixels: Vec<Vec<u8>> = main_screen::BackFace::ALL
                .iter()
                .map(|bf| crate::badge_text::render(bf.lines(), size))
                .collect();

            let mut sources = vec![crate::badge::TextureSource::from(&self.qr_pixels)];
            for px in &back_face_pixels {
                sources.push(crate::badge::TextureSource {
                    width: size,
                    height: size,
                    rgba: px.as_slice(),
                });
            }

            match BadgeRig::new(gl, &sources) {
                Ok(rig) => *self.badge_rig.lock().unwrap() = Some(rig),
                Err(e) => tracing::error!("badge shader init failed: {e}"),
            }
        }

        let status_snapshot = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        let time_s = self.started.elapsed().as_secs_f32();

        // Latch the server-ready timestamp the first frame we see the
        // server is up. The launch_phase clock runs from here (not
        // from app mount) so the intro animation only plays once the
        // server is actually healthy — startup failures route to
        // ServerError before this latches and the spin never fires.
        if status_snapshot.server_ready && self.server_ready_at.is_none() {
            self.server_ready_at = Some(Instant::now());
        }

        // PLAN 16.9.3: while RPCS3's settings GUI is open on the TV, step the
        // launcher fully out of the way. The Qt settings window needs the whole
        // screen + the HTPC keyboard/mouse, and the launcher is otherwise
        // always-on-top — so minimise it for the duration and restore + refocus
        // when the user closes RPCS3. We keep a slow repaint going while
        // minimised so the loop notices the close (the config watcher flips
        // `config_gui_open` from a background task) even though nothing else is
        // animating, and early-return so the always-on-top / render logic below
        // doesn't fight the minimise.
        if status_snapshot.config_gui_open {
            if !self.config_gui_minimized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.config_gui_minimized = true;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        if self.config_gui_minimized {
            // Settings just closed — bring the launcher back to the foreground.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.config_gui_minimized = false;
        }

        // Always-on-top toggle. Release: always on. Dev: only while
        // RPCS3 is running so the launcher overlays the game for
        // in-game testing without sticking on top during normal code
        // iteration (where alt-tab matters).
        //
        // Two layers of enforcement:
        //   1. egui's `ViewportCommand::WindowLevel` for the initial
        //      transition Normal ↔ AlwaysOnTop.
        //   2. Direct `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE)`
        //      every frame on Windows. The egui/winit path isn't
        //      aggressive enough to beat Win32 menus + dropdowns —
        //      those use a higher z-class and activate after us, so
        //      they slide above the launcher. Re-asserting via raw
        //      Win32 with `SWP_NOACTIVATE` keeps us at the top of
        //      the topmost stack without stealing focus from RPCS3
        //      (Chris flagged 2026-04-19, "menus still win").
        // In dev mode, on top whenever a game session is in flight —
        // either loading (RPCS3 spawning + UIA-booting) or already
        // running. Earlier this only flipped on `rpcs3_running`,
        // which meant the loading surface drew BEHIND RPCS3's main
        // window for the ~30s of boot (Chris flagged 2026-04-19).
        // PLAN 16.6.2.2 — z-order. Under IPC + no-GUI there are no dialog / menu
        // windows to out-fight (no Skylanders Manager, no RPCS3 menu bar), so the
        // launcher need not be desktop-topmost — it only sits directly above the
        // borderless game window. That keeps the overlay over the game AND lets the
        // user alt-tab away (the old absolute-topmost trapped the screen). The
        // legacy UIA path keeps the topmost-fighting it still needs.
        if status_snapshot.driver_is_ipc {
            // Never desktop-topmost: keep the OS window level Normal.
            if self.window_on_top_state != Some(false) {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
                self.window_on_top_state = Some(false);
            }
            // P8 surface-embed (macOS, Phase C): when the driver has published the
            // game's CAContextID, host its render layer INSIDE this launcher view
            // via CALayerHost (composited behind egui's chrome) instead of tiling a
            // second top-level window beneath us. This is the PRIMARY macOS path;
            // the `WindowSet` fit below is the fallback for when no contextId is
            // present. The contextId is stable for the session, so we attach once.
            // Only consumed by the `#[cfg(not(windows))]` fit block below, so it's
            // only bound there (Windows tiles via Win32 and never embeds).
            #[cfg(not(windows))]
            let surface_embedded = {
                #[cfg(target_os = "macos")]
                {
                    self.try_embed_game_surface(frame, ctx, &status_snapshot)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    false
                }
            };

            // Re-assert every frame while a game is live: keep the game sandwiched
            // directly below the launcher so no other window (e.g. a terminal the
            // user alt-tabbed to) can drift *between* them and show through the
            // transparent overlay. This is relative + SWP_NOACTIVATE — it positions
            // the game beneath the launcher without raising the launcher to topmost,
            // so alt-tabbing away to other apps still works (they go above both).
            if let Some(game_hwnd) = status_snapshot.game_window_handle {
                // PLAN 20.4: in Desktop window mode the launcher is a resizable
                // window, so the game is *fitted* to its client rect (move +
                // resize) as well as z-ordered below it; TV mode is fullscreen,
                // so the game just needs the every-frame z-order assertion.
                #[cfg(windows)]
                if matches!(self.window_mode, crate::config::WindowMode::Desktop) {
                    fit_game_below_launcher_via_win32(frame, game_hwnd);
                } else {
                    place_game_below_launcher_via_win32(frame, game_hwnd);
                }
                // PLAN B.2 — macOS can't move another app's window via Win32, so
                // the Desktop-mode fit goes over IPC (P7 `WINDOW_SET`). TV mode is
                // a no-op here (the RPCS3 window and egui launcher coexist as plain
                // siblings — window coordination is Win32-only). `game_hwnd` is
                // unused on this path (the IPC fit doesn't need the handle).
                #[cfg(not(windows))]
                {
                    let _ = game_hwnd;
                    // Fit as soon as the game window exists (the enclosing
                    // `if let Some(game_hwnd)` already gates on that), NOT on
                    // `game_playable` — that gate fired too late, after RPCS3's
                    // boot-time self-resize had already stranded the game out of
                    // the pane. We instead absorb the boot churn with a low-rate
                    // re-assert (the ≥0.5s branch below).
                    //
                    // FALLBACK ONLY: when the surface is embedded in-window via
                    // CALayerHost (P8) there's no separate window to tile, so skip
                    // the WINDOW_SET fit entirely.
                    if !surface_embedded
                        && matches!(self.window_mode, crate::config::WindowMode::Desktop)
                        && let Some(rect) = ctx.input(|i| i.viewport().inner_rect)
                    {
                        // The game renders 16:9; the launcher pane is not. Fit the
                        // largest 16:9 sub-rect centered in the pane so the game
                        // WINDOW is all-game (no letterbox black bars inside it).
                        let pane = rect;
                        let ar = 16.0_f32 / 9.0;
                        let (gw, gh) = if pane.width() / pane.height() > ar {
                            (pane.height() * ar, pane.height())
                        } else {
                            (pane.width(), pane.width() / ar)
                        };
                        let gx = pane.min.x + (pane.width() - gw) / 2.0;
                        let gy = pane.min.y + (pane.height() - gh) / 2.0;

                        // Round to the WINDOW_SET geometry. See the FLAG in the PR
                        // notes: whether this is screen vs window-relative and points
                        // vs physical px needs live verification on macOS.
                        let x = gx.round() as i32;
                        let y = gy.round() as i32;
                        let w = gw.round().max(0.0) as u32;
                        let h = gh.round().max(0.0) as u32;
                        let next = (x, y, w, h);
                        // Re-send on a genuine rect change OR if ≥0.5s elapsed since
                        // the last send (the low-rate re-assert that beats RPCS3's
                        // boot-time self-resize without an IPC round-trip per frame).
                        let now = ctx.input(|i| i.time);
                        let changed = self.last_window_set != Some(next);
                        if changed || self.last_window_set_at.is_none_or(|t| now - t >= 0.5) {
                            if changed {
                                tracing::info!(
                                    pane = ?(pane.min.x, pane.min.y, pane.width(), pane.height()),
                                    set = ?(x, y, w, h),
                                    "B.3 fit: inner_rect -> WINDOW_SET"
                                );
                            }
                            let _ = self.driver_tx.try_send(crate::state::DriverJob::WindowSet {
                                x,
                                y,
                                w,
                                h,
                            });
                            self.last_window_set = Some(next);
                            self.last_window_set_at = Some(now);
                        }
                    }
                }
            }
        } else {
            // Legacy UIA z-order. Two layers: egui WindowLevel for the Normal ↔
            // AlwaysOnTop transition, plus a per-frame raw SetWindowPos(HWND_TOPMOST,
            // SWP_NOACTIVATE) — the egui/winit path isn't aggressive enough to beat
            // Win32 menus + the Skylanders Manager dialog. In dev, on top only while
            // a game session is in flight (loading or running) so alt-tab works
            // during normal code iteration.
            let want_on_top = if cfg!(feature = "dev-tools") {
                status_snapshot.rpcs3_running || status_snapshot.loading_game.is_some()
            } else {
                true
            };
            if self.window_on_top_state != Some(want_on_top) {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if want_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
                self.window_on_top_state = Some(want_on_top);
            }
            if want_on_top {
                force_topmost_via_win32(frame);
            }

            // While a game is played, demote RPCS3's main (library / menu-bar)
            // window to the bottom: UIA Invoke on the Skylanders Manager dialog
            // transiently promotes main over the game viewport (the library flashes
            // through the transparent in-game surface on each figure swap). Gate on
            // `!switching` so the mid-game-switch menu drive isn't kneecapped.
            let game_live = status_snapshot.rpcs3_running
                && status_snapshot.current_game.is_some()
                && !status_snapshot.switching;
            if game_live {
                push_rpcs3_main_to_bottom_via_win32();
            }
        }

        // Per-screen entry detection — compare variant discriminants
        // (not full equality) so e.g. `Crashed { msg }` with a
        // changing message doesn't re-trigger the entry animation.
        // Reset the entry clock on every variant change; non-Main
        // screens use it to drive their badge-spin-in.
        if std::mem::discriminant(&self.current_screen)
            != std::mem::discriminant(&status_snapshot.screen)
        {
            tracing::info!(
                from = ?self.current_screen,
                to = ?status_snapshot.screen,
                "launcher screen variant changed",
            );
            self.current_screen = status_snapshot.screen.clone();
            self.screen_entered_at = Instant::now();
        }
        let screen_intro = ScreenIntro {
            elapsed_s: self.screen_entered_at.elapsed().as_secs_f32(),
        };

        // Reset the farewell timer when we're NOT on the farewell surface
        // — so if a future path flips screen out of Farewell (none today)
        // the next time we enter it, the 3s countdown restarts from zero.
        if !matches!(status_snapshot.screen, LauncherScreen::Farewell) {
            self.farewell_started_at = None;
        }

        // Close-animation timers — see `sequencer::CloseTimers` for
        // the full lifecycle rules. The two trigger timestamps
        // (in-game start vs shutdown) feed into one elapsed value
        // because `LaunchPhase::compute` doesn't care which trigger
        // fired; only that one is in flight.
        let now = Instant::now();
        self.close_timers.tick(now, &status_snapshot);
        let closing_elapsed_s = self.close_timers.elapsed_s(now);

        // Launcher start-of-life phasing (PLAN 4.19.2a). Only meaningful
        // for the Main screen — Crashed and Farewell are explicit
        // overrides that should render immediately regardless of how
        // long ago the launcher booted. The phase drives the vortex
        // iris, the badge spin scale + alpha, and the text fade — see
        // `launch_phase.rs` for the choreography.
        // Game-end detection. If last frame was rendered as in_game
        // and this frame isn't, the user is returning to the
        // launcher (RPCS3 quit normally → screen still Main, no
        // crash). Stamp `returning_from_game_at` so the launch_phase
        // computation switches to `ReturnFromGame` for the next
        // ~INTRO_TRANSITION_S, replaying the iris reveal + badge
        // spin-in without the brand intro.
        if detect_returning_from_game(self.was_in_game, &status_snapshot) {
            self.returning_from_game_at = Some(now);
            // Defensive: if a fast quit sequence skipped the in-game
            // render's `loading_game = None` clear (e.g. game
            // launched + quit before the close animation completed),
            // belt-and-suspenders clear it here so the launcher
            // doesn't return to Main with a stale LOADING badge
            // pinned over the QR (Chris 2026-05-04).
            if let Ok(mut st) = self.status.lock() {
                st.loading_game = None;
            }
        }

        let returning_elapsed_s = self
            .returning_from_game_at
            .map(|t| t.elapsed().as_secs_f32());

        // PLAN 10.8.7d: Farewell now does 0→open (ReturnFromGame
        // style) with the GOODBYE badge spinning in, matching the
        // grammar of every other "cover transition." The iris-close
        // animation it used to do is the only state in the launcher
        // that visually resembled "going dark," but the strict
        // invariant is "only Farewell's black-fade overlay shows
        // black" — and that overlay is rendered separately in
        // `farewell::render` regardless of iris direction. So:
        //   - feed `close_timers.shutdown_at` as the returning
        //     timeline (was: closing) so launch_phase resolves to
        //     `ReturnFromGame { progress }` instead of
        //     `ClosingToInGame { progress }`.
        //   - iris animates 0 → IRIS_FULL with `iris_mode = Reveal`
        //     (default for ReturnFromGame), badge spins in,
        //     vortex retreats to a ring around the iris.
        //   - black-fade kicks in after FAREWELL_COUNTDOWN (3 s)
        //     in `farewell::render`, painting a full-viewport
        //     `Color32::from_rgba(0,0,0,alpha)` over everything.
        let launch_phase = if matches!(status_snapshot.screen, LauncherScreen::Farewell) {
            LaunchPhase::compute(
                self.started.elapsed().as_secs_f32(),
                None,
                closing_elapsed_s,
                false,
            )
        } else if matches!(status_snapshot.screen, LauncherScreen::Main) {
            // PLAN 4.15.16 regression fix: `has_activity` used to read
            // `rpcs3_running || clients > 0`, but under the always-
            // running RPCS3 contract rpcs3_running is true from the
            // moment of launcher boot (RPCS3 spawns at startup, lives
            // at library view). That made the intro skip every time
            // and the launcher jumped straight to AwaitingConnect
            // without the iris reveal / badge spin-in. Gate on
            // `current_game.is_some()` instead — true activity is a
            // game booted or a phone connected.
            let has_activity =
                status_snapshot.current_game.is_some() || self.clients.load(Ordering::Relaxed) > 0;
            // Phase elapsed measured from app mount. 2026-04-24 — the
            // prior timeline used `server_ready_at` as the clock base
            // so the intro animations only began once the server was
            // healthy (indexer + driver + axum bind complete). Once the
            // STARTING heraldic title was folded into the card's
            // `BackFace::Starting`, that gate moved: the intro spins up
            // immediately at app mount showing Starting on the card,
            // and the back-face flips to QR/etc. once `server_ready`
            // fires (see main_screen.rs back_face selection). That way
            // the user gets visible motion during the entire server-
            // boot window instead of a static starfield.
            let phase_elapsed_s = self.started.elapsed().as_secs_f32();
            // 2026-04-24 — PLAN 4.15.9 used to override the natural
            // phase to `ClosingToInGame { progress: 1.0 }` while
            // `switching` was set, pinning the iris fully closed so
            // the transparent in-game surface wouldn't flash back to
            // "SCAN TO CONNECT" between games. That override was
            // retired alongside the SWITCHING GAMES QR back-face —
            // the card flip + halos carry the bridge visual now, so
            // we want the normal AwaitingConnect iris (open + vortex
            // visible) behind it, matching the Loading state.
            LaunchPhase::compute(
                phase_elapsed_s,
                closing_elapsed_s,
                returning_elapsed_s,
                has_activity,
            )
        } else {
            LaunchPhase::AwaitingConnect
        };

        // Clear the return timestamp once the animation finishes so
        // we don't keep recomputing ReturnFromGame past its useful
        // life — once the phase resolves to AwaitingConnect we're
        // back to steady state.
        if matches!(launch_phase, LaunchPhase::AwaitingConnect)
            && self.returning_from_game_at.is_some()
        {
            self.returning_from_game_at = None;
        }

        // PLAN 10.8.7e+: single-branch render. The launcher CentralPanel
        // is always transparent and always painted; the iris animation
        // (sky + starfield + vortex) is alpha-masked when a game is
        // running underneath, so the launcher punches through to the
        // RPCS3 viewport instead of doing a hard panel-flip at
        // `reveal_complete`. Cover transitions (graceful quit, switch,
        // crash) drive the same iris machinery in reverse: the
        // launcher disc grows from the centre, with the game still
        // visible at the corners until cover is fully landed.
        //
        // Logical "in-game" state (iris fully open over a live game,
        // no cover transition mid-flight) drives the reconnect-QR
        // overlay + the 4Hz repaint cadence. Same predicate as the
        // old in-game branch — just no longer tied to a panel flip.
        let is_in_game = launch_phase.reveal_complete()
            && status_snapshot.rpcs3_running
            && status_snapshot.current_game.is_some()
            && !status_snapshot.switching
            && !status_snapshot.cover_active
            && matches!(status_snapshot.screen, LauncherScreen::Main);

        // Cache for the next frame's `detect_returning_from_game`.
        let prev_was_in_game = self.was_in_game;
        self.was_in_game = is_in_game;

        // PLAN 4.19.12 — reconnect-QR fade timer. Stamps when in-game
        // with no clients, clears otherwise so a subsequent in-game
        // entry with clients=0 starts the fade fresh.
        let reconnect_fade_elapsed_s = if is_in_game {
            let clients_now = self.clients.load(Ordering::Relaxed);
            if clients_now == 0 {
                if self.reconnect_qr_shown_at.is_none() {
                    self.reconnect_qr_shown_at = Some(Instant::now());
                }
            } else {
                self.reconnect_qr_shown_at = None;
            }
            self.reconnect_qr_shown_at
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0)
        } else {
            self.reconnect_qr_shown_at = None;
            0.0
        };

        // In-game repaint cadence: 60fps while the reconnect-QR fade
        // is animating in (clients=0 case), 4Hz heartbeat once it
        // settles. Otherwise (launcher animations, vortex, badge
        // motion) a full 60fps. PLAN 10.7.9 + 10.8.7.
        if is_in_game {
            let in_game_repaint = if reconnect_fade_elapsed_s > 0.0
                && reconnect_fade_elapsed_s < in_game::RECONNECT_FADE_IN_S
            {
                std::time::Duration::from_millis(16)
            } else {
                std::time::Duration::from_millis(250)
            };
            ctx.request_repaint_after(in_game_repaint);
            // Loading is over — the launch handler intentionally
            // left `loading_game` set so the LOADING badge persisted
            // through compile; clear it now that the iris is fully
            // open over the live game so the next launcher boot
            // starts clean.
            if let Ok(mut st) = self.status.lock() {
                st.loading_game = None;
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        // Punch-through is only meaningful when there's something to
        // show through to. With no RPCS3 alive (boot, picker, post-
        // shutdown), the launcher must paint fully opaque or the
        // user sees the desktop behind the transparent eframe window.
        // Punch-through (iris-masked starfield/vortex revealing the game) is enabled
        // only once the game is actually PLAYABLE — not merely `rpcs3_running`. The
        // boot path can flip `rpcs3_running` early (it breaks on the first
        // `is_playable`, which flickers true in the gap between RPCS3's compile
        // phases), so gating on `game_playable` (the stable end-of-compile signal)
        // holds the opaque loading cover through the whole compile and gives exactly
        // one clean iris reveal — instead of a jump-to-emulator then re-cover/replay
        // (HTPC 2026-05-30 regression).
        let game_underneath = status_snapshot.rpcs3_running
            && status_snapshot.current_game.is_some()
            && status_snapshot.game_playable;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = ui.max_rect();

                // Iris geometry shared by sky/starfield/vortex. The
                // Crashed-from-in-game case overrides the radius with
                // screen_intro's reveal so the vortex grows in alongside
                // the badge spin (without it, the launcher would snap
                // from transparent to full vortex on screen-variant
                // change). Other states use launch_phase directly.
                let iris_radius = match (&status_snapshot.screen, prev_was_in_game) {
                    (LauncherScreen::Crashed { .. }, true) => screen_intro.iris_radius(),
                    _ => launch_phase.iris_radius(),
                };
                let iris_mode = launch_phase.iris_mode();
                let iris_softness = self.vortex_idle.iris_softness;

                // Layer 0: sky backdrop. Iris-masked when a game's
                // underneath so the launcher disc punches through to the
                // RPCS3 viewport at `iris_factor = 0`. The decorative
                // top/bottom radial-glow ellipses are skipped during
                // punch-through (subtle enough that omitting them is
                // imperceptible).
                if game_underneath {
                    let mask = vortex::IrisMask {
                        iris_radius,
                        iris_softness,
                        iris_mode,
                    };
                    vortex::paint_sky_background_masked(ui.painter(), rect, mask);
                } else {
                    vortex::paint_sky_background(ui.painter(), rect);
                }

                // Layer 1: tuned CPU starfield. Same iris-mask treatment
                // — stars inside the iris hole get alpha=0 so the game
                // viewport is unobstructed.
                if game_underneath {
                    let mask = vortex::IrisMask {
                        iris_radius,
                        iris_softness,
                        iris_mode,
                    };
                    vortex::paint_starfield_masked(ui.painter(), rect, time_s, mask);
                } else {
                    vortex::paint_starfield(ui.painter(), rect, time_s);
                }

                // Layer 2: GPU vortex shader. Already iris-aware (same
                // `iris_factor` math the masked CPU paths use, just on
                // the GPU). `star_brightness` is forced to 0; production
                // uses the CPU starfield (Layer 1).
                let mut vortex_params = self.vortex_idle;
                vortex_params.iris_radius = iris_radius;
                vortex_params.iris_mode = iris_mode;
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
                // tuned to. UNLIKE Layer 1 this one isn't iris-masked, so it
                // must be SKIPPED once a game is being revealed — otherwise its
                // white star dots paint on TOP of the game (visible over
                // Giants' dark Activision intro for ~20-30s) instead of
                // clearing with the iris (HTPC 2026-05-30).
                if !game_underneath {
                    vortex::paint_starfield(ui.painter(), rect, time_s);
                }

                // Layer 3: per-screen content.
                match &status_snapshot.screen {
                    LauncherScreen::Main => {
                        // 2026-04-24 — two prior special cases retired from
                        // this branch as part of the ring-badge standard-
                        // isation:
                        //   1. The `switching` heading was folded into the
                        //      card's `BackFace::Switching` (halo spin).
                        //   2. The heraldic `STARTING` brand intro title
                        //      was folded into `BackFace::Starting`.
                        // `render_main` owns the whole centre layout now;
                        // the back-face carries every non-QR state.
                        self.render_main(ui, ctx, &status_snapshot, launch_phase);
                    }
                    LauncherScreen::Crashed { message } => {
                        crashed::render(
                            ui,
                            &self.status,
                            self.badge_rig.clone(),
                            message,
                            screen_intro,
                        );
                    }
                    LauncherScreen::Farewell => {
                        // PLAN 10.8.7d: badge spins in WITH the iris-open
                        // animation (was: badge already landed while iris
                        // closed around it). Pass the live `screen_intro`
                        // — `screen_entered_at` resets on screen-variant
                        // change so the intro starts at 0 the first frame
                        // of Farewell, the badge grows + spins + fades in
                        // over `ScreenIntro::DURATION_S` (1.2 s), iris
                        // opens 0→IRIS_FULL over `INTRO_TRANSITION_S`
                        // (1.8 s) — they land together.
                        //
                        // Countdown still starts on the first call to
                        // farewell::render → first frame of screen=
                        // Farewell, covering the iris-open + steady
                        // GOODBYE beat together (1.8 s open + 1.2 s
                        // hold = 3 s, then the 0.8 s black-fade overlay
                        // kicks in).
                        if self.farewell_started_at.is_none() {
                            tracing::info!("farewell countdown starting");
                        }
                        farewell::render(
                            ui,
                            ctx,
                            self.badge_rig.clone(),
                            &mut self.farewell_started_at,
                            screen_intro,
                        );
                    }
                    LauncherScreen::ServerError { message } => {
                        server_error::render(
                            ui,
                            ctx,
                            self.badge_rig.clone(),
                            message,
                            screen_intro,
                        );
                    }
                }

                // In-game reconnect-QR overlay. Iris is fully open over a
                // live game (`is_in_game`); the launcher's sky/stars/
                // vortex are all alpha=0 across the screen, so painting
                // here lays the QR coin directly on top of the game
                // viewport — same effect as the old transparent in-game
                // panel, just folded into the single-branch render.
                if is_in_game {
                    in_game::render(
                        ui,
                        &self.clients,
                        self.qr_texture.as_ref(),
                        reconnect_fade_elapsed_s,
                    );
                }
            });

        // PLAN 17.1: overlay the "Trouble connecting?" diagnostic on the join
        // screen when the watchdog has flagged that no phone has reached us.
        // Only on Main (not in-game / Crashed / Farewell) — it's about getting
        // the first phone connected.
        if status_snapshot.connectivity_warning
            && !is_in_game
            && matches!(status_snapshot.screen, LauncherScreen::Main)
        {
            self.paint_connectivity_card(ctx, &status_snapshot);
        }
    }

    fn on_exit(&mut self, gl: Option<&egui_glow::glow::Context>) {
        // Release GL resources (program / VBO / VAO) cleanly. eframe
        // would tear them down anyway on context drop, but doing it
        // explicitly avoids spurious "leaked GL handle" warnings on
        // some drivers.
        if let (Some(gl), Some(rig)) = (gl, self.vortex_rig.lock().unwrap().as_ref()) {
            rig.destroy(gl);
        }
        if let (Some(gl), Some(rig)) = (gl, self.badge_rig.lock().unwrap().as_ref()) {
            rig.destroy(gl);
        }
    }
}

/// Force the launcher window to the top of the Win32 z-order via
/// `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)`.
/// Called every frame by `update()` when always-on-top is desired.
/// The `SWP_NOACTIVATE` is critical: it keeps us above other topmost
/// windows (RPCS3, system menus, taskbar) without stealing focus from
/// RPCS3 — the user can still interact with the game while the
/// launcher overlays correctly.
///
/// No-op on non-Windows targets (the project is Windows-only per
/// PLAN Phase 7, but the cfg gate keeps the file portable).
#[cfg(windows)]
fn force_topmost_via_win32(frame: &eframe::Frame) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    // SAFETY: `hwnd` came from eframe's owned window handle this frame;
    // it's a valid HWND for the lifetime of this call. SetWindowPos
    // is thread-safe and the SWP flags ensure we don't move/resize/
    // activate — purely a z-order assertion.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn force_topmost_via_win32(_frame: &eframe::Frame) {}

/// Every-frame z-order demotion of RPCS3's main window while a game is
/// live. Thin wrapper over `skylander_rpcs3_control::hide::
/// push_rpcs3_main_to_bottom`; keeps the Windows-only dependency
/// contained in this file.
#[cfg(windows)]
fn push_rpcs3_main_to_bottom_via_win32() {
    let _ = skylander_rpcs3_control::hide::push_rpcs3_main_to_bottom();
}

#[cfg(not(windows))]
fn push_rpcs3_main_to_bottom_via_win32() {}

/// IPC/no-GUI z-order (PLAN 16.6.2.2): slot the borderless game window directly
/// BELOW the launcher, so the overlay sits just above the game WITHOUT making the
/// launcher desktop-topmost — the user can still alt-tab to other apps. There are
/// no menu / dialog windows under no-GUI + IPC, so the old absolute-topmost +
/// push-to-bottom dance is unnecessary. Called every frame while a game is live —
/// a relative SWP_NOACTIVATE re-order is cheap and a no-op once already in order.
#[cfg(windows)]
fn place_game_below_launcher_via_win32(frame: &eframe::Frame, game_hwnd: u64) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let launcher = HWND(win32.hwnd.get() as *mut _);
    let game = HWND(game_hwnd as *mut _);
    // SAFETY: `launcher` is eframe's owned handle this frame; `game` is the HWND
    // RPCS3 published over IPC. Inserting `game` immediately after `launcher` in the
    // z-order puts the launcher directly above the game. SWP_NOACTIVATE avoids
    // stealing focus; NOMOVE/NOSIZE leave geometry untouched.
    unsafe {
        let _ = SetWindowPos(
            game,
            Some(launcher),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// PLAN 20.4 (Desktop window mode): keep the RPCS3 window fitted to the
/// launcher's client rect (move + resize) AND directly below it in z-order, so
/// the game fills the windowed launcher's content area and tracks its
/// move/resize. The full geometry is re-applied EVERY frame — RPCS3 comes up
/// fullscreen and re-sizes its own window during boot, and a change-detected skip
/// let that override drift out of the fit (the game looked unconstrained); always
/// re-applying stops it (SetWindowPos to identical geometry is a near-no-op).
/// No-op if the launcher is unavailable / minimised (degenerate client rect), so
/// we never fit the game to a 0×0 box.
#[cfg(windows)]
fn fit_game_below_launcher_via_win32(frame: &eframe::Frame, game_hwnd: u64) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, SWP_NOACTIVATE, SetWindowPos};

    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let launcher = HWND(win32.hwnd.get() as *mut _);
    let game = HWND(game_hwnd as *mut _);

    // SAFETY: `launcher` is eframe's owned handle this frame; `game` is the HWND
    // RPCS3 published over IPC. The calls read the launcher's client rect and
    // re-order/resize the game — no ownership transfer, valid for this call.
    unsafe {
        let mut rc = RECT::default();
        if GetClientRect(launcher, &mut rc).is_err() {
            return;
        }
        // Client (0,0) → screen coords is the top-left; the rect's right/bottom
        // are the client width/height (left/top are always 0).
        let mut tl = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(launcher, &mut tl);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return; // minimised / degenerate — don't fit the game to 0×0
        }
        // Move + resize + z-order every frame (re-orders the game directly below
        // the launcher and keeps it sized to the client rect).
        let _ = SetWindowPos(game, Some(launcher), tl.x, tl.y, w, h, SWP_NOACTIVATE);
    }
}
