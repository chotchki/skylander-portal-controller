//! An intro's video (openspec AB, design decision 7): the extract's MP4
//! demuxed and decoded by OpenH264 on a thread of its own, RGBA frames coming
//! back over a short channel, as the AB.7 spike measured it.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use crossbeam_channel::{Receiver, Sender, bounded};
use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;

/// The extract's intros are 1280x720; past this is a bad file, not a bigger intro.
pub const MAX_SIZE: (usize, usize) = (1920, 1080);

/// The longest intro is 12.3 MB; this bounds what one read holds.
const MAX_FILE: u64 = 256 << 20;

/// Frames decoded ahead: about 15 MB at 1280x720.
const AHEAD: usize = 4;

/// One access unit in Annex B, ready for OpenH264.
struct Packet {
    pts: f64,
    annexb: Vec<u8>,
}

struct Demuxed {
    width: usize,
    height: usize,
    packets: Vec<Packet>,
    /// Seconds: the last sample's end.
    end: f64,
}

/// MP4 (AVCC, length-prefixed NALs) to Annex B, with SPS/PPS ahead of each
/// sync sample.
fn demux(bytes: Vec<u8>) -> Result<Demuxed> {
    let size = bytes.len() as u64;
    let mut mp4 = mp4::Mp4Reader::read_header(Cursor::new(bytes), size)?;
    let (id, timescale, count, width, height, len_size, params) = {
        let track = mp4
            .tracks()
            .values()
            .find(|t| matches!(t.media_type(), Ok(mp4::MediaType::H264)))
            .context("no H.264 track")?;
        let avcc = &track
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .context("no avc1 box")?
            .avcc;
        let mut params = Vec::new();
        for nal in avcc
            .sequence_parameter_sets
            .iter()
            .chain(&avcc.picture_parameter_sets)
        {
            params.extend_from_slice(&[0, 0, 0, 1]);
            params.extend_from_slice(&nal.bytes);
        }
        (
            track.track_id(),
            f64::from(track.timescale()),
            track.sample_count(),
            usize::from(track.width()),
            usize::from(track.height()),
            usize::from(avcc.length_size_minus_one & 3) + 1,
            params,
        )
    };
    let (max_w, max_h) = MAX_SIZE;
    ensure!(
        (1..=max_w).contains(&width) && (1..=max_h).contains(&height),
        "{width}x{height} track, outside 1x1..{max_w}x{max_h}"
    );
    ensure!(timescale > 0.0, "the track's timescale is 0");
    let mut packets = Vec::with_capacity(count.min(1 << 16) as usize);
    let mut end = 0f64;
    for i in 1..=count {
        let s = mp4.read_sample(id, i)?.context("missing sample")?;
        let mut annexb = if s.is_sync {
            params.clone()
        } else {
            Vec::new()
        };
        let mut rest: &[u8] = &s.bytes;
        while rest.len() >= len_size {
            let n = rest[..len_size]
                .iter()
                .fold(0usize, |a, b| (a << 8) | usize::from(*b));
            rest = &rest[len_size..];
            ensure!(
                n <= rest.len(),
                "sample {i}: a NAL of {n} bytes overruns it"
            );
            annexb.extend_from_slice(&[0, 0, 0, 1]);
            annexb.extend_from_slice(&rest[..n]);
            rest = &rest[n..];
        }
        let start = s.start_time as f64 / timescale;
        end = end.max(start + f64::from(s.duration) / timescale);
        packets.push(Packet {
            pts: (s.start_time as f64 + f64::from(s.rendering_offset)) / timescale,
            annexb,
        });
    }
    ensure!(!packets.is_empty(), "the H.264 track has no samples");
    Ok(Demuxed {
        width,
        height,
        packets,
        end,
    })
}

/// What the decode thread sends: `Opened` first, then frames in order. The
/// channel closing after the last frame is the end; `Failed` ends it early.
#[derive(Debug)]
pub enum Feed {
    Opened {
        width: usize,
        height: usize,
        frames: usize,
        /// Seconds.
        end: f64,
    },
    Frame {
        pts: f64,
        rgba: Vec<u8>,
    },
    Failed(String),
}

/// Starts decoding `path` on its own thread. Dropping the receiver stops it at
/// its next frame.
pub fn spawn_decoder(path: PathBuf) -> Receiver<Feed> {
    let (tx, rx) = bounded(AHEAD);
    let spawned = std::thread::Builder::new()
        .name("intro-decode".into())
        .spawn({
            let tx = tx.clone();
            move || {
                if let Err(e) = decode(&path, &tx) {
                    let _ = tx.send(Feed::Failed(format!("{}: {e:#}", path.display())));
                }
            }
        });
    if let Err(e) = spawned {
        let _ = tx.try_send(Feed::Failed(format!(
            "couldn't start the decode thread: {e}"
        )));
    }
    rx
}

/// Ok also when the player hung up early.
fn decode(path: &Path, tx: &Sender<Feed>) -> Result<()> {
    let len = std::fs::metadata(path)?.len();
    ensure!(len <= MAX_FILE, "{len} bytes, past the {MAX_FILE}-byte cap");
    let video = demux(std::fs::read(path)?)?;
    let (w, h) = (video.width, video.height);
    let opened = Feed::Opened {
        width: w,
        height: h,
        frames: video.packets.len(),
        end: video.end,
    };
    if tx.send(opened).is_err() {
        return Ok(());
    }
    let mut decoder = Decoder::new()?;
    // Baseline has no reordering: frames come out in packet order.
    let mut pts = video.packets.iter().map(|p| p.pts);
    let mut send = |img: &DecodedYUV| -> Result<bool> {
        let mut rgba = vec![0u8; w * h * 4];
        to_rgba(img, (w, h), &mut rgba)?;
        let pts = pts.next().context("more frames than packets")?;
        Ok(tx.send(Feed::Frame { pts, rgba }).is_ok())
    };
    for packet in &video.packets {
        if let Some(img) = decoder.decode(&packet.annexb)?
            && !send(&img)?
        {
            return Ok(());
        }
    }
    for img in decoder.flush_remaining()? {
        if !send(&img)? {
            return Ok(());
        }
    }
    Ok(())
}

/// `write_rgba8` asserts the buffer fits the picture, so the picture is
/// checked against the track first: an error, not a panic.
fn to_rgba(img: &DecodedYUV, (w, h): (usize, usize), rgba: &mut [u8]) -> Result<()> {
    if img.dimensions() != (w, h) {
        let (dw, dh) = img.dimensions();
        bail!("decoded {dw}x{dh}, the track says {w}x{h}");
    }
    img.write_rgba8(rgba);
    Ok(())
}

/// An MP4 built in code with OpenH264's encoder, so tests need no ffmpeg and
/// no extract: `frames` frames at `fps`, frame `i` a flat grey of `i * 16`.
#[cfg(test)]
pub(crate) fn test_clip(width: usize, height: usize, frames: usize, fps: u32) -> Vec<u8> {
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;

    let mut encoder = Encoder::new().unwrap();
    let (mut sps, mut pps) = (Vec::new(), Vec::new());
    let mut samples = Vec::new();
    for i in 0..frames {
        let grey = (i * 16 % 256) as u8;
        let rgb = vec![grey; width * height * 3];
        let yuv =
            YUVBuffer::from_rgb8_source(openh264::formats::RgbSliceU8::new(&rgb, (width, height)));
        let stream = encoder.encode(&yuv).unwrap();
        // Annex B NALs to AVCC, parameter sets to the avcC box.
        let mut avcc = Vec::new();
        for l in 0..stream.num_layers() {
            let layer = stream.layer(l).unwrap();
            for n in 0..layer.nal_count() {
                let nal = layer.nal_unit(n).unwrap();
                let start = if nal.starts_with(&[0, 0, 0, 1]) { 4 } else { 3 };
                let body = &nal[start..];
                match body[0] & 0x1f {
                    7 => sps = body.to_vec(),
                    8 => pps = body.to_vec(),
                    _ => {
                        avcc.extend_from_slice(&(body.len() as u32).to_be_bytes());
                        avcc.extend_from_slice(body);
                    }
                }
            }
        }
        samples.push((i == 0, avcc));
    }
    let config = mp4::Mp4Config {
        major_brand: "isom".parse().unwrap(),
        minor_version: 512,
        compatible_brands: vec!["isom".parse().unwrap(), "avc1".parse().unwrap()],
        timescale: 1000,
    };
    let mut writer = mp4::Mp4Writer::write_start(Cursor::new(Vec::new()), &config).unwrap();
    writer
        .add_track(&mp4::TrackConfig {
            track_type: mp4::TrackType::Video,
            timescale: fps,
            language: "und".into(),
            media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
                width: width as u16,
                height: height as u16,
                seq_param_set: sps,
                pic_param_set: pps,
            }),
        })
        .unwrap();
    for (i, (sync, bytes)) in samples.into_iter().enumerate() {
        writer
            .write_sample(
                1,
                &mp4::Mp4Sample {
                    start_time: i as u64,
                    duration: 1,
                    rendering_offset: 0,
                    is_sync: sync,
                    bytes: bytes.into(),
                },
            )
            .unwrap();
    }
    writer.write_end().unwrap();
    writer.into_writer().into_inner()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn collect(rx: Receiver<Feed>) -> Vec<Feed> {
        let mut out = Vec::new();
        while let Ok(feed) = rx.recv_timeout(Duration::from_secs(10)) {
            out.push(feed);
        }
        out
    }

    #[test]
    fn a_clip_decodes_every_frame_in_order_with_its_time() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("clip.mp4");
        std::fs::write(&path, test_clip(64, 48, 5, 25)).unwrap();
        let feeds = collect(spawn_decoder(path));
        let Feed::Opened {
            width,
            height,
            frames,
            end,
        } = feeds[0]
        else {
            panic!("{:?}", feeds[0]);
        };
        assert_eq!((width, height, frames), (64, 48, 5));
        assert!((end - 0.2).abs() < 1e-9, "{end}");
        let frames: Vec<(f64, &[u8])> = feeds[1..]
            .iter()
            .map(|f| match f {
                Feed::Frame { pts, rgba } => (*pts, rgba.as_slice()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(frames.len(), 5);
        for (i, (pts, rgba)) in frames.iter().enumerate() {
            assert!((pts - i as f64 * 0.04).abs() < 1e-9, "{i}: {pts}");
            assert_eq!(rgba.len(), 64 * 48 * 4);
            // Lossy, but a flat grey stays near its level.
            let grey = (i * 16) as i32;
            let mid = &rgba[(24 * 64 + 32) * 4..][..4];
            assert!((i32::from(mid[0]) - grey).abs() <= 6, "{i}: {mid:?}");
            assert_eq!(mid[3], 255);
        }
    }

    #[test]
    fn a_missing_or_bad_file_fails_without_a_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let feeds = collect(spawn_decoder(tmp.path().join("gone.mp4")));
        assert!(matches!(&feeds[..], [Feed::Failed(_)]), "{feeds:?}");
        let junk = tmp.path().join("junk.mp4");
        std::fs::write(&junk, b"not an mp4 at all").unwrap();
        let feeds = collect(spawn_decoder(junk.clone()));
        match &feeds[..] {
            [Feed::Failed(why)] => assert!(why.contains("junk.mp4"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_oversized_track_is_refused_before_any_decode() {
        assert_eq!(demux(test_clip(16, 16, 1, 25)).unwrap().width, 16);
        let err = demux(test_clip(MAX_SIZE.0 + 16, 16, 1, 25)).err().unwrap();
        assert!(err.to_string().contains("outside"), "{err}");
    }
}
