//! What a comparison against the RPCS3 oracle needs that isn't the emulator:
//! the input-script format, observation traces keyed by flip offset, envelopes
//! and the trajectory, event, totals and frame comparisons, and an MT19937 model
//! (openspec AF, design decision 6). Pure: no I/O of its own and no RPCS3
//! dependency, so a rung's test can use it without linking `tools/oracle-harness`.
//!
//! AF.10's metrics (design decision 9) are here: [`trace`] (traces,
//! envelopes, the trajectory comparison, the usable horizon), [`events`],
//! [`totals`] and [`frames`] (FLIP, NVlabs' reference). AF.12's defaults for
//! them, calibrated on AF.11's measurement, are in [`tolerance`]. [`input`] is
//! AF.7's input-script format, [`rng`] AF.8's MT19937 model, [`recording`]
//! AG.1's per-frame recording keyed by flip and by game time, [`motion`] AG.8's
//! movement metrics over a trace keyed on game time.

#![forbid(unsafe_code)]

pub mod error;
pub mod events;
pub mod frames;
pub mod input;
pub mod motion;
pub mod recording;
pub mod rng;
pub mod tolerance;
pub mod totals;
pub mod trace;

pub use error::MetricError;
