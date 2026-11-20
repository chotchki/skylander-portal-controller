//! A creature staged for a look (openspec AS.680), the pure half: what the
//! run lists and walks for it and where the camera stands to show the whole
//! of it. The TV's half (the route, the entry, the pin, no save) is the
//! binary's `stage` module, in `test-hooks` builds only.
//!
//! - Listed: the TV runs a level's placements off a whitelist
//!   ([`crate::scripts::level_config`]), and one off it is inert, so no hero
//!   in reach wakes it. A staged creature's name goes on the list
//!   ([`list`]), with an encounter's trigger placement when one is named,
//!   and nothing else.
//! - Walked: it's made a creature the engine moves ([`walk`]), as an
//!   encounter's own placements are ([`ScriptWorld::walk_encounter`]), so the
//!   test view's creature read and the drawing take it.
//! - Woken: by the activity rule alone ([`crate::scripts::awake`]): a managed
//!   placement whose range holds the hero wakes on its own. Nothing of ours
//!   is written on its range, buffer or state. One its file leaves at range
//!   0 is a controller's to wake: that's its encounter's trigger, named.
//! - Framed: [`frame`] stands the camera off the creature's bounding sphere
//!   by the distance that fits the sphere in the picture's narrower angle,
//!   [`ROUND`] degrees round from its facing and [`RISE`] up.

use bevy::math::Vec3;
use guac_script::{Config, Handle, Whitelist};

use crate::scripts::ScriptWorld;

/// How far above the creature the stage's camera looks down, degrees.
pub const RISE: f32 = 20.0;
/// How far round from the creature's facing the camera stands, degrees: off
/// its front, where the dev entry sets the hero down, so he isn't between.
pub const ROUND: f32 = 35.0;
/// The room kept round the creature's bounding sphere.
pub const MARGIN: f32 = 1.15;
/// The nearest the camera stands, metres: a small creature's sphere would
/// put the eye inside the near plane's reach of the level round it.
pub const NEAREST: f32 = 2.5;

/// `names` put on `config`'s whitelist by name, each once; how many were
/// added. A list that runs everything takes nothing.
pub fn list(config: &mut Config, names: &[&str]) -> usize {
    let Whitelist::Only { placements, .. } = &mut config.whitelist else {
        return 0;
    };
    let mut added = 0;
    for n in names {
        if !n.is_empty() && !placements.iter().any(|p| p == n) {
            placements.push((*n).to_owned());
            added += 1;
        }
    }
    added
}

/// The placement `name` in the file linked as `unit` made a creature the
/// engine moves; its handle, none where the file places no such name.
pub fn walk(world: &mut ScriptWorld, unit: u16, name: &str) -> Option<Handle> {
    let h = world.placed_in(unit, name)?;
    world.walk(h).then_some(h)
}

/// Where the stage's camera stands: its eye (metres, the level's frame),
/// its tilt and heading (degrees, as `/api/_test/tv/camera` takes them) and
/// its distance from the sphere's centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub eye: Vec3,
    pub tilt: f32,
    pub heading: f32,
    pub distance: f32,
}

/// The camera that shows a sphere at `centre` of `radius` whole: [`ROUND`]
/// plus `round` degrees round from `facing`, [`RISE`] up, at the distance
/// where the sphere fills the narrower of the picture's two angles
/// (`fov_y` degrees high, `aspect` wide over high) with [`MARGIN`] to
/// spare, never nearer than [`NEAREST`].
pub fn frame(
    centre: Vec3,
    radius: f32,
    facing: f32,
    round: f32,
    fov_y: f32,
    aspect: f32,
) -> Framing {
    let half_y = fov_y.to_radians() / 2.0;
    let half_x = (half_y.tan() * aspect).atan();
    let half = half_y.min(half_x);
    let distance = (radius.max(0.1) / half.sin() * MARGIN).max(NEAREST);
    let (az, up) = ((facing + ROUND + round).to_radians(), RISE.to_radians());
    let out = Vec3::new(up.cos() * az.sin(), up.sin(), up.cos() * az.cos());
    let eye = centre + out * distance;
    let to = centre - eye;
    Framing {
        eye,
        tilt: to.y.atan2(Vec3::new(to.x, 0.0, to.z).length()).to_degrees(),
        heading: to.x.atan2(to.z).to_degrees(),
        distance,
    }
}

/// Whether `point` falls inside the picture a camera at `f` draws (`fov_y`
/// degrees high, `aspect` wide over high), in front of it.
pub fn in_view(f: &Framing, fov_y: f32, aspect: f32, point: Vec3) -> bool {
    let forward = crate::level_scene::facing([f.tilt, f.heading, 0.0]);
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    let d = point - f.eye;
    let z = d.dot(forward);
    if z <= 0.0 {
        return false;
    }
    let half_y = (fov_y.to_radians() / 2.0).tan();
    (d.dot(up) / z).abs() <= half_y && (d.dot(right) / z).abs() <= half_y * aspect
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;

    /// openspec AS.680: the stage lists its creature by name once, beside
    /// what the level lists, and a run of everything takes nothing.
    #[test]
    fn a_staged_creature_goes_on_the_list_once() {
        let mut c = crate::scripts::smash_config(Mode::Play);
        assert!(!c.whitelist.lists("Enemy_Staged(3)", "C:/x/Unlisted.ai"));
        assert_eq!(list(&mut c, &["Enemy_Staged(3)", "", "Enemy_Staged(3)"]), 1);
        assert!(c.whitelist.lists("Enemy_Staged(3)", "C:/x/Unlisted.ai"));
        assert!(!c.whitelist.lists("Enemy_Staged(4)", "C:/x/Unlisted.ai"));
        assert_eq!(list(&mut c, &["Enemy_Staged(3)"]), 0);
        let mut all = c.clone();
        all.whitelist = Whitelist::All;
        assert_eq!(list(&mut all, &["Enemy_Staged(9)"]), 0);
    }

    /// openspec AS.680: the frame holds the whole sphere, every point of its
    /// rim in the picture, round from its facing and looking down on it,
    /// whatever its size and the picture's shape.
    #[test]
    fn the_frame_holds_the_whole_creature() {
        for (radius, fov, aspect) in [(0.5, 50.0, 16.0 / 9.0), (2.0, 40.0, 1.0), (4.0, 60.0, 0.5)] {
            let centre = Vec3::new(10.0, 3.0, -7.0);
            let f = frame(centre, radius, 90.0, 0.0, fov, aspect);
            assert!(f.tilt < -RISE + 0.01 && f.tilt > -RISE - 0.01, "{f:?}");
            assert!((f.eye - centre).length() >= NEAREST - 1e-4);
            assert!(in_view(&f, fov, aspect, centre));
            for d in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
                assert!(
                    in_view(&f, fov, aspect, centre + d * radius),
                    "{radius} {fov} {aspect} {d}"
                );
            }
            // Twice as far out the rim's past the picture's edge: the frame
            // isn't just far away.
            let tight = radius * 2.0 * MARGIN;
            let off = [Vec3::X, Vec3::Y, Vec3::Z].iter().any(|d| {
                !in_view(&f, fov, aspect, centre + *d * tight)
                    || !in_view(&f, fov, aspect, centre - *d * tight)
            });
            assert!(off || f.distance == NEAREST, "{radius} {fov} {aspect}");
            // Round from its facing: the eye stands at facing + ROUND.
            let at = (f.eye.x - centre.x).atan2(f.eye.z - centre.z).to_degrees();
            assert!((at - (90.0 + ROUND)).abs() < 0.01, "{at}");
        }
        // Behind the camera is out of the picture.
        let f = frame(Vec3::ZERO, 1.0, 0.0, 0.0, 50.0, 1.5);
        assert!(!in_view(&f, 50.0, 1.5, f.eye * 2.0));
    }
}
