//! Giants' script state as guest memory holds it, read at a pause through
//! PINE (openspec AH.4, design decision 16): the named cells the tfbScript VM
//! keeps, a placement's script instance, and `global.bld`'s exports by name.
//! [`crate::fight`] builds the fight's readers on these.
//!
//! The layout, Giants 01.00 (`BLUS30968`), offsets from each object's start,
//! big-endian, found in AF.8's fresh state (aj-vm's reading, measured there):
//! - Every script cell is a named object: its name at +0x08. A number is a
//!   `ValueInfo`, its type at +0x1C (the `IntMeasurement` or
//!   `FloatMeasurement` metaobject, named at +0x08: a number retypes itself to
//!   what it's given, so only the type word says how to read the value) and its
//!   value at +0x20. A reference (`ScriptReference`, `PlacementReference`, and
//!   the reference to a set) names its object at +0x1C, 0 for null. A
//!   structure (`ScriptStructure`) holds its fields in an `igObjectList` at
//!   +0x18; a set (`ScriptSet`) its elements in a list of its own class at
//!   +0x18. Both lists are `igDataList`s ([`crate::actors`]): count +0x08,
//!   capacity +0x0C, the array +0x14.
//! - A placement's script instance lives on its `tfbActorInfo`: the flags at
//!   +0x1C (0x8 once its STARTUP has run), `_activityState` +0x54 (1 active, 2
//!   inactive, 3 removed: AG.7's order), `_script` +0xA8, the behaviour it runs
//!   (`_execBehavior` +0xAC, an `OpUserBehavior` named at +0x08), its
//!   variables (`_localVarList` +0xB0, an `igObjectList` its STARTUP builds, so
//!   0 for a placement whose script never started; the dynamic variables
//!   scripts make on it by name join the same list) and `_activeTime` +0xB4,
//!   the seconds it has been active, which `system.script timer` reads.
//! - `global.bld`'s exports are handles in Alchemy's handle manager (the static
//!   at [`HANDLE_MANAGER`]): its table (+0x24) keeps `capacity` (+0x08)
//!   8-byte keys (+0x14: the namespace's FNV-1a hash, then the export's id)
//!   beside a handle pointer each (+0x0C). A handle repeats its id at +0x08 and
//!   its namespace's hash at +0x10 and points at its object at +0x14. The same
//!   arrays hold other entries (empty slots filled with 0xFAFAFAFA, entries
//!   keyed by an object): only a handle whose id and hash match its key is
//!   one.
//!
//! Every pointer is checked against its class's vtable before it's followed
//! and a chain that breaks is refused naming the link, as in
//! [`crate::actors`]. Every address is the 01.00 EBOOT's.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{self, Gather, be32, c_string, expect_vtable, f32_at, object_ptr, one};

/// The script runtime's classes and Alchemy's handle table, every
/// instance's first word.
pub mod vtable {
    pub const VALUE_INFO: u32 = 0x0062_3A40;
    pub const SCRIPT_REFERENCE: u32 = 0x0062_3CB8;
    pub const PLACEMENT_REFERENCE: u32 = 0x0062_3D30;
    pub const SET_REFERENCE: u32 = 0x0062_3E18;
    pub const SCRIPT_STRUCTURE: u32 = 0x0062_5C90;
    pub const OBJECT_LIST: u32 = 0x0062_4180;
    pub const SCRIPT_SET: u32 = 0x0062_37C0;
    /// The class of the list a `ScriptSet`'s +0x18 points at, not the
    /// structures' `igObjectList`.
    pub const SET_LIST: u32 = 0x0062_3838;
    pub const USER_BEHAVIOR: u32 = 0x0062_2EE0;
    pub const HANDLE_MANAGER: u32 = 0x0063_44B0;
    pub const HANDLE_TABLE: u32 = 0x0063_CB8C;
    pub const ACTOR_INFO: u32 = crate::actors::vtable::ACTOR_INFO;
}

/// The EBOOT's static slot holding Alchemy's handle manager.
pub const HANDLE_MANAGER: u32 = 0x0089_D518;

/// The namespace `global.bld`'s exports are keyed under.
pub const GLOBAL: &str = "global.bld";

const NAME: usize = 0x08;
const HEAD: usize = 0x0C;
const NAME_LEN: usize = 64;
const VALUE_TYPE: usize = 0x1C;
const VALUE: usize = 0x20;
const VALUE_LEN: usize = 0x24;
const REF_OBJECT: usize = 0x1C;
const REF_LEN: usize = 0x20;
const LIST_AT: usize = 0x18;
const CONTAINER_LEN: usize = 0x1C;
const PLACEMENT_FLAGS: usize = 0x1C;
const PLACEMENT_STATE: usize = 0x54;
const PLACEMENT_SCRIPT: usize = 0xA8;
const PLACEMENT_BEHAVIOUR: usize = 0xAC;
const PLACEMENT_LOCALS: usize = 0xB0;
const PLACEMENT_ACTIVE_TIME: usize = 0xB4;
const PLACEMENT_LEN: usize = 0xB8;
/// The flag a placement's STARTUP sets.
pub const STARTED: u32 = 0x8;
const MANAGER_TABLE: usize = 0x24;
const MANAGER_LEN: usize = 0x28;
const TABLE_CAPACITY: usize = 0x08;
const TABLE_VALUES: usize = 0x0C;
const TABLE_KEYS: usize = 0x14;
const TABLE_LEN: usize = 0x18;
const HANDLE_ID: usize = 0x08;
const HANDLE_NAMESPACE: usize = 0x10;
const HANDLE_OBJECT: usize = 0x14;
const HANDLE_LEN: usize = 0x18;
/// Slots a handle table may have before it's taken for garbage: AF.8's fresh
/// state's has 53,000.
const MAX_HANDLES: u32 = 1 << 20;
/// Cells a structure or a placement holds at most before its list is taken for
/// garbage: `Macros_UI` keeps 92 variables, the console save params 51 fields.
pub const MAX_FIELDS: usize = 1024;

/// FNV-1a over `s`'s bytes, the hash Alchemy keys its namespaces by.
pub fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(0x811C_9DC5, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// A number as its cell holds it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Number {
    Int(i32),
    Float(f32),
}

impl Number {
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Int(v) => f64::from(v),
            Self::Float(v) => f64::from(v),
        }
    }

    /// The value as the VM's integer cast would take it (truncating).
    pub fn as_int(self) -> i64 {
        match self {
            Self::Int(v) => i64::from(v),
            Self::Float(v) => v as i64,
        }
    }
}

impl std::fmt::Display for Number {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}i"),
            Self::Float(v) => write!(f, "{v}"),
        }
    }
}

/// A named script object: where it is, its class and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub at: u32,
    pub vtable: u32,
    pub name: String,
}

/// What a named object holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(Number),
    /// A script or placement reference, or a reference to a set: the object
    /// it names (none for null) and that object's own name when it has one.
    Reference {
        to: Option<u32>,
        name: Option<String>,
    },
    Structure {
        fields: u32,
    },
    Set {
        count: u32,
    },
    /// A class none of the above.
    Other {
        vtable: u32,
    },
}

/// The two number types, named at their metaobjects' +0x08, cached by type
/// word.
#[derive(Debug, Default, Clone)]
pub struct Types(BTreeMap<u32, Option<bool>>);

impl Types {
    /// The `FloatMeasurement` metaobject, once a float cell has been read.
    pub fn float_type(&self) -> Option<u32> {
        self.0
            .iter()
            .find(|(_, k)| **k == Some(true))
            .map(|(&ty, _)| ty)
    }

    /// Whether `ty` is the `FloatMeasurement` metaobject (`Some(true)`), the
    /// `IntMeasurement` one (`Some(false)`), or neither (`None`). A null type
    /// word reads as an integer, as the VM's evaluators take it.
    fn is_float(&mut self, g: &mut impl Gather, ty: u32) -> Result<Option<bool>> {
        if ty == 0 {
            return Ok(Some(false));
        }
        if let Some(&known) = self.0.get(&ty) {
            return Ok(known);
        }
        let head = one(g, ty, HEAD, "a number's type")?;
        let name = one(g, be32(&head, NAME), NAME_LEN, "a type's name")?;
        let known = match c_string(&name).as_str() {
            "FloatMeasurement" => Some(true),
            "IntMeasurement" => Some(false),
            _ => None,
        };
        self.0.insert(ty, known);
        Ok(known)
    }
}

/// The value word decoded by its type word; refused when the type is neither
/// number type (the name of what it is in the error).
pub fn decode(g: &mut impl Gather, types: &mut Types, bytes: &[u8], at: u32) -> Result<Number> {
    let ty = be32(bytes, VALUE_TYPE);
    match types.is_float(g, ty)? {
        Some(true) => Ok(Number::Float(f32_at(bytes, VALUE))),
        Some(false) => Ok(Number::Int(be32(bytes, VALUE) as i32)),
        None => bail!("the number at {at:#010x} has type {ty:#010x}, neither number type"),
    }
}

/// The number a `ValueInfo` holds, its class checked.
pub fn number(g: &mut impl Gather, types: &mut Types, at: u32, what: &str) -> Result<Number> {
    let b = one(g, at, VALUE_LEN, what)?;
    expect_vtable(&b, vtable::VALUE_INFO, at, "ValueInfo").with_context(|| what.to_owned())?;
    decode(g, types, &b, at).with_context(|| what.to_owned())
}

/// Each object's class and name, in two batched reads; an object that isn't
/// readable or has no name is left out.
pub fn named(g: &mut impl Gather, at: &[u32]) -> Result<Vec<Named>> {
    let ranges: Vec<(u32, usize)> = at
        .iter()
        .filter(|&&a| object_ptr(a))
        .map(|&a| (a, HEAD))
        .collect();
    let heads = g.gather(&ranges)?;
    let found: Vec<(u32, u32, u32)> = ranges
        .iter()
        .zip(heads)
        .filter_map(|(&(a, _), h)| h.map(|h| (a, be32(&h, 0), be32(&h, NAME))))
        .filter(|&(_, _, n)| object_ptr(n))
        .collect();
    let names = g.gather(
        &found
            .iter()
            .map(|&(_, _, n)| (n, NAME_LEN))
            .collect::<Vec<_>>(),
    )?;
    Ok(found
        .iter()
        .zip(names)
        .filter_map(|(&(at, vtable, _), n)| {
            n.map(|n| Named {
                at,
                vtable,
                name: c_string(&n),
            })
        })
        .collect())
}

/// An `igDataList` held at +0x18 of a container whose class is `want`,
/// the list's own class `list_class`, held to [`MAX_FIELDS`].
fn container(
    g: &mut impl Gather,
    at: u32,
    want: u32,
    class: &str,
    list_class: u32,
    what: &str,
) -> Result<Vec<u32>> {
    let head = one(g, at, CONTAINER_LEN, what)?;
    expect_vtable(&head, want, at, class).with_context(|| what.to_owned())?;
    let items = actors::list(
        g,
        be32(&head, LIST_AT),
        list_class,
        &format!("{what}'s list"),
    )?;
    if items.len() > MAX_FIELDS {
        bail!("{what} at {at:#010x} holds {} entries", items.len());
    }
    Ok(items)
}

/// A structure's fields, its class checked.
pub fn fields(g: &mut impl Gather, at: u32, what: &str) -> Result<Vec<Named>> {
    let items = container(
        g,
        at,
        vtable::SCRIPT_STRUCTURE,
        "ScriptStructure",
        vtable::OBJECT_LIST,
        what,
    )?;
    named(g, &items)
}

/// A set's elements, its class checked.
pub fn set_items(g: &mut impl Gather, at: u32, what: &str) -> Result<Vec<u32>> {
    container(
        g,
        at,
        vtable::SCRIPT_SET,
        "ScriptSet",
        vtable::SET_LIST,
        what,
    )
}

/// What a reference names: none for a null one, refused for any other class.
pub fn referent(g: &mut impl Gather, reference: &Named) -> Result<Option<u32>> {
    if !matches!(
        reference.vtable,
        vtable::SCRIPT_REFERENCE | vtable::PLACEMENT_REFERENCE | vtable::SET_REFERENCE
    ) {
        bail!(
            "`{}` at {:#010x} has vtable {:#010x}, not a reference's",
            reference.name,
            reference.at,
            reference.vtable
        );
    }
    let r = one(g, reference.at, REF_LEN, &format!("`{}`", reference.name))?;
    let to = be32(&r, REF_OBJECT);
    Ok((to != 0).then_some(to))
}

/// The one cell named `name` among `cells`, refused when there's none or more
/// than one.
pub fn the<'a>(cells: &'a [Named], name: &str, what: &str) -> Result<&'a Named> {
    let mut found = cells.iter().filter(|c| c.name == name);
    match (found.next(), found.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => bail!("no `{name}` among {what}'s {} cells", cells.len()),
        (Some(_), Some(_)) => bail!("more than one `{name}` among {what}'s cells"),
    }
}

/// The cell at the end of `path` from `cells`: each step a cell by its name,
/// a structure's fields followed into, a reference followed to the structure
/// or placement it names (a placement's variables then). Refused at the step
/// that breaks, naming it.
pub fn walk(g: &mut impl Gather, cells: &[Named], path: &[&str], what: &str) -> Result<Named> {
    let Some((last, steps)) = path.split_last() else {
        bail!("an empty path into {what}");
    };
    let mut cells = cells.to_vec();
    let mut here = what.to_owned();
    for step in steps {
        let cell = the(&cells, step, &here)?.clone();
        here = format!("{here}.{step}");
        let structure = match cell.vtable {
            vtable::SCRIPT_STRUCTURE => cell.at,
            vtable::SCRIPT_REFERENCE | vtable::PLACEMENT_REFERENCE => {
                referent(g, &cell)?.with_context(|| format!("{here} is a null reference"))?
            }
            other => bail!("{here} has vtable {other:#010x}, which holds no cells"),
        };
        let head = one(g, structure, 4, &here)?;
        cells = match be32(&head, 0) {
            vtable::SCRIPT_STRUCTURE => fields(g, structure, &here)?,
            vtable::ACTOR_INFO => variables(g, structure)?
                .with_context(|| format!("{here} names a placement whose script never started"))?,
            other => bail!(
                "{here} names {structure:#010x}, vtable {other:#010x}: neither a structure nor a \
                 placement"
            ),
        };
    }
    the(&cells, last, &here).cloned()
}

/// What a named object holds.
pub fn value(g: &mut impl Gather, types: &mut Types, cell: &Named) -> Result<Value> {
    Ok(match cell.vtable {
        vtable::VALUE_INFO => {
            Value::Number(number(g, types, cell.at, &format!("`{}`", cell.name))?)
        }
        vtable::SCRIPT_REFERENCE | vtable::PLACEMENT_REFERENCE | vtable::SET_REFERENCE => {
            let to = referent(g, cell)?;
            let name = match to {
                Some(o) => named(g, &[o])?.pop().map(|n| n.name),
                None => None,
            };
            Value::Reference { to, name }
        }
        vtable::SCRIPT_STRUCTURE => Value::Structure {
            fields: fields(g, cell.at, &format!("`{}`", cell.name))?.len() as u32,
        },
        vtable::SCRIPT_SET => Value::Set {
            count: set_items(g, cell.at, &format!("`{}`", cell.name))?.len() as u32,
        },
        vtable => Value::Other { vtable },
    })
}

/// The cell at the end of `path` and the number it holds, refused when the
/// walk breaks or the cell isn't a number.
pub fn number_at(
    g: &mut impl Gather,
    types: &mut Types,
    cells: &[Named],
    path: &[&str],
    what: &str,
) -> Result<(Named, Number)> {
    let cell = walk(g, cells, path, what)?;
    let n = number(g, types, cell.at, &format!("{what}.{}", path.join(".")))?;
    Ok((cell, n))
}

/// The structure or placement a reference at the end of `path` names, or the
/// structure itself when the path ends on one.
pub fn structure_at(
    g: &mut impl Gather,
    cells: &[Named],
    path: &[&str],
    what: &str,
) -> Result<u32> {
    let cell = walk(g, cells, path, what)?;
    match cell.vtable {
        vtable::SCRIPT_STRUCTURE => Ok(cell.at),
        _ => referent(g, &cell)?
            .with_context(|| format!("{what}.{} is a null reference", path.join("."))),
    }
}

/// A placement's variables: none when its script never started (no list).
pub fn variables(g: &mut impl Gather, placement: u32) -> Result<Option<Vec<Named>>> {
    let p = one(g, placement, PLACEMENT_LEN, "a placement")?;
    expect_vtable(&p, vtable::ACTOR_INFO, placement, "tfbActorInfo")?;
    let list = be32(&p, PLACEMENT_LOCALS);
    if list == 0 {
        return Ok(None);
    }
    let items = actors::list(g, list, vtable::OBJECT_LIST, "a placement's variables")?;
    if items.len() > MAX_FIELDS {
        bail!(
            "the placement at {placement:#010x} holds {} variables",
            items.len()
        );
    }
    Ok(Some(named(g, &items)?))
}

/// A placement's script instance as the pause left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub placement: u32,
    pub name: Option<String>,
    /// `_activityState`: 1 active, 2 inactive, 3 removed, 4 unmanaged, 5 a
    /// template.
    pub activity_state: u32,
    /// The flags' 0x8: its STARTUP has run.
    pub started: bool,
    /// `_activeTime`, the seconds it has been active.
    pub active_seconds: f32,
    /// Its script (`_script`), none for a placement without one.
    pub script: Option<u32>,
    /// The behaviour it runs, by name; none before its STARTUP picks one.
    pub behaviour: Option<String>,
    /// Its variables by name, none when its script never started.
    pub variables: Option<Vec<Named>>,
}

impl Instance {
    /// Reads `placement`'s instance: refused when it isn't a `tfbActorInfo`, or
    /// its behaviour pointer names anything but an `OpUserBehavior`.
    pub fn read(g: &mut impl Gather, placement: u32) -> Result<Self> {
        let p = one(g, placement, PLACEMENT_LEN, "a placement")?;
        expect_vtable(&p, vtable::ACTOR_INFO, placement, "tfbActorInfo")?;
        let name = named(g, &[placement])?.pop().map(|n| n.name);
        let behaviour = match be32(&p, PLACEMENT_BEHAVIOUR) {
            0 => None,
            at => {
                let what = format!("the behaviour of {placement:#010x}");
                let head = one(g, at, HEAD, &what)?;
                expect_vtable(&head, vtable::USER_BEHAVIOR, at, "OpUserBehavior")
                    .with_context(|| what.clone())?;
                Some(
                    named(g, &[at])?
                        .pop()
                        .with_context(|| format!("{what} has no name"))?
                        .name,
                )
            }
        };
        Ok(Self {
            placement,
            name,
            activity_state: be32(&p, PLACEMENT_STATE),
            started: be32(&p, PLACEMENT_FLAGS) & STARTED != 0,
            active_seconds: f32_at(&p, PLACEMENT_ACTIVE_TIME),
            script: Some(be32(&p, PLACEMENT_SCRIPT)).filter(|&s| s != 0),
            behaviour,
            variables: variables(g, placement)?,
        })
    }

    /// The number at `path` through its variables; none when its script never
    /// started.
    pub fn number(
        &self,
        g: &mut impl Gather,
        types: &mut Types,
        path: &[&str],
    ) -> Result<Option<Number>> {
        let Some(cells) = &self.variables else {
            return Ok(None);
        };
        let what = self
            .name
            .clone()
            .unwrap_or_else(|| format!("{:#010x}", self.placement));
        Ok(Some(number_at(g, types, cells, path, &what)?.1))
    }
}

/// One export as the handle table holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    pub id: u32,
    pub at: u32,
    pub vtable: u32,
}

/// Each of `names` among `namespace`'s exports, by the name its object
/// carries: refused when the handle manager or its table isn't where it
/// should be, or a name is missing or held by two exports.
pub fn exports(
    g: &mut impl Gather,
    namespace: &str,
    names: &[&str],
) -> Result<BTreeMap<String, Export>> {
    let manager = be32(&one(g, HANDLE_MANAGER, 4, "the handle manager pointer")?, 0);
    let m = one(g, manager, MANAGER_LEN, "the handle manager")?;
    expect_vtable(&m, vtable::HANDLE_MANAGER, manager, "igObjectHandleManager")?;
    let table = be32(&m, MANAGER_TABLE);
    let t = one(g, table, TABLE_LEN, "the handle table")?;
    expect_vtable(&t, vtable::HANDLE_TABLE, table, "the handle table")?;
    let capacity = be32(&t, TABLE_CAPACITY);
    if capacity == 0 || capacity > MAX_HANDLES {
        bail!("the handle table at {table:#010x} has {capacity} slots");
    }
    let n = capacity as usize;
    let keys = one(g, be32(&t, TABLE_KEYS), 8 * n, "the handle table's keys")?;
    let values = one(
        g,
        be32(&t, TABLE_VALUES),
        4 * n,
        "the handle table's handles",
    )?;
    let hash = fnv1a(namespace);
    let candidates: Vec<(u32, u32)> = (0..n)
        .filter(|&i| be32(&keys, 8 * i) == hash)
        .map(|i| (be32(&keys, 8 * i + 4), be32(&values, 4 * i)))
        .filter(|&(_, h)| object_ptr(h))
        .collect();
    let handles = g.gather(
        &candidates
            .iter()
            .map(|&(_, h)| (h, HANDLE_LEN))
            .collect::<Vec<_>>(),
    )?;
    let objects: Vec<(u32, u32)> = candidates
        .iter()
        .zip(handles)
        .filter_map(|(&(id, _), h)| {
            let h = h?;
            (be32(&h, HANDLE_ID) == id && be32(&h, HANDLE_NAMESPACE) == hash)
                .then(|| (id, be32(&h, HANDLE_OBJECT)))
        })
        .filter(|&(_, o)| object_ptr(o))
        .collect();
    let named_objects = named(g, &objects.iter().map(|&(_, o)| o).collect::<Vec<_>>())?;
    let id_of: BTreeMap<u32, u32> = objects.iter().map(|&(id, o)| (o, id)).collect();
    let mut out = BTreeMap::new();
    for &want in names {
        let found: Vec<&Named> = named_objects.iter().filter(|o| o.name == want).collect();
        match found[..] {
            [one] => {
                out.insert(
                    want.to_owned(),
                    Export {
                        id: id_of[&one.at],
                        at: one.at,
                        vtable: one.vtable,
                    },
                );
            }
            [] => bail!(
                "no export named `{want}` among {namespace}'s {} handles",
                named_objects.len()
            ),
            _ => bail!(
                "{} of {namespace}'s exports are named `{want}`",
                found.len()
            ),
        }
    }
    Ok(out)
}

/// `bits` bits from bit `bit` of `words` taken as one little-endian stream (bit
/// 0 the first word's lowest), the way the scripts' bit-field macros read a
/// save area: a value may run on into the next word. `signed` reads the top
/// bit as the sign. Refused past the words or over 32 bits.
pub fn bit_field(words: &[u32], bit: u32, bits: u32, signed: bool) -> Result<i64> {
    if bits == 0 || bits > 32 {
        bail!("a bit field of {bits} bits");
    }
    let (w, b) = ((bit / 32) as usize, bit % 32);
    let Some(&first) = words.get(w) else {
        bail!("bit {bit} is past the {} words", words.len());
    };
    let mut v = u64::from(first) >> b;
    if b + bits > 32 {
        let Some(&next) = words.get(w + 1) else {
            bail!(
                "bits {bit} to {} run past the {} words",
                bit + bits - 1,
                words.len()
            );
        };
        v |= u64::from(next) << (32 - b);
    }
    let v = v & ((1u64 << bits) - 1);
    Ok(if signed && v >> (bits - 1) == 1 {
        v as i64 - (1i64 << bits)
    } else {
        v as i64
    })
}

/// Script objects written out in code for the tests here and in
/// [`crate::fight`], on [`crate::actors::fixture::World`]'s heap, laid out as
/// AF.8's fresh state has them. The offsets are typed in, not taken from the
/// reader's constants.
#[cfg(test)]
pub(crate) mod fixture {
    use super::vtable;
    use crate::actors::fixture::World;

    /// The two number types, named.
    pub struct Types {
        pub int: u32,
        pub float: u32,
    }

    pub fn types(w: &mut World) -> Types {
        Types {
            int: named(w, 0x0063_4C88, 0x30, "IntMeasurement"),
            float: named(w, 0x0063_4C88, 0x30, "FloatMeasurement"),
        }
    }

    pub fn named(w: &mut World, vt: u32, len: usize, name: &str) -> u32 {
        let at = w.object(vt, len);
        let n = w.string(name);
        w.put32(at + 0x08, n);
        at
    }

    pub fn int(w: &mut World, t: &Types, name: &str, v: i32) -> u32 {
        let at = named(w, vtable::VALUE_INFO, 0x24, name);
        w.put32(at + 0x1C, t.int);
        w.put32(at + 0x20, v as u32);
        at
    }

    pub fn float(w: &mut World, t: &Types, name: &str, v: f32) -> u32 {
        let at = named(w, vtable::VALUE_INFO, 0x24, name);
        w.put32(at + 0x1C, t.float);
        w.putf(at + 0x20, v);
        at
    }

    pub fn reference(w: &mut World, vt: u32, name: &str, to: u32) -> u32 {
        let at = named(w, vt, 0x38, name);
        w.put32(at + 0x1C, to);
        at
    }

    pub fn structure(w: &mut World, name: &str, fields: &[u32]) -> u32 {
        let at = named(w, vtable::SCRIPT_STRUCTURE, 0x20, name);
        let list = w.list(vtable::OBJECT_LIST, fields, fields.len().max(1) as u32);
        w.put32(at + 0x18, list);
        at
    }

    pub fn set(w: &mut World, name: &str, items: &[u32]) -> u32 {
        let at = named(w, vtable::SCRIPT_SET, 0x20, name);
        let list = w.list(vtable::SET_LIST, items, items.len().max(1) as u32);
        w.put32(at + 0x18, list);
        at
    }

    /// A placement with a started script: flags 0x8, active, `active` seconds
    /// in, running `behaviour`, holding `variables`.
    pub fn placement(
        w: &mut World,
        name: &str,
        behaviour: Option<&str>,
        active: f32,
        variables: Option<&[u32]>,
    ) -> u32 {
        let p = named(w, vtable::ACTOR_INFO, 0xF8, name);
        w.put32(p + 0x54, 1);
        w.putf(p + 0xB4, active);
        if let Some(vars) = variables {
            w.put32(p + 0x1C, 0x8);
            let script = w.object(0x0062_3508, 0x40);
            w.put32(p + 0xA8, script);
            let list = w.list(vtable::OBJECT_LIST, vars, vars.len().max(1) as u32);
            w.put32(p + 0xB0, list);
        }
        if let Some(b) = behaviour {
            let op = named(w, vtable::USER_BEHAVIOR, 0x28, b);
            w.put32(p + 0xAC, op);
        }
        p
    }

    /// The handle manager at its static, its table holding `exports` (name
    /// hash of `namespace`, id, object) among an empty slot and a slot keyed
    /// by an object (both of which the walk has to pass over).
    pub fn handles(w: &mut World, namespace: &str, exports: &[(u32, u32)]) -> u32 {
        let manager = w.object(vtable::HANDLE_MANAGER, 0x30);
        w.put32(super::HANDLE_MANAGER, manager);
        let table = w.object(vtable::HANDLE_TABLE, 0x20);
        w.put32(manager + 0x24, table);
        let n = exports.len() as u32 + 2;
        w.put32(table + 0x08, n);
        let keys = w.alloc(8 * n as usize);
        let values = w.alloc(4 * n as usize);
        w.put32(table + 0x0C, values);
        w.put32(table + 0x14, keys);
        let hash = super::fnv1a(namespace);
        // An empty slot, then an entry keyed by an object whose "handle" is a
        // hash, then the exports.
        w.put32(keys, 0xFAFA_FAFA);
        w.put32(keys + 4, 0xFAFA_FAFA);
        w.put32(values, 0xFAFA_FAFA);
        w.put32(keys + 8, hash);
        w.put32(keys + 12, 0x3000_0040);
        w.put32(values + 4, 0x1002_CE9C);
        for (i, &(id, object)) in exports.iter().enumerate() {
            let i = i as u32 + 2;
            let h = w.alloc(0x18);
            w.put32(h, 2);
            w.put32(h + 0x08, id);
            w.put32(h + 0x10, hash);
            w.put32(h + 0x14, object);
            w.put32(keys + 8 * i, hash);
            w.put32(keys + 8 * i + 4, id);
            w.put32(values + 4 * i, h);
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{self as fx, Types as FxTypes};
    use super::*;
    use crate::actors::fixture::World;

    fn err(r: Result<impl std::fmt::Debug>) -> String {
        format!("{:#}", r.unwrap_err())
    }

    /// A creature's cells as AF.8's fresh state lays them out for Eruptor:
    /// his description's creature info (hp, armor, the character parameters'
    /// hp max), its attention parameters, and a placement holding it.
    struct Creature {
        w: World,
        t: FxTypes,
        placement: u32,
        hp: u32,
        info: u32,
        description_ref: u32,
    }

    fn creature() -> Creature {
        let mut w = World::new();
        let t = fx::types(&mut w);
        let hp_max = fx::int(&mut w, &t, "hp max", 290);
        let exp = fx::float(&mut w, &t, "exp value", 1.0);
        let params = fx::structure(&mut w, "char vars base", &[exp, hp_max]);
        let params_ref = fx::reference(
            &mut w,
            vtable::SCRIPT_REFERENCE,
            "character parameters",
            params,
        );
        let hp = fx::int(&mut w, &t, "hp", 290);
        let armor = fx::float(&mut w, &t, "armor", 6.0);
        let info = fx::structure(&mut w, "", &[params_ref, armor, hp]);
        let alerted = fx::int(&mut w, &t, "alerted", 0);
        let attention = fx::structure(&mut w, "attention info", &[alerted]);
        let attention_ref = fx::reference(
            &mut w,
            vtable::SCRIPT_REFERENCE,
            "attention parameters",
            attention,
        );
        let info_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "creature info", info);
        let description = fx::structure(&mut w, "", &[info_ref, attention_ref]);
        let description_ref =
            fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "description", description);
        let timer = fx::int(&mut w, &t, "Timer", 0);
        let placement = fx::placement(
            &mut w,
            "009_Eruptor",
            Some("default"),
            12.5,
            Some(&[timer, description_ref]),
        );
        Creature {
            w,
            t,
            placement,
            hp,
            info,
            description_ref,
        }
    }

    #[test]
    fn an_instance_reads_its_state_behaviour_and_cells_by_path() {
        let mut c = creature();
        let i = Instance::read(&mut c.w, c.placement).unwrap();
        assert_eq!(i.name.as_deref(), Some("009_Eruptor"));
        assert_eq!(
            (i.activity_state, i.started, i.active_seconds),
            (1, true, 12.5)
        );
        assert!(i.script.is_some());
        assert_eq!(i.behaviour.as_deref(), Some("default"));
        let names: Vec<&str> = i
            .variables
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        assert_eq!(names, ["Timer", "description"]);
        let mut types = Types::default();
        let hp = i
            .number(
                &mut c.w,
                &mut types,
                &["description", "creature info", "hp"],
            )
            .unwrap();
        assert_eq!(hp, Some(Number::Int(290)));
        let max = i
            .number(
                &mut c.w,
                &mut types,
                &[
                    "description",
                    "creature info",
                    "character parameters",
                    "hp max",
                ],
            )
            .unwrap();
        assert_eq!(max, Some(Number::Int(290)));
        let armor = i
            .number(
                &mut c.w,
                &mut types,
                &["description", "creature info", "armor"],
            )
            .unwrap();
        assert_eq!(armor, Some(Number::Float(6.0)));
        let alerted = i
            .number(
                &mut c.w,
                &mut types,
                &["description", "attention parameters", "alerted"],
            )
            .unwrap();
        assert_eq!(alerted, Some(Number::Int(0)));
        // A cell retyped to a float reads as one.
        c.w.put32(c.hp + 0x1C, c.t.float);
        c.w.putf(c.hp + 0x20, 278.5);
        let hp = i
            .number(
                &mut c.w,
                &mut types,
                &["description", "creature info", "hp"],
            )
            .unwrap();
        assert_eq!(hp, Some(Number::Float(278.5)));
        // A value of each kind.
        let vars = i.variables.clone().unwrap();
        let desc = the(&vars, "description", "Eruptor").unwrap();
        match value(&mut c.w, &mut types, desc).unwrap() {
            Value::Reference { to: Some(_), name } => assert_eq!(name.as_deref(), Some("")),
            other => panic!("{other:?}"),
        }
        let info = walk(
            &mut c.w,
            &vars,
            &["description", "creature info"],
            "Eruptor",
        )
        .unwrap();
        assert!(matches!(
            value(&mut c.w, &mut types, &info).unwrap(),
            Value::Reference { .. }
        ));
        let s = Named {
            at: c.info,
            vtable: vtable::SCRIPT_STRUCTURE,
            name: String::new(),
        };
        assert_eq!(
            value(&mut c.w, &mut types, &s).unwrap(),
            Value::Structure { fields: 3 }
        );
    }

    #[test]
    fn a_placement_whose_script_never_started_has_no_cells() {
        let mut w = World::new();
        let p = fx::placement(&mut w, "Enemy_Chompy(1)", None, 0.0, None);
        w.put32(p + 0x54, 2);
        let i = Instance::read(&mut w, p).unwrap();
        assert_eq!(
            (
                i.activity_state,
                i.started,
                i.active_seconds,
                i.behaviour.clone()
            ),
            (2, false, 0.0, None)
        );
        assert!(i.variables.is_none() && i.script.is_none());
        assert_eq!(
            i.number(
                &mut w,
                &mut Types::default(),
                &["description", "creature info", "hp"]
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn each_break_of_a_cells_chain_is_refused_naming_it() {
        let path = ["description", "creature info", "hp"];
        let read = |c: &mut Creature| {
            Instance::read(&mut c.w, c.placement)
                .and_then(|i| i.number(&mut c.w, &mut Types::default(), &path))
        };
        // Not a placement.
        let mut c = creature();
        c.w.put32(c.placement, vtable::VALUE_INFO);
        assert!(err(read(&mut c)).contains("tfbActorInfo"));
        // The behaviour pointer names something else.
        let mut c = creature();
        let op = c.w.get32(c.placement + 0xAC);
        c.w.put32(op, vtable::SCRIPT_STRUCTURE);
        assert!(err(read(&mut c)).contains("OpUserBehavior"));
        // The variable list isn't one.
        let mut c = creature();
        let list = c.w.get32(c.placement + 0xB0);
        c.w.put32(list, vtable::SET_LIST);
        assert!(err(read(&mut c)).contains("variables"));
        // A step missing.
        let mut c = creature();
        let name = c.w.get32(c.description_ref + 0x08);
        c.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read(&mut c)).contains("no `description`"));
        // A reference that's null.
        let mut c = creature();
        c.w.put32(c.description_ref + 0x1C, 0);
        assert!(err(read(&mut c)).contains("null reference"));
        // A reference to something neither a structure nor a placement.
        let mut c = creature();
        let desc = c.w.get32(c.description_ref + 0x1C);
        c.w.put32(desc, vtable::VALUE_INFO);
        assert!(err(read(&mut c)).contains("neither a structure nor a placement"));
        // The structure's field list isn't one.
        let mut c = creature();
        let list = c.w.get32(c.info + 0x18);
        c.w.put32(list, vtable::SET_LIST);
        assert!(err(read(&mut c)).contains("creature info"));
        // The cell at the end isn't a number.
        let mut c = creature();
        c.w.put32(c.hp, vtable::SCRIPT_REFERENCE);
        assert!(err(read(&mut c)).contains("ValueInfo"));
        // Its type is neither number type.
        let mut c = creature();
        let other = fx::named(&mut c.w, 0x0063_4C88, 0x30, "ColorMeasurement");
        c.w.put32(c.hp + 0x1C, other);
        assert!(err(read(&mut c)).contains("neither number type"));
        // Two cells of one name.
        let mut c = creature();
        let list = c.w.get32(c.placement + 0xB0);
        let data = c.w.get32(list + 0x14);
        let twin = c.w.get32(data + 4);
        c.w.put32(data, twin);
        assert!(err(read(&mut c)).contains("more than one `description`"));
        // A stepped-through cell that holds no cells.
        let mut c = creature();
        let vars = Instance::read(&mut c.w, c.placement)
            .unwrap()
            .variables
            .unwrap();
        let e = err(walk(&mut c.w, &vars, &["Timer", "x"], "Eruptor"));
        assert!(e.contains("holds no cells"), "{e}");
    }

    #[test]
    fn exports_are_found_by_their_objects_names() {
        let mut w = World::new();
        let t = fx::types(&mut w);
        let flag = fx::int(&mut w, &t, "ZoneElement_P1_Correct", 0);
        let other = fx::int(&mut w, &t, "Difficulty_Mode", 0);
        let ui = fx::placement(&mut w, "Macros_UI", None, 99.5, Some(&[]));
        fx::handles(
            &mut w,
            GLOBAL,
            &[(0xA21, flag), (0xA16, other), (0x4B9, ui)],
        );
        let got = exports(&mut w, GLOBAL, &["ZoneElement_P1_Correct", "Macros_UI"]).unwrap();
        assert_eq!(
            got["ZoneElement_P1_Correct"],
            Export {
                id: 0xA21,
                at: flag,
                vtable: vtable::VALUE_INFO
            }
        );
        assert_eq!(got["Macros_UI"].at, ui);
        assert_eq!(got.len(), 2);
        // Another namespace's export of the same name isn't one.
        let mut w = World::new();
        let t = fx::types(&mut w);
        let flag = fx::int(&mut w, &t, "ZoneElement_P1_Correct", 0);
        fx::handles(&mut w, "level.bld", &[(0xA21, flag)]);
        assert!(err(exports(&mut w, GLOBAL, &["ZoneElement_P1_Correct"])).contains("no export"));
    }

    #[test]
    fn each_break_of_the_handle_tables_chain_is_refused() {
        let setup = || {
            let mut w = World::new();
            let t = fx::types(&mut w);
            let flag = fx::int(&mut w, &t, "ZoneElement_P1_Correct", 0);
            let table = fx::handles(&mut w, GLOBAL, &[(0xA21, flag)]);
            (w, table, flag)
        };
        let want = ["ZoneElement_P1_Correct"];
        // No manager.
        let (mut w, ..) = setup();
        w.put32(HANDLE_MANAGER, 0);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("handle manager"));
        // The manager isn't one.
        let (mut w, ..) = setup();
        let m = w.get32(HANDLE_MANAGER);
        w.put32(m, vtable::HANDLE_TABLE);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("igObjectHandleManager"));
        // The table isn't one.
        let (mut w, table, _) = setup();
        w.put32(table, vtable::HANDLE_MANAGER);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("handle table"));
        // A capacity past the bound.
        let (mut w, table, _) = setup();
        w.put32(table + 0x08, MAX_HANDLES + 1);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("slots"));
        // A handle whose id isn't its key's is passed over.
        let (mut w, table, _) = setup();
        let values = w.get32(table + 0x0C);
        let h = w.get32(values + 8);
        w.put32(h + 0x08, 0xA22);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("no export"));
        // And one whose namespace hash isn't.
        let (mut w, table, _) = setup();
        let values = w.get32(table + 0x0C);
        let h = w.get32(values + 8);
        w.put32(h + 0x10, 0);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("no export"));
        // Two exports of one name.
        let (mut w, _, flag) = setup();
        let t = fx::types(&mut w);
        let twin = fx::int(&mut w, &t, "ZoneElement_P1_Correct", 1);
        fx::handles(&mut w, GLOBAL, &[(0xA21, flag), (0xA99, twin)]);
        assert!(err(exports(&mut w, GLOBAL, &want)).contains("2 of global.bld's exports"));
    }

    #[test]
    fn a_bit_field_reads_little_endian_across_words() {
        // The save area AF.8's fresh state holds for Eruptor: experience in
        // the first 24 bits, gold the next 16 across into word 1, playtime 32
        // bits from bit 40.
        let mut words = vec![0u32; 44];
        words[0] = 0x2A00_0FA0; // 4,000 experience, gold's low byte 0x2A
        words[1] = 0x0000_0401; // gold's high byte 1: 298; playtime from bit 8
        words[2] = 0x0000_0001;
        assert_eq!(bit_field(&words, 0, 24, false).unwrap(), 4000);
        assert_eq!(bit_field(&words, 24, 16, false).unwrap(), 0x012A);
        assert_eq!(bit_field(&words, 40, 32, false).unwrap(), 0x0100_0004);
        // The console save's difficulty: 4 bits at 2,549 (word 79, bit 21).
        let mut save = vec![0u32; 178];
        save[79] = 1 << 21;
        assert_eq!(bit_field(&save, 2549, 4, false).unwrap(), 1);
        // Signed, and the full 32 bits.
        assert_eq!(bit_field(&[0xF], 0, 4, true).unwrap(), -1);
        assert_eq!(bit_field(&[0x7], 0, 4, true).unwrap(), 7);
        assert_eq!(
            bit_field(&[0xFFFF_FFFF], 0, 32, false).unwrap(),
            0xFFFF_FFFF
        );
        assert_eq!(bit_field(&[0xFFFF_FFFF], 0, 32, true).unwrap(), -1);
        // Past the words, a width of 0 or over 32.
        assert!(bit_field(&words, 44 * 32, 1, false).is_err());
        assert!(bit_field(&words, 43 * 32 + 24, 16, false).is_err());
        assert!(bit_field(&words, 0, 0, false).is_err());
        assert!(bit_field(&words, 0, 33, false).is_err());
    }

    #[test]
    fn fnv1a_matches_the_namespace_hash_the_handles_carry() {
        // AF.8's fresh state keys global.bld's 2,729 exports under this.
        assert_eq!(fnv1a(GLOBAL), 0x2694_BE8E);
        assert_eq!(fnv1a(""), 0x811C_9DC5);
    }

    #[test]
    fn sets_and_their_references() {
        let mut w = World::new();
        let t = fx::types(&mut w);
        let a = fx::int(&mut w, &t, "", 7);
        let b = fx::int(&mut w, &t, "", 9);
        let s = fx::set(&mut w, "SaveGame", &[a, b]);
        let r = fx::reference(&mut w, vtable::SET_REFERENCE, "save val set", s);
        assert_eq!(set_items(&mut w, s, "SaveGame").unwrap(), [a, b]);
        let cells = named(&mut w, &[r]).unwrap();
        assert_eq!(referent(&mut w, &cells[0]).unwrap(), Some(s));
        assert_eq!(
            value(&mut w, &mut Types::default(), &cells[0]).unwrap(),
            Value::Reference {
                to: Some(s),
                name: Some("SaveGame".into())
            }
        );
        assert_eq!(
            value(
                &mut w,
                &mut Types::default(),
                &Named {
                    at: s,
                    vtable: vtable::SCRIPT_SET,
                    name: "SaveGame".into()
                }
            )
            .unwrap(),
            Value::Set { count: 2 }
        );
        // A set whose list isn't a set list, and a non-reference followed.
        let list = w.get32(s + 0x18);
        w.put32(list, vtable::OBJECT_LIST);
        assert!(err(set_items(&mut w, s, "SaveGame")).contains("SaveGame's list"));
        let not_ref = named(&mut w, &[a]).unwrap().pop().unwrap();
        assert!(err(referent(&mut w, &not_ref)).contains("not a reference"));
    }
}
