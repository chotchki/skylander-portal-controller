//! MPEG audio frames as an FSB5 bank stores them (openspec AR.2), and the
//! MP3 they remux to.
//!
//! FMOD writes an MPEG sample's frames back to back with zero bytes after
//! some of them (Giants' banks: three after a 1,044-byte frame); a frame's
//! length is in its own header, so walking the frames and dropping the
//! zeros between them leaves a plain MPEG audio stream, which is all an
//! `.mp3` is: every decoder plays it, sample for sample the bank's, with no
//! re-encode.
//!
//! A frame header is 32 bits, big-endian: 11 sync bits, the version (MPEG-1,
//! 2 or 2.5), the layer (I, II or III), a protection bit, a bitrate index, a
//! rate index, the padding bit, a private bit, the channel mode and five
//! bits this walker doesn't use. A free-format or bad bitrate, a reserved
//! version, layer or rate, and anything but zeros between two frames are
//! refused by name.
//!
//! A sample's data often ends partway into one more frame (74,500 of the
//! disc's 108,959 banks, AR.2's census: 3 to 31 bytes, a header and the
//! start of its body). The bank's sample count stops before it, so the
//! walk ends there ([`Frames::cut`]) and the remux, which stops at the
//! bank's count, leaves it out.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use crate::{ErrorKind, FormatError};

const FORMAT: &str = "MPEG audio";
/// A frame header is one big-endian 32-bit word.
pub const HEADER_LEN: usize = 4;

/// MPEG version, as the header's two version bits name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Version {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}

/// Layer I, II or III.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    One,
    Two,
    Three,
}

/// The channel mode; every mode but mono carries two channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    Stereo,
    JointStereo,
    DualChannel,
    Mono,
}

impl Mode {
    pub fn channels(self) -> u32 {
        match self {
            Self::Mono => 1,
            _ => 2,
        }
    }
}

/// A frame header whose every field passed the standard's tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Header {
    pub version: Version,
    pub layer: Layer,
    /// Bits per second.
    pub bitrate: u32,
    pub rate: u32,
    pub padding: bool,
    pub mode: Mode,
}

/// kbit/s by bitrate index, for MPEG-1's layers I, II and III, then MPEG-2
/// and 2.5's layer I and its layers II and III (ISO 11172-3, 13818-3).
const BITRATES: [[u16; 15]; 5] = [
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

impl Header {
    /// The header at the start of `word`, or why it isn't one.
    pub fn decode(word: u32) -> Result<Self, &'static str> {
        if word >> 21 != 0x7FF {
            return Err("sync");
        }
        let version = match (word >> 19) & 3 {
            0 => Version::Mpeg25,
            2 => Version::Mpeg2,
            3 => Version::Mpeg1,
            _ => return Err("version"),
        };
        let layer = match (word >> 17) & 3 {
            1 => Layer::Three,
            2 => Layer::Two,
            3 => Layer::One,
            _ => return Err("layer"),
        };
        let index = ((word >> 12) & 0xF) as usize;
        if index == 0 {
            return Err("free-format bitrate");
        }
        if index == 15 {
            return Err("bitrate");
        }
        let table = match (version, layer) {
            (Version::Mpeg1, Layer::One) => 0,
            (Version::Mpeg1, Layer::Two) => 1,
            (Version::Mpeg1, Layer::Three) => 2,
            (_, Layer::One) => 3,
            _ => 4,
        };
        let bitrate = u32::from(BITRATES[table][index]) * 1000;
        let base = match (word >> 10) & 3 {
            0 => 44_100,
            1 => 48_000,
            2 => 32_000,
            _ => return Err("rate"),
        };
        let rate = match version {
            Version::Mpeg1 => base,
            Version::Mpeg2 => base / 2,
            Version::Mpeg25 => base / 4,
        };
        let mode = match (word >> 6) & 3 {
            0 => Mode::Stereo,
            1 => Mode::JointStereo,
            2 => Mode::DualChannel,
            _ => Mode::Mono,
        };
        Ok(Self {
            version,
            layer,
            bitrate,
            rate,
            padding: (word >> 9) & 1 != 0,
            mode,
        })
    }

    /// Samples per channel one frame carries.
    pub fn samples(&self) -> u32 {
        match (self.layer, self.version) {
            (Layer::One, _) => 384,
            (Layer::Two, _) | (Layer::Three, Version::Mpeg1) => 1152,
            (Layer::Three, _) => 576,
        }
    }

    /// The frame's length in bytes, header included.
    pub fn len(&self) -> usize {
        let pad = usize::from(self.padding);
        let (bitrate, rate) = (self.bitrate as usize, self.rate as usize);
        match (self.layer, self.version) {
            (Layer::One, _) => (12 * bitrate / rate + pad) * 4,
            (Layer::Two, _) | (Layer::Three, Version::Mpeg1) => 144 * bitrate / rate + pad,
            (Layer::Three, _) => 72 * bitrate / rate + pad,
        }
    }

    /// Never true: the smallest frame (MPEG-2.5 layer III, 8 kbit/s at
    /// 8 kHz) is 72 bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One frame: where it starts in the walked data, its header and its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub at: usize,
    pub header: Header,
    pub bytes: &'a [u8],
}

/// Walks `data`'s frames, skipping zero bytes before each and after the
/// last, and ending at a frame the data's end cuts short
/// ([`Frames::cut`]). Each item is a frame or the reason the walk stops
/// there; nothing follows an error.
pub fn frames(data: &[u8]) -> Frames<'_> {
    Frames {
        data,
        at: 0,
        done: false,
    }
}

pub struct Frames<'a> {
    data: &'a [u8],
    at: usize,
    done: bool,
}

impl Frames<'_> {
    /// The bytes after the last whole frame the walk ended on, zeros
    /// before them excluded: a frame the data's end cuts short.
    pub fn cut(&self) -> usize {
        if self.done {
            self.data.len() - self.at
        } else {
            0
        }
    }
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<Frame<'a>, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let zeros = self.data[self.at..].iter().take_while(|b| **b == 0).count();
        self.at += zeros;
        if self.at == self.data.len() {
            self.done = true;
            return None;
        }
        let at = self.at;
        let fail = |field: &'static str, kind: ErrorKind| {
            Some(Err(FormatError::new(FORMAT, field, at, kind)))
        };
        let Some(head) = self.data.get(at..at + HEADER_LEN) else {
            // Under a header's worth at the end: a cut frame.
            self.done = true;
            return None;
        };
        let word = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        let header = match Header::decode(word) {
            Ok(h) => h,
            Err(field) => {
                self.done = true;
                return fail(field, ErrorKind::Unsupported { value: word.into() });
            }
        };
        let len = header.len();
        let Some(bytes) = self.data.get(at..at + len) else {
            // A header the data's end cuts short.
            self.done = true;
            return None;
        };
        self.at += len;
        Some(Ok(Frame { at, header, bytes }))
    }
}

/// An MPEG sample remuxed: its frames back to back, and what they hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remux {
    pub bytes: Vec<u8>,
    pub frames: usize,
    /// Samples per channel over every frame kept.
    pub samples: u64,
    /// The zero bytes dropped before and between the frames kept.
    pub padding: usize,
    /// Every byte after the last frame kept, dropped: a frame the data's
    /// end cut short, or what follows the sample's count.
    pub cut: usize,
    /// The first frame's header: what the stream is.
    pub first: Header,
}

/// `data`'s frames with the zeros between them dropped, an `.mp3`, up to
/// the first that brings them to `samples` (the bank's count). What follows
/// that frame isn't the sample's: a frame the data's end cut short (74,500
/// of the disc's banks, AR.2), the start of one cut to a header byte or two
/// and the zeros after it (23), or one whole frame more (5). A stream whose
/// frames disagree on version, layer, rate or mode is refused at the first
/// that differs, as is one with no frame at all.
pub fn remux(data: &[u8], samples: u32) -> Result<Remux, FormatError> {
    let mut bytes = Vec::with_capacity(data.len());
    let mut frames_n = 0;
    let mut held = 0u64;
    let mut end = 0;
    let mut first: Option<Header> = None;
    let mut walk = frames(data);
    while held < u64::from(samples) {
        let Some(frame) = walk.next() else { break };
        let f = frame?;
        if let Some(h) = first
            && (h.version, h.layer, h.rate, h.mode)
                != (
                    f.header.version,
                    f.header.layer,
                    f.header.rate,
                    f.header.mode,
                )
        {
            return Err(FormatError::new(
                FORMAT,
                "frame_header",
                f.at,
                ErrorKind::Unsupported {
                    value: u32::from_be_bytes([f.bytes[0], f.bytes[1], f.bytes[2], f.bytes[3]])
                        .into(),
                },
            ));
        }
        first.get_or_insert(f.header);
        bytes.extend_from_slice(f.bytes);
        frames_n += 1;
        held += u64::from(f.header.samples());
        end = f.at + f.bytes.len();
    }
    let first = first.ok_or_else(|| FormatError::new(FORMAT, "frame", 0, ErrorKind::Missing))?;
    Ok(Remux {
        padding: end - bytes.len(),
        cut: data.len() - end,
        bytes,
        frames: frames_n,
        samples: held,
        first,
    })
}

#[cfg(test)]
mod tests;
