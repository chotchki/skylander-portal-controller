//! AH.200: a placement taken off runs its SHUTDOWN once on the way out
//! (crate docs, The tick), on scripts built with AH.13's writer: taken off
//! by another placement, after the step's walk, in a tick that ends at its
//! SHUTDOWN, no seconds counted; taking itself off, at its own tick's end; by
//! the embedder, when it asks; one a SHUTDOWN takes off, after; and none
//! for a placement that never started or stopped.

use super::init::{Engine, actor, behaviour, program_and_objects};
use super::*;

/// A script whose SHUTDOWN runs `shutdown`, PRESCRIPT's own ops after its
/// sections `prescript`, and one behaviour, `idle`, holding `body`.
fn shutting(
    w: &mut Writer<'_>,
    name: &str,
    shutdown: impl FnOnce(&mut Writer<'_>),
    prescript: impl FnOnce(&mut Writer<'_>),
    body: impl FnOnce(&mut Writer<'_>),
) -> Location {
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    w.end_block(start);
    let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
    shutdown(w);
    w.end_block(shut);
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    w.end_block(section);
    prescript(w);
    w.end_block(pre);
    behaviour(w, body);
    w.end(name)
}

/// `seen = test.seq`: when a SHUTDOWN ran against the ticks that count
/// `test.seq` up.
fn see_seq(w: &mut Writer<'_>, seen: Location) {
    let seq = w.rhs(INT, &[El::Member("test", "seq")]);
    w.set(&[El::Local(seen)], seq);
}

/// Taken off by another placement's `remove`, a placement that ticked runs
/// its SHUTDOWN once the step's walk is done (after the remover's tick,
/// `seen` 1), in a tick that ends there, before PRESCRIPT's own ops past
/// its sections (`pre` 1 for one tick), no seconds counted for it and
/// nothing after; the step's report counts the shutdown's tick.
#[test]
fn a_placement_taken_off_runs_its_shutdown_once_the_steps_walk_is_done() {
    let mut other_obj = None;
    let (p, obj) = program_and_objects(|w| {
        let other = actor(w);
        other_obj = Some(other);
        let (count, pre, shut, seen) = (
            var(w, "count"),
            var(w, "pre"),
            var(w, "shut"),
            var(w, "seen"),
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Other.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
                see_seq(w, seen);
            },
            |w| {
                w.inc(&[El::Local(pre)]);
            },
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Remover.ai",
            |_| {},
            |_| {},
            |w| {
                w.remove(&[El::Local(other)]);
                w.inc(&[El::Member("test", "seq")]);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let other = vm.add_at(0x30, "Other", 0, obj.at(other_obj.unwrap()));
    vm.add(0x20, "Remover", 1);
    let mut host = Engine::default();
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 3, "Other, the remover, then Other's shutdown");
    assert!(vm.removed(other));
    let local = |vm: &Vm, name: &str| vm.local(other, name);
    assert_eq!(local(&vm, "count"), Some(Value::Int(1)));
    assert_eq!(local(&vm, "shut"), Some(Value::Int(1)), "its SHUTDOWN ran");
    assert_eq!(local(&vm, "seen"), Some(Value::Int(1)), "after the remover");
    assert_eq!(
        local(&vm, "pre"),
        Some(Value::Int(1)),
        "it ends at SHUTDOWN"
    );
    assert_eq!(
        vm.active_seconds(other),
        Some(TICK_SECONDS),
        "no seconds for it"
    );
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(local(&vm, "count"), Some(Value::Int(1)));
    assert_eq!(local(&vm, "shut"), Some(Value::Int(1)), "once");
    assert_eq!(local(&vm, "pre"), Some(Value::Int(1)));
    assert_eq!(
        cell(&vm, "seq"),
        Some(Value::Int(4)),
        "the remover ticks on"
    );
}

/// A placement taking itself off finishes its tick, then runs its SHUTDOWN
/// at that tick's end (a shutdown pending: before the next placement
/// ticks, `seen` 0), and not again after the walk.
#[test]
fn a_placement_taking_itself_off_runs_its_shutdown_at_its_ticks_end() {
    let p = program(|w| {
        let (count, after, shut, seen) = (
            var(w, "count"),
            var(w, "after"),
            var(w, "shut"),
            var(w, "seen"),
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Leaver.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
                see_seq(w, seen);
            },
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
                w.remove(&[El::Member("optoplevelbehavior", "my")]);
                w.inc(&[El::Local(after)]);
            },
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Later.ai",
            |_| {},
            |_| {},
            |w| {
                w.inc(&[El::Member("test", "seq")]);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let leaver = vm.add(0x10, "Leaver", 0);
    vm.add(0x20, "Later", 1);
    let mut host = Engine::default();
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 2, "no tick of its own for the shutdown");
    let local = |vm: &Vm, name: &str| vm.local(leaver, name);
    assert_eq!(local(&vm, "count"), Some(Value::Int(1)));
    assert_eq!(local(&vm, "after"), Some(Value::Int(1)), "its tick ran on");
    assert_eq!(local(&vm, "shut"), Some(Value::Int(1)));
    assert_eq!(local(&vm, "seen"), Some(Value::Int(0)), "in its own tick");
    vm.step(&mut host).unwrap();
    assert_eq!(local(&vm, "shut"), Some(Value::Int(1)), "once");
    assert_eq!(local(&vm, "count"), Some(Value::Int(1)));
}

/// The embedder's remove leaves the SHUTDOWN to [`Vm::shut_down`] (or the
/// next step's end); a SHUTDOWN that takes another placement off runs that
/// one's after its own, in the same call, and a call with nothing taken off
/// runs nothing.
#[test]
fn what_a_shutdown_takes_off_runs_its_own_after() {
    let mut objs = Vec::new();
    let (p, obj) = program_and_objects(|w| {
        let (first, second) = (actor(w), actor(w));
        objs = vec![first, second];
        let (shut, seen) = (var(w, "shut"), var(w, "seen"));
        shutting(
            w,
            "C:/tfb/Synthetic/First.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
                w.inc(&[El::Member("test", "seq")]);
                w.remove(&[El::Local(second)]);
            },
            |_| {},
            |_| {},
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Second.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
                see_seq(w, seen);
            },
            |_| {},
            |_| {},
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let first = vm.add_at(0x10, "First", 0, obj.at(objs[0]));
    let second = vm.add_at(0x20, "Second", 1, obj.at(objs[1]));
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert!(vm.remove(0x10));
    assert_eq!(vm.local(first, "shut"), None, "left to the call");
    let r = vm.shut_down(&mut host).unwrap();
    assert_eq!(r.ticked, 2);
    assert_eq!(vm.local(first, "shut"), Some(Value::Int(1)));
    assert!(vm.removed(second), "its SHUTDOWN took the second off");
    assert_eq!(vm.local(second, "shut"), Some(Value::Int(1)));
    assert_eq!(
        vm.local(second, "seen"),
        Some(Value::Int(1)),
        "after the first's"
    );
    assert_eq!(vm.shut_down(&mut host).unwrap().ticked, 0, "nothing left");
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(first, "shut"), Some(Value::Int(1)));
    assert_eq!(vm.local(second, "shut"), Some(Value::Int(1)));
}

/// No SHUTDOWN for a placement taken off before its first tick, nor for
/// one whose script stopped in play.
#[test]
fn a_placement_that_never_started_or_stopped_runs_no_shutdown() {
    let p = program(|w| {
        let (count, shut) = (var(w, "count"), var(w, "shut"));
        shutting(
            w,
            "C:/tfb/Synthetic/Thing.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
            },
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
        shutting(
            w,
            "C:/tfb/Synthetic/Faulty.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
            },
            |_| {},
            |w| {
                let one = w.int(1);
                w.set(&[El::Member("synthetic", "unmodelled")], one);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Play));
    let fresh = vm.add(0x10, "Thing", 0);
    let faulty = vm.add(0x20, "Faulty", 1);
    assert!(vm.remove(0x10));
    let r = vm.step(&mut Bare).unwrap();
    assert_eq!(r.stopped.len(), 1, "the faulty one stops in play");
    assert!(vm.stopped(faulty).is_some());
    assert!(vm.remove(0x20));
    for _ in 0..2 {
        vm.step(&mut Bare).unwrap();
    }
    assert_eq!(vm.shut_down(&mut Bare).unwrap().ticked, 0);
    assert_eq!(vm.local(fresh, "count"), None, "never ticked");
    assert_eq!(vm.local(fresh, "shut"), None, "never started");
    assert_eq!(vm.local(faulty, "shut"), None, "stopped");
}
