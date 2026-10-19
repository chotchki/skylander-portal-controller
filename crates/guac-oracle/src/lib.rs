//! What a comparison against the RPCS3 oracle needs that isn't the emulator:
//! the input-script format, observation traces keyed by flip offset, envelopes
//! and the trajectory, event, totals and frame comparisons, and an MT19937 model
//! (openspec AF, design decision 6). Pure: no I/O of its own and no RPCS3
//! dependency, so a rung's test can use it without linking `tools/oracle-harness`.
//!
//! Empty for now. AF.7 adds the input scripts, AF.8 the MT19937 model, AF.10 the
//! traces, envelopes and comparisons.

#![forbid(unsafe_code)]

pub mod input;
pub mod rng;
