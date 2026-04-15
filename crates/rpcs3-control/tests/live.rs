//! Live integration test — drives a running RPCS3 via UIA.
//!
//! Set these env vars before running:
//!   RPCS3_SKY_TEST_PATH=C:/.../Eruptor.sky
//!
//! Then:
//!   cargo test -p skylander-rpcs3-control --test live -- --ignored --nocapture
//!
//! Requires:
//!   * RPCS3 already running.
//!   * Skylanders Manager dialog open (Manage → Manage Skylanders Portal).
//!   * The path in RPCS3_SKY_TEST_PATH must exist and be a valid `.sky` file.

#![cfg(windows)]

use std::path::PathBuf;

use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::{PortalDriver, UiaPortalDriver};

#[test]
#[ignore = "requires interactive RPCS3 and RPCS3_SKY_TEST_PATH"]
fn rpcs3_live_load() {
    let path = match std::env::var("RPCS3_SKY_TEST_PATH") {
        Ok(s) => PathBuf::from(s),
        Err(_) => return,
    };

    let driver = UiaPortalDriver::new().expect("construct driver");
    driver.open_dialog().expect("open dialog");

    let slot = SlotIndex::new(0).unwrap();
    driver.clear(slot).expect("clear slot 0");

    let before = driver.read_slots().expect("read slots");
    assert!(matches!(before[0], SlotState::Empty));

    let name = driver.load(slot, &path).expect("load figure");
    eprintln!("loaded: {name}");
    assert!(!name.is_empty());

    let after = driver.read_slots().expect("read slots");
    match &after[0] {
        SlotState::Loaded { display_name, .. } => assert_eq!(display_name, &name),
        s => panic!("expected Loaded, got {s:?}"),
    }

    driver.clear(slot).expect("clear at end");
}
