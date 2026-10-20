//! The plane solve (openspec AG.16; hero-movement's Walls): the velocity
//! nearest the one asked for that keeps every contact plane, the frictionless
//! slide the original's character proxy gets from Havok's simplex solver (as
//! read, ag-motion section 1). Exact for up to three active planes, which is
//! all a velocity in 3D can have: it tries each set of one, two and three and
//! keeps the nearest that every plane allows with the multipliers a ≥ plane
//! has to have.

use crate::seam::Vec3;

/// A plane the velocity mustn't cross faster than `min` allows: v · n ≥ min.
/// A negative `min` lets him approach a plane short of it; a positive one
/// moves him off a plane he's inside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    pub normal: Vec3,
    pub min: f32,
}

/// How far a velocity may fall short of a limit and still keep it, m/s: f32
/// noise in the projections.
const SLACK: f32 = 1.0e-5;

fn keeps(v: Vec3, limits: &[Limit]) -> bool {
    limits.iter().all(|l| v.dot(l.normal) >= l.min - SLACK)
}

/// The velocity nearest `v` keeping every limit; zero when none does (no
/// set of three or fewer pins a velocity every limit allows).
pub fn solve(v: Vec3, limits: &[Limit]) -> Vec3 {
    if keeps(v, limits) {
        return v;
    }
    let n = limits.len();
    let mut best: Option<(f32, Vec3)> = None;
    let mut consider = |x: Vec3| {
        if keeps(x, limits) {
            let d = (x - v).length_squared();
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, x));
            }
        }
    };
    for i in 0..n {
        if let Some(x) = onto(v, &[limits[i]]) {
            consider(x);
        }
        for j in i + 1..n {
            if let Some(x) = onto(v, &[limits[i], limits[j]]) {
                consider(x);
            }
            for k in j + 1..n {
                if let Some(x) = onto(v, &[limits[i], limits[j], limits[k]]) {
                    consider(x);
                }
            }
        }
    }
    best.map_or(Vec3::ZERO, |(_, x)| x)
}

/// `v` moved onto every plane of `set` (v · n = min on each) the least way,
/// when the set's normals are independent and each multiplier is one a ≥
/// limit allows (pushing off the plane, never pulling into it).
fn onto(v: Vec3, set: &[Limit]) -> Option<Vec3> {
    let m = set.len();
    let mut g = [[0.0f32; 3]; 3];
    let mut r = [0.0f32; 3];
    for a in 0..m {
        for b in 0..m {
            g[a][b] = set[a].normal.dot(set[b].normal);
        }
        r[a] = set[a].min - v.dot(set[a].normal);
    }
    let lambda = solve_small(&g, &r, m)?;
    if lambda[..m].iter().any(|&l| l < -SLACK) {
        return None;
    }
    Some(
        set.iter()
            .zip(lambda)
            .fold(v, |x, (l, lambda)| x + l.normal * lambda),
    )
}

/// `g` λ = `r` for the top-left `m` x `m`, by Cramer's rule; `None` when the
/// planes are near parallel.
fn solve_small(g: &[[f32; 3]; 3], r: &[f32; 3], m: usize) -> Option<[f32; 3]> {
    let det2 = |a: f32, b: f32, c: f32, d: f32| a * d - b * c;
    match m {
        1 => (g[0][0] > 1e-6).then(|| [r[0] / g[0][0], 0.0, 0.0]),
        2 => {
            let det = det2(g[0][0], g[0][1], g[1][0], g[1][1]);
            (det.abs() > 1e-5).then(|| {
                [
                    det2(r[0], g[0][1], r[1], g[1][1]) / det,
                    det2(g[0][0], r[0], g[1][0], r[1]) / det,
                    0.0,
                ]
            })
        }
        _ => {
            let det3 = |c0: [f32; 3], c1: [f32; 3], c2: [f32; 3]| {
                c0[0] * det2(c1[1], c2[1], c1[2], c2[2]) - c1[0] * det2(c0[1], c2[1], c0[2], c2[2])
                    + c2[0] * det2(c0[1], c1[1], c0[2], c1[2])
            };
            let col = |j: usize| [g[0][j], g[1][j], g[2][j]];
            let det = det3(col(0), col(1), col(2));
            (det.abs() > 1e-5).then(|| {
                [
                    det3(*r, col(1), col(2)) / det,
                    det3(col(0), *r, col(2)) / det,
                    det3(col(0), col(1), *r) / det,
                ]
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(normal: Vec3) -> Limit {
        Limit {
            normal: normal.normalize(),
            min: 0.0,
        }
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_velocity_that_keeps_every_plane_is_left_alone() {
        // Along the floor and away from a wall facing +x.
        let v = Vec3::new(1.0, 0.0, 2.0);
        assert_eq!(solve(v, &[wall(Vec3::Y), wall(Vec3::X)]), v);
    }

    #[test]
    fn into_a_wall_slides_along_it() {
        // 45° into a wall facing -x: what's left is along it.
        let v = Vec3::new(1.0, 0.0, 1.0);
        let x = solve(v, &[wall(Vec3::NEG_X), wall(Vec3::Y)]);
        assert!(near(x, Vec3::new(0.0, 0.0, 1.0)), "{x}");
    }

    #[test]
    fn into_a_corner_stops_on_its_crease() {
        // Two walls at right angles and the floor: only the crease is left,
        // and moving into both, nothing.
        let walls = [wall(Vec3::NEG_X), wall(Vec3::NEG_Z), wall(Vec3::Y)];
        assert!(near(solve(Vec3::new(1.0, 0.0, 1.0), &walls), Vec3::ZERO));
        let v = Vec3::new(1.0, 0.0, 0.2);
        assert!(near(solve(v, &walls), Vec3::ZERO));
        let v = Vec3::new(1.0, 0.0, -0.5);
        assert!(near(solve(v, &walls), Vec3::new(0.0, 0.0, -0.5)));
    }

    #[test]
    fn a_plane_short_of_him_lets_him_approach() {
        // 0.05 m away over 1/60 s: 3 m/s toward it is allowed, no more.
        let gap = Limit {
            normal: Vec3::NEG_X,
            min: -3.0,
        };
        assert!(near(solve(Vec3::X * 2.0, &[gap]), Vec3::X * 2.0));
        assert!(near(solve(Vec3::X * 6.0, &[gap]), Vec3::X * 3.0));
        // Inside one, it moves him off.
        let inside = Limit {
            normal: Vec3::Y,
            min: 0.3,
        };
        assert!(near(solve(Vec3::ZERO, &[inside]), Vec3::Y * 0.3));
    }

    #[test]
    fn a_crease_of_two_steep_faces_holds_a_probe_down() {
        // A V of two 60° faces: nothing downward gets through, as
        // checkSupport's probe solve reads it.
        let s = 60f32.to_radians();
        let faces = [
            wall(Vec3::new(s.sin(), s.cos(), 0.0)),
            wall(Vec3::new(-s.sin(), s.cos(), 0.0)),
        ];
        assert!(near(solve(Vec3::NEG_Y, &faces), Vec3::ZERO));
        // One face alone slides it.
        assert!(solve(Vec3::NEG_Y, &faces[..1]).length() > 0.5);
    }
}
