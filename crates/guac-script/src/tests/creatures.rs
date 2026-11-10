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

/// A placement asleep isn't ticked. One asleep from its start never ticks;
/// one that started ticks once more the step it goes to sleep, then its
/// shutdown runs (openspec AR.22) and its locals go with it (AS.233), so
/// woken it runs STARTUP again with them made anew; one put to sleep and
/// woken between two steps never slept and carries on started, its locals
/// kept.
#[test]
fn an_asleep_placement_waits_and_wakes_anew() {
    let p = program(|w| {
        let (starts, ticks) = (var(w, "starts"), var(w, "ticks"));
        let info = idle(
            w,
            |w| {
                w.inc(&[El::Local(starts)]);
            },
            |w| {
                w.inc(&[El::Local(ticks)]);
            },
        );
        w.declare(info, &[starts, ticks]);
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
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)), "never slept");
    assert_eq!(vm.local(id, "ticks"), Some(Value::Int(3)));
    vm.set_asleep(HANDLE, true);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.behaviour(id), None, "its shutdown ran");
    assert_eq!(vm.local(id, "ticks"), None, "its locals gone with it");
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), None, "not ticked");
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "starts"), Some(Value::Int(1)), "started anew");
    assert_eq!(vm.local(id, "ticks"), Some(Value::Int(1)), "made anew");
    assert_eq!(vm.behaviour(id), Some("idle"));
}

/// openspec AS.233 (AS.224's cause): a deadline a placement holds in a
/// local, `system.script timer` plus a quarter second as a hat box's
/// `player ref timer` is, passes again the first tick its woken clock
/// passes the local made anew, not once the clock has run the seconds it
/// had run before it slept. Kept across the sleep, the deadline sat a
/// second ahead of the woken clock and nothing was picked for that second.
/// Put to sleep and woken between two steps, it never slept: deadline kept.
#[test]
fn a_woken_placements_deadline_is_made_anew_with_its_clock() {
    let p = program(|w| {
        let deadline = w.variable("deadline", "ValueInfo", Some(FLOAT));
        let picks = var(w, "picks");
        let info = idle(
            w,
            |_| {},
            |w| {
                let at = w.rhs(FLOAT, &[El::Local(deadline)]);
                let due = w.check(&[El::Member("system", "script timer")], 4, at);
                let now = w.rhs(FLOAT, &[El::Member("system", "script timer")]);
                let quarter = w.float(0.25);
                let next = w.expr(now, Some((0, quarter)));
                w.set(&[El::Local(deadline)], next);
                w.inc(&[El::Local(picks)]);
                w.end_block(due);
            },
        );
        w.declare(info, &[deadline, picks]);
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Box", 0);
    let mut host = Engine::default();
    let number = |vm: &Vm, name: &str| vm.local(id, name).and_then(|v| v.as_f32());
    for _ in 0..61 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(
        vm.local(id, "picks"),
        Some(Value::Int(4)),
        "a pick a quarter second"
    );
    let ahead = number(&vm, "deadline").unwrap();
    assert!(ahead > 1.0, "{ahead}");
    // Between two steps: never slept, its deadline kept.
    vm.set_asleep(HANDLE, true);
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    assert_eq!(number(&vm, "deadline"), Some(ahead), "never slept");
    // A real sleep: its last tick, its shutdown, its locals gone.
    vm.set_asleep(HANDLE, true);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "deadline"), None, "gone with its shutdown");
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    vm.set_asleep(HANDLE, false);
    vm.step(&mut host).unwrap();
    vm.step(&mut host).unwrap();
    assert_eq!(
        vm.local(id, "picks"),
        Some(Value::Int(1)),
        "picked its second tick awake, as at its first start"
    );
    let anew = number(&vm, "deadline").unwrap();
    assert!((anew - (TICK_SECONDS + 0.25)).abs() < 1e-5, "{anew}");
}

/// openspec AS.224, AS.233: an embedder that names the scripts it holds to
/// the rule (`AnewOnWake::Listed`, the TV's stand-in) has those sleepers'
/// declared locals made anew and every other one's kept across a sleep.
#[test]
fn only_a_listed_script_wakes_anew_when_the_embedder_lists() {
    let build = || {
        program(|w| {
            let ticks = var(w, "ticks");
            let info = idle(
                w,
                |_| {},
                |w| {
                    w.inc(&[El::Local(ticks)]);
                },
            );
            w.declare(info, &[ticks]);
        })
    };
    for (listed, after) in [(true, 1), (false, 5)] {
        let p = build();
        let name = p.scripts[0].name.clone();
        let mut config = Config::new(Mode::Strict);
        config.anew_on_wake = AnewOnWake::Listed(match listed {
            true => vec![name],
            false => vec!["/Other.ai".to_owned()],
        });
        let mut vm = Vm::new(p, config);
        let id = vm.add(HANDLE, "Box", 0);
        let mut host = Engine::default();
        for _ in 0..3 {
            vm.step(&mut host).unwrap();
        }
        vm.set_asleep(HANDLE, true);
        vm.step(&mut host).unwrap();
        vm.step(&mut host).unwrap();
        vm.set_asleep(HANDLE, false);
        vm.step(&mut host).unwrap();
        assert_eq!(
            vm.local(id, "ticks"),
            Some(Value::Int(after)),
            "listed {listed}"
        );
    }
}

/// openspec AS.233: a sleeper's shutdown lets go of the locals its script
/// declares alone. Its tags and a variable another script made on it stay,
/// as the original keeps those on the placement itself: level 207's magic
/// wall, its `setup data` made by its surface's definition once at the
/// level's start, lost it with every local and stood for its own element
/// (the au-swap-beams review's probe). Here a definer makes `setup data` on
/// the wall and writes its tag; woken, the wall's STARTUP finds both as they
/// were left, its own local made anew.
#[test]
fn a_sleeper_keeps_its_tags_and_what_others_made_on_it() {
    let mut wall = None;
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        wall = Some(a);
        let seven = w.int(7);
        w.tags(a, &[("dirt tint", "ValueInfo", INT, seven)]);
        let (own, seen, tint) = (var(w, "own"), var(w, "seen"), var(w, "tint"));
        let info = script(
            w,
            "C:/tfb/Synthetic/Wall.ai",
            |w| {
                let (f, fop) = find(w, "setup data", "ValueInfo", INT, 1, &[my()]);
                let found = w.rhs(INT, &[El::Local(fop)]);
                w.set(&[El::Local(seen)], found);
                let e = w.otherwise(f);
                let none = w.int(-1);
                w.set(&[El::Local(seen)], none);
                w.close(e);
                let (f, fop) = find(w, "dirt tint", "ValueInfo", INT, 1, &[my()]);
                let found = w.rhs(INT, &[El::Local(fop)]);
                w.set(&[El::Local(tint)], found);
                w.end_block(f);
            },
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(own)]);
                })
            },
        );
        w.declare(info, &[own, seen, tint]);
        script(
            w,
            "C:/tfb/Synthetic/Definer.ai",
            |w| {
                let (f, fop) = find(w, "setup data", "ValueInfo", INT, 2, &[El::Local(a)]);
                let five = w.int(5);
                w.set(&[El::Local(fop)], five);
                w.end_block(f);
                let (f, fop) = find(w, "dirt tint", "ValueInfo", INT, 1, &[El::Local(a)]);
                let nine = w.int(9);
                w.set(&[El::Local(fop)], nine);
                w.end_block(f);
            },
            |_| {},
            |w| behaviour(w, |_| {}),
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.add(0x30, "Definer", 1);
    let id = vm.add_at(0x20, "ElementalWallController", 0, obj.at(wall.unwrap()));
    let mut host = Engine::default();
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(id, "seen"), Some(Value::Int(5)), "made on it");
    assert_eq!(
        vm.local(id, "tint"),
        Some(Value::Int(9)),
        "its tag, written"
    );
    assert_eq!(vm.local(id, "own"), Some(Value::Int(3)));
    vm.set_asleep(0x20, true);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "own"), None, "its own let go");
    assert_eq!(vm.local(id, "setup data"), Some(Value::Int(5)), "kept");
    vm.step(&mut host).unwrap();
    vm.set_asleep(0x20, false);
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(id, "seen"), Some(Value::Int(5)), "found again");
    assert_eq!(vm.local(id, "tint"), Some(Value::Int(9)), "as written");
    assert_eq!(vm.local(id, "own"), Some(Value::Int(1)), "made anew");
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
        let info = idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(seen)], v);
                w.inc(&[El::Local(f)]);
            },
        );
        w.declare(info, &[seen]);
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
    assert_eq!(
        seen(&vm),
        [Some(Value::Int(3)), None, Some(Value::Int(5))],
        "B's last tick, at its place (openspec AR.22: C reads 5), its locals \
         gone with its shutdown (AS.233)"
    );
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm), [Some(Value::Int(6)), None, Some(Value::Int(7))]);
    vm.set_asleep(0x20, false);
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm),
        [8, 10, 9].map(|n| Some(Value::Int(n))),
        "B woken last"
    );
    vm.set_asleep(0x10, false);
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm),
        [11, 13, 12].map(|n| Some(Value::Int(n))),
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

/// A `find variable` on a clone its spawner made earlier the same turn
/// (AH.39's Mac step: the portal manager spawns the hero's death actor and,
/// the op after, finds its `dying actor` with filter 0 and sets it): the
/// clone is adopted there, as the turn's end would adopt it, so the find
/// reaches its declared local and its first tick, later that tick, reads
/// what the spawner set. Outside a step's walk (a tick the embedder runs
/// itself) nothing is adopted early: the host keeps the clone for the
/// embedder and the find finds nothing, unless the embedder asks its calls
/// to adopt (`Vm::set_adopt_in_calls`, openspec AS.200).
#[test]
fn a_find_on_a_clone_spawned_this_turn_reaches_its_declared_local() {
    let p = program(|w| {
        let given = w.variable("given", "ValueInfo", Some(INT));
        let template = actor(w);
        let (spawned, found) = (var(w, "spawned"), var(w, "found"));
        script(
            w,
            "C:/tfb/Synthetic/Spawner.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let zero = w.int(0);
                    let (c, _) = check(w, spawned, 1, zero);
                    let lhs = w.stack(StackClass::PlacementList, &[El::Local(template)]);
                    let at = w.stack(StackClass::Position, &[]);
                    let (s, so) = w.open(Class::Spawn, "clone||at|facing||cloned");
                    w.refer(so, 0x20, lhs);
                    w.refer(so, 0x28, at);
                    w.refer(so, 0x2C, zero);
                    let (f, fo) = find(w, "given", "ValueInfo", INT, 0, &[El::Local(so)]);
                    let seven = w.int(7);
                    w.set(&[El::Local(fo)], seven);
                    w.inc(&[El::Local(found)]);
                    w.end_block(f);
                    w.end_block(s);
                    w.inc(&[El::Local(spawned)]);
                    w.end_block(c);
                });
            },
        );
        let seen = var(w, "seen");
        let info = script(
            w,
            "C:/tfb/Synthetic/Child.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let r = w.rhs(INT, &[El::Local(given)]);
                    w.set(&[El::Local(seen)], r);
                });
            },
        );
        w.declare(info, &[given]);
    });
    assert_eq!(p.scripts[1].declared.len(), 1, "the child declares `given`");
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let spawner = vm.add(0x10, "Spawner", 0);
    let mut host = Spawning::default();
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(spawner, "found"), Some(Value::Int(1)), "found");
    let child = vm.placement_of(0x901).expect("adopted");
    assert_eq!(vm.local(child, "given"), Some(Value::Int(7)));
    assert_eq!(
        vm.local(child, "seen"),
        Some(Value::Int(7)),
        "its first tick, that tick, reads it"
    );
    assert_eq!(
        vm.tick_order(),
        [(false, "Spawner".to_owned()), (false, "Child1".to_owned())],
        "at the order's end"
    );
    // A tick outside a step's walk, on a VM fresh and on one past a step:
    // the clone stays the host's to hand over.
    let second = vm.add(0x11, "Spawner", 0);
    vm.tick(second, &mut host).unwrap();
    assert_eq!(
        vm.local(second, "found"),
        None,
        "nothing found after a step"
    );
    assert_eq!(vm.placement_of(0x902), None, "not adopted after a step");
    assert_eq!(host.pending.len(), 1, "kept for the embedder");
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let spawner = vm.add(0x10, "Spawner", 0);
    let mut host = Spawning::default();
    vm.tick(spawner, &mut host).unwrap();
    assert_eq!(vm.local(spawner, "found"), None, "nothing found");
    assert_eq!(vm.placement_of(0x901), None, "not adopted");
    assert_eq!(host.pending.len(), 1, "kept for the embedder");
    // An embedder that asks its calls to adopt as the walk does (openspec
    // AS.200: the TV's entry beams' manager): the same tick outside the
    // walk finds the clone's local, the clone adopted at the order's end.
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let spawner = vm.add(0x10, "Spawner", 0);
    let mut host = Spawning::default();
    vm.set_adopt_in_calls(true);
    vm.tick(spawner, &mut host).unwrap();
    vm.set_adopt_in_calls(false);
    assert_eq!(vm.local(spawner, "found"), Some(Value::Int(1)), "found");
    let child = vm.placement_of(0x901).expect("adopted");
    assert_eq!(vm.local(child, "given"), Some(Value::Int(7)));
    assert!(host.pending.is_empty(), "nothing left for the embedder");
    // Off again, the next call outside the walk leaves its clone to the
    // embedder as before.
    let second = vm.add(0x11, "Spawner", 0);
    vm.tick(second, &mut host).unwrap();
    assert_eq!(vm.local(second, "found"), None, "nothing found once off");
    assert_eq!(vm.placement_of(0x902), None, "not adopted once off");
    assert_eq!(host.pending.len(), 1, "kept for the embedder once off");
}

/// The same find from a held placement's native, the way production reaches
/// it: the player loop runs the portal manager's death sequence as a stretch
/// of its tick ([`Vm::run_span`]) where the walk reaches it, so the spawn of
/// the hero's death actor and the find the op after both run inside a
/// native. The clone is adopted there too, at the order's end, and its first
/// tick that step reads what the native's find set.
#[test]
fn a_find_from_a_held_placements_native_reaches_its_new_clones_local() {
    let span = std::cell::Cell::new((0u32, 0u32));
    let p = program(|w| {
        let given = w.variable("given", "ValueInfo", Some(INT));
        let template = actor(w);
        let (go, found) = (var(w, "go"), var(w, "found"));
        script(
            w,
            "C:/tfb/Synthetic/Spawner.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let from = w.len() as u32;
                    let one = w.int(1);
                    let (c, _) = check(w, go, 1, one);
                    let zero = w.int(0);
                    let lhs = w.stack(StackClass::PlacementList, &[El::Local(template)]);
                    let at = w.stack(StackClass::Position, &[]);
                    let (s, so) = w.open(Class::Spawn, "clone||at|facing||cloned");
                    w.refer(so, 0x20, lhs);
                    w.refer(so, 0x28, at);
                    w.refer(so, 0x2C, zero);
                    let (f, fo) = find(w, "given", "ValueInfo", INT, 0, &[El::Local(so)]);
                    let seven = w.int(7);
                    w.set(&[El::Local(fo)], seven);
                    w.inc(&[El::Local(found)]);
                    w.end_block(f);
                    w.end_block(s);
                    w.end_block(c);
                    span.set((from, w.len() as u32));
                });
            },
        );
        let seen = var(w, "seen");
        let info = script(
            w,
            "C:/tfb/Synthetic/Child.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let r = w.rhs(INT, &[El::Local(given)]);
                    w.set(&[El::Local(seen)], r);
                });
            },
        );
        w.declare(info, &[given]);
    });
    let (from, to) = span.get();
    let mut config = Config::new(Mode::Strict);
    config.whitelist =
        Whitelist::with_first_ticks(&[], &["Child.ai"], &["C:/tfb/Synthetic/Spawner.ai"]);
    let mut vm = Vm::new(p, config);
    let spawner = vm.add(0x10, "Spawner", 0);
    let mut host = Spawning::default();
    vm.step(&mut host).unwrap();
    assert_eq!(
        vm.local(spawner, "found"),
        None,
        "its first tick spawns nothing"
    );
    assert!(host.pending.is_empty());
    let mut ran = Vec::new();
    vm.step_with(&mut host, &mut |vm, id, host| {
        assert_eq!(id, spawner, "only the held spawner runs a native");
        assert!(vm.set_local(id, "go", Value::Int(1)));
        ran.push(vm.run_span(id, from, to, host)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(ran, [true], "the stretch ran to its end");
    assert_eq!(vm.local(spawner, "found"), Some(Value::Int(1)), "found");
    let child = vm.placement_of(0x901).expect("adopted in the native");
    assert_eq!(vm.local(child, "given"), Some(Value::Int(7)));
    assert_eq!(
        vm.local(child, "seen"),
        Some(Value::Int(7)),
        "its first tick, that step, reads it"
    );
    assert_eq!(
        vm.tick_order(),
        [(false, "Spawner".to_owned()), (false, "Child1".to_owned())],
        "at the order's end"
    );
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

/// openspec AS.57: an empty set is in no set, an all-check (code 0, or 3
/// with multiplicity) of one on the right skipping its block as an any-check
/// (2) does, where a set holding the right side's members passes; and a
/// membership check read as a path element is the set its left side read,
/// as a value check's is (214's tool spawner asks its next check of its
/// bomb's clones so).
#[test]
fn an_empty_set_is_in_no_set_and_a_check_reads_its_set() {
    let p = program(|w| {
        let full = w.variable("full", "ScriptSet", None);
        let empty = w.variable("empty", "ScriptSet", None);
        let names = [
            "all of empty",
            "all of full",
            "every of empty",
            "any of empty",
            "path",
        ];
        let counters: Vec<Location> = names.iter().map(|n| var(w, n)).collect();
        idle(
            w,
            |_| {},
            |w| {
                let lhs = w.stack(StackClass::Set, &[El::Local(full)]);
                let rhs = w.reference(&[my()], None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, rhs);
                w.push(o);
                for (code, right, counter) in [
                    (0, empty, counters[0]),
                    (0, full, counters[1]),
                    (3, empty, counters[2]),
                    (2, empty, counters[3]),
                ] {
                    let lhs = w.stack(StackClass::Set, &[El::Local(full)]);
                    let rhs = w.reference(&[El::Local(right)], None);
                    let (at, o) = w.open(Class::CheckMembership, "check [set]");
                    w.refer(o, 0x1C, lhs);
                    w.word(o, 0x20, code);
                    w.refer(o, 0x24, rhs);
                    w.inc(&[El::Local(counter)]);
                    w.end_block(at);
                }
                // A check whose own set the next check reads.
                let lhs = w.stack(StackClass::Set, &[El::Local(full)]);
                let rhs = w.reference(&[El::Local(empty)], None);
                let (at, first) = w.open(Class::CheckMembership, "check [set]");
                w.refer(first, 0x1C, lhs);
                w.word(first, 0x20, 0);
                w.refer(first, 0x24, rhs);
                w.end_block(at);
                let lhs = w.stack(StackClass::Set, &[El::Local(first)]);
                let rhs = w.reference(&[my()], None);
                let (at, o) = w.open(Class::CheckMembership, "check [set]");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, rhs);
                w.inc(&[El::Local(counters[4])]);
                w.end_block(at);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Spawner", 0);
    vm.tick(id, &mut Engine::default())
        .unwrap_or_else(|h| panic!("{h}"));
    let ran = |n: &str| vm.local(id, n) == Some(Value::Int(1));
    assert!(!ran("all of empty"), "an empty set isn't in the set");
    assert!(ran("all of full"), "a set is in itself");
    assert!(!ran("every of empty"));
    assert!(!ran("any of empty"));
    assert!(ran("path"), "the check reads the set it checked");
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

/// A file's reference pointed from outside is what scripts read next
/// (openspec AS.38: the Pause Controller's level switch naming the adventure
/// it enters, which guacamole's TV makes in its place); a number file
/// variable takes no reference.
#[test]
fn a_files_reference_pointed_from_outside_is_read_next() {
    let p = program(|w| {
        let f = w.b.object(0, "ScriptReference", 0x20);
        w.text(f, 0x08, "Current Adventure Title");
        let n = w.b.object(0, "ValueInfo", 0x24);
        w.text(n, 0x08, "Count");
        w.meta_word(n, 0x1C, INT);
        w.word(n, 0x20, 1);
        let got = w.variable("got", "ScriptReference", None);
        let counted = var(w, "counted");
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(n)]);
                w.set(&[El::Local(counted)], v);
                let l = w.value(&[El::Local(got)]);
                let r = w.reference(&[El::Local(f)], None);
                let o = w.op(Class::SetReference, "set reference");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
            },
        );
    });
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Pause Controller", 0);
    let title = p.global_named("Current Adventure Title").unwrap();
    let count = p.global_named("Count").unwrap();
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Null));
    assert!(vm.set_global_reference(title, Value::Entity(0x77)));
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "got"), Some(Value::Entity(0x77)));
    assert_eq!(vm.global_peek(title), Some(Value::Entity(0x77)));
    assert!(!vm.set_global_reference(count, Value::Entity(0x77)));
    assert_eq!(vm.global(count), Some(Value::Int(1)));
}
