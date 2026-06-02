//! Real RPCS3 process lifecycle for Unix (macOS / Linux) — the IPC production
//! path. Mirrors the method surface of [`crate::process_mock`] (and the Windows
//! `process::UiaRpcsProcess`), and is wired into the cross-platform
//! [`crate::RpcsProcess`] enum as the `Unix` variant (PLAN 16.11).
//!
//! Differences from the Windows lifecycle (all deliberate):
//!   * Shutdown is SIGTERM-then-SIGKILL — no Job Objects, no `RPCS3.buf`
//!     lockfile cleanup (both Windows-only concepts).
//!   * Readiness polls the AF_UNIX IPC socket (connect probe). The patched
//!     emulator opens the listener in the `usb_device_skylander` ctor, i.e.
//!     once a Skylanders game has booted and opened the portal peripheral.
//!     (The server's actual boot-readiness gate is `driver.emu_state()
//!     .is_playable()` — see `state.rs` BootDirect — so [`wait_ready`] here is
//!     a lower-level socket probe, not the playable signal.)
//!   * **No `SKYLANDER_BORDERLESS`.** macOS window coordination (z-order /
//!     positioning) is Win32-only and explicitly out of scope (PLAN 16.11), so
//!     we let RPCS3 show its normal decorated, movable game window on Mac rather
//!     than a borderless one we can't place. Portal control over IPC is the
//!     cross-platform value; window choreography stays Windows-only.
//!
//! The launcher sets `SKYLANDER_IPC_PATH` on the child so the emulator's
//! listener and our [`crate::IpcPortalDriver`] rendezvous on the same socket
//! regardless of platform tmpdir differences (`/tmp` vs macOS `$TMPDIR`).

#![cfg(unix)]

use anyhow::Result;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use crate::ShutdownPath;

/// Result of a readiness poll: the emulator's IPC socket is accepting
/// connections (`Ready`) or it isn't yet within the timeout (`NotYet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    NotYet,
}

/// A real, supervised RPCS3 process on Unix.
#[derive(Debug)]
pub struct UnixRpcsProcess {
    child: Child,
    socket_path: PathBuf,
}

impl UnixRpcsProcess {
    /// Launch the **patched** RPCS3 in **no-GUI** mode (PLAN 16.6.1 / 16.11):
    /// `--no-gui <EBOOT.BIN>` direct boot, the Skylander IPC socket at
    /// `socket_path` (`SKYLANDER_IPC_PATH`) that the controller's
    /// [`crate::IpcPortalDriver`] connects to, and `config_dir` →
    /// `RPCS3_CONFIG_DIR` (the data/config root holding firmware + `games.yml`,
    /// which may live apart from the exe). The unix twin of
    /// [`crate::process::UiaRpcsProcess::launch_no_gui`] minus the Windows-only
    /// borderless/z-order env (see the module note).
    pub fn launch_no_gui(
        exe: &Path,
        eboot: &Path,
        socket_path: &Path,
        config_dir: Option<&Path>,
    ) -> Result<Self> {
        Self::spawn(
            exe,
            socket_path,
            &["--no-gui".as_ref(), eboot.as_os_str()],
            config_dir,
        )
    }

    fn spawn(
        exe: &Path,
        socket_path: &Path,
        args: &[&std::ffi::OsStr],
        config_dir: Option<&Path>,
    ) -> Result<Self> {
        let mut cmd = Command::new(exe);
        cmd.args(args).env("SKYLANDER_IPC_PATH", socket_path);
        // Point the patched RPCS3 at its data/config root (firmware + games.yml),
        // which may live apart from the exe (Phase 16). Omitted ⇒ RPCS3's own
        // resolution (the bundle's default / `~/.config/rpcs3`).
        if let Some(dir) = config_dir {
            cmd.env("RPCS3_CONFIG_DIR", rpcs3_config_dir_env(dir));
        }
        // macOS: RPCS3's static-linked LLVM OpenMP + the bundle's libomp.dylib
        // trip "OMP: Error #15 … multiple copies of the OpenMP runtime" → abort
        // on launch. KMP_DUPLICATE_LIB_OK is OpenMP's own escape hatch (verified:
        // `rpcs3 --version` then exits 0). Drop once libomp is de-duped in the
        // bundle (distribution polish).
        #[cfg(target_os = "macos")]
        cmd.env("KMP_DUPLICATE_LIB_OK", "TRUE");
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("spawn RPCS3 at {}: {e}", exe.display()))?;
        tracing::info!(pid = child.id(), exe = %exe.display(), "launched RPCS3 (unix)");
        Ok(Self {
            child,
            socket_path: socket_path.to_path_buf(),
        })
    }

    /// The child PID.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Poll the IPC socket until the emulator's listener accepts a connection,
    /// or `timeout` elapses. A successful connect means the patched listener is
    /// bound (the Skylander USB device has been constructed).
    pub fn wait_ready(&self, timeout: Duration) -> Result<Readiness> {
        let deadline = Instant::now() + timeout;
        loop {
            if UnixStream::connect(&self.socket_path).is_ok() {
                return Ok(Readiness::Ready);
            }
            if Instant::now() >= deadline {
                return Ok(Readiness::NotYet);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Whether the process is still running. Reaps on exit so we don't report a
    /// dead-but-unwaited child as alive (which would defeat the supervisor).
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Graceful shutdown: SIGTERM, then wait up to `timeout` for the process to
    /// exit, else SIGKILL. Mirrors [`crate::process_mock::MockRpcsProcess::
    /// shutdown_graceful`]'s `ShutdownPath` contract so the enum arm is uniform.
    pub fn shutdown_graceful(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        if let Ok(Some(_)) = self.child.try_wait() {
            self.cleanup_socket();
            return Ok(ShutdownPath::AlreadyExited);
        }
        // SAFETY: kill(2) with a PID we own and a constant signal; no memory.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        self.wait_for_exit_or_force(timeout)
    }

    /// Wait up to `timeout` for the process to exit on its own; SIGKILL it if it
    /// doesn't. `Graceful` if it exited within the window, `Forced` if we had to
    /// kill it, `AlreadyExited` if it was already gone. Cleans up the socket file
    /// afterward (the emulator unlinks on its own dtor too, but a hard kill skips
    /// that).
    pub fn wait_for_exit_or_force(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        if let Ok(Some(_)) = self.child.try_wait() {
            self.cleanup_socket();
            return Ok(ShutdownPath::AlreadyExited);
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.cleanup_socket();
                return Ok(ShutdownPath::Graceful);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        tracing::warn!(
            pid = self.child.id(),
            "RPCS3 did not exit within timeout; sending SIGKILL"
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.cleanup_socket();
        Ok(ShutdownPath::Forced)
    }

    fn cleanup_socket(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for UnixRpcsProcess {
    fn drop(&mut self) {
        // Best-effort reap so a dropped handle doesn't leak a zombie.
        let _ = self.child.try_wait();
    }
}

/// Build the `RPCS3_CONFIG_DIR` env value for a config-root directory.
///
/// The unix twin of `process::rpcs3_config_dir_env` (PLAN 16.9.5a). RPCS3's
/// `fs::get_config_dir()` normalises `\` → `/` then does
/// `dir.resize(dir.rfind('/') + 1)`, which lops off the trailing path
/// component — file-path logic that, applied to a *bare directory*, drops one
/// level too high and makes RPCS3 miss `dev_flash`/`games.yml` and pop its
/// first-run wizard. Appending a trailing separator makes the last component
/// survive. The resize is platform-agnostic (RPCS3 normalises separators
/// first), so macOS/Linux need the same fix.
fn rpcs3_config_dir_env(dir: &Path) -> std::ffi::OsString {
    let mut value = dir.as_os_str().to_os_string();
    let already_terminated = dir.as_os_str().to_string_lossy().ends_with(['/', '\\']);
    if !value.is_empty() && !already_terminated {
        value.push(std::path::MAIN_SEPARATOR_STR);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn tmp_sock(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("skytest-{}-{}.sock", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn spawn_then_sigterm_shutdown() {
        let sock = tmp_sock("shutdown");
        // `sleep 30` stands in for the emulator: a long-lived process that
        // terminates on SIGTERM.
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()], None)
            .expect("spawn sleep");
        assert!(p.is_alive(), "freshly spawned process should be alive");
        assert!(p.pid() > 0);

        let path = p
            .shutdown_graceful(Duration::from_secs(5))
            .expect("graceful shutdown");
        assert_eq!(path, ShutdownPath::Graceful);
        assert!(
            !p.is_alive(),
            "process should be gone after SIGTERM shutdown"
        );

        // Idempotent: a second shutdown reports it was already gone.
        let path = p
            .shutdown_graceful(Duration::from_secs(1))
            .expect("second shutdown");
        assert_eq!(path, ShutdownPath::AlreadyExited);
    }

    #[test]
    fn wait_for_exit_or_force_kills_a_live_process() {
        let sock = tmp_sock("force");
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()], None)
            .expect("spawn sleep");
        // No signal sent first: `sleep 30` won't exit within the window, so it
        // must be force-killed.
        let path = p
            .wait_for_exit_or_force(Duration::from_millis(300))
            .expect("force shutdown");
        assert_eq!(path, ShutdownPath::Forced);
        assert!(!p.is_alive());
    }

    #[test]
    fn wait_ready_times_out_without_listener() {
        let sock = tmp_sock("notready");
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()], None)
            .expect("spawn sleep");
        // No listener bound the socket → NotYet.
        let r = p
            .wait_ready(Duration::from_millis(400))
            .expect("wait_ready ok");
        assert_eq!(r, Readiness::NotYet);
        p.shutdown_graceful(Duration::from_secs(2)).ok();
    }

    #[test]
    fn wait_ready_detects_listener() {
        let sock = tmp_sock("ready");
        let _listener = UnixListener::bind(&sock).expect("bind fake listener");
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()], None)
            .expect("spawn sleep");
        let r = p.wait_ready(Duration::from_secs(2)).expect("wait_ready ok");
        assert_eq!(r, Readiness::Ready);
        p.shutdown_graceful(Duration::from_secs(2)).ok();
        let _ = std::fs::remove_file(&sock);
    }

    #[test]
    fn config_dir_env_appends_trailing_separator() {
        let v = rpcs3_config_dir_env(Path::new("/Users/me/rpcs3"));
        assert_eq!(v.to_string_lossy(), "/Users/me/rpcs3/");
        // Already-terminated paths are left alone.
        let v = rpcs3_config_dir_env(Path::new("/Users/me/rpcs3/"));
        assert_eq!(v.to_string_lossy(), "/Users/me/rpcs3/");
    }
}
