//! Library surface. `main.rs` wires these modules together into the binary;
//! integration tests under `tests/` import what they need directly.

pub mod config;
pub mod games;
pub mod http;
pub mod logging;
pub mod profiles;
pub mod state;
pub mod ui;
