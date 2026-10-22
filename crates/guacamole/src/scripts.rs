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
use guac_formats::igz::layouts::{ActorWaypoint, ActorWaypointList, Layout, is_a};
use guac_formats::level::Placements;
use guac_formats::script::Class;
use guac_script::members::{self, INIT_CHAIN};
use guac_script::{
    Access, Answer, Config, Generator, Halt, Handle, Host, HostFault, MemberId, Mode, Obj, OpCall,
    PlacementId, Program, ScriptId, Value, Vm, Whitelist,
};
use tracing::{info, warn};

use crate::frame::Frame;
use crate::player_loop::PlayerLoop;
use crate::script_engine::{
    Actor, Angles, Engine, NoEngine, NodePose, Object, Objects, Pad, Physics, Sequence, Solid,
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
    /// Every waypoint's place in the level, metres (openspec AH.18: what a
    /// `move to` along a path walks to and a measurement is taken from).
    pub points: HashMap<Obj, Vec3>,
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
            for (index, o) in igz.objects().iter().enumerate() {
                if is_a(igz.type_name(o), ActorWaypoint::TYPE) {
                    let w = ActorWaypoint::read(igz, index).map_err(|e| e.to_string())?;
                    out.points.insert(
                        Obj {
                            unit,
                            index: index as u32,
                        },
                        Vec3::from_array(w.init_matrix.position),
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

/// Whether a managed placement is awake (openspec AH.18, AG.7's rule with
/// the buffer read as the original's hysteresis): one with an activation
/// range is awake while a hero is within it (3D, feet, from his pivot) and
/// stays awake out to the range plus its deactivation buffer; one with no
/// range is awake when its buffer is 0 too and asleep otherwise, its
/// scripts' to wake (a script giving it a range or a buffer of 0). `hero`
/// is the nearest hero's distance in feet, `None` with none.
pub fn awake(range: f32, buffer: f32, was: bool, hero: Option<f32>) -> bool {
    if range <= 0.0 {
        return buffer <= 0.0;
    }
    match hero {
        Some(d) => d <= range || (was && d <= range + buffer),
        None => false,
    }
}

/// An actor made from a file's placement: where and how its file has it.
fn actor_of(p: &Placed) -> Actor {
    let mut a = Actor::blank();
    a.managed = p.activity_state == RUNNING;
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
    /// Every waypoint's place, metres.
    pub(crate) points: HashMap<Obj, Vec3>,
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
    pub(crate) nodes: HashMap<Handle, Vec<NodePose>>,
    /// The pads as this step reads them, and as the step before did.
    pub(crate) pads: [Pad; 2],
    pub(crate) pads_before: [Pad; 2],
    pub(crate) normalized: f32,
    pub(crate) ray: Ray,
    /// The points references naming nothing keep, by the reference's cell.
    pub(crate) kept_points: HashMap<u32, Vec3>,
    /// The waypoints scripts made ([`Object::Waypoint`]), each where it was
    /// last put, none until it's put somewhere.
    pub(crate) waypoints_made: Vec<Option<Vec3>>,
    /// Clones of scripted templates the scripts made this step, with the
    /// template: the embedder adds each as a placement running its script.
    pub scripted: Vec<(Handle, Obj)>,
    time_scale: f32,
    load_level: Value,
    /// The levels the scripts asked for, as each step took them.
    pub requests: Vec<Value>,
    /// The placements the VM leaves inert, off the whitelist (design
    /// decision 6): a script waking one is logged and listed in `woken`.
    pub(crate) inert: std::collections::HashSet<Handle>,
    /// Each placement off the whitelist a whitelisted script woke, in the
    /// order woken: what it was, and whose script woke it.
    pub woken: Vec<Woken>,
}

/// A placement off the whitelist a whitelisted script woke (design decision
/// 6): it stays as it was, inert, logged and listed here.
#[derive(Debug, Clone, PartialEq)]
pub struct Woken {
    pub placement: Handle,
    pub name: String,
    /// The placement whose script woke it.
    pub by: String,
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
            points: files.points.clone(),
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
            kept_points: HashMap::new(),
            waypoints_made: Vec::new(),
            scripted: Vec::new(),
            time_scale: FULL_SPEED,
            load_level: Value::Null,
            requests: Vec::new(),
            inert: Default::default(),
            woken: Vec::new(),
        }
    }

    /// The placements the VM leaves inert: a script waking one is listed.
    pub fn set_inert(&mut self, inert: impl IntoIterator<Item = Handle>) {
        self.inert = inert.into_iter().collect();
    }

    /// Each managed placement's activity this step, by the activity rule
    /// ([`awake`]) from the heroes' pivots as the step begins, in handle
    /// order: what the embedder wakes and puts to sleep in the VM.
    pub fn activity(&mut self, engine: &mut dyn Engine) -> Vec<(Handle, bool)> {
        let heroes: Vec<Vec3> = engine
            .heroes()
            .into_iter()
            .filter_map(|h| engine.hero(h).map(|x| x.pivot))
            .collect();
        self.activity_at(&heroes)
    }

    /// [`ScriptWorld::activity`] with the heroes' pivots given: the TV's,
    /// which before a hero is in measures from the level's start, as its
    /// props stand (openspec AH.27).
    pub fn activity_at(&mut self, heroes: &[Vec3]) -> Vec<(Handle, bool)> {
        let mut out = Vec::new();
        for (&h, a) in &mut self.actors {
            if !a.managed || a.removed {
                continue;
            }
            let d = heroes
                .iter()
                .map(|p| (*p - a.position).length() * crate::script_engine::FEET_PER_METRE)
                .reduce(f32::min);
            a.active = awake(a.range, a.buffer, a.active, d);
            out.push((h, a.active));
        }
        out.sort_unstable_by_key(|k| k.0);
        out
    }

    /// A placement's name: its file's, or its template's for a clone.
    pub fn name(&self, h: Handle) -> String {
        self.actors
            .get(&h)
            .and_then(|a| a.obj.or(a.template))
            .and_then(|o| self.templates.get(&o))
            .map_or_else(String::new, |p| p.name.clone())
    }

    /// A range or buffer `by`'s script wrote on `h`: an inert placement it
    /// leaves awake by the activity rule is woken off the whitelist, logged
    /// and listed (once).
    pub(crate) fn note_wake(&mut self, h: Handle, by: Handle) {
        if !self.inert.contains(&h) || self.woken.iter().any(|w| w.placement == h) {
            return;
        }
        let Some(a) = self.actors.get(&h) else {
            return;
        };
        // A range gives it to the activity rule (it wakes when a hero comes
        // within it), a buffer of 0 with none wakes it now: either way the
        // script has woken it.
        if a.range <= 0.0 && a.buffer > 0.0 {
            return;
        }
        let (name, by) = (self.name(h), self.name(by));
        warn!(placement = %name, woken_by = %by, "a script woke a placement off the whitelist; it stays inert");
        self.woken.push(Woken {
            placement: h,
            name,
            by,
        });
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

    /// The palette's clip lengths `template` has, if any.
    pub fn palette(&self, template: Obj) -> Option<&[f32]> {
        self.palettes.get(&template).map(Vec::as_slice)
    }

    /// An actor set down at `at` facing `facing`, at rest, as a recorded
    /// state has it (a scripted test's or the dev entry's). False for no
    /// such actor.
    pub fn place(&mut self, h: Handle, at: Vec3, facing: f32) -> bool {
        self.place_moving(h, at, facing, Vec3::ZERO)
    }

    /// [`ScriptWorld::place`] moving at `velocity` (ft/s): a recorded
    /// state's creature mid-stride, which a lob's lead reads (AH.60).
    pub fn place_moving(&mut self, h: Handle, at: Vec3, facing: f32, velocity: Vec3) -> bool {
        let Some(a) = self.actors.get_mut(&h) else {
            return false;
        };
        a.position = at;
        a.angles.facing = facing;
        a.velocity = velocity;
        true
    }

    /// Where `actor`'s attach nodes sit and how each is turned, in its own
    /// frame (metres, +Z its facing), by index; one past the list, or never
    /// set, at its pivot. The embedder poses them off the actor's model by
    /// the clips playing ([`crate::hands`], openspec AH.50), then
    /// [`ScriptWorld::place_hung`] sets what hangs from them there, turned
    /// as its node (AH.60).
    pub fn set_nodes(&mut self, actor: Handle, nodes: Vec<NodePose>) {
        self.nodes.insert(actor, nodes);
    }

    /// `actor`'s attach nodes as last set, in its own frame.
    pub fn nodes(&self, actor: Handle) -> Option<&[NodePose]> {
        self.nodes.get(&actor).map(Vec::as_slice)
    }

    /// Each actor hung from a node set at it, as the end of
    /// [`ScriptWorld::advance`] leaves it: run again once the nodes are
    /// posed after that frame, so what hangs from them follows the pose the
    /// frame's clips leave (the original's collider on Eruptor's hand,
    /// AH.50).
    pub fn place_hung(&mut self, engine: &mut dyn Engine) {
        self.place_hung_actors(engine);
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

    /// Takes the file placement `obj` off, as a remove of it does (a
    /// recorded state's breakable already broken); false for none.
    pub fn take_off(&mut self, obj: Obj) -> bool {
        match self.objects.get(&obj).and_then(|h| self.actors.get_mut(h)) {
            Some(a) => {
                a.removed = true;
                true
            }
            None => false,
        }
    }

    /// Whether a remove took the file placement `obj` off (openspec AH.21:
    /// a breakable broken): the prop it stands for stands nowhere again,
    /// its solid gone from both sides of the seam.
    pub fn taken_off(&self, obj: Obj) -> bool {
        self.objects.get(&obj).is_some_and(|&h| self.removed(h))
    }

    /// Takes the file placement `obj` off and puts it to sleep: a placement
    /// a run carries over as taken off from before its install (openspec
    /// AH.27, [`LevelState`]). Asleep it has no ground, so the destructibles
    /// system shuts it down as it does a breakable out of range, never
    /// making it a target again. False for none.
    pub fn put_away(&mut self, obj: Obj) -> bool {
        match self.objects.get(&obj).and_then(|h| self.actors.get_mut(h)) {
            Some(a) => {
                a.removed = true;
                a.active = false;
                true
            }
            None => false,
        }
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

    pub(crate) fn get(&mut self, at: &Access, engine: &mut dyn Engine) -> Result<Value, HostFault> {
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
            Member::Region(n) if on == Some(self.portal) => Value::Int(match n {
                1 => crate::figure::REGION_1,
                _ => crate::figure::REGION_2,
            }),
            Member::Region(_) => return Err(HostFault::Nothing),
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

    pub(crate) fn elements(
        &mut self,
        at: &Access,
        engine: &mut dyn Engine,
    ) -> Result<Vec<Value>, HostFault> {
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
                Some(k) => {
                    k.buffer = f;
                    if let Some(h) = on {
                        self.note_wake(h, at.placement);
                    }
                }
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
                | Member::Region(_)
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

    pub(crate) fn op(
        &mut self,
        call: &OpCall,
        spawn: &mut Spawner<'_>,
        engine: &mut dyn Engine,
    ) -> Result<Answer, HostFault> {
        let first = call.operands.first().and_then(|(_, v)| *v);
        let target = first.and_then(|v| self.handle(v));
        match call.class {
            // The block runs with what it controls as its receiver, and the
            // op reads as it on a path (`<OpControl>.placement.scale`: AH.54,
            // the eruption grows and moves each blob so; read as nothing, the
            // writes went nowhere and the lava sat still at his feet).
            Class::Control => Ok(match (first, target) {
                (_, Some(h)) if self.actors.get(&h).is_some_and(|k| k.removed) => Answer::Skip,
                (Some(v), Some(_)) => Answer::Run {
                    receiver: Some(v),
                    result: Some(v),
                },
                // An object no placement stands for: the pads, the portal.
                (Some(v @ Value::Entity(_)), None) => Answer::Run {
                    receiver: Some(v),
                    result: Some(v),
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
    fn make(&mut self, class: &str) -> Option<Value> {
        self.world.make(class)
    }
    fn adopt(&mut self, program: &Program) -> Vec<(Handle, String, ScriptId)> {
        self.world.adopt(program)
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
/// advances the combat timer every mover's update reads, and keeps the
/// loot in play) and the movement update's (each tick moving the scripted
/// movers). The game systems' tick moves the orbs; the player loop runs
/// that stretch of it alone (openspec AH.20).
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

/// The VM's config for AH.17's run: AH.16's members, Eruptor's and his
/// figure's (AH.23), the init chain's listing with `global.bld`'s UI and movement update every tick and
/// Eruptor's scripts beside them.
pub fn hero_config(mode: Mode) -> Config {
    let mut c = Config::new(mode);
    members::register(&mut c.members, INIT_CHAIN);
    members::register(&mut c.members, members::HERO);
    // A slider's own members (AH.19): his attacks' and the HUD's slides.
    members::register(&mut c.members, members::COMBAT);
    // His figure's tag on the portal (AH.23): the portal's regions.
    members::register(&mut c.members, members::FIGURE);
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

/// Chapter 1's first fight (openspec AH.18, design decisions 2 and 6): the
/// four Chompies Setup_001 wakes, its controller, Norticus as far as the
/// encounter reaches him (his scene's placement, the trigger that wakes him
/// and the far-off Norticus and Chompies it takes off) by placement name,
/// so the pit's other Chompies running the same script stay inert.
pub const FIRST_FIGHT: [&str; 11] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "Setup_001_Controller",
    "1.NPC_Norticus_First_Encounter",
    "Activate_Norticus and Chompies",
    "Norticus_First_Encounter(1)",
    "Chompie_Dummies(4)",
    "Chompie_Dummies(5)",
    "Chompie_Dummies(6)",
];

/// The loot on the way to the first fight (openspec AH.20, design decision
/// 2's whitelist: the coins in reach and the loot macros): the level's loot
/// system's first tick and the spinning coins on chapter 1's walkway and on
/// the line down into the pit, by placement name, so the level's other
/// coins stay inert.
pub const PICKUPS: [&str; 10] = [
    "LootSystem_Init",
    "Coin_spinning",
    "Coin_spinning_0",
    "Coin_spinning_0(1)",
    "Coin_spinning(1)",
    "Coin_spinning(2)",
    "Coin_spinning(3)",
    "Coin_spinning(4)",
    "Coin_spinning(5)",
    "Coin_spinning(6)",
];

/// The breakables' own scripts (openspec AH.21, design decision 2's
/// whitelist): the level's archetype setups for the clearing's and the
/// ledge route's barrels, gourds and pots, by placement name. Each sets its
/// archetypes' tags (the explosion, the debris set, the sounds) at the
/// level's start and takes itself off; the rest of a breakable's life is
/// `global.bld`'s destructibles system, which the loot system's init lists
/// them with and the game systems' update runs (the player loop's
/// [`crate::player_loop::UPDATES`]).
pub const BREAKABLES: [&str; 3] = [
    "init_temp_barrel_b",
    "init_temp_gourds",
    "init_temp_pottery",
];

/// The VM's config for the TV's run (openspec AH.27): AH.17's with AH.20's
/// pickups and AH.21's breakables and the members they reach (the
/// creatures' and the loop's: the destructibles system makes a breakable a
/// combat target), the first fight's placements left inert until AH.25's
/// entry brings them.
pub fn smash_config(mode: Mode) -> Config {
    let mut c = hero_config(mode);
    members::register(&mut c.members, members::CREATURES);
    members::register(&mut c.members, members::LOOP);
    if let Whitelist::Only { placements, .. } = &mut c.whitelist {
        placements.extend(PICKUPS.iter().map(|s| (*s).to_owned()));
        placements.extend(BREAKABLES.iter().map(|s| (*s).to_owned()));
    }
    c
}

/// The VM's config for AH.18's run: AH.27's ([`smash_config`]) with the
/// first fight's placements.
pub fn fight_config(mode: Mode) -> Config {
    let mut c = smash_config(mode);
    if let Whitelist::Only { placements, .. } = &mut c.whitelist {
        placements.extend(FIRST_FIGHT.iter().map(|s| (*s).to_owned()));
    }
    c
}

/// The VM over `program` with every running placement of `files` added
/// unit by unit (`global.bld`'s, then `init_setup.bld`'s, then the level's,
/// by `order`'s units), each under the handle `handles` gives its object;
/// every placement's object is named, scripted or not. Within a unit, those
/// [`crate::frame::AWAKE_HEAD`] names go first in its order, the order the
/// original's scheduler holds `global.bld`'s in (openspec AH.59), the rest
/// after them in file order. A running placement whose script the program
/// lacks is left out, logged.
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
    let head = |p: &&Placed| {
        crate::frame::AWAKE_HEAD
            .iter()
            .position(|n| *n == p.name)
            .unwrap_or(usize::MAX)
    };
    for &unit in order {
        let mut placed: Vec<&Placed> = files.placed.iter().filter(|p| p.obj.unit == unit).collect();
        placed.sort_by_key(head);
        for p in placed {
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

/// The join's second half, after the definition's first tick: with a
/// figure's tag on the slot ([`crate::figure::put_on_portal`], before
/// [`join_request`]), `Portal - Enter Object`'s links to it
/// ([`crate::figure::enter`], AH.23); then the minion its definition
/// registered becomes `hero` (a spawn of its template answers with him),
/// and `Game - Activate/Deactivate Minion` runs as the portal manager's
/// entry beam calls it (`Portal - Entry Activate Minion`): the minion,
/// spawned where `hero` stands, for the first player's controller, the
/// definition's minion description, no health restored, in a level
/// transition. The minion's own script then joins the VM under `hero` (its
/// template's, listed for every tick), the activator (openspec AH.59: the
/// original's scheduler holds the hero's minion alone in its first group,
/// so his script ticks ahead of every other placement). Returns its
/// placement.
pub fn join_activate(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    hero: Handle,
) -> Result<PlacementId, Halt> {
    let pm = portal_manager(vm, host.world)?;
    crate::figure::enter(vm, host.world)?;
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
            vm.set_activator(h, true);
            minion_placement = Some(id);
        }
    }
    minion_placement.ok_or_else(|| outside("the activation spawned no minion".to_owned()))
}

/// A figure's health kept across a swap within a level (openspec AH.20;
/// design decision 2's stand-in row, Health kept across a swap; combat: A
/// hero's health): in the original the portal manager keeps each figure's
/// health in its stored data while the level lasts, keyed by the tag, and
/// clears it at a level change. guacamole keys it by the figure. The hero
/// taken off mid-level leaves his health here ([`KeptHealth::left`]); put
/// back in the same level, his join sets it ([`set_health`]) from
/// [`KeptHealth::back`]; a level change clears it. The TV's portal has no
/// mid-level removal yet (AG.29's), so nothing calls `left` there today.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeptHealth {
    kept: HashMap<String, i32>,
}

impl KeptHealth {
    /// `figure` taken off mid-level with `health` left.
    pub fn left(&mut self, figure: &str, health: i32) {
        self.kept.insert(figure.to_owned(), health);
    }

    /// The health `figure` comes back in with, if it left this level.
    pub fn back(&self, figure: &str) -> Option<i32> {
        self.kept.get(figure).copied()
    }

    /// A level change: every figure comes in full again.
    pub fn level_changed(&mut self) {
        self.kept.clear();
    }
}

/// The hero's health as his scripts hold it: his minion's
/// `description.creature info.hp`.
pub fn health(vm: &Vm, minion: PlacementId) -> Option<i32> {
    let d = vm.local(minion, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_i32()
}

/// The hero's health set where his scripts hold it, as the swap's stand-in
/// puts back what he left with. False when he has no health to set.
pub fn set_health(vm: &mut Vm, minion: PlacementId, health: i32) -> bool {
    let Some(d) = vm.local(minion, "description").map(|v| vm.value(v)) else {
        return false;
    };
    let Some(c) = vm.field(d, "creature info").map(|v| vm.value(v)) else {
        return false;
    };
    vm.set_field(c, "hp", Value::Int(health))
}

/// A kind of breakable by what it drops (design decision 13): each kind
/// keeps a running count of its own in the level's loot data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
}

impl Rarity {
    /// The loot data's reference to the kind's count.
    fn field(self) -> &'static str {
        match self {
            Rarity::Common => "DestructiblesCommonRef",
            Rarity::Uncommon => "DestructiblesUncommonRef",
            Rarity::Rare => "DestructiblesRareRef",
        }
    }
}

/// The kind's loot count as the UI's `LootData` holds it: its rate and
/// where its running count stands. The loot system's init draws where each
/// starts at the level's start (`Loot - Set Loot Counts`).
fn loot_count(vm: &Vm, world: &ScriptWorld, rarity: Rarity) -> Option<Value> {
    let ui = running(vm, world, "/UI.ai")?;
    let data = vm.local(ui, "LootData").map(|v| vm.value(v))?;
    vm.field(data, rarity.field()).map(|v| vm.value(v))
}

/// Where the kind's running count stands (openspec AH.21), none before the
/// level's loot data is made.
pub fn loot_index(vm: &Vm, world: &ScriptWorld, rarity: Rarity) -> Option<f32> {
    let count = loot_count(vm, world, rarity)?;
    vm.field(count, "DropRateIndex")
        .and_then(|v| vm.value(v).as_f32())
}

/// The kind's running count set where an anchor's read has it (a
/// comparison with the oracle starts from the anchor's index, not the
/// session's draw). False before the loot data is made.
pub fn set_loot_index(vm: &mut Vm, world: &ScriptWorld, rarity: Rarity, index: f32) -> bool {
    match loot_count(vm, world, rarity) {
        Some(count) => vm.set_field(count, "DropRateIndex", Value::Float(index)),
        None => false,
    }
}

/// What a run installed again carries over when the level goes on under it
/// (openspec AH.27): the hero's take-off installs the TV's run again (the
/// swap's stand-in, `openspec/backlog.md`), and this keeps the level as he
/// left it: its placements play took off (each breakable broken, each
/// spinning coin taken: [`LevelState::since`] the join, not the setups that
/// take themselves off at the level's start, which the fresh run runs
/// again) stay off, and each kind's loot count stays where it stood rather
/// than drawn anew. Not carried: the clones in flight (a coin not yet paid,
/// debris), which the original's swap leaves to land.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelState {
    /// The level's placements taken off, in object order.
    pub taken_off: Vec<Obj>,
    /// Each kind's count where the loot data held it.
    pub loot: Vec<(Rarity, f32)>,
}

impl LevelState {
    /// The run's state: unit `level`'s placements its scripts took off and
    /// each kind's count.
    pub fn read(vm: &Vm, world: &ScriptWorld, level: u16) -> Self {
        let mut taken_off: Vec<Obj> = world
            .objects
            .iter()
            .filter(|(o, h)| o.unit == level && world.removed(**h))
            .map(|(o, _)| *o)
            .collect();
        taken_off.sort_unstable_by_key(|o| o.index);
        let loot = [Rarity::Common, Rarity::Uncommon, Rarity::Rare]
            .into_iter()
            .filter_map(|r| loot_index(vm, world, r).map(|i| (r, i)))
            .collect();
        Self { taken_off, loot }
    }

    /// What was taken off since `start` (what [`LevelState::read`] found
    /// taken off then: the level's start's own, the setups taking
    /// themselves off), the counts as they stand.
    pub fn since(&self, start: &[Obj]) -> Self {
        Self {
            taken_off: self
                .taken_off
                .iter()
                .filter(|o| !start.contains(o))
                .copied()
                .collect(),
            loot: self.loot.clone(),
        }
    }

    /// Into a fresh run before its first step: each placement taken off
    /// again, in the world and in the VM, and put to sleep
    /// ([`ScriptWorld::put_away`]).
    pub fn put_back(&self, vm: &mut Vm, world: &mut ScriptWorld) {
        for &o in &self.taken_off {
            world.put_away(o);
            if let Some(&h) = world.objects.get(&o) {
                vm.remove(h);
            }
        }
    }

    /// Each count set back where it stood, once the fresh run's loot data
    /// is made (the loot system's first tick): true once done, or with no
    /// count to set.
    pub fn set_loot(&self, vm: &mut Vm, world: &ScriptWorld) -> bool {
        if self.loot.is_empty() {
            return true;
        }
        if loot_count(vm, world, Rarity::Common).is_none() {
            return false;
        }
        self.loot
            .iter()
            .all(|&(r, i)| set_loot_index(vm, world, r, i))
    }
}

/// Each clone of a scripted template made outside a step's walk and not
/// yet in the VM (a native call's spawn between steps), added at the tick
/// order's end running its template's script, to tick from the next step.
/// What a placement's tick or a slice of the player loop spawns during the
/// walk joins the order's end at once and ticks that same step
/// ([`guac_script::Host::adopt`], openspec AH.54; AH.59's recordings list a
/// clone at the scheduler's list's end the frame its spawner ran), so none
/// of those waits for this.
pub fn add_scripted(vm: &mut Vm, world: &mut ScriptWorld) -> Vec<PlacementId> {
    let program = vm.program_arc();
    world
        .adopt(&program)
        .into_iter()
        .map(|(h, name, sid)| vm.add(h, &name, sid))
        .collect()
}

pub(crate) fn outside(why: String) -> Halt {
    Halt {
        placement: None,
        placement_name: "(the join)".to_owned(),
        script: String::new(),
        position: 0,
        op: None,
        reason: guac_script::Reason::Unsupported(why),
    }
}

/// The portal manager's handle.
fn portal_manager_handle(world: &ScriptWorld) -> Result<Handle, Halt> {
    world
        .templates
        .values()
        .find(|p| p.name == PORTAL_MANAGER)
        .and_then(|p| world.objects.get(&p.obj).copied())
        .ok_or_else(|| outside(format!("no {PORTAL_MANAGER} placement")))
}

/// The placement running the script whose source path ends with `suffix`,
/// if one runs.
pub(crate) fn running(vm: &Vm, world: &ScriptWorld, suffix: &str) -> Option<PlacementId> {
    world
        .templates
        .values()
        .filter(|p| p.script.as_deref().is_some_and(|s| s.ends_with(suffix)))
        .filter_map(|p| world.objects.get(&p.obj))
        .find_map(|&h| vm.placement_of(h))
}

/// The portal manager's placement, once it runs: the context the player
/// loop's stretches and the figure's links run in.
pub fn portal_manager_placement(vm: &Vm, world: &ScriptWorld) -> Option<PlacementId> {
    portal_manager(vm, world).ok()
}

pub(crate) fn portal_manager(vm: &Vm, world: &ScriptWorld) -> Result<PlacementId, Halt> {
    let h = portal_manager_handle(world)?;
    vm.placement_of(h)
        .ok_or_else(|| outside(format!("{PORTAL_MANAGER} doesn't run")))
}

/// The first player's slot's loaded data.
pub(crate) fn slot_zero(vm: &mut Vm) -> Result<Value, Halt> {
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

    /// The clones of scripted templates spawned since last asked, each
    /// with its template's name and script ([`guac_script::Host::adopt`]);
    /// one whose script `program` doesn't hold is dropped.
    pub fn adopt(&mut self, program: &Program) -> Vec<(Handle, String, ScriptId)> {
        std::mem::take(&mut self.scripted)
            .into_iter()
            .filter_map(|(h, t)| {
                let sid = self.script_of(t, program)?;
                Some((h, self.name_of(t).unwrap_or_default(), sid))
            })
            .collect()
    }

    /// Where in `program.scripts` the script a template runs is; none when
    /// `program` doesn't hold it.
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
    /// The portal manager's tick as the player loop runs it (openspec
    /// AH.20): stepped ahead of every placement's tick.
    pub player_loop: PlayerLoop,
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

/// Steps the scripts once a fixed step, in play mode: the frame's slides,
/// then the walk in the original's order with the player loop's slices in
/// their placements' places ([`PlayerLoop::walk`]; no hero joins here).
pub fn step_scripts(mut runtime: ResMut<ScriptRuntime>, mut host: ScriptHost) {
    let ScriptRuntime {
        vm,
        generator,
        player_loop,
        ..
    } = &mut *runtime;
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
    let mut stopped = Vec::new();
    let report = vm.slide(&mut world_host).and_then(|slid| {
        stopped.extend(slid.stopped);
        player_loop.walk(vm, &mut world_host, false, &mut stopped)
    });
    host.world.end_step();
    runtime.steps += 1;
    match report {
        Ok(r) => stopped.extend(r.stopped),
        Err(h) => stopped.push(h),
    }
    for h in &stopped {
        warn!(%h, "a script stopped");
    }
    runtime.stopped.extend(stopped);
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
    let player_loop = PlayerLoop::read(&program);
    let vm = init_chain(program, files, &handles, order, config);
    let mut script_world = ScriptWorld::new(files, &handles, objects);
    // What a listed script wakes off the whitelist is logged and listed
    // (decision 6), as the headless runs list it.
    script_world.set_inert(vm.inert());
    world.insert_resource(script_world);
    world.insert_resource(ScriptRuntime {
        vm,
        generator: Generator::seeded(guac_script::session_seed()),
        player_loop,
        steps: 0,
        stopped: Vec::new(),
    });
}

/// The scripts' system in `FixedUpdate`'s [`Frame::Scripts`]: nothing runs
/// until a level's script part is installed.
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            step_scripts
                .in_set(Frame::Scripts)
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
    use guac_motion::hero::Hero;
    use guac_script::members::{find, label};

    use super::*;
    use crate::script_engine::Pad;

    /// The swap's stand-in: a figure taken off mid-level comes back with
    /// what it left with, another figure with nothing kept, the latest
    /// leaving winning; a level change clears every figure's.
    #[test]
    fn health_is_kept_across_a_swap_and_cleared_at_a_level_change() {
        let mut kept = KeptHealth::default();
        assert_eq!(kept.back("eruptor"), None, "never left: full");
        kept.left("eruptor", 200);
        kept.left("tree rex", 430);
        assert_eq!(kept.back("eruptor"), Some(200));
        kept.left("eruptor", 150);
        assert_eq!(kept.back("eruptor"), Some(150), "the latest leaving");
        assert_eq!(kept.back("bash"), None);
        kept.level_changed();
        assert_eq!(kept.back("eruptor"), None, "the next level: full");
        assert_eq!(kept.back("tree rex"), None);
    }

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
            points: HashMap::new(),
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
            sets: Vec::new(),
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
        // A tell on one runs its block with it, and reads as it.
        let pad = Value::Entity(objects[0]);
        assert_eq!(
            w.op(&call(Class::Control, Some(pad)), &mut |_| 0, &mut NoEngine),
            Ok(Answer::Run {
                receiver: Some(pad),
                result: Some(pad)
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
        // A tell on the clone runs its block with it, and reads as it
        // (AH.54: `<OpControl>.placement.scale` is the clone's).
        let tell = call(Class::Control, Some(clone));
        assert_eq!(
            w.op(&tell, &mut |_| 0, &mut NoEngine),
            Ok(Answer::Run {
                receiver: Some(clone),
                result: Some(clone)
            })
        );
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
        // A field of view with nothing to look from is a path through
        // nothing (AH.18 answers the op).
        assert_eq!(
            w.op(&call(Class::CheckFov, None), &mut |_| 0, &mut NoEngine),
            Err(HostFault::Nothing)
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
            points: HashMap::new(),
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
    fn the_activity_rule_wakes_by_range_and_holds_to_its_buffer() {
        // A range: awake within it, staying awake out to it plus the buffer.
        assert!(awake(20.0, 1.0, false, Some(19.9)));
        assert!(!awake(20.0, 1.0, false, Some(20.5)));
        assert!(awake(20.0, 1.0, true, Some(20.5)));
        assert!(!awake(20.0, 1.0, true, Some(21.1)));
        assert!(!awake(20.0, 1.0, true, None), "no hero");
        // No range: awake with a buffer of 0, else its scripts' to wake.
        assert!(awake(0.0, 0.0, false, None));
        assert!(!awake(0.0, 1.0, true, Some(0.0)));
        // Over a world: the Grunt's 30 ft from a hero 9 m off (29.5 ft).
        let (mut w, grunt, _) = world();
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer, g.managed) = (30.0, 1.0, true);
        let mut hero = him(Vec3::new(9.0, 0.0, 0.0));
        assert_eq!(w.activity(&mut hero), vec![(grunt, true)]);
        hero.0.pivot.x = 9.4;
        assert_eq!(w.activity(&mut hero), vec![(grunt, true)], "its buffer");
        hero.0.pivot.x = 10.0;
        assert_eq!(w.activity(&mut hero), vec![(grunt, false)]);
        let inactive = at("actorinfo", "inactive", None);
        assert_eq!(
            w.elements(&inactive, &mut hero),
            Ok(vec![Value::Entity(grunt)])
        );
    }

    /// A hero standing where the test puts him, over nothing.
    struct Him(Hero, guac_motion::parry::ParryWorld);

    fn him(at: Vec3) -> Him {
        Him(
            Hero::new(
                guac_motion::tuning::eruptor_solid(),
                guac_motion::tuning::Tuning::ERUPTOR,
                at,
                0.0,
            ),
            guac_motion::parry::ParryWorld::new(&[]).unwrap(),
        )
    }

    const HIM: Handle = 0x7000_0000;

    impl Engine for Him {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            (h == HIM).then_some(&mut self.0)
        }
        fn heroes(&self) -> Vec<Handle> {
            vec![HIM]
        }
        fn seam(&self) -> &dyn guac_motion::seam::PhysicsSeam {
            &self.1
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
    }

    #[test]
    fn a_script_waking_a_placement_off_the_whitelist_is_listed_once() {
        let (mut w, grunt, _) = world();
        let by = 0x0004_0000_0063;
        w.actors.get_mut(&grunt).unwrap().buffer = 1.0;
        w.set_inert([grunt]);
        let buffer = Access {
            placement: by,
            ..at(
                "placement",
                "deactivation buffer",
                Some(Value::Entity(grunt)),
            )
        };
        // A buffer of 2 leaves it asleep: nothing woken.
        w.set(&buffer, Value::Float(2.0), &mut NoEngine).unwrap();
        assert!(w.woken.is_empty());
        // 0 with no range wakes it: listed, by the waker's name.
        w.set(&buffer, Value::Float(0.0), &mut NoEngine).unwrap();
        assert_eq!(
            w.woken,
            vec![Woken {
                placement: grunt,
                name: "Grunt".to_owned(),
                by: String::new(),
            }]
        );
        // A range on it again: listed once.
        let range = Access {
            placement: by,
            ..at("placement", "activation range", Some(Value::Entity(grunt)))
        };
        w.set(&range, Value::Float(160.0), &mut NoEngine).unwrap();
        assert_eq!(w.woken.len(), 1);
        // One the whitelist runs isn't listed.
        let (mut w, grunt, _) = world();
        w.actors.get_mut(&grunt).unwrap().buffer = 1.0;
        let buffer = at(
            "placement",
            "deactivation buffer",
            Some(Value::Entity(grunt)),
        );
        w.set(&buffer, Value::Float(0.0), &mut NoEngine).unwrap();
        assert!(w.woken.is_empty());
    }

    #[test]
    fn the_fight_lists_setup_001s_chompies_by_name_and_no_others() {
        let c = fight_config(Mode::Play);
        let chompy = "C:/tfb/Content/Levels/_Enemies/Chompy/scripts/S2_Enemy_Chompy.ai";
        for n in 1..=4 {
            assert!(c.whitelist.lists(&format!("Enemy_Chompy({n})"), chompy));
        }
        assert!(!c.whitelist.lists("Enemy_Chompy(11)", chompy));
        assert!(
            c.whitelist
                .lists("Setup_001_Controller", "C:/x/Setup_001_Controller.ai")
        );
        assert!(
            !c.whitelist
                .lists("Setup_002_Controller", "C:/x/Setup_002_Controller.ai")
        );
        assert!(
            c.whitelist
                .lists("EnemySystem_", "C:/x/_EnemySystem_Functions.ai")
        );
        assert!(
            c.whitelist
                .lists("movementUpdate", "C:/x/movementUpdate.ai")
        );
        assert!(
            !c.whitelist
                .lists("Eon_4_help_Norticus", "C:/x/Pan_Show_Norticu.ai")
        );
        for r in members::CREATURES {
            assert!(
                c.members
                    .get(MemberId::named(r.namespace, r.name))
                    .is_some()
            );
        }
    }

    /// The TV's listing (openspec AH.27): the pickups and the breakables
    /// by name, with the creatures' and the loop's members; the first
    /// fight's Chompies, Setup_001 and what only a Giant moves (the Titan
    /// boulders' and the castle wall's scripts) off it, inert, so nothing
    /// takes them off; the fight's own listing AH.27's with the four.
    #[test]
    fn the_tvs_listing_runs_the_breakables_and_no_creature_or_titan_feat() {
        let c = smash_config(Mode::Play);
        for name in BREAKABLES.iter().chain(&PICKUPS) {
            assert!(c.whitelist.lists(name, "C:/x/Anything.ai"), "{name}");
        }
        let chompy = "C:/tfb/Content/Levels/_Enemies/Chompy/scripts/S2_Enemy_Chompy.ai";
        assert!(!c.whitelist.lists("Enemy_Chompy(1)", chompy));
        assert!(
            !c.whitelist
                .lists("Setup_001_Controller", "C:/x/Setup_001_Controller.ai")
        );
        for (name, script) in [
            (
                "Titan_CarriableObject_Boulder_v3_(4)",
                "C:/tfb/Content/Levels/Includes/Titan_Includes/Titan_CarribleObject/Scripts/Titan_Carriable_Object.ai",
            ),
            (
                "Breakable_Stone_Wall_Castle_206(1)",
                "C:/tfb/Content/Levels/Includes/GameElement_Breakable_Wall/Scripts/Breakable_Wall_213.ai",
            ),
        ] {
            assert!(!c.whitelist.lists(name, script), "{name}");
        }
        assert!(c.whitelist.lists("Eruptor", "C:/x/009_Eruptor.ai"));
        for r in members::CREATURES.iter().chain(members::LOOP) {
            assert!(
                c.members
                    .get(MemberId::named(r.namespace, r.name))
                    .is_some()
            );
        }
        let fight = fight_config(Mode::Play);
        for name in FIRST_FIGHT.iter().chain(&BREAKABLES).chain(&PICKUPS) {
            assert!(fight.whitelist.lists(name, "C:/x/Anything.ai"), "{name}");
        }
    }

    /// The TV's activity rule from the points it's given (openspec AH.27):
    /// none, every ranged placement asleep; the level's start before a hero
    /// is in, as from a hero standing there; a placement put away (carried
    /// over as taken off) never woken again, asleep.
    #[test]
    fn the_activity_rule_from_given_points_and_a_placement_put_away() {
        let (mut w, grunt, _) = world();
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer, g.managed) = (30.0, 1.0, true);
        assert_eq!(w.activity_at(&[]), vec![(grunt, false)], "no one");
        let start = Vec3::new(9.0, 0.0, 0.0);
        assert_eq!(w.activity_at(&[start]), vec![(grunt, true)]);
        let mut hero = him(start);
        assert_eq!(w.activity(&mut hero), w.activity_at(&[start]));
        let obj = *w.objects.iter().find(|(_, h)| **h == grunt).unwrap().0;
        assert!(w.put_away(obj));
        assert!(w.taken_off(obj));
        assert!(w.activity_at(&[start]).is_empty(), "never woken");
        assert!(
            !w.actors[&grunt].active,
            "asleep: no ground, as AH.21's sleeping breakable"
        );
        assert!(!w.put_away(Obj { unit: 9, index: 9 }));
    }

    /// Setup_001's 7 ft rule from a recorded state (AH.8's anchor B and the
    /// route's scaffold): a controller waiting for a minion within 7 ft of
    /// its first Chompy's height stays put while the hero stands 26 ft above
    /// on the pit's rim, and moves on once he's down on its floor.
    #[test]
    fn an_encounter_from_a_recorded_state_wakes_on_the_7_ft_rule() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::builder::{El, Writer};
        use guac_formats::script::{Graph, StackClass};
        use guac_script::{Role, Unit};

        let mut b = IgzBuilder::new();
        let objects;
        {
            let mut w = Writer::new(&mut b);
            let hero = w.b.object(0, "tfbActorInfo", 0xF8);
            let chompy = w.b.object(0, "tfbActorInfo", 0xF8);
            let controller = w.b.object(0, "tfbActorInfo", 0xF8);
            objects = [hero, chompy, controller];
            let minions = w.variable("minions", "ScriptSet", None);
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let lhs = w.stack(StackClass::Set, &[El::Local(minions)]);
            let rhs = w.reference(&[El::Local(hero)], None);
            let o = w.op(Class::ChangeMembership, "set [set]||to");
            w.refer(o, 0x1C, lhs);
            w.word(o, 0x20, 0);
            w.refer(o, 0x24, rhs);
            w.push(o);
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (macros, _) = w.open(Class::MacroSection, "MACROS");
            w.end_block(macros);
            w.end_block(pre);
            let intro = w.op(Class::UserBehavior, "Chompy - Intro");
            let wait = w.behaviour("Wait For Minion");
            let path = w.stack(
                StackClass::Value,
                &[
                    El::Local(minions),
                    El::Null,
                    El::Member("positionmeasurement", "vertical offset"),
                ],
            );
            let from = w.reference(&[El::Local(chompy)], None);
            w.refer(path, 0x18, from);
            let seven = w.int(7);
            let (fs_at, fs) = w.open(Class::FindSubSet, "find subset|of||||subset");
            w.refer(fs, 0x1C, path);
            w.word(fs, 0x20, 0);
            w.refer(fs, 0x24, seven);
            w.set_behavior(intro);
            w.end_block(fs_at);
            w.end_block(wait);
            let at_intro = w.push(intro);
            w.end_block(at_intro);
            w.end("C:/tfb/Synthetic/Setup_001_Controller.ai");
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "level",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let obj = |l| Obj {
            unit: 0,
            index: igz.object_at(l).unwrap().0 as u32,
        };
        let [hero, chompy, controller] = objects.map(obj);
        // Where AH.8's anchor B has `Enemy_Chompy(1)`.
        let files = Files {
            placed: vec![
                Placed {
                    position: [4.5213137, 22.374975, -23.117332],
                    ..Placed::named(chompy, "Enemy_Chompy(1)", None, RUNNING)
                },
                Placed::named(hero, "Hero", None, RUNNING),
                Placed::named(
                    controller,
                    "Setup_001_Controller",
                    Some("C:/tfb/Synthetic/Setup_001_Controller.ai"),
                    RUNNING,
                ),
            ],
            ids: HashMap::new(),
            points: HashMap::new(),
        };
        let mut handles = headless_handles(&files);
        handles.insert(hero, HIM);
        let mut c = fight_config(Mode::Strict);
        c.whitelist = Whitelist::All;
        let mut vm = init_chain(program, &files, &handles, &[0], c);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let id = vm.placement_of(handles[&controller]).unwrap();
        // The route's scaffold at the pit's rim, then landed on its floor.
        let rim = Vec3::new(10.67, 30.35, -1.93);
        let floor = Vec3::new(7.27, 22.39, -7.56);
        let mut him = him(rim);
        let mut generator = Generator::seeded(1);
        let mut spawn = headless_spawner(1 << 40);
        let mut step = |vm: &mut Vm, world: &mut ScriptWorld, him: &mut Him| {
            let mut host = WorldHost {
                world,
                engine: him,
                generator: &mut generator,
                spawn: &mut spawn,
            };
            vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
        };
        for _ in 0..30 {
            step(&mut vm, &mut world, &mut him);
        }
        assert_eq!(vm.behaviour(id), Some("wait for minion"), "26 ft above");
        him.0.pivot = floor;
        step(&mut vm, &mut world, &mut him);
        step(&mut vm, &mut world, &mut him);
        assert_eq!(vm.behaviour(id), Some("chompy - intro"));
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
