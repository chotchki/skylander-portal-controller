//! A character's scripts and what they hang, in code (`test-fixtures`):
//! the shapes the attachment stepper meets on the disc. A Def script sets
//! the references the hero's STARTUP clones from; the STARTUP picks a
//! template, swaps it on a bought upgrade ("Minion - Get Upgrade Details"
//! then `if upgrade N purchased == 1`), clones it onto an entry of the
//! hero's attach list, hangs a part on an earlier clone's `*root*`, loops a
//! pair onto two hands by the loop's index, makes a structure's record, and
//! hangs things under checks it can't read. Every name and number is ours.
//!
//! [`level`] writes them alone, models without bodies ([`super::fresh`]
//! needs no geometry); [`scripts_in`] writes them into a character archive
//! the model builder makes, each template's model a static prop there.

use std::collections::BTreeMap;

use crate::fnv1a32;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;
use crate::level::builder::placement;

use super::{ATTACH_NODE, ATTACH_NODES, MY, TAG_READY, UPGRADE_DETAILS};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// One attach point of a model: the bone, its index, and the offset
/// (position, then tilt, heading and roll).
#[derive(Debug, Clone, PartialEq)]
pub struct PointSpec {
    pub bone: &'static str,
    pub index: i32,
    pub transform: [f32; 6],
}

impl PointSpec {
    pub fn bone(bone: &'static str, index: i32) -> Self {
        Self {
            bone,
            index,
            transform: [0.0; 6],
        }
    }

    /// `*root*` at an offset along z, as a spear head's on its shaft.
    pub fn root(z: f32) -> Self {
        Self {
            bone: "*root*",
            index: -1,
            transform: [0.0, 0.0, z, 0.0, 0.0, 0.0],
        }
    }
}

/// A placement a clone is made from: a `tfbActorInfo`, drawing a model or
/// not (a dummy).
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSpec {
    pub name: &'static str,
    /// The model's path; `None` draws nothing.
    pub model: Option<&'static str>,
    /// The model's attach list.
    pub points: Vec<PointSpec>,
}

/// A clone's template or a reference's value: a template, a file-level
/// reference, a variable, a `global.bld` export or a structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A template, by index.
    Template(usize),
    /// A file-level `PlacementReference`, by name: the Def script sets it.
    Ref(&'static str),
    /// A variable of the hero's script, by name.
    Var(&'static str),
    /// A `global.bld` export, by id.
    Global(u32),
    /// A structure: its clone is a record.
    Record,
}

/// Where a clone hangs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// An entry of the hero's attach list.
    Hero(usize),
    /// The hero itself (`attach node = my`).
    HeroItself,
    /// An entry of an earlier clone's attach list, the clone by the
    /// variable it was put in.
    Clone(&'static str, usize),
}

/// One step of the hero's STARTUP.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `set reference <var> to <source>`.
    Set(&'static str, Source),
    /// A call to "Minion - Get Upgrade Details" with these parameters, then,
    /// for each, `if <parameter> == 1 { steps }`.
    Upgrades(Vec<(&'static str, Vec<Step>)>),
    /// `clone <from> { attach node = <on>; set <keep> to the clone }`.
    Clone {
        from: Source,
        on: On,
        keep: Option<&'static str>,
    },
    /// `if my . collision volume == 1 { steps }`: a check on a value the
    /// stepper can't read.
    Unreadable(Vec<Step>),
    /// `loop 2 { clone <from> { if index == 0 attach <a> else if index == 1
    /// attach <b> } }`, the else-if naming the first check.
    Pair { from: Source, slots: [usize; 2] },
    /// `if <var> == 0 { steps } else if <var> == 1 { more }`.
    Switch(&'static str, Vec<Step>, Vec<Step>),
    /// `set value <var> = <n>`.
    SetInt(&'static str, i32),
    /// `flow end`.
    End,
}

/// What [`level`] and [`scripts_in`] write.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptSpec {
    /// The hero's script's source path.
    pub script: &'static str,
    /// The hero model's attach list.
    pub hero_points: Vec<PointSpec>,
    pub templates: Vec<TemplateSpec>,
    /// The Def script's writes, after its tag-ready check: a reference and
    /// the template it's set to.
    pub def: Vec<(&'static str, usize)>,
    pub startup: Vec<Step>,
}

/// The knife a fresh figure holds, in each hand.
pub const KNIFE: usize = 0;
pub const KNIFE_UPGRADE: usize = 1;
pub const ARMOR: usize = 2;
pub const GUN: usize = 3;
pub const SPEAR: usize = 4;
pub const DUMMY: usize = 5;
pub const BOOMERANG: usize = 6;
pub const HAT: usize = 7;

/// Stealth-Elf-shaped and then some: the Def script sets `knife(1)` to the
/// plain knife and `knife(2)` to the upgraded one; STARTUP takes `knife(1)`,
/// swaps in `knife(2)` if upgrade 2 was bought and adds armour on upgrade
/// 3, then clones the knife onto each hand, a gun with a spear on its
/// `*root*`, a dummy (no model) onto the hero itself, and two boomerangs
/// by a loop's index (the switch before it picks them over the hat on a
/// variable nothing wrote), then makes a structure's record. A fresh figure
/// carries two knives, the gun, the spear and the two boomerangs.
pub fn sample() -> ScriptSpec {
    let model = |name, model, points| TemplateSpec {
        name,
        model: Some(model),
        points,
    };
    ScriptSpec {
        script: "C:/tfb/Content/Levels/Includes/Minions/Synthetic/scripts/026_Synthetic.ai",
        hero_points: vec![
            PointSpec::bone("J_attach_L", 14),
            PointSpec::bone("J_attach_R", 20),
            PointSpec {
                bone: "J_chest",
                index: 3,
                transform: [0.0, 0.1, 0.0, 0.0, 90.0, 0.0],
            },
        ],
        templates: vec![
            model("026_Knife_reg1", "Models/Characters/Knife.mdl", Vec::new()),
            model(
                "026_KnifeUpgrade_reg2",
                "Models/Characters/KnifeUpgrade1.mdl",
                Vec::new(),
            ),
            model("026_Armor", "Models/Characters/Armor.mdl", Vec::new()),
            model(
                "026_Gun",
                "Models/Characters/Gun.mdl",
                vec![PointSpec::root(0.9144)],
            ),
            model("026_Spear", "Models/Characters/Spear.mdl", Vec::new()),
            TemplateSpec {
                name: "026_Dummy",
                model: None,
                points: Vec::new(),
            },
            model(
                "026_Boomerang",
                "Models/Characters/Boomerang.mdl",
                Vec::new(),
            ),
            model("026_Hat", "Models/Characters/Hat.mdl", Vec::new()),
        ],
        def: vec![("026_knife(1)", KNIFE), ("026_knife(2)", KNIFE_UPGRADE)],
        startup: vec![
            Step::Set("knife", Source::Ref("026_knife(1)")),
            Step::Upgrades(vec![
                ("upgrade 1 purchased", Vec::new()),
                (
                    "upgrade 2 purchased",
                    vec![Step::Set("knife", Source::Ref("026_knife(2)"))],
                ),
                (
                    "upgrade 3 purchased",
                    vec![Step::Clone {
                        from: Source::Template(ARMOR),
                        on: On::Hero(2),
                        keep: None,
                    }],
                ),
            ]),
            Step::Clone {
                from: Source::Var("knife"),
                on: On::Hero(1),
                keep: None,
            },
            Step::Clone {
                from: Source::Var("knife"),
                on: On::Hero(0),
                keep: None,
            },
            Step::Clone {
                from: Source::Template(GUN),
                on: On::Hero(2),
                keep: Some("gun"),
            },
            Step::Clone {
                from: Source::Template(SPEAR),
                on: On::Clone("gun", 0),
                keep: None,
            },
            Step::Clone {
                from: Source::Template(DUMMY),
                on: On::HeroItself,
                keep: None,
            },
            Step::Switch(
                "boomerang kind",
                vec![Step::Set("boomerang", Source::Template(BOOMERANG))],
                vec![Step::Set("boomerang", Source::Template(HAT))],
            ),
            Step::Pair {
                from: Source::Var("boomerang"),
                slots: [0, 1],
            },
            Step::Clone {
                from: Source::Record,
                on: On::HeroItself,
                keep: None,
            },
        ],
    }
}

/// The objects a writer shares: the literal classes, the stubs, the cells.
struct Writer<'b> {
    b: &'b mut IgzBuilder,
    blocks: u8,
    ops: Vec<Location>,
    int: u32,
    flow_continue: Location,
    flow_macro: Location,
    vars: BTreeMap<&'static str, Location>,
    refs: &'b BTreeMap<&'static str, Location>,
    templates: &'b [Location],
    record: Location,
}

#[derive(Debug, Clone, Copy)]
enum El {
    Object(Location),
    Engine(&'static str, &'static str),
    My,
    Global(u32),
}

impl<'b> Writer<'b> {
    fn op(&mut self, ty: &str, size: u32, name: &str) -> Location {
        let o = self.b.object(0, ty, size);
        self.b.string(at(o, 0x08), name);
        self.ops.push(o);
        o
    }

    fn close_at(&mut self, start: usize) {
        let n = self.ops.len() - 1 - start;
        let op = self.ops[start];
        self.b.put_u32(at(op, 0x18), n as u32);
    }

    fn path(&mut self, stack: Location, elements: &[El]) {
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
                El::Object(o) => self.b.pointer(w, *o),
                El::Engine(ns, member) => {
                    self.b.import(w, ns, member);
                }
                El::My => {
                    self.b.import_raw(w, MY.1, fnv1a32(MY.0.as_bytes()));
                }
                El::Global(id) => {
                    self.b
                        .import_raw(w, *id, fnv1a32(crate::exports::NAMESPACE.as_bytes()));
                }
            }
        }
    }

    fn stack(&mut self, ty: &str, size: u32, elements: &[El]) -> Location {
        let s = self.b.object(0, ty, size);
        self.path(s, elements);
        s
    }

    fn int(&mut self, value: i32) -> Location {
        let s = self.stack("RHSValueStack", 0x28, &[]);
        self.b.unflagged_meta(at(s, 0x1C), self.int);
        self.b.put_u32(at(s, 0x20), value as u32);
        s
    }

    /// Giants' files write `RHSReferenceStack` 4 bytes over the registry.
    fn reference(&mut self, elements: &[El]) -> Location {
        let s = self.stack("RHSReferenceStack", 0x30, elements);
        self.b.unflagged_meta(at(s, 0x1C), self.int);
        s
    }

    fn var(&mut self, name: &'static str) -> Location {
        if let Some(v) = self.vars.get(name) {
            return *v;
        }
        let v = self.b.object(0, "OpCreateVariable", 0x2C);
        self.b.string(at(v, 0x08), "create variable||with|members");
        self.b.string(at(v, 0x1C), name);
        self.b.put_u32(at(v, 0x28), u32::MAX);
        self.vars.insert(name, v);
        v
    }

    fn source(&mut self, s: Source) -> El {
        match s {
            Source::Template(t) => El::Object(self.templates[t]),
            Source::Ref(name) => El::Object(self.refs[name]),
            Source::Var(name) => El::Object(self.var(name)),
            Source::Global(id) => El::Global(id),
            Source::Record => El::Object(self.record),
        }
    }

    fn set_ref(&mut self, lhs: &[El], rhs: Location, index: Option<i32>) {
        let lhs = self.stack("ReferenceStack", 0x18, lhs);
        let index = index.map(|i| self.int(i));
        let s = self.op("OpSetReference", 0x28, "set *reference||to");
        self.b.pointer(at(s, 0x1C), lhs);
        self.b.pointer(at(s, 0x20), rhs);
        if let Some(i) = index {
            self.b.pointer(at(s, 0x24), i);
        }
    }

    fn set_int(&mut self, var: Location, value: i32) {
        let lhs = self.stack("ValueStack", 0x1C, &[El::Object(var)]);
        let rhs = self.int(value);
        let s = self.op("OpSetValue", 0x24, "set value||=");
        self.b.pointer(at(s, 0x1C), lhs);
        self.b.pointer(at(s, 0x20), rhs);
    }

    /// `if <lhs> <relation> <rhs>`; its position, for [`Writer::close_at`].
    fn check(&mut self, lhs: &[El], relation: u32, rhs: Location) -> usize {
        let lhs = self.stack("ValueStack", 0x1C, lhs);
        let start = self.ops.len();
        let c = self.op("OpCheckValue", 0x28, "check value");
        self.b.pointer(at(c, 0x1C), lhs);
        self.b.put_u32(at(c, 0x20), relation);
        self.b.pointer(at(c, 0x24), rhs);
        start
    }

    /// Hangs the clone at `clone` (an `OpSpawn`) on `on`.
    fn hang(&mut self, clone: Location, on: On) {
        let lhs = [
            El::Object(clone),
            El::Engine("abstractplacement", ATTACH_NODE),
        ];
        let (rhs, index) = match on {
            On::Hero(slot) => (
                self.reference(&[El::My, El::Engine(ATTACH_NODES.0, ATTACH_NODES.1)]),
                Some(slot as i32),
            ),
            On::HeroItself => (self.reference(&[El::My]), None),
            On::Clone(var, slot) => {
                let v = self.var(var);
                (
                    self.reference(&[El::Object(v), El::Engine(ATTACH_NODES.0, ATTACH_NODES.1)]),
                    Some(slot as i32),
                )
            }
        };
        self.set_ref(&lhs, rhs, index);
    }

    /// `clone <from>` and its block; returns the op and its position.
    fn spawn(&mut self, from: Source) -> (Location, usize) {
        let from = self.source(from);
        let template = self.stack("AbstractPlacementList", 0x18, &[from]);
        let at_hero = self.stack("PositionStack", 0x18, &[El::My]);
        let start = self.ops.len();
        let s = self.op("OpSpawn", 0x30, "clone||at|facing||cloned");
        self.b.pointer(at(s, 0x20), template);
        self.b.pointer(at(s, 0x28), at_hero);
        (s, start)
    }

    fn end_block(&mut self, start: usize) {
        self.ops.push(self.flow_continue);
        self.close_at(start);
    }

    fn steps(&mut self, steps: &[Step]) {
        for s in steps {
            self.step(s);
        }
    }

    fn step(&mut self, step: &Step) {
        match step {
            Step::Set(var, source) => {
                let v = self.var(var);
                let el = self.source(*source);
                let rhs = self.reference(&[el]);
                self.set_ref(&[El::Object(v)], rhs, None);
            }
            Step::SetInt(var, value) => {
                let v = self.var(var);
                self.set_int(v, *value);
            }
            Step::Upgrades(params) => {
                let def = self.b.object(0, "OpDefineMacro", 0x38);
                self.b.string(at(def, 0x08), UPGRADE_DETAILS);
                let np = self.stack("ScriptGroupStack", 0x1C, &[El::Object(def)]);
                let start = self.ops.len();
                let call = self.op("OpUseMacro", 0x24, "use macro");
                self.b.pointer(at(call, 0x1C), np);
                let iface_at = self.ops.len();
                self.op("OpMacroInterface", 0x20, "INTERFACE");
                let mut objects = Vec::new();
                for (name, _) in params {
                    let rhs = self.int(0);
                    let p = self.op("OpMacroParameter", 0x48, "parameter");
                    self.b.string(at(p, 0x1C), name);
                    self.b.put_u32(at(p, 0x28), u32::MAX);
                    self.b.pointer(at(p, 0x30), rhs);
                    self.b.pointer(at(p, 0x38), def);
                    objects.push(p);
                }
                self.ops.push(self.flow_macro);
                self.close_at(iface_at);
                for ((_, body), p) in params.iter().zip(objects) {
                    let one = self.int(1);
                    let c = self.check(&[El::Object(p)], 1, one);
                    self.steps(body);
                    self.end_block(c);
                }
                self.end_block(start);
            }
            Step::Clone { from, on, keep } => {
                let (s, start) = self.spawn(*from);
                if *from != Source::Record {
                    self.hang(s, *on);
                }
                if let Some(var) = keep {
                    let v = self.var(var);
                    let rhs = self.reference(&[El::Object(s)]);
                    self.set_ref(&[El::Object(v)], rhs, None);
                }
                self.end_block(start);
            }
            Step::Unreadable(body) => {
                let one = self.int(1);
                let c = self.check(
                    &[El::My, El::Engine("actorinfo", "collision volume")],
                    1,
                    one,
                );
                self.steps(body);
                self.end_block(c);
            }
            Step::Pair { from, slots } => {
                let count = self.int(2);
                let start = self.ops.len();
                let l = self.op("OpLoopValue", 0x28, "loop value|||loop index");
                self.b.pointer(at(l, 0x20), count);
                let (s, spawn_at) = self.spawn(*from);
                let zero = self.int(0);
                let first = self.ops.len();
                let c = self.check(&[El::Object(l)], 1, zero);
                self.hang(s, On::Hero(slots[0]));
                let skip_at = self.ops.len();
                self.op("OpAbstractFlow", 0x1C, "");
                self.close_at(c);
                // The else-if names the first check: `check value` over the
                // check op itself.
                let one = self.int(1);
                let first_op = self.ops[first];
                let c2 = self.check(&[El::Object(first_op)], 1, one);
                self.hang(s, On::Hero(slots[1]));
                self.close_at(c2);
                self.close_at(skip_at);
                self.end_block(spawn_at);
                let flow = self.b.object(0, "OpFlowLoop", 0x1C);
                self.ops.push(flow);
                self.close_at(start);
            }
            Step::Switch(var, zero, one) => {
                let v = self.var(var);
                let lit0 = self.int(0);
                let c = self.check(&[El::Object(v)], 1, lit0);
                self.steps(zero);
                let skip_at = self.ops.len();
                self.op("OpAbstractFlow", 0x1C, "");
                self.close_at(c);
                let lit1 = self.int(1);
                let c2 = self.check(&[El::Object(v)], 1, lit1);
                self.steps(one);
                self.close_at(c2);
                self.close_at(skip_at);
            }
            Step::End => {
                let e = self.op("OpFlowBehavior", 0x1C, "flow end");
                self.b
                    .put_u32(at(e, 0x18), u32::from(crate::igz::layouts::BRANCH_END));
            }
        }
    }
}

/// A script named `name` whose ops `write` pushes inside PRESCRIPT; its
/// STARTUP holds `startup`'s, the top level after it `top`'s.
fn script(
    b: &mut IgzBuilder,
    blocks: u8,
    name: &str,
    refs: &BTreeMap<&'static str, Location>,
    templates: &[Location],
    startup: &dyn Fn(&mut Writer<'_>),
    top: &dyn Fn(&mut Writer<'_>),
) -> Location {
    let info = b.object(0, "tfbScriptInfo", 0x1C);
    let ops = b.object(0, "OpCodeList", 0x18);
    b.string(at(info, 0x08), name);
    b.put_bytes(at(info, 0x10), &[1]);
    b.pointer(at(info, 0x14), ops);
    let int = b.meta_object(at(info, 0x0C), "metaobject", "IntMeasurement");
    let flow_continue = b.object(0, "OpFlowBuiltInBehavior", 0x1C);
    b.string(at(flow_continue, 0x08), "flow continue");
    let flow_macro = b.object(0, "OpFlowMacro", 0x1C);
    b.string(at(flow_macro, 0x08), "flow macro");
    let record = b.object(0, "ScriptStructure", 0x20);
    b.string(at(record, 0x08), "Synthetic Projectile Description");
    let mut w = Writer {
        b,
        blocks,
        ops: Vec::new(),
        int,
        flow_continue,
        flow_macro,
        vars: BTreeMap::new(),
        refs,
        templates,
        record,
    };
    w.op("OpPreScript", 0x1C, "PRESCRIPT");
    w.op("OpStartUp", 0x1C, "STARTUP");
    startup(&mut w);
    w.close_at(1);
    let shut = w.ops.len();
    w.op("OpShutDown", 0x1C, "SHUTDOWN");
    w.close_at(shut);
    top(&mut w);
    w.close_at(0);
    let ops_list: Vec<Option<Location>> = w.ops.iter().copied().map(Some).collect();
    list(w.b, ops, blocks, &ops_list);
    info
}

/// A model's attach list onto `model` (a `tfbModelInfo`).
pub fn attach_list(b: &mut IgzBuilder, blocks: u8, model: Location, points: &[PointSpec]) {
    let l = b.object(0, "BoneAttachInfoList", 0x18);
    b.pointer(at(model, 0x24), l);
    let nodes: Vec<Option<Location>> = points
        .iter()
        .map(|p| {
            let n = b.object(0, "BoneAttachInfo", 0x3C);
            b.string(at(n, 0x08), p.bone);
            b.string(at(n, 0x1C), p.bone);
            b.put_u32(at(n, 0x20), p.index as u32);
            let bytes: Vec<u8> = p.transform.iter().flat_map(|v| v.to_be_bytes()).collect();
            b.put_bytes(at(n, 0x24), &bytes);
            Some(n)
        })
        .collect();
    list(b, l, blocks, &nodes);
}

/// `spec`'s templates, references, Def script and hero script into `b`,
/// the hero being the actor at `hero` drawing `hero_model`. `model` makes
/// each template's model (a static prop in a character archive, a bare
/// `tfbPhysicsModel` in [`level`]); the attach lists are written here.
pub fn scripts_in(
    b: &mut IgzBuilder,
    blocks: u8,
    spec: &ScriptSpec,
    hero: Location,
    hero_model: Location,
    model: &mut dyn FnMut(&mut IgzBuilder, &TemplateSpec) -> Location,
) {
    attach_list(b, blocks, hero_model, &spec.hero_points);
    let templates: Vec<Location> = spec
        .templates
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let o = b.object(0, "tfbActorInfo", 0xF8);
            placement(b, o, t.name, 100 + i as u32, &[0.0; 6], 5);
            if t.model.is_some() {
                let m = model(b, t);
                attach_list(b, blocks, m, &t.points);
                b.pointer(at(o, 0xDC), m);
            }
            o
        })
        .collect();
    let mut refs = BTreeMap::new();
    for s in &spec.startup {
        collect_refs(s, &mut |name| {
            refs.entry(name).or_insert(Location::new(0, 0));
        });
    }
    for (name, _) in &spec.def {
        refs.entry(*name).or_insert(Location::new(0, 0));
    }
    for (name, r) in refs.iter_mut() {
        *r = b.object(0, "PlacementReference", 0x38);
        b.string(at(*r, 0x08), name);
    }

    // The Def script: its tag-ready check, then the references.
    let def = spec.def.clone();
    let def_script = script(
        b,
        blocks,
        "C:/tfb/Content/Levels/Includes/Minions/Synthetic/scripts/026_Def_Synthetic.ai",
        &refs,
        &templates,
        &|_| {},
        &|w| {
            let call_def = w.b.object(0, "OpDefineMacro", 0x38);
            w.b.string(at(call_def, 0x08), TAG_READY);
            let np = w.stack("ScriptGroupStack", 0x1C, &[El::Object(call_def)]);
            let start = w.ops.len();
            let call = w.op("OpUseMacro", 0x24, "use macro");
            w.b.pointer(at(call, 0x1C), np);
            let iface = w.ops.len();
            w.op("OpMacroInterface", 0x20, "INTERFACE");
            let zero = w.int(0);
            let ready = w.op("OpMacroParameter", 0x48, "parameter");
            w.b.string(at(ready, 0x1C), "ready");
            w.b.put_u32(at(ready, 0x28), u32::MAX);
            w.b.pointer(at(ready, 0x30), zero);
            w.b.pointer(at(ready, 0x38), call_def);
            w.ops.push(w.flow_macro);
            w.close_at(iface);
            let zero = w.int(0);
            let c = w.check(&[El::Object(ready)], 1, zero);
            w.step(&Step::End);
            w.end_block(c);
            w.end_block(start);
            for (name, t) in &def {
                let r = w.refs[name];
                let rhs = w.reference(&[El::Object(w.templates[*t])]);
                w.set_ref(&[El::Object(r)], rhs, None);
            }
        },
    );
    let d = b.object(0, "tfbActorInfo", 0xF8);
    placement(b, d, "026_Synthetic_Def", 99, &[0.0; 6], 4);
    b.pointer(at(d, 0xA8), def_script);

    let startup = spec.startup.clone();
    let hero_script = script(
        b,
        blocks,
        spec.script,
        &refs,
        &templates,
        &|w| w.steps(&startup),
        &|_| {},
    );
    b.pointer(at(hero, 0xA8), hero_script);
}

fn collect_refs(step: &Step, f: &mut dyn FnMut(&'static str)) {
    let mut source = |s: &Source| {
        if let Source::Ref(name) = s {
            f(name);
        }
    };
    match step {
        Step::Set(_, s) | Step::Clone { from: s, .. } | Step::Pair { from: s, .. } => source(s),
        Step::Upgrades(params) => {
            for (_, body) in params {
                for s in body {
                    collect_refs(s, f);
                }
            }
        }
        Step::Unreadable(body) => {
            for s in body {
                collect_refs(s, f);
            }
        }
        Step::Switch(_, a, b) => {
            for s in a.iter().chain(b) {
                collect_refs(s, f);
            }
        }
        Step::SetInt(..) | Step::End => {}
    }
}

/// `spec` alone in a level: the hero (`026_Synthetic`, a `tfbActorInfo`
/// drawing a bare `tfbPhysicsModel`), each template's model bare too.
pub fn level(spec: &ScriptSpec) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let hero = b.object(0, "tfbActorInfo", 0xF8);
    placement(&mut b, hero, "026_Synthetic", 26, &[0.0; 6], 5);
    let hero_model = b.object(0, "tfbPhysicsModel", 0x30);
    crate::model::builder::info(&mut b, hero_model, "Models/Characters/Synthetic.mdl");
    b.pointer(at(hero, 0xDC), hero_model);
    scripts_in(&mut b, blocks, spec, hero, hero_model, &mut |b, t| {
        let m = b.object(0, "tfbPhysicsModel", 0x30);
        crate::model::builder::info(b, m, t.model.unwrap_or_default());
        m
    });
    b.build()
}

/// Seeds for the property test and the fuzz corpus: the sample, and the
/// sample with a model under a check the stepper can't read, a clone of a
/// reference nothing wrote and a model hanging on a dummy.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut unreadable = sample();
    unreadable.startup.push(Step::Unreadable(vec![Step::Clone {
        from: Source::Template(HAT),
        on: On::Hero(2),
        keep: None,
    }]));
    let mut unwritten = sample();
    unwritten.startup.push(Step::Clone {
        from: Source::Ref("026_never_set"),
        on: On::Hero(0),
        keep: None,
    });
    let mut on_dummy = sample();
    on_dummy.startup.extend([
        Step::Clone {
            from: Source::Template(DUMMY),
            on: On::Hero(0),
            keep: Some("dummy"),
        },
        Step::Clone {
            from: Source::Template(HAT),
            on: On::Clone("dummy", 0),
            keep: None,
        },
    ]);
    [sample(), unreadable, unwritten, on_dummy]
        .iter()
        .map(level)
        .collect()
}
