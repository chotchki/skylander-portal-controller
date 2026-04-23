//! RPCS3 portal control.
//!
//! The `PortalDriver` trait is the abstraction boundary. `UiaPortalDriver`
//! drives the emulated Skylanders portal dialog via Windows UI Automation
//! (see `docs/research/rpcs3-control.md` for the research basis).
//! `MockPortalDriver` (feature `mock`) is an in-memory stand-in for tests.

use std::path::Path;
use std::time::Duration;

use anyhow::{Result, bail};
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

    /// Enumerate every game serial currently visible in RPCS3's library
    /// view. Prereq: RPCS3 is at the game-list table (same prereq as
    /// `boot_game_by_serial`). The UIA impl walks `game_list_table` for
    /// `DataItem`s and returns each item's name (the PS3 serial, e.g.
    /// `"BLUS31076"`). The mock impl returns whatever was previously
    /// `set_enumerated_games`d (default empty). Used by `/api/launch` to
    /// verify a requested serial actually exists in the library before
    /// committing to a boot, so a stale `games.yml` entry produces a
    /// fast specific error instead of a slow generic boot timeout
    /// (PLAN 3.7.8 phase 1).
    fn enumerate_games(&self, timeout: Duration) -> Result<Vec<String>>;

    /// Stop the currently-running game and return RPCS3 to its library
    /// view. Prereq: a game is actually running (viewport window
    /// present). The UIA impl finds a "Stop Emulation" / "Stop" menu
    /// item or toolbar button and invokes it; the mock impl is a no-op
    /// (mock has no real RPCS3 to stop). Used by `/api/quit` so the
    /// RPCS3 process stays alive across game changes — PLAN 4.15.16's
    /// "always-running RPCS3" contract. Returns once the game viewport
    /// has disappeared or `timeout` elapses.
    fn stop_emulation(&self, timeout: Duration) -> Result<()>;
}

#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub use uia::{UiaPortalDriver, WindowKind, window_kind};
#[cfg(windows)]
pub mod hide;

#[cfg(windows)]
pub mod process;
pub mod process_mock;

#[cfg(windows)]
pub use process::UiaRpcsProcess;
pub use process_mock::MockRpcsProcess;

// Windows-only internal helpers (UIA progress-text scraping + window
// enumeration). Not used by the server crate.
#[cfg(windows)]
pub use process::{
    find_compile_progress_text, list_all_visible_window_titles, read_main_window_title,
};

#[cfg(feature = "mock")]
pub mod mock;
#[cfg(feature = "mock")]
pub use mock::{MockOutcome, MockPortalDriver};

/// Lifecycle handle for an RPCS3 instance.
///
/// Two variants: `Uia` wraps a real Windows process driven by UI Automation
/// (spawned + job-object-bound, drivable via the menu bar). `Mock` is a
/// portable fake — always reports alive until shutdown — used under
/// `DriverKind::Mock` so Mac/Linux dev (and mock-driver tests on Windows)
/// can satisfy PLAN 4.15.16's always-running-RPCS3 contract without a real
/// emulator.
///
/// Callers use this enum directly so the server crate doesn't have to
/// `cfg`-branch on driver kind for every lifecycle call site.
#[derive(Debug)]
pub enum RpcsProcess {
    #[cfg(windows)]
    Uia(UiaRpcsProcess),
    Mock(MockRpcsProcess),
}

impl RpcsProcess {
    /// Launch RPCS3 into library view (Windows only). On non-Windows this
    /// returns an error — use `mock()` instead.
    pub fn launch_library(exe: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            UiaRpcsProcess::launch_library(exe).map(Self::Uia)
        }
        #[cfg(not(windows))]
        {
            let _ = exe;
            bail!(
                "RPCS3 process management is only supported on Windows; \
                 use SKYLANDER_PORTAL_DRIVER=mock on this platform"
            )
        }
    }

    /// Adopt an already-running RPCS3 (Windows only).
    pub fn attach() -> Result<Self> {
        #[cfg(windows)]
        {
            UiaRpcsProcess::attach().map(Self::Uia)
        }
        #[cfg(not(windows))]
        {
            bail!("RPCS3 process management is only supported on Windows")
        }
    }

    /// Construct the portable Mock variant. Always reports alive until
    /// shutdown. Used at startup under `DriverKind::Mock`.
    pub fn mock() -> Self {
        Self::Mock(MockRpcsProcess::new())
    }

    pub fn pid(&self) -> u32 {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.pid(),
            Self::Mock(p) => p.pid(),
        }
    }

    pub fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.wait_ready(timeout),
            Self::Mock(p) => p.wait_ready(timeout),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.is_alive(),
            Self::Mock(p) => p.is_alive(),
        }
    }

    pub fn shutdown_graceful(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.shutdown_graceful(timeout),
            Self::Mock(p) => p.shutdown_graceful(timeout),
        }
    }

    pub fn wait_for_exit_or_force(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.wait_for_exit_or_force(timeout),
            Self::Mock(p) => p.wait_for_exit_or_force(timeout),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPath {
    AlreadyExited,
    Graceful,
    Forced,
}
