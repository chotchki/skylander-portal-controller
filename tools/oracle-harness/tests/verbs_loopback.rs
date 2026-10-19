//! The O-verbs against an in-process fake of P1's listener with O3 in it
//! (openspec AF.5). The fake ports `sky_oracle_handle` from
//! `rpcs3-patches/oracle/0003-O3-*.patch` and models what RPCS3 does around
//! it: a flip counter that advances while running, the PAUSE_AT target checked
//! at each flip, a capture armed at one frame end, recorded through the next and
//! written with a pause before that frame's flip (one flip on from where it was
//! armed), a screenshot taken by the next present (or at once while paused, RSX's
//! native-UI flip), and a savestate that stops, writes its file, boots again and
//! holds at Start Paused's 2 flips, or that RPCS3 turns down after its `OK`,
//! saying why only in its log. With the gate off every verb is
//! `ERR unknown_cmd`. It's the executable spec the live binary has to match;
//! `live_verbs` checks it does.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oracle_harness::control::Control;
use oracle_harness::verbs::{Refusal, Reply, ReplyError, Verb, VerbClient, parse_reply};

const TITLE: &str = "BLUS30968";
/// Fast enough that a test's 40 flips take well under a second.
const FLIP: Duration = Duration::from_millis(2);
/// The fake's SCREENSHOT wait; O3 waits 5 s.
const SHOT_WAIT: Duration = Duration::from_millis(300);
/// The line RPCS3 logged when it turned `live_verbs`' save down (review, 2026-10-18).
const REFUSED_LINE: &str = "\u{b7}E 0:02:09.473813 {Savestate Prepare Thread} SYS: Failed to savestate: \
                            failed to lock SPU threads execution.\n";
const WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Paused,
    Stopping,
    Loading,
}

impl Status {
    fn wire(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Paused => "paused",
            Status::Stopping => "stopping",
            Status::Loading => "loading",
        }
    }
}

struct Emu {
    gate: bool,
    renderer: bool,
    status: Status,
    /// `int_flip_index`.
    frames: u64,
    /// `g_oracle_pause_at_flip`.
    pause_at: u64,
    /// `m_pause_after_x_flips`, set by Start Paused on a savestate boot.
    pause_after: u32,
    /// `g_user_asked_for_frame_capture` and `capture_current_frame`.
    cap_flag: bool,
    capturing: bool,
    /// A capture's pause holds the next flip back: RPCS3 writes the capture
    /// and pauses inside the captured frame's display queue, before the flip
    /// that presents it (every live capture opens with a flip head and ends
    /// with its queue), so armed at a pause at flip n it pauses at n+1 and the
    /// captured frame is n+2's.
    parked: bool,
    /// Every `PAUSE_AT` target is first seen past by the flip check and
    /// dropped, O3's drop path. After an `OK` it takes the target's store and
    /// the counter's bump reaching the two threads out of order
    /// (`int_flip_index` is a plain `u64`): not seen live, but not ruled out.
    misses_targets: bool,
    /// `g_user_asked_for_screenshot`.
    shot_flag: bool,
    /// Off: no present ever happens (a swapchain that's gone), so a SCREENSHOT
    /// times out.
    presents: bool,
    /// How long RPCS3's teardown takes before the state file lands; a test
    /// lengthens it to send a verb into the stop.
    stop_for: Duration,
    /// RPCS3 turns every save down after `SAVESTATE`'s `OK`, logging why (an
    /// SPU it can't lock, say) and leaving the run where it was.
    refuses_saves: bool,
    config_dir: PathBuf,
    written: u32,
}

impl Emu {
    fn new(config_dir: &Path) -> Self {
        Self {
            gate: true,
            renderer: true,
            status: Status::Running,
            frames: 100,
            pause_at: 0,
            pause_after: 0,
            cap_flag: false,
            capturing: false,
            parked: false,
            misses_targets: false,
            shot_flag: false,
            presents: true,
            stop_for: Duration::from_millis(30),
            refuses_saves: false,
            config_dir: config_dir.to_path_buf(),
            written: 0,
        }
    }

    fn next_name(&mut self) -> u32 {
        self.written += 1;
        self.written
    }

    /// `gs_frame::take_screenshot`: the frame's pixels follow from its flip.
    fn take_screenshot(&mut self) {
        let n = self.next_name();
        let dir = self.config_dir.join("screenshots").join(TITLE);
        std::fs::create_dir_all(&dir).unwrap();
        let shade = (self.frames % 200) as u8 + 20;
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([shade, 7, 7, 255]));
        let path = dir.join(format!("{TITLE}_screenshot_{n:04}.png"));
        image
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
    }

    /// One guest frame: `on_frame_end`, then `handle_emu_flip`'s counter and
    /// present, then `rsx::thread::flip`'s pauses.
    fn frame(&mut self) {
        let mut pause = false;
        if std::mem::take(&mut self.parked) {
            // The flip a capture's pause held back goes first.
        } else if self.cap_flag && !self.capturing {
            self.cap_flag = false;
            self.capturing = true;
        } else if self.capturing {
            self.capturing = false;
            let n = self.next_name();
            let dir = self.config_dir.join("captures");
            std::fs::create_dir_all(&dir).unwrap();
            // Committed by rename, as fs::pending_file does.
            let temp = dir.join(format!("\u{ff04}{n}.tmp"));
            std::fs::write(&temp, b"RRC\0").unwrap();
            std::fs::rename(&temp, dir.join(format!("{TITLE}_{n:04}_capture.rrc.gz"))).unwrap();
            self.status = Status::Paused;
            self.parked = true;
            return;
        }
        self.frames += 1;
        if self.shot_flag && self.presents {
            self.shot_flag = false;
            self.take_screenshot();
        }
        if self.pause_after > 0 {
            self.pause_after -= 1;
            if self.pause_after == 0 {
                pause = true;
            }
        }
        if self.pause_at != 0 && self.frames >= self.pause_at {
            // Dropped, never honoured late.
            pause |= self.frames == self.pause_at && !self.misses_targets;
            self.pause_at = 0;
        }
        if pause {
            self.status = Status::Paused;
        }
    }

    fn state_line(&self, prefix: &str) -> String {
        format!(
            "{prefix} status={} frames={} progr=7/7 seg=0/0\n",
            self.status.wire(),
            self.frames
        )
    }
}

/// `sky_parse_flip`.
fn parse_flip(arg: &str) -> Option<u64> {
    if arg.is_empty() || arg.len() > 19 || !arg.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    arg.parse().ok()
}

/// `sky_ipc_handle`'s P1 words the harness uses, then `sky_oracle_handle`
/// behind the gate.
fn handle(emu: &Arc<Mutex<Emu>>, line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    let mut e = emu.lock().unwrap();
    match cmd {
        "STATE" => return e.state_line("OK"),
        "RECONNECT" => return "OK\n".into(),
        _ => {}
    }
    if !e.gate {
        return "ERR unknown_cmd\n".into();
    }
    match cmd {
        "PAUSE_AT" => match parse_flip(arg) {
            None | Some(0) => "ERR bad_flip\n".into(),
            Some(_) if !e.renderer => "ERR no_renderer\n".into(),
            Some(at) if at <= e.frames => "ERR past\n".into(),
            Some(at) => {
                e.pause_at = at;
                "OK\n".into()
            }
        },
        "RESUME" => {
            if e.status != Status::Paused {
                return "ERR not_paused\n".into();
            }
            e.status = Status::Running;
            "OK\n".into()
        }
        "RSXCAP" => {
            if !e.renderer {
                "ERR no_renderer\n".into()
            } else if e.capturing || std::mem::replace(&mut e.cap_flag, true) {
                "ERR busy\n".into()
            } else {
                "OK\n".into()
            }
        }
        "SCREENSHOT" => {
            if !e.renderer {
                return "ERR no_renderer\n".into();
            }
            if std::mem::replace(&mut e.shot_flag, true) {
                return "ERR busy\n".into();
            }
            drop(e);
            let deadline = Instant::now() + SHOT_WAIT;
            loop {
                let mut e = emu.lock().unwrap();
                if !e.shot_flag {
                    return format!("OK flip={}\n", e.frames);
                }
                if Instant::now() > deadline {
                    e.shot_flag = false;
                    return "ERR timeout\n".into();
                }
                drop(e);
                thread::sleep(Duration::from_millis(1));
            }
        }
        "SAVESTATE" => {
            if !matches!(e.status, Status::Running | Status::Paused) {
                return "ERR not_running\n".into();
            }
            if e.refuses_saves {
                // `Emulator::Kill`'s prepare thread, on its own, a moment later.
                let log = e.config_dir.join("RPCS3.log");
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    let mut f = std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(log)
                        .unwrap();
                    f.write_all(REFUSED_LINE.as_bytes()).unwrap();
                });
                return "OK\n".into();
            }
            e.status = Status::Stopping;
            let emu = Arc::clone(emu);
            thread::spawn(move || savestate(&emu));
            "OK\n".into()
        }
        _ => "ERR unknown_cmd\n".into(),
    }
}

/// `Emu.Kill(false, true)`, then `Emu.Restart` from the state.
fn savestate(emu: &Arc<Mutex<Emu>>) {
    let stop_for = emu.lock().unwrap().stop_for;
    thread::sleep(stop_for);
    {
        let mut e = emu.lock().unwrap();
        let n = e.next_name();
        let dir = e.config_dir.join("savestates").join(TITLE);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{TITLE}_{n}.SAVESTAT.zst")), b"RPCS3SAV").unwrap();
        e.status = Status::Loading;
        e.renderer = false;
        e.frames = 0;
    }
    thread::sleep(Duration::from_millis(20));
    let mut e = emu.lock().unwrap();
    // A new RSX session: its constructor clears the capture flag and the target.
    e.renderer = true;
    e.cap_flag = false;
    e.pause_at = 0;
    e.pause_after = 2;
    e.status = Status::Running;
}

struct Fake {
    socket: PathBuf,
    emu: Arc<Mutex<Emu>>,
    config_dir: tempfile::TempDir,
}

impl Fake {
    fn control(&self) -> Control {
        Control::new(&self.socket, self.config_dir.path())
    }

    fn verbs(&self) -> VerbClient {
        VerbClient::new(&self.socket)
    }

    fn set(&self, f: impl FnOnce(&mut Emu)) {
        f(&mut self.emu.lock().unwrap());
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn spawn() -> Fake {
    let config_dir = tempfile::tempdir().unwrap();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let socket = std::env::temp_dir().join(format!("o3-{}-{n}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let emu = Arc::new(Mutex::new(Emu::new(config_dir.path())));

    let ticker = Arc::clone(&emu);
    thread::spawn(move || {
        loop {
            {
                let mut e = ticker.lock().unwrap();
                match e.status {
                    Status::Running => e.frame(),
                    // RSX's paused loop: a native-UI flip re-presents the frame.
                    Status::Paused if e.shot_flag && e.presents => {
                        e.shot_flag = false;
                        e.take_screenshot();
                    }
                    _ => {}
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
                    // P1 trims trailing CR and spaces before it splits the line.
                    let l = line.trim_end_matches(['\n', '\r', ' ']).to_owned();
                    line.clear();
                    // Pushes interleave with replies, as P1's heartbeat and P4 do.
                    let hb = emu.lock().unwrap().state_line("HB");
                    let reply = handle(&emu, &l);
                    let out = format!("{hb}PE cmd=status\n{reply}");
                    if writer.write_all(out.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    Fake {
        socket,
        emu,
        config_dir,
    }
}

fn refusal(err: anyhow::Error) -> Refusal {
    err.downcast_ref::<ReplyError>()
        .and_then(ReplyError::refusal)
        .unwrap_or_else(|| panic!("not a refusal: {err:#}"))
}

#[test]
fn with_the_gate_off_every_verb_is_an_unknown_command() {
    let fake = spawn();
    fake.set(|e| e.gate = false);
    let verbs = fake.verbs();
    for verb in [
        Verb::PauseAt(1_000_000),
        Verb::Resume,
        Verb::RsxCap,
        Verb::Screenshot,
        Verb::SaveState,
    ] {
        let err = verbs.send(verb).unwrap_err();
        assert_eq!(refusal(err), Refusal::UnknownCmd, "{verb:?}");
    }
    // P1 itself still answers, and nothing moved.
    let state = fake.control().state().unwrap();
    assert_eq!(state.status, "running");
    let e = fake.emu.lock().unwrap();
    assert_eq!((e.pause_at, e.cap_flag, e.shot_flag), (0, false, false));
}

#[test]
fn pause_at_lands_on_its_flip_and_resume_moves_on() {
    let fake = spawn();
    let control = fake.control();
    let start = control.state().unwrap().frames;
    let target = start + 40;
    let paused = control.pause_at(target, WAIT).unwrap();
    assert_eq!((paused.status.as_str(), paused.frames), ("paused", target));

    // At or behind the counter is past; nothing changes.
    for at in [target - 1, target] {
        let err = fake.verbs().send(Verb::PauseAt(at)).unwrap_err();
        assert_eq!(refusal(err), Refusal::Past, "PAUSE_AT {at}");
    }
    thread::sleep(Duration::from_millis(20));
    assert_eq!(control.state().unwrap().frames, target, "still held");

    control.resume().unwrap();
    let moved = control
        .wait_state(WAIT, |s| s.status == "running" && s.frames > target + 5)
        .unwrap();
    assert!(moved.frames > target);
    let err = fake.verbs().send(Verb::Resume).unwrap_err();
    assert_eq!(refusal(err), Refusal::NotPaused);

    // A second target from the same run lands as exactly.
    let again = moved.frames + 25;
    assert_eq!(control.pause_at(again, WAIT).unwrap().frames, again);
}

#[test]
fn pause_at_from_a_pause_sets_the_target_first_then_resumes() {
    let fake = spawn();
    let control = fake.control();
    let first = control.state().unwrap().frames + 10;
    control.pause_at(first, WAIT).unwrap();
    // A checkpoint's step: from one pause straight to the next, exactly.
    for step in [1, 7, 60] {
        let from = control.state().unwrap();
        assert_eq!(from.status, "paused");
        let next = control.pause_at(from.frames + step, WAIT).unwrap();
        assert_eq!(
            (next.status.as_str(), next.frames),
            ("paused", from.frames + step)
        );
    }
    // Refused at the pause: nothing resumed it.
    let held = control.state().unwrap();
    let err = control.pause_at(held.frames, WAIT).unwrap_err();
    assert_eq!(refusal(err), Refusal::Past);
    thread::sleep(Duration::from_millis(20));
    assert_eq!(control.state().unwrap(), held, "still held");
}

#[test]
fn a_target_the_flip_check_drops_fails_at_once() {
    let fake = spawn();
    let control = fake.control();
    fake.set(|e| e.misses_targets = true);
    let target = control.state().unwrap().frames + 20;
    let started = Instant::now();
    let err = control
        .pause_at(target, Duration::from_secs(30))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!("PAUSE_AT {target} went by without a pause")),
        "{err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited out the timeout: {:?}",
        started.elapsed()
    );
}

#[test]
fn pause_at_takes_only_a_positive_decimal() {
    let fake = spawn();
    let verbs = fake.verbs();
    for line in [
        "PAUSE_AT",
        "PAUSE_AT ",
        "PAUSE_AT 0",
        "PAUSE_AT -5",
        "PAUSE_AT +5",
        "PAUSE_AT 1e3",
        "PAUSE_AT 0x10",
        "PAUSE_AT 12 34",
        "PAUSE_AT 99999999999999999999",
    ] {
        let reply = verbs.roundtrip_line(&format!("{line}\n")).unwrap();
        assert_eq!(
            parse_reply(Verb::PauseAt(0), &reply).unwrap_err().refusal(),
            Some(Refusal::BadFlip),
            "{line:?} -> {reply:?}"
        );
    }
    assert_eq!(fake.emu.lock().unwrap().pause_at, 0);
}

#[test]
fn a_screenshot_at_a_pause_is_that_frame_every_time() {
    let fake = spawn();
    let control = fake.control();
    let out = tempfile::tempdir().unwrap();
    let at = control.state().unwrap().frames + 30;
    control.pause_at(at, WAIT).unwrap();

    let first = control.screenshot(out.path(), WAIT).unwrap();
    let second = control.screenshot(out.path(), WAIT).unwrap();
    assert_eq!((first.flip, second.flip), (at, at));
    assert_eq!(first.frame, second.frame, "one frame, one image");
    assert_ne!(first.path, second.path);
    // Copied out under its own name, the dir spelled physically.
    let out_dir = out.path().canonicalize().unwrap();
    assert!(first.path.starts_with(&out_dir) && first.path.is_file());
    assert_eq!((first.frame.width, first.frame.height), (4, 4));
    assert_eq!(
        control.state().unwrap().frames,
        at,
        "a shot doesn't move it"
    );

    // Running, the next present takes it: a later frame, another image.
    control.resume().unwrap();
    let running = control.screenshot(out.path(), WAIT).unwrap();
    assert!(running.flip > at);
    assert_ne!(running.frame.pixels_sha256, first.frame.pixels_sha256);
}

#[test]
fn a_screenshot_no_present_takes_times_out_and_one_beside_it_is_busy() {
    let fake = spawn();
    fake.set(|e| e.presents = false);
    let socket = fake.socket.clone();
    let waiting = thread::spawn(move || VerbClient::new(socket).send(Verb::Screenshot));
    // Let the first request set the flag, then ask beside it.
    thread::sleep(SHOT_WAIT / 3);
    let err = fake.verbs().send(Verb::Screenshot).unwrap_err();
    assert_eq!(refusal(err), Refusal::Busy);
    assert_eq!(
        refusal(waiting.join().unwrap().unwrap_err()),
        Refusal::Timeout
    );

    // Withdrawn: the next request is taken once presents come back.
    fake.set(|e| e.presents = true);
    assert!(matches!(
        fake.verbs().send(Verb::Screenshot).unwrap(),
        Reply::Shot { .. }
    ));
}

#[test]
fn a_capture_armed_at_a_pause_lands_a_flip_on_and_pauses_there() {
    let fake = spawn();
    let control = fake.control();
    let out = tempfile::tempdir().unwrap();
    let at = control.state().unwrap().frames + 20;
    control.pause_at(at, WAIT).unwrap();

    // Armed but not yet recording (paused): a second request is busy.
    fake.verbs().send(Verb::RsxCap).unwrap();
    let err = fake.verbs().send(Verb::RsxCap).unwrap_err();
    assert_eq!(refusal(err), Refusal::Busy);
    fake.set(|e| e.cap_flag = false);

    let captured = control.capture(out.path(), WAIT).unwrap();
    assert_eq!(
        (captured.armed_at.status.as_str(), captured.armed_at.frames),
        ("paused", at)
    );
    assert_eq!(
        (
            captured.paused_at.status.as_str(),
            captured.paused_at.frames
        ),
        ("paused", at + 1)
    );
    assert!(
        captured
            .path
            .starts_with(out.path().canonicalize().unwrap())
    );
    assert!(captured.path.to_string_lossy().ends_with("_capture.rrc.gz"));
    // The capture's pause is a pause like any other: a shot there is flip
    // n+1's frame, the one before the captured frame.
    assert_eq!(control.screenshot(out.path(), WAIT).unwrap().flip, at + 1);
    // The captured frame is the flip the pause held back, one target on.
    let presented = control.pause_at(at + 2, WAIT).unwrap();
    assert_eq!(presented.frames, at + 2);
    assert_eq!(control.screenshot(out.path(), WAIT).unwrap().flip, at + 2);
    // And it goes on from there.
    control.resume().unwrap();
    control.wait_state(WAIT, |s| s.frames > at + 5).unwrap();
}

#[test]
fn a_savestate_reboots_to_start_paused_and_a_target_dies_with_its_session() {
    let fake = spawn();
    let control = fake.control();
    let out = tempfile::tempdir().unwrap();
    let at = control.state().unwrap().frames + 20;
    control.pause_at(at, WAIT).unwrap();
    // A target the old session never reaches.
    fake.verbs().send(Verb::PauseAt(at + 50)).unwrap();
    fake.set(|e| e.stop_for = Duration::from_millis(200));

    let socket = fake.socket.clone();
    let config = fake.config_dir.path().to_path_buf();
    let out_dir = out.path().to_path_buf();
    let saving = thread::spawn(move || Control::new(&socket, &config).savestate(&out_dir, WAIT));
    // While it stops, another save has nothing to save.
    thread::sleep(Duration::from_millis(80));
    let err = fake.verbs().send(Verb::SaveState).unwrap_err();
    assert_eq!(refusal(err), Refusal::NotRunning);

    let saved = saving.join().unwrap().unwrap();
    assert_eq!(saved.saved_at.frames, at);
    assert_eq!(
        (saved.rebooted.status.as_str(), saved.rebooted.frames),
        ("paused", 2)
    );
    assert!(saved.path.starts_with(out.path().canonicalize().unwrap()));
    assert!(saved.path.to_string_lossy().ends_with(".SAVESTAT.zst"));
    control.driver().read_state().unwrap();
    assert_eq!(
        fake.emu.lock().unwrap().pause_at,
        0,
        "the old target is gone"
    );

    // The run goes on from the state.
    let reconnect = VerbClient::new(&fake.socket)
        .roundtrip_line("RECONNECT\n")
        .unwrap();
    assert_eq!(reconnect, "OK");
    control.resume().unwrap();
    let on = control.wait_state(WAIT, |s| s.frames > at + 60).unwrap();
    assert_eq!(on.status, "running", "nothing paused it at the dead target");
}

#[test]
fn a_savestate_rpcs3_turns_down_fails_at_once_with_its_reason() {
    let fake = spawn();
    let log = fake.config_dir.path().join("RPCS3.log");
    // A refusal from before this save is history, not this save's answer.
    std::fs::write(&log, REFUSED_LINE).unwrap();
    let control = fake.control().with_log(&log);
    let out = tempfile::tempdir().unwrap();
    let at = control.state().unwrap().frames + 10;
    control.pause_at(at, WAIT).unwrap();

    fake.set(|e| e.refuses_saves = true);
    let asked = Instant::now();
    let err = control
        .savestate(out.path(), Duration::from_secs(30))
        .unwrap_err()
        .to_string();
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "waited {:?} on a save RPCS3 had turned down",
        asked.elapsed()
    );
    assert!(
        err.contains("failed to lock SPU threads execution") && err.contains(&at.to_string()),
        "{err}"
    );
    let state = control.state().unwrap();
    assert_eq!((state.status.as_str(), state.frames), ("paused", at));
    assert!(std::fs::read_dir(out.path()).unwrap().next().is_none());

    // Once RPCS3 takes one, the save goes through, two refusals in the log.
    fake.set(|e| e.refuses_saves = false);
    let saved = control.savestate(out.path(), WAIT).unwrap();
    assert_eq!((saved.saved_at.frames, saved.rebooted.frames), (at, 2));
}

#[test]
fn before_rsx_is_up_the_frame_verbs_have_no_renderer() {
    let fake = spawn();
    fake.set(|e| {
        e.renderer = false;
        e.status = Status::Loading;
    });
    let verbs = fake.verbs();
    for verb in [Verb::PauseAt(10), Verb::RsxCap, Verb::Screenshot] {
        assert_eq!(
            refusal(verbs.send(verb).unwrap_err()),
            Refusal::NoRenderer,
            "{verb:?}"
        );
    }
    assert_eq!(
        refusal(verbs.send(Verb::Resume).unwrap_err()),
        Refusal::NotPaused
    );
    assert_eq!(
        refusal(verbs.send(Verb::SaveState).unwrap_err()),
        Refusal::NotRunning
    );
}
