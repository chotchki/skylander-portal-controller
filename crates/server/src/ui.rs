//! Fullscreen eframe window. Shows the QR code for the phone URL, the URL
//! itself, and a live connected-client count. Phase 3 adds Manage-dialog /
//! restart-game / exit buttons. PLAN 4.15.10 + 4.15.11 layer two additional
//! full-screen surfaces (`Crashed`, `Farewell`) dispatched from the polled
//! `LauncherStatus.screen` enum.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::state::{LauncherScreen, LauncherStatus};
use crate::{fonts, palette};

/// How long the farewell screen lingers before the launcher issues
/// `ViewportCommand::Close`. Matches the navigation-doc spec (§3.5 —
/// ~2.2s read pause + 1.6s fade-in of the "(launcher will exit)" hint).
/// Kept as a single const so the countdown text and the close trigger
/// can't drift apart.
const FAREWELL_COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(3);

pub struct LauncherApp {
    clients: Arc<AtomicUsize>,
    status: Arc<std::sync::Mutex<LauncherStatus>>,
    url: String,
    figure_count: usize,
    qr_texture: Option<egui::TextureHandle>,
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
        let qr_texture = Some(render_qr_texture(&cc.egui_ctx, &url));
        Self {
            clients,
            status,
            url,
            figure_count,
            qr_texture,
            farewell_started_at: None,
        }
    }
}

/// Header strip: RPCS3 connection dot + current-game label (PLAN 4.15.4).
/// Absorbs the 2.8.4 deferral — a steady green dot while the emulator is
/// running, dim grey otherwise. The current-game name renders in Titan
/// One gold when a game is booted; blank otherwise.
fn status_strip(ui: &mut egui::Ui, status: &LauncherStatus) {
    const DOT_RADIUS: f32 = 10.0;
    let (dot_colour, tooltip) = if status.rpcs3_running {
        (palette::SUCCESS_GLOW, "RPCS3 running")
    } else {
        (palette::TEXT_DIM, "RPCS3 idle")
    };

    ui.horizontal(|ui| {
        // Let the strip grow to the panel width so `with_layout` centering
        // inside `vertical_centered` gives us the full row to work with.
        ui.set_min_width(ui.available_width());
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space(24.0);
            // Dot — allocate a small square and paint a circle in its centre.
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(DOT_RADIUS * 2.0 + 4.0, DOT_RADIUS * 2.0 + 4.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .circle_filled(rect.center(), DOT_RADIUS, dot_colour);
            // Subtle outer ring for contrast against the starfield background.
            ui.painter().circle_stroke(
                rect.center(),
                DOT_RADIUS,
                egui::Stroke::new(1.5, palette::GOLD_INK),
            );
            response.on_hover_text(tooltip);

            ui.add_space(12.0);
            match &status.current_game {
                Some(name) => {
                    ui.label(
                        egui::RichText::new(name)
                            .size(26.0)
                            .color(palette::GOLD)
                            .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new("no game running")
                            .size(22.0)
                            .italics()
                            .color(palette::TEXT_DIM),
                    );
                }
            }
        });
    });
}

/// Frame the QR in a gold bezel equivalent to the phone's `GoldBezel` —
/// rectangular rather than circular (the phone uses circles for figures;
/// the QR needs a square frame) but the same colour story: gold body with a
/// darker gold ink hairline outer edge and a near-black bezel plate
/// surrounding the QR (PLAN 4.15.3). The radial-gradient + multi-layer
/// inset shadows of the CSS version would need a custom `egui::Painter`
/// pass; the stacked-Frame approach below is ~95% of the visual payoff for
/// 5% of the code. Revisit via a custom painter in 4.15a polish if the TV
/// looks flat.
fn qr_in_gold_bezel(ui: &mut egui::Ui, tex: &egui::TextureHandle) {
    let size = tex.size_vec2();
    // Outer gold body — the visible bezel ring.
    egui::Frame::none()
        .fill(palette::GOLD)
        .stroke(egui::Stroke::new(2.0, palette::GOLD_INK))
        .inner_margin(egui::Margin::same(18.0))
        .rounding(egui::Rounding::same(14.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 6.0),
            blur: 18.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(160),
        })
        .show(ui, |ui| {
            // Bezel plate — darker SF_3 rim framing the QR itself, matching
            // the phone's `linear-gradient(#1a2a4a, #0a1630)` plate colour
            // (approximated as a solid fill — egui::Frame doesn't do
            // gradients without a custom painter).
            egui::Frame::none()
                .fill(palette::SF_3)
                .stroke(egui::Stroke::new(1.0, palette::GOLD_SHADOW))
                .inner_margin(egui::Margin::same(10.0))
                .rounding(egui::Rounding::same(8.0))
                .show(ui, |ui| {
                    ui.image((tex.id(), size));
                });
        });
}

fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    let code = qrcode::QrCode::new(url).expect("qr encode");
    // QR renders in starfield-blue-on-white for readability. Matches the
    // phone's selection-on-dark treatment (dark modules on a white
    // quiet-zone background is what most QR scanners expect).
    let dark = palette::SF_2;
    let light = egui::Color32::WHITE;
    let scale = 10usize;
    let modules: Vec<Vec<bool>> = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build()
        .lines()
        .map(|l| l.chars().map(|c| c != ' ').collect())
        .collect();
    let h = modules.len();
    let w = modules.first().map(|r| r.len()).unwrap_or(0);
    let img_w = w * scale;
    let img_h = h * scale;
    let mut pixels = Vec::with_capacity(img_w * img_h);
    for y in 0..img_h {
        for x in 0..img_w {
            let b = modules[y / scale][x / scale];
            pixels.push(if b { dark } else { light });
        }
    }
    let color_image = egui::ColorImage {
        size: [img_w, img_h],
        pixels,
    };
    ctx.load_texture("qr", color_image, egui::TextureOptions::NEAREST)
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        let status_snapshot = self.status.lock().map(|s| s.clone()).unwrap_or_default();

        // Reset the farewell timer when we're NOT on the farewell surface
        // — so if a future path flips screen out of Farewell (none today)
        // the next time we enter it, the 3s countdown restarts from zero.
        if !matches!(status_snapshot.screen, LauncherScreen::Farewell) {
            self.farewell_started_at = None;
        }

        egui::CentralPanel::default().show(ctx, |ui| match &status_snapshot.screen {
            LauncherScreen::Main => {
                self.render_main(ui, ctx, &status_snapshot);
            }
            LauncherScreen::Crashed { message } => {
                render_crashed(ui, &self.status, message);
            }
            LauncherScreen::Farewell => {
                render_farewell(ui, ctx, &mut self.farewell_started_at);
            }
        });
    }
}

impl LauncherApp {
    fn render_main(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        status_snapshot: &LauncherStatus,
    ) {
        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            status_strip(ui, status_snapshot);
            ui.add_space(8.0);
            ui.heading(
                egui::RichText::new("SKYLANDER PORTAL")
                    .size(80.0)
                    .color(palette::GOLD)
                    .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
            );
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("Scan to connect:")
                    .size(36.0)
                    .color(palette::TEXT_DIM),
            );
            ui.add_space(24.0);
            if let Some(tex) = &self.qr_texture {
                qr_in_gold_bezel(ui, tex);
            }
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new(&self.url)
                    .size(32.0)
                    .monospace()
                    .color(palette::GOLD),
            );
            ui.add_space(16.0);

            let n = self.clients.load(Ordering::Relaxed);
            let status = if n == 0 {
                "Waiting for phone…".to_string()
            } else if n == 1 {
                "1 device connected".to_string()
            } else {
                format!("{n} devices connected")
            };
            ui.label(egui::RichText::new(status).size(40.0).color(palette::TEXT));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{} figures indexed", self.figure_count))
                    .size(24.0)
                    .color(palette::TEXT_DIM),
            );

            ui.add_space(32.0);
            let btn = egui::Button::new(
                egui::RichText::new("Exit to Desktop")
                    .size(28.0)
                    .color(palette::TEXT),
            )
            .fill(palette::DANGER)
            .rounding(egui::Rounding::same(16.0))
            .min_size(egui::vec2(260.0, 60.0));
            if ui.add(btn).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

/// PLAN 4.15.10 — RPCS3 died unexpectedly. Full-bleed starfield with
/// "SOMETHING WENT WRONG" + the watchdog's message + a gold RESTART
/// button that flips the screen back to Main.
///
/// The cloud vortex overlay lands in 4.15.5; for now we render against
/// the plain starfield fill set by `palette::apply`. RESTART currently
/// just dismisses the screen — a real RPCS3 respawn needs the
/// last-booted-game memory that we don't persist yet (see /api/launch's
/// phone-driven-serial contract). Getting the user back to the startup
/// screen is enough for MVP; auto-respawn is TODO.
fn render_crashed(
    ui: &mut egui::Ui,
    status: &Arc<std::sync::Mutex<LauncherStatus>>,
    message: &str,
) {
    ui.vertical_centered(|ui| {
        // Vertical centering on a full-screen panel: use a chunk of the
        // available height as top padding so the block sits roughly
        // between 1/4 and 3/4 of the viewport height.
        ui.add_space(ui.available_height() * 0.18);
        ui.heading(
            egui::RichText::new("SOMETHING WENT WRONG")
                .size(80.0)
                .color(palette::GOLD)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        );
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new(message)
                .size(26.0)
                .italics()
                .color(palette::TEXT_DIM),
        );
        ui.add_space(56.0);

        let btn = egui::Button::new(
            egui::RichText::new("RESTART")
                .size(40.0)
                .color(palette::GOLD_INK)
                .family(egui::FontFamily::Name(fonts::TITAN_ONE.into())),
        )
        .fill(palette::GOLD)
        .stroke(egui::Stroke::new(2.0, palette::GOLD_SHADOW))
        .rounding(egui::Rounding::same(20.0))
        .min_size(egui::vec2(320.0, 96.0));
        if ui.add(btn).clicked() {
            // TODO(4.15.10 follow-up): respawn RPCS3 with the last
            // booted game's serial once we persist a "last game"
            // memory. Today `/api/launch` expects the phone to pick
            // the serial, so a real restart requires either (a) the
            // phone driving this button via a REST endpoint, or (b)
            // a new server-side "last serial" cache. Flipping back to
            // Main is sufficient for MVP — the user sees the QR again
            // and can reconnect the phone + re-pick the game.
            if let Ok(mut st) = status.lock() {
                st.screen = LauncherScreen::Main;
                // The watchdog already cleared these, but be
                // defensive — the restart-while-still-running case
                // becomes possible once auto-respawn lands.
                st.rpcs3_running = false;
                st.current_game = None;
            }
        }
    });
}

/// PLAN 4.15.11 — user asked to quit the launcher. Show a short
/// farewell, count down ~3 seconds, then call
/// `ViewportCommand::Close` (same mechanism the Exit-to-Desktop button
/// uses). The cloud vortex overlay lands in 4.15.5; for now the
/// surface renders against the plain starfield fill.
fn render_farewell(ui: &mut egui::Ui, ctx: &egui::Context, started_at: &mut Option<Instant>) {
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
