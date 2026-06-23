//! PLAN A.5 — caption rendering for the demo reel.
//!
//! This ffmpeg build has no `drawtext` (no libfreetype), so captions are
//! rasterised here with `ab_glyph` + the project's **TitanOne** aesthetic font
//! and `overlay`-ed onto the reel by the render pass ([`crate::render`]).
//!
//! v1 style is deliberately plain + readable: **white TitanOne on a
//! semi-transparent dark box, single line**. The richer Skylanders look (gold
//! outline, multi-line wrap, themed box) is a styling refinement — chotchki's
//! call — and slots in here without touching the timing/overlay plumbing.

use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use image::{ImageFormat, Rgba, RgbaImage};

/// TitanOne — the project's title font (shared with the launcher badge). The
/// cross-crate `include_bytes!` keeps a single copy; the recorder is dev-only,
/// so the path coupling to `crates/server` is acceptable.
static TITAN_ONE: &[u8] =
    include_bytes!("../../../crates/server/assets/fonts/TitanOne-Regular.ttf");

/// Render `text` to a transparent caption PNG at `out`: white TitanOne on a
/// semi-transparent dark box (the box fills the PNG). Returns the `(w, h)` in px
/// so the render pass can position the `overlay`. Single line (v1).
pub fn render_caption_png(text: &str, font_px: f32, out: &Path) -> Result<(u32, u32)> {
    let font = FontRef::try_from_slice(TITAN_ONE).context("load TitanOne font")?;
    let scale = PxScale::from(font_px);
    let sf = font.as_scaled(scale);
    let pad = (font_px * 0.5).round() as i32; // box padding around the text

    // Lay glyphs along a baseline at y=ascent; accumulate the pen width (+kern).
    let mut pen_x = 0.0f32;
    let mut placed = Vec::new();
    let mut prev = None;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        if let Some(p) = prev {
            pen_x += sf.kern(p, gid);
        }
        placed.push(gid.with_scale_and_position(scale, point(pen_x, sf.ascent())));
        pen_x += sf.h_advance(gid);
        prev = Some(gid);
    }

    let w = (pen_x.ceil() as i32 + 2 * pad).max(2) as u32;
    let h = ((sf.ascent() - sf.descent()).ceil() as i32 + 2 * pad).max(2) as u32;

    // Dark box fills the PNG; white text alpha-composited over it.
    let mut img = RgbaImage::from_pixel(w, h, Rgba([8, 12, 28, 200]));
    for g in placed {
        if let Some(outlined) = font.outline_glyph(g) {
            let bb = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let x = bb.min.x as i32 + gx as i32 + pad;
                let y = bb.min.y as i32 + gy as i32 + pad;
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    let bg = img.get_pixel(x as u32, y as u32).0;
                    let a = cov.clamp(0.0, 1.0);
                    let mix = |c: u8| (255.0 * a + f32::from(c) * (1.0 - a)).round() as u8;
                    img.put_pixel(
                        x as u32,
                        y as u32,
                        Rgba([
                            mix(bg[0]),
                            mix(bg[1]),
                            mix(bg[2]),
                            bg[3].max((a * 255.0) as u8),
                        ]),
                    );
                }
            });
        }
    }

    img.save_with_format(out, ImageFormat::Png)
        .with_context(|| format!("write caption png {}", out.display()))?;
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_sample_caption_png() {
        let out = std::env::temp_dir().join("caption-sample.png");
        let (w, h) =
            render_caption_png("Welcome, Portal Master!", 56.0, &out).expect("render caption");
        // Sized to the text + padding (inspected visually at the temp path).
        assert!(w > 200, "width {w} too small");
        assert!(h > 40 && h < 200, "height {h} unexpected");
        assert!(out.exists());
    }
}
