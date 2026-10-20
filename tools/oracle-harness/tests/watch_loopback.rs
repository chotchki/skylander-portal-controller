//! O5's per-frame recorder and O6's chains against an in-process fake of P1's
//! listener (openspec AG.1, AG.35, AH.5). The fake ports
//! `sky_oracle_watch_handle`, `sky_oracle_chain_handle` and the watch and the
//! chains beside `pad_get_data` from `rpcs3-patches/oracle/0005-O5-*.patch`
//! and `0006-O6-*.patch`, and
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
//! and O6 verb is `ERR unknown_cmd`. It's the executable spec the live binary
//! has to match; `live_watch` and `live_chain` check it does.

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
    Chain, ChainBlock, ChainReply, ChainSpec, Hop, Key, Probe, Refusal, ReplyError, VerbClient,
    WATCH_HEADER, WATCH_PORTS, Watch, WatchReply, name_hash,
};
use oracle_harness::watch::{Chains, Field, WatchList};

const FLIP: Duration = Duration::from_millis(2);
const WAIT: Duration = Duration::from_secs(10);
/// O5's limits.
const MAX_RANGES: usize = 64;
const MAX_BYTES: u32 = 16_384;
const MAX_FRAMES: u32 = 1 << 20;
const BUDGET: u64 = 128 << 20;
const REPLY: u64 = 262_144;
/// O6's limits.
const CHAIN_SLOTS: usize = 8;
const CHAIN_MAX_ITEMS: u32 = 512;
const CHAIN_MAX_RANGES: usize = 32;
const CHAIN_MAX_HOPS: usize = 16;
const CHAIN_MAX_VALUES: usize = 64;
const CHAIN_MAX_RANGE: u32 = 1024;
const CHAIN_MAX_LIST: u32 = 4096;
const CHAIN_MAX_SCAN: u32 = 1024;
const CHAIN_MAX_NAME: u32 = 256;
const CHAIN_HEADER: u32 = 20;
const CHAIN_ITEM_HEADER: u32 = 16;
const CHAIN_BUDGET: u64 = 49_152;
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
    /// O6: a WATCH records the ones armed, in slot order.
    chains: [Option<FakeChain>; CHAIN_SLOTS],
}

/// `oracle_chain_hop`.
#[derive(Debug, Clone, Default)]
struct FakeHop {
    search: bool,
    off: u32,
    check: Option<u32>,
    count: u32,
    array: u32,
    name: u32,
    hash: u32,
}

/// `oracle_chain_probe`.
#[derive(Debug, Clone, Default)]
struct FakeProbe {
    hops: Vec<FakeHop>,
    off: u32,
    hashed: bool,
    len: u32,
}

/// `oracle_chain`.
#[derive(Debug, Clone, Default)]
struct FakeChain {
    items: u32,
    base: u32,
    path: Vec<FakeHop>,
    count: u32,
    array: u32,
    key: FakeProbe,
    values: Vec<u32>,
    ranges: Vec<FakeProbe>,
    item_bytes: u32,
    bytes: u32,
}

/// `oracle_chain_counts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    status: u32,
    listed: u32,
    unread: u32,
    matched: u32,
    kept: u32,
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
    /// Record bytes a `WATCH_READ` reply carries: O6's, or an O5 build's 32 KiB.
    reply: u64,
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
            reply: REPLY,
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
        // O6: each armed chain's block, in slot order, after the ranges.
        for c in w.chains.iter().flatten() {
            payload.extend(self.chain_block(c).0);
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
        if total > u64::from(MAX_BYTES) {
            return Err("too_big");
        }
        total += self
            .watch
            .chains
            .iter()
            .flatten()
            .map(|c| u64::from(c.bytes))
            .sum::<u64>();
        if u64::from(frames) * (WATCH_HEADER as u64 + total) > BUDGET {
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
        let chains = std::mem::take(&mut self.watch.chains);
        self.watch = Ring {
            chains,
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

    /// `oracle_chain_word`: the big-endian word at `addr`, 4-aligned and
    /// readable.
    fn word(&self, addr: u64) -> Option<u32> {
        if !addr.is_multiple_of(4) || addr + 4 > 1 << 32 || !self.readable(addr as u32, 4) {
            return None;
        }
        Some(u32::from_be_bytes(
            self.bytes(addr as u32, 4).try_into().unwrap(),
        ))
    }

    /// `oracle_chain_name_hash`: FNV-1a over the NUL-terminated string at `at`.
    fn name_hash(&self, at: u32) -> Option<u32> {
        let mut h: u32 = 0x811c_9dc5;
        for i in 0..CHAIN_MAX_NAME {
            let a = u64::from(at) + u64::from(i);
            if a >= 1 << 32 || !self.readable(a as u32, 1) {
                return None;
            }
            let c = self.bytes(a as u32, 1)[0];
            if c == 0 {
                return Some(h);
            }
            h = (h ^ u32::from(c)).wrapping_mul(0x0100_0193);
        }
        None
    }

    /// `oracle_chain_follow`: 0 once every hop held, else the 1-based hop
    /// that didn't.
    fn follow(&self, cur: &mut u32, hops: &[FakeHop]) -> usize {
        for (i, h) in hops.iter().enumerate() {
            if !h.search {
                let Some(next) = self.word(u64::from(*cur) + u64::from(h.off)) else {
                    return i + 1;
                };
                if next == 0
                    || h.check
                        .is_some_and(|w| self.word(u64::from(next)) != Some(w))
                {
                    return i + 1;
                }
                *cur = next;
                continue;
            }
            let (Some(n), Some(arr)) = (
                self.word(u64::from(*cur) + u64::from(h.count)),
                self.word(u64::from(*cur) + u64::from(h.array)),
            ) else {
                return i + 1;
            };
            if n > CHAIN_MAX_SCAN {
                return i + 1;
            }
            let found = (0..n).find_map(|k| {
                let e = self
                    .word(u64::from(arr) + 4 * u64::from(k))
                    .filter(|&e| e != 0)?;
                let name = self
                    .word(u64::from(e) + u64::from(h.name))
                    .filter(|&n| n != 0)?;
                (self.name_hash(name)? == h.hash).then_some(e)
            });
            match found {
                Some(e) => *cur = e,
                None => return i + 1,
            }
        }
        0
    }

    /// `oracle_chain_key`.
    fn key(&self, item: u32, p: &FakeProbe) -> Option<u32> {
        let mut cur = item;
        if self.follow(&mut cur, &p.hops) != 0 {
            return None;
        }
        let w = self.word(u64::from(cur) + u64::from(p.off))?;
        if !p.hashed {
            return Some(w);
        }
        if w == 0 {
            return None;
        }
        self.name_hash(w)
    }

    /// `oracle_chain_block`: the block and its counts.
    fn chain_block(&self, c: &FakeChain) -> (Vec<u8>, Counts) {
        let mut out = vec![0u8; c.bytes as usize];
        let mut n = Counts::default();
        let mut list = c.base;
        let hops = c.path.len() as u32;
        let broke = self.follow(&mut list, &c.path);
        let count = self.word(u64::from(list) + u64::from(c.count));
        let arr = self.word(u64::from(list) + u64::from(c.array));
        if broke != 0 {
            n.status = broke as u32;
        } else if count.is_none() {
            n.status = hops + 1;
        } else if arr.is_none() {
            n.status = hops + 2;
        } else if count.unwrap() > CHAIN_MAX_LIST {
            n.status = hops + 3;
        }
        if n.status == 0 {
            let (count, arr) = (count.unwrap(), arr.unwrap());
            n.listed = count;
            let mut item = CHAIN_HEADER as usize;
            for k in 0..count {
                let key = self
                    .word(u64::from(arr) + 4 * u64::from(k))
                    .filter(|&e| e != 0)
                    .and_then(|e| Some((e, self.key(e, &c.key)?)));
                let Some((e, key)) = key else {
                    n.unread += 1;
                    continue;
                };
                let Some(v) = c.values.iter().position(|&v| v == key) else {
                    continue;
                };
                n.matched += 1;
                if n.kept == c.items {
                    continue;
                }
                let mut missing = 0u32;
                let mut at = item + CHAIN_ITEM_HEADER as usize;
                for (r, p) in c.ranges.iter().enumerate() {
                    let mut cur = e;
                    let ok = self.follow(&mut cur, &p.hops) == 0
                        && u64::from(cur) + u64::from(p.off) + u64::from(p.len) <= 1 << 32
                        && self.readable(cur + p.off, p.len);
                    if ok {
                        out[at..at + p.len as usize]
                            .copy_from_slice(self.bytes(cur + p.off, p.len));
                    } else {
                        missing |= 1 << r;
                    }
                    at += p.len as usize;
                }
                for (i, v) in [e, k, v as u32, missing].into_iter().enumerate() {
                    out[item + 4 * i..item + 4 * i + 4].copy_from_slice(&v.to_be_bytes());
                }
                item += c.item_bytes as usize;
                n.kept += 1;
            }
        }
        for (i, v) in [n.status, n.listed, n.unread, n.matched, n.kept]
            .into_iter()
            .enumerate()
        {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        (out, n)
    }

    /// `oracle_chain_arm`, from its argument string.
    fn chain_arm(&mut self, arg: &str) -> Result<String, &'static str> {
        let w: Vec<&str> = if arg.is_empty() {
            Vec::new()
        } else {
            arg.split(' ').collect()
        };
        if w.len() < 8 || w.len() > 8 + CHAIN_MAX_RANGES {
            return Err("bad_args");
        }
        let slot = parse_num(w[0], 10, 1, CHAIN_SLOTS as u32 - 1).ok_or("bad_slot")?;
        let items = parse_num(w[1], 10, 3, CHAIN_MAX_ITEMS)
            .filter(|&i| i != 0)
            .ok_or("bad_items")?;
        let base = parse_num(w[2], 16, 8, u32::MAX);
        let path = parse_path(w[3]);
        let count = parse_num(w[4], 16, 4, 0xffff);
        let array = parse_num(w[5], 16, 4, 0xffff);
        let key = parse_key(w[6]);
        let (Some(base), Some(path), Some(count), Some(array), Some(key)) =
            (base, path, count, array, key)
        else {
            return Err("bad_hop");
        };
        let values: Vec<&str> = w[7].split(',').collect();
        if values.len() > CHAIN_MAX_VALUES {
            return Err("bad_values");
        }
        let values = values
            .iter()
            .map(|v| parse_num(v, 16, 8, u32::MAX))
            .collect::<Option<Vec<u32>>>()
            .ok_or("bad_values")?;
        let ranges = w[8..]
            .iter()
            .map(|r| parse_range(r))
            .collect::<Option<Vec<FakeProbe>>>()
            .ok_or("bad_range")?;
        let item_bytes = CHAIN_ITEM_HEADER + ranges.iter().map(|r| r.len).sum::<u32>();
        let bytes = u64::from(CHAIN_HEADER) + u64::from(items) * u64::from(item_bytes);
        if bytes > CHAIN_BUDGET {
            return Err("too_big");
        }
        if self.watch.frames != 0 {
            return Err("watching");
        }
        let others: u64 = self
            .watch
            .chains
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != slot as usize)
            .filter_map(|(_, c)| c.as_ref())
            .map(|c| u64::from(c.bytes))
            .sum();
        if others + bytes > CHAIN_BUDGET {
            return Err("too_big");
        }
        let chain = FakeChain {
            items,
            base,
            path,
            count,
            array,
            key,
            values,
            ranges,
            item_bytes,
            bytes: bytes as u32,
        };
        let (_, n) = self.chain_block(&chain);
        if n.status != 0 {
            return Err("no_list");
        }
        self.watch.chains[slot as usize] = Some(chain);
        Ok(format!(
            "OK slot={slot} bytes={bytes} listed={} kept={}\n",
            n.listed, n.kept
        ))
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
        if count > self.reply / stride {
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

/// `oracle_chain_parse_hop`: `<off>`, `<off>=<word>` or
/// `?<count>,<array>,<name>=<hash>`.
fn parse_hop(s: &str) -> Option<FakeHop> {
    let eq: Vec<&str> = s.split('=').collect();
    if eq.len() > 2 {
        return None;
    }
    if let Some(rest) = eq[0].strip_prefix('?') {
        let f: Vec<&str> = rest.split(',').collect();
        if eq.len() != 2 || f.len() != 3 {
            return None;
        }
        return Some(FakeHop {
            search: true,
            count: parse_num(f[0], 16, 4, 0xffff)?,
            array: parse_num(f[1], 16, 4, 0xffff)?,
            name: parse_num(f[2], 16, 4, 0xffff)?,
            hash: parse_num(eq[1], 16, 8, u32::MAX)?,
            ..FakeHop::default()
        });
    }
    Some(FakeHop {
        off: parse_num(eq[0], 16, 4, 0xffff)?,
        check: match eq.get(1) {
            Some(w) => Some(parse_num(w, 16, 8, u32::MAX)?),
            None => None,
        },
        ..FakeHop::default()
    })
}

fn parse_hops(parts: &[&str]) -> Option<Vec<FakeHop>> {
    if parts.len() > CHAIN_MAX_HOPS {
        return None;
    }
    parts.iter().map(|p| parse_hop(p)).collect()
}

/// `oracle_chain_parse_path`: `-`, or hops joined by `>`.
fn parse_path(s: &str) -> Option<Vec<FakeHop>> {
    if s == "-" {
        return Some(Vec::new());
    }
    parse_hops(&s.split('>').collect::<Vec<_>>())
}

/// `oracle_chain_parse_key`: hops, then `<off>` or `#<off>`.
fn parse_key(s: &str) -> Option<FakeProbe> {
    let parts: Vec<&str> = s.split('>').collect();
    let (term, hops) = parts.split_last().unwrap();
    let (hashed, term) = match term.strip_prefix('#') {
        Some(t) => (true, t),
        None => (false, *term),
    };
    Some(FakeProbe {
        hops: parse_hops(hops)?,
        off: parse_num(term, 16, 4, 0xffff)?,
        hashed,
        len: 0,
    })
}

/// `oracle_chain_parse_range`: hops, then `<off>+<len>`.
fn parse_range(s: &str) -> Option<FakeProbe> {
    let parts: Vec<&str> = s.split('>').collect();
    let (term, hops) = parts.split_last().unwrap();
    let t: Vec<&str> = term.split('+').collect();
    if t.len() != 2 {
        return None;
    }
    let len = parse_num(t[1], 10, 4, CHAIN_MAX_RANGE).filter(|&l| l != 0)?;
    Some(FakeProbe {
        hops: parse_hops(hops)?,
        off: parse_num(t[0], 16, 4, 0xffff)?,
        hashed: false,
        len,
    })
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
            let chains = std::mem::take(&mut e.watch.chains);
            e.watch = Ring {
                ports,
                chains,
                ..Ring::default()
            };
            "OK\n".into()
        }
        "WATCH_CLEAR" => "ERR bad_args\n".into(),
        "CHAIN" => match e.chain_arm(arg) {
            Ok(line) => line,
            Err(err) => format!("ERR {err}\n"),
        },
        "CHAIN_CLEAR" if !arg.is_empty() => "ERR bad_args\n".into(),
        "CHAIN_CLEAR" if e.watch.frames != 0 => "ERR watching\n".into(),
        "CHAIN_CLEAR" => {
            e.watch.chains = Default::default();
            "OK\n".into()
        }
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
    fake.set(world);
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
    // O6's too, a chain that would arm with the gate on among them.
    for verb in [
        Chain::Arm {
            slot: 0,
            spec: spec(8, &["chompy(1)"]),
        },
        Chain::Clear,
    ] {
        let err = fake.verbs().send_chain(&verb).unwrap_err();
        assert_eq!(refusal(err), Refusal::UnknownCmd, "{verb:?}");
    }
    assert!(fake.set(|e| e.watch.frames == 0 && e.watch.chains.iter().all(Option::is_none)));
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
    // A 16 KiB page of its own for the big range.
    fake.set(|e| e.memory.insert(0x3100_0000, vec![0x5A; 0x4000]));
    let at = fake.pause(5);
    // 16,352 bytes and the header a record: 16 records a 256 KiB reply, so
    // 100 take 7 replies.
    let big = WatchList::new(vec![Field::new("page", 0x3100_0000, 16_352)]).unwrap();
    fake.control.watch_arm(&big, 0, 128).unwrap();
    fake.control.pause_at(at + 100, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!(ring.records.len(), 100);
    assert!(ring.records.windows(2).all(|w| w[1].flip == w[0].flip + 1));
    assert!(ring.records.iter().all(|r| r.payload.len() == 16_352));
    // One reply's worth is the most a read takes.
    let over = Watch::Read {
        first: 0,
        count: 17,
    };
    let err = fake.verbs().send_watch(&over).unwrap_err();
    assert_eq!(refusal(err), Refusal::BadCount);
    match fake
        .verbs()
        .send_watch(&Watch::Read {
            first: 84,
            count: 16,
        })
        .unwrap()
    {
        WatchReply::Records { first, records } => {
            assert_eq!(first, 84);
            assert_eq!(records[15].flip, ring.records[99].flip);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_build_before_o6_has_its_ring_read_in_its_own_pages() {
    let fake = spawn();
    // O5b's 32 KiB a reply: 8 records of 4,064 bytes, so 100 take 13 pages
    // once the first 256 KiB page is refused.
    fake.set(|e| e.reply = 32_768);
    let at = fake.pause(5);
    let big = WatchList::new(vec![Field::new("page", 0x3000_0000, 4_064)]).unwrap();
    fake.control.watch_arm(&big, 0, 128).unwrap();
    fake.control.pause_at(at + 100, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    assert_eq!(ring.records.len(), 100);
    assert!(ring.records.windows(2).all(|w| w[1].flip == w[0].flip + 1));
    let over = Watch::Read { first: 0, count: 9 };
    let err = fake.verbs().send_watch(&over).unwrap_err();
    assert_eq!(refusal(err), Refusal::BadCount);
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
        ("WATCH 1048576 0 30000000 97", "too_big"),
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
    // The ring's budget, headers included: 128 MiB since O6 (16 before).
    assert_eq!(
        fake.line("WATCH 1048576 0 30000000 96"),
        "OK bytes=96 frames=1048576 port=0"
    );
    assert_eq!(fake.line("WATCH 1048576 0 30000000 97"), "ERR too_big");
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

// --- O6: chains (openspec AH.5) ------------------------------------------------

/// The static whose word points at the world's root.
const STATIC: u32 = 0x0090_0010;
const ROOT: u32 = 0x3200_0000;
const LIST: u32 = ROOT + 0x100;
const ARRAY: u32 = ROOT + 0x200;
const ROOT_VT: u32 = 0x00AA_0001;
const LIST_VT: u32 = 0x00AA_0002;
const ACTOR_VT: u32 = 0x00BB_0001;
const INFO_VT: u32 = 0x00CC_0001;

/// Each actor `k` in regions of its own, so a test can free one: the actor
/// at `0x3300_0000 + k * 0x1000` (vtable, its info at +0x08, its position at
/// +0x10, a serial at +0x1C, a pointer to the frame-count word at +0x20), its
/// info at `0x3400_0000 + ...` (vtable, its name at +0x04, its variables'
/// list at +0x10), its name and its variables' names at `0x3500_0000 + ...`
/// and its variables' list at `0x3600_0000 + ...` (count +0x08, the array at
/// +0x14 pointing at +0x40, each cell at +0x80 + 0x30 j: its name at +0x08,
/// its value at +0x20).
fn actor_at(k: u32) -> u32 {
    0x3300_0000 + k * 0x1000
}
fn info_at(k: u32) -> u32 {
    0x3400_0000 + k * 0x1000
}
fn name_at(k: u32) -> u32 {
    0x3500_0000 + k * 0x1000
}
fn vars_at(k: u32) -> u32 {
    0x3600_0000 + k * 0x1000
}

fn put(region: &mut [u8], at: usize, word: u32) {
    region[at..at + 4].copy_from_slice(&word.to_be_bytes());
}

/// Actor `k`, named `name`, at `x`, with its serial and variables.
fn place(e: &mut Emu, k: u32, name: &str, x: f32, serial: u32, vars: &[(&str, u32)]) {
    let mut a = vec![0u8; 0x100];
    put(&mut a, 0, ACTOR_VT);
    put(&mut a, 0x08, info_at(k));
    for (i, v) in [x, 22.5, -60.0].into_iter().enumerate() {
        put(&mut a, 0x10 + 4 * i, v.to_bits());
    }
    put(&mut a, 0x1C, serial);
    put(&mut a, 0x20, FRAME_WORD);
    e.memory.insert(actor_at(k), a);
    let mut info = vec![0u8; 0x100];
    put(&mut info, 0, INFO_VT);
    put(&mut info, 0x04, name_at(k));
    put(&mut info, 0x10, vars_at(k));
    e.memory.insert(info_at(k), info);
    let mut names = vec![0u8; 0x400];
    names[..name.len()].copy_from_slice(name.as_bytes());
    let mut v = vec![0u8; 0x400];
    put(&mut v, 0x08, vars.len() as u32);
    put(&mut v, 0x14, vars_at(k) + 0x40);
    for (j, (var, value)) in vars.iter().enumerate() {
        let cell = 0x80 + 0x30 * j;
        put(&mut v, 0x40 + 4 * j, vars_at(k) + cell as u32);
        let n = 0x40 + 0x20 * j;
        names[n..n + var.len()].copy_from_slice(var.as_bytes());
        put(&mut v, cell + 0x08, name_at(k) + n as u32);
        put(&mut v, cell + 0x20, *value);
    }
    e.memory.insert(name_at(k), names);
    e.memory.insert(vars_at(k), v);
}

/// The list holding actors `ks`, in that order.
fn listing(e: &mut Emu, ks: &[u32]) {
    let (&base, root) = e.memory.range_mut(..=ROOT).next_back().unwrap();
    assert_eq!(base, ROOT);
    let list = (LIST - ROOT) as usize;
    put(root, list + 0x08, ks.len() as u32);
    for (i, &k) in ks.iter().enumerate() {
        put(root, (ARRAY - ROOT) as usize + 4 * i, actor_at(k));
    }
}

/// The static, the root and its list, and four actors listed: two Chompies,
/// an orb and a rock nobody asks for.
fn world(e: &mut Emu) {
    let mut st = vec![0u8; 0x100];
    put(&mut st, 0x10, ROOT);
    e.memory.insert(STATIC - 0x10, st);
    let mut root = vec![0u8; 0x1000];
    put(&mut root, 0, ROOT_VT);
    put(&mut root, 0x10, LIST);
    put(&mut root, 0x100, LIST_VT);
    put(&mut root, 0x100 + 0x14, ARRAY);
    e.memory.insert(ROOT, root);
    place(e, 0, "chompy(1)", 1.0, 70, &[("alerted", 0), ("hp", 1)]);
    place(e, 1, "rock", 2.0, 71, &[("hp", 9)]);
    place(e, 2, "chompy(2)", 3.0, 72, &[("hp", 1), ("alerted", 1)]);
    place(e, 3, "orb", 4.0, 73, &[("worth", 4)]);
    listing(e, &[0, 1, 2, 3]);
}

/// A chain over the world's list keeping `names` (up to `items` a record):
/// each one's position and serial off the item, the frame count through one
/// hop, and its `hp` through two hops and a search by name.
fn spec(items: u32, names: &[&str]) -> ChainSpec {
    ChainSpec {
        items,
        base: STATIC,
        path: vec![Hop::checked(0, ROOT_VT), Hop::checked(0x10, LIST_VT)],
        count: 0x08,
        array: 0x14,
        key: Key {
            hops: vec![Hop::checked(0x08, INFO_VT)],
            off: 0x04,
            hashed: true,
        },
        values: names.iter().map(|n| name_hash(n)).collect(),
        ranges: vec![
            Probe::new(vec![], 0x10, 12),
            Probe::new(vec![], 0x1C, 4),
            Probe::new(vec![Hop::at(0x20)], 0, 4),
            Probe::new(
                vec![
                    Hop::at(0x08),
                    Hop::at(0x10),
                    Hop::named(0x08, 0x14, 0x08, "hp"),
                ],
                0x20,
                4,
            ),
        ],
    }
}

/// A block's listed and matched counts and its items' index, x and serial.
type Shape = (u32, u32, Vec<(u32, f32, u32)>);
/// A block's unread and matched counts and its items' index and missing mask.
type Kept = (u32, u32, Vec<(u32, u32)>);

/// A kept item's x, serial, frame count and `hp` (`None` where missing).
fn item_fields(item: &oracle_harness::verbs::ChainEntry) -> (f32, u32, u32, Option<u32>) {
    let word = |b: &[u8]| u32::from_be_bytes(b[..4].try_into().unwrap());
    (
        f32::from_bits(word(item.range(0).unwrap())),
        word(item.range(1).unwrap()),
        word(item.range(2).unwrap()),
        item.range(3).map(word),
    )
}

/// The ring's records split: each record's chain block in `slot`.
fn blocks(fake: &Fake, list: &WatchList, chains: &Chains, slot: u8) -> Vec<(u64, u64, ChainBlock)> {
    let ring = fake.control.watch_read().unwrap();
    ring.records
        .iter()
        .map(|r| {
            let (o5, mut b) = chains.split(list, r).unwrap();
            assert_eq!(o5.payload.len(), list.bytes());
            (r.flip, r.read, b.remove(&slot).unwrap())
        })
        .collect()
}

fn armed(chains: &[(u8, ChainSpec)]) -> Chains {
    Chains {
        slots: chains.iter().cloned().collect(),
    }
}

#[test]
fn a_chain_rides_every_record_as_its_list_grows_and_shrinks() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    let chains = armed(&[(2, spec(8, &["chompy(1)", "chompy(2)", "orb"]))]);
    let got = fake.control.chains_arm(&chains).unwrap();
    assert_eq!(
        got.iter()
            .map(|a| (a.slot, a.listed, a.kept))
            .collect::<Vec<_>>(),
        [(2, 4, 3)]
    );
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 64)
        .unwrap();
    // A record's payload is the list's ranges and the chain's block together.
    match fake.verbs().send_watch(&Watch::Summary).unwrap() {
        WatchReply::Summary(s) => {
            assert_eq!(s.bytes as usize, list.bytes() + chains.bytes());
            assert_eq!(chains.bytes(), 20 + 8 * (16 + 24));
        }
        other => panic!("{other:?}"),
    }
    // Five flips as they are; then an orb spawns at the end of the list and
    // the rock leaves it; five more; then the first Chompy goes and the orb's
    // slot is taken by another orb (a new serial) at the front.
    fake.control.pause_at(at + 5, WAIT).unwrap();
    fake.set(|e| {
        place(e, 4, "orb", 5.0, 74, &[("worth", 1)]);
        listing(e, &[0, 2, 3, 4]);
    });
    fake.control.pause_at(at + 10, WAIT).unwrap();
    fake.set(|e| {
        place(e, 3, "orb", 6.0, 75, &[("worth", 1)]);
        listing(e, &[3, 2, 4]);
    });
    fake.control.pause_at(at + 15, WAIT).unwrap();
    let b = blocks(&fake, &list, &chains, 2);
    assert_eq!(b.len(), 15);
    let shape: Vec<Shape> = b
        .iter()
        .map(|(_, _, blk)| {
            (
                blk.listed,
                blk.matched,
                blk.items
                    .iter()
                    .map(|i| {
                        let (x, serial, _, _) = item_fields(i);
                        (i.index, x, serial)
                    })
                    .collect(),
            )
        })
        .collect();
    let first = (4, 3, vec![(0, 1.0, 70), (2, 3.0, 72), (3, 4.0, 73)]);
    let second = (
        4,
        4,
        vec![(0, 1.0, 70), (1, 3.0, 72), (2, 4.0, 73), (3, 5.0, 74)],
    );
    let third = (3, 3, vec![(0, 6.0, 75), (1, 3.0, 72), (2, 5.0, 74)]);
    assert_eq!(shape[..5], vec![first; 5][..]);
    assert_eq!(shape[5..10], vec![second; 5][..]);
    assert_eq!(shape[10..], vec![third; 5][..]);
    // Every item's address, value, frame count and hp: the frame count steps
    // with every record, and the orbs have no hp.
    let names = ["chompy(1)", "chompy(2)", "orb"];
    // Serial to slot: the second orb (75) took the first's slot, 3.
    let slot = |serial: u32| if serial == 75 { 3 } else { serial - 70 };
    for (k, (_, _, blk)) in b.iter().enumerate() {
        assert_eq!((blk.status, blk.unread, blk.dropped()), (0, 0, 0));
        for item in &blk.items {
            let (_, serial, frame, hp) = item_fields(item);
            let kind = names[item.value as usize];
            assert_eq!(item.address, actor_at(slot(serial)), "{kind}");
            assert_eq!(frame, item_fields(&b[0].2.items[0]).2 + k as u32);
            assert_eq!(hp, kind.starts_with("chompy").then_some(1), "{kind}");
            assert_eq!(item.missing, if hp.is_some() { 0 } else { 1 << 3 });
        }
    }
}

#[test]
fn the_filter_keeps_what_matches_up_to_its_items_and_counts_the_rest() {
    let fake = spawn();
    fake.set(world);
    // An item whose info is null: its key can't be read.
    fake.set(|e| {
        place(e, 5, "chompy(3)", 7.0, 76, &[]);
        let (_, a) = e.memory.range_mut(..=actor_at(5)).next_back().unwrap();
        put(a, 0x08, 0);
        listing(e, &[0, 1, 5, 2, 3]);
    });
    let at = fake.pause(5);
    // Two items a record for three that match: the first two kept, in list
    // order, the third counted and dropped.
    let chains = armed(&[(0, spec(2, &["orb", "chompy(2)", "chompy(1)", "chompy(3)"]))]);
    let got = fake.control.chains_arm(&chains).unwrap();
    assert_eq!((got[0].listed, got[0].kept), (5, 2));
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 16)
        .unwrap();
    fake.control.pause_at(at + 2, WAIT).unwrap();
    for (_, _, blk) in blocks(&fake, &list, &chains, 0) {
        assert_eq!(
            (
                blk.status,
                blk.listed,
                blk.unread,
                blk.matched,
                blk.dropped()
            ),
            (0, 5, 1, 3, 1)
        );
        let kept: Vec<(u32, u32)> = blk.items.iter().map(|i| (i.index, i.value)).collect();
        // chompy(1) is value 2, chompy(2) value 1; the orb (value 0) dropped.
        assert_eq!(kept, [(0, 2), (3, 1)]);
    }
    // A chain whose path breaks at its second hop now: refused.
    fake.control.watch_clear().unwrap();
    let mut broken = spec(2, &["orb"]);
    broken.path[1] = Hop::checked(0x10, LIST_VT + 1);
    let err = fake.control.chain_arm(1, &broken).unwrap_err();
    assert_eq!(refusal(err), Refusal::NoList);
}

#[test]
fn a_range_through_a_broken_second_link_reads_as_missing() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    let chains = armed(&[(0, spec(8, &["chompy(1)", "chompy(2)"]))]);
    fake.control.chains_arm(&chains).unwrap();
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 32)
        .unwrap();
    fake.control.pause_at(at + 3, WAIT).unwrap();
    // chompy(2)'s info now points its variables' list at memory that isn't
    // mapped: the hp range's second link breaks; its key (one hop) still reads.
    fake.set(|e| {
        let (_, info) = e.memory.range_mut(..=info_at(2)).next_back().unwrap();
        put(info, 0x10, 0x4F00_0000);
    });
    fake.control.pause_at(at + 6, WAIT).unwrap();
    // Then its hp cell renamed: the search finds no `hp` and the range is
    // missing again, the rest of the item whole.
    fake.set(|e| {
        let (_, info) = e.memory.range_mut(..=info_at(2)).next_back().unwrap();
        put(info, 0x10, vars_at(2));
        let (_, names) = e.memory.range_mut(..=name_at(2)).next_back().unwrap();
        names[0x40..0x42].copy_from_slice(b"HP");
    });
    fake.control.pause_at(at + 9, WAIT).unwrap();
    let b = blocks(&fake, &list, &chains, 0);
    assert_eq!(b.len(), 9);
    for (k, (_, _, blk)) in b.iter().enumerate() {
        assert_eq!(blk.items.len(), 2, "record {k}");
        let (one, two) = (&blk.items[0], &blk.items[1]);
        assert_eq!((one.missing, item_fields(one).3), (0, Some(1)));
        let broken = k >= 3;
        assert_eq!(two.missing, u32::from(broken) << 3, "record {k}");
        assert_eq!(item_fields(two).3, (!broken).then_some(1));
        assert_eq!(
            two.ranges[3],
            if broken {
                vec![0; 4]
            } else {
                1u32.to_be_bytes().to_vec()
            }
        );
        assert_eq!(item_fields(two).0, 3.0);
    }
}

#[test]
fn an_item_freed_mid_chain_is_dropped_or_reads_as_missing() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    let chains = armed(&[(0, spec(8, &["chompy(1)", "chompy(2)", "orb"]))]);
    fake.control.chains_arm(&chains).unwrap();
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 32)
        .unwrap();
    fake.control.pause_at(at + 2, WAIT).unwrap();
    // The orb's info freed while the orb stays listed: its key can't be read,
    // so it's counted unread and not kept.
    fake.set(|e| e.memory.remove(&info_at(3)));
    fake.control.pause_at(at + 4, WAIT).unwrap();
    // chompy(1) itself freed under its list entry.
    fake.set(|e| e.memory.remove(&actor_at(0)));
    fake.control.pause_at(at + 6, WAIT).unwrap();
    // chompy(2)'s variables freed: kept (its key reads), its hp missing.
    fake.set(|e| e.memory.remove(&vars_at(2)));
    fake.control.pause_at(at + 8, WAIT).unwrap();
    let b = blocks(&fake, &list, &chains, 0);
    let got: Vec<Kept> = b
        .iter()
        .map(|(_, _, blk)| {
            (
                blk.unread,
                blk.matched,
                blk.items.iter().map(|i| (i.index, i.missing)).collect(),
            )
        })
        .collect();
    let want = [
        (0, 3, vec![(0, 0), (2, 0), (3, 1 << 3)]),
        (1, 2, vec![(0, 0), (2, 0)]),
        (2, 1, vec![(2, 0)]),
        (2, 1, vec![(2, 1 << 3)]),
    ];
    for (k, w) in want.iter().enumerate() {
        assert_eq!(got[2 * k], *w, "records {} and {}", 2 * k, 2 * k + 1);
        assert_eq!(got[2 * k + 1], *w);
    }
}

/// The world's root region, for a test that bends the list under a chain.
fn root(e: &mut Emu) -> &mut Vec<u8> {
    e.memory.get_mut(&ROOT).unwrap()
}

#[test]
fn a_list_broken_mid_run_names_its_link_and_4096_is_the_most_listed() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    let chains = armed(&[(0, spec(8, &["chompy(1)", "chompy(2)"]))]);
    fake.control.chains_arm(&chains).unwrap();
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 64)
        .unwrap();
    // Two records each: whole; the list's vtable off (the path's second hop);
    // the root pointing at a list whose count word isn't mapped, then at one
    // whose array pointer isn't (the path's two hops, then 1 and 2); the list
    // counting 4,097 (3); counting 4,096, its entries past the four zeros or
    // unmapped.
    let (short, shorter) = (0x3F00_0000u32, 0x3F10_0000u32);
    fake.control.pause_at(at + 2, WAIT).unwrap();
    fake.set(|e| put(root(e), 0x100, LIST_VT + 1));
    fake.control.pause_at(at + 4, WAIT).unwrap();
    fake.set(|e| {
        put(root(e), 0x100, LIST_VT);
        let mut s = vec![0u8; 8];
        put(&mut s, 0, LIST_VT);
        e.memory.insert(short, s);
        put(root(e), 0x10, short);
    });
    fake.control.pause_at(at + 6, WAIT).unwrap();
    fake.set(|e| {
        let mut s = vec![0u8; 0x10];
        put(&mut s, 0, LIST_VT);
        put(&mut s, 0x08, 4);
        e.memory.insert(shorter, s);
        put(root(e), 0x10, shorter);
    });
    fake.control.pause_at(at + 8, WAIT).unwrap();
    fake.set(|e| {
        put(root(e), 0x10, LIST);
        put(root(e), 0x108, 4_097);
    });
    fake.control.pause_at(at + 10, WAIT).unwrap();
    fake.set(|e| put(root(e), 0x108, 4_096));
    fake.control.pause_at(at + 12, WAIT).unwrap();
    let got: Vec<[u32; 5]> = blocks(&fake, &list, &chains, 0)
        .iter()
        .map(|(_, _, b)| {
            [
                b.status,
                b.listed,
                b.unread,
                b.matched,
                b.items.len() as u32,
            ]
        })
        .collect();
    let want = [
        [0, 4, 0, 2, 2],
        [2, 0, 0, 0, 0],
        [3, 0, 0, 0, 0],
        [4, 0, 0, 0, 0],
        [5, 0, 0, 0, 0],
        [0, 4_096, 4_092, 2, 2],
    ];
    assert_eq!(got.len(), 2 * want.len());
    for (k, w) in want.iter().enumerate() {
        assert_eq!(
            (got[2 * k], got[2 * k + 1]),
            (*w, *w),
            "records {}-{}",
            2 * k,
            2 * k + 1
        );
    }
}

#[test]
fn a_search_past_1024_elements_and_a_name_past_256_bytes_dont_read() {
    let fake = spawn();
    fake.set(world);
    // A name of 255 bytes and its NUL keys; one of 256 runs past O6's 256
    // bytes before its NUL and is counted unread.
    let (n255, n256) = ("n".repeat(255), "m".repeat(256));
    fake.set(|e| {
        place(e, 4, &n255, 5.0, 74, &[]);
        place(e, 5, &n256, 6.0, 75, &[]);
        listing(e, &[0, 1, 2, 3, 4, 5]);
    });
    let at = fake.pause(5);
    let chains = armed(&[(0, spec(8, &["chompy(1)", &n255, &n256]))]);
    let got = fake.control.chains_arm(&chains).unwrap();
    assert_eq!((got[0].listed, got[0].kept), (6, 2));
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 32)
        .unwrap();
    // chompy(1)'s variables counting 1,024 (its `hp` second among them)
    // still find it; counting 1,025 the search gives up and its hp is missing.
    fake.control.pause_at(at + 2, WAIT).unwrap();
    fake.set(|e| put(e.memory.get_mut(&vars_at(0)).unwrap(), 0x08, 1_024));
    fake.control.pause_at(at + 4, WAIT).unwrap();
    fake.set(|e| put(e.memory.get_mut(&vars_at(0)).unwrap(), 0x08, 1_025));
    fake.control.pause_at(at + 6, WAIT).unwrap();
    let got: Vec<Kept> = blocks(&fake, &list, &chains, 0)
        .iter()
        .map(|(_, _, b)| {
            (
                b.unread,
                b.matched,
                b.items.iter().map(|i| (i.index, i.missing)).collect(),
            )
        })
        .collect();
    let found = (1, 2, vec![(0, 0), (4, 1 << 3)]);
    let gave_up = (1, 2, vec![(0, 1 << 3), (4, 1 << 3)]);
    assert_eq!(got.len(), 6);
    assert_eq!(got[..4], vec![found; 4][..]);
    assert_eq!(got[4..], vec![gave_up; 2][..]);
}

#[test]
fn two_chains_ride_in_slot_order_and_arming_again_drops_a_slot_left_out() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    // Two chains of different sizes: the Chompies in slot 4, the rock's
    // position alone in slot 1.
    let mut rock = spec(2, &["rock"]);
    rock.ranges.truncate(1);
    let both = armed(&[(4, spec(8, &["chompy(1)", "chompy(2)"])), (1, rock.clone())]);
    fake.control.chains_arm(&both).unwrap();
    let list = list();
    fake.control.watch_arm_chained(&list, &both, 0, 16).unwrap();
    fake.control.pause_at(at + 2, WAIT).unwrap();
    let xs = |slot: u8| -> Vec<Vec<f32>> {
        blocks(&fake, &list, &both, slot)
            .iter()
            .map(|(_, _, b)| {
                b.items
                    .iter()
                    .map(|i| {
                        f32::from_bits(u32::from_be_bytes(
                            i.range(0).unwrap()[..4].try_into().unwrap(),
                        ))
                    })
                    .collect()
            })
            .collect()
    };
    assert_eq!(xs(1), vec![vec![2.0]; 2]);
    assert_eq!(xs(4), vec![vec![1.0, 3.0]; 2]);
    // Armed again as slot 4 alone: slot 1 is dropped, so the watch's records
    // are the list and slot 4's block, as the harness expects.
    fake.control.watch_clear().unwrap();
    let one = armed(&[(4, spec(8, &["chompy(1)", "chompy(2)"]))]);
    fake.control.chains_arm(&one).unwrap();
    fake.control.watch_arm_chained(&list, &one, 0, 16).unwrap();
    match fake.verbs().send_watch(&Watch::Summary).unwrap() {
        WatchReply::Summary(s) => assert_eq!(s.bytes as usize, list.bytes() + one.bytes()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_flip_running_two_game_frames_carries_a_block_for_each() {
    let fake = spawn();
    fake.set(world);
    let at = fake.pause(5);
    fake.set(|e| {
        e.frames_at.insert(at + 4, 0);
        e.frames_at.insert(at + 5, 2);
    });
    let chains = armed(&[(0, spec(8, &["chompy(1)"]))]);
    fake.control.chains_arm(&chains).unwrap();
    let list = list();
    fake.control
        .watch_arm_chained(&list, &chains, 0, 32)
        .unwrap();
    fake.control.pause_at(at + 10, WAIT).unwrap();
    let ring = fake.control.watch_read().unwrap();
    let b = blocks(&fake, &list, &chains, 0);
    assert_eq!(b.len(), 10);
    // The record's own frame count and the chain's (read through its item's
    // pointer) agree in every record, the doubled flip's reads 0 and 1 too.
    for (r, (flip, read, blk)) in ring.records.iter().zip(&b) {
        let (o5, _) = chains.split(&list, r).unwrap();
        let counter = u32::from_be_bytes(list.field(&o5, "counter").unwrap().try_into().unwrap());
        assert_eq!(
            item_fields(&blk.items[0]).2,
            counter,
            "flip {flip} read {read}"
        );
    }
    let keys: Vec<(u64, u64)> = b.iter().map(|(f, r, _)| (*f, *r)).collect();
    assert!(keys.contains(&(at + 5, 1)));
    assert!(!keys.iter().any(|&(f, _)| f == at + 4));
}

#[test]
fn the_chain_budget_at_and_one_past_its_limit() {
    let fake = spawn();
    fake.set(world);
    let one = |items: u32, len: u16| {
        let mut s = spec(items, &["chompy(1)"]);
        s.ranges = vec![Probe::new(vec![], 0, len)];
        s
    };
    // 20 + items x (16 + len): 71 items of 692 bytes is exactly 49,152.
    let full = one(71, 692 - CHAIN_ITEM_HEADER as u16);
    assert_eq!(full.bytes() as u64, CHAIN_BUDGET);
    let over = one(71, 693 - CHAIN_ITEM_HEADER as u16);
    assert_eq!(over.bytes() as u64, CHAIN_BUDGET + 71);
    let line = |slot: u8, s: &ChainSpec| {
        fake.line(
            Chain::Arm {
                slot,
                spec: s.clone(),
            }
            .encode()
            .trim_end(),
        )
    };
    assert_eq!(line(0, &full), "OK slot=0 bytes=49152 listed=4 kept=1");
    assert_eq!(line(1, &over), "ERR too_big");
    // Every chain's blocks together: a second chain of 20 bytes doesn't fit
    // beside a full one, and replacing the full one's slot frees its share.
    let small = one(1, 4);
    let small = ChainSpec { items: 1, ..small };
    assert_eq!(small.bytes(), 40);
    assert_eq!(line(1, &small), "ERR too_big");
    assert_eq!(line(0, &small), "OK slot=0 bytes=40 listed=4 kept=1");
    let mut rest = one(1, 4);
    rest.items = 1;
    rest.ranges = vec![Probe::new(vec![], 0, 1024); 47];
    // 32 ranges at most a chain.
    assert_eq!(line(1, &rest), "ERR bad_args");
    rest.ranges.truncate(32);
    rest.items = 1;
    assert_eq!(
        rest.bytes() as u32,
        CHAIN_HEADER + CHAIN_ITEM_HEADER + 32 * 1024
    );
    assert_eq!(
        line(1, &rest),
        format!("OK slot=1 bytes={} listed=4 kept=1", rest.bytes())
    );
    // The ring's budget counts the chains: records of 32 + 4 + 40 + 32,804
    // bytes, 4,082 of them in 128 MiB and not 4,083.
    let stride = WATCH_HEADER + 4 + 40 + rest.bytes();
    assert_eq!(stride, 32_880);
    assert_eq!(BUDGET / stride as u64, 4_082);
    assert_eq!(fake.line("WATCH 4083 0 30000000 4"), "ERR too_big");
    assert_eq!(
        fake.line("WATCH 4082 0 30000000 4"),
        format!("OK bytes={} frames=4082 port=0", stride - WATCH_HEADER)
    );
}

#[test]
fn every_chain_refusal() {
    let fake = spawn();
    fake.set(world);
    let ok = "CHAIN 0 8 900010 0=aa0001>10=aa0002 8 14 8=cc0001>#4 1,2";
    let cases: &[(&str, &str)] = &[
        ("CHAIN", "bad_args"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4", "bad_args"),
        ("CHAIN 8 8 900010 0>10 8 14 8>#4 1", "bad_slot"),
        ("CHAIN 00 8 900010 0>10 8 14 8>#4 1", "bad_slot"),
        ("CHAIN x 8 900010 0>10 8 14 8>#4 1", "bad_slot"),
        ("CHAIN 0 0 900010 0>10 8 14 8>#4 1", "bad_items"),
        ("CHAIN 0 513 900010 0>10 8 14 8>#4 1", "bad_items"),
        ("CHAIN 0 1000 900010 0>10 8 14 8>#4 1", "bad_items"),
        ("CHAIN 0 8 1900010ab 0>10 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 0>>10 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010  8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 0=aa0001=1 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 10000 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 ?8,14=1 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 ?8,14,8 8 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 0>10 10000 14 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 0>10 8 x 8>#4 1", "bad_hop"),
        ("CHAIN 0 8 900010 0>10 8 14 8># 1", "bad_hop"),
        ("CHAIN 0 8 900010 0>10 8 14 8>##4 1", "bad_hop"),
        (
            "CHAIN 0 8 900010 0>1>2>3>4>5>6>7>8>9>a>b>c>d>e>f>10 8 14 4 1",
            "bad_hop",
        ),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 ,1", "bad_values"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1,", "bad_values"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 123456789", "bad_values"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 g", "bad_values"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1 10", "bad_range"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1 10+0", "bad_range"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1 10+1025", "bad_range"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1 10+4+4", "bad_range"),
        ("CHAIN 0 8 900010 0>10 8 14 8>#4 1 x>10+4", "bad_range"),
        ("CHAIN 0 512 900010 0>10 8 14 8>#4 1 10+1024", "too_big"),
        ("CHAIN 0 8 900010 0>10=aa0003 8 14 8>#4 1", "no_list"),
        ("CHAIN 0 8 900014 0>10 8 14 8>#4 1", "no_list"),
        ("CHAIN 0 8 900010 0>10 9 14 8>#4 1", "no_list"),
        ("CHAIN_CLEAR now", "bad_args"),
    ];
    for (request, want) in cases {
        assert_eq!(fake.line(request), format!("ERR {want}"), "{request}");
    }
    // 64 values and 32 ranges go in; 65 and 33 don't.
    let values = |n: u32| {
        (1..=n)
            .map(|v| format!("{v:x}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    assert!(
        fake.line(&format!("CHAIN 0 1 900010 0>10 8 14 8>#4 {}", values(64)))
            .starts_with("OK slot=0 bytes=36 ")
    );
    assert_eq!(
        fake.line(&format!("CHAIN 0 1 900010 0>10 8 14 8>#4 {}", values(65))),
        "ERR bad_values"
    );
    let ranges = |n: usize| " 10+4".repeat(n);
    assert!(
        fake.line(&format!("{ok}{}", ranges(32)))
            .starts_with("OK slot=0 bytes=")
    );
    assert_eq!(fake.line(&format!("{ok}{}", ranges(33))), "ERR bad_args");
    // A watch armed: neither verb, until it's cleared; WATCH_CLEAR keeps the
    // chains.
    // Its block: 20 bytes and 8 items of 16, none kept (values 1 and 2
    // match no name).
    assert_eq!(fake.line(ok), "OK slot=0 bytes=148 listed=4 kept=0");
    assert_eq!(
        fake.line("WATCH 16 0 30000000 4"),
        "OK bytes=152 frames=16 port=0"
    );
    assert_eq!(fake.line(ok), "ERR watching");
    assert_eq!(fake.line("CHAIN_CLEAR"), "ERR watching");
    assert_eq!(fake.line("WATCH_CLEAR"), "OK");
    assert_eq!(
        fake.line("WATCH 16 0 30000000 4"),
        "OK bytes=152 frames=16 port=0"
    );
    assert_eq!(fake.line("WATCH_CLEAR"), "OK");
    assert_eq!(fake.line("CHAIN_CLEAR"), "OK");
    assert_eq!(
        fake.line("WATCH 16 0 30000000 4"),
        "OK bytes=4 frames=16 port=0"
    );
    // A new session drops the chains with the watch.
    assert_eq!(fake.line("WATCH_CLEAR"), "OK");
    assert!(fake.line(ok).starts_with("OK"));
    fake.set(Emu::new_session);
    assert_eq!(
        fake.line("WATCH 16 0 30000000 4"),
        "OK bytes=4 frames=16 port=0"
    );
    // And the client's own reply checks: a reply naming another slot, or a
    // block size the spec doesn't make, isn't the verb's success.
    let verb = Chain::Arm {
        slot: 3,
        spec: spec(8, &["orb"]),
    };
    assert!(matches!(
        oracle_harness::verbs::parse_chain_reply(
            &verb,
            &format!(
                "OK slot=3 bytes={} listed=4 kept=1",
                spec(8, &["orb"]).bytes()
            )
        ),
        Ok(ChainReply::Armed {
            slot: 3,
            listed: 4,
            kept: 1,
            ..
        })
    ));
}
