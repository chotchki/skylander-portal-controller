//! A Bink 1 movie's header (openspec AS.8): its frames, size and rate and
//! each audio track's sample rate, flags and id, which is what picks a
//! movie's voice track before ffmpeg converts it.
//!
//! Layout, little-endian throughout: `BIK` and a revision byte; the file's
//! size less 8; the frame count; the largest frame's bytes; the frame count
//! again; width and height; the rate as numerator and denominator; the
//! video flags; the audio track count; then per track a word the converter
//! doesn't need, then per track its sample rate (u16) and flags (u16), then
//! per track its id (u32). The frame index follows, unread here.
//!
//! On the Giants disc (AB.5's census, AS.8's read): every movie is
//! revision `i`; the 48 intros, fmv12 and the two logos carry one stereo
//! track with id 0; fmv01-fmv04 and fmv06-fmv11 carry 13, 12 mono and one
//! stereo, the mono tracks' ids each a language pack's language id plus one
//! (1, 5-10, 15, 18, 20, 22, 23 against the packs' 0, 4-9, 14, 17, 19, 21,
//! 22) and the stereo one's 0.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

use crate::reader::Reader;
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "Bink";
pub const MAGIC: &[u8; 3] = b"BIK";
/// Bytes before the per-track tables.
pub const HEADER_LEN: usize = 44;
/// More tracks than a header may name: the disc's most is 13.
pub const MAX_TRACKS: u32 = 256;
/// A track flag: two channels (`0x7000` on the disc's stereo tracks,
/// `0x5000` on its mono ones).
pub const STEREO: u16 = 0x2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrack {
    pub sample_rate: u16,
    pub flags: u16,
    pub id: u32,
}

impl AudioTrack {
    pub fn stereo(&self) -> bool {
        self.flags & STEREO != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The byte after `BIK` (`b'i'` on the disc).
    pub revision: u8,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub rate: (u32, u32),
    /// In the file's order, which is ffmpeg's stream order after the video.
    pub tracks: Vec<AudioTrack>,
}

impl Header {
    /// Reads the header off the start of a Bink 1 file. Refused: a bad
    /// magic, a count past [`MAX_TRACKS`] and a header cut short.
    pub fn read(data: &[u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, data);
        r.magic(0, MAGIC, "signature")?;
        let revision = r.bytes(3, 1, "revision")?[0];
        let count = r.u32_le(40, "audio tracks")?;
        if count > MAX_TRACKS {
            return Err(r.err(
                "audio tracks",
                40,
                ErrorKind::OutOfRange {
                    value: count.into(),
                    limit: MAX_TRACKS.into(),
                },
            ));
        }
        let n = count as usize;
        let rates = HEADER_LEN + 4 * n;
        let ids = rates + 4 * n;
        let tracks = (0..n)
            .map(|i| {
                Ok(AudioTrack {
                    sample_rate: r.u16_le(rates + 4 * i, "sample rate")?,
                    flags: r.u16_le(rates + 4 * i + 2, "track flags")?,
                    id: r.u32_le(ids + 4 * i, "track id")?,
                })
            })
            .collect::<Result<Vec<_>, FormatError>>()?;
        Ok(Self {
            revision,
            frames: r.u32_le(8, "frames")?,
            width: r.u32_le(20, "width")?,
            height: r.u32_le(24, "height")?,
            rate: (r.u32_le(28, "rate")?, r.u32_le(32, "rate divider")?),
            tracks,
        })
    }

    /// The track whose id is `id`, by its place in the file.
    pub fn track(&self, id: u32) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }
}
