//! In-memory `PortalDriver`. Use via the `mock` feature.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Result, anyhow};
use skylander_core::{SLOT_COUNT, SlotIndex, SlotState};

use crate::PortalDriver;

/// Inject-able outcomes for the next `load` call. One per queued entry;
/// consumed FIFO. When the queue is empty, `load` behaves normally.
#[derive(Debug, Clone)]
pub enum MockOutcome {
    /// Normal success path.
    Ok,
    /// Simulate the Windows shell "file is in use" TaskDialog path.
    FileInUse { message: String },
    /// Simulate an RPCS3 QMessageBox like "Failed to open the skylander file!".
    QtModal { message: String },
    /// Sleep past the driver's load timeout so the outer loop times out.
    Timeout,
}

/// Mock driver. Tracks per-slot state; `load` pulls the figure name from the
/// filename stem. Default latency 50ms per op (tune for tests).
pub struct MockPortalDriver {
    slots: Mutex<[SlotState; SLOT_COUNT]>,
    dialog_open: Mutex<bool>,
    latency: Duration,
    /// Queued outcomes for upcoming `load` calls.
    load_queue: Mutex<VecDeque<MockOutcome>>,
    /// Mocked library enumeration result. Defaults to every supported
    /// Skylanders serial so dev-mode manual testing has a populated
    /// game picker out of the box; unit tests override via
    /// `set_enumerated_games`.
    enumerated_games: Mutex<Vec<String>>,
}

impl MockPortalDriver {
    pub fn new() -> Self {
        Self::with_latency(Duration::from_millis(50))
    }

    pub fn with_latency(latency: Duration) -> Self {
        Self {
            slots: Mutex::new(std::array::from_fn(|_| SlotState::Empty)),
            dialog_open: Mutex::new(false),
            latency,
            load_queue: Mutex::new(VecDeque::new()),
            enumerated_games: Mutex::new(default_serials()),
        }
    }

    /// Queue a sequence of outcomes for the next N `load` invocations.
    pub fn queue_load_outcomes(&self, outcomes: Vec<MockOutcome>) {
        let mut q = self.load_queue.lock().unwrap();
        for o in outcomes {
            q.push_back(o);
        }
    }

    /// Clear any queued outcomes without touching the slot state.
    pub fn clear_queue(&self) {
        self.load_queue.lock().unwrap().clear();
    }

    /// Set the list of serials that the next `enumerate_games` call will
    /// return. Replaces any previous list. Drives the 3.7.8 verify-at-launch
    /// test path: empty simulates "no library / serial missing",
    /// `vec!["BLUS31076"]` simulates a library that has SWAP Force only.
    pub fn set_enumerated_games(&self, serials: Vec<String>) {
        *self.enumerated_games.lock().unwrap() = serials;
    }

    fn delay(&self) {
        if !self.latency.is_zero() {
            sleep(self.latency);
        }
    }
}

impl Default for MockPortalDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalDriver for MockPortalDriver {
    fn open_dialog(&self) -> Result<()> {
        self.delay();
        *self.dialog_open.lock().unwrap() = true;
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        Ok(self.slots.lock().unwrap().clone())
    }

    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String> {
        self.delay();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".into());

        // Consume the next injected outcome, if any.
        let outcome = self.load_queue.lock().unwrap().pop_front();
        match outcome {
            None | Some(MockOutcome::Ok) => {
                let mut slots = self.slots.lock().unwrap();
                // Driver doesn't know the caller's profile — server fills in
                // `placed_by` on the real state change it broadcasts. The
                // mock's internal slot state is just a fixture so `None` is
                // fine.
                slots[slot.as_usize()] = SlotState::Loaded {
                    figure_id: None,
                    display_name: name.clone(),
                    placed_by: None,
                };
                Ok(name)
            }
            Some(MockOutcome::FileInUse { message }) => {
                Err(anyhow!("Windows file in use: {message}"))
            }
            Some(MockOutcome::QtModal { message }) => Err(anyhow!("RPCS3 reported: {message}")),
            Some(MockOutcome::Timeout) => {
                // Sleep past any reasonable test-side timeout. The real UIA
                // driver would bail at ~10s.
                sleep(Duration::from_secs(11));
                Err(anyhow!("timeout"))
            }
        }
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        self.delay();
        self.slots.lock().unwrap()[slot.as_usize()] = SlotState::Empty;
        Ok(())
    }

    fn boot_game_by_serial(&self, _serial: &str, _timeout: Duration) -> Result<()> {
        // Mock has no RPCS3 process to boot. Tests that need to exercise the
        // launch flow against the mock use `/api/_test/set_game` to inject a
        // running game directly into server state.
        Ok(())
    }

    fn enumerate_games(&self, _timeout: Duration) -> Result<Vec<String>> {
        Ok(self.enumerated_games.lock().unwrap().clone())
    }

    fn stop_emulation(&self, _timeout: Duration) -> Result<()> {
        // Mock has no RPCS3 process, so "return to library" is a no-op.
        // Tests that want to observe the lifecycle use `/api/_test/set_game`
        // to flip `current_game` back to None directly.
        Ok(())
    }
}

/// Seed `enumerate_games` with every supported Skylanders serial so
/// dev-mode manual testing has a populated game picker out of the box.
fn default_serials() -> Vec<String> {
    skylander_core::SKYLANDERS_SERIALS
        .iter()
        .map(|(serial, _)| (*serial).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn queued_file_in_use_errors_and_leaves_slot_empty() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.queue_load_outcomes(vec![MockOutcome::FileInUse {
            message: "Airstrike.sky — This file is in use.".into(),
        }]);
        let err = d
            .load(SlotIndex::new(0).unwrap(), &PathBuf::from("Airstrike.sky"))
            .unwrap_err();
        assert!(err.to_string().contains("file in use"), "err: {err}");
        assert!(matches!(d.read_slots().unwrap()[0], SlotState::Empty));
    }

    #[test]
    fn queued_qt_modal_surfaces_message() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.queue_load_outcomes(vec![MockOutcome::QtModal {
            message: "Failed to open the skylander file!".into(),
        }]);
        let err = d
            .load(SlotIndex::new(1).unwrap(), &PathBuf::from("Bash.sky"))
            .unwrap_err();
        assert!(
            err.to_string().contains("Failed to open"),
            "unexpected err: {err}"
        );
    }

    #[test]
    fn queue_is_fifo_and_bypasses_to_normal_when_empty() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.queue_load_outcomes(vec![MockOutcome::QtModal {
            message: "boom".into(),
        }]);
        // First load hits the injected error.
        assert!(
            d.load(SlotIndex::new(0).unwrap(), &PathBuf::from("a.sky"))
                .is_err()
        );
        // Second load falls through to the normal success path.
        assert!(
            d.load(SlotIndex::new(0).unwrap(), &PathBuf::from("b.sky"))
                .is_ok()
        );
    }

    #[test]
    fn enumerate_games_defaults_to_all_skylanders_and_round_trips_set() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        // Default: every supported Skylanders serial.
        let got = d.enumerate_games(Duration::ZERO).unwrap();
        assert_eq!(got.len(), skylander_core::SKYLANDERS_SERIALS.len());
        assert_eq!(got[0], skylander_core::SKYLANDERS_SERIALS[0].0);

        d.set_enumerated_games(vec!["BLUS31076".into(), "BLUS31442".into()]);
        let serials = d.enumerate_games(Duration::ZERO).unwrap();
        assert_eq!(serials, vec!["BLUS31076", "BLUS31442"]);

        // Replaces, doesn't append.
        d.set_enumerated_games(vec!["BLUS30968".into()]);
        assert_eq!(d.enumerate_games(Duration::ZERO).unwrap(), vec!["BLUS30968"]);

        // Explicit empty models "no library / nothing installed".
        d.set_enumerated_games(vec![]);
        assert!(d.enumerate_games(Duration::ZERO).unwrap().is_empty());
    }

    #[test]
    fn load_then_clear() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.open_dialog().unwrap();

        let before = d.read_slots().unwrap();
        assert!(matches!(before[0], SlotState::Empty));

        let name = d
            .load(
                SlotIndex::new(0).unwrap(),
                &PathBuf::from("/pack/Fire/Eruptor.sky"),
            )
            .unwrap();
        assert_eq!(name, "Eruptor");

        let after_load = d.read_slots().unwrap();
        match &after_load[0] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Eruptor"),
            s => panic!("expected Loaded, got {s:?}"),
        }

        d.clear(SlotIndex::new(0).unwrap()).unwrap();
        let after_clear = d.read_slots().unwrap();
        assert!(matches!(after_clear[0], SlotState::Empty));
    }

    #[test]
    fn slots_are_independent() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.load(
            SlotIndex::new(2).unwrap(),
            &PathBuf::from("/pack/Spyro.sky"),
        )
        .unwrap();
        d.load(
            SlotIndex::new(5).unwrap(),
            &PathBuf::from("/pack/Chop Chop.sky"),
        )
        .unwrap();
        let s = d.read_slots().unwrap();
        assert!(matches!(s[0], SlotState::Empty));
        assert!(matches!(s[1], SlotState::Empty));
        match &s[2] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Spyro"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(s[3], SlotState::Empty));
        assert!(matches!(s[4], SlotState::Empty));
        match &s[5] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Chop Chop"),
            other => panic!("{other:?}"),
        }
    }
}
