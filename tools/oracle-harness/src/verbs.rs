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
//! ->  WATCH <frames> <port> <addr> <len> [<addr> <len> ...]
//!                       <-  OK bytes=<b> frames=<n> port=<p> | ERR bad_args | ERR bad_frames
//!                           | ERR bad_port | ERR bad_range | ERR too_big | ERR no_renderer
//!                           | ERR unmapped
//! ->  WATCH_READ        <-  OK held=<h> total=<t> bytes=<b> frames=<n> armed=<flip> port=<p>
//!                           | ERR not_paused | ERR no_watch
//! ->  WATCH_READ <first> <count>
//!                       <-  OK first=<i> count=<k> data=<hex> | ERR bad_args | ERR not_paused
//!                           | ERR no_watch | ERR bad_index | ERR bad_count
//! ->  WATCH_CLEAR       <-  OK | ERR bad_args
//! ->  CHAIN <slot> <items> <base> <path> <count> <array> <key> <values> [<range> ...]
//!                       <-  OK slot=<s> bytes=<b> listed=<n> kept=<k> | ERR bad_args
//!                           | ERR bad_slot | ERR bad_items | ERR bad_hop | ERR bad_values
//!                           | ERR bad_range | ERR too_big | ERR watching | ERR no_list
//! ->  CHAIN_CLEAR       <-  OK | ERR bad_args | ERR watching
//! (any of them, gate off)  <-  ERR unknown_cmd
//! ```
//!
//! `<flip>` is the counter `STATE` and the heartbeat report as `frames=` (RSX's
//! `int_flip_index`), decimal. A pad's words are `CELL_PAD_CTRL_*` bits in two hex
//! digits and its axes decimal bytes ([`guac_oracle::input::Ds3Pad`]). O5's
//! (AG.1, AG.35) are [`Watch`]'s: a watch's addresses are hex, its port, lengths
//! and counts decimal, and a read's records come back as one hex run
//! ([`WatchRecord`]). O6's (AH.5) are [`Chain`]'s: a chain armed before a
//! watch rides in each of its records after the ranges ([`ChainSpec`] has the
//! grammar, [`ChainBlock`] a record's block).
//! Pushes (`HB`, `PE`) interleave with replies, as on P1. The codec is pure:
//! every line shape unit-tests without a socket.

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
    /// `bad_port`: a port other than 0 or 1 (`PAD`, `PAD_STATE`), or than 0-6
    /// (`WATCH`, [`WATCH_PORTS`]).
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
    /// `bad_frames`: a ring size that isn't a decimal 1 to [`WATCH_MAX_FRAMES`].
    BadFrames,
    /// `bad_range`: an address that isn't 1-8 hex digits, or a length that
    /// isn't a decimal 1 to [`WATCH_MAX_BYTES`], or a range past 4 GiB; for
    /// `CHAIN`, a range O6 can't parse or longer than [`CHAIN_MAX_RANGE`].
    BadRange,
    /// `too_big`: the ranges add up past [`WATCH_MAX_BYTES`] a record, or the
    /// ring (the chains' blocks in each record) past [`WATCH_BUDGET`]; for
    /// `CHAIN`, its block, or every chain's together, past [`CHAIN_BUDGET`].
    TooBig,
    /// `unmapped`: a range isn't readable guest memory when it's armed.
    Unmapped,
    /// `no_watch`: nothing armed (or a new session dropped it).
    NoWatch,
    /// `bad_index`: records past the ones the ring holds.
    BadIndex,
    /// `bad_count`: no records, or more than one reply carries
    /// ([`WATCH_REPLY_BYTES`]).
    BadCount,
    /// `bad_slot`: a chain slot that isn't a decimal 0 to 7 ([`CHAIN_SLOTS`]).
    BadSlot,
    /// `bad_items`: a chain's items a record that isn't a decimal 1 to
    /// [`CHAIN_MAX_ITEMS`].
    BadItems,
    /// `bad_hop`: a base, path, offset or key O6 can't parse, or more than
    /// [`CHAIN_MAX_HOPS`] hops in one.
    BadHop,
    /// `bad_values`: a filter's values that aren't 1 to [`CHAIN_MAX_VALUES`]
    /// hex words joined by commas.
    BadValues,
    /// `watching`: `CHAIN` or `CHAIN_CLEAR` while a watch is armed.
    Watching,
    /// `no_list`: the chain's path doesn't reach a list when it's armed.
    NoList,
}

impl Refusal {
    pub const ALL: [Refusal; 27] = [
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
        Refusal::BadFrames,
        Refusal::BadRange,
        Refusal::TooBig,
        Refusal::Unmapped,
        Refusal::NoWatch,
        Refusal::BadIndex,
        Refusal::BadCount,
        Refusal::BadSlot,
        Refusal::BadItems,
        Refusal::BadHop,
        Refusal::BadValues,
        Refusal::Watching,
        Refusal::NoList,
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
            Refusal::BadFrames => "bad_frames",
            Refusal::BadRange => "bad_range",
            Refusal::TooBig => "too_big",
            Refusal::Unmapped => "unmapped",
            Refusal::NoWatch => "no_watch",
            Refusal::BadIndex => "bad_index",
            Refusal::BadCount => "bad_count",
            Refusal::BadSlot => "bad_slot",
            Refusal::BadItems => "bad_items",
            Refusal::BadHop => "bad_hop",
            Refusal::BadValues => "bad_values",
            Refusal::Watching => "watching",
            Refusal::NoList => "no_list",
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

/// O5's limits (`rpcs3-patches/oracle/0005-O5-*.patch`): ranges a watch.
pub const WATCH_MAX_RANGES: usize = 64;
/// Pad ports a watch can record on, 0 to 6 (cellPad's `CELL_PAD_MAX_PORT_NUM`).
pub const WATCH_PORTS: u8 = 7;
/// Payload bytes a record: the ranges back to back.
pub const WATCH_MAX_BYTES: u32 = 16_384;
/// Records a ring.
pub const WATCH_MAX_FRAMES: u32 = 1 << 20;
/// The ring, headers and chains included (16 MiB before O6).
pub const WATCH_BUDGET: u64 = 128 << 20;
/// A record's header: the flip, the host's steady clock in microseconds, the
/// mask of ranges that weren't readable and which of the port's reads in that
/// flip it was, each a big-endian u64.
pub const WATCH_HEADER: usize = 32;
/// Record bytes (headers included) one `WATCH_READ` reply carries at most
/// (32 KiB before O6).
pub const WATCH_REPLY_BYTES: u64 = 262_144;
/// What an O5 build before O6 carries a reply: [`crate::control::Control::watch_read`]
/// falls back to it when the first page is refused `bad_count`.
pub const WATCH_REPLY_BYTES_BEFORE_O6: u64 = 32_768;

/// A guest range a watch copies at each read of its port.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct WatchRange {
    pub addr: u32,
    pub len: u32,
}

/// O5's verbs (openspec AG.1, AG.35): a ring of guest ranges recorded at every
/// read of one pad port without pausing the run, read back at a pause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    /// A ring of `frames` records of `ranges`, one at every read of pad `port`,
    /// replacing any watch before it; the first record is the port's first read
    /// in a whole flip (the one it's armed in when the port hasn't been read in
    /// it yet, else the next).
    Arm {
        frames: u32,
        port: u8,
        ranges: Vec<WatchRange>,
    },
    /// The ring's summary (only at a pause).
    Summary,
    /// Records `first` to `first + count` of those held, 0 the oldest (only at
    /// a pause).
    Read { first: u64, count: u64 },
    /// The watch and its ring dropped.
    Clear,
}

impl Watch {
    pub fn name(&self) -> &'static str {
        match self {
            Watch::Arm { .. } => "WATCH",
            Watch::Summary | Watch::Read { .. } => "WATCH_READ",
            Watch::Clear => "WATCH_CLEAR",
        }
    }

    /// The newline-terminated request line.
    pub fn encode(&self) -> String {
        match self {
            Watch::Arm {
                frames,
                port,
                ranges,
            } => {
                let mut line = format!("WATCH {frames} {port}");
                for r in ranges {
                    line.push_str(&format!(" {:x} {}", r.addr, r.len));
                }
                line.push('\n');
                line
            }
            Watch::Summary => "WATCH_READ\n".into(),
            Watch::Read { first, count } => format!("WATCH_READ {first} {count}\n"),
            Watch::Clear => "WATCH_CLEAR\n".into(),
        }
    }

    /// The refusals this verb can get besides `unknown_cmd`.
    pub fn refusals(&self) -> &'static [Refusal] {
        match self {
            Watch::Arm { .. } => &[
                Refusal::BadArgs,
                Refusal::BadFrames,
                Refusal::BadPort,
                Refusal::BadRange,
                Refusal::TooBig,
                Refusal::NoRenderer,
                Refusal::Unmapped,
            ],
            Watch::Summary => &[Refusal::NotPaused, Refusal::NoWatch],
            Watch::Read { .. } => &[
                Refusal::BadArgs,
                Refusal::NotPaused,
                Refusal::NoWatch,
                Refusal::BadIndex,
                Refusal::BadCount,
            ],
            Watch::Clear => &[Refusal::BadArgs],
        }
    }
}

/// `WATCH_READ`'s summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchSummary {
    /// Records the ring holds now.
    pub held: u64,
    /// Records taken since it was armed: `total - held` were overwritten.
    pub total: u64,
    /// A record's payload.
    pub bytes: u32,
    /// The ring's size in records.
    pub frames: u32,
    /// The flip counter when it was armed.
    pub armed: u64,
    /// The pad port whose reads it records.
    pub port: u8,
}

/// One record: the flip of the read that took it, the host's steady clock
/// then, which ranges weren't readable (bit `i` for range `i`), which of the
/// port's reads in that flip it was (0 the first: a flip that ran two game
/// frames holds a 0 and a 1) and the ranges' bytes back to back, big-endian as
/// the guest holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRecord {
    pub flip: u64,
    pub host_us: u64,
    pub missing: u64,
    pub read: u64,
    pub payload: Vec<u8>,
}

impl WatchRecord {
    /// `count` records back to back in `data`, each [`WATCH_HEADER`] bytes and
    /// an equal payload; `None` unless they split evenly.
    pub fn split(data: &[u8], count: u64) -> Option<Vec<WatchRecord>> {
        let count = usize::try_from(count).ok().filter(|&c| c > 0)?;
        if !data.len().is_multiple_of(count) {
            return None;
        }
        let stride = data.len() / count;
        if stride < WATCH_HEADER {
            return None;
        }
        let be = |b: &[u8]| u64::from_be_bytes(b.try_into().expect("8 bytes"));
        Some(
            data.chunks_exact(stride)
                .map(|r| WatchRecord {
                    flip: be(&r[0..8]),
                    host_us: be(&r[8..16]),
                    missing: be(&r[16..24]),
                    read: be(&r[24..32]),
                    payload: r[WATCH_HEADER..].to_vec(),
                })
                .collect(),
        )
    }
}

/// An O5 verb's answer when it isn't refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchReply {
    /// `WATCH`'s `OK bytes=<b> frames=<n> port=<p>`.
    Armed {
        bytes: u32,
        frames: u32,
        port: u8,
    },
    Summary(WatchSummary),
    /// `WATCH_READ <first> <count>`'s records, split.
    Records {
        first: u64,
        records: Vec<WatchRecord>,
    },
    /// `WATCH_CLEAR`'s `OK`.
    Ok,
}

/// Lower-case hex, two digits a byte, as O5 writes it; `None` for anything
/// else (an odd length, upper case, a character that isn't a digit).
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[hi, lo]| Some((nibble(hi)? << 4) | nibble(lo)?))
        .collect()
}

fn decimal<T: std::str::FromStr>(s: &str) -> Option<T> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// Parses `verb`'s reply line (trimmed of the newline).
pub fn parse_watch_reply(verb: &Watch, line: &str) -> Result<WatchReply, ReplyError> {
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
    let fields = |keys: &[&str]| -> Option<Vec<&str>> {
        let mut it = line.strip_prefix("OK ")?.split(' ');
        let values = keys
            .iter()
            .map(|k| it.next()?.strip_prefix(k)?.strip_prefix('='))
            .collect::<Option<Vec<&str>>>()?;
        it.next().is_none().then_some(values)
    };
    match verb {
        Watch::Arm { .. } => {
            let v = fields(&["bytes", "frames", "port"]).ok_or_else(unexpected)?;
            let port = decimal(v[2]).ok_or_else(unexpected)?;
            if port >= WATCH_PORTS {
                return Err(unexpected());
            }
            Ok(WatchReply::Armed {
                bytes: decimal(v[0]).ok_or_else(unexpected)?,
                frames: decimal(v[1]).ok_or_else(unexpected)?,
                port,
            })
        }
        Watch::Summary => {
            let v = fields(&["held", "total", "bytes", "frames", "armed", "port"])
                .ok_or_else(unexpected)?;
            let summary = WatchSummary {
                held: decimal(v[0]).ok_or_else(unexpected)?,
                total: decimal(v[1]).ok_or_else(unexpected)?,
                bytes: decimal(v[2]).ok_or_else(unexpected)?,
                frames: decimal(v[3]).ok_or_else(unexpected)?,
                armed: decimal(v[4]).ok_or_else(unexpected)?,
                port: decimal(v[5]).ok_or_else(unexpected)?,
            };
            if summary.held > summary.total
                || summary.held > u64::from(summary.frames)
                || summary.port >= WATCH_PORTS
            {
                return Err(unexpected());
            }
            Ok(WatchReply::Summary(summary))
        }
        Watch::Read { first, count } => {
            let v = fields(&["first", "count", "data"]).ok_or_else(unexpected)?;
            let (got_first, got_count): (u64, u64) = (
                decimal(v[0]).ok_or_else(unexpected)?,
                decimal(v[1]).ok_or_else(unexpected)?,
            );
            if (got_first, got_count) != (*first, *count) {
                return Err(unexpected());
            }
            let data = unhex(v[2]).ok_or_else(unexpected)?;
            let records = WatchRecord::split(&data, *count).ok_or_else(unexpected)?;
            Ok(WatchReply::Records {
                first: *first,
                records,
            })
        }
        Watch::Clear if line == "OK" => Ok(WatchReply::Ok),
        Watch::Clear => Err(unexpected()),
    }
}

/// O6's limits (`rpcs3-patches/oracle/0006-O6-*.patch`): chains armed at once.
pub const CHAIN_SLOTS: u8 = 8;
/// Items a chain keeps a record.
pub const CHAIN_MAX_ITEMS: u32 = 512;
/// Ranges a chain copies of each item (one bit each in its missing mask).
pub const CHAIN_MAX_RANGES: usize = 32;
/// Hops a path, key or range.
pub const CHAIN_MAX_HOPS: usize = 16;
/// Values a filter matches.
pub const CHAIN_MAX_VALUES: usize = 64;
/// Bytes a range.
pub const CHAIN_MAX_RANGE: u16 = 1_024;
/// A list counting more isn't one (the block's status says so).
pub const CHAIN_MAX_LIST: u32 = 4_096;
/// Elements a search hop looks through.
pub const CHAIN_MAX_SCAN: u32 = 1_024;
/// Bytes a name is hashed over, its NUL included, before O6 gives up.
pub const CHAIN_MAX_NAME: usize = 256;
/// A block's header: the status, the list's count, the items whose key
/// didn't read, the items the filter matched and the items kept.
pub const CHAIN_HEADER: usize = 20;
/// A kept item's header: its address, its index in the list, the value it
/// matched and the mask of its ranges that didn't read.
pub const CHAIN_ITEM_HEADER: usize = 16;
/// Every chain's block together, a record.
pub const CHAIN_BUDGET: usize = 49_152;

/// FNV-1a over a name's bytes, as O6 hashes the C string a key or a search
/// hop names (Alchemy keys its namespaces the same way: [`crate::script::fnv1a`]).
pub fn name_hash(name: &str) -> u32 {
    crate::script::fnv1a(name)
}

/// One link O6 follows from where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Hop {
    /// The pointer at `+off`, its object's first word `check` when there is
    /// one (a vtable).
    Deref { off: u16, check: Option<u32> },
    /// Here is a list (its count at `+count`, its array of element pointers
    /// at `+array`): the element whose name, the C string the word at the
    /// element's `+name` points at, hashes to `hash`.
    Search {
        count: u16,
        array: u16,
        name: u16,
        hash: u32,
    },
}

impl Hop {
    /// The pointer at `+off`.
    pub fn at(off: u16) -> Self {
        Hop::Deref { off, check: None }
    }

    /// The pointer at `+off`, its object's first word `vtable`.
    pub fn checked(off: u16, vtable: u32) -> Self {
        Hop::Deref {
            off,
            check: Some(vtable),
        }
    }

    /// The element of the list here named `name`.
    pub fn named(count: u16, array: u16, name_at: u16, name: &str) -> Self {
        Hop::Search {
            count,
            array,
            name: name_at,
            hash: name_hash(name),
        }
    }

    fn text(&self) -> String {
        match *self {
            Hop::Deref { off, check: None } => format!("{off:x}"),
            Hop::Deref {
                off,
                check: Some(w),
            } => format!("{off:x}={w:x}"),
            Hop::Search {
                count,
                array,
                name,
                hash,
            } => format!("?{count:x},{array:x},{name:x}={hash:x}"),
        }
    }
}

fn hops_text(hops: &[Hop], term: String) -> String {
    let mut parts: Vec<String> = hops.iter().map(Hop::text).collect();
    parts.push(term);
    parts.join(">")
}

/// A chain item's key: its hops from the item, then the word at `+off`, or
/// (`hashed`) the FNV-1a of the C string that word points at.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Key {
    pub hops: Vec<Hop>,
    pub off: u16,
    pub hashed: bool,
}

/// A range a chain copies of each kept item: hops from the item, then `len`
/// bytes at `+off`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Probe {
    pub hops: Vec<Hop>,
    pub off: u16,
    pub len: u16,
}

impl Probe {
    pub fn new(hops: Vec<Hop>, off: u16, len: u16) -> Self {
        Self { hops, off, len }
    }
}

/// What O6 follows for one chain: from `base`, `path`'s hops to a list whose
/// count word sits at `+count` and whose pointer to its array of item
/// pointers at `+array`; an item whose key matches one of `values` is kept,
/// up to `items` a record, with its `ranges`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChainSpec {
    pub items: u32,
    pub base: u32,
    pub path: Vec<Hop>,
    pub count: u16,
    pub array: u16,
    pub key: Key,
    pub values: Vec<u32>,
    pub ranges: Vec<Probe>,
}

impl ChainSpec {
    /// A kept item's bytes in a block: its header and its ranges.
    pub fn item_bytes(&self) -> usize {
        CHAIN_ITEM_HEADER + self.ranges.iter().map(|r| r.len as usize).sum::<usize>()
    }

    /// The block this chain adds to each record.
    pub fn bytes(&self) -> usize {
        CHAIN_HEADER + self.items as usize * self.item_bytes()
    }

    /// O6's limits, checked before a request goes: the refusal O6 would give.
    pub fn check(&self) -> Result<(), Refusal> {
        if self.ranges.len() > CHAIN_MAX_RANGES {
            return Err(Refusal::BadArgs);
        }
        if self.items == 0 || self.items > CHAIN_MAX_ITEMS {
            return Err(Refusal::BadItems);
        }
        if self.path.len() > CHAIN_MAX_HOPS || self.key.hops.len() > CHAIN_MAX_HOPS {
            return Err(Refusal::BadHop);
        }
        if self.values.is_empty() || self.values.len() > CHAIN_MAX_VALUES {
            return Err(Refusal::BadValues);
        }
        if self
            .ranges
            .iter()
            .any(|r| r.hops.len() > CHAIN_MAX_HOPS || r.len == 0 || r.len > CHAIN_MAX_RANGE)
        {
            return Err(Refusal::BadRange);
        }
        if self.bytes() > CHAIN_BUDGET {
            return Err(Refusal::TooBig);
        }
        Ok(())
    }

    /// The `CHAIN` request's fields after the slot.
    fn args(&self) -> String {
        let path = if self.path.is_empty() {
            "-".to_owned()
        } else {
            self.path
                .iter()
                .map(Hop::text)
                .collect::<Vec<_>>()
                .join(">")
        };
        let key = hops_text(
            &self.key.hops,
            format!(
                "{}{:x}",
                if self.key.hashed { "#" } else { "" },
                self.key.off
            ),
        );
        let values = self
            .values
            .iter()
            .map(|v| format!("{v:x}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut line = format!(
            "{} {:x} {path} {:x} {:x} {key} {values}",
            self.items, self.base, self.count, self.array
        );
        for r in &self.ranges {
            line.push(' ');
            line.push_str(&hops_text(&r.hops, format!("{:x}+{}", r.off, r.len)));
        }
        line
    }
}

/// O6's verbs (openspec AH.5): chains armed before a watch, whose blocks then
/// ride in each of its records after the ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chain {
    /// Chain `slot` (0-7), replacing the one there; refused while a watch is
    /// armed and when the path doesn't reach a list now.
    Arm { slot: u8, spec: ChainSpec },
    /// Every chain dropped; refused while a watch is armed.
    Clear,
}

impl Chain {
    pub fn name(&self) -> &'static str {
        match self {
            Chain::Arm { .. } => "CHAIN",
            Chain::Clear => "CHAIN_CLEAR",
        }
    }

    /// The newline-terminated request line.
    pub fn encode(&self) -> String {
        match self {
            Chain::Arm { slot, spec } => format!("CHAIN {slot} {}\n", spec.args()),
            Chain::Clear => "CHAIN_CLEAR\n".into(),
        }
    }

    /// The refusals this verb can get besides `unknown_cmd`.
    pub fn refusals(&self) -> &'static [Refusal] {
        match self {
            Chain::Arm { .. } => &[
                Refusal::BadArgs,
                Refusal::BadSlot,
                Refusal::BadItems,
                Refusal::BadHop,
                Refusal::BadValues,
                Refusal::BadRange,
                Refusal::TooBig,
                Refusal::Watching,
                Refusal::NoList,
            ],
            Chain::Clear => &[Refusal::BadArgs, Refusal::Watching],
        }
    }
}

/// An O6 verb's answer when it isn't refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainReply {
    /// `CHAIN`'s `OK slot=<s> bytes=<b> listed=<n> kept=<k>`: the block's
    /// bytes, and the list's count and the items kept when it was armed.
    Armed {
        slot: u8,
        bytes: u32,
        listed: u32,
        kept: u32,
    },
    /// `CHAIN_CLEAR`'s `OK`.
    Ok,
}

/// Parses `verb`'s reply line (trimmed of the newline).
pub fn parse_chain_reply(verb: &Chain, line: &str) -> Result<ChainReply, ReplyError> {
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
    match verb {
        Chain::Arm { slot, spec } => {
            let mut it = line.strip_prefix("OK ").ok_or_else(unexpected)?.split(' ');
            let mut field = |key: &str| -> Option<u32> {
                decimal(it.next()?.strip_prefix(key)?.strip_prefix('=')?)
            };
            let (got_slot, bytes, listed, kept) = (
                field("slot").ok_or_else(unexpected)?,
                field("bytes").ok_or_else(unexpected)?,
                field("listed").ok_or_else(unexpected)?,
                field("kept").ok_or_else(unexpected)?,
            );
            if it.next().is_some()
                || got_slot != u32::from(*slot)
                || bytes as usize != spec.bytes()
                || kept > spec.items
                || kept > listed
            {
                return Err(unexpected());
            }
            Ok(ChainReply::Armed {
                slot: *slot,
                bytes,
                listed,
                kept,
            })
        }
        Chain::Clear if line == "OK" => Ok(ChainReply::Ok),
        Chain::Clear => Err(unexpected()),
    }
}

/// A kept item as its chain's block holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEntry {
    /// The list's entry: the item's address.
    pub address: u32,
    /// Its index in the list.
    pub index: u32,
    /// Which of the chain's values its key matched.
    pub value: u32,
    /// A bit per range that didn't read.
    pub missing: u32,
    /// Each range's bytes, zeros where one didn't read.
    pub ranges: Vec<Vec<u8>>,
}

impl ChainEntry {
    /// Range `i`'s bytes, or `None` when it didn't read.
    pub fn range(&self, i: usize) -> Option<&[u8]> {
        (self.missing & (1 << i) == 0)
            .then(|| self.ranges.get(i).map(Vec::as_slice))
            .flatten()
    }
}

/// One chain's block in one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBlock {
    /// 0 when the list read; else the 1-based link that broke: the path's
    /// hops, then the count word, the array pointer and a count past
    /// [`CHAIN_MAX_LIST`].
    pub status: u32,
    /// The list's count.
    pub listed: u32,
    /// Items whose key didn't read (unreadable, or freed).
    pub unread: u32,
    /// Items the filter matched, kept or not.
    pub matched: u32,
    /// The items kept, in list order.
    pub items: Vec<ChainEntry>,
}

impl ChainBlock {
    /// Items the filter matched past the chain's `items`.
    pub fn dropped(&self) -> u32 {
        self.matched - self.items.len() as u32
    }

    /// `bytes` as `spec`'s block. Refused: the wrong size, counts that don't
    /// hold together (more kept than matched, matched or unread than listed,
    /// anything listed under a broken link), an item matching a value the
    /// spec doesn't have or missing a range it doesn't copy, or a byte past
    /// the kept items that isn't zero.
    pub fn parse(spec: &ChainSpec, bytes: &[u8]) -> Result<Self> {
        if bytes.len() != spec.bytes() {
            bail!(
                "a chain block of {} bytes; the spec makes {}",
                bytes.len(),
                spec.bytes()
            );
        }
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        let (status, listed, unread, matched, kept) =
            (word(0), word(4), word(8), word(12), word(16));
        if kept > spec.items
            || kept > matched
            || matched.saturating_add(unread) > listed
            || listed > CHAIN_MAX_LIST
            || (status != 0 && listed != 0)
        {
            bail!(
                "a chain block's counts don't hold: status {status}, listed {listed}, unread \
                 {unread}, matched {matched}, kept {kept} of {}",
                spec.items
            );
        }
        let stride = spec.item_bytes();
        let all = if spec.ranges.len() >= 32 {
            u32::MAX
        } else {
            (1u32 << spec.ranges.len()) - 1
        };
        let mut items = Vec::with_capacity(kept as usize);
        let mut last = None;
        for k in 0..kept as usize {
            let at = CHAIN_HEADER + k * stride;
            let (address, index, value, missing) =
                (word(at), word(at + 4), word(at + 8), word(at + 12));
            if value as usize >= spec.values.len() || missing & !all != 0 || index >= listed {
                bail!(
                    "chain item {k} at {address:#010x}: value {value}, index {index}, missing \
                     {missing:#x} don't fit the spec"
                );
            }
            if last.is_some_and(|l| index <= l) {
                bail!("chain item {k}'s index {index} isn't past the one before");
            }
            last = Some(index);
            let mut ranges = Vec::with_capacity(spec.ranges.len());
            let mut r = at + CHAIN_ITEM_HEADER;
            for p in &spec.ranges {
                ranges.push(bytes[r..r + p.len as usize].to_vec());
                r += p.len as usize;
            }
            items.push(ChainEntry {
                address,
                index,
                value,
                missing,
                ranges,
            });
        }
        if bytes[CHAIN_HEADER + kept as usize * stride..]
            .iter()
            .any(|&b| b != 0)
        {
            bail!("a chain block holds bytes past its {kept} kept items");
        }
        Ok(Self {
            status,
            listed,
            unread,
            matched,
            items,
        })
    }
}

/// A record's payload split into its watch list's ranges and each armed
/// chain's block, in slot order; `None` unless the sizes add up.
pub fn split_payload<'a>(
    payload: &'a [u8],
    list_bytes: usize,
    chains: &[&ChainSpec],
) -> Option<(&'a [u8], Vec<&'a [u8]>)> {
    let total = list_bytes + chains.iter().map(|c| c.bytes()).sum::<usize>();
    if payload.len() != total {
        return None;
    }
    let (ranges, mut rest) = payload.split_at(list_bytes);
    let mut blocks = Vec::with_capacity(chains.len());
    for c in chains {
        let (block, tail) = rest.split_at(c.bytes());
        blocks.push(block);
        rest = tail;
    }
    Some((ranges, blocks))
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

    /// Sends an O5 verb; a refusal comes back as a [`ReplyError`] inside the
    /// error, as for [`VerbClient::send`].
    pub fn send_watch(&self, verb: &Watch) -> Result<WatchReply> {
        let line = self.roundtrip_line(&verb.encode())?;
        Ok(parse_watch_reply(verb, &line)?)
    }

    /// O5 verbs in order on one connection, as [`VerbClient::send_all`] sends
    /// O3's and O4's: a ring's records page by page without a connection each.
    pub fn send_watch_all(&self, verbs: &[Watch]) -> Result<Vec<Result<WatchReply, ReplyError>>> {
        let requests: Vec<String> = verbs.iter().map(Watch::encode).collect();
        let requests: Vec<&str> = requests.iter().map(String::as_str).collect();
        let lines = roundtrip(&self.path, self.timeout, &requests)?;
        Ok(verbs
            .iter()
            .zip(&lines)
            .map(|(verb, line)| parse_watch_reply(verb, line))
            .collect())
    }

    /// Sends an O6 verb; a refusal comes back as a [`ReplyError`] inside the
    /// error, as for [`VerbClient::send`].
    pub fn send_chain(&self, verb: &Chain) -> Result<ChainReply> {
        let line = self.roundtrip_line(&verb.encode())?;
        Ok(parse_chain_reply(verb, &line)?)
    }

    /// One request line (newline included) and the first reply line that isn't
    /// a push, trimmed.
    pub fn roundtrip_line(&self, request: &str) -> Result<String> {
        Ok(roundtrip(&self.path, self.timeout, &[request])?.remove(0))
    }

    /// `request`'s reply, then for `linger` more what P1 pushes after it on
    /// the same connection (P4 drains its portal events right after a reply):
    /// the reply and every `PE` line seen, before it or after, in order.
    pub fn roundtrip_draining(
        &self,
        request: &str,
        linger: Duration,
    ) -> Result<(String, Vec<String>)> {
        roundtrip_draining(&self.path, self.timeout, request, linger)
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

/// [`VerbClient::roundtrip_draining`].
#[cfg(unix)]
fn roundtrip_draining(
    path: &Path,
    timeout: Duration,
    request: &str,
    linger: Duration,
) -> Result<(String, Vec<String>)> {
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::os::unix::net::UnixStream;

    use skylander_rpcs3_control::ipc::proto::{is_heartbeat, is_portal_event};

    let stream = UnixStream::connect(path)
        .with_context(|| format!("connecting to P1 at {}", path.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut writer = stream.try_clone()?;
    let lingering = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    writer.write_all(format!("{}\n", request.trim_end()).as_bytes())?;
    let mut events = Vec::new();
    let mut line = String::new();
    let reply = loop {
        line.clear();
        if reader.read_line(&mut line).context("reading P1's reply")? == 0 {
            bail!("P1 closed the connection before replying to {request:?}");
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if is_portal_event(trimmed) {
            events.push(trimmed.to_owned());
        } else if !(trimmed.is_empty() || is_heartbeat(trimmed)) {
            break trimmed.to_owned();
        }
    };
    if !linger.is_zero() {
        lingering.set_read_timeout(Some(linger))?;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if is_portal_event(trimmed) {
                        events.push(trimmed.to_owned());
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                Err(e) => return Err(e).context("reading what P1 pushed after a reply"),
            }
        }
    }
    Ok((reply, events))
}

#[cfg(not(unix))]
fn roundtrip_draining(
    _path: &Path,
    _timeout: Duration,
    _request: &str,
    _linger: Duration,
) -> Result<(String, Vec<String>)> {
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
    fn watch_verbs() -> [Watch; 4] {
        [
            Watch::Arm {
                frames: 600,
                port: 0,
                ranges: vec![
                    WatchRange {
                        addr: 0x3032_8364,
                        len: 20,
                    },
                    WatchRange {
                        addr: 0x0089_D4F0,
                        len: 4,
                    },
                ],
            },
            Watch::Summary,
            Watch::Read {
                first: 0,
                count: 40,
            },
            Watch::Clear,
        ]
    }

    #[test]
    fn each_watch_verb_encodes_as_o5_reads_it() {
        let [arm, summary, read, clear] = watch_verbs();
        assert_eq!(arm.encode(), "WATCH 600 0 30328364 20 89d4f0 4\n");
        assert_eq!(arm.name(), "WATCH");
        assert_eq!(summary.encode(), "WATCH_READ\n");
        assert_eq!(read.encode(), "WATCH_READ 0 40\n");
        assert_eq!(
            Watch::Read {
                first: u64::MAX,
                count: 1
            }
            .encode(),
            format!("WATCH_READ {} 1\n", u64::MAX)
        );
        assert_eq!((summary.name(), read.name()), ("WATCH_READ", "WATCH_READ"));
        assert_eq!(clear.encode(), "WATCH_CLEAR\n");
        let edge = Watch::Arm {
            frames: WATCH_MAX_FRAMES,
            port: WATCH_PORTS - 1,
            ranges: vec![WatchRange {
                addr: 0xFFFF_FFFF,
                len: 1,
            }],
        };
        assert_eq!(edge.encode(), "WATCH 1048576 6 ffffffff 1\n");
    }

    /// Three records of a 4-byte payload: flip 7, then flip 8 twice (a flip that
    /// ran two game frames: reads 0 and 1), the second with range 0 unreadable.
    const RECORDS_HEX: &str = "0000000000000007000000000000002a00000000000000000000000000000000deadbeef\
                               0000000000000008000000000000002b0000000000000001000000000000000000000000\
                               0000000000000008000000000000002c000000000000000000000000000000010badcafe";

    #[test]
    fn each_watch_success_shape_parses() {
        let [arm, summary, read, clear] = watch_verbs();
        assert_eq!(
            parse_watch_reply(&arm, "OK bytes=24 frames=600 port=0"),
            Ok(WatchReply::Armed {
                bytes: 24,
                frames: 600,
                port: 0,
            })
        );
        assert_eq!(
            parse_watch_reply(
                &summary,
                "OK held=600 total=1801 bytes=804 frames=600 armed=2 port=6"
            ),
            Ok(WatchReply::Summary(WatchSummary {
                held: 600,
                total: 1_801,
                bytes: 804,
                frames: 600,
                armed: 2,
                port: 6,
            }))
        );
        let three = Watch::Read { first: 5, count: 3 };
        let got = parse_watch_reply(&three, &format!("OK first=5 count=3 data={RECORDS_HEX}"));
        assert_eq!(
            got,
            Ok(WatchReply::Records {
                first: 5,
                records: vec![
                    WatchRecord {
                        flip: 7,
                        host_us: 42,
                        missing: 0,
                        read: 0,
                        payload: vec![0xde, 0xad, 0xbe, 0xef],
                    },
                    WatchRecord {
                        flip: 8,
                        host_us: 43,
                        missing: 1,
                        read: 0,
                        payload: vec![0; 4],
                    },
                    WatchRecord {
                        flip: 8,
                        host_us: 44,
                        missing: 0,
                        read: 1,
                        payload: vec![0x0b, 0xad, 0xca, 0xfe],
                    },
                ],
            })
        );
        // A record can carry no payload beyond its header only if O5 armed none,
        // which it refuses; the split itself takes it.
        assert_eq!(
            WatchRecord::split(&[0; WATCH_HEADER], 1).unwrap()[0]
                .payload
                .len(),
            0
        );
        assert_eq!(parse_watch_reply(&clear, "OK"), Ok(WatchReply::Ok));
        assert!(matches!(
            parse_watch_reply(&read, &format!("OK first=0 count=40 data={}", "00".repeat(40 * 38))),
            Ok(WatchReply::Records { records, .. }) if records.len() == 40 && records[0].payload.len() == 6
        ));
    }

    #[test]
    fn every_refusal_maps_for_the_watch_verbs_that_get_it_and_no_other() {
        for verb in watch_verbs() {
            for refusal in Refusal::ALL {
                let line = format!("ERR {}", refusal.wire());
                let got = parse_watch_reply(&verb, &line).unwrap_err();
                if refusal == Refusal::UnknownCmd || verb.refusals().contains(&refusal) {
                    assert_eq!(got.refusal(), Some(refusal), "{verb:?} {line}");
                    assert_eq!(got.to_string(), format!("{} refused: {line}", verb.name()));
                } else {
                    assert_eq!(got.refusal(), None, "{verb:?} can't get {line}");
                }
            }
        }
        let counts: Vec<usize> = watch_verbs().iter().map(|v| v.refusals().len()).collect();
        assert_eq!(counts, [7, 2, 5, 1]);
        // O3's and O4's verbs never get O5's refusals.
        for verb in VERBS {
            for refusal in &Refusal::ALL[14..] {
                assert!(!verb.refusals().contains(refusal), "{verb:?} {refusal:?}");
            }
        }
    }

    #[test]
    fn a_watch_reply_off_the_table_is_unexpected() {
        let [arm, summary, read, clear] = watch_verbs();
        let three = Watch::Read { first: 5, count: 3 };
        let cases: Vec<(Watch, String)> = vec![
            (arm.clone(), "OK".into()),
            (arm.clone(), "OK bytes=24".into()),
            // AG.1's O5, before it took a port.
            (arm.clone(), "OK bytes=24 frames=600".into()),
            (arm.clone(), "OK frames=600 bytes=24 port=0".into()),
            (arm.clone(), "OK bytes=-1 frames=600 port=0".into()),
            (arm.clone(), "OK bytes=24 frames=600 port=7".into()),
            (arm.clone(), "OK bytes=24 frames=600 port=0 extra=1".into()),
            (arm.clone(), "ERR bad_index".into()),
            (
                summary.clone(),
                "OK held=1 total=1 bytes=4 frames=1 armed=0".into(),
            ),
            (
                summary.clone(),
                "OK held=1 total=1 bytes=4 frames=1 armed=0 port=7".into(),
            ),
            // More held than taken, or than the ring holds.
            (
                summary.clone(),
                "OK held=2 total=1 bytes=4 frames=8 armed=0 port=0".into(),
            ),
            (
                summary.clone(),
                "OK held=9 total=9 bytes=4 frames=8 armed=0 port=0".into(),
            ),
            (summary.clone(), "ERR bad_count".into()),
            (read.clone(), "OK".into()),
            // The records asked for, and none other.
            (
                three.clone(),
                format!("OK first=4 count=3 data={RECORDS_HEX}"),
            ),
            (
                three.clone(),
                format!("OK first=5 count=1 data={RECORDS_HEX}"),
            ),
            // Hex O5 never writes: odd, upper case, not hex.
            (
                three.clone(),
                format!("OK first=5 count=3 data={}", &RECORDS_HEX[1..]),
            ),
            (
                three.clone(),
                format!("OK first=5 count=3 data={}", RECORDS_HEX.to_uppercase()),
            ),
            (
                three.clone(),
                format!("OK first=5 count=3 data=zz{}", &RECORDS_HEX[2..]),
            ),
            // Records that don't split evenly or are shorter than their header
            // (AG.1's 24-byte header with nothing after it).
            (
                three.clone(),
                format!("OK first=5 count=3 data={}00", RECORDS_HEX),
            ),
            (
                three.clone(),
                format!("OK first=5 count=3 data={}", "00".repeat(3 * 24)),
            ),
            (three.clone(), "OK first=5 count=3 data=".into()),
            (read.clone(), "ERR unmapped".into()),
            (clear.clone(), "OK bytes=1 frames=1".into()),
            (clear.clone(), "ERR no_watch".into()),
        ];
        for (verb, line) in &cases {
            let err = parse_watch_reply(verb, line).unwrap_err();
            assert_eq!(
                err,
                ReplyError::Unexpected {
                    verb: verb.name(),
                    line: line.clone()
                },
                "{verb:?} {line:?}"
            );
        }
    }

    /// A chain off a static: its root and list checked by vtable, each item
    /// keyed by the hash of its info's name, its position, and its `hp`
    /// through two hops and a search by name.
    fn chain_spec() -> ChainSpec {
        ChainSpec {
            items: 4,
            base: 0x0089_D4F0,
            path: vec![
                Hop::checked(0, 0x0061_DD60),
                Hop::at(0x2C),
                Hop::checked(0x14, 0x0064_0028),
            ],
            count: 0x08,
            array: 0x14,
            key: Key {
                hops: vec![Hop::checked(0x50, 0x0062_1048)],
                off: 0x08,
                hashed: true,
            },
            values: vec![name_hash("Enemy_Chompy(1)"), 0x0062_1618],
            ranges: vec![
                Probe::new(vec![Hop::at(0x50)], 0x3C, 24),
                Probe::new(vec![], 0x58, 12),
                Probe::new(
                    vec![
                        Hop::at(0x50),
                        Hop::at(0xB0),
                        Hop::named(0x08, 0x14, 0x08, "description"),
                    ],
                    0x1C,
                    8,
                ),
            ],
        }
    }

    #[test]
    fn each_chain_verb_encodes_as_o6_reads_it() {
        let spec = chain_spec();
        assert_eq!(name_hash(""), 0x811C_9DC5);
        assert_eq!(name_hash("global.bld"), 0x2694_BE8E);
        let hash = name_hash("Enemy_Chompy(1)");
        let desc = name_hash("description");
        assert_eq!(
            Chain::Arm {
                slot: 3,
                spec: spec.clone()
            }
            .encode(),
            format!(
                "CHAIN 3 4 89d4f0 0=61dd60>2c>14=640028 8 14 50=621048>#8 {hash:x},621618 \
                 50>3c+24 58+12 50>b0>?8,14,8={desc:x}>1c+8\n"
            )
        );
        // No path, an unhashed key without hops, a range without hops.
        let bare = ChainSpec {
            path: Vec::new(),
            key: Key {
                hops: Vec::new(),
                off: 0x50,
                hashed: false,
            },
            ranges: vec![Probe::new(vec![], 0, 1024)],
            values: vec![0xFFFF_FFFF],
            items: CHAIN_MAX_ITEMS,
            ..spec.clone()
        };
        assert_eq!(
            Chain::Arm {
                slot: CHAIN_SLOTS - 1,
                spec: bare
            }
            .encode(),
            "CHAIN 7 512 89d4f0 - 8 14 50 ffffffff 0+1024\n"
        );
        assert_eq!(Chain::Clear.encode(), "CHAIN_CLEAR\n");
        assert_eq!(
            (Chain::Clear.name(), Chain::Arm { slot: 0, spec }.name()),
            ("CHAIN_CLEAR", "CHAIN")
        );
    }

    #[test]
    fn a_chains_size_and_limits_are_o6s() {
        let spec = chain_spec();
        assert_eq!(spec.item_bytes(), CHAIN_ITEM_HEADER + 24 + 12 + 8);
        assert_eq!(spec.bytes(), CHAIN_HEADER + 4 * 60);
        assert_eq!(spec.check(), Ok(()));
        let bent = |f: &dyn Fn(&mut ChainSpec)| {
            let mut s = chain_spec();
            f(&mut s);
            s.check()
        };
        assert_eq!(bent(&|s| s.items = 0), Err(Refusal::BadItems));
        assert_eq!(
            bent(&|s| s.items = CHAIN_MAX_ITEMS + 1),
            Err(Refusal::BadItems)
        );
        assert_eq!(bent(&|s| s.values.clear()), Err(Refusal::BadValues));
        assert_eq!(
            bent(&|s| s.values = vec![1; CHAIN_MAX_VALUES + 1]),
            Err(Refusal::BadValues)
        );
        assert_eq!(bent(&|s| s.values = vec![1; CHAIN_MAX_VALUES]), Ok(()));
        assert_eq!(
            bent(&|s| s.path = vec![Hop::at(0); CHAIN_MAX_HOPS + 1]),
            Err(Refusal::BadHop)
        );
        assert_eq!(
            bent(&|s| s.key.hops = vec![Hop::at(0); CHAIN_MAX_HOPS + 1]),
            Err(Refusal::BadHop)
        );
        assert_eq!(bent(&|s| s.ranges[0].len = 0), Err(Refusal::BadRange));
        assert_eq!(
            bent(&|s| s.ranges[0].len = CHAIN_MAX_RANGE + 1),
            Err(Refusal::BadRange)
        );
        assert_eq!(
            bent(&|s| s.ranges = vec![Probe::new(vec![], 0, 4); CHAIN_MAX_RANGES + 1]),
            Err(Refusal::BadArgs)
        );
        // 71 items of 692 bytes fill the budget exactly; one more byte an item
        // doesn't fit.
        let at = |len: u16| {
            let mut s = chain_spec();
            s.items = 71;
            s.ranges = vec![Probe::new(vec![], 0, len)];
            s
        };
        assert_eq!(at(676).bytes(), CHAIN_BUDGET);
        assert_eq!(at(676).check(), Ok(()));
        assert_eq!(at(677).check(), Err(Refusal::TooBig));
    }

    #[test]
    fn each_chain_success_shape_parses() {
        let spec = chain_spec();
        let arm = Chain::Arm {
            slot: 2,
            spec: spec.clone(),
        };
        assert_eq!(
            parse_chain_reply(
                &arm,
                &format!("OK slot=2 bytes={} listed=166 kept=4", spec.bytes())
            ),
            Ok(ChainReply::Armed {
                slot: 2,
                bytes: spec.bytes() as u32,
                listed: 166,
                kept: 4
            })
        );
        assert_eq!(parse_chain_reply(&Chain::Clear, "OK"), Ok(ChainReply::Ok));
    }

    #[test]
    fn every_refusal_maps_for_the_chain_verbs_that_get_it_and_no_other() {
        let verbs = [
            Chain::Arm {
                slot: 0,
                spec: chain_spec(),
            },
            Chain::Clear,
        ];
        for verb in &verbs {
            for refusal in Refusal::ALL {
                let line = format!("ERR {}", refusal.wire());
                let got = parse_chain_reply(verb, &line).unwrap_err();
                if refusal == Refusal::UnknownCmd || verb.refusals().contains(&refusal) {
                    assert_eq!(got.refusal(), Some(refusal), "{verb:?} {line}");
                } else {
                    assert_eq!(got.refusal(), None, "{verb:?} can't get {line}");
                }
            }
        }
        assert_eq!(
            verbs.iter().map(|v| v.refusals().len()).collect::<Vec<_>>(),
            [9, 2]
        );
        // O3-O5's verbs never get O6's own refusals.
        for refusal in &Refusal::ALL[21..] {
            assert!(
                VERBS.iter().all(|v| !v.refusals().contains(refusal)),
                "{refusal:?}"
            );
            assert!(
                watch_verbs()
                    .iter()
                    .all(|v| !v.refusals().contains(refusal)),
                "{refusal:?}"
            );
        }
    }

    #[test]
    fn a_chain_reply_off_the_table_is_unexpected() {
        let spec = chain_spec();
        let b = spec.bytes();
        let arm = Chain::Arm { slot: 2, spec };
        let cases = [
            (arm.clone(), "OK".to_owned()),
            (arm.clone(), format!("OK slot=3 bytes={b} listed=1 kept=1")),
            (
                arm.clone(),
                format!("OK slot=2 bytes={} listed=1 kept=1", b + 1),
            ),
            (arm.clone(), format!("OK slot=2 bytes={b} listed=1")),
            (
                arm.clone(),
                format!("OK slot=2 bytes={b} listed=1 kept=1 extra=0"),
            ),
            (arm.clone(), format!("OK bytes={b} slot=2 listed=1 kept=1")),
            // More kept than the chain keeps, or than the list holds.
            (arm.clone(), format!("OK slot=2 bytes={b} listed=9 kept=5")),
            (arm.clone(), format!("OK slot=2 bytes={b} listed=1 kept=2")),
            (arm.clone(), "ERR no_watch".to_owned()),
            (Chain::Clear, "OK slot=0".to_owned()),
            (Chain::Clear, "ERR no_list".to_owned()),
        ];
        for (verb, line) in &cases {
            assert_eq!(
                parse_chain_reply(verb, line).unwrap_err(),
                ReplyError::Unexpected {
                    verb: verb.name(),
                    line: line.clone()
                },
                "{line}"
            );
        }
    }

    /// `spec`'s block holding `items` (address, index, value, missing and a
    /// fill byte for its ranges), under the counts given.
    fn block(spec: &ChainSpec, counts: [u32; 4], items: &[(u32, u32, u32, u32, u8)]) -> Vec<u8> {
        let mut b = vec![0u8; spec.bytes()];
        let [status, listed, unread, matched] = counts;
        for (i, w) in [status, listed, unread, matched, items.len() as u32]
            .iter()
            .enumerate()
        {
            b[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
        }
        for (k, &(address, index, value, missing, fill)) in items.iter().enumerate() {
            let at = CHAIN_HEADER + k * spec.item_bytes();
            for (i, w) in [address, index, value, missing].iter().enumerate() {
                b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&w.to_be_bytes());
            }
            b[at + CHAIN_ITEM_HEADER..at + spec.item_bytes()].fill(fill);
        }
        b
    }

    #[test]
    fn a_chain_block_parses_and_what_doesnt_hold_together_is_refused() {
        let spec = chain_spec();
        let good = block(
            &spec,
            [0, 166, 2, 5],
            &[
                (0x3500_0000, 3, 0, 0, 0x11),
                (0x3500_1000, 90, 1, 0b100, 0x22),
            ],
        );
        let b = ChainBlock::parse(&spec, &good).unwrap();
        assert_eq!(
            (b.status, b.listed, b.unread, b.matched, b.dropped()),
            (0, 166, 2, 5, 3)
        );
        assert_eq!(b.items[0].range(0), Some(&[0x11; 24][..]));
        assert_eq!(b.items[1].range(1), Some(&[0x22; 12][..]));
        assert_eq!(b.items[1].range(2), None);
        assert_eq!(b.items[1].ranges[2].len(), 8);
        // A broken list: nothing listed, nothing kept.
        assert!(ChainBlock::parse(&spec, &block(&spec, [3, 0, 0, 0], &[])).is_ok());
        let refused = [
            // Wrong size.
            good[..good.len() - 1].to_vec(),
            // A broken list listing something.
            block(&spec, [3, 1, 0, 0], &[]),
            // More kept than matched; matched and unread past listed.
            block(&spec, [0, 9, 0, 1], &[(1, 0, 0, 0, 0), (2, 1, 0, 0, 0)]),
            block(&spec, [0, 4, 2, 3], &[]),
            // More kept than the chain keeps: four items laid out, the header
            // saying five.
            {
                let mut b = block(
                    &spec,
                    [0, 9, 0, 5],
                    &[
                        (1, 0, 0, 0, 0),
                        (2, 1, 0, 0, 0),
                        (3, 2, 0, 0, 0),
                        (4, 3, 0, 0, 0),
                    ],
                );
                b[16..20].copy_from_slice(&5u32.to_be_bytes());
                b
            },
            // A value the spec doesn't have, a range it doesn't copy, an index
            // past the list or out of order.
            block(&spec, [0, 9, 0, 1], &[(1, 0, 2, 0, 0)]),
            block(&spec, [0, 9, 0, 1], &[(1, 0, 0, 0b1000, 0)]),
            block(&spec, [0, 9, 0, 1], &[(1, 9, 0, 0, 0)]),
            block(&spec, [0, 9, 0, 2], &[(1, 4, 0, 0, 0), (2, 4, 0, 0, 0)]),
            // A count past O6's list cap.
            block(&spec, [0, CHAIN_MAX_LIST + 1, 0, 0], &[]),
        ];
        for (i, bytes) in refused.iter().enumerate() {
            assert!(ChainBlock::parse(&spec, bytes).is_err(), "case {i}");
        }
        // A byte past the kept items.
        let mut tail = good.clone();
        *tail.last_mut().unwrap() = 1;
        assert!(ChainBlock::parse(&spec, &tail).is_err());
    }

    #[test]
    fn a_payload_splits_into_its_list_and_each_chains_block() {
        let a = chain_spec();
        let mut b = chain_spec();
        b.items = 1;
        let payload: Vec<u8> = (0..(6 + a.bytes() + b.bytes())).map(|i| i as u8).collect();
        let (ranges, blocks) = split_payload(&payload, 6, &[&a, &b]).unwrap();
        assert_eq!(ranges, &payload[..6]);
        assert_eq!(
            blocks.iter().map(|x| x.len()).collect::<Vec<_>>(),
            [a.bytes(), b.bytes()]
        );
        assert_eq!(blocks[1], &payload[6 + a.bytes()..]);
        assert!(split_payload(&payload, 7, &[&a, &b]).is_none());
        assert!(split_payload(&payload[..6], 6, &[]).is_some());
    }
}
