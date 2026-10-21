//! O4's frame-locked input against an in-process fake of P1's listener
//! (openspec AF.7). The fake ports `sky_oracle_pad_handle` and the schedule and
//! readback beside `pad_get_data` from
//! `rpcs3-patches/oracle/0004-O4-*.patch`, and models the guest around them: a
//! flip counter that advances while running, and a fixed number of pad reads a
//! port each flip, each taking the counter once and reading the port's entry at
//! or before it, else the host's input. The reads of the flip a pause lands on
//! happen before the pause, the case that makes an entry for the current flip
//! past. It keeps O3's `STATE`, `PAUSE_AT` and `RESUME` (as
//! `verbs_loopback.rs` models them) so a script loads at a real pause. With the
//! gate off every pad verb is `ERR unknown_cmd`. It's the executable spec the
//! live binary has to match; `live_pad` checks it does.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use guac_oracle::input::{Button, Ds3Pad, InputScript, PadState, Stick};
use oracle_harness::control::Control;
use oracle_harness::verbs::{PadRead, Refusal, Reply, ReplyError, Verb, VerbClient, parse_reply};

const FLIP: Duration = Duration::from_millis(2);
const WAIT: Duration = Duration::from_secs(10);
/// O4's `oracle_pad_history`.
const HISTORY: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Paused,
}

/// `oracle_pad_flip_reads`.
#[derive(Debug, Clone, Copy)]
struct FlipReads {
    flip: u64,
    reads: u32,
    differ: u32,
    first: Ds3Pad,
}

struct Emu {
    gate: bool,
    renderer: bool,
    status: Status,
    frames: u64,
    pause_at: u64,
    /// The host's input per port: the keyboard handler's, P6's forced presses.
    host: [Ds3Pad; 2],
    /// The guest's reads of each port a flip; 0 for a port it never reads.
    reads_per_flip: [u32; 2],
    /// `oracle_pad_max_entries`; a test lowers it.
    max_entries: usize,
    schedule: [BTreeMap<u64, Ds3Pad>; 2],
    history: [Vec<Option<FlipReads>>; 2],
    last: [Option<(u64, Ds3Pad)>; 2],
}

impl Emu {
    fn new() -> Self {
        Self {
            gate: true,
            renderer: true,
            status: Status::Running,
            frames: 100,
            pause_at: 0,
            host: [Ds3Pad::REST; 2],
            reads_per_flip: [3, 2],
            max_entries: 1 << 16,
            schedule: Default::default(),
            history: [vec![None; HISTORY as usize], vec![None; HISTORY as usize]],
            last: [None; 2],
        }
    }

    /// A savestate's reboot or a fresh boot: `oracle_pad_session` drops it all.
    fn new_session(&mut self) {
        self.frames = 2;
        self.pause_at = 0;
        self.status = Status::Paused;
        self.schedule = Default::default();
        self.history = [vec![None; HISTORY as usize], vec![None; HISTORY as usize]];
        self.last = [None; 2];
    }

    /// One `pad_get_data`: the counter once, the entry at or before it or the
    /// host's input, then `oracle_pad_record`.
    fn read(&mut self, port: usize) {
        let flip = self.frames;
        let got = self.schedule[port]
            .range(..=flip)
            .next_back()
            .map_or(self.host[port], |(_, pad)| *pad);
        let slot = &mut self.history[port][(flip % HISTORY) as usize];
        match slot {
            Some(h) if h.flip == flip => {
                h.reads += 1;
                if got != h.first {
                    h.differ += 1;
                }
            }
            _ => {
                *slot = Some(FlipReads {
                    flip,
                    reads: 1,
                    differ: 0,
                    first: got,
                })
            }
        }
        self.last[port] = Some((flip, got));
    }

    /// One guest frame: the flip, the guest's reads in it, then the pause check.
    fn frame(&mut self) {
        self.frames += 1;
        for port in 0..2 {
            for _ in 0..self.reads_per_flip[port] {
                self.read(port);
            }
        }
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

    /// `oracle_pad_set`.
    fn pad_set(&mut self, port: usize, flip: u64, pad: Ds3Pad) -> Option<&'static str> {
        if !self.renderer {
            return Some("no_renderer");
        }
        if flip <= self.frames {
            return Some("past");
        }
        let schedule = &mut self.schedule[port];
        if !schedule.contains_key(&flip) && schedule.len() >= self.max_entries {
            return Some("full");
        }
        schedule.insert(flip, pad);
        None
    }

    /// `oracle_pad_read`.
    fn pad_read(&self, port: usize, flip: Option<u64>) -> Option<(u64, u32, u32, Ds3Pad)> {
        let at = match flip {
            Some(f) => f,
            None => self.last[port]?.0,
        };
        let h = self.history[port][(at % HISTORY) as usize].filter(|h| h.flip == at)?;
        let pad = match flip {
            Some(_) => h.first,
            None => self.last[port]?.1,
        };
        Some((at, h.reads, h.differ, pad))
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

/// `sky_oracle_pad_handle`, behind the gate, plus the O3 verbs a load needs.
fn handle(emu: &Arc<Mutex<Emu>>, line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    let mut e = emu.lock().unwrap();
    if cmd == "STATE" {
        return e.state_line("OK");
    }
    if !e.gate {
        return "ERR unknown_cmd\n".into();
    }
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
        "PAD" => {
            // `sky_words` splits on single spaces, so a doubled one is an empty field.
            let w: Vec<&str> = arg.split(' ').collect();
            if w.len() != 8 {
                return "ERR bad_args\n".into();
            }
            let Some(port) = parse_num(w[0], 10, 1, 1) else {
                return "ERR bad_port\n".into();
            };
            let Some(flip) = parse_flip(w[1]).filter(|f| *f != 0) else {
                return "ERR bad_flip\n".into();
            };
            let (Some(d1), Some(d2)) = (parse_num(w[2], 16, 4, 0xff), parse_num(w[3], 16, 4, 0xff))
            else {
                return "ERR bad_button\n".into();
            };
            let axes: Option<Vec<u8>> = w[4..]
                .iter()
                .map(|a| parse_num(a, 10, 3, 255).map(|v| v as u8))
                .collect();
            let Some(axes) = axes else {
                return "ERR bad_axis\n".into();
            };
            let pad = Ds3Pad {
                digital1: d1 as u8,
                digital2: d2 as u8,
                left: Stick {
                    x: axes[0],
                    y: axes[1],
                },
                right: Stick {
                    x: axes[2],
                    y: axes[3],
                },
            };
            match e.pad_set(port as usize, flip, pad) {
                Some(err) => format!("ERR {err}\n"),
                None => "OK\n".into(),
            }
        }
        "PAD_CLEAR" if arg.is_empty() => {
            e.schedule = Default::default();
            "OK\n".into()
        }
        "PAD_CLEAR" => "ERR bad_args\n".into(),
        "PAD_STATE" => {
            let w: Vec<&str> = if arg.is_empty() {
                Vec::new()
            } else {
                arg.split(' ').collect()
            };
            if w.is_empty() || w.len() > 2 {
                return "ERR bad_args\n".into();
            }
            let Some(port) = parse_num(w[0], 10, 1, 1) else {
                return "ERR bad_port\n".into();
            };
            let flip = match w.get(1) {
                None => None,
                Some(f) => match parse_flip(f) {
                    Some(f) => Some(f),
                    None => return "ERR bad_flip\n".into(),
                },
            };
            match e.pad_read(port as usize, flip) {
                None => "ERR no_read\n".into(),
                Some((at, reads, differ, p)) => format!(
                    "OK flip={at} reads={reads} differ={differ} d1={:02x} d2={:02x} lx={} ly={} rx={} ry={}\n",
                    p.digital1, p.digital2, p.left.x, p.left.y, p.right.x, p.right.y
                ),
            }
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
    fn set(&self, f: impl FnOnce(&mut Emu)) {
        f(&mut self.emu.lock().unwrap());
    }

    fn verbs(&self) -> VerbClient {
        VerbClient::new(&self.socket)
    }

    fn line(&self, request: &str) -> String {
        self.verbs()
            .roundtrip_line(&format!("{request}\n"))
            .unwrap()
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn spawn() -> Fake {
    let config_dir = tempfile::tempdir().unwrap();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let socket = std::env::temp_dir().join(format!("o4-{}-{n}.sock", std::process::id()));
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

fn pad(buttons: &[Button], left: (u8, u8), right: (u8, u8)) -> PadState {
    PadState {
        buttons: buttons.iter().copied().collect(),
        left: Stick {
            x: left.0,
            y: left.1,
        },
        right: Stick {
            x: right.0,
            y: right.1,
        },
    }
}

/// Pauses a few flips on and returns where. A loaded host can let more than
/// five 2 ms flips pass between reading the count and asking (load 157 did,
/// on 2026-10-19), so a refusal as past asks again from the new count.
fn pause(fake: &Fake) -> u64 {
    for _ in 0..50 {
        let at = fake.control.state().unwrap().frames + 5;
        match fake.control.pause_at(at, WAIT) {
            Ok(paused) => return paused.frames,
            Err(e) if e.to_string().contains("ERR past") => continue,
            Err(e) => panic!("{e:#}"),
        }
    }
    panic!("PAUSE_AT landed in the past 50 times running");
}

#[test]
fn with_the_gate_off_the_pad_verbs_are_unknown_commands() {
    let fake = spawn();
    fake.set(|e| e.gate = false);
    for verb in [
        Verb::Pad {
            port: 0,
            flip: 1_000_000,
            pad: Ds3Pad::REST,
        },
        Verb::PadClear,
        Verb::PadState {
            port: 0,
            flip: None,
        },
    ] {
        assert_eq!(
            refusal(fake.verbs().send(verb).unwrap_err()),
            Refusal::UnknownCmd,
            "{verb:?}"
        );
    }
    assert!(
        fake.emu
            .lock()
            .unwrap()
            .schedule
            .iter()
            .all(BTreeMap::is_empty)
    );
}

#[test]
fn a_script_is_read_from_its_flips_on_both_ports_every_read_of_a_flip_alike() {
    let fake = spawn();
    // The host holds CROSS on port 0 (a P6 press, say): the script replaces it.
    let host = pad(&[Button::South], (128, 128), (128, 128)).ds3();
    fake.set(|e| e.host[0] = host);
    let at = pause(&fake);

    let (n, k) = (20, 15);
    let p0 = pad(&[Button::RightTrigger2], (128, 128), (200, 60));
    let p1 = pad(&[Button::LeftTrigger2], (30, 220), (128, 128));
    let script = InputScript::from_ron(&format!(
        "(steps: [
            (at: {n}, pads: {{0: {}, 1: {}}}),
            (at: {}, pads: {{0: (), 1: (left: (x: 30, y: 220))}}),
        ])",
        ron_pad(&p0),
        ron_pad(&p1),
        n + k
    ))
    .unwrap();
    let loaded = fake.control.load_script(&script).unwrap();
    assert_eq!((loaded.base, loaded.entries), (at + 1, 4));
    assert_eq!(loaded.paused_at.frames, at);

    let end = loaded.base + n + k + 10;
    fake.control.pause_at(end, WAIT).unwrap();
    let flip_n = loaded.base + n;
    for port in 0..2u8 {
        let script_at = |offset| script.state_at(usize::from(port), offset).map(|s| s.ds3());
        for flip in (at + 1)..=end {
            let read = fake.control.pad_state(port, Some(flip)).unwrap().unwrap();
            let want = script_at(flip - loaded.base).unwrap_or(if port == 0 {
                host
            } else {
                Ds3Pad::REST
            });
            assert_eq!(read.pad, want, "port {port} flip {flip}");
            assert_eq!(read.differ, 0, "port {port} flip {flip}: reads disagree");
            assert_eq!(
                read.reads,
                [3, 2][usize::from(port)],
                "port {port} flip {flip}"
            );
        }
        // The first read after flip N carries N's state, the one before it doesn't.
        let first = fake.control.pad_state(port, Some(flip_n)).unwrap().unwrap();
        assert_eq!(Some(first.pad), script_at(n), "port {port}");
        let before = fake
            .control
            .pad_state(port, Some(flip_n - 1))
            .unwrap()
            .unwrap();
        assert_ne!(Some(before.pad), script_at(n), "port {port}");
        // Without a flip: the last read, in the flip the run paused on.
        let last = fake.control.pad_state(port, None).unwrap().unwrap();
        assert_eq!((last.flip, Some(last.pad)), (end, script_at(n + k)));
    }
}

/// A pad state as a script spells it.
fn ron_pad(state: &PadState) -> String {
    let buttons: Vec<String> = state.buttons.iter().map(|b| format!("{b:?}")).collect();
    format!(
        "(buttons: [{}], left: (x: {}, y: {}), right: (x: {}, y: {}))",
        buttons.join(", "),
        state.left.x,
        state.left.y,
        state.right.x,
        state.right.y
    )
}

#[test]
fn an_entry_at_or_behind_the_counter_is_past_and_one_ahead_goes_in() {
    let fake = spawn();
    let at = pause(&fake);
    for flip in [at - 1, at] {
        let err = fake
            .verbs()
            .send(Verb::Pad {
                port: 1,
                flip,
                pad: Ds3Pad::REST,
            })
            .unwrap_err();
        assert_eq!(refusal(err), Refusal::Past, "PAD at {flip}, paused at {at}");
    }
    assert_eq!(
        fake.verbs()
            .send(Verb::Pad {
                port: 1,
                flip: at + 1,
                pad: Ds3Pad::REST
            })
            .unwrap(),
        Reply::Ok
    );
    // The reads of the flip the pause landed on happened already.
    let read = fake.control.pad_state(0, Some(at)).unwrap().unwrap();
    assert_eq!(read.reads, 3);
}

#[test]
fn pad_takes_eight_fields_in_range_and_names_what_is_wrong() {
    let fake = spawn();
    let at = pause(&fake);
    let f = at + 10;
    let cases: Vec<(String, Refusal)> = vec![
        ("PAD".into(), Refusal::BadArgs),
        (format!("PAD 0 {f} 00 00 128 128 128"), Refusal::BadArgs),
        (
            format!("PAD 0 {f} 00 00 128 128 128 128 1"),
            Refusal::BadArgs,
        ),
        (
            format!("PAD 0 {f}  00 00 128 128 128 128"),
            Refusal::BadArgs,
        ),
        (format!("PAD 2 {f} 00 00 128 128 128 128"), Refusal::BadPort),
        (
            format!("PAD 00 {f} 00 00 128 128 128 128"),
            Refusal::BadPort,
        ),
        (format!("PAD x {f} 00 00 128 128 128 128"), Refusal::BadPort),
        ("PAD 0 0 00 00 128 128 128 128".into(), Refusal::BadFlip),
        ("PAD 0 -5 00 00 128 128 128 128".into(), Refusal::BadFlip),
        ("PAD 0 1e3 00 00 128 128 128 128".into(), Refusal::BadFlip),
        (
            format!("PAD 0 {f} 100 00 128 128 128 128"),
            Refusal::BadButton,
        ),
        (
            format!("PAD 0 {f} 00 0x40 128 128 128 128"),
            Refusal::BadButton,
        ),
        (
            format!("PAD 0 {f} 00 00000 128 128 128 128"),
            Refusal::BadButton,
        ),
        (
            format!("PAD 0 {f} 00 g0 128 128 128 128"),
            Refusal::BadButton,
        ),
        (format!("PAD 0 {f} 00 00 256 128 128 128"), Refusal::BadAxis),
        (format!("PAD 0 {f} 00 00 128 -1 128 128"), Refusal::BadAxis),
        (
            format!("PAD 0 {f} 00 00 128 128 0128 128"),
            Refusal::BadAxis,
        ),
        (format!("PAD 0 {f} 00 00 128 128 128 ff"), Refusal::BadAxis),
        ("PAD_CLEAR now".into(), Refusal::BadArgs),
        ("PAD_STATE".into(), Refusal::BadArgs),
        ("PAD_STATE 0 1 2".into(), Refusal::BadArgs),
        ("PAD_STATE 7".into(), Refusal::BadPort),
        ("PAD_STATE 0 x".into(), Refusal::BadFlip),
    ];
    for (line, want) in &cases {
        let verb = match line.split(' ').next().unwrap() {
            "PAD" => Verb::Pad {
                port: 0,
                flip: 1,
                pad: Ds3Pad::REST,
            },
            "PAD_CLEAR" => Verb::PadClear,
            _ => Verb::PadState {
                port: 0,
                flip: None,
            },
        };
        let reply = fake.line(line);
        assert_eq!(
            parse_reply(verb, &reply).unwrap_err().refusal(),
            Some(*want),
            "{line:?} -> {reply:?}"
        );
    }
    assert!(
        fake.emu
            .lock()
            .unwrap()
            .schedule
            .iter()
            .all(BTreeMap::is_empty)
    );
    // Hex in either case, axes at their edges.
    assert_eq!(fake.line(&format!("PAD 1 {f} fF 0 0 255 1 254")), "OK");
    assert_eq!(
        fake.emu.lock().unwrap().schedule[1][&f],
        Ds3Pad {
            digital1: 0xff,
            digital2: 0,
            left: Stick { x: 0, y: 255 },
            right: Stick { x: 1, y: 254 },
        }
    );
}

#[test]
fn pad_clear_hands_both_ports_back_to_the_host() {
    let fake = spawn();
    let host = pad(&[Button::Start], (128, 128), (128, 128)).ds3();
    fake.set(|e| e.host[1] = host);
    pause(&fake);
    let script = InputScript::from_ron(
        "(steps: [(at: 0, pads: {0: (buttons: [East]), 1: (buttons: [North])})])",
    )
    .unwrap();
    let loaded = fake.control.load_script(&script).unwrap();
    let held = fake.control.pause_at(loaded.base + 5, WAIT).unwrap();
    let east = pad(&[Button::East], (128, 128), (128, 128)).ds3();
    assert_eq!(fake.control.pad_state(0, None).unwrap().unwrap().pad, east);

    fake.control.pad_clear().unwrap();
    fake.control.pause_at(held.frames + 3, WAIT).unwrap();
    let last = |port| fake.control.pad_state(port, None).unwrap().unwrap().pad;
    assert_eq!((last(0), last(1)), (Ds3Pad::REST, host));
    // What the guest read under the script stays readable.
    let then = fake
        .control
        .pad_state(0, Some(held.frames))
        .unwrap()
        .unwrap();
    assert_eq!(then.pad, east);
}

#[test]
fn a_second_load_replaces_the_first_script_whole() {
    let fake = spawn();
    pause(&fake);
    let first = InputScript::from_ron(
        "(steps: [(at: 0, pads: {0: (buttons: [West])}), (at: 50, pads: {1: (buttons: [West])})])",
    )
    .unwrap();
    fake.control.load_script(&first).unwrap();
    let second =
        InputScript::from_ron("(steps: [(at: 0, pads: {0: (buttons: [North])})])").unwrap();
    let loaded = fake.control.load_script(&second).unwrap();
    assert_eq!(loaded.entries, 1);
    let schedule = fake.emu.lock().unwrap().schedule.clone();
    assert_eq!(schedule[0].len(), 1);
    assert!(
        schedule[1].is_empty(),
        "the first script's port 1 entry outlived it"
    );
}

#[test]
fn an_appended_script_goes_in_beside_the_last_and_the_paused_flip_keeps_its_state() {
    // A run that pauses as it goes (AH.7's route driver): each pause appends
    // the next stretch from the flip after, and the flip it's paused at reads
    // what the last stretch left there, not the host.
    let fake = spawn();
    let host = pad(&[Button::Start], (128, 128), (128, 128)).ds3();
    fake.set(|e| e.host = [host; 2]);
    pause(&fake);
    let east = InputScript::from_ron(
        "(steps: [(at: 0, pads: {0: (buttons: [East]), 1: ()}), (at: 3, pads: {0: (left: (x: 0, y: 128))})])",
    )
    .unwrap();
    let first = fake.control.load_script(&east).unwrap();
    let held = fake.control.pause_at(first.base + 6, WAIT).unwrap();
    let north = InputScript::from_ron("(steps: [(at: 0, pads: {0: (buttons: [North])})])").unwrap();
    let appended = fake
        .control
        .append_script_at(&north, held.frames + 1)
        .unwrap();
    assert_eq!((appended.base, appended.entries), (held.frames + 1, 1));
    let schedule = fake.emu.lock().unwrap().schedule.clone();
    assert_eq!(schedule[0].len(), 3, "the first stretch's entries stay");
    assert_eq!(schedule[1].len(), 1);
    let at = |flip: u64| {
        schedule[0]
            .range(..=flip)
            .next_back()
            .map(|(_, p)| *p)
            .unwrap()
    };
    let left = pad(&[], (0, 128), (128, 128)).ds3();
    assert_eq!(
        at(held.frames),
        left,
        "the paused flip keeps the last state"
    );
    let north_pad = pad(&[Button::North], (128, 128), (128, 128)).ds3();
    assert_eq!(at(held.frames + 1), north_pad);
    // Read through: the paused flip's read is the stick, the next North.
    fake.control.pause_at(held.frames + 4, WAIT).unwrap();
    let read = |flip| fake.control.pad_state(0, Some(flip)).unwrap().unwrap().pad;
    assert_eq!(read(held.frames + 1), north_pad);
    assert_ne!(read(held.frames + 1), host);
    // An append that isn't ahead of the pause is refused like a load, and a
    // load still clears what was appended.
    let paused = fake.control.state().unwrap().frames;
    assert!(fake.control.append_script_at(&north, paused).is_err());
    fake.control.load_script(&north).unwrap();
    assert_eq!(fake.emu.lock().unwrap().schedule[0].len(), 1);
}

#[test]
fn the_loader_wants_a_pause_and_a_base_ahead_of_it() {
    let fake = spawn();
    let script = InputScript::from_ron("(steps: [(at: 0, pads: {0: ()})])").unwrap();
    let err = fake.control.load_script(&script).unwrap_err().to_string();
    assert!(err.contains("load a script at a pause"), "{err}");
    let at = pause(&fake);
    let err = fake
        .control
        .load_script_at(&script, at)
        .unwrap_err()
        .to_string();
    assert!(err.contains("isn't ahead of the pause"), "{err}");
    assert!(
        fake.emu
            .lock()
            .unwrap()
            .schedule
            .iter()
            .all(BTreeMap::is_empty)
    );
    // Offset 0 at a later flip is fine: the port reads the host until then.
    let loaded = fake.control.load_script_at(&script, at + 30).unwrap();
    assert_eq!(loaded.base, at + 30);
}

#[test]
fn a_port_the_guest_never_reads_has_no_read_and_the_session_drops_it_all() {
    let fake = spawn();
    // The ticker runs from spawn on, so port 1 may already have been read by
    // the time this lands: what it read goes with it.
    fake.set(|e| {
        e.reads_per_flip[1] = 0;
        e.history[1] = vec![None; HISTORY as usize];
        e.last[1] = None;
    });
    pause(&fake);
    assert_eq!(fake.control.pad_state(1, None).unwrap(), None);
    assert!(fake.control.pad_state(0, None).unwrap().is_some());
    let script =
        InputScript::from_ron("(steps: [(at: 3, pads: {0: (buttons: [South])})])").unwrap();
    fake.control.load_script(&script).unwrap();

    // A savestate's reboot: flips count from 0 again, so nothing carries over.
    fake.set(Emu::new_session);
    assert_eq!(fake.control.pad_state(0, None).unwrap(), None);
    assert!(
        fake.emu
            .lock()
            .unwrap()
            .schedule
            .iter()
            .all(BTreeMap::is_empty)
    );
    // Older than the history keeps, or never read: no read either way.
    assert_eq!(fake.control.pad_state(0, Some(HISTORY * 9)).unwrap(), None);
}

#[test]
fn a_full_schedule_refuses_one_more_entry_but_takes_a_replacement() {
    let fake = spawn();
    fake.set(|e| e.max_entries = 3);
    let at = pause(&fake);
    let send = |flip| {
        fake.verbs().send(Verb::Pad {
            port: 0,
            flip,
            pad: Ds3Pad::REST,
        })
    };
    for flip in at + 1..=at + 3 {
        send(flip).unwrap();
    }
    assert_eq!(refusal(send(at + 4).unwrap_err()), Refusal::Full);
    assert_eq!(send(at + 2).unwrap(), Reply::Ok);
    // The other port has its own room.
    let other = fake.verbs().send(Verb::Pad {
        port: 1,
        flip: at + 4,
        pad: Ds3Pad::REST,
    });
    assert_eq!(other.unwrap(), Reply::Ok);
}

#[test]
fn a_load_refused_part_way_leaves_the_host_not_half_a_script() {
    let fake = spawn();
    fake.set(|e| e.max_entries = 2);
    let at = pause(&fake);
    // Port 0's third entry is one past the room: the first two went in.
    let script = InputScript::from_ron(
        "(steps: [
            (at: 0, pads: {0: (buttons: [South]), 1: (buttons: [East])}),
            (at: 5, pads: {0: ()}),
            (at: 9, pads: {0: (buttons: [North])}),
        ])",
    )
    .unwrap();
    let err = fake.control.load_script(&script).unwrap_err();
    assert_eq!(refusal(err), Refusal::Full);
    assert!(
        fake.emu
            .lock()
            .unwrap()
            .schedule
            .iter()
            .all(BTreeMap::is_empty),
        "a failed load left part of its script behind"
    );
    // Run past where the script would have pressed: the guest read the host.
    fake.control.pause_at(at + 12, WAIT).unwrap();
    for port in 0..2 {
        for flip in at + 1..=at + 12 {
            let read = fake.control.pad_state(port, Some(flip)).unwrap().unwrap();
            assert_eq!(read.pad, Ds3Pad::REST, "port {port} flip {flip}");
        }
    }
}

#[test]
fn many_lines_share_one_connection() {
    let fake = spawn();
    let at = pause(&fake);
    let verbs: Vec<Verb> = (1..=300)
        .map(|i| Verb::Pad {
            port: (i % 2) as u8,
            flip: at + i,
            pad: Ds3Pad::REST,
        })
        .chain([Verb::PadState {
            port: 0,
            flip: Some(at),
        }])
        .collect();
    let replies = fake.verbs().send_all(&verbs).unwrap();
    assert_eq!(replies.len(), 301);
    assert!(replies[..300].iter().all(|r| *r == Ok(Reply::Ok)));
    assert!(matches!(
        replies[300],
        Ok(Reply::PadRead(PadRead { flip, reads: 3, differ: 0, .. })) if flip == at
    ));
}
