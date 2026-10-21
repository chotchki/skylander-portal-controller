//! The original's scripts running in guacamole (openspec AH.16, design
//! decisions 2, 4 and 21): the VM over the level's script part, the engine
//! members its whitelisted scripts reach, and the system that steps it.
//!
//! - [`Files`]: what the host reads off the files the scripts load with:
//!   every actor placement (its object, name, script, activity state, `_ID`,
//!   deactivation buffer and waypoints) and every placement's `_ID`, sprites
//!   and lights included.
//! - [`ScriptWorld`]: the engine state the scripts read and write, kept as
//!   the engine keeps it and nowhere else (decision 21: no copy of it beside
//!   the VM): the console's constants, the game's speed, the level
//!   a script asks for, each placement's numbers, the objects the scripts
//!   name through members (the pads, the portal).
//! - [`WorldHost`]: [`guac_script::Host`] over a world, a generator and how
//!   clones get handles: plain for the headless runs and the disc tests,
//!   [`ScriptHost`] inside the TV, a `SystemParam` whose clones are entities.
//! - [`init_chain`]: the VM AH.16 runs: `global.bld`'s 17 running placements
//!   for their first tick, `init_setup.bld`'s running placements and level
//!   213's definition and enemy system every tick, every other running
//!   placement inert (decision 6), each placement's handle an entity's bits.
//!
//! What the host answers, member by member, is [`guac_script::members`]'s
//! table: `system.hardware platform` PS 3 (the data is the PS3 build's),
//! `system.game configuration` Golden Master (a retail disc, a reading),
//! both languages English, `system.game time scale` kept as written from
//! 100 %, `system.load level` kept
//! until the step ends, when the host takes the request (guacamole loads
//! chapter 1 by its own rule; the request is logged), `placement.
//! deactivation buffer` per placement from its file, `abstractplacement.id`
//! and `spritematrixmeasurement.id` the object's `_ID` (a clone its
//! template's), `actorinfo.waypoints` the placement's waypoint objects, the
//! pads and the portal one object each. World ops: a tell runs its block on
//! any placement or clone not taken off; a spawn of a template makes a clone
//! (an entity in the TV, nothing drawn yet); a remove takes a clone or a
//! placement off; a reset puts a placement's numbers back as its file has
//! them; a stopped sequence is nothing (none plays yet). Any other op or
//! member stops the script that reaches it, named (play mode: that script
//! alone).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{ActorWaypointList, Layout, is_a};
use guac_formats::level::Placements;
use guac_formats::script::Class;
use guac_script::members::{self, INIT_CHAIN};
use guac_script::{
    Access, Answer, Config, Generator, Halt, Handle, Host, HostFault, MemberId, Mode, Obj, OpCall,
    PlacementId, Program, Value, Vm, Whitelist,
};
use tracing::{info, warn};

use crate::script_engine::{
    Actor, Angles, Engine, NoEngine, Object, Objects, Pad, Physics, Sequence, Solid,
};
use crate::script_members::Member;

/// `system.hardware platform`'s PS 3.
pub const PS3: i32 = 1;
/// `system.game configuration`'s Golden Master: a retail disc.
pub const GOLDEN_MASTER: i32 = 3;
/// English among the languages' labels.
pub const ENGLISH: i32 = 6;
/// `system.game time scale` at full speed, in its registered percent.
pub const FULL_SPEED: f32 = 100.0;

/// `_activityState` UNMANAGED: a placement a level file holds that runs.
const RUNNING: u32 = 4;
/// `AbstractPlacement._ID`'s offset.
const ID: u32 = 0x20;

/// An actor placement of a file the scripts load with, and what the engine
/// makes an actor of it from (AH.17): where it stands, its turn, its scale,
/// its movement numbers and its solid as its file holds them.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub obj: Obj,
    pub name: String,
    /// The `.ai` source path its script has.
    pub script: Option<String>,
    pub activity_state: u32,
    /// `_ID`.
    pub id: i32,
    /// `_activationRangeBuffer`: `placement.deactivation buffer`, in feet.
    pub buffer: f32,
    /// Its `ActorWaypointList`'s objects, in path order.
    pub waypoints: Vec<Obj>,
    /// `_initMatrix`: metres and `[tilt, heading, roll]` degrees.
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    /// Percent per axis.
    pub scale: [f32; 3],
    /// RGBA as stored.
    pub tint: u32,
    /// `_activationRange`, feet.
    pub range: f32,
    pub physics: Option<Physics>,
    pub solid: Option<Solid>,
    /// Its model's path as its file names it, if it has one: what a clone of
    /// it draws (openspec AH.24).
    pub model: Option<String>,
}

impl Placed {
    /// A placement with nothing but its object, name and script: the
    /// tests' stand-in for one read off a file.
    pub fn named(obj: Obj, name: &str, script: Option<&str>, activity_state: u32) -> Placed {
        Placed {
            obj,
            name: name.to_owned(),
            script: script.map(str::to_owned),
            activity_state,
            id: 0,
            buffer: 0.0,
            waypoints: Vec::new(),
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [100.0; 3],
            tint: 0x8080_80FF,
            range: 0.0,
            physics: None,
            solid: None,
            model: None,
        }
    }
}

/// What the host reads off the files the scripts load with.
#[derive(Debug, Clone, Default)]
pub struct Files {
    /// Every actor placement, file by file, each file's in object order.
    pub placed: Vec<Placed>,
    /// Every placement's `_ID`, sprites and lights included.
    pub ids: HashMap<Obj, i32>,
}

impl Files {
    /// Each `(unit, file)` as the program linked it.
    pub fn read(units: &[(u16, &Igz<'_>)]) -> Result<Self, String> {
        let mut out = Self::default();
        for &(unit, igz) in units {
            for (index, o) in igz.objects().iter().enumerate() {
                if is_a(igz.type_name(o), "AbstractPlacement")
                    && let Some(at) = o.field(ID)
                {
                    let id = igz.i32_at(at, "_ID").map_err(|e| e.to_string())?;
                    out.ids.insert(
                        Obj {
                            unit,
                            index: index as u32,
                        },
                        id,
                    );
                }
            }
            let placements = Placements::read(igz).map_err(|e| e.to_string())?;
            for actor in &placements.actors {
                let p = actor.placement();
                let waypoints = match actor.info.waypoints {
                    Some(l) => ActorWaypointList::read(igz, l)
                        .map_err(|e| e.to_string())?
                        .waypoints
                        .into_iter()
                        .map(|w| Obj {
                            unit,
                            index: w as u32,
                        })
                        .collect(),
                    None => Vec::new(),
                };
                let physics = actor.parameters.map(|ap| {
                    let m = ap.parameters;
                    Physics {
                        ground_max_speed: m.ground_max_speed,
                        ground_acceleration: m.ground_acceleration,
                        ground_deceleration: m.ground_deceleration,
                        ground_turn_rate: m.ground_turn_rate,
                        air_max_speed: m.air_max_speed,
                        air_acceleration: m.air_acceleration,
                        air_deceleration: m.air_deceleration,
                        air_turn_rate: m.air_turn_rate,
                        gravity_max_speed: m.gravity_max_speed,
                        gravity_acceleration: m.gravity_acceleration,
                        jump_max_speed: m.jump_max_speed,
                        jump_acceleration: m.jump_acceleration,
                        jump_initial_speed: m.jump_initial_speed,
                        move_type: m.motion_type as i32,
                        tilts_with_ground: i32::from(m.tilts_with_ground),
                        // The file's 1 is full speed; the member's percent.
                        time_scale: ap.object_time_scale * 100.0,
                    }
                });
                let solid = actor.collision.map(|c| Solid {
                    width: c.width,
                    length: c.length,
                    height: c.height,
                    offset: c.transform.position,
                    weight: c.weight,
                    rebound: c.rebound,
                    interacts_with_actors: ((c.properties >> 3) & 1) as i32,
                    interacts_with_level_mesh: ((c.properties >> 4) & 1) as i32,
                    acts_as_actor_surface: ((c.properties >> 5) & 1) as i32,
                    shape: (c.properties & 7) as i32,
                });
                out.placed.push(Placed {
                    obj: Obj {
                        unit,
                        index: actor.index as u32,
                    },
                    name: actor.name().unwrap_or("").to_owned(),
                    script: actor.script.map(str::to_owned),
                    activity_state: p.placement.activity_state,
                    id: p.placement.id,
                    buffer: p.activation_range_buffer,
                    waypoints,
                    position: p.placement.init_matrix.position,
                    rotation: p.placement.init_matrix.rotation,
                    scale: p.scale,
                    tint: u32::from_be_bytes(p.tint),
                    range: p.activation_range,
                    physics,
                    solid,
                    model: placements.model_path(actor).map(str::to_owned),
                });
            }
        }
        Ok(out)
    }
}

/// An actor made from a file's placement: where and how its file has it.
fn actor_of(p: &Placed) -> Actor {
    let mut a = Actor::blank();
    a.obj = Some(p.obj);
    a.id = p.id;
    a.buffer_loaded = p.buffer;
    a.buffer = p.buffer;
    a.range = p.range;
    a.waypoints = p.waypoints.iter().map(|w| Value::Object(*w)).collect();
    a.position = Vec3::from_array(p.position);
    a.origin = a.position;
    a.angles = Angles {
        tilt: p.rotation[0],
        facing: p.rotation[1],
        roll: p.rotation[2],
    };
    a.scale = p.scale;
    a.tint = p.tint;
    if let Some(ph) = p.physics {
        a.physics = ph;
    }
    if let Some(s) = p.solid {
        a.solid = s;
    }
    a
}

/// What a clone handle is asked for: the template it copies.
pub type Spawner<'a> = dyn FnMut(Value) -> Handle + 'a;

/// The ray caster's two points (`system.ray cast`'s start and end), metres.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ray {
    pub start: Vec3,
    pub end: Vec3,
}

/// The engine state the scripts read and write (the module docs).
#[derive(Resource, Debug)]
pub struct ScriptWorld {
    pub(crate) members: HashMap<MemberId, Member>,
    pub(crate) ids: HashMap<Obj, i32>,
    /// Every actor the scripts reach by handle: the files' placements and the
    /// clones the scripts made. A hero's place, turn and velocity are his
    /// controller's ([`Engine::hero`]).
    pub(crate) actors: HashMap<Handle, Actor>,
    pub(crate) objects: HashMap<Obj, Handle>,
    /// Every file placement's data, to make a clone of it from.
    pub(crate) templates: HashMap<Obj, Placed>,
    pub(crate) controllers: [Handle; 2],
    pub(crate) physical: [Handle; 4],
    pub(crate) portal: Handle,
    /// `viewportinfo.default`: the gameplay camera, as one object.
    pub(crate) viewport: Handle,
    pub(crate) objs: Objects,
    pub(crate) sequences: Vec<Sequence>,
    /// Each template's animation palette: every slot's clip length,
    /// seconds at 100 %, 0 where a slot is empty.
    pub(crate) palettes: HashMap<Obj, Vec<f32>>,
    /// The template whose clone is a hero, and the hero's handle: a spawn
    /// of it answers with him.
    pub(crate) hero_templates: HashMap<Obj, Handle>,
    /// Each actor's attach nodes where they're set, in its own frame
    /// (metres, +Z its facing); a node not set sits at its pivot.
    pub(crate) nodes: HashMap<Handle, Vec<Vec3>>,
    /// The pads as this step reads them, and as the step before did.
    pub(crate) pads: [Pad; 2],
    pub(crate) pads_before: [Pad; 2],
    pub(crate) normalized: f32,
    pub(crate) ray: Ray,
    /// Clones of scripted templates the scripts made this step, with the
    /// template: the embedder adds each as a placement running its script.
    pub scripted: Vec<(Handle, Obj)>,
    time_scale: f32,
    load_level: Value,
    /// The levels the scripts asked for, as each step took them.
    pub requests: Vec<Value>,
}

impl ScriptWorld {
    /// Over `files`, each placement under the handle `handles` gives its
    /// object, and the pads and portal under `objects`' seven handles.
    pub fn new(files: &Files, handles: &HashMap<Obj, Handle>, objects: [Handle; 7]) -> Self {
        let mut actors = HashMap::new();
        let mut templates = HashMap::new();
        for p in &files.placed {
            templates.insert(p.obj, p.clone());
            if let Some(&h) = handles.get(&p.obj) {
                actors.insert(h, actor_of(p));
            }
        }
        let mut objs = Objects::default();
        let viewport = objs.handle(Object::Viewport);
        Self {
            members: Member::table(),
            ids: files.ids.clone(),
            actors,
            objects: handles.iter().map(|(o, h)| (*o, *h)).collect(),
            templates,
            controllers: [objects[0], objects[1]],
            physical: [objects[2], objects[3], objects[4], objects[5]],
            portal: objects[6],
            viewport,
            objs,
            sequences: Vec::new(),
            palettes: HashMap::new(),
            hero_templates: HashMap::new(),
            nodes: HashMap::new(),
            pads: [Pad::default(); 2],
            pads_before: [Pad::default(); 2],
            normalized: 0.0,
            ray: Ray::default(),
            scripted: Vec::new(),
            time_scale: FULL_SPEED,
            load_level: Value::Null,
            requests: Vec::new(),
        }
    }

    /// The end of a step: a level a script asked for is taken, and reads
    /// none from the next step on.
    pub fn end_step(&mut self) {
        if self.load_level != Value::Null {
            self.requests.push(self.load_level);
            info!(
                level = ?self.load_level,
                "a script asked for a level; guacamole loads chapter 1 by its own rule"
            );
            self.load_level = Value::Null;
        }
    }

    /// The pads the next step reads: the step before's become the ones a
    /// press is told from.
    pub fn read_pads(&mut self, pads: [Pad; 2]) {
        self.pads_before = self.pads;
        self.pads = pads;
    }

    /// Each palette slot's clip length for `template`'s clones and itself.
    pub fn set_palette(&mut self, template: Obj, lengths: Vec<f32>) {
        self.palettes.insert(template, lengths);
    }

    /// Where `actor`'s attach nodes sit, in its own frame (metres, +Z its
    /// facing), by index; one past the list, or never set, at its pivot.
    /// Where on the model a node is through a clip isn't read yet: a caller
    /// with a measured point sets it here.
    pub fn set_nodes(&mut self, actor: Handle, nodes: Vec<Vec3>) {
        self.nodes.insert(actor, nodes);
    }

    /// A spawn of `template` makes `hero` the clone: the hero's actor, his
    /// numbers from the template's file.
    pub fn hero_template(&mut self, template: Obj, hero: Handle) {
        self.hero_templates.insert(template, hero);
    }

    /// One frame of `dt` seconds for what the engine runs between the
    /// scripts' ticks: the sequences playing and the actors they set going
    /// moving.
    pub fn advance(&mut self, dt: f32, engine: &mut dyn Engine) {
        for s in &mut self.sequences {
            s.advance(dt);
        }
        self.move_actors(dt, engine);
    }

    /// The actor under `h`, if the scripts reach one.
    pub fn actor(&self, h: Handle) -> Option<&Actor> {
        self.actors.get(&h)
    }

    /// Each file placement's handle, by its object.
    pub fn handles(&self) -> &HashMap<Obj, Handle> {
        &self.objects
    }

    /// A file placement as its file holds it: what a clone of it copies.
    pub fn template(&self, obj: Obj) -> Option<&Placed> {
        self.templates.get(&obj)
    }

    /// The palette slot a started sequence plays (its template's object and
    /// the slot), if it's a clip of one: a sound or anything else isn't.
    pub fn clip_of(&self, s: &Sequence) -> Option<(Obj, u32)> {
        match self.objs.get(self.handle(s.template)?)? {
            Object::Clip { template, slot } => Some((template, slot)),
            _ => None,
        }
    }

    /// The pads the scripts read this step, as [`ScriptWorld::read_pads`]
    /// left them.
    pub fn pads(&self) -> [Pad; 2] {
        self.pads
    }

    /// The sequences started so far, in the order started.
    pub fn sequences(&self) -> &[Sequence] {
        &self.sequences
    }

    /// Whether a remove took the placement or clone under `h` off.
    pub fn removed(&self, h: Handle) -> bool {
        self.actors.get(&h).is_some_and(|k| k.removed)
    }

    /// The handles of the clones the scripts have made, live or taken off.
    pub fn clones(&self) -> Vec<Handle> {
        let mut c: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, k)| k.obj.is_none())
            .map(|(h, _)| *h)
            .collect();
        c.sort_unstable();
        c
    }

    fn get(&mut self, at: &Access, engine: &mut dyn Engine) -> Result<Value, HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        Ok(match m {
            Member::Platform => Value::Int(PS3),
            Member::Configuration => Value::Int(GOLDEN_MASTER),
            Member::GameLanguage | Member::ConsoleLanguage => Value::Int(ENGLISH),
            Member::TimeScale => Value::Float(self.time_scale),
            Member::LoadLevel => self.load_level,
            Member::Controller(n) => Value::Entity(self.controllers[n]),
            Member::Physical(n) => Value::Entity(self.physical[n]),
            Member::Portal => Value::Entity(self.portal),
            Member::Buffer => match on.and_then(|h| self.actors.get(&h)) {
                Some(k) => Value::Float(k.buffer),
                None => return Err(HostFault::Nothing),
            },
            Member::Id => match (on.and_then(|h| self.actors.get(&h)), at.receiver) {
                (Some(k), _) => Value::Int(k.id),
                (None, Some(Value::Object(o))) => match self.ids.get(&o) {
                    Some(&id) => Value::Int(id),
                    None => return Err(HostFault::Nothing),
                },
                _ => return Err(HostFault::Nothing),
            },
            m => return self.get_member(at, m, engine),
        })
    }

    fn elements(&mut self, at: &Access, engine: &mut dyn Engine) -> Result<Vec<Value>, HostFault> {
        match self.members.get(&at.member).copied() {
            Some(m) => self.elements_of(at, m, engine),
            None => Err(HostFault::Unmodelled),
        }
    }

    fn set(&mut self, at: &Access, v: Value, engine: &mut dyn Engine) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        match (m, v) {
            (Member::TimeScale, Value::Float(f)) => self.time_scale = f,
            (Member::LoadLevel, v) => self.load_level = v,
            (Member::Buffer, Value::Float(f)) => match on.and_then(|h| self.actors.get_mut(&h)) {
                Some(k) => k.buffer = f,
                None => return Err(HostFault::Nothing),
            },
            (Member::Id, Value::Int(i)) => match on.and_then(|h| self.actors.get_mut(&h)) {
                Some(k) => k.id = i,
                None => match at.receiver {
                    Some(Value::Object(o)) if self.ids.contains_key(&o) => {
                        self.ids.insert(o, i);
                    }
                    _ => return Err(HostFault::Nothing),
                },
            },
            // Read-only: the write goes nowhere, as the original's member
            // with no setter.
            (
                Member::Platform
                | Member::Configuration
                | Member::GameLanguage
                | Member::ConsoleLanguage
                | Member::Controller(_)
                | Member::Physical(_)
                | Member::Portal
                | Member::Waypoints
                | Member::TimeScale
                | Member::Buffer
                | Member::Id,
                _,
            ) => {}
            (m, v) => return self.set_member(at, m, v, engine),
        }
        Ok(())
    }

    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        match m {
            Member::Buffer => match on.and_then(|h| self.actors.get_mut(&h)) {
                Some(k) => k.buffer = k.buffer_loaded,
                None => return Err(HostFault::Nothing),
            },
            Member::LoadLevel => self.load_level = Value::Null,
            _ => {}
        }
        Ok(())
    }

    fn op(
        &mut self,
        call: &OpCall,
        spawn: &mut Spawner<'_>,
        engine: &mut dyn Engine,
    ) -> Result<Answer, HostFault> {
        let first = call.operands.first().and_then(|(_, v)| *v);
        let target = first.and_then(|v| self.handle(v));
        match call.class {
            Class::Control => Ok(match (first, target) {
                (_, Some(h)) if self.actors.get(&h).is_some_and(|k| k.removed) => Answer::Skip,
                (Some(v), Some(_)) => Answer::Run {
                    receiver: Some(v),
                    result: None,
                },
                // An object no placement stands for: the pads, the portal.
                (Some(v @ Value::Entity(_)), None) => Answer::Run {
                    receiver: Some(v),
                    result: None,
                },
                _ => Answer::Skip,
            }),
            Class::Spawn => {
                let Some(template) = first else {
                    return Ok(Answer::Skip);
                };
                let named = |name: &str| {
                    call.operands
                        .iter()
                        .find(|(n, _)| *n == name)
                        .and_then(|(_, v)| *v)
                };
                let at = named("_RHS");
                let facing = named("_facingRHS").and_then(|v| v.as_f32());
                Ok(self.spawn(template, at, facing, spawn, engine))
            }
            Class::Remove => {
                if let Some(k) = target.and_then(|h| self.actors.get_mut(&h)) {
                    k.removed = true;
                }
                Ok(Answer::Done)
            }
            Class::Reset => {
                if let Some(k) = target.and_then(|h| self.actors.get_mut(&h)) {
                    k.buffer = k.buffer_loaded;
                }
                Ok(Answer::Done)
            }
            _ => match self.op_hero(call, spawn, engine) {
                Some(r) => r,
                None => Err(HostFault::Unmodelled),
            },
        }
    }

    /// A clone of `template` at the place `at` names (its pivot), facing
    /// `facing` when given, else the template's: the hero for a template a
    /// hero stands for, else a new actor (its handle from `spawn`) with the
    /// template's file numbers, or a clone's own when it copies a clone. A
    /// scripted template's clone is listed for the embedder to run.
    fn spawn(
        &mut self,
        template: Value,
        at: Option<Value>,
        facing: Option<f32>,
        spawn: &mut Spawner<'_>,
        engine: &mut dyn Engine,
    ) -> Answer {
        let source = match template {
            Value::Object(o) => Some(o),
            v => self
                .handle(v)
                .and_then(|h| self.actors.get(&h).and_then(|a| a.template.or(a.obj))),
        };
        let place = at.and_then(|v| self.point(v, engine));
        let hero = source.and_then(|t| self.hero_templates.get(&t).copied());
        let h = match hero {
            Some(h) => h,
            None => spawn(template),
        };
        let copied = self
            .handle(template)
            .and_then(|t| self.actors.get(&t))
            .filter(|a| a.obj.is_none())
            .cloned();
        let mut a = match (copied, source.and_then(|t| self.templates.get(&t))) {
            (Some(c), _) => c,
            (None, Some(p)) => actor_of(p),
            (None, None) => Actor::blank(),
        };
        a.obj = None;
        a.template = source;
        a.parent = template;
        a.waypoints = Vec::new();
        a.removed = false;
        a.clones = Vec::new();
        a.attach = Value::Null;
        a.velocity = Vec3::ZERO;
        a.launched = false;
        a.contacts = Vec::new();
        a.remembered = None;
        if a.id == 0
            && let Some(t) = source
        {
            a.id = self.ids.get(&t).copied().unwrap_or(0);
        }
        if let Some(p) = place {
            a.position = p;
        }
        if let Some(f) = facing {
            a.angles.facing = f;
        }
        a.origin = a.position;
        self.actors.insert(h, a);
        if let Some(hero) = engine.hero(h) {
            if let Some(p) = place {
                hero.pivot = p;
            }
            if let Some(f) = facing {
                hero.facing = f.rem_euclid(360.0);
            }
        }
        if let Some(t) = self.handle(template)
            && let Some(ta) = self.actors.get_mut(&t)
        {
            ta.clones.push(h);
        }
        if let Some(t) = source
            && self.templates.get(&t).is_some_and(|p| p.script.is_some())
        {
            self.scripted.push((h, t));
        }
        let e = Value::Entity(h);
        Answer::Run {
            receiver: Some(e),
            result: Some(e),
        }
    }
}

/// [`Host`] over a [`ScriptWorld`], the [`Engine`] the TV's entities own,
/// the session's generator and a spawner.
pub struct WorldHost<'a, 's> {
    pub world: &'a mut ScriptWorld,
    pub engine: &'a mut dyn Engine,
    pub generator: &'a mut Generator,
    pub spawn: &'a mut Spawner<'s>,
}

impl Host for WorldHost<'_, '_> {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.world.get(at, self.engine)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.world.set(at, value, self.engine)
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.world.reset(at)
    }
    fn draw(&mut self) -> u32 {
        self.generator.next_u32()
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        self.world.op(call, self.spawn, self.engine)
    }
    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        self.world.elements(at, self.engine)
    }
}

/// `global.bld`'s running placements' scripts: their first tick only.
pub const GLOBAL_FIRST_TICKS: [&str; 17] = [
    "/UI.ai",
    "Spyro_PortalManager.ai",
    "PortalControl.ai",
    "PauseController.ai",
    "TRG_Checks_Manager.ai",
    "Macros_Animation.ai",
    "GameSystems.ai",
    "MusicController.ai",
    "movementUpdate.ai",
    "SoundController.ai",
    "PS3SpecificSetup.ai",
    "360PS3GestureManager.ai",
    "Macros_Achievements.ai",
    "Minion_Tuning.ai",
    "360_PS3_pointer_setup.ai",
    "PointLightManager.ai",
    "/PortalManager.ai",
];

/// `init_setup.bld`'s running placements' scripts, and level 213's
/// definition and enemy system: every tick.
pub const EVERY_TICK: [&str; 8] = [
    "Init_Setup_Plat_360.ai",
    "Init_Setup_Plat_PS3.ai",
    "Init_Setup_Plat_Wii.ai",
    "Init_Setup_Plat_WiiU.ai",
    "/Init_Setup.ai",
    "InitSetup_Achievements.ai",
    "Level_Definition_213.ai",
    "_EnemySystem_Functions.ai",
];

/// The VM's config for AH.16's run: the init chain's members and listing.
pub fn config(mode: Mode) -> Config {
    let mut c = Config::new(mode);
    members::register(&mut c.members, INIT_CHAIN);
    c.whitelist = Whitelist::with_first_ticks(&[], &EVERY_TICK, &GLOBAL_FIRST_TICKS);
    c
}

/// `global.bld`'s placements AH.17 ticks every step: the UI's (its tick
/// advances the combat timer every mover's update reads) and the movement
/// update's (each tick moving the scripted movers).
pub const GLOBAL_EVERY_TICK: [&str; 2] = ["/UI.ai", "movementUpdate.ai"];

/// Eruptor's scripts (openspec AH.17): his definition and tuning actor, his
/// own script on his clone, the eruption's and the magma ball's, every
/// tick. His accolade and portal-entry scripts are macros only.
pub const ERUPTOR_SCRIPTS: [&str; 5] = [
    "009_Def_Eruptor.ai",
    "009_Eruptor_Tuning.ai",
    "/009_Eruptor.ai",
    "009_LavaAttack.ai",
    "009_MagmaBall.ai",
];

/// The VM's config for AH.17's run: AH.16's members and Eruptor's, the init
/// chain's listing with `global.bld`'s UI and movement update every tick and
/// Eruptor's scripts beside them.
pub fn hero_config(mode: Mode) -> Config {
    let mut c = Config::new(mode);
    members::register(&mut c.members, INIT_CHAIN);
    members::register(&mut c.members, members::HERO);
    let first: Vec<&str> = GLOBAL_FIRST_TICKS
        .iter()
        .copied()
        .filter(|s| !GLOBAL_EVERY_TICK.contains(s))
        .collect();
    let every: Vec<&str> = EVERY_TICK
        .iter()
        .chain(&GLOBAL_EVERY_TICK)
        .chain(&ERUPTOR_SCRIPTS)
        .copied()
        .collect();
    c.whitelist = Whitelist::with_first_ticks(&[], &every, &first);
    c
}

/// The VM over `program` with every running placement of `files` added in
/// file order (`global.bld`'s, then `init_setup.bld`'s, then the level's,
/// by `order`'s units), each under the handle `handles` gives its object;
/// every placement's object is named, scripted or not. A running
/// placement whose script the program lacks is left out, logged.
pub fn init_chain(
    program: Arc<Program>,
    files: &Files,
    handles: &HashMap<Obj, Handle>,
    order: &[u16],
    config: Config,
) -> Vm {
    let mut vm = Vm::new(program.clone(), config);
    for p in &files.placed {
        if let Some(&h) = handles.get(&p.obj) {
            vm.name_object(p.obj, h);
        }
    }
    let scripts: HashMap<(u16, &str), u32> = program
        .scripts
        .iter()
        .enumerate()
        .map(|(i, s)| ((s.unit, s.name.as_str()), i as u32))
        .collect();
    for &unit in order {
        for p in files.placed.iter().filter(|p| p.obj.unit == unit) {
            let (Some(script), RUNNING) = (&p.script, p.activity_state) else {
                continue;
            };
            let Some(&h) = handles.get(&p.obj) else {
                continue;
            };
            match scripts.get(&(unit, script.as_str())) {
                Some(&sid) => {
                    vm.add_at(h, &p.name, sid, p.obj);
                }
                None => warn!(placement = %p.name, %script, "no such script in the part"),
            }
        }
    }
    vm
}

/// The portal manager, whose macros the join's stand-in calls as its own
/// tick would ([`join_request`], [`join_activate`]).
pub const PORTAL_MANAGER: &str = "Spyro_PortalManager";

/// The join's stand-in, its first half (openspec AH.17; decision 2's table,
/// The join): what the portal and the portal manager do when a figure goes
/// on the portal, by the original's own macros with only their scheduling
/// ours. The portal manager asks for the figure's character file through
/// `Portal - Load Portal Object Stream` with the first player's slot's
/// loaded data (slot 0 of `portal control data.$loaded objects`), as the
/// portal control's tag handling does; then the character file's running
/// placements (its definition, its tuning actor) join the VM, listed for
/// every tick, under `handles`: the definition's first tick registers the
/// minion into that loaded data (`Portal - Define Minion`). The portal's tag,
/// the level streaming and the entry beam aren't run (AJ's, AM's).
pub fn join_request(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    files: &Files,
    unit: u16,
    handles: &HashMap<Obj, Handle>,
) -> Result<Vec<PlacementId>, Halt> {
    let pm = portal_manager(vm, host.world)?;
    let loaded = slot_zero(vm)?;
    vm.call_with(
        Some(pm),
        "Portal - Load Portal Object Stream",
        &[("use loaded data", loaded)],
        &[],
        host,
    )?;
    let program = vm.program_arc();
    let scripts: HashMap<(u16, &str), u32> = program
        .scripts
        .iter()
        .enumerate()
        .map(|(i, s)| ((s.unit, s.name.as_str()), i as u32))
        .collect();
    let mut added = Vec::new();
    for p in files.placed.iter().filter(|p| p.obj.unit == unit) {
        let (Some(script), RUNNING) = (&p.script, p.activity_state) else {
            continue;
        };
        let (Some(&h), Some(&sid)) = (handles.get(&p.obj), scripts.get(&(unit, script.as_str())))
        else {
            continue;
        };
        added.push(vm.add_at(h, &p.name, sid, p.obj));
    }
    Ok(added)
}

/// The join's second half, after the definition's first tick: the minion
/// its definition registered becomes `hero` (a spawn of its template
/// answers with him), and `Game - Activate/Deactivate Minion` runs as the
/// portal manager's entry beam calls it (`Portal - Entry Activate Minion`):
/// the minion, spawned where `hero` stands, for the first player's
/// controller, the definition's minion description, no health restored, in
/// a level transition. The minion's own script then joins the VM under
/// `hero` (its template's, listed for every tick). Returns its placement.
pub fn join_activate(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    hero: Handle,
) -> Result<PlacementId, Halt> {
    let pm = portal_manager(vm, host.world)?;
    let loaded = slot_zero(vm)?;
    let info = vm.field(loaded, "object info").unwrap_or(Value::Null);
    let minion = vm.field(info, "spawned object").unwrap_or(Value::Null);
    let desc = vm
        .field(info, "minion additional info")
        .unwrap_or(Value::Null);
    let Some(template) = host.world.template_obj(minion) else {
        return Err(outside(format!(
            "the definition registered no minion template ({minion:?})"
        )));
    };
    host.world.hero_template(template, hero);
    let player = Value::Entity(host.world.controllers[0]);
    vm.call_with(
        Some(pm),
        "Game - Activate/Deactivate Minion",
        &[
            ("minion", minion),
            ("spawn location", Value::Entity(hero)),
            ("player", player),
            ("keep minion ref in player set", Value::Int(0)),
            ("input minion data", desc),
            ("restore hp", Value::Int(0)),
            ("in level transition", Value::Int(1)),
        ],
        &[],
        host,
    )?;
    let scripted = std::mem::take(&mut host.world.scripted);
    let mut minion_placement = None;
    let program = vm.program_arc();
    for (h, t) in scripted {
        let Some(sid) = host.world.script_of(t, &program) else {
            continue;
        };
        let name = host.world.name_of(t).unwrap_or_default();
        let id = vm.add(h, &name, sid);
        if h == hero {
            minion_placement = Some(id);
        }
    }
    minion_placement.ok_or_else(|| outside("the activation spawned no minion".to_owned()))
}

/// Each clone of a scripted template the last step made, added to the VM
/// running its template's script: what the original's scheduler ticks from
/// the next frame (a hypothesis: where in a frame a new clone first ticks
/// isn't read).
pub fn add_scripted(vm: &mut Vm, world: &mut ScriptWorld) -> Vec<PlacementId> {
    let program = vm.program_arc();
    std::mem::take(&mut world.scripted)
        .into_iter()
        .filter_map(|(h, t)| {
            let sid = world.script_of(t, &program)?;
            let name = world.name_of(t).unwrap_or_default();
            Some(vm.add(h, &name, sid))
        })
        .collect()
}

/// A slice of the player loop's native rule (AH.20's, decision 2's table):
/// the portal manager's tick calls `Manage Projectiles` with its projectile
/// control (the global `projectile control data` its STARTUP set), so what
/// `Projectile - Fire Projectile` threw runs its midair logic, its hits and
/// its detonation. Called once a step before the placements' ticks, where
/// the portal manager's tick runs ahead of a joined hero's scripts.
pub fn manage_projectiles(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<(), Halt> {
    let pm = portal_manager(vm, host.world)?;
    let g = vm
        .program()
        .global_named("projectile control data")
        .ok_or_else(|| outside("no projectile control data".to_owned()))?;
    let control = vm
        .global(g)
        .ok_or_else(|| outside("projectile control data reads nothing".to_owned()))?;
    vm.call_with(
        Some(pm),
        "Manage Projectiles",
        &[("control data", control)],
        &[],
        host,
    )?;
    Ok(())
}

fn outside(why: String) -> Halt {
    Halt {
        placement: None,
        placement_name: "(the join)".to_owned(),
        script: String::new(),
        position: 0,
        op: None,
        reason: guac_script::Reason::Unsupported(why),
    }
}

fn portal_manager(vm: &Vm, world: &ScriptWorld) -> Result<PlacementId, Halt> {
    let h = world
        .templates
        .values()
        .find(|p| p.name == PORTAL_MANAGER)
        .and_then(|p| world.objects.get(&p.obj).copied())
        .ok_or_else(|| outside(format!("no {PORTAL_MANAGER} placement")))?;
    vm.placement_of(h)
        .ok_or_else(|| outside(format!("{PORTAL_MANAGER} doesn't run")))
}

/// The first player's slot's loaded data.
fn slot_zero(vm: &mut Vm) -> Result<Value, Halt> {
    let g = vm
        .program()
        .global_named("portal control data")
        .ok_or_else(|| outside("no portal control data".to_owned()))?;
    let pcd = vm
        .global(g)
        .ok_or_else(|| outside("portal control data reads nothing".to_owned()))?;
    vm.field(pcd, "loaded objects")
        .and_then(|s| vm.set(s).and_then(|e| e.first().copied()))
        .ok_or_else(|| outside("the portal has no loaded objects yet".to_owned()))
}

impl ScriptWorld {
    /// The file object a value names as a template.
    pub fn template_obj(&self, v: Value) -> Option<Obj> {
        match v {
            Value::Object(o) => Some(o),
            v => {
                let h = self.handle(v)?;
                let a = self.actors.get(&h)?;
                a.template.or(a.obj)
            }
        }
    }

    /// The script a template runs, in `program`.
    pub fn script_of(&self, t: Obj, program: &Program) -> Option<u32> {
        let script = self.templates.get(&t)?.script.as_ref()?;
        program
            .scripts
            .iter()
            .position(|s| s.unit == t.unit && &s.name == script)
            .map(|i| i as u32)
    }

    /// A template's placement name.
    pub fn name_of(&self, t: Obj) -> Option<String> {
        self.templates.get(&t).map(|p| p.name.clone())
    }
}

/// The VM steps in the TV.
#[derive(Resource)]
pub struct ScriptRuntime {
    pub vm: Vm,
    pub generator: Generator,
    /// Every step so far and the scripts play mode stopped.
    pub steps: u64,
    pub stopped: Vec<Halt>,
}

/// A placement the scripts load, as an entity: its object in its file.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPlacement(pub Obj);

/// A clone a script made, as an entity: the template it copies.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ScriptClone(pub Value);

/// An object the scripts name through a member: a pad or the portal.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptObject(pub &'static str);

/// [`Host`] inside the TV (decision 21): the world, the generator and
/// [`Commands`] for clones, which are entities whose handles the scripts
/// have at once.
#[derive(SystemParam)]
pub struct ScriptHost<'w, 's> {
    world: ResMut<'w, ScriptWorld>,
    commands: Commands<'w, 's>,
}

/// Steps the scripts once a fixed step, in play mode.
pub fn step_scripts(mut runtime: ResMut<ScriptRuntime>, mut host: ScriptHost) {
    let ScriptRuntime { vm, generator, .. } = &mut *runtime;
    let commands = &mut host.commands;
    let mut spawn =
        |template: Value| -> Handle { commands.spawn(ScriptClone(template)).id().to_bits() };
    let mut engine = NoEngine;
    let mut world_host = WorldHost {
        world: &mut host.world,
        engine: &mut engine,
        generator,
        spawn: &mut spawn,
    };
    let report = vm.step(&mut world_host);
    host.world.end_step();
    runtime.steps += 1;
    match report {
        Ok(r) => {
            for h in &r.stopped {
                warn!(%h, "a script stopped");
            }
            runtime.stopped.extend(r.stopped);
        }
        Err(h) => {
            warn!(%h, "a script stopped");
            runtime.stopped.push(h);
        }
    }
}

/// Spawns an entity for every placement of `files` and the pads and the
/// portal, and makes the run: what the TV does once a level's script part
/// links, a run from an earlier link and its entities gone first. The
/// generator is the session's, seeded from the clock. AH.16's listing;
/// [`install_with`] takes another.
pub fn install(world: &mut World, program: Arc<Program>, files: &Files, order: &[u16]) {
    install_with(world, program, files, order, config(Mode::Play));
}

/// [`install`] under `config`: the TV's is [`hero_config`]'s (openspec
/// AH.24), the init chain's every tick with the UI's, the movement update's
/// and a fighter's scripts beside them.
pub fn install_with(
    world: &mut World,
    program: Arc<Program>,
    files: &Files,
    order: &[u16],
    config: Config,
) {
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<ScriptPlacement>, With<ScriptClone>, With<ScriptObject>)>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    let mut handles = HashMap::new();
    for p in &files.placed {
        let e = world.spawn((ScriptPlacement(p.obj), Name::new(p.name.clone())));
        handles.insert(p.obj, e.id().to_bits());
    }
    let names = [
        "controller 1",
        "controller 2",
        "physical controller 1",
        "physical controller 2",
        "physical controller 3",
        "physical controller 4",
        "portal",
    ];
    let objects = names.map(|n| world.spawn(ScriptObject(n)).id().to_bits());
    let vm = init_chain(program, files, &handles, order, config);
    world.insert_resource(ScriptWorld::new(files, &handles, objects));
    world.insert_resource(ScriptRuntime {
        vm,
        generator: Generator::seeded(guac_script::session_seed()),
        steps: 0,
        stopped: Vec::new(),
    });
}

/// The scripts' system in `FixedUpdate`: nothing runs until a level's
/// script part is installed.
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            step_scripts
                .run_if(resource_exists::<ScriptRuntime>)
                .run_if(resource_exists::<ScriptWorld>),
        );
    }
}

/// The pads' and the portal's handles in a headless run: past every
/// placement's.
pub fn headless_objects(handles: &HashMap<Obj, Handle>) -> [Handle; 7] {
    let base = handles.values().copied().max().unwrap_or(0) + 1;
    std::array::from_fn(|i| base + i as u64)
}

/// Every placement's handle in a headless run: its unit and index.
pub fn headless_handles(files: &Files) -> HashMap<Obj, Handle> {
    files
        .placed
        .iter()
        .map(|p| {
            (
                p.obj,
                (u64::from(p.obj.unit) << 32) | u64::from(p.obj.index),
            )
        })
        .collect()
}

/// A headless clone handle maker: counting up from past every other.
pub fn headless_spawner(start: Handle) -> impl FnMut(Value) -> Handle {
    let mut next = start;
    move |_| {
        next += 1;
        next
    }
}

#[cfg(test)]
mod tests {
    use guac_script::members::{find, label};

    use super::*;

    const GRUNT: Obj = Obj { unit: 4, index: 7 };
    const SPRITE: Obj = Obj { unit: 4, index: 9 };
    const NODE: Obj = Obj { unit: 4, index: 8 };

    /// One actor placement with a waypoint, a sprite that's only an `_ID`,
    /// and the world over them.
    fn world() -> (ScriptWorld, Handle, [Handle; 7]) {
        let files = Files {
            placed: vec![Placed {
                id: 41,
                buffer: 12.5,
                waypoints: vec![NODE],
                ..Placed::named(GRUNT, "Grunt", Some("C:/x/Grunt.ai"), RUNNING)
            }],
            ids: HashMap::from([(GRUNT, 41), (SPRITE, 77)]),
        };
        let handles = headless_handles(&files);
        let objects = headless_objects(&handles);
        let grunt = handles[&GRUNT];
        (ScriptWorld::new(&files, &handles, objects), grunt, objects)
    }

    fn at(namespace: &str, name: &str, receiver: Option<Value>) -> Access {
        Access {
            placement: 1,
            receiver,
            member: MemberId::named(namespace, name),
            other_point: None,
        }
    }

    fn call(class: Class, first: Option<Value>) -> OpCall {
        OpCall {
            placement: 1,
            receiver: Value::Entity(1),
            class,
            operands: vec![("_LHS", first)],
            words: Vec::new(),
        }
    }

    #[test]
    fn the_console_is_a_retail_english_ps3_by_the_members_labels() {
        let label_of =
            |ns: &str, name: &str, l: &str| label(&find(INIT_CHAIN, ns, name).unwrap(), l).unwrap();
        assert_eq!(
            Value::Int(PS3),
            label_of("system", "hardware platform", "PS 3")
        );
        assert_eq!(
            Value::Int(GOLDEN_MASTER),
            label_of("system", "game configuration", "Golden Master")
        );
        assert_eq!(
            Value::Int(ENGLISH),
            label_of("system", "game language", "English")
        );
        let (mut w, ..) = world();
        assert_eq!(
            w.get(&at("system", "hardware platform", None), &mut NoEngine),
            Ok(Value::Int(PS3))
        );
        assert_eq!(
            w.get(&at("system", "game configuration", None), &mut NoEngine),
            Ok(Value::Int(GOLDEN_MASTER))
        );
        for name in ["game language", "console language"] {
            assert_eq!(
                w.get(&at("system", name, None), &mut NoEngine),
                Ok(Value::Int(ENGLISH))
            );
        }
        // Read-only: a write goes nowhere.
        w.set(
            &at("system", "hardware platform", None),
            Value::Int(3),
            &mut NoEngine,
        )
        .unwrap();
        assert_eq!(
            w.get(&at("system", "hardware platform", None), &mut NoEngine),
            Ok(Value::Int(PS3))
        );
        assert_eq!(
            w.get(&at("system", "pause state", None), &mut NoEngine),
            Err(HostFault::Unmodelled)
        );
    }

    #[test]
    fn the_time_scale_keeps_a_write_and_a_level_asked_for_is_taken_at_the_steps_end() {
        let (mut w, ..) = world();
        let scale = at("system", "game time scale", None);
        assert_eq!(w.get(&scale, &mut NoEngine), Ok(Value::Float(FULL_SPEED)));
        w.set(&scale, Value::Float(50.0), &mut NoEngine).unwrap();
        assert_eq!(w.get(&scale, &mut NoEngine), Ok(Value::Float(50.0)));
        let level = at("system", "load level", None);
        assert_eq!(w.get(&level, &mut NoEngine), Ok(Value::Null));
        w.set(&level, Value::Object(SPRITE), &mut NoEngine).unwrap();
        assert_eq!(w.get(&level, &mut NoEngine), Ok(Value::Object(SPRITE)));
        w.end_step();
        assert_eq!(w.requests, vec![Value::Object(SPRITE)]);
        assert_eq!(w.get(&level, &mut NoEngine), Ok(Value::Null));
        w.end_step();
        assert_eq!(w.requests.len(), 1, "taken once");
    }

    #[test]
    fn a_placements_buffer_id_and_waypoints_are_its_files_by_handle_or_object() {
        let (mut w, grunt, _) = world();
        let buffer = |r| at("placement", "deactivation buffer", Some(r));
        for r in [Value::Entity(grunt), Value::Object(GRUNT)] {
            assert_eq!(w.get(&buffer(r), &mut NoEngine), Ok(Value::Float(12.5)));
            assert_eq!(
                w.get(&at("abstractplacement", "id", Some(r)), &mut NoEngine),
                Ok(Value::Int(41))
            );
        }
        w.set(
            &buffer(Value::Entity(grunt)),
            Value::Float(3.0),
            &mut NoEngine,
        )
        .unwrap();
        assert_eq!(
            w.get(&buffer(Value::Object(GRUNT)), &mut NoEngine),
            Ok(Value::Float(3.0))
        );
        w.reset(&buffer(Value::Entity(grunt))).unwrap();
        assert_eq!(
            w.get(&buffer(Value::Entity(grunt)), &mut NoEngine),
            Ok(Value::Float(12.5))
        );
        w.set(
            &buffer(Value::Entity(grunt)),
            Value::Float(3.0),
            &mut NoEngine,
        )
        .unwrap();
        assert_eq!(
            w.op(
                &call(Class::Reset, Some(Value::Entity(grunt))),
                &mut |_| 0,
                &mut NoEngine
            ),
            Ok(Answer::Done)
        );
        assert_eq!(
            w.get(&buffer(Value::Entity(grunt)), &mut NoEngine),
            Ok(Value::Float(12.5))
        );
        // A sprite has only its `_ID`; nothing else is a placement.
        let sprite_id = at("spritematrixmeasurement", "id", Some(Value::Object(SPRITE)));
        assert_eq!(w.get(&sprite_id, &mut NoEngine), Ok(Value::Int(77)));
        assert_eq!(
            w.get(&buffer(Value::Object(SPRITE)), &mut NoEngine),
            Err(HostFault::Nothing)
        );
        assert_eq!(
            w.get(
                &at("abstractplacement", "id", Some(Value::Int(3))),
                &mut NoEngine
            ),
            Err(HostFault::Nothing)
        );
        let waypoints = at("actorinfo", "waypoints", Some(Value::Entity(grunt)));
        assert_eq!(
            w.elements(&waypoints, &mut NoEngine),
            Ok(vec![Value::Object(NODE)])
        );
        assert_eq!(w.get(&waypoints, &mut NoEngine), Err(HostFault::Nothing));
    }

    #[test]
    fn the_pads_and_the_portal_are_one_object_each() {
        let (mut w, _, objects) = world();
        let mut got = Vec::new();
        for (ns, name) in [
            ("scriptcontroller", "controller 1"),
            ("scriptcontroller", "controller 2"),
            ("scriptcontroller", "physical controller 1"),
            ("scriptcontroller", "physical controller 2"),
            ("scriptcontroller", "physical controller 3"),
            ("scriptcontroller", "physical controller 4"),
            ("redoctaneperipheralsystemobject", "portal"),
        ] {
            got.push(w.get(&at(ns, name, None), &mut NoEngine).unwrap());
        }
        assert_eq!(got, objects.map(Value::Entity));
        // A tell on one runs its block with it.
        let pad = Value::Entity(objects[0]);
        assert_eq!(
            w.op(&call(Class::Control, Some(pad)), &mut |_| 0, &mut NoEngine),
            Ok(Answer::Run {
                receiver: Some(pad),
                result: None
            })
        );
    }

    #[test]
    fn a_clone_copies_its_template_and_a_remove_takes_it_off() {
        let (mut w, grunt, _) = world();
        let mut made = Vec::new();
        let mut spawn = |t: Value| {
            made.push(t);
            900
        };
        let clone = Value::Entity(900);
        assert_eq!(
            w.op(
                &call(Class::Spawn, Some(Value::Object(GRUNT))),
                &mut spawn,
                &mut NoEngine
            ),
            Ok(Answer::Run {
                receiver: Some(clone),
                result: Some(clone)
            })
        );
        assert_eq!(made, vec![Value::Object(GRUNT)]);
        assert_eq!(w.clones(), vec![900]);
        assert_eq!(
            w.get(&at("abstractplacement", "id", Some(clone)), &mut NoEngine),
            Ok(Value::Int(41))
        );
        assert_eq!(
            w.get(
                &at("placement", "deactivation buffer", Some(clone)),
                &mut NoEngine
            ),
            Ok(Value::Float(12.5))
        );
        let tell = call(Class::Control, Some(clone));
        assert!(matches!(
            w.op(&tell, &mut |_| 0, &mut NoEngine),
            Ok(Answer::Run { .. })
        ));
        assert_eq!(
            w.op(&call(Class::Remove, Some(clone)), &mut |_| 0, &mut NoEngine),
            Ok(Answer::Done)
        );
        assert!(w.removed(900));
        assert!(!w.removed(grunt));
        assert_eq!(w.op(&tell, &mut |_| 0, &mut NoEngine), Ok(Answer::Skip));
        assert_eq!(
            w.op(&call(Class::Control, None), &mut |_| 0, &mut NoEngine),
            Ok(Answer::Skip)
        );
        assert_eq!(
            w.op(
                &call(Class::StopSequence, Some(clone)),
                &mut |_| 0,
                &mut NoEngine
            ),
            Ok(Answer::Done)
        );
        // A sequence started on the placement plays (none of a clip the
        // palette lacks: nothing to play, done at once).
        let started = w.op(
            &call(Class::StartSequence, Some(clone)),
            &mut |_| 0,
            &mut NoEngine,
        );
        assert!(
            matches!(
                started,
                Ok(Answer::Run {
                    receiver: None,
                    result: Some(_)
                })
            ),
            "{started:?}"
        );
        assert_eq!(w.sequences().len(), 1);
        assert_eq!(
            w.op(&call(Class::Displace, None), &mut |_| 0, &mut NoEngine),
            Err(HostFault::Nothing)
        );
        assert_eq!(
            w.op(&call(Class::CheckFov, None), &mut |_| 0, &mut NoEngine),
            Err(HostFault::Unmodelled)
        );
    }

    #[test]
    fn nothing_steps_until_a_part_is_installed_and_a_new_part_replaces_the_old() {
        let mut app = App::new();
        app.add_plugins(ScriptPlugin);
        app.world_mut().run_schedule(FixedUpdate);
        assert!(app.world().get_resource::<ScriptRuntime>().is_none());
        let files = Files {
            placed: vec![Placed {
                id: 41,
                buffer: 12.5,
                ..Placed::named(GRUNT, "Grunt", None, RUNNING)
            }],
            ids: HashMap::new(),
        };
        let objects = |app: &mut App| {
            let w = app.world_mut();
            (
                w.query::<&ScriptPlacement>().iter(w).count(),
                w.query::<&ScriptObject>().iter(w).count(),
            )
        };
        for _ in 0..2 {
            scripts_installed(&mut app, &files);
            assert_eq!(objects(&mut app), (1, 7));
        }
        for _ in 0..3 {
            app.world_mut().run_schedule(FixedUpdate);
        }
        let runtime = app.world().resource::<ScriptRuntime>();
        assert_eq!(runtime.steps, 3);
        assert!(runtime.stopped.is_empty());
    }

    fn scripts_installed(app: &mut App, files: &Files) {
        install(app.world_mut(), Arc::new(Program::default()), files, &[4]);
    }

    #[test]
    fn the_config_registers_the_init_chains_members_and_listing() {
        let c = config(Mode::Play);
        for r in INIT_CHAIN {
            let id = MemberId::named(r.namespace, r.name);
            let registered = c.members.get(id).is_some();
            assert_eq!(registered, r.keeper != members::Keeper::Vm, "{}", r.path());
        }
        assert!(
            c.whitelist
                .lists("_Level_Definition", "C:/x/Level_Definition_213.ai")
        );
        assert!(c.whitelist.lists("Macros_UI", "C:/x/UI.ai"));
        assert!(!c.whitelist.lists("Spinner", "C:/x/Spinner.ai"));
    }
}
