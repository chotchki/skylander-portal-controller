//! What AH.21's breakables needed of the VM, each rule on scripts built
//! in code: a placement with no script finding its file's tags (the loot
//! system's init reading each breakable's `archetype ref`), and a kind's
//! running loot count stepping in single precision with the comparisons'
//! tolerance, the original's shape in our ops. A colour's bytes are
//! `value`'s.

use super::init::{actor, program_and_objects, script};
use super::*;

const FLOAT: &str = "FloatMeasurement";

/// `set reference <lhs> = <rhs>`.
fn setref(w: &mut Writer<'_>, lhs: &[El<'_>], rhs: &[El<'_>]) {
    let l = w.value(lhs);
    let r = w.reference(rhs, None);
    let o = w.op(Class::SetReference, "set reference");
    w.refer(o, 0x1C, l);
    w.refer(o, 0x20, r);
    w.push(o);
}

/// A placement with no script has its file's tags too (openspec AH.21): the
/// loot system's init finds each breakable's `archetype ref` from its own
/// script and gets the archetype, the same object however it's named; a
/// tag the breakable lacks isn't found, and the scratch it would have set
/// keeps what it held.
#[test]
fn a_scriptless_placements_tag_is_found_from_another_script() {
    let (mut barrel, mut archetype, mut gourd) = (None, None, None);
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        let kind = actor(w);
        let bare = actor(w);
        (barrel, archetype, gourd) = (Some(a), Some(kind), Some(bare));
        let named = w.reference(&[El::Local(kind)], None);
        w.tags(
            a,
            &[("archetype ref", "PlacementReference", "tfbActorInfo", named)],
        );
        let got = w.variable("got", "ScriptReference", None);
        let kept = w.variable("kept", "ScriptReference", None);
        script(
            w,
            "C:/tfb/Synthetic/LootSystem_Init.ai",
            |w| {
                let (f, fop) = super::init::find(
                    w,
                    "archetype ref",
                    "PlacementReference",
                    "tfbActorInfo",
                    1,
                    &[El::Local(a)],
                );
                setref(w, &[El::Local(got)], &[El::Local(fop)]);
                w.close(f);
                setref(w, &[El::Local(kept)], &[El::Local(a)]);
                let (f, fop) = super::init::find(
                    w,
                    "archetype ref",
                    "PlacementReference",
                    "tfbActorInfo",
                    1,
                    &[El::Local(bare)],
                );
                setref(w, &[El::Local(kept)], &[El::Local(fop)]);
                w.close(f);
            },
            |_| {},
            |w| super::init::behaviour(w, |_| {}),
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.name_object(obj.at(archetype.unwrap()), 0x77);
    vm.name_object(obj.at(barrel.unwrap()), 0x20);
    vm.name_object(obj.at(gourd.unwrap()), 0x21);
    let id = vm.add(HANDLE, "LootSystem_Init", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        vm.local(id, "got"),
        Some(Value::Entity(0x77)),
        "the archetype"
    );
    assert_eq!(
        vm.local(id, "kept"),
        Some(Value::Entity(0x20)),
        "no tag: the find's block skipped"
    );
}

/// A tag naming a file variable holds what that variable does (openspec
/// AG.60): a Titan boulder's `Pebble_Set` is a set reference tag naming the
/// level's set of pebble templates, and its own STARTUP finds it, counts it
/// and reads its members, as the boulder spawns a pebble from each. A tag
/// naming a number of the file reads its number.
#[test]
fn a_tag_naming_a_files_set_holds_the_set() {
    let (mut boulder, mut pebbles) = (None, None);
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        let (one, two) = (actor(w), actor(w));
        let set = w.b.object(0, "ScriptSet", 0x1C);
        w.text(set, 0x08, "Pebble_Set_Template");
        let list = w.b.object(0, "ScriptSetList", 0x24);
        w.path(list, &[El::Local(one), El::Local(two)]);
        w.meta_word(list, 0x18, "tfbActorInfo");
        w.refer(set, 0x18, list);
        let size = w.b.object(0, "ValueInfo", 0x24);
        w.text(size, 0x08, "Pebble_Size");
        w.meta_word(size, 0x1C, INT);
        w.word(size, 0x20, 60);
        let named = w.reference(&[El::Local(set)], None);
        let sized = w.rhs(INT, &[El::Local(size)]);
        w.tags(
            a,
            &[
                ("Pebble_Set", "ScriptSetReference", "tfbActorInfo", named),
                ("pebble size", "ValueInfo", INT, sized),
            ],
        );
        (boulder, pebbles) = (Some(a), Some((one, two)));
        let count = var(w, "count");
        let scale = var(w, "scale");
        let first = w.variable("first", "ScriptReference", None);
        script(
            w,
            "C:/tfb/Synthetic/Titan_Carriable_Object.ai",
            |w| {
                let my = [El::Member("optoplevelbehavior", "my")];
                let (f, fop) = super::init::find(
                    w,
                    "Pebble_Set",
                    "ScriptSetReference",
                    "tfbActorInfo",
                    1,
                    &my,
                );
                let n = w.rhs(INT, &[El::Local(fop), El::Member("setvariant", "count")]);
                w.set(&[El::Local(count)], n);
                setref(
                    w,
                    &[El::Local(first)],
                    &[El::Local(fop), El::Member("setvariant", "first")],
                );
                w.close(f);
                let (f, fop) = super::init::find(w, "pebble size", "ValueInfo", INT, 1, &my);
                let v = w.rhs(INT, &[El::Local(fop)]);
                w.set(&[El::Local(scale)], v);
                w.close(f);
            },
            |_| {},
            |w| super::init::behaviour(w, |_| {}),
        );
    });
    let (one, two) = pebbles.unwrap();
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.name_object(obj.at(one), 0x31);
    vm.name_object(obj.at(two), 0x32);
    let id = vm.add_at(
        HANDLE,
        "Titan_CarriableObject_Boulder_v3_(4)",
        0,
        obj.at(boulder.unwrap()),
    );
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "count"), Some(Value::Int(2)), "the file's set");
    assert_eq!(vm.local(id, "first"), Some(Value::Entity(0x31)));
    assert_eq!(
        vm.local(id, "scale"),
        Some(Value::Int(60)),
        "the file's number"
    );
}

/// A breakable's running loot count, the original's shape in our ops: the
/// kind's rate added, 1 taken off once the count is over 1, a drop when it's
/// at or under the rate, every number single precision and each comparison
/// within 1e-4. Five breaks a step each from known starts: anchor A's common
/// count (0.8242795, AH.9) drops three, its index after the first two
/// AH.9's 0.024279475; the uncommon pots' rate of 1 drops every break and
/// leaves 0.097497 at 0.09749699 for good (AH.9's read); a count landing on
/// 1 exactly isn't over it, so it waits a break to wrap, and the rate's own
/// value is at it.
#[test]
fn a_running_loot_count_over_five_breaks_from_known_starts() {
    let p = program(|w| {
        let index = w.variable("index", "ValueInfo", Some(FLOAT));
        let rate = w.variable("rate", "ValueInfo", Some(FLOAT));
        let value = var(w, "value");
        idle(
            w,
            |_| {},
            |w| {
                let zero = w.int(0);
                w.set(&[El::Local(value)], zero);
                let i = w.rhs(FLOAT, &[El::Local(index)]);
                let r = w.rhs(FLOAT, &[El::Local(rate)]);
                let sum = w.expr(i, Some((0, r)));
                w.set(&[El::Local(index)], sum);
                let one = w.float(1.0);
                let (over, _) = check(w, index, 4, one);
                let i = w.rhs(FLOAT, &[El::Local(index)]);
                let one = w.float(1.0);
                let less = w.expr(i, Some((1, one)));
                w.set(&[El::Local(index)], less);
                w.close(over);
                let r = w.rhs(FLOAT, &[El::Local(rate)]);
                let (at, _) = check(w, index, 0, r);
                let one = w.int(1);
                w.set(&[El::Local(value)], one);
                w.close(at);
            },
        );
    });
    let breaks = |start: f32, rate: f32| -> Vec<(i32, f32)> {
        let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
        let id = vm.add(HANDLE, "LootSystem", 0);
        assert!(vm.set_local(id, "index", Value::Float(start)));
        assert!(vm.set_local(id, "rate", Value::Float(rate)));
        (0..5)
            .map(|_| {
                vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
                let value = vm.local(id, "value").and_then(|v| v.as_i32()).unwrap();
                let index = vm.local(id, "index").and_then(|v| v.as_f32()).unwrap();
                (value, index)
            })
            .collect()
    };
    let dropped = |b: &[(i32, f32)]| b.iter().map(|&(v, _)| v).collect::<Vec<_>>();
    let common = breaks(0.824_279_5, 0.6);
    assert_eq!(dropped(&common), [1, 1, 0, 1, 0], "{common:?}");
    assert_eq!(
        common[1].1, 0.024_279_475,
        "AH.9's index after the lob's two"
    );
    let uncommon = breaks(0.097_497, 1.0);
    assert_eq!(dropped(&uncommon), [1; 5]);
    assert!(
        uncommon.iter().all(|&(_, i)| i == 0.097_496_99),
        "{uncommon:?}"
    );
    let at_one = breaks(0.4, 0.6);
    assert_eq!(dropped(&at_one), [0, 1, 1, 0, 1], "{at_one:?}");
    assert_eq!(at_one[0].1, 1.0, "1 isn't over 1");
    let from_zero = breaks(0.0, 0.6);
    assert_eq!(dropped(&from_zero), [1, 1, 0, 1, 0], "{from_zero:?}");
}
