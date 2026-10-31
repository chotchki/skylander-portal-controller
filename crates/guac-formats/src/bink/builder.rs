//! Bink 1 headers built in code, for the tests and the fuzz seeds: never a
//! disc file.

use super::{AudioTrack, HEADER_LEN, MAGIC};

/// A header with `tracks`, 1280 x 720 at 30 frames a second, and a frame
/// index's worth of zeros after it.
pub fn header(frames: u32, tracks: &[AudioTrack]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + 12 * tracks.len() + 64);
    out.extend_from_slice(MAGIC);
    out.push(b'i');
    for word in [0, frames, 0x1000, frames, 1280, 720, 30, 1, 0] {
        out.extend_from_slice(&u32::to_le_bytes(word));
    }
    out.extend_from_slice(&(tracks.len() as u32).to_le_bytes());
    for _ in tracks {
        out.extend_from_slice(&0x0001_0e00u32.to_le_bytes());
    }
    for t in tracks {
        out.extend_from_slice(&t.sample_rate.to_le_bytes());
        out.extend_from_slice(&t.flags.to_le_bytes());
    }
    for t in tracks {
        out.extend_from_slice(&t.id.to_le_bytes());
    }
    out.extend_from_slice(&[0; 64]);
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out
}

/// One mono track at 44.1 kHz.
pub fn mono(id: u32) -> AudioTrack {
    AudioTrack {
        sample_rate: 44_100,
        flags: 0x5000,
        id,
    }
}

/// One stereo track at 44.1 kHz.
pub fn stereo(id: u32) -> AudioTrack {
    AudioTrack {
        sample_rate: 44_100,
        flags: 0x7000,
        id,
    }
}

/// A cinematic laid out as the disc's ten with 13 tracks: 12 mono voices
/// (each a language id plus one) and the stereo bed, id 0, last.
pub fn cinematic() -> Vec<u8> {
    let mut tracks: Vec<AudioTrack> = [1, 5, 6, 7, 8, 9, 10, 15, 18, 20, 22, 23]
        .into_iter()
        .map(mono)
        .collect();
    tracks.push(stereo(0));
    header(7643, &tracks)
}

/// Seeds for the property test and the fuzz corpus.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![cinematic(), header(100, &[stereo(0)]), header(1, &[])]
}
