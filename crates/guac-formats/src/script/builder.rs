//! Writes tfbScript op graphs in code (openspec AH.13, `test-fixtures`):
//! scripts, their ops by position and blocks, operands, variables and macro
//! calls, laid out as Giants' files have them, every offset a literal from
//! the registry so a wrong reader constant shows. The synthetic scripts of
//! tests and e2e fixtures (a creature, a breakable, an attack) are written
//! with it; nothing here comes from a disc.

use std::collections::BTreeMap;

use super::{Class, METAOBJECT, StackClass};
use crate::exports::NAMESPACE;
use crate::fnv1a32;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::{BRANCH_END, NO_OPERATOR};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// An op class's instance size, as ac4's registry tables give it.
pub fn size(class: Class) -> u32 {
    match class {
        Class::MacroParameter => 0x48,
        Class::SetValue => 0x24,
        Class::CheckValue | Class::ChangeMembership | Class::SetReference => 0x28,
        Class::MacroInterface => 0x20,
        Class::FlowContinue | Class::FlowMacro | Class::Skip | Class::FlowLoop => 0x1C,
        Class::FlowEnd | Class::StartUp | Class::ShutDown | Class::PreScript => 0x1C,
        Class::MacroSection | Class::StructureSection => 0x1C,
        Class::UseMacro | Class::SetBehavior | Class::UserBehavior => 0x24,
        Class::Spawn => 0x30,
        Class::CreateVariable | Class::CheckReference | Class::Control => 0x2C,
        Class::SlideValue | Class::ForEach => 0x34,
        Class::Remove | Class::IncValue | Class::DecValue => 0x20,
        Class::StopSequence | Class::Reset => 0x20,
        Class::Displace | Class::FindSubSet | Class::CheckMembership => 0x30,
        Class::DefineStructure | Class::MoveTo | Class::MoveFrom => 0x30,
        Class::StartSequence | Class::LoopValue | Class::TurnTo | Class::TeleportTo => 0x28,
        Class::FindVariable | Class::CheckFov => 0x3C,
        Class::DefineMacro => 0x38,
        Class::DefineMacroSpecialization => 0x40,
    }
}

/// A stack class's size as Giants' files give it (`RHSReferenceStack` 4
/// over the registry's 0x2C).
pub fn stack_size(class: StackClass) -> u32 {
    match class {
        StackClass::Value | StackClass::Group => 0x1C,
        StackClass::RhsValue => 0x28,
        StackClass::RhsReference => 0x30,
        _ => 0x18,
    }
}

/// What a stack's path element points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum El<'n> {
    Null,
    Local(Location),
    /// An engine member, imported by FNV-1a of its lowercased name and of its
    /// namespace (`actorinfo`, `health`).
    Member(&'n str, &'n str),
    /// A `global.bld` export, by id.
    Global(u32),
    /// A named external object (EXNM, flagged): a namespace and a name.
    Named(&'n str, &'n str),
}

/// Writes scripts into an [`IgzBuilder`]. Ops are pushed at positions of
/// the script being written; [`Writer::end`] writes its `tfbScriptInfo`.
pub struct Writer<'b> {
    pub b: &'b mut IgzBuilder,
    /// The section lists' memory blocks go in.
    pub blocks: u8,
    flows: BTreeMap<Class, Location>,
    positions: Vec<Location>,
}

impl<'b> Writer<'b> {
    pub fn new(b: &'b mut IgzBuilder) -> Self {
        let blocks = b.section(0x10);
        Self {
            b,
            blocks,
            flows: BTreeMap::new(),
            positions: Vec::new(),
        }
    }

    /// The EXNM index of a `metaobject` class.
    pub fn class(&mut self, name: &str) -> u32 {
        self.b.external_name(METAOBJECT, name)
    }

    pub fn word(&mut self, o: Location, offset: u32, value: u32) {
        self.b.put_u32(at(o, offset), value);
    }

    pub fn half(&mut self, o: Location, offset: u32, value: u16) {
        self.b.put_bytes(at(o, offset), &value.to_be_bytes());
    }

    pub fn refer(&mut self, o: Location, offset: u32, target: Location) {
        self.b.pointer(at(o, offset), target);
    }

    pub fn text(&mut self, o: Location, offset: u32, text: &str) {
        self.b.string(at(o, offset), text);
    }

    /// A class word without the index flag, as literals' `_type` and
    /// variables' containers are stored.
    pub fn class_word(&mut self, o: Location, offset: u32, class: &str) {
        let index = self.class(class);
        self.b.unflagged_meta(at(o, offset), index);
    }

    /// A flagged class word, as `_editType` and contents types are stored.
    pub fn meta_word(&mut self, o: Location, offset: u32, class: &str) {
        self.b.meta_object(at(o, offset), METAOBJECT, class);
    }

    /// An `igDataList` at `list` holding `elements`.
    pub fn path(&mut self, list: Location, elements: &[El<'_>]) {
        let n = elements.len() as u32;
        self.word(list, 0x08, n);
        self.word(list, 0x0C, n);
        if elements.is_empty() {
            self.b.null_memory_ref(at(list, 0x10), 0);
            return;
        }
        let data = self
            .b
            .memory_ref(at(list, 0x10), self.blocks, &vec![0; 4 * n as usize], 0);
        for (i, e) in elements.iter().enumerate() {
            let w = at(data, 4 * i as u32);
            match *e {
                El::Null => {}
                El::Local(o) => self.b.pointer(w, o),
                El::Member(namespace, member) => {
                    self.b.import(w, namespace, &member.to_lowercase());
                }
                El::Global(id) => {
                    self.b.import_raw(w, id, fnv1a32(NAMESPACE.as_bytes()));
                }
                El::Named(namespace, name) => {
                    self.b.meta_object(w, namespace, name);
                }
            }
        }
    }

    /// A stack of `class` holding `path`, its other fields zero.
    pub fn stack(&mut self, class: StackClass, path: &[El<'_>]) -> Location {
        let s = self.b.object(0, class.type_name(), stack_size(class));
        self.path(s, path);
        s
    }

    /// A `ValueStack`: a cell read or written.
    pub fn value(&mut self, path: &[El<'_>]) -> Location {
        self.stack(StackClass::Value, path)
    }

    /// An `RHSValueStack` reading `path` as `class`.
    pub fn rhs(&mut self, class: &str, path: &[El<'_>]) -> Location {
        let s = self.stack(StackClass::RhsValue, path);
        self.class_word(s, 0x1C, class);
        s
    }

    /// A literal: an `RHSValueStack` with an empty path.
    pub fn literal(&mut self, class: &str, bits: u32, random: bool) -> Location {
        let s = self.rhs(class, &[]);
        self.word(s, 0x20, bits);
        self.b.put_bytes(at(s, 0x24), &[random.into()]);
        s
    }

    pub fn int(&mut self, value: i32) -> Location {
        self.literal("IntMeasurement", value as u32, false)
    }

    pub fn float(&mut self, value: f32) -> Location {
        self.literal("FloatMeasurement", value.to_bits(), false)
    }

    /// An `RHSReferenceStack` naming `path`, an object of `edit_type`.
    pub fn reference(&mut self, path: &[El<'_>], edit_type: Option<&str>) -> Location {
        let s = self.stack(StackClass::RhsReference, path);
        self.class_word(s, 0x1C, "IntMeasurement");
        if let Some(t) = edit_type {
            self.meta_word(s, 0x28, t);
        }
        s
    }

    /// A `ScriptGroupStack` naming `path`, an object of `edit_type`.
    pub fn group(&mut self, path: &[El<'_>], edit_type: Option<&str>) -> Location {
        let s = self.stack(StackClass::Group, path);
        if let Some(t) = edit_type {
            self.meta_word(s, 0x18, t);
        }
        s
    }

    /// A `ValueRHSVariant`: `a <operator> b` (0 + 1 - 2 * 3 /), or `a`
    /// alone.
    pub fn expr(&mut self, a: Location, operator_and_b: Option<(u32, Location)>) -> Location {
        let v = self.b.object(0, "ValueRHSVariant", 0x1C);
        self.refer(v, 0x08, a);
        match operator_and_b {
            Some((operator, b)) => {
                self.word(v, 0x0C, operator);
                self.refer(v, 0x10, b);
            }
            None => self.word(v, 0x0C, NO_OPERATOR),
        }
        v
    }

    /// A zeroed op of `class` named `name`, not at any position yet.
    pub fn op(&mut self, class: Class, name: &str) -> Location {
        let o = self.b.object(0, class.type_name(), size(class));
        self.text(o, 0x08, name);
        o
    }

    /// An `OpCreateVariable`: `container` its unflagged class, `contents`
    /// its flagged one; `_varIndex` -1 as files hold it.
    pub fn variable(&mut self, name: &str, container: &str, contents: Option<&str>) -> Location {
        let v = self.op(Class::CreateVariable, "create variable||with|members");
        self.text(v, 0x1C, name);
        self.class_word(v, 0x20, container);
        if let Some(c) = contents {
            self.meta_word(v, 0x24, c);
        }
        self.word(v, 0x28, u32::MAX);
        v
    }

    /// The file's one object of a flow class (`flow continue`, `flow
    /// macro`, `flow`, `flow end`), as Giants shares them.
    pub fn flow(&mut self, class: Class) -> Location {
        if let Some(&o) = self.flows.get(&class) {
            return o;
        }
        let name = match class {
            Class::FlowContinue => "flow continue",
            Class::FlowMacro => "flow macro",
            Class::FlowLoop => "flow",
            Class::FlowEnd => "flow end",
            other => panic!("{other:?} isn't a shared flow"),
        };
        let o = self.op(class, name);
        if class == Class::FlowEnd {
            self.half(o, 0x1A, BRANCH_END);
        }
        self.flows.insert(class, o);
        o
    }

    /// Puts `op` at the next position, the index a block's length counts from.
    pub fn push(&mut self, op: Location) -> usize {
        self.positions.push(op);
        self.positions.len() - 1
    }

    /// The open script's position count, where the next op lands.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Sets the block of the op at `start` to run through the last position.
    pub fn close(&mut self, start: usize) {
        let n = self.positions.len() - 1 - start;
        let op = self.positions[start];
        self.half(op, 0x1A, n as u16);
    }

    /// Ends the block opened at `start` with `flow continue`.
    pub fn end_block(&mut self, start: usize) {
        let flow = self.flow(Class::FlowContinue);
        self.push(flow);
        self.close(start);
    }

    /// Opens a block with a fresh op of `class` (a section, a check built
    /// by hand); returns its position and object.
    pub fn open(&mut self, class: Class, name: &str) -> (usize, Location) {
        let o = self.op(class, name);
        (self.push(o), o)
    }

    /// Opens a named behaviour (`idle`, `battle`).
    pub fn behaviour(&mut self, name: &str) -> usize {
        self.open(Class::UserBehavior, name).0
    }

    /// `set <lhs> = <rhs>`.
    pub fn set(&mut self, lhs: &[El<'_>], rhs: Location) -> Location {
        let lhs = self.value(lhs);
        let o = self.op(Class::SetValue, "set||=");
        self.refer(o, 0x1C, lhs);
        self.refer(o, 0x20, rhs);
        self.push(o);
        o
    }

    /// `inc value <lhs>`.
    pub fn inc(&mut self, lhs: &[El<'_>]) -> Location {
        let lhs = self.value(lhs);
        let o = self.op(Class::IncValue, "inc value");
        self.refer(o, 0x1C, lhs);
        self.push(o);
        o
    }

    /// `if <lhs> <relation> <rhs>`: returns its position; the block follows.
    pub fn check(&mut self, lhs: &[El<'_>], relation: u32, rhs: Location) -> usize {
        let lhs = self.value(lhs);
        let (p, o) = self.open(Class::CheckValue, "if");
        self.refer(o, 0x1C, lhs);
        self.word(o, 0x20, relation);
        self.refer(o, 0x24, rhs);
        p
    }

    /// Ends the check at `check`'s block with the else's skip, which is its
    /// last op; returns the skip's position for [`Writer::close`] after the
    /// else block.
    pub fn otherwise(&mut self, check: usize) -> usize {
        let (p, _) = self.open(Class::Skip, "else");
        self.close(check);
        p
    }

    /// `loop value <count>`: returns its position; [`Writer::end_loop`]
    /// closes it.
    pub fn loop_value(&mut self, count: Location) -> usize {
        let (p, o) = self.open(Class::LoopValue, "loop value|||loop index");
        self.refer(o, 0x20, count);
        p
    }

    pub fn end_loop(&mut self, start: usize) {
        let flow = self.flow(Class::FlowLoop);
        self.push(flow);
        self.close(start);
    }

    /// `flow end`: returns from a macro, or ends the tick.
    pub fn flow_end(&mut self) {
        let flow = self.flow(Class::FlowEnd);
        self.push(flow);
    }

    /// Opens a macro definition named `name` at the next position: its op,
    /// its INTERFACE with one parameter per default, and `flow macro`.
    /// Returns its position and object; [`Writer::end_block`] ends it.
    pub fn define(&mut self, name: &str, defaults: &[(&str, Location)]) -> (usize, Location) {
        let (p, def) = self.open(Class::DefineMacro, name);
        self.interface(def, defaults);
        (p, def)
    }

    /// An INTERFACE block binding `args` into `def`'s parameters.
    fn interface(&mut self, def: Location, args: &[(&str, Location)]) {
        let (p, _) = self.open(Class::MacroInterface, "INTERFACE");
        for (name, rhs) in args {
            let o = self.op(Class::MacroParameter, "parameter");
            self.text(o, 0x1C, name);
            self.class_word(o, 0x20, "ValueInfo");
            self.word(o, 0x28, u32::MAX);
            self.refer(o, 0x30, *rhs);
            self.refer(o, 0x38, def);
            self.push(o);
        }
        let flow = self.flow(Class::FlowMacro);
        self.push(flow);
        self.close(p);
    }

    /// `use macro <def>` with `args`, its block closed with nothing after
    /// the return.
    pub fn call(&mut self, def: Location, args: &[(&str, Location)]) -> usize {
        let np = self.group(&[El::Local(def)], None);
        let (p, o) = self.open(Class::UseMacro, "use macro");
        self.refer(o, 0x1C, np);
        self.interface(def, args);
        self.end_block(p);
        p
    }

    /// `spawn <template> at <position>`, facing `facing`: returns its
    /// position; the block (run with the clone) follows.
    pub fn spawn(&mut self, template: &[El<'_>], position: &[El<'_>], facing: Location) -> usize {
        let lhs = self.stack(StackClass::PlacementList, template);
        let rhs = self.stack(StackClass::Position, position);
        let (p, o) = self.open(Class::Spawn, "spawn|at|facing");
        self.refer(o, 0x20, lhs);
        self.refer(o, 0x28, rhs);
        self.refer(o, 0x2C, facing);
        p
    }

    /// `start sequence <sequence>`: returns its position; the block follows.
    pub fn start_sequence(&mut self, sequence: &[El<'_>]) -> usize {
        let lhs = self.group(sequence, None);
        let (p, o) = self.open(Class::StartSequence, "start sequence");
        self.refer(o, 0x1C, lhs);
        p
    }

    /// `set behavior <behaviour>`.
    pub fn set_behavior(&mut self, behaviour: Location) -> Location {
        let np = self.group(&[El::Local(behaviour)], None);
        let o = self.op(Class::SetBehavior, "set behavior");
        self.refer(o, 0x1C, np);
        self.push(o);
        o
    }

    /// `remove <what>`.
    pub fn remove(&mut self, what: &[El<'_>]) -> Location {
        let np = self.group(what, None);
        let o = self.op(Class::Remove, "remove");
        self.refer(o, 0x1C, np);
        self.push(o);
        o
    }

    /// Writes the open script's list and its `tfbScriptInfo` named `name`
    /// (an `.ai` path); returns the info.
    pub fn end(&mut self, name: &str) -> Location {
        let info = self.b.object(0, "tfbScriptInfo", 0x1C);
        let list = self.b.object(0, "OpCodeList", 0x18);
        self.text(info, 0x08, name);
        self.b.put_bytes(at(info, 0x10), &[1]);
        self.refer(info, 0x14, list);
        let ops: Vec<El<'_>> = std::mem::take(&mut self.positions)
            .into_iter()
            .map(El::Local)
            .collect();
        self.path(list, &ops);
        info
    }

    /// A script's frame as every script on the disc has it: PRESCRIPT
    /// holding STARTUP (`startup`'s ops), SHUTDOWN and a MACROS section
    /// (`macros`' definitions), then the behaviours `behaviours` writes.
    /// Returns the info.
    pub fn script(
        &mut self,
        name: &str,
        startup: impl FnOnce(&mut Self),
        macros: impl FnOnce(&mut Self),
        behaviours: impl FnOnce(&mut Self),
    ) -> Location {
        let (pre, _) = self.open(Class::PreScript, "PRESCRIPT");
        let (start, _) = self.open(Class::StartUp, "STARTUP");
        startup(self);
        self.end_block(start);
        let (shut, _) = self.open(Class::ShutDown, "SHUTDOWN");
        self.end_block(shut);
        let (section, _) = self.open(Class::MacroSection, "MACROS");
        macros(self);
        self.end_block(section);
        self.end_block(pre);
        behaviours(self);
        self.end(name)
    }
}

/// A Chompy-shaped script with our numbers, touching the writer's every
/// helper: a health variable and a macro that lowers it, a check with an
/// else, a loop, a behaviour switch, a spawn, a sequence and a removal.
/// Returns the file and where its script landed.
pub fn creature() -> (Vec<u8>, Location) {
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let health = w.variable("health", "ValueInfo", Some("IntMeasurement"));
    let amount = w.variable("amount", "ValueInfo", Some("IntMeasurement"));
    let idle = w.op(Class::UserBehavior, "idle");
    let hurt = std::cell::Cell::new(None);
    let script = w.script(
        "C:/tfb/Content/Levels/Synthetic/Creature.ai",
        |w| {
            let five = w.int(5);
            w.set(&[El::Local(health)], five);
            let half = w.float(0.5);
            w.set(&[El::Member("actorinfo", "Movement")], half);
        },
        // A macro that lowers health by its argument.
        |w| {
            let zero = w.int(0);
            let (at, def) = w.define("Combat - Synthetic Hurt", &[("amount", zero)]);
            let h = w.rhs("IntMeasurement", &[El::Local(health)]);
            let a = w.rhs("IntMeasurement", &[El::Local(amount)]);
            let less = w.expr(h, Some((1, a)));
            w.set(&[El::Local(health)], less);
            w.flow_end();
            w.end_block(at);
            hurt.set(Some(def));
        },
        |w| {
            let start = w.push(idle);
            let two = w.int(2);
            w.call(hurt.get().expect("defined"), &[("amount", two)]);
            // if health <= 0 { remove me } else { loop 3 { inc } }
            let zero = w.int(0);
            let check = w.check(&[El::Local(health)], 0, zero);
            w.remove(&[El::Member("optoplevelbehavior", "my")]);
            let other = w.otherwise(check);
            let three = w.int(3);
            let looped = w.loop_value(three);
            w.inc(&[El::Global(0x30)]);
            w.end_loop(looped);
            w.close(other);
            let facing = w.int(90);
            let spawned = w.spawn(
                &[El::Named("SpriteInfo", "Orb_Template")],
                &[El::Member("optoplevelbehavior", "my")],
                facing,
            );
            w.end_block(spawned);
            let seq = w.start_sequence(&[El::Named("ActorInfo", "Chompy_Bite")]);
            w.end_block(seq);
            w.set_behavior(idle);
            w.end_block(start);
        },
    );
    (b.build(), script)
}

/// Seeds for the property test and the fuzz corpus.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![creature().0, every_class().0]
}

/// One op of each of the 44 classes at a position of one script, each
/// pointing its first operand field at a literal: the shape the
/// interpreter's own fuzz target starts from. Returns the file and the ops
/// in class order.
pub fn every_class() -> (Vec<u8>, Vec<Location>) {
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let mut ops = Vec::new();
    for &class in Class::ALL {
        let o = match class {
            Class::FlowContinue | Class::FlowMacro | Class::FlowLoop | Class::FlowEnd => {
                w.flow(class)
            }
            _ => w.op(class, class.type_name()),
        };
        match class {
            Class::SetValue | Class::IncValue | Class::DecValue | Class::CheckValue => {
                let one = w.int(1);
                let off = if class == Class::CheckValue {
                    0x24
                } else {
                    0x1C
                };
                w.refer(o, off, one);
            }
            Class::CreateVariable | Class::MacroParameter => {
                w.text(o, 0x1C, "v");
                w.class_word(o, 0x20, "ValueInfo");
            }
            _ => {}
        }
        w.push(o);
        ops.push(o);
    }
    w.end("C:/tfb/Content/Levels/Synthetic/EveryClass.ai");
    (b.build(), ops)
}
