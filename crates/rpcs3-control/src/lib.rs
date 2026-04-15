//! RPCS3 portal control.
//!
//! The `PortalDriver` trait is the abstraction boundary. `UiaPortalDriver`
//! drives the emulated Skylanders portal dialog via Windows UI Automation
//! (see `docs/research/rpcs3-control.md` for the research basis).
//! `MockPortalDriver` (feature `mock`) is an in-memory stand-in for tests.

use std::path::Path;

use anyhow::Result;
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};

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
}

#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub use uia::{window_kind, UiaPortalDriver, WindowKind};
#[cfg(windows)]
pub(crate) mod hide;

#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub use process::{RpcsProcess, ShutdownPath};

#[cfg(feature = "mock")]
pub mod mock;
#[cfg(feature = "mock")]
pub use mock::MockPortalDriver;
