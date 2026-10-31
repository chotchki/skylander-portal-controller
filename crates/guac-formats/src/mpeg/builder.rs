//! Synthetic MPEG audio frames for tests and fuzz seeds (`test-fixtures`):
//! a valid header and a body of a counting pattern, so a frame's bytes are
//! told apart from its neighbours'. Nothing here decodes to sound.

use super::{Header, Layer, Mode, Version};

/// The header word for `h`; `h`'s bitrate and rate must be in the tables.
pub fn header_word(h: &Header) -> u32 {
    let version = match h.version {
        Version::Mpeg25 => 0,
        Version::Mpeg2 => 2,
        Version::Mpeg1 => 3,
    };
    let layer = match h.layer {
        Layer::Three => 1,
        Layer::Two => 2,
        Layer::One => 3,
    };
    let base = match h.version {
        Version::Mpeg1 => h.rate,
        Version::Mpeg2 => h.rate * 2,
        Version::Mpeg25 => h.rate * 4,
    };
    let rate = match base {
        44_100 => 0,
        48_000 => 1,
        32_000 => 2,
        other => panic!("no rate index for {other}"),
    };
    let index = (1..15u32)
        .find(|&i| {
            let word = 0xFFE0_0000 | version << 19 | layer << 17 | i << 12 | rate << 10;
            Header::decode(word).is_ok_and(|d| d.bitrate == h.bitrate)
        })
        .unwrap_or_else(|| panic!("no bitrate index for {}", h.bitrate));
    let mode = match h.mode {
        Mode::Stereo => 0,
        Mode::JointStereo => 1,
        Mode::DualChannel => 2,
        Mode::Mono => 3,
    };
    // The protection bit set: no CRC follows the header.
    0xFFE0_0000
        | version << 19
        | layer << 17
        | 1 << 16
        | index << 12
        | rate << 10
        | u32::from(h.padding) << 9
        | mode << 6
}

/// One frame of `h`, its body `fill + i` for its byte `i`, never zero, so a
/// walker can't mistake a body byte for padding.
pub fn frame(h: &Header, fill: u8) -> Vec<u8> {
    let mut out = header_word(h).to_be_bytes().to_vec();
    out.extend((0..h.len() - out.len()).map(|i| (fill.wrapping_add(i as u8)) | 1));
    out
}

/// Giants' frame: MPEG-1 layer III at 320 kbit/s, 44.1 kHz, joint stereo.
pub fn giants(padding: bool) -> Header {
    Header {
        version: Version::Mpeg1,
        layer: Layer::Three,
        bitrate: 320_000,
        rate: 44_100,
        padding,
        mode: Mode::JointStereo,
    }
}

/// `n` Giants frames with `gap` zero bytes after every other one: what a
/// bank's MPEG sample looks like.
pub fn stream(n: usize, gap: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        out.extend(frame(&giants(i % 3 == 1), i as u8));
        if i % 2 == 0 {
            out.extend(std::iter::repeat_n(0, gap));
        }
    }
    out
}
