//! What AH.18's first fight needed of the VM, each rule on scripts built in
//! code: a placement's tags, its declared locals listed once its STARTUP's
//! tick is done, placements asleep and ticking in the order they woke,
//! `find subset` measuring from its path's other point and reading a right
//! side naming `its` per member, a roll up to a path's value, `undefined`
//! holding no set, a host answering with a set of targets, and a file's
//! number or a structure's set from outside.

use std::collections::HashMap;

use super::init::{Engine, actor, behaviour, find, program_and_objects, script};
use super::*;

const FLOAT: &str = "FloatMeasurement";

fn my() -> El<'static> {
    El::Member("optoplevelbehavior", "my")
}

/// A placement's tags are its variables from the start, holding their
/// values (a Chompy's `notice player radius` 30); its own STARTUP doesn't
/// find a local its script declares (it reads its default, 10), and another
/// placement does once that STARTUP's tick is done.
#[test]
fn tags_are_found_from_the_start_and_declared_locals_after_startup() {
    let mut chompy = None;
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        chompy = Some(a);
        let thirty = w.int(30);
        w.tags(a, &[("notice player radius", "ValueInfo", INT, thirty)]);
        let declared = w.variable("communicate range", "ValueInfo", Some(INT));
        let (notice, comm, got) = (var(w, "notice"), var(w, "comm"), var(w, "got"));
        let info = script(
            w,
            "C:/tfb/Synthetic/Chompy.ai",
            |w| {
                let (f, fop) = find(w, "notice player radius", "ValueInfo", INT, 1, &[my()]);
                let found = w.rhs(INT, &[El::Local(fop)]);
                w.set(&[El::Local(notice)], found);
                let e = w.otherwise(f);
                let sixty = w.int(60);
                w.set(&[El::Local(notice)], sixty);
                w.close(e);
                let (f, fop) = find(w, "communicate range", "ValueInfo", INT, 1, &[my()]);
                let found = w.rhs(INT, &[El::Local(fop)]);
                w.set(&[El::Local(comm)], found);
                let e = w.otherwise(f);
                let ten = w.int(10);
                w.set(&[El::Local(comm)], ten);
                w.close(e);
            },
            |_| {},
            |w| behaviour(w, |_| {}),
        );
        w.declare(info, &[declared]);
        script(
            w,
            "C:/tfb/Synthetic/Finder.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let (f, _) = find(w, "communicate range", "ValueInfo", INT, 1, &[El::Local(a)]);
                    w.inc(&[El::Local(got)]);
                    w.end_block(f);
                });
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let c = vm.add_at(0x20, "Enemy_Chompy(1)", 0, obj.at(chompy.unwrap()));
    let finder = vm.add(0x30, "Finder", 1);
    vm.step(&mut Engine::default()).unwrap();
    assert_eq!(vm.local(c, "notice"), Some(Value::Int(30)), "its tag");
    assert_eq!(vm.local(c, "comm"), Some(Value::Int(10)), "not yet listed");
    assert_eq!(
        vm.local(finder, "got"),
        Some(Value::Int(1)),
        "listed once its STARTUP's tick is done"
    );
}

/// A placement asleep isn't ticked and keeps its state; woken, it carries
/// on where it was, its STARTUP run once.
#[test]
fn an_asleep_placement_waits_and_wakes_where_it_was() {
    let p = program(|w| {
        let (starts, ticks) = (var(w, "starts"), var(w, "ticks"));
        idle(
            w,
            |w| {
                w.inc(&[El::Local(starts)]);
            },
            |w| {
                w.inc(&[El::Local(ticks)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Sleeper", 0);
    assert!(vm.set_asleep(HANDLE, true));
    assert!(!vm.set_asleep(0x999, true), "no placement under it");
    let mut host = Engine::default();
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    assert!(vm.asleep(id));
    assert_eq!(vm.local(id, "starts"), None, "never ticked");
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    vm.step(&mut host).unwrap();
    vm.set_asleep(HANDLE, true);
    vm.step(&mut host).unwrap();
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "ticks"), Some(Value::Int(3)));
}

/// A structure's number set from outside is what scripts read next (the
/// combat manager's next alert put where a recorded state has it); a field
/// it doesn't have, or a value that isn't a structure, takes nothing.
#[test]
fn a_structures_number_set_from_outside_is_read_next() {
    let p = program(|w| {
        let (section, _) = w.open(Class::StructureSection, "STRUCTURES");
        let (sp, def) = w.open(Class::DefineStructure, "Control");
        let next = w.variable("alert next time", "ValueInfo", Some(INT));
        w.push(next);
        w.end_block(sp);
        w.end_block(section);
        w.end("C:/tfb/Synthetic/Types.ai");
        let b = w.variable("control", "ScriptStructure", None);
        w.refer(b, 0x24, def);
        let got = var(w, "got");
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(b), El::Local(next)]);
                w.set(&[El::Local(got)], v);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Manager", 1);
    let mut host = Engine::default();
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Int(0)));
    let control = vm.local(id, "control").unwrap();
    assert!(vm.set_field(control, "alert next time", Value::Int(7)));
    assert!(!vm.set_field(control, "no such field", Value::Int(7)));
    assert!(!vm.set_field(Value::Int(3), "alert next time", Value::Int(7)));
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Int(7)));
}

/// Placements tick in the order they woke: the order added, one woken
/// after every placement already awake, one kept awake staying put. Each
/// reads a file's counter, then counts it on.
#[test]
fn a_placement_woken_ticks_after_those_already_awake() {
    let p = program(|w| {
        let f = w.b.object(0, "ValueInfo", 0x24);
        w.text(f, 0x08, "Counter");
        w.meta_word(f, 0x1C, INT);
        w.word(f, 0x20, 0);
        let seen = var(w, "seen");
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(seen)], v);
                w.inc(&[El::Local(f)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let ids = [
        vm.add(0x10, "A", 0),
        vm.add(0x20, "B", 0),
        vm.add(0x30, "C", 0),
    ];
    let mut host = Engine::default();
    let seen = |vm: &Vm| ids.map(|id| vm.local(id, "seen"));
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm),
        [0, 1, 2].map(|n| Some(Value::Int(n))),
        "as added"
    );
    vm.set_asleep(0x20, true);
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm), [3, 1, 4].map(|n| Some(Value::Int(n))));
    vm.set_asleep(0x20, false);
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm),
        [5, 7, 6].map(|n| Some(Value::Int(n))),
        "B woken last"
    );
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm),
        [8, 10, 9].map(|n| Some(Value::Int(n))),
        "A kept awake"
    );
}

/// A host that spawns `Child` placements running script 1 and hands them
/// over through [`Host::adopt`] (AH.54).
#[derive(Default)]
struct Spawning {
    inner: Engine,
    next: u64,
    pending: Vec<(Handle, String, crate::ScriptId)>,
}

impl Host for Spawning {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.inner.get(at)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.inner.set(at, value)
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.inner.reset(at)
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        if call.class != Class::Spawn {
            return self.inner.op(call);
        }
        self.next += 1;
        let h = 0x900 + self.next;
        self.pending.push((h, format!("Child{}", self.next), 1));
        let e = Value::Entity(h);
        Ok(Answer::Run {
            receiver: Some(e),
            result: Some(e),
        })
    }
    fn adopt(&mut self, _: &Program) -> Vec<(Handle, String, crate::ScriptId)> {
        std::mem::take(&mut self.pending)
    }
}

/// What a placement's tick spawns with a script of its own ticks in the
/// same tick, after every placement already in the order (AH.54: the
/// eruption's lava is out at the eruption's first record), then keeps its
/// place in the order; a host that doesn't hand it over leaves it to the
/// embedder, untouched this tick. Each reads a file's counter, then counts
/// it on; the spawner spawns once.
#[test]
fn a_placement_spawned_with_a_script_ticks_the_tick_it_was_spawned() {
    let p = program(|w| {
        let f = w.b.object(0, "ValueInfo", 0x24);
        w.text(f, 0x08, "Counter");
        w.meta_word(f, 0x1C, INT);
        w.word(f, 0x20, 0);
        let seen = var(w, "seen");
        let spawned = var(w, "spawned");
        let template = actor(w);
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(seen)], v);
                w.inc(&[El::Local(f)]);
                let zero = w.int(0);
                let (c, _) = check(w, spawned, 1, zero);
                let lhs = w.stack(StackClass::PlacementList, &[El::Local(template)]);
                let at = w.stack(StackClass::Position, &[]);
                let (s, so) = w.open(Class::Spawn, "clone||at|facing||cloned");
                w.refer(so, 0x20, lhs);
                w.refer(so, 0x28, at);
                w.refer(so, 0x2C, zero);
                w.end_block(s);
                w.inc(&[El::Local(spawned)]);
                w.end_block(c);
            },
        );
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(seen)], v);
                w.inc(&[El::Local(f)]);
            },
        );
    });
    let run = |host: &mut dyn Host| {
        let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
        let spawner = vm.add(0x10, "Spawner", 0);
        let other = vm.add(0x20, "Other", 1);
        vm.step(host).unwrap();
        let first = (vm.local(spawner, "seen"), vm.local(other, "seen"));
        let child = vm.placement_of(0x901);
        let child_first = child.and_then(|c| vm.local(c, "seen"));
        vm.step(host).unwrap();
        let second = [spawner, other].map(|id| vm.local(id, "seen"));
        (
            first,
            child_first,
            second,
            child.and_then(|c| vm.local(c, "seen")),
        )
    };
    let (first, child, second, child_second) = run(&mut Spawning::default());
    assert_eq!(first, (Some(Value::Int(0)), Some(Value::Int(1))));
    assert_eq!(child, Some(Value::Int(2)), "the child's first tick, last");
    assert_eq!(second, [Some(Value::Int(3)), Some(Value::Int(4))]);
    assert_eq!(child_second, Some(Value::Int(5)), "then in its place");
    // A host that keeps its spawns: nothing more ticks this tick.
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let spawner = vm.add(0x10, "Spawner", 0);
    vm.step(&mut Engine::default()).unwrap();
    assert_eq!(vm.local(spawner, "seen"), Some(Value::Int(0)));
    assert_eq!(vm.placement_of(0x1001), None, "not added");
}

/// Three things at 10, 20 and 30 the host keeps as `test.items`; each one's
/// `test.distance` from the path's other point is how far apart their
/// numbers are, `test.size` its number and `test.limit` 15, 25 and 5.
struct Ruler;

impl Ruler {
    fn number(v: Option<Value>) -> Option<f32> {
        match v? {
            Value::Entity(h) => Some(h as f32),
            _ => None,
        }
    }
}

impl Host for Ruler {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        let me = Ruler::number(at.receiver).ok_or(HostFault::Nothing)?;
        if at.member == MemberId::named("test", "distance") {
            let from = Ruler::number(at.other_point).ok_or(HostFault::Nothing)?;
            return Ok(Value::Float((me - from).abs()));
        }
        if at.member == MemberId::named("test", "size") {
            return Ok(Value::Float(me));
        }
        if at.member == MemberId::named("test", "limit") {
            return Ok(Value::Float(match me as u32 {
                10 => 15.0,
                20 => 25.0,
                _ => 5.0,
            }));
        }
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Ok(())
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn elements(&mut self, _: &Access) -> Result<Vec<Value>, HostFault> {
        Ok(vec![
            Value::Entity(10),
            Value::Entity(20),
            Value::Entity(30),
        ])
    }
}

fn ruler_config() -> Config {
    let mut c = Config::new(Mode::Strict);
    c.members
        .host("test", "items", MemberSpec::new(Kind::Set, 0.0, 0.0))
        .host("test", "distance", MemberSpec::new(Kind::Float, 0.0, 0.0))
        .host("test", "size", MemberSpec::new(Kind::Float, 0.0, 0.0))
        .host("test", "limit", MemberSpec::new(Kind::Float, 0.0, 0.0));
    c
}

/// `find subset` measures each member from its path's other point (the
/// Chompy's 7 ft rule: `G:Minions.null.vertical offset.@Enemy_Chompy(1)`),
/// and reads a right side naming `its` anew for each member even inside an
/// expression (`Alert Enemies`' `its.$attention parameters.$notice player
/// radius`).
#[test]
fn find_subset_measures_from_its_other_point_and_reads_its_per_member() {
    let p = program(|w| {
        let (near, over) = (var(w, "near"), var(w, "over"));
        idle(
            w,
            |_| {},
            |w| {
                // Within 5 of my 18: only 20.
                let path = w.stack(
                    StackClass::Value,
                    &[
                        El::Member("test", "items"),
                        El::Null,
                        El::Member("test", "distance"),
                    ],
                );
                let from = w.reference(&[my()], None);
                w.refer(path, 0x18, from);
                let five = w.int(5);
                let (at, fs) = w.open(Class::FindSubSet, "find subset|of||||subset");
                w.refer(fs, 0x1C, path);
                w.word(fs, 0x20, 0);
                w.refer(fs, 0x24, five);
                let n = w.rhs(INT, &[El::Local(fs), El::Member("setvariant", "count")]);
                w.set(&[El::Local(near)], n);
                w.end_block(at);
                // Bigger than its own limit: only 30 (5); read once from the
                // first member (15) it'd be 20 and 30.
                let path = w.stack(
                    StackClass::Value,
                    &[
                        El::Member("test", "items"),
                        El::Null,
                        El::Member("test", "size"),
                    ],
                );
                let limit = w.rhs(
                    FLOAT,
                    &[
                        El::Member("abstractscriptgroup", "its"),
                        El::Member("test", "limit"),
                    ],
                );
                let wrapped = w.expr(limit, None);
                let (at, fs) = w.open(Class::FindSubSet, "find subset|of||||subset");
                w.refer(fs, 0x1C, path);
                w.word(fs, 0x20, 4);
                w.refer(fs, 0x24, wrapped);
                let n = w.rhs(INT, &[El::Local(fs), El::Member("setvariant", "count")]);
                w.set(&[El::Local(over)], n);
                w.end_block(at);
            },
        );
    });
    let mut vm = Vm::new(p, ruler_config());
    let id = vm.add(18, "Chompy", 0);
    vm.tick(id, &mut Ruler).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "near"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "over"), Some(Value::Int(1)));
}

/// Draws whatever the host hands out, in turn.
struct Dice(Vec<u32>);

impl Host for Dice {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Ok(())
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        self.0.remove(0)
    }
}

/// `random(X)` over a path rolls up to the path's value with each read,
/// both ends in (the Chompy's bite: `Enemy_Chompy_DirectDMG +
/// random(Enemy_DMG_Rand_Small)`, 11 to 14).
#[test]
fn a_roll_up_to_a_paths_value_draws_each_read() {
    let p = program(|w| {
        let (top, bite) = (var(w, "top"), var(w, "bite"));
        idle(
            w,
            |w| {
                let three = w.int(3);
                w.set(&[El::Local(top)], three);
            },
            |w| {
                let roll = w.rhs(INT, &[El::Local(top)]);
                w.b.put_bytes(roll.offset_by(0x24).unwrap(), &[1]);
                let eleven = w.int(11);
                let sum = w.expr(eleven, Some((0, roll)));
                w.set(&[El::Local(bite)], sum);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Chompy", 0);
    // (word & 0x7fffffff) mod 4: 0, 3, 1, 2.
    let mut dice = Dice(vec![4, 7, 5, 0x8000_0006]);
    let mut bites = Vec::new();
    for _ in 0..4 {
        vm.tick(id, &mut dice).unwrap_or_else(|h| panic!("{h}"));
        bites.push(vm.local(id, "bite").unwrap());
    }
    assert_eq!(bites, [11, 14, 12, 13].map(Value::Int));
    assert!(dice.0.is_empty(), "one draw a read");
}

/// `random(X)` over a path holding a fraction rolls a fraction up to it,
/// whatever its literal's class: an integer literal over 0.5 (the coins'
/// sparkle wait, `random(treasure sparkle spawn rate time)`: AG.176) reads
/// 0.5 x the number x 2^-32, never the whole roll of 0.
#[test]
fn a_roll_up_to_a_fraction_is_a_fraction_whatever_its_literal() {
    let p = program(|w| {
        let (top, wait) = (var(w, "top"), var(w, "wait"));
        idle(
            w,
            |w| {
                let half = w.float(0.5);
                w.set(&[El::Local(top)], half);
            },
            |w| {
                let roll = w.rhs(INT, &[El::Local(top)]);
                w.b.put_bytes(roll.offset_by(0x24).unwrap(), &[1]);
                w.set(&[El::Local(wait)], roll);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Coin", 0);
    let words = [0x8000_0000u32, 0x2000_0000, 0xFFFF_FFC0];
    let mut dice = Dice(words.to_vec());
    for w in words {
        vm.tick(id, &mut dice).unwrap_or_else(|h| panic!("{h}"));
        assert_eq!(
            vm.local(id, "wait"),
            Some(Value::Float(0.5 * (w as f32 * (1.0 / 4_294_967_296.0))))
        );
    }
    assert_eq!(vm.local(id, "wait"), Some(Value::Float(0.5)));
    assert!(dice.0.is_empty(), "one draw a read");
}

/// `undefined` holds no set: a change into it goes nowhere and a membership
/// check of it skips its block, as its writes do (the Chompy's STARTUP adds
/// itself to a set its file names so).
#[test]
fn a_set_change_into_undefined_goes_nowhere() {
    let p = program(|w| {
        let (after, inside) = (var(w, "after"), var(w, "inside"));
        idle(
            w,
            |_| {},
            |w| {
                let undefined = [El::Member("abstractscriptgroup", "undefined")];
                let lhs = w.stack(StackClass::Set, &undefined);
                let rhs = w.reference(&[my()], None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, rhs);
                w.push(o);
                let lhs = w.stack(StackClass::Set, &undefined);
                let rhs = w.reference(&[my()], None);
                let (at, o) = w.open(Class::CheckMembership, "check [set]");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, rhs);
                w.inc(&[El::Local(inside)]);
                w.end_block(at);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Chompy", 0);
    vm.tick(id, &mut Engine::default())
        .unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "inside"), None);
}

/// Answers a field-of-view check with targets from its `_LHS` set: the
/// members it was handed past the first, or none.
#[derive(Default)]
struct Watcher {
    handed: Vec<Vec<Value>>,
    none: bool,
}

impl Host for Watcher {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Ok(())
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        let set: HashMap<&str, Vec<Value>> = call.sets.iter().cloned().collect();
        let members = set.get("_LHS").cloned().unwrap_or_default();
        self.handed.push(members.clone());
        Ok(if self.none {
            Answer::Found(Vec::new())
        } else {
            Answer::Found(members.into_iter().skip(1).collect())
        })
    }
}

/// A world op's set operand reaches the host as its members, and a host's
/// targets run the block, the op reading as a set of them; none skips it.
#[test]
fn a_hosts_targets_run_the_block_as_a_set() {
    let p = program(|w| {
        let watched = w.variable("watched", "ScriptSet", Some("ValueInfo"));
        let (seen, ran) = (var(w, "seen"), var(w, "ran"));
        idle(
            w,
            |w| {
                for n in [4, 5, 6] {
                    let lhs = w.stack(StackClass::Set, &[El::Local(watched)]);
                    let rhs = w.int(n);
                    let o = w.op(Class::ChangeMembership, "set [set]||to");
                    w.refer(o, 0x1C, lhs);
                    w.word(o, 0x20, 4);
                    w.refer(o, 0x24, rhs);
                    w.push(o);
                }
            },
            |w| {
                let targets = w.group(&[El::Local(watched)], None);
                let (at, fov) = w.open(Class::CheckFov, "check fov");
                w.refer(fov, 0x30, targets);
                let n = w.rhs(INT, &[El::Local(fov), El::Member("setvariant", "count")]);
                w.set(&[El::Local(seen)], n);
                w.inc(&[El::Local(ran)]);
                w.end_block(at);
            },
        );
    });
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Norticus", 0);
    let mut host = Watcher::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(host.handed, [[4, 5, 6].map(Value::Int).to_vec()]);
    assert_eq!(vm.local(id, "seen"), Some(Value::Int(2)));
    assert_eq!(vm.local(id, "ran"), Some(Value::Int(1)));
    host.none = true;
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "ran"), Some(Value::Int(1)), "none skips");
}

/// A file's number set from outside is what scripts read next (how a test
/// stands in for a script off the whitelist: the cutscene's end).
#[test]
fn a_files_number_set_from_outside_is_read_next() {
    let p = program(|w| {
        let f = w.b.object(0, "ValueInfo", 0x24);
        w.text(f, 0x08, "Cutscene State");
        w.meta_word(f, 0x1C, INT);
        w.word(f, 0x20, 1);
        let got = var(w, "got");
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(got)], v);
            },
        );
    });
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Norticus", 0);
    let g = p.global_named("Cutscene State").unwrap();
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Int(1)));
    assert!(vm.set_global(g, Value::Int(0)));
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Int(0)));
}
