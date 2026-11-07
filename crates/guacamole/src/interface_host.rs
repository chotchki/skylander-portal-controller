//! The original's interface on the run (openspec AT.9, design decisions 1
//! and 10): the model the run holds, loaded at its install off the
//! extract's interface part and the level's sprites part, and the host's
//! side of forwarding the interface's members to it. What a member does is
//! `guac-interface`'s ([`guac_interface::Keeper`]); the host only decides
//! what goes there:
//!
//! - every one of the interface's 64 members, but `spritematrixmeasurement.id`
//!   on what isn't a sprite (an actor's, a placement's `_ID`), which the
//!   host answers as before;
//! - the gameplay viewport's `container sprite`, the model's `RT Viewport
//!   0` (the camera's framing reads its screen size, AG.28: the model
//!   answers 1280 by 720 as the host's own container did), and its `title
//!   safe zone`, the model's title-safe info;
//! - any member on one of the keeper's objects (a sprite, a string, a
//!   converter, a sprite's scale, an initial matrix);
//! - a spawn of a sprite template or a sprite (an instance, not an actor)
//!   and a remove of a sprite.
//!
//! A run holds a model only when its part loaded: [`with_interface`]
//! registers the members then, and a run without keeps the earlier tables'
//! value cells, as before AT.9.
//!
//! What the model draws reaches the TV by name (openspec AS.99): the TV
//! draws its own HUD and screens, so most of the model's sprites stay
//! unseen, and [`DRAWN_SPRITES`] lists the templates whose clones it draws
//! as the model lays them out, [`ScriptWorld::drawn_sprites`]. The next
//! script sprite joins by its template's name.

use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use guac_extract::intros::IrFormat;
use guac_interface::Keeper;
use guac_interface::draw::Entry;
use guac_interface::load::{self, Imports, Keys};
use guac_interface::sprite::State;
use guac_ir::interface::{Interface, RECORD};
use guac_script::{Access, Answer, Config, HostFault, MemberId, Value, members};

use crate::script_engine::Object;
use crate::script_members::Member;
use crate::scripts::ScriptWorld;

/// `viewportinfo.title safe zone`, hashed once: [`ScriptWorld::to_interface`]
/// asks it of nearly every member access a run holding the model makes.
static TITLE_SAFE_ZONE: LazyLock<MemberId> =
    LazyLock::new(|| MemberId::named("viewportinfo", "title safe zone"));

/// The sprite templates whose clones the TV draws over a level (openspec
/// AS.99), by name: a carried bomb's countdown numbers.
pub const DRAWN_SPRITES: [&str; 1] = ["Bomb_Counter"];

/// One image of a listed sprite this frame, in frame pixels (1280 by 720,
/// the model's frame): a glyph off its font's atlas or a piece of art.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteQuad {
    /// Its template's name.
    pub sprite: String,
    /// `textures/<sha256>.ktx2`.
    pub image: String,
    /// Left, top, right, bottom.
    pub rect: [f32; 4],
    /// The image's corner at the rectangle's top-left, then bottom-right.
    pub uv: [f32; 4],
    /// Its colour: a glyph's its sprite's as the letter's tints it, 0x80 a
    /// channel at full in the letter's bytes.
    pub colour: [u8; 4],
    /// The sprite's `numeric value`.
    pub value: f32,
}

/// `config` with the interface's members on the host, which hands them to
/// the model it holds (`members::INTERFACE`, over the earlier tables'
/// cells): only for a run whose world holds one.
pub fn with_interface(mut config: Config) -> Config {
    members::register(&mut config.members, members::INTERFACE);
    members::register(&mut config.members, members::VIEWPORT_INTERFACE);
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

/// A glyph's colour: its sprite's times its letter's, 0x80 a letter's channel
/// at full, the sprite's alpha.
fn tinted(sprite: [u8; 4], letter: [u8; 4]) -> [u8; 4] {
    let channel = |c: usize| (u32::from(sprite[c]) * u32::from(letter[c]) / 0x80).min(255) as u8;
    [channel(0), channel(1), channel(2), sprite[3]]
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

/// One interface record off the extract at `dir`, in `ir`'s format.
pub fn read_record(dir: &Path, ir: IrFormat, rel: &str) -> Result<Interface, String> {
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

/// The English text of the file string `obj` (a `StringInfo` of one of
/// `units`, whose strings `keys` hold), off its file's record in the extract
/// at `dir` (openspec AS.15, found at its review): how a run without the
/// model, its part landing after the level's load as a first run's does
/// (AT.26), reads its level end's string. Why not, when it can't.
pub fn file_text(
    dir: &Path,
    ir: IrFormat,
    units: &[String],
    keys: &[Keys],
    obj: guac_script::Obj,
) -> Result<String, String> {
    let unit = units
        .get(usize::from(obj.unit))
        .ok_or_else(|| format!("no unit {}", obj.unit))?;
    let rel = record_path(ir, unit).ok_or_else(|| format!("{unit} has no interface record"))?;
    let k = keys
        .iter()
        .find(|k| k.unit == obj.unit)
        .ok_or_else(|| format!("{unit}'s strings weren't read"))?;
    let at = k
        .strings
        .iter()
        .position(|&o| o == obj.index)
        .ok_or_else(|| format!("object {} isn't one of {unit}'s strings", obj.index))?;
    if !dir.join(&rel).is_file() {
        return Err(format!("{rel}: not in the extract yet"));
    }
    let r = read_record(dir, ir, &rel)?;
    if r.strings.len() != k.strings.len() {
        return Err(format!(
            "{rel}: {} strings for {} StringInfos",
            r.strings.len(),
            k.strings.len()
        ));
    }
    Ok(r.strings[at].text.clone())
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

    /// One frame of the model's layout: what a sprite's `screen location`
    /// reads off (openspec AS.76).
    pub fn lay_out_interface(&mut self) {
        if let Some(k) = &mut self.interface {
            k.model_mut().lay_out(&guac_interface::NoWorld);
        }
    }

    /// A clone of a [`DRAWN_SPRITES`] template stands in the model.
    pub fn draws_sprites(&self) -> bool {
        self.interface.as_ref().is_some_and(|k| {
            k.instances().into_iter().any(|id| {
                let s = k.model().sprite(id);
                s.state == State::Awake && DRAWN_SPRITES.contains(&s.name.as_str())
            })
        })
    }

    /// One frame of the model's layout and the listed sprites' images in it
    /// (the module docs), in the order the model draws them. A kind of
    /// sprite nothing here draws yet adds nothing.
    pub fn drawn_sprites(&mut self) -> Vec<SpriteQuad> {
        if !self.draws_sprites() {
            return Vec::new();
        }
        let Some(keeper) = &mut self.interface else {
            return Vec::new();
        };
        let model = keeper.model_mut();
        let frame = model.lay_out(&guac_interface::NoWorld);
        let list = model.draw_list(&frame);
        let quad = |e: &Entry| {
            let s = model.sprite(e.sprite());
            let (image, rect, uv, colour) = match e {
                Entry::Glyph {
                    atlas,
                    rect,
                    uv,
                    letter,
                    paint,
                    ..
                } => (atlas.clone(), *rect, *uv, tinted(paint.color, *letter)),
                Entry::Art {
                    texture: Some(texture),
                    rect,
                    uv,
                    paint,
                    ..
                } => (texture.clone(), *rect, *uv, paint.color),
                _ => return None,
            };
            DRAWN_SPRITES
                .contains(&s.name.as_str())
                .then(|| SpriteQuad {
                    sprite: s.name.clone(),
                    image,
                    rect,
                    uv,
                    colour,
                    value: s.numeric_value,
                })
        };
        list.iter().filter_map(quad).collect()
    }

    /// Whether `at` goes to the model (the module docs).
    pub(crate) fn to_interface(&self, at: &Access) -> bool {
        let Some(k) = &self.interface else {
            return false;
        };
        let held = at.receiver.is_some_and(|r| k.holds(r));
        // The viewport's container (`RT Viewport 0`) and title-safe info
        // are the model's.
        if self.members.get(&at.member) == Some(&Member::Container) || at.member == *TITLE_SAFE_ZONE
        {
            return at.receiver.and_then(|r| self.handle(r)) == Some(self.viewport);
        }
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
        let container = self.members.get(&at.member) == Some(&Member::Container);
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        if container {
            return Ok(k.container());
        }
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

    /// A set reference written: the model's copy when it's the
    /// interface's, else none kept.
    pub(crate) fn set_elements(&mut self, at: &Access, e: &[Value]) -> Result<(), HostFault> {
        if !self.to_interface(at) {
            return Err(HostFault::Nothing);
        }
        let k = self.interface.as_mut().ok_or(HostFault::Unmodelled)?;
        k.set_elements(at, e)
    }

    /// A set reference about to be read: the set the model changed it to.
    pub(crate) fn set_ref(&mut self, at: &Access) -> Option<Vec<Value>> {
        if !self.to_interface(at) {
            return None;
        }
        self.interface
            .as_mut()?
            .set_ref(at, &guac_interface::NoWorld)
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
        // The viewport's container is the model's, the screen's size.
        let viewport = w
            .get(&at(None, "viewportinfo", "default"), &mut NoEngine)
            .unwrap();
        let container = w
            .get(
                &at(Some(viewport), "viewportinfo", "container sprite"),
                &mut NoEngine,
            )
            .unwrap();
        assert_eq!(container, w.interface().unwrap().container());
        for (name, size) in [
            ("screen width", crate::viewport::SCREEN_WIDTH),
            ("screen height", crate::viewport::SCREEN_HEIGHT),
        ] {
            assert_eq!(
                w.get(&at(Some(container), "spriteinfo", name), &mut NoEngine),
                Ok(Value::Float(size))
            );
        }
        // The class's own set reads through the host.
        assert_eq!(
            w.elements(&at(None, "spriteinfo", "active"), &mut NoEngine),
            Ok(vec![container, clone])
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

    /// The level's load's read of the part (the review of AT.9): no model
    /// until `global.bld`'s record is in, whatever else is (a first run
    /// hands chapter 1 over before the part reads, AT.40); with it, every
    /// record the extract holds under its unit, each template in the model
    /// beside the viewport's container, and each unit whose record is
    /// absent or won't read named with why.
    #[test]
    fn a_part_loads_once_globals_record_is_in_and_names_what_it_lacks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(guac_extract::interface::DIR)).unwrap();
        let identity = guac_ir::DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        };
        let record = |archive: &str, templates: Vec<Template>| Interface {
            identity: identity.clone(),
            archive: archive.into(),
            templates,
            fonts: Vec::new(),
            strings: Vec::new(),
            refused: Vec::new(),
        };
        let write = |unit: &str, r: &Interface| {
            let rel = record_path(IrFormat::Ron, unit).unwrap();
            std::fs::write(dir.join(rel), guac_ir::to_ron(r).unwrap()).unwrap();
        };
        let units: Vec<String> = [
            "scripts/global.igz",
            "scripts/009_eruptor.igz",
            "scripts/010_hotdog.igz",
            "levels/level_213_tutotial/script.igz",
        ]
        .map(String::from)
        .to_vec();
        let keys: Vec<Keys> = (0..units.len() as u16)
            .map(|unit| Keys {
                unit,
                ..Keys::default()
            })
            .collect();
        let grunt = Template {
            name: "Generic_Text".into(),
            object: 3,
            ..container()
        };
        write(
            &units[1],
            &record("characters/009_eruptor.bld", vec![grunt]),
        );
        assert!(
            load(dir, IrFormat::Ron, &units, &keys, None).is_none(),
            "no model before global's record"
        );
        write(&units[0], &record(load::GLOBAL, vec![container()]));
        let rel = record_path(IrFormat::Ron, &units[2]).unwrap();
        std::fs::write(dir.join(rel), "not a record").unwrap();
        let part = load(dir, IrFormat::Ron, &units, &keys, None).expect("the model");
        assert_eq!(
            part.read,
            [
                (0, load::GLOBAL.to_owned()),
                (1, "characters/009_eruptor.bld".to_owned())
            ]
        );
        assert_eq!(
            part.templates(),
            3,
            "two templates and the viewport's container"
        );
        let missing: Vec<&str> = part.missing.iter().map(|(u, _)| u.as_str()).collect();
        assert_eq!(missing, [units[2].as_str(), units[3].as_str()]);
        assert!(part.missing[1].1.contains("not in the extract"));
        let model = part.keeper.model();
        assert!(
            model
                .sprite_of(Value::Object(Obj { unit: 0, index: 7 }))
                .is_some()
        );
        assert!(
            model
                .sprite_of(Value::Object(Obj { unit: 1, index: 3 }))
                .is_some()
        );
    }

    /// AS.15's review: a file string's English text read off its record
    /// once the part is in, by the string's place among its unit's
    /// `StringInfo`s; why not before the record lands, for an object that
    /// isn't one of the unit's strings, or a record whose strings don't
    /// pair with them.
    #[test]
    fn a_file_strings_text_reads_off_its_record_once_its_in() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(guac_extract::interface::DIR)).unwrap();
        let units: Vec<String> = ["scripts/bootstrap.igz", "scripts/global.igz"]
            .map(String::from)
            .to_vec();
        let keys = vec![Keys {
            unit: 1,
            strings: vec![40, 52, 77],
            ..Keys::default()
        }];
        let at = |index| Obj { unit: 1, index };
        let read = |obj| file_text(dir, IrFormat::Ron, &units, &keys, obj);
        assert!(read(at(52)).unwrap_err().contains("not in the extract yet"));
        let text = |t: &str| guac_ir::interface::Str {
            text: t.into(),
            flags: 0,
            integer_digits: 0,
            decimal_digits: 0,
        };
        let mut record = Interface {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            archive: load::GLOBAL.into(),
            templates: Vec::new(),
            fonts: Vec::new(),
            strings: vec![text("Some_Item"), text("Level_214_Tutotial_2"), text("")],
            refused: Vec::new(),
        };
        let rel = record_path(IrFormat::Ron, &units[1]).unwrap();
        std::fs::write(dir.join(&rel), guac_ir::to_ron(&record).unwrap()).unwrap();
        assert_eq!(read(at(52)).as_deref(), Ok("Level_214_Tutotial_2"));
        assert_eq!(read(at(77)).as_deref(), Ok(""));
        assert!(read(at(53)).is_err(), "not one of its strings");
        assert!(
            read(Obj { unit: 0, index: 52 }).is_err(),
            "its unit's strings unread"
        );
        record.strings.pop();
        std::fs::write(dir.join(&rel), guac_ir::to_ron(&record).unwrap()).unwrap();
        assert!(read(at(52)).unwrap_err().contains("2 strings for 3"));
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
