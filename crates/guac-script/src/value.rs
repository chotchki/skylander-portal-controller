//! Values and the arithmetic, comparisons, conversions and rolls over them
//! (the crate docs' Values and arithmetic, Comparisons, Literals and rolls,
//! Members). Single precision throughout: an `f64` appears only in
//! [`Precision::Double`], the perturbation the exact layer's tests run to
//! see their checks fail without the original's rounding.

use crate::host::{Kind, MemberSpec};
use crate::program::{Obj, Sym};
use crate::vm::Precision;

/// What a cell, a member or a path step holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// Nothing: `abstractscriptgroup.null`, an unset reference.
    Null,
    Int(i32),
    Float(f32),
    /// A fraction carried in double precision: only [`Precision::Double`]'s
    /// arithmetic makes one.
    Double(f64),
    /// A packed RGBA colour, as stored.
    Color(u32),
    /// A packed screen position (two 12.4 fixed-point halves).
    Screen(u32),
    /// An object in a loaded file: a definition, a behaviour, a text key, a
    /// template, a placement in the level.
    Object(Obj),
    /// An object named by namespace and name (EXNM, flagged), interned.
    Named(Sym, Sym),
    /// Something the host owns: an actor, a measurement it hands back.
    Entity(u64),
    /// An index into the VM's sets ([`crate::Vm::set`]).
    Set(u32),
    /// An index into the VM's structure instances ([`crate::Vm::field`]).
    Struct(u32),
    /// An index into the VM's cells: a variable made at run time (a number
    /// a set grew).
    Cell(u32),
    /// A reference to a part of a packed value or a value cell's member (a
    /// set element's `screenmeasurement.x`): an index into the VM's bound
    /// places, read and written through `valuedereferencer`.
    Bound(u32),
    /// A slider: what `slide value` hands its block, an index into the VM's
    /// sliders (crate docs, Slides).
    Slider(u32),
}

impl Value {
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_) | Value::Double(_))
    }

    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Value::Int(i) => Some(i as f32),
            Value::Float(f) => Some(f),
            Value::Double(d) => Some(d as f32),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Int(i) => Some(f64::from(i)),
            Value::Float(f) => Some(f64::from(f)),
            Value::Double(d) => Some(d),
            _ => None,
        }
    }

    /// The value as an integer, a float truncated toward 0.
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            Value::Int(i) => Some(i),
            Value::Float(f) => Some(truncate(f)),
            Value::Double(d) => Some(d as i32),
            _ => None,
        }
    }
}

/// A float to an integer toward 0, saturating; NaN reads 0. The PPU's
/// `fctiwz` gives `i32::MIN` for NaN: a hypothesis nothing on the disc has
/// shown matters.
pub fn truncate(f: f32) -> i32 {
    f as i32
}

/// `a <operator> b`, codes 0 + 1 - 2 * 3 /. Two integers stay an integer
/// (wrapping; 0 for a division by 0 and for `i32::MIN / -1`); anything with a
/// float is computed in single precision. Nothing counts as the integer 0.
/// `None` for an operator past 3 or an operand that isn't a number.
pub fn arith(operator: u32, a: Value, b: Value) -> Option<Value> {
    arith_in(Precision::Single, operator, a, b)
}

/// [`arith`] at a [`Precision`]: in [`Precision::Double`] anything with a
/// fraction is computed and kept in double precision.
pub fn arith_in(precision: Precision, operator: u32, a: Value, b: Value) -> Option<Value> {
    let zero = |v: Value| if v == Value::Null { Value::Int(0) } else { v };
    let (a, b) = (zero(a), zero(b));
    let double = precision == Precision::Double
        || matches!(a, Value::Double(_))
        || matches!(b, Value::Double(_));
    if double && a.is_number() && b.is_number() && !matches!((a, b), (Value::Int(_), Value::Int(_)))
    {
        let (x, y) = (a.as_f64()?, b.as_f64()?);
        return Some(Value::Double(match operator {
            0 => x + y,
            1 => x - y,
            2 => x * y,
            3 => x / y,
            _ => return None,
        }));
    }
    match (a, b) {
        // A screen position's halves, each on its own and held to 16 bits;
        // a product scaled back by the halves' four fraction bits.
        (Value::Screen(x), Value::Screen(y)) => {
            let half = |w: u32, at: u32| i32::from((w >> at) as u16 as i16);
            let op = |p: i32, q: i32| -> Option<u32> {
                let r = match operator {
                    0 => p + q,
                    1 => p - q,
                    2 => (p * q) >> 4,
                    _ => return None,
                };
                Some(u32::from(r.clamp(-0x8000, 0x7FFF) as i16 as u16))
            };
            Some(Value::Screen(
                op(half(x, 16), half(y, 16))? << 16 | op(half(x, 0), half(y, 0))?,
            ))
        }
        // A colour's bytes, each on its own and held to 0-255 (openspec
        // AH.21: a pot's particle tint, white less the tint).
        (Value::Color(x), Value::Color(y)) => {
            let mut out = 0u32;
            for at in [24, 16, 8, 0] {
                let (p, q) = (((x >> at) & 0xFF) as i32, ((y >> at) & 0xFF) as i32);
                let r = match operator {
                    0 => p + q,
                    1 => p - q,
                    _ => return None,
                };
                out |= (r.clamp(0, 0xFF) as u32) << at;
            }
            Some(Value::Color(out))
        }
        (Value::Int(x), Value::Int(y)) => Some(Value::Int(match operator {
            0 => x.wrapping_add(y),
            1 => x.wrapping_sub(y),
            2 => x.wrapping_mul(y),
            3 => x.checked_div(y).unwrap_or(0),
            _ => return None,
        })),
        (a, b) => {
            let (x, y) = (a.as_f32()?, b.as_f32()?);
            Some(Value::Float(match operator {
                0 => x + y,
                1 => x - y,
                2 => x * y,
                3 => x / y,
                _ => return None,
            }))
        }
    }
}

/// The tolerance a comparison with a float in it allows.
pub const TOLERANCE: f32 = 1e-4;

/// Comparison code `code` (0 `<=`, 1 `==`, 2 `>=`, 3 `<`, 4 `>`, 5 `!=`):
/// `Some(holds)`, or `None` for a code that can't compare these (any code
/// past 5, or one past `==`/`!=` on non-numbers), which fails a check.
pub fn compare(code: u32, l: Value, r: Value) -> Option<bool> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Some(match code {
            0 => a <= b,
            1 => a == b,
            2 => a >= b,
            3 => a < b,
            4 => a > b,
            5 => a != b,
            _ => return None,
        }),
        (a, b)
            if a.is_number()
                && b.is_number()
                && (matches!(a, Value::Double(_)) || matches!(b, Value::Double(_))) =>
        {
            let d = a.as_f64()? - b.as_f64()?;
            let e = f64::from(TOLERANCE);
            Some(match code {
                0 => d <= e,
                1 => d.abs() <= e,
                2 => d >= -e,
                3 => d < -e,
                4 => d > e,
                5 => d.abs() > e,
                _ => return None,
            })
        }
        (a, b) if a.is_number() && b.is_number() => {
            let d = a.as_f32()? - b.as_f32()?;
            Some(match code {
                0 => d <= TOLERANCE,
                1 => d.abs() <= TOLERANCE,
                2 => d >= -TOLERANCE,
                3 => d < -TOLERANCE,
                4 => d > TOLERANCE,
                5 => d.abs() > TOLERANCE,
                _ => return None,
            })
        }
        (a, b) => match code {
            1 => Some(a == b),
            5 => Some(a != b),
            _ => None,
        },
    }
}

/// A write into a member of `spec`'s type, converted and clamped; `None`
/// drops the write.
pub fn convert(spec: &MemberSpec, v: Value) -> Option<Value> {
    let clamps = spec.min != 0.0 || spec.max != 0.0;
    match spec.kind {
        Kind::Int => {
            let i = v.as_i32()?;
            Some(Value::Int(if clamps {
                // The bounds of an integer member are whole numbers.
                i.clamp(
                    truncate(spec.min),
                    truncate(spec.max).max(truncate(spec.min)),
                )
            } else {
                i
            }))
        }
        Kind::Float => {
            let f = v.as_f32()?;
            Some(Value::Float(if clamps {
                f.max(spec.min).min(spec.max.max(spec.min))
            } else {
                f
            }))
        }
        Kind::Color => matches!(v, Value::Color(_)).then_some(v),
        Kind::Screen => matches!(v, Value::Screen(_)).then_some(v),
        Kind::Reference => (!v.is_number()).then_some(v),
        // The engine's sets are read, never written.
        Kind::Set => None,
        Kind::Any => Some(v),
    }
}

/// A random literal of `v` drawn with `word` (crate docs, Literals and
/// rolls): a float r reads r x word x 2^-32, the word rounded to single
/// precision first, an integer n reads `(word & 0x7fffffff) mod (n + 1)`
/// for n >= 0 and `mod (n - 1)` below (so `random(0)` is always 0); a packed
/// value is drawn per byte or half and isn't modelled yet (`None`).
pub fn random_literal(v: Value, word: u32) -> Option<Value> {
    match v {
        Value::Float(r) => Some(Value::Float(r * (word as f32 * (1.0 / 4_294_967_296.0)))),
        Value::Int(n) => {
            let m = if n < 0 {
                n.wrapping_sub(1)
            } else {
                n.wrapping_add(1)
            };
            let w = (word & 0x7fff_ffff) as i32;
            Some(Value::Int(w.checked_rem(m).unwrap_or(0)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour's bytes add and subtract each on its own, held to 0-255
    /// (openspec AH.21): the particle tinter's white less a pot's tint is
    /// the tint's complement byte by byte, a generator's colour less that
    /// stops at 0 where it would go under, and a sum stops at 255. A colour
    /// times or over anything stops unsupported, as does a colour with a
    /// number.
    #[test]
    fn a_colours_bytes_add_and_subtract_each_held_to_0_255() {
        let white = Value::Color(0xFFFF_FFFF);
        let tint = Value::Color(14_149_367); // 0x00D7E6F7
        assert_eq!(arith(1, white, tint), Some(Value::Color(0xFF28_1908)));
        assert_eq!(
            arith(1, Value::Color(0x1050_9020), Value::Color(0xFF28_1908)),
            Some(Value::Color(0x0028_7718)),
            "each byte stops at 0"
        );
        assert_eq!(
            arith(0, Value::Color(0x80F0_0102), Value::Color(0x8020_0304)),
            Some(Value::Color(0xFFFF_0406)),
            "each byte stops at 255"
        );
        assert_eq!(arith(2, white, tint), None);
        assert_eq!(arith(3, white, tint), None);
        assert_eq!(arith(0, white, Value::Int(1)), None);
    }

    #[test]
    fn every_comparison_code_at_and_either_side_of_the_tolerance() {
        // Each code against 1.0 with the left side 1.0 - 2e-4, 1.0 - 1e-4
        // (at the tolerance, inside it), 1.0, 1.0 + 1e-4 and 1.0 + 2e-4.
        // Offsets of 1e-4 are taken as the float nearest the tolerance inside
        // it, so the edge case is the tolerance's, not rounding's.
        let inside = TOLERANCE * 0.99;
        let outside = TOLERANCE * 2.0;
        let lefts = [
            1.0 - outside,
            1.0 - inside,
            1.0,
            1.0 + inside,
            1.0 + outside,
        ];
        // Expected per code, in `lefts` order.
        let table: [(u32, [bool; 5]); 6] = [
            (0, [true, true, true, true, false]),    // <=
            (1, [false, true, true, true, false]),   // ==
            (2, [false, true, true, true, true]),    // >=
            (3, [true, false, false, false, false]), // <
            (4, [false, false, false, false, true]), // >
            (5, [true, false, false, false, true]),  // !=
        ];
        for (code, want) in table {
            for (l, w) in lefts.iter().zip(want) {
                assert_eq!(
                    compare(code, Value::Float(*l), Value::Float(1.0)),
                    Some(w),
                    "code {code} with {l}"
                );
                // Mixed: an integer on the right converts.
                assert_eq!(compare(code, Value::Float(*l), Value::Int(1)), Some(w));
            }
        }
        // Integers compare exactly: 1 and 2 differ by more than nothing.
        for (code, want) in [
            (0, true),
            (1, false),
            (2, false),
            (3, true),
            (4, false),
            (5, true),
        ] {
            assert_eq!(compare(code, Value::Int(1), Value::Int(2)), Some(want));
        }
        // Past 5 nothing holds; non-numbers have only == and !=.
        assert_eq!(compare(6, Value::Int(1), Value::Int(1)), None);
        let o = Value::Entity(7);
        assert_eq!(compare(1, o, o), Some(true));
        assert_eq!(compare(5, o, Value::Null), Some(true));
        assert_eq!(compare(0, o, o), None);
    }

    #[test]
    fn single_precision_in_the_order_written() {
        // (1e8 + 1) - 1e8 is 0 in single precision (1e8 + 1 rounds to 1e8),
        // and 1 in double; (1e8 - 1e8) + 1 is 1 either way.
        let big = Value::Float(1e8);
        let one = Value::Int(1);
        let left = arith(1, arith(0, big, one).unwrap(), big).unwrap();
        assert_eq!(left, Value::Float(0.0));
        let right = arith(0, arith(1, big, big).unwrap(), one).unwrap();
        assert_eq!(right, Value::Float(1.0));
        // 0.1 + 0.2 in single precision is 0.3f32's neighbour 0.30000001.
        let s = arith(0, Value::Float(0.1), Value::Float(0.2)).unwrap();
        assert_eq!(s, Value::Float(0.1f32 + 0.2f32));
        assert_ne!(s.as_f32().unwrap() as f64, 0.1f64 + 0.2f64);
        // Integers wrap and keep their type; a float makes it float.
        assert_eq!(
            arith(0, Value::Int(i32::MAX), one),
            Some(Value::Int(i32::MIN))
        );
        assert_eq!(arith(3, Value::Int(7), Value::Int(2)), Some(Value::Int(3)));
        assert_eq!(arith(3, Value::Int(7), Value::Int(0)), Some(Value::Int(0)));
        assert_eq!(
            arith(3, Value::Int(i32::MIN), Value::Int(-1)),
            Some(Value::Int(0))
        );
        assert_eq!(
            arith(3, Value::Int(7), Value::Float(2.0)),
            Some(Value::Float(3.5))
        );
        assert_eq!(
            arith(3, Value::Float(1.0), Value::Int(0)),
            Some(Value::Float(f32::INFINITY))
        );
        assert_eq!(arith(2, Value::Null, Value::Int(5)), Some(Value::Int(0)));
        assert_eq!(arith(4, one, one), None);
        assert_eq!(arith(0, Value::Color(1), one), None);
    }

    #[test]
    fn a_cast_truncates_toward_zero() {
        let int = MemberSpec::new(Kind::Int, 0.0, 0.0);
        assert_eq!(convert(&int, Value::Float(2.99)), Some(Value::Int(2)));
        assert_eq!(convert(&int, Value::Float(-2.99)), Some(Value::Int(-2)));
        assert_eq!(convert(&int, Value::Float(0.5 + 0.99)), Some(Value::Int(1)));
        assert_eq!(convert(&int, Value::Int(-7)), Some(Value::Int(-7)));
        assert_eq!(convert(&int, Value::Entity(1)), None);
    }

    #[test]
    fn members_clamp_only_with_a_bound() {
        let alpha = MemberSpec::new(Kind::Int, 0.0, 255.0);
        assert_eq!(convert(&alpha, Value::Int(300)), Some(Value::Int(255)));
        assert_eq!(convert(&alpha, Value::Float(-3.5)), Some(Value::Int(0)));
        let speed = MemberSpec::new(Kind::Float, 0.0, 10.0);
        assert_eq!(convert(&speed, Value::Int(12)), Some(Value::Float(10.0)));
        let free = MemberSpec::new(Kind::Float, 0.0, 0.0);
        assert_eq!(convert(&free, Value::Int(-12)), Some(Value::Float(-12.0)));
        let colour = MemberSpec::new(Kind::Color, 0.0, 0.0);
        assert_eq!(convert(&colour, Value::Int(1)), None);
        assert_eq!(convert(&colour, Value::Color(9)), Some(Value::Color(9)));
        let r = MemberSpec::new(Kind::Reference, 0.0, 0.0);
        assert_eq!(convert(&r, Value::Float(1.0)), None);
        assert_eq!(convert(&r, Value::Null), Some(Value::Null));
    }

    #[test]
    fn random_literals_cover_both_ends_and_a_floats_top_on_its_128_highest_words() {
        // random(3): word mod 4, so 0 and 3 come up and 4 never does.
        assert_eq!(random_literal(Value::Int(3), 0), Some(Value::Int(0)));
        assert_eq!(random_literal(Value::Int(3), 3), Some(Value::Int(3)));
        assert_eq!(random_literal(Value::Int(3), 4), Some(Value::Int(0)));
        assert_eq!(random_literal(Value::Int(3), u32::MAX), Some(Value::Int(3)));
        // random(-3): mod -4 keeps the word's sign, 0..=3.
        assert_eq!(random_literal(Value::Int(-3), 7), Some(Value::Int(3)));
        // random(0.5): the word rounds to single precision first. The
        // highest word that stays under 2^32 reads the largest float under
        // 0.5; the 128 from 0xFFFF_FF80 up round to 2^32 and read 0.5 itself.
        assert_eq!(
            random_literal(Value::Float(0.5), 0xFFFF_FF7F),
            Some(Value::Float(0.5 - f32::EPSILON / 4.0))
        );
        for word in [0xFFFF_FF80, u32::MAX] {
            assert_eq!(
                random_literal(Value::Float(0.5), word),
                Some(Value::Float(0.5))
            );
        }
        assert_eq!(
            random_literal(Value::Float(0.5), 0),
            Some(Value::Float(0.0))
        );
        assert_eq!(random_literal(Value::Int(i32::MAX), 5), Some(Value::Int(5)));
        assert_eq!(random_literal(Value::Color(1), 5), None);
    }
}
