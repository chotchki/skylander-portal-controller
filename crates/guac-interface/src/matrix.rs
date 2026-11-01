//! A placement's matrix as the interface reads it (the crate docs, The
//! sprite: Matrices): a place and three turns in degrees made into rows,
//! and a point carried or turned by them. Single precision as the
//! original's, its trigonometry on the angle rounded to single first.

use guac_ir::interface::Matrix;

/// Degrees to radians as the original's constant (0x113684).
const RADIANS: f32 = 0.017_453_292;

/// sin and cos of `degrees`, the angle rounded to single precision first.
fn sin_cos(degrees: f32) -> (f32, f32) {
    let a = (f64::from(degrees) * f64::from(RADIANS)) as f32;
    let (s, c) = f64::from(a).sin_cos();
    (s as f32, c as f32)
}

/// The matrix's rows, x, depth, y and the place, as 16 floats (0x113688):
/// the turn about x negated, then the turns about depth and y. A turn of 0
/// takes sin 0 and cos 1 exactly, so a matrix turned by nothing is the
/// identity with its place.
pub fn rows(m: &Matrix) -> [f32; 16] {
    let [rx, ry, rz] = m.rotation;
    let (sx, cx) = sin_cos(-rx);
    let (sy, cy) = sin_cos(ry);
    let (sz, cz) = sin_cos(rz);
    let sxsy = sx * sy;
    let [px, pd, py] = m.position;
    [
        sxsy * sz + cy * cz,
        cx * sz,
        (cy * sx) * sz - cz * sy,
        0.0,
        cz * sxsy - cy * sz,
        cx * cz,
        (cy * cz) * sx + sy * sz,
        0.0,
        cx * sy,
        -sx,
        cx * cy,
        0.0,
        px,
        pd,
        py,
        1.0,
    ]
}

/// `v` (x, depth, y) carried by the rows: turned, then moved by the place
/// (0x1E2EA0; its divide by w never happens on rows made by [`rows`]).
pub fn carry(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    let [x, d, y] = v;
    std::array::from_fn(|i| d * m[4 + i] + y * m[8 + i] + x * m[i] + m[12 + i])
}

/// `v` turned by the rows alone, no place (0x1E2E24).
pub fn turn(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    let [x, d, y] = v;
    std::array::from_fn(|i| y * m[8 + i] + x * m[i] + d * m[4 + i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(position: [f32; 3], rotation: [f32; 3]) -> Matrix {
        Matrix { position, rotation }
    }

    #[test]
    fn no_turn_is_the_identity_with_its_place() {
        let m = rows(&at([5.0, 7.0, -3.0], [0.0; 3]));
        assert_eq!(carry(&m, [1.0, 2.0, 3.0]), [6.0, 9.0, 0.0]);
        assert_eq!(turn(&m, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    /// A turn about depth turns x toward y the way the corner shades turn:
    /// 90 takes x to -y and y to x.
    #[test]
    fn a_turn_about_depth_turns_in_the_screens_plane() {
        let m = rows(&at([0.0; 3], [0.0, 90.0, 0.0]));
        let [x, d, y] = turn(&m, [10.0, 0.0, 0.0]);
        assert!(
            x.abs() < 1e-5 && d == 0.0 && (y + 10.0).abs() < 1e-5,
            "{x} {d} {y}"
        );
        let [x, _, y] = turn(&m, [0.0, 0.0, 10.0]);
        assert!((x - 10.0).abs() < 1e-5 && y.abs() < 1e-5, "{x} {y}");
        // 180 points both back; 270 the other way round from 90.
        let half = rows(&at([0.0; 3], [0.0, 180.0, 0.0]));
        let [x, _, y] = turn(&half, [10.0, 0.0, 4.0]);
        assert!((x + 10.0).abs() < 1e-4 && (y + 4.0).abs() < 1e-4, "{x} {y}");
        let three = rows(&at([0.0; 3], [0.0, 270.0, 0.0]));
        let [x, _, y] = turn(&three, [10.0, 0.0, 0.0]);
        assert!(x.abs() < 1e-4 && (y - 10.0).abs() < 1e-4, "{x} {y}");
    }

    /// The general rows against the closed form of each axis's turn: x
    /// negated (a turn of +90 about x takes depth to -y), y's about the y
    /// axis taking x toward depth.
    #[test]
    fn each_axis_turns_as_its_rows_say() {
        let mx = rows(&at([0.0; 3], [90.0, 0.0, 0.0]));
        let [x, d, y] = turn(&mx, [0.0, 10.0, 0.0]);
        assert!(
            x.abs() < 1e-5 && d.abs() < 1e-5 && (y + 10.0).abs() < 1e-5,
            "{x} {d} {y}"
        );
        let mz = rows(&at([0.0; 3], [0.0, 0.0, 90.0]));
        let [x, d, y] = turn(&mz, [10.0, 0.0, 0.0]);
        assert!(
            x.abs() < 1e-5 && (d - 10.0).abs() < 1e-5 && y.abs() < 1e-5,
            "{x} {d} {y}"
        );
        // Every turn keeps a length.
        let all = rows(&at([0.0; 3], [30.0, 50.0, 70.0]));
        let v = turn(&all, [3.0, 4.0, 12.0]);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len - 13.0).abs() < 1e-4, "{len}");
    }
}
