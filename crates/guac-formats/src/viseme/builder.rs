//! Viseme streams built in code, for the tests and the fuzz seeds: never a
//! disc file (openspec AS.820).

use super::{CONSTANT, FOOTER, MAGIC, SPLINE};

/// One track as [`block_bytes`] writes it.
pub enum T {
    C(f32),
    /// degree, knots, low, high, raw points.
    S(u8, Vec<u8>, f32, f32, Vec<u8>),
}

fn pad(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

/// A block's bytes, laid out from a 4-aligned start.
pub fn block_bytes(tracks: &[T]) -> Vec<u8> {
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
pub fn stream_bytes(frames: u32, tracks: u32, blocks: &[Vec<u8>]) -> Vec<u8> {
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

/// One block of three tracks: a quadratic, a constant and a linear one.
pub fn one_block() -> Vec<u8> {
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

/// Two blocks of one track each, the second starting where its word says.
pub fn two_blocks() -> Vec<u8> {
    let a = block_bytes(&[T::S(1, vec![0, 0, 255, 255], 0.0, 1.0, vec![255, 0])]);
    let b = block_bytes(&[T::C(0.25)]);
    stream_bytes(300, 1, &[a, b])
}

/// Well-formed streams for the property test and the fuzz target.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![one_block(), two_blocks()]
}
