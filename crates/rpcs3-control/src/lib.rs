//! RPCS3 portal control.
//!
//! The `PortalDriver` trait is the abstraction boundary. `UiaPortalDriver`
//! drives the emulated Skylanders portal dialog via Windows UI Automation
//! (see `docs/research/rpcs3-control.md` for the research basis).
//! `MockPortalDriver` (feature `mock`) is an in-memory stand-in for tests.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use skylander_core::{SLOT_COUNT, SlotIndex, SlotState};

/// Drive the emulated Skylanders portal.
///
/// Implementations MUST be safe to call from multiple threads, but the server
/// is responsible for serializing operations — Qt dialogs aren't re-entrant
/// from external driving.
pub trait PortalDriver: Send + Sync {
    /// Ensure the "Skylanders Manager" dialog is visible inside RPCS3. Opens
    /// it via the Manage menu if necessary. Idempotent.
    fn open_dialog(&self) -> Result<()>;

    /// Read the current state of all 8 portal slots. The returned slot states
    /// use `Loaded { display_name, figure_id: None }` — figure-id
    /// reconciliation against the pack index is a higher-layer concern.
    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]>;

    /// Load the `.sky` file at `path` into `slot`. Returns RPCS3's display
    /// name for the loaded figure. Clears the slot first if it was occupied.
    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String>;

    /// Clear `slot`. Returns once the slot shows "None".
    fn clear(&self, slot: SlotIndex) -> Result<()>;

    /// Boot the game with PS3 serial `serial` from the library view. Prereq:
    /// RPCS3 was just launched via `RpcsProcess::launch_library` and is
    /// sitting at the game list. The UIA impl clicks the matching `DataItem`
    /// and synthesises Enter; the mock impl is a no-op (mock has no RPCS3
    /// process to boot). Called by the server's `/api/launch` handler after
    /// `open_dialog()` so Qt's focus state is cold when boot runs.
    fn boot_game_by_serial(&self, serial: &str, timeout: Duration) -> Result<()>;
}

#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub use uia::{UiaPortalDriver, WindowKind, window_kind};
#[cfg(windows)]
pub mod hide;

#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub use process::{
    RpcsProcess, ShutdownPath, find_compile_progress_text, list_all_visible_window_titles,
    read_main_window_title,
};

#[cfg(feature = "mock")]
pub mod mock;
#[cfg(feature = "mock")]
pub use mock::{MockOutcome, MockPortalDriver};
