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
use std::time::Duration;

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
    /// Send this many bytes of each reply, wait this long, then send the rest.
    stall_reply: Option<(usize, Duration)>,
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
        if let Some((n, wait)) = quirks.stall_reply {
            let n = n.min(reply.len());
            if stream.write_all(&reply[..n]).is_err() {
                return;
            }
            thread::sleep(wait);
            reply.drain(..n);
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
    // Out of step now: refused without another message going out.
    assert!(matches!(c.title_id(), Err(PineError::Poisoned)));
    assert_eq!(fake.messages.load(Ordering::Relaxed), 1);
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

#[test]
fn memory_waits_for_a_pause_when_asked_to() {
    let mut running = guest();
    running.status = 0;
    let fake = spawn(running, Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    assert!(matches!(
        c.read_paused(0x3000_0000, 8),
        Err(PineError::NotPaused(EmuStatus::Running))
    ));
    assert!(matches!(
        c.write_paused(0x3000_0000, &[0xAA; 8]),
        Err(PineError::NotPaused(EmuStatus::Running))
    ));
    // Only the two status asks went out; the guest kept its bytes.
    assert_eq!(fake.messages.load(Ordering::Relaxed), 2);
    assert_eq!(fake.guest.lock().unwrap().regions[0].bytes[..2], [0, 1]);

    fake.guest.lock().unwrap().status = 1;
    c.write_paused(0x3000_0000, &[0xAA; 8]).unwrap();
    assert_eq!(c.read_paused(0x3000_0000, 8).unwrap(), [0xAA; 8]);
}

#[test]
fn a_reply_that_times_out_midway_poisons_the_connection() {
    // 6 of 13 bytes, then the rest after the client has given up: read again,
    // the tail would be taken for the next reply's size field.
    let fake = spawn(
        guest(),
        Quirks {
            stall_reply: Some((6, Duration::from_millis(400))),
            ..Quirks::default()
        },
    );
    let mut c = PineClient::connect(&fake.path).unwrap();
    c.set_timeout(Duration::from_millis(100)).unwrap();
    assert!(matches!(
        c.read_bytes(0x3000_0000, 8),
        Err(PineError::Io(_))
    ));
    thread::sleep(Duration::from_millis(500));
    assert!(matches!(c.status(), Err(PineError::Poisoned)));
    assert_eq!(fake.messages.load(Ordering::Relaxed), 1);
    // A new connection is in step again (the server takes one client at a time).
    drop(c);
    let mut fresh = PineClient::connect(&fake.path).unwrap();
    assert_eq!(fresh.title_id().unwrap(), "BLUS30968");
}

// ---- AF.8: the word search and the RNG verbs over the same fake -------------

fn region(base: u32, len: usize) -> Region {
    Region {
        base,
        bytes: vec![0; len],
        writable: true,
    }
}

fn plant(guest: &mut Guest, addr: u32, word: u32) {
    let (mem, at) = guest.region(addr, 4, false).expect("planted in a region");
    mem[at..at + 4].copy_from_slice(&word.to_be_bytes());
}

#[test]
fn a_word_search_probes_pages_and_reads_around_a_hole() {
    use std::collections::{BTreeMap, BTreeSet};

    use oracle_harness::scan::{Area, find_words};

    // A 64 KiB page whose first 32 KiB are mapped, then a 4 KiB hole, then the
    // rest; a page past it unmapped; and a lone 4 KiB stack page.
    let mut g = Guest {
        regions: vec![
            region(0x3000_0000, 0x8000),
            region(0x3000_9000, 0x7000),
            region(0xD000_1000, 0x1000),
        ],
        status: 1,
    };
    for at in [0x3000_0010, 0x3000_9FFC, 0x3000_FFFC, 0xD000_1008] {
        plant(&mut g, at, 0x0064_6638);
    }
    plant(&mut g, 0x3000_0020, 0x3000_0010);
    let fake = spawn(g, Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    let areas = [
        Area {
            start: 0x3000_0000,
            end: 0x3002_0000,
            page: 0x1_0000,
        },
        Area {
            start: 0xD000_0000,
            end: 0xD000_4000,
            page: 0x1000,
        },
    ];
    let words = BTreeSet::from([0x0064_6638, 0x3000_0010]);
    let found = find_words(&mut c, &areas, &words).unwrap();
    assert_eq!(
        found.hits,
        BTreeMap::from([
            (
                0x0064_6638,
                vec![0x3000_0010, 0x3000_9FFC, 0x3000_FFFC, 0xD000_1008]
            ),
            (0x3000_0010, vec![0x3000_0020]),
        ])
    );
    assert_eq!(
        found.mapped,
        [0x3000_0000..0x3001_0000, 0xD000_1000..0xD000_2000]
    );
    // The chunk read failed on the hole; the 4 KiB pages around it were read.
    assert_eq!(found.unreadable, [0x3000_8000]);
    assert_eq!(found.bytes, 0xF000 + 0x1000);
    // 2 + 4 probes, the failed chunk and its 16 pages, the stack page, two status asks.
    assert_eq!(found.messages, 6 + 1 + 16 + 1);
    assert_eq!(
        fake.messages.load(Ordering::Relaxed) as u64,
        found.messages + 2
    );

    fake.guest.lock().unwrap().status = 0;
    let err = find_words(&mut c, &areas, &words).unwrap_err().to_string();
    assert!(err.contains("Running before the search"), "{err}");
}

/// Giants' RNG layout in the fake: the globals in the ELF's data, the meta and
/// the generator in the heap, as the census found them. The generator is
/// seeded 10554 and has drawn 104,201 numbers, as AF.5's state had.
fn giants_rng() -> (Guest, guac_oracle::rng::Mt19937) {
    use oracle_harness::rng::{ANIM_LCG, FMOD_GLOBAL, MT_GLOBAL, MT_META_SLOT, MT_VTABLE};

    let mut mt = guac_oracle::rng::Mt19937::sgenrand(10_554);
    mt.advance(104_201);
    let mut g = Guest {
        regions: vec![
            region(0x0089_8000, 0x1000),
            region(0x008E_0000, 0x3000),
            region(0x008F_0000, 0x1000),
            region(0x0090_0000, 0x1000),
            region(0x3028_0000, 0x1_0000),
        ],
        status: 1,
    };
    plant(&mut g, MT_META_SLOT, 0x3028_E734);
    plant(&mut g, MT_GLOBAL, 0x3028_E7F0);
    plant(&mut g, 0x3028_E734 + 0x18, 1);
    plant(&mut g, 0x3028_E734 + 0x20, MT_VTABLE);
    plant(&mut g, 0x3028_E7F0, MT_VTABLE);
    plant(&mut g, 0x3028_E7F4, 1);
    plant(&mut g, 0x3028_E7F8, mt.mti());
    for (i, w) in mt.words().iter().enumerate() {
        plant(&mut g, 0x3028_E7FC + 4 * i as u32, *w);
    }
    plant(&mut g, ANIM_LCG, 0x12345);
    plant(&mut g, ANIM_LCG + 4, 0x12345);
    plant(&mut g, FMOD_GLOBAL, 0x0090_0000);
    (g, mt)
}

#[test]
fn the_rng_verbs_peek_seed_restore_and_census_over_pine() {
    use oracle_harness::rng::{self, Hit, MT_GLOBAL, Peek};
    use oracle_harness::scan::Area;

    let (g, mt) = giants_rng();
    let fake = spawn(g, Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    rng::check_title(&mut c).unwrap();

    // Running: refused before a byte is read or written.
    fake.guest.lock().unwrap().status = 0;
    let err = format!("{:#}", Peek::read(&mut c).unwrap_err());
    assert!(err.contains("Running"), "{err}");
    assert!(rng::seed(&mut c, 5150).is_err());
    fake.guest.lock().unwrap().status = 1;

    let first = Peek::read(&mut c).unwrap();
    assert_eq!(first, Peek::read(&mut c).unwrap(), "two peeks at one pause");
    assert_eq!(first.mt, mt);
    assert_eq!((first.instance, first.instances), (0x3028_E7F0, 1));
    assert_eq!(first.origin(rng::ORIGIN_TWISTS).unwrap().seed, 10_554);
    assert_eq!(first.fmod, Some(0));

    rng::seed(&mut c, 4357).unwrap();
    let seeded = Peek::read(&mut c).unwrap();
    assert_eq!(seeded.mt, guac_oracle::rng::Mt19937::sgenrand(4357));
    assert_eq!(seeded.anim, first.anim);
    rng::restore(&mut c, &first).unwrap();
    assert_eq!(Peek::read(&mut c).unwrap(), first);

    let areas = [
        Area {
            start: 0x0080_0000,
            end: 0x0092_0000,
            page: 0x1_0000,
        },
        Area {
            start: 0x3028_0000,
            end: 0x3029_0000,
            page: 0x1_0000,
        },
    ];
    let census = rng::census(&mut c, &areas).unwrap();
    assert_eq!(
        census.hits,
        [
            Hit::MetaField { at: 0x3028_E754 },
            Hit::Instance {
                at: 0x3028_E7F0,
                refcount: 1,
                mti: 617
            }
        ]
    );
    assert_eq!(census.owners[&0x3028_E7F0], [MT_GLOBAL]);
    assert_eq!(census.peek, first);
}

/// The actor reader (AF.9) over PINE: the fixture's world, which lays out
/// Giants' chain from the application pointer down to the actors, served by the
/// fake.
fn actor_world() -> (
    oracle_harness::actors::fixture::World,
    oracle_harness::actors::fixture::Added,
) {
    use oracle_harness::actors::fixture::{Spec, World};

    let mut w = World::new();
    w.camera([6.5107, 52.2663, -107.1263], [-23.3597, 14.9997, 0.0]);
    let hero = w.actor(&Spec {
        name: "009_Eruptor".into(),
        position: [10.190_27, 43.382_67, -93.394_06],
        scale: [150.0; 3],
        skinned: true,
        pad: Some(-1),
        ..Spec::default()
    });
    for i in 0..40 {
        w.actor(&Spec {
            name: format!("Bird_template_Red({i})"),
            position: [i as f32, 44.0, -90.0],
            rotation: [0.0, 9.0 * i as f32, 0.0],
            scale: [85.0; 3],
            skinned: i % 2 == 0,
            ..Spec::default()
        });
    }
    (w, hero)
}

fn served(world: &oracle_harness::actors::fixture::World) -> Guest {
    Guest {
        regions: world
            .regions()
            .into_iter()
            .map(|(base, bytes)| Region {
                base,
                bytes,
                writable: true,
            })
            .collect(),
        status: 1,
    }
}

#[test]
fn the_actor_reader_walks_the_chain_over_pine_in_a_few_batches() {
    use oracle_harness::actors::{self, read_frame};

    let (mut world, hero) = actor_world();
    let want = actors::read(&mut world).unwrap();
    assert_eq!(want.counts().skinned, 21);
    let fake = spawn(served(&world), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();

    // Running: refused at the first status ask, before a byte is read.
    fake.guest.lock().unwrap().status = 0;
    let err = format!("{:#}", read_frame(&mut c).unwrap_err());
    assert!(err.contains("Running"), "{err}");
    let asked = fake.messages.load(Ordering::Relaxed);
    assert_eq!(asked, 3, "the title, the version and one status ask");
    fake.guest.lock().unwrap().status = 1;

    let frame = read_frame(&mut c).unwrap();
    assert_eq!(frame, want);
    assert_eq!(frame.hero().unwrap().corpus, hero.corpus);
    // The title and version, then a status ask and one batch per stage of the
    // walk (14), then the status ask after: 41 actors in 31 messages.
    assert_eq!(
        fake.messages.load(Ordering::Relaxed) - asked,
        2 + 2 * 14 + 1
    );

    // A corpus pointer into nothing fails its stage's batch; that stage is read
    // again range by range, and only that corpus is lost.
    let mut broken = world;
    broken.push_corpus(0x4F00_0000);
    let fake = spawn(served(&broken), Quirks::default());
    let mut c = PineClient::connect(&fake.path).unwrap();
    let frame = read_frame(&mut c).unwrap();
    assert_eq!(frame.listed, 42);
    assert_eq!(frame.actors, want.actors);
    assert_eq!(frame.skipped.len(), 1);
    assert_eq!(
        (frame.skipped[0].corpus, frame.skipped[0].why.as_str()),
        (0x4F00_0000, "not readable")
    );
}
