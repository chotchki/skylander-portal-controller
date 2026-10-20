//! The walk Giants' movement script keeps for the hero (openspec AG.16 on
//! AG.8, AG.10 and AG.38; design decision 4): a velocity in ft/s with a
//! vertical part, which the engine lays along the ground at its whole length
//! ([`crate::hero`]). Two steps a frame change it, in the script's order:
//!
//! - [`Walk::face_and_move`], with the stick held: the walk turns to the stick,
//!   its pitch kept, unless it's over the top speed by more than the script's
//!   float tolerance, when it keeps its own heading and steers; the push (160
//!   ft/s² x S x the step) goes along the stick, tilted to the slope, level
//!   when he's up in the air; the sum is cut to the larger of the top speed x
//!   S and the length it had (its across-the-ground part's up in the air).
//!   hero-movement's Turning and hard reversals, Top speed, Speeding up and
//!   slowing down, Steering in the air.
//! - [`Walk::update_movers`]: let go, or with the across part over the top by
//!   the tolerance, the across part slows (80 ft/s², 50 up in the air), the
//!   vertical part held; supported with no across part left, the walk stops
//!   dead; a frame off the ground within 0.4 ft of it adds 10 ft/s downward and
//!   cuts the whole to the top speed (a crest's gain), higher up gravity
//!   (136.21 ft/s² to 120 ft/s); landing from up there slides him a frame down
//!   the fall line. hero-movement's Speeding up and slowing down, Slopes,
//!   Falling and landing.
//!
//! "Up in the air" is unsupported 0.4 ft or more above the ground finder's
//! point. AG.38 replayed this a record at a time from the stored walk of the
//! record before to 0.032 ft/s on all 19,293 of its records bar the edge
//! probe's holds.
//!
//! Hypotheses here (AG.16's box has the list): the walk never creeps past the
//! tolerance on a slope as the original's does through float rounding in its
//! helper actor's measurements (AG.38), so on the ground it steers only after
//! something sped it past the top; the landing's slide speed is the
//! decompile's 1.2 x the fall's speed x the slope/90° (AG.10: 1.08-4.75 m/s
//! with no law yet, AG.47's); what the band does to a walk rising past 20 ft/s
//! is the reading's, never seen.

use crate::seam::Vec3;
use crate::stick::Push;
use crate::tuning::{BAND, BAND_KICK, BAND_RISE, FLOAT_TOLERANCE, FOOT, Tuning};

/// What the movement script reads of the hero's last engine step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Footing {
    /// The proxy's support check passed: on ground he can stand on.
    pub supported: bool,
    /// His pivot's height over the ground finder's point, metres; `None` with
    /// no ground found.
    pub height: Option<f32>,
    /// The normal the push tilts to.
    pub slope: Vec3,
    /// The ground finder's normal under him, for a landing's fall line.
    pub ground: Option<Vec3>,
}

impl Footing {
    /// What the tests and a settled start stand on.
    pub fn level(height: f32) -> Footing {
        Footing {
            supported: true,
            height: Some(height),
            slope: Vec3::Y,
            ground: Some(Vec3::Y),
        }
    }

    /// Up in the air: unsupported 0.4 ft or more above the ground point, or
    /// over none.
    pub fn high(&self) -> bool {
        !self.supported && self.height.is_none_or(|h| h / FOOT >= BAND)
    }
}

/// The walk, ft/s, and whether his last update off the ground began up in
/// the air, so landing slides him.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Walk {
    pub velocity: Vec3,
    pub falling: bool,
}

/// The part every slowing and cap in the air acts on.
pub fn across(v: Vec3) -> f32 {
    v.x.hypot(v.z)
}

/// The heading a vector's across part faces, degrees in [0, 360), `None`
/// with no across part.
pub fn heading(v: Vec3) -> Option<f32> {
    (across(v) > 0.0).then(|| v.x.atan2(v.z).to_degrees().rem_euclid(360.0))
}

/// `degrees` into (−180, 180].
pub fn wrap(degrees: f32) -> f32 {
    let d = degrees.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

fn along(heading: f32) -> Vec3 {
    let (s, c) = heading.to_radians().sin_cos();
    Vec3::new(s, 0.0, c)
}

/// The walk turned to `heading`, its pitch and length kept.
fn turned(v: Vec3, heading: f32) -> Vec3 {
    let a = across(v);
    let u = along(heading);
    Vec3::new(u.x * a, v.y, u.z * a)
}

/// Along `heading`, tilted to the plane with normal `slope` along it.
pub fn push_direction(heading: f32, slope: Option<Vec3>) -> Vec3 {
    let u = along(heading);
    match slope {
        Some(n) if n.y > 1e-3 => Vec3::new(u.x, -(n.x * u.x + n.z * u.z) / n.y, u.z).normalize(),
        _ => u,
    }
}

fn cut(v: Vec3, cap: f32) -> Vec3 {
    let len = v.length();
    if len > cap { v * (cap / len) } else { v }
}

/// The landing's slide: the decompile's 1.2 x the fall's speed x the
/// slope/90° straight down the fall line (a hypothesis, AG.47's).
fn slide(fall: f32, ground: Option<Vec3>) -> Vec3 {
    let Some(n) = ground else {
        return Vec3::ZERO;
    };
    let slope = n.y.clamp(-1.0, 1.0).acos().to_degrees();
    let down = Vec3::NEG_Y + n * n.y;
    if down.length() < 1e-6 {
        return Vec3::ZERO;
    }
    down.normalize() * (1.2 * fall.max(0.0) * slope / 90.0)
}

impl Walk {
    pub const STILL: Walk = Walk {
        velocity: Vec3::ZERO,
        falling: false,
    };

    /// Face and Move: the stick turns and pushes the walk; `step` is the one
    /// two frames before this one's own (AG.8's lag).
    pub fn face_and_move(&mut self, push: Push, footing: &Footing, tuning: &Tuning, step: f32) {
        if !push.held() {
            return;
        }
        let v = self.velocity;
        let length = v.length();
        let v = if length <= tuning.max_speed + FLOAT_TOLERANCE {
            turned(v, push.heading)
        } else {
            v
        };
        let high = footing.high();
        let add = push_direction(push.heading, (!high).then_some(footing.slope))
            * (tuning.push * push.strength * step);
        let floor = tuning.max_speed * push.strength;
        self.velocity = if high {
            let a = Vec3::new(v.x, 0.0, v.z);
            let sum = cut(a + add, floor.max(a.length()));
            Vec3::new(sum.x, v.y, sum.z)
        } else {
            cut(v + add, floor.max(length))
        };
    }

    /// Update Movers: the slowing, the stop, the crest's band, gravity and the
    /// landing; `step` is the one a frame before this one's own (AG.8's lag).
    pub fn update_movers(&mut self, held: bool, footing: &Footing, tuning: &Tuning, step: f32) {
        let high = footing.high();
        let mut v = self.velocity;
        let a = across(v);
        if !held || a > tuning.max_speed + FLOAT_TOLERANCE {
            let rate = if high {
                tuning.air_let_go
            } else {
                tuning.let_go
            };
            let left = a - rate * step;
            if left > 0.0 {
                v.x *= left / a;
                v.z *= left / a;
            } else {
                v.x = 0.0;
                v.z = 0.0;
            }
        }
        if footing.supported {
            if self.falling {
                v = slide(-v.y, footing.ground);
            } else if v.x == 0.0 && v.z == 0.0 {
                v = Vec3::ZERO;
            }
            self.falling = false;
        } else if !high && v.y <= BAND_RISE {
            v.y -= BAND_KICK;
            v = cut(v, tuning.max_speed);
            self.falling = false;
        } else {
            v.y = (v.y - tuning.gravity * step).max(-tuning.terminal);
            self.falling = high;
        }
        self.velocity = v;
    }
}

/// His facing turned toward the stick the short way, `rate` x S degrees a
/// second over `step` (two frames before this one's own, AG.8), and left
/// where it is with the stick let go.
pub fn turn_facing(facing: f32, push: Push, rate: f32, step: f32) -> f32 {
    if !push.held() {
        return facing;
    }
    let diff = wrap(push.heading - facing);
    let most = rate * push.strength * step;
    let turned = if diff.abs() <= most {
        push.heading
    } else {
        facing + most.copysign(diff)
    };
    turned.rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const T: Tuning = Tuning::ERUPTOR;

    fn full(heading: f32) -> Push {
        Push {
            strength: 1.0,
            heading,
        }
    }

    fn part(strength: f32, heading: f32) -> Push {
        Push { strength, heading }
    }

    fn frame(walk: &mut Walk, push: Push, footing: &Footing) {
        walk.face_and_move(push, footing, &T, DT);
        walk.update_movers(push.held(), footing, &T, DT);
    }

    fn up_in_the_air() -> Footing {
        Footing {
            supported: false,
            height: Some(3.0),
            slope: Vec3::Y,
            ground: Some(Vec3::Y),
        }
    }

    fn in_the_band() -> Footing {
        Footing {
            supported: false,
            height: Some(0.05),
            ..Footing::level(0.05)
        }
    }

    /// Top speed and Speeding up: 160 ft/s² x S a frame to 21.5 x S, the
    /// ninth frame at 1/60 s reaching it.
    #[test]
    fn the_push_reaches_top_speed_on_the_ninth_frame() {
        let ground = Footing::level(0.005);
        for s in [1.0, 0.5] {
            let mut walk = Walk::STILL;
            for k in 1..=9 {
                frame(&mut walk, part(s, 15.0), &ground);
                let want = (160.0 * s * DT * k as f32).min(21.5 * s);
                assert!((walk.velocity.length() - want).abs() < 1e-4, "{s} {k}");
            }
            assert_eq!(walk.velocity.length(), 21.5 * s);
            assert!((heading(walk.velocity).unwrap() - 15.0).abs() < 1e-3);
        }
    }

    /// Speeding up and slowing down: a speed over a weaker push's top holds.
    #[test]
    fn a_faster_walk_holds_under_a_weaker_push() {
        let ground = Footing::level(0.005);
        let mut walk = Walk {
            velocity: Vec3::new(0.0, 0.0, 21.5),
            falling: false,
        };
        for _ in 0..36 {
            frame(&mut walk, part(0.9896, 90.0), &ground);
        }
        assert!((walk.velocity.length() - 21.5).abs() < 1e-5);
        assert!((heading(walk.velocity).unwrap() - 90.0).abs() < 1e-3);
    }

    /// Letting go: 80 ft/s² a frame off, then a dead stop: 16 frames moving
    /// at 1/60 s, 2.711 ft (0.826 m, AG.8's 0.81-0.85 m in 0.26-0.27 s).
    #[test]
    fn letting_go_slows_at_80_and_stops_dead() {
        let ground = Footing::level(0.005);
        let mut walk = Walk {
            velocity: Vec3::new(0.0, 0.0, 21.5),
            falling: false,
        };
        let mut feet = 0.0;
        let mut moving = 0;
        for _ in 0..30 {
            frame(&mut walk, Push::NONE, &ground);
            if walk.velocity != Vec3::ZERO {
                moving += 1;
                feet += walk.velocity.length() * DT;
            }
        }
        assert_eq!(moving, 16);
        assert!((feet * FOOT - 0.8263).abs() < 1e-3, "{}", feet * FOOT);
        assert_eq!(walk.velocity, Vec3::ZERO);
    }

    /// Turning and hard reversals: at or under 21.5 + 1e-4 ft/s the walk takes
    /// the stick's way at once, so a reversal goes straight back at speed.
    #[test]
    fn up_to_the_tolerance_the_walk_turns_at_once() {
        let ground = Footing::level(0.005);
        for length in [21.5, 21.5 + 0.99e-4] {
            let mut walk = Walk {
                velocity: Vec3::new(0.0, 0.0, length),
                falling: false,
            };
            frame(&mut walk, full(180.0), &ground);
            assert!((heading(walk.velocity).unwrap() - 180.0).abs() < 1e-3);
            assert!((walk.velocity.length() - length).abs() < 1e-5);
        }
    }

    /// A swing at the gate: over the tolerance it steers, 160 ft/s² x S along
    /// the stick, cut to the length it had: 7.07° at full stick at 1/60 s.
    #[test]
    fn over_the_tolerance_the_walk_steers() {
        let ground = Footing::level(0.005);
        let over = 21.5 + 1.5e-4;
        let mut walk = Walk {
            velocity: Vec3::new(0.0, 0.0, over),
            falling: false,
        };
        walk.face_and_move(full(90.0), &ground, &T, DT);
        let turned = heading(walk.velocity).unwrap();
        assert!((turned - (160.0 * DT / 21.5f32).atan().to_degrees()).abs() < 0.01);
        assert!((walk.velocity.length() - over).abs() < 1e-5);
        // Over top speed across the ground: the stick held, the across part
        // over the tolerance slows by 80 ft/s² x the step for the frame.
        walk.update_movers(true, &ground, &T, DT);
        assert!((walk.velocity.length() - (over - 80.0 * DT)).abs() < 1e-4);
        // and the next frame it's back at the top, at once along the stick.
        frame(&mut walk, full(90.0), &ground);
        assert!((walk.velocity.length() - 21.5).abs() < 1e-5);
        assert!((heading(walk.velocity).unwrap() - 90.0).abs() < 1e-3);
    }

    /// Slopes: the push tilts to the ground's slope along the stick, so up a
    /// 17.3° slope the walk climbs it.
    #[test]
    fn the_push_tilts_to_the_slope() {
        let slope = 17.3f32.to_radians();
        let n = Vec3::new(0.0, slope.cos(), -slope.sin());
        let ground = Footing {
            slope: n,
            ground: Some(n),
            ..Footing::level(0.01)
        };
        let mut walk = Walk::STILL;
        for _ in 0..12 {
            frame(&mut walk, full(0.0), &ground);
        }
        let pitch = walk.velocity.y.atan2(across(walk.velocity)).to_degrees();
        assert!((pitch - 17.3).abs() < 1e-3, "{pitch}");
        assert!((walk.velocity.length() - 21.5).abs() < 1e-4);
        // Up in the air it doesn't tilt.
        let mut air = Walk::STILL;
        air.face_and_move(full(0.0), &up_in_the_air(), &T, DT);
        assert_eq!(air.velocity.y, 0.0);
    }

    /// Slopes: a frame off the ground within 0.4 ft adds 10 ft/s downward to
    /// the walk, cut to 21.5 ft/s, so landed he walks at √(v² + 10²) from v:
    /// at half stick 3.18 m/s comes out 4.40, at full 6.553 stays 6.553.
    #[test]
    fn a_crest_folds_ten_down_into_the_walk() {
        for (from, to) in [(3.179, 4.404), (2.269, 3.800), (6.553, 6.553)] {
            let mut walk = Walk {
                velocity: Vec3::new(0.0, 0.0, from / FOOT),
                falling: false,
            };
            walk.update_movers(true, &in_the_band(), &T, DT);
            assert!((walk.velocity.length() * FOOT - to).abs() < 1e-3, "{from}");
            assert!(!walk.falling);
        }
        // Held until let go: the push can't take it back under its length.
        let mut walk = Walk {
            velocity: Vec3::new(0.0, -10.0, 3.179 / FOOT),
            falling: false,
        };
        let length = walk.velocity.length();
        for _ in 0..30 {
            frame(&mut walk, part(0.485, 0.0), &Footing::level(0.005));
        }
        assert!((walk.velocity.length() - length).abs() < 1e-4);
    }

    /// Let go on a slope: the across part runs out while the vertical part
    /// holds, so he stops dead from what it leaves: walking down 17.3° at
    /// 21.5 ft/s, 15 frames and 0.94 m, from 1.96 m/s (Let go on a slope's
    /// 0.81-1.05 m in 0.23-0.27 s from 1.32-2.54 m/s).
    #[test]
    fn let_go_on_a_slope_stops_dead_from_the_vertical_part() {
        let slope = 17.3f32.to_radians();
        let n = Vec3::new(0.0, slope.cos(), slope.sin());
        let ground = Footing {
            slope: n,
            ground: Some(n),
            ..Footing::level(0.01)
        };
        let mut walk = Walk {
            velocity: Vec3::new(0.0, -slope.sin(), slope.cos()) * 21.5,
            falling: false,
        };
        let (mut feet, mut moving, mut last) = (0.0, 0, 0.0);
        for _ in 0..30 {
            frame(&mut walk, Push::NONE, &ground);
            if walk.velocity != Vec3::ZERO {
                moving += 1;
                last = walk.velocity.length();
                feet += last * DT;
            }
        }
        assert_eq!(moving, 15);
        assert!((0.81..=1.05).contains(&(feet * FOOT)), "{}", feet * FOOT);
        assert!((1.32..=2.54).contains(&(last * FOOT)), "{}", last * FOOT);
    }

    /// Falling and landing: up in the air 136.21 ft/s² x the step to 120 ft/s.
    #[test]
    fn up_in_the_air_he_falls_at_136_21_to_120() {
        let mut walk = Walk {
            velocity: Vec3::new(0.0, 0.0, 21.5),
            falling: false,
        };
        walk.update_movers(true, &up_in_the_air(), &T, 0.02);
        assert!((walk.velocity.y + 136.21 * 0.02).abs() < 1e-4);
        assert!(walk.falling);
        for _ in 0..200 {
            walk.update_movers(true, &up_in_the_air(), &T, DT);
        }
        assert_eq!(walk.velocity.y, -120.0);
        assert_eq!(across(walk.velocity), 21.5);
    }

    /// Steering in the air: swung 90° at full stick it turns 7.07° the first
    /// frame at 1/60 s, at half 3.55°, keeping 21.5 ft/s; reversed it slows
    /// 2.67 ft/s (0.81 m/s) a frame; let go, 50 ft/s² (0.254 m/s a frame), its
    /// fall unchanged.
    #[test]
    fn in_the_air_the_walk_steers_and_slows_at_50() {
        let air = up_in_the_air();
        let falling = Vec3::new(0.0, -30.0, 21.5);
        for (s, angle) in [(1.0, 7.07), (0.5, 3.55)] {
            let mut walk = Walk {
                velocity: falling,
                falling: true,
            };
            walk.face_and_move(part(s, 90.0), &air, &T, DT);
            assert!(
                (heading(walk.velocity).unwrap() - angle).abs() < 0.01,
                "{s}"
            );
            assert!((across(walk.velocity) - 21.5).abs() < 1e-4);
            assert_eq!(walk.velocity.y, -30.0);
        }
        let mut walk = Walk {
            velocity: falling,
            falling: true,
        };
        walk.face_and_move(full(180.0), &air, &T, DT);
        assert!((across(walk.velocity) - (21.5 - 160.0 * DT)).abs() < 1e-4);
        let mut walk = Walk {
            velocity: falling,
            falling: true,
        };
        walk.update_movers(false, &air, &T, DT);
        assert!((across(walk.velocity) - (21.5 - 50.0 * DT)).abs() < 1e-4);
    }

    /// Falling and landing: from up in the air he slides a frame down the fall
    /// line, then walks the stick's way at that speed; from within the band
    /// he doesn't.
    #[test]
    fn landing_from_up_in_the_air_slides_down_the_fall_line() {
        let slope = 20.0f32.to_radians();
        let n = Vec3::new(slope.sin(), slope.cos(), 0.0);
        let landed = Footing {
            slope: n,
            ground: Some(n),
            ..Footing::level(0.005)
        };
        let mut walk = Walk {
            velocity: Vec3::new(0.0, -50.0, 21.5),
            falling: true,
        };
        walk.update_movers(true, &landed, &T, DT);
        let want = 1.2 * 50.0 * 20.0 / 90.0;
        assert!((walk.velocity.length() - want).abs() < 1e-3);
        assert!(
            (heading(walk.velocity).unwrap() - 90.0).abs() < 1e-3,
            "downhill is +x"
        );
        assert!(walk.velocity.y < 0.0 && !walk.falling);
        // The next frame it turns to the stick at once, its pitch kept, and
        // the push adds to it.
        let pitched = walk.velocity;
        frame(&mut walk, full(0.0), &landed);
        assert!((heading(walk.velocity).unwrap()).abs() < 1e-3);
        let pushed = Vec3::new(0.0, pitched.y, across(pitched) + 160.0 * DT);
        assert!(
            (walk.velocity - pushed).length() < 1e-3,
            "{}",
            walk.velocity
        );

        let mut walk = Walk {
            velocity: Vec3::new(0.0, -10.0, 19.0),
            falling: false,
        };
        let before = walk.velocity;
        walk.update_movers(true, &landed, &T, DT);
        assert_eq!(walk.velocity, before);
    }

    /// Turning: 1440°/s x S the short way; let go, the facing stays.
    #[test]
    fn the_facing_turns_at_1440_times_s() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(turn_facing(0.0, full(105.0), 1440.0, DT), 24.0));
        assert!(near(turn_facing(0.0, part(0.5, 105.0), 1440.0, DT), 12.0));
        assert!(near(turn_facing(350.0, full(10.0), 1440.0, DT), 10.0));
        assert!(near(turn_facing(0.0, full(-100.0), 1440.0, DT), 336.0));
        assert_eq!(turn_facing(42.0, Push::NONE, 1440.0, DT), 42.0);
        let mut facing = 0.0;
        let mut frames = 0;
        while facing != 105.0 {
            facing = turn_facing(facing, full(105.0), 1440.0, DT);
            frames += 1;
        }
        assert_eq!(frames, 5, "Left at the start: 105° in the fifth frame");
    }
}
