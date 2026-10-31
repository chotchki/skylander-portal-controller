use super::builder::{cinematic, header, mono, stereo};
use super::*;

#[test]
fn a_cinematics_tracks_read_in_file_order() {
    let h = Header::read(&cinematic()).unwrap();
    assert_eq!((h.revision, h.frames), (b'i', 7643));
    assert_eq!((h.width, h.height, h.rate), (1280, 720, (30, 1)));
    let ids: Vec<u32> = h.tracks.iter().map(|t| t.id).collect();
    assert_eq!(ids, [1, 5, 6, 7, 8, 9, 10, 15, 18, 20, 22, 23, 0]);
    assert_eq!(h.track(7), Some(3));
    assert_eq!(h.track(0), Some(12));
    assert_eq!(h.track(2), None);
    assert!(h.tracks[12].stereo() && !h.tracks[3].stereo());
    assert!(h.tracks.iter().all(|t| t.sample_rate == 44_100));
    // One track, and none.
    let one = Header::read(&header(5, &[stereo(0)])).unwrap();
    assert_eq!(one.tracks, [stereo(0)]);
    assert!(Header::read(&header(5, &[])).unwrap().tracks.is_empty());
    assert_eq!(mono(3).flags, 0x5000);
}

#[test]
fn a_bad_magic_a_count_past_the_cap_and_a_cut_short_header_are_refused() {
    let mut bad = cinematic();
    bad[0] = b'K';
    assert!(matches!(
        Header::read(&bad).unwrap_err().kind,
        ErrorKind::BadMagic { .. }
    ));
    let mut many = cinematic();
    many[40..44].copy_from_slice(&(MAX_TRACKS + 1).to_le_bytes());
    let e = Header::read(&many).unwrap_err();
    assert_eq!((e.field, e.offset), ("audio tracks", 40));
    // Every cut short of the last id is refused, never a panic.
    let whole = cinematic();
    let end = HEADER_LEN + 12 * 13;
    for cut in 0..end {
        assert!(Header::read(&whole[..cut]).is_err(), "cut at {cut}");
    }
    assert!(Header::read(&whole[..end]).is_ok());
}
