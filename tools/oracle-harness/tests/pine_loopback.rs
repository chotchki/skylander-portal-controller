//! The PINE client against an in-process fake of RPCS3's PINE server (openspec
//! AF.6). The fake ports `pine_server<Impl>::ParseCommand` and its read loop
//! from `3rdparty/pine/pine_server.h` at `accfecd2d` as they are, quirks
//! included (`MsgStatus` skips 4 request bytes; a failing op fails the whole
//! batch after the ops before it ran), so it's the executable spec the live
//! server has to match. Guest memory is big-endian, as on the PS3.

#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use oracle_harness::pine::{
    Answer, EmuStatus, MAX_REPLY, MAX_REQUEST, Op, PineClient, PineError, Width,
};

const OK: u8 = 0;
const FAIL: u8 = 0xFF;

/// One mapped range of guest memory.
struct Region {
    base: u32,
    bytes: Vec<u8>,
    writable: bool,
}

#[derive(Default)]
struct Guest {
    regions: Vec<Region>,
    status: u32,
}

impl Guest {
    /// `check_addr<N>`: the whole range mapped (and writable for a write).
    fn region(&mut self, addr: u32, n: usize, write: bool) -> Option<(&mut Vec<u8>, usize)> {
        let r = self.regions.iter_mut().find(|r| {
            addr >= r.base
                && (addr - r.base) as usize + n <= r.bytes.len()
                && (!write || r.writable)
        })?;
        let at = (addr - r.base) as usize;
        Some((&mut r.bytes, at))
    }
}

/// How the fake misbehaves, for the error paths.
#[derive(Default, Clone, Copy)]
struct Quirks {
    /// Send only this many bytes of each reply, then hang up.
    truncate_reply: Option<usize>,
    /// Overwrite the reply's status byte.
    status_byte: Option<u8>,
}

struct Fake {
    path: PathBuf,
    guest: Arc<Mutex<Guest>>,
    messages: Arc<AtomicUsize>,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn spawn(guest: Guest, quirks: Quirks) -> Fake {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("pine-{}-{n}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let guest = Arc::new(Mutex::new(guest));
    let messages = Arc::new(AtomicUsize::new(0));
    let (g, m) = (Arc::clone(&guest), Arc::clone(&messages));
    thread::spawn(move || {
        // The server serves one client at a time and re-accepts after it goes.
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            serve(stream, &g, &m, quirks);
        }
    });
    Fake {
        path,
        guest,
        messages,
    }
}

/// `pine_server::operator()`'s loop for one connection.
fn serve(mut stream: UnixStream, guest: &Mutex<Guest>, messages: &AtomicUsize, quirks: Quirks) {
    let mut buf = vec![0u8; MAX_REQUEST];
    loop {
        let mut received = 0usize;
        let mut end = 4usize;
        while received < end {
            match stream.read(&mut buf[received..]) {
                Ok(0) | Err(_) => return,
                Ok(n) => received += n,
            }
            if end == 4 && received >= 4 {
                end = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
                if !(4..=MAX_REQUEST).contains(&end) {
                    // Dropped unanswered, as RPCS3 does.
                    received = 0;
                    break;
                }
            }
        }
        if received == 0 {
            continue;
        }
        messages.fetch_add(1, Ordering::Relaxed);
        let mut reply = parse_command(&buf[4..end], &mut guest.lock().unwrap());
        if let Some(status) = quirks.status_byte {
            reply[4] = status;
        }
        if let Some(n) = quirks.truncate_reply {
            let _ = stream.write_all(&reply[..n.min(reply.len())]);
            return;
        }
        if stream.write_all(&reply).is_err() {
            return;
        }
    }
}

/// `SafetyChecks`: true when the command and the reply both still fit.
fn fits(
    cmd_len: usize,
    cmd_size: usize,
    reply_len: usize,
    reply_size: usize,
    buf_size: usize,
) -> bool {
    !(cmd_len + cmd_size > buf_size || reply_len + reply_size >= MAX_REPLY)
}

/// `ParseCommand`, ported line for line.
fn parse_command(buf: &[u8], guest: &mut Guest) -> Vec<u8> {
    let buf_size = buf.len();
    let mut ret = vec![0u8; 5];
    let mut at = 0usize;
    let fail = || {
        let mut r = 5u32.to_le_bytes().to_vec();
        r.push(FAIL);
        r
    };
    let addr = |at: usize| u32::from_le_bytes(buf[at..at + 4].try_into().unwrap());
    while at < buf_size {
        if !fits(at, 1, ret.len(), 0, buf_size) {
            return fail();
        }
        at += 1;
        match buf[at - 1] {
            op @ 0..=3 => {
                let n = 1usize << op;
                if !fits(at, 4, ret.len(), n, buf_size) {
                    return fail();
                }
                let Some((mem, i)) = guest.region(addr(at), n, false) else {
                    return fail();
                };
                // vm::readN reads the guest's big-endian word; ToArray writes it LE.
                let mut word = [0u8; 8];
                word[8 - n..].copy_from_slice(&mem[i..i + n]);
                let value = u64::from_be_bytes(word);
                ret.extend_from_slice(&value.to_le_bytes()[..n]);
                at += 4;
            }
            op @ 4..=7 => {
                let n = 1usize << (op - 4);
                if !fits(at, n + 4, ret.len(), 0, buf_size) {
                    return fail();
                }
                let mut word = [0u8; 8];
                word[..n].copy_from_slice(&buf[at + 4..at + 4 + n]);
                let value = u64::from_le_bytes(word);
                let Some((mem, i)) = guest.region(addr(at), n, true) else {
                    return fail();
                };
                mem[i..i + n].copy_from_slice(&value.to_be_bytes()[8 - n..]);
                at += 4 + n;
            }
            0x0F => {
                if !fits(at, 0, ret.len(), 4, buf_size) {
                    return fail();
                }
                ret.extend_from_slice(&guest.status.to_le_bytes());
                at += 4; // RPCS3's quirk: MsgStatus steps over 4 request bytes
            }
            code @ (0x08 | 0x0B..=0x0E) => {
                let text = match code {
                    0x08 => "RPCS3 fake-pine",
                    0x0B => "Skylanders Giants",
                    0x0C => "BLUS30968",
                    0x0D => "0123abcd",
                    _ => "01.00",
                };
                if !fits(at, 0, ret.len(), text.len() + 1 + 4, buf_size) {
                    return fail();
                }
                ret.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
                ret.extend_from_slice(text.as_bytes());
                ret.push(0);
            }
            _ => return fail(),
        }
    }
    let len = ret.len() as u32;
    ret[..4].copy_from_slice(&len.to_le_bytes());
    ret[4] = OK;
    ret
}

/// RAM at 0x3000_0000 (1 MiB, byte i = i % 251) and a read-only page at 0x1000.
fn guest() -> Guest {
    Guest {
        regions: vec![
            Region {
                base: 0x3000_0000,
                bytes: (0..1 << 20).map(|i| (i % 251) as u8).collect(),
                writable: true,
            },
            Region {
                base: 0x1000,
                bytes: vec![0xEE; 0x1000],
                writable: false,
            },
        ],
        status: 1,
    }
}

#[test]
fn strings_and_status_come_back_framed() {
    let fake = spawn(guest(), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    assert_eq!(c.title_id().unwrap(), "BLUS30968");
    assert_eq!(c.title().unwrap(), "Skylanders Giants");
    assert_eq!(c.version().unwrap(), "RPCS3 fake-pine");
    assert_eq!(c.status().unwrap(), EmuStatus::Paused);
    for (code, want) in [
        (0, EmuStatus::Running),
        (2, EmuStatus::Shutdown),
        (9, EmuStatus::Other(9)),
    ] {
        fake.guest.lock().unwrap().status = code;
        assert_eq!(c.status().unwrap(), want);
    }
}

#[test]
fn a_status_op_mid_batch_keeps_the_ops_after_it_aligned() {
    let fake = spawn(guest(), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    let got = c
        .batch(&[
            Op::Read {
                width: Width::W32,
                addr: 0x3000_0000,
            },
            Op::Status,
            Op::Read {
                width: Width::W16,
                addr: 0x3000_00FB,
            },
        ])
        .unwrap();
    assert_eq!(
        got,
        [
            Answer::Value(0x0001_0203),
            Answer::Status(EmuStatus::Paused),
            Answer::Value(0x0001), // 0xFB % 251 = 0, then 1
        ]
    );
}

#[test]
fn bytes_round_trip_at_every_alignment() {
    let fake = spawn(guest(), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    for (addr, len) in [
        (0x3000_0000u32, 64usize),
        (0x3000_0003, 29),
        (0x3000_0011, 1),
        (0x3000_0008, 0),
    ] {
        let want: Vec<u8> = (0..len)
            .map(|i| (((addr - 0x3000_0000) as usize + i) % 251) as u8)
            .collect();
        assert_eq!(c.read_bytes(addr, len).unwrap(), want, "0x{addr:x}+{len}");
    }
    let new: Vec<u8> = (0..37u8).map(|b| b.wrapping_mul(7)).collect();
    c.write_bytes(0x3000_0105, &new).unwrap();
    assert_eq!(c.read_bytes(0x3000_0105, new.len()).unwrap(), new);
    let guest = fake.guest.lock().unwrap();
    assert_eq!(&guest.regions[0].bytes[0x105..0x105 + 37], &new[..]);
}

#[test]
fn a_mebibyte_read_splits_under_the_reply_cap() {
    let fake = spawn(guest(), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    let bytes = c.read_bytes(0x3000_0000, 1 << 20).unwrap();
    assert_eq!(bytes, fake.guest.lock().unwrap().regions[0].bytes);
    // 131,072 64-bit reads at 56,249 a reply: three messages.
    assert_eq!(fake.messages.load(Ordering::Relaxed), 3);

    // Writes answer nothing, so 512 KiB of them split on the request cap instead.
    let fill = vec![0x5A; 512 << 10];
    c.write_bytes(0x3000_0000, &fill).unwrap();
    assert_eq!(fake.messages.load(Ordering::Relaxed), 3 + 2);
    assert_eq!(c.read_bytes(0x3000_0000, fill.len()).unwrap(), fill);
}

#[test]
fn a_bad_address_fails_the_batch_after_the_ops_before_it_ran() {
    let fake = spawn(guest(), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    let err = c
        .batch(&[
            Op::Write {
                width: Width::W8,
                addr: 0x3000_0000,
                value: 0x77,
            },
            Op::Write {
                width: Width::W8,
                addr: 0x1000,
                value: 0x77,
            }, // read-only
        ])
        .unwrap_err();
    assert!(matches!(err, PineError::Failed), "{err}");
    assert_eq!(fake.guest.lock().unwrap().regions[0].bytes[0], 0x77);
    assert!(matches!(
        c.read_bytes(0x0800_0000, 8),
        Err(PineError::Failed)
    ));
    // The connection survives a failed batch.
    assert_eq!(c.read_bytes(0x1000, 2).unwrap(), [0xEE, 0xEE]);
}

#[test]
fn a_short_reply_is_refused() {
    // 9 of the 13 bytes an 8-byte read answers with, then the server hangs up.
    let fake = spawn(
        guest(),
        Quirks {
            truncate_reply: Some(9),
            ..Quirks::default()
        },
    );
    let mut c = PineClient::connect(&fake.path).unwrap();
    let err = c.read_bytes(0x3000_0000, 8).unwrap_err();
    assert!(
        matches!(err, PineError::Short { needed: 13, got: 9 }),
        "{err}"
    );
    // Cut inside the size field: the read itself fails.
    let fake = spawn(
        guest(),
        Quirks {
            truncate_reply: Some(2),
            ..Quirks::default()
        },
    );
    let mut c = PineClient::connect(&fake.path).unwrap();
    assert!(matches!(c.title_id(), Err(PineError::Io(_))));
}

#[test]
fn an_unknown_status_byte_is_refused() {
    let fake = spawn(
        guest(),
        Quirks {
            status_byte: Some(0x01),
            ..Quirks::default()
        },
    );
    let mut c = PineClient::connect(&fake.path).unwrap();
    assert!(matches!(
        c.read_bytes(0x3000_0000, 8),
        Err(PineError::UnknownStatus(0x01))
    ));
}
