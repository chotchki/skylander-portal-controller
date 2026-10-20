//! The server both binaries link: HTTP + WS routes, the driver worker, the
//! profile store, Kaos, mDNS and the shared config types. Code that stays with
//! the product (`crates/server`) is reached through the traits in [`hooks`].

pub mod boot;
pub mod config;
pub mod dumps;
pub mod editor;
pub mod embedded_assets;
pub mod figures;
pub mod firewall;
pub mod hooks;
pub mod http;
pub mod instances;
pub mod kaos;
pub mod logging;
pub mod mdns;
#[cfg(test)]
mod native_tests;
pub mod paths;
pub mod profiles;
pub mod progress;
pub mod round_qr;
pub mod setup;
#[cfg(test)]
mod setup_tests;
pub mod state;
