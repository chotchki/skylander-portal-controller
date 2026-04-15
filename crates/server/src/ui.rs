//! Fullscreen eframe window. Shows the QR code for the phone URL, the URL
//! itself, and a live connected-client count. Phase 3 adds Manage-dialog /
//! restart-game / exit buttons.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct LauncherApp {
    clients: Arc<AtomicUsize>,
    url: String,
    figure_count: usize,
    qr_texture: Option<egui::TextureHandle>,
}

impl LauncherApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        clients: Arc<AtomicUsize>,
        figure_count: usize,
        url: String,
    ) -> Self {
        let qr_texture = Some(render_qr_texture(&cc.egui_ctx, &url));
        Self {
            clients,
            url,
            figure_count,
            qr_texture,
        }
    }
}

fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    let code = qrcode::QrCode::new(url).expect("qr encode");
    let dark = egui::Color32::from_rgb(0x0b, 0x1e, 0x3f);
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.heading(
                    egui::RichText::new("Skylander Portal")
                        .size(64.0)
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Scan to connect:")
                        .size(36.0)
                        .color(egui::Color32::from_gray(220)),
                );
                ui.add_space(24.0);
                if let Some(tex) = &self.qr_texture {
                    let size = tex.size_vec2();
                    ui.image((tex.id(), size));
                }
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(&self.url)
                        .size(32.0)
                        .monospace()
                        .color(egui::Color32::from_rgb(0xff, 0xcf, 0x3a)),
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
                ui.label(
                    egui::RichText::new(status)
                        .size(40.0)
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{} figures indexed", self.figure_count))
                        .size(24.0)
                        .color(egui::Color32::from_gray(180)),
                );

                ui.add_space(32.0);
                let btn = egui::Button::new(
                    egui::RichText::new("Exit to Desktop")
                        .size(28.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(0x8a, 0x20, 0x20))
                .rounding(egui::Rounding::same(16.0))
                .min_size(egui::vec2(260.0, 60.0));
                if ui.add(btn).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}
