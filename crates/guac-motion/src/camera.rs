//! The gameplay camera (openspec AG.17 on AG.13 and AG.40, design decision
//! 11): the original's chain of points written from gameplay-camera's
//! requirements, stepped once a game frame beside the hero.
//!
//! Per frame, after the hero's own step (so he walked against the view the
//! frame before showed, The stick held through a switch), on his pivot as it
//! stood when the frame began (AG.13: the follow point's step at record k
//! reads his pivot and the camera target at record k; AG.40: a plane's weight
//! reads his feet the record before):
//! 1. each camera in use moves its [`FollowPoint`] toward its aim point, the
//!    share [`CameraDef::aim_share`] of the way from his feet to the camera
//!    target (The camera follows: 35 % for camera 0);
//! 2. the camera target ([`LookAhead`]) moves after his walk;
//! 3. the view is the camera's own off its follow point, its eye at the
//!    definition's distance and pitch, aimed at the look-at height above it
//!    (The camera starts behind the hero), or inside a transition plane's box
//!    its two cameras' views weighted by where his feet are
//!    ([`TransitionPlane`], A transition plane blends by where he stands).
//!
//! Nothing here asks the world anything: no camera macro casts a ray and the
//! clearing's rim between the eye and him moved nothing (AG.13; Nothing
//! pushes the camera in).
//!
//! Lengths the camera scripts hold are feet; the world is metres.
//!
//! Measured, not read (AG.17's Verify, from the recordings' own pivots and
//! steps): a follow point new to the level sits on its aim for 0.5 s of game
//! time and one coming back into use jumps onto it once ([`FollowPoint::made`],
//! [`GameCamera::step`]); a plane is in use while his pivot is inside its box
//! along its facing; the follow point's speed raised past 5 ft falls back to
//! 0.4 ft once it has come to rest on its aim (AG.40: without that fall-back
//! four of six crossings miss the eye by 1.3-15 cm, with it by 1.4e-4 m).
//!
//! Hypotheses here, each with the reading or the recording behind it:
//! - the camera target's law is `Camera - Update Player Cam Target` as read
//!   (an intent point 0.9 s of his speed ahead along his last move, a
//!   velocity point closing on it by at most his speed / 42.85 a frame and
//!   held within his speed / 4.285 of the target, the target closing 10 % on
//!   it and held within 17 ft of him at his height, both caps scaled by the
//!   frame's step over 1/60 s), with his speed his pivot's move across the
//!   ground into the frame over that move's step and the velocity point at
//!   his height: from the recorded pivots it holds the recorded target to
//!   1.4-2.6 cm over AG.13's six level walks and 2.9 cm up its corridor, but
//!   to 0.42-0.59 m over AG.40's crossings, where on the walkway's slope the
//!   velocity point read back off the recorded target moves 3-4 % under the
//!   cap his ground speed gives (AG.17's box);
//! - past 5 ft the follow point's speed eases toward the scripts' "current"
//!   speed (0.2 + 1.2 x min(gap / 100 ft, 1) + 0.2 ft, or his move this
//!   frame + 0.2 ft if more) by 0.01 x the gap a frame: AG.13 and AG.40 saw
//!   only single frames past 5 ft, each raising it by 0.01 x the gap, which
//!   no candidate for "current" changes; the fall-back's 0.005 ft is as read;
//! - which follow points hold 0.5 s: AG.40's camera 1 was both new to the
//!   level and behind the plane, and camera 7 both made before the
//!   recordings (at the rock slide's switch) and in front of it, so the
//!   recordings can't tell which of the two decides it; this takes "new";
//! - the box's width and height count the same way as its length (as read:
//!   AG.40's crossings only showed the length), and leaving it by a side
//!   settles the view on the camera of the side of its centre he left on;
//! - with one target the framing zoom moves nothing (AG.13: the eye kept
//!   camera 0's distance to 8.7e-5 m at every record but the rock slide's
//!   shake); its zoom for two targets is co-op's.

use crate::seam::Vec3;
use crate::tuning::{FOOT, NOMINAL_STEP};

/// The picture's height in degrees for the cameras' `_FOV` 68, every level
/// 213 camera's (AG.13: the capture's projection, 53.9605° top to bottom,
/// 84.2934° across at 16:9).
pub const FOV_Y: f32 = 53.9605;
/// The near clip, metres (AG.13's projection).
pub const NEAR: f32 = 0.1;

/// The follow point's speed at rest, ft a frame (`cameraSpeed`): within
/// [`EASE_BAND`] it closes [`CAMERA_SPEED`] / 5 = 8 % of its gap a frame.
pub const CAMERA_SPEED: f32 = 0.4;
/// Within this many feet the follow point closes a share of its gap; past it
/// it moves its speed in feet (AG.13's 5 ft band).
pub const EASE_BAND: f32 = 5.0;
/// Past the band the speed eases by this share of the gap a frame
/// (`PanEasing`; AG.13: 0.4 to 0.45 ft after one frame at 5.0038 ft).
pub const PAN_EASING: f32 = 0.01;
/// What the scripts add to the speed they ease toward (`MaxSpeedModifier`).
pub const MAX_SPEED_MODIFIER: f32 = 0.2;
/// A gap this small, ft, sets the speed back to [`CAMERA_SPEED`] the next
/// frame (the threshold as read; AG.40 shows the fall-back).
pub const STILL_GAP: f32 = 0.005;

/// The camera target's intent point leads his last move by this many
/// seconds of it.
pub const INTENT_AHEAD: f32 = 0.9;
/// The velocity point closes on the intent point by at most his speed over
/// this a frame, and stays within his speed over [`VELOCITY_REACH`] of the
/// target (both in the script's per-frame units, scaled by the frame's step
/// over 1/60 s).
pub const VELOCITY_STEP: f32 = 42.85;
pub const VELOCITY_REACH: f32 = 4.285;
/// A camera's follow point made as it comes into use sits on its aim this
/// long, game seconds, counted from the frame after (AG.40: camera 1's, made
/// at `Transition 7-1`, in all six crossings).
pub const NEW_HOLD: f32 = 0.5;

/// The target closes this share of its gap to the velocity point a frame.
pub const TARGET_SHARE: f32 = 0.1;
/// The target stays within this many feet of him.
pub const TARGET_REACH: f32 = 17.0;

/// Degrees: a heading θ faces (sin θ, 0, cos θ).
pub fn along(heading: f32) -> Vec3 {
    let (s, c) = heading.to_radians().sin_cos();
    Vec3::new(s, 0.0, c)
}

/// The heading of a direction across the ground, degrees in [0, 360).
pub fn bearing(v: Vec3) -> f32 {
    v.x.atan2(v.z).to_degrees().rem_euclid(360.0)
}

fn ground(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Where a camera's eye sits off its follow point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LookFrom {
    /// At a fixed heading, as seen from the follow point: the view looks
    /// along it + 180° (camera 0's 195° looks along 15°).
    Heading(f32),
    /// Behind the follow point as seen from a placement, so the view always
    /// faces it (a look-from heading of 0 relative to the placement: AG.13's
    /// camera 7 on `Look_At_Target_0`, AG.40's camera 1 on
    /// `Look_At_Target_1`).
    Facing(Vec3),
}

/// A camera as a level defines it (`Camera - Define Camera`): lengths in feet,
/// angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraDef {
    pub id: u32,
    /// The eye's distance from the follow point.
    pub distance: f32,
    /// The eye's elevation above the follow point.
    pub pitch: f32,
    pub look_from: LookFrom,
    /// The aim's height above the follow point (look-at pitch 90°, straight
    /// up, on every level 213 camera AG.13 and AG.40 read).
    pub look_at: f32,
    /// The picture's height, degrees ([`FOV_Y`]).
    pub fov_y: f32,
    pub far_clip: f32,
}

impl CameraDef {
    /// Level 213's camera 0, the one at the start: 55 ft at 32° behind the
    /// view along 15°, aimed 9 ft up, far clip 800 ft (ag-motion section 3;
    /// AG.13 held the live eye to it within 1e-4 m).
    pub const LEVEL_213_START: CameraDef = CameraDef {
        id: 0,
        distance: 55.0,
        pitch: 32.0,
        look_from: LookFrom::Heading(195.0),
        look_at: 9.0,
        fov_y: FOV_Y,
        far_clip: 800.0,
    };

    /// How far from his feet toward the camera target the follow point aims:
    /// (pitch − 25°) / 20° (`Camera - Game Camera - Update Look At Position`
    /// as read; AG.13 measured 0.35000 at 32°, AG.40 0.15 at 28° and 0.50 at
    /// 35°). Clamped to [0, 1], a hypothesis: no camera AG met lies outside
    /// 25-45°.
    pub fn aim_share(&self) -> f32 {
        ((self.pitch - 25.0) / 20.0).clamp(0.0, 1.0)
    }

    /// The eye at the definition's distance and pitch behind `follow`, aimed
    /// the look-at height above it.
    pub fn view(&self, follow: Vec3) -> View {
        let behind = match self.look_from {
            LookFrom::Heading(h) => along(h),
            LookFrom::Facing(point) => {
                let to = ground(point - follow);
                if to.length_squared() > 0.0 {
                    -to.normalize()
                } else {
                    Vec3::NEG_Z
                }
            }
        };
        let (s, c) = self.pitch.to_radians().sin_cos();
        let d = self.distance * FOOT;
        View {
            eye: follow + behind * (d * c) + Vec3::Y * (d * s),
            aim: follow + Vec3::Y * (self.look_at * FOOT),
            fov_y: self.fov_y,
            far_clip: self.far_clip,
        }
    }
}

/// What the TV draws from: the eye, the point it looks at, the picture's
/// height in degrees and the far clip in feet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub eye: Vec3,
    pub aim: Vec3,
    pub fov_y: f32,
    pub far_clip: f32,
}

impl View {
    /// The heading the view looks along across the ground, degrees in [0,
    /// 360): what the stick walks him against.
    pub fn heading(&self) -> f32 {
        bearing(self.aim - self.eye)
    }

    /// Degrees above level, negative looking down.
    pub fn pitch(&self) -> f32 {
        let d = self.aim - self.eye;
        d.y.atan2(ground(d).length()).to_degrees()
    }

    /// The far clip in metres.
    pub fn far(&self) -> f32 {
        self.far_clip * FOOT
    }

    /// Two views weighted: `w` of `other`, each of the eye, the aim, the far
    /// clip and the picture's height the weighted sum (AG.40: the view's
    /// angles follow from the summed eye and aim to 2.0e-4°, where its tilt
    /// weighted alone misses by 0.256°).
    pub fn blend(&self, other: &View, w: f32) -> View {
        View {
            eye: self.eye.lerp(other.eye, w),
            aim: self.aim.lerp(other.aim, w),
            fov_y: self.fov_y + (other.fov_y - self.fov_y) * w,
            far_clip: self.far_clip + (other.far_clip - self.far_clip) * w,
        }
    }
}

/// A camera's follow point (the scripts' magic point): closes 8 % of its gap
/// to its aim point a game frame within 5 ft, whatever the frame's step
/// (AG.13: 0.0798-0.0802 in frames clamped at 0.05 s); a frame past 5 ft
/// raises its speed (The camera follows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowPoint {
    pub point: Vec3,
    /// Feet a frame; the share within the band is this over 5 ft.
    speed: f32,
    /// The last frame's gap, feet.
    gap: f32,
    /// Made this many game seconds ago (from the frame after), while it's
    /// held on its aim.
    held: Option<f32>,
}

impl FollowPoint {
    pub fn at(point: Vec3) -> FollowPoint {
        FollowPoint {
            point,
            speed: CAMERA_SPEED,
            gap: 0.0,
            held: None,
        }
    }

    /// Made on its aim: it sits there until [`NEW_HOLD`] of game time has
    /// passed since the frame after, then eases (AG.40: camera 1's follow
    /// point, read back off the eye through the blend, sat on its aim to 2
    /// mm for 31-32 frames, its first 8 % step the frame the steps since
    /// the one after it was made reached 0.5 s, in all six crossings).
    pub fn made(aim: Vec3) -> FollowPoint {
        FollowPoint {
            held: Some(0.0),
            ..FollowPoint::at(aim)
        }
    }

    /// Feet a frame: within the band it closes this over 5 ft of its gap.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// One game frame of `step` seconds toward `aim`; `run` is how far the
    /// fastest target moved this frame, feet. Only a new point's hold reads
    /// the step: the follow itself steps per frame.
    pub fn step(&mut self, aim: Vec3, run: f32, step: f32) {
        if let Some(held) = self.held {
            if held < NEW_HOLD {
                self.point = aim;
                self.gap = 0.0;
                self.held = Some(held + step);
                return;
            }
            self.held = None;
        }
        let to = aim - self.point;
        let gap = to.length() / FOOT;
        if self.gap <= STILL_GAP {
            self.speed = CAMERA_SPEED;
        }
        self.gap = gap;
        let moved = if gap <= EASE_BAND {
            gap / EASE_BAND * self.speed
        } else {
            let current = (0.5 * CAMERA_SPEED + 1.2 * (gap / 100.0).min(1.0) + MAX_SPEED_MODIFIER)
                .max(run + MAX_SPEED_MODIFIER);
            let ease = PAN_EASING * gap;
            self.speed = current.clamp(self.speed - ease, self.speed + ease);
            self.speed
        };
        if gap > 0.0 {
            self.point += to * (moved / gap).min(1.0);
        }
    }
}

/// The player's camera target (`Camera - Update Player Cam Target`): a point
/// that trails him as he sets off, draws ahead as he walks and holds still
/// once he stops (The camera follows). The law is a hypothesis (the module's
/// list).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookAhead {
    pub point: Vec3,
    velocity: Vec3,
    /// His pivot when the last frame began, and that frame's step.
    last: Option<(Vec3, f32)>,
}

impl LookAhead {
    /// At rest on his feet.
    pub fn at(feet: Vec3) -> LookAhead {
        LookAhead {
            point: feet,
            velocity: feet,
            last: Some((feet, NOMINAL_STEP)),
        }
    }

    /// At rest where a recording or a save left it, the hero standing at
    /// `feet`: the velocity point on it (a frame without a move pulls it
    /// there).
    pub fn resting(point: Vec3, feet: Vec3) -> LookAhead {
        LookAhead {
            point,
            velocity: point,
            last: Some((feet, NOMINAL_STEP)),
        }
    }

    /// His move across the ground into this frame, metres.
    fn moved(&self, feet: Vec3) -> Vec3 {
        self.last
            .map_or(Vec3::ZERO, |(before, _)| ground(feet - before))
    }

    /// One game frame of `step` seconds, his pivot `feet` as the frame began.
    pub fn step(&mut self, feet: Vec3, step: f32) {
        let moved = self.moved(feet);
        let len = moved.length();
        let speed = match self.last {
            Some((_, before)) if before > 0.0 => len / before,
            _ => 0.0,
        };
        self.last = Some((feet, step));
        let rate = step / NOMINAL_STEP;
        let intent = if len > 0.0 {
            feet + moved * (INTENT_AHEAD * speed / len)
        } else {
            feet
        };
        let reach = speed / VELOCITY_REACH * rate;
        let pace = speed / VELOCITY_STEP * rate;

        self.velocity.y = feet.y;
        let to = intent - self.velocity;
        let gap = to.length();
        if gap > 0.0 {
            self.velocity += to * (gap.min(pace) / gap);
        }
        let off = self.velocity - self.point;
        let gap = off.length();
        if gap > reach {
            self.velocity = self.point + off * (reach / gap);
        }
        self.point += (self.velocity - self.point) * TARGET_SHARE;
        let off = self.point - feet;
        let gap = off.length();
        if gap > TARGET_REACH * FOOT {
            self.point = feet + off * (TARGET_REACH * FOOT / gap);
        }
        self.point.y = feet.y;
    }
}

/// A camera transition plane (`Camera - Define Camera Transition Plane`): a
/// box with a camera in front of it along its facing and one behind
/// (AG.40's `Transition 7-1`: 41 x 14 x 35 ft at 92°, camera 7 in front,
/// camera 1 behind).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionPlane {
    /// Where its facing and size are measured from.
    pub centre: Vec3,
    /// The box's facing, degrees.
    pub facing: f32,
    /// Across the facing, up and along it, metres: the box's
    /// `CollisionInfo` size as authored.
    pub size: Vec3,
    pub front: CameraDef,
    pub back: CameraDef,
}

impl TransitionPlane {
    /// His feet's distance in front of the box's centre along its facing,
    /// metres.
    pub fn ahead(&self, feet: Vec3) -> f32 {
        ground(feet - self.centre).dot(along(self.facing))
    }

    /// The weight on the camera behind the plane: 3G² − 2G³, G = 0.5 − a / L
    /// clamped to [0, 1], a = [`Self::ahead`], L the box's length (AG.40: to
    /// 4.5e-7 on every record of 12 crossings).
    pub fn weight(&self, feet: Vec3) -> f32 {
        let g = (0.5 - self.ahead(feet) / self.size.z).clamp(0.0, 1.0);
        g * g * (3.0 - 2.0 * g)
    }

    /// His pivot within the box's half-sizes across, up and along its
    /// facing.
    pub fn contains(&self, feet: Vec3) -> bool {
        let d = feet - self.centre;
        let forward = along(self.facing);
        let across = Vec3::new(forward.z, 0.0, -forward.x);
        d.dot(across).abs() <= self.size.x / 2.0
            && d.y.abs() <= self.size.y / 2.0
            && d.dot(forward).abs() <= self.size.z / 2.0
    }
}

/// A camera with the follow point it views from, and whether the view used
/// it last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shot {
    def: CameraDef,
    follow: FollowPoint,
    used: bool,
}

/// What the view is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Current {
    Camera(u32),
    /// An index into the planes [`GameCamera::step`] is given.
    Plane(usize),
}

/// The gameplay camera: the camera target, every camera that has a follow
/// point and the view, stepped once a game frame.
#[derive(Debug, Clone, PartialEq)]
pub struct GameCamera {
    target: LookAhead,
    shots: Vec<Shot>,
    current: Current,
    view: View,
}

impl GameCamera {
    /// At rest on a hero standing at `feet`: the follow point and the camera
    /// target on his pivot (AG.13: at F0 the follow point sat 5.4e-5 m off
    /// it, an 8 % approach stalled at float resolution).
    pub fn new(def: CameraDef, feet: Vec3) -> GameCamera {
        GameCamera::resting(def, feet, feet, feet)
    }

    /// At rest where a recording or a save left it: `def` in use from
    /// `follow`, the camera target at `target`, the hero standing at `feet`.
    pub fn resting(def: CameraDef, follow: Vec3, target: Vec3, feet: Vec3) -> GameCamera {
        GameCamera {
            target: LookAhead::resting(target, feet),
            shots: vec![Shot {
                def,
                follow: FollowPoint::at(follow),
                used: true,
            }],
            current: Current::Camera(def.id),
            view: def.view(follow),
        }
    }

    /// The view after the last step: what the TV shows, and what the hero's
    /// next step walks him against.
    pub fn view(&self) -> View {
        self.view
    }

    /// The point each follow point's aim sits a share of the way to from his
    /// feet.
    pub fn target(&self) -> Vec3 {
        self.target.point
    }

    /// A camera's follow point, once it has one.
    pub fn follow_point(&self, id: u32) -> Option<Vec3> {
        self.shot(id).map(|s| s.follow.point)
    }

    /// A camera's follow point's speed, ft a frame.
    pub fn follow_speed(&self, id: u32) -> Option<f32> {
        self.shot(id).map(|s| s.follow.speed())
    }

    /// The camera the view is wholly made of, or `None` inside a plane.
    pub fn camera(&self) -> Option<u32> {
        match self.current {
            Current::Camera(id) => Some(id),
            Current::Plane(_) => None,
        }
    }

    fn shot(&self, id: u32) -> Option<&Shot> {
        self.shots.iter().find(|s| s.def.id == id)
    }

    /// Switches the view wholly to `def` at once; its follow point starts
    /// on its aim if it wasn't in use.
    pub fn cut(&mut self, def: CameraDef, feet: Vec3) {
        self.current = Current::Camera(def.id);
        self.track(&[def], feet, self.target.point, None);
        self.view = self.render(None);
    }

    /// One game frame of `step` seconds. `feet` is his pivot as the frame
    /// began, before his own step; `planes` are the level's transition
    /// planes, the same list every frame.
    pub fn step(&mut self, feet: Vec3, step: f32, planes: &[TransitionPlane]) {
        self.step_on(feet, step, planes, None);
    }

    /// [`Self::step`] with the follow points aiming on `target` instead of
    /// the camera's own target, which still steps: a replay feeding a
    /// recording's target (AG.17's Verify).
    pub fn step_on(
        &mut self,
        feet: Vec3,
        step: f32,
        planes: &[TransitionPlane],
        target: Option<Vec3>,
    ) {
        match planes.iter().position(|p| p.contains(feet)) {
            Some(i) => self.current = Current::Plane(i),
            None => {
                if let Current::Plane(i) = self.current {
                    let p = &planes[i];
                    let side = if p.ahead(feet) >= 0.0 {
                        p.front
                    } else {
                        p.back
                    };
                    self.current = Current::Camera(side.id);
                }
            }
        }
        let defs: Vec<CameraDef> = match self.current {
            Current::Camera(id) => {
                vec![
                    self.shot(id)
                        .expect("the camera in use has a follow point")
                        .def,
                ]
            }
            Current::Plane(i) => vec![planes[i].front, planes[i].back],
        };
        let run = self.target.moved(feet).length() / FOOT;
        self.track(
            &defs,
            feet,
            target.unwrap_or(self.target.point),
            Some((run, step)),
        );
        self.target.step(feet, step);
        let plane = match self.current {
            Current::Plane(i) => Some((&planes[i], feet)),
            Current::Camera(_) => None,
        };
        self.view = self.render(plane);
    }

    /// The cameras in `defs` are in use this frame: each steps its follow
    /// point toward its aim; one coming back into use starts on its aim
    /// (AG.40: camera 7's jumped its whole gap onto it, 1.325 m and 1.457 m,
    /// the frame his pivot came back inside `Transition 7-1`'s box, and
    /// closed 8 % from the next), one new to the level is made on it
    /// ([`FollowPoint::made`]); every other camera's stands still. `run` is
    /// `None` for a cut, which only starts them.
    fn track(&mut self, defs: &[CameraDef], feet: Vec3, target: Vec3, run: Option<(f32, f32)>) {
        let made = self.shots.len();
        for def in defs {
            if self.shot(def.id).is_none() {
                let aim = feet + (target - feet) * def.aim_share();
                self.shots.push(Shot {
                    def: *def,
                    follow: FollowPoint::made(aim),
                    used: true,
                });
            }
        }
        for shot in &mut self.shots[..made] {
            let used = defs.iter().any(|d| d.id == shot.def.id);
            if used {
                let aim = feet + (target - feet) * shot.def.aim_share();
                match (shot.used, run) {
                    (true, Some((run, step))) => shot.follow.step(aim, run, step),
                    (true, None) => {}
                    (false, _) => shot.follow = FollowPoint::at(aim),
                }
            }
            shot.used = used;
        }
    }

    fn render(&self, plane: Option<(&TransitionPlane, Vec3)>) -> View {
        let view = |id: u32| {
            let shot = self.shot(id).expect("a camera in use has a follow point");
            shot.def.view(shot.follow.point)
        };
        match (plane, self.current) {
            (Some((p, feet)), _) => view(p.front.id).blend(&view(p.back.id), p.weight(feet)),
            (None, Current::Camera(id)) => view(id),
            (None, Current::Plane(_)) => unreachable!("a plane in use renders with its plane"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEET: Vec3 = Vec3::new(10.19027, 43.38267, -93.39406);

    /// gameplay-camera's Eruptor at the start: camera 0 puts the eye 55 ft at
    /// 32° behind the view along 15° and aims 9 ft above his feet, (−3.679547,
    /// 8.883567, −13.732257) off them, 23.360138° down (AG.13's definition).
    #[test]
    fn the_rest_pose_is_camera_0s_definition() {
        let camera = GameCamera::new(CameraDef::LEVEL_213_START, FEET);
        let view = camera.view();
        let off = view.eye - FEET;
        let want = Vec3::new(-3.679547, 8.883567, -13.732257);
        assert!((off - want).abs().max_element() < 1e-4, "{off}");
        assert!((view.heading() - 15.0).abs() < 1e-4, "{}", view.heading());
        assert!((view.pitch() + 23.360138).abs() < 1e-4, "{}", view.pitch());
        assert!((view.aim - FEET - Vec3::Y * 2.7432).length() < 1e-5);
        assert_eq!(view.far_clip, 800.0);
        assert!((view.far() - 243.84).abs() < 1e-3);
        let back = ground(-off).length();
        assert!((back - 14.2167).abs() < 1e-4 && (off.y - 8.8836).abs() < 1e-4);
    }

    /// The camera follows: within 5 ft the follow point closes 8 % of its gap
    /// a frame, and a 50 ms frame closes the same 8 % as a 16.7 ms one (A
    /// long frame).
    #[test]
    fn eight_percent_a_frame_whatever_the_step() {
        let mut f = FollowPoint::at(Vec3::ZERO);
        f.step(Vec3::new(1.0, 0.0, 0.0), 0.0, 1.0 / 60.0);
        assert!((f.point.x - 0.08).abs() < 1e-6, "{}", f.point.x);
        let ends: Vec<Vec3> = [1.0 / 60.0, 0.05]
            .into_iter()
            .map(|step| {
                // He stands 1 m off where the camera rests: the follow point
                // aims 65 % of the way to his feet (the target still where he
                // was).
                let mut camera = GameCamera::new(CameraDef::LEVEL_213_START, Vec3::ZERO);
                camera.step(Vec3::new(1.0, 0.0, 0.0), step, &[]);
                camera.follow_point(0).unwrap()
            })
            .collect();
        assert!((ends[0].x - 0.08 * 0.65).abs() < 1e-6, "{ends:?}");
        assert_eq!(ends[0], ends[1]);
    }

    /// AG.13's corridor walk: one frame 5.0038 ft out raises the speed from
    /// 0.4 to 0.45 ft (0.01 x the gap), so the share within the band is 9 %
    /// after it, until the follow point comes to rest on its aim.
    #[test]
    fn a_frame_past_5_ft_raises_the_share() {
        let far = Vec3::new(5.0038 * FOOT, 0.0, 0.0);
        let mut g = FollowPoint::at(Vec3::ZERO);
        g.step(Vec3::new(0.3, 0.0, 0.0), 0.0, 1.0 / 60.0);
        let before = g.point;
        g.step(before + far, 0.0, 1.0 / 60.0);
        assert!((g.speed() - 0.450038).abs() < 1e-5, "{}", g.speed());
        assert!(((g.point - before).length() / FOOT - 0.450038).abs() < 1e-4);
        let at = g.point;
        let aim = at + Vec3::new(1.0, 0.0, 0.0);
        g.step(aim, 0.0, 1.0 / 60.0);
        let share = (g.point - at).length();
        assert!((share - 0.0900076).abs() < 1e-5, "{share}");
        // Come to rest on the aim, the speed falls back to 0.4 ft: 8 % again
        // (AG.40's crossings hold the eye to 1.4e-4 m with the fall-back,
        // 1.3-15 cm without).
        while (aim - g.point).length() / FOOT > STILL_GAP {
            g.step(aim, 0.0, 1.0 / 60.0);
        }
        g.step(aim, 0.0, 1.0 / 60.0);
        let at = g.point;
        g.step(at + Vec3::new(1.0, 0.0, 0.0), 0.0, 1.0 / 60.0);
        let share = (g.point - at).length();
        assert!((share - 0.08).abs() < 1e-5, "{share}");
    }

    #[test]
    fn the_target_holds_still_once_he_stops_and_trails_as_he_sets_off() {
        let mut t = LookAhead::at(Vec3::ZERO);
        let dt = 1.0 / 60.0;
        let mut feet = Vec3::ZERO;
        // Setting off at 6.553 m/s along +x: the target trails his feet.
        for _ in 0..20 {
            t.step(feet, dt);
            feet.x += 6.553 * dt;
        }
        assert!(t.point.x < feet.x, "{} {}", t.point.x, feet.x);
        // Walking on, it draws ahead.
        for _ in 0..240 {
            t.step(feet, dt);
            feet.x += 6.553 * dt;
        }
        let lead = t.point.x - feet.x;
        assert!(lead > 3.0, "{lead}");
        // He stops: a frame on it holds still, at his height.
        for _ in 0..5 {
            t.step(feet, dt);
        }
        let held = t.point;
        for _ in 0..60 {
            t.step(feet, dt);
        }
        assert_eq!(t.point, held);
        assert_eq!(t.point.y, feet.y);
    }

    fn plane() -> TransitionPlane {
        let mut front = CameraDef::LEVEL_213_START;
        front.id = 7;
        front.far_clip = 1000.0;
        let mut back = CameraDef::LEVEL_213_START;
        back.id = 1;
        back.pitch = 35.0;
        back.look_from = LookFrom::Heading(100.0);
        TransitionPlane {
            centre: Vec3::new(0.0, 0.0, 0.0),
            facing: 90.0,
            size: Vec3::new(41.0, 14.0, 35.0) * FOOT,
            front,
            back,
        }
    }

    /// A transition plane blends by where he stands: 0 in front of the box's
    /// middle by half its length or more, 1 behind it, 0.5 at its centre, the
    /// smoothstep between.
    #[test]
    fn the_weight_is_3g2_minus_2g3_of_where_he_stands() {
        let p = plane();
        let l = 35.0 * FOOT;
        let at = |a: f32| p.weight(Vec3::new(a, 0.0, 3.0));
        assert_eq!(at(l / 2.0), 0.0);
        assert_eq!(at(l), 0.0);
        assert_eq!(at(-l / 2.0), 1.0);
        assert!((at(0.0) - 0.5).abs() < 1e-6);
        let g: f32 = 0.5 - 0.25;
        assert!((at(0.25 * l) - (3.0 * g * g - 2.0 * g * g * g)).abs() < 1e-6);
        assert!(p.contains(Vec3::new(0.0, 1.0, 5.0)));
        assert!(!p.contains(Vec3::new(0.0, 1.0, 7.0)));
        assert!(!p.contains(Vec3::new(5.4, 1.0, 0.0)));
    }

    /// Through a plane the view is the two cameras' weighted on his feet as
    /// the frame began; standing in it, it holds; out the back it's wholly
    /// the back camera's and the front one's follow point stands still.
    #[test]
    fn a_plane_crossed_blends_holds_and_settles() {
        let p = plane();
        let planes = [p];
        let dt = 1.0 / 60.0;
        let start = Vec3::new(8.0, 0.0, 0.0);
        let mut camera = GameCamera::new(p.front, start);
        let mut feet = start;
        let mut last = camera.view();
        let mut weights = Vec::new();
        // West at 6 m/s across the box.
        for _ in 0..200 {
            let before = feet;
            feet.x -= 6.0 * dt;
            camera.step(before, dt, &planes);
            let view = camera.view();
            let w = p.weight(before);
            weights.push(w);
            if planes[0].contains(before) {
                let want = (1000.0 - 200.0 * w) - view.far_clip;
                assert!(want.abs() < 1e-3, "{want}");
            }
            last = view;
        }
        assert_eq!(camera.camera(), Some(1));
        assert!(weights.iter().any(|&w| w > 0.2 && w < 0.8));
        assert_eq!(last.far_clip, 800.0);
        let frozen = camera.follow_point(7).unwrap();
        for _ in 0..30 {
            let before = feet;
            feet.x -= 6.0 * dt;
            camera.step(before, dt, &planes);
        }
        assert_eq!(camera.follow_point(7), Some(frozen));
        // Back into the box and stop half way: the view holds.
        let mut feet = Vec3::new(-0.5, 0.0, 0.0);
        for _ in 0..240 {
            camera.step(feet, dt, &planes);
        }
        let held = camera.view();
        for _ in 0..30 {
            camera.step(feet, dt, &planes);
        }
        assert!((camera.view().eye - held.eye).length() < 1e-5);
        assert!((held.far_clip - (1000.0 - 200.0 * p.weight(feet))).abs() < 1e-3);
        feet.x += 0.1;
        camera.step(feet, dt, &planes);
        assert!((camera.view().far_clip - (1000.0 - 200.0 * p.weight(feet))).abs() < 1e-3);
    }

    /// A camera coming into use starts on its aim: camera 1, new to the
    /// level, sits there for the first 0.5 s of game time from the frame
    /// after the plane takes him; camera 7, back in use, jumps onto it once
    /// and closes 8 % from the next frame (AG.40).
    #[test]
    fn a_camera_coming_into_use_starts_on_its_aim() {
        let p = plane();
        let planes = [p];
        let dt = 1.0 / 60.0;
        let aim = |camera: &GameCamera, feet: Vec3, def: &CameraDef| {
            feet + (camera.target() - feet) * def.aim_share()
        };
        // West at 3 m/s from in front of the box until camera 1 is made.
        let mut camera = GameCamera::new(p.front, Vec3::new(8.0, 0.0, 0.0));
        let mut feet = Vec3::new(8.0, 0.0, 0.0);
        while camera.follow_point(1).is_none() {
            let before = feet;
            feet.x -= 3.0 * dt;
            camera.step(before, dt, &planes);
        }
        let mut on_aim = 0;
        for _ in 0..40 {
            let before = feet;
            feet.x -= 3.0 * dt;
            let want = aim(&camera, before, &p.back);
            camera.step(before, dt, &planes);
            if (camera.follow_point(1).unwrap() - want).length() < 1e-6 {
                on_aim += 1;
            } else {
                break;
            }
        }
        assert!((30..=31).contains(&on_aim), "{on_aim}");
        // On west out of the box, then back east into it: camera 7 jumps.
        while planes[0].contains(feet) || p.ahead(feet) > -7.0 {
            let before = feet;
            feet.x -= 6.0 * dt;
            camera.step(before, dt, &planes);
        }
        let frozen = camera.follow_point(7).unwrap();
        let mut jumped = false;
        for _ in 0..200 {
            let before = feet;
            feet.x += 6.0 * dt;
            let want = aim(&camera, before, &p.front);
            let was = camera.follow_point(7).unwrap();
            camera.step(before, dt, &planes);
            let now = camera.follow_point(7).unwrap();
            if !jumped && now != was {
                assert!((now - want).length() < 1e-6, "{now} {want}");
                assert!((was - frozen).length() == 0.0);
                jumped = true;
                let next = aim(&camera, feet, &p.front);
                camera.step(feet, dt, &planes);
                let moved = camera.follow_point(7).unwrap() - now;
                assert!((moved.length() / (next - now).length() - 0.08).abs() < 1e-4);
                break;
            }
        }
        assert!(jumped);
    }

    /// A camera facing a placement looks at it from its follow point
    /// wherever that is, its eye behind the follow point on the line.
    #[test]
    fn a_facing_camera_keeps_its_placement_ahead() {
        let target = Vec3::new(11.758, 53.862, -39.364);
        let def = CameraDef {
            id: 7,
            distance: 57.0,
            pitch: 28.0,
            look_from: LookFrom::Facing(target),
            look_at: 6.0,
            fov_y: FOV_Y,
            far_clip: 1000.0,
        };
        for follow in [Vec3::new(30.0, 50.0, -75.0), Vec3::new(20.0, 52.0, -78.0)] {
            let view = def.view(follow);
            let want = bearing(target - follow);
            assert!(
                (view.heading() - want).abs() < 1e-3,
                "{} {want}",
                view.heading()
            );
            assert!((view.pitch() + 22.4157).abs() < 1e-3, "{}", view.pitch());
            assert!(((view.eye - follow).length() - 57.0 * FOOT).abs() < 1e-4);
        }
        assert!((def.aim_share() - 0.15).abs() < 1e-6);
    }
}
