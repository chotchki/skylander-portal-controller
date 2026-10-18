//! The O-series capture and control verbs (openspec AF.5, design decision 3):
//! the codec and a client on P1's socket. O3 (`rpcs3-patches/oracle/`) adds
//! them to P1's line protocol in `Emu/Io/Skylander.cpp`, answered only when
//! `GUAC_ORACLE=1` is in the emulator's environment; without it each one gets
//! P1's `ERR unknown_cmd`, like any word P1 doesn't know.
//!
//! ```text
//! ->  PAUSE_AT <flip>   <-  OK | ERR bad_flip | ERR no_renderer | ERR past
//! ->  RESUME            <-  OK | ERR not_paused
//! ->  RSXCAP            <-  OK | ERR busy | ERR no_renderer
//! ->  SCREENSHOT        <-  OK flip=<n> | ERR busy | ERR no_renderer | ERR timeout
//! ->  SAVESTATE         <-  OK | ERR not_running
//! (any of the five, gate off)  <-  ERR unknown_cmd
//! ```
//!
//! `<flip>` is the counter `STATE` and the heartbeat report as `frames=` (RSX's
//! `int_flip_index`), decimal. Pushes (`HB`, `PE`) interleave with replies, as
//! on P1. The codec is pure: every line shape unit-tests without a socket.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

/// An O-verb, one P1 request line each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Pause when the flip counter reaches this flip; `STATE` then reads
    /// `status=paused frames=<flip>`.
    PauseAt(u64),
    /// Resume a paused emulator on its main thread.
    Resume,
    /// Arm an RSX capture: RPCS3 starts recording at the next frame end and, at
    /// the one after, writes `captures/<TITLE>_<time>_capture.rrc.gz` and pauses
    /// there, after that frame's queue and before its flip (Giants' LLE GCM
    /// path; [`crate::control::frame_bounds`]). Armed at a pause at flip n,
    /// `STATE` reads n+1 and the captured frame is n+2's.
    RsxCap,
    /// A PNG of the frame the counter names (the next present while running; the
    /// current frame, re-presented, while paused).
    Screenshot,
    /// Stop, write a savestate and boot it again (Start Paused holds it 2 flips in).
    SaveState,
}

impl Verb {
    /// The wire word.
    pub fn name(self) -> &'static str {
        match self {
            Verb::PauseAt(_) => "PAUSE_AT",
            Verb::Resume => "RESUME",
            Verb::RsxCap => "RSXCAP",
            Verb::Screenshot => "SCREENSHOT",
            Verb::SaveState => "SAVESTATE",
        }
    }

    /// The newline-terminated request line.
    pub fn encode(self) -> String {
        match self {
            Verb::PauseAt(flip) => format!("PAUSE_AT {flip}\n"),
            other => format!("{}\n", other.name()),
        }
    }

    /// The refusals this verb can get besides `unknown_cmd`, which every verb
    /// gets with the gate off.
    pub fn refusals(self) -> &'static [Refusal] {
        match self {
            Verb::PauseAt(_) => &[Refusal::BadFlip, Refusal::NoRenderer, Refusal::Past],
            Verb::Resume => &[Refusal::NotPaused],
            Verb::RsxCap => &[Refusal::Busy, Refusal::NoRenderer],
            Verb::Screenshot => &[Refusal::Busy, Refusal::NoRenderer, Refusal::Timeout],
            Verb::SaveState => &[Refusal::NotRunning],
        }
    }
}

/// An `ERR <reason>` an O-verb can get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `bad_flip`: not a positive decimal flip number.
    BadFlip,
    /// `no_renderer`: RSX isn't up yet (or is gone, mid-savestate).
    NoRenderer,
    /// `past`: the counter is at or beyond that flip already.
    Past,
    /// `not_paused`: `RESUME` while not paused.
    NotPaused,
    /// `busy`: a capture or screenshot request is already pending.
    Busy,
    /// `timeout`: no present took the screenshot request within 5 s; withdrawn.
    Timeout,
    /// `not_running`: `SAVESTATE` while neither running nor paused.
    NotRunning,
    /// `unknown_cmd`: the gate is off (`GUAC_ORACLE=1` unset), or the binary has
    /// no O-series.
    UnknownCmd,
}

impl Refusal {
    pub const ALL: [Refusal; 8] = [
        Refusal::BadFlip,
        Refusal::NoRenderer,
        Refusal::Past,
        Refusal::NotPaused,
        Refusal::Busy,
        Refusal::Timeout,
        Refusal::NotRunning,
        Refusal::UnknownCmd,
    ];

    /// The reason word after `ERR `.
    pub fn wire(self) -> &'static str {
        match self {
            Refusal::BadFlip => "bad_flip",
            Refusal::NoRenderer => "no_renderer",
            Refusal::Past => "past",
            Refusal::NotPaused => "not_paused",
            Refusal::Busy => "busy",
            Refusal::Timeout => "timeout",
            Refusal::NotRunning => "not_running",
            Refusal::UnknownCmd => "unknown_cmd",
        }
    }

    fn from_wire(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.wire() == word)
    }
}

/// A verb's answer when it isn't refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Ok,
    /// `SCREENSHOT`'s `OK flip=<n>`: the counter when a present took the request
    /// (exact at a pause; while running, as P1's thread saw it just after).
    Shot {
        flip: u64,
    },
}

/// Why a reply line isn't the verb's success.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    #[error("{verb} refused: ERR {}", .refusal.wire())]
    Refused {
        verb: &'static str,
        refusal: Refusal,
    },
    /// A line this verb never gets: a refusal word it can't have, or anything
    /// else.
    #[error("{verb}: unexpected reply {line:?}")]
    Unexpected { verb: &'static str, line: String },
}

impl ReplyError {
    /// `None` for an [`ReplyError::Unexpected`] line.
    pub fn refusal(&self) -> Option<Refusal> {
        match self {
            ReplyError::Refused { refusal, .. } => Some(*refusal),
            ReplyError::Unexpected { .. } => None,
        }
    }
}

/// Parses `verb`'s reply line (trimmed of the newline).
pub fn parse_reply(verb: Verb, line: &str) -> Result<Reply, ReplyError> {
    let unexpected = || ReplyError::Unexpected {
        verb: verb.name(),
        line: line.to_owned(),
    };
    if let Some(word) = line.strip_prefix("ERR ") {
        return match Refusal::from_wire(word) {
            Some(r) if r == Refusal::UnknownCmd || verb.refusals().contains(&r) => {
                Err(ReplyError::Refused {
                    verb: verb.name(),
                    refusal: r,
                })
            }
            _ => Err(unexpected()),
        };
    }
    match (verb, line) {
        (Verb::Screenshot, _) => {
            let n = line.strip_prefix("OK flip=").ok_or_else(unexpected)?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return Err(unexpected());
            }
            let flip = n.parse().map_err(|_| unexpected())?;
            Ok(Reply::Shot { flip })
        }
        (_, "OK") => Ok(Reply::Ok),
        _ => Err(unexpected()),
    }
}

/// The O-verbs on one run's P1 socket, a fresh connection per request as
/// `IpcPortalDriver` does (the emulator serves each on a thread of its own).
#[derive(Debug, Clone)]
pub struct VerbClient {
    path: PathBuf,
    timeout: Duration,
}

impl VerbClient {
    /// `SCREENSHOT` holds its connection up to 5 s waiting for a present, so the
    /// default read timeout sits above that.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn new(p1_socket: impl Into<PathBuf>) -> Self {
        Self {
            path: p1_socket.into(),
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn socket(&self) -> &Path {
        &self.path
    }

    /// Sends `verb`; a refusal comes back as a [`ReplyError`] inside the error
    /// (`err.downcast_ref::<ReplyError>()`).
    pub fn send(&self, verb: Verb) -> Result<Reply> {
        let line = self.roundtrip_line(&verb.encode())?;
        Ok(parse_reply(verb, &line)?)
    }

    /// One request line (newline included) and the first reply line that isn't
    /// a push, trimmed.
    pub fn roundtrip_line(&self, request: &str) -> Result<String> {
        roundtrip(&self.path, self.timeout, request)
    }
}

#[cfg(unix)]
fn roundtrip(path: &Path, timeout: Duration, request: &str) -> Result<String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    use skylander_rpcs3_control::ipc::proto::{is_heartbeat, is_portal_event};

    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("connecting to P1 at {}", path.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(request.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).context("reading P1's reply")? == 0 {
            bail!("P1 closed the connection before replying to {request:?}");
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() || is_heartbeat(trimmed) || is_portal_event(trimmed) {
            continue;
        }
        return Ok(trimmed.to_owned());
    }
}

#[cfg(not(unix))]
fn roundtrip(_path: &Path, _timeout: Duration, _request: &str) -> Result<String> {
    bail!("the oracle is Mac only until the Windows/HTPC bring-up")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERBS: [Verb; 5] = [
        Verb::PauseAt(7),
        Verb::Resume,
        Verb::RsxCap,
        Verb::Screenshot,
        Verb::SaveState,
    ];

    #[test]
    fn each_verb_encodes_as_o3_reads_it() {
        assert_eq!(Verb::PauseAt(18_000).encode(), "PAUSE_AT 18000\n");
        assert_eq!(
            Verb::PauseAt(u64::MAX).encode(),
            format!("PAUSE_AT {}\n", u64::MAX)
        );
        assert_eq!(Verb::Resume.encode(), "RESUME\n");
        assert_eq!(Verb::RsxCap.encode(), "RSXCAP\n");
        assert_eq!(Verb::Screenshot.encode(), "SCREENSHOT\n");
        assert_eq!(Verb::SaveState.encode(), "SAVESTATE\n");
    }

    #[test]
    fn each_success_shape_parses() {
        for verb in [
            Verb::PauseAt(3),
            Verb::Resume,
            Verb::RsxCap,
            Verb::SaveState,
        ] {
            assert_eq!(parse_reply(verb, "OK"), Ok(Reply::Ok), "{verb:?}");
        }
        assert_eq!(
            parse_reply(Verb::Screenshot, "OK flip=1234"),
            Ok(Reply::Shot { flip: 1234 })
        );
        assert_eq!(
            parse_reply(Verb::Screenshot, &format!("OK flip={}", u64::MAX)),
            Ok(Reply::Shot { flip: u64::MAX })
        );
    }

    #[test]
    fn every_refusal_maps_for_the_verbs_that_get_it_and_no_other() {
        for verb in VERBS {
            for refusal in Refusal::ALL {
                let line = format!("ERR {}", refusal.wire());
                let got = parse_reply(verb, &line).unwrap_err();
                if refusal == Refusal::UnknownCmd || verb.refusals().contains(&refusal) {
                    assert_eq!(got.refusal(), Some(refusal), "{verb:?} {line}");
                    assert_eq!(got.to_string(), format!("{} refused: {line}", verb.name()));
                } else {
                    assert_eq!(got.refusal(), None, "{verb:?} can't get {line}");
                }
            }
        }
        // Exactly the table in the module doc.
        let counts: Vec<usize> = VERBS.iter().map(|v| v.refusals().len()).collect();
        assert_eq!(counts, [3, 1, 2, 3, 1]);
    }

    #[test]
    fn a_reply_off_the_table_is_unexpected() {
        let cases: &[(Verb, &str)] = &[
            (Verb::PauseAt(1), "OK slot=0"),
            (Verb::PauseAt(1), "OK "),
            (Verb::Resume, "PONG"),
            (Verb::Resume, ""),
            (Verb::RsxCap, "ERR"),
            (Verb::RsxCap, "ERR busy now"),
            (Verb::SaveState, "ERR something_new"),
            (Verb::SaveState, "ok"),
            // SCREENSHOT always names its flip.
            (Verb::Screenshot, "OK"),
            (Verb::Screenshot, "OK flip="),
            (Verb::Screenshot, "OK flip=12a"),
            (Verb::Screenshot, "OK flip=-1"),
            (Verb::Screenshot, "OK flip=+1"),
            (Verb::Screenshot, "OK flip=99999999999999999999"),
            (Verb::Screenshot, "OK flip=5 extra"),
        ];
        for (verb, line) in cases {
            let err = parse_reply(*verb, line).unwrap_err();
            assert_eq!(
                err,
                ReplyError::Unexpected {
                    verb: verb.name(),
                    line: (*line).to_owned()
                },
                "{verb:?} {line:?}"
            );
        }
    }
}
