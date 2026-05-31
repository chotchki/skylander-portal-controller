//! Real RPCS3 process lifecycle for Unix (macOS / Linux) — the IPC production
//! path. Mirrors the method surface of [`crate::process_mock`] (and the Windows
//! `process::RpcsProcess`), but actually spawns and supervises the emulator.
//!
//! Differences from the Windows lifecycle (all deliberate):
//!   * Shutdown is SIGTERM-then-SIGKILL — no Job Objects, no `RPCS3.buf`
//!     lockfile cleanup (both Windows-only concepts).
//!   * Readiness polls the AF_UNIX IPC socket (connect probe). The patched
//!     emulator opens the listener in the `usb_device_skylander` ctor, i.e.
//!     once a Skylanders game has booted and opened the portal peripheral.
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

/// Result of a readiness poll: the emulator's IPC socket is accepting
/// connections (`Ready`) or it isn't yet within the timeout (`NotYet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    NotYet,
}

/// A real, supervised RPCS3 process on Unix.
pub struct UnixRpcsProcess {
    child: Child,
    socket_path: PathBuf,
}

impl UnixRpcsProcess {
    /// Launch RPCS3 headless (no game). The IPC listener will not come up until
    /// a Skylanders game boots, so [`wait_ready`](Self::wait_ready) will report
    /// `NotYet` until then — useful for a launcher that boots a game later.
    pub fn launch_no_gui(exe: &Path, socket_path: &Path) -> Result<Self> {
        Self::spawn(exe, socket_path, &["--no-gui".as_ref()])
    }

    /// Launch RPCS3 booting a game directly via its `EBOOT.BIN` (RPCS3 takes the
    /// eboot path as its first positional argument).
    pub fn launch_with_eboot(exe: &Path, eboot: &Path, socket_path: &Path) -> Result<Self> {
        Self::spawn(exe, socket_path, &["--no-gui".as_ref(), eboot.as_os_str()])
    }

    /// Launch RPCS3 to its library view (no direct boot, GUI up).
    pub fn launch_library(exe: &Path, socket_path: &Path) -> Result<Self> {
        Self::spawn(exe, socket_path, &[])
    }

    fn spawn(exe: &Path, socket_path: &Path, args: &[&std::ffi::OsStr]) -> Result<Self> {
        let mut cmd = Command::new(exe);
        cmd.args(args).env("SKYLANDER_IPC_PATH", socket_path);
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

    /// Graceful shutdown: SIGTERM, wait up to 5 s, then SIGKILL. Cleans up the
    /// socket file afterward (the emulator unlinks on its own dtor too, but a
    /// hard kill may skip that).
    pub fn shutdown_graceful(&mut self) -> Result<()> {
        if let Ok(Some(_)) = self.child.try_wait() {
            let _ = std::fs::remove_file(&self.socket_path);
            return Ok(());
        }

        // SAFETY: kill(2) with a PID we own and a constant signal; no memory.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                let _ = std::fs::remove_file(&self.socket_path);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        tracing::warn!(
            pid = self.child.id(),
            "RPCS3 ignored SIGTERM; sending SIGKILL"
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
}

impl Drop for UnixRpcsProcess {
    fn drop(&mut self) {
        // Best-effort reap so a dropped handle doesn't leak a zombie.
        let _ = self.child.try_wait();
    }
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
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()])
            .expect("spawn sleep");
        assert!(p.is_alive(), "freshly spawned process should be alive");
        assert!(p.pid() > 0);

        p.shutdown_graceful().expect("graceful shutdown");
        assert!(
            !p.is_alive(),
            "process should be gone after SIGTERM shutdown"
        );
    }

    #[test]
    fn wait_ready_times_out_without_listener() {
        let sock = tmp_sock("notready");
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()])
            .expect("spawn sleep");
        // No listener bound the socket → NotYet.
        let r = p
            .wait_ready(Duration::from_millis(400))
            .expect("wait_ready ok");
        assert_eq!(r, Readiness::NotYet);
        p.shutdown_graceful().ok();
    }

    #[test]
    fn wait_ready_detects_listener() {
        let sock = tmp_sock("ready");
        let _listener = UnixListener::bind(&sock).expect("bind fake listener");
        let mut p = UnixRpcsProcess::spawn(Path::new("/bin/sleep"), &sock, &["30".as_ref()])
            .expect("spawn sleep");
        let r = p.wait_ready(Duration::from_secs(2)).expect("wait_ready ok");
        assert_eq!(r, Readiness::Ready);
        p.shutdown_graceful().ok();
        let _ = std::fs::remove_file(&sock);
    }
}
