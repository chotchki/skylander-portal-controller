//! `slide value`'s easing and in-between values (crate docs, Slides): pure
//! arithmetic the VM's sliders step with.

use crate::value::Value;

/// How far through a slide `x` of its time is, with `out` of the time eased
/// at its start and `in_` at its end (each its operand × 0.02): the rate
/// rises linearly over the first `out`, holds, then falls linearly over the
/// last `in_`, continuous, 0 at 0 and 1 at 1; linear with neither. Two that
/// add past the whole time are scaled down to fill it. With `out == in_`
/// this is `guac_motion::locomotion::eased`, AG.11's reading of the
/// locomotion's slides (15 and 15); AH.10's pushes read `out` 50 as a rate
/// rising over the whole slide, the decel's x², at its start.
pub fn eased(x: f32, out: f32, in_: f32) -> f32 {
    if !x.is_finite() || x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (mut a, mut b) = (out.clamp(0.0, 1.0), in_.clamp(0.0, 1.0));
    if a + b > 1.0 {
        let s = a + b;
        a /= s;
        b /= s;
    }
    if a == 0.0 && b == 0.0 {
        return x;
    }
    let top = 1.0 / (1.0 - (a + b) / 2.0);
    if x < a {
        top * x * x / (2.0 * a)
    } else if x <= 1.0 - b {
        top * (a / 2.0 + (x - a))
    } else {
        let y = 1.0 - x;
        1.0 - top * y * y / (2.0 * b)
    }
}

/// The value `k` of the way from `from` to `to`: numbers in single
/// precision, a packed colour channel by channel, a packed screen position
/// half by half; `None` for anything else (a slide of it lands at once).
pub fn between(from: Value, to: Value, k: f32) -> Option<Value> {
    match (from, to) {
        (Value::Color(a), Value::Color(b)) => {
            let mut out = 0u32;
            for shift in [24u32, 16, 8, 0] {
                let p = ((a >> shift) & 0xff) as f32;
                let q = ((b >> shift) & 0xff) as f32;
                let c = (p + (q - p) * k).round().clamp(0.0, 255.0) as u32;
                out |= c << shift;
            }
            Some(Value::Color(out))
        }
        (Value::Screen(a), Value::Screen(b)) => {
            let half = |w: u32, at: u32| f32::from((w >> at) as u16 as i16);
            let mut out = 0u32;
            for at in [16u32, 0] {
                let (p, q) = (half(a, at), half(b, at));
                let h = (p + (q - p) * k).round().clamp(-32768.0, 32767.0) as i16 as u16;
                out |= u32::from(h) << at;
            }
            Some(Value::Screen(out))
        }
        (a, b) => {
            let (p, q) = (a.as_f32()?, b.as_f32()?);
            Some(Value::Float(p + (q - p) * k))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eased_runs_from_0_to_1_and_is_linear_with_no_ease() {
        for (out, in_) in [
            (0.0, 0.0),
            (0.3, 0.3),
            (1.0, 0.0),
            (0.0, 1.0),
            (0.2, 0.5),
            (1.0, 1.0),
        ] {
            assert_eq!(eased(0.0, out, in_), 0.0);
            assert_eq!(eased(1.0, out, in_), 1.0);
            let mut last = 0.0;
            for k in 1..100 {
                let v = eased(k as f32 / 100.0, out, in_);
                assert!(v >= last, "{out} {in_} at {k}");
                last = v;
            }
        }
        assert_eq!(eased(0.25, 0.0, 0.0), 0.25);
        assert_eq!(eased(-1.0, 0.3, 0.3), 0.0);
        assert_eq!(eased(f32::NAN, 0.3, 0.3), 0.0);
        assert_eq!(eased(2.0, 0.3, 0.3), 1.0);
    }

    /// AH.10's pushes: `out` 50 (1.0 of the time) and `in_` 0 make the
    /// rate rise over the whole slide, the value x².
    #[test]
    fn a_whole_ease_out_is_x_squared() {
        for k in 0..=20 {
            let x = k as f32 / 20.0;
            assert!((eased(x, 1.0, 0.0) - x * x).abs() < 1e-6, "{x}");
            // And the mirror at the end.
            let y = 1.0 - x;
            assert!((eased(x, 0.0, 1.0) - (1.0 - y * y)).abs() < 1e-6, "{x}");
        }
    }

    /// Symmetric, as AG.11 reads the locomotion's slides (15 and 15, 0.3 of
    /// the time each end): the rate's ramps and its top.
    #[test]
    fn a_symmetric_ease_ramps_both_ends() {
        let top = 1.0 / 0.7;
        assert!((eased(0.15, 0.3, 0.3) - top * 0.15 * 0.15 / 0.6).abs() < 1e-6);
        assert!((eased(0.5, 0.3, 0.3) - 0.5).abs() < 1e-6);
        assert!((eased(0.85, 0.3, 0.3) - (1.0 - top * 0.15 * 0.15 / 0.6)).abs() < 1e-6);
    }

    #[test]
    fn between_interpolates_numbers_colours_and_screen_positions() {
        assert_eq!(
            between(Value::Int(0), Value::Int(600), 0.25),
            Some(Value::Float(150.0))
        );
        assert_eq!(
            between(Value::Color(0x0000_00ff), Value::Color(0xff00_0000), 0.5),
            Some(Value::Color(0x8000_0080))
        );
        let screen =
            |x: i16, y: i16| Value::Screen((u32::from(x as u16) << 16) | u32::from(y as u16));
        assert_eq!(
            between(screen(-16, 32), screen(16, 0), 0.5),
            Some(screen(0, 16))
        );
        assert_eq!(between(Value::Null, Value::Int(1), 0.5), None);
        assert_eq!(between(Value::Color(0), Value::Int(1), 0.5), None);
    }
}
