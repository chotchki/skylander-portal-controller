//! The interpreter's unit tests (AH.14), on scripts built in code with
//! AH.13's op-graph writer: every flow rule, the comparison codes through
//! the VM, a cast, single-precision order, strict and play mode, value
//! cells, the budget, macros, sets and the whitelist. The stall detector's
//! are in `stall`, the pure arithmetic's in `value`.

use std::sync::Arc;

use guac_formats::igz::builder::IgzBuilder;
use guac_formats::igz::{Igz, Location};
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph, StackClass};

use crate::*;

mod creatures;
mod embed;
mod hero;
mod init;
mod loot;
mod order;
mod slides;
mod span;

const INT: &str = "IntMeasurement";
const HANDLE: Handle = 0x51;

fn program(build: impl FnOnce(&mut Writer<'_>)) -> Arc<Program> {
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
    Arc::new(Program::link(&[unit]).expect("link"))
}

fn var(w: &mut Writer<'_>, name: &str) -> Location {
    w.variable(name, "ValueInfo", Some(INT))
}

fn read(w: &mut Writer<'_>, v: Location) -> Location {
    w.rhs(INT, &[El::Local(v)])
}

/// A script: PRESCRIPT holding STARTUP, SHUTDOWN, MACROS and then its own
/// ops, then the behaviours.
fn frame(
    w: &mut Writer<'_>,
    startup: impl FnOnce(&mut Writer<'_>),
    shutdown: impl FnOnce(&mut Writer<'_>),
    macros: impl FnOnce(&mut Writer<'_>),
    prescript: impl FnOnce(&mut Writer<'_>),
    behaviours: impl FnOnce(&mut Writer<'_>),
) {
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    startup(w);
    w.end_block(start);
    let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
    shutdown(w);
    w.end_block(shut);
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    macros(w);
    w.end_block(section);
    prescript(w);
    w.end_block(pre);
    behaviours(w);
    w.end("C:/tfb/Content/Levels/Synthetic/Test.ai");
}

/// A script that is one behaviour, `idle`, after an empty frame.
fn idle(
    w: &mut Writer<'_>,
    startup: impl FnOnce(&mut Writer<'_>),
    body: impl FnOnce(&mut Writer<'_>),
) {
    frame(
        w,
        startup,
        |_| {},
        |_| {},
        |_| {},
        |w| {
            let b = w.behaviour("idle");
            body(w);
            w.end_block(b);
        },
    );
}

/// `if <var> <relation> <rhs>`, its block left open: position and op.
fn check(w: &mut Writer<'_>, v: Location, relation: u32, rhs: Location) -> (usize, Location) {
    let lhs = w.value(&[El::Local(v)]);
    let (p, o) = w.open(Class::CheckValue, "if");
    w.refer(o, 0x1C, lhs);
    w.word(o, 0x20, relation);
    w.refer(o, 0x24, rhs);
    (p, o)
}

fn branch(w: &mut Writer<'_>, op: Location, n: u16) {
    w.half(op, 0x1A, n);
}

/// `use macro <def>` with `args`, then `after` as its after-return code.
fn call_then(
    w: &mut Writer<'_>,
    def: Location,
    args: &[(&str, Location)],
    after: impl FnOnce(&mut Writer<'_>),
) {
    let np = w.group(&[El::Local(def)], None);
    let (p, o) = w.open(Class::UseMacro, "use macro");
    w.refer(o, 0x1C, np);
    let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
    for (name, rhs) in args {
        let param = parameter(w, name, def);
        w.refer(param, 0x30, *rhs);
        w.push(param);
    }
    let enter = w.flow(Class::FlowMacro);
    w.push(enter);
    w.close(iface);
    after(w);
    w.end_block(p);
}

/// A number parameter named `name` binding into `def`, at no position.
fn parameter(w: &mut Writer<'_>, name: &str, def: Location) -> Location {
    let o = w.op(Class::MacroParameter, "parameter");
    w.text(o, 0x1C, name);
    w.class_word(o, 0x20, "ValueInfo");
    w.word(o, 0x28, u32::MAX);
    w.refer(o, 0x38, def);
    o
}

fn reset(w: &mut Writer<'_>, path: &[El<'_>]) {
    let np = w.group(path, None);
    let o = w.op(Class::Reset, "reset");
    w.refer(o, 0x1C, np);
    w.push(o);
}

fn int_cell() -> MemberSpec {
    MemberSpec::new(Kind::Int, 0.0, 0.0)
}

fn config(mode: Mode, cells: &[&str]) -> Config {
    let mut c = Config::new(mode);
    for name in cells {
        c.members.value_cell("test", name, int_cell());
    }
    c
}

fn cell(vm: &Vm, name: &str) -> Option<Value> {
    vm.value_cell(None, MemberId::named("test", name))
}

/// A host modelling no member and answering every world op "no target".
struct Bare;

impl Host for Bare {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn draw(&mut self) -> u32 {
        0
    }
}

/// [`Bare`], drawing from a seeded generator.
struct Rolling(Generator);

impl Host for Rolling {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn draw(&mut self) -> u32 {
        self.0.next_u32()
    }
}

fn run(program: &Arc<Program>, config: Config, ticks: usize) -> (Vm, PlacementId) {
    let mut vm = Vm::new(program.clone(), config);
    let id = vm.add(HANDLE, "Thing", 0);
    for _ in 0..ticks {
        vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    }
    (vm, id)
}

#[test]
fn an_else_if_chain_whose_inner_check_crosses_its_blocks_end() {
    for (x, want) in [(1, 10), (2, 20), (3, 30)] {
        let p = program(|w| {
            let xv = var(w, "x");
            let r = var(w, "r");
            let after = var(w, "after");
            idle(
                w,
                |w| {
                    let lit = w.int(x);
                    w.set(&[El::Local(xv)], lit);
                },
                |w| {
                    // 1: if x == 1 [2]   2: r = 10   3: else [2]
                    // 4: if x == 2 [2]   5: r = 20   6: else [1]
                    // 7: r = 30          8: inc after
                    // The inner check's block (5-6) runs past the outer
                    // else's (4-5): only a counter stepping the list gets it
                    // right.
                    let one = w.int(1);
                    let (_, c1) = check(w, xv, 1, one);
                    let ten = w.int(10);
                    w.set(&[El::Local(r)], ten);
                    let (_, e1) = w.open(Class::Skip, "else");
                    let two = w.int(2);
                    let (_, c2) = check(w, xv, 1, two);
                    let twenty = w.int(20);
                    w.set(&[El::Local(r)], twenty);
                    let (_, e2) = w.open(Class::Skip, "else");
                    let thirty = w.int(30);
                    w.set(&[El::Local(r)], thirty);
                    w.inc(&[El::Local(after)]);
                    branch(w, c1, 2);
                    branch(w, e1, 2);
                    branch(w, c2, 2);
                    branch(w, e2, 1);
                },
            );
        });
        let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
        assert_eq!(vm.local(id, "r"), Some(Value::Int(want)), "x = {x}");
        assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
        // The blocks it left behind are gone: the next tick is whole.
        vm.tick(id, &mut Bare).unwrap();
        assert_eq!(vm.local(id, "after"), Some(Value::Int(2)));
    }
}

#[test]
fn a_skip_out_of_a_loop_breaks_it() {
    let p = program(|w| {
        let count = var(w, "count");
        let after = var(w, "after");
        idle(
            w,
            |_| {},
            |w| {
                // 1: loop 5 [4]  2: inc count  3: if count >= 3 [1]
                // 4: skip [1] (past the loop's flow)  5: flow  6: inc after
                let five = w.int(5);
                let (_, l) = w.open(Class::LoopValue, "loop value|||loop index");
                w.refer(l, 0x20, five);
                w.inc(&[El::Local(count)]);
                let three = w.int(3);
                let (_, c) = check(w, count, 2, three);
                let (_, s) = w.open(Class::Skip, "else");
                let flow = w.flow(Class::FlowLoop);
                w.push(flow);
                w.inc(&[El::Local(after)]);
                branch(w, l, 4);
                branch(w, c, 1);
                branch(w, s, 1);
            },
        );
    });
    let (mut vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert_eq!(vm.local(id, "count"), Some(Value::Int(3)));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
    // Next tick count is already 3: one pass, then the break.
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!(vm.local(id, "count"), Some(Value::Int(4)));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(2)));
}

#[test]
fn a_macro_called_while_it_runs_is_skipped_whole() {
    let p = program(|w| {
        let m = w.op(Class::DefineMacro, "Recurse");
        let n = w.op(Class::DefineMacro, "Other");
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                // M: inc entries; call M (after: inc inner); call N; inc outer
                let at = w.push(m);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.inc(&[El::Member("test", "entries")]);
                call_then(w, m, &[], |w| {
                    w.inc(&[El::Member("test", "inner")]);
                });
                w.call(n, &[]);
                w.inc(&[El::Member("test", "outer")]);
                w.end_block(at);
                // N: inc n; call M (M is running: skipped)
                let at = w.push(n);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.inc(&[El::Member("test", "n")]);
                w.call(m, &[]);
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                w.call(m, &[]);
                w.end_block(b);
            },
        );
    });
    let (vm, _) = run(
        &p,
        config(Mode::Strict, &["entries", "inner", "outer", "n"]),
        1,
    );
    assert_eq!(cell(&vm, "entries"), Some(Value::Int(1)));
    assert_eq!(cell(&vm, "n"), Some(Value::Int(1)));
    assert_eq!(cell(&vm, "outer"), Some(Value::Int(1)));
    assert_eq!(
        cell(&vm, "inner"),
        None,
        "the skipped call's after-return code never ran"
    );
    let skipped = vm
        .log()
        .iter()
        .filter(|l| l.text.contains("is running"))
        .count();
    assert_eq!(skipped, 2, "{:?}", vm.log());
}

#[test]
fn a_call_binds_by_name_returns_through_its_frame_and_resets_it() {
    let p = program(|w| {
        let a = var(w, "a");
        let b = var(w, "b");
        let result = var(w, "result");
        let sum = var(w, "sum");
        let def = std::cell::Cell::new(None);
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                // Add(a, b): result = a + b; flow end; result = 999
                let zero = w.int(0);
                let (at, d) = w.define("Add", &[("a", zero), ("b", zero)]);
                let ra = read(w, a);
                let rb = read(w, b);
                let plus = w.expr(ra, Some((0, rb)));
                w.set(&[El::Local(result)], plus);
                w.flow_end();
                let never = w.int(999);
                w.set(&[El::Local(result)], never);
                w.end_block(at);
                def.set(Some(d));
            },
            |_| {},
            |w| {
                let d = def.get().unwrap();
                let bh = w.behaviour("idle");
                let two = w.int(2);
                let three = w.int(3);
                // The after-return code reads the callee's result.
                let ret = parameter(w, "result", d);
                call_then(w, d, &[("a", two), ("b", three)], |w| {
                    let r = w.rhs(INT, &[El::Local(ret)]);
                    w.set(&[El::Local(sum)], r);
                });
                // b left out: it reads 0, the first call's 3 reset.
                let four = w.int(4);
                let ret2 = parameter(w, "result", d);
                call_then(w, d, &[("a", four)], |w| {
                    let r = w.rhs(INT, &[El::Local(ret2)]);
                    let s2 = w.variable("sum2", "ValueInfo", Some(INT));
                    w.set(&[El::Local(s2)], r);
                });
                w.end_block(bh);
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert_eq!(vm.local(id, "sum"), Some(Value::Int(5)));
    assert_eq!(vm.local(id, "sum2"), Some(Value::Int(4)));
    let add = vm.program().macro_named("Add").unwrap();
    assert_eq!(
        vm.frame(add, "result"),
        Some(Value::Int(0)),
        "reset once the use block closed"
    );
    assert_eq!(vm.frame(add, "b"), Some(Value::Int(0)));
}

#[test]
fn a_specialization_reads_its_bases_parameters_from_its_own_frame() {
    let p = program(|w| {
        let base = w.op(Class::DefineMacro, "Get Save Param");
        let spec = w.op(Class::DefineMacroSpecialization, "Set Save Param");
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                let at = w.push(base);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let val = parameter(w, "val", base);
                let zero = w.int(0);
                w.refer(val, 0x30, zero);
                w.push(val);
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.end_block(at);
                // The specialization's own parameter names the base, as
                // Giants' do; its body reads it.
                let np = w.stack(StackClass::MacroDefinitions, &[El::Local(base)]);
                w.refer(spec, 0x38, np);
                let at = w.push(spec);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let own = parameter(w, "val", base);
                let zero = w.int(0);
                w.refer(own, 0x30, zero);
                w.push(own);
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                let r = w.rhs(INT, &[El::Local(own)]);
                w.set(&[El::Member("test", "out")], r);
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                let five = w.int(5);
                call_then(w, spec, &[("val", five)], |_| {});
                w.end_block(b);
            },
        );
    });
    let (vm, _) = run(&p, config(Mode::Strict, &["out"]), 1);
    assert_eq!(cell(&vm, "out"), Some(Value::Int(5)));
}

#[test]
fn after_return_a_family_reads_its_callees_returned_parameter() {
    // Macros: a family (a base and its specializations) shares its
    // parameter ops, as Giants' `Calc - Sin`, `Calc - Cosine` and `Calc -
    // Tangent` do (AH.60). In a call's after-return code a parameter naming
    // the callee's definition, or the base it specializes, reads the
    // callee's frame, even where the caller declares that parameter too;
    // binding the call's arguments and the caller's own body read its own.
    let p = program(|w| {
        let base = w.op(Class::DefineMacro, "Double");
        let spec = w.op(Class::DefineMacroSpecialization, "Double Plus 100");
        let angle = parameter(w, "angle", base);
        let result = parameter(w, "result", base);
        let scratch = var(w, "scratch");
        let header = |w: &mut Writer<'_>, def: Location| {
            let at = w.push(def);
            let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
            w.push(angle);
            w.push(result);
            let enter = w.flow(Class::FlowMacro);
            w.push(enter);
            w.close(iface);
            at
        };
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                // Double(angle, result): result = angle * 2.
                let at = header(w, base);
                let a = w.rhs(INT, &[El::Local(angle)]);
                let two = w.int(2);
                let times = w.expr(a, Some((2, two)));
                w.set(&[El::Local(result)], times);
                w.end_block(at);
                // Double Plus 100(angle, result), the same parameter ops:
                // Double(angle = angle + 1), after it test.after = result
                // and scratch = result; then result = scratch + 100.
                let np = w.stack(StackClass::MacroDefinitions, &[El::Local(base)]);
                w.refer(spec, 0x38, np);
                let at = header(w, spec);
                let a = w.rhs(INT, &[El::Local(angle)]);
                let one = w.int(1);
                let plus = w.expr(a, Some((0, one)));
                call_then(w, base, &[("angle", plus)], |w| {
                    let r = w.rhs(INT, &[El::Local(result)]);
                    w.set(&[El::Member("test", "after")], r);
                    let r = w.rhs(INT, &[El::Local(result)]);
                    w.set(&[El::Local(scratch)], r);
                });
                let s = w.rhs(INT, &[El::Local(scratch)]);
                let hundred = w.int(100);
                let plus = w.expr(s, Some((0, hundred)));
                w.set(&[El::Local(result)], plus);
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                let five = w.int(5);
                call_then(w, spec, &[("angle", five)], |w| {
                    let r = w.rhs(INT, &[El::Local(result)]);
                    w.set(&[El::Member("test", "out")], r);
                });
                w.end_block(b);
            },
        );
    });
    let (vm, _) = run(&p, config(Mode::Strict, &["after", "out"]), 1);
    assert_eq!(
        cell(&vm, "after"),
        Some(Value::Int(12)),
        "the callee's result after its return, its argument bound from the caller's angle"
    );
    assert_eq!(
        cell(&vm, "out"),
        Some(Value::Int(112)),
        "the specialization's own result once the nested call closed"
    );
}

#[test]
fn a_behaviour_switch_applies_next_tick_or_this_tick_from_prescript() {
    for from_prescript in [false, true] {
        let p = program(|w| {
            let battle = w.op(Class::UserBehavior, "Battle");
            frame(
                w,
                |_| {},
                |_| {},
                |_| {},
                |w| {
                    if from_prescript {
                        w.set_behavior(battle);
                    }
                },
                |w| {
                    let idle = w.behaviour("idle");
                    w.inc(&[El::Member("test", "idle")]);
                    if !from_prescript {
                        w.set_behavior(battle);
                    }
                    w.end_block(idle);
                    let b = w.push(battle);
                    w.inc(&[El::Member("test", "battle")]);
                    w.end_block(b);
                },
            );
        });
        let mut vm = Vm::new(p, config(Mode::Strict, &["idle", "battle"]));
        let id = vm.add(HANDLE, "Thing", 0);
        vm.tick(id, &mut Bare).unwrap();
        if from_prescript {
            assert_eq!(cell(&vm, "idle"), None);
            assert_eq!(cell(&vm, "battle"), Some(Value::Int(1)));
        } else {
            assert_eq!(cell(&vm, "idle"), Some(Value::Int(1)));
            assert_eq!(
                cell(&vm, "battle"),
                None,
                "the switch waits for the next tick"
            );
        }
        assert_eq!(vm.behaviour(id), Some("battle"));
        vm.tick(id, &mut Bare).unwrap();
        vm.tick(id, &mut Bare).unwrap();
        let battles = if from_prescript { 3 } else { 2 };
        assert_eq!(cell(&vm, "battle"), Some(Value::Int(battles)));
    }
}

#[test]
fn startup_runs_once_and_a_shutdown_ends_the_tick_then_starts_again() {
    let p = program(|w| {
        frame(
            w,
            |w| {
                w.inc(&[El::Member("test", "starts")]);
            },
            |w| {
                w.inc(&[El::Member("test", "shutdowns")]);
            },
            |_| {},
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                w.inc(&[El::Member("test", "ticks")]);
                w.end_block(b);
            },
        );
    });
    let mut vm = Vm::new(p, config(Mode::Strict, &["starts", "shutdowns", "ticks"]));
    let id = vm.add(HANDLE, "Thing", 0);
    vm.tick(id, &mut Bare).unwrap();
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!(cell(&vm, "starts"), Some(Value::Int(1)));
    assert_eq!(cell(&vm, "ticks"), Some(Value::Int(2)));
    vm.request_shutdown(id);
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!(cell(&vm, "shutdowns"), Some(Value::Int(1)));
    assert_eq!(
        cell(&vm, "ticks"),
        Some(Value::Int(2)),
        "SHUTDOWN ends the tick"
    );
    vm.tick(id, &mut Bare).unwrap();
    assert_eq!(cell(&vm, "starts"), Some(Value::Int(2)), "started again");
    assert_eq!(cell(&vm, "ticks"), Some(Value::Int(3)));
}

#[test]
fn every_comparison_code_at_and_either_side_of_the_tolerance_through_the_vm() {
    let inside = value::TOLERANCE * 0.99;
    let outside = value::TOLERANCE * 2.0;
    let lefts = [
        1.0 - outside,
        1.0 - inside,
        1.0,
        1.0 + inside,
        1.0 + outside,
    ];
    let p = program(|w| {
        let xs: Vec<Location> = (0..lefts.len()).map(|k| var(w, &format!("x{k}"))).collect();
        idle(
            w,
            |w| {
                for (k, x) in xs.iter().enumerate() {
                    let lit = w.float(lefts[k]);
                    w.set(&[El::Local(*x)], lit);
                }
            },
            |w| {
                for code in 0..6u32 {
                    for (k, x) in xs.iter().enumerate() {
                        let one = w.float(1.0);
                        let (at, _) = check(w, *x, code, one);
                        let r = w.variable(&format!("r{code}{k}"), "ValueInfo", Some(INT));
                        let yes = w.int(1);
                        w.set(&[El::Local(r)], yes);
                        w.close(at);
                    }
                }
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    let table: [[bool; 5]; 6] = [
        [true, true, true, true, false],
        [false, true, true, true, false],
        [false, true, true, true, true],
        [true, false, false, false, false],
        [false, false, false, false, true],
        [true, false, false, false, true],
    ];
    for (code, row) in table.iter().enumerate() {
        for (k, held) in row.iter().enumerate() {
            let got = vm.local(id, &format!("r{code}{k}"));
            assert_eq!(got.is_some(), *held, "code {code} with {}", lefts[k]);
        }
    }
}

#[test]
fn a_cast_to_integer_truncates_toward_zero() {
    let p = program(|w| {
        let (a, b, c) = (var(w, "a"), var(w, "b"), var(w, "c"));
        idle(
            w,
            |_| {},
            |w| {
                for (target, f) in [(a, 2.99f32), (b, -2.99)] {
                    let lit = w.float(f);
                    w.set(&[El::Member("system", "cast to integer")], lit);
                    let back = w.rhs(INT, &[El::Member("system", "cast to integer")]);
                    w.set(&[El::Local(target)], back);
                }
                // `Calc - Round Number Up`'s shape: cast(x + 0.99).
                let x = w.float(0.5);
                let up = w.float(0.99);
                let sum = w.expr(x, Some((0, up)));
                w.set(&[El::Member("system", "cast to integer")], sum);
                let back = w.rhs(INT, &[El::Member("system", "cast to integer")]);
                w.set(&[El::Local(c)], back);
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert_eq!(vm.local(id, "a"), Some(Value::Int(2)));
    assert_eq!(vm.local(id, "b"), Some(Value::Int(-2)));
    assert_eq!(vm.local(id, "c"), Some(Value::Int(1)));
}

#[test]
fn arithmetic_runs_in_single_precision_in_the_order_written() {
    let p = program(|w| {
        let (big, r, s, q, h) = (
            var(w, "big"),
            var(w, "r"),
            var(w, "s"),
            var(w, "q"),
            var(w, "h"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let lit = w.float(1e8);
                w.set(&[El::Local(big)], lit);
                // (big + 1) - big: 0 in single precision, 1 in double.
                let (b1, b2) = (read(w, big), read(w, big));
                let one = w.int(1);
                let inner = w.expr(b1, Some((0, one)));
                let outer = w.expr(inner, Some((1, b2)));
                w.set(&[El::Local(r)], outer);
                // (big - big) + 1: 1 either way.
                let (b3, b4) = (read(w, big), read(w, big));
                let inner = w.expr(b3, Some((1, b4)));
                let one = w.int(1);
                let outer = w.expr(inner, Some((0, one)));
                w.set(&[El::Local(s)], outer);
                // Integers stay integers; a float makes it a float.
                let (seven, two) = (w.int(7), w.int(2));
                let div = w.expr(seven, Some((3, two)));
                w.set(&[El::Local(q)], div);
                let (seven, two) = (w.int(7), w.float(2.0));
                let div = w.expr(seven, Some((3, two)));
                w.set(&[El::Local(h)], div);
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    assert_eq!(vm.local(id, "r"), Some(Value::Float(0.0)));
    assert_eq!(vm.local(id, "s"), Some(Value::Float(1.0)));
    assert_eq!(vm.local(id, "q"), Some(Value::Int(3)));
    assert_eq!(vm.local(id, "h"), Some(Value::Float(3.5)));
}

fn hurts() -> Arc<Program> {
    program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
                let five = w.int(5);
                w.set(&[El::Member("actorinfo", "health")], five);
                w.inc(&[El::Local(count)]);
            },
        );
    })
}

#[test]
fn strict_mode_halts_naming_the_unmodelled_member() {
    let mut c = Config::new(Mode::Strict);
    c.members.known("actorinfo", "health");
    let mut vm = Vm::new(hurts(), c);
    let id = vm.add(HANDLE, "Enemy_Chompy(1)", 0);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert_eq!(
        halt.reason,
        Reason::UnmodelledMember("actorinfo.health".into())
    );
    assert_eq!(halt.placement, Some(HANDLE));
    assert_eq!(halt.placement_name, "Enemy_Chompy(1)");
    assert_eq!(halt.op, Some(Class::SetValue));
    assert!(halt.script.ends_with("Test.ai"));
    let text = halt.to_string();
    assert!(
        text.contains("actorinfo.health") && text.contains("Enemy_Chompy(1)"),
        "{text}"
    );
    assert_eq!(
        vm.local(id, "count"),
        Some(Value::Int(1)),
        "it stopped mid-tick"
    );
}

#[test]
fn play_mode_stops_one_placements_script_and_runs_the_rest() {
    let healthy = program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
    });
    // One program: the healthy script first, the hurting one second.
    let p = program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
                let five = w.int(5);
                w.set(&[El::Member("actorinfo", "health")], five);
                w.inc(&[El::Local(count)]);
            },
        );
    });
    assert_eq!(healthy.scripts.len(), 1);
    assert_eq!(p.scripts.len(), 2);
    let mut c = Config::new(Mode::Play);
    c.members.known("actorinfo", "health");
    let mut vm = Vm::new(p, c);
    let a = vm.add(1, "A", 0);
    let b = vm.add(2, "B", 1);
    let c = vm.add(3, "C", 0);
    let report = vm.step(&mut Bare).unwrap();
    assert_eq!(report.ticked, 3);
    assert_eq!(report.stopped.len(), 1);
    assert_eq!(report.stopped[0].placement_name, "B");
    let report = vm.step(&mut Bare).unwrap();
    assert_eq!(report.ticked, 2, "B's script stays stopped");
    assert!(report.stopped.is_empty());
    assert_eq!(vm.local(a, "count"), Some(Value::Int(2)));
    assert_eq!(vm.local(c, "count"), Some(Value::Int(2)));
    assert_eq!(vm.local(b, "count"), Some(Value::Int(1)));
    let stop = vm.stopped(b).unwrap();
    assert_eq!(
        stop.reason,
        Reason::UnmodelledMember("actorinfo.health".into())
    );
    assert!(
        vm.log()
            .iter()
            .any(|l| l.placement == Some(2) && l.text.contains("actorinfo.health"))
    );
}

#[test]
fn a_value_cell_reads_its_default_keeps_what_is_written_and_clamps() {
    let p = program(|w| {
        let (r0, r1, r2, r3, mine) = (
            var(w, "r0"),
            var(w, "r1"),
            var(w, "r2"),
            var(w, "r3"),
            var(w, "mine"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let alpha = [El::Member("test", "alpha")];
                let read_alpha = |w: &mut Writer<'_>| w.rhs(INT, &alpha);
                let v = read_alpha(w);
                w.set(&[El::Local(r0)], v);
                let seven = w.int(7);
                w.set(&alpha, seven);
                let v = read_alpha(w);
                w.set(&[El::Local(r1)], v);
                let big = w.int(300);
                w.set(&alpha, big);
                let v = read_alpha(w);
                w.set(&[El::Local(r2)], v);
                reset(w, &alpha);
                let v = read_alpha(w);
                w.set(&[El::Local(r3)], v);
                // Per object: the placement's own alpha is another cell.
                let mine_alpha = [
                    El::Member("optoplevelbehavior", "my"),
                    El::Member("test", "alpha"),
                ];
                let nine = w.int(9);
                w.set(&mine_alpha, nine);
                let v = w.rhs(INT, &mine_alpha);
                w.set(&[El::Local(mine)], v);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.members.value_cell(
        "test",
        "alpha",
        MemberSpec::new(Kind::Int, 0.0, 255.0).with_default(Value::Int(255)),
    );
    let (vm, id) = run(&p, c, 1);
    assert_eq!(
        vm.local(id, "r0"),
        Some(Value::Int(255)),
        "the registered default"
    );
    assert_eq!(vm.local(id, "r1"), Some(Value::Int(7)), "kept");
    assert_eq!(vm.local(id, "r2"), Some(Value::Int(255)), "clamped");
    assert_eq!(vm.local(id, "r3"), Some(Value::Int(255)), "reset");
    assert_eq!(vm.local(id, "mine"), Some(Value::Int(9)));
    let alpha = MemberId::named("test", "alpha");
    assert_eq!(vm.value_cell(None, alpha), Some(Value::Int(255)));
    assert_eq!(
        vm.value_cell(Some(Value::Entity(HANDLE)), alpha),
        Some(Value::Int(9))
    );
}

#[test]
fn the_budget_stops_a_long_loop_and_a_loop_that_never_ends() {
    let long = program(|w| {
        let x = var(w, "x");
        idle(
            w,
            |_| {},
            |w| {
                let n = w.int(1_000_000);
                let at = w.loop_value(n);
                w.inc(&[El::Local(x)]);
                w.end_loop(at);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.budget = 1_000;
    let mut vm = Vm::new(long, c);
    let id = vm.add(HANDLE, "Spinner", 0);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert_eq!(halt.reason, Reason::Budget(1_000));
    // The budget's 1,000 ran; the 1,001st, counted, stopped it.
    assert_eq!(vm.executed(), 1_001);
    // `flow` on a behaviour's block runs it again: forever, but for the
    // budget. In play mode it stops that script and the next tick skips it.
    let forever = program(|w| {
        let x = var(w, "x");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(x)]);
                let flow = w.flow(Class::FlowLoop);
                w.push(flow);
            },
        );
    });
    let mut c = Config::new(Mode::Play);
    c.budget = 500;
    let mut vm = Vm::new(forever, c);
    let id = vm.add(HANDLE, "Spinner", 0);
    let stop = vm.tick(id, &mut Bare).unwrap().expect("stopped");
    assert_eq!(stop.reason, Reason::Budget(500));
    let before = vm.executed();
    assert_eq!(vm.tick(id, &mut Bare).unwrap(), None);
    assert_eq!(vm.executed(), before);
}

#[test]
fn sets_combine_grow_iterate_and_filter() {
    let p = program(|w| {
        let s = w.variable("s", "ScriptSet", Some("ValueInfo"));
        let names = w.variable("names", "ScriptSet", None);
        let total = var(w, "total");
        let seen = var(w, "seen");
        let low = var(w, "low");
        let high = var(w, "high");
        let combine = |w: &mut Writer<'_>, code: u32, name: &str| {
            let lhs = w.stack(StackClass::Set, &[El::Local(names)]);
            let rhs = w.reference(&[El::Named("SpriteInfo", name)], None);
            let o = w.op(Class::ChangeMembership, "set [set]||to");
            w.refer(o, 0x1C, lhs);
            w.word(o, 0x20, code);
            w.refer(o, 0x24, rhs);
            w.push(o);
        };
        idle(
            w,
            |_| {},
            |w| {
                // names: add A, add A again (no duplicate), append B twice,
                // remove one B, add C: A B C.
                combine(w, 0, "A");
                combine(w, 0, "A");
                combine(w, 4, "B");
                combine(w, 4, "B");
                combine(w, 1, "B");
                combine(w, 0, "C");
                // s: count set to 3 grows three number cells; write 7, 3, 5.
                let three = w.int(3);
                w.set(&[El::Local(s), El::Member("setvariant", "count")], three);
                for (i, v) in [7, 3, 5].into_iter().enumerate() {
                    let set_ref = w.reference(&[El::Local(s)], None);
                    let lit = w.int(v);
                    let idx = w.int(i as i32);
                    let elem = w.variable(&format!("e{i}"), "ScriptReference", None);
                    let o = w.op(Class::SetReference, "set *reference||to");
                    let lhs = w.stack(StackClass::Reference, &[El::Local(elem)]);
                    w.refer(o, 0x1C, lhs);
                    w.refer(o, 0x20, set_ref);
                    w.refer(o, 0x24, idx);
                    w.push(o);
                    w.set(
                        &[
                            El::Local(elem),
                            El::Member("abstractscriptgroup", "valuedereferencer"),
                        ],
                        lit,
                    );
                }
                // for each e in s: total += e; inc seen
                let lhs = w.stack(StackClass::Set, &[El::Local(s)]);
                let zero = w.int(0);
                let (at, fe) = w.open(Class::ForEach, "for each");
                w.refer(fe, 0x20, lhs);
                w.refer(fe, 0x28, zero);
                let item = w.rhs(INT, &[El::Local(fe)]);
                let t = read(w, total);
                let sum = w.expr(t, Some((0, item)));
                w.set(&[El::Local(total)], sum);
                w.inc(&[El::Local(seen)]);
                w.end_loop(at);
                // find subset of s whose value > 4: 5 then 7, ascending.
                let path = w.stack(StackClass::Value, &[El::Local(s), El::Null]);
                let four = w.int(4);
                let (at, fs) = w.open(Class::FindSubSet, "find subset|of||||subset");
                w.refer(fs, 0x1C, path);
                w.word(fs, 0x20, 4);
                w.refer(fs, 0x24, four);
                let first = w.rhs(INT, &[El::Local(fs), El::Member("setvariant", "first")]);
                w.set(&[El::Local(low)], first);
                let last = w.rhs(INT, &[El::Local(fs), El::Member("setvariant", "last")]);
                w.set(&[El::Local(high)], last);
                w.end_block(at);
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    let names = vm.local(id, "names").unwrap();
    let named: Vec<String> = vm
        .set(names)
        .unwrap()
        .iter()
        .map(|v| match v {
            Value::Named(_, n) => vm.program().sym(*n).to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(named, ["a", "b", "c"]);
    assert_eq!(vm.local(id, "total"), Some(Value::Int(15)));
    assert_eq!(vm.local(id, "seen"), Some(Value::Int(3)));
    assert_eq!(vm.local(id, "low"), Some(Value::Int(5)));
    assert_eq!(vm.local(id, "high"), Some(Value::Int(7)));
}

#[test]
fn a_placement_off_the_whitelist_stays_inert() {
    let p = program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.whitelist = Whitelist::only(&["Enemy_Chompy(1)"], &[]);
    let mut vm = Vm::new(p.clone(), c);
    let on = vm.add(1, "Enemy_Chompy(1)", 0);
    let off = vm.add(2, "Setup_002_Controller", 0);
    let report = vm.step(&mut Bare).unwrap();
    assert_eq!(report.ticked, 1);
    assert_eq!(vm.local(on, "count"), Some(Value::Int(1)));
    assert_eq!(vm.local(off, "count"), None);
    assert_eq!(vm.inert(), [2]);
    // Listed by script instead.
    let mut c = Config::new(Mode::Strict);
    c.whitelist = Whitelist::only(&[], &["Test.ai"]);
    let mut vm = Vm::new(p, c);
    vm.add(3, "Anything", 0);
    assert!(vm.inert().is_empty());
}

/// An embedder's remove (openspec AH.27: a placement a run carries over as
/// taken off) stops its script for good, as a script's `remove` does: no
/// tick after it, the other placement ticking on; a second remove, or one
/// of a handle nothing runs under, changes nothing.
#[test]
fn an_embedders_remove_stops_a_placements_script_for_good() {
    let p = program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let coin = vm.add(1, "Coin_spinning(1)", 0);
    let other = vm.add(2, "Coin_spinning(2)", 0);
    vm.step(&mut Bare).unwrap();
    assert!(vm.remove(1));
    assert!(vm.removed(coin));
    for _ in 0..3 {
        vm.step(&mut Bare).unwrap();
    }
    assert_eq!(vm.local(coin, "count"), Some(Value::Int(1)));
    assert_eq!(vm.local(other, "count"), Some(Value::Int(4)));
    assert!(!vm.remove(1), "already off");
    assert!(!vm.remove(99), "nothing runs under it");
    assert!(!vm.removed(other));
}

/// A placement added again under a handle whose placement was taken off
/// (openspec AH.105: a character's file joining the run again after his
/// swap took it off) runs under it from its first tick: it ticks from its
/// own start, the handle names it (asleep, an activator, removed again),
/// and the one taken off stays off; one added under a live placement's
/// handle leaves the handle naming the first.
#[test]
fn a_placement_added_again_under_a_handle_taken_off_runs_under_it() {
    let p = program(|w| {
        let count = var(w, "count");
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(count)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let first = vm.add(1, "009_Eruptor_Def", 0);
    vm.step(&mut Bare).unwrap();
    vm.step(&mut Bare).unwrap();
    assert!(vm.remove(1));
    let again = vm.add(1, "009_Eruptor_Def", 0);
    assert_ne!(again, first);
    assert_eq!(vm.placement_of(1), Some(again));
    vm.step(&mut Bare).unwrap();
    assert_eq!(
        vm.local(again, "count"),
        Some(Value::Int(1)),
        "from its start"
    );
    assert_eq!(
        vm.local(first, "count"),
        Some(Value::Int(2)),
        "off for good"
    );
    assert!(vm.set_activator(1, true));
    assert!(vm.is_activator(again) && !vm.is_activator(first));
    assert!(vm.set_asleep(1, true));
    vm.step(&mut Bare).unwrap();
    assert_eq!(vm.local(again, "count"), Some(Value::Int(1)), "asleep");
    assert!(vm.remove(1));
    assert!(vm.removed(again));
    // Under a live placement's handle the first keeps it.
    let live = vm.add(2, "Coin_spinning(1)", 0);
    let twin = vm.add(2, "Coin_spinning(1)", 0);
    assert_eq!(vm.placement_of(2), Some(live));
    assert_ne!(twin, live);
}

#[test]
fn a_scripts_rolls_draw_from_the_generator_in_the_order_written() {
    // bite = 11 + random(3), then frac = random(0.5), each tick: one number
    // a roll, in the order written, read as the generator's own `random`.
    let p = program(|w| {
        let (bite, frac) = (var(w, "bite"), var(w, "frac"));
        idle(
            w,
            |_| {},
            |w| {
                let eleven = w.int(11);
                let roll = w.literal(INT, 3, true);
                let sum = w.expr(eleven, Some((0, roll)));
                w.set(&[El::Local(bite)], sum);
                let half = w.literal("FloatMeasurement", 0.5f32.to_bits(), true);
                w.set(&[El::Local(frac)], half);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Thing", 0);
    let mut host = Rolling(Generator::seeded(5150));
    let mut twin = Generator::seeded(5150);
    let mut seen = [0u32; 4];
    for tick in 0..1_000 {
        vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
        let Some(Value::Int(n)) = twin.random(Value::Int(3)) else {
            unreachable!()
        };
        assert_eq!(
            vm.local(id, "bite"),
            Some(Value::Int(11 + n)),
            "tick {tick}"
        );
        assert_eq!(
            vm.local(id, "frac"),
            twin.random(Value::Float(0.5)),
            "tick {tick}"
        );
        seen[n as usize] += 1;
    }
    assert!(seen.iter().all(|&c| c > 0), "{seen:?}");
    assert_eq!(host.0, twin);
}

// ---- The review's probes (2026-10-20): one op list per flow rule the crate
// docs state that the tests above don't pin, the host seam (decision 21) and
// a specialization's parameter read while it calls on.

#[test]
fn flow_end_at_a_scripts_top_ends_the_tick_and_the_next_runs_again() {
    // Checks and flows: `flow end` at the top of a script ends the tick; the
    // ops after it never run, and the next tick starts over.
    let p = program(|w| {
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Member("test", "before")]);
                w.flow_end();
                w.inc(&[El::Member("test", "after")]);
            },
        );
    });
    let (vm, _) = run(&p, config(Mode::Strict, &["before", "after"]), 2);
    assert_eq!(cell(&vm, "before"), Some(Value::Int(2)));
    assert_eq!(cell(&vm, "after"), None);
}

#[test]
fn for_each_turns_its_set_by_the_offset_and_runs_either_way() {
    // Loops: the set rotated left by the offset (modulo the count, negatives
    // wrapping), then run forward (direction 0) or backward (1).
    for (offset, dir, want) in [
        (0, 0, ["a", "b", "c", "d"]),
        (1, 0, ["b", "c", "d", "a"]),
        (1, 1, ["a", "d", "c", "b"]),
        (-1, 0, ["d", "a", "b", "c"]),
        (6, 0, ["c", "d", "a", "b"]),
    ] {
        let p = program(|w| {
            let names = w.variable("names", "ScriptSet", None);
            let order = w.variable("order", "ScriptSet", None);
            let append = |w: &mut Writer<'_>, set: Location, rhs: Location| {
                let lhs = w.stack(StackClass::Set, &[El::Local(set)]);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 4);
                w.refer(o, 0x24, rhs);
                w.push(o);
            };
            idle(
                w,
                |_| {},
                |w| {
                    for name in ["A", "B", "C", "D"] {
                        let rhs = w.reference(&[El::Named("SpriteInfo", name)], None);
                        append(w, names, rhs);
                    }
                    let lhs = w.stack(StackClass::Set, &[El::Local(names)]);
                    let off = w.int(offset);
                    let (at, fe) = w.open(Class::ForEach, "for each");
                    w.refer(fe, 0x20, lhs);
                    w.word(fe, 0x24, dir);
                    w.refer(fe, 0x28, off);
                    let item = w.reference(&[El::Local(fe)], None);
                    append(w, order, item);
                    w.end_loop(at);
                },
            );
        });
        let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
        let order = vm.local(id, "order").unwrap();
        let got: Vec<String> = vm
            .set(order)
            .unwrap()
            .iter()
            .map(|v| match v {
                Value::Named(_, n) => vm.program().sym(*n).to_owned(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(got, want, "offset {offset}, direction {dir}");
    }
}

#[test]
fn loop_value_reads_its_count_once_truncated_and_counts_from_0() {
    // Loops: the count read once as an integer (2.7 runs twice, 0.99 not at
    // all), 0 or less skips, the index 0 to count - 1. The body grows the
    // count's variable, which changes nothing.
    for (count, passes, index_sum) in [
        (2.7f32, 2, 1),
        (4.0, 4, 6),
        (0.99, 0, 0),
        (0.0, 0, 0),
        (-3.0, 0, 0),
    ] {
        let p = program(|w| {
            let (n, done, sum) = (var(w, "n"), var(w, "passes"), var(w, "sum"));
            idle(
                w,
                |_| {},
                |w| {
                    let lit = w.float(count);
                    w.set(&[El::Local(n)], lit);
                    let rn = read(w, n);
                    let (at, lp) = w.open(Class::LoopValue, "loop value|||loop index");
                    w.refer(lp, 0x20, rn);
                    w.inc(&[El::Local(done)]);
                    let s = read(w, sum);
                    let i = w.rhs(INT, &[El::Local(lp)]);
                    let plus = w.expr(s, Some((0, i)));
                    w.set(&[El::Local(sum)], plus);
                    w.inc(&[El::Local(n)]);
                    w.end_loop(at);
                },
            );
        });
        let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
        let got = |name| vm.local(id, name).unwrap_or(Value::Int(0));
        assert_eq!(got("passes"), Value::Int(passes), "count {count}");
        assert_eq!(got("sum"), Value::Int(index_sum), "count {count}");
    }
}

/// A host keeping its members in a map by receiver, answering "nothing" for
/// the members in `absent`, and logging every get and set in order.
#[derive(Default)]
struct World {
    cells: std::collections::HashMap<(Option<u64>, MemberId), Value>,
    absent: Vec<MemberId>,
    seen: Vec<String>,
}

impl World {
    fn key(at: &Access) -> (Option<u64>, MemberId) {
        let receiver = match at.receiver {
            None => None,
            Some(Value::Entity(e)) => Some(e),
            Some(other) => panic!("a member read on {other:?}"),
        };
        (receiver, at.member)
    }
}

impl Host for World {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        if self.absent.contains(&at.member) {
            return Err(HostFault::Nothing);
        }
        let v = self
            .cells
            .get(&Self::key(at))
            .copied()
            .unwrap_or(Value::Int(0));
        self.seen.push(format!("get {:x} {v:?}", at.placement));
        Ok(v)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        if self.absent.contains(&at.member) {
            return Err(HostFault::Nothing);
        }
        self.seen.push(format!("set {:x} {value:?}", at.placement));
        self.cells.insert(Self::key(at), value);
        Ok(())
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.cells.remove(&Self::key(at));
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        0
    }
}

#[test]
fn a_path_through_nothing_skips_a_check_and_drops_a_write_and_the_tick_runs_on() {
    // Checks and flows, Members: a check whose left side can't be read skips
    // its block; a write through nothing (a null receiver, a member the host
    // answers with nothing) drops; neither stops the script.
    let p = program(|w| {
        idle(
            w,
            |_| {},
            |w| {
                let gone = [
                    El::Member("optoplevelbehavior", "my"),
                    El::Member("actorinfo", "target"),
                ];
                let zero = w.int(0);
                let at = w.check(&gone, 1, zero);
                w.inc(&[El::Member("test", "inside")]);
                w.end_block(at);
                let five = w.int(5);
                w.set(
                    &[
                        El::Member("abstractscriptgroup", "null"),
                        El::Member("actorinfo", "health"),
                    ],
                    five,
                );
                let five = w.int(5);
                w.set(&gone, five);
                w.inc(&[El::Member("test", "after")]);
            },
        );
    });
    let mut c = config(Mode::Strict, &["inside", "after"]);
    c.members
        .host("actorinfo", "target", int_cell())
        .host("actorinfo", "health", int_cell());
    let mut vm = Vm::new(p, c);
    let id = vm.add(HANDLE, "Thing", 0);
    let mut world = World {
        absent: vec![MemberId::named("actorinfo", "target")],
        ..World::default()
    };
    vm.tick(id, &mut world).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(cell(&vm, "inside"), None, "the check skipped its block");
    assert_eq!(cell(&vm, "after"), Some(Value::Int(1)), "the tick ran on");
    assert!(world.cells.is_empty(), "nothing written: {:?}", world.cells);
}

#[test]
fn a_script_reads_its_own_host_write_in_the_tick_and_a_later_placement_sees_it() {
    // Design decision 21: a member's set goes straight to the host and its
    // get straight back (nothing cached or deferred in the VM), so a script
    // reads its own write within the tick, as the hurt chain lowers health
    // and checks it, and a placement ticked later in the same step reads it.
    let p = program(|w| {
        // The hurt: my.health = 5; my.health = my.health - 1;
        // if my.health == 4 { saw = 1 }; the shared health = my.health.
        let saw = var(w, "saw");
        idle(
            w,
            |_| {},
            |w| {
                let health = [
                    El::Member("optoplevelbehavior", "my"),
                    El::Member("actorinfo", "health"),
                ];
                let five = w.int(5);
                w.set(&health, five);
                let h = w.rhs(INT, &health);
                let one = w.int(1);
                let less = w.expr(h, Some((1, one)));
                w.set(&health, less);
                let four = w.int(4);
                let at = w.check(&health, 1, four);
                let yes = w.int(1);
                w.set(&[El::Local(saw)], yes);
                w.end_block(at);
                let h = w.rhs(INT, &health);
                w.set(&[El::Member("actorinfo", "health")], h);
            },
        );
        // A later placement: seen = the shared health.
        let seen = var(w, "seen");
        idle(
            w,
            |_| {},
            |w| {
                let h = w.rhs(INT, &[El::Member("actorinfo", "health")]);
                w.set(&[El::Local(seen)], h);
            },
        );
    });
    let mut c = Config::new(Mode::Strict);
    c.members.host("actorinfo", "health", int_cell());
    let mut vm = Vm::new(p, c);
    let hurt = vm.add(0xA, "Enemy_Chompy(1)", 0);
    let look = vm.add(0xB, "Setup_001_Controller", 1);
    let mut world = World::default();
    let report = vm.step(&mut world).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(report.ticked, 2);
    assert_eq!(vm.local(hurt, "saw"), Some(Value::Int(1)), "its own write");
    assert_eq!(vm.local(look, "seen"), Some(Value::Int(4)), "the same step");
    assert_eq!(
        world.seen,
        [
            "set a Int(5)",
            "get a Int(5)",
            "set a Int(4)",
            "get a Int(4)",
            "get a Int(4)",
            "set a Int(4)",
            "get b Int(4)",
        ]
    );
}

#[test]
fn the_tolerance_holds_to_the_float_through_the_vm() {
    // Comparisons: d exactly the tolerance still compares equal and not
    // greater, the next float out the other way, the same below 0; 0 and a
    // literal as the operands, so d carries no rounding of its own.
    let e = value::TOLERANCE;
    let out = f32::from_bits(e.to_bits() + 1);
    // Left 0 against right r: d = -r.
    let rights = [out, e, 0.0, -e, -out];
    let p = program(|w| {
        let zero = var(w, "zero");
        idle(
            w,
            |_| {},
            |w| {
                for code in 0..6u32 {
                    for (k, r) in rights.iter().enumerate() {
                        let lit = w.float(*r);
                        let (at, _) = check(w, zero, code, lit);
                        let hit = w.variable(&format!("r{code}{k}"), "ValueInfo", Some(INT));
                        let yes = w.int(1);
                        w.set(&[El::Local(hit)], yes);
                        w.close(at);
                    }
                }
            },
        );
    });
    let (vm, id) = run(&p, Config::new(Mode::Strict), 1);
    // d = -out, -e, 0, e, out, per code 0 <=, 1 ==, 2 >=, 3 <, 4 >, 5 !=.
    let table: [[bool; 5]; 6] = [
        [true, true, true, true, false],
        [false, true, true, true, false],
        [false, true, true, true, true],
        [true, false, false, false, false],
        [false, false, false, false, true],
        [true, false, false, false, true],
    ];
    for (code, row) in table.iter().enumerate() {
        for (k, held) in row.iter().enumerate() {
            let got = vm.local(id, &format!("r{code}{k}"));
            assert_eq!(got.is_some(), *held, "code {code} with d = {}", -rights[k]);
        }
    }
}

#[test]
fn a_specializations_parameter_reads_its_frame_while_it_calls_on() {
    // Macros, Variables: a specialization's own parameter names the base and
    // reads the specialization's frame while it runs, and it still runs
    // while it binds a nested call's arguments (to another macro or to its
    // own base) and in a nested call's after-return code; the base's frame
    // is never bound there.
    let p = program(|w| {
        let base = w.op(Class::DefineMacro, "Get Save Param");
        let spec = w.op(Class::DefineMacroSpecialization, "Set Save Param");
        let sink = w.op(Class::DefineMacro, "Sink");
        let header = |w: &mut Writer<'_>, def: Location, name: &str, owner: Location| {
            let at = w.push(def);
            let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
            let own = parameter(w, name, owner);
            let zero = w.int(0);
            w.refer(own, 0x30, zero);
            w.push(own);
            let enter = w.flow(Class::FlowMacro);
            w.push(enter);
            w.close(iface);
            (at, own)
        };
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                // Get Save Param(val): nothing.
                let (at, _) = header(w, base, "val", base);
                w.end_block(at);
                // Sink(m): test.sunk = m.
                let (at, m) = header(w, sink, "m", sink);
                let r = w.rhs(INT, &[El::Local(m)]);
                w.set(&[El::Member("test", "sunk")], r);
                w.end_block(at);
                // Set Save Param(val), its parameter naming the base:
                // Sink(m = val), after it test.after = val; then
                // Get Save Param(val = val), after it test.based = its val.
                let np = w.stack(StackClass::MacroDefinitions, &[El::Local(base)]);
                w.refer(spec, 0x38, np);
                let (at, own) = header(w, spec, "val", base);
                let arg = w.rhs(INT, &[El::Local(own)]);
                call_then(w, sink, &[("m", arg)], |w| {
                    let r = w.rhs(INT, &[El::Local(own)]);
                    w.set(&[El::Member("test", "after")], r);
                });
                let arg = w.rhs(INT, &[El::Local(own)]);
                let ret = parameter(w, "val", base);
                call_then(w, base, &[("val", arg)], |w| {
                    let r = w.rhs(INT, &[El::Local(ret)]);
                    w.set(&[El::Member("test", "based")], r);
                });
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                let five = w.int(5);
                call_then(w, spec, &[("val", five)], |_| {});
                w.end_block(b);
            },
        );
    });
    let (vm, _) = run(&p, config(Mode::Strict, &["sunk", "after", "based"]), 1);
    assert_eq!(
        cell(&vm, "sunk"),
        Some(Value::Int(5)),
        "an argument passed on"
    );
    assert_eq!(
        cell(&vm, "after"),
        Some(Value::Int(5)),
        "after a nested call"
    );
    assert_eq!(
        cell(&vm, "based"),
        Some(Value::Int(5)),
        "passed to its base"
    );
}
