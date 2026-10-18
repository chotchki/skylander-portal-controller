#![no_main]

use guac_formats::havok::spline::QuatTrack;
use guac_formats::havok::{Container, qs};
use guac_formats::registry::walk_havok;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_havok(data);
    let Ok(c) = Container::parse(data) else {
        return;
    };
    for s in &c.skeletons {
        assert_eq!(s.bones.len(), s.parents.len());
        assert_eq!(s.reference_pose.len(), s.parents.len());
        assert!(s.parents.iter().enumerate().all(|(i, p)| (-1..i as i64).contains(&i64::from(*p))));
        assert_eq!(s.bind_pose().len(), s.parents.len());
    }
    let Some(clip) = &c.clip else { return };
    let a = &clip.animation;
    assert_eq!(clip.binding.track_to_bone.len(), a.tracks.len());
    let end = f64::from(a.duration);
    for t in [-1.0, 0.0, end / 3.0, end, end + 1.0] {
        let s = a.sample(t);
        assert_eq!(s.len(), a.tracks.len());
        for (q, tr) in s.iter().zip(&a.tracks) {
            // Spline rotations come out unit length, or the identity.
            if let QuatTrack::Spline(_) = tr.rotation {
                assert!((qs::length(q.rotation) - 1.0).abs() < 1e-9);
            }
            assert!(q.translation.iter().chain(&q.scale).all(|x| x.is_finite()));
        }
    }
    assert_eq!(a.sample(-1.0), a.sample(0.0));
    assert_eq!(a.sample(end + 1.0), a.sample(a.last_frame() * f64::from(a.frame_duration)));
});
