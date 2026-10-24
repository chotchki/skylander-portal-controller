use std::collections::BTreeMap;

use super::builder::{Writer, creature, every_class, size, stack_size};
use super::*;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::{TYPES, file_size, type_def};
use crate::igz::{ExternalName, Location, MEMORY_PRESENT};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

#[derive(Debug, Clone, Copy)]
enum K {
    /// An object ref.
    R,
    /// An enum or unsigned word.
    W,
    I,
    S,
    L,
    /// A class word.
    T,
}
use K::*;

/// One field: its registry name, offset and kind.
type Registered = (&'static str, u32, K);

/// Each class's fields below `OpCode`, from ac4's registry: name, offset,
/// kind. Written as literals so a wrong reader constant shows.
#[rustfmt::skip]
const FIELDS: &[(&str, u32, &[Registered])] = &[
    ("OpMacroParameter", 0x48, &[("_varName", 0x1C, S), ("_varContainerType", 0x20, T), ("_varContentsType", 0x24, T), ("_varIndex", 0x28, I), ("_combineOp", 0x2C, W), ("_RHS", 0x30, R), ("_indexRHS", 0x34, R), ("_macroDef", 0x38, R), ("_parameterID", 0x40, L)]),
    ("OpSetValue", 0x24, &[("_LHS", 0x1C, R), ("_RHS", 0x20, R)]),
    ("OpFlowBuiltInBehavior", 0x1C, &[]),
    ("OpCheckValue", 0x28, &[("_LHS", 0x1C, R), ("_relOperator", 0x20, W), ("_RHS", 0x24, R)]),
    ("OpMacroInterface", 0x20, &[("_varTemplateList", 0x1C, R)]),
    ("OpFlowMacro", 0x1C, &[]),
    ("OpSetReference", 0x28, &[("_LHS", 0x1C, R), ("_RHS", 0x20, R), ("_indexRHS", 0x24, R)]),
    ("OpUseMacro", 0x24, &[("_NP", 0x1C, R), ("_indexRHS", 0x20, R)]),
    ("OpAbstractFlow", 0x1C, &[]),
    ("OpChangeMembership", 0x28, &[("_LHS", 0x1C, R), ("_combineOp", 0x20, W), ("_RHS", 0x24, R)]),
    ("OpSpawn", 0x30, &[("_lastReceiver", 0x1C, R), ("_LHS", 0x20, R), ("_cachedObject", 0x24, R), ("_RHS", 0x28, R), ("_facingRHS", 0x2C, R)]),
    ("OpCreateVariable", 0x2C, &[("_varName", 0x1C, S), ("_varContainerType", 0x20, T), ("_varContentsType", 0x24, T), ("_varIndex", 0x28, I)]),
    ("OpCheckReference", 0x2C, &[("_LHS", 0x1C, R), ("_relOperator", 0x20, W), ("_RHS", 0x24, R), ("_indexRHS", 0x28, R)]),
    ("OpSlideValue", 0x34, &[("_LHS", 0x1C, R), ("_RHS", 0x20, R), ("_secondsRHS", 0x24, R), ("_easeOutRHS", 0x28, R), ("_easeInRHS", 0x2C, R), ("_cachedSlider", 0x30, R)]),
    ("OpRemove", 0x20, &[("_NP", 0x1C, R)]),
    ("OpFlowLoop", 0x1C, &[]),
    ("OpIncValue", 0x20, &[("_LHS", 0x1C, R)]),
    ("OpDisplace", 0x30, &[("_NP", 0x1C, R), ("_combineMode", 0x20, W), ("_lenNP", 0x24, R), ("_headNP", 0x28, R), ("_pitchNP", 0x2C, R)]),
    ("OpStartSequence", 0x28, &[("_LHS", 0x1C, R), ("_indexRHS", 0x20, R), ("_cachedObject", 0x24, R)]),
    ("OpFindVariable", 0x3C, &[("_varContainerType", 0x1C, T), ("_varContentsType", 0x20, T), ("_varName", 0x24, S), ("_filter", 0x28, W), ("_skipRHS", 0x2C, R), ("_owner", 0x30, R), ("_indexRHS", 0x34, R), ("_cachedObject", 0x38, R)]),
    ("OpFlowBehavior", 0x1C, &[]),
    ("OpSetBehavior", 0x24, &[("_NP", 0x1C, R), ("_indexRHS", 0x20, R)]),
    ("OpUserBehavior", 0x24, &[("_thisPC", 0x1C, W), ("_scriptInfo", 0x20, R)]),
    ("OpForEach", 0x34, &[("_index", 0x1C, I), ("_LHS", 0x20, R), ("_dir", 0x24, W), ("_RHS", 0x28, R), ("_cachedSet", 0x2C, R), ("_cachedObject", 0x30, R)]),
    ("OpLoopValue", 0x28, &[("_index", 0x1C, I), ("_RHS", 0x20, R), ("_numLoops", 0x24, I)]),
    ("OpDefineMacro", 0x38, &[("_publicity", 0x1C, W), ("_creationHandle", 0x20, L), ("_thisPC", 0x28, W), ("_scriptInfo", 0x2C, R), ("_macroVarList", 0x30, R), ("_returnMacro", 0x34, R)]),
    ("OpStartUp", 0x1C, &[]),
    ("OpShutDown", 0x1C, &[]),
    ("OpPreScript", 0x1C, &[]),
    ("OpFindSubSet", 0x30, &[("_LHS", 0x1C, R), ("_relOperator", 0x20, W), ("_RHS", 0x24, R), ("_setRemainder", 0x28, I), ("_cachedObject", 0x2C, R)]),
    ("OpDefineMacroSpecialization", 0x40, &[("_publicity", 0x1C, W), ("_creationHandle", 0x20, L), ("_thisPC", 0x28, W), ("_scriptInfo", 0x2C, R), ("_macroVarList", 0x30, R), ("_returnMacro", 0x34, R), ("_NP", 0x38, R)]),
    ("OpControl", 0x2C, &[("_lastReceiver", 0x1C, R), ("_LHS", 0x20, R), ("_cachedObject", 0x24, R), ("_indexRHS", 0x28, R)]),
    ("OpStopSequence", 0x20, &[("_NP", 0x1C, R)]),
    ("OpCheckMembership", 0x30, &[("_LHS", 0x1C, R), ("_membershipOp", 0x20, W), ("_RHS", 0x24, R), ("_cachedType", 0x28, R), ("_cachedObj", 0x2C, R)]),
    ("OpDefineStructure", 0x30, &[("_publicity", 0x1C, W), ("_creationHandle", 0x20, L), ("_thisPC", 0x28, W), ("_scriptInfo", 0x2C, R)]),
    ("OpReset", 0x20, &[("_NP", 0x1C, R)]),
    ("OpMacroSection", 0x1C, &[]),
    ("OpCheckFOV", 0x3C, &[("_RHSfacing", 0x1C, R), ("_RHSfov", 0x20, R), ("_mode", 0x24, W), ("_cachedObject", 0x28, R), ("_fromLHS", 0x2C, R), ("_LHS", 0x30, R), ("_relOperator", 0x34, W), ("_RHS", 0x38, R)]),
    ("OpDecValue", 0x20, &[("_LHS", 0x1C, R)]),
    ("OpStructureSection", 0x1C, &[]),
    ("OpTurnTo", 0x28, &[("_facingRHS", 0x1C, R), ("_NP", 0x20, R), ("_indexRHS", 0x24, R)]),
    ("OpMoveTo", 0x30, &[("_indexRHS", 0x1C, R), ("_LHS", 0x20, R), ("_dir", 0x24, W), ("_RHS", 0x28, R), ("_NP", 0x2C, R)]),
    ("OpTeleportTo", 0x28, &[("_LHS", 0x1C, R), ("_dir", 0x20, W), ("_facingRHS", 0x24, R)]),
    ("OpMoveFrom", 0x30, &[("_indexRHS", 0x1C, R), ("_LHS", 0x20, R), ("_dir", 0x24, W), ("_RHS", 0x28, R), ("_NP", 0x2C, R)]),
];

#[test]
fn the_classes_are_the_registry_s() {
    assert_eq!(Class::ALL.len(), 44);
    assert_eq!(FIELDS.len(), 44);
    for (i, &class) in Class::ALL.iter().enumerate() {
        let name = class.type_name();
        assert_eq!(Class::from_type(name), Some(class));
        assert_eq!(FIELDS[i].0, name, "the table follows Class::ALL");
        let def = type_def(name).unwrap_or_else(|| panic!("{name} isn't in TYPES"));
        assert_eq!(def.size, FIELDS[i].1, "{name}");
        assert_eq!(size(class), def.size, "{name}");
        assert_eq!(class.opens_block(), is_a(name, "OpBranch"), "{name}");
        assert_eq!(class.is_flow(), is_a(name, "OpAbstractFlow"), "{name}");
    }
    // The other op types TYPES holds: the classes' bases, and `OpParameter`,
    // which the disc holds 4,577 of at no position.
    let ops = TYPES.iter().filter(|t| is_a(t.name, "OpCode")).count();
    let others = [
        "OpCode",
        "OpBranch",
        "OpAbstractBehavior",
        "OpAbstractDefinition",
        "OpAbstractCreateVariable",
        "OpParameter",
        "OpLoop",
        "OpAbstractCheckValue",
        "OpAbstractControl",
        "OpAbstractMove",
        "OpTopLevelBehavior",
    ];
    assert_eq!(ops, 44 + others.len());
    assert!(others.iter().all(|a| Class::from_type(a).is_none()));
    assert_eq!(Class::from_type("OpPrint"), None);
    for class in StackClass::ALL {
        assert_eq!(StackClass::from_type(class.type_name()), Some(class));
        assert_eq!(file_size(class.type_name()), Some(stack_size(class)));
        assert!(is_a(class.type_name(), "ScriptObjectList"));
    }
}

#[test]
fn every_op_class_reads_back_field_for_field() {
    for (i, &(name, size, fields)) in FIELDS.iter().enumerate() {
        let mut b = IgzBuilder::new();
        let o = b.object(0, name, size);
        b.string(at(o, 0x08), &format!("op {name}"));
        let (flags, branch) = (0x0010 + i as u16, 0x0100 + i as u16);
        b.put_bytes(
            at(o, 0x18),
            &[flags.to_be_bytes(), branch.to_be_bytes()].concat(),
        );
        // What each field should read back as, once the file is built.
        enum Want {
            Object(Location),
            Field(Field<'static>),
            Class(u32, &'static str, bool),
        }
        let mut wants: Vec<(&str, Want)> = Vec::new();
        let names: Vec<&'static str> = (0..fields.len())
            .map(|k| &*Box::leak(format!("Class{i}x{k}").into_boxed_str()))
            .collect();
        for (k, &(field, offset, kind)) in fields.iter().enumerate() {
            let n = (i * 16 + k) as u32;
            let w = at(o, offset);
            let want = match kind {
                R => {
                    let (ty, sz) = match (name, field) {
                        ("OpUseMacro", "_NP") => ("ScriptGroupStack", 0x1C),
                        ("OpDefineMacroSpecialization", "_NP") => ("OpDefineMacroStack", 0x18),
                        _ => ("igObject", 0x8),
                    };
                    let target = b.object(0, ty, sz);
                    b.pointer(w, target);
                    Want::Object(target)
                }
                W => {
                    b.put_u32(w, 0x100 + n);
                    Want::Field(Field::Word(0x100 + n))
                }
                I => {
                    b.put_u32(w, (-2 - n as i32) as u32);
                    Want::Field(Field::Int(-2 - n as i32))
                }
                S => {
                    let text: &'static str = Box::leak(format!("s{n}").into_boxed_str());
                    b.string(w, text);
                    Want::Field(Field::Text(Some(text)))
                }
                L => {
                    b.put_u32(w, 0x1000 + n);
                    b.put_u32(at(o, offset + 4), 0x2000 + n);
                    Want::Field(Field::Long(
                        u64::from(0x1000 + n) << 32 | u64::from(0x2000 + n),
                    ))
                }
                T if field == "_varContainerType" => {
                    let index = b.external_name(METAOBJECT, names[k]);
                    b.unflagged_meta(w, index);
                    Want::Class(index, names[k], false)
                }
                T => {
                    let index = b.meta_object(w, METAOBJECT, names[k]);
                    Want::Class(index, names[k], true)
                }
            };
            wants.push((field, want));
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let op = Op::read(&igz, index(&igz, o)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(op.class().type_name(), name);
        let code = op.code();
        assert_eq!((code.flags, code.branch), (flags, branch), "{name}");
        let listed = op.fields();
        let got: BTreeMap<&str, Field<'_>> = listed.iter().copied().collect();
        assert_eq!(got.len(), listed.len(), "{name} lists a field twice");
        assert_eq!(listed.len(), 3 + fields.len(), "{name}: {listed:?}");
        assert_eq!(got["_name"], Field::Text(Some(&*format!("op {name}"))));
        for (field, want) in wants {
            let want = match want {
                Want::Object(l) => Field::Ref(ObjectRef::Object(index(&igz, l))),
                Want::Field(f) => f,
                Want::Class(index, class, true) => Field::Type(TypeRef::Named {
                    index,
                    name: ExternalName {
                        namespace: METAOBJECT,
                        name: class,
                    },
                }),
                Want::Class(index, class, false) => Field::Type(TypeRef::Indexed {
                    index,
                    name: Some(ExternalName {
                        namespace: METAOBJECT,
                        name: class,
                    }),
                }),
            };
            assert_eq!(got.get(field), Some(&want), "{name}.{field}");
        }
    }
}

/// Each position as `class name +branch`.
fn positions<'a>(g: &Graph<'a>, s: &Script<'a>) -> Vec<String> {
    s.ops
        .iter()
        .map(|o| {
            let op = &g.ops[o];
            let c = op.code();
            format!("{:?} {} +{}", op.class(), c.name.unwrap_or(""), c.branch)
        })
        .collect()
}

fn path<'g, 'a>(g: &'g Graph<'a>, r: ObjectRef<'a>) -> &'g Path<'a> {
    let ObjectRef::Object(o) = r else {
        panic!("{r:?} isn't local")
    };
    match &g.operands[&o] {
        Operand::Stack(p) => p,
        other => panic!("{other:?} isn't a stack"),
    }
}

#[test]
fn a_graph_round_trips_through_the_writer_and_the_readers() {
    let (bytes, script) = creature();
    let igz = Igz::parse(&bytes).unwrap();
    let g = Graph::read(&igz).unwrap();
    assert_eq!(g.scripts.len(), 1);
    let s = &g.scripts[0];
    assert_eq!(s.object, index(&igz, script));
    assert_eq!(s.name, Some("C:/tfb/Content/Levels/Synthetic/Creature.ai"));
    assert_eq!(g.script("Creature.ai"), Some(s));
    assert_eq!(
        positions(&g, s),
        [
            "PreScript PRESCRIPT +16",
            "StartUp STARTUP +3",
            "SetValue set||= +0",
            "SetValue set||= +0",
            "FlowContinue flow continue +0",
            "ShutDown SHUTDOWN +1",
            "FlowContinue flow continue +0",
            "MacroSection MACROS +8",
            "DefineMacro Combat - Synthetic Hurt +6",
            "MacroInterface INTERFACE +2",
            "MacroParameter parameter +0",
            "FlowMacro flow macro +0",
            "SetValue set||= +0",
            "FlowEnd flow end +65535",
            "FlowContinue flow continue +0",
            "FlowContinue flow continue +0",
            "FlowContinue flow continue +0",
            "UserBehavior idle +17",
            "UseMacro use macro +4",
            "MacroInterface INTERFACE +2",
            "MacroParameter parameter +0",
            "FlowMacro flow macro +0",
            "FlowContinue flow continue +0",
            "CheckValue if +2",
            "Remove remove +0",
            "Skip else +3",
            "LoopValue loop value|||loop index +2",
            "IncValue inc value +0",
            "FlowLoop flow +0",
            "Spawn spawn|at|facing +1",
            "FlowContinue flow continue +0",
            "StartSequence start sequence +1",
            "FlowContinue flow continue +0",
            "SetBehavior set behavior +0",
            "FlowContinue flow continue +0",
        ]
    );
    // The shared flows are one object each, decoded once.
    let flows = s
        .ops
        .iter()
        .filter(|o| g.ops[o].class() == Class::FlowContinue);
    assert_eq!(flows.clone().count(), 9);
    assert!(flows.clone().all(|o| *o == s.ops[4]));

    // `set health = 5`: a value stack on the variable, an int literal.
    let Op::SetValue(set) = g.ops[&s.ops[2]] else {
        panic!()
    };
    let health = path(&g, set.lhs);
    assert_eq!(health.class, StackClass::Value);
    let [ObjectRef::Object(v)] = health.elements[..] else {
        panic!()
    };
    let Op::CreateVariable(var) = g.ops[&v] else {
        panic!()
    };
    assert_eq!(var.name, Some("health"));
    assert_eq!(var.container_type.metaobject(), Some("ValueInfo"));
    assert_eq!(var.contents_type.metaobject(), Some("IntMeasurement"));
    let five = path(&g, set.rhs);
    assert_eq!((five.class, five.elements.len()), (StackClass::RhsValue, 0));
    let rhs = five.rhs.unwrap();
    assert_eq!(
        (rhs.class.metaobject(), rhs.bits, rhs.is_random),
        (Some("IntMeasurement"), 5, false)
    );
    // `set actorinfo.movement = 0.5`: an engine member import.
    let Op::SetValue(set) = g.ops[&s.ops[3]] else {
        panic!()
    };
    let [ObjectRef::Import(member)] = path(&g, set.lhs).elements[..] else {
        panic!()
    };
    assert_eq!(member.id.object, crate::fnv1a32(b"movement"));
    assert_eq!(member.id.namespace, crate::fnv1a32(b"actorinfo"));
    let half = path(&g, set.rhs).rhs.unwrap();
    assert_eq!(
        (half.class.metaobject(), f32::from_bits(half.bits as u32)),
        (Some("FloatMeasurement"), 0.5)
    );

    // The macro: its parameter binds into it, and the body's expression
    // is `health - amount`.
    let def = s.ops[8];
    let Op::DefineMacro(d) = g.ops[&def] else {
        panic!()
    };
    assert_eq!(d.op.name, Some("Combat - Synthetic Hurt"));
    let Op::MacroParameter(p) = g.ops[&s.ops[10]] else {
        panic!()
    };
    assert_eq!(
        (p.var.name, p.macro_def),
        (Some("amount"), ObjectRef::Object(def))
    );
    let Op::SetValue(set) = g.ops[&s.ops[12]] else {
        panic!()
    };
    let ObjectRef::Object(e) = set.rhs else {
        panic!()
    };
    let Operand::Expression(e) = &g.operands[&e] else {
        panic!()
    };
    assert_eq!(e.operator, 1);
    let [ObjectRef::Object(a)] = path(&g, e.op2).elements[..] else {
        panic!()
    };
    assert!(matches!(g.ops[&a], Op::CreateVariable(v) if v.name == Some("amount")));
    // The call names the definition and binds 2 into it.
    let Op::UseMacro(call) = g.ops[&s.ops[18]] else {
        panic!()
    };
    let np = path(&g, ObjectRef::Object(call.np.unwrap()));
    assert_eq!(
        (np.class, &np.elements[..]),
        (StackClass::Group, &[ObjectRef::Object(def)][..])
    );
    let Op::MacroParameter(arg) = g.ops[&s.ops[20]] else {
        panic!()
    };
    assert_eq!(path(&g, arg.rhs).rhs.unwrap().bits, 2);

    // `if health <= 0`, the else skipping the loop, a global export, a
    // named template and the behaviour switch back to `idle`.
    let Op::CheckValue(check) = g.ops[&s.ops[23]] else {
        panic!()
    };
    assert_eq!(check.relation, 0);
    let Op::IncValue(inc) = g.ops[&s.ops[27]] else {
        panic!()
    };
    let [ObjectRef::Import(global)] = path(&g, inc.lhs).elements[..] else {
        panic!()
    };
    assert_eq!(
        (global.id.object, global.id.namespace),
        (0x30, crate::fnv1a32(b"global.bld"))
    );
    let Op::Spawn(spawn) = g.ops[&s.ops[29]] else {
        panic!()
    };
    let template = path(&g, spawn.control.lhs);
    assert_eq!(template.class, StackClass::PlacementList);
    let [ObjectRef::Named { name, .. }] = template.elements[..] else {
        panic!()
    };
    assert_eq!((name.namespace, name.name), ("SpriteInfo", "Orb_Template"));
    assert_eq!(path(&g, spawn.rhs).class, StackClass::Position);
    let Op::SetBehavior(switch) = g.ops[&s.ops[33]] else {
        panic!()
    };
    assert_eq!(path(&g, switch.np).elements, [ObjectRef::Object(s.ops[17])]);

    // Every class word the writer stored unflagged resolved: two variables'
    // containers, both parameters', seven literals' and two RHS paths'.
    assert_eq!(g.unflagged_classes, 2 + 2 + 7 + 2);
}

#[test]
fn one_op_of_every_class_reads_in_a_graph() {
    let (bytes, ops) = every_class();
    let igz = Igz::parse(&bytes).unwrap();
    let g = Graph::read(&igz).unwrap();
    let s = &g.scripts[0];
    assert_eq!(s.ops.len(), 44);
    for (p, &class) in Class::ALL.iter().enumerate() {
        assert_eq!(s.ops[p], index(&igz, ops[p]));
        assert_eq!(g.op(s, p).map(Op::class), Some(class));
    }
}

/// A one-script file whose ops `ops` writes; returns the bytes.
fn one_script(ops: impl FnOnce(&mut Writer<'_>)) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    ops(&mut w);
    w.end("C:/tfb/Synthetic.ai");
    b.build()
}

fn refusal(bytes: &[u8]) -> FormatError {
    let igz = Igz::parse(bytes).unwrap();
    Graph::read(&igz).expect_err("refused")
}

#[test]
fn a_skip_past_the_end_of_its_list_is_refused_by_name() {
    let bytes = one_script(|w| {
        let zero = w.int(0);
        let (_, check) = w.open(Class::CheckValue, "if");
        w.refer(check, 0x24, zero);
        let o = w.op(Class::SetValue, "set||=");
        w.push(o);
        let flow = w.flow(Class::FlowContinue);
        w.push(flow);
        // The check's block claims three ops after it; two follow.
        w.half(check, 0x1A, 3);
    });
    let e = refusal(&bytes);
    assert_eq!((e.format, e.field), ("tfbScript", "_branchPC"));
    assert_eq!(e.kind, ErrorKind::OutOfRange { value: 3, limit: 3 });
}

#[test]
fn a_block_on_an_op_that_opens_none_or_flow_end_on_a_check_is_refused() {
    let bytes = one_script(|w| {
        let o = w.op(Class::SetValue, "set||=");
        w.half(o, 0x1A, 1);
        w.push(o);
        let flow = w.flow(Class::FlowContinue);
        w.push(flow);
    });
    let e = refusal(&bytes);
    assert_eq!(
        (e.field, e.kind.clone()),
        ("_branchPC", ErrorKind::OutOfRange { value: 1, limit: 2 })
    );
    let bytes = one_script(|w| {
        let o = w.op(Class::CheckValue, "if");
        w.half(o, 0x1A, crate::igz::layouts::BRANCH_END);
        w.push(o);
    });
    assert_eq!(refusal(&bytes).field, "_branchPC");
    // `flow end` itself stores it.
    let bytes = one_script(|w| w.flow_end());
    Graph::read(&Igz::parse(&bytes).unwrap()).unwrap();
}

#[test]
fn a_class_index_out_of_range_or_outside_metaobject_is_refused_by_name() {
    let bytes = one_script(|w| {
        let v = w.op(Class::CreateVariable, "create variable");
        w.b.unflagged_meta(at(v, 0x20), 999);
        w.push(v);
    });
    let e = refusal(&bytes);
    assert_eq!((e.format, e.field), ("tfbScript", "_varContainerType"));
    assert!(
        matches!(e.kind, ErrorKind::OutOfRange { value: 999, .. }),
        "{e}"
    );
    let bytes = one_script(|w| {
        let s = w.literal("IntMeasurement", 1, false);
        let other = w.b.external_name("level.bld", "SpriteInfo");
        w.b.put_u32(at(s, 0x1C), other);
        let set = w.op(Class::SetValue, "set||=");
        w.refer(set, 0x20, s);
        w.push(set);
    });
    let e = refusal(&bytes);
    assert_eq!(e.field, "_type");
    assert_eq!(
        e.kind,
        ErrorKind::WrongType {
            expected: "a metaobject class",
            found: "level.bld::SpriteInfo".into()
        }
    );
}

#[test]
fn an_unknown_class_or_operand_is_refused_by_name() {
    // A registered type that isn't an op, at a position.
    let bytes = one_script(|w| {
        let v = w.b.object(0, "ValueInfo", 0x24);
        w.push(v);
    });
    let e = refusal(&bytes);
    assert_eq!(e.field, "op");
    assert_eq!(
        e.kind,
        ErrorKind::WrongType {
            expected: "one of the 44 op classes",
            found: "ValueInfo".into()
        }
    );
    // An op class the disc never uses.
    let bytes = one_script(|w| {
        let o = w.b.object(0, "OpPrint", 0x20);
        w.push(o);
    });
    assert!(
        matches!(refusal(&bytes).kind, ErrorKind::WrongType { found, .. } if found == "OpPrint")
    );
    // An operand that's neither a stack nor an expression.
    let bytes = one_script(|w| {
        let v = w.b.object(0, "ValueInfo", 0x24);
        let set = w.op(Class::SetValue, "set||=");
        w.refer(set, 0x20, v);
        w.push(set);
    });
    let e = refusal(&bytes);
    assert_eq!(e.field, "operand");
    assert!(matches!(e.kind, ErrorKind::WrongType { found, .. } if found == "ValueInfo"));
}

#[test]
fn lists_that_share_their_words_stop_at_the_entry_budget() {
    // 200 stacks over one 1,000-word block: 200,000 entries from 4 KB.
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let mut data = None;
    for _ in 0..200 {
        let s = w.b.object(0, "ValueStack", 0x1C);
        w.word(s, 0x08, 1000);
        w.word(s, 0x0C, 1000);
        match data {
            None => data = Some(w.b.memory_ref(at(s, 0x10), w.blocks, &[0; 4000], 0)),
            Some(d) => {
                w.word(s, 0x10, MEMORY_PRESENT | 4000);
                w.refer(s, 0x14, d);
            }
        }
        let inc = w.op(Class::IncValue, "inc value");
        w.refer(inc, 0x1C, s);
        w.push(inc);
    }
    w.end("C:/tfb/Shared.ai");
    let e = refusal(&b.build());
    assert_eq!(e.field, "_count");
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }), "{e}");
}

/// AH.24's synthetic attack reads back through the graph and the
/// placements' reader: its global's portal manager running its script, the
/// fighter's definition running and his minion and the ball templates, each
/// under the script names the TV's whitelist runs, the ball and the helper
/// (no script) naming their model; its init setup a file of one script and
/// no placement.
#[test]
fn the_synthetic_attack_reads_back() {
    use super::builder::{ATTACK_BALL, ATTACK_BALL_MODEL, ATTACK_HELPER, attack};
    use crate::level::Placements;
    let a = attack();
    let scripts = |bytes: &[u8]| -> Vec<String> {
        let igz = Igz::parse(bytes).unwrap();
        Graph::read(&igz)
            .unwrap()
            .scripts
            .iter()
            .map(|s| {
                s.name
                    .unwrap_or_default()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    assert_eq!(scripts(&a.global), ["Spyro_PortalManager.ai"]);
    assert_eq!(scripts(&a.init_setup), ["Init_Setup_Synthetic.ai"]);
    let mut fighter = scripts(&a.fighter);
    fighter.sort();
    assert_eq!(
        fighter,
        ["009_Def_Eruptor.ai", "009_Eruptor.ai", "009_MagmaBall.ai"]
    );
    let placed = |bytes: &[u8]| -> Vec<(String, u32, String, Option<String>)> {
        let igz = Igz::parse(bytes).unwrap();
        let p = Placements::read(&igz).unwrap();
        p.actors
            .iter()
            .map(|a| {
                (
                    a.name().unwrap_or_default().to_owned(),
                    a.placement().placement.activity_state,
                    a.script
                        .unwrap_or_default()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_owned(),
                    p.model_path(a).map(str::to_owned),
                )
            })
            .collect()
    };
    assert_eq!(
        placed(&a.global),
        [(
            "Spyro_PortalManager".to_owned(),
            4,
            "Spyro_PortalManager.ai".to_owned(),
            None
        )]
    );
    assert!(placed(&a.init_setup).is_empty());
    let mut fighter = placed(&a.fighter);
    fighter.sort();
    assert_eq!(
        fighter,
        [
            (
                "009_Eruptor".to_owned(),
                5,
                "009_Eruptor.ai".to_owned(),
                None
            ),
            (
                "009_Eruptor_Def".to_owned(),
                4,
                "009_Def_Eruptor.ai".to_owned(),
                None
            ),
            (
                ATTACK_HELPER.to_owned(),
                5,
                String::new(),
                Some(ATTACK_BALL_MODEL.to_owned())
            ),
            (
                ATTACK_BALL.to_owned(),
                5,
                "009_MagmaBall.ai".to_owned(),
                Some(ATTACK_BALL_MODEL.to_owned())
            ),
        ]
    );
}

/// AH.27's synthetic breakable reads back: the barrel a running placement
/// with no script where it was put, the level builder's 160 ft range, its
/// solid a solid cylinder of its size, its object index the one `smash`
/// says; the coin a template running the coin's script, its solid a sphere
/// that stops nothing; the coin's script defining the break the ball asks
/// for, its spin checking what touches it and calling the award; and the
/// ball, its first op of a flight a call to the break, the stub
/// unresolved in its own file.
#[test]
fn the_synthetic_breakable_reads_back() {
    use super::builder::{
        SMASH_BARREL, SMASH_BARREL_SIZE, SMASH_BREAK, SMASH_COIN, SMASH_COIN_REACH, SMASH_FAR,
        SMASH_FAR_AHEAD, SMASH_SETUP, attack, smash,
    };
    use crate::level::Placements;
    let s = smash([1.0, 2.0, 3.0]);
    let igz = Igz::parse(&s.level).unwrap();
    let p = Placements::read(&igz).unwrap();
    let barrel = p
        .actors
        .iter()
        .find(|a| a.name() == Some(SMASH_BARREL))
        .unwrap();
    assert_eq!(barrel.index as u32, s.barrel);
    let placed = barrel.placement();
    assert_eq!((placed.placement.activity_state, barrel.script), (4, None));
    assert_eq!(placed.placement.init_matrix.position, [1.0, 2.0, 3.0]);
    assert_eq!(placed.activation_range, 160.0);
    let solid = barrel.collision.unwrap();
    assert_eq!(solid.properties, 0x0C, "a solid cylinder");
    assert_eq!([solid.width, solid.height, solid.length], SMASH_BARREL_SIZE);
    let coin = p
        .actors
        .iter()
        .find(|a| a.name() == Some(SMASH_COIN))
        .unwrap();
    assert_eq!(coin.placement().placement.activity_state, 5);
    assert!(coin.script.unwrap().ends_with("Synthetic_Coin.ai"));
    let reach = coin.collision.unwrap();
    assert_eq!(reach.properties, 0x03, "a sphere that stops nothing");
    assert_eq!(reach.width, SMASH_COIN_REACH);
    let far = p
        .actors
        .iter()
        .find(|a| a.name() == Some(SMASH_FAR))
        .unwrap();
    assert_eq!(
        far.placement().placement.init_matrix.position,
        [1.0, 2.0, 3.0 + SMASH_FAR_AHEAD]
    );
    let setup = p
        .actors
        .iter()
        .find(|a| a.name() == Some(SMASH_SETUP))
        .unwrap();
    assert_eq!(setup.placement().placement.activity_state, 4);
    assert_eq!(setup.placement().activation_range, 0.0, "no range: awake");
    assert!(setup.script.unwrap().ends_with("Synthetic_Setup.ai"));
    let graph = Graph::read(&igz).unwrap();
    let script = graph
        .scripts
        .iter()
        .find(|s| s.name.is_some_and(|n| n.ends_with("Synthetic_Coin.ai")))
        .unwrap();
    let ops = positions(&graph, script);
    assert!(
        ops.iter().any(|o| o.contains(SMASH_BREAK)),
        "the break defined: {ops:?}"
    );
    assert!(ops.iter().any(|o| o.contains("CheckMembership")), "{ops:?}");
    assert!(ops.iter().any(|o| o.contains("spin")), "{ops:?}");
    // The ball's flight asks for the break first.
    let fighter = attack().fighter;
    let igz = Igz::parse(&fighter).unwrap();
    let graph = Graph::read(&igz).unwrap();
    let ball = graph
        .scripts
        .iter()
        .find(|s| s.name.is_some_and(|n| n.ends_with("009_MagmaBall.ai")))
        .unwrap();
    let ops = positions(&graph, ball);
    let fly = ops.iter().position(|o| o.contains("fly")).unwrap();
    assert!(ops[fly + 1].contains("UseMacro"), "{ops:?}");
}

/// A placement's tags (openspec AH.18): its `_tagList` a `TagList` whose
/// `_initialTags` hold each `OpParameter`, read back field for field, the
/// list's own elements empty as level files hold them.
#[test]
fn a_placements_tags_read_back_field_for_field() {
    use crate::igz::layouts::{Layout, ObjectRef, OpParameter, TagList};
    let mut b = IgzBuilder::new();
    let (placement, values) = {
        let mut w = Writer::new(&mut b);
        let placement = w.b.object(0, "tfbActorInfo", 0xF8);
        let thirty = w.int(30);
        let fifty = w.int(50);
        w.tags(
            placement,
            &[
                (
                    "notice player radius",
                    "ValueInfo",
                    "IntMeasurement",
                    thirty,
                ),
                ("communicate range", "ValueInfo", "IntMeasurement", fifty),
            ],
        );
        (placement, [thirty, fifty])
    };
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let p = index(&igz, placement);
    let at = igz.objects()[p].field(0x18).unwrap();
    let crate::igz::Ref::Object(t) = igz.reference(at, "_tagList").unwrap() else {
        panic!("no tag list");
    };
    let list = TagList::read(&igz, t).unwrap();
    assert_eq!(list.list.count, 0, "the list's own elements are run-time");
    assert_eq!(list.initial_tags.count, 2);
    assert_eq!(list.tags, Vec::<usize>::new());
    let read: Vec<(Option<&str>, ObjectRef<'_>)> = list
        .initial
        .iter()
        .map(|&k| {
            let tag = OpParameter::read(&igz, k).unwrap();
            assert_eq!(tag.var.index, -1);
            (tag.var.name, tag.rhs)
        })
        .collect();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].0, Some("notice player radius"));
    assert_eq!(read[1].0, Some("communicate range"));
    for ((_, rhs), v) in read.iter().zip(values) {
        assert_eq!(*rhs, ObjectRef::Object(index(&igz, v)));
    }
}

/// AG.27's synthetic walker reads back for every fault: two scripts (his
/// definition's and the minion's, named for his load file) and two
/// placements, the minion's named as the join finds it; the setup reaches
/// the movement members and the faults reach the one member nothing models.
#[test]
fn the_synthetic_walker_reads_back() {
    use super::builder::{WALKER_LOAD, WalkerFault, walker};
    use crate::level::Placements;
    for fault in [WalkerFault::None, WalkerFault::InSetup, WalkerFault::InPlay] {
        let bytes = walker(fault);
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let mut names: Vec<&str> = graph
            .scripts
            .iter()
            .map(|s| s.name.unwrap_or_default().rsplit('/').next().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["026_Def_Synthetic.ai", "026_Synthetic.ai"]);
        let placed = Placements::read(&igz).unwrap();
        let actors: Vec<(&str, u32)> = placed
            .actors
            .iter()
            .map(|a| {
                (
                    a.name().unwrap_or(""),
                    a.placement().placement.activity_state,
                )
            })
            .collect();
        assert_eq!(
            actors,
            [(WALKER_LOAD, 5), ("026_Synthetic_Def", 4)],
            "{fault:?}"
        );
        let minion = graph
            .scripts
            .iter()
            .find(|s| s.name.is_some_and(|n| n.ends_with("026_Synthetic.ai")))
            .unwrap();
        let ops = positions(&graph, minion).join("\n");
        assert!(ops.contains("Displace"), "{fault:?}: {ops}");
    }
}
