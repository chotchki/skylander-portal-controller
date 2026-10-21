//! What AH.17's hero needed of the VM, each rule on scripts built in code:
//! a reference bound to a host member's number, a measurement's other point
//! carried on its path to the read, and a native call with arguments and
//! returns.

use std::collections::HashMap;

use super::*;

const FLOAT: &str = "FloatMeasurement";

/// Keeps `test` members by name; `test.distance` answers 7 measured from
/// `HANDLE` and -1 from anything else (or nothing).
#[derive(Default)]
struct Members {
    values: HashMap<MemberId, Value>,
    reads: u32,
}

impl Host for Members {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.reads += 1;
        if at.member == MemberId::named("test", "distance") {
            return Ok(Value::Float(
                if at.other_point == Some(Value::Entity(HANDLE)) {
                    7.0
                } else {
                    -1.0
                },
            ));
        }
        Ok(*self.values.get(&at.member).unwrap_or(&Value::Float(0.0)))
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.values.insert(at.member, value);
        Ok(())
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.values.remove(&at.member);
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        0
    }
}

/// A number parameter of `def` named `name`, bound to `rhs`.
fn parameter_of(w: &mut Writer<'_>, def: Location, name: &str, rhs: Location) -> Location {
    let o = w.op(Class::MacroParameter, "parameter");
    w.text(o, 0x1C, name);
    w.class_word(o, 0x20, "ValueInfo");
    w.word(o, 0x28, u32::MAX);
    w.refer(o, 0x30, rhs);
    w.refer(o, 0x38, def);
    o
}

/// An INTERFACE block of `params`.
fn interface_of(w: &mut Writer<'_>, params: &[Location]) {
    let (p, _) = w.open(Class::MacroInterface, "INTERFACE");
    for &o in params {
        w.push(o);
    }
    let flow = w.flow(Class::FlowMacro);
    w.push(flow);
    w.close(p);
}

fn config() -> Config {
    let mut c = Config::new(Mode::Strict);
    for name in ["speed", "distance"] {
        c.members
            .host("test", name, MemberSpec::new(Kind::Float, 0.0, 0.0));
    }
    c
}

/// `setref r = my.test.speed` binds the member itself (the hero's movement
/// data keeps his turn rate so): a write through the dereferencer lands in
/// the host, a read through it asks the host anew, and two references to
/// the one member compare equal.
#[test]
fn a_reference_binds_a_host_members_number() {
    let p = program(|w| {
        let r = w.variable("r", "ScriptReference", None);
        let s = w.variable("s", "ScriptReference", None);
        let got = w.variable("got", "ValueInfo", Some(FLOAT));
        let plain = w.variable("plain", "ValueInfo", Some(FLOAT));
        let same = var(w, "same");
        let speed = [
            El::Member("optoplevelbehavior", "my"),
            El::Member("test", "speed"),
        ];
        idle(
            w,
            |w| {
                for v in [r, s] {
                    let l = w.value(&[El::Local(v)]);
                    let rhs = w.reference(&speed, None);
                    let o = w.op(Class::SetReference, "set reference");
                    w.refer(o, 0x1C, l);
                    w.refer(o, 0x20, rhs);
                    w.push(o);
                }
                let five = w.float(5.0);
                w.set(
                    &[
                        El::Local(r),
                        El::Member("abstractscriptgroup", "valuedereferencer"),
                    ],
                    five,
                );
            },
            |w| {
                let v = w.rhs(
                    FLOAT,
                    &[
                        El::Local(r),
                        El::Member("abstractscriptgroup", "valuedereferencer"),
                    ],
                );
                w.set(&[El::Local(got)], v);
                // Read as a number, the reference reads the member.
                let v = w.rhs(FLOAT, &[El::Local(r)]);
                w.set(&[El::Local(plain)], v);
                let l = w.value(&[El::Local(r)]);
                let rhs = w.reference(&[El::Local(s)], None);
                let (c, o) = w.open(Class::CheckReference, "if reference");
                w.refer(o, 0x1C, l);
                w.word(o, 0x20, 1);
                w.refer(o, 0x24, rhs);
                w.inc(&[El::Local(same)]);
                w.end_block(c);
            },
        );
    });
    let mut vm = Vm::new(p, config());
    let id = vm.add(HANDLE, "Hero", 0);
    let mut host = Members::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    let speed = MemberId::named("test", "speed");
    assert_eq!(host.values.get(&speed), Some(&Value::Float(5.0)));
    assert_eq!(vm.local(id, "got"), Some(Value::Float(5.0)));
    assert_eq!(vm.local(id, "plain"), Some(Value::Float(5.0)));
    assert_eq!(vm.local(id, "same"), Some(Value::Int(1)));
    // The host moves it on: read through the reference, the new number.
    host.values.insert(speed, Value::Float(9.0));
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "got"), Some(Value::Float(9.0)));
    assert_eq!(vm.local(id, "plain"), Some(Value::Float(9.0)));
}

/// A measurement's other point is the path's own (`_otherPoint`), still
/// there when the host is asked for the member at the path's end.
#[test]
fn a_measurement_reads_from_its_paths_other_point() {
    let p = program(|w| {
        let got = w.variable("got", "ValueInfo", Some(FLOAT));
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(
                    FLOAT,
                    &[
                        El::Member("optoplevelbehavior", "my"),
                        El::Member("test", "distance"),
                    ],
                );
                let from = w.reference(&[El::Member("optoplevelbehavior", "my")], None);
                w.refer(v, 0x18, from);
                w.set(&[El::Local(got)], v);
            },
        );
    });
    let mut vm = Vm::new(p, config());
    let id = vm.add(HANDLE, "Hero", 0);
    let mut host = Members::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "got"), Some(Value::Float(7.0)));
}

/// A native call binds each argument to the parameter the macro's own
/// INTERFACE names (any case) and reads what it asks back from the callee's
/// frame as the body returns; a parameter the macro doesn't declare stops.
#[test]
fn a_native_call_passes_arguments_and_reads_returns() {
    let p = program(|w| {
        let def = w.op(Class::DefineMacro, "Add One");
        let zero = w.int(0);
        let a = parameter_of(w, def, "a", zero);
        let zero = w.int(0);
        let out = parameter_of(w, def, "out", zero);
        frame(
            w,
            |_| {},
            |_| {},
            |w| {
                let at = w.push(def);
                interface_of(w, &[a, out]);
                let x = w.rhs(INT, &[El::Local(a)]);
                let one = w.int(1);
                let sum = w.expr(x, Some((0, one)));
                w.set(&[El::Local(out)], sum);
                w.end_block(at);
            },
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                w.end_block(b);
            },
        );
    });
    let mut vm = Vm::new(p, config());
    let mut host = Members::default();
    let (_, got) = vm
        .call_with(
            None,
            "Add One",
            &[("A", Value::Int(4))],
            &["out"],
            &mut host,
        )
        .unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(got, [Some(Value::Int(5))]);
    let halt = vm
        .call_with(None, "Add One", &[("b", Value::Int(4))], &[], &mut host)
        .unwrap_err();
    assert!(
        matches!(&halt.reason, Reason::Unsupported(s) if s.contains("no parameter")),
        "{halt}"
    );
}
