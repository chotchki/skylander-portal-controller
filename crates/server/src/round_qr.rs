//! Shared round-QR renderer. Produces an RGBA pixel buffer whose outer
//! corners are TRANSPARENT so the QR reads as circular on any background.
//!
//! Two consumers today:
//!   - The egui launcher (wraps the buffer in an `egui::ColorImage` and
//!     loads it as a `TextureHandle` — see `ui/main_screen.rs`).
//!   - `GET /api/join-qr.png` (encodes the buffer as PNG for the phone's
//!     menu overlay — see `http.rs`).
//!
//! The pixel-layout logic lives here once so both paths stay visually in
//! sync. The earlier revision had the launcher compose its own pixels
//! inside `render_qr_texture`; the phone could have grown a parallel
//! implementation and drifted.
//!
//! Composition, inside-out:
//!   1. A white "screen" disc inscribed in the canvas (standard
//!      QR quiet-zone colour — scanners lock on it).
//!   2. A ring of noise dots between the QR data and the disc edge, with
//!      a configurable clear quiet zone around the QR.
//!   3. The real QR modules painted on top at the centre, ECC level H
//!      so the ~30% Reed-Solomon margin covers any noise encroachment.
//!   4. Pixels OUTSIDE the noise disc are left transparent — the
//!      caller's background (egui plate / CSS / PNG alpha) shows through
//!      and the inscribed-square corners read as empty, not white.

use anyhow::{Context, Result};
use qrcode::{EcLevel, QrCode};
use rand_core::{OsRng, RngCore};

/// Styling knobs. `launcher_default()` matches the TV-validated values
/// (PLAN 4.19.x); both the egui launcher and the phone endpoint use it.
#[derive(Debug, Clone)]
pub struct RoundQrConfig {
    /// Pixel size of one QR module. Bigger = easier to scan from across
    /// the room; smaller = tighter visual. 14 is the user-validated
    /// default (PLAN 4.19.6 spike).
    pub scale: u32,
    /// Modules of breathing room between the noise ring and the canvas
    /// edge. 8 keeps the QR's diagonal corners comfortably inside the
    /// circle without kissing any outer rim the consumer paints.
    pub breathing_modules: u32,
    /// Clear quiet-zone ring between the QR data and the noise, in
    /// modules. 2 is the spike-validated sweet spot: scanners lock on
    /// quickly while the composition still reads as round.
    pub gap_modules: u32,
    /// QR data dark modules (RGBA). Pure black for max contrast.
    pub qr_color: [u8; 4],
    /// Noise ring dark modules (RGBA). SF_2 (dark blue) — darker than
    /// SF_1 so the dots read at TV distance without competing with the
    /// real QR data for scanner attention.
    pub noise_color: [u8; 4],
    /// Background disc (RGBA). White = standard QR quiet-zone colour.
    pub background: [u8; 4],
}

impl RoundQrConfig {
    /// TV- and phone-validated defaults. Both surfaces share this so
    /// the heraldic look is consistent across the launcher QR, the
    /// in-game reconnect corner panel, and the phone's INVITE menu.
    pub fn launcher_default() -> Self {
        Self {
            scale: 14,
            breathing_modules: 8,
            gap_modules: 2,
            qr_color: [0x00, 0x00, 0x00, 0xff],
            noise_color: [0x06, 0x14, 0x36, 0xff], // SF_2
            background: [0xff, 0xff, 0xff, 0xff],
        }
    }
}

/// RGBA pixel buffer returned by `render`. `rgba.len() == width * height * 4`.
#[derive(Debug, Clone)]
pub struct RoundQrPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Encode `url` as a QR at ECC level H and paint the round composition
/// into an RGBA buffer. Returns an error only if the URL is too long for
/// the QR spec (which isn't possible with our ~80-byte phone URLs).
pub fn render(url: &str, cfg: &RoundQrConfig) -> Result<RoundQrPixels> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::H)
        .context("encode QR at ECC level H")?;
    let n = code.width() as u32;
    let qr_modules: Vec<bool> = code
        .to_colors()
        .into_iter()
        .map(|c| matches!(c, qrcode::Color::Dark))
        .collect();

    // Reserve the standard 4-module quiet zone plus `breathing_modules`
    // so the noise circle doesn't kiss the canvas edge.
    let canvas_modules = n + 2 * (4 + cfg.breathing_modules);
    let canvas_px = canvas_modules * cfg.scale;
    let w = canvas_px as usize;
    let h = canvas_px as usize;

    let qr_origin = (canvas_modules - n) / 2;
    let center = (canvas_px as f32) / 2.0;
    let noise_radius = center - cfg.scale as f32;
    let reserved_half = (n as f32 / 2.0 + cfg.gap_modules as f32) * cfg.scale as f32;
    let r2 = noise_radius * noise_radius;

    // One random bit per candidate noise module. Sized generously — the
    // loop skips most positions (outside the circle or inside the
    // reserved square) so unused bits are fine.
    let mut noise_buf = vec![0u8; (canvas_modules * canvas_modules / 8 + 1) as usize];
    OsRng.fill_bytes(&mut noise_buf);
    let bit_at = |idx: u32| -> bool {
        let i = (idx as usize) % (noise_buf.len() * 8);
        (noise_buf[i / 8] >> (i % 8)) & 1 == 1
    };

    // Outside the circle stays (0,0,0,0); `vec!` zero-inits.
    let mut rgba = vec![0u8; w * h * 4];
    let put = |rgba: &mut [u8], x: usize, y: usize, c: [u8; 4]| {
        let i = (y * w + x) * 4;
        rgba[i..i + 4].copy_from_slice(&c);
    };

    // Pass 1: fill the inscribed disc with `background`. Per-pixel
    // circle test so the edge is smooth at any scale.
    for y in 0..canvas_px {
        let dy = y as f32 + 0.5 - center;
        let dy2 = dy * dy;
        for x in 0..canvas_px {
            let dx = x as f32 + 0.5 - center;
            if dx * dx + dy2 <= r2 {
                put(&mut rgba, x as usize, y as usize, cfg.background);
            }
        }
    }

    // Pass 2: noise modules, clipped per-pixel to the disc. The
    // per-pixel clip (not just the per-module centre test) matters:
    // a module centre just inside the circle still has its 14×14 box
    // extend a few pixels past the boundary, producing coloured dots
    // that bleed into the transparent corners.
    let mut bit_idx = 0u32;
    for my in 0..canvas_modules {
        for mx in 0..canvas_modules {
            let cx = (mx as f32 + 0.5) * cfg.scale as f32;
            let cy = (my as f32 + 0.5) * cfg.scale as f32;
            let dx = cx - center;
            let dy = cy - center;
            if dx * dx + dy * dy > r2 {
                continue;
            }
            if dx.abs() < reserved_half && dy.abs() < reserved_half {
                continue;
            }
            let dark = bit_at(bit_idx);
            bit_idx = bit_idx.wrapping_add(1);
            if !dark {
                continue;
            }
            let x0 = (mx * cfg.scale) as usize;
            let y0 = (my * cfg.scale) as usize;
            let s = cfg.scale as usize;
            for ddy in 0..s {
                let py = y0 + ddy;
                let dpy = py as f32 + 0.5 - center;
                let dpy2 = dpy * dpy;
                for ddx in 0..s {
                    let px = x0 + ddx;
                    let dpx = px as f32 + 0.5 - center;
                    if dpx * dpx + dpy2 <= r2 {
                        put(&mut rgba, px, py, cfg.noise_color);
                    }
                }
            }
        }
    }

    // Pass 3: real QR on top — guaranteed inside the reserved square,
    // so it never overlaps the noise ring.
    for y in 0..n {
        for x in 0..n {
            if qr_modules[(y * n + x) as usize] {
                let x0 = ((qr_origin + x) * cfg.scale) as usize;
                let y0 = ((qr_origin + y) * cfg.scale) as usize;
                let s = cfg.scale as usize;
                for dy in 0..s {
                    for dx in 0..s {
                        put(&mut rgba, x0 + dx, y0 + dy, cfg.qr_color);
                    }
                }
            }
        }
    }

    Ok(RoundQrPixels {
        width: canvas_px,
        height: canvas_px,
        rgba,
    })
}

/// Encode the round QR as a PNG. Used by the `/api/join-qr.png`
/// handler; the launcher goes straight from `RoundQrPixels` to
/// `egui::ColorImage` without a PNG round-trip.
pub fn render_png(url: &str, cfg: &RoundQrConfig) -> Result<Vec<u8>> {
    let pixels = render(url, cfg)?;
    let img = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba)
        .context("construct RgbaImage from round-qr pixels")?;
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .context("encode round-qr PNG")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_buffer_has_correct_length_and_transparent_corners() {
        let px = render(
            "http://host.local:8080/#k=deadbeef",
            &RoundQrConfig::launcher_default(),
        )
        .unwrap();
        assert_eq!(
            px.rgba.len(),
            (px.width * px.height * 4) as usize,
            "rgba buffer length mismatch"
        );
        // Top-left pixel sits well outside the inscribed circle — must
        // be transparent so a CSS background / egui plate shows through.
        let tl = &px.rgba[0..4];
        assert_eq!(tl, &[0, 0, 0, 0], "corner pixel must be transparent");
    }

    #[test]
    fn png_roundtrip_decodes() {
        let bytes = render_png(
            "http://host.local:8080/#k=abc",
            &RoundQrConfig::launcher_default(),
        )
        .unwrap();
        // PNG signature (first 8 bytes).
        assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n", "missing PNG magic");
        // Decode back — catches any width/height/stride mismatch in
        // `RgbaImage::from_raw`.
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() > 0 && decoded.height() > 0);
    }
}
