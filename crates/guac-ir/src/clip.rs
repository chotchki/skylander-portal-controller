//! A clip as the spline data the disc holds (openspec AE.6, design decision
//! 8), and the one evaluator the extractor's tests and the runtime share:
//! exact at any time, nothing resampled.
//!
//! Each track drives one joint's translation, rotation and scale. A channel
//! is a constant or follows a clamped B-spline (degree 1-3) over frame
//! numbers, its control points dequantized at extraction. A rotation spline
//! blends its control points as 4-vectors and normalizes the sum. Time
//! loops over the clip's duration, then counts frames, clamped to the last.

use serde::{Deserialize, Serialize};

use crate::model::{Skeleton, Transform};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// The clip's file name (`Eruptor_Idle.ma`).
    pub name: String,
    /// The loop's length, seconds.
    pub duration: f32,
    /// Seconds per frame: knots count frames.
    pub frame_duration: f32,
    pub frames: u32,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// A [`Skeleton`] joint.
    pub joint: u16,
    pub translation: VecTrack,
    pub rotation: RotationTrack,
    pub scale: VecTrack,
}

/// A translation or scale: the constant channels' values (0 or 1 where the
/// disc leaves a channel out) and the curve the others follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VecTrack {
    pub constant: [f32; 3],
    pub spline: Option<VecSpline>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VecSpline {
    /// Which of x, y and z follow the curve.
    pub channels: [bool; 3],
    pub degree: u8,
    /// Frame numbers: `degree + 1` zeros first, `degree + 1` last frames
    /// last, never decreasing.
    pub knots: Vec<u8>,
    pub points: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RotationTrack {
    /// x, y, z, w.
    Constant([f32; 4]),
    Spline {
        degree: u8,
        knots: Vec<u8>,
        points: Vec<[f32; 4]>,
    },
}

/// The non-zero B-spline basis at `u` and the first control point it
/// weights: Cox-de Boor over the knot span holding `u`.
fn basis(degree: usize, knots: &[u8], points: usize, u: f64) -> (usize, [f64; 4]) {
    let k = |i: usize| f64::from(knots[i]);
    let last = points - 1;
    // The span: knots[span] <= u < knots[span + 1], the end clamped in.
    let span = if u >= k(last + 1) {
        last
    } else {
        (degree..=last).rev().find(|&i| k(i) <= u).unwrap_or(degree)
    };
    let mut n = [0.0; 4];
    n[0] = 1.0;
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
    (span - degree, n)
}

/// The curve at `frame`, clamped to its knots.
fn curve<const N: usize>(degree: u8, knots: &[u8], points: &[[f32; N]], frame: f64) -> [f64; N] {
    let d = usize::from(degree);
    let last = knots.last().map_or(0.0, |&k| f64::from(k));
    let (first, n) = basis(d, knots, points.len(), frame.clamp(0.0, last));
    let mut out = [0.0; N];
    for (r, w) in n.iter().take(d + 1).enumerate() {
        for (o, c) in out.iter_mut().zip(points[first + r]) {
            *o += w * f64::from(c);
        }
    }
    out
}

impl VecTrack {
    pub fn value(&self, frame: f64) -> [f64; 3] {
        let mut out = self.constant.map(f64::from);
        if let Some(s) = &self.spline {
            let c = curve(s.degree, &s.knots, &s.points, frame);
            for i in 0..3 {
                if s.channels[i] {
                    out[i] = c[i];
                }
            }
        }
        out
    }
}

fn normalize(q: [f64; 4]) -> [f64; 4] {
    let n = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if n > 0.0 && n.is_finite() {
        q.map(|c| c / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

impl RotationTrack {
    pub fn value(&self, frame: f64) -> [f64; 4] {
        match self {
            Self::Constant(q) => q.map(f64::from),
            Self::Spline {
                degree,
                knots,
                points,
            } => normalize(curve(*degree, knots, points, frame)),
        }
    }
}

/// A transform in f64, the evaluator's working precision.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Qs {
    t: [f64; 3],
    r: [f64; 4],
    s: [f64; 3],
}

fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `v` turned by the unit quaternion `q`.
fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let c = [
        y * v[2] - z * v[1],
        z * v[0] - x * v[2],
        x * v[1] - y * v[0],
    ];
    let cc = [
        y * c[2] - z * c[1],
        z * c[0] - x * c[2],
        x * c[1] - y * c[0],
    ];
    std::array::from_fn(|i| v[i] + 2.0 * (w * c[i] + cc[i]))
}

impl Qs {
    fn of(t: &Transform) -> Self {
        Self {
            t: t.translation.map(f64::from),
            r: t.rotation.map(f64::from),
            s: t.scale.map(f64::from),
        }
    }

    /// `child` in this transform's space, Havok's way: t = tp + rp (sp t),
    /// r = rp r, s = sp s.
    fn then(&self, child: &Self) -> Self {
        let st = std::array::from_fn(|i| self.s[i] * child.t[i]);
        let rt = rotate(self.r, st);
        Self {
            t: std::array::from_fn(|i| self.t[i] + rt[i]),
            r: qmul(self.r, child.r),
            s: std::array::from_fn(|i| self.s[i] * child.s[i]),
        }
    }

    fn transform(&self) -> Transform {
        Transform {
            translation: self.t.map(|c| c as f32),
            rotation: self.r.map(|c| c as f32),
            scale: self.s.map(|c| c as f32),
        }
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };

    /// `child` in this transform's space, composed as Havok composes them.
    pub fn then(&self, child: &Transform) -> Transform {
        Qs::of(self).then(&Qs::of(child)).transform()
    }
}

impl Clip {
    /// The frame `t` seconds in: `t` looped over the duration, then counted
    /// in frames and clamped to the last.
    pub fn frame_at(&self, t: f64) -> f64 {
        let d = f64::from(self.duration);
        let t = if d > 0.0 { t.rem_euclid(d) } else { 0.0 };
        let f = t / f64::from(self.frame_duration);
        let last = f64::from(self.frames.saturating_sub(1));
        if f.is_nan() { 0.0 } else { f.clamp(0.0, last) }
    }

    fn track_qs(track: &Track, frame: f64) -> Qs {
        Qs {
            t: track.translation.value(frame),
            r: track.rotation.value(frame),
            s: track.scale.value(frame),
        }
    }

    /// Every track's transform at `t` seconds, looped, with its joint.
    pub fn sample(&self, t: f64) -> Vec<(u16, Transform)> {
        let frame = self.frame_at(t);
        self.tracks
            .iter()
            .map(|tr| (tr.joint, Self::track_qs(tr, frame).transform()))
            .collect()
    }

    /// `skeleton`'s model-space pose at `t` seconds, looped: tracked joints
    /// from the clip, the rest at their reference pose, each composed onto
    /// its parent's. A track naming a joint past the skeleton is skipped.
    pub fn model_pose(&self, skeleton: &Skeleton, t: f64) -> Vec<Transform> {
        let frame = self.frame_at(t);
        let mut local: Vec<Qs> = skeleton
            .joints
            .iter()
            .map(|j| Qs::of(&j.reference))
            .collect();
        for tr in &self.tracks {
            if let Some(slot) = local.get_mut(usize::from(tr.joint)) {
                *slot = Self::track_qs(tr, frame);
            }
        }
        skeleton.compose(&local).iter().map(Qs::transform).collect()
    }
}

impl Skeleton {
    /// Each joint's local transform composed onto its parent's, parents
    /// first (a parent always comes before its children).
    fn compose(&self, local: &[Qs]) -> Vec<Qs> {
        let mut out: Vec<Qs> = Vec::with_capacity(local.len());
        for (j, l) in self.joints.iter().zip(local) {
            let m = match j.parent.and_then(|p| out.get(usize::from(p))) {
                Some(parent) => parent.then(l),
                None => *l,
            };
            out.push(m);
        }
        out
    }

    /// `local` in model space: a joint whose parent isn't before it is
    /// treated as a root.
    pub fn model_pose(&self, local: &[Transform]) -> Vec<Transform> {
        let local: Vec<Qs> = local.iter().map(Qs::of).collect();
        self.compose(&local).iter().map(Qs::transform).collect()
    }

    /// Where an untracked clip leaves every joint, in model space: the bind
    /// pose Havok's skeleton holds (not the one the game skins with).
    pub fn reference_pose(&self) -> Vec<Transform> {
        let local: Vec<Transform> = self.joints.iter().map(|j| j.reference).collect();
        self.model_pose(&local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Joint;

    fn joint(name: &str, parent: Option<u16>, translation: [f32; 3]) -> Joint {
        Joint {
            name: name.into(),
            parent,
            reference: Transform {
                translation,
                ..Transform::IDENTITY
            },
            inverse_bind: [[0.0; 4]; 4],
        }
    }

    fn skeleton() -> Skeleton {
        Skeleton {
            joints: vec![
                joint("root", None, [0.0, 1.0, 0.0]),
                joint("arm", Some(0), [2.0, 0.0, 0.0]),
            ],
        }
    }

    /// The root rises 0 -> 3 over 3 frames on a line, and turns a quarter
    /// about y on the way; the arm is untracked.
    fn clip() -> Clip {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        Clip {
            name: "Test_Idle.ma".into(),
            duration: 0.1,
            frame_duration: 1.0 / 30.0,
            frames: 4,
            tracks: vec![Track {
                joint: 0,
                translation: VecTrack {
                    constant: [0.5, 0.0, 0.0],
                    spline: Some(VecSpline {
                        channels: [false, true, false],
                        degree: 1,
                        knots: vec![0, 0, 3, 3],
                        points: vec![[0.0; 3], [0.0, 3.0, 0.0]],
                    }),
                },
                rotation: RotationTrack::Spline {
                    degree: 1,
                    knots: vec![0, 0, 3, 3],
                    points: vec![[0.0, 0.0, 0.0, 1.0], [0.0, h, 0.0, h]],
                },
                scale: VecTrack {
                    constant: [1.0; 3],
                    spline: None,
                },
            }],
        }
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn a_linear_spline_hits_its_knots_and_halves_between() {
        let c = clip();
        let tr = &c.tracks[0];
        for (frame, y) in [(0.0, 0.0), (1.5, 1.5), (3.0, 3.0)] {
            let v = tr.translation.value(frame);
            assert!((v[1] - y).abs() < 1e-12, "{frame}: {v:?}");
            assert_eq!(v[0], 0.5, "a constant channel keeps its value");
        }
        // The rotation's halfway point is normalized: an eighth turn.
        let q = tr.rotation.value(1.5);
        let s = (std::f64::consts::PI / 8.0).sin();
        assert!((q[1] - s).abs() < 1e-6, "{q:?}");
        // Past the last knot is the last knot.
        assert_eq!(tr.translation.value(9.0)[1], 3.0);
    }

    #[test]
    fn a_cubic_spline_reproduces_a_cubic() {
        // Clamped cubic with control points on a line: the curve is that
        // line's parametrization, linear in the frame on uniform knots.
        let s = VecTrack {
            constant: [0.0; 3],
            spline: Some(VecSpline {
                channels: [true; 3],
                degree: 3,
                knots: vec![0, 0, 0, 0, 6, 6, 6, 6],
                points: vec![[0.0; 3], [2.0; 3], [4.0; 3], [6.0; 3]],
            }),
        };
        for frame in [0.0, 1.0, 2.5, 6.0] {
            assert!((s.value(frame)[0] - frame).abs() < 1e-12, "{frame}");
        }
    }

    #[test]
    fn time_loops_over_the_duration() {
        let c = clip();
        assert_eq!(c.frame_at(0.0), 0.0);
        assert!((c.frame_at(0.05) - 1.5).abs() < 1e-6);
        // The duration wraps to 0; -0.05 s is 0.05 s.
        assert_eq!(c.frame_at(f64::from(c.duration)), 0.0);
        assert!((c.frame_at(-0.05) - 1.5).abs() < 1e-6);
        assert!((c.frame_at(1.05) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn the_pose_composes_onto_parents_and_keeps_untracked_joints() {
        let c = clip();
        let sk = skeleton();
        let p = c.model_pose(&sk, 0.1 * 1.5 / 3.0);
        // Root at y 1.5 (its x from the constant), turned an eighth.
        assert!(close(p[0].translation, [0.5, 1.5, 0.0]), "{p:?}");
        // The arm keeps its reference 2 along x, turned with the root.
        let a = std::f32::consts::FRAC_PI_4;
        let want = [0.5 + 2.0 * a.cos(), 1.5, -2.0 * a.sin()];
        assert!(close(p[1].translation, want), "{p:?}");
        assert_eq!(p[1].rotation, p[0].rotation);
        // Untracked, the arm sits at its reference offset from the root's.
        let r = sk.reference_pose();
        assert!(close(r[1].translation, [2.0, 1.0, 0.0]));
        // A track past the skeleton is skipped, not a panic.
        let mut stray = clip();
        stray.tracks[0].joint = 9;
        assert_eq!(stray.model_pose(&sk, 0.0), r);
    }

    #[test]
    fn scale_scales_the_childs_offset() {
        let parent = Transform {
            scale: [2.0, 1.0, 1.0],
            ..Transform::IDENTITY
        };
        let child = Transform {
            translation: [1.0, 1.0, 0.0],
            ..Transform::IDENTITY
        };
        assert_eq!(parent.then(&child).translation, [2.0, 1.0, 0.0]);
    }
}
