//! A PINE client: guest memory in bulk through the server RPCS3 runs when
//! `ipc.yml` turns it on (openspec AF.6, design decision 4). PINE has no pause
//! of its own, so a read of a running game races every guest thread and a write
//! races them all the worse: [`PineClient::read_paused`] and
//! [`PineClient::write_paused`] ask PINE's status first and refuse unless the
//! emulator is paused, and the harness's own reads and writes go through them.
//! [`PineClient::read_bytes`] and [`PineClient::write_bytes`] don't ask.
//!
//! The wire, as `3rdparty/pine/pine_server.h` reads and writes it at both pins,
//! every integer little-endian:
//! - request: a u32 total size (its own 4 bytes included), then ops back to
//!   back, each an opcode byte and its arguments (a u32 address, plus the value
//!   at the op's width for a write);
//! - reply: a u32 total size, a status byte (0 OK, 0xFF failed), then each op's
//!   answer in order: a read's value at its width, a string as a u32 length
//!   (NUL included) and its bytes and NUL, a status as a u32; a write answers
//!   nothing.
//!
//! Values cross as numbers: RPCS3 reads the guest's big-endian word, so a read's
//! value is the guest bytes taken big-endian, and a write stores its value
//! big-endian. A request over [`MAX_REQUEST`] bytes is dropped unanswered (and
//! its tail parsed as the next message), so the client never sends one; a reply
//! that would reach [`MAX_REPLY`] bytes fails the whole batch. A failed batch has
//! already run every op before the one that failed, writes included.
//!
//! RPCS3's server consumes 4 request bytes after `MsgStatus`'s opcode, where
//! PCSX2's takes none; the client pads it with 4 zeros, so a status op works
//! anywhere in a batch against RPCS3 (only).
//!
//! Drop the client before anything stops the emulator (`SAVESTATE`'s reboot, a
//! shutdown). The server's one thread serves one client with a blocking read
//! and checks for its abort only between messages, so the stop waits on it for
//! as long as an idle client stays connected: AF.8's first live run hung at
//! `stopped` for 120 s, RPCS3's log repeating "Thread [IPC Server] is too
//! sleepy", until the harness gave up.

use std::fmt;
#[cfg(unix)]
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

/// `MAX_IPC_SIZE`: the longest request the server reads, size field included.
pub const MAX_REQUEST: usize = 650_000;
/// `MAX_IPC_RETURN_SIZE`: a reply must stay strictly below it.
pub const MAX_REPLY: usize = 450_000;
const SIZE_FIELD: usize = 4;
const REPLY_HEAD: usize = SIZE_FIELD + 1;

const OK: u8 = 0x00;
const FAIL: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    W8,
    W16,
    W32,
    W64,
}

impl Width {
    pub fn bytes(self) -> usize {
        match self {
            Width::W8 => 1,
            Width::W16 => 2,
            Width::W32 => 4,
            Width::W64 => 8,
        }
    }

    fn read_opcode(self) -> u8 {
        match self {
            Width::W8 => 0,
            Width::W16 => 1,
            Width::W32 => 2,
            Width::W64 => 3,
        }
    }

    fn write_opcode(self) -> u8 {
        self.read_opcode() + 4
    }
}

/// One PINE command; [`batches`] packs a run of them into messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read {
        width: Width,
        addr: u32,
    },
    /// `value` is truncated to `width`.
    Write {
        width: Width,
        addr: u32,
        value: u64,
    },
    /// `RPCS3 <version and branch>`.
    Version,
    Title,
    /// The serial, `BLUS30968` for Giants.
    TitleId,
    /// The executable's hash.
    Uuid,
    GameVersion,
    Status,
}

impl Op {
    pub fn opcode(&self) -> u8 {
        match self {
            Op::Read { width, .. } => width.read_opcode(),
            Op::Write { width, .. } => width.write_opcode(),
            Op::Version => 0x08,
            Op::Title => 0x0B,
            Op::TitleId => 0x0C,
            Op::Uuid => 0x0D,
            Op::GameVersion => 0x0E,
            Op::Status => 0x0F,
        }
    }

    fn request_len(&self) -> usize {
        match self {
            Op::Read { .. } => 1 + 4,
            Op::Write { width, .. } => 1 + 4 + width.bytes(),
            Op::Status => 1 + 4,
            _ => 1,
        }
    }

    /// The answer's length, `None` for a string (its length comes first).
    fn reply_len(&self) -> Option<usize> {
        match self {
            Op::Read { width, .. } => Some(width.bytes()),
            Op::Write { .. } => Some(0),
            Op::Status => Some(4),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Value(u64),
    Written,
    Text(String),
    Status(EmuStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmuStatus {
    Running,
    Paused,
    Shutdown,
    /// A code PINE doesn't define.
    Other(u32),
}

impl From<u32> for EmuStatus {
    fn from(code: u32) -> Self {
        match code {
            0 => EmuStatus::Running,
            1 => EmuStatus::Paused,
            2 => EmuStatus::Shutdown,
            n => EmuStatus::Other(n),
        }
    }
}

#[derive(Debug)]
pub enum PineError {
    /// The ops don't fit one request; [`batches`] splits them.
    RequestTooLong(usize),
    /// The answers would reach the reply cap; [`batches`] splits them.
    ReplyTooLong(usize),
    /// Status 0xFF: an op hit an unmapped or unwritable address, or a reply ran
    /// past the cap. Ops before it ran.
    Failed,
    UnknownStatus(u8),
    /// The reply ended before the answers the ops need.
    Short {
        needed: usize,
        got: usize,
    },
    /// The reply's size field disagrees with the bytes that came.
    SizeMismatch {
        field: usize,
        got: usize,
    },
    /// Bytes left after the last answer.
    Trailing(usize),
    BadText(String),
    /// A byte range past the end of the 32-bit guest address space.
    AddressRange {
        addr: u32,
        len: usize,
    },
    Io(std::io::Error),
    /// PINE is TCP on Windows, which the harness doesn't speak yet.
    Unsupported,
    /// A paused-only read or write found the emulator in this state.
    NotPaused(EmuStatus),
    /// An earlier exchange broke off mid-message (an I/O error or timeout, a
    /// short reply, a size field out of range), so the stream is out of step;
    /// connect again.
    Poisoned,
}

impl fmt::Display for PineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PineError::RequestTooLong(n) => write!(
                f,
                "PINE request of {n} bytes is over the {MAX_REQUEST}-byte cap"
            ),
            PineError::ReplyTooLong(n) => write!(
                f,
                "PINE reply of {n} bytes would reach the {MAX_REPLY}-byte cap"
            ),
            PineError::Failed => write!(
                f,
                "PINE failed the batch (an unmapped or unwritable address; ops before it ran)"
            ),
            PineError::UnknownStatus(s) => {
                write!(f, "PINE reply status 0x{s:02x} is neither OK nor FAIL")
            }
            PineError::Short { needed, got } => write!(
                f,
                "PINE reply is short: {got} bytes where the ops need {needed}"
            ),
            PineError::SizeMismatch { field, got } => {
                write!(f, "PINE reply says {field} bytes but {got} came")
            }
            PineError::Trailing(n) => write!(f, "PINE reply has {n} bytes past the last answer"),
            PineError::BadText(why) => write!(f, "PINE string answer: {why}"),
            PineError::AddressRange { addr, len } => {
                write!(
                    f,
                    "{len} bytes from 0x{addr:08x} run past the 32-bit address space"
                )
            }
            PineError::Io(e) => write!(f, "PINE socket: {e}"),
            PineError::Unsupported => write!(
                f,
                "PINE over TCP (Windows) is Mac only until the Windows/HTPC bring-up"
            ),
            PineError::NotPaused(s) => write!(
                f,
                "PINE reports the emulator {s:?}; guest memory is read and written only while it's paused"
            ),
            PineError::Poisoned => write!(
                f,
                "this PINE connection broke off mid-message and is out of step; connect again"
            ),
        }
    }
}

impl std::error::Error for PineError {}

impl From<std::io::Error> for PineError {
    fn from(e: std::io::Error) -> Self {
        PineError::Io(e)
    }
}

/// The request bytes for `ops`, in one message.
pub fn encode(ops: &[Op]) -> Result<Vec<u8>, PineError> {
    let len = SIZE_FIELD + ops.iter().map(Op::request_len).sum::<usize>();
    if len > MAX_REQUEST {
        return Err(PineError::RequestTooLong(len));
    }
    let reply = REPLY_HEAD + ops.iter().filter_map(Op::reply_len).sum::<usize>();
    if reply >= MAX_REPLY {
        return Err(PineError::ReplyTooLong(reply));
    }
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    for op in ops {
        out.push(op.opcode());
        match *op {
            Op::Read { addr, .. } => out.extend_from_slice(&addr.to_le_bytes()),
            Op::Write { width, addr, value } => {
                out.extend_from_slice(&addr.to_le_bytes());
                out.extend_from_slice(&value.to_le_bytes()[..width.bytes()]);
            }
            Op::Status => out.extend_from_slice(&[0; 4]),
            _ => {}
        }
    }
    Ok(out)
}

/// The answers in `reply` (size field included) to `ops`.
pub fn decode(ops: &[Op], reply: &[u8]) -> Result<Vec<Answer>, PineError> {
    if reply.len() < REPLY_HEAD {
        return Err(PineError::Short {
            needed: REPLY_HEAD,
            got: reply.len(),
        });
    }
    let field = u32::from_le_bytes(reply[..4].try_into().unwrap()) as usize;
    if field != reply.len() {
        return Err(PineError::SizeMismatch {
            field,
            got: reply.len(),
        });
    }
    match reply[4] {
        OK => {}
        FAIL => return Err(PineError::Failed),
        other => return Err(PineError::UnknownStatus(other)),
    }
    let mut at = REPLY_HEAD;
    let mut take = |n: usize| -> Result<&[u8], PineError> {
        let got = reply.get(at..at + n).ok_or(PineError::Short {
            needed: at + n,
            got: reply.len(),
        })?;
        at += n;
        Ok(got)
    };
    let mut answers = Vec::with_capacity(ops.len());
    for op in ops {
        let answer = match op {
            Op::Read { width, .. } => {
                let mut word = [0u8; 8];
                word[..width.bytes()].copy_from_slice(take(width.bytes())?);
                Answer::Value(u64::from_le_bytes(word))
            }
            Op::Write { .. } => Answer::Written,
            Op::Status => Answer::Status(EmuStatus::from(u32::from_le_bytes(
                take(4)?.try_into().unwrap(),
            ))),
            _ => {
                let n = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
                let text = take(n)?;
                let Some((&0, body)) = text.split_last() else {
                    return Err(PineError::BadText(format!("{n} bytes with no closing NUL")));
                };
                Answer::Text(
                    String::from_utf8(body.to_vec())
                        .map_err(|e| PineError::BadText(e.to_string()))?,
                )
            }
        };
        answers.push(answer);
    }
    if at != reply.len() {
        return Err(PineError::Trailing(reply.len() - at));
    }
    Ok(answers)
}

/// Splits `ops` into runs that each fit one request and one reply. A string op
/// goes alone: its answer's length isn't known until it comes.
pub fn batches(ops: &[Op]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut request, mut reply) = (0, SIZE_FIELD, REPLY_HEAD);
    for (i, op) in ops.iter().enumerate() {
        let Some(answer) = op.reply_len() else {
            if start < i {
                out.push(start..i);
            }
            out.push(i..i + 1);
            (start, request, reply) = (i + 1, SIZE_FIELD, REPLY_HEAD);
            continue;
        };
        if request + op.request_len() > MAX_REQUEST || reply + answer >= MAX_REPLY {
            out.push(start..i);
            (start, request, reply) = (i, SIZE_FIELD, REPLY_HEAD);
        }
        request += op.request_len();
        reply += answer;
    }
    if start < ops.len() {
        out.push(start..ops.len());
    }
    out
}

/// The ops that read `len` guest bytes from `addr`: 64-bit reads over the
/// aligned middle, byte reads at the ragged ends.
pub fn read_ops(addr: u32, len: usize) -> Result<Vec<Op>, PineError> {
    let end = addr as u64 + len as u64;
    if end > 1 << 32 {
        return Err(PineError::AddressRange { addr, len });
    }
    let mut ops = Vec::new();
    let mut at = addr as u64;
    while at < end {
        let width = if at.is_multiple_of(8) && end - at >= 8 {
            Width::W64
        } else {
            Width::W8
        };
        ops.push(Op::Read {
            width,
            addr: at as u32,
        });
        at += width.bytes() as u64;
    }
    Ok(ops)
}

/// The guest bytes `answers` to [`read_ops`] spell.
pub fn bytes_of(ops: &[Op], answers: &[Answer]) -> Vec<u8> {
    let mut out = Vec::new();
    for (op, answer) in ops.iter().zip(answers) {
        if let (Op::Read { width, .. }, Answer::Value(v)) = (op, answer) {
            out.extend_from_slice(&v.to_be_bytes()[8 - width.bytes()..]);
        }
    }
    out
}

/// The ops that write `bytes` to the guest at `addr`, shaped as [`read_ops`].
pub fn write_ops(addr: u32, bytes: &[u8]) -> Result<Vec<Op>, PineError> {
    let reads = read_ops(addr, bytes.len())?;
    let mut at = 0;
    Ok(reads
        .into_iter()
        .map(|op| {
            let Op::Read { width, addr } = op else {
                unreachable!()
            };
            let mut word = [0u8; 8];
            word[8 - width.bytes()..].copy_from_slice(&bytes[at..at + width.bytes()]);
            at += width.bytes();
            Op::Write {
                width,
                addr,
                value: u64::from_be_bytes(word),
            }
        })
        .collect())
}

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

/// One connection to RPCS3's PINE server, which serves one client at a time.
/// After an exchange breaks off mid-message every later call is
/// [`PineError::Poisoned`]: the next reply's size field would be stale bytes.
pub struct PineClient {
    #[cfg(unix)]
    stream: Stream,
    poisoned: bool,
}

impl PineClient {
    /// Connects to the socket at `path` (`$TMPDIR/rpcs3.sock` of the oracle's run).
    pub fn connect(path: &Path) -> Result<Self, PineError> {
        #[cfg(unix)]
        {
            let stream = Stream::connect(path)?;
            let mut client = Self {
                stream,
                poisoned: false,
            };
            client.set_timeout(Duration::from_secs(10))?;
            Ok(client)
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(PineError::Unsupported)
        }
    }

    /// The read and write timeout for each exchange (10 s from `connect`). One
    /// that runs out mid-reply poisons the connection.
    #[cfg(unix)]
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<(), PineError> {
        self.stream.set_read_timeout(Some(timeout))?;
        self.stream.set_write_timeout(Some(timeout))?;
        Ok(())
    }

    /// `ops` as one message. Errors when they don't fit one; [`Self::run`] splits.
    /// An error before anything is sent, or in decoding a reply read whole,
    /// leaves the connection usable; one in between poisons it.
    pub fn batch(&mut self, ops: &[Op]) -> Result<Vec<Answer>, PineError> {
        if self.poisoned {
            return Err(PineError::Poisoned);
        }
        let request = encode(ops)?;
        let reply = self
            .exchange(&request)
            .inspect_err(|_| self.poisoned = true)?;
        decode(ops, &reply)
    }

    /// Sends `request` and reads back one whole reply, size field included.
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, PineError> {
        #[cfg(unix)]
        {
            self.stream.write_all(request)?;
            let mut field = [0u8; SIZE_FIELD];
            self.stream.read_exact(&mut field)?;
            let len = u32::from_le_bytes(field) as usize;
            if !(REPLY_HEAD..MAX_REPLY).contains(&len) {
                return Err(PineError::SizeMismatch {
                    field: len,
                    got: SIZE_FIELD,
                });
            }
            let mut reply = field.to_vec();
            reply.resize(len, 0);
            let mut got = SIZE_FIELD;
            while got < len {
                match self.stream.read(&mut reply[got..])? {
                    0 => return Err(PineError::Short { needed: len, got }),
                    n => got += n,
                }
            }
            Ok(reply)
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err(PineError::Unsupported)
        }
    }

    /// `ops` in as many messages as the caps need, answers in order.
    pub fn run(&mut self, ops: &[Op]) -> Result<Vec<Answer>, PineError> {
        let mut answers = Vec::with_capacity(ops.len());
        for range in batches(ops) {
            answers.extend(self.batch(&ops[range])?);
        }
        Ok(answers)
    }

    pub fn read_bytes(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, PineError> {
        let ops = read_ops(addr, len)?;
        let answers = self.run(&ops)?;
        Ok(bytes_of(&ops, &answers))
    }

    pub fn write_bytes(&mut self, addr: u32, bytes: &[u8]) -> Result<(), PineError> {
        self.run(&write_ops(addr, bytes)?).map(|_| ())
    }

    /// [`Self::read_bytes`], refused unless PINE reports the emulator paused.
    pub fn read_paused(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, PineError> {
        self.require_paused()?;
        self.read_bytes(addr, len)
    }

    /// [`Self::write_bytes`], refused unless PINE reports the emulator paused.
    pub fn write_paused(&mut self, addr: u32, bytes: &[u8]) -> Result<(), PineError> {
        self.require_paused()?;
        self.write_bytes(addr, bytes)
    }

    fn require_paused(&mut self) -> Result<(), PineError> {
        match self.status()? {
            EmuStatus::Paused => Ok(()),
            other => Err(PineError::NotPaused(other)),
        }
    }

    fn text(&mut self, op: Op) -> Result<String, PineError> {
        match self.batch(&[op])?.pop() {
            Some(Answer::Text(t)) => Ok(t),
            other => Err(PineError::BadText(format!("{op:?} answered {other:?}"))),
        }
    }

    /// The running game's serial.
    pub fn title_id(&mut self) -> Result<String, PineError> {
        self.text(Op::TitleId)
    }

    pub fn title(&mut self) -> Result<String, PineError> {
        self.text(Op::Title)
    }

    /// The running game's `APP_VER` (`01.00` for the Giants disc).
    pub fn game_version(&mut self) -> Result<String, PineError> {
        self.text(Op::GameVersion)
    }

    pub fn version(&mut self) -> Result<String, PineError> {
        self.text(Op::Version)
    }

    pub fn status(&mut self) -> Result<EmuStatus, PineError> {
        match self.batch(&[Op::Status])?.pop() {
            Some(Answer::Status(s)) => Ok(s),
            other => Err(PineError::BadText(format!("Status answered {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u8, body: &[u8]) -> Vec<u8> {
        let mut out = ((REPLY_HEAD + body.len()) as u32).to_le_bytes().to_vec();
        out.push(status);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn requests_encode_as_rpcs3_reads_them() {
        let ops = [
            Op::Read {
                width: Width::W8,
                addr: 0x3000_0001,
            },
            Op::Read {
                width: Width::W64,
                addr: 0x3000_0008,
            },
            Op::Write {
                width: Width::W16,
                addr: 0x10,
                value: 0xABCD_1234,
            },
            Op::Status,
            Op::TitleId,
        ];
        let bytes = encode(&ops).unwrap();
        #[rustfmt::skip]
        let want: Vec<u8> = [
            &[27, 0, 0, 0][..],
            &[0x00, 0x01, 0x00, 0x00, 0x30],
            &[0x03, 0x08, 0x00, 0x00, 0x30],
            &[0x05, 0x10, 0x00, 0x00, 0x00, 0x34, 0x12], // the value cut to 16 bits
            &[0x0F, 0, 0, 0, 0], // RPCS3 skips 4 bytes after MsgStatus
            &[0x0C],
        ]
        .concat();
        assert_eq!(bytes, want);
    }

    #[test]
    fn replies_decode_every_answer_shape() {
        let ops = [
            Op::Read {
                width: Width::W16,
                addr: 0,
            },
            Op::Write {
                width: Width::W64,
                addr: 8,
                value: 1,
            },
            Op::Read {
                width: Width::W64,
                addr: 8,
            },
            Op::Status,
            Op::TitleId,
            Op::Read {
                width: Width::W32,
                addr: 4,
            },
        ];
        let body = [
            &[0x34, 0x12][..],
            &0x0102_0304_0506_0708u64.to_le_bytes(),
            &1u32.to_le_bytes(),
            &10u32.to_le_bytes(),
            b"BLUS30968\0",
            &0xDEAD_BEEFu32.to_le_bytes(),
        ]
        .concat();
        assert_eq!(
            decode(&ops, &reply(OK, &body)).unwrap(),
            [
                Answer::Value(0x1234),
                Answer::Written,
                Answer::Value(0x0102_0304_0506_0708),
                Answer::Status(EmuStatus::Paused),
                Answer::Text("BLUS30968".into()),
                Answer::Value(0xDEAD_BEEF),
            ]
        );
    }

    #[test]
    fn every_reply_status_and_emulator_status() {
        let ops = [Op::Status];
        for (code, want) in [
            (0, EmuStatus::Running),
            (1, EmuStatus::Paused),
            (2, EmuStatus::Shutdown),
            (7, EmuStatus::Other(7)),
        ] {
            let got = decode(&ops, &reply(OK, &(code as u32).to_le_bytes())).unwrap();
            assert_eq!(got, [Answer::Status(want)]);
        }
        assert!(matches!(
            decode(&ops, &reply(FAIL, &[])),
            Err(PineError::Failed)
        ));
        assert!(matches!(
            decode(&ops, &reply(0x01, &[0; 4])),
            Err(PineError::UnknownStatus(0x01))
        ));
    }

    #[test]
    fn a_short_or_mismatched_reply_is_refused() {
        let ops = [Op::Read {
            width: Width::W64,
            addr: 0,
        }];
        // The size field matches, but 4 of the 8 value bytes are missing.
        assert!(matches!(
            decode(&ops, &reply(OK, &[1, 2, 3, 4])),
            Err(PineError::Short { needed: 13, got: 9 })
        ));
        // Truncated in transit: the field promises more than came.
        let mut cut = reply(OK, &[0; 8]);
        cut.truncate(10);
        assert!(matches!(
            decode(&ops, &cut),
            Err(PineError::SizeMismatch { field: 13, got: 10 })
        ));
        assert!(matches!(
            decode(&ops, &[13, 0]),
            Err(PineError::Short { .. })
        ));
        assert!(matches!(
            decode(&ops, &reply(OK, &[0; 9])),
            Err(PineError::Trailing(1))
        ));
        let text = [Op::Title];
        assert!(matches!(
            decode(&text, &reply(OK, &[3, 0, 0, 0, b'a', b'b', b'c'])),
            Err(PineError::BadText(_))
        ));
    }

    #[test]
    fn batches_split_at_the_reply_cap_and_around_strings() {
        // 56,249 64-bit reads leave the reply at 449,997 bytes; one more reaches the cap.
        let per_batch = (MAX_REPLY - 1 - REPLY_HEAD) / 8;
        assert_eq!(per_batch, 56_249);
        let ops = vec![
            Op::Read {
                width: Width::W64,
                addr: 0
            };
            2 * per_batch + 1
        ];
        let split = batches(&ops);
        assert_eq!(
            split,
            [
                0..per_batch,
                per_batch..2 * per_batch,
                2 * per_batch..ops.len()
            ]
        );
        for range in &split {
            encode(&ops[range.clone()]).unwrap();
        }
        assert!(matches!(
            encode(&ops[..per_batch + 1]),
            Err(PineError::ReplyTooLong(450_005))
        ));

        // Writes answer nothing, so the request cap binds: 49,999 13-byte ops.
        let writes = vec![
            Op::Write {
                width: Width::W64,
                addr: 0,
                value: 0
            };
            50_000
        ];
        let split = batches(&writes);
        assert_eq!(split, [0..49_999, 49_999..50_000]);
        assert!(matches!(
            encode(&writes),
            Err(PineError::RequestTooLong(650_004))
        ));

        let mixed = [
            Op::Read {
                width: Width::W8,
                addr: 0,
            },
            Op::Read {
                width: Width::W8,
                addr: 1,
            },
            Op::TitleId,
            Op::Read {
                width: Width::W8,
                addr: 2,
            },
        ];
        assert_eq!(batches(&mixed), [0..2, 2..3, 3..4]);
        assert!(batches(&[]).is_empty());
    }

    #[test]
    fn byte_ranges_map_to_aligned_reads_and_back() {
        let ops = read_ops(0x1006, 13).unwrap();
        let widths: Vec<_> = ops
            .iter()
            .map(|op| match op {
                Op::Read { width, addr } => (*addr, width.bytes()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            widths,
            [
                (0x1006, 1),
                (0x1007, 1),
                (0x1008, 8),
                (0x1010, 1),
                (0x1011, 1),
                (0x1012, 1)
            ]
        );
        // Guest bytes 0x00.. at 0x1006: what RPCS3 would answer for each read.
        let guest: Vec<u8> = (0..13).collect();
        let answers: Vec<Answer> = ops
            .iter()
            .map(|op| {
                let Op::Read { width, addr } = op else {
                    unreachable!()
                };
                let at = (*addr - 0x1006) as usize;
                let mut word = [0u8; 8];
                word[8 - width.bytes()..].copy_from_slice(&guest[at..at + width.bytes()]);
                Answer::Value(u64::from_be_bytes(word))
            })
            .collect();
        assert_eq!(bytes_of(&ops, &answers), guest);
        let writes = write_ops(0x1006, &guest).unwrap();
        assert_eq!(
            writes[2],
            Op::Write {
                width: Width::W64,
                addr: 0x1008,
                value: 0x0203_0405_0607_0809
            }
        );
        assert!(matches!(
            read_ops(0xFFFF_FFF8, 9),
            Err(PineError::AddressRange { len: 9, .. })
        ));
        assert_eq!(read_ops(0xFFFF_FFF8, 8).unwrap().len(), 1);
    }
}
