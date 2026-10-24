//! An MP4 built in code, from video.rs; the intro player's tests stand on it
//! too.

use std::io::Cursor;

/// An MP4 built in code with OpenH264's encoder, so tests need no ffmpeg and
/// no extract: `frames` frames at `fps`, frame `i` a flat grey of `i * 16`.
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
