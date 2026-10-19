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
use crate::rpcs3_log::LogTail;
use crate::verbs::{Reply, Verb, VerbClient};

/// What RPCS3 logs when it turns a savestate down after `SAVESTATE` has
/// answered `OK` (`Emulator::Kill`, `rpcs3/Emu/System.cpp`): an SPU thread it
/// can't lock (one asleep in an unsavable call for 5 s or more, a long pause
/// included), an HLE video decoder, or a savedata operation in flight. The
/// emulator stays where it was, so nothing but the log says so.
pub const SAVESTATE_REFUSED: &str = "Failed to savestate";

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
/// at the pause it ends in. That pause sits inside the captured frame's queue,
/// before the flip that presents it ([`frame_bounds`]): a shot there is the
/// frame before the captured one, and the captured frame is `paused_at + 1`'s.
#[derive(Debug, Clone)]
pub struct Captured {
    pub path: PathBuf,
    pub armed_at: EmuState,
    pub paused_at: EmuState,
}

/// GCM's flip-head methods (`0xE920`, `0xE924`) and display-queue methods
/// (`0xE940`-`0xE95C`), as register indices, the way RPCS3's LLE GCM driver
/// writes them (`gcm_enums.h`).
const GCM_FLIP_HEAD: std::ops::RangeInclusive<u32> = (0xE920 >> 2)..=(0xE924 >> 2);
const GCM_DRIVER_QUEUE: std::ops::RangeInclusive<u32> = (0xE940 >> 2)..=(0xE95C >> 2);

/// Where a capture's frame sits against the flip counter, read off its own
/// commands, which RPCS3 records before it runs each one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBounds {
    /// The first flip-head command (the flip of the frame queued just before
    /// the capture armed, the one that took the counter to `paused_at`).
    pub opening_flip: Option<usize>,
    /// Draws begun before that flip; 0 when the capture is one whole frame.
    pub draws_before_opening_flip: usize,
    /// The last command when it's a display queue, with its buffer: the
    /// captured frame's own queue, inside which RPCS3 wrote the capture and
    /// paused.
    pub closing_queue: Option<u32>,
    /// Flip-head commands after the opening one: more than one frame flipped
    /// inside the capture.
    pub later_flips: usize,
}

impl FrameBounds {
    /// The shape every Giants capture on the Mac has had (AF.5): one flip at
    /// the start, before any draw, and the frame's own queue at the end. Then
    /// the frame is the one the flip after the capture's pause presents.
    pub fn is_one_queued_frame(&self) -> bool {
        self.opening_flip.is_some()
            && self.draws_before_opening_flip == 0
            && self.closing_queue.is_some()
            && self.later_flips == 0
    }
}

/// One pass over the commands: draws counted by `SET_BEGIN_END` opening a
/// primitive, the first flip head and those after it, and whether the last
/// command is a display queue. Only an LLE-GCM game's flips show here (an HLE
/// one flips through `GCM_FLIP_COMMAND`, and reads as no opening flip).
pub fn frame_bounds(capture: &rrc_dump::Capture) -> FrameBounds {
    use rrc_dump::nv4097 as m;
    let mut bounds = FrameBounds {
        opening_flip: None,
        draws_before_opening_flip: 0,
        closing_queue: None,
        later_flips: 0,
    };
    let mut draws = 0;
    for (i, cmd) in capture.commands.iter().enumerate() {
        let reg = (cmd.header & m::HEADER_METHOD_MASK) >> 2;
        if reg == m::SET_BEGIN_END && cmd.value & 0xff != 0 {
            draws += 1;
        } else if GCM_FLIP_HEAD.contains(&reg) {
            if bounds.opening_flip.is_none() {
                bounds.opening_flip = Some(i);
                bounds.draws_before_opening_flip = draws;
            } else {
                bounds.later_flips += 1;
            }
        }
    }
    bounds.closing_queue = capture.commands.last().and_then(|cmd| {
        let reg = (cmd.header & m::HEADER_METHOD_MASK) >> 2;
        GCM_DRIVER_QUEUE.contains(&reg).then_some(cmd.value)
    });
    bounds
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
    /// RPCS3's log (`RPCS3.log` in the home's cache dir), when the caller has it.
    log: Option<PathBuf>,
}

impl Control {
    /// The verbs on `p1_socket`, RPCS3's outputs under `config_dir` (the oracle
    /// home's RPCS3 config dir).
    pub fn new(p1_socket: &Path, config_dir: &Path) -> Self {
        Self {
            verbs: VerbClient::new(p1_socket),
            // 15 s: with other lanes building, a STATE took over 5 s to come back on a
            // load average of 28 (AF.7's exploration), and every step here asks for one.
            driver: IpcPortalDriver::with_path(p1_socket).with_timeout(Duration::from_secs(15)),
            outputs: Rpcs3Outputs::new(config_dir),
            log: None,
        }
    }

    /// RPCS3's log, so [`Control::savestate`] fails as soon as RPCS3 turns the
    /// save down instead of waiting out its timeout.
    pub fn with_log(mut self, log: impl Into<PathBuf>) -> Self {
        self.log = Some(log.into());
        self
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
        done: impl FnMut(&EmuState) -> bool,
    ) -> Result<EmuState> {
        self.wait_state_or(timeout, done, || Ok(()))
    }

    /// [`Control::wait_state`], failing as soon as `check` does.
    fn wait_state_or(
        &self,
        timeout: Duration,
        mut done: impl FnMut(&EmuState) -> bool,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<EmuState> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        loop {
            check()?;
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
    /// `status=paused frames=<flip>`. At a pause the target goes in first and
    /// then `RESUME`, so it can't race the counter (a checkpoint's step: observe,
    /// next target, resume). O3 drops a target the RSX thread first sees past
    /// rather than pause late, so the counter running past `flip` fails at once
    /// instead of at the timeout.
    pub fn pause_at(&self, flip: u64, timeout: Duration) -> Result<EmuState> {
        let before = self.state()?;
        self.verbs.send(Verb::PauseAt(flip))?;
        if before.status == "paused" {
            self.resume()?;
        }
        let state = self.wait_state(timeout, |s| s.status == "paused" || s.frames > flip)?;
        if state.status != "paused" {
            bail!(
                "PAUSE_AT {flip} went by without a pause: STATE reads {} frames={} \
                 (O3 drops a target the RSX thread first sees past; RPCS3's log says so)",
                state.status,
                state.frames
            );
        }
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
    /// caller's, once this returns. With [`Control::with_log`] a save RPCS3
    /// turns down ([`SAVESTATE_REFUSED`]) fails at once with RPCS3's reason;
    /// without it, at the timeout.
    pub fn savestate(&self, out_dir: &Path, timeout: Duration) -> Result<Saved> {
        let before: BTreeSet<PathBuf> = self.outputs.list(Kind::SaveState);
        let saved_at = self.state()?;
        // RPCS3's log from here on: a save it turns down leaves the run where it
        // was, so the wait below would otherwise sit out its whole timeout.
        let mut log = self.log.clone().map(LogTail::new);
        if let Some(log) = log.as_mut() {
            log.lines();
        }
        self.verbs.send(Verb::SaveState)?;
        // The old session goes first: a status other than running or paused, or
        // the counter starting over.
        self.wait_state_or(
            timeout,
            |s| !matches!(s.status.as_str(), "running" | "paused") || s.frames < saved_at.frames,
            || match log.as_mut().and_then(|l| {
                l.lines()
                    .into_iter()
                    .find(|l| l.contains(SAVESTATE_REFUSED))
            }) {
                Some(line) => bail!(
                    "RPCS3 turned the savestate down at frame {}, the run left as it was: {}",
                    saved_at.frames,
                    line.trim()
                ),
                None => Ok(()),
            },
        )?;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rrc_dump::capture::{RegState, ReplayCommand};
    use rrc_dump::nv4097 as m;

    use super::*;

    /// A capture holding just these `(register index, value)` commands, each
    /// with the header RPCS3 records (`reg << 2 | 1 << 18`).
    fn capture(cmds: &[(u32, u32)]) -> rrc_dump::Capture {
        rrc_dump::Capture {
            version: 6,
            tile_states: 0,
            display_buffer_states: 0,
            blocks: HashMap::new(),
            data: HashMap::new(),
            commands: cmds
                .iter()
                .map(|&(reg, value)| ReplayCommand {
                    header: (reg << 2) | (1 << 18),
                    value,
                    memory_state: Vec::new(),
                    tile_state: 0,
                    display_buffer_state: 0,
                })
                .collect(),
            reg_state: RegState {
                transform_program: Vec::new(),
                transform_constants: None,
                registers: Vec::new(),
            },
        }
    }

    const FLIP_HEAD_1: u32 = 0xE924 >> 2;
    const QUEUE_1: u32 = 0xE944 >> 2;
    /// Two draws: a primitive opened and closed, twice.
    const DRAWS: [(u32, u32); 4] = [
        (m::SET_BEGIN_END, 5),
        (m::SET_BEGIN_END, 0),
        (m::SET_BEGIN_END, 6),
        (m::SET_BEGIN_END, 0),
    ];

    #[test]
    fn a_giants_capture_is_one_queued_frame() {
        // The shape all three of AF.5's live captures had: the capture-start
        // NOP, setup, the flip head, the frame's draws, its own queue last.
        let mut cmds = vec![(m::NO_OPERATION, 0), (m::SET_TRANSFORM_PROGRAM, 9)];
        cmds.push((FLIP_HEAD_1, 0x8000_010f));
        cmds.extend(DRAWS);
        cmds.push((QUEUE_1, 1));
        let bounds = frame_bounds(&capture(&cmds));
        assert_eq!(
            bounds,
            FrameBounds {
                opening_flip: Some(2),
                draws_before_opening_flip: 0,
                closing_queue: Some(1),
                later_flips: 0,
            }
        );
        assert!(bounds.is_one_queued_frame());
    }

    #[test]
    fn anything_else_is_not_one_queued_frame() {
        let flip = (FLIP_HEAD_1, 0x8000_010f);
        let queue = (QUEUE_1, 0);
        let cases: [(&str, Vec<(u32, u32)>); 4] = [
            // An HLE game's flips don't show as flip heads.
            ("no flip head", [&DRAWS[..], &[queue]].concat()),
            (
                "draws before the flip",
                [&DRAWS[..], &[flip], &DRAWS[..], &[queue]].concat(),
            ),
            ("no queue at the end", [&[flip][..], &DRAWS[..]].concat()),
            (
                "two flips",
                [&[flip][..], &DRAWS[..], &[flip], &DRAWS[..], &[queue]].concat(),
            ),
        ];
        for (label, cmds) in &cases {
            let bounds = frame_bounds(&capture(cmds));
            assert!(!bounds.is_one_queued_frame(), "{label}: {bounds:?}");
        }
        let two = frame_bounds(&capture(&cases[3].1));
        assert_eq!((two.opening_flip, two.later_flips), (Some(0), 1));
        let early = frame_bounds(&capture(&cases[1].1));
        assert_eq!(early.draws_before_opening_flip, 2);
    }
}
