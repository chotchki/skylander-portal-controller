//! A sound's audio as the TV plays it (openspec AR.9, AR.11): the extract's
//! MP3 decoded by rodio, bevy_audio's own decoder, put out in stereo with a
//! left and a right gain the TV's frame sets as the sound's actor moves.
//!
//! A looping sound loops its frames, not its samples ([`Cycle`]): the
//! decoder is fed the recording's frames again and again and carries its
//! state over the wrap. AR.11 measured why: the disc's music is encoded for
//! that loop (its first frames lean on the last, so a fresh decode of them
//! starts from silence, the first 277 frames of `mu_213_explore` under
//! 1e-4), and decoded on through the wrap each of chapter 1's four beds
//! steps across the seam by 0.04 to 0.83 of a 99th-percentile step of its
//! own music, while a loop of the decoded samples steps 2.25 to 4.22 times
//! that and then holds 87 to 348 frames of silence.
//!
//! Each play is a [`Voice`] asset of its own (its bytes shared with every
//! other play of the file), so its gains are its alone. The gains glide to a
//! new setting over a few milliseconds rather than stepping, so a moving
//! actor doesn't crackle.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;

/// How many times a loop's frames repeat before its reader ends: for a
/// 129 s bed, about 98 days of play.
pub const PASSES: u64 = 1 << 16;

/// A recording's frames, again and again: what a looping play's decoder
/// reads, so its loop is the frames decoded on through the wrap (the module
/// docs). It seeks within its [`PASSES`] repeats as one long stream.
#[derive(Debug, Clone)]
pub struct Cycle {
    bytes: Arc<[u8]>,
    at: u64,
}

impl Cycle {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes, at: 0 }
    }

    /// The stream's length: the recording's bytes [`PASSES`] times.
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64 * PASSES
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl std::io::Read for Cycle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.bytes.len() as u64;
        if len == 0 || self.at >= self.len() {
            return Ok(0);
        }
        let from = (self.at % len) as usize;
        let n = buf.len().min(self.bytes.len() - from);
        buf[..n].copy_from_slice(&self.bytes[from..from + n]);
        self.at += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Cycle {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            std::io::SeekFrom::Start(n) => Some(n),
            std::io::SeekFrom::End(d) => self.len().checked_add_signed(d),
            std::io::SeekFrom::Current(d) => self.at.checked_add_signed(d),
        };
        match at {
            Some(at) => {
                self.at = at;
                Ok(at)
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a seek before the stream's start",
            )),
        }
    }
}

/// rodio's MP3 decoder (bevy_audio's own copy, Cargo.lock) over a once
/// play's bytes or a loop's [`Cycle`], built so a file that won't open is an
/// error, not a panic.
enum Inner {
    Once(rodio::Decoder<Cursor<Arc<[u8]>>>),
    Looped(rodio::Decoder<Cycle>),
    /// Samples given outright, interleaved, with their channels and rate:
    /// the decoder's tests.
    #[cfg(test)]
    Given(std::vec::IntoIter<Sample>, u16, SampleRate),
}

impl Inner {
    fn open(bytes: &Arc<[u8]>, looped: bool) -> Option<Self> {
        if looped {
            let cycle = Cycle::new(bytes.clone());
            rodio::Decoder::builder()
                .with_byte_len(cycle.len())
                .with_data(cycle)
                .with_gapless(false)
                .build()
                .ok()
                .map(Inner::Looped)
        } else {
            rodio::Decoder::builder()
                .with_byte_len(bytes.len() as u64)
                .with_data(Cursor::new(bytes.clone()))
                .build()
                .ok()
                .map(Inner::Once)
        }
    }

    fn next(&mut self) -> Option<Sample> {
        match self {
            Inner::Once(d) => d.next(),
            Inner::Looped(d) => d.next(),
            #[cfg(test)]
            Inner::Given(s, ..) => s.next(),
        }
    }

    fn channels(&self) -> u16 {
        match self {
            Inner::Once(d) => d.channels().get(),
            Inner::Looped(d) => d.channels().get(),
            #[cfg(test)]
            Inner::Given(_, c, _) => *c,
        }
    }

    fn sample_rate(&self) -> SampleRate {
        match self {
            Inner::Once(d) => d.sample_rate(),
            Inner::Looped(d) => d.sample_rate(),
            #[cfg(test)]
            Inner::Given(_, _, r) => *r,
        }
    }
}

/// A play's left and right gains, each linear, as the TV's frame last set
/// them; the audio thread reads them.
#[derive(Debug)]
pub struct Gains([AtomicU32; 2]);

impl Gains {
    pub fn new(left: f32, right: f32) -> Arc<Self> {
        Arc::new(Self([
            AtomicU32::new(left.to_bits()),
            AtomicU32::new(right.to_bits()),
        ]))
    }

    pub fn set(&self, left: f32, right: f32) {
        self.0[0].store(left.to_bits(), Ordering::Relaxed);
        self.0[1].store(right.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.0[0].load(Ordering::Relaxed)),
            f32::from_bits(self.0[1].load(Ordering::Relaxed)),
        )
    }
}

/// One play's audio: the file's bytes, its gains, and whether and where it
/// loops.
#[derive(Asset, TypePath, Debug, Clone)]
pub struct Voice {
    pub bytes: Arc<[u8]>,
    pub gains: Arc<Gains>,
    /// Whether it loops its frames for good ([`Cycle`]); else it plays
    /// them once.
    pub looped: bool,
}

impl Decodable for Voice {
    type Decoder = VoiceDecoder;

    fn decoder(&self) -> VoiceDecoder {
        VoiceDecoder::new(self.bytes.clone(), self.gains.clone(), self.looped)
    }
}

/// How fast a gain glides to its setting, a share of the way each frame:
/// about 10 ms to settle at 48 kHz.
const GLIDE: f32 = 0.002;

/// [`Voice`]'s decoder: two channels out whatever went in, each by its gain.
pub struct VoiceDecoder {
    inner: Option<Inner>,
    channels_in: u16,
    rate: SampleRate,
    gains: Arc<Gains>,
    now: (f32, f32),
    /// The right sample of the frame whose left went out last.
    right: Option<Sample>,
}

/// Whether `bytes` open as audio the decoder reads: the TV's check before
/// it plays a file.
pub fn opens(bytes: &Arc<[u8]>) -> bool {
    Inner::open(bytes, false).is_some()
}

impl VoiceDecoder {
    pub fn new(bytes: Arc<[u8]>, gains: Arc<Gains>, looped: bool) -> Self {
        Self::over(Inner::open(&bytes, looped), gains)
    }

    fn over(inner: Option<Inner>, gains: Arc<Gains>) -> Self {
        let (channels_in, rate) = inner
            .as_ref()
            .map_or((1, SampleRate::new(48_000).unwrap()), |d| {
                (d.channels(), d.sample_rate())
            });
        let now = gains.get();
        Self {
            inner,
            channels_in: channels_in.max(1),
            rate,
            gains,
            now,
            right: None,
        }
    }

    /// The next frame in: its left and right samples, a mono one on both.
    fn frame_in(&mut self) -> Option<(Sample, Sample)> {
        let channels = self.channels_in;
        let inner = self.inner.as_mut()?;
        let left = inner.next()?;
        let right = if channels >= 2 { inner.next()? } else { left };
        // No recording on the disc has more than two (AR.2's census).
        for _ in 2..channels {
            inner.next()?;
        }
        Some((left, right))
    }
}

impl Iterator for VoiceDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if let Some(r) = self.right.take() {
            return Some(r);
        }
        let (l, r) = self.frame_in()?;
        let (tl, tr) = self.gains.get();
        self.now.0 += (tl - self.now.0) * GLIDE;
        self.now.1 += (tr - self.now.1) * GLIDE;
        self.right = Some(r * self.now.1 as Sample);
        Some(l * self.now.0 as Sample)
    }
}

impl Source for VoiceDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A play's left and right gains for `pan` (-1 left to 1 right): the side
/// it's on at full, the other side down to [`FAR_SIDE`] at a full turn
/// (**hypothesis**: the original's 3D pan isn't read; the oracle runs
/// silent), each times `gain`.
pub fn stereo(gain: f32, pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    let left = 1.0 - (1.0 - FAR_SIDE) * p.max(0.0);
    let right = 1.0 - (1.0 - FAR_SIDE) * (-p).max(0.0);
    (gain * left, gain * right)
}

/// What the far side keeps of a sound panned all the way over.
pub const FAR_SIDE: f32 = 0.3;

/// Fixtures for the tests of what plays a voice, here and in the TV.
#[doc(hidden)]
pub mod tests_support {
    /// MPEG-1 Layer III frames of silence, 128 kbit/s at 44.1 kHz, mono:
    /// 1,152 samples each.
    pub fn silent_mp3(frames: usize) -> Vec<u8> {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0xC0]);
        frame.repeat(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pan_turns_the_far_side_down_and_keeps_the_near_one() {
        assert_eq!(stereo(0.5, 0.0), (0.5, 0.5));
        assert_eq!(stereo(1.0, 1.0), (FAR_SIDE, 1.0));
        assert_eq!(stereo(1.0, -1.0), (1.0, FAR_SIDE));
        assert_eq!(stereo(2.0, 4.0), (2.0 * FAR_SIDE, 2.0));
        let (l, r) = stereo(1.0, 0.5);
        assert!((l - 0.65).abs() < 1e-6 && r == 1.0, "{l} {r}");
    }

    use super::tests_support::silent_mp3;

    fn given(samples: &[f32], channels: u16, gains: Arc<Gains>) -> VoiceDecoder {
        let s: Vec<Sample> = samples.iter().map(|&v| v as Sample).collect();
        VoiceDecoder::over(
            Some(Inner::Given(
                s.into_iter(),
                channels,
                SampleRate::new(48_000).unwrap(),
            )),
            gains,
        )
    }

    /// Mono goes out on both sides, stereo side for side, a third channel
    /// dropped; each side by its gain, held from the start.
    #[test]
    fn a_voice_puts_two_channels_out_each_by_its_gain() {
        let out: Vec<Sample> = given(&[0.5, -0.5], 1, Gains::new(1.0, 0.5)).collect();
        assert_eq!(out, [0.5, 0.25, -0.5, -0.25]);
        let out: Vec<Sample> = given(&[0.5, 0.25, 1.0, -1.0], 2, Gains::new(0.5, 1.0)).collect();
        assert_eq!(out, [0.25, 0.25, 0.5, -1.0]);
        let out: Vec<Sample> =
            given(&[0.5, 0.25, 0.9, 1.0, -1.0, 0.9], 3, Gains::new(1.0, 1.0)).collect();
        assert_eq!(out, [0.5, 0.25, 1.0, -1.0]);
        let d = given(&[0.0], 1, Gains::new(1.0, 1.0));
        assert_eq!(d.channels().get(), 2);
        assert_eq!(d.sample_rate().get(), 48_000);
    }

    /// A gain set mid-play glides there: a share of the way each frame,
    /// never past it.
    #[test]
    fn a_gain_glides_to_its_new_setting() {
        let gains = Gains::new(1.0, 1.0);
        let mut d = given(&[1.0; 4000], 1, gains.clone());
        assert_eq!((d.next(), d.next()), (Some(1.0), Some(1.0)));
        gains.set(0.0, 1.0);
        let left: Vec<Sample> = (0..3998).map(|_| d.next().unwrap()).step_by(2).collect();
        assert!(left[0] < 1.0 && left[0] > 0.99, "{}", left[0]);
        assert!(left.windows(2).all(|w| w[1] <= w[0] && w[1] >= 0.0));
        assert!(left[left.len() - 1] < 0.05, "{}", left[left.len() - 1]);
    }

    /// A once voice ends with its recording; a looping one plays its frames
    /// on past it, and on.
    #[test]
    fn a_looping_voice_plays_on_past_its_recording() {
        let bytes: Arc<[u8]> = Arc::from(silent_mp3(20));
        assert!(opens(&bytes));
        assert!(!opens(&Arc::from(&b"not an mp3"[..])));
        let once = VoiceDecoder::new(bytes.clone(), Gains::new(1.0, 1.0), false).count();
        assert!(once > 0 && once <= 20 * 1152 * 2, "{once}");
        let looped = VoiceDecoder::new(bytes, Gains::new(1.0, 1.0), true)
            .take(3 * once)
            .count();
        assert_eq!(looped, 3 * once);
    }

    /// The loop's reader: the bytes again and again, read across the wrap,
    /// a seek landing in its pass, its length the passes'.
    #[test]
    fn a_cycle_reads_its_bytes_again_and_again() {
        use std::io::{Read, Seek, SeekFrom};
        let mut c = Cycle::new(Arc::from(&b"abcde"[..]));
        assert_eq!(c.len(), 5 * PASSES);
        let mut buf = [0u8; 4];
        // A read stops at a pass's end; the next goes on from its start.
        let mut got = Vec::new();
        for _ in 0..4 {
            let n = c.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, b"abcdeabcde");
        assert_eq!(c.seek(SeekFrom::Start(13)).unwrap(), 13);
        let n = c.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"de");
        assert_eq!(c.seek(SeekFrom::End(-1)).unwrap(), 5 * PASSES - 1);
        assert_eq!(c.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'e');
        assert_eq!(c.read(&mut buf).unwrap(), 0, "past its passes");
        assert!(c.seek(SeekFrom::Current(-(6 * PASSES as i64))).is_err());
        assert_eq!(Cycle::new(Arc::from(&b""[..])).read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn gains_read_back_as_set() {
        let g = Gains::new(0.25, 0.75);
        assert_eq!(g.get(), (0.25, 0.75));
        g.set(1.0, 0.0);
        assert_eq!(g.get(), (1.0, 0.0));
    }
}
