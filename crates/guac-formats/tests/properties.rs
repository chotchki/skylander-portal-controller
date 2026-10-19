//! The stable property test every parser in `registry::PARSERS` runs under
//! (openspec AB, design decision 3): random bytes and mutated well-formed files,
//! asserting no panic and a peak heap within the parser's budget. This is the
//! fuzzing `cargo test` and CI get; the cargo-fuzz targets in `fuzz/` go deeper
//! on nightly.
//!
//! The heap is measured by a thread-local tracking allocator, which is why this
//! lives in an integration test: the library itself forbids `unsafe`.
//! `PROPTEST_CASES` raises the case count for a longer local run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::panic::{self, AssertUnwindSafe};

use guac_formats::FormatError;
use guac_formats::registry::{PARSERS, RegisteredParser};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

struct Tracking;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn note(delta: isize) {
    // try_with: the allocator also runs during thread teardown, after the
    // thread-locals are gone.
    let _ = LIVE.try_with(|live| {
        let now = live.get() + delta;
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}

// SAFETY: every call forwards to `System` unchanged; the counters only observe.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note(layout.size() as isize);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        note(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            note(new_size as isize - layout.size() as isize);
        }
        new
    }
}

#[global_allocator]
static GLOBAL: Tracking = Tracking;

/// Peak heap this thread held above where it started, while `f` ran.
fn peak_heap_of(f: impl FnOnce()) -> usize {
    let start = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(start));
    f();
    (PEAK.with(Cell::get) - start).max(0) as usize
}

/// One input through one parser: an error result is fine, a panic or a heap
/// peak over the parser's budget is not.
fn check(parser: &RegisteredParser, input: &[u8]) -> Result<(), String> {
    let mut outcome = None;
    let peak = peak_heap_of(|| {
        outcome = Some(panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = black_box((parser.parse)(black_box(input)));
        })));
    });
    if outcome.is_some_and(|o| o.is_err()) {
        return Err(format!(
            "{} panicked on a {}-byte input",
            parser.name,
            input.len()
        ));
    }
    let budget = (parser.heap_budget)(input.len());
    if peak > budget {
        return Err(format!(
            "{} peaked at {peak} heap bytes on a {}-byte input (budget {budget})",
            parser.name,
            input.len()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Mutation {
    FlipBit {
        at: usize,
        bit: u8,
    },
    SetByte {
        at: usize,
        value: u8,
    },
    /// Length and offset fields are where parsers go wrong, so whole words get
    /// overwritten with edge values, both byte orders.
    SetU32 {
        at: usize,
        value: u32,
        big_endian: bool,
    },
    /// A word set relative to the input's own length (just short of, at and
    /// just past the end).
    SetU32LenRelative {
        at: usize,
        delta: i8,
    },
    Truncate {
        at: usize,
    },
    Insert {
        at: usize,
        bytes: Vec<u8>,
    },
    Remove {
        at: usize,
        len: usize,
    },
}

fn edge_u32() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(0),
        Just(1),
        Just(4),
        Just(0x10),
        Just(0x7fff),
        Just(0x8000),
        Just(0x8001),
        Just(0xffff),
        Just(0x7fff_ffff),
        Just(0x8000_0000),
        Just(u32::MAX),
        any::<u32>(),
    ]
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<usize>(), 0u8..8).prop_map(|(at, bit)| Mutation::FlipBit { at, bit }),
        (any::<usize>(), any::<u8>()).prop_map(|(at, value)| Mutation::SetByte { at, value }),
        (any::<usize>(), edge_u32(), any::<bool>()).prop_map(|(at, value, big_endian)| {
            Mutation::SetU32 {
                at,
                value,
                big_endian,
            }
        }),
        (any::<usize>(), any::<i8>())
            .prop_map(|(at, delta)| Mutation::SetU32LenRelative { at, delta }),
        any::<usize>().prop_map(|at| Mutation::Truncate { at }),
        (any::<usize>(), vec(any::<u8>(), 1..64))
            .prop_map(|(at, bytes)| Mutation::Insert { at, bytes }),
        (any::<usize>(), 1usize..64).prop_map(|(at, len)| Mutation::Remove { at, len }),
    ]
}

fn put_u32(buf: &mut [u8], at: usize, value: u32, big_endian: bool) {
    if buf.len() < 4 {
        return;
    }
    let at = at % (buf.len() - 3);
    let bytes = if big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    buf[at..at + 4].copy_from_slice(&bytes);
}

fn apply(seed: &[u8], mutations: &[Mutation]) -> Vec<u8> {
    let mut buf = seed.to_vec();
    for m in mutations {
        let len = buf.len();
        match m {
            Mutation::FlipBit { at, bit } if len > 0 => buf[at % len] ^= 1 << bit,
            Mutation::SetByte { at, value } if len > 0 => buf[at % len] = *value,
            Mutation::SetU32 {
                at,
                value,
                big_endian,
            } => put_u32(&mut buf, *at, *value, *big_endian),
            Mutation::SetU32LenRelative { at, delta } => {
                let value = (len as i64 + i64::from(*delta)).clamp(0, i64::from(u32::MAX)) as u32;
                put_u32(&mut buf, *at, value, false);
            }
            Mutation::Truncate { at } => buf.truncate(at % (len + 1)),
            Mutation::Insert { at, bytes } => {
                let at = at % (len + 1);
                buf.splice(at..at, bytes.iter().copied());
            }
            Mutation::Remove { at, len: n } if len > 0 => {
                let at = at % len;
                buf.drain(at..(at + n).min(len));
            }
            _ => {}
        }
    }
    buf
}

fn runner() -> TestRunner {
    let defaults = Config::default();
    TestRunner::new(Config {
        // Parsers are cheap; 256 (proptest's default) is thin for a fuzz pass.
        cases: defaults.cases.max(2048),
        failure_persistence: None,
        ..defaults
    })
}

fn run_random(parser: &RegisteredParser) -> Result<(), String> {
    runner()
        .run(&vec(any::<u8>(), 0..4096), |input| {
            check(parser, &input).map_err(TestCaseError::fail)
        })
        .map_err(|e| e.to_string())
}

fn run_mutated(parser: &RegisteredParser) -> Result<(), String> {
    let seeds = (parser.seeds)();
    if seeds.is_empty() {
        return Err(format!("{} registers no seeds", parser.name));
    }
    let strategy = (0..seeds.len(), vec(mutation(), 1..8));
    runner()
        .run(&strategy, |(which, mutations)| {
            check(parser, &apply(&seeds[which], &mutations)).map_err(TestCaseError::fail)
        })
        .map_err(|e| e.to_string())
}

#[test]
fn every_parser_accepts_its_own_seeds_within_budget() {
    for parser in PARSERS {
        let seeds = (parser.seeds)();
        assert!(!seeds.is_empty(), "{} registers no seeds", parser.name);
        for (i, seed) in seeds.iter().enumerate() {
            if let Err(e) = (parser.parse)(seed) {
                panic!("{} rejects its own seed {i}: {e}", parser.name);
            }
            if let Err(e) = check(parser, seed) {
                panic!("seed {i}: {e}");
            }
        }
    }
}

#[test]
fn every_parser_survives_random_bytes() {
    for parser in PARSERS {
        if let Err(e) = run_random(parser) {
            panic!("{e}");
        }
    }
}

#[test]
fn every_parser_survives_mutated_seeds() {
    for parser in PARSERS {
        if let Err(e) = run_mutated(parser) {
            panic!("{e}");
        }
    }
}

// The harness checks itself against parsers that misbehave on purpose, so a
// green run above means something.

fn tiny_seed() -> Vec<Vec<u8>> {
    vec![vec![0; 16]]
}

fn linear_budget(len: usize) -> usize {
    16 * len + 4096
}

fn panics(input: &[u8]) -> Result<(), FormatError> {
    // Panics on every input, the way an unchecked slice index past the end
    // would. Panicking only on short inputs left the self-test flaky: the
    // mutated 16-byte seeds rarely come out under 9 bytes, and a run could
    // miss them all.
    let _ = black_box(&input[..input.len() + 1]);
    Ok(())
}

fn greedy(input: &[u8]) -> Result<(), FormatError> {
    // Allocates a megabyte whatever the input says, the way trusting a length
    // field would.
    black_box(vec![0u8; 1 << 20]);
    let _ = input;
    Ok(())
}

fn frugal(input: &[u8]) -> Result<(), FormatError> {
    black_box(input.to_vec());
    Ok(())
}

fn fake(name: &'static str, parse: fn(&[u8]) -> Result<(), FormatError>) -> RegisteredParser {
    RegisteredParser {
        name,
        parse,
        seeds: tiny_seed,
        heap_budget: linear_budget,
    }
}

#[test]
fn harness_reports_a_panic() {
    let err = run_random(&fake("panics", panics)).unwrap_err();
    assert!(err.contains("panics panicked"), "{err}");
}

#[test]
fn harness_reports_a_heap_peak_over_budget() {
    let err = run_mutated(&fake("greedy", greedy)).unwrap_err();
    assert!(err.contains("greedy peaked at"), "{err}");
}

#[test]
fn harness_passes_a_parser_within_budget() {
    let parser = fake("frugal", frugal);
    run_random(&parser).unwrap();
    run_mutated(&parser).unwrap();
}

#[test]
fn mutations_stay_in_bounds_on_empty_and_short_inputs() {
    let all = [
        Mutation::FlipBit { at: 7, bit: 3 },
        Mutation::SetByte { at: 9, value: 1 },
        Mutation::SetU32 {
            at: usize::MAX,
            value: 1,
            big_endian: true,
        },
        Mutation::SetU32LenRelative { at: 3, delta: -128 },
        Mutation::Truncate { at: usize::MAX },
        Mutation::Remove {
            at: usize::MAX,
            len: 63,
        },
        Mutation::Insert {
            at: usize::MAX,
            bytes: vec![1, 2],
        },
    ];
    for seed in [&[][..], &[1][..], &[1, 2, 3][..], &[0; 4][..]] {
        apply(seed, &all);
    }
}
