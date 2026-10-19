//! The O-series verbs (openspec AF.5 and AF.7, design decision 3): the codec and
//! a client on P1's socket. O3 (capture and control) and O4 (frame-locked input)
//! in `rpcs3-patches/oracle/` add them to P1's line protocol in
//! `Emu/Io/Skylander.cpp`, answered only when `GUAC_ORACLE=1` is in the
//! emulator's environment; without it each one gets P1's `ERR unknown_cmd`, like
//! any word P1 doesn't know.
//!
//! ```text
//! ->  PAUSE_AT <flip>   <-  OK | ERR bad_flip | ERR no_renderer | ERR past
//! ->  RESUME            <-  OK | ERR not_paused
//! ->  RSXCAP            <-  OK | ERR busy | ERR no_renderer
//! ->  SCREENSHOT        <-  OK flip=<n> | ERR busy | ERR no_renderer | ERR timeout
//! ->  SAVESTATE         <-  OK | ERR not_running
//! ->  PAD <port> <flip> <d1> <d2> <lx> <ly> <rx> <ry>
//!                       <-  OK | ERR bad_args | ERR bad_port | ERR bad_flip | ERR bad_button
//!                           | ERR bad_axis | ERR no_renderer | ERR past | ERR full
//! ->  PAD_CLEAR         <-  OK | ERR bad_args
//! ->  PAD_STATE <port> [<flip>]
//!                       <-  OK flip=<n> reads=<k> differ=<m> d1=<hh> d2=<hh> lx=<n> ly=<n> rx=<n> ry=<n>
//!                           | ERR bad_args | ERR bad_port | ERR bad_flip | ERR no_read
//! (any of them, gate off)  <-  ERR unknown_cmd
//! ```
//!
//! `<flip>` is the counter `STATE` and the heartbeat report as `frames=` (RSX's
//! `int_flip_index`), decimal. A pad's words are `CELL_PAD_CTRL_*` bits in two hex
//! digits and its axes decimal bytes ([`guac_oracle::input::Ds3Pad`]). Pushes
//! (`HB`, `PE`) interleave with replies, as on P1. The codec is pure: every line
//! shape unit-tests without a socket.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use guac_oracle::input::{Ds3Pad, Stick};

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
    /// From `flip` on, `port` (0 or 1) reads `pad`, held until that port's next
    /// entry; one at a flip that already has an entry replaces it. The flip has
    /// to be ahead of the counter, so a schedule goes in at a pause.
    Pad { port: u8, flip: u64, pad: Ds3Pad },
    /// Both schedules dropped: the guest reads the host's input again.
    PadClear,
    /// What the guest read on `port`: its last read, or the first read of `flip`.
    PadState { port: u8, flip: Option<u64> },
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
            Verb::Pad { .. } => "PAD",
            Verb::PadClear => "PAD_CLEAR",
            Verb::PadState { .. } => "PAD_STATE",
        }
    }

    /// The newline-terminated request line.
    pub fn encode(self) -> String {
        match self {
            Verb::PauseAt(flip) => format!("PAUSE_AT {flip}\n"),
            Verb::Pad { port, flip, pad } => format!(
                "PAD {port} {flip} {:02x} {:02x} {} {} {} {}\n",
                pad.digital1, pad.digital2, pad.left.x, pad.left.y, pad.right.x, pad.right.y
            ),
            Verb::PadState { port, flip: None } => format!("PAD_STATE {port}\n"),
            Verb::PadState {
                port,
                flip: Some(flip),
            } => format!("PAD_STATE {port} {flip}\n"),
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
            Verb::Pad { .. } => &[
                Refusal::BadArgs,
                Refusal::BadPort,
                Refusal::BadFlip,
                Refusal::BadButton,
                Refusal::BadAxis,
                Refusal::NoRenderer,
                Refusal::Past,
                Refusal::Full,
            ],
            Verb::PadClear => &[Refusal::BadArgs],
            Verb::PadState { .. } => &[
                Refusal::BadArgs,
                Refusal::BadPort,
                Refusal::BadFlip,
                Refusal::NoRead,
            ],
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
    /// `bad_args`: a `PAD` without its eight fields, or anything after
    /// `PAD_CLEAR`, or a `PAD_STATE` without a port or with more than a flip.
    BadArgs,
    /// `bad_port`: a port other than 0 or 1.
    BadPort,
    /// `bad_button`: a digital word that isn't 1-4 hex digits up to `ff` (the PS
    /// button's bit and above are refused).
    BadButton,
    /// `bad_axis`: an axis that isn't a decimal 0-255.
    BadAxis,
    /// `full`: the port's schedule already holds 65,536 entries.
    Full,
    /// `no_read`: the guest made no read in that flip (or none since the
    /// session began), or it's past the 4,096 flips O4 keeps.
    NoRead,
}

impl Refusal {
    pub const ALL: [Refusal; 14] = [
        Refusal::BadFlip,
        Refusal::NoRenderer,
        Refusal::Past,
        Refusal::NotPaused,
        Refusal::Busy,
        Refusal::Timeout,
        Refusal::NotRunning,
        Refusal::UnknownCmd,
        Refusal::BadArgs,
        Refusal::BadPort,
        Refusal::BadButton,
        Refusal::BadAxis,
        Refusal::Full,
        Refusal::NoRead,
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
            Refusal::BadArgs => "bad_args",
            Refusal::BadPort => "bad_port",
            Refusal::BadButton => "bad_button",
            Refusal::BadAxis => "bad_axis",
            Refusal::Full => "full",
            Refusal::NoRead => "no_read",
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
    /// `PAD_STATE`'s answer.
    PadRead(PadRead),
}

/// What the guest read on a port, as O4 records it inside `pad_get_data`: the
/// state it holds after a read, which changes only with a report that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadRead {
    /// The flip the read fell in (the counter when it began).
    pub flip: u64,
    /// The guest's reads of this port during that flip.
    pub reads: u32,
    /// How many of them read a state other than the flip's first read: 0 is the
    /// frame lock holding.
    pub differ: u32,
    /// The last read's state for `PAD_STATE <port>`, the flip's first read's for
    /// `PAD_STATE <port> <flip>`.
    pub pad: Ds3Pad,
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
        (Verb::PadState { .. }, _) => parse_pad_read(line)
            .map(Reply::PadRead)
            .ok_or_else(unexpected),
        (_, "OK") => Ok(Reply::Ok),
        _ => Err(unexpected()),
    }
}

/// `OK flip=<n> reads=<k> differ=<m> d1=<hh> d2=<hh> lx=<n> ly=<n> rx=<n> ry=<n>`,
/// fields in that order, as O4's `snprintf` writes them.
fn parse_pad_read(line: &str) -> Option<PadRead> {
    fn decimal<T: std::str::FromStr>(s: &str) -> Option<T> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    }
    fn hex_byte(s: &str) -> Option<u8> {
        (s.len() == 2 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| u8::from_str_radix(s, 16).ok())
            .flatten()
    }
    let mut fields = line.strip_prefix("OK ")?.split(' ');
    let mut field = |key: &str| fields.next()?.strip_prefix(key)?.strip_prefix('=');
    let read = PadRead {
        flip: decimal(field("flip")?)?,
        reads: decimal(field("reads")?)?,
        differ: decimal(field("differ")?)?,
        pad: Ds3Pad {
            digital1: hex_byte(field("d1")?)?,
            digital2: hex_byte(field("d2")?)?,
            left: Stick {
                x: decimal(field("lx")?)?,
                y: decimal(field("ly")?)?,
            },
            right: Stick {
                x: decimal(field("rx")?)?,
                y: decimal(field("ry")?)?,
            },
        },
    };
    fields.next().is_none().then_some(read)
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
        Ok(roundtrip(&self.path, self.timeout, &[request])?.remove(0))
    }

    /// `verbs` in order on ONE connection, each reply read before the next
    /// request goes (P1 answers a connection's lines in order): a schedule of
    /// hundreds of `PAD` lines without a connection, and an RPCS3 thread, each.
    /// The outer error is the socket's; each verb's refusal is its own.
    pub fn send_all(&self, verbs: &[Verb]) -> Result<Vec<Result<Reply, ReplyError>>> {
        let requests: Vec<String> = verbs.iter().map(|v| v.encode()).collect();
        let requests: Vec<&str> = requests.iter().map(String::as_str).collect();
        let lines = roundtrip(&self.path, self.timeout, &requests)?;
        Ok(verbs
            .iter()
            .zip(&lines)
            .map(|(verb, line)| parse_reply(*verb, line))
            .collect())
    }
}

/// Each request line in turn on one connection, and for each the first reply
/// line that isn't a push, trimmed.
#[cfg(unix)]
fn roundtrip(path: &Path, timeout: Duration, requests: &[&str]) -> Result<Vec<String>> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    use skylander_rpcs3_control::ipc::proto::{is_heartbeat, is_portal_event};

    let stream = UnixStream::connect(path)
        .with_context(|| format!("connecting to P1 at {}", path.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut replies = Vec::with_capacity(requests.len());
    let mut line = String::new();
    for request in requests {
        writer.write_all(request.as_bytes())?;
        loop {
            line.clear();
            if reader.read_line(&mut line).context("reading P1's reply")? == 0 {
                bail!("P1 closed the connection before replying to {request:?}");
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() || is_heartbeat(trimmed) || is_portal_event(trimmed) {
                continue;
            }
            replies.push(trimmed.to_owned());
            break;
        }
    }
    Ok(replies)
}

#[cfg(not(unix))]
fn roundtrip(_path: &Path, _timeout: Duration, _requests: &[&str]) -> Result<Vec<String>> {
    bail!("the oracle is Mac only until the Windows/HTPC bring-up")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: Ds3Pad = Ds3Pad {
        digital1: 0x80,
        digital2: 0x04,
        left: Stick { x: 0, y: 255 },
        right: Stick { x: 128, y: 7 },
    };

    const VERBS: [Verb; 8] = [
        Verb::PauseAt(7),
        Verb::Resume,
        Verb::RsxCap,
        Verb::Screenshot,
        Verb::SaveState,
        Verb::Pad {
            port: 1,
            flip: 9,
            pad: PAD,
        },
        Verb::PadClear,
        Verb::PadState {
            port: 0,
            flip: None,
        },
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
        assert_eq!(
            Verb::Pad {
                port: 1,
                flip: 1234,
                pad: PAD
            }
            .encode(),
            "PAD 1 1234 80 04 0 255 128 7\n"
        );
        assert_eq!(
            Verb::Pad {
                port: 0,
                flip: 2,
                pad: Ds3Pad::REST
            }
            .encode(),
            "PAD 0 2 00 00 128 128 128 128\n"
        );
        assert_eq!(Verb::PadClear.encode(), "PAD_CLEAR\n");
        assert_eq!(
            Verb::PadState {
                port: 1,
                flip: None
            }
            .encode(),
            "PAD_STATE 1\n"
        );
        assert_eq!(
            Verb::PadState {
                port: 0,
                flip: Some(u64::MAX)
            }
            .encode(),
            format!("PAD_STATE 0 {}\n", u64::MAX)
        );
    }

    #[test]
    fn each_success_shape_parses() {
        for verb in [
            Verb::PauseAt(3),
            Verb::Resume,
            Verb::RsxCap,
            Verb::SaveState,
            VERBS[5],
            Verb::PadClear,
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
        assert_eq!(
            parse_reply(
                VERBS[7],
                "OK flip=5260 reads=2 differ=0 d1=80 d2=04 lx=0 ly=255 rx=128 ry=7"
            ),
            Ok(Reply::PadRead(PadRead {
                flip: 5260,
                reads: 2,
                differ: 0,
                pad: PAD,
            }))
        );
        assert_eq!(
            parse_reply(
                VERBS[7],
                "OK flip=0 reads=4294967295 differ=3 d1=ff d2=ff lx=255 ly=0 rx=1 ry=128"
            ),
            Ok(Reply::PadRead(PadRead {
                flip: 0,
                reads: u32::MAX,
                differ: 3,
                pad: Ds3Pad {
                    digital1: 0xff,
                    digital2: 0xff,
                    left: Stick { x: 255, y: 0 },
                    right: Stick { x: 1, y: 128 },
                },
            }))
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
        assert_eq!(counts, [3, 1, 2, 3, 1, 8, 1, 4]);
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
            (
                VERBS[5],
                "OK flip=1 reads=1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            // PAD_STATE always carries the whole read, in O4's order and spelling.
            (VERBS[7], "OK"),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128 ry=128 x=1",
            ),
            (
                VERBS[7],
                "OK reads=1 flip=1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=0 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=FF d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=100 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=00 d2=00 lx=256 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=-1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1 reads=1 differ=0 d1=00 d2=00 lx=+12 ly=128 rx=128 ry=128",
            ),
            (
                VERBS[7],
                "OK flip=1  reads=1 differ=0 d1=00 d2=00 lx=128 ly=128 rx=128 ry=128",
            ),
            (VERBS[6], "OK flip=1"),
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
