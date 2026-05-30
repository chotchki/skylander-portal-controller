//! RPCS3 portal control.
//!
//! The `PortalDriver` trait is the abstraction boundary. `IpcPortalDriver`
//! (PLAN 16.5) drives the patched RPCS3 portal over an AF_UNIX socket — the
//! Phase-16 production control path, no GUI/dialog. `UiaPortalDriver` drives the
//! emulated Skylanders portal dialog via Windows UI Automation (the legacy /
//! fallback path; see `docs/research/rpcs3-control.md`). `MockPortalDriver`
//! (feature `mock`) is an in-memory stand-in for tests.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
#[cfg(not(windows))]
use anyhow::bail;
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

    // --- IPC capability (PLAN 16.5/16.6). Default impls report "not supported"
    // so UIA / mock are unaffected; only IpcPortalDriver overrides them. ---

    /// The AF_UNIX socket this driver talks to, if it is the IPC driver. `Some`
    /// ⇒ the launcher boots RPCS3 in **no-GUI** mode with this `SKYLANDER_IPC_PATH`
    /// and uses the IPC readiness/liveness signal; `None` ⇒ the legacy UIA launch
    /// + window-title polling. Default `None`.
    fn ipc_socket_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Programmatic emulator run state over IPC. `Ok(None)` for drivers without an
    /// IPC channel (UIA/mock); `Err` is a transient IPC failure (retry during boot).
    fn emu_state(&self) -> Result<Option<ipc::proto::EmuState>> {
        Ok(None)
    }

    /// Native game-window handle over IPC, non-zero once the window exists.
    /// `Ok(None)` when unavailable / not yet created.
    fn game_window_handle(&self) -> Result<Option<u64>> {
        Ok(None)
    }
}

#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub use uia::UiaPortalDriver;
#[cfg(windows)]
pub mod hide;

pub mod games_yml;

// IPC portal driver (PLAN 16.5). Cross-platform (std UnixStream on unix,
// uds_windows on Windows) — the production control path for the patched-RPCS3 +
// no-GUI architecture (Phase 16), superseding UIA as the portal mechanism.
pub mod ipc;
pub use ipc::IpcPortalDriver;

#[cfg(windows)]
pub mod process;
pub mod process_mock;

#[cfg(windows)]
pub use process::UiaRpcsProcess;
pub use process_mock::MockRpcsProcess;

// Windows-only window-title read used by the legacy UIA boot path
// (`BootDirect` waits for the `FPS: … [SERIAL]` viewport before driving the
// dialog). The diagnostic scrapers (main-window / compile-progress / all-titles)
// and the FPS-based readiness sampler were retired in PLAN 16.6.3.2/.3 — the IPC
// STATE poller supersedes them.
#[cfg(windows)]
pub use process::read_viewport_title;

// Non-Windows stubs so server code that polls for an FPS: viewport
// (BootDirect handler) compiles on Mac/Linux. The mock-driver lifecycle
// never spawns a real RPCS3, so a viewport never exists — `None`
// always is the correct answer there.
#[cfg(not(windows))]
pub fn read_viewport_title() -> Option<String> {
    None
}

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

    /// Launch RPCS3 with `EBOOT.BIN` so the game starts directly (PLAN
    /// 10.8.4 direct-boot). Windows only — returns an error on
    /// non-Windows.
    pub fn launch_with_eboot(exe: &Path, eboot: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            UiaRpcsProcess::launch_with_eboot(exe, eboot).map(Self::Uia)
        }
        #[cfg(not(windows))]
        {
            let _ = (exe, eboot);
            bail!("RPCS3 process management is only supported on Windows")
        }
    }

    /// Launch the **patched** RPCS3 in **no-GUI** mode (Phase 16): `--no-gui`
    /// direct-EBOOT boot, borderless game window, and the Skylander IPC socket at
    /// `ipc_path`. Windows only — returns an error on non-Windows.
    pub fn launch_no_gui(
        exe: &Path,
        eboot: &Path,
        ipc_path: &Path,
        config_dir: Option<&Path>,
    ) -> Result<Self> {
        #[cfg(windows)]
        {
            UiaRpcsProcess::launch_no_gui(exe, eboot, ipc_path, config_dir).map(Self::Uia)
        }
        #[cfg(not(windows))]
        {
            let _ = (exe, eboot, ipc_path, config_dir);
            bail!("RPCS3 process management is only supported on Windows")
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
