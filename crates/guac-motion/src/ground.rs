//! What the engine finds under the hero after it moves him (openspec AG.16;
//! hero-movement's Slopes, Steps and drops, He leans with the ground): the
//! ground finder, the support check, the snap and the lean.
//!
//! - The ground finder (ag-motion section 2, as read): a ray straight down
//!   from 0.1 m over a point, out to 160 m (its 2.5 m doubling past 100),
//!   else up to 80 m; a normal pointing down is flipped. Its point is what
//!   the script's 0.4 ft band and the snap measure from.
//! - The support check: the contact planes within [`SUPPORT_GAP`] of his
//!   shape, those facing up at all (n.y > 0.08) summed into a surface normal;
//!   supported when that's within 45° of up (AG.39: the cliff's faces held him
//!   to 44.92° and not from 45.04°), or when they pin a probe straight down
//!   (a crease of two steep faces, as read).
//! - The snap (AG.9): on the ground, a pivot 0.01-0.375 m over the ground's
//!   point, or under it, is set 0.01 m over it. That's why he rides 10 mm
//!   above a slope once set there and 5 mm on level ground, where nothing
//!   lifts him past 10 mm, and is put back down from 189 mm in one frame.
//! - The lean (AG.9): his up axis turns toward the ground's normal at 120°/s
//!   until it's within half a degree.
//!
//! Hypotheses: the support's reach, [`SUPPORT_GAP`], the read probe's travel
//! at 1 m/s over 1/60 s: stepped a frame at a time from AG.38's 19,308
//! records, 8 mm calls 1,089 supported records unsupported, 1/60 m 165 and
//! 25 mm 143, the 93 without support called so at each, since the frames he
//! loses it nearer are the airborne test's ([`crate::hero`]), AG.9's gate's
//! 14.5 mm among them; the snap's 0.375 m past the 189 mm seen; that the
//! lean turns over the frame's own step; the ground finder's up-cast, read,
//! and seen only through the edge probe (AG.39's cliff foot).

use crate::seam::{BodyId, ContactPlane, PhysicsSeam, Pose, Quat, RayHit, Shape, Vec3};
use crate::solve::{Limit, solve};

/// cos 45°: the steepest ground that holds him.
pub const MAX_SLOPE_COS: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// How far a plane may be from his shape (past its skin) and still hold him
/// up, metres (a hypothesis, the module's).
pub const SUPPORT_GAP: f32 = 1.0 / 60.0;
/// Planes facing up less than this don't count toward support (as read).
const SUPPORT_FACING: f32 = 0.08;
/// Where the snap sets him, and how far it reaches.
pub const SNAP_HEIGHT: f32 = 0.01;
pub const SNAP_REACH: f32 = 0.375;
/// His lean's rate and where it stops.
pub const LEAN_RATE: f32 = 120.0;
pub const LEAN_SETTLED: f32 = 0.5;

/// Where the ground finder's ray met a surface, and that surface's normal
/// turned to face up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    pub point: Vec3,
    pub normal: Vec3,
}

/// The first surface along a ray as the original's rays see one: a ray
/// starting inside a convex solid reports nothing from it (as read of
/// Havok's convex rays, a hypothesis), where the seam reports where it
/// leaves, its normal facing back along the ray as a front face's would
/// (parry's non-solid ray), so the tell is the ray's start overlapping the
/// body it hit; the ray goes on from where it left. A mesh triangle's back
/// is a hit either way.
pub fn ray(
    seam: &dyn PhysicsSeam,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    exclude: &[BodyId],
) -> Option<RayHit> {
    let dir = direction.try_normalize()?;
    let mut travelled = 0.0;
    for _ in 0..8 {
        let from = origin + dir * travelled;
        let hit = seam.ray(from, dir, max_distance - travelled, exclude)?;
        let inside = hit.triangle.is_none()
            && seam
                .overlaps(&Shape::Sphere { radius: POINT }, Pose::at(from), exclude)
                .contains(&hit.body);
        if !inside {
            return Some(RayHit {
                distance: travelled + hit.distance,
                ..hit
            });
        }
        travelled += hit.distance + LEAVE_STEP;
        if travelled >= max_distance {
            return None;
        }
    }
    None
}

/// A point, as a sphere the seam can overlap, metres.
const POINT: f32 = 1.0e-5;
/// How far past where a ray left a convex solid it goes on from, metres.
const LEAVE_STEP: f32 = 1.0e-4;

/// The ground under (or over) `at`, the ground finder's way.
pub fn find(seam: &dyn PhysicsSeam, at: Vec3, exclude: &[BodyId]) -> Option<Ground> {
    let origin = at + Vec3::Y * 0.1;
    let hit = ray(seam, origin, Vec3::NEG_Y, 160.0, exclude)
        .or_else(|| ray(seam, origin, Vec3::Y, 80.0, exclude))?;
    let normal = if hit.normal.y < -0.05 {
        -hit.normal
    } else {
        hit.normal
    };
    Some(Ground {
        point: hit.point,
        normal,
    })
}

/// The support check over the planes at his shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Support {
    pub supported: bool,
    /// The summed normal of the planes facing up, when any is in reach.
    pub surface: Option<Vec3>,
}

pub fn support(planes: &[ContactPlane]) -> Support {
    let up: Vec<&ContactPlane> = planes
        .iter()
        .filter(|p| p.distance <= SUPPORT_GAP && p.normal.y > SUPPORT_FACING)
        .collect();
    let sum: Vec3 = up.iter().map(|p| p.normal).sum();
    let surface = (sum.length() > 1e-6).then(|| sum.normalize());
    let steep_enough = surface.is_some_and(|n| n.y >= MAX_SLOPE_COS);
    let pinned = up.len() > 1 && {
        let limits: Vec<Limit> = up
            .iter()
            .map(|p| Limit {
                normal: p.normal,
                min: 0.0,
            })
            .collect();
        solve(Vec3::NEG_Y, &limits).length_squared() < 1e-3
    };
    Support {
        supported: steep_enough || pinned,
        surface,
    }
}

/// The snap: the height to set his pivot at over the ground's point, if it
/// moves him.
pub fn snap(height: f32) -> Option<f32> {
    ((height > SNAP_HEIGHT && height < SNAP_REACH) || height < 0.0).then_some(SNAP_HEIGHT)
}

/// Degrees, the unit the lean and the slope thresholds are written in.
pub fn angle(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// His up axis turned toward `normal` by at most 120°/s over `step`,
/// unmoved within half a degree.
pub fn lean(up: Vec3, normal: Vec3, step: f32) -> Vec3 {
    let gap = angle(up, normal);
    if gap <= LEAN_SETTLED {
        return up;
    }
    let turn = (LEAN_RATE * step).min(gap) / gap;
    (Quat::IDENTITY.slerp(Quat::from_rotation_arc(up, normal), turn) * up).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parry::ParryWorld;
    use crate::seam::{Body, BodyKind, Collider, Mesh, Pose};
    use std::sync::Arc;

    fn plane(normal: Vec3, distance: f32) -> ContactPlane {
        ContactPlane {
            body: BodyId(1),
            triangle: Some(0),
            material: Some(1),
            normal: normal.normalize(),
            point: Vec3::ZERO,
            distance,
        }
    }

    fn face(degrees: f32) -> Vec3 {
        let a = degrees.to_radians();
        Vec3::new(a.sin(), a.cos(), 0.0)
    }

    /// The cliff under the ledge route: 44.92° holds him, 45.04° doesn't.
    #[test]
    fn ground_to_45_holds_him() {
        assert!(support(&[plane(face(44.92), 0.0)]).supported);
        assert!(!support(&[plane(face(45.04), 0.0)]).supported);
        assert!(!support(&[plane(face(50.0), 0.0)]).supported);
        // Out of reach, level ground holds nothing up.
        assert!(!support(&[plane(Vec3::Y, SUPPORT_GAP + 1e-3)]).supported);
        assert!(support(&[plane(Vec3::Y, SUPPORT_GAP)]).supported);
        // A crease of two 60° faces pins him.
        assert!(support(&[plane(face(60.0), 0.0), plane(face(-60.0), 0.0)]).supported);
    }

    /// Snapped back down: from 189 mm, and from 11 mm, to 10; at 5 mm, left.
    #[test]
    fn the_snap_sets_him_10_mm_up() {
        assert_eq!(snap(0.189), Some(0.01));
        assert_eq!(snap(0.011), Some(0.01));
        assert_eq!(snap(0.005), None);
        assert_eq!(snap(-0.002), Some(0.01));
        assert_eq!(snap(0.4), None);
    }

    /// He leans with the ground: 2° a frame at 1/60 s toward 11.3° from
    /// 3.5°, left within half a degree.
    #[test]
    fn the_lean_turns_at_120_a_second() {
        let (from, to) = (face(3.5), face(11.3));
        let mut up = from;
        let mut turns = Vec::new();
        for _ in 0..6 {
            let next = lean(up, to, 1.0 / 60.0);
            turns.push(angle(up, next));
            up = next;
        }
        assert!((turns[0] - 2.0).abs() < 1e-3 && (turns[2] - 2.0).abs() < 1e-3);
        assert!(angle(up, to) <= LEAN_SETTLED, "{}", angle(up, to));
        assert_eq!(lean(face(1.0), face(1.4), 1.0 / 60.0), face(1.0));
    }

    #[test]
    fn the_ground_finder_looks_down_then_up() {
        let floor = Mesh::new(
            vec![
                Vec3::new(-5.0, 2.0, -5.0),
                Vec3::new(-5.0, 2.0, 5.0),
                Vec3::new(5.0, 2.0, 5.0),
                Vec3::new(5.0, 2.0, -5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![1, 1],
        )
        .unwrap();
        let world = ParryWorld::new(&[Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::IDENTITY,
            collider: Collider::Mesh(Arc::new(floor)),
        }])
        .unwrap();
        let above = find(&world, Vec3::new(0.0, 3.0, 0.0), &[]).unwrap();
        assert!((above.point.y - 2.0).abs() < 1e-5 && above.normal.y > 0.99);
        // Under the floor nothing's below: it finds the floor above, its
        // normal turned up.
        let under = find(&world, Vec3::new(0.0, -1.0, 0.0), &[]).unwrap();
        assert!((under.point.y - 2.0).abs() < 1e-5 && under.normal.y > 0.99);
        assert!(find(&world, Vec3::new(20.0, 3.0, 0.0), &[]).is_none());
    }

    /// From inside a convex solid the finder sees through it: a box around
    /// the point over a floor gives the floor, not the box's underside.
    #[test]
    fn a_ray_from_inside_a_convex_goes_on_through_it() {
        let floor = Mesh::new(
            vec![
                Vec3::new(-5.0, 0.0, -5.0),
                Vec3::new(-5.0, 0.0, 5.0),
                Vec3::new(5.0, 0.0, 5.0),
                Vec3::new(5.0, 0.0, -5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![1, 1],
        )
        .unwrap();
        let world = ParryWorld::new(&[
            Body {
                id: BodyId(1),
                kind: BodyKind::Static,
                pose: Pose::IDENTITY,
                collider: Collider::Mesh(Arc::new(floor)),
            },
            Body {
                id: BodyId(2),
                kind: BodyKind::Kinematic,
                pose: Pose::at(Vec3::new(0.0, 2.0, 0.0)),
                collider: Collider::Convex(Shape::Box {
                    half_extents: Vec3::splat(1.0),
                    margin: 0.0,
                }),
            },
        ])
        .unwrap();
        let at = Vec3::new(0.0, 2.0, 0.0);
        let raw = world
            .ray(at + Vec3::Y * 0.1, Vec3::NEG_Y, 160.0, &[])
            .unwrap();
        assert_eq!(raw.body, BodyId(2), "the seam reports the box's underside");
        assert!((raw.point.y - 1.0).abs() < 1e-5, "{raw:?}");
        let g = find(&world, at, &[]).unwrap();
        assert!(g.point.y.abs() < 1e-5 && g.normal.y > 0.99, "{g:?}");
        let hit = ray(&world, at + Vec3::Y * 0.1, Vec3::NEG_Y, 160.0, &[]).unwrap();
        assert_eq!(hit.body, BodyId(1));
        assert!((hit.distance - 2.1).abs() < 1e-3, "{hit:?}");
        // From above, the box's top is a hit as ever.
        let top = find(&world, Vec3::new(0.0, 5.0, 0.0), &[]).unwrap();
        assert!((top.point.y - 3.0).abs() < 1e-5);
    }
}
