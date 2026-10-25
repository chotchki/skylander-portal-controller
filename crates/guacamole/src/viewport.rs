//! `viewportinfo.default` as the camera's scripts keep it (openspec AG.28,
//! AH design decision 5's one writer): the gameplay camera is all script
//! (`CameraSystem.ai`, `CameraControl.ai`), and the native viewport takes
//! the place and turn they leave, its field of view and its far clip. The
//! TV draws from it; the hero's scripts read its facing a frame later, as
//! the original's stick reads the view of the frame before (AG.13).
//!
//! Units are the members': the eye in metres (converted at the member, as
//! every place is), the turn in degrees (facing the way it looks, tilt up
//! from level, so negative looking down), the far clip in feet.
//!
//! Hypotheses here, each with what it rests on:
//! - the picture's height for a field of view f is 2 atan(k tan(f / 2)),
//!   k fixed so 68 gives AG.13's 53.9605° (the capture's projection, at
//!   16:9); every camera level 213 defines has 68, so only that value is
//!   measured;
//! - the view converter projects onto a 1280 x 720 screen (the PS3's 720p
//!   picture) whose origin is its top-left corner, x right and y down: the
//!   container sprite's `screen width` and `screen height`, read nowhere
//!   live. Only the framing zoom reads the projection, and with one target
//!   near the picture's middle it zooms by none of it (AG.13: the eye kept
//!   camera 0's distance to 8.7e-5 m at every record but the rock slide's
//!   shake);
//! - its roll turns the picture about the way it looks, clockwise as seen
//!   from the eye for a positive roll; level 213's camera writes 0 every
//!   frame, so no roll is measured.

use bevy::math::{Quat, Vec3};
use bevy::transform::components::Transform;
use guac_motion::camera::{CameraDef, GameCamera, LookFrom, View};

use crate::script_engine::{Angles, direction};

/// The container sprite's size, pixels (a hypothesis: the module docs).
pub const SCREEN_WIDTH: f32 = 1280.0;
pub const SCREEN_HEIGHT: f32 = 720.0;

/// Every level 213 camera's field of view (`_FOV`), degrees.
pub const FOV: f32 = 68.0;

/// Who stands for the viewport: until the camera's scripts place it, and
/// again for good once they stop, the TV's own camera does (its view the
/// engine's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Engine,
    Scripts,
    /// The scripts' camera stopped in play: the TV's chain has the view
    /// from here on, and a write to the viewport goes nowhere.
    HandedBack,
}

/// The viewport's state (the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub eye: Vec3,
    pub angles: Angles,
    /// `field of view`, degrees.
    pub fov: f32,
    /// `far clip`, feet.
    pub far_clip: f32,
    /// The view converter's world position, metres.
    pub converter: Vec3,
    pub source: Source,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            eye: Vec3::ZERO,
            angles: Angles::default(),
            fov: FOV,
            far_clip: 500.0,
            converter: Vec3::ZERO,
            source: Source::Engine,
        }
    }
}

impl Viewport {
    /// Whether the scripts' writes stand for the view.
    pub fn scripted(&self) -> bool {
        self.source == Source::Scripts
    }

    /// A script wrote it: the scripts hold the view, unless it was handed
    /// back; `false` when the write goes nowhere.
    pub fn take(&mut self) -> bool {
        match self.source {
            Source::HandedBack => false,
            _ => {
                self.source = Source::Scripts;
                true
            }
        }
    }

    /// The unit vector it looks along, its tilt up from level.
    pub fn forward(&self) -> Vec3 {
        direction(self.angles.facing, self.angles.tilt)
    }

    /// The picture's height, degrees.
    pub fn fov_y(&self) -> f32 {
        fov_y(self.fov)
    }

    /// The far clip, metres.
    pub fn far(&self) -> f32 {
        self.far_clip * crate::script_engine::METRES_PER_FOOT
    }

    /// Its turn: looking along [`Self::forward`] with up as near the
    /// world's as it can be, turned about the way it looks by its roll.
    pub fn rotation(&self) -> Quat {
        let look = Transform::IDENTITY.looking_to(self.forward(), Vec3::Y);
        look.rotation * Quat::from_rotation_z(-self.angles.roll.to_radians())
    }

    /// What the TV's camera entity takes: one writer of its place and turn.
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.eye).with_rotation(self.rotation())
    }

    /// Where `p` lands on the screen, pixels from its top-left corner;
    /// `None` at or behind the eye.
    pub fn project(&self, p: Vec3) -> Option<(f32, f32)> {
        let local = self.rotation().inverse() * (p - self.eye);
        let depth = -local.z;
        if depth <= 0.0 {
            return None;
        }
        let tan_y = (self.fov_y().to_radians() * 0.5).tan();
        let tan_x = tan_y * SCREEN_WIDTH / SCREEN_HEIGHT;
        let nx = local.x / (depth * tan_x);
        let ny = local.y / (depth * tan_y);
        Some((
            (nx + 1.0) * 0.5 * SCREEN_WIDTH,
            (1.0 - ny) * 0.5 * SCREEN_HEIGHT,
        ))
    }
}

/// The picture's height, degrees, for a field of view of `fov` (the module
/// docs' hypothesis, exact at [`FOV`]).
pub fn fov_y(fov: f32) -> f32 {
    let measured = (guac_motion::camera::FOV_Y.to_radians() * 0.5).tan();
    let k = measured / (FOV.to_radians() * 0.5).tan();
    2.0 * (k * (fov.to_radians() * 0.5).tan()).atan().to_degrees()
}

/// A screen position packed as the scripts hold one: x the high half, y
/// the low, each 12.4 fixed point held to 16 bits.
pub fn pack_screen(x: f32, y: f32) -> u32 {
    let half = |v: f32| (v * 16.0).trunc().clamp(-32768.0, 32767.0) as i16 as u16;
    u32::from(half(x)) << 16 | u32::from(half(y))
}

/// The view the camera's scripts left, as the TV draws it: their eye and
/// turn, looking at the point along it nearest his feet (where the chain
/// would aim), their picture and far clip.
pub fn scripts_view(v: &Viewport, feet: Option<Vec3>) -> View {
    let forward = v.forward();
    let along = feet.map_or(10.0, |f| (f - v.eye).dot(forward)).max(1.0);
    View {
        eye: v.eye,
        aim: v.eye + forward * along,
        fov_y: v.fov_y(),
        far_clip: v.far_clip,
    }
}

/// The chain at rest where `view` stands, his feet at `feet` (openspec
/// AG.28: the view handed back from where it stood): its follow point at
/// his feet's height under the point of the view's line nearest them across
/// the ground, a camera of its own whose eye sits at the view's and whose
/// aim is that point, so its first frame draws the view unmoved and its
/// follow point then closes on his feet as the chain's does.
pub fn chain_from(view: &View, feet: Vec3) -> GameCamera {
    let foot = guac_motion::tuning::FOOT;
    let look = (view.aim - view.eye).normalize_or_zero();
    let across = look.with_y(0.0);
    let along = if across.length_squared() > 1e-8 {
        ((feet - view.eye).dot(across) / across.length_squared()).max(0.1)
    } else {
        (view.aim - view.eye).length()
    };
    let aim = view.eye + look * along;
    let follow = aim.with_y(feet.y);
    let back = view.eye - follow;
    let distance = back.length().max(1e-3);
    let def = CameraDef {
        id: u32::MAX,
        distance: distance / foot,
        pitch: (back.y / distance).clamp(-1.0, 1.0).asin().to_degrees(),
        look_from: LookFrom::Heading(guac_motion::camera::bearing(back)),
        look_at: (aim.y - feet.y) / foot,
        fov_y: view.fov_y,
        far_clip: view.far_clip,
    };
    GameCamera::resting(def, follow, feet, feet)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 68 is AG.13's 53.9605° high; a wider field is a taller picture.
    #[test]
    fn the_field_of_view_68_is_the_measured_picture() {
        assert!((fov_y(68.0) - 53.9605).abs() < 1e-4, "{}", fov_y(68.0));
        assert!(fov_y(80.0) > fov_y(68.0));
        assert!(fov_y(0.0).abs() < 1e-6);
    }

    /// What it looks at lands in the screen's middle; a point above it
    /// higher up, one to its right further right; one behind it nowhere.
    #[test]
    fn a_point_projects_where_the_view_shows_it() {
        let v = Viewport {
            eye: Vec3::new(1.0, 2.0, 3.0),
            angles: Angles {
                facing: 15.0,
                tilt: -23.36,
                roll: 0.0,
            },
            ..Viewport::default()
        };
        let ahead = v.eye + v.forward() * 10.0;
        let (x, y) = v.project(ahead).unwrap();
        assert!(
            (x - 640.0).abs() < 1e-3 && (y - 360.0).abs() < 1e-3,
            "{x} {y}"
        );
        let (_, up) = v.project(ahead + Vec3::Y).unwrap();
        assert!(up < 360.0, "{up}");
        // Right of the view, facing 15°, is the heading -75°'s way: a
        // heading grows to the left (+x at 90° is left of +z at 0°).
        let (right, _) = v.project(ahead + direction(-75.0, 0.0)).unwrap();
        assert!(right > 640.0, "{right}");
        let (left, _) = v.project(ahead + direction(105.0, 0.0)).unwrap();
        assert!(left < 640.0, "{left}");
        assert_eq!(v.project(v.eye - v.forward()), None);
        // The edge of the picture is half its height off the middle.
        let half = (v.fov_y() * 0.5).to_radians();
        let edge = Quat::from_axis_angle(v.rotation() * Vec3::X, half) * v.forward();
        let (_, top) = v.project(v.eye + edge * 10.0).unwrap();
        assert!(top.abs() < 1e-2, "{top}");
    }

    /// Its transform looks the way its turn names, not back down it, and
    /// stays upright.
    #[test]
    fn the_transform_looks_along_its_turn() {
        let v = Viewport {
            angles: Angles {
                facing: 195.0,
                tilt: -30.0,
                roll: 0.0,
            },
            ..Viewport::default()
        };
        let fwd = v.transform().forward();
        assert!((fwd.as_vec3() - direction(195.0, -30.0)).length() < 1e-5);
        assert!(v.transform().up().y > 0.0);
    }

    #[test]
    fn a_screen_position_packs_its_halves_in_sixteenths() {
        assert_eq!(pack_screen(640.0, 360.0), (640 * 16) << 16 | (360 * 16));
        assert_eq!(pack_screen(-1.0, 0.5) >> 16, u32::from((-16i16) as u16));
        assert_eq!(pack_screen(5000.0, 0.0) >> 16, 0x7FFF);
    }

    /// Handed back, the scripts don't take it again.
    #[test]
    fn handed_back_the_view_stays_the_tvs() {
        let mut v = Viewport::default();
        assert!(!v.scripted());
        assert!(v.take() && v.scripted());
        v.source = Source::HandedBack;
        assert!(!v.take() && !v.scripted());
    }
}
