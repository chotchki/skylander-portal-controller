//! Library surface. `main.rs` wires these modules together into the binary;
//! integration tests under `tests/` import what they need directly.

pub mod config;
pub mod fonts;
pub mod games;
pub mod http;
pub mod logging;
pub mod palette;
pub mod paths;
pub mod profiles;
#[cfg(feature = "sky-stats")]
pub mod sky_stats;
pub mod state;
pub mod ui;
pub mod wizard;
pub mod working_copies;
