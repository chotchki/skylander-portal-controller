//! O5's per-frame recorder against an in-process fake of P1's listener
//! (openspec AG.1). The fake ports `sky_oracle_watch_handle` and the watch
//! beside `pad_get_data` from `rpcs3-patches/oracle/0005-O5-*.patch`, and
//! models the guest around them: guest memory as mapped regions, a flip
//! counter, and a fixed number of pad reads a port each flip, the first of
//! which (on any port) copies the watched ranges into the ring. As on the Mac
//! (AF.7), the reads of the flip a pause lands on come after the pause, once
//! the run resumes. A test can change memory between a flip's reads, unmap a
//! region, or start a new session. It keeps O3's `STATE`, `PAUSE_AT` and
//! `RESUME` so a ring is read at a real pause. With the gate off every O5 verb
//! is `ERR unknown_cmd`. It's the executable spec the live binary has to
//! match; `live_watch` checks it does.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use oracle_harness::control::Control;
use oracle_harness::verbs::{Refusal, ReplyError, VerbClient, WATCH_HEADER, Watch, WatchReply};
use oracle_harness::watch::{Field, WatchList};

const FLIP: Duration = Duration::from_millis(2);
const WAIT: Duration = Duration::from_secs(10);
/// O5's limits.
const MAX_RANGES: usize = 64;
const MAX_BYTES: u32 = 16_384;
const MAX_FRAMES: u32 = 1 << 20;
const BUDGET: u64 = 16 << 20;
const REPLY: u64 = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Paused,
}

/// `oracle_watch_state`.
#[derive(Default)]
struct Ring {
    last_flip: Option<u64>,
    ranges: Vec<(u32, u32)>,
    bytes: u32,
    frames: u32,
    armed_at: u64,
    total: u64,
    ring: Vec<u8>,
}

struct Emu {
    gate: bool,
    renderer: bool,
    status: Status,
    frames: u64,
    pause_at: u64,
    /// Whether the current flip's reads have happened.
    read_this_flip: bool,
    /// Pad reads a port makes each flip.
    reads_per_flip: [u32; 2],
    /// Guest memory: each region's base and bytes.
    memory: BTreeMap<u32, Vec<u8>>,
    /// Called between a flip's reads, to change memory mid-flip.
    between_reads: Option<fn(&mut Emu)>,
    watch: Ring,
    /// Each read's flip, in order, so a test can see every read happened.
    reads: Vec<u64>,
}

impl Emu {
    fn new() -> Self {
        let mut memory = BTreeMap::new();
        // The "heap": a counter word at 0x30000000 the guest bumps each flip,
        // 4 KiB of pattern after it.
        memory.insert(0x3000_0000, (0..0x1000u32).map(|i| i as u8).collect());
        // A second page elsewhere.
        memory.insert(0x0089_0000, vec![0xAB; 0x1000]);
        Self {
            gate: true,
            renderer: true,
            status: Status::Running,
            frames: 100,
            pause_at: 0,
            read_this_flip: false,
            reads_per_flip: [2, 1],
            memory,
            between_reads: None,
            watch: Ring::default(),
            reads: Vec::new(),
        }
    }

    /// `vm::check_addr(addr, page_readable, len)`.
    fn readable(&self, addr: u32, len: u32) -> bool {
        self.memory
            .range(..=addr)
            .next_back()
            .is_some_and(|(&base, bytes)| {
                u64::from(addr) + u64::from(len) <= u64::from(base) + bytes.len() as u64
            })
    }

    fn bytes(&self, addr: u32, len: u32) -> &[u8] {
        let (&base, bytes) = self.memory.range(..=addr).next_back().unwrap();
        let at = (addr - base) as usize;
        &bytes[at..at + len as usize]
    }

    fn set_word(&mut self, addr: u32, v: u32) {
        let (&base, bytes) = self.memory.range_mut(..=addr).next_back().unwrap();
        let at = (addr - base) as usize;
        bytes[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    /// A savestate's reboot or a fresh boot: `oracle_watch_session` drops it all.
    fn new_session(&mut self) {
        self.frames = 2;
        self.pause_at = 0;
        self.status = Status::Paused;
        self.read_this_flip = false;
        self.watch = Ring::default();
    }

    /// `oracle_watch_read`, the first thing a pad read does.
    fn watch_read(&mut self, flip: u64) {
        self.reads.push(flip);
        if self.watch.last_flip == Some(flip) {
            return;
        }
        self.watch.last_flip = Some(flip);
        if self.watch.frames == 0 {
            return;
        }
        let stride = WATCH_HEADER + self.watch.bytes as usize;
        let slot = (self.watch.total % u64::from(self.watch.frames)) as usize * stride;
        let mut payload = Vec::with_capacity(self.watch.bytes as usize);
        let mut missing = 0u64;
        for (i, &(addr, len)) in self.watch.ranges.iter().enumerate() {
            if self.readable(addr, len) {
                payload.extend_from_slice(self.bytes(addr, len));
            } else {
                payload.extend(std::iter::repeat_n(0, len as usize));
                missing |= 1 << i;
            }
        }
        let rec = &mut self.watch.ring[slot..slot + stride];
        rec[0..8].copy_from_slice(&flip.to_be_bytes());
        rec[8..16].copy_from_slice(&(flip * 16_667).to_be_bytes());
        rec[16..24].copy_from_slice(&missing.to_be_bytes());
        rec[WATCH_HEADER..].copy_from_slice(&payload);
        self.watch.total += 1;
    }

    /// One guest frame: the current flip's reads (unless done), the guest's
    /// update of its counter word, then the flip and the pause check.
    fn frame(&mut self) {
        if !self.read_this_flip {
            let flip = self.frames;
            for port in 0..2 {
                for k in 0..self.reads_per_flip[port] {
                    self.watch_read(flip);
                    if k == 0
                        && port == 0
                        && let Some(f) = self.between_reads
                    {
                        f(self);
                    }
                }
            }
            // The update after the reads: the counter word holds the flip it ran in.
            if self.readable(0x3000_0000, 4) {
                self.set_word(0x3000_0000, flip as u32);
            }
        }
        self.frames += 1;
        self.read_this_flip = false;
        if self.pause_at != 0 && self.frames >= self.pause_at {
            if self.frames == self.pause_at {
                self.status = Status::Paused;
            }
            self.pause_at = 0;
        }
    }

    fn state_line(&self, prefix: &str) -> String {
        let status = match self.status {
            Status::Running => "running",
            Status::Paused => "paused",
        };
        format!(
            "{prefix} status={status} frames={} progr=7/7 seg=0/0\n",
            self.frames
        )
    }

    /// `oracle_watch_arm`.
    fn arm(&mut self, frames: u32, ranges: Vec<(u32, u32)>) -> Result<u32, &'static str> {
        if frames == 0 || frames > MAX_FRAMES {
            return Err("bad_frames");
        }
        if ranges.is_empty() || ranges.len() > MAX_RANGES {
            return Err("bad_args");
        }
        let mut total = 0u64;
        for &(addr, len) in &ranges {
            if len == 0 || len > MAX_BYTES || u64::from(addr) + u64::from(len) > 1 << 32 {
                return Err("bad_range");
            }
            total += u64::from(len);
        }
        if total > u64::from(MAX_BYTES)
            || u64::from(frames) * (WATCH_HEADER as u64 + total) > BUDGET
        {
            return Err("too_big");
        }
        if !self.renderer {
            return Err("no_renderer");
        }
        if ranges.iter().any(|&(a, l)| !self.readable(a, l)) {
            return Err("unmapped");
        }
        let last_flip = self.watch.last_flip;
        self.watch = Ring {
            last_flip,
            bytes: total as u32,
            frames,
            armed_at: self.frames,
            total: 0,
            ring: vec![0; frames as usize * (WATCH_HEADER + total as usize)],
            ranges,
        };
        Ok(total as u32)
    }

    /// `oracle_watch_records`.
    fn records(&self, summary: bool, first: u64, count: u64) -> Result<String, &'static str> {
        let w = &self.watch;
        if w.frames == 0 {
            return Err("no_watch");
        }
        let held = w.total.min(u64::from(w.frames));
        if summary {
            return Ok(format!(
                "OK held={held} total={} bytes={} frames={} armed={}\n",
                w.total, w.bytes, w.frames, w.armed_at
            ));
        }
        let stride = (WATCH_HEADER + w.bytes as usize) as u64;
        if count == 0 {
            return Err("bad_count");
        }
        if first >= held || count > held - first {
            return Err("bad_index");
        }
        if count > REPLY / stride {
            return Err("bad_count");
        }
        let oldest = w.total - held;
        let mut hex = String::new();
        for i in first..first + count {
            let at = ((oldest + i) % u64::from(w.frames)) as usize * stride as usize;
            for b in &w.ring[at..at + stride as usize] {
                hex.push_str(&format!("{b:02x}"));
            }
        }
        Ok(format!("OK first={first} count={count} data={hex}\n"))
    }
}

/// `sky_parse_flip`.
fn parse_flip(arg: &str) -> Option<u64> {
    if arg.is_empty() || arg.len() > 19 || !arg.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    arg.parse().ok()
}

/// `sky_parse_num`: 1 to `digits` characters of `radix`, no greater than `max`.
fn parse_num(s: &str, radix: u32, digits: usize, max: u32) -> Option<u32> {
    if s.is_empty() || s.len() > digits || !s.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u32::from_str_radix(s, radix).ok().filter(|v| *v <= max)
}

/// `sky_oracle_watch_handle`, behind the gate, plus the O3 verbs a read needs.
fn handle(emu: &Arc<Mutex<Emu>>, line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    let mut e = emu.lock().unwrap();
    if cmd == "STATE" {
        return e.state_line("OK");
    }
    if !e.gate {
        return "ERR unknown_cmd\n".into();
    }
    // `sky_words` splits on single spaces, so a doubled one is an empty field.
    let words = || -> Vec<&str> {
        if arg.is_empty() {
            Vec::new()
        } else {
            arg.split(' ').collect()
        }
    };
    match cmd {
        "PAUSE_AT" => match parse_flip(arg) {
            None | Some(0) => "ERR bad_flip\n".into(),
            Some(at) if at <= e.frames => "ERR past\n".into(),
            Some(at) => {
                e.pause_at = at;
                "OK\n".into()
            }
        },
        "RESUME" if e.status == Status::Paused => {
            e.status = Status::Running;
            "OK\n".into()
        }
        "RESUME" => "ERR not_paused\n".into(),
        "WATCH" => {
            let w = words();
            if w.len() < 3 || (w.len() - 1) % 2 != 0 || (w.len() - 1) / 2 > MAX_RANGES {
                return "ERR bad_args\n".into();
            }
            let Some(frames) = parse_num(w[0], 10, 7, u32::MAX) else {
                return "ERR bad_frames\n".into();
            };
            let mut ranges = Vec::new();
            for pair in w[1..].chunks(2) {
                match (
                    parse_num(pair[0], 16, 8, u32::MAX),
                    parse_num(pair[1], 10, 5, u32::MAX),
                ) {
                    (Some(addr), Some(len)) => ranges.push((addr, len)),
                    _ => return "ERR bad_range\n".into(),
                }
            }
            match e.arm(frames, ranges) {
                Ok(bytes) => format!("OK bytes={bytes} frames={frames}\n"),
                Err(err) => format!("ERR {err}\n"),
            }
        }
        "WATCH_READ" => {
            let w = words();
            let (first, count) = match w.as_slice() {
                [] => (0, 0),
                [a, b] => match (parse_flip(a), parse_flip(b)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return "ERR bad_args\n".into(),
                },
                _ => return "ERR bad_args\n".into(),
            };
            if e.status != Status::Paused {
                return "ERR not_paused\n".into();
            }
            match e.records(w.is_empty(), first, count) {
                Ok(line) => line,
                Err(err) => format!("ERR {err}\n"),
            }
        }
        "WATCH_CLEAR" if arg.is_empty() => {
            let last_flip = e.watch.last_flip;
            e.watch = Ring {
                last_flip,
                ..Ring::default()
            };
            "OK\n".into()
        }
        "WATCH_CLEAR" => "ERR bad_args\n".into(),
        _ => "ERR unknown_cmd\n".into(),
    }
}

struct Fake {
    socket: PathBuf,
    emu: Arc<Mutex<Emu>>,
    _config_dir: tempfile::TempDir,
    control: Control,
}

impl Fake {
    fn set<T>(&self, f: impl FnOnce(&mut Emu) -> T) -> T {
        f(&mut self.emu.lock().unwrap())
    }

    fn verbs(&self) -> VerbClient {
        VerbClient::new(&self.socket)
    }

    fn line(&self, request: &str) -> String {
        self.verbs()
            .roundtrip_line(&format!("{request}\n"))
            .unwrap()
    }

    /// Pauses `ahead` flips on and returns where.
    fn pause(&self, ahead: u64) -> u64 {
        let at = self.control.state().unwrap().frames + ahead;
        self.control.pause_at(at, WAIT).unwrap().frames
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn spawn() -> Fake {
    let config_dir = tempfile::tempdir().unwrap();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let socket = std::env::temp_dir().join(format!("o5-{}-{n}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let emu = Arc::new(Mutex::new(Emu::new()));

    let ticker = Arc::clone(&emu);
    thread::spawn(move || {
        loop {
            {
                let mut e = ticker.lock().unwrap();
                if e.status == Status::Running {
                    e.frame();
                }
            }
            if Arc::strong_count(&ticker) == 1 {
                return;
            }
            thread::sleep(FLIP);
        }
    });

    let server = Arc::clone(&emu);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let emu = Arc::clone(&server);
            thread::spawn(move || {
                let mut writer = stream.try_clone().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                    let l = line.trim_end_matches(['\n', '\r', ' ']).to_owned();
                    line.clear();
                    let hb = emu.lock().unwrap().state_line("HB");
                    let reply = handle(&emu, &l);
                    if writer.write_all(format!("{hb}{reply}").as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    let control = Control::new(&socket, config_dir.path());
    Fake {
        socket,
        emu,
        _config_dir: config_dir,
        control,
    }
}

fn refusal(err: anyhow::Error) -> Refusal {
    err.downcast_ref::<ReplyError>()
        .and_then(ReplyError::refusal)
        .unwrap_or_else(|| panic!("not a refusal: {err:#}"))
}

/// The counter word and 12 pattern bytes, and 4 bytes of the second page.
fn list() -> WatchList {
    WatchList::new(vec![
        Field::new("counter", 0x3000_0000, 4),
        Field::new("pattern", 0x3000_0004, 12),
        Field::new("statics", 0x0089_0010, 4),
    ])
    .unwrap()
}

#[test]
fn with_the_gate_off_the_watch_verbs_are_unknown_commands() {
    let fake = spawn();
    fake.set(|e| e.gate = false);
    for verb in [
        list().arm(16),
        Watch::Summary,
        Watch::Read { first: 0, count: 1 },
        Watch::Clear,
    ] {
        let err = fake.verbs().send_watch(&verb).unwrap_err();
        assert_eq!(refusal(err), Refusal::UnknownCmd, "{verb:?}");
    }
    assert!(fake.set(|e| e.watch.frames == 0));
}

#[test]
fn a_run_records_every_flip_once_at_its_first_read_without_a_pause() {
    let fake = spawn();
    // Between a flip's first read and its second, the guest rewrites the
    // pattern's first word: a record holds what the FIRST read saw.
    fake.set(|e| {
        e.between_reads = Some(|e: &mut Emu| {
            let flip = e.frames as u32;
            e.set_word(0x3000_0004, 0xDEAD_0000 | (flip & 0xFFFF));
        })
    });
    let at = fake.pause(5);
    let list = list();
    fake.control.watch_arm(&list, 64).unwrap();
    let end = at + 40;
    fake.control.pause_at(end, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    // Flips at..end - 1, one record each: the pause's own flip read after it,
    // the end flip not yet.
    assert_eq!(ring.summary.held, 40);
    assert_eq!(ring.summary.total, 40);
    assert_eq!(ring.summary.armed, at);
    assert_eq!(ring.summary.bytes as usize, list.bytes());
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, (at..end).collect::<Vec<_>>());
    // Every flip had three reads (two on port 0, one on port 1), one record.
    let reads = fake.set(|e| e.reads.iter().filter(|&&f| f == at + 7).count());
    assert_eq!(reads, 3);
    for r in &ring.records {
        assert_eq!(r.missing, 0);
        // The counter word: the update of the flip before wrote it.
        let counter = u32::from_be_bytes(list.field(r, "counter").unwrap().try_into().unwrap());
        assert_eq!(u64::from(counter), r.flip - 1, "flip {}", r.flip);
        // The first read came before the rewrite: the word is last flip's.
        let word = u32::from_be_bytes(list.field(r, "pattern").unwrap()[..4].try_into().unwrap());
        if r.flip > at {
            assert_eq!(
                word,
                0xDEAD_0000 | ((r.flip - 1) as u32 & 0xFFFF),
                "flip {}",
                r.flip
            );
        }
        assert_eq!(list.field(r, "statics").unwrap(), [0xAB; 4]);
    }
    // Running again, the ring can't be read.
    fake.control.resume().unwrap();
    let err = fake.verbs().send_watch(&Watch::Summary).unwrap_err();
    assert_eq!(refusal(err), Refusal::NotPaused);
}

#[test]
fn the_ring_keeps_the_newest_records_and_says_how_many_it_lost() {
    let fake = spawn();
    let at = fake.pause(5);
    fake.control.watch_arm(&list(), 8).unwrap();
    let end = at + 30;
    fake.control.pause_at(end, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!((ring.summary.held, ring.summary.total), (8, 30));
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, ((end - 8)..end).collect::<Vec<_>>());
}

#[test]
fn a_ring_bigger_than_a_reply_comes_back_whole_in_order() {
    let fake = spawn();
    let at = fake.pause(5);
    // 4,072 bytes a record: 8 records a reply, so 100 take 13 replies.
    let big = WatchList::new(vec![Field::new("page", 0x3000_0000, 4_072)]).unwrap();
    fake.control.watch_arm(&big, 128).unwrap();
    fake.control.pause_at(at + 100, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!(ring.records.len(), 100);
    assert!(ring.records.windows(2).all(|w| w[1].flip == w[0].flip + 1));
    assert!(ring.records.iter().all(|r| r.payload.len() == 4_072));
    // One reply's worth is the most a read takes.
    let over = Watch::Read { first: 0, count: 9 };
    let err = fake.verbs().send_watch(&over).unwrap_err();
    assert_eq!(refusal(err), Refusal::BadCount);
    match fake
        .verbs()
        .send_watch(&Watch::Read {
            first: 92,
            count: 8,
        })
        .unwrap()
    {
        WatchReply::Records { first, records } => {
            assert_eq!(first, 92);
            assert_eq!(records[7].flip, ring.records[99].flip);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_range_unmapped_mid_run_reads_as_zeros_with_its_bit_set() {
    let fake = spawn();
    let at = fake.pause(5);
    let list = list();
    fake.control.watch_arm(&list, 64).unwrap();
    fake.control.pause_at(at + 10, WAIT).unwrap();
    fake.set(|e| e.memory.remove(&0x0089_0000));
    fake.control.pause_at(at + 20, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let statics = list.placed("statics").unwrap().range;
    for r in &ring.records {
        let gone = r.flip >= at + 10;
        assert_eq!(r.missing, u64::from(gone) << statics, "flip {}", r.flip);
        assert_eq!(list.field(r, "statics").is_none(), gone);
        if gone {
            let p = list.placed("statics").unwrap();
            assert_eq!(r.payload[p.offset..p.offset + 4], [0; 4]);
        }
        assert!(list.field(r, "counter").is_some());
    }
    // Armed now, the unmapped range is refused outright.
    let err = fake.control.watch_arm(&list, 8).unwrap_err();
    assert_eq!(refusal(err), Refusal::Unmapped);
}

#[test]
fn every_watch_refusal() {
    let fake = spawn();
    let cases: &[(&str, &str)] = &[
        // Fields: a ring size and pairs of address and length.
        ("WATCH", "bad_args"),
        ("WATCH 16", "bad_args"),
        ("WATCH 16 30000000", "bad_args"),
        ("WATCH 16 30000000 4 89000", "bad_args"),
        ("WATCH 0 30000000 4", "bad_frames"),
        ("WATCH 1048577 30000000 4", "bad_frames"),
        ("WATCH 12345678 30000000 4", "bad_frames"),
        ("WATCH x 30000000 4", "bad_frames"),
        ("WATCH  30000000 4", "bad_frames"),
        ("WATCH 16 0x30000000 4", "bad_range"),
        ("WATCH 16 300000000 4", "bad_range"),
        ("WATCH 16 3000000g 4", "bad_range"),
        ("WATCH 16 30000000 0", "bad_range"),
        ("WATCH 16 30000000 16385", "bad_range"),
        ("WATCH 16 30000000 123456", "bad_range"),
        ("WATCH 16 30000000 -4", "bad_range"),
        ("WATCH 16 fffffffe 4", "bad_range"),
        ("WATCH 16 30000000 16384 30008000 1", "too_big"),
        ("WATCH 1048576 30000000 4", "too_big"),
        ("WATCH 16 40000000 4", "unmapped"),
        ("WATCH 16 30000ffe 4", "unmapped"),
        ("WATCH_READ", "not_paused"),
        ("WATCH_READ 1", "bad_args"),
        ("WATCH_READ 1 2 3", "bad_args"),
        ("WATCH_READ a 1", "bad_args"),
        ("WATCH_CLEAR now", "bad_args"),
    ];
    for (request, want) in cases {
        assert_eq!(fake.line(request), format!("ERR {want}"), "{request}");
    }
    // 64 ranges go in, 65 don't.
    let ranges = |n: u32| -> String {
        (0..n)
            .map(|i| format!(" {:x} 4", 0x3000_0000 + 16 * i))
            .collect()
    };
    assert_eq!(
        fake.line(&format!("WATCH 4{}", ranges(64))),
        "OK bytes=256 frames=4"
    );
    assert_eq!(fake.line(&format!("WATCH 4{}", ranges(65))), "ERR bad_args");
    // The ring's budget, headers included: 16 MiB.
    assert_eq!(
        fake.line("WATCH 65536 30000000 232"),
        "OK bytes=232 frames=65536"
    );
    assert_eq!(fake.line("WATCH 65536 30000000 233"), "ERR too_big");
    // No renderer: refused once everything parses and fits.
    fake.set(|e| e.renderer = false);
    assert_eq!(fake.line("WATCH 16 30000000 4"), "ERR no_renderer");
    fake.set(|e| e.renderer = true);
    // Reading: nothing armed, then indices and counts.
    assert_eq!(fake.line("WATCH_CLEAR"), "OK");
    let at = fake.pause(5);
    assert_eq!(fake.line("WATCH_READ"), "ERR no_watch");
    assert_eq!(fake.line("WATCH_READ 0 1"), "ERR no_watch");
    fake.control.watch_arm(&list(), 16).unwrap();
    assert_eq!(fake.line("WATCH_READ 0 1"), "ERR bad_index");
    assert_eq!(
        fake.line("WATCH_READ"),
        format!("OK held=0 total=0 bytes=20 frames=16 armed={at}")
    );
    fake.control.pause_at(at + 4, WAIT).unwrap();
    assert_eq!(fake.line("WATCH_READ 0 0"), "ERR bad_count");
    assert_eq!(fake.line("WATCH_READ 4 1"), "ERR bad_index");
    assert_eq!(fake.line("WATCH_READ 2 3"), "ERR bad_index");
    assert!(
        fake.line("WATCH_READ 2 2")
            .starts_with("OK first=2 count=2 data=")
    );
    // `sky_parse_flip` takes 19 digits: past that a number isn't one.
    assert_eq!(
        fake.line("WATCH_READ 9999999999999999999 1"),
        "ERR bad_index"
    );
    assert_eq!(
        fake.line(&format!("WATCH_READ {} 1", u64::MAX)),
        "ERR bad_args"
    );
    // Cleared, there's nothing to read.
    fake.control.watch_clear().unwrap();
    assert_eq!(fake.line("WATCH_READ"), "ERR no_watch");
}

#[test]
fn a_new_session_drops_the_watch_and_arming_again_replaces_it() {
    let fake = spawn();
    let at = fake.pause(5);
    fake.control.watch_arm(&list(), 16).unwrap();
    fake.control.pause_at(at + 6, WAIT).unwrap();
    // Armed again: the old ring goes, the new one starts empty.
    let small = WatchList::new(vec![Field::new("counter", 0x3000_0000, 4)]).unwrap();
    fake.control.watch_arm(&small, 4).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!((ring.summary.held, ring.summary.bytes), (0, 4));
    // A savestate's reboot.
    fake.set(Emu::new_session);
    assert_eq!(fake.line("WATCH_READ"), "ERR no_watch");
    // Its flips count from 2 again; armed there, the first record is flip 2.
    fake.control.watch_arm(&small, 4).unwrap();
    fake.control.pause_at(5, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, [2, 3, 4]);
}

#[test]
fn armed_after_a_flips_first_read_the_first_record_is_the_next_flip() {
    let fake = spawn();
    let at = fake.pause(5);
    // The pause's flip read, then armed (as a run that armed mid-flip would be).
    fake.set(|e| {
        e.watch_read(at);
        e.read_this_flip = true;
    });
    let arm = list().arm(16);
    match fake.verbs().send_watch(&arm).unwrap() {
        WatchReply::Armed { bytes, frames } => assert_eq!((bytes, frames), (20, 16)),
        other => panic!("{other:?}"),
    }
    fake.control.pause_at(at + 3, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, [at + 1, at + 2]);
}
