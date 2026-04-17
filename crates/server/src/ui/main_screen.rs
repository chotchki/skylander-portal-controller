//! Main launcher surface — QR + heading + status strip (PLAN 4.15.1–.4).
//!
//! This is the default screen the launcher boots into: a starfield backdrop
//! (provided by [`crate::vortex`]), a gold-bezeled QR for the phone pairing
//! URL, a `SKYLANDER PORTAL` heading in Titan One, and a status strip with
//! the RPCS3 connection dot + current-game label.
//!
//! Future 4.15 items that enrich this screen (QR card-flip on max players,
//! player-orbit indicators) slot in here as additional helpers or separate
//! overlays; the top-level dispatcher in [`super`] doesn't need to know.

use std::sync::atomic::Ordering;

use super::LauncherApp;
use crate::state::LauncherStatus;
use crate::{fonts, palette};

impl LauncherApp {
    /// Render the Main surface. Called from the top-level dispatcher in
    /// [`super`] when `LauncherStatus::screen == LauncherScreen::Main`.
    pub(super) fn render_main(
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

/// Rasterise the pairing URL into a QR texture. Called once from
/// [`LauncherApp::new`]; the texture is cached on the app for the life of
/// the viewport.
pub(super) fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
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
