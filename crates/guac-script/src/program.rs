//! Linking (crate docs, Programs and placements): AH.13's op graphs of the
//! files loaded together become one [`Program`] of ids, every `global.bld`
//! import resolved through the export ids and every object an op reaches
//! decoded once. Nothing here runs a script.

use std::collections::{HashMap, VecDeque};

use guac_formats::exports::{Export, Exports, NAMESPACE, Root};
use guac_formats::igz::layouts::{
    Layout, NO_OPERATOR, ObjectList, ObjectRef, ScriptSet, ScriptSetList, TypeRef, ValueInfo,
    Variable,
};
use guac_formats::igz::{ExternalId, Igz, Ref};
use guac_formats::progression::ScriptStructure;
use guac_formats::script::{Class, Graph, Op, Operand as Raw, StackClass};
use guac_formats::{FormatError, fnv1a32};

use crate::host::MemberId;
use crate::value::Value;

/// An object of a loaded file: the unit's place in [`Program::link`]'s
/// list, and the object's index in that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Obj {
    pub unit: u16,
    pub index: u32,
}

pub type OpId = u32;
pub type OperandId = u32;
pub type ScriptId = u32;
pub type DefId = u32;
pub type GlobalId = u32;
/// An interned name, lowercased: frames, fields and behaviours match names
/// ignoring case.
pub type Sym = u32;

/// What a file is to the export ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `permanent/bootstrap.bld`: its roots take export ids 1-19.
    Bootstrap,
    /// `permanent/global.bld`: its roots follow bootstrap's.
    Global,
    /// A level, character or any other file.
    Other,
}

/// A file's parsed IGZ and its op graph, and what it is to the export ids.
pub struct Unit<'u, 'a> {
    /// For logs (`character/init_setup.bld`).
    pub name: &'u str,
    pub role: Role,
    pub igz: &'u Igz<'a>,
    pub graph: &'u Graph<'a>,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("{unit}: {error}")]
    Format { unit: String, error: FormatError },
    #[error("two files given the role {0:?}")]
    TwoRoles(Role),
    #[error("more than u32::MAX {0}")]
    TooMany(&'static str),
}

/// What an RHS literal's 32 bits are, by its `_type` class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralClass {
    Int,
    Float,
    Color,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Literal {
    pub class: LiteralClass,
    pub bits: u32,
    pub random: bool,
}

impl Literal {
    pub fn value(&self) -> Value {
        match self.class {
            LiteralClass::Int => Value::Int(self.bits as i32),
            LiteralClass::Float => Value::Float(f32::from_bits(self.bits)),
            LiteralClass::Color => Value::Color(self.bits),
            LiteralClass::Screen => Value::Screen(self.bits),
        }
    }
}

/// `+ - * /`, `_arithOperator` 0-3; any other word is kept and refused when
/// computed.
pub type Arith = u32;

/// A path step with its import or object resolved to the program's ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    /// `find subset`'s empty slot.
    Null,
    /// An op: a variable, a parameter, a loop or check whose result a path
    /// reads, or a definition or behaviour read as an object.
    Op(OpId),
    /// A `ValueInfo`, set, structure or reference of a file.
    Global(GlobalId),
    /// Any other object of a file, read as itself.
    Object(Obj),
    /// An object named by namespace and name.
    Named(Sym, Sym),
    Member(MemberId),
    /// An import nothing loaded resolves; reaching it stops as unsupported.
    Unresolved(Sym),
}

/// An operand with its steps and sub-operands as the program's ids.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Path {
        class: StackClass,
        elems: Vec<Elem>,
        /// `_otherPoint`.
        other: Option<OperandId>,
        /// An RHS stack's literal, read when the path is empty.
        literal: Option<Literal>,
        /// The path names `abstractscriptgroup.its`.
        names_its: bool,
    },
    /// `a <op> b`, or `a` alone.
    Expr {
        a: Option<OperandId>,
        op: Option<Arith>,
        b: Option<OperandId>,
    },
}

/// What a variable's declaration makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
    Value,
    Reference,
    Set,
    SetReference,
    Structure,
    /// A container class the VM doesn't make; making one is unsupported.
    Unknown(Sym),
}

/// A declaration's contents type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Contents {
    None,
    Class(Sym),
    Def(DefId),
    /// An import nothing loaded resolves.
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decl {
    pub container: Container,
    pub contents: Contents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub name: Sym,
    pub id: u64,
    pub combine: u32,
    pub rhs: Option<OperandId>,
    pub index: Option<OperandId>,
    /// `_macroDef`: the definition it binds into.
    pub def: Option<DefId>,
    pub decl: Decl,
}

/// One op, linked; its block length is [`Program::branch`].
#[derive(Debug, Clone, PartialEq)]
pub enum Ins {
    PreScript,
    StartUp,
    ShutDown,
    /// A MACROS or STRUCTURES section: skipped.
    Section,
    /// A definition at a position: skipped; run only when called.
    Define(DefId),
    Behaviour {
        name: Sym,
    },
    /// `flow continue`.
    Continue,
    /// `flow macro`.
    EnterMacro,
    /// `else`: a forward jump, then close.
    Skip,
    /// `flow`.
    Loop,
    /// `flow end`.
    End,
    Interface,
    Parameter(Param),
    Variable {
        name: Sym,
        decl: Decl,
    },
    UseMacro {
        np: Option<OperandId>,
        index: Option<OperandId>,
    },
    SetValue {
        lhs: Option<OperandId>,
        rhs: Option<OperandId>,
    },
    SetReference {
        lhs: Option<OperandId>,
        rhs: Option<OperandId>,
        index: Option<OperandId>,
    },
    Inc {
        lhs: Option<OperandId>,
    },
    Dec {
        lhs: Option<OperandId>,
    },
    Reset {
        np: Option<OperandId>,
    },
    CheckValue {
        lhs: Option<OperandId>,
        relation: u32,
        rhs: Option<OperandId>,
    },
    CheckReference {
        lhs: Option<OperandId>,
        relation: u32,
        rhs: Option<OperandId>,
        index: Option<OperandId>,
    },
    CheckMembership {
        lhs: Option<OperandId>,
        code: u32,
        rhs: Option<OperandId>,
    },
    ChangeMembership {
        lhs: Option<OperandId>,
        code: u32,
        rhs: Option<OperandId>,
    },
    LoopValue {
        rhs: Option<OperandId>,
    },
    ForEach {
        lhs: Option<OperandId>,
        dir: u32,
        rhs: Option<OperandId>,
    },
    FindSubSet {
        lhs: Option<OperandId>,
        relation: u32,
        rhs: Option<OperandId>,
    },
    SetBehavior {
        np: Option<OperandId>,
        index: Option<OperandId>,
    },
    /// `find variable`: a variable named `name` of `decl`'s types on what
    /// `owner` names (an element of it when `index` is given), found, made
    /// or taken off as `filter` says (crate docs, Variables, structures and
    /// sets).
    FindVariable {
        name: Sym,
        decl: Decl,
        filter: u32,
        owner: Option<OperandId>,
        index: Option<OperandId>,
        skip: Option<OperandId>,
    },
    /// An op left to the host ([`crate::Host::op`]).
    World {
        operands: Vec<(&'static str, Option<OperandId>)>,
        words: Vec<(&'static str, u32)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpInfo {
    pub class: Class,
    pub obj: Obj,
    pub branch: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Macro,
    Specialization,
    Structure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub kind: DefKind,
    pub name: Sym,
    /// As the file spells it, for logs.
    pub display: String,
    pub op: OpId,
    pub obj: Obj,
    /// Its script and position, when it sits in a list (else a stub).
    pub placed: Option<(ScriptId, u32)>,
    pub publicity: u32,
    /// A specialization's base.
    pub base: Option<DefId>,
    /// A structure's field declarations, in its block's order.
    pub fields: Vec<OpId>,
    base_operand: Option<OperandId>,
}

/// A file's variable: shared by every placement, made on first use.
#[derive(Debug, Clone, PartialEq)]
pub enum Global {
    Number(Value),
    Set { members: Vec<Elem>, elem: Contents },
    Structure { def: Option<DefId> },
    Reference(Elem),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptInfo {
    /// The `.ai` source path.
    pub name: String,
    pub unit: u16,
    pub ops: Vec<OpId>,
    /// Behaviours by name, first position wins.
    pub behaviours: HashMap<Sym, u32>,
    /// The op right after PRESCRIPT's block, when it's a behaviour.
    pub first_behaviour: Option<u32>,
    /// SHUTDOWN's position.
    pub shutdown: Option<u32>,
    /// The variables its `_masterVarList` declares, which a placement's
    /// locals are built from, by name.
    pub declared: HashMap<Sym, Decl>,
}

/// Every op list of the files linked together.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub units: Vec<String>,
    pub scripts: Vec<ScriptInfo>,
    pub ops: Vec<Ins>,
    pub op_info: Vec<OpInfo>,
    pub operands: Vec<Operand>,
    pub defs: Vec<Def>,
    pub globals: Vec<(Obj, Global)>,
    /// A stub's definition: itself when placed, else the placed one of its
    /// name and kind (one with `_publicity` 1 first), else none.
    resolved: Vec<Option<DefId>>,
    syms: Vec<String>,
    sym_ids: HashMap<String, Sym>,
    macros: HashMap<Sym, Vec<DefId>>,
    structures: HashMap<Sym, Vec<DefId>>,
    defs_by_obj: HashMap<Obj, DefId>,
    ops_by_obj: HashMap<Obj, OpId>,
    globals_by_obj: HashMap<Obj, GlobalId>,
    names: HashMap<Obj, String>,
}

impl Program {
    /// Links `units`. At most one each may be [`Role::Bootstrap`] and
    /// [`Role::Global`]; `global.bld` imports resolve only with both.
    pub fn link(units: &[Unit<'_, '_>]) -> Result<Self, LinkError> {
        Linker::new(units)?.run()
    }

    pub fn sym(&self, s: Sym) -> &str {
        self.syms.get(s as usize).map_or("", String::as_str)
    }

    /// The interned id of `name`, lowercased, if any op names it.
    pub fn find_sym(&self, name: &str) -> Option<Sym> {
        self.sym_ids.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn branch(&self, op: OpId) -> u32 {
        self.op_info
            .get(op as usize)
            .map_or(0, |i| u32::from(i.branch))
    }

    pub fn class(&self, op: OpId) -> Option<Class> {
        self.op_info.get(op as usize).map(|i| i.class)
    }

    /// The placed definition a definition or stub runs as.
    pub fn resolve(&self, def: DefId) -> Option<DefId> {
        self.resolved.get(def as usize).copied().flatten()
    }

    pub fn def_of(&self, obj: Obj) -> Option<DefId> {
        self.defs_by_obj.get(&obj).copied()
    }

    pub fn op_of(&self, obj: Obj) -> Option<OpId> {
        self.ops_by_obj.get(&obj).copied()
    }

    pub fn global_of(&self, obj: Obj) -> Option<GlobalId> {
        self.globals_by_obj.get(&obj).copied()
    }

    /// A placed macro by name.
    pub fn macro_named(&self, name: &str) -> Option<DefId> {
        let s = self.find_sym(name)?;
        self.macros.get(&s)?.first().copied()
    }

    /// The script whose source path ends with `suffix`.
    pub fn script_named(&self, suffix: &str) -> Option<ScriptId> {
        self.scripts
            .iter()
            .position(|s| s.name.ends_with(suffix))
            .map(|i| i as ScriptId)
    }

    /// A file variable by its name (`toy data`), first unit first.
    pub fn global_named(&self, name: &str) -> Option<GlobalId> {
        let mut found: Vec<_> = self
            .globals
            .iter()
            .enumerate()
            .filter(|(_, (o, _))| self.names.get(o).is_some_and(|n| n == name))
            .map(|(i, (o, _))| (*o, i as GlobalId))
            .collect();
        found.sort();
        found.first().map(|(_, g)| *g)
    }

    /// An object's `_name`, for one an op reaches that has one.
    pub fn name_of(&self, obj: Obj) -> Option<&str> {
        self.names.get(&obj).map(String::as_str)
    }
}

/// Whether an object links as an op, a file variable or a plain object.
enum Kind {
    Op,
    Global,
    Plain,
}

fn kind_of(type_name: &str) -> Kind {
    if Class::from_type(type_name).is_some() {
        Kind::Op
    } else if matches!(
        type_name,
        "ValueInfo"
            | "ScriptScreenInfo"
            | "ScriptColorInfo"
            | "ScriptSet"
            | "ScriptStructure"
            | "ScriptReference"
            | "PlacementReference"
            | "ScriptSetReference"
    ) {
        Kind::Global
    } else {
        Kind::Plain
    }
}

/// The reference globals' `_obj`: not a layout `guac-formats` registers, so
/// read as the one word aj-vm read it as (0x1C).
const REFERENCE_TARGET: u32 = 0x1C;

/// A screen position's or colour's `_value` (`ScriptScreenInfo`,
/// `ScriptColorInfo`), at the registry's offset, read as the packed word.
const PACKED_VALUE: u32 = 0x18;

enum Work {
    Op(Obj),
    Operand(Obj),
    Global(Obj),
}

struct Linker<'l, 'u, 'a> {
    units: &'l [Unit<'u, 'a>],
    exports: Option<(u16, u16, Exports)>,
    p: Program,
    operands_by_obj: HashMap<Obj, OperandId>,
    queue: VecDeque<Work>,
    global_ns: u32,
    level_ns: u32,
}

fn count<T>(v: &[T], what: &'static str) -> Result<u32, LinkError> {
    u32::try_from(v.len()).map_err(|_| LinkError::TooMany(what))
}

impl<'l, 'u, 'a> Linker<'l, 'u, 'a> {
    fn new(units: &'l [Unit<'u, 'a>]) -> Result<Self, LinkError> {
        let role = |r: Role| -> Result<Option<u16>, LinkError> {
            let mut it = units.iter().enumerate().filter(|(_, u)| u.role == r);
            let first = it.next().map(|(i, _)| i as u16);
            if it.next().is_some() {
                return Err(LinkError::TwoRoles(r));
            }
            Ok(first)
        };
        let exports = match (role(Role::Bootstrap)?, role(Role::Global)?) {
            (Some(b), Some(g)) => {
                let e = Exports::new(units[b as usize].igz, units[g as usize].igz).map_err(
                    |error| LinkError::Format {
                        unit: units[g as usize].name.to_owned(),
                        error,
                    },
                )?;
                Some((b, g, e))
            }
            _ => None,
        };
        Ok(Self {
            units,
            exports,
            p: Program {
                units: units.iter().map(|u| u.name.to_owned()).collect(),
                ..Program::default()
            },
            operands_by_obj: HashMap::new(),
            queue: VecDeque::new(),
            global_ns: fnv1a32(NAMESPACE.as_bytes()),
            level_ns: fnv1a32(b"level.bld"),
        })
    }

    fn sym(&mut self, name: &str) -> Sym {
        let key = name.to_ascii_lowercase();
        if let Some(&s) = self.p.sym_ids.get(&key) {
            return s;
        }
        let s = self.p.syms.len() as Sym;
        self.p.syms.push(key.clone());
        self.p.sym_ids.insert(key, s);
        s
    }

    fn igz(&self, unit: u16) -> &'u Igz<'a> {
        self.units[unit as usize].igz
    }

    fn type_name(&self, obj: Obj) -> &'a str {
        let igz = self.igz(obj.unit);
        igz.objects()
            .get(obj.index as usize)
            .map_or("", |o| igz.type_name(o))
    }

    fn fail(&self, unit: u16, error: FormatError) -> LinkError {
        LinkError::Format {
            unit: self.units[unit as usize].name.to_owned(),
            error,
        }
    }

    fn remember_name(&mut self, obj: Obj) {
        if self.p.names.contains_key(&obj) {
            return;
        }
        let igz = self.igz(obj.unit);
        let Some(o) = igz.objects().get(obj.index as usize) else {
            return;
        };
        if let Some(at) = o.field(0x08)
            && let Ok(Some(s)) = igz.string(at, "_name")
        {
            self.p.names.insert(obj, s.to_owned());
        }
    }

    fn run(mut self) -> Result<Program, LinkError> {
        let mut lists = Vec::new();
        for (u, unit) in self.units.iter().enumerate() {
            for s in &unit.graph.scripts {
                let mut ops = Vec::with_capacity(s.ops.len());
                for &o in &s.ops {
                    let obj = Obj {
                        unit: u as u16,
                        index: o as u32,
                    };
                    ops.push(self.op_id(obj)?);
                }
                let mut declared = Vec::new();
                if let ObjectRef::Object(l) = s.master_var_list {
                    let list = ObjectList::read(unit.igz, l).map_err(|e| self.fail(u as u16, e))?;
                    for v in list.elements.into_iter().flatten() {
                        declared.push(self.op_id(Obj {
                            unit: u as u16,
                            index: v as u32,
                        })?);
                    }
                }
                lists.push(declared);
                self.p.scripts.push(ScriptInfo {
                    name: s.name.unwrap_or("").to_owned(),
                    unit: u as u16,
                    ops,
                    behaviours: HashMap::new(),
                    first_behaviour: None,
                    shutdown: None,
                    declared: HashMap::new(),
                });
            }
        }
        count(&self.p.scripts, "scripts")?;
        while let Some(w) = self.queue.pop_front() {
            match w {
                Work::Op(obj) => self.link_op(obj)?,
                Work::Operand(obj) => self.link_operand(obj)?,
                Work::Global(obj) => self.link_global(obj)?,
            }
        }
        for (script, declared) in self.p.scripts.iter_mut().zip(lists) {
            for op in declared {
                if let Ins::Variable { name, decl } = &self.p.ops[op as usize] {
                    script.declared.entry(*name).or_insert(*decl);
                }
            }
        }
        self.finish();
        Ok(self.p)
    }

    /// The id of the op at `obj`, queued for linking the first time.
    fn op_id(&mut self, obj: Obj) -> Result<OpId, LinkError> {
        if let Some(&id) = self.p.ops_by_obj.get(&obj) {
            return Ok(id);
        }
        let id = count(&self.p.ops, "ops")?;
        let class = Class::from_type(self.type_name(obj)).unwrap_or(Class::FlowContinue);
        self.p.ops.push(Ins::Continue);
        self.p.op_info.push(OpInfo {
            class,
            obj,
            branch: 0,
        });
        self.p.ops_by_obj.insert(obj, id);
        self.queue.push_back(Work::Op(obj));
        Ok(id)
    }

    fn operand_id(&mut self, unit: u16, r: ObjectRef<'_>) -> Result<Option<OperandId>, LinkError> {
        let ObjectRef::Object(i) = r else {
            return Ok(None);
        };
        let obj = Obj {
            unit,
            index: i as u32,
        };
        if let Some(&id) = self.operands_by_obj.get(&obj) {
            return Ok(Some(id));
        }
        let id = count(&self.p.operands, "operands")?;
        self.p.operands.push(Operand::Expr {
            a: None,
            op: None,
            b: None,
        });
        self.operands_by_obj.insert(obj, id);
        self.queue.push_back(Work::Operand(obj));
        Ok(Some(id))
    }

    fn def_id(&mut self, obj: Obj) -> Result<DefId, LinkError> {
        if let Some(&d) = self.p.defs_by_obj.get(&obj) {
            return Ok(d);
        }
        let op = self.op_id(obj)?;
        let d = count(&self.p.defs, "definitions")?;
        self.p.defs.push(Def {
            kind: DefKind::Macro,
            name: 0,
            display: String::new(),
            op,
            obj,
            placed: None,
            publicity: 0,
            base: None,
            fields: Vec::new(),
            base_operand: None,
        });
        self.p.defs_by_obj.insert(obj, d);
        Ok(d)
    }

    fn global_id(&mut self, obj: Obj) -> Result<GlobalId, LinkError> {
        if let Some(&g) = self.p.globals_by_obj.get(&obj) {
            return Ok(g);
        }
        let g = count(&self.p.globals, "globals")?;
        self.p.globals.push((obj, Global::Number(Value::Int(0))));
        self.p.globals_by_obj.insert(obj, g);
        self.remember_name(obj);
        self.queue.push_back(Work::Global(obj));
        Ok(g)
    }

    /// An object of a file as a path element or set member.
    fn local(&mut self, obj: Obj) -> Result<Elem, LinkError> {
        if self
            .igz(obj.unit)
            .objects()
            .get(obj.index as usize)
            .is_none()
        {
            return Ok(Elem::Unresolved(self.sym("object past the file's end")));
        }
        Ok(match kind_of(self.type_name(obj)) {
            Kind::Op => {
                let op = self.op_id(obj)?;
                if matches!(
                    Class::from_type(self.type_name(obj)),
                    Some(
                        Class::DefineMacro
                            | Class::DefineMacroSpecialization
                            | Class::DefineStructure
                    )
                ) {
                    self.def_id(obj)?;
                }
                self.remember_name(obj);
                Elem::Op(op)
            }
            Kind::Global => Elem::Global(self.global_id(obj)?),
            Kind::Plain => {
                self.remember_name(obj);
                Elem::Object(obj)
            }
        })
    }

    /// A `global.bld` export, or `None` when no such file is loaded.
    fn export(&mut self, id: u32) -> Result<Option<Elem>, LinkError> {
        let Some((b, g, exports)) = &self.exports else {
            return Ok(None);
        };
        let (b, g) = (*b, *g);
        Ok(Some(match exports.resolve(id) {
            Ok(Export::Bootstrap(Root::Object(o))) => self.local(Obj {
                unit: b,
                index: o as u32,
            })?,
            Ok(Export::Global(Root::Object(o))) => self.local(Obj {
                unit: g,
                index: o as u32,
            })?,
            Ok(Export::Bootstrap(Root::Import(e)) | Export::Global(Root::Import(e))) => {
                if e.namespace == self.global_ns {
                    Elem::Unresolved(self.sym("an export of an export"))
                } else {
                    self.import(e)?
                }
            }
            Err(_) => Elem::Unresolved(self.sym(&format!("global.bld export {id:#x}"))),
        }))
    }

    fn import(&mut self, id: ExternalId) -> Result<Elem, LinkError> {
        if id.namespace == self.global_ns {
            return Ok(match self.export(id.object)? {
                Some(e) => e,
                None => Elem::Unresolved(self.sym(&format!("global.bld export {:#x}", id.object))),
            });
        }
        if id.namespace == self.level_ns {
            return Ok(Elem::Unresolved(
                self.sym(&format!("level.bld import {:#x}", id.object)),
            ));
        }
        Ok(Elem::Member(MemberId {
            namespace: id.namespace,
            name: id.object,
        }))
    }

    fn elem(&mut self, unit: u16, r: ObjectRef<'_>) -> Result<Elem, LinkError> {
        match r {
            ObjectRef::Null => Ok(Elem::Null),
            ObjectRef::Object(i) => self.local(Obj {
                unit,
                index: i as u32,
            }),
            ObjectRef::Import(i) => self.import(i.id),
            ObjectRef::Named { name, .. } => {
                Ok(Elem::Named(self.sym(name.namespace), self.sym(name.name)))
            }
        }
    }

    /// A definition a field names (`_macroDef`, a contents type).
    fn def_ref(&mut self, elem: Elem) -> Option<DefId> {
        match elem {
            Elem::Op(op) => {
                let obj = self.p.op_info[op as usize].obj;
                self.p.defs_by_obj.get(&obj).copied()
            }
            _ => None,
        }
    }

    fn container(&mut self, t: TypeRef<'_>) -> Container {
        match t.metaobject() {
            Some("ValueInfo" | "BoundValue" | "ScriptScreenInfo" | "ScriptColorInfo") => {
                Container::Value
            }
            Some("ScriptReference" | "PlacementReference") => Container::Reference,
            Some("ScriptSet") => Container::Set,
            Some("ScriptSetReference") => Container::SetReference,
            Some("ScriptStructure") => Container::Structure,
            Some(other) => Container::Unknown(self.sym(other)),
            None => Container::Unknown(self.sym("no container class")),
        }
    }

    fn contents(&mut self, unit: u16, t: TypeRef<'_>) -> Result<Contents, LinkError> {
        Ok(match t {
            TypeRef::Null => Contents::None,
            TypeRef::Object(i) => {
                let e = self.local(Obj {
                    unit,
                    index: i as u32,
                })?;
                match self.def_ref(e) {
                    Some(d) => Contents::Def(d),
                    None => Contents::Unresolved,
                }
            }
            TypeRef::Import(i) => {
                let e = self.import(i.id)?;
                match self.def_ref(e) {
                    Some(d) => Contents::Def(d),
                    None => Contents::Unresolved,
                }
            }
            other => match other.metaobject() {
                Some(name) => Contents::Class(self.sym(name)),
                None => Contents::Unresolved,
            },
        })
    }

    fn decl(&mut self, unit: u16, v: &Variable<'_>) -> Result<Decl, LinkError> {
        Ok(Decl {
            container: self.container(v.container_type),
            contents: self.contents(unit, v.contents_type)?,
        })
    }

    fn link_op(&mut self, obj: Obj) -> Result<(), LinkError> {
        let unit = obj.unit;
        let igz = self.igz(unit);
        let read;
        let op = match self.units[unit as usize]
            .graph
            .ops
            .get(&(obj.index as usize))
        {
            Some(op) => op,
            None => {
                read = Op::read(igz, obj.index as usize).map_err(|e| self.fail(unit, e))?;
                &read
            }
        };
        let id = self.p.ops_by_obj[&obj];
        let code = op.code();
        let o = |s: &mut Self, r: ObjectRef<'_>| s.operand_id(unit, r);
        let ins = match op {
            Op::PreScript(_) => Ins::PreScript,
            Op::StartUp(_) => Ins::StartUp,
            Op::ShutDown(_) => Ins::ShutDown,
            Op::MacroSection(_) | Op::StructureSection(_) => Ins::Section,
            Op::FlowContinue(_) => Ins::Continue,
            Op::FlowMacro(_) => Ins::EnterMacro,
            Op::Skip(_) => Ins::Skip,
            Op::FlowLoop(_) => Ins::Loop,
            Op::FlowEnd(_) => Ins::End,
            Op::MacroInterface(_) => Ins::Interface,
            Op::DefineMacro(d) => {
                let def = self.def_id(obj)?;
                let name = code.name.unwrap_or("");
                let sym = self.sym(name);
                let e = &mut self.p.defs[def as usize];
                e.kind = DefKind::Macro;
                e.name = sym;
                e.display = name.to_owned();
                e.publicity = d.publicity;
                Ins::Define(def)
            }
            Op::DefineMacroSpecialization(d) => {
                let def = self.def_id(obj)?;
                let name = code.name.unwrap_or("");
                let sym = self.sym(name);
                let base = match d.np {
                    Some(np) => o(self, ObjectRef::Object(np))?,
                    None => None,
                };
                let e = &mut self.p.defs[def as usize];
                e.kind = DefKind::Specialization;
                e.name = sym;
                e.display = name.to_owned();
                e.publicity = d.def.publicity;
                e.base_operand = base;
                Ins::Define(def)
            }
            Op::DefineStructure(d) => {
                let def = self.def_id(obj)?;
                let name = code.name.unwrap_or("");
                let sym = self.sym(name);
                let e = &mut self.p.defs[def as usize];
                e.kind = DefKind::Structure;
                e.name = sym;
                e.display = name.to_owned();
                e.publicity = d.publicity;
                Ins::Define(def)
            }
            Op::UserBehavior(_) => Ins::Behaviour {
                name: self.sym(code.name.unwrap_or("")),
            },
            Op::CreateVariable(v) => Ins::Variable {
                name: self.sym(v.name.unwrap_or("")),
                decl: self.decl(unit, v)?,
            },
            Op::MacroParameter(p) => {
                let target = self.elem(unit, p.macro_def)?;
                Ins::Parameter(Param {
                    name: self.sym(p.var.name.unwrap_or("")),
                    id: p.parameter_id,
                    combine: p.combine_op,
                    rhs: o(self, p.rhs)?,
                    index: o(self, p.index_rhs)?,
                    def: self.def_ref(target),
                    decl: self.decl(unit, &p.var)?,
                })
            }
            Op::UseMacro(u) => Ins::UseMacro {
                np: match u.np {
                    Some(np) => o(self, ObjectRef::Object(np))?,
                    None => None,
                },
                index: o(self, u.index_rhs)?,
            },
            Op::SetValue(s) => Ins::SetValue {
                lhs: o(self, s.lhs)?,
                rhs: o(self, s.rhs)?,
            },
            Op::SetReference(s) => Ins::SetReference {
                lhs: o(self, s.lhs)?,
                rhs: o(self, s.rhs)?,
                index: o(self, s.index_rhs)?,
            },
            Op::IncValue(s) => Ins::Inc {
                lhs: o(self, s.lhs)?,
            },
            Op::DecValue(s) => Ins::Dec {
                lhs: o(self, s.lhs)?,
            },
            Op::Reset(s) => Ins::Reset { np: o(self, s.np)? },
            Op::CheckValue(c) => Ins::CheckValue {
                lhs: o(self, c.lhs)?,
                relation: c.relation,
                rhs: o(self, c.rhs)?,
            },
            Op::CheckReference(c) => Ins::CheckReference {
                lhs: o(self, c.lhs)?,
                relation: c.relation,
                rhs: o(self, c.rhs)?,
                index: o(self, c.index_rhs)?,
            },
            Op::CheckMembership(c) => Ins::CheckMembership {
                lhs: o(self, c.lhs)?,
                code: c.membership_op,
                rhs: o(self, c.rhs)?,
            },
            Op::ChangeMembership(c) => Ins::ChangeMembership {
                lhs: o(self, c.lhs)?,
                code: c.combine_op,
                rhs: o(self, c.rhs)?,
            },
            Op::LoopValue(l) => Ins::LoopValue {
                rhs: o(self, l.rhs)?,
            },
            Op::ForEach(f) => Ins::ForEach {
                lhs: o(self, f.lhs)?,
                dir: f.dir,
                rhs: o(self, f.rhs)?,
            },
            Op::FindSubSet(f) => Ins::FindSubSet {
                lhs: o(self, f.lhs)?,
                relation: f.relation,
                rhs: o(self, f.rhs)?,
            },
            Op::SetBehavior(s) => Ins::SetBehavior {
                np: o(self, s.np)?,
                index: o(self, s.index_rhs)?,
            },
            Op::Spawn(s) => Ins::World {
                operands: vec![
                    ("_LHS", o(self, s.control.lhs)?),
                    ("_RHS", o(self, s.rhs)?),
                    ("_facingRHS", o(self, s.facing_rhs)?),
                ],
                words: vec![],
            },
            Op::Control(c) => Ins::World {
                operands: vec![
                    ("_LHS", o(self, c.control.lhs)?),
                    ("_indexRHS", o(self, c.index_rhs)?),
                ],
                words: vec![],
            },
            Op::Remove(r) => Ins::World {
                operands: vec![("_NP", o(self, r.np)?)],
                words: vec![],
            },
            Op::StopSequence(r) => Ins::World {
                operands: vec![("_NP", o(self, r.np)?)],
                words: vec![],
            },
            Op::SlideValue(s) => Ins::World {
                operands: vec![
                    ("_LHS", o(self, s.lhs)?),
                    ("_RHS", o(self, s.rhs)?),
                    ("_secondsRHS", o(self, s.seconds_rhs)?),
                    ("_easeOutRHS", o(self, s.ease_out_rhs)?),
                    ("_easeInRHS", o(self, s.ease_in_rhs)?),
                ],
                words: vec![],
            },
            Op::Displace(d) => Ins::World {
                operands: vec![
                    ("_NP", o(self, d.np)?),
                    ("_lenNP", o(self, d.len_np)?),
                    ("_headNP", o(self, d.head_np)?),
                    ("_pitchNP", o(self, d.pitch_np)?),
                ],
                words: vec![("_combineMode", d.combine_mode)],
            },
            Op::StartSequence(s) => Ins::World {
                operands: vec![
                    ("_LHS", o(self, s.lhs)?),
                    ("_indexRHS", o(self, s.index_rhs)?),
                ],
                words: vec![],
            },
            Op::FindVariable(f) => Ins::FindVariable {
                name: self.sym(f.name.unwrap_or("")),
                decl: Decl {
                    container: self.container(f.container_type),
                    contents: self.contents(unit, f.contents_type)?,
                },
                filter: f.filter,
                owner: o(self, f.owner)?,
                index: o(self, f.index_rhs)?,
                skip: o(self, f.skip_rhs)?,
            },
            Op::CheckFov(c) => Ins::World {
                operands: vec![
                    ("_RHSfacing", o(self, c.rhs_facing)?),
                    ("_RHSfov", o(self, c.rhs_fov)?),
                    ("_fromLHS", o(self, c.from_lhs)?),
                    ("_LHS", o(self, c.lhs)?),
                    ("_RHS", o(self, c.rhs)?),
                ],
                words: vec![("_mode", c.mode), ("_relOperator", c.relation)],
            },
            Op::TurnTo(t) => Ins::World {
                operands: vec![
                    ("_facingRHS", o(self, t.facing_rhs)?),
                    ("_NP", o(self, t.np)?),
                    ("_indexRHS", o(self, t.index_rhs)?),
                ],
                words: vec![],
            },
            Op::MoveTo(m) | Op::MoveFrom(m) => Ins::World {
                operands: vec![
                    ("_indexRHS", o(self, m.index_rhs)?),
                    ("_LHS", o(self, m.lhs)?),
                    ("_RHS", o(self, m.rhs)?),
                    ("_NP", o(self, m.np)?),
                ],
                words: vec![("_dir", m.dir)],
            },
            Op::TeleportTo(t) => Ins::World {
                operands: vec![
                    ("_LHS", o(self, t.lhs)?),
                    ("_facingRHS", o(self, t.facing_rhs)?),
                ],
                words: vec![("_dir", t.dir)],
            },
        };
        self.p.ops[id as usize] = ins;
        self.p.op_info[id as usize] = OpInfo {
            class: op.class(),
            obj,
            branch: code.branch,
        };
        Ok(())
    }

    fn link_operand(&mut self, obj: Obj) -> Result<(), LinkError> {
        let unit = obj.unit;
        let igz = self.igz(unit);
        let read;
        let raw = match self.units[unit as usize]
            .graph
            .operands
            .get(&(obj.index as usize))
        {
            Some(r) => r,
            None => {
                read = Raw::read(igz, obj.index as usize).map_err(|e| self.fail(unit, e))?;
                &read
            }
        };
        let id = self.operands_by_obj[&obj];
        let operand = match raw {
            Raw::Expression(e) => Operand::Expr {
                a: self.operand_id(unit, e.op1)?,
                op: (e.operator != NO_OPERATOR).then_some(e.operator),
                b: self.operand_id(unit, e.op2)?,
            },
            Raw::Stack(path) => {
                let mut elems = Vec::with_capacity(path.elements.len());
                for &e in &path.elements {
                    elems.push(self.elem(unit, e)?);
                }
                let its = crate::host::Builtin::Its.id();
                let names_its = elems.contains(&Elem::Member(its));
                let literal = path.rhs.map(|r| Literal {
                    class: match r.class.metaobject() {
                        Some("FloatMeasurement") => LiteralClass::Float,
                        Some("ColorMeasurement") => LiteralClass::Color,
                        Some("ScreenMeasurement") => LiteralClass::Screen,
                        _ => LiteralClass::Int,
                    },
                    bits: r.bits as u32,
                    random: r.is_random,
                });
                Operand::Path {
                    class: path.class,
                    elems,
                    other: self.operand_id(unit, path.other_point)?,
                    literal,
                    names_its,
                }
            }
        };
        self.p.operands[id as usize] = operand;
        Ok(())
    }

    fn link_global(&mut self, obj: Obj) -> Result<(), LinkError> {
        let unit = obj.unit;
        let igz = self.igz(unit);
        let i = obj.index as usize;
        let fail = |s: &Self, e| s.fail(unit, e);
        let g = match self.type_name(obj) {
            "ValueInfo" => {
                let v = ValueInfo::read(igz, i).map_err(|e| fail(self, e))?;
                Global::Number(match v.value_type.metaobject() {
                    Some("FloatMeasurement") => Value::Float(f32::from_bits(v.value as u32)),
                    _ => Value::Int(v.value),
                })
            }
            "ScriptSet" => {
                let s = ScriptSet::read(igz, i).map_err(|e| fail(self, e))?;
                match s.list {
                    Some(l) => {
                        let list = ScriptSetList::read(igz, l).map_err(|e| fail(self, e))?;
                        let mut members = Vec::with_capacity(list.elements.len());
                        for &e in &list.elements {
                            members.push(self.elem(unit, e)?);
                        }
                        Global::Set {
                            members,
                            elem: self.contents(unit, list.edit_type)?,
                        }
                    }
                    None => Global::Set {
                        members: Vec::new(),
                        elem: Contents::None,
                    },
                }
            }
            ty @ ("ScriptScreenInfo" | "ScriptColorInfo") => {
                let o = igz.objects()[i];
                let word = match o.field(PACKED_VALUE) {
                    Some(at) => igz.u32_at(at, "_value").map_err(|e| fail(self, e))?,
                    None => 0,
                };
                Global::Number(if ty == "ScriptScreenInfo" {
                    Value::Screen(word)
                } else {
                    Value::Color(word)
                })
            }
            "ScriptStructure" => {
                let s = ScriptStructure::read(igz, i).map_err(|e| fail(self, e))?;
                let def = match self.contents(unit, s.contents_type)? {
                    Contents::Def(d) => Some(d),
                    _ => None,
                };
                Global::Structure { def }
            }
            _ => {
                let o = igz.objects()[i];
                let target = match o
                    .field(REFERENCE_TARGET)
                    .map(|at| igz.reference(at, "_obj"))
                {
                    Some(Ok(Ref::Object(t))) => self.local(Obj {
                        unit,
                        index: t as u32,
                    })?,
                    Some(Ok(Ref::Import { id, .. })) => self.import(id)?,
                    Some(Ok(Ref::MetaObject { name, .. })) => {
                        Elem::Named(self.sym(name.namespace), self.sym(name.name))
                    }
                    _ => Elem::Null,
                };
                Global::Reference(target)
            }
        };
        let gid = self.p.globals_by_obj[&obj];
        self.p.globals[gid as usize].1 = g;
        Ok(())
    }

    /// Placement, frames' ends, structure fields, bases and the name tables.
    fn finish(&mut self) {
        let p = &mut self.p;
        for (sid, s) in p.scripts.iter_mut().enumerate() {
            for (pos, &op) in s.ops.iter().enumerate() {
                match &p.ops[op as usize] {
                    Ins::Define(d) => {
                        let def = &mut p.defs[*d as usize];
                        if def.placed.is_none() {
                            def.placed = Some((sid as ScriptId, pos as u32));
                        }
                    }
                    Ins::Behaviour { name } => {
                        s.behaviours.entry(*name).or_insert(pos as u32);
                    }
                    Ins::ShutDown if s.shutdown.is_none() => s.shutdown = Some(pos as u32),
                    _ => {}
                }
            }
            if let Some(&first) = s.ops.first()
                && p.ops[first as usize] == Ins::PreScript
            {
                let after = 1 + p.op_info[first as usize].branch as usize;
                if let Some(&b) = s.ops.get(after)
                    && matches!(p.ops[b as usize], Ins::Behaviour { .. })
                {
                    s.first_behaviour = Some(after as u32);
                }
            }
        }
        for d in 0..p.defs.len() {
            let def = &p.defs[d];
            if def.kind == DefKind::Structure
                && let Some((sid, pos)) = def.placed
            {
                let n = p.op_info[def.op as usize].branch as usize;
                let ops = &p.scripts[sid as usize].ops;
                let fields: Vec<OpId> = (pos as usize + 1..=pos as usize + n)
                    .filter_map(|q| ops.get(q).copied())
                    .filter(|&q| matches!(p.ops[q as usize], Ins::Variable { .. }))
                    .collect();
                p.defs[d].fields = fields;
            }
            if let Some(o) = p.defs[d].base_operand
                && let Some(Operand::Path { elems, .. }) = p.operands.get(o as usize)
                && let Some(Elem::Op(b)) = elems.first()
            {
                let obj = p.op_info[*b as usize].obj;
                p.defs[d].base = p.defs_by_obj.get(&obj).copied();
            }
            let def = &p.defs[d];
            if def.placed.is_some() {
                let table = match def.kind {
                    DefKind::Structure => &mut p.structures,
                    _ => &mut p.macros,
                };
                table.entry(def.name).or_default().push(d as DefId);
            }
        }
        let pick = |p: &Program, list: Option<&Vec<DefId>>| -> Option<DefId> {
            let list = list?;
            list.iter()
                .copied()
                .find(|&d| p.defs[d as usize].publicity == 1)
                .or_else(|| list.first().copied())
        };
        let resolved: Vec<Option<DefId>> = (0..p.defs.len())
            .map(|d| {
                let def = &p.defs[d];
                if def.placed.is_some() {
                    return Some(d as DefId);
                }
                let table = match def.kind {
                    DefKind::Structure => &p.structures,
                    _ => &p.macros,
                };
                pick(p, table.get(&def.name))
            })
            .collect();
        p.resolved = resolved;
    }
}
