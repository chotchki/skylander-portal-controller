//! What AH.23's figure needed of the VM, on a script built in code: an
//! embedder growing a set of structures as `inc count` does, putting
//! numbers in a set's members and pointing a structure's reference at what
//! it made, which the script then reads and writes as its own.

use super::*;

/// Points `lhs` at what `rhs` names, as the scripts link a reference.
fn setref(w: &mut Writer<'_>, lhs: &[El<'_>], rhs: &[El<'_>]) {
    let l = w.value(lhs);
    let r = w.reference(rhs, None);
    let o = w.op(Class::SetReference, "set reference");
    w.refer(o, 0x1C, l);
    w.refer(o, 0x20, r);
    w.push(o);
}

/// The original's shape in our ops: a set of tag structures, each a set of
/// words and an id, and a slot whose reference names one; each tick the
/// script reads the slot's tag's first word and adds 4 to it, as an award
/// reads and writes a figure's words.
fn tagged() -> Arc<Program> {
    program(|w| {
        let (section, _) = w.open(Class::StructureSection, "STRUCTURES");
        let (tp, tag_ty) = w.open(Class::DefineStructure, "Tag");
        let area = w.variable("area", "ScriptSet", Some("ValueInfo"));
        w.push(area);
        let id = var(w, "id");
        w.push(id);
        w.end_block(tp);
        let (sp, slot_ty) = w.open(Class::DefineStructure, "Slot");
        let tag = w.variable("tag", "ScriptReference", None);
        w.push(tag);
        w.end_block(sp);
        w.end_block(section);
        w.end("C:/tfb/Synthetic/Types.ai");
        let tags = w.variable("tags", "ScriptSet", None);
        w.refer(tags, 0x24, tag_ty);
        let slot = w.variable("slot", "ScriptStructure", None);
        w.refer(slot, 0x24, slot_ty);
        let word = w.variable("word", "ScriptReference", None);
        let got = var(w, "got");
        let made = var(w, "made");
        idle(
            w,
            |_| {},
            |w| {
                let n = w.rhs(INT, &[El::Local(tags), El::Member("setvariant", "count")]);
                w.set(&[El::Local(made)], n);
                setref(
                    w,
                    &[El::Local(word)],
                    &[
                        El::Local(slot),
                        El::Local(tag),
                        El::Local(area),
                        El::Member("setvariant", "first"),
                    ],
                );
                let now = read(w, word);
                w.set(&[El::Local(got)], now);
                let now = read(w, word);
                let four = w.int(4);
                let more = w.expr(now, Some((0, four)));
                w.set(
                    &[
                        El::Local(word),
                        El::Member("abstractscriptgroup", "valuedereferencer"),
                    ],
                    more,
                );
            },
        );
    })
}

#[test]
fn an_embedders_structure_words_and_link_are_the_scripts_own() {
    let mut vm = Vm::new(tagged(), Config::new(Mode::Strict));
    // Script 1: script 0 holds the structures.
    let id = vm.add(HANDLE, "Thing", 1);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "made"), Some(Value::Int(0)));
    let tags = vm.local(id, "tags").unwrap();
    let slot = vm.local(id, "slot").unwrap();

    // A tag made as `inc count` makes one, its words grown the same way.
    let tag = vm.grow(tags, &mut Bare).expect("a member");
    assert!(matches!(vm.value(tag), Value::Struct(_)), "{tag:?}");
    assert_eq!(vm.set(tags).unwrap(), [tag]);
    let area = vm.field(tag, "area").unwrap();
    let words: Vec<Value> = (0..3).map(|_| vm.grow(area, &mut Bare).unwrap()).collect();
    assert_eq!(vm.set(area).unwrap(), words.as_slice());
    assert!(vm.put(words[0], Value::Int(41)));
    assert!(vm.put(words[2], Value::Float(2.5)), "converted as written");
    assert!(vm.set_field(tag, "id", Value::Int(9)));
    assert!(vm.set_reference(slot, "tag", tag));

    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "made"), Some(Value::Int(1)));
    assert_eq!(vm.local(id, "got"), Some(Value::Int(41)), "read through it");
    let held: Vec<Value> = words.iter().map(|&w| vm.value(w)).collect();
    assert_eq!(
        held,
        [Value::Int(45), Value::Int(0), Value::Float(2.5)],
        "the script's write in the embedder's word"
    );

    // What isn't the place for each refuses, nothing changed.
    assert!(!vm.put(Value::Int(3), Value::Int(1)), "a number");
    assert!(!vm.put(tag, Value::Int(1)), "a structure");
    assert!(!vm.put(area, Value::Int(1)), "a set");
    assert!(!vm.set_reference(slot, "nothing", tag), "no such field");
    assert!(!vm.set_reference(tag, "id", slot), "a number field");
    assert!(
        !vm.set_field(slot, "tag", Value::Int(1)),
        "a reference field"
    );
    assert_eq!(vm.grow(Value::Int(1), &mut Bare), None, "a number");
    assert_eq!(vm.grow(tag, &mut Bare), None, "a structure");
    assert_eq!(vm.set(tags).unwrap().len(), 1);
    assert_eq!(vm.field(tag, "id"), Some(Value::Int(9)));
}
