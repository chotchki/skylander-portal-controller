//! Wire protocol for the patched-RPCS3 Skylander AF_UNIX IPC channel
//! (PLAN 16.3.2). Newline-terminated ASCII. This module is a **pure codec** —
//! no sockets, no I/O — so every line shape unit-tests without a connection.
//!
//! The shapes here mirror the patched emulator exactly (`rpcs3-patches/`,
//! `Emu/Io/Skylander.cpp::sky_ipc_handle`):
//!
//! ```text
//! ->  PING                     <-  PONG
//! ->  STATE                    <-  OK status=<s> frames=<n> progr=<a>/<b> seg=<c>/<d>
//! ->  STATUS                   <-  OK 0:<serial|empty> 1:<..> .. 7:<..>
//! ->  WINDOW                   <-  OK handle=<hex>
//! ->  LOAD <abs .sky path>     <-  OK slot=<n>   | ERR <reason>
//! ->  CLEAR <slot 0-7>         <-  OK            | ERR <reason>
//! (push, ~1 Hz)                <-  HB status=<s> frames=<n> progr=<a>/<b> seg=<c>/<d>
//! ```

use std::path::Path;

use anyhow::{Context, Result, bail};
use skylander_core::SLOT_COUNT;

/// A command the controller sends to the patched emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command<'a> {
    Ping,
    State,
    Status,
    Window,
    /// Load the `.sky` at this path. The **emulator** assigns the slot (it
    /// places into its first free slot and reports which); the controller does
    /// not dictate a position.
    Load(&'a Path),
    /// Clear the figure currently in this slot (`0..SLOT_COUNT`).
    Clear(u8),
}

impl Command<'_> {
    /// Encode to the newline-terminated wire form.
    pub fn encode(&self) -> String {
        match self {
            Command::Ping => "PING\n".to_string(),
            Command::State => "STATE\n".to_string(),
            Command::Status => "STATUS\n".to_string(),
            Command::Window => "WINDOW\n".to_string(),
            // The server takes everything after the first space as the path
            // (verbatim, to end-of-line), so paths with spaces need no quoting.
            Command::Load(p) => format!("LOAD {}\n", p.display()),
            Command::Clear(slot) => format!("CLEAR {slot}\n"),
        }
    }
}

/// Emulator runtime state, parsed from a `STATE` reply or an `HB` heartbeat.
/// This is the clean liveness signal that replaces log-scraping / shader-compile
/// guessing (PLAN 16.1.2) — and what the 16.7 freeze supervisor will watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmuState {
    /// `running` / `paused` / `frozen` / `loading` / `stopped` / ...
    pub status: String,
    /// RSX `int_flip_index` — advances ~60/s while a game renders.
    pub frames: u64,
    pub progr_done: u32,
    pub progr_total: u32,
    pub seg_done: u32,
    pub seg_total: u32,
}

impl EmuState {
    /// True when the game is actually rendering: `running`, at least one frame
    /// flipped, and **both** compile-progress phases complete — `progr` (e.g. PPU
    /// module analysis) AND `seg` (e.g. shader segments). RPCS3 runs these as two
    /// phases, so checking only `progr` reports playable while shaders are still
    /// compiling — the launcher would reveal the game mid-compile (HTPC 2026-05-30).
    /// A `running` status with a stalled `frames` counter is the freeze signal
    /// (PLAN 16.7.1).
    pub fn is_playable(&self) -> bool {
        self.status == "running"
            && self.frames > 0
            && (self.progr_total == 0 || self.progr_done >= self.progr_total)
            && (self.seg_total == 0 || self.seg_done >= self.seg_total)
    }
}

/// Per-slot occupancy from a `STATUS` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOccupancy {
    Empty,
    /// 4-byte little-endian Mifare NUID serial (the `<uid>.sky` key).
    Occupied {
        serial: u32,
    },
}

/// True for a heartbeat push line (skipped while awaiting a command reply).
pub fn is_heartbeat(line: &str) -> bool {
    line == "HB" || line.starts_with("HB ")
}

/// Split an `OK ...` / `OK` / `ERR <reason>` reply, returning the payload after
/// `OK ` (empty for a bare `OK`) or a built error from the `ERR` reason.
fn expect_ok(line: &str) -> Result<&str> {
    if let Some(rest) = line.strip_prefix("OK ") {
        Ok(rest)
    } else if line == "OK" {
        Ok("")
    } else if let Some(reason) = line.strip_prefix("ERR ") {
        bail!("emulator rejected command: {reason}")
    } else if line == "ERR" {
        bail!("emulator rejected command (unspecified)")
    } else {
        bail!("malformed IPC reply: {line:?}")
    }
}

/// `OK` / `ERR <reason>` → unit. Used by `CLEAR`.
pub fn parse_ok(line: &str) -> Result<()> {
    expect_ok(line).map(|_| ())
}

/// `OK slot=<n>` → the slot the emulator placed the figure in. Used by `LOAD`.
pub fn parse_load(line: &str) -> Result<u8> {
    let rest = expect_ok(line)?;
    let n = rest
        .strip_prefix("slot=")
        .with_context(|| format!("LOAD reply missing `slot=`: {line:?}"))?;
    let slot: u8 = n
        .trim()
        .parse()
        .with_context(|| format!("LOAD reply has non-numeric slot: {n:?}"))?;
    if (slot as usize) >= SLOT_COUNT {
        bail!("emulator returned out-of-range slot {slot}");
    }
    Ok(slot)
}

/// `OK handle=<hex>` → native game-window handle. Used by `WINDOW` (PLAN 16.4).
pub fn parse_window(line: &str) -> Result<u64> {
    let rest = expect_ok(line)?;
    let h = rest
        .strip_prefix("handle=")
        .with_context(|| format!("WINDOW reply missing `handle=`: {line:?}"))?;
    u64::from_str_radix(h.trim(), 16).with_context(|| format!("WINDOW reply bad hex: {h:?}"))
}

/// `OK 0:<serial|empty> .. 7:<..>` → per-slot occupancy. Used by `STATUS`.
pub fn parse_status(line: &str) -> Result<[SlotOccupancy; SLOT_COUNT]> {
    let rest = expect_ok(line)?;
    let mut out = [SlotOccupancy::Empty; SLOT_COUNT];
    let mut seen = 0usize;
    for tok in rest.split_whitespace() {
        let (idx, val) = tok
            .split_once(':')
            .with_context(|| format!("STATUS token missing ':' — {tok:?}"))?;
        let i: usize = idx
            .parse()
            .with_context(|| format!("STATUS bad slot index {idx:?}"))?;
        if i >= SLOT_COUNT {
            bail!("STATUS slot index {i} out of range");
        }
        out[i] = if val == "empty" {
            SlotOccupancy::Empty
        } else {
            SlotOccupancy::Occupied {
                serial: u32::from_str_radix(val, 16)
                    .with_context(|| format!("STATUS bad serial {val:?}"))?,
            }
        };
        seen += 1;
    }
    if seen != SLOT_COUNT {
        bail!("STATUS reported {seen} slots, expected {SLOT_COUNT}");
    }
    Ok(out)
}

/// Parse the `status=.. frames=.. progr=a/b seg=c/d` body of a `STATE` reply or
/// an `HB` heartbeat (accepts both the `OK ` and `HB ` prefixes). Unknown
/// fields are ignored for forward-compat.
pub fn parse_state(line: &str) -> Result<EmuState> {
    let body = line
        .strip_prefix("OK ")
        .or_else(|| line.strip_prefix("HB "))
        .with_context(|| format!("not a STATE/HB line: {line:?}"))?;

    let mut status = None;
    let mut frames = None;
    let (mut pd, mut pt, mut sd, mut st) = (0u32, 0u32, 0u32, 0u32);
    for field in body.split_whitespace() {
        let (k, v) = field
            .split_once('=')
            .with_context(|| format!("STATE field missing '=': {field:?}"))?;
        match k {
            "status" => status = Some(v.to_string()),
            "frames" => frames = Some(v.parse().with_context(|| format!("bad frames {v:?}"))?),
            "progr" => (pd, pt) = split_ratio(v)?,
            "seg" => (sd, st) = split_ratio(v)?,
            _ => {} // forward-compat: ignore fields we don't know
        }
    }
    Ok(EmuState {
        status: status.context("STATE missing `status=`")?,
        frames: frames.context("STATE missing `frames=`")?,
        progr_done: pd,
        progr_total: pt,
        seg_done: sd,
        seg_total: st,
    })
}

fn split_ratio(v: &str) -> Result<(u32, u32)> {
    let (a, b) = v
        .split_once('/')
        .with_context(|| format!("ratio missing '/': {v:?}"))?;
    Ok((
        a.parse()
            .with_context(|| format!("bad ratio numerator {a:?}"))?,
        b.parse()
            .with_context(|| format!("bad ratio denominator {b:?}"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn encode_shapes() {
        assert_eq!(Command::Ping.encode(), "PING\n");
        assert_eq!(Command::Status.encode(), "STATUS\n");
        assert_eq!(Command::Clear(3).encode(), "CLEAR 3\n");
        // Paths with spaces are not quoted (server takes the line remainder).
        assert_eq!(
            Command::Load(Path::new("/pack/Fire/Gill Grunt.sky")).encode(),
            "LOAD /pack/Fire/Gill Grunt.sky\n"
        );
    }

    #[test]
    fn parse_load_ok_and_err() {
        assert_eq!(parse_load("OK slot=0").unwrap(), 0);
        assert_eq!(parse_load("OK slot=7").unwrap(), 7);
        assert!(parse_load("OK slot=8").is_err()); // out of range
        let e = parse_load("ERR portal_full").unwrap_err().to_string();
        assert!(e.contains("portal_full"), "{e}");
        assert!(parse_load("OK").is_err()); // missing slot=
    }

    #[test]
    fn parse_ok_unit() {
        assert!(parse_ok("OK").is_ok());
        let e = parse_ok("ERR not_loaded").unwrap_err().to_string();
        assert!(e.contains("not_loaded"), "{e}");
        assert!(parse_ok("garbage").is_err());
    }

    #[test]
    fn parse_status_mixed() {
        let occ = parse_status(
            "OK 0:7FC1ADA3 1:empty 2:empty 3:0000ABCD 4:empty 5:empty 6:empty 7:empty",
        )
        .unwrap();
        assert_eq!(
            occ[0],
            SlotOccupancy::Occupied {
                serial: 0x7FC1_ADA3
            }
        );
        assert_eq!(occ[1], SlotOccupancy::Empty);
        assert_eq!(
            occ[3],
            SlotOccupancy::Occupied {
                serial: 0x0000_ABCD
            }
        );
        assert_eq!(occ[7], SlotOccupancy::Empty);
    }

    #[test]
    fn parse_status_rejects_wrong_count() {
        assert!(parse_status("OK 0:empty 1:empty").is_err());
    }

    #[test]
    fn parse_window_hex() {
        assert_eq!(parse_window("OK handle=120412").unwrap(), 0x0012_0412);
        assert_eq!(parse_window("OK handle=0").unwrap(), 0);
        assert!(parse_window("OK handle=zzz").is_err());
    }

    #[test]
    fn parse_state_and_heartbeat() {
        let st = parse_state("OK status=running frames=312 progr=8/8 seg=0/0").unwrap();
        assert_eq!(st.status, "running");
        assert_eq!(st.frames, 312);
        assert_eq!((st.progr_done, st.progr_total), (8, 8));
        assert!(st.is_playable());

        // Heartbeat shape parses identically.
        let hb = parse_state("HB status=loading frames=0 progr=2/8 seg=1/4").unwrap();
        assert_eq!(hb.status, "loading");
        assert!(!hb.is_playable()); // not running, no frames, progress incomplete

        assert!(is_heartbeat("HB status=running frames=1 progr=0/0 seg=0/0"));
        assert!(!is_heartbeat("OK slot=0"));
    }

    #[test]
    fn parse_state_frozen_status() {
        // RPCS3's own fatal ("Emulation has been frozen!") reports status=frozen
        // while the process stays alive — the wire value the freeze supervisor
        // keys on (PLAN 16.10.2). It must parse cleanly and never read playable.
        let st = parse_state("OK status=frozen frames=9973 progr=8/8 seg=4/4").unwrap();
        assert_eq!(st.status, "frozen");
        assert!(!st.is_playable(), "a frozen emulator is not playable");
    }

    #[test]
    fn playable_needs_frames_and_progress() {
        let stalled = EmuState {
            status: "running".into(),
            frames: 0, // not rendering yet (or frozen at 0)
            progr_done: 8,
            progr_total: 8,
            seg_done: 0,
            seg_total: 0,
        };
        assert!(!stalled.is_playable());
    }
}
