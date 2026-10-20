//! Writes the progression readers' inputs in code, for tests and fuzz seeds
//! (`test-fixtures`): an `init_setup` whose save-param table macro calls the
//! level-up and setup macros in each shape the 01.00 disc's body has (literal
//! rows, a spillover limit off the level table, loops over a set by the pass
//! and with an offset, a bit at a set's count, two 16-bit fields a word off
//! a scratch pair), and a `global.bld` with the save params, sets, scratch
//! values and "Game - Award Money"'s clamp. Every number is ours; the names
//! are the ones the readers find by.
//!
//! Standalone ([`disc`]) or as parts of the roster builder's two levels
//! ([`global_part`], [`init_setup_part`]), so every synthetic dump carries
//! a progression.

use std::collections::BTreeMap;

use crate::exports::NAMESPACE;
use crate::fnv1a32;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

use super::{
    AMOUNT, AWARD_MONEY, DEFINE_LEVELUP, EXTENDED_BITFIELD, MIRROR_AND_CLEAR, SAVE_TABLE,
    SETUP_PARAM, SPILLOVER, SaveKind, params,
};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// The structure the level table lives in, as the disc names it.
pub const CONTROL: &str = "portal control data";
/// The field of it the table is.
pub const TABLE: &str = "level xp table";
pub const SCRATCH_WORD: &str = "Scratch G1";
pub const SCRATCH_HALF: &str = "Scratch G2";

/// One setup call, or the loop around one, in the save-param table's body.
#[derive(Debug, Clone)]
pub enum RowSpec {
    /// Every argument a literal.
    Literal {
        param: &'static str,
        kind: SaveKind,
        word: i32,
        bit: i32,
        width: i32,
        bits_per_field: i32,
        limit: i32,
    },
    /// A spillover whose limit is the level table's last entry less the
    /// param's own `limit to val`.
    Spill {
        param: &'static str,
        word: i32,
        bit: i32,
        width: i32,
    },
    /// A loop over `set`'s count: the set indexed by the pass, one bit each
    /// at the pass plus `offset` (a bare pass when it's 0).
    PerMember {
        set: &'static str,
        members: Vec<&'static str>,
        word: i32,
        offset: i32,
    },
    /// One field at bit `set . setvariant.count`.
    AfterSet {
        param: &'static str,
        word: i32,
        set: &'static str,
        width: i32,
        limit: i32,
    },
    /// The name characters' shape: the body sizes `set` to `count`, then a
    /// loop puts 16 bits a pass at the scratch word and half, the half
    /// flipping each pass and the word moving on after the second.
    Pairs {
        set: &'static str,
        count: i32,
        word: i32,
    },
    /// A plain call on `param` with one argument in a shape the disc never
    /// has, for the refusals.
    Odd { param: &'static str, odd: Odd },
}

/// The shapes [`RowSpec::Odd`] puts in a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Odd {
    /// `bit index` a float literal.
    FloatBit,
    /// `bit index` a random literal.
    RandomBit,
    /// `bit index` a cell the body wrote with a float.
    UnknownCell,
    /// `save param` a value, not a structure.
    ValueAsParam,
    /// A parameter the setup macro doesn't have.
    UnknownParameter,
}

/// What "Game - Award Money"'s body holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClampSpec {
    /// `if amount > C`, its block setting `amount` to `C`.
    One(i32),
    /// No such check.
    None,
    /// Two of them.
    Twice(i32),
    /// The check, its block setting another value.
    Mismatched(i32),
}

#[derive(Debug, Clone)]
pub struct ProgressionSpec {
    /// `level xp (1)` on, in order.
    pub level_xp: Vec<i32>,
    /// The count the level-up definition sizes its table to; one more than
    /// `level_xp` holds when it's well-formed.
    pub table_len: i32,
    pub rows: Vec<RowSpec>,
    pub clamp: ClampSpec,
    /// A loop over the body's own count, nothing under it: what a loop
    /// whose count runs away looks like when mutated.
    pub bare_loop: Option<i32>,
    /// An op class the body has nowhere on the disc, after the rows.
    pub stray_op: bool,
    /// The level-up call's arguments in this order of `level xp (k)` (the
    /// identity when empty), for a call that omits or repeats one.
    pub level_args: Vec<usize>,
}

/// One row of every shape, our numbers: a 5-level table and its spillover,
/// gold, playtime, four upgrade bits, a count past them, three platform bits
/// at an offset, six name characters, a mirror and an extended bitfield.
pub fn sample() -> ProgressionSpec {
    ProgressionSpec {
        level_xp: vec![400, 1100, 2500, 4500],
        table_len: 5,
        rows: vec![
            RowSpec::Literal {
                param: "pp_Exp",
                kind: SaveKind::Plain,
                word: 0,
                bit: 0,
                width: 12,
                bits_per_field: 0,
                limit: 3000,
            },
            RowSpec::Literal {
                param: "pp_Money",
                kind: SaveKind::Plain,
                word: 0,
                bit: 12,
                width: 14,
                bits_per_field: 0,
                limit: 9999,
            },
            RowSpec::Literal {
                param: "pp_CumulativeTime",
                kind: SaveKind::Plain,
                word: 1,
                bit: 0,
                width: 20,
                bits_per_field: 0,
                limit: 0,
            },
            RowSpec::PerMember {
                set: "pp_UpgradeBits",
                members: vec!["pp_Path", "pp_Upgrade(1)", "pp_Upgrade(2)", "pp_Upgrade(3)"],
                word: 2,
                offset: 0,
            },
            RowSpec::AfterSet {
                param: "pp_ElementCount",
                word: 2,
                set: "pp_UpgradeBits",
                width: 2,
                limit: 3,
            },
            RowSpec::PerMember {
                set: "pp_Platforms",
                members: vec!["pp_Platform(0)", "pp_Platform(1)", "pp_Platform(2)"],
                word: 2,
                offset: 20,
            },
            RowSpec::Pairs {
                set: "pp_Letters",
                count: 6,
                word: 4,
            },
            RowSpec::Spill {
                param: "pp_Exp",
                word: 7,
                bit: 0,
                width: 11,
            },
            RowSpec::Literal {
                param: "pp_Hat",
                kind: SaveKind::MirrorAndClear,
                word: 7,
                bit: 16,
                width: 6,
                bits_per_field: 0,
                limit: 0,
            },
            RowSpec::Literal {
                param: "pp_Challenges",
                kind: SaveKind::ExtendedBitfield,
                word: 8,
                bit: 0,
                width: 9,
                bits_per_field: 1,
                limit: 0,
            },
        ],
        clamp: ClampSpec::One(9000),
        bare_loop: None,
        stray_op: false,
        level_args: Vec::new(),
    }
}

/// The three levels the readers take, standalone.
#[derive(Debug, Clone)]
pub struct Disc {
    pub init_setup: Vec<u8>,
    pub global: Vec<u8>,
    pub bootstrap: Vec<u8>,
}

/// Giants' bootstrap shape: global's ids start past its 19 roots.
const BOOTSTRAP_ROOTS: u32 = 19;

pub fn disc(spec: &ProgressionSpec) -> Disc {
    let mut g = IgzBuilder::new();
    let root = g.object(0, "igObjectList", 0x18);
    let blocks = g.section(0x10);
    let mut roots = Vec::new();
    let mut ids = BTreeMap::new();
    global_part(&mut g, blocks, spec, &mut roots, &mut ids, BOOTSTRAP_ROOTS);
    list(&mut g, root, blocks, &roots);

    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let blocks = b.section(0x10);
    let script = init_setup_part(&mut b, blocks, spec, &ids);
    list(&mut b, root, blocks, &[Some(script)]);
    Disc {
        init_setup: b.build(),
        global: g.build(),
        bootstrap: crate::exports::builder::bootstrap(),
    }
}

/// The registry's bundle, in the order its walker unpacks it.
pub fn bundle(d: &Disc) -> Vec<u8> {
    crate::roster::builder::bundle(&[&d.init_setup, &d.global, &d.bootstrap])
}

/// Every name the rows give a structure, then the sets with their members.
fn names(spec: &ProgressionSpec) -> (Vec<&'static str>, Vec<(&'static str, Vec<&'static str>)>) {
    let mut structures = Vec::new();
    let mut sets: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    let mut structure = |n: &'static str| {
        if !structures.contains(&n) {
            structures.push(n);
        }
    };
    for row in &spec.rows {
        match row {
            RowSpec::Literal { param, .. }
            | RowSpec::Spill { param, .. }
            | RowSpec::Odd { param, .. } => structure(param),
            RowSpec::AfterSet { param, set, .. } => {
                structure(param);
                if !sets.iter().any(|(s, _)| s == set) {
                    sets.push((*set, Vec::new()));
                }
            }
            RowSpec::PerMember { set, members, .. } => {
                match sets.iter_mut().find(|(s, _)| s == set) {
                    Some((_, m)) if m.is_empty() => m.clone_from(members),
                    Some(_) => {}
                    None => sets.push((*set, members.clone())),
                }
            }
            RowSpec::Pairs { set, .. } => {
                if !sets.iter().any(|(s, _)| s == set) {
                    sets.push((*set, Vec::new()));
                }
            }
        }
    }
    (structures, sets)
}

/// `global.bld`'s side: the save params, their sets (members as
/// structures), the scratch pair, the level table's structure and its
/// control, each a root with its export id in `ids` (root `k` of `roots`
/// is id `base + 1 + k`), then a script defining "Game - Award Money".
pub fn global_part(
    b: &mut IgzBuilder,
    blocks: u8,
    spec: &ProgressionSpec,
    roots: &mut Vec<Option<Location>>,
    ids: &mut BTreeMap<String, u32>,
    base: u32,
) {
    let mut root = |b: &mut IgzBuilder, name: &str, o: Location| {
        b.string(at(o, 0x08), name);
        ids.insert(name.to_owned(), base + 1 + roots.len() as u32);
        roots.push(Some(o));
    };
    let (structures, sets) = names(spec);
    for name in structures {
        let o = b.object(0, "ScriptStructure", 0x20);
        root(b, name, o);
    }
    for (name, members) in sets {
        let set = b.object(0, "ScriptSet", 0x1C);
        let members_list = b.object(0, "ScriptSetList", 0x24);
        b.pointer(at(set, 0x18), members_list);
        let elements: Vec<Option<Location>> = members
            .iter()
            .map(|m| {
                let o = b.object(0, "ScriptStructure", 0x20);
                b.string(at(o, 0x08), m);
                Some(o)
            })
            .collect();
        list(b, members_list, blocks, &elements);
        root(b, name, set);
    }
    for name in [SCRATCH_WORD, SCRATCH_HALF] {
        let v = b.object(0, "ValueInfo", 0x24);
        b.meta_object(at(v, 0x1C), "metaobject", "IntMeasurement");
        root(b, name, v);
    }
    for name in [CONTROL, "portal save control"] {
        let o = b.object(0, "ScriptReference", 0x20);
        root(b, name, o);
    }
    award_money(b, blocks, spec.clamp);
}

/// Writes one script's ops.
struct Writer<'b> {
    b: &'b mut IgzBuilder,
    blocks: u8,
    ops: Vec<Location>,
    ids: &'b BTreeMap<String, u32>,
    flow_macro: Location,
    flow_continue: Location,
    /// EXNM indexes of `metaobject::IntMeasurement` and `FloatMeasurement`.
    int: u32,
    float: u32,
}

#[derive(Debug, Clone, Copy)]
enum El<'n> {
    Global(&'n str),
    Engine(&'static str, &'static str),
    Local(Location),
}

impl<'b> Writer<'b> {
    /// A script named `name` with its own type hosts; its ops go into its
    /// list at [`Writer::finish`].
    fn new(
        b: &'b mut IgzBuilder,
        blocks: u8,
        ids: &'b BTreeMap<String, u32>,
        name: &str,
    ) -> (Self, Location, Location) {
        let script = b.object(0, "tfbScriptInfo", 0x1C);
        let ops = b.object(0, "OpCodeList", 0x18);
        b.string(at(script, 0x08), name);
        b.put_bytes(at(script, 0x10), &[1]);
        b.pointer(at(script, 0x14), ops);
        // The literals' classes, hosted on the script's unread refs.
        let int = b.meta_object(at(script, 0x0C), "metaobject", "IntMeasurement");
        let float = b.meta_object(at(script, 0x18), "metaobject", "FloatMeasurement");
        let flow_macro = b.object(0, "OpFlowMacro", 0x1C);
        b.string(at(flow_macro, 0x08), "flow macro");
        let flow_continue = b.object(0, "OpFlowBuiltInBehavior", 0x1C);
        b.string(at(flow_continue, 0x08), "flow continue");
        (
            Self {
                b,
                blocks,
                ops: Vec::new(),
                ids,
                flow_macro,
                flow_continue,
                int,
                float,
            },
            script,
            ops,
        )
    }

    fn finish(self, list_at: Location) {
        let ops: Vec<Option<Location>> = self.ops.iter().copied().map(Some).collect();
        list(self.b, list_at, self.blocks, &ops);
    }

    fn op(&mut self, ty: &str, size: u32, name: &str) -> Location {
        let o = self.b.object(0, ty, size);
        self.b.string(at(o, 0x08), name);
        self.ops.push(o);
        o
    }

    /// `_branchPC` of the op at position `start`: the ops after it its
    /// block holds, through the last pushed.
    fn close_at(&mut self, start: usize) {
        let n = self.ops.len() - 1 - start;
        let op = self.ops[start];
        self.b.put_u32(at(op, 0x18), n as u32);
    }

    fn branch(&mut self, op: Location, n: u32) {
        self.b.put_u32(at(op, 0x18), n);
    }

    fn path(&mut self, stack: Location, elements: &[El<'_>]) {
        let n = elements.len() as u32;
        self.b.put_u32(at(stack, 0x08), n);
        self.b.put_u32(at(stack, 0x0C), n);
        if elements.is_empty() {
            self.b.null_memory_ref(at(stack, 0x10), 0);
            return;
        }
        let data = self.b.memory_ref(
            at(stack, 0x10),
            self.blocks,
            &vec![0; 4 * elements.len()],
            0,
        );
        for (i, e) in elements.iter().enumerate() {
            let w = at(data, 4 * i as u32);
            match e {
                El::Global(name) => {
                    let id = *self
                        .ids
                        .get(*name)
                        .unwrap_or_else(|| panic!("no export named {name}"));
                    self.b.import_raw(w, id, fnv1a32(NAMESPACE.as_bytes()));
                }
                El::Engine(ns, member) => {
                    self.b.import(w, ns, member);
                }
                El::Local(o) => self.b.pointer(w, *o),
            }
        }
    }

    fn literal(&mut self, value: i32) -> Location {
        let s = self.b.object(0, "RHSValueStack", 0x28);
        self.path(s, &[]);
        self.b.unflagged_meta(at(s, 0x1C), self.int);
        self.b.put_u32(at(s, 0x20), value as u32);
        s
    }

    fn float(&mut self, value: f32) -> Location {
        let s = self.b.object(0, "RHSValueStack", 0x28);
        self.path(s, &[]);
        self.b.unflagged_meta(at(s, 0x1C), self.float);
        self.b.put_u32(at(s, 0x20), value.to_bits());
        s
    }

    fn value(&mut self, elements: &[El<'_>]) -> Location {
        let s = self.b.object(0, "RHSValueStack", 0x28);
        self.path(s, elements);
        self.b.unflagged_meta(at(s, 0x1C), self.int);
        s
    }

    fn reference(&mut self, elements: &[El<'_>]) -> Location {
        // Giants' files write RHSReferenceStack 4 bytes over the registry.
        let s = self.b.object(0, "RHSReferenceStack", 0x30);
        self.path(s, elements);
        self.b.unflagged_meta(at(s, 0x1C), self.int);
        self.b.put_u32(at(s, 0x2C), 0xE82A_A02A);
        s
    }

    /// A `ValueStack`: an LHS.
    fn cell(&mut self, elements: &[El<'_>]) -> Location {
        let s = self.b.object(0, "ValueStack", 0x1C);
        self.path(s, elements);
        s
    }

    fn arith(&mut self, a: Location, operator: u32, b: Location) -> Location {
        let v = self.b.object(0, "ValueRHSVariant", 0x1C);
        self.b.pointer(at(v, 0x08), a);
        self.b.put_u32(at(v, 0x0C), operator);
        self.b.pointer(at(v, 0x10), b);
        v
    }

    fn variable(&mut self, name: &str) -> Location {
        let v = self.b.object(0, "OpCreateVariable", 0x2C);
        self.b.string(at(v, 0x08), "create variable||with|members");
        self.b.string(at(v, 0x1C), name);
        self.b.put_u32(at(v, 0x28), u32::MAX);
        v
    }

    fn parameter(&mut self, name: &str, rhs: Location, def: Location) -> Location {
        let p = self.op("OpMacroParameter", 0x48, "parameter");
        self.b.string(at(p, 0x1C), name);
        self.b.put_u32(at(p, 0x28), u32::MAX);
        self.b.pointer(at(p, 0x30), rhs);
        self.b.pointer(at(p, 0x38), def);
        p
    }

    /// Opens a definition at the next position: its op, INTERFACE and
    /// parameters (each with a default). Returns where it starts and its
    /// parameters; [`Writer::close`] ends it.
    fn define(
        &mut self,
        ty: &str,
        size: u32,
        name: &str,
        defaults: &[(&str, Location)],
    ) -> (usize, Location, Vec<Location>) {
        let start = self.ops.len();
        let def = self.op(ty, size, name);
        let iface = self.op("OpMacroInterface", 0x20, "INTERFACE");
        self.branch(iface, defaults.len() as u32 + 1);
        let params = defaults
            .iter()
            .map(|(n, rhs)| self.parameter(n, *rhs, def))
            .collect();
        self.ops.push(self.flow_macro);
        (start, def, params)
    }

    fn close(&mut self, start: usize) {
        self.ops.push(self.flow_continue);
        self.close_at(start);
    }

    fn call(&mut self, def: Location, args: &[(&str, Location, Option<Location>)]) {
        let np = self.b.object(0, "ScriptGroupStack", 0x1C);
        self.path(np, &[El::Local(def)]);
        let start = self.ops.len();
        let call = self.op("OpUseMacro", 0x24, "use macro");
        self.b.pointer(at(call, 0x1C), np);
        let iface = self.op("OpMacroInterface", 0x20, "INTERFACE");
        self.branch(iface, args.len() as u32 + 1);
        for (name, rhs, index) in args {
            let p = self.parameter(name, *rhs, def);
            if let Some(i) = index {
                self.b.pointer(at(p, 0x34), *i);
            }
        }
        self.ops.push(self.flow_macro);
        self.close(start);
    }

    fn set(&mut self, lhs: &[El<'_>], rhs: Location) {
        let lhs = self.cell(lhs);
        let s = self.op("OpSetValue", 0x24, "set||=");
        self.b.pointer(at(s, 0x1C), lhs);
        self.b.pointer(at(s, 0x20), rhs);
    }

    /// `if <lhs> <relation> <rhs>`; returns its position for
    /// [`Writer::close_at`].
    fn check(&mut self, lhs: Location, relation: u32, rhs: Location) -> usize {
        let start = self.ops.len();
        let c = self.op("OpCheckValue", 0x28, "if");
        self.b.pointer(at(c, 0x1C), lhs);
        self.b.put_u32(at(c, 0x20), relation);
        self.b.pointer(at(c, 0x24), rhs);
        start
    }

    fn else_skip(&mut self, n: u32) {
        let e = self.op("OpAbstractFlow", 0x1C, "else");
        self.branch(e, n);
    }

    /// `loop value <count>`; returns the op and its position. The body
    /// follows; [`Writer::end_loop`] closes it.
    fn begin_loop(&mut self, count: Location) -> (Location, usize) {
        let start = self.ops.len();
        let l = self.op("OpLoopValue", 0x28, "loop value|||loop index");
        self.b.pointer(at(l, 0x20), count);
        (l, start)
    }

    fn end_loop(&mut self, start: usize) {
        let flow = self.b.object(0, "OpFlowLoop", 0x1C);
        self.ops.push(flow);
        self.close_at(start);
    }
}

/// The save-param table's script, into `b`'s `init_setup` level: the table
/// macro first, then the level-up, setup and specialization definitions it
/// calls. Returns the script, for a root list.
pub fn init_setup_part(
    b: &mut IgzBuilder,
    blocks: u8,
    spec: &ProgressionSpec,
    ids: &BTreeMap<String, u32>,
) -> Location {
    let (mut w, script, list_at) = Writer::new(
        b,
        blocks,
        ids,
        "C:/tfb/Content/Levels/Includes/Global/scripts/Init_Setup_SaveGame_Portal.ai",
    );
    // The definitions' objects first: the table calls them by these.
    let table_var = w.variable(TABLE);
    let limit_var = w.variable(params::LIMIT);
    let frequency_var = w.variable("max frequency");
    let defs = Defs {
        levelup: w.b.object(0, "OpDefineMacro", 0x38),
        setup: w.b.object(0, "OpDefineMacro", 0x38),
        spillover: w.b.object(0, "OpDefineMacroSpecialization", 0x40),
        mirror: w.b.object(0, "OpDefineMacroSpecialization", 0x40),
        extended: w.b.object(0, "OpDefineMacroSpecialization", 0x40),
    };

    let null = w.reference(&[El::Engine("abstractscriptgroup", "null")]);
    let (table_start, _, _) = w.define(
        "OpDefineMacro",
        0x38,
        SAVE_TABLE,
        &[("save params", null), ("save ctrl", null)],
    );
    // The level-up call.
    let hp = w.float(0.25);
    let mut args = vec![(params::HP_MULT.to_owned(), hp, None)];
    let order: Vec<usize> = if spec.level_args.is_empty() {
        (1..=spec.level_xp.len()).collect()
    } else {
        spec.level_args.clone()
    };
    for k in order {
        let xp = spec.level_xp.get(k.wrapping_sub(1)).copied().unwrap_or(0);
        let rhs = w.literal(xp);
        args.push((format!("{} ({k})", params::LEVEL_XP), rhs, None));
    }
    let args: Vec<(&str, Location, Option<Location>)> =
        args.iter().map(|(n, r, i)| (n.as_str(), *r, *i)).collect();
    w.call(defs.levelup, &args);
    // What the disc's body does besides the rows: a float set, a set
    // membership change and a reference set, none of which a row reads.
    let two = w.float(2.0);
    w.set(
        &[El::Global("portal save control"), El::Local(frequency_var)],
        two,
    );
    let set_lhs = w.b.object(0, "SetStack", 0x18);
    w.path(
        set_lhs,
        &[El::Global("portal save control"), El::Local(frequency_var)],
    );
    let added = w.b.object(0, "ScriptGroupStack", 0x1C);
    w.path(added, &[El::Global(CONTROL)]);
    let change = w.op("OpChangeMembership", 0x28, "set [set]||to");
    w.b.pointer(at(change, 0x1C), set_lhs);
    w.b.pointer(at(change, 0x24), added);
    let ref_lhs = w.b.object(0, "ReferenceStack", 0x18);
    w.path(ref_lhs, &[El::Global(SCRATCH_WORD)]);
    let setref = w.op("OpSetReference", 0x28, "setref||=");
    w.b.pointer(at(setref, 0x1C), ref_lhs);
    w.b.pointer(at(setref, 0x20), null);

    let vars = Vars {
        table: table_var,
        limit: limit_var,
        frequency: frequency_var,
    };
    for row in &spec.rows {
        row_ops(&mut w, &defs, &vars, row);
    }
    if let Some(n) = spec.bare_loop {
        let count = w.literal(n);
        let (_, start) = w.begin_loop(count);
        w.end_loop(start);
    }
    if spec.stray_op {
        let inc = w.cell(&[El::Global(SCRATCH_WORD)]);
        let o = w.op("OpFindSubSet", 0x30, "find subset");
        w.b.pointer(at(o, 0x1C), inc);
    }
    w.close(table_start);

    // "Portal - Define Levelup Table": the table sized, then (as on the
    // disc) a loop the reader doesn't step.
    let start = w.ops.len();
    w.ops.push(defs.levelup);
    w.b.string(at(defs.levelup, 0x08), DEFINE_LEVELUP);
    let iface = w.op("OpMacroInterface", 0x20, "INTERFACE");
    let zero = w.literal(0);
    let mut n = 0;
    let hp_default = w.float(0.25);
    w.parameter(params::HP_MULT, hp_default, defs.levelup);
    n += 1;
    for k in 1..=spec.level_xp.len() {
        w.parameter(&format!("{} ({k})", params::LEVEL_XP), zero, defs.levelup);
        n += 1;
    }
    w.ops.push(w.flow_macro);
    w.branch(iface, n + 1);
    let len = w.literal(spec.table_len);
    w.set(
        &[
            El::Global(CONTROL),
            El::Local(table_var),
            El::Engine("setvariant", "count"),
        ],
        len,
    );
    let count = w.value(&[
        El::Global(CONTROL),
        El::Local(table_var),
        El::Engine("setvariant", "count"),
    ]);
    let (_, loop_start) = w.begin_loop(count);
    w.end_loop(loop_start);
    w.close(start);

    // "Save - Setup Portal Save Param": each argument into the param's own
    // field of that name.
    let start = w.ops.len();
    w.ops.push(defs.setup);
    w.b.string(at(defs.setup, 0x08), SETUP_PARAM);
    let iface = w.op("OpMacroInterface", 0x20, "INTERFACE");
    let mut formal = Vec::new();
    for name in params::ALL {
        let default = if name == params::SAVE_PARAM {
            null
        } else {
            zero
        };
        formal.push(w.parameter(name, default, defs.setup));
    }
    w.ops.push(w.flow_macro);
    w.branch(iface, params::ALL.len() as u32 + 1);
    for (i, name) in params::ALL.iter().enumerate().skip(1) {
        let field = if *name == params::LIMIT {
            limit_var
        } else {
            w.variable(name)
        };
        let from = w.value(&[El::Local(formal[i])]);
        w.set(&[El::Local(formal[0]), El::Local(field)], from);
    }
    w.close(start);

    // The three specializations, each naming the setup as its base.
    for (def, name) in [
        (defs.spillover, SPILLOVER),
        (defs.mirror, MIRROR_AND_CLEAR),
        (defs.extended, EXTENDED_BITFIELD),
    ] {
        let start = w.ops.len();
        w.ops.push(def);
        w.b.string(at(def, 0x08), name);
        let np = w.b.object(0, "OpDefineMacroStack", 0x18);
        w.path(np, &[El::Local(defs.setup)]);
        w.b.pointer(at(def, 0x38), np);
        let iface = w.op("OpMacroInterface", 0x20, "INTERFACE");
        w.ops.push(w.flow_macro);
        w.branch(iface, 1);
        w.close(start);
    }
    w.finish(list_at);
    script
}

/// The fields the body's paths name.
struct Vars {
    table: Location,
    limit: Location,
    frequency: Location,
}

struct Defs {
    levelup: Location,
    setup: Location,
    spillover: Location,
    mirror: Location,
    extended: Location,
}

impl Defs {
    fn of(&self, kind: SaveKind) -> Location {
        match kind {
            SaveKind::Plain => self.setup,
            SaveKind::Spillover => self.spillover,
            SaveKind::MirrorAndClear => self.mirror,
            SaveKind::ExtendedBitfield => self.extended,
        }
    }
}

fn row_ops(w: &mut Writer<'_>, defs: &Defs, vars: &Vars, row: &RowSpec) {
    let call = |w: &mut Writer<'_>,
                kind: SaveKind,
                save: Location,
                index: Option<Location>,
                values: [Location; 5]| {
        let mut args = vec![(params::SAVE_PARAM, save, index)];
        args.extend(
            params::ALL[1..]
                .iter()
                .zip(values)
                .map(|(n, v)| (*n, v, None)),
        );
        w.call(defs.of(kind), &args);
    };
    match *row {
        RowSpec::Literal {
            param,
            kind,
            word,
            bit,
            width,
            bits_per_field,
            limit,
        } => {
            let save = w.reference(&[El::Global(param)]);
            let values = [word, bit, width, bits_per_field, limit].map(|v| w.literal(v));
            call(w, kind, save, None, values);
        }
        RowSpec::Spill {
            param,
            word,
            bit,
            width,
        } => {
            let save = w.reference(&[El::Global(param)]);
            let last = w.value(&[
                El::Global(CONTROL),
                El::Local(vars.table),
                El::Engine("setvariant", "last"),
            ]);
            let own = w.value(&[El::Global(param), El::Local(vars.limit)]);
            let limit = w.arith(last, 1, own);
            let [word, bit, width, bpf] = [word, bit, width, 0].map(|v| w.literal(v));
            call(
                w,
                SaveKind::Spillover,
                save,
                None,
                [word, bit, width, bpf, limit],
            );
        }
        RowSpec::PerMember {
            set, word, offset, ..
        } => {
            let count = w.value(&[El::Global(set), El::Engine("setvariant", "count")]);
            let (looped, start) = w.begin_loop(count);
            let save = w.reference(&[El::Global(set)]);
            let index = w.value(&[El::Local(looped)]);
            let pass = w.value(&[El::Local(looped)]);
            let bit = if offset == 0 {
                pass
            } else {
                let off = w.literal(offset);
                w.arith(pass, 0, off)
            };
            let [word, width, bpf, limit] = [word, 1, 0, 0].map(|v| w.literal(v));
            call(
                w,
                SaveKind::Plain,
                save,
                Some(index),
                [word, bit, width, bpf, limit],
            );
            w.end_loop(start);
        }
        RowSpec::AfterSet {
            param,
            word,
            set,
            width,
            limit,
        } => {
            let save = w.reference(&[El::Global(param)]);
            let bit = w.value(&[El::Global(set), El::Engine("setvariant", "count")]);
            let [word, width, bpf, limit] = [word, width, 0, limit].map(|v| w.literal(v));
            call(
                w,
                SaveKind::Plain,
                save,
                None,
                [word, bit, width, bpf, limit],
            );
        }
        RowSpec::Odd { param, odd } => {
            let save = match odd {
                Odd::ValueAsParam => w.reference(&[El::Global(SCRATCH_WORD)]),
                _ => w.reference(&[El::Global(param)]),
            };
            let bit = match odd {
                Odd::FloatBit => w.float(3.0),
                Odd::RandomBit => {
                    let l = w.literal(3);
                    w.b.put_bytes(at(l, 0x24), &[1]);
                    l
                }
                Odd::UnknownCell => {
                    w.value(&[El::Global("portal save control"), El::Local(vars.frequency)])
                }
                _ => w.literal(0),
            };
            let [word, width, bpf, limit] = [0, 1, 0, 0].map(|v| w.literal(v));
            let mut args = vec![(params::SAVE_PARAM, save, None)];
            args.extend(
                params::ALL[1..]
                    .iter()
                    .zip([word, bit, width, bpf, limit])
                    .map(|(n, v)| (*n, v, None)),
            );
            if odd == Odd::UnknownParameter {
                let one = w.literal(1);
                args.push(("colour", one, None));
            }
            w.call(defs.setup, &args);
        }
        RowSpec::Pairs { set, count, word } => {
            let n = w.literal(count);
            w.set(&[El::Global(set), El::Engine("setvariant", "count")], n);
            let start_word = w.literal(word);
            w.set(&[El::Global(SCRATCH_WORD)], start_word);
            let zero = w.literal(0);
            w.set(&[El::Global(SCRATCH_HALF)], zero);
            let count = w.value(&[El::Global(set), El::Engine("setvariant", "count")]);
            let (looped, start) = w.begin_loop(count);
            let save = w.reference(&[El::Global(set)]);
            let index = w.value(&[El::Local(looped)]);
            let at_word = w.value(&[El::Global(SCRATCH_WORD)]);
            let half = w.value(&[El::Global(SCRATCH_HALF)]);
            let sixteen = w.literal(16);
            let bit = w.arith(half, 2, sixteen);
            let [width, bpf, limit] = [16, 0, 0].map(|v| w.literal(v));
            call(
                w,
                SaveKind::Plain,
                save,
                Some(index),
                [at_word, bit, width, bpf, limit],
            );
            // Two 16-bit characters a word: after the second half the word
            // moves on, as the disc's loop steps it.
            let half = w.value(&[El::Global(SCRATCH_HALF)]);
            let one = w.literal(1);
            let check = w.check(half, 1, one);
            let inc_lhs = w.cell(&[El::Global(SCRATCH_WORD)]);
            let inc = w.op("OpIncValue", 0x20, "inc value");
            w.b.pointer(at(inc, 0x1C), inc_lhs);
            let zero = w.literal(0);
            w.set(&[El::Global(SCRATCH_HALF)], zero);
            w.else_skip(1);
            w.close_at(check);
            let one = w.literal(1);
            w.set(&[El::Global(SCRATCH_HALF)], one);
            w.end_loop(start);
        }
    }
}

/// "Game - Award Money" in a script of its own, its clamp as `clamp` says,
/// shaped as the disc's: a check for a negative amount ending the macro,
/// then `if amount > C { if prev != C { amount = C } else { invalid; end } }`.
fn award_money(b: &mut IgzBuilder, blocks: u8, clamp: ClampSpec) {
    let ids = BTreeMap::new();
    let (mut w, _, list_at) = Writer::new(
        b,
        blocks,
        &ids,
        "C:/tfb/Content/Levels/Includes/Spyro_GenericStuff/scripts/Spyro_PortalManager.ai",
    );
    let null = w.reference(&[El::Engine("abstractscriptgroup", "null")]);
    let zero = w.literal(0);
    let prev = w.variable("prev amount");
    let (start, _, formal) = w.define(
        "OpDefineMacro",
        0x38,
        AWARD_MONEY,
        &[("player", null), (AMOUNT, zero), ("invalid", zero)],
    );
    let (amount, invalid) = (formal[1], formal[2]);
    let lhs = w.value(&[El::Local(amount)]);
    let zero = w.literal(0);
    let negative = w.check(lhs, 3, zero);
    let one = w.literal(1);
    w.set(&[El::Local(invalid)], one);
    let end = w.op("OpFlowBehavior", 0x1C, "flow end");
    w.branch(end, u32::from(crate::igz::layouts::BRANCH_END));
    w.close_at(negative);
    let blocks_of = match clamp {
        ClampSpec::None => vec![],
        ClampSpec::One(c) => vec![(c, c)],
        ClampSpec::Twice(c) => vec![(c, c), (c, c)],
        ClampSpec::Mismatched(c) => vec![(c, c + 1)],
    };
    for (limit, written) in blocks_of {
        // The disc's check names the earlier check's operand; ours the
        // earlier check itself, as the reader doesn't follow it.
        let earlier = w.ops[negative];
        let same = w.value(&[El::Local(earlier)]);
        let c = w.literal(limit);
        let over = w.check(same, 4, c);
        let prev_value = w.value(&[El::Local(prev)]);
        let c = w.literal(limit);
        let unchanged = w.check(prev_value, 5, c);
        let c = w.literal(written);
        w.set(&[El::Local(amount)], c);
        w.else_skip(2);
        w.close_at(unchanged);
        let one = w.literal(1);
        w.set(&[El::Local(invalid)], one);
        let end = w.op("OpFlowBehavior", 0x1C, "flow end");
        w.branch(end, u32::from(crate::igz::layouts::BRANCH_END));
        w.close_at(over);
    }
    w.close(start);
    w.finish(list_at);
}

/// The readers' seeds: the sample, one without spillover or pairs, and one
/// whose body ends in a loop of 3, so a mutation has a count to blow up.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut short = sample();
    short
        .rows
        .retain(|r| !matches!(r, RowSpec::Spill { .. } | RowSpec::Pairs { .. }));
    let mut looped = sample();
    looped.bare_loop = Some(3);
    [sample(), short, looped]
        .iter()
        .map(|s| bundle(&disc(s)))
        .collect()
}

/// "Game - Award Money"'s seeds: `global.bld` levels each with one clamp
/// (the registry's seeds must read), the sample's, a bigger one and 1.
pub fn clamp_seeds() -> Vec<Vec<u8>> {
    [9000, 65_000, 1]
        .into_iter()
        .map(|c| {
            disc(&ProgressionSpec {
                clamp: ClampSpec::One(c),
                ..sample()
            })
            .global
        })
        .collect()
}
