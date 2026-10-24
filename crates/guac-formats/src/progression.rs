//! Giants' progression tables, read statically for good (openspec AH.2,
//! design decisions 2 and 10): the experience each level starts at, the
//! figure's save params (where each value sits in the tag's data) and the
//! most gold a figure holds. The server needs them outside a game (stats and
//! edits while no level runs, the store's caps), so they're permanent static
//! reads that no rung deletes; in play AH.16's VM reads the same data and
//! must agree with this.
//!
//! `character/init_setup.bld`'s "Save - Init Portal Save Param Table" builds
//! both tables. Its body calls "Portal - Define Levelup Table" once, whose
//! arguments `level xp (1)` to `level xp (N)` fill entries 1 to N of a set
//! its own body sizes to N + 1 (entry 0, level 1's, stays 0), then calls
//! "Save - Setup Portal Save Param" or one of its specializations once per
//! save param (24 calls on the 01.00 disc), three of them inside loops. The
//! loops and a scratch counter make the per-pass words and bits, so this
//! reader steps the body's ops in order over a static environment: literals,
//! the loop's pass, arithmetic, `global.bld` values and set counts, what the
//! body itself wrote, the level table and the fields the plain setup macro
//! copies its arguments into (read off that macro's own body). It refuses
//! any op class, operand shape or value it doesn't model, by name, rather
//! than guess; a cell written with something it can't follow is remembered
//! as unknown, and refused only if a row or a branch reads it.
//!
//! `permanent/global.bld`'s "Game - Award Money" clamps a figure's gold: the
//! one `if <x> > C` in its body whose block sets `amount` to `C`. The check's
//! left side isn't read (on the 01.00 disc it isn't a plain path to `amount`);
//! the block's write is what pins the check.
//!
//! Comparison codes are ah-progression's reading (`0 <=, 1 ==, 2 >=, 3 <,
//! 4 >, 5 !=`, design decision 3), which AH.11's level-up at exactly 1,000
//! confirms or breaks. Stepping a body by hand is the narrow rewrite AE.2's
//! toy table also makes; AH.16's VM runs the body itself and must reproduce
//! this reader's output.

use std::collections::HashMap;

use crate::exports::{Export, Exports, Root};
use crate::igz::Igz;
use crate::igz::layouts::{
    BRANCH_END, Layout, NO_OPERATOR, ObjectRef, OpCode, OpCodeList, OpDefineMacro, OpLoopValue,
    OpMacroParameter, OpUseMacro, RhsValueStack, ScriptInfo, ScriptSet, ScriptSetList, Stack,
    ValueInfo, ValueRhsVariant, Variable, is_a, read_all,
};
use crate::script::Op;
use crate::{ErrorKind, FormatError, fnv1a32};

mod ops;

pub use crate::script::{
    OpCheckValue, OpDefineMacroSpecialization, OpIncValue, OpSetReference, OpSetValue,
};
pub use ops::ScriptStructure;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

const FORMAT: &str = "progression";

/// The macro whose body builds the level table and the save params.
pub const SAVE_TABLE: &str = "Save - Init Portal Save Param Table";
pub const DEFINE_LEVELUP: &str = "Portal - Define Levelup Table";
pub const SETUP_PARAM: &str = "Save - Setup Portal Save Param";
pub const SPILLOVER: &str = "Setup Portal Param Spillover";
pub const MIRROR_AND_CLEAR: &str = "Setup Portal Param Mirror and Clear If Over Limit";
pub const EXTENDED_BITFIELD: &str = "Setup Portal Param Extended Bitfield";
/// The macro whose body clamps a figure's gold (`global.bld`).
pub const AWARD_MONEY: &str = "Game - Award Money";
/// The parameter the clamp sets.
pub const AMOUNT: &str = "amount";

/// `_relOperator`'s codes (design decision 3).
pub const RELATIONS: [&str; 6] = ["<=", "==", ">=", "<", ">", "!="];
/// `>`.
pub const GREATER: u32 = 4;

/// The setup call's parameters, as the disc names them.
pub mod params {
    pub const SAVE_PARAM: &str = "save param";
    pub const WORD: &str = "val index";
    pub const BIT: &str = "bit index";
    pub const WIDTH: &str = "num bits";
    pub const BITS_PER_FIELD: &str = "bits per field";
    pub const LIMIT: &str = "limit to val";
    pub const ALL: [&str; 6] = [SAVE_PARAM, WORD, BIT, WIDTH, BITS_PER_FIELD, LIMIT];
    /// The level-up call's: `level xp (1)` and on, and one it passes but
    /// the table doesn't hold.
    pub const LEVEL_XP: &str = "level xp";
    pub const HP_MULT: &str = "level hp mult";
}

/// How many ops one body may step through. The 01.00 body steps 233.
pub const MAX_STEPS: usize = 1 << 16;
/// The most passes one loop may make. The 01.00 disc's loops make 16 at most.
pub const MAX_PASSES: i64 = 1 << 10;
/// The most entries a level table may hold: the server keeps a level in a
/// byte. The 01.00 disc's holds 15.
pub const MAX_LEVELS: i64 = 255;
/// How many `ValueRHSVariant`s an operand may nest, one inside the next.
pub const MAX_DEPTH: usize = 8;

/// Which macro a save param's call makes: the plain setup or one of its
/// specializations, each of which records the param for a special case
/// besides its own fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveKind {
    Plain,
    /// The second area a value runs on into past the first's limit
    /// (experience past 33,000 on the disc).
    Spillover,
    MirrorAndClear,
    ExtendedBitfield,
}

impl SaveKind {
    /// The macro a row of this kind calls, as the disc names it.
    pub fn macro_name(self) -> &'static str {
        match self {
            Self::Plain => SETUP_PARAM,
            Self::Spillover => SPILLOVER,
            Self::MirrorAndClear => MIRROR_AND_CLEAR,
            Self::ExtendedBitfield => EXTENDED_BITFIELD,
        }
    }
}

/// Where one pass of a call puts its value: the 32-bit word and the bit in
/// it it starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveField {
    pub word: u32,
    pub bit: u32,
    /// For a call indexing a set by the pass, the member it names; `None`
    /// past the members the disc lists (a fresh slot) or for a plain param.
    pub member: Option<String>,
}

/// One setup call: a row of the save-param table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveParam {
    /// The call's place in its op list.
    pub position: usize,
    /// The save param it names, or the set it indexes (`pp_Flags1_UpgradeBits`).
    pub param: String,
    pub kind: SaveKind,
    /// One per time the call ran: once outside a loop, once per pass in one.
    pub fields: Vec<SaveField>,
    /// `num bits`.
    pub width: u32,
    /// `bits per field`: 0 for one value, else the width of each of the
    /// values the bits hold.
    pub bits_per_field: u32,
    /// `limit to val`: the most the value holds, 0 for no limit.
    pub limit: u32,
}

/// The level table and the save params, as the save-param table's body
/// builds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTable {
    /// The experience each level starts at, level 1's 0 first.
    pub level_xp: Vec<u32>,
    /// In call order.
    pub params: Vec<SaveParam>,
    /// The ops stepping the body took, loops' passes each counted: a
    /// measured tick for AH.14's op budget to sit above.
    pub steps: usize,
}

impl SaveTable {
    /// Reads both tables out of `init_setup`'s `level.bld`, following imports
    /// into `global` (`global.bld`'s `level.bld`) through `exports`.
    pub fn read(
        init_setup: &Igz<'_>,
        global: &Igz<'_>,
        exports: &Exports,
    ) -> Result<Self, FormatError> {
        let defs = Defs::new(init_setup)?;
        // The table's own macro first, so a file without it says so.
        defs.body(init_setup, SAVE_TABLE)?;
        let reader = Reader::new(init_setup, global, exports, defs)?;
        let (ops, from, to) = reader.defs.body(init_setup, SAVE_TABLE)?;
        let mut run = Run::default();
        reader.step(ops, from, to, &mut run)?;
        let (_, level_xp) = run
            .table
            .ok_or_else(|| refuse(DEFINE_LEVELUP, ErrorKind::Missing))?;
        if run.rows.is_empty() {
            return Err(refuse(SETUP_PARAM, ErrorKind::Missing));
        }
        Ok(Self {
            level_xp,
            params: run.rows,
            steps: run.steps,
        })
    }
}

/// The most gold a figure holds: the `C` of the one `if <x> > C` in
/// "Game - Award Money"'s body whose block sets `amount` to `C` (`x` not
/// read, see the module docs), read off
/// `global.bld`'s `level.bld`. Refused by the macro's name when there's no
/// such check, more than one, or the macro isn't defined exactly once.
pub fn gold_clamp(global: &Igz<'_>) -> Result<u32, FormatError> {
    let defs = Defs::new(global)?;
    let (ops, from, to) = defs.body(global, AWARD_MONEY)?;
    let mut found = Vec::new();
    for p in from..=to {
        if type_of(global, ops[p]) != OpCheckValue::TYPE {
            continue;
        }
        let check = OpCheckValue::read(global, ops[p])?;
        if check.relation != GREATER {
            continue;
        }
        let Some(clamp) = int_literal(global, check.rhs)? else {
            continue;
        };
        let Some(end) = check.op.block_end(p) else {
            continue;
        };
        for &q in &ops[p + 1..=end.min(to)] {
            if type_of(global, q) != OpSetValue::TYPE {
                continue;
            }
            let set = OpSetValue::read(global, q)?;
            if sets_parameter(global, set.lhs, AMOUNT)?
                && int_literal(global, set.rhs)? == Some(clamp)
            {
                found.push(clamp);
                break;
            }
        }
    }
    match found[..] {
        [clamp] if clamp > 0 => Ok(clamp as u32),
        [clamp] => Err(refuse(
            AWARD_MONEY,
            ErrorKind::OutOfRange {
                value: clamp as i64 as u64,
                limit: i32::MAX as u64,
            },
        )),
        [] => Err(refuse(AWARD_MONEY, ErrorKind::Missing)),
        _ => Err(refuse(AWARD_MONEY, ErrorKind::Duplicate)),
    }
}

fn refuse(field: &'static str, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, 0, kind)
}

fn type_of<'a>(igz: &Igz<'a>, object: usize) -> &'a str {
    igz.type_name(&igz.objects()[object])
}

fn stranger(igz: &Igz<'_>, object: usize, expected: &'static str) -> FormatError {
    let o = igz.objects()[object];
    igz.err_at(
        o.at,
        expected,
        ErrorKind::WrongType {
            expected,
            found: igz.type_name(&o).to_owned(),
        },
    )
}

/// The operand inside any `ValueRHSVariant`s with no operator (and no
/// conditioner) around it, to [`MAX_DEPTH`].
fn unwrapped(igz: &Igz<'_>, mut o: usize) -> Result<usize, FormatError> {
    for depth in 0..=MAX_DEPTH {
        if !is_a(type_of(igz, o), ValueRhsVariant::TYPE) {
            return Ok(o);
        }
        let v = ValueRhsVariant::read(igz, o)?;
        match (v.op1, v.operator, v.conditioner) {
            (ObjectRef::Object(inner), NO_OPERATOR, ObjectRef::Null) if depth < MAX_DEPTH => {
                o = inner;
            }
            _ => break,
        }
    }
    Err(refuse(
        "_varOp1",
        ErrorKind::Unsupported { value: o as u64 },
    ))
}

/// An operand that's an int literal: an RHS stack with an empty path and an
/// `IntMeasurement` type, bare or inside plain `ValueRHSVariant`s. `None`
/// for any other operand.
fn int_literal(igz: &Igz<'_>, operand: ObjectRef<'_>) -> Result<Option<i32>, FormatError> {
    let ObjectRef::Object(o) = operand else {
        return Ok(None);
    };
    let Ok(o) = unwrapped(igz, o) else {
        return Ok(None);
    };
    if !is_a(type_of(igz, o), RhsValueStack::TYPE) {
        return Ok(None);
    }
    let s = RhsValueStack::read(igz, o)?;
    let int = s.stack.path.is_empty()
        && !s.is_random
        && s.value_type.metaobject() == Some("IntMeasurement");
    Ok(int.then_some(s.value))
}

/// Whether `lhs` is a stack whose path is the one parameter `name`.
fn sets_parameter(igz: &Igz<'_>, lhs: ObjectRef<'_>, name: &str) -> Result<bool, FormatError> {
    let ObjectRef::Object(o) = lhs else {
        return Ok(false);
    };
    let Ok(o) = unwrapped(igz, o) else {
        return Ok(false);
    };
    if !is_a(type_of(igz, o), Stack::TYPE) {
        return Ok(false);
    }
    let [ObjectRef::Object(p)] = Stack::read(igz, o)?.path[..] else {
        return Ok(false);
    };
    if !is_a(type_of(igz, p), OpMacroParameter::TYPE) {
        return Ok(false);
    }
    Ok(OpMacroParameter::read(igz, p)?.var.name == Some(name))
}

/// Every macro definition that sits in an op list, by name, with the list's
/// ops and its place there.
struct Defs {
    lists: Vec<Vec<usize>>,
    by_name: HashMap<String, Vec<(usize, usize)>>,
}

impl Defs {
    fn new(igz: &Igz<'_>) -> Result<Self, FormatError> {
        let mut lists = Vec::new();
        let mut by_name: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (i, _) in read_all::<ScriptInfo>(igz) {
            let Some(list) = ScriptInfo::read(igz, i)?.op_list else {
                continue;
            };
            let ops = OpCodeList::read(igz, list)?.ops;
            for (p, &op) in ops.iter().enumerate() {
                if is_a(type_of(igz, op), OpDefineMacro::TYPE)
                    && let Some(name) = OpDefineMacro::read(igz, op)?.op.name
                {
                    by_name
                        .entry(name.to_owned())
                        .or_default()
                        .push((lists.len(), p));
                }
            }
            lists.push(ops);
        }
        Ok(Self { lists, by_name })
    }

    /// The op list of the one definition named `name` and its body's first
    /// and last positions there: past its INTERFACE block, through the op
    /// that closes the definition.
    fn body(
        &self,
        igz: &Igz<'_>,
        name: &'static str,
    ) -> Result<(&[usize], usize, usize), FormatError> {
        let (list, p) = match self.by_name.get(name).map(Vec::as_slice) {
            Some(&[one]) => one,
            Some([]) | None => return Err(refuse(name, ErrorKind::Missing)),
            Some(_) => return Err(refuse(name, ErrorKind::Duplicate)),
        };
        let ops = &self.lists[list];
        let end = OpCode::read(igz, ops[p])?
            .block_end(p)
            .filter(|&e| e < ops.len())
            .ok_or_else(|| refuse(name, ErrorKind::Missing))?;
        let iface_end = interface_end(igz, ops, p)?;
        if iface_end > end {
            return Err(refuse(name, ErrorKind::Missing));
        }
        Ok((ops, iface_end + 1, end))
    }
}

/// The last position of the INTERFACE block after the call or definition
/// at `p`.
fn interface_end(igz: &Igz<'_>, ops: &[usize], p: usize) -> Result<usize, FormatError> {
    let iface = ops
        .get(p + 1)
        .filter(|&&o| type_of(igz, o) == "OpMacroInterface")
        .ok_or_else(|| refuse("INTERFACE", ErrorKind::Missing))?;
    OpCode::read(igz, *iface)?
        .block_end(p + 1)
        .filter(|&e| e < ops.len())
        .ok_or_else(|| refuse("INTERFACE", ErrorKind::Missing))
}

/// One step of an operand's path, keyed so two paths to the same cell are
/// equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// A `global.bld` object, by its index there.
    Global(usize),
    /// An engine member (`setvariant.count`).
    Engine(u32, u32),
    /// A macro parameter, by name.
    Param(String),
    /// A variable or structure field, by name.
    Var(String),
    /// A loop's index, by the `loop value` op.
    Loop(usize),
}

type Path = Vec<Key>;

fn engine(namespace: &str, member: &str) -> Key {
    Key::Engine(fnv1a32(namespace.as_bytes()), fnv1a32(member.as_bytes()))
}

fn count() -> Key {
    engine("setvariant", "count")
}

fn last() -> Key {
    engine("setvariant", "last")
}

/// A loop being stepped.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The `loop value` op and its place.
    op: usize,
    at: usize,
    /// Its closing `flow loop`.
    end: usize,
    pass: i64,
    passes: i64,
}

/// What stepping the body has built and written.
#[derive(Debug, Default)]
struct Run {
    /// Cells the body wrote with a value the reader follows.
    values: HashMap<Path, i64>,
    /// Paths written with what it doesn't follow: anything under one reads
    /// as unknown.
    unknown: Vec<Path>,
    loops: Vec<Frame>,
    /// The level table's set and its entries.
    table: Option<(Path, Vec<u32>)>,
    rows: Vec<SaveParam>,
    steps: usize,
}

impl Run {
    fn write(&mut self, path: Path, value: Option<i64>) {
        match value {
            Some(v) => {
                self.values.insert(path, v);
            }
            None => {
                self.values.retain(|p, _| !p.starts_with(&path));
                self.unknown.push(path);
            }
        }
    }
}

/// A call's argument: its parameter's name, value and index operands.
struct Argument<'a> {
    name: String,
    rhs: ObjectRef<'a>,
    index: ObjectRef<'a>,
}

struct Reader<'f, 'a, 'g> {
    igz: &'f Igz<'a>,
    global: &'f Igz<'g>,
    exports: &'f Exports,
    defs: Defs,
    /// The plain setup macro's body: each field it sets on the save param
    /// and the parameter it copies there.
    copies: Vec<(String, String)>,
    /// The level-up definition's table set and the count its body sizes it to.
    table_set: (Path, i64),
}

impl<'f, 'a, 'g> Reader<'f, 'a, 'g> {
    fn new(
        igz: &'f Igz<'a>,
        global: &'f Igz<'g>,
        exports: &'f Exports,
        defs: Defs,
    ) -> Result<Self, FormatError> {
        let mut reader = Self {
            igz,
            global,
            exports,
            defs,
            copies: Vec::new(),
            table_set: (Vec::new(), 0),
        };
        reader.copies = reader.setup_copies()?;
        reader.table_set = reader.levelup_set()?;
        Ok(reader)
    }

    fn type_of(&self, object: usize) -> &'a str {
        type_of(self.igz, object)
    }

    /// "Save - Setup Portal Save Param"'s body: every op an `OpSetValue` of
    /// `save param . <field>` from parameter `<field>`.
    fn setup_copies(&self) -> Result<Vec<(String, String)>, FormatError> {
        let (ops, from, to) = self.defs.body(self.igz, SETUP_PARAM)?;
        let shape = || refuse(SETUP_PARAM, ErrorKind::Unsupported { value: 0 });
        let mut copies = Vec::new();
        for &op in &ops[from..=to] {
            match self.type_of(op) {
                "OpFlowBuiltInBehavior" => {}
                t if t == OpSetValue::TYPE => {
                    let set = OpSetValue::read(self.igz, op)?;
                    let lhs = self.operand_path(set.lhs)?;
                    let rhs = self.operand_path(set.rhs)?;
                    match (&lhs[..], &rhs[..]) {
                        ([Key::Param(p), Key::Var(field)], [Key::Param(from)])
                            if p == params::SAVE_PARAM && params::ALL.contains(&from.as_str()) =>
                        {
                            copies.push((field.clone(), from.clone()));
                        }
                        _ => return Err(shape()),
                    }
                }
                _ => return Err(stranger(self.igz, op, SETUP_PARAM)),
            }
        }
        if copies.is_empty() {
            return Err(shape());
        }
        Ok(copies)
    }

    /// The level-up definition's one `set <table> . setvariant.count = L`:
    /// the table's set and its length.
    fn levelup_set(&self) -> Result<(Path, i64), FormatError> {
        let (ops, from, to) = self.defs.body(self.igz, DEFINE_LEVELUP)?;
        let mut found = Vec::new();
        for &op in &ops[from..=to] {
            if self.type_of(op) != OpSetValue::TYPE {
                continue;
            }
            let set = OpSetValue::read(self.igz, op)?;
            let mut lhs = self.operand_path(set.lhs)?;
            if lhs.last() != Some(&count()) {
                continue;
            }
            lhs.pop();
            let len = int_literal(self.igz, set.rhs)?
                .ok_or_else(|| refuse(DEFINE_LEVELUP, ErrorKind::Unsupported { value: 0 }))?;
            found.push((lhs, i64::from(len)));
        }
        match found.len() {
            1 => {}
            0 => return Err(refuse(DEFINE_LEVELUP, ErrorKind::Missing)),
            _ => return Err(refuse(DEFINE_LEVELUP, ErrorKind::Duplicate)),
        }
        let (set, len) = found.remove(0);
        if !(2..=MAX_LEVELS).contains(&len) {
            return Err(refuse(
                DEFINE_LEVELUP,
                ErrorKind::OutOfRange {
                    value: len as u64,
                    limit: MAX_LEVELS as u64,
                },
            ));
        }
        Ok((set, len))
    }

    /// A stack operand's path, keyed, through any `ValueRHSVariant`s with no
    /// operator around it.
    fn operand_path(&self, operand: ObjectRef<'_>) -> Result<Path, FormatError> {
        let ObjectRef::Object(o) = operand else {
            return Err(refuse("operand", ErrorKind::Missing));
        };
        let o = unwrapped(self.igz, o)?;
        if !is_a(self.type_of(o), Stack::TYPE) {
            return Err(stranger(self.igz, o, Stack::TYPE));
        }
        self.path(&Stack::read(self.igz, o)?.path)
    }

    fn path(&self, elements: &[ObjectRef<'_>]) -> Result<Path, FormatError> {
        elements.iter().map(|e| self.key(e)).collect()
    }

    fn key(&self, element: &ObjectRef<'_>) -> Result<Key, FormatError> {
        match *element {
            ObjectRef::Import(i) => match self.exports.resolve_import(&i.id) {
                None => Ok(Key::Engine(i.id.namespace, i.id.object)),
                Some(Ok(Export::Global(Root::Object(o)))) => Ok(Key::Global(o)),
                Some(Ok(_)) => Err(refuse(
                    "import",
                    ErrorKind::Unsupported {
                        value: i.id.object.into(),
                    },
                )),
                Some(Err(e)) => Err(e),
            },
            ObjectRef::Object(o) => {
                let ty = self.type_of(o);
                if ty == OpLoopValue::TYPE {
                    Ok(Key::Loop(o))
                } else if is_a(ty, OpMacroParameter::TYPE) {
                    let p = OpMacroParameter::read(self.igz, o)?;
                    Ok(Key::Param(p.var.name.unwrap_or("").to_owned()))
                } else if is_a(ty, Variable::TYPE) {
                    let v = Variable::read(self.igz, o)?;
                    Ok(Key::Var(v.name.unwrap_or("").to_owned()))
                } else {
                    Err(stranger(self.igz, o, "a variable, parameter or loop"))
                }
            }
            ObjectRef::Null | ObjectRef::Named { .. } => {
                Err(refuse("path", ErrorKind::Unsupported { value: 0 }))
            }
        }
    }

    fn global_type(&self, object: usize) -> &'g str {
        type_of(self.global, object)
    }

    /// A set of `global.bld`'s members as the disc lists them.
    fn members(&self, set: usize) -> Result<Vec<ObjectRef<'g>>, FormatError> {
        match ScriptSet::read(self.global, set)?.list {
            Some(list) => Ok(ScriptSetList::read(self.global, list)?.elements),
            None => Ok(Vec::new()),
        }
    }

    /// What a cell holds at this point of the body.
    fn read(&self, run: &Run, path: &Path) -> Result<i64, FormatError> {
        if let Some(&v) = run.values.get(path) {
            return Ok(v);
        }
        let unknown = || refuse("path", ErrorKind::Unsupported { value: 0 });
        if run.unknown.iter().any(|p| path.starts_with(p)) {
            return Err(unknown());
        }
        match &path[..] {
            [Key::Loop(op)] => run
                .loops
                .iter()
                .rev()
                .find(|f| f.op == *op)
                .map(|f| f.pass)
                .ok_or_else(|| refuse("loop index", ErrorKind::Missing)),
            [Key::Global(o)] if is_a(self.global_type(*o), ValueInfo::TYPE) => {
                let v = ValueInfo::read(self.global, *o)?;
                if v.value_type.metaobject() != Some("IntMeasurement") {
                    return Err(refuse("_type", ErrorKind::Unsupported { value: *o as u64 }));
                }
                Ok(i64::from(v.value))
            }
            [Key::Global(o), k] if *k == count() && is_a(self.global_type(*o), ScriptSet::TYPE) => {
                Ok(self.members(*o)?.len() as i64)
            }
            [set @ .., k] if *k == last() || *k == count() => match &run.table {
                Some((table, entries)) if table[..] == *set => Ok(if *k == count() {
                    entries.len() as i64
                } else {
                    entries.last().copied().map_or(0, i64::from)
                }),
                _ => Err(unknown()),
            },
            _ => Err(unknown()),
        }
    }

    /// An operand's value: a literal, a cell, or arithmetic over them.
    fn eval(&self, run: &Run, operand: ObjectRef<'_>, depth: usize) -> Result<i64, FormatError> {
        let ObjectRef::Object(o) = operand else {
            return Err(refuse("operand", ErrorKind::Missing));
        };
        let ty = self.type_of(o);
        if is_a(ty, ValueRhsVariant::TYPE) {
            if depth == MAX_DEPTH {
                return Err(refuse(
                    "_varOp1",
                    ErrorKind::OutOfRange {
                        value: depth as u64 + 1,
                        limit: MAX_DEPTH as u64,
                    },
                ));
            }
            let v = ValueRhsVariant::read(self.igz, o)?;
            if v.conditioner != ObjectRef::Null {
                return Err(refuse("_conditioner", ErrorKind::Unsupported { value: 0 }));
            }
            let a = self.eval(run, v.op1, depth + 1)?;
            if v.operator == NO_OPERATOR {
                return Ok(a);
            }
            let b = self.eval(run, v.op2, depth + 1)?;
            let out = match v.operator {
                0 => a.checked_add(b),
                1 => a.checked_sub(b),
                2 => a.checked_mul(b),
                3 => a.checked_div(b),
                _ => None,
            };
            return out.ok_or_else(|| {
                refuse(
                    "_arithOperator",
                    ErrorKind::Unsupported {
                        value: v.operator.into(),
                    },
                )
            });
        }
        if !is_a(ty, RhsValueStack::TYPE) {
            // A stack with no literal fields reads its path.
            if !is_a(ty, Stack::TYPE) {
                return Err(stranger(self.igz, o, RhsValueStack::TYPE));
            }
            let path = self.path(&Stack::read(self.igz, o)?.path)?;
            return self.read(run, &path);
        }
        let s = RhsValueStack::read(self.igz, o)?;
        if s.is_random {
            return Err(refuse("_isRandom", ErrorKind::Unsupported { value: 1 }));
        }
        if s.stack.path.is_empty() {
            return match s.value_type.metaobject() {
                Some("IntMeasurement") => Ok(i64::from(s.value)),
                _ => Err(refuse("_type", ErrorKind::Unsupported { value: o as u64 })),
            };
        }
        let path = self.path(&s.stack.path)?;
        self.read(run, &path)
    }

    /// Runs the body's ops in order, taking each branch and loop as the
    /// values it follows say, and stops at the first refusal.
    fn step(
        &self,
        ops: &[usize],
        from: usize,
        to: usize,
        run: &mut Run,
    ) -> Result<(), FormatError> {
        let mut pc = from;
        while pc <= to {
            run.steps += 1;
            if run.steps > MAX_STEPS {
                return Err(refuse(
                    SAVE_TABLE,
                    ErrorKind::OutOfRange {
                        value: run.steps as u64,
                        limit: MAX_STEPS as u64,
                    },
                ));
            }
            let op = ops[pc];
            // AH.13's decoder: the class picks the arm, the layout comes read.
            let next = match Op::read(self.igz, op)? {
                Op::FlowContinue(_) => pc + 1,
                Op::UseMacro(_) => self.call(ops, pc, run)?,
                Op::SetValue(set) => {
                    let lhs = self.operand_path(set.lhs)?;
                    let value = self.eval(run, set.rhs, 0).ok();
                    run.write(lhs, value);
                    pc + 1
                }
                Op::IncValue(inc) => {
                    let lhs = self.operand_path(inc.lhs)?;
                    let value = self.read(run, &lhs).ok().and_then(|v| v.checked_add(1));
                    run.write(lhs, value);
                    pc + 1
                }
                Op::SetReference(set) => {
                    let lhs = self.operand_path(set.lhs)?;
                    run.write(lhs, None);
                    pc + 1
                }
                Op::ChangeMembership(change) => {
                    let lhs = self.operand_path(change.lhs)?;
                    run.write(lhs, None);
                    pc + 1
                }
                Op::CheckValue(check) => {
                    let a = self.eval(run, check.lhs, 0)?;
                    let b = self.eval(run, check.rhs, 0)?;
                    if holds(check.relation, a, b)? {
                        pc + 1
                    } else {
                        check.op.block_end(pc).unwrap_or(pc) + 1
                    }
                }
                // `else/skip n`: the branch taken skips the else block.
                Op::Skip(code) => {
                    if code.branch == BRANCH_END {
                        return Err(stranger(self.igz, op, "an op of the save-param table"));
                    }
                    pc + usize::from(code.branch) + 1
                }
                Op::LoopValue(looped) => {
                    let end = looped
                        .op
                        .block_end(pc)
                        .filter(|&e| e <= to && self.type_of(ops[e]) == "OpFlowLoop")
                        .ok_or_else(|| refuse("loop value", ErrorKind::Missing))?;
                    let passes = self.eval(run, looped.rhs, 0)?;
                    if passes > MAX_PASSES {
                        return Err(refuse(
                            "loop value",
                            ErrorKind::OutOfRange {
                                value: passes as u64,
                                limit: MAX_PASSES as u64,
                            },
                        ));
                    }
                    if passes <= 0 {
                        end + 1
                    } else {
                        run.loops.push(Frame {
                            op,
                            at: pc,
                            end,
                            pass: 0,
                            passes,
                        });
                        pc + 1
                    }
                }
                Op::FlowLoop(_) => {
                    let frame = run
                        .loops
                        .last_mut()
                        .filter(|f| f.end == pc)
                        .ok_or_else(|| stranger(self.igz, op, "the end of a loop"))?;
                    frame.pass += 1;
                    if frame.pass < frame.passes {
                        frame.at + 1
                    } else {
                        run.loops.pop();
                        pc + 1
                    }
                }
                _ => return Err(stranger(self.igz, op, "an op of the save-param table")),
            };
            if run.loops.last().is_some_and(|f| next > f.end) {
                return Err(refuse(
                    "loop value",
                    ErrorKind::Unsupported { value: next as u64 },
                ));
            }
            pc = next;
        }
        if !run.loops.is_empty() {
            return Err(refuse("loop value", ErrorKind::Missing));
        }
        Ok(())
    }

    /// The macro a call makes: its definition object and name.
    fn target(&self, call: &OpUseMacro<'_>) -> Result<(usize, &'a str), FormatError> {
        let np = call.np.ok_or_else(|| refuse("_NP", ErrorKind::Missing))?;
        let def = match Stack::read(self.igz, np)?.path[..] {
            [ObjectRef::Object(def)] if is_a(self.type_of(def), OpDefineMacro::TYPE) => def,
            _ => return Err(refuse("_NP", ErrorKind::Unsupported { value: np as u64 })),
        };
        let name = OpDefineMacro::read(self.igz, def)?.op.name.unwrap_or("");
        Ok((def, name))
    }

    /// What a call to `def`, named `name`, is in the table.
    fn kind(&self, def: usize, name: &str) -> Result<Option<SaveKind>, FormatError> {
        if name == SETUP_PARAM {
            return Ok(Some(SaveKind::Plain));
        }
        if self.type_of(def) != OpDefineMacroSpecialization::TYPE {
            return Ok(None);
        }
        let spec = OpDefineMacroSpecialization::read(self.igz, def)?;
        let base = spec
            .np
            .map(|np| Stack::read(self.igz, np))
            .transpose()?
            .and_then(|s| match s.path[..] {
                [ObjectRef::Object(b)] => Some(b),
                _ => None,
            });
        let Some(base) = base.filter(|&b| is_a(self.type_of(b), OpDefineMacro::TYPE)) else {
            return Ok(None);
        };
        if OpDefineMacro::read(self.igz, base)?.op.name != Some(SETUP_PARAM) {
            return Ok(None);
        }
        match name {
            SPILLOVER => Ok(Some(SaveKind::Spillover)),
            MIRROR_AND_CLEAR => Ok(Some(SaveKind::MirrorAndClear)),
            EXTENDED_BITFIELD => Ok(Some(SaveKind::ExtendedBitfield)),
            _ => Err(refuse(
                SETUP_PARAM,
                ErrorKind::Unsupported { value: def as u64 },
            )),
        }
    }

    /// A call's arguments, each name at most once and one of `allowed`
    /// (any `level xp (k)` when `allowed` is the level-up's).
    fn arguments(
        &self,
        ops: &[usize],
        p: usize,
        allowed: &dyn Fn(&str) -> bool,
    ) -> Result<(Vec<Argument<'a>>, usize), FormatError> {
        let iface_end = interface_end(self.igz, ops, p)?;
        let mut out: Vec<Argument<'a>> = Vec::new();
        for &op in &ops[p + 2..=iface_end] {
            match self.type_of(op) {
                "OpFlowMacro" => {}
                t if is_a(t, OpMacroParameter::TYPE) => {
                    let param = OpMacroParameter::read(self.igz, op)?;
                    let name = param.var.name.unwrap_or("");
                    if !allowed(name) || out.iter().any(|a| a.name == name) {
                        return Err(refuse(
                            "_varName",
                            ErrorKind::Unsupported { value: op as u64 },
                        ));
                    }
                    out.push(Argument {
                        name: name.to_owned(),
                        rhs: param.rhs,
                        index: param.index_rhs,
                    });
                }
                _ => return Err(stranger(self.igz, op, OpMacroParameter::TYPE)),
            }
        }
        Ok((out, iface_end))
    }

    /// Reads the call at `p` into the table; returns where stepping goes on:
    /// the caller's ops after the INTERFACE block.
    fn call(&self, ops: &[usize], p: usize, run: &mut Run) -> Result<usize, FormatError> {
        let call = OpUseMacro::read(self.igz, ops[p])?;
        if call.index_rhs != ObjectRef::Null {
            return Err(refuse(
                "_indexRHS",
                ErrorKind::Unsupported { value: p as u64 },
            ));
        }
        let (def, name) = self.target(&call)?;
        if name == DEFINE_LEVELUP {
            let (args, iface_end) = self.arguments(ops, p, &|n| {
                n == params::HP_MULT || level_xp_index(n).is_some()
            })?;
            self.levelup(run, &args)?;
            return Ok(iface_end + 1);
        }
        let Some(kind) = self.kind(def, name)? else {
            return Err(stranger(self.igz, ops[p], SETUP_PARAM));
        };
        let (args, iface_end) = self.arguments(ops, p, &|n| params::ALL.contains(&n))?;
        self.row(run, p, kind, &args)?;
        Ok(iface_end + 1)
    }

    /// The level-up call: `level xp (k)` fills entry k of a table its
    /// definition sizes; entry 0 stays 0.
    fn levelup(&self, run: &mut Run, args: &[Argument<'_>]) -> Result<(), FormatError> {
        if run.table.is_some() {
            return Err(refuse(DEFINE_LEVELUP, ErrorKind::Duplicate));
        }
        let (set, len) = &self.table_set;
        let mut table = vec![0u32; *len as usize];
        for a in args {
            let Some(k) = level_xp_index(&a.name) else {
                continue;
            };
            let out_of_range = |value: u64| {
                refuse(
                    params::LEVEL_XP,
                    ErrorKind::OutOfRange {
                        value,
                        limit: *len as u64,
                    },
                )
            };
            if k == 0 || k >= table.len() {
                return Err(out_of_range(k as u64));
            }
            if a.index != ObjectRef::Null {
                return Err(refuse(
                    "_indexRHS",
                    ErrorKind::Unsupported { value: k as u64 },
                ));
            }
            let xp = self.eval(run, a.rhs, 0)?;
            table[k] = u32::try_from(xp).map_err(|_| out_of_range(xp as u64))?;
        }
        // An omitted entry reads 0, which this refuses with any other order.
        if let Some(w) = table.windows(2).position(|w| w[1] <= w[0]) {
            return Err(refuse(
                params::LEVEL_XP,
                ErrorKind::OutOfRange {
                    value: u64::from(table[w + 1]),
                    limit: u64::from(table[w]),
                },
            ));
        }
        run.table = Some((set.clone(), table));
        Ok(())
    }

    /// A setup call: one more field of the row at `position`.
    fn row(
        &self,
        run: &mut Run,
        position: usize,
        kind: SaveKind,
        args: &[Argument<'_>],
    ) -> Result<(), FormatError> {
        let get = |n: &str| args.iter().find(|a| a.name == n);
        let save = get(params::SAVE_PARAM)
            .ok_or_else(|| refuse(params::SAVE_PARAM, ErrorKind::Missing))?;
        let (param, object, member, indexed) = self.save_param(run, save)?;
        // An omitted value parameter reads 0 (aj-vm).
        let value = |n: &'static str| -> Result<u32, FormatError> {
            let Some(a) = get(n) else { return Ok(0) };
            if a.index != ObjectRef::Null {
                return Err(refuse(n, ErrorKind::Unsupported { value: 0 }));
            }
            let v = self.eval(run, a.rhs, 0)?;
            u32::try_from(v).map_err(|_| {
                refuse(
                    n,
                    ErrorKind::OutOfRange {
                        value: v as u64,
                        limit: u32::MAX.into(),
                    },
                )
            })
        };
        let field = SaveField {
            word: value(params::WORD)?,
            bit: value(params::BIT)?,
            member,
        };
        let (width, bits_per_field, limit) = (
            value(params::WIDTH)?,
            value(params::BITS_PER_FIELD)?,
            value(params::LIMIT)?,
        );
        // The plain macro copies its arguments into the param's own fields,
        // which later calls read (a spillover's limit).
        if kind == SaveKind::Plain && !indexed {
            for (f, from) in &self.copies {
                let v = match from.as_str() {
                    params::WORD => field.word,
                    params::BIT => field.bit,
                    params::WIDTH => width,
                    params::BITS_PER_FIELD => bits_per_field,
                    params::LIMIT => limit,
                    _ => continue,
                };
                run.write(
                    vec![Key::Global(object), Key::Var(f.clone())],
                    Some(v.into()),
                );
            }
        }
        if let Some(row) = run.rows.iter_mut().find(|r| r.position == position) {
            if (
                row.kind,
                row.width,
                row.bits_per_field,
                row.limit,
                row.param.as_str(),
            ) != (kind, width, bits_per_field, limit, param.as_str())
            {
                return Err(refuse(
                    SETUP_PARAM,
                    ErrorKind::Unsupported {
                        value: position as u64,
                    },
                ));
            }
            row.fields.push(field);
        } else {
            run.rows.push(SaveParam {
                position,
                param,
                kind,
                fields: vec![field],
                width,
                bits_per_field,
                limit,
            });
        }
        Ok(())
    }

    /// The save param a call names: a structure of `global.bld`, or a set of
    /// them with an index. Its name, object, the member named and whether
    /// it was indexed.
    fn save_param(
        &self,
        run: &Run,
        arg: &Argument<'_>,
    ) -> Result<(String, usize, Option<String>, bool), FormatError> {
        let field = params::SAVE_PARAM;
        let ObjectRef::Object(o) = arg.rhs else {
            return Err(refuse(field, ErrorKind::Missing));
        };
        let path = self.operand_path(arg.rhs)?;
        let [Key::Global(g)] = path[..] else {
            return Err(refuse(field, ErrorKind::Unsupported { value: o as u64 }));
        };
        let ty = self.global_type(g);
        match arg.index {
            ObjectRef::Null if is_a(ty, ScriptStructure::TYPE) => {
                let name = ScriptStructure::read(self.global, g)?.name.unwrap_or("");
                Ok((name.to_owned(), g, None, false))
            }
            ObjectRef::Object(_) if is_a(ty, ScriptSet::TYPE) => {
                let set = ScriptSet::read(self.global, g)?
                    .name
                    .unwrap_or("")
                    .to_owned();
                let i = self.eval(run, arg.index, 0)?;
                let i = usize::try_from(i).map_err(|_| {
                    refuse(
                        "_indexRHS",
                        ErrorKind::OutOfRange {
                            value: i as u64,
                            limit: 0,
                        },
                    )
                })?;
                let member = match self.members(g)?.get(i) {
                    Some(&ObjectRef::Object(m))
                        if is_a(self.global_type(m), ScriptStructure::TYPE) =>
                    {
                        Some(
                            ScriptStructure::read(self.global, m)?
                                .name
                                .unwrap_or("")
                                .to_owned(),
                        )
                    }
                    Some(_) => {
                        return Err(refuse(
                            "element",
                            ErrorKind::Unsupported { value: i as u64 },
                        ));
                    }
                    None => None,
                };
                Ok((set, g, member, true))
            }
            _ => Err(stranger(
                self.global,
                g,
                "a ScriptStructure, or a ScriptSet with an index",
            )),
        }
    }
}

/// `k` of a `level xp (k)` parameter name.
fn level_xp_index(name: &str) -> Option<usize> {
    name.strip_prefix(params::LEVEL_XP)?
        .strip_prefix(" (")?
        .strip_suffix(')')?
        .parse()
        .ok()
}

/// Whether `a <code> b` holds.
fn holds(code: u32, a: i64, b: i64) -> Result<bool, FormatError> {
    Ok(match code {
        0 => a <= b,
        1 => a == b,
        2 => a >= b,
        3 => a < b,
        4 => a > b,
        5 => a != b,
        _ => {
            return Err(refuse(
                "_relOperator",
                ErrorKind::Unsupported { value: code.into() },
            ));
        }
    })
}

#[cfg(test)]
mod tests;
