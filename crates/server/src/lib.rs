//! Library surface. `main.rs` wires these modules together into the binary;
//! integration tests under `tests/` import what they need directly.

pub mod badge;
pub mod badge_text;
pub mod config;
pub mod display_mode;
pub mod embedded_assets;
pub mod firewall;
pub mod fonts;
pub mod gl_fallback;
pub mod http;
pub mod kaos;
pub mod logging;
pub mod mdns;
#[cfg(feature = "nfc-import")]
pub mod nfc;
pub mod palette;
pub mod paths;
pub mod profiles;
pub mod round_qr;
pub mod rpcs3_config;
#[cfg(feature = "sky-stats")]
pub mod sky_edit;
#[cfg(feature = "sky-stats")]
pub mod sky_stats;
// PLAN 20.1 spike (temporary, Windows-only, gated on SKYLANDER_SPIKE_DESKTOP).
#[cfg(windows)]
pub mod spike_desktop;
pub mod state;
pub mod ui;
pub mod vortex;
pub mod wizard;
pub mod working_copies;
