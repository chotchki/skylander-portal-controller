//! A clip as the spline data the disc holds (openspec AE.6, design decision
//! 8), and the one evaluator the extractor's tests and the runtime share:
//! exact at any time, nothing resampled.
//!
//! Each track drives one joint's translation, rotation and scale. A channel
//! is a constant or follows a clamped B-spline (degree 1-3) over frame
//! numbers, its control points dequantized at extraction. A rotation spline
//! blends its control points as 4-vectors and normalizes the sum. Time
//! loops over the clip's duration, then counts frames, clamped to the last.
//!
//! A crossfade (AG.14, design decision 14) plays two clips at once while
//! one takes over from the other: each joint's local transform is blended
//! linearly by the incoming clip's weight, which rises from 0 to 1 over the
//! switch time ([`crossfade_weight`], [`Skeleton::crossfade`]). How the
//! original blends one is AC's open item; AG.11's recording says how close
//! this is.

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

    /// `weight` of the way from this transform to `to`: translation and
    /// scale linearly, the rotation as a 4-vector turned onto this one's
    /// hemisphere (so it takes the short way round) and normalized. The
    /// ends are the transforms themselves.
    fn blend(&self, to: &Self, weight: f64) -> Self {
        if weight <= 0.0 {
            return *self;
        }
        if weight >= 1.0 {
            return *to;
        }
        let lerp = |a: f64, b: f64| a + (b - a) * weight;
        let dot: f64 = (0..4).map(|i| self.r[i] * to.r[i]).sum();
        let r = if dot < 0.0 { to.r.map(|c| -c) } else { to.r };
        Self {
            t: std::array::from_fn(|i| lerp(self.t[i], to.t[i])),
            r: normalize(std::array::from_fn(|i| lerp(self.r[i], r[i]))),
            s: std::array::from_fn(|i| lerp(self.s[i], to.s[i])),
        }
    }
}

/// The weight of the clip being switched to, `elapsed` seconds into a
/// crossfade lasting `time`: 0 at its start, rising linearly to 1 at its
/// end, held at 0 before and 1 after. A switch of no length (or a time
/// that isn't a number) is done at once.
pub fn crossfade_weight(elapsed: f64, time: f64) -> f64 {
    if time.is_nan() || time <= 0.0 {
        return 1.0;
    }
    let w = elapsed / time;
    if w.is_nan() { 1.0 } else { w.clamp(0.0, 1.0) }
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

    /// `weight` of the way from this transform to `to`, as a crossfade
    /// blends a joint: translation and scale linearly, the rotation the
    /// short way round, normalized; 0 is this one and 1 is `to`.
    pub fn blend(&self, to: &Transform, weight: f64) -> Transform {
        Qs::of(self).blend(&Qs::of(to), weight).transform()
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

    /// Every joint's local transform at `t` seconds, looped: tracked joints
    /// from the clip, the rest at their reference. A track naming a joint
    /// past the skeleton is skipped.
    fn local_qs(&self, skeleton: &Skeleton, t: f64) -> Vec<Qs> {
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
        local
    }

    /// `skeleton`'s local pose at `t` seconds, looped: each joint relative
    /// to its parent, tracked joints from the clip and the rest at their
    /// reference.
    pub fn local_pose(&self, skeleton: &Skeleton, t: f64) -> Vec<Transform> {
        self.local_qs(skeleton, t)
            .iter()
            .map(Qs::transform)
            .collect()
    }

    /// `skeleton`'s model-space pose at `t` seconds, looped: tracked joints
    /// from the clip, the rest at their reference pose, each composed onto
    /// its parent's. A track naming a joint past the skeleton is skipped.
    pub fn model_pose(&self, skeleton: &Skeleton, t: f64) -> Vec<Transform> {
        let local = self.local_qs(skeleton, t);
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

    /// The model-space pose `weight` of the way through a crossfade from
    /// `from` (a clip and its time in seconds) to `to`: the two clips'
    /// local poses blended joint by joint ([`Transform::blend`]), then
    /// composed onto parents. Weight 0 is `from`'s pose and 1 is `to`'s,
    /// exactly; [`crossfade_weight`] gives it from the switch's progress.
    pub fn crossfade(&self, from: (&Clip, f64), to: (&Clip, f64), weight: f64) -> Vec<Transform> {
        let a = from.0.local_qs(self, from.1);
        let b = to.0.local_qs(self, to.1);
        let local: Vec<Qs> = a.iter().zip(&b).map(|(a, b)| a.blend(b, weight)).collect();
        self.compose(&local).iter().map(Qs::transform).collect()
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

    /// A second clip on [`skeleton`]: the root held at (0.5, 4.5, 0),
    /// unturned and at half scale; the arm untracked.
    fn held() -> Clip {
        Clip {
            name: "Test_Run.ma".into(),
            duration: 0.5,
            frame_duration: 1.0 / 30.0,
            frames: 16,
            tracks: vec![Track {
                joint: 0,
                translation: VecTrack {
                    constant: [0.5, 4.5, 0.0],
                    spline: None,
                },
                rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                scale: VecTrack {
                    constant: [0.5; 3],
                    spline: None,
                },
            }],
        }
    }

    fn near(a: &[Transform], b: &[Transform], eps: f32) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| {
                let d = |p: &[f32], q: &[f32]| p.iter().zip(q).all(|(u, v)| (u - v).abs() <= eps);
                d(&x.translation, &y.translation)
                    && d(&x.rotation, &y.rotation)
                    && d(&x.scale, &y.scale)
            })
    }

    #[test]
    fn the_crossfade_weight_rises_linearly_over_the_switch() {
        // The 0.25 s switch the locomotion scripts set: its start, middle and
        // end, then held either side.
        assert_eq!(crossfade_weight(0.0, 0.25), 0.0);
        assert_eq!(crossfade_weight(0.125, 0.25), 0.5);
        assert_eq!(crossfade_weight(0.0625, 0.25), 0.25);
        assert_eq!(crossfade_weight(0.25, 0.25), 1.0);
        assert_eq!(crossfade_weight(-1.0, 0.25), 0.0);
        assert_eq!(crossfade_weight(9.0, 0.25), 1.0);
        // No switch time: the new clip at once, whenever it's asked.
        assert_eq!(crossfade_weight(0.0, 0.0), 1.0);
        assert_eq!(crossfade_weight(-0.1, 0.0), 1.0);
        assert_eq!(crossfade_weight(0.1, -0.25), 1.0);
        assert_eq!(crossfade_weight(0.1, f64::NAN), 1.0);
        assert_eq!(crossfade_weight(f64::NAN, 0.25), 1.0);
    }

    #[test]
    fn a_crossfade_starts_on_one_clip_ends_on_the_other_and_halves_between() {
        let sk = skeleton();
        let (walk, run) = (clip(), held());
        // Each clip at its own time: the incoming one starts where it starts.
        let (tw, tr) = (0.05, 0.0);
        assert_eq!(
            sk.crossfade((&walk, tw), (&run, tr), 0.0),
            walk.model_pose(&sk, tw)
        );
        assert_eq!(
            sk.crossfade((&walk, tw), (&run, tr), 1.0),
            run.model_pose(&sk, tr)
        );
        // A rotation stored a hair off unit length comes through the ends as
        // stored, not renormalized.
        let mut tilted = held();
        tilted.tracks[0].rotation = RotationTrack::Constant([0.6, 0.0, 0.0, 0.800_000_1]);
        assert_eq!(
            sk.crossfade((&tilted, tr), (&walk, tw), 0.0),
            tilted.model_pose(&sk, tr)
        );
        assert_eq!(
            sk.crossfade((&walk, tw), (&tilted, tr), 1.0),
            tilted.model_pose(&sk, tr)
        );
        // Past either end the weight holds.
        assert_eq!(
            sk.crossfade((&walk, tw), (&run, tr), -0.5),
            walk.model_pose(&sk, tw)
        );
        assert_eq!(
            sk.crossfade((&walk, tw), (&run, tr), 2.0),
            run.model_pose(&sk, tr)
        );

        // Halfway: the walk's root is at y 1.5 turned an eighth about y at
        // full scale, the run's at y 4.5 unturned at half; the blend is at
        // y 3, turned a sixteenth, at three quarters.
        let p = sk.crossfade((&walk, tw), (&run, tr), 0.5);
        assert!(close(p[0].translation, [0.5, 3.0, 0.0]), "{p:?}");
        let s = (std::f64::consts::PI / 16.0).sin() as f32;
        assert!((p[0].rotation[1] - s).abs() < 1e-6, "{:?}", p[0].rotation);
        assert!(close(p[0].scale, [0.75; 3]), "{p:?}");
        // The untracked arm keeps its reference offset from the blended
        // root: 2 along x, turned and scaled with it.
        let a = std::f32::consts::FRAC_PI_8;
        let want = [0.5 + 1.5 * a.cos(), 3.0, -1.5 * a.sin()];
        assert!(close(p[1].translation, want), "{p:?}");

        // A quarter of the way, which halfway can't tell from three
        // quarters: the root a quarter of the way up, at seven eighths
        // scale, and the turn the normalized 3:1 mix of the two (not a
        // quarter of the angle: the blend is linear, then normalized).
        let p = sk.crossfade((&walk, tw), (&run, tr), 0.25);
        assert!(close(p[0].translation, [0.5, 2.25, 0.0]), "{p:?}");
        assert!(close(p[0].scale, [0.875; 3]), "{p:?}");
        let (s8, c8) = (std::f64::consts::PI / 8.0).sin_cos();
        let (y, w) = (0.75 * s8, 0.75 * c8 + 0.25);
        let want = (y / y.hypot(w)) as f32;
        assert!(
            (p[0].rotation[1] - want).abs() < 1e-6,
            "{:?}",
            p[0].rotation
        );
    }

    #[test]
    fn a_pose_crossfaded_with_itself_is_unchanged() {
        let sk = skeleton();
        let c = clip();
        for t in [0.0, 0.05, 0.08] {
            let pose = c.model_pose(&sk, t);
            for w in [0.25, 0.5, 0.75] {
                let blended = sk.crossfade((&c, t), (&c, t), w);
                assert!(near(&blended, &pose, 1e-7), "t {t}, w {w}");
            }
        }
        // A transform with itself, and with its rotation's other sign (the
        // same turn): the short way round, not through zero.
        let turned = Transform {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.6, 0.0, 0.8],
            scale: [2.0, 2.0, 2.0],
        };
        let negated = Transform {
            rotation: turned.rotation.map(|c| -c),
            ..turned
        };
        for other in [turned, negated] {
            let b = turned.blend(&other, 0.5);
            assert!(near(&[b], &[turned], 1e-7), "{b:?}");
        }
    }

    #[test]
    fn local_pose_is_the_model_pose_before_composing() {
        let sk = skeleton();
        let c = clip();
        let local = c.local_pose(&sk, 0.05);
        assert!(close(local[0].translation, [0.5, 1.5, 0.0]));
        assert_eq!(local[1], sk.joints[1].reference);
        // Composed again from its f32 rounding: the same to float precision.
        assert!(near(&sk.model_pose(&local), &c.model_pose(&sk, 0.05), 1e-6));
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
