use super::builder::{T, block_bytes, one_block, stream_bytes};
use super::*;

#[test]
fn a_one_block_stream_reads_its_header_and_tracks() {
    let s = parse(&one_block()).unwrap();
    assert_eq!(s.frames, 48);
    assert!((s.frame_time - 1.0 / 24.0).abs() < 1e-7);
    assert!((s.length - 47.0 / 24.0).abs() < 1e-5);
    assert_eq!(s.blocks.len(), 1);
    let t = &s.blocks[0].tracks;
    assert_eq!(t.len(), 3);
    match &t[0] {
        Track::Spline {
            degree,
            knots,
            points,
        } => {
            assert_eq!(*degree, 2);
            assert_eq!(knots, &[0, 0, 0, 10, 20, 47, 47, 47]);
            assert_eq!(points.len(), 5);
            assert!((points[1] - 0.8).abs() < 1e-6, "255 is the high");
            assert!((points[2] - 0.8 * 128.0 / 255.0).abs() < 1e-6);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t[1], Track::Constant(0.0));
    match &t[2] {
        Track::Spline { degree, points, .. } => {
            assert_eq!(*degree, 1);
            assert_eq!(points, &[-0.5, 0.5], "low at 0, high at 255");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_later_block_starts_where_its_word_says() {
    let a = block_bytes(&[T::S(1, vec![0, 0, 255, 255], 0.0, 1.0, vec![255, 0])]);
    let b = block_bytes(&[T::C(0.25)]);
    let s = parse(&stream_bytes(300, 1, &[a, b])).unwrap();
    assert_eq!(s.blocks.len(), 2);
    assert_eq!(s.blocks[1].tracks, [Track::Constant(0.25)]);
    match &s.blocks[0].tracks[0] {
        Track::Spline { knots, .. } => assert_eq!(knots, &[0, 0, 255, 255]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_magic_a_missing_footer_or_a_short_block_refuses() {
    let good = one_block();
    let mut b = good.clone();
    b[0] = 0;
    assert!(matches!(
        parse(&b).unwrap_err().kind,
        ErrorKind::BadMagic { .. }
    ));
    let mut b = good.clone();
    *b.last_mut().unwrap() = 0;
    assert!(matches!(
        parse(&b).unwrap_err().kind,
        ErrorKind::BadMagic { .. }
    ));
    // The last track's points cut off: the footer's word is all that's left.
    let mut b = good[..good.len() - 16].to_vec();
    b.extend(FOOTER);
    assert!(parse(&b).is_err());
    // An unknown kind byte.
    let mut b = good.clone();
    b[48 + 8] = 0x22;
    assert!(matches!(
        parse(&b).unwrap_err().kind,
        ErrorKind::Unsupported { value: 0x22 }
    ));
}

#[test]
fn a_block_offset_past_the_data_or_backwards_refuses() {
    let a = block_bytes(&[T::C(0.0)]);
    let b = block_bytes(&[T::C(0.0)]);
    let mut bytes = stream_bytes(300, 1, &[a, b]);
    bytes[48..52].copy_from_slice(&0xFFFF_u32.to_be_bytes());
    assert!(matches!(
        parse(&bytes).unwrap_err().kind,
        ErrorKind::OutOfRange { .. }
    ));
    bytes[48..52].copy_from_slice(&0u32.to_be_bytes());
    assert!(parse(&bytes).is_err(), "block 1 at block 0's start");
}

#[test]
fn a_huge_count_refuses_without_allocating() {
    let mut b = one_block();
    b[32..36].copy_from_slice(&u32::MAX.to_be_bytes());
    b[36..40].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(parse(&b).is_err());
    let mut b = one_block();
    b[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(parse(&b).is_err());
}

/// Every prefix and every single-byte change of a good stream reads or
/// refuses, never panics.
#[test]
fn no_prefix_or_byte_change_panics() {
    let good = one_block();
    for n in 0..=good.len() {
        let _ = parse(&good[..n]);
    }
    for i in 0..good.len() {
        for v in [0x00, 0x01, 0x10, 0x7F, 0xFF] {
            let mut b = good.clone();
            b[i] = v;
            let _ = parse(&b);
        }
    }
}

/// The header's own refusals (the as-mouths review): a block count of 0 or
/// one that disagrees with its copy, a frame time that isn't a positive
/// number, a spline's degree outside 1-3, knots that run backwards, and a
/// stream too short for its header, each an error and never a panic.
#[test]
fn a_bad_header_or_spline_refuses() {
    let good = one_block();
    let with = |at: usize, word: [u8; 4]| {
        let mut b = good.clone();
        b[at..at + 4].copy_from_slice(&word);
        parse(&b)
    };
    assert!(matches!(
        with(32, 0u32.to_be_bytes()).unwrap_err().kind,
        ErrorKind::CountMismatch { .. }
    ));
    let mut b = good.clone();
    b[32..40].copy_from_slice(&[0; 8]);
    assert!(matches!(
        parse(&b).unwrap_err().kind,
        ErrorKind::OutOfRange { value: 0, .. }
    ));
    assert!(matches!(
        with(36, 2u32.to_be_bytes()).unwrap_err().kind,
        ErrorKind::CountMismatch {
            found: 2,
            expected: 1
        }
    ));
    for ft in [0.0f32, -1.0 / 24.0, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(
                with(28, ft.to_be_bytes()).unwrap_err().kind,
                ErrorKind::Unsupported { .. }
            ),
            "{ft}"
        );
    }
    // The first track's spline head: its degree byte, then its knots.
    let spline = 48 + 8 + 4;
    assert_eq!(good[spline + 2], 2, "the builder's quadratic");
    for degree in [0u8, 4, 0xFF] {
        let mut b = good.clone();
        b[spline + 2] = degree;
        assert!(
            matches!(
                parse(&b).unwrap_err().kind,
                ErrorKind::Unsupported { value } if value == u64::from(degree)
            ),
            "{degree}"
        );
    }
    let mut b = good.clone();
    b[spline + 3 + 3] = 30; // 0, 0, 0, 30, 20: backwards
    assert!(parse(&b).is_err());
    assert!(parse(&good[..40]).is_err(), "a header cut short");
    assert!(parse(&MAGIC).is_err(), "the magic alone");
}

/// A spline's point count is a u16 off the disc: one past what its block
/// holds refuses without reading past the slice (the as-mouths review).
#[test]
fn a_spline_counting_past_its_block_refuses() {
    let good = one_block();
    let spline = 48 + 8 + 4;
    for n in [5u16, 200, u16::MAX] {
        let mut b = good.clone();
        b[spline..spline + 2].copy_from_slice(&n.to_be_bytes());
        assert!(parse(&b).is_err(), "{n}");
    }
}
