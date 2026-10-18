//! Library surface. `main.rs` wires these modules together into the binary;
//! integration tests under `tests/` import what they need directly.
//!
//! The server itself (routes, driver worker, profile store, Kaos, mDNS) lives
//! in `guac_server`; its modules are re-exported here so the product's code
//! keeps reaching them as `crate::<module>`. What stays here is the product's
//! own: the eframe launcher, the first-launch wizard and config loader, and
//! the code behind the four `guac_server::hooks` ([`product_hooks`]).

pub use guac_server::{
    embedded_assets, firewall, http, kaos, logging, mdns, paths, profiles, round_qr, state,
};

pub mod badge;
pub mod badge_text;
pub mod compositor;
pub mod config;
pub mod display_mode;
pub mod fonts;
pub mod gl_fallback;
pub mod palette;
pub mod product_hooks;
pub mod rpcs3_config;
#[cfg(feature = "sky-stats")]
pub mod sky_edit;
#[cfg(feature = "sky-stats")]
pub mod sky_stats;
pub mod ui;
pub mod vortex;
pub mod wizard;
pub mod working_copies;

/// The product's migration set (`crates/server/migrations/`, frozen at its
/// three), run on `db.sqlite` by `ProfileStore::open`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
