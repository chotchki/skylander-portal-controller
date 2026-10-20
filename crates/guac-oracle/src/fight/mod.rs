//! The first fight's metrics (openspec AH.6, design decisions 17 and 18):
//! pure code over O5 and O6 traces keyed on game time, ahead of every
//! measurement as AG.8's extractors were. AH.9-AH.11 calibrate with them and
//! AH.30 judges guacamole with them.
//!
//! - [`ticks`]: the per-frame trace both sides fill, every actor named the
//!   same way on both (a placement by its name, a spawn by its parent).
//! - [`oracle`]: what O6 adds to an O5 record (AH.5's reader fills
//!   [`oracle::ChainRecord`]) and the oracle's ticks from the two, on AG.1's
//!   game time.
//! - [`draws`]: MT draws since F0 per game frame from the two watched words,
//!   and an anchor's horizon as the last frame every replay agrees on them.
//! - [`extract`]: one extractor per metric, run unchanged over both sides.
//! - [`log`]: one event log per actor, judged on its own: stamps in game
//!   seconds inside the horizon, order alone past it, floors calibrated by
//!   leave-one-out.
//! - [`equivalence`]: decision 17's check at an anchor.
//!
//! The distribution tests are [`crate::stats`]'s and the totals AF.10's
//! [`crate::totals`].
//!
//! Game time starts at the trace's first tick. The oracle's first O5 record
//! is F0's own flip, read once the run resumes, and its step is the pause
//! clamped to 0.05 s (AG.1), so a headless run compared against it steps the
//! oracle's recorded steps (AG.18's model replay does) rather than 1/60 s
//! from F0, or every stamp sits 1/30 s early.

pub mod draws;
pub mod equivalence;
pub mod extract;
pub mod log;
pub mod oracle;
pub mod ticks;
