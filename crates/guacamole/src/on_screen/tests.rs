//! The on-screen asks found on scripts built in code (openspec AH.107).

use super::*;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::igz::{Igz, Location};
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph};
use guac_script::{Role, Unit};

const MY: El<'static> = El::Member("optoplevelbehavior", "my");
const COLLISION: El<'static> = El::Member("positionmeasurement", "collision");
const VIEWPORT: El<'static> = El::Member("viewportinfo", "default");

/// `if <path>.@<other> == 0`, its block left open.
fn ask(w: &mut Writer<'_>, path: &[El<'_>], other: &[El<'_>]) -> usize {
    let lhs = w.value(path);
    let at = w.reference(other, None);
    w.refer(lhs, 0x18, at);
    let zero = w.int(0);
    let (p, o) = w.open(Class::CheckValue, "if");
    w.refer(o, 0x1C, lhs);
    w.word(o, 0x20, 1);
    w.refer(o, 0x24, zero);
    p
}

/// A script: PRESCRIPT holding STARTUP, SHUTDOWN and a MACROS section
/// `macros` writes, then one behaviour `body` writes.
fn script(
    w: &mut Writer<'_>,
    name: &str,
    macros: impl FnOnce(&mut Writer<'_>) -> Option<Location>,
    body: impl FnOnce(&mut Writer<'_>, Option<Location>),
) {
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    w.end_block(start);
    let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
    w.end_block(shut);
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    let def = macros(w);
    w.end_block(section);
    w.end_block(pre);
    let b = w.behaviour("idle");
    body(w, def);
    w.end_block(b);
    w.end(name);
}

/// Five scripts: one asking a point of its own against the gameplay
/// camera; one asking a point against another actor and reading the
/// camera's field of view, which isn't an ask, beside a macro holding an
/// ask it never calls; one calling a macro that asks the camera about its
/// point the other way round; one naming that macro as a value (an update
/// macro put in a set), which reaches it too; one calling it through a
/// stub of its name, as a file calls another file's macro.
fn program() -> Program {
    let mut b = IgzBuilder::new();
    {
        let w = &mut Writer::new(&mut b);
        script(
            w,
            "C:/tfb/Synthetic/Asks.ai",
            |_| None,
            |w, _| {
                let c = ask(w, &[MY, COLLISION], &[VIEWPORT]);
                w.end_block(c);
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Quiet.ai",
            |w| {
                let (d, def) = w.define("Synthetic - Never Called", &[]);
                let c = ask(w, &[MY, COLLISION], &[VIEWPORT]);
                w.end_block(c);
                w.end_block(d);
                Some(def)
            },
            |w, _| {
                let c = ask(w, &[MY, COLLISION], &[MY]);
                w.end_block(c);
                let one = w.int(1);
                w.set(
                    &[VIEWPORT, El::Member("viewportinfo", "field of view")],
                    one,
                );
            },
        );
        let mut called = None;
        script(
            w,
            "C:/tfb/Synthetic/Calls.ai",
            |w| {
                let (d, def) = w.define("Synthetic - Off Screen", &[]);
                let c = ask(w, &[VIEWPORT, COLLISION], &[MY]);
                w.end_block(c);
                w.end_block(d);
                called = Some(def);
                Some(def)
            },
            |w, def| {
                w.call(def.unwrap(), &[]);
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Names.ai",
            |_| None,
            |w, _| {
                let set = w.variable("update macros", "ScriptSet", Some("ScriptMacro"));
                let lhs = w.stack(guac_formats::script::StackClass::Set, &[El::Local(set)]);
                let rhs = w.reference(&[El::Local(called.unwrap())], None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, rhs);
                w.push(o);
            },
        );
        script(
            w,
            "C:/tfb/Synthetic/Stub.ai",
            |_| None,
            |w, _| {
                let stub = w.op(Class::DefineMacro, "Synthetic - Off Screen");
                w.call(stub, &[]);
            },
        );
    }
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let graph = Graph::read(&igz).unwrap();
    Program::link(&[Unit {
        name: "level",
        role: Role::Other,
        igz: &igz,
        graph: &graph,
    }])
    .unwrap()
}

fn named(p: &Program, suffix: &str) -> ScriptId {
    p.script_named(suffix).unwrap()
}

/// A script asking its own point against the gameplay camera counts one
/// ask, in its own ops; one asking against another actor, or reading the
/// camera's other members, counts none, a macro it never calls aside.
#[test]
fn a_script_that_asks_counts_one_and_one_that_doesnt_none() {
    let p = program();
    let (asks, _) = reached(&p, named(&p, "Asks.ai"));
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].def, None);
    assert_eq!(asks[0].script, named(&p, "Asks.ai"));
    let (quiet, defs) = reached(&p, named(&p, "Quiet.ai"));
    assert!(quiet.is_empty(), "{quiet:?}");
    assert!(defs.is_empty(), "{defs:?}");
}

/// An ask inside a macro is the caller's, whichever way round it asks; a
/// macro named as a value, or called through a stub, is reached as a call
/// is; every ask in the
/// program is found wherever it sits, a macro nothing calls included.
#[test]
fn an_ask_in_a_macro_is_its_callers() {
    let p = program();
    let (calls, defs) = reached(&p, named(&p, "Calls.ai"));
    assert_eq!(calls.len(), 1, "{calls:?}");
    let def = calls[0].def.expect("in the macro");
    assert_eq!(p.defs[def as usize].display, "Synthetic - Off Screen");
    assert!(defs.contains(&def));
    let (names, _) = reached(&p, named(&p, "Names.ai"));
    assert_eq!(names, calls);
    let (stub, _) = reached(&p, named(&p, "Stub.ai"));
    assert_eq!(stub, calls);
    let every = every_ask(&p);
    assert_eq!(every.len(), 3, "{every:?}");
    assert!(every.iter().any(|a| {
        a.def
            .is_some_and(|d| p.defs[d as usize].display == "Synthetic - Never Called")
    }));
}
