//! NFC scanner bridge — spawns the long-running `nfc-reader` worker on its
//! own OS thread. Feature-gated behind `nfc-import` so the base build
//! doesn't pull in pcsc-lite / winscard linkage.
//!
//! The worker owns a single ACR122U handle; all pcsc calls are blocking, so
//! a dedicated OS thread beats tokio's `spawn_blocking` pool (which is
//! sized for short-lived blocking ops, not a never-ending poll loop).
//!
//! PLAN 6.5.1.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use skylander_core::{Event, FigureId};
use tokio::sync::broadcast;

/// Spawn the scanner worker. Non-blocking; the thread runs until the
/// process exits. Logs its own errors via `tracing`.
///
/// `scanned_dir` holds raw `<uid>.sky` dumps (PLAN 6.5.3).
/// `library_identities` is the pack-plus-prior-scans `(fid, variant)`
/// map built at startup (PLAN 6.5.5a); the worker uses it to decide
/// whether a scan is "already in your collection".
pub fn spawn(
    events: broadcast::Sender<Event>,
    scanned_dir: PathBuf,
    library_identities: Arc<HashMap<(u32, u16), FigureId>>,
) {
    if let Err(e) = std::thread::Builder::new()
        .name("nfc-scanner".into())
        .spawn(move || {
            skylander_nfc_reader::run_scanner_worker(events, scanned_dir, library_identities);
        })
    {
        tracing::error!(error = %e, "nfc-scanner: failed to spawn worker thread");
    }
}
