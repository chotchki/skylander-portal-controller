//! What a comparison against the RPCS3 oracle needs that isn't the emulator:
//! the input-script format, observation traces keyed by flip offset, envelopes
//! and the trajectory, event, totals and frame comparisons, and an MT19937 model
//! (openspec AF, design decision 6). Pure: no I/O of its own and no RPCS3
//! dependency, so a rung's test can use it without linking `tools/oracle-harness`.
//!
//! AF.10's metrics (design decision 9) are here: [`trace`] (traces,
//! envelopes, the trajectory comparison, the usable horizon), [`events`],
//! [`totals`] and [`frames`] (FLIP, NVlabs' reference). Their floors and k are
//! parameters until AF.12 calibrates them. AF.7 adds the input scripts, AF.8
//! the MT19937 model.

#![forbid(unsafe_code)]

pub mod error;
pub mod events;
pub mod frames;
pub mod totals;
pub mod trace;

pub use error::MetricError;
