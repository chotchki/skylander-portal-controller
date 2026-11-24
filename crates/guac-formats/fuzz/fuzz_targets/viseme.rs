#![no_main]

use guac_formats::viseme::{Track, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(stream) = parse(data) else {
        return;
    };
    // A stream that reads hangs together: a positive frame time, every
    // block a track per kind byte, every spline's knots in order and one
    // point more than its count.
    assert!(stream.frame_time > 0.0);
    let tracks = stream.blocks[0].tracks.len();
    for b in &stream.blocks {
        assert_eq!(b.tracks.len(), tracks);
        for t in &b.tracks {
            if let Track::Spline { degree, knots, points } = t {
                assert!((1..=3).contains(degree));
                assert!(knots.windows(2).all(|w| w[0] <= w[1]));
                assert_eq!(knots.len(), points.len() + usize::from(*degree) + 1);
            }
        }
    }
});
