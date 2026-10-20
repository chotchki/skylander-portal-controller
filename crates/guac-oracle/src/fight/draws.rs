//! MT draws since F0 per game frame, and an anchor's horizon from them
//! (openspec AH.6, design decisions 16 and 18).
//!
//! O5 watches two of the generator's words each frame (AH.5): `_mti`, how
//! many of the 624 have been drawn since the last twist, and the first word
//! of the state. With the full state at F0 (an `rng peek` at the anchor, or
//! `sgenrand` of an `rng seed` value) that's enough to count every draw: a
//! twist rewrites the first word, so stepping the F0 state's twists forward
//! until its first word matches says how many twists lie behind a record,
//! and the count is 624 a twist plus `_mti` against F0's own. A burst of more
//! than 624 draws inside one frame is counted whole too, which `_mti` alone
//! would miss.
//!
//! The horizon is AF.10's ([`crate::trace::usable_horizon`]) over the draw
//! counts at [`crate::tolerance::COUNT`]'s floor (0): the last frame since F0
//! every replay has drawn exactly as often by. Inside it no roll has parted
//! the replays yet, so an event no roll times can be stamped; past it only
//! order holds ([`super::log`]).

use serde::{Deserialize, Serialize};

use super::ticks::FightTrace;
use crate::MetricError;
use crate::rng::{Mt19937, N};
use crate::tolerance;
use crate::trace::{Horizon, Sample, Trace, usable_horizon};

/// What O5 reads of the generator at a record: `_mti` (words drawn since
/// the last twist, 624 when the next draw twists) and the state's first word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MtWords {
    pub mti: u32,
    pub word0: u32,
}

/// Draws since `start` at each `(frame, words)`, in order: frames since F0
/// and the count. `max_twists` bounds the twists searched between two
/// records (and before the first). Refused when a record's first word isn't
/// within reach ([`MetricError::RngLost`]: another generator, or one
/// reseeded) or the count goes back ([`MetricError::RngBackwards`]).
pub fn draws_since(
    start: &Mt19937,
    words: &[(u32, MtWords)],
    max_twists: u64,
) -> Result<Vec<(u32, u64)>, MetricError> {
    let mut state = start.clone();
    let mut twists: u64 = 0;
    let origin = u64::from(start.mti());
    let mut previous = origin;
    let mut out = Vec::with_capacity(words.len());
    for &(frame, w) in words {
        if w.mti as usize > N {
            return Err(MetricError::RngLost { frame });
        }
        let mut stepped = 0;
        while state.words()[0] != w.word0 {
            if stepped == max_twists {
                return Err(MetricError::RngLost { frame });
            }
            state.twist();
            twists += 1;
            stepped += 1;
        }
        let at = twists * N as u64 + u64::from(w.mti);
        if at < previous {
            return Err(MetricError::RngBackwards { frame });
        }
        previous = at;
        out.push((frame, at - origin));
    }
    Ok(out)
}

/// [`draws_since`] over a trace's ticks, every one of which must carry the
/// words ([`MetricError::NoRng`] names the first that doesn't).
pub fn draws(
    trace: &FightTrace,
    start: &Mt19937,
    max_twists: u64,
) -> Result<Vec<(u32, u64)>, MetricError> {
    let words = trace
        .ticks()
        .iter()
        .map(|t| {
            t.mt.map(|w| (t.frame, w))
                .ok_or(MetricError::NoRng { frame: t.frame })
        })
        .collect::<Result<Vec<_>, _>>()?;
    draws_since(start, &words, max_twists)
}

/// The anchor's horizon over two or more replays' draw counts: compared on
/// the frames every replay holds, the last of the unbroken run from the
/// first on which they all agree, and the first on which they don't.
pub fn anchor_horizon(replays: &[Vec<(u32, u64)>]) -> Result<Horizon, MetricError> {
    if replays.len() < 2 {
        return Err(MetricError::TooFewReplays {
            needed: 2,
            found: replays.len(),
        });
    }
    let low = replays
        .iter()
        .map(|r| r.first().map_or(u32::MAX, |s| s.0))
        .max()
        .unwrap_or(u32::MAX);
    let high = replays
        .iter()
        .map(|r| r.last().map_or(0, |s| s.0))
        .min()
        .unwrap_or(0);
    let traces = replays
        .iter()
        .map(|r| {
            Trace::new(
                r.iter()
                    .filter(|(f, _)| (low..=high).contains(f))
                    .map(|&(f, d)| Sample::new(u64::from(f), [d as f64]))
                    .collect(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    usable_horizon(&traces, tolerance::COUNT.floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run drawing `per_frame[k]` numbers in frame k, recorded before each
    /// frame's draws, from `start`.
    fn run(start: &Mt19937, per_frame: &[u64]) -> Vec<(u32, MtWords)> {
        let mut g = start.clone();
        per_frame
            .iter()
            .enumerate()
            .map(|(k, &n)| {
                let w = MtWords {
                    mti: g.mti(),
                    word0: g.words()[0],
                };
                for _ in 0..n {
                    g.next_u32();
                }
                (k as u32, w)
            })
            .collect()
    }

    #[test]
    fn draws_are_counted_across_twists_and_bursts() {
        // Fresh from sgenrand (mti 624, the first draw twists), and from a
        // peek mid-stream.
        for start in [Mt19937::sgenrand(4357), {
            let mut g = Mt19937::sgenrand(10_554);
            g.advance(1_000);
            g
        }] {
            // 18 a frame for a while, a burst of 1,500 (two twists in one
            // frame), none, then 700.
            let per_frame = [0, 18, 18, 18, 1_500, 0, 700, 1, 1];
            let counted = draws_since(&start, &run(&start, &per_frame), 4).unwrap();
            let mut total = 0;
            for (k, &(frame, drawn)) in counted.iter().enumerate() {
                assert_eq!((frame, drawn), (k as u32, total), "{k}");
                total += per_frame[k];
            }
        }
    }

    #[test]
    fn a_generator_out_of_reach_or_going_back_is_refused() {
        let start = Mt19937::sgenrand(4357);
        let mut words = run(&start, &[10, 10, 10]);
        // Reseeded mid-run: the first word matches no twist of F0's.
        words[2].1.word0 = Mt19937::sgenrand(99).words()[0] ^ 1;
        assert_eq!(
            draws_since(&start, &words, 3),
            Err(MetricError::RngLost { frame: 2 })
        );
        // More twists than the bound allows between two records.
        let words = run(&start, &[2_000, 0]);
        assert_eq!(
            draws_since(&start, &words, 2),
            Err(MetricError::RngLost { frame: 1 })
        );
        assert!(draws_since(&start, &words, 4).is_ok());
        let mut back = run(&start, &[10, 10, 10]);
        back[2].1.mti -= 15;
        assert_eq!(
            draws_since(&start, &back, 3),
            Err(MetricError::RngBackwards { frame: 2 })
        );
    }

    #[test]
    fn the_horizon_is_the_last_frame_before_the_first_disagreeing_count() {
        let start = Mt19937::sgenrand(4357);
        let base = [0u64, 12, 12, 30, 12, 12, 12, 12, 12, 12];
        let count = |per: &[u64]| draws_since(&start, &run(&start, per), 4).unwrap();
        // Three replays drawing alike through frame 5; the third draws one
        // more in frame 5, so its count differs from frame 6 on.
        let a = count(&base);
        let b = count(&base);
        let mut third = base;
        third[5] += 1;
        let c = count(&third);
        let horizon = anchor_horizon(&[a.clone(), b.clone(), c]).unwrap();
        assert_eq!(horizon.last_agreeing, Some(5));
        assert_eq!(horizon.parted_at, Some(6));
        // Replays that never part hold to the last common frame.
        let short = a[..8].to_vec();
        let held = anchor_horizon(&[a, b, short]).unwrap();
        assert_eq!((held.last_agreeing, held.parted_at), (Some(7), None));
        assert!(anchor_horizon(&[count(&base)]).is_err());
    }
}
