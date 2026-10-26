//! The turntable's framing and light (openspec AE.13, design decision 14):
//! what the TV's per-slot cameras need that's pure arithmetic, apart from
//! the systems that spawn them.
//!
//! Framing: the model turns about a vertical axis through the middle of
//! what its idle loop reaches across the ground (AE.35: about its own
//! origin, Spyro's tail and Wham-Shell's mace swept a cylinder twice as
//! wide as they are, and the card drew them at a quarter of its circle), so
//! everything it shows over a turn lies in the cylinder about that axis
//! holding every skinned vertex of its idle loop ([`Sweep`], sampled over
//! the clip, so an arm that swings out still counts). [`Framing::fit`]
//! backs a camera off at a fixed downward pitch until that cylinder sits
//! inside the circle a card's bezel shows, giants and small characters
//! alike.
//!
//! Light: the 3D-UI sprite rig off the disc (`global.bld`'s three
//! `_spriteLight`s, Open question 4), fixed to the camera. Its angles are
//! read the way AF.9 pinned a placement's (`tfbEulerTransform`: tilt,
//! heading, roll in degrees; heading θ faces (sin θ, 0, cos θ), a positive
//! tilt lifts it), each light travelling the way it faces, in a frame whose
//! camera looks along +Z as a heading-0 one does. That makes the key light
//! (tilt -45) shine down onto the side the camera sees and the fill come up
//! from behind. Which way the original's UI camera looks is unread; AE.16's
//! screenshot of the game's own placement screen is the check.

use bevy::camera::PerspectiveProjection;
use bevy::math::{Mat4, Vec3};
use bevy::prelude::Transform;
use guac_ir::model::Model;
use guac_ir::rig::Rig;
use guac_ir::shading::{DirectionalLight, Scene};

use super::mesh::{inverse_bindposes, pose};
use super::remaster;

/// Degrees below the horizontal the camera looks, so the top of a head and
/// the feet both read.
pub const PITCH_DEGREES: f32 = 12.0;
/// The camera's field of view, top to bottom (the image is square): narrow,
/// so the near side of a turning model doesn't swell.
pub const FOV_DEGREES: f32 = 30.0;
/// The share of the bezel's circle left clear round the model.
pub const MARGIN: f32 = 0.06;
/// Poses sampled over the idle loop for its bounds.
pub const SAMPLES: usize = 24;
/// Points on each rim of the swept cylinder that the fit checks.
const RIM_POINTS: usize = 64;
/// Fog on (so a spec-mapped part keeps its eye direction and its sheen) and
/// past anything drawn, so it adds nothing: design decision 14.
pub const FOG: (f32, f32) = (1.0e4, 2.0e4);
/// Seconds a turn takes: slow enough to read a face as it passes.
pub const TURN_SECONDS: f32 = 12.0;

/// Every vertex of `model` skinned at `seconds` into its idle, in the
/// model's own space: what Bevy's GPU skinning draws with the root at rest
/// (each joint's pose times its inverse joint, blended by the weights).
pub fn skinned(model: &Model, seconds: f64) -> Vec<Vec3> {
    let joints: Vec<Mat4> = pose(model, seconds)
        .iter()
        .zip(inverse_bindposes(&model.skeleton))
        .map(|(t, inverse)| t.to_matrix() * inverse)
        .collect();
    let mut out = Vec::with_capacity(model.vertices());
    for s in &model.segments {
        for ((p, js), ws) in s.positions.iter().zip(&s.joints).zip(&s.weights) {
            let p = Vec3::from_array(*p);
            let mut at = Vec3::ZERO;
            for (j, w) in js.iter().zip(ws) {
                if let Some(m) = joints.get(usize::from(*j)) {
                    at += m.transform_point3(p) * (f32::from(*w) / 255.0);
                }
            }
            out.push(at);
        }
    }
    out
}

/// The cylinder a turn sweeps: about the vertical axis through `axis`,
/// `radius` out, from `bottom` to `top`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    pub radius: f32,
    pub bottom: f32,
    pub top: f32,
    /// Where the turn's axis passes, in the model's space (`y` 0): the
    /// middle of the box holding every point across the ground, so a model
    /// reaching far to one side turns about its own middle (openspec AE.35).
    pub axis: Vec3,
}

impl Sweep {
    /// What `points` sweep in a turn about their middle across the ground;
    /// `None` for no points or any that isn't finite.
    pub fn of(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let points: Vec<Vec3> = points.into_iter().collect();
        let first = *points.first()?;
        if points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let (lo, hi) = points
            .iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let axis = Vec3::new(0.5 * (lo.x + hi.x), 0.0, 0.5 * (lo.z + hi.z));
        let radius = points
            .iter()
            .map(|p| (p.x - axis.x).hypot(p.z - axis.z))
            .fold(0.0, f32::max);
        Some(Self {
            radius,
            bottom: lo.y,
            top: hi.y,
            axis,
        })
    }

    /// The idle loop's sweep: `samples` poses spread over the clip, its
    /// start included, what the model carries (AE.23) with it.
    pub fn of_idle(model: &Model, samples: usize) -> Option<Self> {
        let duration = f64::from(model.idle.duration.max(0.0));
        let samples = samples.max(1);
        let times = (0..samples).map(|i| duration * i as f64 / samples as f64);
        Self::of(times.flat_map(|t| {
            let mut points = skinned(model, t);
            points.extend(super::attach::points(model, t));
            points
        }))
    }

    fn centre(&self) -> Vec3 {
        Vec3::new(0.0, 0.5 * (self.bottom + self.top), 0.0)
    }

    /// The two rims' points, pushed out so the polygon through them holds
    /// the circle.
    fn rims(&self) -> impl Iterator<Item = Vec3> + '_ {
        let out = self.radius / (std::f32::consts::PI / RIM_POINTS as f32).cos();
        (0..RIM_POINTS).flat_map(move |i| {
            let a = std::f32::consts::TAU * i as f32 / RIM_POINTS as f32;
            let (s, c) = a.sin_cos();
            [
                Vec3::new(out * c, self.bottom, out * s),
                Vec3::new(out * c, self.top, out * s),
            ]
        })
    }
}

/// A turntable camera: where it stands, what it looks at and its frustum,
/// and the axis in the model's space the stage turns the model about
/// ([`Framing::model_at`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub eye: Vec3,
    pub target: Vec3,
    pub axis: Vec3,
    /// Vertical (and horizontal) field of view, radians.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Framing {
    /// The camera that holds `sweep` inside the circle inscribed in its
    /// square image, [`MARGIN`] clear, looking down at [`PITCH_DEGREES`].
    pub fn fit(sweep: &Sweep) -> Self {
        let fov = FOV_DEGREES.to_radians();
        let pitch = PITCH_DEGREES.to_radians();
        let target = sweep.centre();
        // From the target back to the eye: in front (+Z, where the model
        // faces) and above.
        let back = Vec3::new(0.0, pitch.sin(), pitch.cos());
        let up = Vec3::new(0.0, pitch.cos(), -pitch.sin());
        let k = (0.5 * fov).tan() * (1.0 - MARGIN);
        let mut distance = f32::MIN_POSITIVE;
        let mut reach: f32 = 0.0;
        for p in sweep.rims() {
            let d = p - target;
            let off = (d.x * d.x + d.dot(up).powi(2)).sqrt();
            distance = distance.max(d.dot(back) + off / k);
            reach = reach.max(d.length());
        }
        // A model with no size still gets a camera that sees its point.
        let distance = distance.max(1e-3);
        let reach = reach.max(1e-3);
        Self {
            eye: target + back * distance,
            target,
            axis: sweep.axis,
            fov,
            near: (distance - 1.05 * reach).max(distance * 1e-3),
            far: distance + 1.05 * reach,
        }
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.eye).looking_at(self.target, Vec3::Y)
    }

    /// The model's root turned `yaw` (radians) about [`Framing::axis`],
    /// which sits on the turntable's own axis, where the camera looks.
    pub fn model_at(&self, yaw: f32) -> Transform {
        let rotation = bevy::math::Quat::from_rotation_y(yaw);
        Transform {
            translation: rotation * -self.axis,
            rotation,
            ..Transform::IDENTITY
        }
    }

    pub fn projection(&self) -> PerspectiveProjection {
        PerspectiveProjection {
            fov: self.fov,
            aspect_ratio: 1.0,
            near: self.near,
            far: self.far,
            ..PerspectiveProjection::default()
        }
    }

    /// Where `p` (in the turntable's space) lands on the image, as a share
    /// of the inscribed circle's radius from the centre: 1 on the circle,
    /// past 1 outside it, infinite behind the camera.
    pub fn reach(&self, p: Vec3) -> f32 {
        let view = self.transform().to_matrix().inverse().transform_point3(p);
        if view.z >= 0.0 {
            return f32::INFINITY;
        }
        view.truncate().length() / (-view.z * (0.5 * self.fov).tan())
    }
}

/// A rig light's travel in the turntable camera's view space (the module's
/// note says how its angles read): the way it faces at `rotation`'s tilt and
/// heading, turned from the game's camera looking along +Z into Bevy's,
/// which looks along -Z.
pub fn rig_travel(rotation: [f32; 3]) -> [f32; 3] {
    let (tilt, heading) = (rotation[0].to_radians(), rotation[1].to_radians());
    let facing = Vec3::new(
        tilt.cos() * heading.sin(),
        tilt.sin(),
        tilt.cos() * heading.cos(),
    );
    [-facing.x, facing.y, -facing.z]
}

/// Why a rig can't light the turntable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RigError {
    #[error("the rig has {0} lights, not the sprite rig's three")]
    Lights(usize),
}

/// The scene a turntable draws in: the sprite rig's three lights (their
/// colours decoded as the remaster decodes them, each with its own ambient),
/// fog on past anything drawn, no shadows and no point lights, neutral
/// placement tint, no flash. Every light's multiplier is 1 on the disc; what
/// another one would do is unread, so it isn't applied.
pub fn rig_scene(rig: &Rig) -> Result<Scene, RigError> {
    let [key, fill, back] = rig.lights.as_slice() else {
        return Err(RigError::Lights(rig.lights.len()));
    };
    let light = |l: &guac_ir::RigLight| -> DirectionalLight {
        let rgb = |c: [u8; 4]| [c[0], c[1], c[2]];
        remaster::light(
            rig_travel(l.rotation),
            rgb(l.ambient),
            rgb(l.diffuse),
            rgb(l.specular),
        )
    };
    let mut scene = Scene::new();
    scene.lights = [light(key), light(fill), light(back)];
    scene.fog = true;
    scene.fog_colour = [0.0; 3];
    (scene.fog_near, scene.fog_far) = FOG;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_ir::clip::{Clip, RotationTrack, Track, VecSpline, VecTrack};
    use guac_ir::model::{self, Joint, Segment, Skeleton};

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn the_sprite_rigs_angles_light_from_above_the_camera() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // The key (-45, 0, 0): down and away from the camera, onto the side
        // it sees.
        assert!(close(rig_travel([-45.0, 0.0, 0.0]), [0.0, -s, -s]));
        // The fill (45, 180, 0): up and toward the camera, from behind below.
        assert!(close(rig_travel([45.0, 180.0, 0.0]), [0.0, s, s]));
        // The back light (-45, 90, 0): down and to the camera's left.
        assert!(close(rig_travel([-45.0, 90.0, 0.0]), [-s, -s, 0.0]));
        // Heading 0 faces +Z (AF.9), which the camera looks along.
        assert!(close(rig_travel([0.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
        // Roll leaves the way it faces alone.
        assert!(close(rig_travel([-45.0, 0.0, 30.0]), [0.0, -s, -s]));
    }

    fn rig_light(name: &str, rgb: [u8; 3], ambient: u8, rotation: [f32; 3]) -> guac_ir::RigLight {
        guac_ir::RigLight {
            name: name.into(),
            diffuse: [rgb[0], rgb[1], rgb[2], 255],
            ambient: [ambient, ambient, ambient, 255],
            specular: [rgb[0], rgb[1], rgb[2], 255],
            multiplier: 1.0,
            position: [0.0; 3],
            rotation,
        }
    }

    /// Made-up colours, none of the disc's.
    fn rig(lights: usize) -> Rig {
        let all = [
            rig_light("_spriteLight1", [200, 100, 50], 10, [-45.0, 0.0, 0.0]),
            rig_light("_spriteLight2", [20, 40, 60], 0, [45.0, 180.0, 0.0]),
            rig_light("_spriteLight3", [30, 30, 30], 0, [-45.0, 90.0, 0.0]),
        ];
        Rig {
            identity: guac_ir::DumpIdentity {
                title_id: "TEST00000".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            lights: all[..lights].to_vec(),
        }
    }

    #[test]
    fn the_rig_lights_in_linear_light_with_fog_past_everything() {
        let scene = rig_scene(&rig(3)).unwrap();
        assert_eq!(scene.lights[0].diffuse, remaster::linear([200, 100, 50]));
        assert_eq!(scene.lights[0].ambient, remaster::linear([10, 10, 10]));
        assert_eq!(scene.lights[1].ambient, [0.0; 3]);
        assert!(close(
            scene.lights[2].travel,
            rig_travel([-45.0, 90.0, 0.0])
        ));
        assert!(scene.fog && !scene.shadows && scene.point_lights.is_empty());
        assert_eq!((scene.fog_near, scene.fog_far), FOG);
        assert_eq!(scene.placement_tint, Scene::new().placement_tint);
        assert_eq!(rig_scene(&rig(2)), Err(RigError::Lights(2)));
    }

    /// A column `height` tall and `width` across, one joint at its foot and
    /// one at its head, the head swaying sideways by `sway` over a 2 s idle:
    /// the head's vertices follow the head joint alone.
    fn column(height: f32, width: f32, sway: f32) -> Model {
        let joint = |name: &str, parent, y: f32| Joint {
            name: name.into(),
            parent,
            reference: model::Transform {
                translation: [0.0, y, 0.0],
                ..model::Transform::IDENTITY
            },
            inverse_bind: Mat4::from_translation(Vec3::new(0.0, -y, 0.0)).to_cols_array_2d(),
        };
        let skeleton = Skeleton {
            joints: vec![joint("foot", None, 0.0), joint("head", Some(0), height)],
        };
        let h = 0.5 * width;
        let positions = vec![[-h, 0.0, -h], [h, 0.0, h], [-h, height, h], [h, height, -h]];
        let joints = vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
        let segment = Segment {
            material: 0,
            source: 0,
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: None,
            uvs: vec![vec![[0.0, 0.0]; 4]],
            colors: None,
            joints,
            weights: vec![[255, 0, 0, 0]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
            positions,
        };
        // The head's translation runs 0 → sway → 0 sideways over frames 0-60.
        let head = Track {
            joint: 1,
            translation: VecTrack {
                constant: [0.0, height, 0.0],
                spline: Some(VecSpline {
                    channels: [true, false, false],
                    degree: 1,
                    knots: vec![0, 0, 30, 60, 60],
                    points: vec![[0.0; 3], [sway, 0.0, 0.0], [0.0; 3]],
                }),
            },
            rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
            scale: VecTrack {
                constant: [1.0; 3],
                spline: None,
            },
        };
        Model {
            identity: rig(0).identity,
            load_file: "000_Test".into(),
            name: "Column.mdl".into(),
            skeleton,
            materials: Vec::new(),
            segments: vec![segment],
            idle: Clip {
                name: "Column_Idle.ma".into(),
                duration: 2.0,
                frame_duration: 1.0 / 30.0,
                frames: 61,
                tracks: vec![head],
            },
            attachments: Vec::new(),
            attachments_unsettled: None,
        }
    }

    #[test]
    fn skinning_on_the_cpu_follows_the_joints() {
        let m = column(10.0, 2.0, 3.0);
        let at_rest = skinned(&m, 0.0);
        assert!(at_rest[2].abs_diff_eq(Vec3::new(-1.0, 10.0, 1.0), 1e-5));
        // Halfway, the head is 3 out and its vertices with it; the foot stays.
        let swung = skinned(&m, 1.0);
        assert!(
            swung[2].abs_diff_eq(Vec3::new(2.0, 10.0, 1.0), 1e-4),
            "{swung:?}"
        );
        assert!(swung[0].abs_diff_eq(Vec3::new(-1.0, 0.0, -1.0), 1e-5));
    }

    #[test]
    fn the_sweep_takes_the_whole_loop() {
        let m = column(10.0, 2.0, 3.0);
        // At rest the column reaches √2 out about its middle, the origin;
        // swung, its head's far corner (4, 10, ±1) moves the middle across
        // the ground to (1.5, 0), the head and the foot's far corners each
        // 2.5 and 1 off it (AE.35: √17 about the origin).
        let still = Sweep::of(skinned(&m, 0.0)).unwrap();
        assert!((still.radius - 2f32.sqrt()).abs() < 1e-5);
        assert_eq!(still.axis, Vec3::ZERO);
        let loop_ = Sweep::of_idle(&m, SAMPLES).unwrap();
        assert!((loop_.radius - 7.25f32.sqrt()).abs() < 1e-3, "{loop_:?}");
        assert!(
            loop_.axis.abs_diff_eq(Vec3::new(1.5, 0.0, 0.0), 1e-3),
            "{loop_:?}"
        );
        assert_eq!((loop_.bottom, loop_.top), (0.0, 10.0));
        assert_eq!(Sweep::of([]), None);
        assert_eq!(Sweep::of([Vec3::new(f32::NAN, 0.0, 0.0)]), None);
    }

    /// Every vertex of every sampled pose, turned to every eighth of a
    /// degree round, lands inside the circle with the margin clear; and the
    /// model fills it: something comes within a fifth of the edge.
    fn assert_framed(m: &Model) {
        let sweep = Sweep::of_idle(m, SAMPLES).unwrap();
        let f = Framing::fit(&sweep);
        let mut most: f32 = 0.0;
        for i in 0..SAMPLES {
            let t = f64::from(m.idle.duration) * i as f64 / SAMPLES as f64;
            for p in skinned(m, t) {
                for step in 0..2880 {
                    let yaw = (step as f32 / 8.0).to_radians();
                    let reach = f.reach(f.model_at(yaw).transform_point(p));
                    assert!(reach <= 1.0 - MARGIN + 1e-4, "{reach} at {t} s, {yaw} rad");
                    most = most.max(reach);
                }
            }
        }
        assert!(most > 0.8 * (1.0 - MARGIN), "only {most} of the circle");
        // Nothing of the loop is clipped by the near or far plane.
        let at = f.transform().to_matrix().inverse();
        for p in sweep.rims() {
            let depth = -at.transform_point3(p).z;
            assert!(f.near < depth && depth < f.far, "{depth} outside {f:?}");
        }
    }

    #[test]
    fn a_tall_and_a_short_model_each_stay_whole_through_a_turn() {
        // A giant: tall and narrow, its head swinging well out.
        assert_framed(&column(40.0, 4.0, 9.0));
        // A small one: short and wide, barely moving.
        assert_framed(&column(1.0, 1.6, 0.05));
    }

    /// AE.35: a model reaching far to one side (Spyro's tail, Wham-Shell's
    /// mace) turns about the middle of what it reaches across the ground,
    /// the stage's axis, so its card frames it on that middle's cylinder:
    /// a dragon 1 m wide reaching 1 m ahead and 3 m behind its feet sweeps
    /// √4.25 m about its middle where about its feet it swept √9.25 m, and
    /// it stands over 1.35 times as large on the card.
    #[test]
    fn a_model_reaching_to_one_side_turns_about_its_middle() {
        let dragon = [
            Vec3::new(-0.5, 0.0, 1.0),
            Vec3::new(0.5, 1.5, 1.0),
            Vec3::new(-0.5, 0.0, -3.0),
            Vec3::new(0.5, 0.5, -3.0),
        ];
        let sweep = Sweep::of(dragon).unwrap();
        assert!(
            sweep.axis.abs_diff_eq(Vec3::new(0.0, 0.0, -1.0), 1e-6),
            "{sweep:?}"
        );
        assert!((sweep.radius - 4.25f32.sqrt()).abs() < 1e-5, "{sweep:?}");
        let f = Framing::fit(&sweep);
        // The stage sets the axis on its own, where the camera looks: the
        // middle at the origin whatever the turn, the tail's end swung by
        // the turn about it.
        for yaw in [0.0f32, 1.0, 2.5] {
            let at = f.model_at(yaw);
            assert!(at.transform_point(sweep.axis).abs_diff_eq(Vec3::ZERO, 1e-6));
            let tail = at.transform_point(dragon[2]);
            assert!(((tail.x * tail.x + tail.z * tail.z).sqrt() - 4.25f32.sqrt()).abs() < 1e-5);
        }
        // The same dragon turned about its feet, as the stage turned every
        // model before.
        let about_feet = Framing::fit(&Sweep {
            radius: (0.25f32 + 9.0).sqrt(),
            axis: Vec3::ZERO,
            ..sweep
        });
        let head = |f: &Framing, p: Vec3| f.reach(f.model_at(0.0).transform_point(p));
        let (top, bottom) = (dragon[1], dragon[0]);
        let ours = head(&f, top) + head(&f, bottom);
        let feet = head(&about_feet, top) + head(&about_feet, bottom);
        assert!(ours > 1.35 * feet, "{ours} against {feet}");
    }

    #[test]
    fn the_camera_looks_down_at_the_middle_of_the_sweep() {
        let f = Framing::fit(&Sweep {
            radius: 1.0,
            bottom: 0.0,
            top: 4.0,
            axis: Vec3::ZERO,
        });
        assert_eq!(f.target, Vec3::new(0.0, 2.0, 0.0));
        let forward = f.transform().forward();
        let pitch = forward.y.asin().to_degrees();
        assert!((pitch + PITCH_DEGREES).abs() < 1e-3, "{pitch}");
        assert!(forward.z < 0.0, "in front of the model, looking back at it");
        assert!(f.reach(f.target) < 1e-6);
        assert_eq!(f.reach(f.eye + Vec3::Z), f32::INFINITY);
        let p = f.projection();
        assert_eq!((p.fov, p.aspect_ratio), (f.fov, 1.0));
    }
}
