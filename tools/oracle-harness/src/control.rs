//! The O-verbs as steps a run takes (openspec AF.5): each sends its verb, waits
//! on `STATE` and RPCS3's output dirs until the step has happened, and copies
//! what RPCS3 wrote into the run's output dir. [`crate::launch::Oracle::control`]
//! makes one for a running oracle; the loopback test makes one against the fake.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use skylander_rpcs3_control::ipc::IpcPortalDriver;
use skylander_rpcs3_control::ipc::proto::EmuState;

use crate::outputs::{self, Frame, Kind, Rpcs3Outputs};
use crate::verbs::{Reply, Verb, VerbClient};

/// The gap [`Control::screenshot`] keeps after the last shot.
pub const SHOT_SPACING: Duration = Duration::from_millis(1100);

/// A screenshot copied out, with the flip it shows.
#[derive(Debug, Clone)]
pub struct Shot {
    pub path: PathBuf,
    pub flip: u64,
    pub frame: Frame,
}

/// An RSX capture copied out, with where the run stood when it was armed and
/// at the pause it ends in.
#[derive(Debug, Clone)]
pub struct Captured {
    pub path: PathBuf,
    pub armed_at: EmuState,
    pub paused_at: EmuState,
}

/// A savestate copied out, and the pause its reboot reached.
#[derive(Debug, Clone)]
pub struct Saved {
    pub path: PathBuf,
    pub saved_at: EmuState,
    pub rebooted: EmuState,
}

pub struct Control {
    verbs: VerbClient,
    driver: IpcPortalDriver,
    outputs: Rpcs3Outputs,
}

impl Control {
    /// The verbs on `p1_socket`, RPCS3's outputs under `config_dir` (the oracle
    /// home's RPCS3 config dir).
    pub fn new(p1_socket: &Path, config_dir: &Path) -> Self {
        Self {
            verbs: VerbClient::new(p1_socket),
            driver: IpcPortalDriver::with_path(p1_socket).with_timeout(Duration::from_secs(5)),
            outputs: Rpcs3Outputs::new(config_dir),
        }
    }

    pub fn verbs(&self) -> &VerbClient {
        &self.verbs
    }

    /// P1's driver on the same socket (`STATE`, `LOAD`, `CLEAR`, `RECONNECT`).
    pub fn driver(&self) -> &IpcPortalDriver {
        &self.driver
    }

    pub fn outputs(&self) -> &Rpcs3Outputs {
        &self.outputs
    }

    pub fn state(&self) -> Result<EmuState> {
        self.driver.read_state()
    }

    /// Polls `STATE` until `done` holds. A `STATE` that fails is retried (P1's
    /// listener outlives a savestate's reboot, but RSX doesn't); `frozen` fails
    /// at once.
    pub fn wait_state(
        &self,
        timeout: Duration,
        mut done: impl FnMut(&EmuState) -> bool,
    ) -> Result<EmuState> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        loop {
            if let Ok(state) = self.state() {
                if state.status == "frozen" {
                    bail!("RPCS3 reports frozen at frame {}", state.frames);
                }
                if done(&state) {
                    return Ok(state);
                }
                last = Some(state);
            }
            if Instant::now() > deadline {
                bail!("timed out after {timeout:?} (last state {last:?})");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// `PAUSE_AT flip`, then the pause: fails unless `STATE` reads
    /// `status=paused frames=<flip>`.
    pub fn pause_at(&self, flip: u64, timeout: Duration) -> Result<EmuState> {
        self.verbs.send(Verb::PauseAt(flip))?;
        let state = self.wait_state(timeout, |s| s.status == "paused")?;
        if state.frames != flip {
            bail!("PAUSE_AT {flip} paused at frame {}", state.frames);
        }
        Ok(state)
    }

    /// `RESUME`. Back means the emulator's main thread has resumed it, not
    /// that a flip has come yet.
    pub fn resume(&self) -> Result<()> {
        self.verbs.send(Verb::Resume)?;
        Ok(())
    }

    /// `SCREENSHOT`, then the PNG RPCS3 writes, copied into `out_dir` and read.
    /// RPCS3 names a shot to the second and creates it exclusively, so a second
    /// shot inside the same second would be lost: this waits until the newest
    /// shot there is over [`SHOT_SPACING`] old.
    pub fn screenshot(&self, out_dir: &Path, timeout: Duration) -> Result<Shot> {
        let before = self.outputs.list(Kind::Screenshot);
        let newest = before
            .iter()
            .filter_map(|p| p.metadata().ok()?.modified().ok())
            .max();
        if let Some(age) = newest.and_then(|t| t.elapsed().ok()) {
            std::thread::sleep(SHOT_SPACING.saturating_sub(age));
        }
        let Reply::Shot { flip } = self.verbs.send(Verb::Screenshot)? else {
            bail!("SCREENSHOT answered without a flip");
        };
        let source = self.outputs.wait_new(Kind::Screenshot, &before, timeout)?;
        let path = outputs::copy_out(&source, out_dir)?;
        let frame = outputs::read_frame(&path)?;
        Ok(Shot { path, flip, frame })
    }

    /// `RSXCAP`; at a pause, `RESUME` so the frames it records come; then the
    /// capture file and the pause RPCS3 takes after it, the file copied into
    /// `out_dir`.
    pub fn capture(&self, out_dir: &Path, timeout: Duration) -> Result<Captured> {
        let before = self.outputs.list(Kind::Capture);
        let armed_at = self.state()?;
        self.verbs.send(Verb::RsxCap)?;
        if armed_at.status == "paused" {
            self.resume()?;
        }
        let source = self.outputs.wait_new(Kind::Capture, &before, timeout)?;
        let paused_at = self.wait_state(timeout, |s| s.status == "paused")?;
        let path = outputs::copy_out(&source, out_dir)?;
        Ok(Captured {
            path,
            armed_at,
            paused_at,
        })
    }

    /// `SAVESTATE`, then the stop, the new state file and the reboot's Start
    /// Paused pause; the file copied into `out_dir`. P5's `RECONNECT` is the
    /// caller's, once this returns.
    pub fn savestate(&self, out_dir: &Path, timeout: Duration) -> Result<Saved> {
        let before: BTreeSet<PathBuf> = self.outputs.list(Kind::SaveState);
        let saved_at = self.state()?;
        self.verbs.send(Verb::SaveState)?;
        // The old session goes first: a status other than running or paused, or
        // the counter starting over.
        self.wait_state(timeout, |s| {
            !matches!(s.status.as_str(), "running" | "paused") || s.frames < saved_at.frames
        })?;
        let source = self.outputs.wait_new(Kind::SaveState, &before, timeout)?;
        let rebooted = self.wait_state(timeout, |s| s.status == "paused" && s.frames > 0)?;
        let path = outputs::copy_out(&source, out_dir)?;
        Ok(Saved {
            path,
            saved_at,
            rebooted,
        })
    }
}
