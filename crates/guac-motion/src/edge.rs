//! The edge probe (openspec AG.16 on AG.36 and AG.39; hero-movement's An edge
//! with nothing below; design decision 2's edge-guard stand-in, which AJ's VM
//! deletes once it runs Minion.ai's per-frame macros).
//!
//! What stops him at an edge isn't the proxy, the support check or the ground
//! under him but a script's probe ahead of him (AG.36: at all 453 stops he
//! was supported with only the ground in his contacts). Each frame, after
//! Face and Move, it looks for ground under a point across the ground from his
//! pivot: along his walk when it's faster than 2 ft/s, [`FAST_REACH`] out;
//! slower, along the stick, half his authored width and a foot out (3 ft for
//! Eruptor). Finding none within 225 ft below, it zeroes his walk's
//! across-the-ground part, before he moves, and holds his controller off
//! (Face and Move ends early) until a later frame's probe passes. Ground
//! below lets him walk off and fall (AG.39's ledge route), and since the probe
//! runs in the air too, a refusal with his pivot already over an edge leaves
//! him slipping down it with his walk held at 0 (AG.39's `g39_b45_in`).
//!
//! Hypotheses: a solid between him and the point lets the probe pass (AG.9's
//! walls: the original walks into the wall behind the start and slides along
//! it, where a point 1.21 m along his walk has nothing under or over it, the
//! level's outside); [`FAST_REACH`] is the 1.2104-1.2106 m that fits every stop
//! AG.36 saw, 8.7 mm short of the script's 4 ft, and why (the probe's tilt
//! or its start) is unknown; the slow point is the reading's half width and a
//! foot (AG.36's slow refusals ended 0.896-0.911 m short of an edge, within a
//! creep step of it); the search is the ground finder's, so ground over the
//! point counts (AG.39's cliff foot, where his point under the ledge route
//! 3.06-3.20 m above him let him walk on); that ground deeper than 225 ft
//! stops him is read, not seen; with the stick let go and his walk at 2 ft/s
//! or under, nothing is probed.

use crate::ground;
use crate::seam::{BodyId, PhysicsSeam, Vec3};
use crate::stick::Push;
use crate::tuning::{FOOT, Tuning};
use crate::walk::across;

/// How far ahead along his walk the probe looks over 2 ft/s, metres.
pub const FAST_REACH: f32 = 1.2105;
/// Over this the probe looks along his walk, ft/s.
pub const PROBE_SPEED: f32 = 2.0;
/// The deepest ground below the point that counts (`WalkableSurface_MaxRange`).
pub const WALKABLE_RANGE: f32 = 225.0 * FOOT;

/// The point the probe looks under, if it looks: `walk` in ft/s, the stick's
/// push, from `pivot`.
pub fn point(pivot: Vec3, walk: Vec3, push: Push, tuning: &Tuning) -> Option<Vec3> {
    let (direction, reach) = if walk.length() > PROBE_SPEED && across(walk) > 0.0 {
        (Vec3::new(walk.x, 0.0, walk.z).normalize(), FAST_REACH)
    } else if push.held() {
        let (s, c) = push.heading.to_radians().sin_cos();
        (Vec3::new(s, 0.0, c), (tuning.width / 2.0 + 1.0) * FOOT)
    } else {
        return None;
    };
    Some(pivot + direction * reach)
}

/// How far over his pivot the line to the point runs, metres: the ground
/// finder's own start.
const SIGHT_HEIGHT: f32 = 0.1;

/// The probe passes: a solid stands across the ground between his pivot and
/// `point` (a wall stops him itself), or the ground finder found ground over
/// `point` or within 225 ft under it.
pub fn walkable(seam: &dyn PhysicsSeam, pivot: Vec3, point: Vec3, exclude: &[BodyId]) -> bool {
    let from = pivot + Vec3::Y * SIGHT_HEIGHT;
    let across = Vec3::new(point.x - from.x, 0.0, point.z - from.z);
    let blocked =
        across.length() > 1e-6 && seam.ray(from, across, across.length(), exclude).is_some();
    blocked
        || ground::find(seam, point, exclude).is_some_and(|g| point.y - g.point.y <= WALKABLE_RANGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parry::ParryWorld;
    use crate::seam::{Body, BodyKind, Collider, Mesh, Pose};
    use std::sync::Arc;

    const T: Tuning = Tuning::ERUPTOR;

    #[test]
    fn fast_along_the_walk_slow_along_the_stick() {
        let pivot = Vec3::new(1.0, 2.0, 3.0);
        let push = Push {
            strength: 0.5,
            heading: 90.0,
        };
        let fast = point(pivot, Vec3::new(0.0, -3.0, 21.5), push, &T).unwrap();
        assert!(
            (fast - (pivot + Vec3::Z * FAST_REACH)).length() < 1e-5,
            "{fast}"
        );
        let slow = point(pivot, Vec3::new(0.0, 0.0, 1.9), push, &T).unwrap();
        assert!(
            (slow - (pivot + Vec3::X * 3.0 * FOOT)).length() < 1e-5,
            "{slow}"
        );
        assert_eq!(point(pivot, Vec3::ZERO, Push::NONE, &T), None);
    }

    #[test]
    fn ground_within_225_ft_below_counts() {
        let at = |y: f32| {
            Mesh::new(
                vec![
                    Vec3::new(-5.0, y, -5.0),
                    Vec3::new(-5.0, y, 5.0),
                    Vec3::new(5.0, y, 5.0),
                    Vec3::new(5.0, y, -5.0),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                vec![1, 1],
            )
            .unwrap()
        };
        let world = |y: f32| {
            ParryWorld::new(&[Body {
                id: BodyId(1),
                kind: BodyKind::Static,
                pose: Pose::IDENTITY,
                collider: Collider::Mesh(Arc::new(at(y))),
            }])
            .unwrap()
        };
        let here = Vec3::new(-1.0, 0.0, 0.0);
        assert!(walkable(&world(-60.0), here, Vec3::ZERO, &[]));
        assert!(!walkable(&world(-70.0), here, Vec3::ZERO, &[]));
        assert!(
            walkable(&world(3.0), here, Vec3::ZERO, &[]),
            "ground over the point"
        );
        assert!(!walkable(
            &world(0.0),
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(9.0, 0.0, 0.0),
            &[]
        ));
    }

    /// A wall at the level's outside, nothing under or over the point behind
    /// it: the line to the point meets the wall, so the probe passes and the
    /// wall stops him (AG.9's wall behind the start).
    #[test]
    fn a_wall_before_the_point_passes() {
        let mesh = Mesh::new(
            vec![
                Vec3::new(-5.0, 0.0, -5.0),
                Vec3::new(-5.0, 0.0, 5.0),
                Vec3::new(5.0, 0.0, 5.0),
                Vec3::new(5.0, 0.0, -5.0),
                Vec3::new(-5.0, 3.0, 5.0),
                Vec3::new(5.0, 3.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3], [1, 4, 5], [1, 5, 2]],
            vec![1; 4],
        )
        .unwrap();
        let world = ParryWorld::new(&[Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::IDENTITY,
            collider: Collider::Mesh(Arc::new(mesh)),
        }])
        .unwrap();
        let pivot = Vec3::new(0.0, 0.005, 4.0);
        let behind = pivot + Vec3::Z * FAST_REACH;
        assert!(ground::find(&world, behind, &[]).is_none());
        assert!(walkable(&world, pivot, behind, &[]));
        // Off the floor's open side, no wall: refused.
        let open = Vec3::new(4.0, 0.005, 0.0);
        assert!(!walkable(&world, open, open + Vec3::X * FAST_REACH, &[]));
    }
}
