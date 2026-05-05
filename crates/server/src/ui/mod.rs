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
        // Render once into raw RGBA pixels; the egui-side texture and
        // the GL-side BadgeRig texture (PLAN 10.7.2) both consume the
        // same buffer so they stay byte-identical.
        let qr_pixels = main_screen::render_qr_pixels(&url);
        let qr_texture = Some(main_screen::pixels_to_egui_texture(
            &cc.egui_ctx,
            &qr_pixels,
        ));
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
            vortex_rig: Arc::new(Mutex::new(None)),
            vortex_idle: vortex::idle_params(),
            badge_rig: Arc::new(Mutex::new(None)),
            qr_pixels,
        }
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

        // While a game is actually being played, demote RPCS3's main
        // (library / menu-bar) window to the bottom of the z-order.
        // UIA `Invoke` on the Skylanders Manager dialog's Load/Clear
        // buttons transiently promotes main over the game viewport —
        // visible to the player as the library flashing through the
        // launcher's transparent in-game surface each time a figure
        // is swapped. Gate on `!switching` so the mid-game-switch
        // window (where `trigger_dialog_via_menu` briefly needs main
        // foregrounded to drive the menu) isn't kneecapped. Chris
        // flagged 2026-04-24.
        let game_live = status_snapshot.rpcs3_running
            && status_snapshot.current_game.is_some()
            && !status_snapshot.switching;
        if game_live {
            push_rpcs3_main_to_bottom_via_win32();
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
        let game_underneath =
            status_snapshot.rpcs3_running && status_snapshot.current_game.is_some();

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
                // tuned to.
                vortex::paint_starfield(ui.painter(), rect, time_s);

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
