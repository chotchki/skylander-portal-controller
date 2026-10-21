//! What AH.16's init chain needed of the VM, each rule on scripts built in
//! code: `find variable` over a placement's locals and other owners, a
//! placement taken off, a placement's file object reading as its handle,
//! packed screen positions and colours, a structure's clone, first-tick
//! listings, the double-precision perturbation, the tick's clock, the
//! engine's sets, resets, slides, `undefined` as an object, member counts,
//! a placement seeded from outside and the master variable list.

use std::collections::HashMap;

use super::*;

const FLOAT: &str = "FloatMeasurement";

/// A script with STARTUP, an empty SHUTDOWN and MACROS, PRESCRIPT's own
/// ops and its behaviours; returns its info for [`Writer::declare`].
pub(super) fn script(
    w: &mut Writer<'_>,
    name: &str,
    startup: impl FnOnce(&mut Writer<'_>),
    prescript: impl FnOnce(&mut Writer<'_>),
    behaviours: impl FnOnce(&mut Writer<'_>),
) -> Location {
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    startup(w);
    w.end_block(start);
    let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
    w.end_block(shut);
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    w.end_block(section);
    prescript(w);
    w.end_block(pre);
    behaviours(w);
    w.end(name)
}

/// One behaviour, `idle`, holding `body`.
pub(super) fn behaviour(w: &mut Writer<'_>, body: impl FnOnce(&mut Writer<'_>)) {
    let b = w.behaviour("idle");
    body(w);
    w.end_block(b);
}

/// `find variable <name>` of `container` (`contents`) with `filter`, owned
/// by what `owner` names; its block left open.
pub(super) fn find(
    w: &mut Writer<'_>,
    name: &str,
    container: &str,
    contents: &str,
    filter: u32,
    owner: &[El<'_>],
) -> (usize, Location) {
    let owner = w.group(owner, None);
    let (p, o) = w.open(Class::FindVariable, "find variable|||owned by");
    w.class_word(o, 0x1C, container);
    w.meta_word(o, 0x20, contents);
    w.text(o, 0x24, name);
    w.word(o, 0x28, filter);
    w.refer(o, 0x30, owner);
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

/// `if reference <lhs> <relation> <rhs>`, its block left open.
fn check_ref(w: &mut Writer<'_>, lhs: &[El<'_>], relation: u32, rhs: &[El<'_>]) -> usize {
    let l = w.value(lhs);
    let r = w.reference(rhs, None);
    let (p, o) = w.open(Class::CheckReference, "if reference");
    w.refer(o, 0x1C, l);
    w.word(o, 0x20, relation);
    w.refer(o, 0x24, r);
    p
}

/// `tell <what>`, its block left open.
fn tell(w: &mut Writer<'_>, what: &[El<'_>]) -> usize {
    let lhs = w.group(what, None);
    let (p, o) = w.open(Class::Control, "control||||controlled");
    w.refer(o, 0x20, lhs);
    p
}

/// A plain object of the file standing for a placement (an actor).
pub(super) fn actor(w: &mut Writer<'_>) -> Location {
    w.b.object(0, "tfbActorInfo", 0xF8)
}

/// [`program`], and how to name the file's objects: each written object's
/// [`Obj`] by where it was written.
pub(super) fn program_and_objects(build: impl FnOnce(&mut Writer<'_>)) -> (Arc<Program>, Objects) {
    let mut b = IgzBuilder::new();
    build(&mut Writer::new(&mut b));
    let bytes = b.build();
    let igz = Igz::parse(&bytes).expect("parse");
    let graph = Graph::read(&igz).expect("graph");
    let unit = Unit {
        name: "test",
        role: Role::Other,
        igz: &igz,
        graph: &graph,
    };
    let p = Arc::new(Program::link(&[unit]).expect("link"));
    (p, Objects(igz.objects().iter().map(|o| o.at).collect()))
}

/// Where each of a file's objects was written, in index order.
pub(super) struct Objects(Vec<Location>);

impl Objects {
    pub(super) fn at(&self, l: Location) -> Obj {
        Obj {
            unit: 0,
            index: self
                .0
                .iter()
                .position(|&a| a == l)
                .expect("an object written") as u32,
        }
    }
}

/// Answers tells (every object, as a placement the host keeps), spawns
/// (a fresh handle), removals, resets and stops; keeps `test` members in
/// a map and the engine set `test.items`.
#[derive(Default)]
pub(super) struct Engine {
    next: u64,
    cells: HashMap<(String, MemberId), Value>,
    ops: Vec<(Class, Vec<Option<Value>>)>,
    /// Each op's receiver, in order.
    receivers: Vec<Value>,
    items: Vec<Value>,
}

impl Host for Engine {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        Ok(*self
            .cells
            .get(&(format!("{:?}", at.receiver), at.member))
            .unwrap_or(&Value::Int(0)))
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.cells
            .insert((format!("{:?}", at.receiver), at.member), value);
        Ok(())
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.cells
            .remove(&(format!("{:?}", at.receiver), at.member));
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        self.ops
            .push((call.class, call.operands.iter().map(|(_, v)| *v).collect()));
        self.receivers.push(call.receiver);
        let first = call.operands.first().and_then(|(_, v)| *v);
        Ok(match call.class {
            Class::Control => match first {
                Some(v @ (Value::Entity(_) | Value::Object(_))) => Answer::Run {
                    receiver: Some(v),
                    result: None,
                },
                _ => Answer::Skip,
            },
            Class::Spawn => {
                self.next += 1;
                let e = Value::Entity(0x1000 + self.next);
                Answer::Run {
                    receiver: Some(e),
                    result: Some(e),
                }
            }
            _ => Answer::Done,
        })
    }
    fn elements(&mut self, _: &Access) -> Result<Vec<Value>, HostFault> {
        Ok(self.items.clone())
    }
}

/// Filter 0 finds a declared local on a placement whose STARTUP hasn't run
/// yet: asking for its list makes it (the portal manager's STARTUP finds the
/// portal control's `portal data` so, ticking before it).
#[test]
fn find_variable_reaches_a_declared_local_before_its_placements_startup() {
    let mut owner_obj = None;
    let (p, obj) = program_and_objects(|w| {
        let shared = w.variable("shared", "ValueInfo", Some(INT));
        let other = w.variable("other", "ValueInfo", Some(INT));
        let spare = w.variable("spare", "ValueInfo", Some(INT));
        let a = actor(w);
        owner_obj = Some(a);
        // The finder, added first: its tick runs before the owner's.
        let (seen, took) = (var(w, "seen"), var(w, "took"));
        script(
            w,
            "C:/tfb/Synthetic/Finder.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let (f, fo) = find(w, "shared", "ValueInfo", INT, 0, &[El::Local(a)]);
                    w.inc(&[El::Local(fo)]);
                    let r = w.rhs(INT, &[El::Local(fo)]);
                    w.set(&[El::Local(seen)], r);
                    w.end_block(f);
                    // The same name of another container isn't the one
                    // declared.
                    let (g, _) = find(w, "shared", "ScriptSet", INT, 0, &[El::Local(a)]);
                    w.inc(&[El::Member("test", "wrong kind")]);
                    w.end_block(g);
                    // Taken off: a declared local not made yet is made
                    // for the block and gone after it.
                    let (h, _) = find(w, "spare", "ValueInfo", INT, 3, &[El::Local(a)]);
                    w.inc(&[El::Local(took)]);
                    w.end_block(h);
                });
            },
        );
        let info = script(
            w,
            "C:/tfb/Synthetic/Owner.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(other)]);
                });
            },
        );
        w.declare(info, &[shared, other, spare]);
    });
    let a = obj.at(owner_obj.unwrap());
    assert_eq!(
        p.scripts[1].declared.len(),
        3,
        "the master list's variables"
    );
    let mut c = Config::new(Mode::Strict);
    c.members.value_cell("test", "wrong kind", int_cell());
    let mut vm = Vm::new(p.clone(), c);
    let finder = vm.add(0x10, "Finder", 0);
    let owner = vm.add_at(0x20, "Owner", 1, a);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    // The finder ticks first: filter 0 made the owner's list.
    assert_eq!(vm.local(owner, "shared"), Some(Value::Int(1)));
    assert_eq!(vm.local(finder, "seen"), Some(Value::Int(1)));
    assert_eq!(vm.local(finder, "took"), Some(Value::Int(1)));
    assert_eq!(vm.local(owner, "spare"), None, "taken off");
    assert_eq!(
        vm.local(owner, "other"),
        Some(Value::Int(1)),
        "its STARTUP kept it"
    );
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(owner, "shared"), Some(Value::Int(2)));
    assert_eq!(vm.local(finder, "seen"), Some(Value::Int(2)));
    assert_eq!(vm.local(finder, "took"), Some(Value::Int(2)));
    assert_eq!(
        vm.value_cell(None, MemberId::named("test", "wrong kind")),
        None,
        "a set of that name isn't declared"
    );
}

#[test]
fn find_variable_makes_finds_and_takes_off_on_an_owner_the_vm_doesnt_run() {
    let p = program(|w| {
        let thing = actor(w);
        let (copy, taken, after) = (var(w, "copy"), var(w, "taken"), var(w, "after"));
        idle(
            w,
            |_| {},
            |w| {
                let (f, fo) = find(w, "made", "ValueInfo", INT, 2, &[El::Local(thing)]);
                let four = w.int(4);
                w.set(&[El::Local(fo)], four);
                w.end_block(f);
                let (f, fo) = find(w, "made", "ValueInfo", INT, 0, &[El::Local(thing)]);
                let r = w.rhs(INT, &[El::Local(fo)]);
                w.set(&[El::Local(copy)], r);
                w.end_block(f);
                let (f, fo) = find(w, "made", "ValueInfo", INT, 3, &[El::Local(thing)]);
                let r = w.rhs(INT, &[El::Local(fo)]);
                w.set(&[El::Local(taken)], r);
                w.end_block(f);
                let (f, _) = find(w, "made", "ValueInfo", INT, 0, &[El::Local(thing)]);
                let one = w.int(1);
                w.set(&[El::Local(after)], one);
                w.end_block(f);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "copy"), Some(Value::Int(4)));
    assert_eq!(vm.local(id, "taken"), Some(Value::Int(4)));
    assert_eq!(vm.local(id, "after"), None, "taken off: found no more");
}

#[test]
fn find_variable_with_an_unmodelled_filter_stops_naming_it() {
    let p = program(|w| {
        idle(
            w,
            |_| {},
            |w| {
                let (f, _) = find(
                    w,
                    "x",
                    "ValueInfo",
                    INT,
                    4,
                    &[El::Member("optoplevelbehavior", "my")],
                );
                w.end_block(f);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    let halt = vm.tick(id, &mut Engine::default()).unwrap_err();
    assert_eq!(
        halt.reason,
        Reason::Unsupported("find variable filter 4".into())
    );
}

/// Filter 1 finds only in a list its owner already has: nothing on a
/// placement before its STARTUP (or a filter-0 find) made one, then its
/// declared local.
#[test]
fn find_variable_filter_1_finds_only_in_a_list_already_made() {
    let mut owner_obj = None;
    let (p, obj) = program_and_objects(|w| {
        let shared = w.variable("shared", "ValueInfo", Some(INT));
        let a = actor(w);
        owner_obj = Some(a);
        let seen = var(w, "seen");
        script(
            w,
            "C:/tfb/Synthetic/Finder.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let (f, _) = find(w, "shared", "ValueInfo", INT, 1, &[El::Local(a)]);
                    w.inc(&[El::Local(seen)]);
                    w.end_block(f);
                });
            },
        );
        let info = script(
            w,
            "C:/tfb/Synthetic/Owner.ai",
            |_| {},
            |_| {},
            |w| behaviour(w, |_| {}),
        );
        w.declare(info, &[shared]);
    });
    let a = obj.at(owner_obj.unwrap());
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let finder = vm.add(0x10, "Finder", 0);
    vm.add_at(0x20, "Owner", 1, a);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(finder, "seen"), None, "no list before its STARTUP");
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(finder, "seen"), Some(Value::Int(1)));
}

#[test]
fn a_removed_placement_finishes_its_tick_then_never_runs_and_cant_be_told() {
    let mut objs = Vec::new();
    let (p, obj) = program_and_objects(|w| {
        let leaver = actor(w);
        let other = actor(w);
        objs = vec![leaver, other];
        let both = w.variable("both", "ScriptSet", None);
        let (count, after, told) = (var(w, "count"), var(w, "after"), var(w, "told"));
        script(
            w,
            "C:/tfb/Synthetic/Leaver.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(count)]);
                    w.remove(&[El::Member("optoplevelbehavior", "my")]);
                    w.inc(&[El::Local(after)]);
                });
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Teller.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let t = tell(w, &[El::Local(leaver)]);
                    w.inc(&[El::Local(told)]);
                    w.end_block(t);
                    // A set of placements, taken off together.
                    let cs = w.stack(StackClass::Group, &[El::Local(both)]);
                    let one = w.reference(&[El::Local(other)], None);
                    let o = w.op(Class::ChangeMembership, "set [set]||to");
                    w.refer(o, 0x1C, cs);
                    w.word(o, 0x20, 0);
                    w.refer(o, 0x24, one);
                    w.push(o);
                    w.remove(&[El::Local(both)]);
                });
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Other.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(count)]);
                })
            },
        );
    });
    let (l, o) = (obj.at(objs[0]), obj.at(objs[1]));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let leaver = vm.add_at(0x10, "Leaver", 0, l);
    let teller = vm.add(0x20, "Teller", 1);
    let other = vm.add_at(0x30, "Other", 2, o);
    let mut host = Engine::default();
    let r = vm.step(&mut host).unwrap();
    assert_eq!(r.ticked, 2, "Other was taken off before its turn");
    assert_eq!(vm.local(leaver, "count"), Some(Value::Int(1)));
    assert_eq!(
        vm.local(leaver, "after"),
        Some(Value::Int(1)),
        "its tick runs to the end"
    );
    assert_eq!(
        vm.local(teller, "told"),
        None,
        "told after it left: skipped"
    );
    assert!(vm.removed(leaver) && vm.removed(other) && !vm.removed(teller));
    vm.step(&mut host).unwrap();
    assert_eq!(vm.local(leaver, "count"), Some(Value::Int(1)));
    assert_eq!(vm.local(other, "count"), None);
    // The host takes each off too, one at a time, and once.
    let removes: Vec<_> = host
        .ops
        .iter()
        .filter(|(c, _)| *c == Class::Remove)
        .map(|(_, v)| v.clone())
        .collect();
    assert_eq!(
        removes,
        vec![
            vec![Some(Value::Entity(0x10))],
            vec![Some(Value::Entity(0x30))]
        ]
    );
}

#[test]
fn a_placements_file_object_reads_as_its_handle() {
    let mut a = None;
    let (p, obj) = program_and_objects(|w| {
        let me = actor(w);
        a = Some(me);
        let same = var(w, "same");
        idle(
            w,
            |_| {},
            |w| {
                let c = check_ref(
                    w,
                    &[El::Member("optoplevelbehavior", "my")],
                    1,
                    &[El::Local(me)],
                );
                let one = w.int(1);
                w.set(&[El::Local(same)], one);
                w.end_block(c);
            },
        );
    });
    let me = obj.at(a.unwrap());
    for (named, want) in [(true, Some(Value::Int(1))), (false, None)] {
        let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
        let id = if named {
            vm.add_at(HANDLE, "Me", 0, me)
        } else {
            vm.add(HANDLE, "Me", 0)
        };
        vm.tick(id, &mut Engine::default()).unwrap();
        assert_eq!(vm.local(id, "same"), want, "named {named}");
        if named {
            assert_eq!(vm.placement_of(HANDLE), Some(id));
        }
    }
}

#[test]
fn screen_positions_and_colours_are_read_and_written_by_their_parts() {
    let p = program(|w| {
        let pos = w.variable("pos", "ScriptScreenInfo", Some("ScreenMeasurement"));
        let col = w.variable("col", "ScriptColorInfo", Some("ColorMeasurement"));
        let marks = w.variable("marks", "ScriptSet", Some("ScriptScreenInfo"));
        let r = w.variable("r", "ScriptReference", None);
        let (rx, ry, ra, sum, product, high) = (
            var(w, "rx"),
            var(w, "ry"),
            var(w, "ra"),
            var(w, "sum"),
            var(w, "product"),
            var(w, "high"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let x = [El::Local(pos), El::Member("screenmeasurement", "x")];
                let y = [El::Local(pos), El::Member("screenmeasurement", "y")];
                let v = w.float(2.53);
                w.set(&x, v);
                let v = w.int(-1);
                w.set(&y, v);
                let v = w.rhs(FLOAT, &x);
                w.set(&[El::Local(rx)], v);
                let v = w.rhs(FLOAT, &y);
                w.set(&[El::Local(ry)], v);
                let alpha = [El::Local(col), El::Member("colormeasurement", "alpha")];
                let v = w.int(300);
                w.set(&alpha, v);
                let v = w.int(16);
                w.set(&[El::Local(col), El::Member("colormeasurement", "red")], v);
                let v = w.rhs(INT, &alpha);
                w.set(&[El::Local(ra)], v);
                // A reference to a set element's half, written through.
                let one = w.int(1);
                w.set(&[El::Local(marks), El::Member("setvariant", "count")], one);
                setref(
                    w,
                    &[El::Local(r)],
                    &[
                        El::Local(marks),
                        El::Member("setvariant", "first"),
                        El::Member("screenmeasurement", "x"),
                    ],
                );
                let three = w.int(3);
                w.set(
                    &[
                        El::Local(r),
                        El::Member("abstractscriptgroup", "valuedereferencer"),
                    ],
                    three,
                );
                // Halves add on their own; a product shifts back by four
                // fraction bits; both hold to 16 bits.
                let a = w.literal("ScreenMeasurement", 0x0010_0020, false);
                let b = w.literal("ScreenMeasurement", 0xFFF0_0030, false);
                let s = w.expr(a, Some((0, b)));
                w.set(&[El::Local(sum)], s);
                let a = w.literal("ScreenMeasurement", 0x0010_0020, false);
                let b = w.literal("ScreenMeasurement", 0xFFF0_0030, false);
                let m = w.expr(a, Some((2, b)));
                w.set(&[El::Local(product)], m);
                let a = w.literal("ScreenMeasurement", 0x7FFF_0000, false);
                let b = w.literal("ScreenMeasurement", 0x0001_0000, false);
                let h = w.expr(a, Some((0, b)));
                w.set(&[El::Local(high)], h);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    vm.tick(id, &mut Engine::default()).unwrap();
    // 2.53 in sixteenths truncates to 40: 2.5.
    assert_eq!(vm.local(id, "rx"), Some(Value::Float(2.5)));
    assert_eq!(vm.local(id, "ry"), Some(Value::Float(-1.0)));
    assert_eq!(vm.local(id, "pos"), Some(Value::Screen(0x0028_FFF0)));
    assert_eq!(
        vm.local(id, "ra"),
        Some(Value::Int(255)),
        "clamped to a byte"
    );
    assert_eq!(vm.local(id, "col"), Some(Value::Color(0x1000_00FF)));
    let marks = vm.local(id, "marks").unwrap();
    let first = vm.set(marks).unwrap()[0];
    assert_eq!(vm.value(first), Value::Screen(0x0030_0000), "3.0 in its x");
    assert_eq!(vm.local(id, "sum"), Some(Value::Screen(0x0000_0050)));
    // 1.0 x -1.0 is -1.0 (0xFFF0); 2.0 x 3.0 is 6.0 (0x0060).
    assert_eq!(vm.local(id, "product"), Some(Value::Screen(0xFFF0_0060)));
    assert_eq!(
        vm.local(id, "high"),
        Some(Value::Screen(0x7FFF_0000)),
        "saturates"
    );
}

#[test]
fn a_spawned_structure_is_a_copy_the_block_runs_on() {
    let p = program(|w| {
        let (section, _) = w.open(Class::StructureSection, "STRUCTURES");
        let (bp, boxed) = w.open(Class::DefineStructure, "Box");
        let v = w.variable("v", "ValueInfo", Some(INT));
        w.push(v);
        w.end_block(bp);
        let (sp, def) = w.open(Class::DefineStructure, "Pair");
        let a = w.variable("a", "ValueInfo", Some(INT));
        w.push(a);
        let items = w.variable("items", "ScriptSet", Some("ValueInfo"));
        w.push(items);
        // A structure inside it, copied too.
        let inner = w.variable("inner", "ScriptStructure", None);
        w.refer(inner, 0x24, boxed);
        w.push(inner);
        w.end_block(sp);
        w.end_block(section);
        w.end("C:/tfb/Synthetic/Types.ai");
        let pair = w.variable("p", "ScriptStructure", None);
        w.refer(pair, 0x24, def);
        let q = w.variable("q", "ScriptReference", None);
        let inside = var(w, "inside");
        let thing = actor(w);
        idle(
            w,
            |_| {},
            |w| {
                let one = w.int(1);
                w.set(&[El::Local(pair), El::Local(a)], one);
                w.inc(&[
                    El::Local(pair),
                    El::Local(items),
                    El::Member("setvariant", "count"),
                ]);
                w.inc(&[El::Local(pair), El::Local(inner), El::Local(v)]);
                let lhs = w.stack(StackClass::PlacementList, &[El::Local(pair)]);
                let at = w.stack(StackClass::Position, &[]);
                let zero = w.int(0);
                let (s, so) = w.open(Class::Spawn, "clone||at|facing||cloned");
                w.refer(so, 0x20, lhs);
                w.refer(so, 0x28, at);
                w.refer(so, 0x2C, zero);
                let two = w.int(2);
                w.set(&[El::Local(so), El::Local(a)], two);
                w.inc(&[
                    El::Local(so),
                    El::Local(items),
                    El::Member("setvariant", "count"),
                ]);
                let three = w.int(3);
                w.set(&[El::Local(so), El::Local(inner), El::Local(v)], three);
                // The block's receiver is the copy: a world op in it goes
                // to the host on it (`my` stays the placement).
                let t = tell(w, &[El::Local(thing)]);
                w.inc(&[El::Local(inside)]);
                w.end_block(t);
                w.end_block(s);
                setref(w, &[El::Local(q)], &[El::Local(so)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 1);
    let mut host = Engine::default();
    vm.tick(id, &mut host).unwrap();
    assert!(
        host.ops.iter().all(|(class, _)| *class != Class::Spawn),
        "the VM copies its own: {:?}",
        host.ops
    );
    let original = vm.local(id, "p").unwrap();
    let copy = vm.local(id, "q").unwrap();
    assert_eq!(host.ops.len(), 1, "the tell: {:?}", host.ops);
    assert_eq!(host.receivers, vec![vm.value(copy)], "on the copy");
    assert_ne!(vm.value(original), vm.value(copy));
    assert_eq!(vm.field(original, "a"), Some(Value::Int(1)));
    assert_eq!(vm.field(copy, "a"), Some(Value::Int(2)));
    let count = |v: Value| vm.set(vm.field(v, "items").unwrap()).unwrap().len();
    assert_eq!((count(original), count(copy)), (1, 2), "its own set");
    let inner = |v: Value| vm.field(vm.field(v, "inner").unwrap(), "v");
    assert_eq!(
        (inner(original), inner(copy)),
        (Some(Value::Int(1)), Some(Value::Int(3))),
        "its own structure inside"
    );
    assert_eq!(vm.local(id, "inside"), Some(Value::Int(1)), "the tell ran");
}

#[test]
fn a_first_tick_listing_runs_startup_then_holds() {
    let p = program(|w| {
        let (started, ticks) = (var(w, "started"), var(w, "ticks"));
        script(
            w,
            "C:/tfb/Synthetic/Once.ai",
            |w| {
                w.inc(&[El::Local(started)]);
            },
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(ticks)]);
                })
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Every.ai",
            |_| {},
            |_| {},
            |w| {
                behaviour(w, |w| {
                    w.inc(&[El::Local(ticks)]);
                })
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.whitelist = Whitelist::with_first_ticks(&["Every"], &[], &["Once.ai"]);
    let mut vm = Vm::new(p, c);
    let once = vm.add(0x10, "Once", 0);
    let every = vm.add(0x20, "Every", 1);
    let off = vm.add(0x30, "Off", 1);
    let mut host = Engine::default();
    for _ in 0..3 {
        vm.step(&mut host).unwrap();
    }
    assert_eq!(vm.local(once, "started"), Some(Value::Int(1)));
    assert_eq!(
        vm.local(once, "ticks"),
        Some(Value::Int(1)),
        "its first tick"
    );
    assert_eq!(vm.local(every, "ticks"), Some(Value::Int(3)));
    assert_eq!(vm.held(), vec![0x10]);
    assert_eq!(vm.inert(), vec![0x30]);
    assert_eq!(vm.local(off, "ticks"), None);
}

/// The level's near-enemy award as `Game - Set Global XP award values`
/// computes it at chapter 1: shares 6 x 1.2, 700 over them, x 1.1 then
/// x 1.2, each step stored.
fn award() -> Arc<Program> {
    program(|w| {
        let (share, one, near, big) = (
            var(w, "share"),
            var(w, "one"),
            var(w, "near"),
            var(w, "big"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let six = w.int(6);
                let k = w.float(1.2);
                let e = w.expr(six, Some((2, k)));
                w.set(&[El::Local(share)], e);
                let budget = w.int(700);
                let s = read(w, share);
                let e = w.expr(budget, Some((3, s)));
                w.set(&[El::Local(one)], e);
                let o = read(w, one);
                let m = w.float(1.1);
                let e = w.expr(o, Some((2, m)));
                w.set(&[El::Local(one)], e);
                let o = read(w, one);
                let k = w.float(1.2);
                let e = w.expr(o, Some((2, k)));
                w.set(&[El::Local(near)], e);
                // (1e8 + 1) - 1e8, nested in one expression.
                let a = w.float(1e8);
                let one_ = w.int(1);
                let sum = w.expr(a, Some((0, one_)));
                let b = w.float(1e8);
                let e = w.expr(sum, Some((1, b)));
                w.set(&[El::Local(big)], e);
            },
        );
    })
}

#[test]
fn double_precision_carries_fractions_and_parts_from_the_originals_bits() {
    let p = award();
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Award", 0);
    vm.tick(id, &mut Engine::default()).unwrap();
    let Some(Value::Float(near)) = vm.local(id, "near") else {
        panic!("{:?}", vm.local(id, "near"))
    };
    assert_eq!(near.to_bits(), 0x4300_5556, "the original's bits");
    assert_eq!(vm.local(id, "big"), Some(Value::Float(0.0)));
    let mut c = Config::new(Mode::Strict);
    c.precision = Precision::Double;
    let mut vm = Vm::new(p, c);
    let id = vm.add(HANDLE, "Award", 0);
    vm.tick(id, &mut Engine::default()).unwrap();
    let Some(Value::Double(near)) = vm.local(id, "near") else {
        panic!("{:?}", vm.local(id, "near"))
    };
    // Carried in double precision the award isn't the original's number,
    // though rounding it once to single precision lands on the same bits.
    assert_ne!(near, f64::from(f32::from_bits(0x4300_5556)));
    assert!((near - 128.333_336).abs() < 1e-6, "{near}");
    assert_eq!((near as f32).to_bits(), 0x4300_5556);
    assert_eq!(vm.local(id, "big"), Some(Value::Double(1.0)));
    // Compared in double precision, within the scripts' tolerance as in
    // single.
    let compare = crate::value::compare;
    let (a, b) = (Value::Double(1.0), Value::Double(1.000_05));
    assert_eq!(compare(1, a, b), Some(true), "equal within 1e-4");
    assert_eq!(compare(3, a, b), Some(false), "not less");
    assert_eq!(compare(1, a, Value::Double(1.001)), Some(false));
}

#[test]
fn the_script_timer_counts_a_placements_ticks_and_the_frame_scale_is_one() {
    let p = program(|w| {
        let (t, f) = (var(w, "t"), var(w, "f"));
        idle(
            w,
            |_| {},
            |w| {
                let r = w.rhs(FLOAT, &[El::Member("system", "script timer")]);
                w.set(&[El::Local(t)], r);
                let r = w.rhs(FLOAT, &[El::Member("system", "frame rate scale")]);
                w.set(&[El::Local(f)], r);
            },
        );
    });
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Clock", 0);
    let mut host = Engine::default();
    vm.tick(id, &mut host).unwrap();
    assert_eq!(
        vm.local(id, "t"),
        Some(Value::Float(0.0)),
        "during its first"
    );
    assert_eq!(vm.local(id, "f"), Some(Value::Float(1.0)));
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "t"), Some(Value::Float(TICK_SECONDS)));
    vm.tick(id, &mut host).unwrap();
    assert_eq!(
        vm.local(id, "t"),
        Some(Value::Float(TICK_SECONDS + TICK_SECONDS))
    );
    assert_eq!(vm.active_seconds(id), Some(3.0 * TICK_SECONDS));
    let mut c = Config::new(Mode::Strict);
    c.tick_seconds = 0.05;
    let mut vm = Vm::new(p, c);
    let id = vm.add(HANDLE, "Clock", 0);
    vm.tick(id, &mut host).unwrap();
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "t"), Some(Value::Float(0.05)));
    assert_eq!(vm.local(id, "f"), Some(Value::Float(3.0)));
}

#[test]
fn an_engine_set_member_is_read_whole_from_the_host() {
    let p = program(|w| {
        let (n, visits) = (var(w, "n"), var(w, "visits"));
        idle(
            w,
            |_| {},
            |w| {
                let items = [El::Member("test", "items")];
                let c = w.rhs(
                    INT,
                    &[
                        El::Member("test", "items"),
                        El::Member("setvariant", "count"),
                    ],
                );
                w.set(&[El::Local(n)], c);
                let set = w.stack(StackClass::Group, &items);
                let zero = w.int(0);
                let (fp, fe) = w.open(Class::ForEach, "for each");
                w.refer(fe, 0x20, set);
                w.refer(fe, 0x28, zero);
                w.inc(&[El::Local(visits)]);
                let flow = w.flow(Class::FlowLoop);
                w.push(flow);
                w.close(fp);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.members
        .host("test", "items", MemberSpec::new(Kind::Set, 0.0, 0.0));
    let mut vm = Vm::new(p, c);
    let id = vm.add(HANDLE, "Reader", 0);
    let mut host = Engine {
        items: vec![Value::Entity(1), Value::Entity(2), Value::Entity(3)],
        ..Engine::default()
    };
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "n"), Some(Value::Int(3)));
    assert_eq!(vm.local(id, "visits"), Some(Value::Int(3)));
    host.items.pop();
    vm.tick(id, &mut host).unwrap();
    assert_eq!(
        vm.local(id, "n"),
        Some(Value::Int(2)),
        "read anew each time"
    );
}

#[test]
fn a_reset_puts_a_structure_back_and_asks_the_host_for_a_placement() {
    let mut a = None;
    let (p, obj) = program_and_objects(|w| {
        let me = actor(w);
        a = Some(me);
        let (section, _) = w.open(Class::StructureSection, "STRUCTURES");
        let (sp, def) = w.open(Class::DefineStructure, "Box");
        let v = w.variable("v", "ValueInfo", Some(INT));
        w.push(v);
        w.end_block(sp);
        w.end_block(section);
        w.end("C:/tfb/Synthetic/Types.ai");
        let b = w.variable("b", "ScriptStructure", None);
        w.refer(b, 0x24, def);
        let r = w.variable("r", "ScriptReference", None);
        idle(
            w,
            |_| {},
            |w| {
                let five = w.int(5);
                w.set(&[El::Local(b), El::Local(v)], five);
                setref(w, &[El::Local(r)], &[El::Local(b)]);
                reset(w, &[El::Local(r)]);
                reset(w, &[El::Local(me)]);
            },
        );
    });
    let me = obj.at(a.unwrap());
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add_at(HANDLE, "Me", 1, me);
    let mut host = Engine::default();
    vm.tick(id, &mut host).unwrap();
    let b = vm.local(id, "b").unwrap();
    assert_eq!(vm.field(b, "v"), Some(Value::Int(0)));
    assert_eq!(
        host.ops,
        vec![(Class::Reset, vec![Some(Value::Entity(HANDLE))])]
    );
}

/// A reset through a reference to a number puts that number back as a
/// reset of the variable itself does, the reference kept (AH.16's review:
/// it stopped as an unsupported reset of a constant).
#[test]
fn a_reset_through_a_reference_puts_back_the_number_it_names() {
    let p = program(|w| {
        let n = var(w, "n");
        let r = w.variable("r", "ScriptReference", None);
        // A file's number variable holding 9.
        let f = w.b.object(0, "ValueInfo", 0x24);
        w.text(f, 0x08, "file number");
        w.meta_word(f, 0x1C, INT);
        w.word(f, 0x20, 9);
        let s = w.variable("s", "ScriptReference", None);
        idle(
            w,
            |_| {},
            |w| {
                let five = w.int(5);
                w.set(&[El::Local(n)], five);
                setref(w, &[El::Local(r)], &[El::Local(n)]);
                reset(w, &[El::Local(r)]);
                setref(w, &[El::Local(s)], &[El::Local(f)]);
                reset(w, &[El::Local(s)]);
            },
        );
    });
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    let g = p.global_named("file number").expect("a file variable");
    assert_eq!(vm.global(g), Some(Value::Int(9)));
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "n"), Some(Value::Int(0)));
    let r = vm.local(id, "r").unwrap();
    assert!(matches!(r, Value::Cell(_)), "the reference kept: {r:?}");
    assert_eq!(vm.global(g), Some(Value::Int(0)), "the file's number");
    let s = vm.local(id, "s").unwrap();
    assert!(matches!(s, Value::Object(_)), "the reference kept: {s:?}");
}

#[test]
fn a_slide_lands_at_once_and_skips_its_block() {
    let p = program(|w| {
        let (v, inside) = (var(w, "v"), var(w, "inside"));
        idle(
            w,
            |_| {},
            |w| {
                let lhs = w.value(&[El::Local(v)]);
                let to = w.int(10);
                let secs = w.float(2.0);
                let ease = w.int(20);
                let (sp, so) = w.open(Class::SlideValue, "slide value");
                w.refer(so, 0x1C, lhs);
                w.refer(so, 0x20, to);
                w.refer(so, 0x24, secs);
                w.refer(so, 0x28, ease);
                w.refer(so, 0x2C, ease);
                w.inc(&[El::Local(inside)]);
                w.end_block(sp);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Slider", 0);
    let mut host = Engine::default();
    vm.tick(id, &mut host).unwrap();
    assert_eq!(vm.local(id, "v"), Some(Value::Int(10)));
    assert_eq!(vm.local(id, "inside"), None);
    assert!(host.ops.is_empty(), "the host isn't asked");
}

#[test]
fn undefined_reads_0_as_a_number_and_none_as_an_object() {
    let p = program(|w| {
        let r = w.variable("r", "ScriptReference", None);
        let (some, zero) = (var(w, "some"), var(w, "zero"));
        idle(
            w,
            |_| {},
            |w| {
                setref(
                    w,
                    &[El::Local(r)],
                    &[El::Member("abstractscriptgroup", "undefined")],
                );
                let c = check_ref(
                    w,
                    &[El::Local(r)],
                    5,
                    &[El::Member("abstractscriptgroup", "null")],
                );
                let one = w.int(1);
                w.set(&[El::Local(some)], one);
                w.end_block(c);
                let z = w.int(0);
                let u = w.value(&[El::Member("abstractscriptgroup", "undefined")]);
                let (cp, co) = w.open(Class::CheckValue, "if");
                w.refer(co, 0x1C, u);
                w.word(co, 0x20, 1);
                w.refer(co, 0x24, z);
                let one = w.int(1);
                w.set(&[El::Local(zero)], one);
                w.end_block(cp);
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert_eq!(vm.local(id, "some"), None, "undefined is no object");
    assert_eq!(vm.local(id, "zero"), Some(Value::Int(1)));
}

#[test]
fn member_reads_and_writes_are_counted_and_a_local_can_be_seeded() {
    let p = program(|w| {
        let (x, y) = (var(w, "x"), var(w, "y"));
        idle(
            w,
            |_| {},
            |w| {
                let cell = [El::Member("test", "c")];
                let r = read(w, x);
                w.set(&cell, r);
                let v = w.rhs(INT, &cell);
                w.set(&[El::Local(y)], v);
                let v = w.rhs(INT, &cell);
                w.set(&[El::Local(y)], v);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["c"]));
    let id = vm.add(HANDLE, "Seeded", 0);
    assert!(vm.set_local(id, "x", Value::Int(9)));
    assert!(!vm.set_local(id, "no such name", Value::Int(1)));
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "y"), Some(Value::Int(9)));
    assert_eq!(vm.uses().get(&MemberId::named("test", "c")), Some(&(2, 1)));
}

#[test]
fn an_operand_reads_outside_any_placement() {
    let p = program(|w| {
        let x = var(w, "x");
        idle(
            w,
            |_| {},
            |w| {
                let a = w.int(300);
                let b = w.int(10);
                let e = w.expr(a, Some((1, b)));
                w.set(&[El::Local(x)], e);
                let r = w.rhs(INT, &[El::Member("test", "unmodelled")]);
                w.set(&[El::Local(x)], r);
            },
        );
    });
    let exprs: Vec<OperandId> = (0..p.operands.len() as OperandId)
        .filter(|&o| matches!(p.operands[o as usize], Operand::Expr { op: Some(_), .. }))
        .collect();
    let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
    let mut host = Engine::default();
    assert_eq!(vm.eval(exprs[0], &mut host), Ok(Value::Int(290)));
    let member = (0..p.operands.len() as OperandId)
        .find(|&o| {
            matches!(&p.operands[o as usize], Operand::Path { elems, .. }
                if elems.first() == Some(&Elem::Member(MemberId::named("test", "unmodelled"))))
        })
        .unwrap();
    let halt = vm.eval(member, &mut host).unwrap_err();
    assert_eq!(
        halt.reason,
        Reason::UnmodelledMember(format!(
            "{:08x}.{:08x}",
            MemberId::named("test", "unmodelled").namespace,
            MemberId::named("test", "unmodelled").name
        ))
    );
}

/// A parameter op of `def` named `name`, a `container` bound to `rhs`.
fn parameter(
    w: &mut Writer<'_>,
    def: Location,
    name: &str,
    container: &str,
    rhs: Location,
) -> Location {
    let o = w.op(Class::MacroParameter, "parameter");
    w.text(o, 0x1C, name);
    w.class_word(o, 0x20, container);
    w.word(o, 0x28, u32::MAX);
    w.refer(o, 0x30, rhs);
    w.refer(o, 0x38, def);
    o
}

/// An INTERFACE block of `params`, which [`parameter`] made.
fn interface(w: &mut Writer<'_>, params: &[Location]) {
    let (p, _) = w.open(Class::MacroInterface, "INTERFACE");
    for &o in params {
        w.push(o);
    }
    let flow = w.flow(Class::FlowMacro);
    w.push(flow);
    w.close(p);
}

/// Combat's full-health rule (the combat spec) in single precision.
fn full_health(base: f32, level: i32) -> i32 {
    let multiplier = (level - 1) as f32 * 0.111 + 1.0;
    (base * multiplier + 0.99) as i32
}

#[test]
fn full_health_on_synthetic_bases_is_combats_rule_at_levels_1_to_15() {
    // The call sites' shape, our names: a multiplier macro returning
    // 1 + (level - 1) x 0.111, the base times it, then a round-up macro
    // through a reference to the product.
    let p = program(|w| {
        let level = w.variable("level", "ValueInfo", Some(INT));
        let base = w.variable("base", "ValueInfo", Some(FLOAT));
        let factor = w.variable("factor", "ValueInfo", Some(FLOAT));
        let health = w.variable("health", "ValueInfo", Some(INT));
        let mult = w.op(Class::DefineMacro, "HP Multiplier");
        let round = w.op(Class::DefineMacro, "Round Up");
        let one = w.int(1);
        let m_level = parameter(w, mult, "level", "ValueInfo", one);
        let unit = w.float(1.0);
        let m_out = parameter(w, mult, "multiplier", "ValueInfo", unit);
        let none = w.reference(&[], None);
        let r_value = parameter(w, round, "value", "ScriptReference", none);
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                let at = w.push(mult);
                interface(w, &[m_level, m_out]);
                let l = w.rhs(INT, &[El::Local(m_level)]);
                let one = w.int(1);
                let less = w.expr(l, Some((1, one)));
                w.set(&[El::Local(m_out)], less);
                let m = w.rhs(FLOAT, &[El::Local(m_out)]);
                let step = w.float(0.111);
                let times = w.expr(m, Some((2, step)));
                w.set(&[El::Local(m_out)], times);
                let m = w.rhs(FLOAT, &[El::Local(m_out)]);
                let one = w.float(1.0);
                let plus = w.expr(m, Some((0, one)));
                w.set(&[El::Local(m_out)], plus);
                w.end_block(at);
                let at = w.push(round);
                interface(w, &[r_value]);
                let deref = [
                    El::Local(r_value),
                    El::Member("abstractscriptgroup", "valuedereferencer"),
                ];
                let v = w.rhs(FLOAT, &deref);
                let up = w.float(0.99);
                let sum = w.expr(v, Some((0, up)));
                w.set(&[El::Member("system", "cast to integer")], sum);
                let cast = w.rhs(INT, &[El::Member("system", "cast to integer")]);
                w.set(&deref, cast);
                w.end_block(at);
            },
            |_| {},
            |w| {
                behaviour(w, |w| {
                    let np = w.group(&[El::Local(mult)], None);
                    let (call, o) = w.open(Class::UseMacro, "use macro");
                    w.refer(o, 0x1C, np);
                    let l = w.rhs(INT, &[El::Local(level)]);
                    let a = parameter(w, mult, "level", "ValueInfo", l);
                    let unit = w.float(1.0);
                    let b = parameter(w, mult, "multiplier", "ValueInfo", unit);
                    interface(w, &[a, b]);
                    let got = w.rhs(FLOAT, &[El::Local(b)]);
                    w.set(&[El::Local(factor)], got);
                    w.end_block(call);
                    let hp = w.rhs(FLOAT, &[El::Local(base)]);
                    let f = w.rhs(FLOAT, &[El::Local(factor)]);
                    let product = w.expr(hp, Some((2, f)));
                    w.set(&[El::Local(health)], product);
                    let np = w.group(&[El::Local(round)], None);
                    let (call, o) = w.open(Class::UseMacro, "use macro");
                    w.refer(o, 0x1C, np);
                    let r = w.reference(&[El::Local(health)], None);
                    let a = parameter(w, round, "value", "ScriptReference", r);
                    interface(w, &[a]);
                    w.end_block(call);
                });
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    let mut checked = 0;
    for base in (100..=500)
        .step_by(5)
        .map(|b| b as f32)
        .chain([287.5, 333.3])
    {
        for level in 1..=15 {
            vm.set_local(id, "base", Value::Float(base));
            vm.set_local(id, "level", Value::Int(level));
            vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
            assert_eq!(
                vm.local(id, "health"),
                Some(Value::Int(full_health(base, level))),
                "base {base} at level {level}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 83 * 15);
    // The spec's own: a 290 hero's 290, 323, 451, 580 and 741.
    for (level, want) in [(1, 290), (2, 323), (6, 451), (10, 580), (15, 741)] {
        assert_eq!(full_health(290.0, level), want);
    }
}

#[test]
fn next_reads_a_set_in_turn_and_behavior_is_the_running_behaviour() {
    let p = program(|w| {
        let names = w.variable("names", "ScriptSet", None);
        let picks: Vec<Location> = (0..4)
            .map(|i| w.variable(&format!("pick{i}"), "ScriptReference", None))
            .collect();
        let idle = var(w, "idle");
        let battle = var(w, "battle");
        let idle_op = w.op(Class::UserBehavior, "Idle");
        let battle_op = w.op(Class::UserBehavior, "Battle");
        frame(
            w,
            |w| {
                for name in ["A", "B", "C"] {
                    let lhs = w.stack(StackClass::Set, &[El::Local(names)]);
                    let rhs = w.reference(&[El::Named("SpriteInfo", name)], None);
                    let o = w.op(Class::ChangeMembership, "set [set]||to");
                    w.refer(o, 0x1C, lhs);
                    w.word(o, 0x20, 4);
                    w.refer(o, 0x24, rhs);
                    w.push(o);
                }
            },
            |_| {},
            |_| {},
            |_| {},
            |w| {
                let b = w.push(idle_op);
                for &pick in &picks {
                    setref(
                        w,
                        &[El::Local(pick)],
                        &[El::Local(names), El::Member("setvariant", "next")],
                    );
                }
                for (op, count) in [(idle_op, idle), (battle_op, battle)] {
                    let c = check_ref(
                        w,
                        &[El::Member("optoplevelbehavior", "behavior")],
                        1,
                        &[El::Local(op)],
                    );
                    w.inc(&[El::Local(count)]);
                    w.end_block(c);
                }
                w.set_behavior(battle_op);
                w.end_block(b);
                let b = w.push(battle_op);
                let c = check_ref(
                    w,
                    &[El::Member("optoplevelbehavior", "behavior")],
                    1,
                    &[El::Local(battle_op)],
                );
                w.inc(&[El::Local(battle)]);
                w.end_block(c);
                w.end_block(b);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    vm.tick(id, &mut Engine::default()).unwrap();
    let pick = |i: usize| vm.local(id, &format!("pick{i}")).unwrap();
    let names = vm.local(id, "names").unwrap();
    let elems = vm.set(names).unwrap();
    assert_eq!(elems.len(), 3);
    // In the set's order from its first, then round again.
    assert_eq!([pick(0), pick(1), pick(2)], [elems[0], elems[1], elems[2]]);
    assert_eq!(pick(3), elems[0]);
    assert_eq!(vm.local(id, "idle"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "battle"), None, "still idle this tick");
    vm.tick(id, &mut Engine::default()).unwrap();
    assert_eq!(vm.local(id, "battle"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "idle"), Some(Value::Int(1)));
}
