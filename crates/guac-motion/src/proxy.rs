//! The engine's move (openspec AG.16; hero-movement's Walls, Slopes, Steps and
//! drops): the body follows the walk, laid along the ground at its whole
//! length while he's on it, through a cast-and-slide loop over the seam, as
//! ag-motion section 1 read the original's character proxy (a cast, the planes
//! it meets, a frictionless solve, at most 10 casts a step, no height gained
//! from a wall) and AG.9 measured it:
//!
//! - his shape is kept [`KEEP`] off every plane (AG.9: 4.99-5.00 mm off a wall
//!   by the proxy's own distance, 5 mm over level ground);
//! - walking into a wall he slides along it at his speed times
//!   1 − [`WALL_SLOWING`] x (1 − sin θ), θ the push's angle from head-on, from
//!   2° on (AG.9: w 0.1218-0.1250 per recording, 0.1235 their middle); nearer
//!   head-on he creeps at the push's share (0.1216 m/s at 1.06°) and within
//!   0.15° stops dead;
//! - the solve gains him no height from a wall: a slide that would lift him
//!   is solved again with each wall stood upright, so only ground he can
//!   stand on (within 45° of up) raises him.
//!
//! Hypotheses: the cast count and the plane dedupe (the read 10 and Havok's
//! Δd² + 1000 (1 − nᵢ·nⱼ) < 0.1; AG.9's manifold grew past two entries only in
//! corners the watch lost); where the wall law's three regimes part
//! ([`DEAD_ANGLE`] and [`SHARE_ANGLE`] sit midway between AG.9's 0.15° and
//! 1.06°, and 1.06° and 1.99°); that the law runs only while he walks (on the
//! ground), not in the air; that a step's edge within 45° of up is ground, so
//! the sphere's rounded bottom climbs a riser to where its edge meets it at
//! 45° (0.27 m for Eruptor), where AG.9 saw 4 cm climbed and 2.36 m not, and
//! nothing between; [`CONTACT_REACH`] for the planes the solve sees (Havok's
//! keep-contact tolerance as a guess); a surface's own velocity in the dedupe
//! (every solid still, until a moving body comes into reach); the
//! [`RECOVERY`] speed off a plane he's inside.

use crate::ground::MAX_SLOPE_COS;
use crate::seam::{BodyId, ContactPlane, PhysicsSeam, Pose, Quat, Shape, Vec3};
use crate::solve::{Limit, solve};

/// AG.9's gap between his shape and a wall or level ground, metres: the
/// solve lets him close to it and the cast stops him at it.
pub const KEEP: f32 = 0.005;
/// How far past the skin a plane still limits a solve, metres: Havok's
/// keep-contact tolerance as a guess (a hypothesis).
pub const CONTACT_REACH: f32 = 0.1;
pub const MAX_CASTS: usize = 10;
/// Time left in a step under which the loop stops (as read).
pub const MIN_TIME: f32 = 1.19e-7;
/// The wall slide's w.
pub const WALL_SLOWING: f32 = 0.1235;
/// Head-on within this he stops dead against a wall; under [`SHARE_ANGLE`] he
/// creeps at the push's share; from there the wall law (degrees).
pub const DEAD_ANGLE: f32 = 0.6;
pub const SHARE_ANGLE: f32 = 1.5;
/// The fastest the solve moves him off a plane he's inside, m/s.
pub const RECOVERY: f32 = 1.0;
/// A plane this near the skin is touching, metres.
const TOUCHING: f32 = 1.0e-3;
/// The velocity model's leave-the-ground test (as read): ground facing up
/// less than this, or a walk moving off it faster than [`LEAVE_SPEED`] m/s,
/// isn't walked along.
pub const LEAVE_NORMAL: f32 = 0.69;
pub const LEAVE_SPEED: f32 = 0.01;

/// `v` along the plane with normal `n` at its whole length, its across part's
/// direction kept (the velocity model's ground step, as read).
pub fn lay_along(v: Vec3, n: Vec3) -> Vec3 {
    let len = v.length();
    if len < 0.001 || n.y.abs() < 1e-6 {
        return v;
    }
    let c = n.dot(v / len).abs();
    if !(c > 1e-5 && c <= 0.99999) {
        return v;
    }
    let laid = Vec3::new(v.x, -(n.x * v.x + n.z * v.z) / n.y, v.z);
    let l = laid.length();
    if l < 1e-6 { v } else { laid * (len / l) }
}

/// Planes the same within Havok's dedupe (a surface's velocity taken as 0).
fn same(a: &ContactPlane, b: &ContactPlane) -> bool {
    let dd = a.distance - b.distance;
    dd * dd + 1000.0 * (1.0 - a.normal.dot(b.normal)) < 0.1
}

/// The hero's shape against the world.
pub struct Proxy<'a> {
    pub seam: &'a dyn PhysicsSeam,
    /// His query shape: the original's grown by the level mesh's radius
    /// ([`crate::solid::HavokSolid::grown`]).
    pub shape: Shape,
    /// The shape's centre from his pivot and its own turn, before his lean.
    pub centre: Vec3,
    pub rotation: Quat,
    pub exclude: &'a [BodyId],
}

/// Where a move left him, and the planes its last solve saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub pivot: Vec3,
    pub planes: Vec<ContactPlane>,
}

impl Proxy<'_> {
    /// His shape's pose with his pivot at `pivot`, leaning to `up`.
    pub fn pose(&self, pivot: Vec3, up: Vec3) -> Pose {
        let lean = Quat::from_rotation_arc(Vec3::Y, up);
        Pose::new(pivot + lean * self.centre, lean * self.rotation)
    }

    /// The planes within `reach` past his skin, nearest of each near-same set
    /// kept, their distances past the skin.
    pub fn planes(&self, pivot: Vec3, up: Vec3, reach: f32) -> Vec<ContactPlane> {
        let mut found = self.seam.contacts(
            &self.shape,
            self.pose(pivot, up),
            reach + KEEP,
            self.exclude,
        );
        for p in &mut found {
            p.distance -= KEEP;
        }
        found.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        let mut kept: Vec<ContactPlane> = Vec::new();
        for p in found {
            if !kept.iter().any(|k| same(k, &p)) {
                kept.push(p);
            }
        }
        kept
    }

    /// Moves him from `pivot` at `velocity` (m/s) for `step` seconds;
    /// `walking` when he's on the ground and the wall law applies.
    pub fn integrate(
        &self,
        pivot: Vec3,
        up: Vec3,
        velocity: Vec3,
        step: f32,
        walking: bool,
    ) -> Moved {
        let asked = velocity;
        let mut pos = pivot;
        let mut v = velocity;
        let mut remaining = step;
        let mut seen = Vec::new();
        for _ in 0..MAX_CASTS {
            if remaining <= MIN_TIME {
                break;
            }
            let planes = self.planes(pos, up, CONTACT_REACH);
            if walking && let Some(slid) = wall_law(asked, &planes) {
                v = slid;
            }
            let limits: Vec<Limit> = planes
                .iter()
                .map(|p| Limit {
                    normal: p.normal,
                    min: if p.distance >= 0.0 {
                        -p.distance / remaining
                    } else {
                        (-p.distance / remaining).min(RECOVERY)
                    },
                })
                .collect();
            let mut solved = solve(v, &limits);
            if solved.y > v.y.max(0.0) + 1e-6 {
                solved = solve(v, &upright(&limits));
            }
            seen = planes;
            let motion = solved * remaining;
            let len = motion.length();
            if len < 1e-7 {
                break;
            }
            let Some(hit) =
                self.seam
                    .cast(&self.shape, self.pose(pos, up), motion, 0.0, self.exclude)
            else {
                pos += motion;
                break;
            };
            let dir = motion / len;
            let facing = hit
                .planes
                .iter()
                .map(|p| -dir.dot(p.normal))
                .fold(0.0f32, f32::max);
            let back = if facing > 1e-3 { KEEP / facing } else { 0.0 };
            let travel = (hit.distance - back).clamp(0.0, len);
            pos += dir * travel;
            remaining *= 1.0 - travel / len;
            v = solved;
        }
        Moved {
            pivot: pos,
            planes: seen,
        }
    }
}

/// Each wall (a plane steeper than 45° facing up at all) stood upright, so a
/// solve over them can't lift him.
fn upright(limits: &[Limit]) -> Vec<Limit> {
    limits
        .iter()
        .map(|l| {
            let flat = Vec3::new(l.normal.x, 0.0, l.normal.z);
            if l.normal.y > 0.0 && l.normal.y < MAX_SLOPE_COS && flat.length() > 1e-3 {
                Limit {
                    normal: flat.normalize(),
                    min: l.min,
                }
            } else {
                *l
            }
        })
        .collect()
}

/// The wall law: with `asked` pushing him into a wall he touches, what he
/// moves at instead, by how near head-on it pushes.
fn wall_law(asked: Vec3, planes: &[ContactPlane]) -> Option<Vec3> {
    let flat = Vec3::new(asked.x, 0.0, asked.z);
    if flat.length() < 1e-6 {
        return None;
    }
    let flat = flat.normalize();
    let head_on = planes
        .iter()
        .filter(|p| p.distance <= TOUCHING && p.normal.y < MAX_SLOPE_COS)
        .filter(|p| asked.dot(p.normal) < 0.0)
        .filter_map(|p| {
            let n = Vec3::new(p.normal.x, 0.0, p.normal.z);
            (n.length() > 1e-3).then(|| (-flat.dot(n.normalize())).clamp(-1.0, 1.0))
        })
        .fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.max(c))))?;
    let theta = head_on.acos().to_degrees();
    if theta < DEAD_ANGLE {
        return Some(Vec3::ZERO);
    }
    let touching: Vec<Limit> = planes
        .iter()
        .filter(|p| p.distance <= TOUCHING)
        .map(|p| Limit {
            normal: p.normal,
            min: 0.0,
        })
        .collect();
    let slid = solve(asked, &upright(&touching));
    let len = slid.length();
    if theta < SHARE_ANGLE || len < 1e-6 {
        return Some(slid);
    }
    Some(slid * ((len + (1.0 - WALL_SLOWING) * (asked.length() - len)) / len))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Walls: 1 − 0.1235 (1 − sin θ) of his speed from 2°, the push's share
    /// at 1.06°, nothing within 0.15°.
    #[test]
    fn the_wall_law() {
        let speed = 6.5532;
        let wall = plane(Vec3::NEG_Z, 0.0);
        let floor = plane(Vec3::Y, 0.0);
        let at = |deg: f32| {
            let a = deg.to_radians();
            let asked = Vec3::new(a.sin(), 0.0, a.cos()) * speed;
            wall_law(asked, &[wall, floor]).unwrap()
        };
        for deg in [2.0f32, 5.0, 10.0, 20.0, 45.0, 70.0] {
            let want = speed * (1.0 - WALL_SLOWING * (1.0 - deg.to_radians().sin()));
            let got = at(deg);
            assert!((got.length() - want).abs() < 1e-4, "{deg}: {got}");
            assert!(got.z.abs() < 1e-5 && got.y.abs() < 1e-6, "{deg}: {got}");
        }
        // 1.06°: 6.5532 sin 1.06° (AG.9's 0.1216 m/s at 1.063°).
        let creep = at(1.06);
        assert!((creep.length() - 0.12123).abs() < 1e-4, "{creep}");
        assert_eq!(at(0.15), Vec3::ZERO);
        assert_eq!(at(0.04), Vec3::ZERO);
        // Not touching, or pushed away from: no law.
        assert_eq!(wall_law(Vec3::Z, &[plane(Vec3::NEG_Z, 0.01)]), None);
        assert_eq!(wall_law(Vec3::NEG_Z, &[wall]), None);
    }

    #[test]
    fn a_walk_lays_along_the_ground_at_its_whole_length() {
        let a = 17.3f32.to_radians();
        let n = Vec3::new(0.0, a.cos(), -a.sin());
        let laid = lay_along(Vec3::new(0.0, 0.0, 6.5532), n);
        assert!((laid.length() - 6.5532).abs() < 1e-4);
        assert!(laid.dot(n).abs() < 1e-5);
        assert!(laid.y > 0.0, "uphill along +z");
        // Straight down onto level ground it's left alone.
        assert_eq!(lay_along(Vec3::NEG_Y, Vec3::Y), Vec3::NEG_Y);
    }

    #[test]
    fn upright_walls_lift_nothing() {
        let bank = Limit {
            normal: Vec3::new(-50f32.to_radians().sin(), 50f32.to_radians().cos(), 0.0),
            min: 0.0,
        };
        let lifted = solve(Vec3::X, &[bank]);
        assert!(lifted.y > 0.1, "a 50° bank lifts a plain solve: {lifted}");
        let held = solve(Vec3::X, &upright(&[bank]));
        assert!(held.y.abs() < 1e-6 && held.x.abs() < 1e-6, "{held}");
    }
}
