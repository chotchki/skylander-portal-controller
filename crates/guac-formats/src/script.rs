//! tfbScript op graphs (openspec AH.13, design decision 3): the scripts an
//! IGZ holds, every op and operand they reach typed by its registry layout,
//! for the interpreter (`guac-script`, AH.14) to run.
//!
//! A script is a `tfbScriptInfo` (named by its `.ai` source path) whose
//! `OpCodeList` lists op objects by position, one object at many positions
//! where the file shares it (`flow continue` closes every block). Each op's
//! `_branchPC` is how many ops after it its block holds, which a failed
//! check, a finished loop, a definition or a section skips; `flow end`
//! stores 0xFFFF and ends every block of its context. Blocks don't nest
//! strictly (an else-if's skip runs to its chain's end), so a run steps a
//! position over the list, never a tree. The disc uses 44 op classes
//! ([`Class`]), laid out in [`ops`] and AE.2's [`crate::igz::layouts`].
//!
//! Operands ([`Operand`]) are stacks, a path of local objects, imports and
//! named objects (an RHS stack adds a value class and, with no path, a
//! literal), and expressions, `op1 <+ - * /> op2`. Class words (a literal's
//! class, a variable's container) index the file's EXNM `metaobject`
//! names, most without the index flag (aj-script's 1,485,888 on the disc,
//! section 6.1). [`Graph::read`] resolves every one it reads and refuses, by
//! name, an op outside the 44 classes, a `_branchPC` past its list or on an
//! op that opens no block, an operand that's neither a stack nor an
//! expression and a class word past EXNM or outside `metaobject`.

pub mod ops;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::igz::Igz;
use crate::igz::layouts::{
    BRANCH_END, Layout, ObjectRef, OpChangeMembership, OpCode, OpCodeList, OpDefineMacro,
    OpLoopValue, OpMacroInterface, OpMacroParameter, OpUseMacro, RhsReferenceStack, RhsValueStack,
    ScriptInfo, Stack, TypeRef, ValueInfo, ValueRhsVariant, Variable, is_a,
};
use crate::{ErrorKind, FormatError};

pub use ops::{
    Control, Definition, GroupStack, OpCheckFov, OpCheckMembership, OpCheckReference, OpCheckValue,
    OpControl, OpDecValue, OpDefineMacroSpecialization, OpDisplace, OpFields, OpFindSubSet,
    OpFindVariable, OpForEach, OpIncValue, OpMove, OpRemove, OpReset, OpSetBehavior,
    OpSetReference, OpSetValue, OpSlideValue, OpSpawn, OpStartSequence, OpStopSequence,
    OpTeleportTo, OpTurnTo, OpUserBehavior, ValueStack,
};

const FORMAT: &str = "tfbScript";

/// The EXNM namespace class words index.
pub const METAOBJECT: &str = "metaobject";

/// One field of an op, as [`Op::fields`] lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field<'a> {
    Ref(ObjectRef<'a>),
    Type(TypeRef<'a>),
    /// An enum or unsigned word.
    Word(u32),
    Int(i32),
    Long(u64),
    Text(Option<&'a str>),
}

impl<'a> From<ObjectRef<'a>> for Field<'a> {
    fn from(r: ObjectRef<'a>) -> Self {
        Field::Ref(r)
    }
}

impl<'a> From<TypeRef<'a>> for Field<'a> {
    fn from(t: TypeRef<'a>) -> Self {
        Field::Type(t)
    }
}

impl From<u32> for Field<'_> {
    fn from(w: u32) -> Self {
        Field::Word(w)
    }
}

impl From<i32> for Field<'_> {
    fn from(i: i32) -> Self {
        Field::Int(i)
    }
}

impl<'a> From<Option<&'a str>> for Field<'a> {
    fn from(t: Option<&'a str>) -> Self {
        Field::Text(t)
    }
}

/// The 44 op classes, their layouts, and whether each opens a block (an
/// `OpBranch`) and is a flow (an `OpAbstractFlow`, which may store
/// [`BRANCH_END`]). The two flags are the registry's parent chains, which a
/// unit test checks against [`crate::igz::layouts::TYPES`].
macro_rules! op_classes {
    ($( $(#[$doc:meta])* $variant:ident = $name:literal => $layout:ident, $branch:literal, $flow:literal; )*) => {
        /// An op class the disc uses, by its registry name.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Class {
            $( $(#[$doc])* $variant, )*
        }

        impl Class {
            /// In aj-script's census order, most positions first.
            pub const ALL: &'static [Class] = &[$( Class::$variant, )*];

            pub fn type_name(self) -> &'static str {
                match self {
                    $( Class::$variant => $name, )*
                }
            }

            pub fn from_type(name: &str) -> Option<Self> {
                match name {
                    $( $name => Some(Class::$variant), )*
                    _ => None,
                }
            }

            /// An `OpBranch`: `_branchPC` is the length of its block.
            pub fn opens_block(self) -> bool {
                match self {
                    $( Class::$variant => $branch, )*
                }
            }

            /// An `OpAbstractFlow`: it pops blocks, and may store [`BRANCH_END`].
            pub fn is_flow(self) -> bool {
                match self {
                    $( Class::$variant => $flow, )*
                }
            }
        }

        /// One op, typed by its class.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Op<'a> {
            $( $variant($layout<'a>), )*
        }

        impl<'a> Op<'a> {
            /// Reads `igz.objects()[object]` by its class, refusing a type
            /// that isn't one of the 44 by its name.
            pub fn read(igz: &Igz<'a>, object: usize) -> Result<Self, FormatError> {
                Ok(match class_of(igz, object)? {
                    $( Class::$variant => Op::$variant(<$layout<'a> as Layout<'a>>::read(igz, object)?), )*
                })
            }

            pub fn class(&self) -> Class {
                match self {
                    $( Op::$variant(_) => Class::$variant, )*
                }
            }

            /// Its `OpCode` fields: name, flags and block length.
            pub fn code(&self) -> OpCode<'a> {
                match self {
                    $( Op::$variant(l) => l.code(), )*
                }
            }

            /// Every field by its registry name, parents' first.
            pub fn fields(&self) -> Vec<(&'static str, Field<'a>)> {
                let mut out = Vec::new();
                match self {
                    $( Op::$variant(l) => l.listed(&mut out), )*
                }
                out
            }
        }
    };
}

op_classes! {
    MacroParameter = "OpMacroParameter" => OpMacroParameter, false, false;
    SetValue = "OpSetValue" => OpSetValue, false, false;
    /// `flow continue`: closes a block.
    FlowContinue = "OpFlowBuiltInBehavior" => OpCode, true, true;
    CheckValue = "OpCheckValue" => OpCheckValue, true, false;
    MacroInterface = "OpMacroInterface" => OpMacroInterface, true, false;
    /// `flow macro`: enters the called macro's body.
    FlowMacro = "OpFlowMacro" => OpCode, true, true;
    SetReference = "OpSetReference" => OpSetReference, false, false;
    UseMacro = "OpUseMacro" => OpUseMacro, true, false;
    /// A forward skip; the last op of a check's block, it's the else.
    Skip = "OpAbstractFlow" => OpCode, true, true;
    ChangeMembership = "OpChangeMembership" => OpChangeMembership, false, false;
    Spawn = "OpSpawn" => OpSpawn, true, false;
    CreateVariable = "OpCreateVariable" => Variable, false, false;
    CheckReference = "OpCheckReference" => OpCheckReference, true, false;
    SlideValue = "OpSlideValue" => OpSlideValue, true, false;
    Remove = "OpRemove" => OpRemove, false, false;
    /// `flow`: steps the loop it closes.
    FlowLoop = "OpFlowLoop" => OpCode, true, true;
    IncValue = "OpIncValue" => OpIncValue, false, false;
    Displace = "OpDisplace" => OpDisplace, false, false;
    StartSequence = "OpStartSequence" => OpStartSequence, true, false;
    FindVariable = "OpFindVariable" => OpFindVariable, true, false;
    /// `flow end`: returns from a macro, or ends the tick.
    FlowEnd = "OpFlowBehavior" => OpCode, true, true;
    SetBehavior = "OpSetBehavior" => OpSetBehavior, false, false;
    UserBehavior = "OpUserBehavior" => OpUserBehavior, true, false;
    ForEach = "OpForEach" => OpForEach, true, false;
    LoopValue = "OpLoopValue" => OpLoopValue, true, false;
    DefineMacro = "OpDefineMacro" => OpDefineMacro, true, false;
    StartUp = "OpStartUp" => OpCode, true, false;
    ShutDown = "OpShutDown" => OpCode, true, false;
    PreScript = "OpPreScript" => OpCode, true, false;
    FindSubSet = "OpFindSubSet" => OpFindSubSet, true, false;
    DefineMacroSpecialization = "OpDefineMacroSpecialization" => OpDefineMacroSpecialization, true, false;
    Control = "OpControl" => OpControl, true, false;
    StopSequence = "OpStopSequence" => OpStopSequence, false, false;
    CheckMembership = "OpCheckMembership" => OpCheckMembership, true, false;
    DefineStructure = "OpDefineStructure" => Definition, true, false;
    Reset = "OpReset" => OpReset, false, false;
    MacroSection = "OpMacroSection" => OpCode, true, false;
    CheckFov = "OpCheckFOV" => OpCheckFov, true, false;
    DecValue = "OpDecValue" => OpDecValue, false, false;
    StructureSection = "OpStructureSection" => OpCode, true, false;
    TurnTo = "OpTurnTo" => OpTurnTo, true, false;
    MoveTo = "OpMoveTo" => OpMove, true, false;
    TeleportTo = "OpTeleportTo" => OpTeleportTo, false, false;
    MoveFrom = "OpMoveFrom" => OpMove, true, false;
}

/// An object's op class, or its type refused by name.
pub fn class_of(igz: &Igz<'_>, object: usize) -> Result<Class, FormatError> {
    let Some(o) = igz.objects().get(object) else {
        return Err(FormatError::new(
            FORMAT,
            "op",
            0,
            ErrorKind::OutOfRange {
                value: object as u64,
                limit: igz.objects().len() as u64,
            },
        ));
    };
    let name = igz.type_name(o);
    Class::from_type(name).ok_or_else(|| {
        refuse(
            igz,
            object,
            "op",
            ErrorKind::WrongType {
                expected: "one of the 44 op classes",
                found: name.to_owned(),
            },
        )
    })
}

/// The stack classes an operand can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StackClass {
    /// `ValueStack`: a cell read or written.
    Value,
    /// `RHSValueStack`: a value, a literal when its path is empty.
    RhsValue,
    /// `RHSReferenceStack`: an object, `_editType` its class.
    RhsReference,
    /// `ReferenceStack`: a reference cell written.
    Reference,
    /// `SetStack`: a set.
    Set,
    /// `ScriptGroupStack`: a script object (a macro, a behaviour, a placement).
    Group,
    /// `PositionStack`: a point.
    Position,
    /// `VectorStack`: what `OpDisplace` moves.
    Vector,
    /// `AbstractPlacementList`: what `OpSpawn` clones.
    PlacementList,
    /// `ScriptObjectList`: what `OpReset` resets.
    ObjectList,
    /// `OpDefineMacroStack`: a specialization's base.
    MacroDefinitions,
}

impl StackClass {
    pub const ALL: [StackClass; 11] = [
        Self::Value,
        Self::RhsValue,
        Self::RhsReference,
        Self::Reference,
        Self::Set,
        Self::Group,
        Self::Position,
        Self::Vector,
        Self::PlacementList,
        Self::ObjectList,
        Self::MacroDefinitions,
    ];

    pub fn type_name(self) -> &'static str {
        match self {
            Self::Value => "ValueStack",
            Self::RhsValue => "RHSValueStack",
            Self::RhsReference => "RHSReferenceStack",
            Self::Reference => "ReferenceStack",
            Self::Set => "SetStack",
            Self::Group => "ScriptGroupStack",
            Self::Position => "PositionStack",
            Self::Vector => "VectorStack",
            Self::PlacementList => "AbstractPlacementList",
            Self::ObjectList => "ScriptObjectList",
            Self::MacroDefinitions => "OpDefineMacroStack",
        }
    }

    pub fn from_type(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.type_name() == name)
    }
}

/// An RHS stack's own fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhsValue<'a> {
    /// The class the value reads as (`IntMeasurement`, `FloatMeasurement`).
    pub class: TypeRef<'a>,
    /// The literal's bits, read when the path is empty.
    pub bits: i32,
    /// A random literal: a roll up to `bits` (aj-vm).
    pub is_random: bool,
    /// Filled at link: false in files.
    pub type_is_number: bool,
    /// `RHSReferenceStack` only.
    pub type_is_meta: bool,
}

/// A stack operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path<'a> {
    pub class: StackClass,
    pub elements: Vec<ObjectRef<'a>>,
    /// Value stacks' `_otherPoint`: a `PositionStack` a distance or a
    /// heading on the path is measured from.
    pub other_point: ObjectRef<'a>,
    /// RHS stacks' class and literal.
    pub rhs: Option<RhsValue<'a>>,
    /// Group and RHS reference stacks' `_editType`: what the path names.
    pub edit_type: TypeRef<'a>,
}

/// What an op's operand field points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand<'a> {
    Stack(Path<'a>),
    /// `ValueRHSVariant`: `op1 <operator> op2`, or `op1` alone.
    Expression(ValueRhsVariant<'a>),
}

impl<'a> Operand<'a> {
    /// Reads `igz.objects()[object]`, a stack or an expression; any other
    /// type is refused by name.
    pub fn read(igz: &Igz<'a>, object: usize) -> Result<Self, FormatError> {
        let name = igz.objects().get(object).map_or("", |o| igz.type_name(o));
        if name == ValueRhsVariant::TYPE {
            return Ok(Operand::Expression(ValueRhsVariant::read(igz, object)?));
        }
        let Some(class) = StackClass::from_type(name) else {
            return Err(refuse(
                igz,
                object,
                "operand",
                ErrorKind::WrongType {
                    expected: "a tfbScript stack or expression",
                    found: name.to_owned(),
                },
            ));
        };
        let mut path = Path {
            class,
            elements: Vec::new(),
            other_point: ObjectRef::Null,
            rhs: None,
            edit_type: TypeRef::Null,
        };
        match class {
            StackClass::Value => {
                let s = ValueStack::read(igz, object)?;
                path.elements = s.stack.path;
                path.other_point = s.other_point;
            }
            StackClass::RhsValue => {
                let s = RhsValueStack::read(igz, object)?;
                path.rhs = Some(rhs_value(&s, false));
                path.elements = s.stack.path;
                path.other_point = s.other_point;
            }
            StackClass::RhsReference => {
                let s = RhsReferenceStack::read(igz, object)?;
                path.rhs = Some(rhs_value(&s.value, s.type_is_meta));
                path.edit_type = s.edit_type;
                path.elements = s.value.stack.path;
                path.other_point = s.value.other_point;
            }
            StackClass::Group => {
                let s = GroupStack::read(igz, object)?;
                path.elements = s.stack.path;
                path.edit_type = s.edit_type;
            }
            _ => path.elements = Stack::read(igz, object)?.path,
        }
        Ok(Operand::Stack(path))
    }
}

fn rhs_value<'a>(s: &RhsValueStack<'a>, type_is_meta: bool) -> RhsValue<'a> {
    RhsValue {
        class: s.value_type,
        bits: s.value,
        is_random: s.is_random,
        type_is_number: s.type_is_number,
        type_is_meta,
    }
}

/// One script: its `tfbScriptInfo`, its list and the op object at each
/// position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script<'a> {
    pub object: usize,
    /// The `.ai` source path.
    pub name: Option<&'a str>,
    /// Its `OpCodeList`.
    pub list: usize,
    pub ops: Vec<usize>,
    /// Its variables' declarations, as stored; built into a placement's
    /// locals at its first tick (aj-vm), not followed here.
    pub master_var_list: ObjectRef<'a>,
}

/// Every script of one IGZ and everything they reach, each object decoded
/// once: the ops at their positions, the variables and definitions their
/// paths and parameters name (stubs included), the operands, and the
/// `ValueInfo`s on paths.
#[derive(Debug, Clone, Default)]
pub struct Graph<'a> {
    pub scripts: Vec<Script<'a>>,
    pub ops: BTreeMap<usize, Op<'a>>,
    pub operands: BTreeMap<usize, Operand<'a>>,
    pub values: BTreeMap<usize, ValueInfo<'a>>,
    /// Class words stored without the index flag, each resolved through
    /// EXNM to a `metaobject` class.
    pub unflagged_classes: usize,
}

/// How an object of a given type is reached, per the file's type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Script,
    Op,
    Operand,
    Value,
    Other,
}

#[derive(Debug, Clone, Copy)]
enum Pending {
    Op(usize),
    Operand(usize),
    Value(usize),
}

/// Bytes of the file's data per list entry (positions and path elements)
/// one read may decode. Every entry is a 4-byte word of its own in a file
/// Alchemy writes, so this is twice that much room, and lists that share
/// their words can't multiply the work past it.
const DATA_BYTES_PER_ENTRY: usize = 2;

impl<'a> Graph<'a> {
    /// Reads every `tfbScriptInfo` in `igz` and everything its ops reach.
    pub fn read(igz: &Igz<'a>) -> Result<Self, FormatError> {
        let data: usize = igz.data_sections().iter().map(|s| s.len()).sum();
        let mut reader = Reader {
            igz,
            kinds: kinds(igz),
            graph: Graph::default(),
            pending: Vec::new(),
            entries: 0,
            budget: (data / DATA_BYTES_PER_ENTRY).max(64),
        };
        for i in 0..igz.objects().len() {
            if reader.kind(i) == Kind::Script {
                reader.script(i)?;
            }
        }
        while let Some(p) = reader.pending.pop() {
            match p {
                Pending::Op(i) => reader.op(i)?,
                Pending::Operand(i) => reader.operand(i)?,
                Pending::Value(i) => reader.value(i)?,
            }
        }
        reader.check_branches()?;
        Ok(reader.graph)
    }

    /// The op at `position` of `script`.
    pub fn op(&self, script: &Script<'_>, position: usize) -> Option<&Op<'a>> {
        self.ops.get(script.ops.get(position)?)
    }

    /// The script whose source path ends with `name` (`009_Eruptor.ai`).
    pub fn script(&self, name: &str) -> Option<&Script<'a>> {
        self.scripts
            .iter()
            .find(|s| s.name.is_some_and(|n| n.ends_with(name)))
    }
}

fn kinds(igz: &Igz<'_>) -> Vec<Kind> {
    igz.type_names()
        .iter()
        .map(|&name| {
            if name == ScriptInfo::TYPE {
                Kind::Script
            } else if Class::from_type(name).is_some() || is_a(name, OpCode::TYPE) {
                Kind::Op
            } else if name == ValueRhsVariant::TYPE || StackClass::from_type(name).is_some() {
                Kind::Operand
            } else if name == ValueInfo::TYPE {
                Kind::Value
            } else {
                Kind::Other
            }
        })
        .collect()
}

fn refuse(igz: &Igz<'_>, object: usize, field: &'static str, kind: ErrorKind) -> FormatError {
    let at = igz.objects()[object].at;
    let mut e = igz.err_at(at, field, kind);
    e.format = FORMAT;
    e
}

struct Reader<'g, 'a> {
    igz: &'g Igz<'a>,
    kinds: Vec<Kind>,
    graph: Graph<'a>,
    pending: Vec<Pending>,
    entries: usize,
    budget: usize,
}

impl<'a> Reader<'_, 'a> {
    fn kind(&self, object: usize) -> Kind {
        let t = self.igz.objects()[object].type_index as usize;
        self.kinds.get(t).copied().unwrap_or(Kind::Other)
    }

    /// Counts `n` list entries against the budget.
    fn charge(&mut self, object: usize, n: usize) -> Result<(), FormatError> {
        self.entries = self.entries.saturating_add(n);
        if self.entries > self.budget {
            return Err(refuse(
                self.igz,
                object,
                "_count",
                ErrorKind::OutOfRange {
                    value: self.entries as u64,
                    limit: self.budget as u64,
                },
            ));
        }
        Ok(())
    }

    fn script(&mut self, object: usize) -> Result<(), FormatError> {
        let info = ScriptInfo::read(self.igz, object)?;
        let list = info
            .op_list
            .ok_or_else(|| refuse(self.igz, object, "_opList", ErrorKind::Missing))?;
        let ops = OpCodeList::read(self.igz, list)?.ops;
        self.charge(list, ops.len())?;
        for &op in &ops {
            if !self.graph.ops.contains_key(&op) {
                self.pending.push(Pending::Op(op));
            }
        }
        self.graph.scripts.push(Script {
            object,
            name: info.info.name,
            list,
            ops,
            master_var_list: info.master_var_list,
        });
        Ok(())
    }

    fn op(&mut self, object: usize) -> Result<(), FormatError> {
        if self.graph.ops.contains_key(&object) {
            return Ok(());
        }
        let op = Op::read(self.igz, object)?;
        let mut operands: Vec<(&'static str, ObjectRef<'a>)> = Vec::new();
        match &op {
            Op::MacroParameter(p) => {
                self.variable(object, &p.var)?;
                operands.extend([("_RHS", p.rhs), ("_indexRHS", p.index_rhs)]);
                self.definition(object, "_macroDef", p.macro_def)?;
            }
            Op::CreateVariable(v) => self.variable(object, v)?,
            Op::FindVariable(f) => {
                self.class_word(object, "_varContainerType", f.container_type)?;
                self.content_word(object, "_varContentsType", f.contents_type)?;
                operands.extend([
                    ("_skipRHS", f.skip_rhs),
                    ("_owner", f.owner),
                    ("_indexRHS", f.index_rhs),
                ]);
            }
            _ => operands = operand_fields(&op),
        }
        for (field, r) in operands {
            self.operand_ref(object, field, r)?;
        }
        self.graph.ops.insert(object, op);
        Ok(())
    }

    fn variable(&mut self, object: usize, v: &Variable<'a>) -> Result<(), FormatError> {
        self.class_word(object, "_varContainerType", v.container_type)?;
        self.content_word(object, "_varContentsType", v.contents_type)
    }

    /// A definition an op names (`_macroDef`): null, or an op to read.
    fn definition(
        &mut self,
        object: usize,
        field: &'static str,
        r: ObjectRef<'a>,
    ) -> Result<(), FormatError> {
        match r {
            ObjectRef::Null => Ok(()),
            ObjectRef::Object(d) if self.kind(d) == Kind::Op => {
                self.pending.push(Pending::Op(d));
                Ok(())
            }
            other => Err(self.wrong(object, field, "a local op", other)),
        }
    }

    fn operand_ref(
        &mut self,
        object: usize,
        field: &'static str,
        r: ObjectRef<'a>,
    ) -> Result<(), FormatError> {
        match r {
            ObjectRef::Null => Ok(()),
            ObjectRef::Object(o) => {
                if !self.graph.operands.contains_key(&o) {
                    self.pending.push(Pending::Operand(o));
                }
                Ok(())
            }
            other => Err(self.wrong(object, field, "a local operand", other)),
        }
    }

    /// `field` holds `found` where `expected` belongs.
    fn wrong(
        &self,
        object: usize,
        field: &'static str,
        expected: &'static str,
        found: impl std::fmt::Debug,
    ) -> FormatError {
        refuse(
            self.igz,
            object,
            field,
            ErrorKind::WrongType {
                expected,
                found: format!("{found:?}"),
            },
        )
    }

    fn operand(&mut self, object: usize) -> Result<(), FormatError> {
        if self.graph.operands.contains_key(&object) {
            return Ok(());
        }
        let operand = Operand::read(self.igz, object)?;
        match &operand {
            Operand::Expression(e) => {
                for (field, r) in [
                    ("_varOp1", e.op1),
                    ("_varOp2", e.op2),
                    ("_conditioner", e.conditioner),
                ] {
                    self.operand_ref(object, field, r)?;
                }
            }
            Operand::Stack(p) => {
                self.charge(object, p.elements.len())?;
                if let Some(rhs) = &p.rhs {
                    self.class_word(object, "_type", rhs.class)?;
                }
                if matches!(p.class, StackClass::Group | StackClass::RhsReference) {
                    self.content_word(object, "_editType", p.edit_type)?;
                }
                self.operand_ref(object, "_otherPoint", p.other_point)?;
                for e in &p.elements {
                    if let ObjectRef::Object(o) = *e {
                        match self.kind(o) {
                            Kind::Op => self.pending.push(Pending::Op(o)),
                            Kind::Value => self.pending.push(Pending::Value(o)),
                            _ => {}
                        }
                    }
                }
            }
        }
        self.graph.operands.insert(object, operand);
        Ok(())
    }

    fn value(&mut self, object: usize) -> Result<(), FormatError> {
        if self.graph.values.contains_key(&object) {
            return Ok(());
        }
        let v = ValueInfo::read(self.igz, object)?;
        self.class_word(object, "_type", v.value_type)?;
        self.graph.values.insert(object, v);
        Ok(())
    }

    /// A field that names a class: null, or a `metaobject` EXNM entry.
    fn class_word(
        &mut self,
        object: usize,
        field: &'static str,
        t: TypeRef<'a>,
    ) -> Result<(), FormatError> {
        match t {
            TypeRef::Null => Ok(()),
            TypeRef::Named { .. } | TypeRef::Indexed { .. } => self.metaobject(object, field, t),
            other => Err(self.wrong(object, field, "a metaobject class", other)),
        }
    }

    /// A field that names a class or a definition: also a local
    /// `OpDefineMacro`/`OpDefineStructure` (read as an op) or an import
    /// (a `global.bld` definition).
    fn content_word(
        &mut self,
        object: usize,
        field: &'static str,
        t: TypeRef<'a>,
    ) -> Result<(), FormatError> {
        match t {
            TypeRef::Null | TypeRef::Import(_) => Ok(()),
            TypeRef::Object(d) => self.definition(object, field, ObjectRef::Object(d)),
            TypeRef::Named { .. } | TypeRef::Indexed { .. } => self.metaobject(object, field, t),
        }
    }

    fn metaobject(
        &mut self,
        object: usize,
        field: &'static str,
        t: TypeRef<'a>,
    ) -> Result<(), FormatError> {
        let (name, unflagged) = match t {
            TypeRef::Named { name, .. } => (name, false),
            TypeRef::Indexed {
                name: Some(name), ..
            } => (name, true),
            TypeRef::Indexed { index, name: None } => {
                return Err(refuse(
                    self.igz,
                    object,
                    field,
                    ErrorKind::OutOfRange {
                        value: index.into(),
                        limit: self.igz.external_names().len() as u64,
                    },
                ));
            }
            other => return Err(self.wrong(object, field, "a metaobject class", other)),
        };
        if name.namespace != METAOBJECT {
            return Err(refuse(
                self.igz,
                object,
                field,
                ErrorKind::WrongType {
                    expected: "a metaobject class",
                    found: format!("{}::{}", name.namespace, name.name),
                },
            ));
        }
        if unflagged {
            self.graph.unflagged_classes += 1;
        }
        Ok(())
    }

    /// Every position's block stays inside its list, and only ops that
    /// open blocks have one.
    fn check_branches(&self) -> Result<(), FormatError> {
        for s in &self.graph.scripts {
            for (p, &object) in s.ops.iter().enumerate() {
                let Some(op) = self.graph.ops.get(&object) else {
                    continue;
                };
                let (class, n) = (op.class(), op.code().branch);
                let fits = match n {
                    0 => true,
                    BRANCH_END => class.is_flow(),
                    n => class.opens_block() && p + usize::from(n) < s.ops.len(),
                };
                if !fits {
                    return Err(refuse(
                        self.igz,
                        object,
                        "_branchPC",
                        ErrorKind::OutOfRange {
                            value: (p + usize::from(n)) as u64,
                            limit: s.ops.len() as u64,
                        },
                    ));
                }
            }
        }
        Ok(())
    }
}

/// An op's operand fields, by registry name: what [`Graph`] reads as
/// [`Operand`]s. Definitions, caches and link-time fields aren't operands.
fn operand_fields<'a>(op: &Op<'a>) -> Vec<(&'static str, ObjectRef<'a>)> {
    match op {
        Op::SetValue(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::CheckValue(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::SetReference(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs), ("_indexRHS", o.index_rhs)],
        Op::UseMacro(o) => vec![
            ("_NP", o.np.map_or(ObjectRef::Null, ObjectRef::Object)),
            ("_indexRHS", o.index_rhs),
        ],
        Op::ChangeMembership(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::Spawn(o) => vec![
            ("_LHS", o.control.lhs),
            ("_RHS", o.rhs),
            ("_facingRHS", o.facing_rhs),
        ],
        Op::CheckReference(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs), ("_indexRHS", o.index_rhs)],
        Op::SlideValue(o) => vec![
            ("_LHS", o.lhs),
            ("_RHS", o.rhs),
            ("_secondsRHS", o.seconds_rhs),
            ("_easeOutRHS", o.ease_out_rhs),
            ("_easeInRHS", o.ease_in_rhs),
        ],
        Op::Remove(o) => vec![("_NP", o.np)],
        Op::IncValue(o) => vec![("_LHS", o.lhs)],
        Op::DecValue(o) => vec![("_LHS", o.lhs)],
        Op::Displace(o) => vec![
            ("_NP", o.np),
            ("_lenNP", o.len_np),
            ("_headNP", o.head_np),
            ("_pitchNP", o.pitch_np),
        ],
        Op::StartSequence(o) => vec![("_LHS", o.lhs), ("_indexRHS", o.index_rhs)],
        Op::SetBehavior(o) => vec![("_NP", o.np), ("_indexRHS", o.index_rhs)],
        Op::ForEach(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::LoopValue(o) => vec![("_RHS", o.rhs)],
        Op::FindSubSet(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::DefineMacroSpecialization(o) => {
            vec![("_NP", o.np.map_or(ObjectRef::Null, ObjectRef::Object))]
        }
        Op::Control(o) => vec![("_LHS", o.control.lhs), ("_indexRHS", o.index_rhs)],
        Op::StopSequence(o) => vec![("_NP", o.np)],
        Op::CheckMembership(o) => vec![("_LHS", o.lhs), ("_RHS", o.rhs)],
        Op::Reset(o) => vec![("_NP", o.np)],
        Op::CheckFov(o) => vec![
            ("_RHSfacing", o.rhs_facing),
            ("_RHSfov", o.rhs_fov),
            ("_fromLHS", o.from_lhs),
            ("_LHS", o.lhs),
            ("_RHS", o.rhs),
        ],
        Op::TurnTo(o) => vec![
            ("_facingRHS", o.facing_rhs),
            ("_NP", o.np),
            ("_indexRHS", o.index_rhs),
        ],
        Op::MoveTo(o) | Op::MoveFrom(o) => vec![
            ("_indexRHS", o.index_rhs),
            ("_LHS", o.lhs),
            ("_RHS", o.rhs),
            ("_NP", o.np),
        ],
        Op::TeleportTo(o) => vec![("_LHS", o.lhs), ("_facingRHS", o.facing_rhs)],
        // The variable ops are handled by the caller; the rest carry none.
        Op::MacroParameter(_)
        | Op::CreateVariable(_)
        | Op::FindVariable(_)
        | Op::FlowContinue(_)
        | Op::MacroInterface(_)
        | Op::FlowMacro(_)
        | Op::Skip(_)
        | Op::FlowLoop(_)
        | Op::FlowEnd(_)
        | Op::UserBehavior(_)
        | Op::DefineMacro(_)
        | Op::StartUp(_)
        | Op::ShutDown(_)
        | Op::PreScript(_)
        | Op::DefineStructure(_)
        | Op::MacroSection(_)
        | Op::StructureSection(_) => Vec::new(),
    }
}
