//! What AH.20's player loop, orbs and coins needed of the VM, each rule on
//! scripts built in code: a stretch of a tick (ops between two positions of
//! a placement's own script, run natively as its tick would reach them), a
//! reference naming nothing keeping its own point, a set of engine objects
//! grown by what the host makes, a reference tag naming an object, and a
//! big orb's split by its roll.

use std::collections::HashMap;

use super::init::{actor, program_and_objects, script};
use super::*;

const FLOAT: &str = "FloatMeasurement";

fn fvar(w: &mut Writer<'_>, name: &str) -> Location {
    w.variable(name, "ValueInfo", Some(FLOAT))
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

/// Positions of the statements a test's PRESCRIPT holds after its sections.
#[derive(Default, Clone, Copy)]
struct At {
    first: u32,
    check: u32,
    looped: u32,
    call: u32,
    last: u32,
    end: u32,
}

/// A script whose PRESCRIPT, after its sections, holds: `inc a`; `if a >=
/// 2 [inc b]`; `loop 3 [inc c]`; `Twice(c)` returning into `d`; `set t =
/// system.script timer`; `inc e`; and, with `ending`, a `flow end` before
/// `inc e`. Returns the program and the statements' positions.
fn statements(ending: bool) -> (Arc<Program>, At) {
    let at = std::cell::Cell::new(At::default());
    let p = program(|w| {
        let (a, b, c, d, e) = (
            var(w, "a"),
            var(w, "b"),
            var(w, "c"),
            var(w, "d"),
            var(w, "e"),
        );
        let t = w.variable("t", "ValueInfo", Some(FLOAT));
        let x = var(w, "x");
        let out = var(w, "out");
        let def = std::cell::Cell::new(None);
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                // Twice(x): out = x + x
                let zero = w.int(0);
                let (p, d) = w.define("Twice", &[("x", zero), ("out", zero)]);
                let rx = read(w, x);
                let rx2 = read(w, x);
                let sum = w.expr(rx, Some((0, rx2)));
                w.set(&[El::Local(out)], sum);
                w.end_block(p);
                def.set(Some(d));
            },
            |w| {
                let mut s = At {
                    first: w.len() as u32,
                    ..At::default()
                };
                w.inc(&[El::Local(a)]);
                s.check = w.len() as u32;
                let two = w.int(2);
                let ck = w.check(&[El::Local(a)], 2, two);
                w.inc(&[El::Local(b)]);
                w.close(ck);
                s.looped = w.len() as u32;
                let three = w.int(3);
                let l = w.loop_value(three);
                w.inc(&[El::Local(c)]);
                w.end_loop(l);
                s.call = w.len() as u32;
                let d_def = def.get().unwrap();
                let rc = read(w, c);
                let ret = parameter(w, "out", d_def);
                call_then(w, d_def, &[("x", rc)], |w| {
                    let r = w.rhs(INT, &[El::Local(ret)]);
                    w.set(&[El::Local(d)], r);
                });
                let timer = w.rhs(FLOAT, &[El::Member("system", "script timer")]);
                w.set(&[El::Local(t)], timer);
                if ending {
                    w.flow_end();
                }
                s.last = w.len() as u32;
                w.inc(&[El::Local(e)]);
                s.end = w.len() as u32;
                at.set(s);
            },
            |_| {},
        );
    });
    (p, at.get())
}

fn int(vm: &Vm, id: PlacementId, name: &str) -> i32 {
    match vm.local(id, name) {
        Some(Value::Int(i)) => i,
        Some(Value::Float(f)) => f as i32,
        _ => 0,
    }
}

#[test]
fn a_stretch_runs_its_ops_in_the_placements_context_and_counts_no_tick() {
    let (p, at) = statements(false);
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 2);
    // Two ticks: a 2, b 1 (the second), c 6, d 12 (Twice(6)), e 2.
    assert_eq!(
        (int(&vm, id, "a"), int(&vm, id, "b"), int(&vm, id, "c")),
        (2, 1, 6)
    );
    assert_eq!((int(&vm, id, "d"), int(&vm, id, "e")), (12, 2));
    let clock = vm.active_seconds(id).unwrap();
    // The check, the loop and the call, nothing before or after them.
    let ran = vm.run_span(id, at.check, at.last, &mut Bare);
    assert_eq!(ran, Ok(true));
    assert_eq!(int(&vm, id, "a"), 2, "before the stretch: not run");
    assert_eq!(int(&vm, id, "b"), 2, "a >= 2 holds");
    assert_eq!(int(&vm, id, "c"), 9, "the loop's three passes");
    assert_eq!(int(&vm, id, "d"), 18, "the call's return read after it");
    assert_eq!(int(&vm, id, "e"), 2, "after the stretch: not run");
    // Its clock as the tick reads it: two ticks' worth, none counted.
    let t = vm.local(id, "t").and_then(|v| v.as_f32()).unwrap();
    assert!((t - 2.0 / 60.0).abs() < 1e-6, "{t}");
    assert_eq!(vm.active_seconds(id), Some(clock));
    // The macro's frame reset and the loop's state whole: a tick after it
    // runs as one would.
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!((int(&vm, id, "c"), int(&vm, id, "d")), (12, 24));
    // An empty stretch runs nothing.
    assert_eq!(vm.run_span(id, at.first, at.first, &mut Bare), Ok(true));
    assert_eq!(int(&vm, id, "a"), 3);
}

#[test]
fn a_flow_end_inside_a_stretch_ends_it_there() {
    let (p, at) = statements(true);
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
    // The tick itself ends at the flow end: e never counts.
    assert_eq!(int(&vm, id, "e"), 0);
    let ran = vm.run_span(id, at.first, at.end, &mut Bare);
    assert_eq!(ran, Ok(false), "ended by the flow end, not run to its end");
    assert_eq!(int(&vm, id, "a"), 2);
    assert_eq!(int(&vm, id, "e"), 0);
}

#[test]
fn a_stretch_cut_through_a_block_or_off_its_list_is_refused() {
    let (p, at) = statements(false);
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
    // From the loop to inside its block: the loop's block left open.
    let inside = vm.run_span(id, at.looped, at.looped + 2, &mut Bare);
    let halt = inside.expect_err("cut through the loop");
    assert!(
        matches!(&halt.reason, Reason::Unsupported(s) if s.contains("inside a block")),
        "{halt}"
    );
    assert!(vm.stopped(id).is_some(), "the placement stops with it");
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
    let len = vm.program().scripts[0].ops.len() as u32;
    assert!(vm.run_span(id, at.last, at.check, &mut Bare).is_err());
    assert!(vm.run_span(id, at.first, len + 1, &mut Bare).is_err());
    // A placement never added runs nothing.
    assert_eq!(vm.run_span(99, at.first, at.end, &mut Bare), Ok(false));
}

#[test]
fn a_stretch_in_play_mode_returns_its_stop_and_stops_its_placement() {
    let (p, at) = statements(false);
    let (mut vm, id) = run(&p, Config::new(Mode::Play), 1);
    let stop = vm.run_span(id, at.looped, at.looped + 2, &mut Bare);
    assert!(stop.is_err());
    assert!(vm.stopped(id).is_some());
    // Stopped, it runs nothing more.
    assert_eq!(vm.run_span(id, at.first, at.end, &mut Bare), Ok(false));
}

/// Answers `placementreference.remembered` with an object of its own per
/// receiver (an actor's, or a reference cell's for a reference naming
/// nothing), keeps the points written to them by `positionmeasurement.
/// distance`, and makes `actorwaypoint`s.
#[derive(Default)]
struct Points {
    /// Each remembered point asked for: the receiver it was asked on.
    asked: Vec<Option<Value>>,
    /// The reference cell each was read through ([`Access::via`]).
    via: Vec<Option<u32>>,
    /// The object handed back for each receiver, and where each was put.
    objects: HashMap<String, u64>,
    put: HashMap<u64, f32>,
    made: u64,
}

impl Host for Points {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        if at.member == MemberId::named("placementreference", "remembered") {
            self.asked.push(at.receiver);
            self.via.push(at.via);
            let n = self.objects.len() as u64 + 100;
            let h = *self
                .objects
                .entry(format!("{:?} {:?}", at.receiver, at.via))
                .or_insert(n);
            return Ok(Value::Entity(h));
        }
        if at.member == MemberId::named("positionmeasurement", "distance") {
            let Some(Value::Entity(h)) = at.receiver else {
                return Err(HostFault::Nothing);
            };
            return self
                .put
                .get(&h)
                .map(|d| Value::Float(*d))
                .ok_or(HostFault::Nothing);
        }
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        let Some(Value::Entity(h)) = at.receiver else {
            return Err(HostFault::Nothing);
        };
        self.put.insert(h, value.as_f32().unwrap_or(f32::NAN));
        Ok(())
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn make(&mut self, class: &str) -> Option<Value> {
        (class == "actorwaypoint").then(|| {
            self.made += 1;
            Value::Entity(0x9000 + self.made)
        })
    }
}

fn points_config() -> Config {
    let mut c = Config::new(Mode::Strict);
    c.members.host(
        "placementreference",
        "remembered",
        MemberSpec::new(Kind::Reference, 0.0, 0.0),
    );
    c.members.host(
        "positionmeasurement",
        "distance",
        MemberSpec::new(Kind::Float, 0.0, 0.0),
    );
    c
}

/// A reference naming nothing keeps a point of its own: its remembered
/// point is asked for on the reference itself, kept, and read back
/// through it; one naming an actor asks on the actor; any other member
/// through nothing goes nowhere.
#[test]
fn a_reference_naming_nothing_keeps_its_own_point() {
    let p = program(|w| {
        let empty = w.variable("empty", "PlacementReference", None);
        let named = w.variable("named", "PlacementReference", None);
        let (got, other) = (fvar(w, "got"), fvar(w, "other"));
        idle(
            w,
            |w| {
                setref(
                    w,
                    &[El::Local(named)],
                    &[El::Member("optoplevelbehavior", "my")],
                );
            },
            |w| {
                let four = w.float(4.0);
                w.set(
                    &[
                        El::Local(empty),
                        El::Member("placementreference", "remembered"),
                        El::Member("positionmeasurement", "distance"),
                    ],
                    four,
                );
                let back = w.rhs(
                    FLOAT,
                    &[
                        El::Local(empty),
                        El::Member("placementreference", "remembered"),
                        El::Member("positionmeasurement", "distance"),
                    ],
                );
                w.set(&[El::Local(got)], back);
                let nine = w.float(9.0);
                w.set(
                    &[
                        El::Local(named),
                        El::Member("placementreference", "remembered"),
                        El::Member("positionmeasurement", "distance"),
                    ],
                    nine,
                );
                // Any other member through the empty reference: nothing.
                let nowhere = w.rhs(
                    FLOAT,
                    &[
                        El::Local(empty),
                        El::Member("positionmeasurement", "distance"),
                    ],
                );
                w.set(&[El::Local(other)], nowhere);
            },
        );
    });
    let mut vm = Vm::new(p, points_config());
    let id = vm.add(HANDLE, "Orb", 0);
    let mut host = Points::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        vm.local(id, "got"),
        Some(Value::Float(4.0)),
        "kept and read back"
    );
    assert_eq!(vm.local(id, "other"), Some(Value::Int(0)), "never written");
    assert!(
        matches!(host.asked[0], Some(Value::Cell(_))),
        "asked on the reference: {:?}",
        host.asked
    );
    assert_eq!(
        host.asked[0], host.asked[1],
        "the same reference both times"
    );
    assert_eq!(
        host.asked[2],
        Some(Value::Entity(HANDLE)),
        "on the actor named"
    );
    assert_eq!(host.asked.len(), 3);
}

/// Two references to one actor keep two points (openspec AG.28: the
/// camera's look-from and look-at name one follower clone and sit 55 ft
/// apart): each asks on the actor through its own cell, and what one keeps
/// the other doesn't read. A member through no reference names none.
#[test]
fn two_references_to_one_actor_keep_their_own_points() {
    let p = program(|w| {
        let from = w.variable("from", "PlacementReference", None);
        let at = w.variable("at", "PlacementReference", None);
        let (a, b) = (fvar(w, "a"), fvar(w, "b"));
        idle(
            w,
            |w| {
                for r in [from, at] {
                    setref(
                        w,
                        &[El::Local(r)],
                        &[El::Member("optoplevelbehavior", "my")],
                    );
                }
            },
            |w| {
                for (r, d) in [(from, 55.0), (at, 9.0)] {
                    let d = w.float(d);
                    w.set(
                        &[
                            El::Local(r),
                            El::Member("placementreference", "remembered"),
                            El::Member("positionmeasurement", "distance"),
                        ],
                        d,
                    );
                }
                for (r, out) in [(from, a), (at, b)] {
                    let back = w.rhs(
                        FLOAT,
                        &[
                            El::Local(r),
                            El::Member("placementreference", "remembered"),
                            El::Member("positionmeasurement", "distance"),
                        ],
                    );
                    w.set(&[El::Local(out)], back);
                }
            },
        );
    });
    let mut vm = Vm::new(p, points_config());
    let id = vm.add(HANDLE, "Camera", 0);
    let mut host = Points::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        vm.local(id, "a"),
        Some(Value::Float(55.0)),
        "the look-from's"
    );
    assert_eq!(vm.local(id, "b"), Some(Value::Float(9.0)), "the look-at's");
    assert!(host.asked.iter().all(|r| *r == Some(Value::Entity(HANDLE))));
    let (f, t) = (host.via[0], host.via[1]);
    assert!(f.is_some() && t.is_some() && f != t, "{:?}", host.via);
    assert_eq!((host.via[2], host.via[3]), (f, t), "each its own again");
}

/// A stretch run apart stops alone in play mode (openspec AG.28: the
/// camera's frame in the portal manager's tick): its stop is kept and
/// logged, the placement runs on, the stretch never runs again while any
/// other runs; strict mode halts as a stretch does.
#[test]
fn a_stretch_run_apart_stops_alone_in_play_mode() {
    let (p, at) = statements(false);
    let (mut vm, id) = run(&p, Config::new(Mode::Play), 1);
    let cut = (at.looped, at.looped + 2);
    assert_eq!(vm.run_span_apart(id, cut.0, cut.1, &mut Bare), Ok(false));
    assert!(vm.stopped(id).is_none(), "the placement runs on");
    let why = vm.span_stopped(id, cut.0).expect("kept").clone();
    assert!(matches!(why.reason, Reason::Unsupported(_)), "{why}");
    assert!(vm.log().iter().any(|l| l.text == why.to_string()), "logged");
    // Skipped from then on; any other stretch, and the tick, run.
    let c = int(&vm, id, "c");
    assert_eq!(vm.run_span_apart(id, cut.0, cut.1, &mut Bare), Ok(false));
    assert_eq!(int(&vm, id, "c"), c);
    assert_eq!(vm.run_span(id, at.check, at.last, &mut Bare), Ok(true));
    assert_eq!(int(&vm, id, "c"), c + 3);
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!(int(&vm, id, "e"), 2);
    // One that runs whole runs as `run_span` does.
    assert_eq!(
        vm.run_span_apart(id, at.first, at.check, &mut Bare),
        Ok(true)
    );
    // Strict: the halt, the placement stopped, nothing kept apart.
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert!(vm.run_span_apart(id, cut.0, cut.1, &mut Bare).is_err());
    assert!(vm.stopped(id).is_some() && vm.span_stopped(id, cut.0).is_none());
}

/// A set of engine objects grows by what the host makes (an orb's landing
/// point, a new `ActorWaypoint`); a host that makes none leaves the new
/// element reading nothing.
#[test]
fn a_set_of_engine_objects_grows_by_what_the_host_makes() {
    let p = program(|w| {
        let points = w.variable("points", "ScriptSet", Some("ActorWaypoint"));
        let (n, first) = (var(w, "n"), w.variable("first", "ScriptReference", None));
        idle(
            w,
            |_| {},
            |w| {
                for _ in 0..2 {
                    w.inc(&[El::Local(points), El::Member("setvariant", "count")]);
                }
                let c = w.rhs(INT, &[El::Local(points), El::Member("setvariant", "count")]);
                w.set(&[El::Local(n)], c);
                setref(
                    w,
                    &[El::Local(first)],
                    &[El::Local(points), El::Member("setvariant", "first")],
                );
            },
        );
    });
    let mut vm = Vm::new(p.clone(), points_config());
    let id = vm.add(HANDLE, "Orbs", 0);
    let mut host = Points::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "n"), Some(Value::Int(2)));
    let set = vm.local(id, "points").unwrap();
    let elems = vm.set(vm.value(set)).unwrap().to_vec();
    assert_eq!(elems, [Value::Entity(0x9001), Value::Entity(0x9002)]);
    assert_eq!(vm.local(id, "first"), Some(Value::Entity(0x9001)));
    let mut vm = Vm::new(p, points_config());
    let id = vm.add(HANDLE, "Orbs", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    let set = vm.local(id, "points").unwrap();
    assert_eq!(vm.set(vm.value(set)).unwrap(), [Value::Null, Value::Null]);
}

/// A reference tag a placement's file gives it names an object of the
/// file (a breakable's `archetype ref`): found, it holds that object.
#[test]
fn a_reference_tag_holds_the_object_it_names() {
    let mut barrel = None;
    let mut archetype = None;
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        let kind = actor(w);
        barrel = Some(a);
        archetype = Some(kind);
        let named = w.reference(&[El::Local(kind)], None);
        w.tags(
            a,
            &[("archetype ref", "PlacementReference", "tfbActorInfo", named)],
        );
        let got = w.variable("got", "ScriptReference", None);
        script(
            w,
            "C:/tfb/Synthetic/Barrel.ai",
            |w| {
                let (f, fop) = super::init::find(
                    w,
                    "archetype ref",
                    "PlacementReference",
                    "tfbActorInfo",
                    1,
                    &[El::Member("optoplevelbehavior", "my")],
                );
                setref(w, &[El::Local(got)], &[El::Local(fop)]);
                w.close(f);
            },
            |_| {},
            |w| super::init::behaviour(w, |_| {}),
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let kind = obj.at(archetype.unwrap());
    vm.name_object(kind, 0x77);
    let id = vm.add_at(0x20, "barrel_b_intact(1)", 0, obj.at(barrel.unwrap()));
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        vm.local(id, "got"),
        Some(Value::Entity(0x77)),
        "the archetype"
    );
}

/// A big orb's first split, the original's way written in our ops: a
/// whole-number roll from 0 to 100, 50 at or under 75, 25 over it. Over
/// 1,000 seeded rolls a 25 comes exactly when the roll is over 75, between
/// 207 and 288 times (pickups' A big orb: 25 in 101, mean +- 3 sd).
#[test]
fn a_50_orbs_split_follows_its_roll() {
    let p = program(|w| {
        let (roll, worth) = (var(w, "roll"), var(w, "worth"));
        idle(
            w,
            |_| {},
            |w| {
                let r = w.literal(INT, 100, true);
                w.set(&[El::Local(roll)], r);
                let seventy_five = w.int(75);
                let c = w.check(&[El::Local(roll)], 0, seventy_five);
                let fifty = w.int(50);
                w.set(&[El::Local(worth)], fifty);
                let e = w.otherwise(c);
                let twenty_five = w.int(25);
                w.set(&[El::Local(worth)], twenty_five);
                w.close(e);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Kill", 0);
    let mut host = Rolling(Generator::seeded(20261021));
    let mut smaller = 0;
    for _ in 0..1000 {
        vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
        let roll = vm.local(id, "roll").and_then(|v| v.as_i32()).unwrap();
        let worth = vm.local(id, "worth").and_then(|v| v.as_i32()).unwrap();
        assert!((0..=100).contains(&roll));
        assert_eq!(worth, if roll > 75 { 25 } else { 50 }, "roll {roll}");
        smaller += i32::from(worth == 25);
    }
    assert!((207..=288).contains(&smaller), "{smaller}");
}
