//! PLAN A.5 — caption rendering for the demo reel.
//!
//! This ffmpeg build has no `drawtext` (no libfreetype), so captions are
//! rasterised here with `ab_glyph` + the project's **TitanOne** aesthetic font
//! and `overlay`-ed onto the reel by the render pass ([`crate::render`]).
//!
//! v1 style is deliberately plain + readable: **white TitanOne on a
//! semi-transparent dark box, greedy word-wrapped** to fit a width budget (so a
//! long caption stacks onto 2+ centred lines instead of clipping off the 1920px
//! canvas — A.7). The richer Skylanders look (gold outline, themed box) is a
//! styling refinement — chotchki's call — and slots in here without touching the
//! timing/overlay plumbing.

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
/// semi-transparent dark box (the box fills the PNG). **Greedy word-wraps** so
/// every line fits `max_width_px` — a long caption stacks onto 2+ centred lines
/// rather than clipping off the canvas (A.7). Returns `(w, h)` in px so the
/// render pass can position the `overlay`.
pub fn render_caption_png(
    text: &str,
    font_px: f32,
    max_width_px: f32,
    out: &Path,
) -> Result<(u32, u32)> {
    let font = FontRef::try_from_slice(TITAN_ONE).context("load TitanOne font")?;
    let scale = PxScale::from(font_px);
    let sf = font.as_scaled(scale);
    let pad = (font_px * 0.5).round() as i32; // box padding around the text

    // Pen advance of a whole string at this scale (+kerning) — drives both the
    // wrap decision and per-line centring.
    let advance = |s: &str| -> f32 {
        let mut w = 0.0f32;
        let mut prev = None;
        for c in s.chars() {
            let gid = font.glyph_id(c);
            if let Some(p) = prev {
                w += sf.kern(p, gid);
            }
            w += sf.h_advance(gid);
            prev = Some(gid);
        }
        w
    };

    // Greedy word-wrap to the budget. A single word wider than the budget gets
    // its own (overflowing) line rather than vanishing.
    let budget = (max_width_px - 2.0 * pad as f32).max(1.0);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if cur.is_empty() || advance(&candidate) <= budget {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }

    let line_h = (sf.ascent() - sf.descent()).ceil(); // per-line vertical step
    let text_w = lines.iter().map(|l| advance(l)).fold(0.0f32, f32::max);
    let w = (text_w.ceil() as i32 + 2 * pad).max(2) as u32;
    let h = ((line_h * lines.len() as f32).ceil() as i32 + 2 * pad).max(2) as u32;

    // Dark box fills the PNG; white text alpha-composited over it, each line
    // centred horizontally.
    let mut img = RgbaImage::from_pixel(w, h, Rgba([8, 12, 28, 200]));
    let inner_w = w as f32 - 2.0 * pad as f32;
    for (li, line) in lines.iter().enumerate() {
        let mut pen_x = pad as f32 + (inner_w - advance(line)) / 2.0;
        let baseline_y = pad as f32 + sf.ascent() + li as f32 * line_h;
        let mut prev = None;
        for c in line.chars() {
            let gid = font.glyph_id(c);
            if let Some(p) = prev {
                pen_x += sf.kern(p, gid);
            }
            let glyph = gid.with_scale_and_position(scale, point(pen_x, baseline_y));
            pen_x += sf.h_advance(gid);
            prev = Some(gid);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bb = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let x = bb.min.x as i32 + gx as i32;
                    let y = bb.min.y as i32 + gy as i32;
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
        let (w, h) = render_caption_png("Welcome, Portal Master!", 56.0, 1600.0, &out)
            .expect("render caption");
        // Sized to the text + padding (inspected visually at the temp path).
        // Fits the budget → one line.
        assert!(w > 200, "width {w} too small");
        assert!(
            h > 40 && h < 200,
            "height {h} unexpected (should be one line)"
        );
        assert!(out.exists());
    }

    /// A caption longer than the budget wraps onto extra lines (a taller box)
    /// instead of clipping — and each line stays within the budget (A.7).
    #[test]
    fn long_caption_wraps_to_multiple_lines() {
        let out = std::env::temp_dir().join("caption-wrap.png");
        let (_, h1) = render_caption_png("One", 56.0, 1600.0, &out).expect("single line");
        let (w, h) = render_caption_png(
            "Pick your archived game it boots the real emulator on the TV",
            56.0,
            600.0,
            &out,
        )
        .expect("wrapped");
        // Taller than a single line → it wrapped rather than ran off one line.
        assert!(h > h1, "expected >1 line: h={h} vs single {h1}");
        // Each line respected the ~600px budget (+ box padding).
        assert!(w <= 700, "lines should fit the 600px budget: w={w}");
    }
}
