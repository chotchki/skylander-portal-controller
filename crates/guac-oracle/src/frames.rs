//! Frames compared with FLIP (openspec AF, design decision 9): a candidate's
//! mean FLIP error against the oracle's frames at a checkpoint has to be at or
//! below the largest the replays score against each other there, plus a floor.
//! AF.12 sets the floor from AF.11's measurement.
//!
//! FLIP is NVlabs' reference, through the `nv-flip` crate: its C++ is the
//! reference's own (v1.2, the LDR path unchanged since), built by `cc`. AF.10
//! checked it against the reference's published example pair: at
//! [`reference_pixels_per_degree`] its error map is the current reference's
//! bit for bit, and [`FlipMap::mean`] gives the reference's 0.159693.
//! `nv_flip::DEFAULT_PIXELS_PER_DEGREE` is that value rounded to 67, which
//! moves the mean to 0.159717, so it's never used here.

use serde::Serialize;

use crate::MetricError;

/// An 8-bit sRGB frame, row-major from the top-left, no alpha: what an
/// oracle `SCREENSHOT` PNG or a Guacamole render decodes to.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

impl Frame {
    pub fn from_rgb8(width: u32, height: u32, rgb: Vec<u8>) -> Result<Self, MetricError> {
        let expected = pixels(width, height)? * 3;
        if rgb.len() != expected {
            return Err(MetricError::FrameData {
                width,
                height,
                channels: 3,
                expected,
                found: rgb.len(),
            });
        }
        Ok(Self { width, height, rgb })
    }

    /// Drops alpha: FLIP compares colour only, and a screenshot's alpha is
    /// 255 throughout.
    pub fn from_rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Self, MetricError> {
        let expected = pixels(width, height)? * 4;
        if rgba.len() != expected {
            return Err(MetricError::FrameData {
                width,
                height,
                channels: 4,
                expected,
                found: rgba.len(),
            });
        }
        let rgb = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect();
        Ok(Self { width, height, rgb })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Pixels in a frame FLIP can take: not empty, and its RGB bytes within the
/// u32 nv-flip sizes them with.
fn pixels(width: u32, height: u32) -> Result<usize, MetricError> {
    let bytes = u64::from(width) * u64::from(height) * 3;
    if width == 0 || height == 0 || bytes > u64::from(u32::MAX) {
        return Err(MetricError::FrameSize { width, height });
    }
    Ok(width as usize * height as usize)
}

/// The reference's default viewing condition, 0.7 m from a 0.7 m wide
/// monitor 3,840 pixels across: 67.0206 pixels per degree. FLIP's numbers
/// are comparable only at one value, and this one reproduces the reference's.
pub fn reference_pixels_per_degree() -> f32 {
    nv_flip::pixels_per_degree(0.7, 3840.0, 0.7)
}

/// Per-pixel FLIP error, each in [0, 1], row-major like the frames.
#[derive(Clone, PartialEq)]
pub struct FlipMap {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl std::fmt::Debug for FlipMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FlipMap({}x{}, mean {})",
            self.width,
            self.height,
            self.mean()
        )
    }
}

impl FlipMap {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The mean over every pixel, summed in f64 in row order as the reference
    /// has since its 2026-10-12 fix (`414653b`); its f32 sum gave 0.159691 on
    /// the example pair, this 0.159693.
    pub fn mean(&self) -> f64 {
        self.values.iter().map(|&v| f64::from(v)).sum::<f64>() / self.values.len() as f64
    }

    pub fn max(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }

    /// The mean over the pixels `mask` selects (a hero's silhouette, say);
    /// `None` when it selects none.
    pub fn mean_masked(&self, mask: &[bool]) -> Result<Option<f64>, MetricError> {
        if mask.len() != self.values.len() {
            return Err(MetricError::MaskSize {
                expected: self.values.len(),
                found: mask.len(),
            });
        }
        let (sum, count) = self
            .values
            .iter()
            .zip(mask)
            .filter(|(_, m)| **m)
            .fold((0.0f64, 0usize), |(s, n), (&v, _)| {
                (s + f64::from(v), n + 1)
            });
        Ok((count > 0).then(|| sum / count as f64))
    }
}

/// FLIP's error map of `test` against `reference`, at `pixels_per_degree`
/// ([`reference_pixels_per_degree`] unless a comparison argues another).
pub fn flip(
    reference: &Frame,
    test: &Frame,
    pixels_per_degree: f32,
) -> Result<FlipMap, MetricError> {
    if reference.size() != test.size() {
        return Err(MetricError::FrameMismatch {
            reference: reference.size(),
            test: test.size(),
        });
    }
    if !(pixels_per_degree.is_finite() && pixels_per_degree > 0.0) {
        return Err(MetricError::BadPixelsPerDegree(pixels_per_degree));
    }
    let (width, height) = reference.size();
    let r = nv_flip::FlipImageRgb8::with_data(width, height, &reference.rgb);
    let t = nv_flip::FlipImageRgb8::with_data(width, height, &test.rgb);
    let map = nv_flip::flip(r, t, pixels_per_degree);
    Ok(FlipMap {
        width,
        height,
        values: map.to_vec(),
    })
}

/// The mean FLIP of every pair of N replays' frames at one checkpoint, the
/// lower-indexed frame as the reference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlipMatrix {
    replays: usize,
    /// (i, j, mean) for i < j, in order.
    pairs: Vec<(usize, usize, f64)>,
}

impl FlipMatrix {
    pub fn of(frames: &[Frame], pixels_per_degree: f32) -> Result<Self, MetricError> {
        if frames.len() < 2 {
            return Err(MetricError::TooFewFrames(frames.len()));
        }
        let mut pairs = Vec::with_capacity(frames.len() * (frames.len() - 1) / 2);
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                pairs.push((
                    i,
                    j,
                    flip(&frames[i], &frames[j], pixels_per_degree)?.mean(),
                ));
            }
        }
        Ok(Self {
            replays: frames.len(),
            pairs,
        })
    }

    pub fn replays(&self) -> usize {
        self.replays
    }

    pub fn pairs(&self) -> &[(usize, usize, f64)] {
        &self.pairs
    }

    /// The pair's mean, either way round; `None` for i == j or out of range.
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        let (a, b) = (i.min(j), i.max(j));
        self.pairs
            .iter()
            .find(|&&(x, y, _)| (x, y) == (a, b))
            .map(|&(_, _, m)| m)
    }

    /// Replay `i`'s means against every other replay, in replay order.
    pub fn against(&self, i: usize) -> Vec<f64> {
        (0..self.replays)
            .filter(|&j| j != i)
            .filter_map(|j| self.get(i, j))
            .collect()
    }

    /// The band a candidate that isn't one of the replays is judged against.
    pub fn band(&self) -> FrameBand {
        FrameBand::over(self.replays, self.pairs.iter().map(|p| p.2))
    }

    /// The band over the pairs that don't involve `left_out`: the envelope
    /// of the other replays, for leave-one-out. Needs three replays for a
    /// pair to remain.
    pub fn band_without(&self, left_out: usize) -> Result<FrameBand, MetricError> {
        if self.replays < 3 {
            return Err(MetricError::TooFewFrames(self.replays.saturating_sub(1)));
        }
        Ok(FrameBand::over(
            self.replays - 1,
            self.pairs
                .iter()
                .filter(|&&(i, j, _)| i != left_out && j != left_out)
                .map(|p| p.2),
        ))
    }
}

/// The largest mean FLIP among N replays' frames at one checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FrameBand {
    pub replays: usize,
    pub replay_max: f64,
}

impl FrameBand {
    fn over(replays: usize, means: impl Iterator<Item = f64>) -> Self {
        Self {
            replays,
            replay_max: means.fold(0.0, f64::max),
        }
    }

    /// `candidate_means` are the candidate's mean FLIP against each replay's
    /// frame; the largest counts, so every pair the candidate makes has to
    /// look like a pair the replays make, plus the floor.
    pub fn check(&self, candidate_means: &[f64], floor: f64) -> Result<FrameVerdict, MetricError> {
        crate::trace::check_tolerance("the frame floor", floor)?;
        if candidate_means.is_empty() {
            return Err(MetricError::NoReplays);
        }
        let worst = candidate_means.iter().copied().fold(0.0, f64::max);
        Ok(FrameVerdict {
            replay_max: self.replay_max,
            limit: self.replay_max + floor,
            candidate_means: candidate_means.to_vec(),
            worst,
        })
    }
}

/// Passed when the candidate's worst mean is at or below `limit`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FrameVerdict {
    pub replay_max: f64,
    /// `replay_max` plus the floor.
    pub limit: f64,
    pub candidate_means: Vec<f64>,
    pub worst: f64,
}

impl FrameVerdict {
    pub fn passed(&self) -> bool {
        self.worst <= self.limit
    }
}

/// The whole frame comparison at one checkpoint: the replays' matrix, then
/// the candidate against each replay's frame (the replay as FLIP's reference).
pub fn compare_frame(
    candidate: &Frame,
    replays: &[Frame],
    floor: f64,
    pixels_per_degree: f32,
) -> Result<FrameVerdict, MetricError> {
    let band = FlipMatrix::of(replays, pixels_per_degree)?.band();
    let means = replays
        .iter()
        .map(|r| flip(r, candidate, pixels_per_degree).map(|m| m.mean()))
        .collect::<Result<Vec<_>, _>>()?;
    band.check(&means, floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 64;
    const H: u32 = 48;

    /// A textured synthetic frame: gradients plus a stripe pattern, with
    /// `noise` levels of xorshift noise on every channel (seeded by `seed`).
    fn frame(seed: u32, noise: u8) -> Frame {
        let mut state = seed.wrapping_mul(2_654_435_761) | 1;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut rgb = Vec::with_capacity((W * H * 3) as usize);
        for y in 0..H {
            for x in 0..W {
                let base = [
                    (x * 4) as u8,
                    (y * 5) as u8,
                    if (x / 4 + y / 4) % 2 == 0 { 200 } else { 40 },
                ];
                for b in base {
                    let jitter = if noise == 0 {
                        0
                    } else {
                        (next() % (2 * u32::from(noise) + 1)) as i32 - i32::from(noise)
                    };
                    rgb.push((i32::from(b) + jitter).clamp(0, 255) as u8);
                }
            }
        }
        Frame::from_rgb8(W, H, rgb).unwrap()
    }

    #[test]
    fn flip_of_a_frame_against_itself_is_zero() {
        for f in [frame(1, 0), frame(7, 3)] {
            for ppd in [reference_pixels_per_degree(), 30.0, 120.0] {
                let map = flip(&f, &f.clone(), ppd).unwrap();
                assert_eq!((map.width(), map.height()), (W, H));
                assert_eq!(map.values().len(), (W * H) as usize);
                assert!(map.values().iter().all(|&v| v == 0.0));
                assert_eq!(map.mean(), 0.0);
                assert_eq!(map.max(), 0.0);
            }
        }
    }

    #[test]
    fn flip_grows_with_the_difference() {
        let base = frame(1, 0);
        let slight = flip(&base, &frame(2, 2), reference_pixels_per_degree()).unwrap();
        let heavy = flip(&base, &frame(3, 40), reference_pixels_per_degree()).unwrap();
        assert!(slight.mean() > 0.0, "{slight:?}");
        assert!(heavy.mean() > 2.0 * slight.mean(), "{slight:?} {heavy:?}");
        assert!(heavy.values().iter().all(|v| (0.0..=1.0).contains(v)));
        let sum: f64 = heavy.values().iter().map(|&v| f64::from(v)).sum();
        assert_eq!(heavy.mean(), sum / f64::from(W * H));
        assert!(f64::from(heavy.max()) >= heavy.mean());
    }

    #[test]
    fn a_mask_picks_the_pixels_the_mean_covers() {
        let map = flip(&frame(1, 0), &frame(3, 40), reference_pixels_per_degree()).unwrap();
        let n = (W * H) as usize;
        assert_eq!(map.mean_masked(&vec![true; n]).unwrap(), Some(map.mean()));
        assert_eq!(map.mean_masked(&vec![false; n]).unwrap(), None);
        // The left half alone.
        let left: Vec<bool> = (0..n).map(|i| (i as u32 % W) < W / 2).collect();
        let expected = map
            .values()
            .iter()
            .zip(&left)
            .filter(|(_, l)| **l)
            .map(|(&v, _)| f64::from(v))
            .sum::<f64>()
            / (n / 2) as f64;
        assert_eq!(map.mean_masked(&left).unwrap(), Some(expected));
        assert_eq!(
            map.mean_masked(&[true; 3]),
            Err(MetricError::MaskSize {
                expected: n,
                found: 3
            })
        );
    }

    #[test]
    fn frames_and_flip_refuse_what_doesnt_fit() {
        assert!(matches!(
            Frame::from_rgb8(4, 4, vec![0; 47]),
            Err(MetricError::FrameData {
                expected: 48,
                found: 47,
                ..
            })
        ));
        assert_eq!(
            Frame::from_rgb8(0, 4, vec![]),
            Err(MetricError::FrameSize {
                width: 0,
                height: 4
            })
        );
        assert_eq!(
            Frame::from_rgb8(65_536, 65_536, vec![]),
            Err(MetricError::FrameSize {
                width: 65_536,
                height: 65_536
            })
        );
        let rgba: Vec<u8> = frame(5, 3)
            .rgb()
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        assert_eq!(Frame::from_rgba8(W, H, &rgba).unwrap(), frame(5, 3));
        assert!(matches!(
            Frame::from_rgba8(W, H, &rgba[1..]),
            Err(MetricError::FrameData { channels: 4, .. })
        ));
        let small = Frame::from_rgb8(4, 4, vec![9; 48]).unwrap();
        assert_eq!(
            flip(&frame(1, 0), &small, 67.0),
            Err(MetricError::FrameMismatch {
                reference: (W, H),
                test: (4, 4)
            })
        );
        for ppd in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                flip(&small, &small, ppd),
                Err(MetricError::BadPixelsPerDegree(_))
            ));
        }
    }

    #[test]
    fn a_candidate_inside_the_replays_spread_passes_and_one_past_it_fails() {
        let ppd = reference_pixels_per_degree();
        // Five replays of one frame, each with its own 2-level noise.
        let replays: Vec<Frame> = (10..15).map(|s| frame(s, 2)).collect();
        let matrix = FlipMatrix::of(&replays, ppd).unwrap();
        assert_eq!(matrix.pairs().len(), 10);
        assert_eq!(matrix.get(3, 1), matrix.get(1, 3));
        assert_eq!(matrix.get(2, 2), None);
        assert_eq!(matrix.get(0, 5), None);
        assert_eq!(matrix.against(4).len(), 4);
        let band = matrix.band();
        assert!(band.replay_max > 0.0);

        // Another replay-like frame passes with no floor; heavier noise
        // fails, and its worst mean is reported against the limit.
        let like = compare_frame(&frame(20, 1), &replays, 0.0, ppd).unwrap();
        assert!(like.passed(), "{like:?}");
        assert_eq!(like.candidate_means.len(), 5);
        let unlike = compare_frame(&frame(21, 12), &replays, 0.0, ppd).unwrap();
        assert!(!unlike.passed(), "{unlike:?}");
        assert_eq!(unlike.limit, band.replay_max);
        assert!(unlike.worst > unlike.limit);
        // A floor wide enough lets it through.
        let floor = unlike.worst - band.replay_max;
        assert!(
            compare_frame(&frame(21, 12), &replays, floor, ppd)
                .unwrap()
                .passed()
        );

        // Leave-one-out: every replay against the others' band, from the
        // one matrix. A sixth, noisier "replay" fails it.
        for i in 0..5 {
            let band = matrix.band_without(i).unwrap();
            assert_eq!(band.replays, 4);
            assert!(band.replay_max <= matrix.band().replay_max);
        }
        let mut with_outlier = replays.clone();
        with_outlier.push(frame(30, 12));
        let matrix = FlipMatrix::of(&with_outlier, ppd).unwrap();
        let verdict = matrix
            .band_without(5)
            .unwrap()
            .check(&matrix.against(5), 0.0)
            .unwrap();
        assert!(!verdict.passed());
    }

    #[test]
    fn bands_refuse_too_few_replays_and_bad_floors() {
        let ppd = reference_pixels_per_degree();
        assert_eq!(
            FlipMatrix::of(&[frame(1, 0)], ppd),
            Err(MetricError::TooFewFrames(1))
        );
        let two = FlipMatrix::of(&[frame(1, 1), frame(2, 1)], ppd).unwrap();
        assert_eq!(two.band_without(0), Err(MetricError::TooFewFrames(1)));
        assert!(matches!(
            two.band().check(&[0.1], -0.5),
            Err(MetricError::BadTolerance { .. })
        ));
        assert_eq!(two.band().check(&[], 0.0), Err(MetricError::NoReplays));
    }
}
