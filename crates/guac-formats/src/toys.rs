//! Giants' toy table, read statically (openspec AE.2, design decision 1).
//!
//! `character/init_setup.bld`'s `Init_Setup_Minions.ai` builds the table the
//! portal looks a tag up in by calling three macros: "Portal - Define Object
//! Level Load" once per toy type (77 calls on the 01.00 disc, DEBUG 999 among
//! them), "Portal - Define Legendary Alt Level Load" once per alternate deco
//! (15) and "Portal - Define Alt Character" once per Spyro's Adventure toy
//! that is its own toy type but another's variant (5). This reader takes
//! each call's arguments, follows each to its value (a literal, a
//! `global.bld` `StringInfo` or `ValueInfo`, `Minion_Names` at an index) and
//! applies what the macro bodies do with them: which sets a toy's flags put
//! it in, the sets the caller's after-return code adds it to, which toy an
//! alt belongs to (the one defined last, `toy level loads . last`) and the
//! level loads the LightCore loop adds to `display as lightcore`.
//!
//! Replaying three macro bodies by hand is a narrow rewrite of script logic,
//! which the Method avoids; chotchki took it on 2026-10-18 ("that's fine")
//! until AJ's VM runs the table itself and must reproduce this reader's
//! output. It refuses what it doesn't model rather than guess: an argument
//! of any other shape, an unknown parameter, an op in an after-return block
//! other than the calls' own and set additions.

use crate::exports::{Export, Exports, Root};
use crate::igz::layouts::{
    Layout, NO_OPERATOR, ObjectRef, OpChangeMembership, OpCode, OpCodeList, OpDefineMacro,
    OpLoopValue, OpMacroParameter, OpUseMacro, RhsValueStack, ScriptInfo, ScriptSet, ScriptSetList,
    Stack, StringInfo, ValueInfo, ValueRhsVariant, Variable, is_a,
};
use crate::igz::{ExternalId, Igz};
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "toy table";

/// The script, by the last part of its source path.
pub const SCRIPT: &str = "Init_Setup_Minions.ai";
pub const DEFINE_TOY: &str = "Portal - Define Object Level Load";
pub const DEFINE_ALT: &str = "Portal - Define Legendary Alt Level Load";
pub const DEFINE_ALT_CHARACTER: &str = "Portal - Define Alt Character";
/// The set the LightCore loop fills.
pub const LIGHTCORE_SET: &str = "display as lightcore";
/// The DEBUG toy, which the table defines and no figure is.
pub const DEBUG_TOY: i32 = 999;

/// A text key: a `StringInfo` object of `global.bld`.
pub type Key = usize;

/// The sets "Define Object Level Load" puts a toy in, by its flags.
pub mod sets {
    pub const MAGIC_ITEM: &str = "magic item level loads";
    pub const CHARACTER: &str = "character level loads";
    pub const SKY1_CHARACTER: &str = "sky1 character loads";
    pub const REPOSE: &str = "sky1 characters with repose";
    pub const GIANT: &str = "giant loads";
    /// Filled by the caller's after-return code, not the flags.
    pub const PET: &str = "pet level loads";
}

/// One "Define Object Level Load" call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toy {
    /// The call's place in the op list.
    pub position: usize,
    /// The level load key; its text is the file the toy loads.
    pub level_load: Key,
    pub name: Key,
    /// The toy type (the tag's toy id).
    pub item_code: i32,
    pub magic_item: bool,
    pub sky1: bool,
    pub known_repose: bool,
    pub giant: bool,
    /// 0 Magic, 1 Water, 2 Air, 3 Undead, 4 Tech, 5 Fire, 6 Earth, 7 Life.
    pub element: i32,
    /// Sets the caller's after-return code adds the toy to.
    pub added_to: Vec<String>,
}

impl Toy {
    /// Every set the toy is in: the flags' (as the macro body has it), then
    /// the after-return additions.
    pub fn sets(&self) -> Vec<&str> {
        let mut out = vec![if self.magic_item {
            sets::MAGIC_ITEM
        } else {
            sets::CHARACTER
        }];
        if self.sky1 && !self.magic_item {
            out.push(sets::SKY1_CHARACTER);
        }
        if self.known_repose {
            out.push(sets::REPOSE);
        }
        if self.giant {
            out.push(sets::GIANT);
        }
        out.extend(self.added_to.iter().map(String::as_str));
        out
    }

    pub fn in_set(&self, set: &str) -> bool {
        self.sets().contains(&set)
    }
}

/// One "Define Legendary Alt Level Load" call: an alternate deco of `toy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alt {
    pub position: usize,
    /// Index into [`ToyTable::toys`]: the toy defined last before the call.
    pub toy: usize,
    pub level_load: Key,
    /// The deco id the tag's subtype word carries in bits 0-7.
    pub deco: i32,
    /// The variant word; `None` (a null argument) reads "Legendary".
    pub display: Option<Key>,
}

/// One "Define Alt Character" call: toy `alt` is toy `normal`'s variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AltCharacter {
    pub position: usize,
    /// Indexes into [`ToyTable::toys`].
    pub normal: usize,
    pub alt: usize,
    /// The variant word (Dark, Legendary).
    pub prefix: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToyTable {
    /// In call order, DEBUG included.
    pub toys: Vec<Toy>,
    pub alts: Vec<Alt>,
    pub alt_characters: Vec<AltCharacter>,
    /// The level loads the LightCore loop adds to [`LIGHTCORE_SET`].
    pub lightcore_loads: Vec<Key>,
}

impl ToyTable {
    pub fn is_lightcore(&self, toy: &Toy) -> bool {
        self.lightcore_loads.contains(&toy.level_load)
    }

    /// The toy defining level load `key`.
    pub fn toy_by_load(&self, key: Key) -> Option<usize> {
        self.toys.iter().position(|t| t.level_load == key)
    }

    /// Reads the table out of `init_setup`'s `level.bld`, following imports
    /// into `global` (`global.bld`'s `level.bld`) through `exports`.
    pub fn read(
        init_setup: &Igz<'_>,
        global: &Igz<'_>,
        exports: &Exports,
    ) -> Result<Self, FormatError> {
        Reader {
            igz: init_setup,
            global,
            exports,
        }
        .table()
    }
}

/// What an argument is, followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arg {
    Int(i32),
    Key(Key),
    /// `abstractscriptgroup.null`.
    Null,
}

fn refuse(field: &'static str, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, 0, kind)
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

struct Reader<'f, 'a, 'g> {
    igz: &'f Igz<'a>,
    global: &'f Igz<'g>,
    exports: &'f Exports,
}

impl Reader<'_, '_, '_> {
    fn type_of(&self, object: usize) -> &str {
        self.igz.type_name(&self.igz.objects()[object])
    }

    /// The op list of [`SCRIPT`].
    fn ops(&self) -> Result<Vec<usize>, FormatError> {
        let mut found = None;
        for (i, _) in crate::igz::layouts::read_all::<ScriptInfo>(self.igz) {
            let script = ScriptInfo::read(self.igz, i)?;
            let name = script.info.name.unwrap_or("");
            if name.rsplit(['/', '\\']).next() == Some(SCRIPT) {
                if found.is_some() {
                    return Err(refuse("tfbScriptInfo", ErrorKind::Duplicate));
                }
                found = script.op_list;
            }
        }
        let list = found.ok_or_else(|| refuse("tfbScriptInfo", ErrorKind::Missing))?;
        Ok(OpCodeList::read(self.igz, list)?.ops)
    }

    /// The macro an `OpUseMacro` calls, by name; `None` for one named by
    /// anything but a local definition.
    fn macro_name(&self, call: &OpUseMacro<'_>) -> Result<Option<String>, FormatError> {
        let Some(np) = call.np else { return Ok(None) };
        let stack = Stack::read(self.igz, np)?;
        let [ObjectRef::Object(def)] = stack.path[..] else {
            return Ok(None);
        };
        if !is_a(self.type_of(def), OpDefineMacro::TYPE) {
            return Ok(None);
        }
        Ok(OpDefineMacro::read(self.igz, def)?
            .op
            .name
            .map(str::to_owned))
    }

    /// The literal an index operand holds.
    fn index(&self, index: ObjectRef<'_>) -> Result<Option<i32>, FormatError> {
        match index {
            ObjectRef::Null => Ok(None),
            ObjectRef::Object(o) => match self.value(o)? {
                Arg::Int(i) => Ok(Some(i)),
                _ => Err(refuse(
                    "_indexRHS",
                    ErrorKind::Unsupported { value: o as u64 },
                )),
            },
            _ => Err(refuse("_indexRHS", ErrorKind::Missing)),
        }
    }

    /// A value operand, followed: a literal, an import of a `global.bld`
    /// `ValueInfo` or `StringInfo`, `abstractscriptgroup.null`, or a
    /// `ValueRHSVariant` with no operator around one of those.
    fn value(&self, object: usize) -> Result<Arg, FormatError> {
        self.argument(object, None)
    }

    fn argument(&self, object: usize, index: Option<i32>) -> Result<Arg, FormatError> {
        let ty = self.type_of(object);
        if is_a(ty, ValueRhsVariant::TYPE) {
            let v = ValueRhsVariant::read(self.igz, object)?;
            let ObjectRef::Object(inner) = v.op1 else {
                return Err(refuse("_varOp1", ErrorKind::Missing));
            };
            if v.operator != NO_OPERATOR {
                return Err(refuse(
                    "_arithOperator",
                    ErrorKind::Unsupported {
                        value: v.operator.into(),
                    },
                ));
            }
            return self.argument(inner, index);
        }
        if !is_a(ty, RhsValueStack::TYPE) {
            return Err(stranger(self.igz, object, RhsValueStack::TYPE));
        }
        let stack = RhsValueStack::read(self.igz, object)?;
        match stack.stack.path[..] {
            [] => {
                if index.is_some() {
                    return Err(refuse("_indexRHS", ErrorKind::Unsupported { value: 0 }));
                }
                if stack.value_type.metaobject() != Some("IntMeasurement") {
                    return Err(refuse("_type", ErrorKind::Unsupported { value: 0 }));
                }
                Ok(Arg::Int(stack.value))
            }
            [ObjectRef::Import(import)] => self.import(&import.id, index),
            _ => Err(refuse(
                "path",
                ErrorKind::Unsupported {
                    value: stack.stack.path.len() as u64,
                },
            )),
        }
    }

    fn global_object(&self, id: &ExternalId) -> Result<usize, FormatError> {
        let export = self.exports.resolve_import(id).ok_or_else(|| {
            refuse(
                "import",
                ErrorKind::Unsupported {
                    value: id.namespace.into(),
                },
            )
        })??;
        match export {
            Export::Global(Root::Object(o)) => Ok(o),
            _ => Err(refuse(
                "import",
                ErrorKind::Unsupported {
                    value: id.object.into(),
                },
            )),
        }
    }

    fn import(&self, id: &ExternalId, index: Option<i32>) -> Result<Arg, FormatError> {
        if id.is("abstractscriptgroup", "null") {
            return Ok(Arg::Null);
        }
        let o = self.global_object(id)?;
        let ty = self.global.type_name(&self.global.objects()[o]);
        match (ty, index) {
            (t, None) if t == StringInfo::TYPE => Ok(Arg::Key(o)),
            (t, None) if is_a(t, ValueInfo::TYPE) => {
                let v = ValueInfo::read(self.global, o)?;
                if v.value_type.metaobject() != Some("IntMeasurement") {
                    return Err(refuse("_type", ErrorKind::Unsupported { value: o as u64 }));
                }
                Ok(Arg::Int(v.value))
            }
            (t, Some(i)) if is_a(t, ScriptSet::TYPE) => {
                let set = ScriptSet::read(self.global, o)?;
                let list = set
                    .list
                    .ok_or_else(|| refuse("_list", ErrorKind::Missing))?;
                let members = ScriptSetList::read(self.global, list)?.elements;
                let member = usize::try_from(i)
                    .ok()
                    .and_then(|i| members.get(i))
                    .ok_or_else(|| {
                        refuse(
                            "_indexRHS",
                            ErrorKind::OutOfRange {
                                value: i as i64 as u64,
                                limit: members.len() as u64,
                            },
                        )
                    })?;
                match *member {
                    ObjectRef::Object(k)
                        if self.global.type_name(&self.global.objects()[k]) == StringInfo::TYPE =>
                    {
                        Ok(Arg::Key(k))
                    }
                    _ => Err(refuse(
                        "element",
                        ErrorKind::Unsupported { value: i as u64 },
                    )),
                }
            }
            _ => Err(stranger(
                self.global,
                o,
                "StringInfo, ValueInfo or ScriptSet",
            )),
        }
    }

    /// A call's arguments by parameter name, every name one of `allowed`.
    fn arguments(
        &self,
        ops: &[usize],
        call_at: usize,
        allowed: &[&str],
    ) -> Result<(Vec<(String, Arg)>, usize), FormatError> {
        let iface_at = call_at + 1;
        let iface = ops
            .get(iface_at)
            .filter(|&&o| self.type_of(o) == "OpMacroInterface")
            .ok_or_else(|| refuse("INTERFACE", ErrorKind::Missing))?;
        let iface_end = OpCode::read(self.igz, *iface)?
            .block_end(iface_at)
            .filter(|&e| e < ops.len())
            .ok_or_else(|| refuse("INTERFACE", ErrorKind::Missing))?;
        let mut out = Vec::new();
        for &op in &ops[iface_at + 1..=iface_end] {
            match self.type_of(op) {
                "OpFlowMacro" => {}
                t if is_a(t, OpMacroParameter::TYPE) => {
                    let p = OpMacroParameter::read(self.igz, op)?;
                    let name = p.var.name.unwrap_or("");
                    if !allowed.contains(&name) || out.iter().any(|(n, _)| n == name) {
                        return Err(refuse(
                            "_varName",
                            ErrorKind::Unsupported { value: op as u64 },
                        ));
                    }
                    let index = self.index(p.index_rhs)?;
                    let arg = match p.rhs {
                        ObjectRef::Object(rhs) => self.argument(rhs, index)?,
                        _ => return Err(refuse("_RHS", ErrorKind::Missing)),
                    };
                    out.push((name.to_owned(), arg));
                }
                _ => return Err(stranger(self.igz, op, OpMacroParameter::TYPE)),
            }
        }
        Ok((out, iface_end))
    }

    /// The set an `OpChangeMembership` adds to, named by the last element
    /// of its target path (a variable), and what it adds.
    fn addition<'s>(
        &self,
        change: &OpChangeMembership<'s>,
    ) -> Result<Option<(String, Vec<ObjectRef<'s>>)>, FormatError>
    where
        Self: 's,
    {
        if change.combine_op != OpChangeMembership::ADD {
            return Ok(None);
        }
        let Some(target) = self.stack(change.lhs)? else {
            return Ok(None);
        };
        let Some(&ObjectRef::Object(var)) = target.path.last() else {
            return Ok(None);
        };
        if !is_a(self.type_of(var), Variable::TYPE) {
            return Ok(None);
        }
        let name = Variable::read(self.igz, var)?.name.unwrap_or("").to_owned();
        let path = self.stack(change.rhs)?.map(|s| s.path).unwrap_or_default();
        Ok(Some((name, path)))
    }

    /// An operand that's a local stack, read; anything else is `None`.
    fn stack<'s>(&self, r: ObjectRef<'s>) -> Result<Option<Stack<'s>>, FormatError>
    where
        Self: 's,
    {
        match r {
            ObjectRef::Object(o) if is_a(self.type_of(o), Stack::TYPE) => {
                Stack::read(self.igz, o).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn variable_name(&self, r: &ObjectRef<'_>) -> Result<Option<String>, FormatError> {
        let &ObjectRef::Object(o) = r else {
            return Ok(None);
        };
        if !is_a(self.type_of(o), Variable::TYPE) {
            return Ok(None);
        }
        Ok(Variable::read(self.igz, o)?.name.map(str::to_owned))
    }

    fn table(&self) -> Result<ToyTable, FormatError> {
        let ops = self.ops()?;
        let mut table = ToyTable {
            toys: Vec::new(),
            alts: Vec::new(),
            alt_characters: Vec::new(),
            lightcore_loads: Vec::new(),
        };
        let mut alt_characters = Vec::new();
        // A toy call's after-return block, `from..=to`: the caller's code
        // that runs once the macro returns (its alts, its set additions).
        struct After {
            from: usize,
            to: usize,
            toy: usize,
        }
        let mut open: Vec<After> = Vec::new();
        for (p, &op) in ops.iter().enumerate() {
            open.retain(|a| p <= a.to);
            let ty = self.type_of(op);
            if let Some(after) = open.last().filter(|a| p >= a.from) {
                match ty {
                    "OpMacroInterface"
                    | "OpMacroParameter"
                    | "OpFlowMacro"
                    | "OpFlowBuiltInBehavior" => continue,
                    "OpChangeMembership" => {
                        let change = OpChangeMembership::read(self.igz, op)?;
                        match self.addition(&change)? {
                            Some((set, path)) if self.adds_return_info(&path)? => {
                                table.toys[after.toy].added_to.push(set);
                                continue;
                            }
                            _ => return Err(stranger(self.igz, op, "an addition of the toy")),
                        }
                    }
                    "OpUseMacro" => {}
                    _ => return Err(stranger(self.igz, op, "an op of a toy-table call")),
                }
            }
            if ty != OpUseMacro::TYPE {
                continue;
            }
            let call = OpUseMacro::read(self.igz, op)?;
            let name = self.macro_name(&call)?;
            let in_after = open.last().is_some_and(|a| p >= a.from);
            if in_after && name.as_deref() != Some(DEFINE_ALT) {
                return Err(stranger(self.igz, op, DEFINE_ALT));
            }
            let Some(name) = name else { continue };
            let use_end = call
                .op
                .block_end(p)
                .filter(|&e| e < ops.len())
                .ok_or_else(|| refuse("_branchPC", ErrorKind::Missing))?;
            match name.as_str() {
                DEFINE_TOY => {
                    let (args, iface_end) = self.arguments(
                        &ops,
                        p,
                        &[
                            "level load",
                            "name",
                            "item code",
                            "is magic item",
                            "is sky1 character",
                            "has known repose",
                            "is giant",
                            "element",
                            "return info",
                        ],
                    )?;
                    let get = |n: &str| args.iter().find(|(k, _)| k == n).map(|(_, a)| *a);
                    // An omitted value parameter reads 0 (aj-vm).
                    let int = |n: &'static str| match get(n) {
                        None => Ok(0),
                        Some(Arg::Int(i)) => Ok(i),
                        Some(_) => Err(refuse(n, ErrorKind::Unsupported { value: p as u64 })),
                    };
                    let key = |n: &'static str| match get(n) {
                        Some(Arg::Key(k)) => Ok(k),
                        _ => Err(refuse(n, ErrorKind::Missing)),
                    };
                    table.toys.push(Toy {
                        position: p,
                        level_load: key("level load")?,
                        name: key("name")?,
                        item_code: int("item code")?,
                        magic_item: int("is magic item")? != 0,
                        sky1: int("is sky1 character")? != 0,
                        known_repose: int("has known repose")? != 0,
                        giant: int("is giant")? != 0,
                        element: int("element")?,
                        added_to: Vec::new(),
                    });
                    if iface_end < use_end {
                        open.push(After {
                            from: iface_end + 1,
                            to: use_end,
                            toy: table.toys.len() - 1,
                        });
                    }
                }
                DEFINE_ALT => {
                    let (args, _) = self.arguments(
                        &ops,
                        p,
                        &[
                            "level load",
                            "check against deco id",
                            "subtype display string",
                        ],
                    )?;
                    let get = |n: &str| args.iter().find(|(k, _)| k == n).map(|(_, a)| *a);
                    // `toy level loads . last`: the toy defined last.
                    let toy = table
                        .toys
                        .len()
                        .checked_sub(1)
                        .ok_or_else(|| refuse(DEFINE_ALT, ErrorKind::Missing))?;
                    let Some(Arg::Key(level_load)) = get("level load") else {
                        return Err(refuse("level load", ErrorKind::Missing));
                    };
                    let deco = match get("check against deco id") {
                        None => 0,
                        Some(Arg::Int(i)) => i,
                        Some(_) => return Err(refuse("check against deco id", ErrorKind::Missing)),
                    };
                    let display = match get("subtype display string") {
                        None | Some(Arg::Null) => None,
                        Some(Arg::Key(k)) => Some(k),
                        Some(Arg::Int(_)) => {
                            return Err(refuse("subtype display string", ErrorKind::Missing));
                        }
                    };
                    table.alts.push(Alt {
                        position: p,
                        toy,
                        level_load,
                        deco,
                        display,
                    });
                }
                DEFINE_ALT_CHARACTER => {
                    let (args, _) = self.arguments(
                        &ops,
                        p,
                        &["normal toy load", "alt prefix", "alt toy load"],
                    )?;
                    let key = |n: &'static str| match args.iter().find(|(k, _)| k == n) {
                        Some((_, Arg::Key(k))) => Ok(*k),
                        _ => Err(refuse(n, ErrorKind::Missing)),
                    };
                    alt_characters.push((
                        p,
                        key("normal toy load")?,
                        key("alt toy load")?,
                        key("alt prefix")?,
                    ));
                }
                _ => {}
            }
        }
        // Alt characters link toys by level load, wherever the calls sit.
        for (position, normal, alt, prefix) in alt_characters {
            let toy = |key| {
                table
                    .toy_by_load(key)
                    .ok_or_else(|| refuse(DEFINE_ALT_CHARACTER, ErrorKind::Missing))
            };
            table.alt_characters.push(AltCharacter {
                position,
                normal: toy(normal)?,
                alt: toy(alt)?,
                prefix,
            });
        }
        table.lightcore_loads = self.lightcore_loads(&ops)?;
        Ok(table)
    }

    /// Whether an addition's operand is the call's `return info`: a group
    /// stack holding that parameter alone.
    fn adds_return_info(&self, path: &[ObjectRef<'_>]) -> Result<bool, FormatError> {
        let [only] = path else { return Ok(false) };
        Ok(self.variable_name(only)?.as_deref() == Some("return info"))
    }

    /// The level loads the LightCore loop adds to [`LIGHTCORE_SET`]: the
    /// loop that encloses the one addition into that set iterates a
    /// variable, and the imports added to that variable before the loop are
    /// the loads.
    fn lightcore_loads(&self, ops: &[usize]) -> Result<Vec<Key>, FormatError> {
        let mut additions = Vec::new();
        for (p, &op) in ops.iter().enumerate() {
            if self.type_of(op) != OpChangeMembership::TYPE {
                continue;
            }
            let change = OpChangeMembership::read(self.igz, op)?;
            if let Some((set, _)) = self.addition(&change)?
                && set == LIGHTCORE_SET
            {
                additions.push(p);
            }
        }
        let [at] = additions[..] else {
            let kind = if additions.is_empty() {
                ErrorKind::Missing
            } else {
                ErrorKind::Duplicate
            };
            return Err(refuse(LIGHTCORE_SET, kind));
        };
        let mut lp = None;
        for q in (0..at).rev() {
            if self.type_of(ops[q]) == OpLoopValue::TYPE
                && OpCode::read(self.igz, ops[q])?
                    .block_end(q)
                    .is_some_and(|end| end >= at)
            {
                lp = Some(q);
                break;
            }
        }
        let lp = lp.ok_or_else(|| refuse("loop value", ErrorKind::Missing))?;
        let looped = OpLoopValue::read(self.igz, ops[lp])?;
        let var = self.loop_variable(looped.rhs)?;
        let mut loads = Vec::new();
        for &op in &ops[..lp] {
            if self.type_of(op) != OpChangeMembership::TYPE {
                continue;
            }
            let change = OpChangeMembership::read(self.igz, op)?;
            let Some(target) = self.stack(change.lhs)? else {
                continue;
            };
            if target.path[..] != [ObjectRef::Object(var)]
                || change.combine_op != OpChangeMembership::ADD
            {
                continue;
            }
            let Some(added) = self.stack(change.rhs)? else {
                continue;
            };
            match added.path[..] {
                [ObjectRef::Import(i)] => {
                    let o = self.global_object(&i.id)?;
                    if self.global.type_name(&self.global.objects()[o]) != StringInfo::TYPE {
                        return Err(stranger(self.global, o, StringInfo::TYPE));
                    }
                    loads.push(o);
                }
                _ => {
                    return Err(refuse(
                        LIGHTCORE_SET,
                        ErrorKind::Unsupported { value: op as u64 },
                    ));
                }
            }
        }
        if loads.is_empty() {
            return Err(refuse(LIGHTCORE_SET, ErrorKind::Missing));
        }
        Ok(loads)
    }

    /// The variable a `loop value` counts: its operand is `<variable> .
    /// setvariant.count`, possibly inside a `ValueRHSVariant`.
    fn loop_variable(&self, rhs: ObjectRef<'_>) -> Result<usize, FormatError> {
        let ObjectRef::Object(mut o) = rhs else {
            return Err(refuse("loop value", ErrorKind::Missing));
        };
        if is_a(self.type_of(o), ValueRhsVariant::TYPE) {
            let v = ValueRhsVariant::read(self.igz, o)?;
            match (v.op1, v.operator) {
                (ObjectRef::Object(inner), NO_OPERATOR) => o = inner,
                _ => {
                    return Err(refuse(
                        "loop value",
                        ErrorKind::Unsupported { value: o as u64 },
                    ));
                }
            }
        }
        let stack = RhsValueStack::read(self.igz, o)?;
        match stack.stack.path[..] {
            [ObjectRef::Object(var), ObjectRef::Import(count)]
                if count.id.is("setvariant", "count")
                    && is_a(self.type_of(var), Variable::TYPE) =>
            {
                Ok(var)
            }
            _ => Err(refuse(
                "loop value",
                ErrorKind::Unsupported { value: o as u64 },
            )),
        }
    }
}

#[cfg(test)]
mod tests;
