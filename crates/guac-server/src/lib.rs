//! The server both binaries link: HTTP + WS routes, the driver worker, the
//! profile store, Kaos, mDNS and the shared config types. Code that stays with
//! the product (`crates/server`) is reached through the traits in [`hooks`].

pub mod boot;
pub mod config;
pub mod embedded_assets;
pub mod firewall;
pub mod hooks;
pub mod http;
pub mod kaos;
pub mod logging;
pub mod mdns;
pub mod native_catalogue;
#[cfg(test)]
mod native_tests;
pub mod paths;
pub mod profiles;
pub mod round_qr;
pub mod state;
