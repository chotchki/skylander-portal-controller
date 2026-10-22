//! What AH.19's hurt chain needed of the VM, each rule on scripts built in
//! code: `slide value` over time (a slider of the pool stepped once a frame
//! before the scripts, eased by its two operands, its first frame
//! unchanged, the op's result readable as a sequence's time remaining,
//! stopped by `stop`, taken over by a script's own write, a full pool or a
//! bad time landing at once), a placement's clock counted by a native tick,
//! and a definition stub naming the definition it resolves to.

use super::*;

const FLOAT: &str = "FloatMeasurement";

fn my() -> El<'static> {
    El::Member("optoplevelbehavior", "my")
}

fn fvar(w: &mut Writer<'_>, name: &str) -> Location {
    w.variable(name, "ValueInfo", Some(FLOAT))
}

/// `slide value <lhs> to <to> over <seconds> EASE: out <out> in <in_>`,
/// its block left open: position and op.
fn slide(
    w: &mut Writer<'_>,
    lhs: &[El<'_>],
    to: Location,
    seconds: Location,
    out: Location,
    in_: Location,
) -> (usize, Location) {
    let l = w.value(lhs);
    let (p, o) = w.open(
        Class::SlideValue,
        "slide value||to|over|EASE:|out|in||slider",
    );
    w.refer(o, 0x1C, l);
    w.refer(o, 0x20, to);
    w.refer(o, 0x24, seconds);
    w.refer(o, 0x28, out);
    w.refer(o, 0x2C, in_);
    (p, o)
}

/// `set reference <lhs> = <rhs>`.
fn setref(w: &mut Writer<'_>, lhs: &[El<'_>], rhs: &[El<'_>]) {
    let l = w.value(lhs);
    let r = w.reference(rhs, None);
    let o = w.op(Class::SetReference, "set reference");
    w.refer(o, 0x1C, l);
    w.refer(o, 0x20, r);
    w.push(o);
}

/// `stop <what>`: a sequence, a sound or a slider.
fn stop(w: &mut Writer<'_>, what: &[El<'_>]) {
    let np = w.group(what, None);
    let o = w.op(Class::StopSequence, "stop");
    w.refer(o, 0x1C, np);
    w.push(o);
}

fn float(vm: &Vm, id: PlacementId, name: &str) -> f32 {
    vm.local(id, name)
        .and_then(|v| v.as_f32())
        .unwrap_or(f32::NAN)
}

/// A script whose STARTUP slides `v` from 0 to 600 over `seconds`, ease
/// out 50 and in 0 (AH.10's pushes), keeping the slider in `r` and counting
/// its block's runs in `inside`; each tick `left` and `percent` read the
/// slider's time and percent remaining, `ticks` counts, and `body` runs.
fn knock_slide(
    seconds: f32,
    body: impl FnOnce(&mut Writer<'_>, Location, Location),
) -> Arc<Program> {
    program(|w| {
        let v = fvar(w, "v");
        let r = w.variable("r", "ScriptReference", None);
        let (inside, ticks) = (var(w, "inside"), var(w, "ticks"));
        let (left, percent) = (fvar(w, "left"), fvar(w, "percent"));
        idle(
            w,
            |w| {
                let zero = w.int(0);
                w.set(&[El::Local(v)], zero);
                let to = w.int(600);
                let secs = w.float(seconds);
                let out = w.int(50);
                let in_ = w.int(0);
                let (p, o) = slide(w, &[El::Local(v)], to, secs, out, in_);
                w.inc(&[El::Local(inside)]);
                setref(w, &[El::Local(r)], &[El::Local(o)]);
                w.end_block(p);
            },
            |w| {
                w.inc(&[El::Local(ticks)]);
                let t = w.rhs(
                    FLOAT,
                    &[El::Local(r), El::Member("sequence", "time remaining")],
                );
                w.set(&[El::Local(left)], t);
                let pc = w.rhs(
                    FLOAT,
                    &[El::Local(r), El::Member("sequence", "percent remaining")],
                );
                w.set(&[El::Local(percent)], pc);
                body(w, v, r);
            },
        );
    })
}

/// A slide eases its place from the frame after it began: x² of the way at
/// AH.10's ease out 50, on its target and ended once its time is up, its
/// block run with the slider as the op's result, whose time and percent
/// remaining count down.
#[test]
fn a_slide_eases_its_place_from_the_next_frame_and_lands_on_its_target() {
    let p = knock_slide(0.125, |_, _, _| {});
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    vm.step(&mut host).unwrap();
    assert_eq!(
        vm.local(id, "inside"),
        Some(Value::Int(1)),
        "its block runs"
    );
    assert_eq!(float(&vm, id, "v"), 0.0, "no change the frame it began");
    assert_eq!(vm.sliders_running(), 1);
    assert!((float(&vm, id, "left") - 0.125).abs() < 1e-6);
    assert!((float(&vm, id, "percent") - 100.0).abs() < 1e-4);
    for k in 1..=7 {
        vm.step(&mut host).unwrap();
        let x = k as f32 * TICK_SECONDS / 0.125;
        let v = float(&vm, id, "v");
        assert!((v - 600.0 * x * x).abs() < 1e-3, "frame {k}: {v}");
        let left = float(&vm, id, "left");
        assert!(
            (left - (0.125 - k as f32 * TICK_SECONDS)).abs() < 1e-5,
            "{k}: {left}"
        );
        assert!((float(&vm, id, "percent") - 100.0 * left / 0.125).abs() < 1e-3);
    }
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "v"), Some(Value::Int(600)), "its target");
    assert_eq!(vm.sliders_running(), 0);
    assert_eq!(float(&vm, id, "left"), 0.0);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "v"), Some(Value::Int(600)), "and no further");
    assert_eq!(vm.local(id, "inside"), Some(Value::Int(1)));
}

/// A slider that ended leaves the scan (the AH.19 and AH.20 review): a
/// script beginning a two-frame slide every tick makes 300 over 300 frames,
/// and each frame's slides and each write scan only those still running,
/// not every slider the session made.
#[test]
fn an_ended_slider_leaves_the_scan() {
    let p = program(|w| {
        let v = fvar(w, "v");
        idle(
            w,
            |_| {},
            |w| {
                let to = w.int(600);
                let secs = w.float(2.0 * TICK_SECONDS);
                let (out, in_) = (w.int(0), w.int(0));
                let (p, _) = slide(w, &[El::Local(v)], to, secs, out, in_);
                w.end_block(p);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    for _ in 0..300 {
        vm.step(&mut host).unwrap();
        assert!(vm.sliders_scanned() <= 2, "{}", vm.sliders_scanned());
        assert_eq!(vm.sliders_scanned(), vm.sliders_running());
    }
}

/// [`Vm::step`] slides itself only when the embedder hasn't this frame:
/// [`Vm::slide`] first, then a step, moves a slide once.
#[test]
fn a_step_slides_once_a_frame_whoever_starts_it() {
    let p = knock_slide(0.125, |_, _, _| {});
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    vm.step(&mut host).unwrap();
    let report = vm.slide(&mut host).unwrap();
    assert_eq!(report.ticked, 1, "one slider moved");
    vm.step(&mut host).unwrap();
    let x = TICK_SECONDS / 0.125;
    assert!((float(&vm, id, "v") - 600.0 * x * x).abs() < 1e-3);
}

/// A script's own write to the place takes it over: the slide ends and the
/// write holds (AH.10: his hurt reaction writes his deceleration back to
/// 80 while a push's slide eases it, and 80 holds).
#[test]
fn a_scripts_write_to_the_place_ends_its_slide() {
    let p = knock_slide(0.125, |w, v, _| {
        let three = w.int(3);
        let ticks = var(w, "ticks");
        let at = w.check(&[El::Local(ticks)], 1, three);
        let eighty = w.int(80);
        w.set(&[El::Local(v)], eighty);
        w.end_block(at);
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(id, "v"), Some(Value::Int(80)));
    assert_eq!(vm.sliders_running(), 0);
    for _ in 0..10 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(id, "v"), Some(Value::Int(80)), "it holds");
}

/// `stop` on the slider (its reference, as the movement macros keep a
/// push's) ends it: the place holds where the last frame left it, and the
/// slider reads no time left.
#[test]
fn stop_ends_a_slide_where_it_stands() {
    let p = knock_slide(0.125, |w, _, r| {
        let three = w.int(3);
        let ticks = var(w, "ticks");
        let at = w.check(&[El::Local(ticks)], 1, three);
        stop(w, &[El::Local(r)]);
        w.end_block(at);
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    let x = 2.0 * TICK_SECONDS / 0.125;
    let held = float(&vm, id, "v");
    assert!((held - 600.0 * x * x).abs() < 1e-3, "{held}");
    assert_eq!(vm.sliders_running(), 0);
    for _ in 0..10 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(float(&vm, id, "v"), held);
    assert_eq!(float(&vm, id, "left"), 0.0, "an ended slider has none left");
}

/// A second slide on the place takes over from the first, from where it
/// stands.
#[test]
fn a_new_slide_on_the_place_takes_over() {
    let p = knock_slide(1.0, |w, v, _| {
        let three = w.int(3);
        let ticks = var(w, "ticks");
        let at = w.check(&[El::Local(ticks)], 1, three);
        let to = w.int(-100);
        let secs = w.float(0.5);
        let zero = w.int(0);
        let (p, _) = slide(w, &[El::Local(v)], to, secs, zero, zero);
        w.end_block(p);
        w.end_block(at);
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Bare;
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    let from = float(&vm, id, "v");
    let x = 2.0 * TICK_SECONDS;
    assert!((from - 600.0 * x * x).abs() < 1e-3);
    assert_eq!(vm.sliders_running(), 1, "one slide on it");
    vm.step(&mut host).unwrap();
    let k = TICK_SECONDS / 0.5;
    assert!((float(&vm, id, "v") - (from + (-100.0 - from) * k)).abs() < 1e-3);
    for _ in 0..40 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(id, "v"), Some(Value::Int(-100)));
}

/// With all of the pool's sliders running, or a time that isn't over 0, a
/// slide lands at once and skips its block.
#[test]
fn a_full_pool_or_no_time_lands_at_once_and_skips_the_block() {
    let p = knock_slide(10.0, |_, _, _| {});
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let ids: Vec<PlacementId> = (0..=SLIDERS as u64)
        .map(|k| vm.add(HANDLE + k, "Slider", 0))
        .collect();
    let mut host = Bare;
    vm.step(&mut host).unwrap();
    assert_eq!(vm.sliders_running(), SLIDERS);
    let last = *ids.last().unwrap();
    assert_eq!(vm.local(last, "v"), Some(Value::Int(600)), "at once");
    assert_eq!(vm.local(last, "inside"), None, "its block skipped");
    assert_eq!(vm.local(ids[0], "inside"), Some(Value::Int(1)));
    let p = knock_slide(0.0, |_, _, _| {});
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "v"), Some(Value::Int(600)));
    assert_eq!(vm.local(id, "inside"), None);
    assert_eq!(vm.sliders_running(), 0);
}

/// The portal manager's tick is the embedder's (decision 2's player loop),
/// so a native tick has to grow its clock as its own would: what the state
/// system's immunity reads through its `script timer local`.
#[test]
fn a_native_tick_counts_toward_a_placements_clock() {
    let p = program(|w| idle(w, |_| {}, |_| {}));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Clock", 0);
    vm.step(&mut Bare).unwrap();
    vm.count_tick(id);
    vm.count_tick(id);
    assert!((vm.active_seconds(id).unwrap() - 3.0 * TICK_SECONDS).abs() < 1e-7);
}

/// A definition stub names the definition it resolves to: a reference to
/// either is the same object (AH.19: Eruptor's file names `State - On Fire`
/// through a stub, and the Chompy's state list holds the definition).
#[test]
fn a_stub_and_its_definition_are_one_object() {
    let p = program(|w| {
        let stub = w.op(Class::DefineMacro, "State - On Fire");
        let (a, b) = (
            w.variable("a", "ScriptReference", None),
            w.variable("b", "ScriptReference", None),
        );
        let (same, differ) = (var(w, "same"), var(w, "differ"));
        let def = std::cell::Cell::new(None);
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                let (p, d) = w.define("State - On Fire", &[]);
                w.end_block(p);
                def.set(Some(d));
            },
            |_| {},
            |w| {
                let body = w.behaviour("idle");
                setref(w, &[El::Local(a)], &[El::Local(def.get().unwrap())]);
                setref(w, &[El::Local(b)], &[El::Local(stub)]);
                let l = w.value(&[El::Local(a)]);
                let r = w.reference(&[El::Local(b)], None);
                let (c, o) = w.open(Class::CheckReference, "if reference");
                w.refer(o, 0x1C, l);
                w.word(o, 0x20, 1);
                w.refer(o, 0x24, r);
                w.inc(&[El::Local(same)]);
                w.end_block(c);
                let l = w.value(&[El::Local(a)]);
                let r = w.reference(&[El::Local(b)], None);
                let (c, o) = w.open(Class::CheckReference, "if reference");
                w.refer(o, 0x1C, l);
                w.word(o, 0x20, 5);
                w.refer(o, 0x24, r);
                w.inc(&[El::Local(differ)]);
                w.end_block(c);
                w.end_block(body);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Stubs", 0);
    vm.step(&mut Bare).unwrap();
    assert_eq!(vm.local(id, "same"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "differ"), None);
}

/// A slider's own members: at half rate it goes half as far a frame;
/// looping and ping pong (the HUD's sprite shakes) it comes back the way
/// it went and never ends; `raw duration` reads its time; `ignores time
/// scales` keeps what's written; any other member of it is unmodelled.
#[test]
fn a_sliders_rate_looping_and_ping_pong() {
    let p = program(|w| {
        let v = w.variable("v", "ValueInfo", Some(FLOAT));
        let r = w.variable("r", "ScriptReference", None);
        let (raw, ignores) = (
            w.variable("raw", "ValueInfo", Some(FLOAT)),
            var(w, "ignores"),
        );
        idle(
            w,
            |w| {
                let to = w.int(60);
                let secs = w.float(1.0);
                let zero = w.int(0);
                let (p, o) = slide(w, &[El::Local(v)], to, secs, zero, zero);
                setref(w, &[El::Local(r)], &[El::Local(o)]);
                let half = w.int(50);
                w.set(
                    &[El::Local(r), El::Member("sequence", "playback rate")],
                    half,
                );
                let one = w.int(1);
                w.set(&[El::Local(r), El::Member("sequence", "looping")], one);
                let one = w.int(1);
                w.set(
                    &[El::Local(r), El::Member("sequence", "playback mode")],
                    one,
                );
                let one = w.int(1);
                w.set(
                    &[El::Local(r), El::Member("slider", "ignores time scales")],
                    one,
                );
                let d = w.rhs(
                    FLOAT,
                    &[El::Local(r), El::Member("sequence", "raw duration")],
                );
                w.set(&[El::Local(raw)], d);
                let i = w.rhs(
                    INT,
                    &[El::Local(r), El::Member("slider", "ignores time scales")],
                );
                w.set(&[El::Local(ignores)], i);
                w.end_block(p);
            },
            |_| {},
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Shake", 0);
    let mut host = Bare;
    vm.step(&mut host).unwrap();
    assert_eq!(float(&vm, id, "raw"), 1.0);
    assert_eq!(vm.local(id, "ignores"), Some(Value::Int(1)));
    vm.step(&mut host).unwrap();
    // Half rate: half a frame's share of the time, 30 a second.
    assert!((float(&vm, id, "v") - 0.5).abs() < 1e-3);
    // Two seconds out at half rate, then back the way it came.
    for _ in 0..119 {
        vm.step(&mut host).unwrap();
    }
    assert!(
        (float(&vm, id, "v") - 60.0).abs() < 0.6,
        "{}",
        float(&vm, id, "v")
    );
    for _ in 0..60 {
        vm.step(&mut host).unwrap();
    }
    assert!(
        (float(&vm, id, "v") - 30.0).abs() < 0.6,
        "{}",
        float(&vm, id, "v")
    );
    assert_eq!(vm.sliders_running(), 1, "a loop never ends on its own");
}

/// Any member of a slider but its own is unmodelled: strict mode names it.
#[test]
fn another_member_of_a_slider_is_unmodelled() {
    let p = knock_slide(1.0, |w, _, r| {
        let one = w.int(1);
        w.set(
            &[El::Local(r), El::Member("sequence", "allow restart")],
            one,
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.add(HANDLE, "Slider", 0);
    let halt = vm.step(&mut Bare).unwrap_err();
    let want = vm
        .config()
        .members
        .name(MemberId::named("sequence", "allow restart"));
    assert_eq!(halt.reason, Reason::UnmodelledMember(want), "{halt}");
}

/// A slide a native call begins ahead of the frame's slides (as the
/// embedder's player loop calls the hurt chain before the step) first moves
/// in the frame after, as one a script begins does.
#[test]
fn a_slide_a_native_call_begins_first_moves_the_frame_after() {
    let p = program(|w| {
        let def = w.op(Class::DefineMacro, "Push");
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                let at = w.push(def);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                let to = w.int(600);
                let secs = w.float(0.125);
                let (out, in_) = (w.int(50), w.int(0));
                let (p, _) = slide(w, &[my(), El::Member("test", "v")], to, secs, out, in_);
                w.end_block(p);
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                w.end_block(b);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["v"]));
    let id = vm.add(HANDLE, "Pusher", 0);
    let mut host = Bare;
    vm.step(&mut host).unwrap();
    let v = |vm: &Vm| {
        vm.value_cell(Some(Value::Entity(HANDLE)), MemberId::named("test", "v"))
            .and_then(|v| v.as_f32())
            .unwrap_or(f32::NAN)
    };
    vm.call_with(Some(id), "Push", &[], &[], &mut host).unwrap();
    assert_eq!(vm.sliders_running(), 1);
    vm.step(&mut host).unwrap();
    assert_eq!(v(&vm), 0.0, "no change the frame it began");
    vm.step(&mut host).unwrap();
    // The cell is an integer member: the eased 10.67 truncated.
    let x = TICK_SECONDS / 0.125;
    assert_eq!(v(&vm), (600.0 * x * x).trunc());
}
