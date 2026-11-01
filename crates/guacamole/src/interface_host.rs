//! The original's interface on the run (openspec AT.9, design decisions 1
//! and 10): the model the run holds, loaded at its install off the
//! extract's interface part and the level's sprites part, and the host's
//! side of forwarding the interface's members to it. What a member does is
//! `guac-interface`'s ([`guac_interface::Keeper`]); the host only decides
//! what goes there:
//!
//! - every one of the interface's 64 members, but a sprite's screen size on
//!   the gameplay viewport's container (the camera's framing reads it, AG.28)
//!   and `spritematrixmeasurement.id` on what isn't a sprite (an actor's,
//!   a placement's `_ID`), which the host answers as before;
//! - any member on one of the keeper's objects (a sprite, a string, a
//!   converter, a sprite's scale, an initial matrix);
//! - a spawn of a sprite template or a sprite (an instance, not an actor)
//!   and a remove of a sprite.
//!
//! A run holds a model only when its part loaded: [`with_interface`]
//! registers the members then, and a run without keeps the earlier tables'
//! value cells, as before AT.9.

use std::path::Path;
use std::time::{Duration, Instant};

use guac_extract::intros::IrFormat;
use guac_interface::Keeper;
use guac_interface::load::{self, Imports, Keys};
use guac_ir::interface::{Interface, RECORD};
use guac_script::{Access, Answer, Config, HostFault, Value, members};

use crate::script_engine::Object;
use crate::script_members::Member;
use crate::scripts::ScriptWorld;

/// `config` with the interface's members on the host, which hands them to
/// the model it holds (`members::INTERFACE`, over the earlier tables'
/// cells): only for a run whose world holds one.
pub fn with_interface(mut config: Config) -> Config {
    members::register(&mut config.members, members::INTERFACE);
    config
}

/// The model a run starts from, as the level's load reads it: every
/// template, font and string of the linked archives the extract holds an
/// interface record for, what didn't load and what reading took. Each
/// install starts its run from a copy.
#[derive(Debug, Clone)]
pub struct InterfacePart {
    pub keeper: Keeper,
    /// The archives read, by unit, and those the extract held no record
    /// for, with why.
    pub read: Vec<(u16, String)>,
    pub missing: Vec<(String, String)>,
    /// What the loader couldn't resolve, by archive.
    pub refused: Vec<(String, String)>,
    pub took: Duration,
}

impl InterfacePart {
    pub fn templates(&self) -> usize {
        self.keeper.model().len()
    }
}

/// The interface record a unit's file has in the extract: the extract-wide
/// part's for an archive's `scripts/<stem>.igz`, the level's sprites part
/// for its `levels/<level>/script.igz`.
pub fn record_path(ir: IrFormat, unit: &str) -> Option<String> {
    let ext = match ir {
        IrFormat::Ron => "ron",
        IrFormat::Postcard => "postcard",
    };
    let script_dir = format!("{}/", guac_extract::scripts::SCRIPT_DIR);
    if let Some(stem) = unit
        .strip_prefix(&script_dir)
        .and_then(|r| r.strip_suffix(".igz"))
        .filter(|s| !s.is_empty() && !s.contains('/'))
    {
        return Some(format!(
            "{}/{stem}.v{RECORD}.{ext}",
            guac_extract::interface::DIR
        ));
    }
    let level_dir = format!("{}/", guac_extract::levels::LEVEL_DIR);
    let (level, file) = unit.strip_prefix(&level_dir)?.split_once('/')?;
    (file == format!("{}.igz", guac_extract::levels::SCRIPT) && !level.is_empty())
        .then(|| guac_extract::interface::level_path(ir, level))
}

fn read_record(dir: &Path, ir: IrFormat, rel: &str) -> Result<Interface, String> {
    let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    match ir {
        IrFormat::Ron => std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| guac_ir::from_ron(t).map_err(|e| e.to_string())),
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{rel}: {e}"))
}

/// The model for `units` (the linked program's, by their files in the
/// extract at `dir`), each with its file's `keys`: `None` when the extract
/// holds no record for `global.bld`, the interface part not in yet (a first
/// run hands chapter 1 over before it reads, AT.26), so the run keeps its
/// cells.
pub fn load(
    dir: &Path,
    ir: IrFormat,
    units: &[String],
    keys: &[Keys],
    imports: Option<&Imports>,
) -> Option<InterfacePart> {
    let started = Instant::now();
    let mut records = Vec::new();
    let mut missing = Vec::new();
    for k in keys {
        let Some(name) = units.get(usize::from(k.unit)) else {
            continue;
        };
        let Some(rel) = record_path(ir, name) else {
            continue;
        };
        if !dir.join(&rel).is_file() {
            missing.push((name.clone(), format!("{rel}: not in the extract")));
            continue;
        }
        match read_record(dir, ir, &rel) {
            Ok(r) => records.push((k, r)),
            Err(why) => missing.push((name.clone(), why)),
        }
    }
    if !records
        .iter()
        .any(|(_, r)| r.archive == guac_interface::load::GLOBAL)
    {
        return None;
    }
    let pairs: Vec<(&Keys, &Interface)> = records.iter().map(|(k, r)| (*k, r)).collect();
    let loaded = load::load(&pairs, imports);
    Some(InterfacePart {
        keeper: loaded.keeper,
        read: records
            .iter()
            .map(|(k, r)| (k.unit, r.archive.clone()))
            .collect(),
        missing,
        refused: loaded.refused,
        took: started.elapsed(),
    })
}

/// The level's load's line for the part: what it read, or why the run
/// keeps its cells.
pub fn log(part: Option<&InterfacePart>) {
    match part {
        Some(p) => {
            tracing::info!(
                archives = p.read.len(),
                templates = p.templates(),
                missing = p.missing.len(),
                refused = p.refused.len(),
                took_ms = p.took.as_millis() as u64,
                "the interface's model loaded: its members are the model's"
            );
            for (archive, why) in p.missing.iter().chain(&p.refused) {
                tracing::warn!(%archive, reason = %why, "an interface record didn't load whole");
            }
        }
        None => tracing::warn!(
            "no interface part in the extract yet: the run keeps the interface's members as cells"
        ),
    }
}

impl ScriptWorld {
    /// The model the scripts write from here on (openspec AT.9): the run's
    /// config should register [`with_interface`]'s members.
    pub fn set_interface(&mut self, keeper: Keeper) {
        self.interface = Some(keeper);
    }

    /// The model the run holds: none on a run installed before the
    /// extract held the interface part, whose members are cells.
    pub fn interface(&self) -> Option<&Keeper> {
        self.interface.as_ref()
    }

    /// Whether `at` goes to the model (the module docs).
    pub(crate) fn to_interface(&self, at: &Access) -> bool {
        let Some(k) = &self.interface else {
            return false;
        };
        let held = at.receiver.is_some_and(|r| k.holds(r));
        if !k.is_member(at.member) {
            return held;
        }
        match self.members.get(&at.member) {
            // The camera's framing reads the container's size (AG.28).
            Some(Member::ScreenWidth | Member::ScreenHeight) => {
                let container = at
                    .receiver
                    .and_then(|r| self.handle(r))
                    .and_then(|h| self.objs.get(h));
                container != Some(Object::ContainerSprite)
            }
            // An actor's or a placement's `_ID` stays the host's.
            Some(Member::Id) => held,
            _ => true,
        }
    }

    pub(crate) fn interface_get(&mut self, at: &Access) -> Result<Value, HostFault> {
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.get(at, &guac_interface::NoWorld)
    }

    pub(crate) fn interface_set(&mut self, at: &Access, v: Value) -> Result<(), HostFault> {
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.set(at, v, &guac_interface::NoWorld)
    }

    pub(crate) fn interface_reset(&mut self, at: &Access) -> Result<(), HostFault> {
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.reset(at, &guac_interface::NoWorld)
    }

    pub(crate) fn interface_elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.elements(at, &guac_interface::NoWorld)
    }

    /// A set written whole: the model's when it's the interface's, else
    /// dropped as before.
    pub(crate) fn set_elements(&mut self, at: &Access, e: &[Value]) -> Result<(), HostFault> {
        if !self.to_interface(at) {
            return Err(HostFault::Nothing);
        }
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.set_elements(at, e)
    }

    /// A spawn of one of the model's templates or sprites: an instance,
    /// spawned by `spawner`.
    pub(crate) fn spawn_sprite(
        &mut self,
        template: Value,
        at: Option<Value>,
        facing: Option<f32>,
        spawner: guac_script::Handle,
    ) -> Option<Answer> {
        let k = self.interface.as_mut()?;
        let clone = k.spawn(template, at, facing, Value::Entity(spawner))?;
        Some(Answer::Run {
            receiver: Some(clone),
            result: Some(clone),
        })
    }

    /// A remove taking one of the model's sprites off: false, and the
    /// host's to handle, for anything else.
    pub(crate) fn remove_sprite(&mut self, v: Option<Value>) -> bool {
        match (&mut self.interface, v) {
            (Some(k), Some(v)) => k.remove(v),
            _ => false,
        }
    }

    /// Whether `v` is one of the model's sprites, taken off: a tell on it
    /// skips its block, as on an actor taken off.
    pub(crate) fn sprite_removed(&self, v: Value) -> bool {
        self.interface.as_ref().is_some_and(|k| k.removed(v))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use guac_formats::script::Class;
    use guac_interface::Model;
    use guac_interface::keeper::BASES;
    use guac_ir::interface::{Content, Matrix, Template};
    use guac_script::{Handle, MemberId, Obj, OpCall};

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{Files, Placed};

    const BOX: Obj = Obj { unit: 1, index: 7 };
    const GRUNT: Obj = Obj { unit: 2, index: 3 };

    fn container() -> Template {
        Template {
            name: "Generic_Container".into(),
            object: BOX.index,
            properties: (1 << 16) | 3,
            flags: 0,
            position: [0, 0],
            z_depth: 0.0,
            priority: 0.0,
            width: 237.0,
            height: 96.0,
            scale: [100.0; 3],
            color: [0x80, 0x80, 0x80, 0xFF],
            replace_color: [0x80, 0x80, 0x80, 0],
            num_value: 0,
            depth: 0.0,
            init_matrix: Matrix::default(),
            matrix: Matrix::default(),
            content: Content::Container,
            string: None,
        }
    }

    /// A world over one actor placement, with the model holding one
    /// container template when `held`.
    fn world(held: bool) -> (ScriptWorld, Handle) {
        let files = Files {
            placed: vec![Placed {
                id: 41,
                ..Placed::named(GRUNT, "Grunt", None, 4)
            }],
            ids: HashMap::from([(GRUNT, 41)]),
            ..Files::default()
        };
        let handles = HashMap::from([(GRUNT, 0x20)]);
        let mut w = ScriptWorld::new(&files, &handles, [0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36]);
        if held {
            let mut m = Model::new(BASES);
            m.add_template(Some(BOX), &container(), Value::Null);
            w.set_interface(Keeper::new(m));
        }
        (w, 0x20)
    }

    fn at(receiver: Option<Value>, ns: &str, name: &str) -> Access {
        Access {
            placement: 0x20,
            receiver,
            member: MemberId::named(ns, name),
            other_point: None,
            via: None,
            op: None,
        }
    }

    fn spawn_call(template: Value) -> OpCall {
        OpCall {
            placement: 0x20,
            receiver: Value::Entity(0x20),
            class: Class::Spawn,
            operands: vec![("_LHS", Some(template))],
            words: Vec::new(),
            sets: Vec::new(),
        }
    }

    /// A clone of a sprite template is the model's instance, not an actor;
    /// its members land in the model; an actor's `_ID` and the viewport's
    /// container size stay the host's; a remove takes the instance off.
    #[test]
    fn the_host_forwards_the_interface_to_the_model_it_holds() {
        let (mut w, grunt) = world(true);
        let asked = std::cell::Cell::new(0);
        let mut spawner = |_| {
            asked.set(asked.get() + 1);
            0x100 + asked.get()
        };
        let clone = match w.op(&spawn_call(Value::Object(BOX)), &mut spawner, &mut NoEngine) {
            Ok(Answer::Run {
                result: Some(c), ..
            }) => c,
            other => panic!("{other:?}"),
        };
        assert!(w.interface().unwrap().holds(clone));
        assert!(w.clones().is_empty(), "no actor made for it");
        assert_eq!(asked.get(), 0, "no handle asked for");
        w.set(
            &at(Some(clone), "spriteinfo", "width"),
            Value::Float(12.0),
            &mut NoEngine,
        )
        .unwrap();
        let k = w.interface().unwrap();
        let id = k.model().sprite_of(clone).unwrap();
        assert_eq!(k.model().sprite(id).size[0], 12.0);
        assert_eq!(
            w.get(&at(Some(clone), "spriteinfo", "width"), &mut NoEngine),
            Ok(Value::Float(12.0))
        );
        // The host's own.
        assert_eq!(
            w.get(
                &at(Some(Value::Entity(grunt)), "spritematrixmeasurement", "id"),
                &mut NoEngine
            ),
            Ok(Value::Int(41))
        );
        let viewport = w
            .get(&at(None, "viewportinfo", "default"), &mut NoEngine)
            .unwrap();
        let container = w
            .get(
                &at(Some(viewport), "viewportinfo", "container sprite"),
                &mut NoEngine,
            )
            .unwrap();
        assert_eq!(
            w.get(
                &at(Some(container), "spriteinfo", "screen width"),
                &mut NoEngine
            ),
            Ok(Value::Float(crate::viewport::SCREEN_WIDTH))
        );
        // The class's own set reads through the host.
        assert_eq!(
            w.elements(&at(None, "spriteinfo", "active"), &mut NoEngine),
            Ok(vec![clone])
        );
        let remove = OpCall {
            class: Class::Remove,
            ..spawn_call(clone)
        };
        assert_eq!(w.op(&remove, &mut spawner, &mut NoEngine), Ok(Answer::Done));
        assert!(w.interface().unwrap().removed(clone));
        let tell = OpCall {
            class: Class::Control,
            ..spawn_call(clone)
        };
        assert_eq!(w.op(&tell, &mut spawner, &mut NoEngine), Ok(Answer::Skip));
    }

    /// Without a model the host is as before AT.9: a sprite template's clone
    /// a blank actor, an interface member the host doesn't answer refused.
    #[test]
    fn a_world_without_the_model_keeps_its_actors() {
        let (mut w, _) = world(false);
        let mut spawner = |_| 0x200;
        let clone = match w.op(&spawn_call(Value::Object(BOX)), &mut spawner, &mut NoEngine) {
            Ok(Answer::Run {
                result: Some(c), ..
            }) => c,
            other => panic!("{other:?}"),
        };
        assert_eq!(clone, Value::Entity(0x200));
        assert_eq!(w.clones(), vec![0x200]);
        assert_eq!(
            w.get(&at(Some(clone), "spriteinfo", "width"), &mut NoEngine),
            Err(HostFault::Unmodelled)
        );
    }

    #[test]
    fn a_units_record_is_its_archives_or_its_levels() {
        assert_eq!(
            record_path(IrFormat::Ron, "scripts/global.igz").as_deref(),
            Some("interface/global.v2.ron")
        );
        assert_eq!(
            record_path(IrFormat::Postcard, "scripts/009_eruptor.igz").as_deref(),
            Some("interface/009_eruptor.v2.postcard")
        );
        assert_eq!(
            record_path(IrFormat::Ron, "levels/level_213_tutotial/script.igz").as_deref(),
            Some("levels/level_213_tutotial/sprites.v2.ron")
        );
        assert_eq!(record_path(IrFormat::Ron, "scripts/a/b.igz"), None);
        assert_eq!(record_path(IrFormat::Ron, "elsewhere.igz"), None);
    }
}
