//! Spike: can a circular noise field around a QR still scan?
//!
//! Interactive egui app so you can toggle between variants on the TV
//! and scan with your phone in real time. Adjust gap thickness,
//! module size, regenerate noise — see scan threshold instantly.
//!
//! Run with: `cargo run --example round_qr_spike`

use eframe::egui;
use image::{Rgba, RgbaImage};
use qrcode::{EcLevel, QrCode};
use rand_core::{OsRng, RngCore};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Round-QR Spike")
            .with_inner_size([1280.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Round-QR Spike",
        opts,
        Box::new(|_cc| Ok(Box::new(SpikeApp::new()))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// Plain square QR — control / sanity check.
    SquareControl,
    /// Round, noise OUTSIDE a multi-module quiet ring around the QR.
    RoundWithGap,
    /// Round, noise touches the QR data (gap=0).
    RoundNoGap,
}

struct SpikeApp {
    url: String,
    variant: Variant,
    /// Quiet-zone (no-noise) buffer between QR data and the noise
    /// ring, in QR modules. Only meaningful for `RoundWithGap`.
    gap_modules: u32,
    /// Pixel size of one QR module on the canvas. Bigger = easier to
    /// scan from across the room; smaller = tighter visual.
    module_px: u32,
    /// Cached noise field (`canvas_modules²` random bits). Stable
    /// across variant/gap changes so A/B comparisons aren't muddled
    /// by fresh randomness; regenerate via the "Reseed noise" button.
    noise_buf: Vec<u8>,
    /// Tracks whether the texture needs regenerating. Set by any
    /// control change; cleared after rebuild.
    dirty: bool,
    texture: Option<egui::TextureHandle>,
    /// Cached (n, modules) — recomputed only on URL change.
    code: Option<(u32, Vec<bool>)>,
    last_url: String,
}

impl SpikeApp {
    fn new() -> Self {
        let mut buf = vec![0u8; 16 * 1024];
        OsRng.fill_bytes(&mut buf);
        Self {
            url: "http://skylander-portal.local:8080/#k=abcdef0123456789".into(),
            variant: Variant::RoundWithGap,
            gap_modules: 4,
            module_px: 14,
            noise_buf: buf,
            dirty: true,
            texture: None,
            code: None,
            last_url: String::new(),
        }
    }

    fn ensure_code(&mut self) {
        if self.code.is_some() && self.url == self.last_url {
            return;
        }
        let code = QrCode::with_error_correction_level(self.url.as_bytes(), EcLevel::H)
            .expect("qr encode at ECC level H");
        let n = code.width() as u32;
        let modules: Vec<bool> = code
            .to_colors()
            .into_iter()
            .map(|c| matches!(c, qrcode::Color::Dark))
            .collect();
        self.code = Some((n, modules));
        self.last_url = self.url.clone();
        self.dirty = true;
    }

    fn rebuild_texture(&mut self, ctx: &egui::Context) {
        let Some((n, modules)) = self.code.clone() else {
            return;
        };
        let img = render_variant(
            n,
            &modules,
            self.variant,
            self.gap_modules,
            self.module_px,
            &self.noise_buf,
        );
        let size = [img.width() as usize, img.height() as usize];
        let pixels = img.into_raw();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let handle = ctx.load_texture("qr_spike", color_image, egui::TextureOptions::NEAREST);
        self.texture = Some(handle);
        self.dirty = false;
    }
}

impl eframe::App for SpikeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_code();

        egui::SidePanel::left("controls")
            .resizable(false)
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.heading("Round-QR Spike");
                ui.separator();

                ui.label("URL");
                if ui.text_edit_singleline(&mut self.url).changed() {
                    // ensure_code on next frame will detect the change.
                }
                ui.separator();

                ui.label("Variant");
                let mut changed = false;
                changed |= ui
                    .radio_value(
                        &mut self.variant,
                        Variant::SquareControl,
                        "A · Square (control)",
                    )
                    .changed();
                changed |= ui
                    .radio_value(&mut self.variant, Variant::RoundWithGap, "B · Round w/ gap")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.variant, Variant::RoundNoGap, "C · Round, no gap")
                    .changed();
                ui.separator();

                ui.add_enabled_ui(matches!(self.variant, Variant::RoundWithGap), |ui| {
                    ui.label("Gap (modules of clear quiet zone)");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.gap_modules, 0..=8))
                        .changed();
                });
                ui.separator();

                ui.label("Module size (px)");
                changed |= ui
                    .add(egui::Slider::new(&mut self.module_px, 6..=24))
                    .changed();
                ui.separator();

                if ui.button("Reseed noise").clicked() {
                    OsRng.fill_bytes(&mut self.noise_buf);
                    changed = true;
                }
                ui.separator();

                ui.label(egui::RichText::new(scan_hint(self.variant, self.gap_modules)).small());

                if changed {
                    self.dirty = true;
                }
            });

        if self.dirty {
            self.rebuild_texture(ctx);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_gray(20)))
            .show(ctx, |ui| {
                if let Some(tex) = &self.texture {
                    let avail = ui.available_size();
                    let tex_size = tex.size_vec2();
                    // Fit-to-panel scale, never upscale past 1:1 (noise/QR
                    // pixels look best at native resolution).
                    let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y).min(1.0);
                    let display_size = tex_size * scale;
                    ui.centered_and_justified(|ui| {
                        ui.image((tex.id(), display_size));
                    });
                }
            });
    }
}

fn scan_hint(v: Variant, gap: u32) -> &'static str {
    match (v, gap) {
        (Variant::SquareControl, _) => "Sanity check — should always scan.",
        (Variant::RoundWithGap, g) if g >= 4 => {
            "Safest round form — full standard quiet zone preserved."
        }
        (Variant::RoundWithGap, g) if g >= 2 => "Reduced quiet zone — most scanners still OK.",
        (Variant::RoundWithGap, _) => "Minimal/no quiet zone — riskier; iOS Camera may struggle.",
        (Variant::RoundNoGap, _) => "Noise touches QR data — relies on ECC H (~30%) to recover.",
    }
}

fn render_variant(
    n: u32,
    qr_modules: &[bool],
    variant: Variant,
    gap_modules: u32,
    module_px: u32,
    noise_buf: &[u8],
) -> RgbaImage {
    // Always reserve at least the standard 4-module quiet zone in the
    // canvas dimensions; whether to PAINT noise into that zone depends
    // on the variant. Plus 6 modules of breathing room around the noise
    // ring so the circle doesn't kiss the canvas edge.
    let breathing = 6_u32;
    let canvas_modules = n + 2 * (4 + breathing);
    let canvas_px = canvas_modules * module_px;

    let mut img = RgbaImage::from_pixel(canvas_px, canvas_px, Rgba([255, 255, 255, 255]));

    let qr_origin = (canvas_modules - n) / 2;
    let qr_center_px = (canvas_px / 2) as f32;
    let noise_radius_px = (canvas_px as f32 / 2.0) - (module_px as f32);

    // Reserved central square (no noise). For SquareControl this is a
    // big square that suppresses ALL noise. For RoundNoGap this is just
    // the QR bounding box. For RoundWithGap it's QR + gap_modules ring.
    let reserved_half_modules: f32 = match variant {
        Variant::SquareControl => canvas_modules as f32, // nothing painted
        Variant::RoundNoGap => n as f32 / 2.0,
        Variant::RoundWithGap => n as f32 / 2.0 + gap_modules as f32,
    };
    let reserved_half_px = reserved_half_modules * module_px as f32;

    // Pass 1: noise modules (skipped for SquareControl since reserved
    // covers the whole canvas).
    if !matches!(variant, Variant::SquareControl) {
        let mut bit_idx = 0u32;
        for my in 0..canvas_modules {
            for mx in 0..canvas_modules {
                let cx = (mx as f32 + 0.5) * module_px as f32;
                let cy = (my as f32 + 0.5) * module_px as f32;
                let dx = cx - qr_center_px;
                let dy = cy - qr_center_px;
                let dist_sq = dx * dx + dy * dy;

                if dist_sq > noise_radius_px * noise_radius_px {
                    continue;
                }
                if dx.abs() < reserved_half_px && dy.abs() < reserved_half_px {
                    continue;
                }

                let dark = bit_at(noise_buf, bit_idx);
                bit_idx = bit_idx.wrapping_add(1);
                if dark {
                    paint_module(&mut img, mx, my, [0, 0, 0], module_px);
                }
            }
        }
    }

    // Pass 2: real QR painted on top in starfield blue.
    let qr_color = [11, 30, 82];
    for y in 0..n {
        for x in 0..n {
            if qr_modules[(y * n + x) as usize] {
                paint_module(&mut img, qr_origin + x, qr_origin + y, qr_color, module_px);
            }
        }
    }

    img
}

fn bit_at(buf: &[u8], idx: u32) -> bool {
    let i = (idx as usize) % (buf.len() * 8);
    (buf[i / 8] >> (i % 8)) & 1 == 1
}

fn paint_module(img: &mut RgbaImage, mx: u32, my: u32, rgb: [u8; 3], module_px: u32) {
    let x0 = mx * module_px;
    let y0 = my * module_px;
    let (w, h) = (img.width(), img.height());
    for dy in 0..module_px {
        let py = y0 + dy;
        if py >= h {
            break;
        }
        for dx in 0..module_px {
            let px = x0 + dx;
            if px >= w {
                break;
            }
            img.put_pixel(px, py, Rgba([rgb[0], rgb[1], rgb[2], 255]));
        }
    }
}
