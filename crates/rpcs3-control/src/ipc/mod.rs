//! IPC portal driver (PLAN 16.5).
//!
//! Drives the patched RPCS3 emulated Skylander portal over an **AF_UNIX
//! socket** — no Skylanders Manager dialog, no UI Automation, no focus games.
//! Drops in behind the existing [`PortalDriver`] trait beside `UiaPortalDriver`
//! / `MockPortalDriver`. Transport: `std::os::unix::net::UnixStream` on unix,
//! `uds_windows::UnixStream` on Windows (both blocking — the trait is sync and
//! the server already calls it inside `spawn_blocking`).
//!
//! **Slot model (PLAN 16.x decision):** the *emulator* owns slot numbering.
//! `LOAD` places a figure in the emulator's first free slot and reports which
//! one; the controller reflects that rather than dictating a position. So the
//! trait's `load(slot, …)` slot argument is a **hint only** on this driver. The
//! user cares about portal *contents*, not positions — the positional model the
//! old UIA path implied is being collapsed (the phone never surfaced it).

pub mod proto;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use skylander_core::{SLOT_COUNT, SlotIndex, SlotState};

use crate::PortalDriver;
use proto::{Command, SlotOccupancy};

#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(windows)]
use uds_windows::UnixStream;

/// Default per-op round-trip timeout. Portal ops are human-driven and fast; a
/// generous ceiling guards against a wedged emulator without hanging the worker.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// `PortalDriver` over the patched RPCS3 AF_UNIX IPC channel.
pub struct IpcPortalDriver {
    path: PathBuf,
    timeout: Duration,
    /// Best-effort slot → display-name memory, populated by our own `load`
    /// calls. `STATUS` only returns NUID serials, so this lets `read_slots`
    /// report a real name for slots we placed; an occupied slot we didn't place
    /// falls back to the serial hex. Mapping a name back to a pack figure is a
    /// higher layer's job (`reconcile_slot_names` in the server).
    names: Mutex<[Option<String>; SLOT_COUNT]>,
}

impl IpcPortalDriver {
    /// Target the socket at `$SKYLANDER_IPC_PATH` (matching the patched RPCS3's
    /// own resolution) or the per-platform default.
    pub fn new() -> Result<Self> {
        Ok(Self::with_path(default_socket_path()))
    }

    /// Target an explicit socket path (tests, non-default deployments).
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            timeout: DEFAULT_TIMEOUT,
            names: Mutex::new(std::array::from_fn(|_| None::<String>)),
        }
    }

    /// Override the per-op timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The socket path this driver targets.
    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    /// Connect, send one command, and return the first non-heartbeat reply line
    /// (trimmed of CR/LF). A fresh connection per op keeps the driver stateless
    /// — no partial-read buffer to recover across calls — which suits the low,
    /// human-driven op rate and matches the emulator's accept-per-connection
    /// server.
    fn roundtrip(&self, cmd: &Command<'_>) -> Result<String> {
        let mut stream = UnixStream::connect(&self.path)
            .with_context(|| format!("connect RPCS3 IPC socket {}", self.path.display()))?;
        stream.set_read_timeout(Some(self.timeout)).ok();
        stream.set_write_timeout(Some(self.timeout)).ok();

        stream
            .write_all(cmd.encode().as_bytes())
            .context("send IPC command")?;
        stream.flush().ok();

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).context("read IPC reply")?;
            if n == 0 {
                bail!("RPCS3 IPC connection closed before a reply");
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            // Pushes (the ~1 Hz `HB` heartbeat and `PE` portal-events, P4)
            // interleave with replies; skip them while waiting for this command's
            // answer. Without the PE skip a portal-event pushed between a command
            // and its reply would be mis-parsed as the reply and break a live run.
            if trimmed.is_empty() || proto::is_heartbeat(trimmed) || proto::is_portal_event(trimmed)
            {
                continue;
            }
            return Ok(trimmed.to_string());
        }
    }

    /// Read the emulator's programmatic run state (`STATE`). Not part of the
    /// `PortalDriver` trait — exposed for the 16.7 freeze supervisor.
    pub fn read_state(&self) -> Result<proto::EmuState> {
        let reply = self.roundtrip(&Command::State)?;
        proto::parse_state(&reply)
    }

    /// Read the native game-window handle (`WINDOW`), `0` until the window is
    /// created. Not part of the trait — for the 16.6 window coordinator.
    pub fn window_handle(&self) -> Result<u64> {
        let reply = self.roundtrip(&Command::Window)?;
        proto::parse_window(&reply)
    }

    /// Liveness check (`PING` → `PONG`).
    pub fn ping(&self) -> Result<()> {
        let reply = self.roundtrip(&Command::Ping)?;
        if reply == "PONG" {
            Ok(())
        } else {
            bail!("unexpected PING reply: {reply:?}")
        }
    }

    /// Open **one persistent connection** and stream every *pushed* line — the
    /// 1 Hz `HB` heartbeat and the `PE` portal-event pushes (P4) — to `on_line`
    /// until `dur` elapses. The patched emulator pushes on its own 1 s `select`
    /// timeout, so a connection that sends nothing still receives the full feed.
    /// Each call gets `(elapsed_since_start, line)`, trimmed of CR/LF.
    ///
    /// Diagnostic for PLAN 15.12: watch whether the guest game keeps polling the
    /// portal (`PE cmd=status`) after a save-state resume, and whether a
    /// `PE cmd=query` burst follows a late `LOAD` (= the game re-read the figure).
    /// Not part of the `PortalDriver` trait.
    pub fn watch_events<F: FnMut(Duration, &str)>(
        &self,
        dur: Duration,
        mut on_line: F,
    ) -> Result<()> {
        let start = Instant::now();
        let stream = UnixStream::connect(&self.path)
            .with_context(|| format!("connect RPCS3 IPC socket {}", self.path.display()))?;
        // A read timeout shorter than the deadline lets us re-check `dur` between
        // pushes (and notice a stream that has gone silent — the very signal we
        // are looking for). A timeout normally fires on an empty socket; if it
        // ever lands mid-frame, the loop below preserves the partial bytes.
        stream
            .set_read_timeout(Some(Duration::from_millis(300)))
            .ok();

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while start.elapsed() < dur {
            match reader.read_line(&mut line) {
                Ok(0) => bail!("RPCS3 IPC connection closed while watching events"),
                Ok(_) => {
                    // read_line stops at '\n'; pushes are newline-framed. Hand the
                    // (trimmed) line to the callback, then reset for the next one.
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if !trimmed.is_empty() {
                        on_line(start.elapsed(), trimmed);
                    }
                    line.clear();
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Read timeout on an idle (or mid-frame) socket. Leave any
                    // partial bytes in `line` — AF_UNIX is SOCK_STREAM, so a frame
                    // can split across reads and the next read_line resumes it —
                    // and loop to re-check the deadline.
                }
                Err(e) => return Err(e).context("read IPC push stream"),
            }
        }
        Ok(())
    }
}

impl PortalDriver for IpcPortalDriver {
    /// No-op: the IPC path has no Skylanders Manager dialog. Kept so the driver
    /// drops in behind the trait (the server calls this before every load/clear).
    fn open_dialog(&self) -> Result<()> {
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        let reply = self.roundtrip(&Command::Status)?;
        let occ = proto::parse_status(&reply)?;
        let names = self.names.lock().unwrap();
        Ok(std::array::from_fn(|i| match occ[i] {
            SlotOccupancy::Empty => SlotState::Empty,
            SlotOccupancy::Occupied { serial } => SlotState::Loaded {
                figure_id: None,
                display_name: names[i].clone().unwrap_or_else(|| format!("{serial:08X}")),
                placed_by: None,
            },
        }))
    }

    /// Load the `.sky` at `path`. The **emulator assigns the slot** (see the
    /// module note); the `_slot_hint` is ignored. Returns a best-effort display
    /// name (the file stem) — the server overrides it with the pack's canonical
    /// name regardless (`DriverJob::LoadFigure.canonical_name`).
    fn load(&self, _slot_hint: SlotIndex, path: &Path) -> Result<String> {
        // The patched RPCS3 P1 `LOAD` handler opens the path against *its own*
        // process CWD (which differs from the server's), so a server-relative
        // working-copy path like `dev-data/working/…/x.sky` resolves to nothing on
        // the emulator side and comes back `ERR open_failed`. Resolve to an absolute
        // path here — lexically, against the server's CWD — to honour the handler's
        // documented contract ("arg = absolute path"). `std::path::absolute` (vs
        // `canonicalize`) avoids the `\\?\` extended-length prefix, which RPCS3's
        // `fs::file` path-handling doesn't expect.
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let reply = self.roundtrip(&Command::Load(&abs))?;
        let assigned = proto::parse_load(&reply)?;

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".to_string());

        if let Some(slot) = self.names.lock().unwrap().get_mut(assigned as usize) {
            *slot = Some(name.clone());
        }
        Ok(name)
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        let reply = self.roundtrip(&Command::Clear(slot.as_u8()))?;
        proto::parse_ok(&reply)?;
        if let Some(s) = self.names.lock().unwrap().get_mut(slot.as_usize()) {
            *s = None;
        }
        Ok(())
    }

    // --- IPC capability (PLAN 16.6): the launcher uses these to no-GUI-boot RPCS3,
    // wait on the liveness signal, and position the window. ---

    fn ipc_socket_path(&self) -> Option<std::path::PathBuf> {
        Some(self.path.clone())
    }

    fn emu_state(&self) -> Result<Option<proto::EmuState>> {
        Ok(Some(self.read_state()?))
    }

    fn game_window_handle(&self) -> Result<Option<u64>> {
        let h = self.window_handle()?;
        Ok((h != 0).then_some(h))
    }
}

/// Mirror the patched RPCS3's socket-path resolution: `$SKYLANDER_IPC_PATH`,
/// else a per-platform default under the temp dir.
pub fn default_socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SKYLANDER_IPC_PATH") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        let tmp = std::env::var_os("TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Temp"));
        tmp.join("rpcs3-skylander.sock")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/tmp/rpcs3-skylander.sock")
    }
}
