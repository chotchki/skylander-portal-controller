//! A spline-compressed block as Giants writes it: mask byte 0x45 on every
//! track, little-endian, clamped B-splines over u8 frame knots.

use super::qs::{self, QsTransform, Quat};
use super::{err, unsupported};
use crate::{ErrorKind, FormatError};

/// 16-bit positions (bits 0-1), THREECOMP40 rotations (2-5), 16-bit scales.
pub const QUANTIZATION: u8 = 0x45;
pub const MAX_DEGREE: u8 = 3;
/// THREECOMP40's 40 bits.
pub const QUAT_BYTES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    Identity,
    Static,
    Spline,
}

/// A clamped B-spline over frame numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve<const N: usize> {
    pub degree: u8,
    /// Exactly `degree + 1` zeros first and `degree + 1` last frames last.
    pub knots: Vec<u8>,
    pub points: Vec<[f64; N]>,
}

impl<const N: usize> Curve<N> {
    /// The blend at `frame`, clamped to the knots; not normalized.
    pub fn evaluate(&self, frame: f64) -> [f64; N] {
        let p = usize::from(self.degree);
        let last = f64::from(self.knots[self.knots.len() - 1]);
        let u = frame.clamp(0.0, last);
        let span = find_span(p, &self.knots, self.points.len(), u);
        let n = basis(span, p, &self.knots, u);
        let mut out = [0.0; N];
        for (r, w) in n.iter().take(p + 1).enumerate() {
            let cp = self.points[span - p + r];
            for (o, c) in out.iter_mut().zip(cp) {
                *o += w * c;
            }
        }
        out
    }
}

/// The knot span holding `u` (The NURBS Book A2.1); `knots` must be clamped.
pub fn find_span(degree: usize, knots: &[u8], points: usize, u: f64) -> usize {
    let n = points - 1;
    let k = |i: usize| f64::from(knots[i]);
    if u >= k(n + 1) {
        return n;
    }
    if u <= k(degree) {
        return degree;
    }
    let (mut low, mut high) = (degree, n + 1);
    let mut mid = (low + high) / 2;
    while u < k(mid) || u >= k(mid + 1) {
        if u < k(mid) {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    mid
}

/// The non-zero basis functions at `u` (Cox-de Boor, A2.2): entry `r`
/// weights control point `span - degree + r`.
pub fn basis(span: usize, degree: usize, knots: &[u8], u: f64) -> [f64; 4] {
    let k = |i: usize| f64::from(knots[i]);
    let mut n = [1.0, 0.0, 0.0, 0.0];
    let (mut left, mut right) = ([0.0; 4], [0.0; 4]);
    for j in 1..=degree {
        left[j] = u - k(span + 1 - j);
        right[j] = k(span + j) - u;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// THREECOMP40, a 40-bit little-endian word: three 12-bit components
/// `(v - 2047) / (2047 sqrt 2)`, the dropped one's index in bits 36-37 and
/// its sign in bit 38. `None` when bit 39 is set.
pub fn threecomp40(bytes: [u8; QUAT_BYTES]) -> Option<Quat> {
    let [b0, b1, b2, b3, b4] = bytes;
    let v = u64::from_le_bytes([b0, b1, b2, b3, b4, 0, 0, 0]);
    if v >> 39 != 0 {
        return None;
    }
    let k = std::f64::consts::FRAC_1_SQRT_2 / 2047.0;
    let c = |shift: u32| (((v >> shift) & 0xFFF) as f64 - 2047.0) * k;
    let kept = [c(0), c(12), c(24)];
    let missing = ((v >> 36) & 3) as usize;
    let s = 1.0 - kept.iter().map(|x| x * x).sum::<f64>();
    let w = if s > 0.0 { s.sqrt() } else { 0.0 };
    let w = if (v >> 38) & 1 == 1 { -w } else { w };
    Some(std::array::from_fn(|i| match i.cmp(&missing) {
        std::cmp::Ordering::Less => kept[i],
        std::cmp::Ordering::Equal => w,
        std::cmp::Ordering::Greater => kept[i - 1],
    }))
}

/// A position or scale: each channel identity, static or on the shared curve.
#[derive(Debug, Clone, PartialEq)]
pub struct VecTrack {
    pub channels: [Channel; 3],
    /// Identity and static channels' values; spline channels hold 0.
    pub constant: [f64; 3],
    pub curve: Option<Curve<3>>,
}

impl VecTrack {
    pub fn value(&self, frame: f64) -> [f64; 3] {
        let blend = self.curve.as_ref().map(|c| c.evaluate(frame));
        std::array::from_fn(|c| match (self.channels[c], blend) {
            (Channel::Spline, Some(b)) => b[c],
            _ => self.constant[c],
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuatTrack {
    Identity,
    Static(Quat),
    /// Blended as 4-vectors, then normalized.
    Spline(Curve<4>),
}

impl QuatTrack {
    pub fn value(&self, frame: f64) -> Quat {
        match self {
            Self::Identity => qs::IDENTITY,
            Self::Static(q) => *q,
            Self::Spline(c) => qs::normalize(c.evaluate(frame)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransformTrack {
    /// Quantization, position, rotation and scale masks.
    pub mask: [u8; 4],
    pub position: VecTrack,
    pub rotation: QuatTrack,
    pub scale: VecTrack,
}

impl TransformTrack {
    pub fn value(&self, frame: f64) -> QsTransform {
        QsTransform {
            translation: self.position.value(frame),
            rotation: self.rotation.value(frame),
            scale: self.scale.value(frame),
        }
    }
}

/// One block's transform tracks and where their data ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub tracks: Vec<TransformTrack>,
    pub end: usize,
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    /// Absolute offset of `data[0]`, for errors.
    base: usize,
}

impl<'a> Cursor<'a> {
    fn at(&self) -> usize {
        self.base.saturating_add(self.pos)
    }

    fn take(&mut self, n: usize, field: &'static str) -> Result<&'a [u8], FormatError> {
        let out = self
            .pos
            .checked_add(n)
            .and_then(|end| self.data.get(self.pos..end))
            .ok_or_else(|| {
                let kind = ErrorKind::Truncated {
                    needed: n,
                    available: self.data.len().saturating_sub(self.pos),
                };
                err(field, self.at(), kind)
            })?;
        self.pos += n;
        Ok(out)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, FormatError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, FormatError> {
        let b = self.take(2, field)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn f32(&mut self, field: &'static str) -> Result<f32, FormatError> {
        let at = self.at();
        let b = self.take(4, field)?;
        let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if !v.is_finite() {
            return Err(unsupported(field, at, u64::from(v.to_bits())));
        }
        Ok(v)
    }

    /// Skips to a multiple of `n`; the bytes skipped must be zero.
    fn align(&mut self, n: usize) -> Result<(), FormatError> {
        let pad = self.pos.next_multiple_of(n) - self.pos;
        let at = self.at();
        let bytes = self.take(pad, "spline padding")?;
        match bytes.iter().position(|b| *b != 0) {
            Some(i) => Err(unsupported("spline padding", at + i, u64::from(bytes[i]))),
            None => Ok(()),
        }
    }

    /// `n + 1` control points: a u16 count `n`, the degree and the knots.
    fn knots(&mut self, last_frame: u8) -> Result<(u8, Vec<u8>, usize), FormatError> {
        let at = self.at();
        let n = usize::from(self.u16("control point count")?);
        let degree = self.u8("degree")?;
        if !(1..=MAX_DEGREE).contains(&degree) {
            return Err(unsupported("degree", at + 2, u64::from(degree)));
        }
        let d = usize::from(degree);
        if n < d {
            return Err(unsupported("control point count", at, n as u64));
        }
        let knots_at = self.at();
        let knots = self.take(n + d + 2, "knots")?.to_vec();
        let ends = |run: &[u8], v: u8| run.iter().all(|k| *k == v);
        let clamped = last_frame > 0
            && ends(&knots[..=d], 0)
            && knots[d + 1] > 0
            && ends(&knots[n + 1..], last_frame)
            && knots[n] < last_frame
            && knots.windows(2).all(|w| w[0] <= w[1]);
        if !clamped {
            return Err(unsupported("knots", knots_at, u64::from(last_frame)));
        }
        Ok((degree, knots, n + 1))
    }

    fn vector(&mut self, mask: u8, default: f64, last_frame: u8) -> Result<VecTrack, FormatError> {
        let at = self.at();
        let channels = [0, 1, 2].map(|c| match (mask & (0x10 << c) != 0, mask & (1 << c) != 0) {
            (true, _) => Channel::Spline,
            (false, true) => Channel::Static,
            (false, false) => Channel::Identity,
        });
        if mask & 0x88 != 0 || (0..3).any(|c| (mask >> c) & 0x11 == 0x11) {
            return Err(unsupported("vector mask", at, u64::from(mask)));
        }
        let mut constant = [0.0; 3];
        if !channels.contains(&Channel::Spline) {
            for (c, ch) in channels.iter().enumerate() {
                constant[c] = match ch {
                    Channel::Static => f64::from(self.f32("static value")?),
                    _ => default,
                };
            }
            return Ok(VecTrack {
                channels,
                constant,
                curve: None,
            });
        }
        let (degree, knots, count) = self.knots(last_frame)?;
        self.align(4)?;
        let mut range = [(0.0, 0.0); 3];
        for (c, ch) in channels.iter().enumerate() {
            match ch {
                Channel::Spline => {
                    let min = f64::from(self.f32("spline min")?);
                    range[c] = (min, f64::from(self.f32("spline max")?));
                }
                Channel::Static => constant[c] = f64::from(self.f32("static value")?),
                Channel::Identity => constant[c] = default,
            }
        }
        let splines: Vec<usize> = (0..3).filter(|c| channels[*c] == Channel::Spline).collect();
        let raw = self.take(2 * count * splines.len(), "control points")?;
        let mut words = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u16::from_le_bytes(*w));
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            let mut p = [0.0; 3];
            for &c in &splines {
                let q = f64::from(words.next().unwrap_or_default()) / 65535.0;
                p[c] = range[c].0 + (range[c].1 - range[c].0) * q;
            }
            points.push(p);
        }
        self.align(4)?;
        Ok(VecTrack {
            channels,
            constant,
            curve: Some(Curve {
                degree,
                knots,
                points,
            }),
        })
    }

    fn quats(&mut self, count: usize) -> Result<Vec<Quat>, FormatError> {
        let at = self.at();
        let raw = self.take(QUAT_BYTES * count, "rotation")?;
        let quat = |(i, b): (usize, &[u8; QUAT_BYTES])| {
            threecomp40(*b)
                .ok_or_else(|| unsupported("rotation", at + QUAT_BYTES * i + 4, u64::from(b[4])))
        };
        raw.as_chunks::<QUAT_BYTES>()
            .0
            .iter()
            .enumerate()
            .map(quat)
            .collect()
    }

    fn rotation(&mut self, mask: u8, last_frame: u8) -> Result<QuatTrack, FormatError> {
        let track = if mask & 0xF0 != 0 {
            let (degree, knots, count) = self.knots(last_frame)?;
            let points = self.quats(count)?;
            QuatTrack::Spline(Curve {
                degree,
                knots,
                points,
            })
        } else if mask & 0x0F != 0 {
            QuatTrack::Static(self.quats(1)?[0])
        } else {
            QuatTrack::Identity
        };
        self.align(4)?;
        Ok(track)
    }
}

/// Decodes one block of `tracks` transform tracks and no float tracks.
/// `base` is `data`'s absolute offset; knots end at `last_frame`.
pub fn decode(
    data: &[u8],
    base: usize,
    tracks: usize,
    last_frame: u8,
) -> Result<Block, FormatError> {
    let mut cur = Cursor { data, pos: 0, base };
    let masks = cur.take(tracks.saturating_mul(4), "masks")?;
    let mut out = Vec::with_capacity(tracks);
    for (i, m) in masks.as_chunks::<4>().0.iter().enumerate() {
        if m[0] != QUANTIZATION {
            return Err(unsupported("quantization", base + 4 * i, u64::from(m[0])));
        }
        let position = cur.vector(m[1], 0.0, last_frame)?;
        let rotation = cur.rotation(m[2], last_frame)?;
        let scale = cur.vector(m[3], 1.0, last_frame)?;
        out.push(TransformTrack {
            mask: *m,
            position,
            rotation,
            scale,
        });
    }
    Ok(Block {
        tracks: out,
        end: cur.pos,
    })
}
