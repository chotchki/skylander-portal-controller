//! AH.200: a placement taken off runs its SHUTDOWN once on the way out
//! (crate docs, The tick), on scripts built with AH.13's writer: taken off
//! by another placement, after the step's walk, in a tick that ends at its
//! SHUTDOWN, no seconds counted; taking itself off, at its own tick's end; by
//! the embedder, when it asks; one a SHUTDOWN takes off, after; and none
//! for a placement that never started or stopped. AR.22: one that started
//! going to sleep runs its SHUTDOWN the same way and starts again awake.

use super::init::{Engine, actor, behaviour, program_and_objects, script};
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

/// Nor for a held one (listed for its first tick alone): its STARTUP ran,
/// then the VM holds it, so its SHUTDOWN isn't the VM's to run when the
/// embedder takes it off.
#[test]
fn a_held_placement_taken_off_runs_no_shutdown() {
    let p = program(|w| {
        let (count, shut) = (var(w, "count"), var(w, "shut"));
        shutting(
            w,
            "C:/tfb/Synthetic/Once.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
            },
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.whitelist = Whitelist::with_first_ticks(&[], &[], &["Once.ai"]);
    let mut vm = Vm::new(p, c);
    let once = vm.add(0x10, "Once", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert_eq!(vm.held(), vec![0x10], "held after its first tick");
    assert_eq!(vm.local(once, "count"), Some(Value::Int(1)));
    assert!(vm.remove(0x10));
    assert_eq!(vm.shut_down(&mut host).unwrap().ticked, 0);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(once, "shut"), None, "held");
}

/// A script whose STARTUP counts `starts`, whose SHUTDOWN (when `shutdown`)
/// counts `shut` and notes `test.seq` in `seen`, and whose one behaviour,
/// `idle`, counts `count`; with no SHUTDOWN section when `shutdown` is
/// false.
fn sleeper(w: &mut Writer<'_>, name: &str, shutdown: bool) -> Location {
    let (starts, count, shut, seen) = (
        var(w, "starts"),
        var(w, "count"),
        var(w, "shut"),
        var(w, "seen"),
    );
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    w.inc(&[El::Local(starts)]);
    w.end_block(start);
    if shutdown {
        let (s, _) = w.open(Class::ShutDown, "SHUTDOWN");
        w.inc(&[El::Local(shut)]);
        see_seq(w, seen);
        w.end_block(s);
    }
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    w.end_block(section);
    w.end_block(pre);
    behaviour(w, |w| {
        w.inc(&[El::Local(count)]);
    });
    w.end(name)
}

/// openspec AR.22: a placement that started, put to sleep, joins the
/// shutdown queue (the original's scheduler: the combat watcher's SHUTDOWN
/// plays the first fight's stinger once its own range takes it to sleep).
/// The step's walk still ticks it once at its place, its seconds counted
/// (AH.59's and AH.195's scheduler reads: each started sleeper has that
/// frame's time added), then its SHUTDOWN runs once the walk is done (after
/// the ticker's tick that step, `seen` 3), in a tick of its own that counts
/// no seconds, its behaviour cleared; asleep it runs nothing more; woken,
/// its timer starts from 0 (the 6 placements those reads see woken again)
/// and it runs STARTUP again and its behaviour from there. One asleep from
/// its start runs no SHUTDOWN, nor one put to sleep and woken again between
/// two steps, which ticks on with its timer kept.
#[test]
fn a_started_placement_put_to_sleep_runs_its_shutdown_and_starts_again_awake() {
    let p = program(|w| {
        sleeper(w, "C:/tfb/Synthetic/Sleeper.ai", true);
        shutting(
            w,
            "C:/tfb/Synthetic/Ticker.ai",
            |_| {},
            |_| {},
            |w| {
                w.inc(&[El::Member("test", "seq")]);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let tick = vm.config().tick_seconds;
    let sleeper = vm.add(0x10, "Sleeper", 0);
    vm.add(0x20, "Ticker", 1);
    let fresh = vm.add(0x30, "Sleeper", 0);
    assert!(vm.set_asleep(0x30, true));
    let mut host = Engine::default();
    let local = |vm: &Vm, id, name: &str| vm.local(id, name);
    vm.step(&mut host).unwrap();
    assert_eq!(local(&vm, sleeper, "starts"), Some(Value::Int(1)));
    assert_eq!(vm.behaviour(sleeper), Some("idle"));
    // Asleep and awake again before a step: it never slept.
    vm.set_asleep(0x10, true);
    vm.set_asleep(0x10, false);
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 2, "the sleeper and the ticker, no shutdown");
    assert_eq!(local(&vm, sleeper, "shut"), None, "never slept a step");
    assert_eq!(local(&vm, sleeper, "starts"), Some(Value::Int(1)));
    assert_eq!(local(&vm, sleeper, "count"), Some(Value::Int(2)));
    let active = vm.active_seconds(sleeper).unwrap();
    assert!(
        (active - 2.0 * tick).abs() < 1e-6,
        "its timer kept: {active}"
    );
    vm.set_asleep(0x10, true);
    let r = vm.step(&mut host).unwrap();
    assert_eq!(
        r.ticked, 3,
        "its last tick, the ticker, then the sleeper's shutdown"
    );
    assert_eq!(
        local(&vm, sleeper, "count"),
        Some(Value::Int(3)),
        "a last tick"
    );
    assert_eq!(local(&vm, sleeper, "shut"), Some(Value::Int(1)));
    assert_eq!(
        local(&vm, sleeper, "seen"),
        Some(Value::Int(3)),
        "after the walk"
    );
    assert_eq!(vm.behaviour(sleeper), None);
    let slept = vm.active_seconds(sleeper).unwrap();
    assert!(
        (slept - 3.0 * tick).abs() < 1e-6,
        "the last tick's seconds, none for its shutdown: {slept}"
    );
    assert!(vm.asleep(sleeper) && !vm.removed(sleeper));
    for _ in 0..3 {
        assert_eq!(vm.step(&mut host).unwrap().ticked, 1, "the ticker alone");
    }
    assert_eq!(local(&vm, sleeper, "shut"), Some(Value::Int(1)), "once");
    assert_eq!(local(&vm, sleeper, "count"), Some(Value::Int(3)));
    assert_eq!(vm.active_seconds(sleeper), Some(slept), "held asleep");
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(local(&vm, sleeper, "starts"), Some(Value::Int(2)), "again");
    assert_eq!(local(&vm, sleeper, "count"), Some(Value::Int(4)));
    assert_eq!(vm.behaviour(sleeper), Some("idle"));
    let woken = vm.active_seconds(sleeper).unwrap();
    assert!((woken - tick).abs() < 1e-6, "its timer from 0: {woken}");
    assert_eq!(local(&vm, fresh, "starts"), None, "never ticked");
    assert_eq!(local(&vm, fresh, "shut"), None, "never started");
    vm.set_asleep(0x30, false);
    vm.step(&mut host).unwrap();
    assert_eq!(local(&vm, fresh, "starts"), Some(Value::Int(1)));
    assert_eq!(
        local(&vm, fresh, "count"),
        Some(Value::Int(1)),
        "its first tick"
    );
    assert_eq!(local(&vm, fresh, "shut"), None, "nothing pending");
}

/// One put to sleep started and taken off before its last tick runs its
/// SHUTDOWN once, as a taken-off one; one whose last tick takes it off runs
/// it at that tick's end, once.
#[test]
fn a_sleeper_taken_off_runs_its_shutdown_once() {
    let p = program(|w| {
        sleeper(w, "C:/tfb/Synthetic/Sleeper.ai", true);
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let id = vm.add(0x10, "Sleeper", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    vm.set_asleep(0x10, true);
    assert!(vm.remove(0x10));
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 1, "no last tick for one taken off, its shutdown");
    assert_eq!(vm.local(id, "shut"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "count"), Some(Value::Int(1)));
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "shut"), Some(Value::Int(1)), "once");
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)), "off for good");

    // Its last tick takes it off (its behaviour does once `go` is set): the
    // SHUTDOWN at that tick's end, none from the queue after.
    let p = program(|w| {
        let (count, shut, go) = (var(w, "count"), var(w, "shut"), var(w, "go"));
        shutting(
            w,
            "C:/tfb/Synthetic/Leaver.ai",
            |w| {
                w.inc(&[El::Local(shut)]);
            },
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
                let one = w.int(1);
                let at = w.check(&[El::Local(go)], 1, one);
                w.remove(&[El::Member("optoplevelbehavior", "my")]);
                w.end_block(at);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let id = vm.add(0x10, "Leaver", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert!(vm.set_local(id, "go", Value::Int(1)));
    vm.set_asleep(0x10, true);
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 1, "its last tick, its shutdown inside it");
    assert_eq!(vm.local(id, "count"), Some(Value::Int(2)));
    assert_eq!(vm.local(id, "shut"), Some(Value::Int(1)));
    assert!(vm.removed(id));
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "shut"), Some(Value::Int(1)), "once");
}

/// Nor for a held one (listed for its first tick alone) put to sleep: its
/// SHUTDOWN isn't the VM's to run.
#[test]
fn a_held_placement_put_to_sleep_runs_no_shutdown() {
    let p = program(|w| {
        sleeper(w, "C:/tfb/Synthetic/Once.ai", true);
    });
    let mut c = Config::new(Mode::Strict);
    c.whitelist = Whitelist::with_first_ticks(&[], &[], &["Once.ai"]);
    let mut vm = Vm::new(p, c);
    let once = vm.add(0x10, "Once", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert_eq!(vm.held(), vec![0x10], "held after its first tick");
    vm.set_asleep(0x10, true);
    assert_eq!(vm.step(&mut host).unwrap().ticked, 0);
    assert_eq!(vm.local(once, "shut"), None, "held");
}

/// A placement whose script has no SHUTDOWN, put to sleep once started,
/// ticks its last tick and runs nothing more as it sleeps, then starts
/// again awake: the queue clears its started flag as it does a taken-off
/// one's. Put to sleep and woken between two steps, it never slept and
/// carries on started.
#[test]
fn a_placement_with_no_shutdown_put_to_sleep_starts_again_awake() {
    let p = program(|w| {
        sleeper(w, "C:/tfb/Synthetic/Bare.ai", false);
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(0x10, "Bare", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    vm.set_asleep(0x10, true);
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)), "never slept");
    assert_eq!(vm.local(id, "count"), Some(Value::Int(2)));
    assert_eq!(vm.behaviour(id), Some("idle"));
    vm.set_asleep(0x10, true);
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 1, "its last tick, no shutdown tick");
    assert_eq!(vm.local(id, "count"), Some(Value::Int(3)));
    assert_eq!(vm.behaviour(id), None);
    assert_eq!(vm.step(&mut host).unwrap().ticked, 0, "asleep");
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(2)));
    assert_eq!(vm.local(id, "count"), Some(Value::Int(4)));
}

/// A placement suspended from outside ([`Vm::set_suspended`]) isn't ticked
/// and runs no SHUTDOWN, started as it is; let go, it carries on where it
/// was, STARTUP not again.
#[test]
fn a_suspended_placement_holds_still_and_goes_on_where_it_was() {
    let p = program(|w| {
        sleeper(w, "C:/tfb/Synthetic/Held.ai", true);
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["seq"]));
    let id = vm.add(0x10, "Held", 0);
    assert!(!vm.set_suspended(0x99, true), "no placement under it");
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert!(vm.set_suspended(0x10, true));
    assert!(vm.is_suspended(id));
    for _ in 0..3 {
        assert_eq!(vm.step(&mut host).unwrap().ticked, 0);
    }
    assert_eq!(vm.local(id, "count"), Some(Value::Int(1)), "not ticked");
    assert_eq!(vm.local(id, "shut"), None, "no SHUTDOWN");
    assert!(vm.set_suspended(0x10, false));
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)), "not again");
    assert_eq!(vm.local(id, "count"), Some(Value::Int(2)));
}

/// openspec AS.67: a placement's `reset` of itself sends it round from its
/// STARTUP. Bomb adds to a local it never sets and resets itself at 3 (level
/// 214's bomb after its burst): its SHUTDOWN runs once at that tick's end,
/// then it rejoins the list's end and ticks there that same step, behind
/// Other, its STARTUP again (`at start` reads Other's count, 0 the first
/// time, 3 the second), its local made again at 0 (1 after its tick, not
/// 4, where a kept one would reset it again at once), its timer one tick's.
/// A reset of another placement (Resetter's of Other, each tick) leaves it
/// running as it was, its local growing and its STARTUP once, and sends
/// the resetter round no more than it.
#[test]
fn a_placement_resetting_itself_goes_round_from_its_startup() {
    let mut other_obj = None;
    let (p, obj) = program_and_objects(|w| {
        let other = actor(w);
        other_obj = Some(other);
        let bounces = var(w, "bounces");
        let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
        let (start, _) = w.open(Class::StartUp, "STARTUP");
        w.inc(&[El::Member("test", "starts")]);
        let seq = w.rhs(INT, &[El::Member("test", "seq")]);
        w.set(&[El::Member("test", "at start")], seq);
        w.end_block(start);
        let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
        w.inc(&[El::Member("test", "shuts")]);
        w.end_block(shut);
        let (section, _) = w.open(Class::MacroSection, "MACROS");
        w.end_block(section);
        w.end_block(pre);
        behaviour(w, |w| {
            w.inc(&[El::Local(bounces)]);
            let three = w.literal(INT, 3, false);
            let c = w.check(&[El::Local(bounces)], 2, three);
            reset(w, &[El::Member("optoplevelbehavior", "my")]);
            w.end_block(c);
        });
        w.end("C:/tfb/Synthetic/Bomb.ai");
        let n = var(w, "n");
        script(
            w,
            "C:/tfb/Synthetic/Other.ai",
            |w| {
                w.inc(&[El::Member("test", "other starts")]);
            },
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(n)]);
                    w.inc(&[El::Member("test", "seq")]);
                });
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Resetter.ai",
            |w| {
                w.inc(&[El::Member("test", "resetter starts")]);
            },
            |_| {},
            |w| {
                behaviour(w, |w| reset(w, &[El::Local(other)]));
            },
        );
    });
    let cells = [
        "starts",
        "at start",
        "shuts",
        "seq",
        "other starts",
        "resetter starts",
    ];
    let mut vm = Vm::new(p, config(Mode::Strict, &cells));
    let bomb = vm.add(0x10, "Bomb", 0);
    let other = vm.add_at(0x20, "Other", 1, obj.at(other_obj.unwrap()));
    vm.add(0x30, "Resetter", 2);
    let mut host = Engine::default();
    for _ in 0..2 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(bomb, "bounces"), Some(Value::Int(2)));
    assert_eq!(cell(&vm, "at start"), Some(Value::Int(0)), "ahead of Other");
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 4, "Bomb, Other, Resetter, then Bomb again");
    assert_eq!(cell(&vm, "shuts"), Some(Value::Int(1)), "its SHUTDOWN once");
    assert_eq!(
        cell(&vm, "starts"),
        Some(Value::Int(2)),
        "its STARTUP again"
    );
    assert_eq!(
        cell(&vm, "at start"),
        Some(Value::Int(3)),
        "behind Other, this step"
    );
    assert_eq!(vm.local(bomb, "bounces"), Some(Value::Int(1)), "made again");
    assert_eq!(vm.active_seconds(bomb), Some(TICK_SECONDS), "from 0");
    assert_eq!(vm.behaviour(bomb), Some("idle"));
    assert!(!vm.removed(bomb));
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(bomb, "bounces"), Some(Value::Int(2)));
    assert_eq!(cell(&vm, "starts"), Some(Value::Int(2)));
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 4, "round again at 3");
    assert_eq!(cell(&vm, "starts"), Some(Value::Int(3)));
    assert_eq!(cell(&vm, "shuts"), Some(Value::Int(2)));
    assert_eq!(vm.local(bomb, "bounces"), Some(Value::Int(1)));
    assert_eq!(vm.local(other, "n"), Some(Value::Int(5)), "Other runs on");
    assert_eq!(cell(&vm, "other starts"), Some(Value::Int(1)));
    assert_eq!(
        cell(&vm, "resetter starts"),
        Some(Value::Int(1)),
        "nor is the one resetting it"
    );
}

/// The AS.67 review: a placement resetting itself in its STARTUP can't spin
/// a step. Looper's STARTUP counts and resets it; its SHUTDOWN runs at that
/// tick's end, before any behaviour, and it goes round to the list's end:
/// the step ticks it there once more (two STARTUPs in its first step), and
/// sent again after that turn it waits for the next step, then goes round
/// once a step, its behaviour never reached. Other, behind it, ticks once a
/// step throughout, and every step returns.
#[test]
fn a_placement_resetting_itself_in_its_startup_goes_round_once_a_step() {
    let p = program(|w| {
        let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
        let (start, _) = w.open(Class::StartUp, "STARTUP");
        w.inc(&[El::Member("test", "starts")]);
        reset(w, &[El::Member("optoplevelbehavior", "my")]);
        w.end_block(start);
        let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
        w.inc(&[El::Member("test", "shuts")]);
        w.end_block(shut);
        let (section, _) = w.open(Class::MacroSection, "MACROS");
        w.end_block(section);
        w.end_block(pre);
        behaviour(w, |w| {
            w.inc(&[El::Member("test", "behaved")]);
        });
        w.end("C:/tfb/Synthetic/Looper.ai");
        script(
            w,
            "C:/tfb/Synthetic/Other.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Member("test", "other")]);
                })
            },
        );
    });
    let cells = ["starts", "shuts", "behaved", "other"];
    let mut vm = Vm::new(p, config(Mode::Strict, &cells));
    let looper = vm.add(0x10, "Looper", 0);
    vm.add(0x20, "Other", 1);
    let mut host = Engine::default();
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 3, "Looper, Other, Looper again");
    assert_eq!(cell(&vm, "starts"), Some(Value::Int(2)));
    assert_eq!(cell(&vm, "shuts"), Some(Value::Int(2)));
    for n in 3..=6 {
        let r = vm.step(&mut host).unwrap();
        assert_eq!(r.ticked, 2, "Other, then Looper off the rejoin list");
        assert_eq!(cell(&vm, "starts"), Some(Value::Int(n)));
        assert_eq!(cell(&vm, "shuts"), Some(Value::Int(n)));
    }
    assert_eq!(cell(&vm, "behaved"), None, "its behaviour never reached");
    assert_eq!(cell(&vm, "other"), Some(Value::Int(5)));
    assert!(!vm.removed(looper));
}

/// The AS.67 review: a script with no SHUTDOWN goes round as well. Plain
/// resets itself from its behaviour every tick: its first tick runs its
/// STARTUP and behaviour, round it goes and ticks again behind (STARTUP and
/// behaviour once more), and sent again it waits; each step after, one
/// round.
#[test]
fn a_placement_with_no_shutdown_resetting_itself_goes_round_too() {
    let p = program(|w| {
        let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
        let (start, _) = w.open(Class::StartUp, "STARTUP");
        w.inc(&[El::Member("test", "starts")]);
        w.end_block(start);
        let (section, _) = w.open(Class::MacroSection, "MACROS");
        w.end_block(section);
        w.end_block(pre);
        behaviour(w, |w| {
            w.inc(&[El::Member("test", "behaved")]);
            reset(w, &[El::Member("optoplevelbehavior", "my")]);
        });
        w.end("C:/tfb/Synthetic/Plain.ai");
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["starts", "behaved"]));
    vm.add(0x10, "Plain", 0);
    let mut host = Engine::default();
    for n in 1..=4 {
        vm.step(&mut host).unwrap();
        assert_eq!(cell(&vm, "starts"), Some(Value::Int(n + 1)), "round at {n}");
        assert_eq!(cell(&vm, "behaved"), Some(Value::Int(n + 1)));
    }
}
