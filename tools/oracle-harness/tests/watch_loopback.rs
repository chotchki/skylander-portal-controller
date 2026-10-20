//! O5's per-frame recorder against an in-process fake of P1's listener
//! (openspec AG.1, AG.35). The fake ports `sky_oracle_watch_handle` and the
//! watch beside `pad_get_data` from `rpcs3-patches/oracle/0005-O5-*.patch`, and
//! models the guest around them: guest memory as mapped regions, a flip
//! counter, and game frames run against the flips, one a flip unless a test
//! says a flip runs none or two (a long frame and the catch-up after it, AG.1's
//! walk-W-03). Each game frame reads port 0 and then port 1 (once each unless a
//! test says more), then updates a frame-count word, and every read of the
//! watched port copies the watched ranges into the ring. As on the Mac (AF.7),
//! the reads of the flip a pause lands on come after the pause, once the run
//! resumes. A test can change memory between port 0's first read and the rest,
//! unmap a region, or start a new session. It keeps O3's `STATE`, `PAUSE_AT`
//! and `RESUME` so a ring is read at a real pause. With the gate off every O5
//! verb is `ERR unknown_cmd`. It's the executable spec the live binary has to
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
use oracle_harness::verbs::{
    Refusal, ReplyError, VerbClient, WATCH_HEADER, WATCH_PORTS, Watch, WatchReply,
};
use oracle_harness::watch::{Field, WatchList};

const FLIP: Duration = Duration::from_millis(2);
const WAIT: Duration = Duration::from_secs(10);
/// O5's limits.
const MAX_RANGES: usize = 64;
const MAX_BYTES: u32 = 16_384;
const MAX_FRAMES: u32 = 1 << 20;
const BUDGET: u64 = 16 << 20;
const REPLY: u64 = 32_768;
/// The frame-count word the guest's update writes, and where it starts.
const FRAME_WORD: u32 = 0x3000_0000;
const FIRST_FRAME: u32 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Paused,
}

/// `oracle_watch_port`: a port's latest read's flip and its reads in it.
#[derive(Debug, Clone, Copy, Default)]
struct PortReads {
    flip: Option<u64>,
    reads: u64,
}

/// `oracle_watch_state`.
#[derive(Default)]
struct Ring {
    ports: [PortReads; WATCH_PORTS as usize],
    port: u8,
    from_flip: u64,
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
    /// Whether the current flip's game frames have run.
    ran_this_flip: bool,
    /// Game frames a flip runs where it isn't one.
    frames_at: BTreeMap<u64, u32>,
    /// Reads each game frame makes of port 0 and port 1.
    reads_per_frame: [u32; 2],
    /// The game's frame count: the update after a frame's reads bumps it.
    game_frame: u32,
    /// Guest memory: each region's base and bytes.
    memory: BTreeMap<u32, Vec<u8>>,
    /// Called right after a game frame's first read of port 0.
    between_reads: Option<fn(&mut Emu)>,
    watch: Ring,
    /// Each read's port and flip, in order, so a test can see every read happened.
    reads: Vec<(u8, u64)>,
}

impl Emu {
    fn new() -> Self {
        let mut memory = BTreeMap::new();
        // The "heap": the frame-count word at 0x30000000, 4 KiB of pattern after it.
        let mut heap: Vec<u8> = (0..0x1000u32).map(|i| i as u8).collect();
        heap[..4].copy_from_slice(&FIRST_FRAME.to_be_bytes());
        memory.insert(FRAME_WORD, heap);
        // A second page elsewhere.
        memory.insert(0x0089_0000, vec![0xAB; 0x1000]);
        Self {
            gate: true,
            renderer: true,
            status: Status::Running,
            frames: 100,
            pause_at: 0,
            ran_this_flip: false,
            frames_at: BTreeMap::new(),
            reads_per_frame: [1, 1],
            game_frame: FIRST_FRAME,
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
        self.ran_this_flip = false;
        self.watch = Ring::default();
    }

    /// `oracle_watch_read`, the first thing a pad read does.
    fn watch_read(&mut self, port: u8, flip: u64) {
        self.reads.push((port, flip));
        let p = &mut self.watch.ports[port as usize];
        if p.flip != Some(flip) {
            *p = PortReads {
                flip: Some(flip),
                reads: 0,
            };
        }
        let read = p.reads;
        p.reads += 1;
        let w = &self.watch;
        if w.frames == 0 || port != w.port || flip < w.from_flip {
            return;
        }
        let stride = WATCH_HEADER + w.bytes as usize;
        let slot = (w.total % u64::from(w.frames)) as usize * stride;
        let mut payload = Vec::with_capacity(w.bytes as usize);
        let mut missing = 0u64;
        for (i, &(addr, len)) in w.ranges.iter().enumerate() {
            if self.readable(addr, len) {
                payload.extend_from_slice(self.bytes(addr, len));
            } else {
                payload.extend(std::iter::repeat_n(0, len as usize));
                missing |= 1 << i;
            }
        }
        let w = &mut self.watch;
        let rec = &mut w.ring[slot..slot + stride];
        rec[0..8].copy_from_slice(&flip.to_be_bytes());
        rec[8..16].copy_from_slice(&(flip * 16_667 + read).to_be_bytes());
        rec[16..24].copy_from_slice(&missing.to_be_bytes());
        rec[24..32].copy_from_slice(&read.to_be_bytes());
        rec[WATCH_HEADER..].copy_from_slice(&payload);
        w.total += 1;
    }

    /// One flip: its game frames (unless they've run), each reading port 0
    /// then port 1 and then updating the frame-count word, then the flip and
    /// the pause check.
    fn frame(&mut self) {
        if !self.ran_this_flip {
            let flip = self.frames;
            for _ in 0..self.frames_at.get(&flip).copied().unwrap_or(1) {
                for port in 0..2u8 {
                    for k in 0..self.reads_per_frame[port as usize] {
                        self.watch_read(port, flip);
                        if k == 0
                            && port == 0
                            && let Some(f) = self.between_reads
                        {
                            f(self);
                        }
                    }
                }
                // The update after the reads: the next frame's count.
                self.game_frame += 1;
                if self.readable(FRAME_WORD, 4) {
                    self.set_word(FRAME_WORD, self.game_frame);
                }
            }
        }
        self.frames += 1;
        self.ran_this_flip = false;
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
    fn arm(&mut self, frames: u32, port: u8, ranges: Vec<(u32, u32)>) -> Result<u32, &'static str> {
        if frames == 0 || frames > MAX_FRAMES {
            return Err("bad_frames");
        }
        if port >= WATCH_PORTS {
            return Err("bad_port");
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
        let now = self.frames;
        let ports = self.watch.ports;
        self.watch = Ring {
            ports,
            port,
            from_flip: if ports[port as usize].flip == Some(now) {
                now + 1
            } else {
                now
            },
            bytes: total as u32,
            frames,
            armed_at: now,
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
                "OK held={held} total={} bytes={} frames={} armed={} port={}\n",
                w.total, w.bytes, w.frames, w.armed_at, w.port
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
            if w.len() < 4 || (w.len() - 2) % 2 != 0 || (w.len() - 2) / 2 > MAX_RANGES {
                return "ERR bad_args\n".into();
            }
            let Some(frames) = parse_num(w[0], 10, 7, u32::MAX) else {
                return "ERR bad_frames\n".into();
            };
            let Some(port) = parse_num(w[1], 10, 1, 6) else {
                return "ERR bad_port\n".into();
            };
            let mut ranges = Vec::new();
            for pair in w[2..].chunks(2) {
                match (
                    parse_num(pair[0], 16, 8, u32::MAX),
                    parse_num(pair[1], 10, 5, u32::MAX),
                ) {
                    (Some(addr), Some(len)) => ranges.push((addr, len)),
                    _ => return "ERR bad_range\n".into(),
                }
            }
            match e.arm(frames, port as u8, ranges) {
                Ok(bytes) => format!("OK bytes={bytes} frames={frames} port={port}\n"),
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
            let ports = e.watch.ports;
            e.watch = Ring {
                ports,
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

/// A record's frame-count word and the first word of its pattern.
fn words(list: &WatchList, r: &oracle_harness::verbs::WatchRecord) -> (u32, u32) {
    let word =
        |name: &str| u32::from_be_bytes(list.field(r, name).unwrap()[..4].try_into().unwrap());
    (word("counter"), word("pattern"))
}

#[test]
fn with_the_gate_off_the_watch_verbs_are_unknown_commands() {
    let fake = spawn();
    fake.set(|e| e.gate = false);
    for verb in [
        list().arm(16, 0),
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
fn a_run_records_every_read_of_its_port_and_no_other_without_a_pause() {
    let fake = spawn();
    // Right after each game frame's read of port 0, before port 1's, the guest
    // rewrites the pattern's first word with the flip.
    fake.set(|e| {
        e.between_reads = Some(|e: &mut Emu| {
            let flip = e.frames as u32;
            e.set_word(0x3000_0004, 0xDEAD_0000 | (flip & 0xFFFF));
        })
    });
    let at = fake.pause(5);
    let list = list();
    fake.control.watch_arm(&list, 0, 64).unwrap();
    let end = at + 40;
    fake.control.pause_at(end, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    // Flips at..end - 1, one game frame each, so one record each: the pause's
    // own flip read after it, the end flip not yet.
    assert_eq!(ring.summary.held, 40);
    assert_eq!(ring.summary.total, 40);
    assert_eq!(ring.summary.armed, at);
    assert_eq!(ring.summary.port, 0);
    assert_eq!(ring.summary.bytes as usize, list.bytes());
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, (at..end).collect::<Vec<_>>());
    assert_eq!(ring.flips(), flips);
    // Every flip read port 0 and port 1; only port 0's read took a record.
    let reads = fake.set(|e| e.reads.iter().filter(|&&(_, f)| f == at + 7).count());
    assert_eq!(reads, 2);
    let first = words(&list, &ring.records[0]).0;
    for (k, r) in ring.records.iter().enumerate() {
        assert_eq!((r.missing, r.read), (0, 0));
        let (counter, word) = words(&list, r);
        // The frame count: the update after each read bumps it by one.
        assert_eq!(counter, first + k as u32, "flip {}", r.flip);
        // Port 0's read came before the rewrite: the word is last flip's.
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
    // Armed on port 1, a record holds what port 1's read saw: this flip's word.
    fake.control.watch_arm(&list, 1, 64).unwrap();
    let at = end;
    fake.control.pause_at(at + 10, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!((ring.summary.held, ring.summary.port), (10, 1));
    for r in &ring.records {
        assert_eq!(
            words(&list, r).1,
            0xDEAD_0000 | (r.flip as u32 & 0xFFFF),
            "flip {}",
            r.flip
        );
    }
    // Running again, the ring can't be read.
    fake.control.resume().unwrap();
    let err = fake.verbs().send_watch(&Watch::Summary).unwrap_err();
    assert_eq!(refusal(err), Refusal::NotPaused);
}

#[test]
fn a_flip_running_two_game_frames_holds_a_record_for_each() {
    let fake = spawn();
    let at = fake.pause(5);
    // A long frame: flip at + 10 runs no game frame and flip at + 11 runs two,
    // as AG.1's walk-W-03 did at flips 1578 and 1579.
    fake.set(|e| {
        e.frames_at.insert(at + 10, 0);
        e.frames_at.insert(at + 11, 2);
    });
    let list = list();
    fake.control.watch_arm(&list, 0, 64).unwrap();
    let end = at + 40;
    fake.control.pause_at(end, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    // 40 flips ran 40 game frames: 40 records, flip at + 11 holding two.
    assert_eq!((ring.summary.held, ring.summary.total), (40, 40));
    let keys: Vec<(u64, u64)> = ring.records.iter().map(|r| (r.flip, r.read)).collect();
    let mut want: Vec<(u64, u64)> = (at..end)
        .filter(|&f| f != at + 10)
        .map(|f| (f, 0))
        .collect();
    want.insert(11, (at + 11, 1));
    assert_eq!(keys, want);
    assert_eq!(ring.flips().len(), 39);
    // No game frame lost: the frame count steps by one from record to record,
    // across the flip without one and through the flip with two.
    let counters: Vec<u32> = ring.records.iter().map(|r| words(&list, r).0).collect();
    assert!(
        counters.windows(2).all(|w| w[1] == w[0] + 1),
        "{counters:?}"
    );
    // One record a flip, at its first read, as O5 took them before AG.35,
    // would have held 39 and skipped the second frame of flip at + 11.
    let firsts: Vec<u32> = ring
        .records
        .iter()
        .filter(|r| r.read == 0)
        .map(|r| words(&list, r).0)
        .collect();
    assert_eq!(firsts.len(), 39);
    assert_eq!(firsts.windows(2).filter(|w| w[1] != w[0] + 1).count(), 1);
}

#[test]
fn a_port_read_twice_a_frame_is_recorded_twice() {
    let fake = spawn();
    fake.set(|e| e.reads_per_frame = [2, 1]);
    let at = fake.pause(5);
    let list = list();
    fake.control.watch_arm(&list, 0, 64).unwrap();
    fake.control.pause_at(at + 5, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    // Every read of the port takes a record: two a flip, reads 0 and 1, one
    // game frame apart in neither (the harness's check counts it a repeat).
    assert_eq!(ring.summary.held, 10);
    for pair in ring.records.chunks(2) {
        assert_eq!(
            (pair[0].flip, pair[0].read, pair[1].read),
            (pair[1].flip, 0, 1)
        );
        assert_eq!(words(&list, &pair[0]).0, words(&list, &pair[1]).0);
    }
}

#[test]
fn the_ring_keeps_the_newest_records_and_says_how_many_it_lost() {
    let fake = spawn();
    let at = fake.pause(5);
    fake.control.watch_arm(&list(), 0, 8).unwrap();
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
    // 4,064 bytes and the header a record: 8 records a reply, so 100 take 13
    // replies.
    let big = WatchList::new(vec![Field::new("page", 0x3000_0000, 4_064)]).unwrap();
    fake.control.watch_arm(&big, 0, 128).unwrap();
    fake.control.pause_at(at + 100, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!(ring.records.len(), 100);
    assert!(ring.records.windows(2).all(|w| w[1].flip == w[0].flip + 1));
    assert!(ring.records.iter().all(|r| r.payload.len() == 4_064));
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
    fake.control.watch_arm(&list, 0, 64).unwrap();
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
    let err = fake.control.watch_arm(&list, 0, 8).unwrap_err();
    assert_eq!(refusal(err), Refusal::Unmapped);
}

#[test]
fn every_watch_refusal() {
    let fake = spawn();
    let cases: &[(&str, &str)] = &[
        // Fields: a ring size, a port and pairs of address and length.
        ("WATCH", "bad_args"),
        ("WATCH 16", "bad_args"),
        ("WATCH 16 0", "bad_args"),
        ("WATCH 16 0 30000000", "bad_args"),
        ("WATCH 16 0 30000000 4 89000", "bad_args"),
        // AG.1's shape, without a port, is refused whole, never misread.
        ("WATCH 16 30000000 4", "bad_args"),
        ("WATCH 16 30000000 4 89000 4", "bad_args"),
        ("WATCH 0 0 30000000 4", "bad_frames"),
        ("WATCH 1048577 0 30000000 4", "bad_frames"),
        ("WATCH 12345678 0 30000000 4", "bad_frames"),
        ("WATCH x 0 30000000 4", "bad_frames"),
        ("WATCH  0 30000000 4", "bad_frames"),
        ("WATCH 16 7 30000000 4", "bad_port"),
        ("WATCH 16 00 30000000 4", "bad_port"),
        ("WATCH 16 x 30000000 4", "bad_port"),
        ("WATCH 16 -1 30000000 4", "bad_port"),
        ("WATCH 16  30000000 4", "bad_port"),
        ("WATCH 16 0 0x30000000 4", "bad_range"),
        ("WATCH 16 0 300000000 4", "bad_range"),
        ("WATCH 16 0 3000000g 4", "bad_range"),
        ("WATCH 16 0 30000000 0", "bad_range"),
        ("WATCH 16 0 30000000 16385", "bad_range"),
        ("WATCH 16 0 30000000 123456", "bad_range"),
        ("WATCH 16 0 30000000 -4", "bad_range"),
        ("WATCH 16 0 fffffffe 4", "bad_range"),
        ("WATCH 16 0 30000000 16384 30008000 1", "too_big"),
        ("WATCH 1048576 0 30000000 4", "too_big"),
        ("WATCH 16 0 40000000 4", "unmapped"),
        ("WATCH 16 0 30000ffe 4", "unmapped"),
        ("WATCH_READ", "not_paused"),
        ("WATCH_READ 1", "bad_args"),
        ("WATCH_READ 1 2 3", "bad_args"),
        ("WATCH_READ a 1", "bad_args"),
        ("WATCH_CLEAR now", "bad_args"),
    ];
    for (request, want) in cases {
        assert_eq!(fake.line(request), format!("ERR {want}"), "{request}");
    }
    // 64 ranges go in, 65 don't; every port 0-6 does.
    let ranges = |n: u32| -> String {
        (0..n)
            .map(|i| format!(" {:x} 4", 0x3000_0000 + 16 * i))
            .collect()
    };
    assert_eq!(
        fake.line(&format!("WATCH 4 0{}", ranges(64))),
        "OK bytes=256 frames=4 port=0"
    );
    assert_eq!(
        fake.line(&format!("WATCH 4 0{}", ranges(65))),
        "ERR bad_args"
    );
    assert_eq!(
        fake.line("WATCH 4 6 30000000 4"),
        "OK bytes=4 frames=4 port=6"
    );
    // The ring's budget, headers included: 16 MiB.
    assert_eq!(
        fake.line("WATCH 65536 0 30000000 224"),
        "OK bytes=224 frames=65536 port=0"
    );
    assert_eq!(fake.line("WATCH 65536 0 30000000 225"), "ERR too_big");
    // No renderer: refused once everything parses and fits.
    fake.set(|e| e.renderer = false);
    assert_eq!(fake.line("WATCH 16 0 30000000 4"), "ERR no_renderer");
    fake.set(|e| e.renderer = true);
    // Reading: nothing armed, then indices and counts.
    assert_eq!(fake.line("WATCH_CLEAR"), "OK");
    let at = fake.pause(5);
    assert_eq!(fake.line("WATCH_READ"), "ERR no_watch");
    assert_eq!(fake.line("WATCH_READ 0 1"), "ERR no_watch");
    fake.control.watch_arm(&list(), 0, 16).unwrap();
    assert_eq!(fake.line("WATCH_READ 0 1"), "ERR bad_index");
    assert_eq!(
        fake.line("WATCH_READ"),
        format!("OK held=0 total=0 bytes=20 frames=16 armed={at} port=0")
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
    fake.control.watch_arm(&list(), 0, 16).unwrap();
    fake.control.pause_at(at + 6, WAIT).unwrap();
    // Armed again: the old ring goes, the new one starts empty.
    let small = WatchList::new(vec![Field::new("counter", 0x3000_0000, 4)]).unwrap();
    fake.control.watch_arm(&small, 0, 4).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!((ring.summary.held, ring.summary.bytes), (0, 4));
    // A savestate's reboot.
    fake.set(Emu::new_session);
    assert_eq!(fake.line("WATCH_READ"), "ERR no_watch");
    // Its flips count from 2 again; armed there, the first record is flip 2.
    fake.control.watch_arm(&small, 0, 4).unwrap();
    fake.control.pause_at(5, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, [2, 3, 4]);
}

#[test]
fn armed_after_its_ports_first_read_in_a_flip_the_first_record_is_the_next_flip() {
    let fake = spawn();
    let at = fake.pause(5);
    // Port 0 read once in the pause's flip, then armed (as a run that armed
    // mid-flip would be): that flip's reads of it aren't whole any more, so its
    // later read there (the game frame the flip runs after the pause) isn't
    // recorded, and recording starts with the next flip.
    fake.set(|e| e.watch_read(0, at));
    let arm = list().arm(16, 0);
    match fake.verbs().send_watch(&arm).unwrap() {
        WatchReply::Armed {
            bytes,
            frames,
            port,
        } => assert_eq!((bytes, frames, port), (20, 16, 0)),
        other => panic!("{other:?}"),
    }
    fake.control.pause_at(at + 3, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    assert_eq!(flips, [at + 1, at + 2]);

    // Port 0 read and port 1 not yet: a watch on port 1 starts in that flip,
    // whose reads of port 1 are all still to come.
    let fake = spawn();
    let at = fake.pause(5);
    fake.set(|e| e.watch_read(0, at));
    fake.control.watch_arm(&list(), 1, 16).unwrap();
    fake.control.pause_at(at + 3, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let keys: Vec<(u64, u64)> = ring.records.iter().map(|r| (r.flip, r.read)).collect();
    assert_eq!(keys, [(at, 0), (at + 1, 0), (at + 2, 0)]);
}
