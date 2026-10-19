//! Writes the roster's four inputs in code, for tests and fuzz seeds
//! (`test-fixtures`): an `init_setup` whose `Init_Setup_Minions.ai` calls
//! the three toy-table macros as Giants' does, a `global.bld` level with its
//! text keys, sets and values plus two language packs, a bootstrap and a
//! collection file with portrait sprites. Every name and number is ours;
//! the shapes follow the disc (AE.2).

use std::collections::BTreeMap;

use crate::exports::NAMESPACE;
use crate::fnv1a32;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;
use crate::language::builder::{dict, pack_info};
use crate::roster::{ITEM_LOADS, LOCATION_LOADS, sprite_sets, words};
use crate::toys::{DEFINE_ALT, DEFINE_ALT_CHARACTER, DEFINE_TOY, LIGHTCORE_SET, sets};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// The two shapes the disc's name arguments take: Spyro's Adventure toys
/// index `Minion_Names`, the rest import their key.
#[derive(Debug, Clone, Copy)]
pub enum NameSpec {
    /// A `global.bld` `StringInfo` import.
    Key(&'static str),
    /// `Minion_Names` with this index.
    Minion(i32),
}

/// The element argument's shapes on the disc, plus leaving it out.
#[derive(Debug, Clone, Copy)]
pub enum ElementSpec {
    Literal(i32),
    /// A literal inside a `ValueRHSVariant` with no operator.
    Wrapped(i32),
    /// A `global.bld` `ValueInfo` import.
    Value(&'static str),
    /// No parameter: reads 0.
    Omitted,
}

#[derive(Debug, Clone)]
pub struct AltSpec {
    pub load: &'static str,
    pub deco: i32,
    /// `None` passes `abstractscriptgroup.null`.
    pub display: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ToySpec {
    pub load: &'static str,
    pub name: NameSpec,
    pub code: i32,
    pub magic: bool,
    pub sky1: bool,
    pub repose: bool,
    pub giant: bool,
    pub element: ElementSpec,
    /// The call's after-return block adds it to `pet level loads`.
    pub pet: bool,
    /// Calls nested in its after-return block.
    pub alts: Vec<AltSpec>,
}

impl ToySpec {
    pub fn new(load: &'static str, name: NameSpec, code: i32) -> Self {
        Self {
            load,
            name,
            code,
            magic: false,
            sky1: false,
            repose: false,
            giant: false,
            element: ElementSpec::Literal(0),
            pet: false,
            alts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AltCharacterSpec {
    pub normal: &'static str,
    pub prefix: &'static str,
    pub alt: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct DiscSpec {
    pub toys: Vec<ToySpec>,
    pub alt_characters: Vec<AltCharacterSpec>,
    /// Level loads the LightCore loop adds.
    pub lightcore: Vec<&'static str>,
    pub minion_names: Vec<&'static str>,
    pub item_loads: Vec<&'static str>,
    pub location_loads: Vec<&'static str>,
    /// `ValueInfo`s: name, value.
    pub values: Vec<(&'static str, i32)>,
    /// English text by key; a key not here reads as itself. French is
    /// `fr:` and the English.
    pub texts: Vec<(&'static str, &'static str)>,
    /// Sprite set name, then its sprites' names.
    pub sprites: Vec<(&'static str, Vec<&'static str>)>,
}

/// The four inputs, as `level.bld` IGZ bytes plus `global.bld`'s packs.
#[derive(Debug, Clone)]
pub struct Disc {
    pub init_setup: Vec<u8>,
    pub global: Vec<u8>,
    /// English (6), then French (8).
    pub packs: Vec<Vec<u8>>,
    pub bootstrap: Vec<u8>,
    pub collection: Vec<u8>,
}

/// `global.bld`'s level and the export id of each root by name.
struct Global {
    bytes: Vec<u8>,
    ids: BTreeMap<String, u32>,
    keys: Vec<String>,
}

const BOOTSTRAP_ROOTS: u32 = 19;

fn global(spec: &DiscSpec) -> Global {
    let mut keys: Vec<String> = Vec::new();
    let mut key = |k: &str| {
        if !keys.iter().any(|x| x == k) {
            keys.push(k.to_owned());
        }
    };
    for w in [words::SERIES_2, words::LIGHTCORE, words::LEGENDARY] {
        key(w);
    }
    for t in &spec.toys {
        key(t.load);
        if let NameSpec::Key(k) = t.name {
            key(k);
        }
        for a in &t.alts {
            key(a.load);
            if let Some(d) = a.display {
                key(d);
            }
        }
    }
    for a in &spec.alt_characters {
        key(a.prefix);
    }
    for k in spec
        .minion_names
        .iter()
        .chain(&spec.item_loads)
        .chain(&spec.location_loads)
        .chain(&spec.lightcore)
    {
        key(k);
    }
    for (k, _) in &spec.texts {
        key(k);
    }

    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let objects: Vec<Location> = keys
        .iter()
        .map(|k| {
            let o = b.object(0, "StringInfo", 0x28);
            b.string(at(o, 0x08), k);
            o
        })
        .collect();
    let blocks = b.section(0x10);
    let mut roots: Vec<Option<Location>> = objects.iter().copied().map(Some).collect();
    let mut ids: BTreeMap<String, u32> = BTreeMap::new();
    for (i, k) in keys.iter().enumerate() {
        ids.insert(k.clone(), BOOTSTRAP_ROOTS + 1 + i as u32);
    }
    let key_at = |k: &str| objects[keys.iter().position(|x| x == k).expect("collected")];
    for (name, members) in [
        ("Minion_Names", &spec.minion_names),
        (ITEM_LOADS, &spec.item_loads),
        (LOCATION_LOADS, &spec.location_loads),
        ("toy data", &Vec::new()),
    ] {
        let set = b.object(0, "ScriptSet", 0x1C);
        let members_list = b.object(0, "ScriptSetList", 0x24);
        b.string(at(set, 0x08), name);
        b.pointer(at(set, 0x18), members_list);
        let elements: Vec<_> = members.iter().map(|k| Some(key_at(k))).collect();
        list(&mut b, members_list, blocks, &elements);
        ids.insert(name.to_owned(), BOOTSTRAP_ROOTS + 1 + roots.len() as u32);
        roots.push(Some(set));
    }
    for (name, value) in &spec.values {
        let v = b.object(0, "ValueInfo", 0x24);
        b.string(at(v, 0x08), name);
        b.meta_object(at(v, 0x1C), "metaobject", "IntMeasurement");
        b.put_u32(at(v, 0x20), *value as u32);
        ids.insert((*name).to_owned(), BOOTSTRAP_ROOTS + 1 + roots.len() as u32);
        roots.push(Some(v));
    }
    list(&mut b, root, blocks, &roots);
    Global {
        bytes: b.build(),
        ids,
        keys,
    }
}

fn english<'s>(spec: &'s DiscSpec, key: &'s str) -> &'s str {
    spec.texts
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(key, |(_, t)| t)
}

/// The packs for `keys`, in the order the keys' `StringInfo`s were made.
fn packs(spec: &DiscSpec, keys: &[String]) -> Vec<Vec<u8>> {
    let en: Vec<String> = keys.iter().map(|k| english(spec, k).to_owned()).collect();
    let fr: Vec<String> = en.iter().map(|t| format!("fr:{t}")).collect();
    [("ENGLISH", 6, en), ("FRENCH", 8, fr)]
        .into_iter()
        .map(|(name, id, texts)| {
            let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
            let mut b = IgzBuilder::new();
            let root = b.object(0, "igObjectList", 0x18);
            let info = b.object(0, "LanguagePackInfo", 0x20);
            let blocks = b.section(0x10);
            list(&mut b, root, blocks, &[Some(info)]);
            pack_info(&mut b, info, blocks, name, id, &dict(&texts));
            b.build()
        })
        .collect()
}

/// Writes `Init_Setup_Minions.ai`'s ops.
struct Script<'g> {
    b: IgzBuilder,
    blocks: u8,
    ops: Vec<Location>,
    ids: &'g BTreeMap<String, u32>,
    flow_macro: Location,
    flow_continue: Location,
    defs: BTreeMap<&'static str, Location>,
    /// EXNM index of `metaobject::IntMeasurement`, for literal types.
    int_measurement: u32,
}

impl Script<'_> {
    fn op(&mut self, ty: &str, size: u32, name: &str) -> Location {
        let o = self.b.object(0, ty, size);
        self.b.string(at(o, 0x08), name);
        self.ops.push(o);
        o
    }

    /// Flags 0, `_branchPC` n.
    fn branch(&mut self, op: Location, n: usize) {
        self.b.put_u32(at(op, 0x18), n as u32);
    }

    fn global_import(&mut self, at_: Location, name: &str) {
        let id = self.ids[name];
        self.b.import_raw(at_, id, fnv1a32(NAMESPACE.as_bytes()));
    }

    /// A one-element path in a stack at `stack` (its data list).
    fn path(&mut self, stack: Location, elements: &[PathElement<'_>]) {
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
                PathElement::Global(name) => self.global_import(w, name),
                PathElement::Engine(class, member) => {
                    self.b.import(w, class, member);
                }
                PathElement::Local(o) => self.b.pointer(w, *o),
            }
        }
    }

    fn literal(&mut self, value: i32) -> Location {
        let s = self.b.object(0, "RHSValueStack", 0x28);
        self.path(s, &[]);
        self.b.unflagged_meta(at(s, 0x1C), self.int_measurement);
        self.b.put_u32(at(s, 0x20), value as u32);
        s
    }

    fn reference(&mut self, elements: &[PathElement<'_>]) -> Location {
        // Giants' files write RHSReferenceStack 4 bytes over the registry.
        let s = self.b.object(0, "RHSReferenceStack", 0x30);
        self.path(s, elements);
        self.b.unflagged_meta(at(s, 0x1C), self.int_measurement);
        self.b.put_u32(at(s, 0x2C), 0xE82A_A02A);
        s
    }

    fn value_path(&mut self, elements: &[PathElement<'_>]) -> Location {
        let s = self.b.object(0, "RHSValueStack", 0x28);
        self.path(s, elements);
        self.b.unflagged_meta(at(s, 0x1C), self.int_measurement);
        s
    }

    fn wrapped(&mut self, inner: Location) -> Location {
        let v = self.b.object(0, "ValueRHSVariant", 0x1C);
        self.b.pointer(at(v, 0x08), inner);
        self.b.put_u32(at(v, 0x0C), u32::MAX);
        v
    }

    fn group(&mut self, elements: &[PathElement<'_>]) -> Location {
        let s = self.b.object(0, "ScriptGroupStack", 0x1C);
        self.path(s, elements);
        s
    }

    fn set_stack(&mut self, elements: &[PathElement<'_>]) -> Location {
        let s = self.b.object(0, "SetStack", 0x18);
        self.path(s, elements);
        s
    }

    fn variable(&mut self, name: &str) -> Location {
        let v = self.b.object(0, "OpCreateVariable", 0x2C);
        self.b.string(at(v, 0x08), "create variable||with|members");
        self.b.string(at(v, 0x1C), name);
        self.b.put_u32(at(v, 0x28), u32::MAX);
        v
    }

    fn def(&mut self, name: &'static str) -> Location {
        if let Some(&d) = self.defs.get(name) {
            return d;
        }
        let d = self.b.object(0, "OpDefineMacro", 0x38);
        self.b.string(at(d, 0x08), name);
        self.b.put_u32(at(d, 0x1C), 1);
        self.defs.insert(name, d);
        d
    }

    /// Opens `use macro <name>` with these parameters; returns the call op
    /// and its parameters. The caller adds after-return ops and closes it.
    fn call(&mut self, name: &'static str, params: &[(&str, Location)]) -> (usize, Vec<Location>) {
        let def = self.def(name);
        let np = self.group(&[PathElement::Local(def)]);
        let start = self.ops.len();
        let call = self.op("OpUseMacro", 0x24, "use macro");
        self.b.pointer(at(call, 0x1C), np);
        let iface = self.op("OpMacroInterface", 0x20, "INTERFACE");
        self.branch(iface, params.len() + 1);
        let mut out = Vec::new();
        for (i, (pname, rhs)) in params.iter().enumerate() {
            let p = self.op("OpMacroParameter", 0x48, "parameter");
            self.b.string(at(p, 0x1C), pname);
            self.b.put_u32(at(p, 0x28), u32::MAX);
            self.b.pointer(at(p, 0x30), *rhs);
            self.b.pointer(at(p, 0x38), def);
            self.b.put_u32(at(p, 0x40), 0x01CD_19B7);
            self.b.put_u32(at(p, 0x44), 0x416B_6800 + i as u32);
            out.push(p);
        }
        self.ops.push(self.flow_macro);
        (start, out)
    }

    /// Closes the block opened at `start` with `flow continue`.
    fn close(&mut self, start: usize) {
        self.ops.push(self.flow_continue);
        let n = self.ops.len() - 1 - start;
        let op = self.ops[start];
        self.branch(op, n);
    }

    fn change(&mut self, lhs: Location, rhs: Location, combine: u32) -> Location {
        let c = self.op("OpChangeMembership", 0x28, "set [set]||to");
        self.b.pointer(at(c, 0x1C), lhs);
        self.b.put_u32(at(c, 0x20), combine);
        self.b.pointer(at(c, 0x24), rhs);
        c
    }
}

#[derive(Debug, Clone, Copy)]
enum PathElement<'n> {
    Global(&'n str),
    Engine(&'static str, &'static str),
    Local(Location),
}

const NULL: PathElement<'static> = PathElement::Engine("abstractscriptgroup", "null");

fn init_setup(spec: &DiscSpec, ids: &BTreeMap<String, u32>) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let scripts = [(); 2].map(|_| b.object(0, "tfbScriptInfo", 0x1C));
    let lists = [(); 2].map(|_| b.object(0, "OpCodeList", 0x18));
    let blocks = b.section(0x10);
    // EXNM 0: the class every literal's `_type` names, unflagged.
    let int_measurement = b.meta_object(at(scripts[0], 0x18), "metaobject", "IntMeasurement");
    b.string(
        at(scripts[0], 0x08),
        "C:/tfb/Content/Levels/Includes/Global/scripts/Init_Setup.ai",
    );
    b.string(
        at(scripts[1], 0x08),
        "C:/tfb/Content/Levels/Includes/Global/scripts/Init_Setup_Minions.ai",
    );
    for (s, l) in scripts.iter().zip(&lists) {
        b.put_bytes(at(*s, 0x10), &[1]);
        b.pointer(at(*s, 0x14), *l);
    }
    list(&mut b, root, blocks, &scripts.map(Some));
    let flow_macro = b.object(0, "OpFlowMacro", 0x1C);
    b.string(at(flow_macro, 0x08), "flow macro");
    let flow_continue = b.object(0, "OpFlowBuiltInBehavior", 0x1C);
    b.string(at(flow_continue, 0x08), "flow continue");

    let mut s = Script {
        b,
        blocks,
        ops: Vec::new(),
        ids,
        flow_macro,
        flow_continue,
        defs: BTreeMap::new(),
        int_measurement,
    };
    let pet_set = s.variable(sets::PET);
    let lightcore_set = s.variable(LIGHTCORE_SET);
    let loads_temp = s.variable("loads temp");

    for toy in &spec.toys {
        let load = s.reference(&[PathElement::Global(toy.load)]);
        let name = match toy.name {
            NameSpec::Key(k) => s.reference(&[PathElement::Global(k)]),
            NameSpec::Minion(_) => s.reference(&[PathElement::Global("Minion_Names")]),
        };
        let code = s.literal(toy.code);
        let flags = [toy.magic, toy.sky1, toy.repose, toy.giant].map(|f| s.literal(i32::from(f)));
        let element = match toy.element {
            ElementSpec::Literal(e) => Some(s.literal(e)),
            ElementSpec::Wrapped(e) => {
                let l = s.literal(e);
                Some(s.wrapped(l))
            }
            ElementSpec::Value(v) => Some(s.value_path(&[PathElement::Global(v)])),
            ElementSpec::Omitted => None,
        };
        let ret = s.reference(&[NULL]);
        let mut params = vec![
            ("level load", load),
            ("name", name),
            ("item code", code),
            ("is magic item", flags[0]),
            ("is sky1 character", flags[1]),
            ("has known repose", flags[2]),
            ("is giant", flags[3]),
        ];
        if let Some(e) = element {
            params.push(("element", e));
        }
        params.push(("return info", ret));
        let (start, ps) = s.call(DEFINE_TOY, &params);
        if let NameSpec::Minion(i) = toy.name {
            let index = s.literal(i);
            s.b.pointer(at(ps[1], 0x34), index);
        }
        let return_info = *ps.last().unwrap();
        for alt in &toy.alts {
            let load = s.reference(&[PathElement::Global(alt.load)]);
            let deco = s.literal(alt.deco);
            let display = match alt.display {
                Some(d) => s.reference(&[PathElement::Global(d)]),
                None => s.reference(&[NULL]),
            };
            let (alt_start, _) = s.call(
                DEFINE_ALT,
                &[
                    ("level load", load),
                    ("check against deco id", deco),
                    ("subtype display string", display),
                ],
            );
            s.close(alt_start);
        }
        if toy.pet {
            let lhs = s.set_stack(&[PathElement::Global("toy data"), PathElement::Local(pet_set)]);
            let rhs = s.group(&[PathElement::Local(return_info)]);
            s.change(lhs, rhs, 0);
        }
        s.close(start);
    }

    // The LightCore loop: fill `loads temp`, then add each load's info.
    let target = s.set_stack(&[PathElement::Local(loads_temp)]);
    let clear = s.group(&[PathElement::Local(loads_temp)]);
    s.change(target, clear, 1);
    for load in &spec.lightcore {
        let added = s.group(&[PathElement::Global(load)]);
        s.change(target, added, 0);
    }
    let count = s.value_path(&[
        PathElement::Local(loads_temp),
        PathElement::Engine("setvariant", "count"),
    ]);
    let count = s.wrapped(count);
    let loop_at = s.ops.len();
    let looped = s.op("OpLoopValue", 0x28, "loop value|||loop index");
    s.b.pointer(at(looped, 0x20), count);
    let info = s.reference(&[NULL]);
    let (start, ps) = s.call("Portal - Get Object Level Info", &[("info", info)]);
    let lhs = s.set_stack(&[
        PathElement::Global("toy data"),
        PathElement::Local(lightcore_set),
    ]);
    let rhs = s.group(&[PathElement::Local(ps[0])]);
    s.change(lhs, rhs, 0);
    s.close(start);
    let flow = s.b.object(0, "OpFlowLoop", 0x1C);
    s.ops.push(flow);
    let n = s.ops.len() - 1 - loop_at;
    s.branch(looped, n);
    // Finalize's sort replaces the set; not an addition.
    let lhs = s.set_stack(&[
        PathElement::Global("toy data"),
        PathElement::Local(lightcore_set),
    ]);
    let rhs = s.group(&[PathElement::Local(lightcore_set)]);
    s.change(lhs, rhs, 3);

    for a in &spec.alt_characters {
        let normal = s.reference(&[PathElement::Global(a.normal)]);
        let prefix = s.reference(&[PathElement::Global(a.prefix)]);
        let alt = s.reference(&[PathElement::Global(a.alt)]);
        let (start, _) = s.call(
            DEFINE_ALT_CHARACTER,
            &[
                ("normal toy load", normal),
                ("alt prefix", prefix),
                ("alt toy load", alt),
            ],
        );
        s.close(start);
    }

    let ops: Vec<Option<Location>> = s.ops.iter().copied().map(Some).collect();
    list(&mut s.b, lists[1], blocks, &ops);
    list(&mut s.b, lists[0], blocks, &[]);
    s.b.build()
}

fn collection(spec: &DiscSpec) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let blocks = b.section(0x10);
    let mut roots = Vec::new();
    for (set_name, sprites) in &spec.sprites {
        let set = b.object(0, "ScriptSet", 0x1C);
        let members = b.object(0, "ScriptSetList", 0x24);
        b.string(at(set, 0x08), set_name);
        b.pointer(at(set, 0x18), members);
        roots.push(Some(set));
        let mut elements = Vec::new();
        for (i, name) in sprites.iter().enumerate() {
            let sprite = b.object(0, "tfbSpriteInfo", 0xE4);
            let data = b.object(0, "AnimatableTextureDataInfo", 0x34);
            let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
            let texture = b.object(0, "igTextureAttr2", 0x40);
            let image = b.object(0, "igImage2", 0x60);
            b.string(at(sprite, 0x08), name);
            b.pointer(at(sprite, 0xD0), data);
            b.string(at(data, 0x08), &format!("portraits/{name}.png"));
            b.put_bytes(at(data, 0x10), &[1]);
            b.null_memory_ref(at(data, 0x14), 0);
            b.pointer(at(data, 0x30), bind);
            b.pointer(at(bind, 0x10), texture);
            b.pointer(at(texture, 0x30), image);
            b.string(at(image, 0x08), &format!("{name}.png"));
            b.put_bytes(at(image, 0x30), &[0, 4, 0, 4, 0, 1, 0, 1, 0, 1]);
            b.import(at(image, 0x3C), "metaimages", "dxt5_big_ps3");
            b.handle(at(image, 0x48), blocks, &[i as u8; 0x80], 5);
            elements.push(Some(sprite));
        }
        list(&mut b, members, blocks, &elements);
    }
    list(&mut b, root, blocks, &roots);
    b.build()
}

/// Builds `global.bld` first: its root positions are the export ids the
/// script imports.
pub fn disc(spec: &DiscSpec) -> Disc {
    let g = global(spec);
    Disc {
        init_setup: init_setup(spec, &g.ids),
        packs: packs(spec, &g.keys),
        global: g.bytes,
        bootstrap: crate::exports::builder::bootstrap(),
        collection: collection(spec),
    }
}

/// A small table with every shape the disc's has: Minion_Names and direct
/// names, a wrapped literal and `ValueInfo` elements, an omitted element,
/// Series 2 and LightCore toys, alts with and without a display string (one
/// a Legendary-LightCore portrait), an alt character, items, a location, the
/// location-hundred pack, sidekicks (one Magic) and DEBUG.
pub fn sample() -> DiscSpec {
    use ElementSpec::*;
    use NameSpec::*;
    let toy = ToySpec::new;
    let magic = |mut t: ToySpec| {
        t.magic = true;
        t
    };
    let toys = vec![
        ToySpec {
            sky1: true,
            ..toy("MinionLoad_999_DebugMinion", Key("999_Toy_Name"), 999)
        },
        ToySpec {
            sky1: true,
            repose: true,
            element: Wrapped(2),
            alts: vec![AltSpec {
                load: "MinionLoad_000_AirDragonLegendary",
                deco: 2,
                display: Some("ToyType_Polar"),
            }],
            ..toy("MinionLoad_000_AirDragon", Minion(0), 0)
        },
        ToySpec {
            sky1: true,
            repose: true,
            element: Literal(6),
            ..toy("MinionLoad_007_GemGolem", Minion(1), 7)
        },
        ToySpec {
            sky1: true,
            repose: true,
            element: Omitted,
            ..toy("MinionLoad_016_Spyro", Minion(2), 16)
        },
        ToySpec {
            sky1: true,
            ..toy("MinionLoad_016_SpyroDark", Minion(2), 28)
        },
        ToySpec {
            element: Value("ElementIndex_Air"),
            alts: vec![AltSpec {
                load: "MinionLoad_100_EagleLegendary",
                deco: 3,
                display: None,
            }],
            ..toy("MinionLoad_100_Eagle", Key("100_Toy_Name"), 100)
        },
        ToySpec {
            element: Value("ElementIndex_Water"),
            alts: vec![AltSpec {
                load: "MinionLoad_106_IceSorcLegendary",
                deco: 3,
                display: None,
            }],
            ..toy("MinionLoad_106_IceSorc", Key("106_Toy_Name"), 106)
        },
        ToySpec {
            giant: true,
            element: Literal(7),
            alts: vec![AltSpec {
                load: "MinionLoad_112_TitanLifeAlt",
                deco: 2,
                display: Some("ToyType_Gnarly"),
            }],
            ..toy("MinionLoad_112_TitanLife", Key("112_Toy_Name"), 112)
        },
        magic(toy("ItemLoad_Anvil", Key("ItemName_Anvil"), 200)),
        ToySpec {
            alts: vec![AltSpec {
                load: "ItemLoad_Cannon_Red",
                deco: 2,
                display: Some("ToyType_CannonSpecial"),
            }],
            ..magic(toy("ItemLoad_Cannon", Key("ItemName_Cannon"), 208))
        },
        magic(toy(
            "ItemLoad_Location_Dragon",
            Key("ItemName_Location_Dragon"),
            300,
        )),
        ToySpec {
            sky1: true,
            element: Literal(5),
            ..magic(toy(
                "ItemLoad_PvP_BonusLevel",
                Key("ItemName_PvP_BonusLevel"),
                304,
            ))
        },
        ToySpec {
            pet: true,
            element: Literal(1),
            ..magic(toy(
                "ItemLoad_Pet_02_GillGrunt",
                Key("ItemName_Pet_02"),
                514,
            ))
        },
        ToySpec {
            pet: true,
            element: Literal(0),
            ..magic(toy("ItemLoad_Pet_06_Djinni", Key("ItemName_Pet_06"), 542))
        },
    ];
    DiscSpec {
        toys,
        alt_characters: vec![AltCharacterSpec {
            normal: "MinionLoad_016_Spyro",
            prefix: "ToyType_Dark",
            alt: "MinionLoad_016_SpyroDark",
        }],
        lightcore: vec![
            "MinionLoad_007_GemGolem",
            "MinionLoad_106_IceSorc",
            "MinionLoad_100_Eagle",
        ],
        minion_names: vec!["000_Toy_Name", "007_Toy_Name", "016_Toy_Name"],
        item_loads: vec![
            "ItemLoad_Anvil",
            "ItemLoad_Cannon",
            "ItemLoad_Pet_02_GillGrunt",
            "ItemLoad_Pet_06_Djinni",
            "ItemLoad_PvP_BonusLevel",
            "ItemLoad_Cannon_Red",
            "ItemLoad_Location_Dragon",
        ],
        location_loads: vec!["ItemLoad_Location_Dragon"],
        values: vec![("ElementIndex_Air", 2), ("ElementIndex_Water", 1)],
        texts: vec![
            ("ToyType_Series2", "Series 2"),
            ("ToyType_LightCore", "LightCore"),
            ("ToyType_Legendary", "Legendary"),
            ("ToyType_Polar", "Polar"),
            ("ToyType_Dark", "Dark"),
            ("ToyType_Gnarly", "Gnarly"),
            ("ToyType_CannonSpecial", "Golden Dragonfire"),
            ("000_Toy_Name", "Whirlwind"),
            ("007_Toy_Name", "Prism Break"),
            ("016_Toy_Name", "Spyro"),
            ("100_Toy_Name", "Jet-Vac"),
            ("106_Toy_Name", "Chill"),
            ("112_Toy_Name", "Tree Rex"),
            ("999_Toy_Name", "DEBUG"),
            ("ItemName_Anvil", "Anvil Rain"),
            ("ItemName_Cannon", "Cannon"),
            ("ItemName_Location_Dragon", "Dragon's Peak"),
            ("ItemName_PvP_BonusLevel", "Volcanic Vault"),
            ("ItemName_Pet_02", "Gill Runt"),
            ("ItemName_Pet_06", "Mini-Jini"),
        ],
        sprites: vec![
            (
                sprite_sets::MINIONS,
                vec![
                    "Collection_MinionIcon_000",
                    "Collection_MinionIcon_007",
                    "Collection_MinionIcon_016",
                    "Collection_MinionIcon_100",
                    "Collection_MinionIcon_106",
                    "Collection_MinionIcon_112",
                ],
            ),
            (
                sprite_sets::LEGENDARY,
                vec![
                    "Collection_MinionIcon_Leg_000_AirDragon",
                    "Collection_MinionIcon_Leg_100_Eagle",
                    "Collection_MinionIcon_Leg_112_TitanLife",
                ],
            ),
            (
                sprite_sets::LEGENDARY_LIGHTCORE,
                vec!["Collection_MinionIcon_LegLight_106_IceSorc"],
            ),
            (
                sprite_sets::LIGHTCORE,
                vec![
                    "Collection_MinionIcon_LightCore_007",
                    "Collection_MinionIcon_LightCore_100",
                    "Collection_MinionIcon_LightCore_106",
                ],
            ),
            (
                sprite_sets::OLD_LEGENDARIES,
                vec!["Collection_MinionIcon_416_Leg_SpyroDark"],
            ),
            (
                sprite_sets::SERIES_2,
                vec![
                    "Collection_MinionIcon_Series2_000",
                    "Collection_MinionIcon_Series2_007",
                    "Collection_MinionIcon_Series2_016",
                ],
            ),
            (
                sprite_sets::ITEMS,
                vec![
                    "Collection_ItemIcon_200_Anvil",
                    "Collection_ItemIcon_208_Cannon",
                    "Collection_ItemIcon_300_LocDragon",
                    "Collection_ItemIcon_304_PvPBonus",
                    "Collection_ItemIcon_514_Pet_GillGrunt",
                    "Collection_ItemIcon_542_Pet_Djinni",
                ],
            ),
            (
                sprite_sets::ITEMS_LEGENDARY,
                vec!["Collection_ItemIcon_208_Leg_Cannon"],
            ),
        ],
    }
}

/// Parts packed for the `toy_table` registry entry: a big-endian u32 length
/// before each.
pub fn bundle(parts: &[&[u8]]) -> Vec<u8> {
    parts
        .iter()
        .flat_map(|p| {
            (p.len() as u32)
                .to_be_bytes()
                .into_iter()
                .chain(p.iter().copied())
        })
        .collect()
}

/// The sample disc as a bundle: init_setup, global, bootstrap, collection,
/// the English pack.
pub fn sample_bundle() -> Vec<u8> {
    let d = disc(&sample());
    bundle(&[
        &d.init_setup,
        &d.global,
        &d.bootstrap,
        &d.collection,
        &d.packs[0],
    ])
}
