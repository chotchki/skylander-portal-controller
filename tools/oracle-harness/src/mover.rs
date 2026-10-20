//! The hero's walk as Giants' movement script keeps it (openspec AG.38): the
//! velocity and the rest of the mover data `global.bld`'s movementUpdate.ai
//! holds for him between frames, found at a pause and put on O5's watch list
//! beside his corpus and proxy, since the body's velocity is what the engine
//! made of the script's and the two differ.
//!
//! A script variable is a named object in guest memory (aj-vm's reading,
//! measured on AG.39's end states): a number is a `ValueInfo`, its type (the
//! `FloatMeasurement` or `IntMeasurement` metaobject: a number retypes itself
//! to what it's given) at +0x1C and its value at +0x20; a reference a
//! `ScriptReference` or `PlacementReference`, its object at +0x1C; a
//! structure a `ScriptStructure`, its fields in an `igObjectList` at +0x18;
//! every one named at +0x08. From the hero's placement: his script's local
//! variables (+0xB0, an `igObjectList`) hold `description`, a reference to his
//! creature description, whose `velocity data` is the velocity the movement
//! script reads and writes for him (its `velocity length`, `velocity heading`,
//! `velocity pitch`, `vertical difference` and `horizontal length`, feet per
//! second and degrees) and whose `movement parameters` hold his top speeds,
//! rates and gravity, each a structure whose `virtual` number is the one the
//! script reads (its `value` is a reference to it). His mover data, the
//! `Movement Data` global's `movers` element for him (the time since its last
//! update, the gravity velocity, whether he's trying to move ...), is a
//! structure whose `mover` field references his placement and whose
//! `velocity` field references that velocity data. It was made beside the
//! description when he was added, so the resolve looks for it in the memory
//! around the description ([`SEARCH`] either side) rather than walking the
//! global's set, whose own address nothing static names.
//!
//! Every address is the 01.00 EBOOT's and every pointer is checked against its
//! class's vtable before it's followed, as in [`crate::actors`].

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use guac_oracle::recording::Mover;
use serde::{Deserialize, Serialize};

use crate::actors::{self, Gather, be32, c_string, expect_vtable, f32_at, object_ptr, one};
use crate::verbs::WatchRecord;
use crate::watch::{Field, WatchList};

/// The script runtime's classes the resolve follows.
pub mod vtable {
    pub const VALUE_INFO: u32 = 0x0062_3A40;
    pub const SCRIPT_REFERENCE: u32 = 0x0062_3CB8;
    pub const PLACEMENT_REFERENCE: u32 = 0x0062_3D30;
    pub const SCRIPT_STRUCTURE: u32 = 0x0062_5C90;
    pub const OBJECT_LIST: u32 = 0x0062_4180;
}

const PLACEMENT_LOCALS: usize = 0xB0;
const OBJECT_HEAD: usize = 0x20;
const NAME: usize = 0x08;
const STRUCT_FIELDS: usize = 0x18;
const REF_OBJECT: usize = 0x1C;
const VALUE_TYPE: u32 = 0x1C;
const VALUE: u32 = 0x20;
const META_NAME: usize = 0x08;
const NAME_LEN: usize = 64;
/// Fields a structure is taken to hold at most; past it, garbage.
const MAX_FIELDS: u32 = 128;
/// How far either side of the description the mover data is looked for.
pub const SEARCH: u32 = 0x10_0000;
/// The search's reads: an unmapped piece costs only itself.
const CHUNK: u32 = 0x1_0000;
/// A watched block of numbers spans at most this many bytes.
const MAX_SPAN: u32 = 0x1000;

/// The velocity data's numbers, in a [`Mover`]'s order.
pub const VELOCITY: [&str; 5] = [
    "velocity length",
    "velocity heading",
    "velocity pitch",
    "vertical difference",
    "horizontal length",
];

/// The mover data's numbers the watch takes.
pub const MOVER: [&str; 12] = [
    "time since last update",
    "last update time",
    "gravity velocity",
    "vertical difference last update",
    "trying to move",
    "trying to move with stick",
    "updating facing",
    "controller inactive",
    "starting facing",
    "starting facing data already stored",
    "starting velocity data already stored",
    "simple",
];

/// The movement parameters read once at the resolve, each by its `virtual`.
pub const PARAMETERS: [&str; 11] = [
    "ground max speed",
    "ground acceleration",
    "ground deceleration",
    "ground turn rate",
    "air max speed",
    "air acceleration",
    "air deceleration",
    "air turn rate",
    "gravity acceleration",
    "gravity max speed",
    "move type",
];

/// Where the hero's script walk state lives, resolved at a pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoverWatch {
    pub description: u32,
    pub velocity_data: u32,
    pub mover: u32,
    /// The velocity data's numbers by name, in [`VELOCITY`]'s order.
    pub velocity: Vec<(String, u32)>,
    /// The mover data's numbers by name, those of [`MOVER`] it holds.
    pub fields: Vec<(String, u32)>,
    /// The `FloatMeasurement` metaobject: a number whose type word is this is
    /// a float, anything else an integer.
    pub float_type: u32,
    /// The movement parameters' `virtual` numbers at the resolve.
    pub parameters: BTreeMap<String, f32>,
}

/// A named script object: where it is, its class and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Var {
    at: u32,
    vtable: u32,
    name: String,
}

impl MoverWatch {
    /// The watch-list field holding the velocity data's numbers.
    pub const VELOCITY_FIELD: &'static str = "mover.velocity";
    /// The watch-list field holding the mover data's numbers.
    pub const MOVER_FIELD: &'static str = "mover.fields";

    /// Finds the hero's script walk state from his placement: none when his
    /// placement holds no script variables or no `description` (a list written
    /// for an actor the movement script doesn't drive); refused when a pointer
    /// on the way isn't its class's, a number the walk needs is missing, or
    /// the search finds no mover data, or more than one, for him.
    pub fn resolve(g: &mut impl Gather, hero_placement: u32) -> Result<Option<Self>> {
        let p = one(g, hero_placement, actors::ACTOR_LEN, "the hero's placement")?;
        let locals_at = be32(&p, PLACEMENT_LOCALS);
        if !object_ptr(locals_at) {
            return Ok(None);
        }
        let locals = actors::list(
            g,
            locals_at,
            vtable::OBJECT_LIST,
            "the hero's script variables",
        )?;
        let locals = vars(g, &locals)?;
        let Some(desc_ref) = locals
            .iter()
            .find(|v| v.name == "description" && v.vtable == vtable::SCRIPT_REFERENCE)
        else {
            return Ok(None);
        };
        let description = object_of(g, desc_ref.at)?;
        let desc = fields_of(g, description, "the hero's creature description")?;
        let velocity_data = named(&desc, "velocity data", vtable::SCRIPT_STRUCTURE)?.at;
        let vdata = fields_of(g, velocity_data, "his velocity data")?;
        let velocity = VELOCITY
            .iter()
            .map(|n| Ok(((*n).to_owned(), named(&vdata, n, vtable::VALUE_INFO)?.at)))
            .collect::<Result<Vec<_>>>()?;
        let mover = find_mover(g, description, hero_placement, velocity_data)?;
        let mdata = fields_of(g, mover, "his mover data")?;
        let fields: Vec<(String, u32)> = MOVER
            .iter()
            .filter_map(|n| {
                mdata
                    .iter()
                    .find(|v| v.name == *n && v.vtable == vtable::VALUE_INFO)
                    .map(|v| ((*n).to_owned(), v.at))
            })
            .collect();
        for need in [
            "time since last update",
            "gravity velocity",
            "trying to move",
        ] {
            if !fields.iter().any(|(n, _)| n == need) {
                bail!("the mover data at {mover:#010x} has no `{need}`");
            }
        }
        let float_type = float_type(g, velocity.iter().chain(&fields))?;
        let params_at = object_of(
            g,
            named(&desc, "movement parameters", vtable::SCRIPT_REFERENCE)?.at,
        )?;
        let params = fields_of(g, params_at, "his movement parameters")?;
        let mut parameters = BTreeMap::new();
        for n in PARAMETERS {
            let Ok(param) = named(&params, n, vtable::SCRIPT_STRUCTURE) else {
                continue;
            };
            let inner = fields_of(g, param.at, n)?;
            let virt = named(&inner, "virtual", vtable::VALUE_INFO)?;
            parameters.insert(n.to_owned(), number(g, virt.at, float_type)?);
        }
        let watch = Self {
            description,
            velocity_data,
            mover,
            velocity,
            fields,
            float_type,
            parameters,
        };
        for (what, block) in [
            (Self::VELOCITY_FIELD, &watch.velocity),
            (Self::MOVER_FIELD, &watch.fields),
        ] {
            let (lo, hi) = span(block);
            if hi - lo > MAX_SPAN {
                bail!("{what} would span {:#x} bytes from {lo:#010x}", hi - lo);
            }
        }
        Ok(Some(watch))
    }

    /// The watch-list fields: each block from its first number's type word to
    /// its last's value.
    pub fn list_fields(&self) -> Vec<Field> {
        [
            (Self::VELOCITY_FIELD, &self.velocity),
            (Self::MOVER_FIELD, &self.fields),
        ]
        .into_iter()
        .map(|(name, block)| {
            let (lo, hi) = span(block);
            Field::new(name, lo, hi - lo)
        })
        .collect()
    }

    /// The hero's script walk state in a record: none when either block
    /// wasn't readable.
    pub fn read(&self, list: &WatchList, record: &WatchRecord) -> Option<Mover> {
        let v = list.field(record, Self::VELOCITY_FIELD)?;
        let m = list.field(record, Self::MOVER_FIELD)?;
        let at = |bytes: &[u8], block: &[(String, u32)], addr: u32| {
            let lo = span(block).0;
            let ty = be32(bytes, (addr + VALUE_TYPE - lo) as usize);
            let off = (addr + VALUE - lo) as usize;
            if ty == self.float_type {
                f32_at(bytes, off)
            } else {
                be32(bytes, off) as i32 as f32
            }
        };
        let vel: Vec<f32> = self
            .velocity
            .iter()
            .map(|(_, a)| at(v, &self.velocity, *a))
            .collect();
        Some(Mover {
            length: vel[0],
            heading: vel[1],
            pitch: vel[2],
            vertical: vel[3],
            horizontal: vel[4],
            fields: self
                .fields
                .iter()
                .map(|(n, a)| (n.clone(), at(m, &self.fields, *a)))
                .collect(),
        })
    }
}

/// From the first number's type word to the last number's value's end.
fn span(block: &[(String, u32)]) -> (u32, u32) {
    let lo = block.iter().map(|(_, a)| *a).min().unwrap_or(0) + VALUE_TYPE;
    let hi = block.iter().map(|(_, a)| *a).max().unwrap_or(0) + VALUE + 4;
    (lo, hi)
}

/// Each object's class and name, in two batched reads.
fn vars(g: &mut impl Gather, at: &[u32]) -> Result<Vec<Var>> {
    let ranges: Vec<(u32, usize)> = at
        .iter()
        .filter(|&&a| object_ptr(a))
        .map(|&a| (a, NAME + 4))
        .collect();
    let heads = g.gather(&ranges)?;
    let named: Vec<(u32, u32, u32)> = ranges
        .iter()
        .zip(heads)
        .filter_map(|(&(a, _), h)| h.map(|h| (a, be32(&h, 0), be32(&h, NAME))))
        .filter(|&(_, _, n)| (0x0001_0000..0x5000_0000).contains(&n))
        .collect();
    let names = g.gather(
        &named
            .iter()
            .map(|&(_, _, n)| (n, NAME_LEN))
            .collect::<Vec<_>>(),
    )?;
    Ok(named
        .iter()
        .zip(names)
        .filter_map(|(&(at, vtable, _), n)| {
            n.map(|n| Var {
                at,
                vtable,
                name: c_string(&n),
            })
        })
        .collect())
}

/// A structure's fields, its vtable checked.
fn fields_of(g: &mut impl Gather, at: u32, what: &str) -> Result<Vec<Var>> {
    let s = one(g, at, OBJECT_HEAD, what)?;
    expect_vtable(&s, vtable::SCRIPT_STRUCTURE, at, "ScriptStructure")
        .with_context(|| what.to_owned())?;
    let list = actors::list(
        g,
        be32(&s, STRUCT_FIELDS),
        vtable::OBJECT_LIST,
        &format!("{what}'s fields"),
    )?;
    if list.len() as u32 > MAX_FIELDS {
        bail!("{what} at {at:#010x} holds {} fields", list.len());
    }
    vars(g, &list)
}

/// The object a reference names, refused when it names none (a null
/// reference reads 0).
fn object_of(g: &mut impl Gather, reference: u32) -> Result<u32> {
    let r = one(g, reference, OBJECT_HEAD, "a script reference")?;
    let obj = be32(&r, REF_OBJECT);
    if !object_ptr(obj) {
        bail!("the reference at {reference:#010x} names {obj:#010x}");
    }
    Ok(obj)
}

fn named<'a>(vars: &'a [Var], name: &str, want: u32) -> Result<&'a Var> {
    vars.iter()
        .find(|v| v.name == name && v.vtable == want)
        .with_context(|| format!("no `{name}` with vtable {want:#010x}"))
}

/// The `FloatMeasurement` metaobject, from the numbers' own types (each
/// metaobject named at +0x08): the walk's timer is a float from his first
/// update, so one turns up even with every velocity number still an integer 0.
fn float_type<'a>(
    g: &mut impl Gather,
    numbers: impl Iterator<Item = &'a (String, u32)>,
) -> Result<u32> {
    for (_, at) in numbers {
        let ty = be32(
            &one(g, *at, (VALUE + 4) as usize, "a number")?,
            VALUE_TYPE as usize,
        );
        if !object_ptr(ty) {
            continue;
        }
        let meta = one(g, ty, META_NAME + 4, "a number's type")?;
        let name = one(g, be32(&meta, META_NAME), NAME_LEN, "a type's name")?;
        if c_string(&name) == "FloatMeasurement" {
            return Ok(ty);
        }
    }
    bail!("none of the hero's walk numbers is a FloatMeasurement")
}

/// A number's value: the same word holds an int or a float bit pattern, and
/// only the type word says which.
fn number(g: &mut impl Gather, at: u32, float_type: u32) -> Result<f32> {
    let b = one(g, at, (VALUE + 4) as usize, "a number")?;
    Ok(if be32(&b, VALUE_TYPE as usize) == float_type {
        f32_at(&b, VALUE as usize)
    } else {
        be32(&b, VALUE as usize) as i32 as f32
    })
}

/// The hero's mover data: the structure near his description whose `mover`
/// field references his placement and whose `velocity` field references his
/// velocity data.
fn find_mover(g: &mut impl Gather, description: u32, hero: u32, velocity_data: u32) -> Result<u32> {
    let lo = description.saturating_sub(SEARCH) & !(CHUNK - 1);
    let hi = description.saturating_add(SEARCH);
    let chunks: Vec<(u32, usize)> = (lo..hi)
        .step_by(CHUNK as usize)
        .map(|a| (a, CHUNK as usize))
        .collect();
    let bytes = g.gather(&chunks)?;
    let mut found = Vec::new();
    for ((base, _), chunk) in chunks.iter().zip(&bytes) {
        let Some(chunk) = chunk else { continue };
        for off in (0..chunk.len().saturating_sub(OBJECT_HEAD)).step_by(4) {
            if be32(chunk, off) != vtable::SCRIPT_STRUCTURE {
                continue;
            }
            let at = base + off as u32;
            let Ok(vars) = fields_of(g, at, "a structure") else {
                continue;
            };
            let mut is_his = false;
            let mut holds_velocity = false;
            for v in &vars {
                if v.name == "mover" && v.vtable == vtable::PLACEMENT_REFERENCE {
                    is_his |= object_of(g, v.at).is_ok_and(|o| o == hero);
                }
                if v.name == "velocity" && v.vtable == vtable::SCRIPT_REFERENCE {
                    holds_velocity |= object_of(g, v.at).is_ok_and(|o| o == velocity_data);
                }
            }
            if is_his && holds_velocity {
                found.push(at);
            }
        }
    }
    match found[..] {
        [one] => Ok(one),
        [] => bail!(
            "no mover data for the hero at {hero:#010x} within {SEARCH:#x} of his description at {description:#010x}"
        ),
        _ => bail!(
            "{} mover data for the hero at {hero:#010x}: {found:#010x?}",
            found.len()
        ),
    }
}

/// The hero's script walk state laid out in a fixture's memory, as AG.39's end
/// states hold it.
#[cfg(test)]
pub(crate) mod fixture {
    use super::vtable;
    use crate::actors::fixture::World;

    /// Where [`with_mover`] put things.
    pub struct MoverFixture {
        pub description: u32,
        pub velocity_data: u32,
        pub mover: u32,
        /// The velocity data's numbers, in [`super::VELOCITY`]'s order.
        pub velocity: Vec<u32>,
        /// `time since last update`, `gravity velocity`, `trying to move`.
        pub timer: u32,
        pub gravity: u32,
        pub trying: u32,
        pub float_type: u32,
        pub int_type: u32,
    }

    fn named(w: &mut World, vt: u32, len: usize, name: &str) -> u32 {
        let at = w.object(vt, len);
        let n = w.string(name);
        w.put32(at + 0x08, n);
        at
    }

    pub fn number(w: &mut World, name: &str, ty: u32, bits: u32) -> u32 {
        let at = named(w, vtable::VALUE_INFO, 0x24, name);
        w.put32(at + 0x1C, ty);
        w.put32(at + 0x20, bits);
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

    /// The hero's description (his velocity data at 21.5 ft/s along 105°,
    /// his movement parameters' ground max speed), a decoy structure whose
    /// `mover` names another actor, and his mover data, all on the heap; his
    /// placement's script variables naming the description.
    pub fn with_mover(w: &mut World, hero_placement: u32) -> MoverFixture {
        let float_type = named(w, 0x0063_4C88, 0x30, "FloatMeasurement");
        let int_type = named(w, 0x0063_4C88, 0x30, "IntMeasurement");
        let velocity: Vec<u32> = [
            ("velocity length", 21.500_15f32),
            ("velocity heading", 105.0),
            ("velocity pitch", -24.9),
            ("vertical difference", -9.054),
            ("horizontal length", 19.5),
        ]
        .into_iter()
        .map(|(n, v)| number(w, n, float_type, v.to_bits()))
        .collect();
        let velocity_data = structure(w, "velocity data", &velocity);
        let virt = number(w, "virtual", float_type, 21.5f32.to_bits());
        let value = reference(w, vtable::SCRIPT_REFERENCE, "value", virt);
        let max = structure(w, "ground max speed", &[virt, value]);
        let move_type = number(w, "virtual", int_type, 0);
        let mt = structure(w, "move type", &[move_type]);
        let params = structure(w, "movement params datas base", &[max, mt]);
        let params_ref = reference(w, vtable::SCRIPT_REFERENCE, "movement parameters", params);
        let creature = reference(w, vtable::PLACEMENT_REFERENCE, "creature", hero_placement);
        let description = structure(w, "", &[velocity_data, creature, params_ref]);
        // Someone else's mover data first: it names his velocity data but not
        // his placement.
        let other = reference(w, vtable::PLACEMENT_REFERENCE, "mover", 0x3000_0100);
        let vel_ref = reference(w, vtable::SCRIPT_REFERENCE, "velocity", velocity_data);
        structure(w, "", &[other, vel_ref]);
        let simple = number(w, "simple", int_type, 0);
        let gravity = number(w, "gravity velocity", int_type, 10);
        let trying = number(w, "trying to move", int_type, 1);
        let timer = number(
            w,
            "time since last update",
            float_type,
            0.016_845_703f32.to_bits(),
        );
        let vel_ref = reference(w, vtable::SCRIPT_REFERENCE, "velocity", velocity_data);
        let mover_ref = reference(w, vtable::PLACEMENT_REFERENCE, "mover", hero_placement);
        let mover = structure(w, "", &[simple, gravity, trying, vel_ref, timer, mover_ref]);
        let desc_ref = reference(w, vtable::SCRIPT_REFERENCE, "description", description);
        let other_local = number(w, "Timer", float_type, 0);
        let locals = w.list(vtable::OBJECT_LIST, &[other_local, desc_ref], 4);
        w.put32(hero_placement + 0xB0, locals);
        MoverFixture {
            description,
            velocity_data,
            mover,
            velocity,
            timer,
            gravity,
            trying,
            float_type,
            int_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{number, with_mover};
    use super::*;
    use crate::actors::fixture::{Spec, World};

    fn add_hero(w: &mut World) -> u32 {
        w.actor(&Spec {
            name: "009_Eruptor".into(),
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        })
        .placement
    }

    #[test]
    fn the_heros_walk_resolves_from_his_description_and_his_mover_data() {
        let mut w = World::new();
        let hero = add_hero(&mut w);
        let fx = with_mover(&mut w, hero);
        let m = MoverWatch::resolve(&mut w, hero).unwrap().unwrap();
        assert_eq!(
            (m.description, m.velocity_data, m.mover, m.float_type),
            (fx.description, fx.velocity_data, fx.mover, fx.float_type)
        );
        assert_eq!(
            m.velocity.iter().map(|(_, a)| *a).collect::<Vec<_>>(),
            fx.velocity
        );
        // The decoy names another actor, so the search took his.
        let names: Vec<&str> = m.fields.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "time since last update",
                "gravity velocity",
                "trying to move",
                "simple"
            ]
        );
        assert_eq!(m.parameters["ground max speed"], 21.5);
        assert_eq!(m.parameters["move type"], 0.0);
        assert!(!m.parameters.contains_key("air max speed"));
        // Two fields, each a block from its first type word to its last value.
        let fields = m.list_fields();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].addr, fx.velocity.iter().min().unwrap() + 0x1C);
        let list = WatchList::new(fields).unwrap();
        let record = sample(&mut w, &list);
        let got = m.read(&list, &record).unwrap();
        assert_eq!(
            (
                got.length,
                got.heading,
                got.pitch,
                got.vertical,
                got.horizontal
            ),
            (21.500_15, 105.0, -24.9, -9.054, 19.5)
        );
        assert_eq!(got.fields["gravity velocity"], 10.0);
        assert_eq!(got.fields["trying to move"], 1.0);
        assert_eq!(got.fields["time since last update"], 0.016_845_703);
        // A block unreadable: no walk state.
        let mut unread = record.clone();
        unread.missing = 1 << list.placed(MoverWatch::MOVER_FIELD).unwrap().range;
        assert!(m.read(&list, &unread).is_none());
        // A number that retypes itself to a float reads as one, and back.
        w.put32(fx.gravity + 0x1C, fx.float_type);
        w.putf(fx.gravity + 0x20, 9.75);
        w.put32(fx.timer + 0x1C, fx.int_type);
        w.put32(fx.timer + 0x20, 3);
        let got = m.read(&list, &sample(&mut w, &list)).unwrap();
        assert_eq!(got.fields["gravity velocity"], 9.75);
        assert_eq!(got.fields["time since last update"], 3.0);
        assert_eq!(got.fields["trying to move"], 1.0);
        let _ = fx.trying;
    }

    /// What O5 copies at a pad read: each range's bytes back to back.
    fn sample(w: &mut World, list: &WatchList) -> WatchRecord {
        let ranges: Vec<(u32, usize)> = list
            .ranges()
            .iter()
            .map(|r| (r.addr, r.len as usize))
            .collect();
        let payload: Vec<u8> = w
            .gather(&ranges)
            .unwrap()
            .into_iter()
            .flat_map(|b| b.unwrap())
            .collect();
        WatchRecord {
            flip: 152,
            host_us: 0,
            missing: 0,
            read: 0,
            payload,
        }
    }

    #[test]
    fn a_hero_without_a_description_has_no_walk_state_and_a_broken_chain_is_refused() {
        // No script variables at all.
        let mut w = World::new();
        let hero = add_hero(&mut w);
        assert!(MoverWatch::resolve(&mut w, hero).unwrap().is_none());
        // Variables, but no description among them.
        let mut w = World::new();
        let hero_at = add_hero(&mut w);
        let t = number(&mut w, "Timer", 0, 0);
        let locals = w.list(vtable::OBJECT_LIST, &[t], 4);
        w.put32(hero_at + 0xB0, locals);
        assert!(MoverWatch::resolve(&mut w, hero_at).unwrap().is_none());
        // The description isn't a structure.
        let mut w = World::new();
        let hero_at = add_hero(&mut w);
        let fx = with_mover(&mut w, hero_at);
        w.put32(fx.description, vtable::VALUE_INFO);
        let e = format!("{:#}", MoverWatch::resolve(&mut w, hero_at).unwrap_err());
        assert!(e.contains("ScriptStructure"), "{e}");
        // His mover data names someone else: none found.
        let mut w = World::new();
        let hero_at = add_hero(&mut w);
        let fx = with_mover(&mut w, hero_at);
        let fields = w.get32(fx.mover + 0x18);
        let data = w.get32(fields + 0x14);
        let mover_ref = w.get32(data + 4 * 5);
        w.put32(mover_ref + 0x1C, 0x3000_0200);
        let e = format!("{:#}", MoverWatch::resolve(&mut w, hero_at).unwrap_err());
        assert!(e.contains("no mover data"), "{e}");
        // A velocity number missing.
        let mut w = World::new();
        let hero_at = add_hero(&mut w);
        let fx = with_mover(&mut w, hero_at);
        let name = w.get32(fx.velocity[2] + 0x08);
        w.put32(name, u32::from_be_bytes(*b"xxxx"));
        let e = format!("{:#}", MoverWatch::resolve(&mut w, hero_at).unwrap_err());
        assert!(e.contains("velocity pitch"), "{e}");
    }
}
