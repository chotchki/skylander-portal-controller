//! RPCS3 portal control.
//!
//! The `PortalDriver` trait is the abstraction boundary. `IpcPortalDriver`
//! (PLAN 16.5) drives the patched RPCS3 portal over an AF_UNIX socket — the
//! Phase-16 production control path, no GUI/dialog. `UiaPortalDriver` drives the
//! emulated Skylanders portal dialog via Windows UI Automation (the legacy /
//! fallback path; see `docs/research/rpcs3-control.md`). `MockPortalDriver`
//! (feature `mock`) is an in-memory stand-in for tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
#[cfg(not(windows))]
use anyhow::bail;
use skylander_core::{Element, FigureId, SLOT_COUNT, SlotIndex, SlotState, SubtypeWord, ToyTypeId};

/// A figure placed by identity, with no `.sky` file behind it (openspec AA,
/// design decision 4): what a driver without an emulator gets instead of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub figure_id: FigureId,
    /// Parsed from `figure_id`, so callers key on the tag, not the id string.
    pub toy_type: ToyTypeId,
    pub variant: SubtypeWord,
    pub name: String,
    pub element: Option<Element>,
    /// The figure's portrait in the extract, `portraits/<id>.png`, when the
    /// disc has one. Server-private.
    pub portrait: Option<PathBuf>,
    /// The placing profile; `None` when no profile was unlocked.
    pub owner: Option<PlacementOwner>,
}

/// Who placed a [`Placement`], as the TV's owner pip draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementOwner {
    pub profile_id: String,
    /// Uppercased first letter of the profile's name.
    pub initial: String,
    /// The profile's colour, as the profile store keeps it.
    pub color: String,
}

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

    /// Put `placement` in `slot` by identity; the worker calls this instead of
    /// [`Self::load`] when the job has no file. Default errors, so a driver
    /// that loads files never quietly accepts one.
    fn place(&self, slot: SlotIndex, placement: &Placement) -> Result<()> {
        let _ = (slot, placement);
        anyhow::bail!("place: this driver loads figure files")
    }

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

    /// macOS render surface over IPC (P8): the `CAContextID` the launcher hosts
    /// via `CALayerHost` to composite the game INSIDE its own window, plus the
    /// surface's native size in points (so the launcher can scale the hosted
    /// layer to its pane). `Some` once the game's render layer has a published
    /// `CAContext`; `Ok(None)` when unavailable / not yet published. Default is
    /// `None` (non-IPC + non-macOS drivers).
    fn game_surface(&self) -> Result<Option<ipc::proto::GameSurface>> {
        Ok(None)
    }

    /// Move + resize the running game window (P7) — the launcher tiling the game
    /// below itself in Desktop mode. On Windows the launcher repositions the
    /// game HWND directly via `SetWindowPos`; on macOS it can't touch another
    /// app's window, so it routes the fit through this IPC command instead.
    /// Default errors so non-IPC drivers (UIA / mock) report "no window control"
    /// rather than silently no-op; `IpcPortalDriver` overrides.
    fn window_set(&self, x: i32, y: i32, w: u32, h: u32) -> Result<()> {
        let _ = (x, y, w, h);
        anyhow::bail!("window_set: this driver has no window control")
    }

    /// Hot-plug-cycle the emulated portal (P5) — detach+reattach so a
    /// save-state-resumed guest re-enumerates it and refreshes the stale USB
    /// handles that otherwise fail every portal transfer with `CELL_EINVAL`.
    /// Default no-op for non-IPC drivers (UIA / mock); `IpcPortalDriver` overrides.
    fn reconnect(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub use uia::UiaPortalDriver;
#[cfg(windows)]
pub mod hide;

pub mod games_yml;

pub mod inject;
pub use inject::{InjectedOutcome, LoadInjector};

// IPC portal driver (PLAN 16.5). Cross-platform (std UnixStream on unix,
// uds_windows on Windows) — the production control path for the patched-RPCS3 +
// no-GUI architecture (Phase 16), superseding UIA as the portal mechanism.
pub mod ipc;
pub use ipc::IpcPortalDriver;

#[cfg(windows)]
pub mod process;
pub mod process_mock;

/// Real RPCS3 process lifecycle for macOS/Linux (Phase 16 IPC path). Wired into
/// the `RpcsProcess::Unix` variant (PLAN 16.11) — `SKYLANDER_PORTAL_DRIVER=ipc`
/// drives the patched binary here. The compiled non-Windows *default* stays the
/// mock (IPC is opt-in); see docs/dev/macos-rpcs3-build.md.
#[cfg(unix)]
pub mod process_unix;
#[cfg(unix)]
pub use process_unix::UnixRpcsProcess;

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
pub use mock::MockPortalDriver;

/// Lifecycle handle for an RPCS3 instance.
///
/// Variants are platform-gated. `Uia` (Windows) wraps a real process driven by
/// UI Automation (spawned + job-object-bound, drivable via the menu bar).
/// `Unix` (macOS/Linux, PLAN 16.11) wraps a real patched-RPCS3 process driven
/// over the AF_UNIX IPC socket — the cross-platform production control path
/// (no UIA, no Job Object; SIGTERM→SIGKILL shutdown). `Mock` is the portable
/// fake — always reports alive until shutdown — used under `DriverKind::Mock`
/// so Mac/Linux dev (and mock-driver tests on Windows) can satisfy PLAN
/// 4.15.16's always-running-RPCS3 contract without a real emulator.
///
/// Callers use this enum directly so the server crate doesn't have to
/// `cfg`-branch on driver kind for every lifecycle call site.
#[derive(Debug)]
pub enum RpcsProcess {
    #[cfg(windows)]
    Uia(UiaRpcsProcess),
    #[cfg(unix)]
    Unix(UnixRpcsProcess),
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
    /// direct-EBOOT boot + the Skylander IPC socket at `ipc_path`. The
    /// **cross-platform** production control path (PLAN 16.11): Windows drives
    /// it via [`UiaRpcsProcess`] (borderless game window + Job Object), macOS /
    /// Linux via [`UnixRpcsProcess`] (SIGTERM→SIGKILL, no borderless — see its
    /// module note). Both rendezvous with `IpcPortalDriver` on `ipc_path`.
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
        #[cfg(unix)]
        {
            UnixRpcsProcess::launch_no_gui(exe, eboot, ipc_path, config_dir).map(Self::Unix)
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (exe, eboot, ipc_path, config_dir);
            bail!("RPCS3 process management requires Windows or a Unix platform")
        }
    }

    /// Launch RPCS3 with an argv and environment the caller spells out in full
    /// (openspec AF.3, the dev oracle's launch wrapper): no `SKYLANDER_BORDERLESS`
    /// or `RPCS3_CONFIG_DIR` unless `spec.env` sets them. Unix only; Windows
    /// reports it unsupported, since the oracle is Mac first.
    pub fn launch_spec(spec: &LaunchSpec) -> Result<Self> {
        #[cfg(unix)]
        {
            UnixRpcsProcess::launch_spec(spec).map(Self::Unix)
        }
        #[cfg(not(unix))]
        {
            let _ = spec;
            anyhow::bail!("RpcsProcess::launch_spec is Unix only (the oracle is Mac first)")
        }
    }

    /// Launch RPCS3's **full settings GUI** for on-demand per-game config (PLAN
    /// 16.9.3): plain GUI (no `--no-gui`, no borderless/IPC env), pointed at
    /// `config_dir` so it reads the user's games and persists per-game Custom
    /// Configurations. Windows only — returns an error on non-Windows (the mock
    /// platform has no real RPCS3 to configure).
    pub fn launch_gui_config(exe: &Path, config_dir: Option<&Path>) -> Result<Self> {
        #[cfg(windows)]
        {
            UiaRpcsProcess::launch_gui_config(exe, config_dir).map(Self::Uia)
        }
        #[cfg(not(windows))]
        {
            let _ = (exe, config_dir);
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
            #[cfg(unix)]
            Self::Unix(p) => p.pid(),
            Self::Mock(p) => p.pid(),
        }
    }

    pub fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.wait_ready(timeout),
            #[cfg(unix)]
            Self::Unix(p) => match p.wait_ready(timeout)? {
                process_unix::Readiness::Ready => Ok(()),
                process_unix::Readiness::NotYet => {
                    bail!("patched RPCS3 IPC socket not ready within {timeout:?}")
                }
            },
            Self::Mock(p) => p.wait_ready(timeout),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.is_alive(),
            #[cfg(unix)]
            Self::Unix(p) => p.is_alive(),
            Self::Mock(p) => p.is_alive(),
        }
    }

    pub fn shutdown_graceful(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.shutdown_graceful(timeout),
            #[cfg(unix)]
            Self::Unix(p) => p.shutdown_graceful(timeout),
            Self::Mock(p) => p.shutdown_graceful(timeout),
        }
    }

    /// Graceful shutdown targeting an explicit (IPC-published) window handle when
    /// known — see [`UiaRpcsProcess::shutdown_graceful_to_hwnd`]. The Unix and
    /// mock variants ignore the handle (no Win32 `WM_CLOSE`-to-HWND path). PLAN
    /// 16.6.1.3.
    pub fn shutdown_graceful_to_hwnd(
        &mut self,
        hwnd: Option<u64>,
        timeout: Duration,
    ) -> Result<ShutdownPath> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.shutdown_graceful_to_hwnd(hwnd, timeout),
            #[cfg(unix)]
            Self::Unix(p) => {
                let _ = hwnd;
                p.shutdown_graceful(timeout)
            }
            Self::Mock(p) => {
                let _ = hwnd;
                p.shutdown_graceful(timeout)
            }
        }
    }

    pub fn wait_for_exit_or_force(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        match self {
            #[cfg(windows)]
            Self::Uia(p) => p.wait_for_exit_or_force(timeout),
            #[cfg(unix)]
            Self::Unix(p) => p.wait_for_exit_or_force(timeout),
            Self::Mock(p) => p.wait_for_exit_or_force(timeout),
        }
    }
}

/// One RPCS3 launch spelled out in full, for [`RpcsProcess::launch_spec`].
#[derive(Debug, Clone, Default)]
pub struct LaunchSpec {
    pub exe: PathBuf,
    /// Everything after the executable on the command line.
    pub args: Vec<std::ffi::OsString>,
    /// `SKYLANDER_IPC_PATH` for the child; readiness probes and the post-exit
    /// cleanup use it too.
    pub ipc_path: PathBuf,
    /// Applied in order over the caller's own environment: `Some` sets the key,
    /// `None` removes one the child would otherwise inherit.
    pub env: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPath {
    AlreadyExited,
    Graceful,
    Forced,
}

#[cfg(all(test, unix))]
mod unix_enum_tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    /// On Unix the only real driver is the cross-platform IPC path
    /// (`launch_no_gui` → `Self::Unix`). The UIA-era constructors have no Unix
    /// impl, so they must error cleanly (not panic) if ever reached — a phone
    /// misconfig hitting the legacy path on Mac should surface an error, not UB.
    #[test]
    fn legacy_launch_modes_error_on_unix() {
        assert!(RpcsProcess::launch_library(Path::new("/bin/true")).is_err());
        assert!(
            RpcsProcess::launch_with_eboot(Path::new("/bin/true"), Path::new("/bin/true")).is_err()
        );
        assert!(RpcsProcess::launch_gui_config(Path::new("/bin/true"), None).is_err());
        assert!(RpcsProcess::attach().is_err());
    }

    /// The `Mock` variant is present on every platform and drives the enum arms
    /// (so a Unix build that selects `DriverKind::Mock` still has a working
    /// lifecycle alongside the `Unix` variant).
    #[test]
    fn mock_variant_lifecycle_on_unix() {
        let mut p = RpcsProcess::mock();
        assert!(p.is_alive());
        assert_eq!(p.pid(), 0);
        p.wait_ready(Duration::from_secs(1))
            .expect("mock wait_ready");
        assert_eq!(
            p.shutdown_graceful(Duration::from_secs(1)).unwrap(),
            ShutdownPath::Graceful
        );
        assert!(!p.is_alive());
    }
}

#[cfg(test)]
mod place_default_tests {
    use super::*;

    /// The trait's default error, which only the native driver overrides.
    const DEFAULT_PLACE_ERROR: &str = "place: this driver loads figure files";

    fn eruptor() -> Placement {
        let figure_id = FigureId::new("000009-0000");
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: "Eruptor".into(),
            element: Some(Element::Fire),
            portrait: None,
            owner: None,
        }
    }

    #[test]
    fn the_ipc_driver_keeps_the_default_place_error() {
        // No socket behind the path: the default must answer without one.
        let d = IpcPortalDriver::with_path("/nonexistent/place-default.sock");
        let err = d.place(SlotIndex::new(0).unwrap(), &eruptor()).unwrap_err();
        assert_eq!(err.to_string(), DEFAULT_PLACE_ERROR);
    }

    #[cfg(windows)]
    #[test]
    fn the_uia_driver_keeps_the_default_place_error() {
        let d = UiaPortalDriver::new().unwrap();
        let err = d.place(SlotIndex::new(0).unwrap(), &eruptor()).unwrap_err();
        assert_eq!(err.to_string(), DEFAULT_PLACE_ERROR);
    }

    #[cfg(feature = "mock")]
    #[test]
    fn the_mock_keeps_the_default_place_error_and_its_slots() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        let slot = SlotIndex::new(3).unwrap();
        let err = d.place(slot, &eruptor()).unwrap_err();
        assert_eq!(err.to_string(), DEFAULT_PLACE_ERROR);
        assert!(matches!(d.read_slots().unwrap()[3], SlotState::Empty));
    }
}
