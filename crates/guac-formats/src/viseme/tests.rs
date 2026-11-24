use super::*;

/// One track as the builder writes it.
enum T {
    C(f32),
    /// degree, knots, low, high, raw points.
    S(u8, Vec<u8>, f32, f32, Vec<u8>),
}

fn pad(b: &mut Vec<u8>) {
    while b.len() % 4 != 0 {
        b.push(0);
    }
}

/// A block's bytes, laid out from a 4-aligned start.
fn block_bytes(tracks: &[T]) -> Vec<u8> {
    let mut b: Vec<u8> = tracks
        .iter()
        .map(|t| match t {
            T::C(_) => CONSTANT,
            T::S(..) => SPLINE,
        })
        .collect();
    pad(&mut b);
    for t in tracks {
        match t {
            T::C(v) => b.extend(v.to_be_bytes()),
            T::S(d, knots, lo, hi, points) => {
                let n = points.len() as u16 - 1;
                b.extend(n.to_be_bytes());
                b.push(*d);
                b.extend(knots);
                pad(&mut b);
                b.extend(lo.to_be_bytes());
                b.extend(hi.to_be_bytes());
                b.extend(points);
                pad(&mut b);
            }
        }
    }
    b
}

/// A whole stream of `frames` frames at 1/24 s, `blocks` its blocks.
fn stream_bytes(frames: u32, tracks: u32, blocks: &[Vec<u8>]) -> Vec<u8> {
    let nb = blocks.len() as u32;
    let mut b = MAGIC.to_vec();
    let ft = 1.0f32 / 24.0;
    for w in [frames, 16, 1, tracks] {
        b.extend(w.to_be_bytes());
    }
    for f in [(frames - 1) as f32 * ft, 255.0 * ft, ft] {
        b.extend(f.to_be_bytes());
    }
    for w in [nb, nb, 0xDEAD, 0] {
        b.extend(w.to_be_bytes());
    }
    let mut starts = Vec::new();
    let mut at = 0u32;
    for bl in blocks {
        starts.push(at);
        at += bl.len() as u32;
    }
    let mut words: Vec<u32> = starts[1..].to_vec();
    words.extend(std::iter::repeat_n(16, nb as usize));
    words.push(0);
    for w in words {
        b.extend(w.to_be_bytes());
    }
    for bl in blocks {
        b.extend(bl);
    }
    b.extend([0; 4]);
    b.extend(FOOTER);
    b
}

fn one_block() -> Vec<u8> {
    stream_bytes(
        48,
        3,
        &[block_bytes(&[
            T::S(
                2,
                vec![0, 0, 0, 10, 20, 47, 47, 47],
                0.0,
                0.8,
                vec![0, 255, 128, 0, 0],
            ),
            T::C(0.0),
            T::S(1, vec![0, 0, 47, 47], -0.5, 0.5, vec![0, 255]),
        ])],
    )
}

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
