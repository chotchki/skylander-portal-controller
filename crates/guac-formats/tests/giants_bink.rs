//! AS.8's disc test: every movie's Bink header on the Giants dump, against
//! AB.5's census: 61 movies at 1280 x 720; fmv01-fmv04 and fmv06-fmv11 with
//! 13 audio tracks, 12 mono whose ids are the disc's 12 language packs' ids
//! plus one, and one stereo with id 0; every other movie one stereo track
//! with id 0. It reads the first 4 KiB of each file. Ignored by default; it
//! reads `GUAC_GIANTS_DUMP` like the other disc tests:
//!
//! `cargo test -p guac-formats --test giants_bink -- --ignored --nocapture`

mod common;

use std::io::Read;

use guac_formats::bink::Header;

/// The 12 language packs' ids (`guac_formats::language`).
const LANGUAGES: [u32; 12] = [0, 4, 5, 6, 7, 8, 9, 14, 17, 19, 21, 22];

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_movies_header_is_the_censuss() {
    let dir = common::usrdir().join("movies");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".mov"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 61);
    let mut cinematics = 0;
    for name in &names {
        let mut head = vec![0; 4096];
        let mut f = std::fs::File::open(dir.join(name)).unwrap();
        let n = f.read(&mut head).unwrap();
        let h = Header::read(&head[..n]).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((h.revision, h.width, h.height), (b'i', 1280, 720), "{name}");
        let many = name.starts_with("fmv") && name != "fmv12.mov";
        if many {
            cinematics += 1;
            assert_eq!(h.tracks.len(), 13, "{name}");
            let mut voices: Vec<u32> = h
                .tracks
                .iter()
                .filter(|t| !t.stereo())
                .map(|t| t.id)
                .collect();
            voices.sort();
            let want: Vec<u32> = LANGUAGES.iter().map(|l| l + 1).collect();
            assert_eq!(voices, want, "{name}: each voice a language id plus one");
            let bed = h.track(0).expect("the stereo bed");
            assert!(h.tracks[bed].stereo(), "{name}");
            println!(
                "{name}: {} frames at {}/{}, voices {:?}, bed at {bed}",
                h.frames,
                h.rate.0,
                h.rate.1,
                h.tracks.iter().map(|t| t.id).collect::<Vec<_>>()
            );
        } else {
            assert_eq!(h.tracks.len(), 1, "{name}");
            assert!(h.tracks[0].stereo() && h.tracks[0].id == 0, "{name}");
        }
    }
    assert_eq!(cinematics, 10);
}
