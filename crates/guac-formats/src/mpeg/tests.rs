use super::builder::{frame, giants, header_word, stream};
use super::*;

#[test]
fn giants_frame_is_1044_bytes_or_1045_padded_and_carries_1152_samples() {
    let h = giants(false);
    assert_eq!((h.len(), h.samples()), (1044, 1152));
    assert_eq!(giants(true).len(), 1045);
    assert_eq!(Header::decode(header_word(&h)), Ok(h));
    let at_48 = Header { rate: 48_000, ..h };
    assert_eq!(at_48.len(), 960);
    let at_32 = Header { rate: 32_000, ..h };
    assert_eq!(at_32.len(), 1440);
}

#[test]
fn every_version_and_layer_decodes_its_own_lengths() {
    let cases = [
        (Version::Mpeg1, Layer::One, 448_000, 48_000, 384, 448),
        (Version::Mpeg1, Layer::Two, 384_000, 32_000, 1152, 1728),
        (Version::Mpeg2, Layer::Three, 160_000, 24_000, 576, 480),
        (Version::Mpeg25, Layer::Three, 8_000, 8_000, 576, 72),
        (Version::Mpeg2, Layer::One, 256_000, 16_000, 384, 768),
    ];
    for (version, layer, bitrate, rate, samples, len) in cases {
        let h = Header {
            version,
            layer,
            bitrate,
            rate,
            padding: false,
            mode: Mode::Mono,
        };
        assert_eq!(
            Header::decode(header_word(&h)),
            Ok(h),
            "{version:?} {layer:?}"
        );
        assert_eq!(
            (h.samples(), h.len()),
            (samples, len),
            "{version:?} {layer:?}"
        );
        assert_eq!(h.mode.channels(), 1);
    }
}

#[test]
fn reserved_and_free_format_headers_are_refused_by_name() {
    let good = header_word(&giants(false));
    assert_eq!(Header::decode(good & !0x0020_0000), Err("sync"));
    assert_eq!(Header::decode(good & !(3 << 19) | 1 << 19), Err("version"));
    assert_eq!(Header::decode(good & !(3 << 17)), Err("layer"));
    assert_eq!(
        Header::decode(good & !(0xF << 12)),
        Err("free-format bitrate")
    );
    assert_eq!(Header::decode(good | 0xF << 12), Err("bitrate"));
    assert_eq!(Header::decode(good | 3 << 10), Err("rate"));
}

#[test]
fn a_stream_with_padding_remuxes_to_its_frames_alone() {
    let data = stream(7, 3);
    let r = remux(&data, 7 * 1152).unwrap();
    let expected: Vec<u8> = (0..7)
        .flat_map(|i| frame(&giants(i % 3 == 1), i as u8))
        .collect();
    assert_eq!(r.bytes, expected);
    assert_eq!((r.frames, r.samples), (7, 7 * 1152));
    assert_eq!(r.padding, 3 * 3, "after frames 0, 2 and 4");
    assert_eq!(r.cut, 3, "the zeros after the last frame");
    assert_eq!(r.first, giants(false));
    // Leading zeros are padding too.
    let mut led = vec![0; 5];
    led.extend(&data);
    assert_eq!(remux(&led, 7 * 1152).unwrap().bytes, expected);
}

#[test]
fn frames_name_where_each_starts() {
    let data = stream(3, 3);
    let at: Vec<usize> = frames(&data).map(|f| f.unwrap().at).collect();
    assert_eq!(at, [0, 1044 + 3, 1044 + 3 + 1045]);
}

#[test]
fn non_zero_bytes_between_frames_are_refused_where_they_are() {
    let mut data = stream(3, 3);
    data[1044 + 1] = 0x17;
    let e = remux(&data, 3 * 1152).unwrap_err();
    assert_eq!((e.field, e.offset), ("sync", 1044 + 1));
}

#[test]
fn a_last_frame_the_end_cuts_short_is_left_out_and_counted() {
    // A header and 9 bytes of its body after two whole frames (1,044 and a
    // padded 1,045).
    let mut data = stream(3, 0);
    let whole = 1044 + 1045;
    data.truncate(whole + 13);
    let r = remux(&data, 2 * 1152).unwrap();
    assert_eq!(
        (r.frames, r.samples, r.cut, r.padding),
        (2, 2 * 1152, 13, 0)
    );
    assert_eq!(r.bytes, data[..whole]);
    // The walk alone ends at it too.
    let mut walk = frames(&data);
    assert_eq!(walk.by_ref().filter(Result::is_ok).count(), 2);
    assert_eq!(walk.cut(), 13);
    // Zeros, then three bytes of a header.
    let mut short = stream(1, 0);
    short.extend([0, 0, 0xFF, 0xFB, 0x90]);
    let mut walk = frames(&short);
    assert_eq!(walk.by_ref().filter(Result::is_ok).count(), 1);
    assert_eq!(walk.cut(), 3);
    // A walk that ends whole cuts nothing.
    let mut walk = frames(&short[..1044 + 2]);
    assert!(walk.by_ref().all(|f| f.is_ok()));
    assert_eq!(walk.cut(), 0);
}

#[test]
fn the_remux_stops_at_the_banks_count() {
    // One whole frame past the count: left out.
    let data = stream(3, 0);
    let r = remux(&data, 2 * 1152).unwrap();
    assert_eq!((r.frames, r.samples, r.cut), (2, 2 * 1152, 1044));
    assert_eq!(r.bytes, data[..1044 + 1045]);
    // A header's first two bytes and zeros after the count: left out, never
    // read as a frame.
    let mut tail = stream(2, 0);
    tail.extend([0xFF, 0xF3, 0, 0, 0, 0, 0, 0]);
    let r = remux(&tail, 2 * 1152).unwrap();
    assert_eq!((r.frames, r.cut), (2, 8));
    // The same bytes before the count: refused where they are.
    let e = remux(&tail, 3 * 1152).unwrap_err();
    assert_eq!((e.field, e.offset), ("free-format bitrate", 1044 + 1045));
    // A count the frames don't reach: what they hold.
    let r = remux(&data, 10 * 1152).unwrap();
    assert_eq!((r.frames, r.samples, r.cut), (3, 3 * 1152, 0));
}

#[test]
fn a_stream_of_no_whole_frame_is_refused() {
    let e = remux(&[0; 16], 1152).unwrap_err();
    assert_eq!((e.field, e.kind), ("frame", ErrorKind::Missing));
    let e = remux(&stream(1, 0)[..600], 1152).unwrap_err();
    assert_eq!((e.field, e.kind), ("frame", ErrorKind::Missing));
    let e = remux(&stream(1, 0), 0).unwrap_err();
    assert_eq!((e.field, e.kind), ("frame", ErrorKind::Missing));
}

#[test]
fn a_frame_that_changes_the_stream_is_refused() {
    let mut data = frame(&giants(false), 1);
    data.extend(frame(
        &Header {
            rate: 48_000,
            ..giants(false)
        },
        2,
    ));
    let e = remux(&data, 2 * 1152).unwrap_err();
    assert_eq!((e.field, e.offset), ("frame_header", 1044));
    // A bitrate change is allowed: variable bitrate streams are MP3s too.
    let mut vbr = frame(&giants(false), 1);
    vbr.extend(frame(
        &Header {
            bitrate: 128_000,
            ..giants(false)
        },
        2,
    ));
    assert_eq!(remux(&vbr, 2 * 1152).unwrap().frames, 2);
}
