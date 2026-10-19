//! The glow chain as a CPU reference (openspec AE.11): the character-shading
//! spec's extract, blur and add-back requirements on f32 images, written
//! from the spec's text, so guacamole's glow shader has a second
//! implementation to be checked against (design decision 10). The mask it
//! starts from is [`crate::shading::glow_mask`]'s.
//!
//! Every sample is a linear filter worked out here in f32 (clamped to the
//! edge texel), not left to a sampler, and nothing is clamped: the remaster
//! runs the chain on linear values at high dynamic range.

pub type Rgba = [f32; 4];

/// An RGBA image of f32 texels, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<Rgba>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Self {
        Self::from_fn(width, height, |_, _| [0.0; 4])
    }

    pub fn from_fn(width: usize, height: usize, f: impl Fn(usize, usize) -> Rgba) -> Self {
        let texels = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        Self {
            width,
            height,
            texels,
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Rgba {
        self.texels[y * self.width + x]
    }

    /// The texel at (x, y), or the nearest edge texel past the edge.
    fn clamped(&self, x: i64, y: i64) -> Rgba {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        self.get(x, y)
    }

    /// A linear filter at (x, y) in texel units, texel `i`'s centre at `i`:
    /// the four texels around it weighted by distance.
    pub fn filtered(&self, x: f32, y: f32) -> Rgba {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let row = |y: i64| {
            let (a, b) = (self.clamped(x0, y), self.clamped(x0 + 1, y));
            std::array::from_fn::<f32, 4, _>(|c| a[c] + (b[c] - a[c]) * fx)
        };
        let (top, bottom) = (row(y0), row(y0 + 1));
        std::array::from_fn(|c| top[c] + (bottom[c] - top[c]) * fy)
    }
}

/// The glow buffer for a frame `width` by `height`: 512/960 of its width
/// and half its height, rounded, at least one texel; 512x270 at the
/// original's 960x540.
pub fn buffer_size(width: usize, height: usize) -> (usize, usize) {
    let w = (width * 512 + 480) / 960;
    let h = height.div_ceil(2);
    (w.max(1), h.max(1))
}

/// Where texel `i` of an image `to` texels long samples one `from` long,
/// both spanning the frame: its centre, in the other's texel units.
fn centre(i: usize, to: usize, from: usize) -> f32 {
    (i as f32 + 0.5) * from as f32 / to as f32 - 0.5
}

/// The extract: each glow-buffer texel samples the frame's colour and the
/// mask (its red) at its own centre and multiplies the two.
pub fn extract(frame: &Image, mask: &Image, size: (usize, usize)) -> Image {
    Image::from_fn(size.0, size.1, |x, y| {
        let (fx, fy) = (
            centre(x, size.0, frame.width),
            centre(y, size.1, frame.height),
        );
        let (mx, my) = (
            centre(x, size.0, mask.width),
            centre(y, size.1, mask.height),
        );
        let c = frame.filtered(fx, fy);
        let m = mask.filtered(mx, my)[0];
        c.map(|v| v * m)
    })
}

/// The original's 15 taps (AC.13's captures), indexed from the centre.
pub const TAPS: std::ops::RangeInclusive<i32> = -7..=7;

/// Tap `k`'s weight: exp(-k²/32) over the 15 weights' sum.
pub fn tap_weight(k: i32) -> f32 {
    let w = |k: i32| (-((k * k) as f32) / 32.0).exp();
    w(k) / TAPS.map(w).sum::<f32>()
}

/// Tap `k`'s offset from the texel, in texels: 0 at the centre, else
/// 2|k| - 0.5 to its side, halfway between two texels.
pub fn tap_offset(k: i32) -> f32 {
    if k == 0 {
        0.0
    } else {
        k.signum() as f32 * (2.0 * k.abs() as f32 - 0.5)
    }
}

/// What the taps give each texel `d` from the centre (0 to 14): the
/// centre tap's weight at 0, and half tap j's at 2j - 1 and 2j.
pub fn kernel() -> [f32; 15] {
    std::array::from_fn(|d| {
        if d == 0 {
            tap_weight(0)
        } else {
            tap_weight(d.div_ceil(2) as i32) / 2.0
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Across,
    Down,
}

/// One blur pass: each texel the weighted sum of its 15 taps along `axis`.
pub fn blur(image: &Image, axis: Axis) -> Image {
    Image::from_fn(image.width, image.height, |x, y| {
        let mut sum = [0.0f32; 4];
        for k in TAPS {
            let o = tap_offset(k);
            let s = match axis {
                Axis::Across => image.filtered(x as f32 + o, y as f32),
                Axis::Down => image.filtered(x as f32, y as f32 + o),
            };
            let w = tap_weight(k);
            for c in 0..4 {
                sum[c] += s[c] * w;
            }
        }
        sum
    })
}

/// The add-back: the blurred glow, sampled at each pixel's centre, added to
/// the frame's colour twice over; alpha stays the frame's.
pub fn add_back(frame: &Image, glow: &Image) -> Image {
    Image::from_fn(frame.width, frame.height, |x, y| {
        let g = glow.filtered(
            centre(x, frame.width, glow.width),
            centre(y, frame.height, glow.height),
        );
        let f = frame.get(x, y);
        [
            f[0] + 2.0 * g[0],
            f[1] + 2.0 * g[1],
            f[2] + 2.0 * g[2],
            f[3],
        ]
    })
}

/// The whole chain on a frame and its mask, which may differ in size.
pub fn chain(frame: &Image, mask: &Image) -> Image {
    let size = buffer_size(frame.width, frame.height);
    let glow = blur(&blur(&extract(frame, mask, size), Axis::Across), Axis::Down);
    add_back(frame, &glow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn the_buffer_is_512_by_270_at_the_originals_frame() {
        assert_eq!(buffer_size(960, 540), (512, 270));
        assert_eq!(buffer_size(1920, 1080), (1024, 540));
        assert_eq!(buffer_size(3840, 2160), (2048, 1080));
        // Rounded, and never empty: an odd height's half rounds up.
        assert_eq!(buffer_size(512, 512), (273, 256));
        assert_eq!(buffer_size(333, 197), (178, 99));
        assert_eq!(buffer_size(1, 1), (1, 1));
    }

    #[test]
    fn the_weights_are_a_normalized_gaussian_over_the_tap_index() {
        let sum: f32 = TAPS.map(tap_weight).sum();
        assert!(close(sum, 1.0, 1e-6));
        assert!(close(tap_weight(0), 0.106_115_4, 1e-6));
        assert!(close(tap_weight(1), 0.102_850_57, 1e-6));
        assert!(close(tap_weight(7), 0.022_949_064, 1e-6));
        assert_eq!(tap_weight(-3), tap_weight(3));
        for k in 1..=7 {
            let ln = (tap_weight(k) / tap_weight(0)).ln();
            assert!(close(ln, -((k * k) as f32) / 32.0, 1e-5), "tap {k}");
        }
        assert_eq!(
            (1..=7).map(tap_offset).collect::<Vec<_>>(),
            [1.5, 3.5, 5.5, 7.5, 9.5, 11.5, 13.5]
        );
        assert_eq!(tap_offset(-2), -3.5);
        assert_eq!(tap_offset(0), 0.0);
        let k = kernel();
        assert!(close(k.iter().sum::<f32>() * 2.0 - k[0], 1.0, 1e-6));
        assert_eq!((k[1], k[2]), (tap_weight(1) / 2.0, tap_weight(1) / 2.0));
        assert_eq!((k[13], k[14]), (tap_weight(7) / 2.0, tap_weight(7) / 2.0));
    }

    #[test]
    fn a_linear_filter_halfway_takes_both_texels_equally_and_clamps_at_the_edge() {
        let img = Image::from_fn(4, 2, |x, y| [x as f32 + 10.0 * y as f32, 0.0, 0.0, 1.0]);
        assert_eq!(img.filtered(1.5, 0.0)[0], 1.5);
        assert_eq!(img.filtered(1.0, 0.5)[0], 6.0);
        assert_eq!(img.filtered(-3.0, 0.0)[0], 0.0);
        assert_eq!(img.filtered(3.5, 1.0)[0], 13.0);
        assert_eq!(img.filtered(0.25, 0.0)[0], 0.25);
    }

    #[test]
    fn one_bright_texel_spreads_by_exactly_the_kernel() {
        let (w, h, at) = (48, 40, (20usize, 17usize));
        let img = Image::from_fn(w, h, |x, y| {
            if (x, y) == at {
                [8.0, 4.0, 2.0, 1.0]
            } else {
                [0.0; 4]
            }
        });
        let k = kernel();
        let weight = |d: i64| k.get(d.unsigned_abs() as usize).copied().unwrap_or(0.0);
        let across = blur(&img, Axis::Across);
        for x in 0..w {
            let d = x as i64 - at.0 as i64;
            let v = across.get(x, at.1);
            assert!(close(v[0], 8.0 * weight(d), 1e-6), "across at {d}: {v:?}");
            assert_eq!(across.get(x, at.1 + 1), [0.0; 4], "the row below");
        }
        let both = blur(&across, Axis::Down);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as i64 - at.0 as i64, y as i64 - at.1 as i64);
                let want = weight(dx) * weight(dy);
                let v = both.get(x, y);
                for (c, scale) in [8.0, 4.0, 2.0, 1.0].into_iter().enumerate() {
                    assert!(close(v[c], scale * want, 1e-6), "({dx}, {dy}) channel {c}");
                }
            }
        }
        // 29 texels each way and nothing past them.
        assert!(both.get(at.0 + 14, at.1)[0] > 0.0);
        assert_eq!(both.get(at.0 + 15, at.1), [0.0; 4]);
        assert!(both.get(at.0, at.1 - 14)[0] > 0.0);
        assert_eq!(both.get(at.0, at.1 - 15), [0.0; 4]);
    }

    #[test]
    fn a_flat_image_stays_flat_through_the_blur_to_the_edges() {
        let img = Image::from_fn(9, 5, |_, _| [0.25, 0.5, 2.0, 1.0]);
        let both = blur(&blur(&img, Axis::Across), Axis::Down);
        for t in &both.texels {
            for (c, want) in [0.25, 0.5, 2.0, 1.0].into_iter().enumerate() {
                assert!(close(t[c], want, 1e-6));
            }
        }
    }

    #[test]
    fn a_zero_mask_adds_nothing() {
        let frame = Image::from_fn(96, 54, |x, y| [x as f32 * 0.1, y as f32 * 0.2, 3.0, 1.0]);
        let mask = Image::new(96, 54);
        assert_eq!(chain(&frame, &mask), frame);
    }

    #[test]
    fn the_extract_multiplies_the_frame_by_the_mask_at_each_texel_centre() {
        // 960 to 512 across samples (i + 0.5) * 1.875 - 0.5; 540 to 270
        // down takes rows 2j and 2j + 1 equally.
        let frame = Image::from_fn(960, 540, |x, y| [x as f32, y as f32, 1.0, 1.0]);
        let mask = Image::from_fn(960, 540, |_, _| [0.25, 0.0, 0.0, 1.0]);
        let g = extract(&frame, &mask, (512, 270));
        let t = g.get(3, 7);
        assert!(close(t[0], (3.5 * 1.875 - 0.5) * 0.25, 1e-5), "{t:?}");
        assert!(close(t[1], 14.5 * 0.25, 1e-5), "{t:?}");
        assert_eq!((t[2], t[3]), (0.25, 0.25));
        // The mask's red, not its other channels.
        let green = Image::from_fn(960, 540, |_, _| [0.0, 1.0, 1.0, 1.0]);
        assert!(
            extract(&frame, &green, (512, 270))
                .texels
                .iter()
                .all(|t| *t == [0.0; 4])
        );
    }

    #[test]
    fn the_glow_adds_back_twice_over_and_leaves_alpha() {
        let frame = Image::from_fn(8, 4, |_, _| [1.0, 2.0, 3.0, 0.5]);
        let glow = Image::from_fn(4, 2, |_, _| [0.25, 0.5, 1.0, 9.0]);
        let out = add_back(&frame, &glow);
        assert!(out.texels.iter().all(|t| *t == [1.5, 3.0, 5.0, 0.5]));
    }

    #[test]
    fn a_glowing_patch_brightens_round_itself_and_more_where_it_is() {
        // A 16x16 patch of mask 0.25 on a grey frame: the glow spreads past
        // the patch, fades with distance and lifts the patch by about half
        // its own colour (twice the mask) at its middle.
        let frame = Image::from_fn(192, 108, |_, _| [0.5, 0.5, 0.5, 1.0]);
        let mask = Image::from_fn(192, 108, |x, y| {
            if (88..104).contains(&x) && (46..62).contains(&y) {
                [0.25; 4]
            } else {
                [0.0; 4]
            }
        });
        let out = chain(&frame, &mask);
        let lift = |x, y| out.get(x, y)[0] - 0.5;
        assert!(lift(96, 54) > lift(104, 54) && lift(104, 54) > lift(110, 54));
        assert!(lift(110, 54) > 0.0 && lift(96, 10) == 0.0);
        assert!(lift(96, 54) < 2.0 * 0.25 * 0.5, "{}", lift(96, 54));
    }
}
