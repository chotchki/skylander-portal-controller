//! What a fresh figure carries (openspec AE.23): the placements a
//! character's script hangs on it as the figure lands, with every upgrade
//! unbought. Stealth Elf's two knives, Gill Grunt's harpoon gun with its
//! spear and his backpack, Tree Rex's rocket arm.
//!
//! The rule, read off every character archive's scripts and checked against
//! the oracle's sweep of every figure (AE.22): the hero ([`hero_actor`])
//! runs its script's STARTUP block once when it lands. Each attachment is a
//! clone made there whose block sets the clone's `attach node` to an entry
//! of the hero's `attach nodes` (its model's attach list, by position) or of
//! an earlier clone's. Upgrades pick other templates (Stealth Elf's
//! `026_KnifeUpgrade_reg2`) or attach more, under checks on what "Minion -
//! Get Upgrade Details" reports, so stepping STARTUP with every upgrade
//! unbought gives the fresh figure's set. No sprite field or attach point
//! tells them apart (AE.5); the script does.
//!
//! [`fresh`] steps it over a static environment, the narrow rewrite AE.2's
//! toy table and AH.2's save-param table make (AH's VM must reproduce it):
//! - every other script of the archive first, its STARTUP and then its top
//!   level, keeping what each writes: a Def script sets the references the
//!   hero's clones come from (`026_knife(1)`, `018_Default_Wizard_Staff`);
//!   two scripts writing one cell differently leave it unsure;
//! - `set value` and `set reference` write a cell (a variable, a member of
//!   one, a file-level reference or value) or, on a path ending in `attach
//!   node`, hang the clone the path starts with;
//! - `check value` and `check reference` compare with ah-progression's codes
//!   ([`crate::progression::RELATIONS`]); an else-if naming an earlier check
//!   tests that check's left side again;
//! - `clone` makes a clone of its template and runs its block once with it;
//!   `loop value` runs its block the count's times, the index bound;
//! - `use macro` doesn't run the macro: [`UPGRADE_DETAILS`] reports every
//!   upgrade unbought, [`TAG_READY`] reports the tag ready, and any other
//!   macro's parameters read as their arguments or whatever the macro wrote,
//!   which is unsure;
//! - a variable nothing wrote reads 0 or null, as a new one does; a
//!   `ValueInfo` reads its stored value;
//! - `flow end` ends the script;
//! - any other block (a sequence's, a behaviour's, a find's, a tell's, a
//!   motion's, a for-each's) runs on an event or over a set the stepper
//!   can't list, so it's stepped as unsure.
//!
//! What it can't tell (a check on a value it can't read, a loop of unknown
//! count, those blocks) it steps as unsure: a clone made or hung there, or
//! from a template written there, is [`Clone::unsure`], and why is in
//! [`Fresh::unsure`]. The caller decides what an unsure clone means for a
//! model ([`hung`]).

use std::collections::HashMap;

use crate::igz::layouts::{
    ActorInfo, BoneAttachInfo, BoneAttachInfoList, Layout, ModelInfo, ObjectRef, OpCode,
    OpCodeList, OpLoopValue, OpMacroParameter, OpUseMacro, RhsValueStack, ScriptInfo, Stack,
    TypeRef, ValueInfo, ValueRhsVariant, Variable, is_a,
};
use crate::igz::{ExternalId, Igz};
use crate::level::Actor;
use crate::script::ops::{
    OpCheckReference, OpCheckValue, OpDecValue, OpIncValue, OpSetReference, OpSetValue, OpSpawn,
};
use crate::{ErrorKind, FormatError, fnv1a32};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

pub use crate::model::hero_actor;

const FORMAT: &str = "attachments";

/// The macro whose parameters say which upgrades a figure has bought.
pub const UPGRADE_DETAILS: &str = "Minion - Get Upgrade Details";
/// The parameters of it a fresh figure reads as 0: every one whose name
/// says `purchased` (`upgrade 1 purchased` to `wow pow purchased`).
pub const PURCHASED: &str = "purchased";
/// The macro a Def script returns early on until the tag is ready.
pub const TAG_READY: &str = "Portal - Toy Def Check if Tag is Ready";
/// `optoplevelbehavior.my`: the placement a script runs on.
pub const MY: (&str, u32) = ("optoplevelbehavior", 0x4C2D_FE93);
/// The member a placement hangs by, and the list a placement's attach
/// points read as.
pub const ATTACH_NODE: &str = "attach node";
pub const ATTACH_NODES: (&str, &str) = ("actorinfo", "attach nodes");

/// How many ops one archive's stepping may take, and the most passes a loop
/// may make: far past the disc's (a hero's STARTUP is at most a few hundred
/// ops, its loops at most 4 passes), so a file looping on itself stops.
pub const MAX_STEPS: usize = 1_000_000;
pub const MAX_PASSES: i64 = 256;
/// The most clones one script may make, the most alternatives an unsure
/// value keeps (past it, it's anything) and the most reasons kept: the
/// disc's heroes make a few dozen clones at most, and the bounds keep a file
/// that loops on itself from growing them.
pub const MAX_CLONES: usize = 1024;
pub const MAX_ALTERNATIVES: usize = 16;
pub const MAX_NOTES: usize = 256;
/// How deep blocks may nest as the stepper enters them: the disc's go a
/// dozen deep, and a deeper file is refused before the stack is.
pub const MAX_DEPTH: usize = 64;

/// A clone's template: a placement of the archive or a `global.bld` export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// A placement of the archive (`026_Knife_reg1`), by object index.
    Local(usize),
    /// A `global.bld` export (`Generic_Dummy`, `Dummy_Inviso`).
    Global(ExternalId),
}

/// The placement a clone hangs from: the hero, or an earlier clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Hero,
    /// Another clone, an index into [`Fresh::clones`].
    Clone(usize),
}

/// Where a clone hangs: an entry of its owner's attach list, or the owner
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub owner: Owner,
    /// The entry of the owner's attach list; `None` hangs it on the owner
    /// itself.
    pub slot: Option<usize>,
}

/// One clone the hero's STARTUP makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clone {
    /// The clone op's position in the hero's script.
    pub at: usize,
    /// `None` where the stepper couldn't tell.
    pub template: Option<Template>,
    /// `None` when nothing hung it, or the stepper couldn't tell where.
    pub node: Option<Node>,
    /// Made, given its template or hung where the stepper couldn't tell
    /// whether a fresh figure gets there.
    pub unsure: bool,
}

/// What the hero's STARTUP makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    /// The hero's script, by object index.
    pub script: usize,
    pub clones: Vec<Clone>,
    /// What the stepper couldn't tell in the hero's script: the position and
    /// why.
    pub unsure: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Object(usize),
    Import(u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ref {
    Null,
    Hero,
    /// Another actor's `my`, while stepping its script.
    Other,
    Placement(usize),
    Import(ExternalId),
    Clone(usize),
    Node(Node),
    /// A structure's instance (`clone` of a `ScriptStructure` makes one): a
    /// record, not a placement.
    Structure,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Int(i64),
    Num(f64),
    Ref(Ref),
    /// One of these, or, when empty, anything.
    Unsure(Vec<Value>),
}

impl Value {
    fn unknown() -> Self {
        Value::Unsure(Vec::new())
    }

    fn number(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
}

/// A cell's value and the script that wrote it.
#[derive(Debug, Clone, PartialEq)]
struct Cell {
    value: Value,
    script: usize,
}

struct Script {
    object: usize,
    ops: Vec<usize>,
}

struct Stepper<'s, 'a> {
    igz: &'s Igz<'a>,
    cells: HashMap<Vec<Key>, Cell>,
    /// What a clone op, a loop or a macro parameter stands for in a block.
    bound: HashMap<usize, Value>,
    clones: Vec<Clone>,
    /// How many unsure blocks enclose the step.
    unsure: usize,
    notes: Vec<(usize, String)>,
    steps: usize,
    depth: usize,
    /// Variables passed to a macro the stepper doesn't run, whose members
    /// that macro may have written.
    passed: std::collections::HashSet<Key>,
    /// Stepping the hero's own script.
    hero: bool,
    script: usize,
}

fn refuse(igz: &Igz<'_>, object: usize, field: &'static str, kind: ErrorKind) -> FormatError {
    match igz.objects().get(object) {
        Some(o) => igz.err_at(o.at, field, kind),
        None => FormatError::new(FORMAT, field, 0, kind),
    }
}

fn import_is(r: &ObjectRef<'_>, namespace: &str, member: &str) -> bool {
    matches!(r, ObjectRef::Import(i)
        if i.id.namespace == fnv1a32(namespace.as_bytes()) && i.id.object == fnv1a32(member.as_bytes()))
}

fn is_my(r: &ObjectRef<'_>) -> bool {
    matches!(r, ObjectRef::Import(i)
        if i.id.namespace == fnv1a32(MY.0.as_bytes()) && i.id.object == MY.1)
}

/// The attach member of a placement or a sprite (`abstractplacement.attach
/// node`, `spritematrixmeasurement.attach node`).
fn is_attach_node(r: &ObjectRef<'_>) -> bool {
    matches!(r, ObjectRef::Import(i) if i.id.object == fnv1a32(ATTACH_NODE.as_bytes()))
}

/// The types a path's leading cell can be.
const CELLS: [&str; 7] = [
    "OpCreateVariable",
    "PlacementReference",
    "ScriptReference",
    "ScriptSetReference",
    "AttachNodeReference",
    "ValueInfo",
    "OpMacroParameter",
];

/// A structure, which `clone` instantiates as a record, and the op that
/// defines one.
const STRUCTURE: &str = "ScriptStructure";
const DEFINE_STRUCTURE: &str = "OpDefineStructure";

/// What a placement can be as a clone's template.
const PLACEMENTS: [&str; 4] = [
    "tfbActorInfo",
    "tfbParticleInfo",
    "tfbLightInfo",
    "tfbSpriteInfo",
];

impl<'s, 'a> Stepper<'s, 'a> {
    fn ty(&self, o: usize) -> &'a str {
        self.igz
            .objects()
            .get(o)
            .map_or("", |obj| self.igz.type_name(obj))
    }

    fn note(&mut self, at: usize, why: String) {
        if self.hero && self.notes.len() < MAX_NOTES {
            self.notes.push((at, why));
        }
    }

    fn literal(class: &TypeRef<'_>, bits: i32) -> Value {
        let name = match class {
            TypeRef::Indexed { name: Some(n), .. } | TypeRef::Named { name: n, .. } => n.name,
            _ => "",
        };
        match name {
            "FloatMeasurement" => Value::Num(f64::from(f32::from_bits(bits as u32))),
            "IntMeasurement" | "BoolMeasurement" | "EnumMeasurement" => Value::Int(bits.into()),
            _ => Value::unknown(),
        }
    }

    fn eval_ref(&mut self, r: &ObjectRef<'a>, index: Option<&Value>) -> Result<Value, FormatError> {
        Ok(match r {
            ObjectRef::Null => Value::Ref(Ref::Null),
            ObjectRef::Object(o) => self.eval(*o, index)?,
            ObjectRef::Import(i) if is_my(r) => {
                let _ = i;
                Value::Ref(if self.hero { Ref::Hero } else { Ref::Other })
            }
            ObjectRef::Import(i) => Value::Ref(Ref::Import(i.id)),
            ObjectRef::Named { .. } => Value::unknown(),
        })
    }

    /// An operand (a stack or an expression), or an object a path names.
    fn eval(&mut self, o: usize, index: Option<&Value>) -> Result<Value, FormatError> {
        let ty = self.ty(o);
        if ty == ValueRhsVariant::TYPE {
            let e = ValueRhsVariant::read(self.igz, o)?;
            let a = self.eval_ref(&e.op1, None)?;
            if e.op2 == ObjectRef::Null {
                return Ok(a);
            }
            let b = self.eval_ref(&e.op2, None)?;
            let (Some(x), Some(y)) = (a.number(), b.number()) else {
                return Ok(Value::unknown());
            };
            let r = match e.operator {
                0 => x + y,
                1 => x - y,
                2 => x * y,
                3 if y != 0.0 => x / y,
                _ => return Ok(Value::unknown()),
            };
            return Ok(match (a, b) {
                (Value::Int(_), Value::Int(_)) if e.operator != 3 => Value::Int(r as i64),
                _ => Value::Num(r),
            });
        }
        if is_a(ty, RhsValueStack::TYPE) {
            let s = RhsValueStack::read(self.igz, o)?;
            if s.stack.path.is_empty() {
                if s.is_random {
                    return Ok(Value::unknown());
                }
                return Ok(Self::literal(&s.value_type, s.value));
            }
            return self.path(&s.stack.path, index);
        }
        if is_a(ty, Stack::TYPE) {
            let s = Stack::read(self.igz, o)?;
            return self.path(&s.path, index);
        }
        self.object(o)
    }

    /// An object standing first on a path.
    fn object(&mut self, o: usize) -> Result<Value, FormatError> {
        if let Some(v) = self.bound.get(&o) {
            return Ok(v.clone());
        }
        let ty = self.ty(o);
        if PLACEMENTS.iter().any(|p| is_a(ty, p)) {
            return Ok(Value::Ref(Ref::Placement(o)));
        }
        if is_a(ty, STRUCTURE) {
            return Ok(Value::Ref(Ref::Structure));
        }
        if let Some(c) = self.cells.get(&vec![Key::Object(o)]) {
            return Ok(c.value.clone());
        }
        Ok(match ty {
            "ValueInfo" => {
                let v = ValueInfo::read(self.igz, o)?;
                Self::literal(&v.value_type, v.value)
            }
            // An else-if names the check whose left side it tests again.
            t if t == OpCheckValue::TYPE => {
                let c = OpCheckValue::read(self.igz, o)?;
                self.eval_ref(&c.lhs, None)?
            }
            t if t == OpCheckReference::TYPE => {
                let c = OpCheckReference::read(self.igz, o)?;
                self.eval_ref(&c.lhs, None)?
            }
            // A variable nothing wrote reads 0 or null, unless a macro the
            // stepper doesn't run was given it.
            "OpCreateVariable" if !self.passed.contains(&Key::Object(o)) => Value::Int(0),
            _ => Value::unknown(),
        })
    }

    fn key(e: &ObjectRef<'_>) -> Option<Key> {
        match e {
            ObjectRef::Object(o) => Some(Key::Object(*o)),
            ObjectRef::Import(i) => Some(Key::Import(i.id.namespace, i.id.object)),
            _ => None,
        }
    }

    fn leading_cells(&self, elements: &[ObjectRef<'a>]) -> usize {
        elements
            .iter()
            .take_while(|e| matches!(e, ObjectRef::Object(o) if CELLS.contains(&self.ty(*o))))
            .count()
    }

    fn path(
        &mut self,
        elements: &[ObjectRef<'a>],
        index: Option<&Value>,
    ) -> Result<Value, FormatError> {
        let Some(first) = elements.first() else {
            return Ok(Value::Ref(Ref::Null));
        };
        let n = self.leading_cells(elements);
        let mut v = if n > 1 {
            let key: Vec<Key> = elements[..n].iter().filter_map(Self::key).collect();
            match self.cells.get(&key) {
                Some(c) => c.value.clone(),
                // A member of a variable a macro was given: the macro may
                // have written it.
                None if self.passed.contains(&key[0]) => Value::unknown(),
                // A member nothing wrote: 0 or null, as a new variable's.
                None => Value::Int(0),
            }
        } else {
            self.eval_ref(first, None)?
        };
        let rest = &elements[n.max(1)..];
        for (k, e) in rest.iter().enumerate() {
            let last = k + 1 == rest.len();
            v = if import_is(e, ATTACH_NODES.0, ATTACH_NODES.1) && last {
                let owner = match v {
                    Value::Ref(Ref::Hero) => Some(Owner::Hero),
                    Value::Ref(Ref::Clone(c)) => Some(Owner::Clone(c)),
                    _ => None,
                };
                match (owner, index) {
                    (Some(owner), Some(Value::Int(slot))) if *slot >= 0 => {
                        Value::Ref(Ref::Node(Node {
                            owner,
                            slot: Some(*slot as usize),
                        }))
                    }
                    _ => Value::unknown(),
                }
            } else {
                Value::unknown()
            };
        }
        Ok(v)
    }

    /// `value`, or, inside an unsure block, the unsure choice between it and
    /// `before`.
    fn guarded(&self, before: Option<Value>, value: Value) -> Value {
        if self.unsure == 0 {
            return value;
        }
        let mut alternatives = match before {
            Some(Value::Unsure(v)) if !v.is_empty() => v,
            Some(Value::Unsure(_)) | None => return Value::unknown(),
            Some(b) => vec![b],
        };
        alternatives.push(value);
        if alternatives.len() > MAX_ALTERNATIVES {
            return Value::unknown();
        }
        Value::Unsure(alternatives)
    }

    fn write(&mut self, at: usize, lhs: &ObjectRef<'a>, value: Value) -> Result<(), FormatError> {
        let ObjectRef::Object(o) = lhs else {
            return Ok(());
        };
        if !is_a(self.ty(*o), Stack::TYPE) {
            return Err(refuse(
                self.igz,
                *o,
                "_LHS",
                ErrorKind::WrongType {
                    expected: "a tfbScript stack",
                    found: self.ty(*o).to_owned(),
                },
            ));
        }
        let elements = Stack::read(self.igz, *o)?.path;
        if elements.last().is_some_and(is_attach_node) {
            let owner = self.path(&elements[..elements.len() - 1], None)?;
            return self.hang(at, owner, value);
        }
        let Some(key) = elements.iter().map(Self::key).collect::<Option<Vec<Key>>>() else {
            return Ok(());
        };
        let before = self.cells.get(&key).cloned();
        let mut value = self.guarded(before.as_ref().map(|c| c.value.clone()), value);
        // Two other scripts writing one cell differently: unsure which the
        // hero reads, as the order they run in isn't known.
        if let Some(b) = &before
            && !self.hero
            && b.script != self.script
            && self.unsure == 0
            && b.value != value
        {
            value = Value::Unsure(vec![b.value.clone(), value]);
        }
        self.cells.insert(
            key,
            Cell {
                value,
                script: self.script,
            },
        );
        Ok(())
    }

    /// Hangs the clone `owner` stands for on `node`.
    fn hang(&mut self, at: usize, owner: Value, node: Value) -> Result<(), FormatError> {
        let target = match node {
            Value::Ref(Ref::Node(n)) => Some(n),
            Value::Ref(Ref::Hero) => Some(Node {
                owner: Owner::Hero,
                slot: None,
            }),
            Value::Ref(Ref::Clone(c)) => Some(Node {
                owner: Owner::Clone(c),
                slot: None,
            }),
            Value::Ref(Ref::Null) => None,
            ref other => {
                if let Value::Ref(Ref::Clone(c)) = owner {
                    self.clones[c].unsure = true;
                    self.clones[c].node = None;
                    self.note(at, format!("clone {c} hung on {other:?}"));
                }
                return Ok(());
            }
        };
        match owner {
            Value::Ref(Ref::Clone(c)) => {
                self.clones[c].node = target;
                if self.unsure > 0 {
                    self.clones[c].unsure = true;
                }
            }
            Value::Unsure(alternatives) => {
                for a in alternatives {
                    if let Value::Ref(Ref::Clone(c)) = a {
                        self.clones[c].unsure = true;
                    }
                }
                self.note(at, "an unsure clone hung".to_owned());
            }
            _ => {}
        }
        Ok(())
    }

    fn holds(code: u32, a: &Value, b: &Value) -> Option<bool> {
        if let (Some(x), Some(y)) = (a.number(), b.number()) {
            return Some(match code {
                0 => x <= y,
                1 => x == y,
                2 => x >= y,
                3 => x < y,
                4 => x > y,
                5 => x != y,
                _ => return None,
            });
        }
        match (a, b) {
            (Value::Ref(x), Value::Ref(y)) => match code {
                1 => Some(x == y),
                5 => Some(x != y),
                _ => None,
            },
            _ => None,
        }
    }

    fn code(&self, op: usize) -> Result<OpCode<'a>, FormatError> {
        OpCode::read(self.igz, op)
    }

    /// The last position of the block the op at `pc` opens.
    fn block_end(&self, s: &Script, pc: usize) -> Result<usize, FormatError> {
        let op = s.ops[pc];
        let code = self.code(op)?;
        let end = code.block_end(pc).unwrap_or(pc);
        if end >= s.ops.len() {
            return Err(refuse(
                self.igz,
                op,
                "_branchPC",
                ErrorKind::OutOfRange {
                    value: end as u64,
                    limit: s.ops.len() as u64,
                },
            ));
        }
        Ok(end)
    }

    fn deeper(
        &mut self,
        s: &Script,
        from: usize,
        to: usize,
        unsure: bool,
    ) -> Result<bool, FormatError> {
        if self.depth >= MAX_DEPTH {
            return Err(refuse(
                self.igz,
                s.ops[from.min(s.ops.len() - 1)],
                "block",
                ErrorKind::OutOfRange {
                    value: self.depth as u64,
                    limit: MAX_DEPTH as u64,
                },
            ));
        }
        self.depth += 1;
        self.unsure += usize::from(unsure);
        let r = self.step(s, from, to);
        self.unsure -= usize::from(unsure);
        self.depth -= 1;
        r
    }

    /// Steps `s.ops[from..=to]`; `Ok(false)` when a `flow end` ends it.
    fn step(&mut self, s: &Script, from: usize, to: usize) -> Result<bool, FormatError> {
        let mut pc = from;
        while pc <= to && pc < s.ops.len() {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return Err(refuse(
                    self.igz,
                    s.ops[pc],
                    "op",
                    ErrorKind::OutOfRange {
                        value: self.steps as u64,
                        limit: MAX_STEPS as u64,
                    },
                ));
            }
            let op = s.ops[pc];
            let ty = self.ty(op);
            let code = self.code(op)?;
            let end = self.block_end(s, pc)?;
            pc = match ty {
                t if t == OpSetValue::TYPE => {
                    let set = OpSetValue::read(self.igz, op)?;
                    let v = self.eval_ref(&set.rhs, None)?;
                    self.write(pc, &set.lhs, v)?;
                    pc + 1
                }
                t if t == OpSetReference::TYPE => {
                    let set = OpSetReference::read(self.igz, op)?;
                    let index = match set.index_rhs {
                        ObjectRef::Null => None,
                        r => Some(self.eval_ref(&r, None)?),
                    };
                    let v = self.eval_ref(&set.rhs, index.as_ref())?;
                    self.write(pc, &set.lhs, v)?;
                    pc + 1
                }
                t if t == OpIncValue::TYPE || t == OpDecValue::TYPE => {
                    let (lhs, by) = if t == OpIncValue::TYPE {
                        (OpIncValue::read(self.igz, op)?.lhs, 1)
                    } else {
                        (OpDecValue::read(self.igz, op)?.lhs, -1)
                    };
                    let v = match self.eval_ref(&lhs, None)? {
                        Value::Int(i) => Value::Int(i + by),
                        Value::Num(n) => Value::Num(n + by as f64),
                        _ => Value::unknown(),
                    };
                    self.write(pc, &lhs, v)?;
                    pc + 1
                }
                t if t == OpCheckValue::TYPE || t == OpCheckReference::TYPE => {
                    let (lhs, relation, rhs, index) = if t == OpCheckValue::TYPE {
                        let c = OpCheckValue::read(self.igz, op)?;
                        (c.lhs, c.relation, c.rhs, ObjectRef::Null)
                    } else {
                        let c = OpCheckReference::read(self.igz, op)?;
                        (c.lhs, c.relation, c.rhs, c.index_rhs)
                    };
                    let a = self.eval_ref(&lhs, None)?;
                    let index = match index {
                        ObjectRef::Null => None,
                        r => Some(self.eval_ref(&r, None)?),
                    };
                    let b = self.eval_ref(&rhs, index.as_ref())?;
                    match Self::holds(relation, &a, &b) {
                        Some(true) => pc + 1,
                        Some(false) => end + 1,
                        None => {
                            self.note(pc, format!("a check it can't read: {a:?} {relation} {b:?}"));
                            self.unsure_branches(s, pc, end)?
                        }
                    }
                }
                // `else`: the branch taken skips the else block.
                "OpAbstractFlow" => {
                    if code.branch == crate::igz::layouts::BRANCH_END {
                        return Err(refuse(
                            self.igz,
                            op,
                            "_branchPC",
                            ErrorKind::Unsupported {
                                value: code.branch.into(),
                            },
                        ));
                    }
                    end + 1
                }
                "OpFlowBehavior" => {
                    if self.unsure == 0 {
                        return Ok(false);
                    }
                    pc + 1
                }
                "OpFlowBuiltInBehavior" | "OpFlowLoop" | "OpFlowMacro" => pc + 1,
                t if t == OpSpawn::TYPE => {
                    let spawn = OpSpawn::read(self.igz, op)?;
                    let template = self.eval_ref(&spawn.control.lhs, None)?;
                    if template == Value::Ref(Ref::Structure)
                        || self.makes_record(&spawn.control.lhs)?
                    {
                        self.bound.insert(op, template);
                        if !self.deeper(s, pc + 1, end, false)? {
                            return Ok(false);
                        }
                        pc = end + 1;
                        continue;
                    }
                    let mut unsure = self.unsure > 0;
                    let template = match template {
                        Value::Ref(Ref::Placement(p)) => Some(Template::Local(p)),
                        Value::Ref(Ref::Import(i)) => Some(Template::Global(i)),
                        other => {
                            unsure = true;
                            self.note(pc, format!("a clone of {other:?}"));
                            None
                        }
                    };
                    let c = self.clones.len();
                    if c >= MAX_CLONES {
                        return Err(refuse(
                            self.igz,
                            op,
                            "clone",
                            ErrorKind::OutOfRange {
                                value: c as u64,
                                limit: MAX_CLONES as u64,
                            },
                        ));
                    }
                    self.clones.push(Clone {
                        at: pc,
                        template,
                        node: None,
                        unsure,
                    });
                    self.bound.insert(op, Value::Ref(Ref::Clone(c)));
                    if !self.deeper(s, pc + 1, end, false)? {
                        return Ok(false);
                    }
                    end + 1
                }
                t if t == OpLoopValue::TYPE => {
                    let l = OpLoopValue::read(self.igz, op)?;
                    match self.eval_ref(&l.rhs, None)? {
                        Value::Int(n) if (0..=MAX_PASSES).contains(&n) => {
                            for k in 0..n {
                                self.bound.insert(op, Value::Int(k));
                                if !self.deeper(s, pc + 1, end, false)? {
                                    return Ok(false);
                                }
                            }
                        }
                        other => {
                            self.note(pc, format!("a loop of {other:?} passes"));
                            self.bound.insert(op, Value::unknown());
                            self.deeper(s, pc + 1, end, true)?;
                        }
                    }
                    end + 1
                }
                t if t == OpUseMacro::TYPE => {
                    let call = OpUseMacro::read(self.igz, op)?;
                    let name = self.macro_name(&call)?;
                    let interface = pc + 1;
                    let after = if interface <= end {
                        let iend = self.block_end(s, interface)?;
                        for q in interface + 1..=iend.min(end) {
                            let p = s.ops[q];
                            if self.ty(p) == OpMacroParameter::TYPE {
                                let param = OpMacroParameter::read(self.igz, p)?;
                                let v = self.argument(name, &param)?;
                                self.bound.insert(p, v);
                            }
                        }
                        iend + 1
                    } else {
                        end + 1
                    };
                    if after <= end && !self.deeper(s, after, end, false)? {
                        return Ok(false);
                    }
                    end + 1
                }
                // Definitions, and the sections and behaviours run elsewhere.
                "OpMacroInterface"
                | "OpDefineMacro"
                | "OpDefineMacroSpecialization"
                | "OpDefineStructure"
                | "OpMacroSection"
                | "OpStructureSection"
                | "OpStartUp"
                | "OpShutDown"
                | "OpPreScript" => end + 1,
                _ if end > pc => {
                    // A block run on an event or over a set: unsure.
                    let before = self.clones.len();
                    self.deeper(s, pc + 1, end, true)?;
                    if self.clones.len() > before {
                        self.note(pc, format!("clones inside a {ty} block"));
                    }
                    end + 1
                }
                _ => pc + 1,
            };
        }
        Ok(true)
    }

    /// A check the stepper can't read: its block and any else block after
    /// it, each stepped as unsure. Returns where stepping goes on.
    fn unsure_branches(&mut self, s: &Script, pc: usize, end: usize) -> Result<usize, FormatError> {
        let last = s.ops[end];
        let (then_end, else_end) = if end > pc && self.ty(last) == "OpAbstractFlow" {
            let e = self.block_end(s, end)?;
            (end - 1, Some(e))
        } else {
            (end, None)
        };
        if then_end > pc {
            self.deeper(s, pc + 1, then_end, true)?;
        }
        if let Some(e) = else_end {
            if e > end {
                self.deeper(s, end + 1, e, true)?;
            }
            return Ok(e + 1);
        }
        Ok(end + 1)
    }

    /// Whether a clone's template is a structure's record: a structure, or a
    /// variable typed as one (Wham-Shell's projectile description).
    fn makes_record(&self, lhs: &ObjectRef<'a>) -> Result<bool, FormatError> {
        let ObjectRef::Object(o) = lhs else {
            return Ok(false);
        };
        if !is_a(self.ty(*o), Stack::TYPE) {
            return Ok(false);
        }
        let Some(ObjectRef::Object(last)) = Stack::read(self.igz, *o)?.path.last().copied() else {
            return Ok(false);
        };
        let ty = self.ty(last);
        if is_a(ty, STRUCTURE) {
            return Ok(true);
        }
        if !is_a(ty, Variable::TYPE) {
            return Ok(false);
        }
        Ok(match Variable::read(self.igz, last)?.contents_type {
            TypeRef::Object(d) => is_a(self.ty(d), STRUCTURE) || is_a(self.ty(d), DEFINE_STRUCTURE),
            _ => false,
        })
    }

    fn macro_name(&self, call: &OpUseMacro<'a>) -> Result<Option<&'a str>, FormatError> {
        let Some(np) = call.np else {
            return Ok(None);
        };
        let path = Stack::read(self.igz, np)?.path;
        Ok(match path.first() {
            Some(ObjectRef::Object(def)) if is_a(self.ty(*def), "OpDefineMacro") => {
                OpCode::read(self.igz, *def)?.name
            }
            _ => None,
        })
    }

    /// What a call's parameter reads as after the call.
    fn argument(
        &mut self,
        name: Option<&str>,
        p: &OpMacroParameter<'a>,
    ) -> Result<Value, FormatError> {
        let param = p.var.name.unwrap_or_default();
        Ok(match name {
            Some(UPGRADE_DETAILS) if param.contains(PURCHASED) => Value::Int(0),
            Some(UPGRADE_DETAILS) => Value::unknown(),
            Some(TAG_READY) => Value::Int(1),
            _ => {
                if let ObjectRef::Object(rhs) = p.rhs
                    && is_a(self.ty(rhs), Stack::TYPE)
                    && let Some(first) = Stack::read(self.igz, rhs)?.path.first()
                    && let Some(key) = Self::key(first)
                {
                    self.passed.insert(key);
                }
                self.eval_ref(&p.rhs, None)?.or_anything()
            }
        })
    }

    /// The archive's scripts, the hero's by index.
    fn script(&self, object: usize) -> Result<Script, FormatError> {
        let info = ScriptInfo::read(self.igz, object)?;
        let list = info
            .op_list
            .ok_or_else(|| refuse(self.igz, object, "_opList", ErrorKind::Missing))?;
        Ok(Script {
            object,
            ops: OpCodeList::read(self.igz, list)?.ops,
        })
    }

    /// The STARTUP block of a script: the positions inside it.
    fn startup(&self, s: &Script) -> Result<Option<(usize, usize)>, FormatError> {
        for (pc, &op) in s.ops.iter().enumerate() {
            if self.ty(op) == "OpStartUp" {
                let end = self.block_end(s, pc)?;
                return Ok((end > pc).then_some((pc + 1, end)));
            }
        }
        Ok(None)
    }

    /// The script's own top level: inside its PRESCRIPT.
    fn top(&self, s: &Script) -> Result<Option<(usize, usize)>, FormatError> {
        match s.ops.first() {
            Some(&op) if self.ty(op) == "OpPreScript" => {
                let end = self.block_end(s, 0)?;
                Ok((end > 0).then_some((1, end)))
            }
            _ => Ok(None),
        }
    }
}

impl Value {
    /// An unsure value that might also be anything the stepper can't name.
    fn or_anything(self) -> Self {
        match self {
            Value::Unsure(mut v) => {
                v.push(Value::unknown());
                Value::Unsure(v)
            }
            v => Value::Unsure(vec![v, Value::unknown()]),
        }
    }
}

/// Steps `hero`'s STARTUP after the archive's other scripts, as the module
/// says. A hero without a script makes no clones.
pub fn fresh(igz: &Igz<'_>, hero: &Actor<'_>) -> Result<Fresh, FormatError> {
    let Some(hero_script) = hero.info.placement.script else {
        return Ok(Fresh {
            script: hero.index,
            clones: Vec::new(),
            unsure: Vec::new(),
        });
    };
    let mut st = Stepper {
        igz,
        cells: HashMap::new(),
        bound: HashMap::new(),
        clones: Vec::new(),
        unsure: 0,
        notes: Vec::new(),
        steps: 0,
        depth: 0,
        passed: std::collections::HashSet::new(),
        hero: false,
        script: 0,
    };
    let scripts: Vec<usize> = igz
        .objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| igz.type_name(o) == ScriptInfo::TYPE)
        .map(|(i, _)| i)
        .collect();
    for &object in &scripts {
        if object == hero_script {
            continue;
        }
        let s = st.script(object)?;
        st.script = object;
        if let Some((from, to)) = st.startup(&s)? {
            st.step(&s, from, to)?;
        }
        if let Some((from, to)) = st.top(&s)? {
            st.step(&s, from, to)?;
        }
    }
    // Clones another actor made aren't the hero's.
    st.clones.clear();
    st.bound.clear();
    st.hero = true;
    let s = st.script(hero_script)?;
    st.script = s.object;
    if let Some((from, to)) = st.startup(&s)? {
        st.step(&s, from, to)?;
    }
    Ok(Fresh {
        script: hero_script,
        clones: st.clones,
        unsure: st.notes,
    })
}

/// One clone hanging on the hero through clones that draw: what a turntable
/// draws beside the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hung {
    /// Into [`Fresh::clones`].
    pub clone: usize,
    pub template: Template,
    /// An earlier entry of the hung list; `None` on the hero.
    pub parent: Option<usize>,
    /// The parent's attach list entry; `None` on the parent itself.
    pub slot: Option<usize>,
}

/// Why [`hung`] can't say what a fresh figure carries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unsettled {
    /// A clone that might hang a model where the stepper couldn't tell.
    #[error("the clone at op {at} might hang a model ({why})")]
    Unsure { at: usize, why: String },
    /// A model hanging on a clone that draws nothing.
    #[error("the clone at op {at} hangs a model on one that draws nothing")]
    Parent { at: usize },
}

/// The clones of `fresh` that draw (by `draws`, which knows the archive's
/// and `global.bld`'s models) and hang on the hero, directly or through
/// others that draw, each after its parent. An unsure clone that might draw
/// and might hang on the hero refuses the set, as does a drawing clone
/// hanging on one that doesn't draw: what a fresh figure carries is then
/// unsettled, never guessed.
pub fn hung(fresh: &Fresh, draws: impl Fn(&Template) -> bool) -> Result<Vec<Hung>, Unsettled> {
    // Whether each clone hangs on the hero, and through which clones.
    let root = |mut c: usize| -> Option<Vec<usize>> {
        let mut chain = vec![c];
        for _ in 0..fresh.clones.len() {
            match fresh.clones[c].node?.owner {
                Owner::Hero => return Some(chain),
                Owner::Clone(p) => {
                    chain.push(p);
                    c = p;
                }
            }
        }
        None
    };
    let mut out: Vec<Hung> = Vec::new();
    let mut index: HashMap<usize, usize> = HashMap::new();
    for (i, c) in fresh.clones.iter().enumerate() {
        let might_draw = c.template.as_ref().is_none_or(&draws);
        if c.unsure {
            if might_draw && (c.node.is_none() || root(i).is_some()) {
                let why = fresh
                    .unsure
                    .iter()
                    .rfind(|(at, _)| *at <= c.at)
                    .map_or_else(String::new, |(at, w)| format!("op {at}: {w}"));
                return Err(Unsettled::Unsure { at: c.at, why });
            }
            continue;
        }
        let (Some(template), Some(node)) = (c.template, c.node) else {
            continue;
        };
        if !draws(&template) || root(i).is_none() {
            continue;
        }
        let parent = match node.owner {
            Owner::Hero => None,
            Owner::Clone(p) => match index.get(&p) {
                Some(&k) => Some(k),
                None => return Err(Unsettled::Parent { at: c.at }),
            },
        };
        index.insert(i, out.len());
        out.push(Hung {
            clone: i,
            template,
            parent,
            slot: node.slot,
        });
    }
    Ok(out)
}

/// A model's attach points in list order (`tfbModelInfo._attachList`),
/// empty when it has none.
pub fn attach_points<'a>(
    igz: &Igz<'a>,
    model: usize,
) -> Result<Vec<BoneAttachInfo<'a>>, FormatError> {
    let m = ModelInfo::read(igz, model)?;
    let ObjectRef::Object(list) = m.attach_list else {
        return Ok(Vec::new());
    };
    BoneAttachInfoList::read(igz, list)?
        .nodes
        .into_iter()
        .map(|n| BoneAttachInfo::read(igz, n))
        .collect()
}

/// The model a placement draws: a `tfbActorInfo`'s `_model`; none for any
/// other placement.
pub fn placement_model(igz: &Igz<'_>, placement: usize) -> Result<Option<usize>, FormatError> {
    let ty = igz
        .objects()
        .get(placement)
        .map_or("", |o| igz.type_name(o));
    if !is_a(ty, ActorInfo::TYPE) {
        return Ok(None);
    }
    Ok(ActorInfo::read(igz, placement)?.model)
}
