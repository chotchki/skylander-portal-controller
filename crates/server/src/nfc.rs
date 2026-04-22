//! NFC scanner bridge — spawns the long-running `nfc-reader` worker on its
//! own OS thread. Feature-gated behind `nfc-import` so the base build
//! doesn't pull in pcsc-lite / winscard linkage.
//!
//! The worker owns a single ACR122U handle; all pcsc calls are blocking, so
//! a dedicated OS thread beats tokio's `spawn_blocking` pool (which is
//! sized for short-lived blocking ops, not a never-ending poll loop).
//!
//! PLAN 6.5.1.

use std::path::PathBuf;

use skylander_core::Event;
use tokio::sync::broadcast;

/// Spawn the scanner worker. Non-blocking; the thread runs until the
/// process exits. Logs its own errors via `tracing`.
///
/// `scanned_dir` is the destination for raw `<uid>.sky` dumps — 6.5.3 will
/// formalize this path; for 6.5.1 it's just `<data_root>/scanned/`.
pub fn spawn(events: broadcast::Sender<Event>, scanned_dir: PathBuf) {
    if let Err(e) = std::thread::Builder::new()
        .name("nfc-scanner".into())
        .spawn(move || {
            skylander_nfc_reader::run_scanner_worker(events, scanned_dir);
        })
    {
        tracing::error!(error = %e, "nfc-scanner: failed to spawn worker thread");
    }
}
