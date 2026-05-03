//! Text → RGBA rasterisation for the 3D badge's back-face textures
//! (PLAN 10.7.6b). Each back-face state (Starting, Loading, …) has
//! its lines pre-rasterised at startup into a square RGBA buffer
//! that's uploaded as one of the `BadgeRig`'s GL textures so the
//! shader can sample it on the disc front face the same way it
//! samples the QR.
//!
//! Background is `palette::SF_1` starfield blue (matches the rest
//! of the launcher's starfield aesthetic and gives the white
//! TitanOne text the highest contrast). The disc's gold ring +
//! torus + back face still come from the shader so the back-face
//! texture's blue circle is *inside* a continuous gold frame —
//! reads as a heraldic medallion with a blue centre, not a free-
//! floating coloured disc.
//!
//! Faux-emboss (PLAN 10.7.6b second pass): each glyph is rendered
//! three times — a dark shadow shifted down-right, a bright
//! highlight shifted up-left, and the body at zero offset. The
//! offsets are sized as a fraction of the chosen px size so the
//! emboss thickness scales with the glyph (small text → 1 px
//! offsets, large text → 2-3 px offsets). Crude but works at the
//! disc's on-screen size; `10.7.6c` can swap this for a per-pixel
//! normal-map sampled in the shader if the flat look needs more
//! depth.
//!
//! Layout choices kept deliberately simple:
//!   - Auto-fits the largest line to ~75 % of the inscribed disc's
//!     width, then divides the disc's vertical real-estate evenly
//!     across `lines.len()`. Lines stack tightly (no extra leading)
//!     because the heraldic font reads best with little air —
//!     matches the visual density of the original 2D back-face
//!     `paint_titled_card` look.
//!   - Glyphs paint with linear alpha-blending over the gold base
//!     (no sub-pixel AA — the texture gets sampled at LINEAR mag in
//!     the shader, which softens edges enough at the disc's
//!     on-screen size that proper AA is overkill for a few-words-
//!     per-state title card).
//!   - Pixels outside the inscribed disc get a transparent fill,
//!     belt-and-suspenders against the front-face shader's analytic
//!     `r > OUTER_RADIUS` discard — the shader handles the cull
//!     correctly, but a transparent corner means a stray sample at
//!     the rim spokes can't pull a stray gold sliver into the
//!     vortex backdrop.

use ab_glyph::{Font, FontRef, PxScale, PxScaleFont, ScaleFont};

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/TitanOne-Regular.ttf");

/// Render `lines` centred on a `size × size` RGBA buffer with the
/// starfield-blue back-face background inside the inscribed disc
/// and transparent corners. Used at `BadgeRig` build time once per
/// back-face state.
pub fn render(lines: &[&str], size: u32) -> Vec<u8> {
    let font = FontRef::try_from_slice(FONT_BYTES).expect("TitanOne TTF parses");
    let mut buffer = vec![0u8; (size * size * 4) as usize];

    fill_inscribed_disc(&mut buffer, size);
    if !lines.is_empty() {
        draw_centred_lines(&mut buffer, size, &font, lines);
    }

    buffer
}

/// Pre-bake the back-face background colour. Pixels outside radius
/// `size/2` stay transparent (zero-alpha) — belt-and-suspenders
/// against the front-face shader's `r > OUTER_RADIUS` discard.
fn fill_inscribed_disc(buffer: &mut [u8], size: u32) {
    // `palette::SF_1` (top starfield blue, 0x0b1e52). RGBA8.
    let bg: [u8; 4] = [0x0b, 0x1e, 0x52, 0xff];
    let centre = size as f32 / 2.0;
    let radius = centre;
    let r2 = radius * radius;

    for py in 0..size {
        for px in 0..size {
            let dx = px as f32 + 0.5 - centre;
            let dy = py as f32 + 0.5 - centre;
            if dx * dx + dy * dy <= r2 {
                let idx = ((py * size + px) * 4) as usize;
                buffer[idx..idx + 4].copy_from_slice(&bg);
            }
        }
    }
}

/// Stack `lines` vertically inside the disc, sized so the longest
/// fits ~75 % of the disc's diameter and the stack fits ~70 % of
/// the disc's height. Each glyph is drawn three times for the
/// faux-emboss: a darker shadow shifted down-right, a lighter
/// highlight shifted up-left, then the body at zero offset.
fn draw_centred_lines(buffer: &mut [u8], size: u32, font: &FontRef, lines: &[&str]) {
    // Available visual envelope: the inscribed disc with some
    // padding so glyph descenders don't kiss the rim.
    let avail_w = size as f32 * 0.75;
    let avail_h = size as f32 * 0.70;

    // Pick a font size that satisfies both width-per-line and
    // total-stack-height constraints. Iterate down from a generous
    // initial guess until both fit.
    let line_count = lines.len() as f32;
    let mut px_size = (avail_h / line_count).min(avail_w / 4.0).max(8.0);

    // Width-fit pass: shrink until the widest line fits avail_w.
    loop {
        let scaled = font.as_scaled(PxScale::from(px_size));
        let widest = widest_line_px(lines, &scaled);
        if widest <= avail_w || px_size <= 8.0 {
            break;
        }
        // Shrink proportionally to the overshoot, plus a 2 %
        // safety margin for rounding.
        px_size *= (avail_w / widest) * 0.98;
    }

    let scale = PxScale::from(px_size);
    let scaled = font.as_scaled(scale);
    let line_h = scaled.height();
    let total_h = line_h * line_count;
    let centre_y = size as f32 / 2.0;
    let first_baseline = centre_y - total_h / 2.0 + scaled.ascent();

    // Faux-emboss offsets scale with the glyph size so small text
    // doesn't get muddy halos and large text doesn't look paper-
    // thin. ~2 % of the px size, clamped to [1, 4] px.
    let emboss_offset = (px_size * 0.02).clamp(1.0, 4.0);
    let shadow_color: [u8; 3] = [0, 0, 0];
    let body_color: [u8; 3] = [247, 247, 251]; // palette::TEXT (near-white)
    let highlight_color: [u8; 3] = [255, 255, 255];

    for (idx, line) in lines.iter().enumerate() {
        let baseline = first_baseline + idx as f32 * line_h;
        let line_w = widest_line_px(&[*line], &scaled);
        let line_x = (size as f32 - line_w) / 2.0;

        // Three passes per line, drawn back-to-front so the body
        // sits on top and the offset shadow / highlight peek out
        // at the bottom-right / upper-left of each glyph as if a
        // light source is hitting from the upper-left.
        draw_line_pass(
            buffer,
            size,
            font,
            &scaled,
            scale,
            line,
            line_x + emboss_offset,
            baseline + emboss_offset,
            shadow_color,
            0.85,
        );
        draw_line_pass(
            buffer,
            size,
            font,
            &scaled,
            scale,
            line,
            line_x - emboss_offset,
            baseline - emboss_offset,
            highlight_color,
            0.65,
        );
        draw_line_pass(
            buffer,
            size,
            font,
            &scaled,
            scale,
            line,
            line_x,
            baseline,
            body_color,
            1.0,
        );
    }
}

/// Rasterise one full line of glyphs onto `buffer` at the given
/// origin and colour. Coverage from `ab_glyph` is multiplied by
/// `alpha_scale` so the shadow / highlight passes can fade
/// against the body without clobbering its sharp edges.
#[allow(clippy::too_many_arguments)]
fn draw_line_pass(
    buffer: &mut [u8],
    size: u32,
    font: &FontRef,
    scaled: &PxScaleFont<&FontRef>,
    scale: PxScale,
    line: &str,
    origin_x: f32,
    baseline: f32,
    color: [u8; 3],
    alpha_scale: f32,
) {
    let mut x = origin_x;
    for ch in line.chars() {
        let glyph_id = font.glyph_id(ch);
        let advance = scaled.h_advance(glyph_id);
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= size as i32 || py >= size as i32 {
                    return;
                }
                let idx = ((py * size as i32 + px) * 4) as usize;
                let t = (coverage * alpha_scale).clamp(0.0, 1.0);
                for c in 0..3 {
                    let bg = buffer[idx + c] as f32;
                    let fg = color[c] as f32;
                    buffer[idx + c] = (bg * (1.0 - t) + fg * t) as u8;
                }
            });
        }
        x += advance;
    }
}

/// Pixel width of the longest line at the given scale. Iterates
/// glyph advances directly off the `PxScaleFont` so layout uses the
/// same metrics rasterisation does.
fn widest_line_px(lines: &[&str], scaled: &PxScaleFont<&FontRef>) -> f32 {
    lines
        .iter()
        .map(|line| {
            line.chars()
                .map(|ch| scaled.h_advance(scaled.font.glyph_id(ch)))
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max)
}
