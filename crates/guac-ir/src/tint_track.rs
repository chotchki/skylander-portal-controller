//! A material tint track's colour over time (openspec AS.451), beside
//! [`crate::scroll`]: a pure function of the track as stored and the
//! seconds it has run on the caller's clock.
//!
//! - Its keys are frames at [`FRAMES_PER_SECOND`] times its rate, each with
//!   an RGBA colour as stored (128 neutral, as `Params::material_tint`).
//!   Between two keys the colour moves in a straight line, each channel on
//!   its own; before the first key it holds the first.
//! - Style 0 loops: past the last key it starts again from frame 0. Style
//!   1 goes out to the last key and comes back (a ping pong). How the
//!   runtime reads the field isn't read off its code; this is the data's
//!   own reading (stand-in ledger: `tint-style`): the six levels' style-0
//!   tracks are flickers (lanterns, light rays, the hub's pop lights) that
//!   read right only looping, and every style-1 track ends on a colour away
//!   from its first (a sky's alpha 255 to 150 over 2,000 frames, a
//!   filament, the stove's fire), which a loop would snap back from each
//!   pass; the engine's own sequences and sound envelopes number their
//!   playback the same way (0 one way, 1 ping pong). Another style holds
//!   its first key.
//! - A track with one key, or whose last key is at frame 0, holds its first.

use crate::model::MaterialTrack;

/// The frames a tint track's keys count per second at rate 1.
pub const FRAMES_PER_SECOND: f64 = 60.0;

/// Style 0: loops.
pub const LOOP: i32 = 0;
/// Style 1: out and back.
pub const PING_PONG: i32 = 1;

impl MaterialTrack {
    /// A tint track's colour once it has run `seconds`, each channel as
    /// stored (0 to 255, 128 neutral) in f32; `None` for another kind of
    /// track or one with no keys.
    pub fn tint_at(&self, seconds: f64) -> Option<[f32; 4]> {
        let MaterialTrack::Tint {
            style,
            rate,
            frames,
            colors,
        } = self
        else {
            return None;
        };
        let keys: Vec<(f64, [f32; 4])> = frames
            .iter()
            .zip(colors)
            .map(|(f, c)| (f64::from(*f), c.map(f32::from)))
            .collect();
        let &(_, first) = keys.first()?;
        let span = keys.last().map_or(0.0, |k| k.0);
        let held = |c| Some(c);
        if keys.len() < 2 || span <= 0.0 || !seconds.is_finite() || !rate.is_finite() {
            return held(first);
        }
        let f = (seconds * FRAMES_PER_SECOND * f64::from(*rate)).max(0.0);
        let t = match *style {
            LOOP => f.rem_euclid(span),
            PING_PONG => {
                let t = f.rem_euclid(2.0 * span);
                if t > span { 2.0 * span - t } else { t }
            }
            _ => return held(first),
        };
        let next = keys.iter().position(|k| k.0 > t);
        Some(match next {
            None => keys[keys.len() - 1].1,
            Some(0) => first,
            Some(i) => {
                let ((a, ca), (b, cb)) = (keys[i - 1], keys[i]);
                let w = ((t - a) / (b - a)) as f32;
                std::array::from_fn(|c| ca[c] + (cb[c] - ca[c]) * w)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(style: i32, rate: f32) -> MaterialTrack {
        MaterialTrack::Tint {
            style,
            rate,
            frames: vec![0, 5, 15, 60],
            colors: vec![
                [100, 40, 40, 255],
                [200, 180, 100, 255],
                [150, 150, 150, 255],
                [120, 90, 60, 255],
            ],
        }
    }

    fn at(t: &MaterialTrack, frame: f64) -> [f32; 4] {
        t.tint_at(frame / FRAMES_PER_SECOND).unwrap()
    }

    /// openspec AS.451: a track steps through its keys at their frames,
    /// straight between them, at its rate.
    #[test]
    fn a_tint_track_moves_between_its_keys_at_its_rate() {
        let t = track(LOOP, 1.0);
        assert_eq!(at(&t, 0.0), [100.0, 40.0, 40.0, 255.0]);
        assert_eq!(at(&t, 5.0), [200.0, 180.0, 100.0, 255.0]);
        let mid = at(&t, 2.5);
        assert!(
            (mid[0] - 150.0).abs() < 1e-3 && (mid[1] - 110.0).abs() < 1e-3,
            "{mid:?}"
        );
        assert_eq!(at(&t, 15.0), [150.0, 150.0, 150.0, 255.0]);
        // Twice the rate reaches frame 5's key in 2.5 frames of the clock.
        assert_eq!(at(&track(LOOP, 2.0), 2.5), [200.0, 180.0, 100.0, 255.0]);
    }

    /// openspec AS.451: style 0 starts again past its last key; style 1
    /// comes back through its keys, so it never snaps.
    #[test]
    fn style_0_loops_and_style_1_comes_back() {
        let (l, p) = (track(LOOP, 1.0), track(PING_PONG, 1.0));
        // Just past the last key.
        assert_eq!(at(&l, 65.0), [200.0, 180.0, 100.0, 255.0]);
        assert_eq!(at(&p, 115.0), [200.0, 180.0, 100.0, 255.0]);
        assert_eq!(at(&p, 60.0), [120.0, 90.0, 60.0, 255.0]);
        assert_eq!(at(&p, 120.0), [100.0, 40.0, 40.0, 255.0]);
        let a = at(&p, 59.0);
        let b = at(&p, 61.0);
        assert!((a[0] - b[0]).abs() < 1e-3, "{a:?} {b:?}");
        // A loop snaps from the last key to the first.
        assert_eq!(at(&l, 60.0), [100.0, 40.0, 40.0, 255.0]);
        // Over many passes it keeps time.
        assert_eq!(at(&l, 600.0 + 5.0), at(&l, 5.0));
        assert_eq!(at(&p, 1200.0 + 15.0), at(&p, 15.0));
    }

    /// An unread style, one key or a zero span holds the first key; another
    /// kind of track has no tint.
    #[test]
    fn an_unread_or_flat_track_holds_its_first_key() {
        let first = [100.0, 40.0, 40.0, 255.0];
        assert_eq!(at(&track(2, 1.0), 33.0), first);
        let one = MaterialTrack::Tint {
            style: LOOP,
            rate: 1.0,
            frames: vec![0],
            colors: vec![[100, 40, 40, 255]],
        };
        assert_eq!(at(&one, 9.0), first);
        let flip = MaterialTrack::FlipBook {
            layer: 0,
            style: 0,
            rows: 1,
            columns: 1,
            rate: 1.0,
            cells: Vec::new(),
        };
        assert_eq!(flip.tint_at(1.0), None);
    }
}
