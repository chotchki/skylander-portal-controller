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
//! - [`join_request`], [`join_activate`] and [`join_deactivate`]: a
//!   figure's character joining the run when it goes on the portal and
//!   leaving it when it comes off mid-level, the original's macros with
//!   their scheduling ours (openspec AH.17, AH.105; stand-in ledger: `join`);
//!   on the TV [`join_enter`] and [`entry_beams`] in [`join_activate`]'s
//!   place, his minion activated by the original's entry beam as its column
//!   lands (openspec AH.224).
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
//! pads and the portal one object each, `lightinfo`'s colours, multiplier
//! and attenuation a placed light's as its file holds them and a clone of
//! one its template's (openspec AH.65: the point-light manager copies them
//! onto its pool's lights, which the TV draws). World ops: a tell runs its
//! block on any placement or clone not taken off; a spawn of a template
//! makes a clone (an entity in the TV, nothing drawn yet); a remove takes a
//! clone or a placement off; a reset puts a placement's numbers back as its
//! file has them, and a light the manager took off back on; a stopped
//! sequence is nothing (none plays yet). Any other op or
//! member stops the script that reaches it, named (play mode: that script
//! alone).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    ActorWaypoint, ActorWaypointList, Layout, LightInfo, ObjectRef, RootList, is_a,
};
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
    Actor, Angles, Engine, Light, NoEngine, NodePose, Object, Objects, Pad, Physics, Sequence,
    Solid,
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
    /// Its place in its file's root list (the file's first object), the
    /// order the original's load takes the file's placements in (openspec
    /// AH.195); one off the list after the listed ones, by its object.
    pub load: u32,
}

impl Placed {
    /// A placement with nothing but its object, name and script: what the
    /// tests use in place of one read off a file.
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
            load: 0,
        }
    }
}

/// What the host reads off the files the scripts load with.
#[derive(Debug, Clone, Default)]
pub struct Files {
    /// Every actor placement, file by file, each file's in object order
    /// (its load order is [`Placed::load`]'s).
    pub placed: Vec<Placed>,
    /// Every placement's `_ID`, sprites and lights included.
    pub ids: HashMap<Obj, i32>,
    /// Every waypoint's place in the level, metres (openspec AH.18: what a
    /// `move to` along a path walks to and a measurement is taken from).
    pub points: HashMap<Obj, Vec3>,
    /// Every waypoint's turn as its file holds it (openspec AS.52: what a
    /// script reads of one until a script turns it, a cutscene's camera
    /// looking from it as it faces).
    pub turns: HashMap<Obj, Angles>,
    /// Every placed light's values as its file holds them (openspec AH.65):
    /// the point-light manager's template and the lights the scripts name
    /// for it to copy.
    pub lights: HashMap<Obj, Light>,
    /// Every emitter's name (openspec AH.87): what a clone of one draws,
    /// the particle part's emitter of that name.
    pub emitters: HashMap<Obj, String>,
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
                    let obj = Obj {
                        unit,
                        index: index as u32,
                    };
                    out.points
                        .insert(obj, Vec3::from_array(w.init_matrix.position));
                    let [tilt, facing, roll] = w.init_matrix.rotation;
                    out.turns.insert(obj, Angles { tilt, facing, roll });
                }
            }
            let placements = Placements::read(igz).map_err(|e| e.to_string())?;
            let roots = root_places(igz);
            for (index, l) in &placements.lights {
                out.lights.insert(
                    Obj {
                        unit,
                        index: *index as u32,
                    },
                    light_of(l),
                );
            }
            for (index, e) in &placements.emitters {
                out.emitters.insert(
                    Obj {
                        unit,
                        index: *index as u32,
                    },
                    e.placement.placement.name.unwrap_or("").to_owned(),
                );
            }
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
                    angles: c.transform.rotation,
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
                    load: match roots.get(&actor.index) {
                        Some(&k) => k,
                        None => roots.len() as u32 + actor.index as u32,
                    },
                });
            }
        }
        Ok(out)
    }
}

/// Each object the file's root list (its first object, an `igObjectList`)
/// holds, by index, with its place on the list; none for a file without
/// one. Every actor placement of the five files level 213's scripts load
/// with is on its file's list once (AH.195's read of the disc).
fn root_places(igz: &Igz<'_>) -> HashMap<usize, u32> {
    let Ok(root) = RootList::read(igz, 0) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (k, e) in root.elements.iter().enumerate() {
        if let ObjectRef::Object(i) = e {
            out.entry(*i).or_insert(k as u32);
        }
    }
    out
}

/// A name a trigger goes by (AH.4's live read, as the oracle harness's
/// census reads them): `Trigger` in it (either case), or `trg_`,
/// `Transition `, `Activate_` or `Pop_` before it.
pub fn is_trigger(name: &str) -> bool {
    name.contains("Trigger")
        || name.contains("trigger")
        || ["trg_", "Transition ", "Activate_", "Pop_"]
            .iter()
            .any(|p| name.starts_with(p))
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

/// A placed light's values as its file holds them, nothing lifted.
fn light_of(l: &LightInfo<'_>) -> Light {
    Light {
        diffuse: u32::from_be_bytes(l.diffuse),
        ambient: u32::from_be_bytes(l.ambient),
        specular: u32::from_be_bytes(l.specular),
        multiplier: l.multiplier,
        attenuation: [
            l.constant_attenuation,
            l.linear_attenuation,
            l.quadratic_attenuation,
        ],
        lift: 0.0,
    }
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
    /// Every placed light's values (openspec AH.65): a light placement's
    /// members read and write these; a clone of one copies them.
    pub(crate) light_templates: HashMap<Obj, Light>,
    /// Every emitter's name (openspec AH.87).
    pub(crate) emitters: HashMap<Obj, String>,
    /// Every waypoint's place, metres.
    pub(crate) points: HashMap<Obj, Vec3>,
    /// The file waypoints' turns: as their files hold them (openspec
    /// AS.52), then as the scripts wrote them (AG.60).
    pub(crate) point_turns: HashMap<Obj, Angles>,
    pub(crate) controllers: [Handle; 2],
    pub(crate) physical: [Handle; 4],
    pub(crate) portal: Handle,
    /// `viewportinfo.default`: the gameplay camera, as one object.
    pub(crate) viewport: Handle,
    /// What the camera's scripts leave the viewport as (openspec AG.28).
    pub(crate) view: crate::viewport::Viewport,
    /// The view handed back at a hero's fall-back, until the next join
    /// (openspec AG.211, [`ScriptWorld::hand_back_view_until_a_join`]).
    pub(crate) view_until_join: bool,
    /// The on-screen asks answered off that view while the scripts drive
    /// it, by op (openspec AH.150's gating reads); every other ask reads on
    /// screen (AH.20; stand-in ledger: `on-screen`).
    pub(crate) view_asks: std::collections::HashSet<guac_script::OpId>,
    pub(crate) objs: Objects,
    pub(crate) sequences: Vec<Sequence>,
    /// Each sequence a play started again (its clip's copy on its actor
    /// handed back, openspec AH.184), by its index, in the order started:
    /// a start the list of sequences doesn't show.
    pub(crate) restarts: Vec<u32>,
    /// Each template's animation palette: every slot's clip length,
    /// seconds at 100 %, 0 where a slot is empty.
    pub(crate) palettes: HashMap<Obj, Vec<f32>>,
    /// Each template's palette's animation info as its file holds it, slot
    /// by slot (openspec AH.49): a template given none reads every slot a
    /// base clip with no layered bit.
    pub(crate) clip_info: HashMap<Obj, Vec<ClipInfo>>,
    /// What the scripts wrote on a palette slot's clip itself (openspec
    /// AH.49): a sequence started from it starts with these.
    pub(crate) clip_state: HashMap<(Obj, u32), ClipState>,
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
    /// The points references to actors keep, by the actor and the
    /// reference's cell (openspec AG.28).
    pub(crate) kept_via: HashMap<(Handle, u32), Vec3>,
    /// The waypoints scripts made ([`Object::Waypoint`]), each where it was
    /// last put, none until it's put somewhere.
    pub(crate) waypoints_made: Vec<Option<Vec3>>,
    /// Clones of scripted templates the scripts made this step, with the
    /// template: the embedder adds each as a placement running its script.
    pub scripted: Vec<(Handle, Obj)>,
    /// File placements a script reset after one took them off, this step
    /// (openspec AS.60): each back on as its file has it, and the embedder
    /// adds each running one's script again from its STARTUP
    /// ([`add_scripted`]).
    pub revived: Vec<(Handle, Obj)>,
    time_scale: f32,
    load_level: Value,
    /// The levels the scripts asked for, as each step took them.
    pub requests: Vec<Value>,
    /// The disc's levels by archive name (openspec AS.15): a request whose
    /// text names one is a level change; none set, no request is one.
    levels: Option<Arc<std::collections::HashSet<String>>>,
    /// The level change a script asked for, held while it loads.
    level_change: Option<LevelChange>,
    /// The last requests that changed no level, as their text read
    /// (unresolved: `None`), newest last.
    pub streams: std::collections::VecDeque<Option<String>>,
    /// The cutscene [`crate::level_end`] last skipped, so it skips each once.
    pub(crate) skipped_cutscene: Option<Value>,
    /// Whether [`crate::level_end`] skips any cutscene in play as it skips a
    /// listed one: a dev entry's settle on a level with an intro, so its
    /// anchor sets him down past the intro as before AS.81 played it.
    pub skip_cutscenes: bool,
    /// Whether [`crate::level_end`] has asked for the level end's level.
    pub(crate) level_end_asked: bool,
    /// The movie the level end queued and how far its play has got
    /// (openspec AS.19, [`crate::level_end`]); none queued, or none yet.
    pub(crate) level_end_movie: Option<LevelEndMovie>,
    /// Whether the embedder plays the level end's movie (the TV's
    /// `movies`): one that doesn't has it over as soon as it's queued, the
    /// level asked in the same step as before AS.19.
    pub plays_movies: bool,
    /// The end-of-level sequencing as [`crate::level_end`] meets it on the
    /// run (openspec AS.53): the completion screen's results and continue,
    /// the movie it queued, the fader's clean-up.
    pub sequence: crate::level_end::Sequence,
    /// The level end [`crate::level_end`] wrote, and why it changed no level
    /// yet if it hasn't.
    level_end: Option<LevelEnd>,
    /// The achievements the scripts unlocked, by id, in the order written
    /// (`system.game achievement`, openspec AH.220): kept and logged, none
    /// awarded until AM's per-profile achievements read them
    /// (stand-in ledger: `achievements`).
    pub achievements: Vec<i32>,
    /// `system.save state` as the host answers it (openspec AS.38,
    /// [`crate::story_save`]): a save or load asked reads active until
    /// guacamole's store has done it.
    pub save: crate::story_save::SaveState,
    /// The placements the VM leaves inert, off the whitelist (design
    /// decision 6): a script waking one is logged and listed in `woken`.
    pub(crate) inert: std::collections::HashSet<Handle>,
    /// Each placement off the whitelist a whitelisted script woke, in the
    /// order woken: what it was, and whose script woke it.
    pub woken: Vec<Woken>,
    /// Each managed placement the activity rule has woken, in the order
    /// first woken (those of one step by handle): a range trigger has fired
    /// once it has been active (AH.4's rule; openspec AH.25's equivalence).
    pub fired: Vec<Handle>,
    /// Where the activity rule measured from as this step began (the
    /// heroes' pivots, or the TV's stand-in with none in): what the asleep
    /// placements' checks after the walk measure from again (openspec
    /// AS.83, [`ScriptWorld::wakes`]).
    pub(crate) activity_from: Vec<Vec3>,
    /// The bodies the embedder keeps in its seam for actors (openspec
    /// AH.25: the TV's heroes and awake creatures in Avian's world, as the
    /// headless runs keep theirs in parry's), each actor's handle and body:
    /// what an engine maps an actor and a body by.
    pub embodied: Vec<(Handle, guac_motion::seam::BodyId)>,
    /// The heroes off their scripts (openspec AH.151): AG.16's controller is
    /// the one writer of each one's walk and facing, so a script's write to
    /// either is dropped ([`ScriptWorld::hand_back_walk`]).
    pub(crate) walking_alone: std::collections::HashSet<Handle>,
    /// What scripts wrote to those, by the placement that wrote it.
    pub(crate) written: std::collections::BTreeMap<(Handle, HeroWrite), u32>,
    /// The sounds part's book (openspec AR.9): a start of a sound info it
    /// gives audio for plays it, by the archive stem of the unit the
    /// sound's file links as.
    pub(crate) sounds: Option<Arc<guac_sound::SoundBook>>,
    pub(crate) sound_stems: HashMap<u16, String>,
    /// The sounds playing, in the order started ([`crate::sounds`]).
    pub(crate) plays: Vec<crate::sounds::SoundPlay>,
    /// The engine frames this run has made, which plays are stamped by.
    pub(crate) sound_clock: u64,
    pub(crate) sounds_started: usize,
    /// The last plays ended and the last starts refused, newest last.
    pub(crate) sound_ended: std::collections::VecDeque<crate::sounds::SoundEnded>,
    pub(crate) sound_refused: std::collections::VecDeque<crate::sounds::SoundRefused>,
    /// What scripts wrote to a sound's members (openspec AR.10), by the
    /// sound info and member: later plays start with it, the playing ones
    /// follow it.
    pub(crate) sound_written: HashMap<(Obj, crate::sound_members::SoundField), Value>,
    /// What scripts wrote to a playing copy's members, by its sequence: the
    /// copy's own, over its sound's.
    pub(crate) copy_written: HashMap<(u32, crate::sound_members::SoundField), Value>,
    /// The sound system's members as written; one never written reads its
    /// start ([`crate::sound_members::SystemField::start`]).
    pub(crate) sound_system: HashMap<crate::sound_members::SystemField, Value>,
    /// The level's music placements [`hold_music`] holds until the level's
    /// sounds are read (openspec AR.23), by handle.
    pub(crate) music_held: Vec<Handle>,
    /// How many of those were let go to run silent, their sounds not coming
    /// (openspec AR.30, [`Music::Silent`]): what `/api/_test/tv` says of it.
    pub(crate) music_silent: usize,
    /// The templates whose clones the engine walks as creatures (openspec
    /// AS.54, [`ScriptWorld::walk_encounter`]): what a level's spawners
    /// clone.
    pub(crate) walk_clones: std::collections::HashSet<Obj>,
    /// The level's intro held until a hero's minion is in (openspec AS.47,
    /// [`hold_intro`]), by handle.
    pub(crate) intro_held: Option<Handle>,
    /// The actors the engine walks as a scene walks them, under a `move
    /// to` with no creature's body (openspec AS.52,
    /// [`crate::script_engine::SceneWalk`]), by handle.
    pub(crate) scene_walks: HashMap<Handle, crate::script_engine::SceneWalk>,
    /// What the static member being read is read under ([`Host::told`]):
    /// the running placement, or the object a tell's block tells.
    pub(crate) told: Value,
    /// The original's interface the scripts write (openspec AT.9), when
    /// the run's install loaded it ([`crate::interface_host`]).
    pub(crate) interface: Option<guac_interface::Keeper>,
    /// The warps owed, each landing once its delay is out (openspec AS.96,
    /// [`crate::interiors::answer`]; stand-in ledger: `warp`).
    pub(crate) warps: Vec<crate::interiors::Warp>,
    /// The warps landed, in order.
    pub landed: Vec<crate::interiors::Landed>,
    /// This run, unique on the TV: a player holding a play of an earlier
    /// run's stops it.
    run: u64,
}

/// Each world's [`ScriptWorld::run`], and each play's serial.
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A fresh number, unique on the TV.
pub(crate) fn fresh_serial() -> u64 {
    RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A level change a script asked for (openspec AS.15): the level's archive,
/// the request's text and what was written.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelChange {
    pub name: String,
    pub text: String,
    pub value: Value,
}

/// The movie a level end queued (openspec AS.19: `Game - Level End - Play
/// Movie`'s, chapter 1's FMV02), by the disc's name for it (`fmv02`), and
/// how far its play has got.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelEndMovie {
    pub name: String,
    pub state: MovieState,
}

/// Where a level end's movie stands: queued for the TV, playing (its
/// conversion included), or over and why.
#[derive(Debug, Clone, PartialEq)]
pub enum MovieState {
    Queued,
    Playing,
    Over(MovieEnd),
}

/// How a level end's movie ended.
#[derive(Debug, Clone, PartialEq)]
pub enum MovieEnd {
    /// Played to its end.
    Finished,
    /// Ended early by the original's skip (△ or Start).
    Skipped,
    /// No player plays it (a headless run): over as it's queued.
    Unplayed,
    /// It wouldn't convert or play, and why: the level goes on as if it had.
    Failed(String),
}

/// The level end the TV wrote to `system.load level` in the Pause
/// Controller's place (openspec AS.15, [`crate::level_end`]): its string,
/// by the name its file gives it (`Level_214`), and why it changed no level
/// yet, if it hasn't. One that can't be read is held as a level change is
/// and reported by name, never read as a stream (found at the AS.15
/// review: a first run's chapter 1 has no interface's model, so its end
/// went nowhere and said nothing).
#[derive(Debug, Clone, PartialEq)]
pub struct LevelEnd {
    pub value: Value,
    pub string: Option<String>,
    /// The level it names as the switch resolves it where its text names
    /// none (the village, openspec AU.27: [`crate::level_switch`]).
    pub resolved: Option<String>,
    pub unread: Option<Unread>,
}

/// Why a level end changed no level.
#[derive(Debug, Clone, PartialEq)]
pub enum Unread {
    /// Its text doesn't read: the run has no interface's model, its part
    /// landing after the level's load as a first run's does (AT.26). The TV
    /// reads it off the part once it's in.
    NoText,
    /// The disc's level list didn't read; the TV reads it again.
    NoList,
    /// Its text names no level on the disc (empty for no text).
    NotALevel(String),
}

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unread::NoText => f.write_str(
                "its text doesn't read: the run has no interface's model (the part not in at the level's load)",
            ),
            Unread::NoList => f.write_str("the disc's level list didn't read"),
            Unread::NotALevel(n) if n.is_empty() => f.write_str("its text is empty"),
            Unread::NotALevel(n) => write!(f, "{n} is no level on the disc"),
        }
    }
}

/// How many requests that changed no level a world keeps for its report.
const STREAMS_KEPT: usize = 8;

/// The level archive a request's text names (openspec AS.15): the text
/// lower-cased, a `level/` folder and a `.bld` taken off. The original's
/// level strings read as their archives' names in English
/// (`Level_214_Tutotial_2`), a stream's as a path (`debug/debug`); none
/// for an empty text.
pub fn level_name(text: &str) -> Option<String> {
    let t = text.trim().to_ascii_lowercase();
    let t = t.strip_prefix("level/").unwrap_or(&t);
    let t = t.strip_suffix(".bld").unwrap_or(t);
    (!t.is_empty()).then(|| t.to_owned())
}

/// A script's write to a hero off his scripts (openspec AH.151): to his
/// walk (`actorinfo.velocity`, by a member or a displace) or to his facing
/// (a member or a `turn to`), which the host drops, or to his place (a
/// `teleport to`, a displace, a measurement, an attach), which it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeroWrite {
    Walk,
    Facing,
    Place,
}

/// A palette slot's animation info as its file holds it (openspec AH.49;
/// `guac_ir::palette::SlotEntry::Clip`'s flags): what `animationinfo.type`
/// reads and whether its layered bit starts set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClipInfo {
    /// `animationinfo.type`: 0 a base clip, 1 partial (its tracks drive
    /// some joints), 2 additive; additive wins over partial, as the
    /// original's getter reads the two flags.
    pub kind: u8,
    /// The wrapper's layered flag as stored: the layered bit's start.
    pub layered: bool,
}

impl ClipInfo {
    pub const BASE: u8 = 0;
    pub const PARTIAL: u8 = 1;
    pub const ADDITIVE: u8 = 2;

    /// The flags a palette entry's clip carries.
    pub fn of(additive: bool, partial: bool, layered: bool) -> Self {
        let kind = if additive {
            Self::ADDITIVE
        } else if partial {
            Self::PARTIAL
        } else {
            Self::BASE
        };
        Self { kind, layered }
    }
}

/// Each slot's animation info in `palette`, a base clip with no layered bit
/// where a slot holds no clip.
pub fn clip_infos(palette: &guac_ir::palette::Palette) -> Vec<ClipInfo> {
    let mut out = vec![ClipInfo::default(); usize::from(palette.slots)];
    for e in &palette.entries {
        use guac_ir::palette::SlotEntry;
        if let SlotEntry::Clip {
            additive,
            partial,
            layered,
            ..
        }
        | SlotEntry::Blocks {
            additive,
            partial,
            layered,
            ..
        } = &e.entry
            && let Some(i) = out.get_mut(usize::from(e.slot))
        {
            *i = ClipInfo::of(*additive, *partial, *layered);
        }
    }
    out
}

/// What the scripts wrote on a palette slot's clip (openspec AH.49), each
/// `None` until written: the layered bit (`animationinfo.is layered`, which
/// only a clip takes), whether it loops (`sequence.looping`), its weight
/// (`animationinfo.blend percent`) and whether a play of it while it plays
/// starts it again (`sequence.allow restart`, AH.184). A sequence started
/// from the clip starts with them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipState {
    pub layered: Option<bool>,
    pub looping: Option<bool>,
    pub blend: Option<f32>,
    pub allow_restart: Option<bool>,
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
            light_templates: files.lights.clone(),
            emitters: files.emitters.clone(),
            points: files.points.clone(),
            point_turns: files.turns.clone(),
            controllers: [objects[0], objects[1]],
            physical: [objects[2], objects[3], objects[4], objects[5]],
            portal: objects[6],
            viewport,
            view: crate::viewport::Viewport::default(),
            view_until_join: false,
            view_asks: Default::default(),
            objs,
            sequences: Vec::new(),
            restarts: Vec::new(),
            palettes: HashMap::new(),
            clip_info: HashMap::new(),
            clip_state: HashMap::new(),
            hero_templates: HashMap::new(),
            nodes: HashMap::new(),
            pads: [Pad::default(); 2],
            pads_before: [Pad::default(); 2],
            normalized: 0.0,
            ray: Ray::default(),
            kept_points: HashMap::new(),
            kept_via: HashMap::new(),
            waypoints_made: Vec::new(),
            scripted: Vec::new(),
            revived: Vec::new(),
            time_scale: FULL_SPEED,
            load_level: Value::Null,
            requests: Vec::new(),
            levels: None,
            level_change: None,
            streams: Default::default(),
            skipped_cutscene: None,
            skip_cutscenes: false,
            level_end_asked: false,
            level_end_movie: None,
            plays_movies: false,
            sequence: Default::default(),
            level_end: None,
            achievements: Vec::new(),
            save: Default::default(),
            inert: Default::default(),
            woken: Vec::new(),
            fired: Vec::new(),
            activity_from: Vec::new(),
            embodied: Vec::new(),
            walking_alone: Default::default(),
            written: Default::default(),
            sounds: None,
            sound_stems: HashMap::new(),
            plays: Vec::new(),
            sound_clock: 0,
            sounds_started: 0,
            sound_ended: Default::default(),
            sound_refused: Default::default(),
            sound_written: HashMap::new(),
            copy_written: HashMap::new(),
            sound_system: HashMap::new(),
            music_held: Vec::new(),
            music_silent: 0,
            walk_clones: Default::default(),
            intro_held: None,
            scene_walks: HashMap::new(),
            told: Value::Null,
            interface: None,
            warps: Vec::new(),
            landed: Vec::new(),
            run: fresh_serial(),
        }
    }

    /// The sounds the scripts play from here on (openspec AR.9): `book`,
    /// each unit's archive stem by `stems`.
    pub fn set_sounds(&mut self, book: Arc<guac_sound::SoundBook>, stems: HashMap<u16, String>) {
        self.sounds = Some(book);
        self.sound_stems = stems;
    }

    /// Whether the book holds the sounds of the file linked as `unit` (its
    /// archive's part read): what the level's music waits on (openspec
    /// AR.23, [`hold_music`]).
    pub fn holds_sounds_of(&self, unit: u16) -> bool {
        let (Some(book), Some(stem)) = (self.sounds.as_deref(), self.sound_stems.get(&unit)) else {
            return false;
        };
        book.file(stem).is_some()
    }

    /// How many of the level's music placements [`hold_music`] holds now,
    /// waiting for the level's sounds (openspec AR.23).
    pub fn music_held(&self) -> usize {
        self.music_held.len()
    }

    /// How many held music placements were let go to run silent, the
    /// level's sounds not coming (openspec AR.30).
    pub fn music_silent(&self) -> usize {
        self.music_silent
    }

    /// The book the scripts' sounds come from, if one was set.
    pub fn sound_book(&self) -> Option<&guac_sound::SoundBook> {
        self.sounds.as_deref()
    }

    /// This run's number, unique on the TV.
    pub fn run(&self) -> u64 {
        self.run
    }

    /// The sounds playing, in the order started.
    pub fn plays(&self) -> &[crate::sounds::SoundPlay] {
        &self.plays
    }

    /// The engine frames the run has made.
    pub fn sound_clock(&self) -> u64 {
        self.sound_clock
    }

    /// Plays started, every one since the run began.
    pub fn sounds_started(&self) -> usize {
        self.sounds_started
    }

    /// The last plays that ended, newest last ([`crate::sounds::RECENT`]).
    pub fn sounds_ended(&self) -> impl ExactSizeIterator<Item = &crate::sounds::SoundEnded> {
        self.sound_ended.iter()
    }

    /// The last starts that played nothing, newest last.
    pub fn sounds_refused(&self) -> impl ExactSizeIterator<Item = &crate::sounds::SoundRefused> {
        self.sound_refused.iter()
    }

    /// `hero`'s walk and facing handed back to AG.16's controller for good
    /// (openspec AH.151; stand-in ledger: `walker`): his scripts stopped, so
    /// from here on a script's write to his walk or his facing is dropped,
    /// counted by its writer ([`ScriptWorld::hero_writes`]). The
    /// mover update is why: his minion's `Movement - Face and Move` no longer
    /// keeps the velocity it holds for him, and each step it hands the engine
    /// that velocity, slowed to nothing, over his walk (`movementUpdate.ai`
    /// #608-#610). A write to his place still moves him (a level's `teleport
    /// to`, which sets him down facing as it says and stops his walk, a
    /// displace, a measurement), as it would move him in the original, where
    /// his scripts don't stop. His scripts joining him again
    /// ([`ScriptWorld::hero_template`], the join's) gives them both back.
    pub fn hand_back_walk(&mut self, hero: Handle) {
        self.walking_alone.insert(hero);
    }

    /// Whether `hero`'s walk is the controller's alone
    /// ([`ScriptWorld::hand_back_walk`]).
    pub fn walks_alone(&self, hero: Handle) -> bool {
        self.walking_alone.contains(&hero)
    }

    /// What scripts wrote to a hero off his scripts
    /// ([`ScriptWorld::hand_back_walk`]): each by the placement that wrote it
    /// (its handle) and what it wrote, and how many; his walk and facing
    /// dropped, his place kept.
    pub fn hero_writes(&self) -> &std::collections::BTreeMap<(Handle, HeroWrite), u32> {
        &self.written
    }

    /// Whether a write of `what` to `hero` by placement `by` is dropped, his
    /// walk being the controller's alone: counted, the first of each logged.
    pub(crate) fn drops(&mut self, by: Handle, hero: Handle, what: HeroWrite) -> bool {
        if !self.walking_alone.contains(&hero) {
            return false;
        }
        self.count_write(by, what, "dropped: the controller walks him");
        true
    }

    /// A write to `hero`'s place by placement `by`, kept: counted while he's
    /// off his scripts.
    pub(crate) fn writes_place(&mut self, by: Handle, hero: Handle) {
        if self.walking_alone.contains(&hero) {
            self.count_write(
                by,
                HeroWrite::Place,
                "kept: his place is the level's to set",
            );
        }
    }

    /// One more of `by`'s writes of `what` counted, its first logged.
    fn count_write(&mut self, by: Handle, what: HeroWrite, then: &str) {
        let n = self.written.entry((by, what)).or_default();
        if *n == 0 {
            let writer = self
                .actors
                .get(&by)
                .and_then(|a| a.obj.or(a.template))
                .and_then(|o| self.templates.get(&o))
                .map_or("", |p| p.name.as_str());
            info!(
                writer,
                ?what,
                then,
                "a script wrote to a hero off his scripts"
            );
        }
        *n += 1;
    }

    /// The viewport as the camera's scripts leave it (openspec AG.28).
    pub fn viewport(&self) -> &crate::viewport::Viewport {
        &self.view
    }

    /// The viewport, to write as a camera script's write would: how the
    /// TV's tests send it wandering (openspec AH.227).
    pub fn viewport_mut(&mut self) -> &mut crate::viewport::Viewport {
        &mut self.view
    }

    /// The view handed back to the TV for good: the camera's scripts
    /// stopped in play, and from here on the TV's own camera stands for the
    /// viewport (openspec AG.28). A hand-back until a join
    /// ([`ScriptWorld::hand_back_view_until_a_join`]) becomes this one.
    pub fn hand_back_view(&mut self) {
        self.view.source = crate::viewport::Source::HandedBack;
        self.view_until_join = false;
    }

    /// The view handed back to the TV while no hero's scripts step a camera
    /// target (openspec AG.190: the hero fell back in play, off the player
    /// loop's joined run), until the next hero's scripts join the run
    /// (AG.211): the join's activation makes his camera target afresh on
    /// his minion and the camera's frame never stopped, so from there their
    /// next write to the viewport takes it back
    /// ([`ScriptWorld::hero_template`]). The scripts' camera stopping keeps
    /// it for good; a view the scripts don't have is left as it is.
    pub fn hand_back_view_until_a_join(&mut self) {
        if self.view.scripted() {
            self.view.source = crate::viewport::Source::HandedBack;
            self.view_until_join = true;
        }
    }

    /// Whether the view is handed back until the next join
    /// ([`ScriptWorld::hand_back_view_until_a_join`]), not for good.
    pub fn view_back_at_a_join(&self) -> bool {
        self.view_until_join
    }

    /// The on-screen asks, by op, answered off the camera's scripts' view
    /// while they drive it (openspec AH.150: the asks read as gating play,
    /// [`crate::on_screen::answered`]); the rest read on screen (AH.20).
    pub fn answer_from_view(&mut self, asks: impl IntoIterator<Item = guac_script::OpId>) {
        self.view_asks = asks.into_iter().collect();
    }

    /// Whether the ask at `op` is answered off the camera's scripts' view
    /// ([`ScriptWorld::answer_from_view`]).
    pub fn answers_from_view(&self, op: guac_script::OpId) -> bool {
        self.view_asks.contains(&op)
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
        self.activity_from = heroes.to_vec();
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
        for &(h, active) in &out {
            if active && !self.fired.contains(&h) {
                self.fired.push(h);
            }
        }
        out
    }

    /// The asleep placements' checks after the walk (openspec AS.83,
    /// `guac_script`'s crate docs, The tick): of `asleep`, those the
    /// activity rule finds awake now, measured from where this step's pass
    /// measured ([`ScriptWorld::activity_at`]), each taken as awake and as
    /// fired. What a script's write of a range or a buffer during the walk,
    /// or its move of one into range, wakes the step it does it.
    pub fn wakes(&mut self, asleep: &[Handle]) -> Vec<Handle> {
        let mut out = Vec::new();
        for &h in asleep {
            let Some(a) = self.actors.get_mut(&h) else {
                continue;
            };
            if !a.managed || a.removed || a.active {
                continue;
            }
            let d = self
                .activity_from
                .iter()
                .map(|p| (*p - a.position).length() * crate::script_engine::FEET_PER_METRE)
                .reduce(f32::min);
            if awake(a.range, a.buffer, false, d) {
                a.active = true;
                if !self.fired.contains(&h) {
                    self.fired.push(h);
                }
                out.push(h);
            }
        }
        out
    }

    /// The level placement named `name` in the file linked as `unit`, by
    /// its handle.
    pub fn placed_in(&self, unit: u16, name: &str) -> Option<Handle> {
        let mut found: Vec<(&Obj, &Handle)> = self
            .objects
            .iter()
            .filter(|(o, _)| o.unit == unit)
            .filter(|(o, _)| self.templates.get(o).is_some_and(|p| p.name == name))
            .collect();
        found.sort_unstable();
        found.first().map(|(_, h)| **h)
    }

    /// The triggers of the file linked as `unit` that have fired, by name in
    /// the order they first woke ([`ScriptWorld::fired`]): each a placement
    /// with an activation range whose name a trigger's goes by (AH.4's
    /// rule, [`is_trigger`]), so the list reads as the oracle's census does.
    pub fn fired_triggers(&self, unit: u16) -> Vec<String> {
        self.fired
            .iter()
            .filter_map(|h| {
                let p = self
                    .actors
                    .get(h)?
                    .obj
                    .and_then(|o| self.templates.get(&o))?;
                (p.obj.unit == unit && p.range > 0.0 && is_trigger(&p.name)).then(|| p.name.clone())
            })
            .collect()
    }

    /// Every actor the engine walks as a creature ([`ScriptWorld::walk`]),
    /// taken off or asleep too, in handle order.
    pub fn creatures(&self) -> Vec<Handle> {
        let mut out: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, a)| a.walker.is_some())
            .map(|(h, _)| *h)
            .collect();
        out.sort_unstable();
        out
    }

    /// The hub's people ([`HUB_PEOPLE`], openspec AU.28): each placement
    /// whose script is one of theirs, by its actor, unwalked, in handle
    /// order; whether its script runs is the caller's to check.
    pub fn people(&self) -> Vec<Handle> {
        let mut out: Vec<Handle> = self
            .actors
            .iter()
            .filter(|(_, a)| {
                a.walker.is_none()
                    && a.obj
                        .and_then(|o| self.templates.get(&o))
                        .and_then(|p| p.script.as_deref())
                        .is_some_and(is_hub_person)
            })
            .map(|(h, _)| *h)
            .collect();
        out.sort_unstable();
        out
    }

    /// Makes each of the first fight's placements in the file linked as
    /// `unit` with movement numbers ([`FIRST_FIGHT`]) a creature the engine
    /// moves ([`ScriptWorld::walk`], openspec AH.18): what the headless
    /// runs and the TV's fight listing (AH.25) do once the run is made.
    pub fn walk_first_fight(&mut self, unit: u16) -> usize {
        let walkers: Vec<Handle> = FIRST_FIGHT
            .iter()
            .filter_map(|n| self.placed_in(unit, n))
            .filter(|h| {
                self.actors
                    .get(h)
                    .and_then(|a| a.obj)
                    .and_then(|o| self.templates.get(&o))
                    .is_some_and(|p| p.physics.is_some())
            })
            .collect();
        for &h in &walkers {
            self.walk(h);
        }
        walkers.len()
    }

    /// Makes a level's first encounter creatures the engine moves (openspec
    /// AS.54, [`Encounter`]): each of its placements in the file linked as
    /// `unit` with movement numbers walked now, as chapter 1's first fight's
    /// are, and each clone a script makes later of one of its templates
    /// walked as it's made. The placements walked and the templates found.
    pub fn walk_encounter(&mut self, unit: u16, encounter: &Encounter) -> (usize, usize) {
        let walkers: Vec<Handle> = encounter
            .placements
            .iter()
            .filter_map(|n| self.placed_in(unit, n))
            .filter(|h| {
                self.actors
                    .get(h)
                    .and_then(|a| a.obj)
                    .and_then(|o| self.templates.get(&o))
                    .is_some_and(|p| p.physics.is_some())
            })
            .collect();
        for &h in &walkers {
            self.walk(h);
        }
        let templates: Vec<Obj> = self
            .templates
            .iter()
            .filter(|(o, p)| {
                o.unit == unit && p.physics.is_some() && encounter.clones.contains(&p.name.as_str())
            })
            .map(|(o, _)| *o)
            .collect();
        let found = templates.len();
        self.walk_clones.extend(templates);
        (walkers.len(), found)
    }

    /// Makes each placement of the file linked as `unit` that runs one of
    /// [`GOAT_SCRIPTS`] and has movement numbers a creature the engine moves
    /// ([`ScriptWorld::walk`], openspec AG.177): the jumping goats run their
    /// waypoints by `move to` before the jump that opens the third rock.
    pub fn walk_goats(&mut self, unit: u16) -> usize {
        let mut walkers: Vec<Handle> = self
            .objects
            .iter()
            .filter(|(o, _)| o.unit == unit)
            .filter(|(o, _)| {
                self.templates.get(o).is_some_and(|p| {
                    p.physics.is_some()
                        && p.script
                            .as_deref()
                            .is_some_and(|s| guac_extract::scripts::listed(&GOAT_SCRIPTS, s))
                })
            })
            .map(|(_, h)| *h)
            .collect();
        walkers.sort_unstable();
        for &h in &walkers {
            self.walk(h);
        }
        walkers.len()
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

    /// The end of a step (openspec AS.15): a request a script wrote to
    /// `system.load level` is taken. One whose text names a level on the
    /// disc ([`level_name`]) is a level change: it's held, so the member
    /// reads it while the level loads, as the original's does, until this
    /// run gives way to that level's. The TV's level end
    /// ([`ScriptWorld::ask_level_end`]) is held the same when it can't be
    /// read as one ([`Unread`]), reported and read again
    /// ([`ScriptWorld::read_level_end`]). Any other (a stream: a menu, a
    /// character's, the objectives screen; one whose text doesn't read)
    /// isn't loaded and reads none from the next step on, as one loaded at
    /// once (stand-in ledger: `level-request`).
    pub fn end_step(&mut self) {
        let load = self.load_level;
        if load == Value::Null
            || self.level_change.as_ref().is_some_and(|c| c.value == load)
            || self
                .level_end
                .as_ref()
                .is_some_and(|e| e.value == load && e.unread.is_some())
        {
            return;
        }
        self.requests.push(load);
        let text = self.text_of(load);
        let ending = self.level_end.as_ref().is_some_and(|e| e.value == load);
        let resolved = self
            .level_end
            .as_ref()
            .filter(|e| e.value == load)
            .and_then(|e| e.resolved.clone());
        let read = match resolved {
            Some(name) => self.on_disc(name),
            None => self.level_named(text.as_deref()),
        };
        match read {
            Ok(name) => {
                info!(%name, ?text, "a script asked for a level: it loads, the run holds its request");
                self.level_change = Some(LevelChange {
                    name,
                    text: text.unwrap_or_default(),
                    value: load,
                });
            }
            Err(why) if ending => {
                let string = self.level_end.as_ref().and_then(|e| e.string.clone());
                warn!(?string, reason = %why, "the level end changes no level yet: held, read again");
                if let Some(e) = &mut self.level_end {
                    e.unread = Some(why);
                }
            }
            Err(_) => {
                info!(
                    value = ?load,
                    ?text,
                    "a script asked for a stream (or a level not on the disc): read as loaded, nothing loaded"
                );
                if self.streams.len() == STREAMS_KEPT {
                    self.streams.pop_front();
                }
                self.streams.push_back(text);
                self.load_level = Value::Null;
            }
        }
    }

    /// The level on the disc a request's `text` names, or why none.
    fn level_named(&self, text: Option<&str>) -> Result<String, Unread> {
        let text = text.ok_or(Unread::NoText)?;
        let name = level_name(text).ok_or_else(|| Unread::NotALevel(String::new()))?;
        self.on_disc(name)
    }

    /// `name`, when the disc's level list holds it, or why not.
    fn on_disc(&self, name: String) -> Result<String, Unread> {
        match self.levels.as_deref() {
            Some(l) if !l.is_empty() => l
                .contains(&name)
                .then_some(name.clone())
                .ok_or(Unread::NotALevel(name)),
            _ => Err(Unread::NoList),
        }
    }

    /// The TV's level end read again (openspec AS.15): `text`, the level end's
    /// string read off the interface's part once it's in, and `levels`, the
    /// disc's list read again, each when given. Named a level, it's a held
    /// level change from here as one the step read; otherwise why stays
    /// reported. Nothing without an unread level end.
    pub fn read_level_end(
        &mut self,
        text: Option<String>,
        levels: Option<Arc<std::collections::HashSet<String>>>,
    ) {
        if let Some(l) = levels {
            self.levels = Some(l);
        }
        let Some(end) = self.level_end.clone().filter(|e| e.unread.is_some()) else {
            return;
        };
        let text = text.or_else(|| self.text_of(end.value));
        let read = match end.resolved.clone() {
            Some(name) => self.on_disc(name),
            None => self.level_named(text.as_deref()),
        };
        match read {
            Ok(name) => {
                info!(%name, string = ?end.string, ?text, "the level end read: it loads, the run holds its request");
                self.level_change = Some(LevelChange {
                    name,
                    text: text.unwrap_or_default(),
                    value: end.value,
                });
                if let Some(e) = &mut self.level_end {
                    e.unread = None;
                }
            }
            Err(why) => {
                if end.unread.as_ref() != Some(&why) {
                    warn!(string = ?end.string, reason = %why, "the level end still changes no level");
                }
                if let Some(e) = &mut self.level_end {
                    e.unread = Some(why);
                }
            }
        }
    }

    /// The placement under `h` woken as a script wakes one: its activation
    /// range and deactivation buffer set to 0 (what `Rope_around_Post(2)`'s
    /// last behaviour writes on `Temp_Level_Ender`, openspec AS.15's dev
    /// entry). False when `h` is no actor.
    pub fn wake(&mut self, h: Handle) -> bool {
        let Some(a) = self.actors.get_mut(&h) else {
            return false;
        };
        a.range = 0.0;
        a.buffer = 0.0;
        true
    }

    /// The disc's levels by archive name, which a request's text is held
    /// against ([`ScriptWorld::end_step`]).
    pub fn set_levels(&mut self, levels: Arc<std::collections::HashSet<String>>) {
        self.levels = Some(levels);
    }

    /// The level change a script asked for, held while it loads.
    pub fn level_change(&self) -> Option<&LevelChange> {
        self.level_change.as_ref()
    }

    /// `v` written to `system.load level`, as a script's write is: what the
    /// TV's level end does in the Pause Controller's place
    /// ([`crate::level_end`]).
    pub fn ask_level(&mut self, v: Value) {
        self.load_level = v;
    }

    /// The TV's level end ([`crate::level_end`]): `v` written to `system.load
    /// level` as [`ScriptWorld::ask_level`] writes it, kept as the level end
    /// with `string`, the name its file gives the string (`Level_214`), so
    /// one that can't be read is held and reported ([`LevelEnd`]).
    pub fn ask_level_end(&mut self, v: Value, string: Option<String>) {
        self.level_end = Some(LevelEnd {
            value: v,
            string,
            resolved: None,
            unread: None,
        });
        self.ask_level(v);
    }

    /// The TV's level end resolved to `level`, an archive, as the switch
    /// resolves one whose text names none (the village, openspec AU.27:
    /// [`crate::level_switch`]); the step's end asks it by that name.
    /// Nothing without a level end.
    pub fn resolve_level_end(&mut self, level: String) {
        if let Some(e) = &mut self.level_end {
            e.resolved = Some(level);
        }
    }

    /// The TV's level end, once written.
    pub fn level_end(&self) -> Option<&LevelEnd> {
        self.level_end.as_ref()
    }

    /// The movie the level end queued, once [`crate::level_end`] has read
    /// it (openspec AS.19).
    pub fn level_end_movie(&self) -> Option<&LevelEndMovie> {
        self.level_end_movie.as_ref()
    }

    /// `name` queued as a level end queues its movie
    /// ([`crate::level_end`]): what the TV's player tests start from.
    pub fn queue_movie(&mut self, name: &str) {
        self.level_end_movie = Some(LevelEndMovie {
            name: name.to_owned(),
            state: MovieState::Queued,
        });
    }

    /// The queued movie taken up by the player: playing (or converting).
    pub fn movie_playing(&mut self) {
        if let Some(m) = &mut self.level_end_movie
            && m.state == MovieState::Queued
        {
            m.state = MovieState::Playing;
        }
    }

    /// The level end's movie over, `end` saying how: the level end asks its
    /// level at its next step.
    pub fn movie_over(&mut self, end: MovieEnd) {
        if let Some(m) = &mut self.level_end_movie
            && !matches!(m.state, MovieState::Over(_))
        {
            m.state = MovieState::Over(end);
        }
    }

    /// Whether the disc's level list is in (some level on it).
    pub fn has_levels(&self) -> bool {
        self.levels.as_deref().is_some_and(|l| !l.is_empty())
    }

    /// A string's text as the run's interface reads it (a file's
    /// `StringInfo` in English, or one the converter made): none without
    /// the interface's model, or for anything else.
    pub fn text_of(&self, v: Value) -> Option<String> {
        let s = self.interface.as_ref()?.model().string(v)?;
        Some(String::from_utf16_lossy(&s.units))
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

    /// Each palette slot's animation info for `template`'s clones and
    /// itself, as its file holds it (openspec AH.49).
    pub fn set_clip_info(&mut self, template: Obj, info: Vec<ClipInfo>) {
        self.clip_info.insert(template, info);
    }

    /// `palette` for `template`'s clones and itself: each slot's clip
    /// length ([`ScriptWorld::set_palette`]) and its animation info
    /// ([`ScriptWorld::set_clip_info`]).
    pub fn set_palette_of(&mut self, template: Obj, palette: &guac_ir::palette::Palette) {
        self.set_palette(template, crate::level_scripts::lengths(palette));
        self.set_clip_info(template, clip_infos(palette));
    }

    /// Slot `slot` of `template`'s palette's animation info as its file
    /// holds it: a base clip with no layered bit where none was given.
    pub fn clip_info(&self, template: Obj, slot: u32) -> ClipInfo {
        self.clip_info
            .get(&template)
            .and_then(|i| i.get(slot as usize))
            .copied()
            .unwrap_or_default()
    }

    /// An actor set down at `at` facing `facing`, at rest, as a recorded
    /// state has it (a scripted test's or the dev entry's). False for no
    /// such actor.
    pub fn place(&mut self, h: Handle, at: Vec3, facing: f32) -> bool {
        self.place_moving(h, at, facing, Vec3::ZERO)
    }

    /// Every point the scripts have remembered through a reference to
    /// `actor` set to `at`: a recorded state's follow actor at rest, its
    /// intent, velocity and target points together (openspec AG.28's
    /// verify; `Camera - Update Player Cam Target` keeps those three apart
    /// on one clone). Returns how many were set.
    pub fn rest_remembered(&mut self, actor: Handle, at: Vec3) -> usize {
        let mut n = 0;
        for ((h, _), p) in &mut self.kept_via {
            if *h == actor {
                *p = at;
                n += 1;
            }
        }
        n
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

    /// `actor` hung on node `node` of `owner` with no offset, as a script's
    /// `abstractplacement.attach node` hangs it: placed there once the hung
    /// are next placed ([`ScriptWorld::place_hung`]). False for no such
    /// actor.
    pub fn hang(&mut self, actor: Handle, owner: Handle, node: u32) -> bool {
        let on = self.objs.value(Object::Node(owner, node));
        match self.actors.get_mut(&actor) {
            Some(a) => {
                a.attach = on;
                a.hung = Vec3::ZERO;
                true
            }
            None => false,
        }
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
    /// numbers from the template's file. His scripts joining him so, his
    /// walk and facing are theirs again (openspec AH.151), and a view handed
    /// back at a fall-back until a join is the camera's scripts' to take at
    /// their next write (AG.211).
    pub fn hero_template(&mut self, template: Obj, hero: Handle) {
        self.hero_templates.insert(template, hero);
        self.walking_alone.remove(&hero);
        if self.view_until_join {
            self.view_until_join = false;
            self.view.source = crate::viewport::Source::Engine;
            info!("a hero joins: the camera's scripts take the view at their next write");
        }
    }

    /// One frame of `dt` seconds for what the engine runs between the
    /// scripts' ticks: the sequences playing and the actors they set going
    /// moving.
    pub fn advance(&mut self, dt: f32, engine: &mut dyn Engine) {
        for s in &mut self.sequences {
            s.advance(dt);
        }
        self.move_actors(dt, engine);
        crate::interiors::land(self, dt, engine);
        self.advance_sounds(engine);
    }

    /// The actor under `h`, if the scripts reach one.
    pub fn actor(&self, h: Handle) -> Option<&Actor> {
        self.actors.get(&h)
    }

    /// `h`'s tint set as a script's write of `placement.tint` leaves it (RGBA,
    /// red in the high byte): a test's, where no script fades one. False when
    /// the scripts reach no actor there.
    pub fn set_tint(&mut self, h: Handle, packed: u32) -> bool {
        match self.actors.get_mut(&h) {
            Some(a) => {
                a.tint = packed;
                true
            }
            None => false,
        }
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
        self.clip_named(s.template)
    }

    /// The palette slot `v` names (a clip of `animationinfo.animation
    /// palette`, as a script holds one): its template's object and the slot.
    pub fn clip_named(&self, v: Value) -> Option<(Obj, u32)> {
        match self.objs.get(self.handle(v)?)? {
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

    /// The sequences a play started again since the run began, by index
    /// into [`ScriptWorld::sequences`], in order (openspec AH.184): a
    /// clip played while its copy on the actor plays is that copy, so the
    /// list of sequences doesn't grow.
    pub fn restarts(&self) -> &[u32] {
        &self.restarts
    }

    /// Where [`ScriptWorld::started_since`] counts from now: the sequences
    /// and the restarts so far.
    pub fn start_count(&self) -> (usize, usize) {
        (self.sequences.len(), self.restarts.len())
    }

    /// The sequences started since `since` (a [`ScriptWorld::start_count`]),
    /// by index, each once: those new since, then those a play started again
    /// (openspec AH.184), in the order started again.
    pub fn started_since(&self, since: (usize, usize)) -> Vec<usize> {
        let from = since.0.min(self.sequences.len());
        let mut out: Vec<usize> = (from..self.sequences.len()).collect();
        for &i in &self.restarts[since.1.min(self.restarts.len())..] {
            if !out.contains(&(i as usize)) {
                out.push(i as usize);
            }
        }
        out
    }

    /// The sequences, to set where a recorded state has them (a replay's
    /// start: openspec AH.49's, each clip at its record's time and rate).
    pub fn sequences_mut(&mut self) -> &mut [Sequence] {
        &mut self.sequences
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

    /// The lights the scripts have lit (openspec AH.65), in handle order:
    /// each clone of a placed light not taken off, where it hangs (its place
    /// as the point-light manager last hung it, raised by its lift, metres)
    /// and its values as the manager last copied them.
    pub fn lit_lights(&self) -> Vec<(Vec3, Light)> {
        let mut lit: Vec<(Handle, Vec3, Light)> = self
            .actors
            .iter()
            .filter(|(_, a)| a.obj.is_none() && !a.removed)
            .filter_map(|(h, a)| {
                let l = a.light?;
                let up = Vec3::Y * l.lift * crate::script_engine::METRES_PER_FOOT;
                Some((*h, a.position + up, l))
            })
            .collect();
        lit.sort_unstable_by_key(|k| k.0);
        lit.into_iter().map(|(_, at, l)| (at, l)).collect()
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
        if self.to_interface(at) {
            return self.interface_get(at);
        }
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
            // The last unlocked, 0 before any (nothing on the disc reads it).
            Member::Achievement => Value::Int(self.achievements.last().copied().unwrap_or(0)),
            Member::SaveState => Value::Int(self.save.get()),
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
        if self.to_interface(at) {
            return self.interface_elements(at);
        }
        match self.members.get(&at.member).copied() {
            Some(m) => self.elements_of(at, m, engine),
            None => Err(HostFault::Unmodelled),
        }
    }

    pub(crate) fn set(
        &mut self,
        at: &Access,
        v: Value,
        engine: &mut dyn Engine,
    ) -> Result<(), HostFault> {
        if self.to_interface(at) {
            return self.interface_set(at, v);
        }
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        match (m, v) {
            (Member::TimeScale, Value::Float(f)) => self.time_scale = f,
            (Member::LoadLevel, v) => self.load_level = v,
            // An unlock (openspec AH.220): kept and logged, nothing awarded
            // (stand-in ledger: `achievements`).
            (Member::Achievement, v) => {
                let id = match v {
                    Value::Int(i) => i,
                    other => other.as_f32().ok_or(HostFault::Nothing)? as i32,
                };
                self.achievements.push(id);
                info!(
                    id,
                    "a script unlocked an achievement: kept, nothing awarded until per-profile achievements"
                );
            }
            // A save, load or finalize asked, or the state cleared
            // (openspec AS.38, `crate::story_save`).
            (Member::SaveState, v) => match v {
                Value::Int(i) => self.save.set(i),
                other => self
                    .save
                    .set(other.as_f32().ok_or(HostFault::Nothing)? as i32),
            },
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
        if self.to_interface(at) {
            return self.interface_reset(at);
        }
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
                (Some(v), _) if self.sprite_removed(v) => Answer::Skip,
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
                // Nothing to clone (openspec AG.160: the locomotion clones a
                // state's sound control, which a character whose states
                // play no footsteps leaves none): the block is skipped.
                let Some(template) = first.filter(|t| *t != Value::Null) else {
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
                // A sprite's clone is the interface's (openspec AT.9).
                if let Some(a) = self.spawn_sprite(template, at, facing, call.placement) {
                    return Ok(a);
                }
                Ok(self.spawn(template, at, facing, call.placement, spawn, engine))
            }
            Class::Remove if self.remove_sprite(first) => Ok(Answer::Done),
            Class::Remove => {
                if let Some(k) = target.and_then(|h| self.actors.get_mut(&h)) {
                    k.removed = true;
                }
                Ok(Answer::Done)
            }
            Class::Reset => {
                // A file placement taken off comes back as its file has it
                // (openspec AS.60, a reading: level 214's intro takes the
                // ship the players stand on off at its start and resets it
                // in its SHUTDOWN, as it does the barrels it took off), its
                // script running again from its STARTUP.
                if let Some(h) = target
                    && let Some(o) = self
                        .actors
                        .get(&h)
                        .filter(|k| k.removed)
                        .and_then(|k| k.obj)
                    && let Some(p) = self.templates.get(&o)
                {
                    self.actors.insert(h, actor_of(p));
                    self.revived.push((h, o));
                    return Ok(Answer::Done);
                }
                // A placement's reset of itself (openspec AS.67): the VM
                // sends its script round from its STARTUP, and it stands
                // again as it was made ([`ScriptWorld::as_made`]).
                if let Some(h) = target
                    && h == call.placement
                    && let Some(a) = self.as_made(h)
                {
                    self.actors.insert(h, a);
                    return Ok(Answer::Done);
                }
                if let Some(k) = target.and_then(|h| self.actors.get_mut(&h)) {
                    k.buffer = k.buffer_loaded;
                    // A light the point-light manager took off goes back
                    // on with its template's values (openspec AH.65: the
                    // manager removes a light it returns to its pool and
                    // resets one it takes out; a reset leaving it off would
                    // light nothing after the first request).
                    if let Some(l) = k.template.and_then(|t| self.light_templates.get(&t))
                        && k.light.is_some()
                    {
                        k.light = Some(*l);
                        k.removed = false;
                    }
                }
                Ok(Answer::Done)
            }
            _ => match self.op_hero(call, spawn, engine) {
                Some(r) => r,
                None => Err(HostFault::Unmodelled),
            },
        }
    }

    /// The live actor under `h` as it stood when made, for its reset of
    /// itself (openspec AS.67, a reading: level 214's bomb resets itself
    /// after its burst, and nothing in its script or its spawner's takes it
    /// back to the spawner, which clones no other while it stands): a
    /// file's placement as its file has it, a clone its template's numbers
    /// at the place it was cloned (`abstractplacement.origin`) with its
    /// turn its template's, keeping what makes it this clone (its id, its
    /// buffer and range, its parent, activator and template, its own
    /// clones), its motion and what it hung from gone. Either way one the
    /// engine walks still walks, its body made again ([`ScriptWorld::walk`]
    /// is the embedder's, once). None for one taken off or a template it
    /// can't find.
    pub fn as_made(&self, h: Handle) -> Option<Actor> {
        let k = self.actors.get(&h).filter(|k| !k.removed)?;
        let walker = k.walker.as_ref().map(|_| {
            Box::new(crate::script_engine::Walker {
                body: None,
                aim: guac_motion::steer::Aim::Still,
                along: 0,
            })
        });
        if let Some(p) = k.obj.and_then(|o| self.templates.get(&o)) {
            return Some(Actor {
                clones: k.clones.clone(),
                walker,
                ..actor_of(p)
            });
        }
        let mut a = actor_of(self.templates.get(&k.template?)?);
        a.obj = None;
        a.template = k.template;
        a.parent = k.parent;
        a.activator = k.activator;
        a.id = k.id;
        a.buffer_loaded = k.buffer_loaded;
        a.buffer = k.buffer;
        a.range = k.range;
        a.waypoints = Vec::new();
        a.clones = k.clones.clone();
        a.position = k.origin;
        a.origin = k.origin;
        a.light = k
            .template
            .and_then(|t| self.light_templates.get(&t))
            .copied();
        a.walker = walker;
        Some(a)
    }

    /// A clone of `template` at the place `at` names (its pivot), facing
    /// `facing` when given, else the template's: the hero for a template a
    /// hero stands for, else a new actor (its handle from `spawn`) with the
    /// template's file numbers, or a clone's own when it copies a clone, its
    /// activator `spawner`, the placement whose script spawned it (openspec
    /// AG.60: the fence's cross beam hangs on the fence that spawned it). A
    /// scripted template's clone is listed for the embedder to run.
    fn spawn(
        &mut self,
        template: Value,
        at: Option<Value>,
        facing: Option<f32>,
        spawner: Handle,
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
        a.activator = if hero.is_some() {
            Value::Null
        } else {
            Value::Entity(spawner)
        };
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
        if a.light.is_none()
            && let Some(l) = source.and_then(|t| self.light_templates.get(&t))
        {
            a.light = Some(*l);
        }
        if let Some(p) = place {
            a.position = p;
        }
        if let Some(f) = facing {
            a.angles.facing = f;
        }
        a.origin = a.position;
        // A clone of a template the level's encounter walks walks too
        // (openspec AS.54): a spawner's Chompy steered as a placed one is.
        if hero.is_none()
            && a.walker.is_none()
            && source.is_some_and(|t| self.walk_clones.contains(&t))
        {
            a.walker = Some(Box::new(crate::script_engine::Walker {
                body: None,
                aim: guac_motion::steer::Aim::Still,
                along: 0,
            }));
        }
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
    fn referenced(&mut self, cell: u32, target: Value) {
        self.world.referenced(cell, target, self.engine);
    }
    fn told(&mut self, receiver: Value) {
        self.world.told = receiver;
    }
    fn adopt(&mut self, program: &Program) -> Vec<(Handle, String, ScriptId)> {
        self.world.adopt(program)
    }
    fn set_elements(&mut self, at: &Access, elements: &[Value]) -> Result<(), HostFault> {
        self.world.set_elements(at, elements)
    }
    fn set_ref(&mut self, at: &Access) -> Option<Vec<Value>> {
        self.world.set_ref(at)
    }
    fn wakes(&mut self, asleep: &[Handle]) -> Vec<Handle> {
        self.world.wakes(asleep)
    }
    fn answer(&mut self, name: &str, args: &[(&str, Value)]) -> bool {
        crate::interiors::answer(self.world, name, args)
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

/// The VM's config for AH.16's run: the init chain's members and listing,
/// with the sounds' and the music's ([`music_members`]).
pub fn config(mode: Mode) -> Config {
    let mut c = Config::new(mode);
    members::register(&mut c.members, INIT_CHAIN);
    music_members(&mut c);
    c.whitelist = Whitelist::with_first_ticks(&[], &EVERY_TICK, &GLOBAL_FIRST_TICKS);
    c
}

/// The sounds' members on the host and what the music controller's sound
/// system update reaches past them (openspec AR.10, AR.11), registered
/// last: the player loop runs that update at `Macros_Music`'s place in any
/// run that holds it (AR.11), so a config without them stops there
/// (openspec AR.25: `giants_eruptor` and the init chain's plugin run, on
/// `491e0a9c.0a72b658`), and on a placement's clones (the hero's table's
/// `abstractplacement.clones`) in a run without the hero's members.
fn music_members(c: &mut Config) {
    members::register(&mut c.members, members::SOUNDS);
    members::register(&mut c.members, members::MUSIC);
    if c.members
        .get(guac_script::MemberId::named("abstractplacement", "clones"))
        .is_none()
        && let Some(r) = members::find(members::HERO, "abstractplacement", "clones")
    {
        members::register(&mut c.members, &[r]);
    }
}

/// `global.bld`'s placements AH.17 ticks every step: the UI's (its tick
/// advances the combat timer every mover's update reads, and keeps the
/// loot in play) and the movement update's (each tick moving the scripted
/// movers), and the point-light manager's (openspec AH.65: each tick it
/// hands its pool's three lights to what asked for one, hangs each on what
/// it lights, copies the asker's colours onto it and takes it back once
/// that's gone or faded out). The game systems' tick moves the orbs; the
/// player loop runs that stretch of it alone (openspec AH.20).
pub const GLOBAL_EVERY_TICK: [&str; 4] = [
    "Macros_Animation.ai",
    "/UI.ai",
    "movementUpdate.ai",
    "PointLightManager.ai",
];

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
    // The gameplay camera's (openspec AG.28), over the cells it takes over.
    members::register(&mut c.members, members::CAMERA);
    // What the walkers' scripts reach past those (openspec AG.160), and
    // every character's attacks (openspec AU.2).
    members::register(&mut c.members, members::WALKERS);
    members::register(&mut c.members, members::ATTACKS);
    music_members(&mut c);
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
    // A warp lands the minion where it asks, his minion kept (openspec
    // AS.96; stand-in ledger: `warp`).
    c.answered.push(crate::interiors::WARP.to_owned());
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

/// A level's first encounter past chapter 1 (openspec AS.54): its own
/// placements the TV's listing names, by name, as chapter 1's
/// [`FIRST_FIGHT`] does (another of the level's placements running the same
/// script stays inert), the templates its scripts clone, whose clones
/// are listed (a clone runs under its template's name) and walked, and the
/// templates whose clones are listed and never walked (AS.68): what the
/// engine steers under `move to` with no body ([`ScriptWorld::scene_move`]),
/// whose solid meets neither actors nor the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encounter {
    pub level: &'static str,
    pub placements: &'static [&'static str],
    pub clones: &'static [&'static str],
    pub steered: &'static [&'static str],
}

/// The first encounters the TV runs past chapter 1's. Level 214's: the
/// Chompy pods on the first island past the dock, Setup_001's controller
/// (it plays the pods' scene as the hero passes its first trigger, then
/// wakes them) and its two spawners, each cloning its pod at its start and
/// Chompies from the undead Chompy template (its `chompy type`) once woken;
/// then its second fight past the pods (openspec AS.58), Setup_002's
/// controller (at its first trigger it sets its four undead Chompies going
/// and waits for Eon's elemental hint, `Explain_Elemental`, to take itself
/// off once its line has played, then at its second trigger runs them in)
/// and its helper (the two Elf Nears spar, invulnerable, until a bomb opens
/// their wall, then play their scene and fight); then the second island's
/// first fight (openspec AS.75), Setup_003's controller (at its trigger off
/// the ship it wakes its Tree Rex and four undead Chompies; the Tree Rex
/// eats one), and the Tree Rex's homing charge (AS.68,
/// `TreeRex_special_homing_actor`: a clone its ranged attack makes 10 ft in
/// front of it, whose script steers it at the hero by `move to` at 25 ft/s
/// turning 50°/s for 2.5 s, laying dirt mounds behind it, and strikes the
/// first within 4 ft; its box meets neither actors nor the level, so it's
/// steered with no body, never walked); then its next fight (AS.84),
/// Setup_004's controller (woken at Setup_003's trigger, it walks its three
/// Elf Nears to their posts; past the locked door ([`DOOR_SCRIPTS`]) its
/// first trigger sets them on their rounds, its second scares its Mabu,
/// who runs his waypoints to the house by the yard and goes in, and sets
/// the elves on the hero, its third wakes its Tree Rex and Chompy spawner)
/// and the five it wakes, its Mabu walked as a creature (his capsule meets
/// the level).
pub const ENCOUNTERS: [Encounter; 1] = [Encounter {
    level: "level_214_tutotial_2",
    placements: &[
        "Setup_001_Controller",
        "Enemy_Chompy_Spawner(1)",
        "Enemy_Chompy_Spawner(5)",
        // Setup_002 (openspec AS.58).
        "Setup_002_Controller",
        "Setup_002_Controller_Helper",
        "Explain_Elemental",
        "Enemy_Chompy2_E",
        "Enemy_Chompy2_F",
        "Enemy_Chompy2_G",
        "Enemy_Chompy2_H",
        "Enemy_Elf_Near_S2(3)",
        "Enemy_Elf_Near_S2(4)",
        // Setup_003, the second island's first fight (openspec AS.75).
        "Setup_003_Controller",
        "Enemy_Chompy2_E(1)",
        "Enemy_Chompy2_E(2)",
        "Enemy_Chompy2_E(3)",
        "Enemy_Chompy2_E(4)",
        "Enemy_TreeRex(2)",
        // Setup_004, past the locked door (openspec AS.84).
        "Setup_004_Controller",
        "Setup_004_NPC_Mabu",
        "Enemy_Elf_Near_A",
        "Enemy_Elf_Near_B",
        "Enemy_Elf_Near_C",
        "Enemy_TreeRex(1)",
        "Enemy_Chompy_Spawner(2)",
    ],
    clones: &["_Template_Undead_Chompy"],
    steered: &["TreeRex_special_homing_actor"],
}];

/// The intros the TV runs past chapter 1's, by level and placement
/// (openspec AS.47, AS.60). Level 214's (`214_Intro_IGC`, awake from the
/// load), held for the hero ([`hold_intro`]) and played to its end since
/// AS.81 (or skipped by a player's △), hands him over on the ship's deck:
/// its STARTUP takes `The_Ship_1` off (its parts with it, its SHUTDOWN's)
/// and the barrels by the dock, its cutscene draws two clones of
/// `214_Intro_Ship`, which its end takes off, and its own SHUTDOWN resets
/// the ship, the gun deck's hatch and the barrels, which a reset brings back
/// as their files have them, the ship's script cloning its parts again
/// ([`SHIP`]). Each hub stage's (openspec AS.47: its `CS_Hub_Intro` file,
/// awake from the load) plays the stage's arrival for the adventures done
/// (on stage 1 after level 214, `CS - (HS 2) Post - Tutorial 2`): the
/// story's scenes when the last adventure was just finished (`First_Time_Here`,
/// which the hub's definition sets off its `complete` flags), only the
/// landing on a load; either way its last scene hands the players over at
/// `CS_Hub_Intro_Final_Positions`' first two waypoints, which every stage
/// holds alike ([`crate::level_scene::HUB_START_PLACEMENT`]).
pub const INTROS: [(&str, &str); 5] = [
    ("level_214_tutotial_2", "214_Intro_IGC"),
    ("level_hub_2_stage_1", "CS_Intro_Controller"),
    ("level_hub_2_stage_2", "CS_Hub_Intro_Stage2"),
    ("level_hub_2_stage_3", "CS_Hub_Intro_Stage3"),
    ("level_hub_2_stage_4", "CS_Hub_Intro_Stage4"),
];

/// The hub stages' own scripts past the listing every level gets (openspec
/// AS.46, AS.47), by file name, every tick, on a hub stage alone
/// ([`level_config`]):
/// - the sky (`SkyDome_Controller.ai`): at its first tick it picks the
///   skydome for the adventures done (`Levels_Completed`, which the hub's
///   definition counts and stage 1's controller holds at 2 or more: after
///   level 214 `Skydome_201`), clones it at its own place at 400 % and
///   switches the atmospheric zone to that adventure's (201's), then keeps
///   the clone at its own height (which it moves to the nearest player's
///   when he's over 350 ft above or below it) and turns it 0.015° a tick
///   while he stands within 3 ft of its placed height (on the main deck,
///   not up at the helm), holding through a cutscene played outside an
///   interior. The scene holds no sky of its own; the clone draws by
///   AS.63's rule;
/// - the stage's controller (`Hub2_Stage_<n>_Controller.ai`): which of the
///   stage's placements stand for the adventures done (after level 214 the
///   snowmen, the ice and snow, the pirates' lights, the turret and
///   Dreadbeard off, 23 placements; the snow barrels are the stage's
///   intro's to take off) and the NPCs' waypoint (`Waypoint Value`);
/// - the background: its controllers clone the boulders and the islands
///   that drift past the ship for the adventures done, each moving on its
///   own script, and the deck's and the crane's zone controllers switch to
///   the stage's exterior zone;
/// - the way on (openspec AU.6): Flynn at the helm and Cali on the main
///   deck, each a dialogue a player within its active range starts with
///   the talk button (the UI's tick, on every level's listing, shows its
///   exclamation and button prompt and raises it). After level 214 Flynn's
///   sends a player to Cali until her intro has run (`Cali_Intro`, which
///   her intro sets as it starts and the save keeps), her intro asks her
///   challenge question ([`crate::prompt`]), and then Flynn's asks his
///   ready question and on a yes plays his outro (`HS2 - Outro`), which changes
///   level to the chapter he offers, level 201 after 214
///   ([`crate::level_switch`]).
pub const HUB_SCRIPTS: [&str; 11] = [
    "SkyDome_Controller.ai",
    "Hub2_Stage_1_Controller.ai",
    "Hub2_Stage_2_Controller.ai",
    "Hub2_Stage_3_Controller.ai",
    "Hub2_Stage_4_Controller.ai",
    "BackgroundController.ai",
    "Hub2_Background_Island.ai",
    "Hub2_Background_Actor.ai",
    "AtmZone_Controller.ai",
    "/Flynn.ai",
    "/Cali.ai",
];

/// The hub's people the TV draws (openspec AU.28), by their scripts:
/// Flynn at the helm and Cali on the main deck, each drawn on its
/// placement's model where its scripts stand and turn it, posed by the
/// clips they play, never walked (walked, a `turn to` is the engine's
/// steering, which left Cali facing 73° and her intro's turn undone at
/// AU.6; unwalked, the host turns it at once, as the headless runs do).
pub const HUB_PEOPLE: [&str; 2] = ["/Flynn.ai", "/Cali.ai"];

/// Whether a placement whose script is `script` is one of the hub's
/// people ([`HUB_PEOPLE`]).
pub fn is_hub_person(script: &str) -> bool {
    HUB_PEOPLE.iter().any(|p| script.ends_with(p))
}

/// Whether the level whose archive is `level` is a hub stage.
pub fn is_hub(level: &str) -> bool {
    guac_extract::levels::Kind::of(level) == guac_extract::levels::Kind::Hub
}

/// Level 214's ship (openspec AS.60), by placement name, every tick: the
/// ship the players stand on (`Ships.ai`, cloning its engine, masthead and
/// roof onto itself at its STARTUP and taking them off at its SHUTDOWN),
/// the intro's own ship, and the engine (`214_Hub_Engine.ai`, cloning its
/// oars, propeller and body onto its nodes and turning between its two
/// clips). By name, since the ships' scripts are the hub's too.
pub const SHIP: [&str; 3] = ["The_Ship_1", "214_Intro_Ship", "Hub_Engine"];

/// The intro placement of the level whose archive is `level`, if the TV
/// runs one.
pub fn intro(level: &str) -> Option<&'static str> {
    INTROS.iter().find(|(l, _)| *l == level).map(|(_, p)| *p)
}

/// The intro placement `placement` of the file linked as `level` (a level's
/// [`intro`]) held before its first tick until a hero's
/// minion is in (his join's activation lets it go, openspec AS.47; on the
/// TV once his entry beam lands): the original's
/// players are in the level when its intro starts, so its cutscene takes
/// them and hands them over where it ends; the TV's run starts on the
/// portal view, before any hero is in (a hypothesis: stand-in ledger
/// `spawn`). Whether it held one.
pub fn hold_intro(
    vm: &mut Vm,
    world: &mut ScriptWorld,
    files: &Files,
    level: u16,
    placement: &str,
) -> bool {
    let Some(&h) = files
        .placed
        .iter()
        .find(|p| p.obj.unit == level && p.name == placement)
        .and_then(|p| world.handles().get(&p.obj))
    else {
        return false;
    };
    if vm.placement_of(h).is_none() || !vm.set_suspended(h, true) {
        return false;
    }
    world.intro_held = Some(h);
    true
}

/// The intro [`hold_intro`] held let go, to run its STARTUP at its next
/// tick: what a hero's join does once his minion's in. Whether one was
/// held.
pub fn release_intro(vm: &mut Vm, world: &mut ScriptWorld) -> bool {
    match world.intro_held.take() {
        Some(h) => vm.set_suspended(h, false),
        None => false,
    }
}

/// The first encounter of the level whose archive is `level`, if the TV
/// runs one past chapter 1's.
pub fn encounter(level: &str) -> Option<&'static Encounter> {
    ENCOUNTERS.iter().find(|e| e.level == level)
}

/// The loot on the way to the first fight (openspec AH.20, design decision
/// 2's whitelist: the coins in reach and the loot macros): the level's loot
/// system's first tick and the spinning coins on chapter 1's walkway and on
/// the line down into the pit, by placement name. AG.60 runs every placed
/// coin and gem by its script ([`START_PROPS`]), these nine among them.
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

/// The start's scripts on the props (openspec AG.60, design decision 2's
/// whitelist), every tick, by script: what level 213's start-up scripts do
/// to its placements before the hero moves, run on the VM rather than
/// written as our own (stand-in ledger: `start-set`, struck). The gourds'
/// and bricks' scales (`Scale.ai`), the propeller's turn, the bird nests'
/// size and eggs, every placed coin and gem (the spinning ones spin by the
/// UI's loot update), the push blocks and the cannon (their art, their 15 x
/// 12 x 15 ft boxes; level 214's three by its pods, its way up onto the
/// rise, openspec AS.74), the walkway's fence (its cross beam and planks; it
/// makes itself a combat target), the cannon's rubble wall, the sliding
/// rocks and the global reference the chests share; then the scripts of
/// what those spawn. The Titan boulders run their first tick alone
/// ([`START_PROPS_FIRST_TICK`]). Off: the chest's (`Chest.ai` and its
/// locks'), a creature's
/// left out as the creatures are: undrawn, it took `Active_Collectable` and
/// stopped on `clonedsequence.clones` (its triggered sound's, unmodelled) the
/// moment he came within 18 ft (the AG.60 review).
pub const START_PROPS: [&str; 23] = [
    "Scale.ai",
    "Windmill_Rotate.ai",
    "BirdNest.ai",
    "Placed_Loot_Spinning.ai",
    "Placed_Loot.ai",
    "PushBlock_Template.ai",
    "WoodFence_Spawner.ai",
    "StoneWall_2.ai",
    "Falling_Rocks.ai",
    "Chest_SetGlobalRef.ai",
    "Wood_Plank.ai",
    "Fence_Sign.ai",
    "Bouncing_Wood_Pieces.ai",
    "Torus_Effect.ai",
    "Art_Generic_Stone.ai",
    "Box_Tortoise.ai",
    "Canon_bottom.ai",
    "Pushblock_canon.ai",
    "CannonWarpRing.ai",
    "Switch_90_Setup.ai",
    "Boulder_Pebbles.ai",
    "Boulder_Chunks.ai",
    "StoneWall_Piece.ai",
];

/// The Titan boulders' script, its first tick alone (openspec AG.60): its
/// STARTUP makes each boulder's solid the original's cylinder and lays its
/// pebbles, then it's held, so the feat it waits for (a Giant lifting it)
/// stays off for every hero (stand-in ledger: `titan`).
pub const START_PROPS_FIRST_TICK: [&str; 1] = ["Titan_Carriable_Object.ai"];

/// The gameplay camera's level scripts (openspec AG.28, design decision 2's
/// whitelist, AH.42's way), every tick, by script: the camera volumes, each
/// switching to the camera its placement's id names over the seconds its
/// tilt holds once the hero is inside it (`Pop_To_Camera_x.ai` polling
/// every 0.08 s), and the sliding rocks' activator, which drops the rocks
/// (their `Falling_Rocks.ai` shakes the camera) and switches to camera 7
/// two seconds on. Their camera system is `global.bld`'s macros, which the
/// portal manager's tick calls ([`crate::player_loop`]).
pub const CAMERA_SCRIPTS: [&str; 8] = [
    "Pop_To_Camera_x.ai",
    "Pop_to_Camera_5.ai",
    "Special_Pop_To_Camera.ai",
    "Jump_Pad_Camera_Pop.ai",
    "Special_Camera_Transition_213.ai",
    "Pop_To_X_Special_213.ai",
    "Pop_to_42_end.ai",
    "Activator_Sliding_Rocks.ai",
];

/// The jumping goats (openspec AG.177, design decision 2's whitelist,
/// AH.42's way), every tick, by script: `Sheep_1` and `Sheep_3` idle until a
/// minion comes within 30 ft, startle, run their waypoints and jump, and the
/// jump's landing at its fourth waypoint opens `Sliding_Rock(5)`, the rock
/// slide's third rock, whose fall shakes the camera a third time. How they're
/// drawn and walk is AH's route line's.
pub const GOAT_SCRIPTS: [&str; 1] = ["Goat_Jumping_Pit.ai"];

/// AH.8's route from the walkway to the pit (openspec AH.28, design
/// decision 2's whitelist, AH.42's way), every tick, by script: what the
/// route's obstacles spawn or wake that the start's scripts don't run
/// already. The cannon's ball, which breaks the rubble wall it lands on;
/// the push blocks' water ends, which the blocks look for as they drop
/// (one pushed into the channel goes inactive there, a stepping stone);
/// the spike gate Norticus' trigger wakes, and its holes; the camera's pan
/// to the cannon as he comes near it, and the pit's intro cutscene the
/// controller wakes as the hero lands (both on the cutscene system,
/// [`crate::player_loop::UPDATES`]).
pub const ROUTE_SCRIPTS: [&str; 6] = [
    "Canon_Ball.ai",
    "Push_Block_End.ai",
    "Gate_Spikes.ai",
    "Gate_Spikes_Hole.ai",
    "Pan_Show_Cannon.ai",
    "Pan_Show_Norticu.ai",
];

/// Level 214's bombs and the walls they open (openspec AS.57), every tick, by
/// script: each bomb spawner (`Tools_Spawner.ai`, its id 1 the bomb's)
/// clones a bomb while none of its own stands and no player carries one;
/// the bomb (`Bomb_Rocket_Template.ai`) is taken by a hero coming within
/// 10 ft and carried 12 ft over him, counting down, until a tap of his
/// fourth action (△, the PS3's tap gesture: `global.bld`'s gesture manager,
/// `UI_Controller_Prompt.ai`, its update run by the game systems' tick) throws
/// it, and where it bursts it opens every breakable castle wall within 20 ft
/// (`StoneWall_Game2_Version.ai`), which falls in pieces; the pan to the
/// first bomb (`Pan_Show_Bomb.ai`) with them, and the bomb hints by name
/// ([`BOMB_HINTS`]); and its countdown's floating numbers (AS.68,
/// `BombTImer.ai`): each count of a carried bomb past its first clones a
/// timer at it, its id the count, which follows the bomb by `move to`
/// (steered with no body: it has no shape) and fades its sprite out over
/// 2 s, then takes itself off. Its sprite the TV draws nowhere yet.
pub const BOMB_SCRIPTS: [&str; 6] = [
    "Tools_Spawner.ai",
    "Bomb_Rocket_Template.ai",
    "UI_Controller_Prompt.ai",
    "StoneWall_Game2_Version.ai",
    "Pan_Show_Bomb.ai",
    "BombTImer.ai",
];

/// The locked doors and their keys (openspec AS.84), every tick, by script:
/// a key spawner (`Key_Pickup_Spawner.ai`) lays its key (`Key_pickup.ai`),
/// which a hero walking into takes; at a door (`Door_Skylanders_2.ai`, its
/// id the keys it wants) a hero with a key starts the door's prompt, a tap
/// of his fourth action (△, the gesture manager [`BOMB_SCRIPTS`] lists)
/// turns it in the lock (`Door_Key.ai`, `Door_Lock_Intact_Wood.ai`, the
/// lock's pieces `Door_Lock.ai`), and once every lock turns the door opens.
/// Level 214's second island holds three: the first, at the way out of the
/// gangplank's yard, opens to the key in the lobe below it. Chapter 1
/// places none.
pub const DOOR_SCRIPTS: [&str; 6] = [
    "Key_Pickup_Spawner.ai",
    "Key_pickup.ai",
    "Door_Skylanders_2.ai",
    "Door_Key.ai",
    "Door_Lock_Intact_Wood.ai",
    "Door_Lock.ai",
];

/// The interiors on every level (openspec AS.96), every tick, by script:
/// each controller (`Interior_Controller.ai`, [`crate::interiors`]) takes a
/// hero walking into its box through a darkness to the room its waypoints
/// name and back, by the portal's warp the TV answers; the arrow a
/// controller with more than three waypoints clones over its last
/// (`Extra_arrow.ai`, tinting in as a hero comes within 40 ft); and a
/// house's door swinging open as a hero comes near and shut as he goes
/// (`Generic_House_door_Script.ai`). By script: 137 controllers run on 21
/// levels (chapter 1's four, level 214's nine), each level's own copy of
/// the scripts under its own path.
pub const INTERIOR_SCRIPTS: [&str; 3] = [
    "Interior_Controller.ai",
    "Extra_arrow.ai",
    "Generic_House_door_Script.ai",
];

/// Level 214's bomb hints (openspec AS.57), by placement name: each plays
/// Eon's line once a hero touches its volume, its text an announcement the
/// TV draws nowhere. By name, since the hints' script is every level's
/// (chapter 1's fifteen stay off).
pub const BOMB_HINTS: [&str; 2] = ["0_Hint_Use_Bombs", "0_Hint_Use_Bombs(1)"];

/// What the scripts spawn that draws from `global.bld`'s own models (openspec
/// AH.39), by file name: the level-up's flourish (its warp ring, tube and
/// star, and the shockwave its scripts strike) and the hero's death actor
/// (the death stars).
pub const EFFECT_SCRIPTS: [&str; 2] = ["LevelUp.ai", "MinionDeathActor.ai"];

/// The levels' music, every tick, by script. Chapter 1's (openspec AR.11,
/// AR.4's reading): the level's music controller, which plays its intro
/// track at its start, its tracks through the level's transitions and the
/// combat bed for the robot, and its watchers, the combat watcher (the
/// inside combat bed and its two percussion stingers), the interior watcher
/// (inside and explore swapped), the three elemental gates' and the audio
/// environment's and the robot's ducking, each placed or spawned by the
/// controller. Level 214's (openspec AS.54): its dynamic sound controller,
/// which starts its explore bed at the level's start, its combat, interior,
/// elemental and cabin watchers and explore switchers, the wall blow-ups'
/// transitions, the scenes' music player, the two jukeboxes (the hub's
/// script), the music's mute mix and the beat timer. Neither level places
/// the other's, but by script they join every level placing them: the beat
/// timer, the mute mix and the jukebox controller run on 36 other levels
/// too (the story levels' beat timer, the hub stages' jukeboxes and radios,
/// the arena challenges'; the as-214-alive review's whole-disc listing).
pub const MUSIC_SCRIPTS: [&str; 20] = [
    "Music_Level_Controller.ai",
    "snd_combat_watcher_213.ai",
    "snd_interior_watcher_213.ai",
    "snd_elemental_air_watcher_213.ai",
    "snd_elemental_life_watcher_213.ai",
    "snd_elemental_undead_watcher_213.ai",
    "snd_level_audio_environment.ai",
    "snd_robo_ducking.ai",
    "snd_dynamic_sound_controller_214.ai",
    "snd_combat_watcher_214.ai",
    "snd_interior_watcher_214.ai",
    "snd_elemental_earth_watcher_214.ai",
    "snd_elemental_fire_watcher_214.ai",
    "snd_explore_switcher_214.ai",
    "snd_persephone_cabin_watcher.ai",
    "snd_wall_blowup_transitions_214.ai",
    "IGC_Music_Player.ai",
    "snd_jukebox_controller.ai",
    "snd_mixtype_music_mute.ai",
    "time_till_next_beat.ai",
];

/// Chapter 1's end (openspec AS.15), every tick: `Temp_Level_Ender`, asleep
/// until a script wakes it (`Rope_around_Post(2)`'s last behaviour, or the
/// dev entry's end anchor), fades the screen out and wakes the outro, whose
/// cutscene (played to its end since AS.52, its actors clones of their
/// templates the scene walks and animates) ends in the outro's own `Game -
/// Level End`, naming the next level. The outro's actors run their own
/// (AS.52): Norticus and the Mabu at 140 %, each with his hat on his head's
/// node.
pub const LEVEL_END_SCRIPTS: [&str; 4] = [
    "Temp_Level_Ender.ai",
    "213_Outro_IGC.ai",
    "213_Norticus_NPC.ai",
    "213_Mabu_Generic.ai",
];

/// `config` over a level's own file, linked as `level` (openspec AS.51):
/// each level placement's definition script ([`is_level_definition`];
/// chapter 1's is on [`EVERY_TICK`] already) listed every tick, and each
/// script the listing names that the level's file spells in another case
/// listed under the level's spelling (level 214's camera volumes run
/// `Pop_to_Camera_X.ai`, chapter 1's [`CAMERA_SCRIPTS`] name
/// `Pop_To_Camera_x.ai`), a first tick's as a first tick. Chapter 1 gains
/// nothing by it.
pub fn with_level(mut config: Config, files: &Files, level: u16) -> Config {
    let Whitelist::Only {
        scripts,
        first_tick,
        ..
    } = &mut config.whitelist
    else {
        return config;
    };
    let exact = |l: &[String], path: &str| {
        l.iter()
            .any(|s| !s.is_empty() && path.ends_with(s.as_str()))
    };
    let any_case =
        |l: &[String], file: &str| l.iter().any(|s| file_name(s).eq_ignore_ascii_case(file));
    let (mut every, mut first) = (Vec::new(), Vec::new());
    for p in files.placed.iter().filter(|p| p.obj.unit == level) {
        let Some(path) = p.script.as_deref() else {
            continue;
        };
        let file = file_name(path);
        let definition = is_level_definition(file);
        if exact(scripts, path) || exact(first_tick, path) {
            continue;
        }
        if definition || any_case(scripts, file) {
            every.push(file.to_owned());
        } else if any_case(first_tick, file) {
            first.push(file.to_owned());
        }
    }
    for (list, add) in [(scripts, every), (first_tick, first)] {
        for f in add {
            if !list.contains(&f) {
                list.push(f);
            }
        }
    }
    config
}

/// Whether a script's file name is a level definition's (openspec AS.51):
/// its letters and digits, any case, holding `leveldefin` (214's
/// `Level_Definition_214.ai`, `LevelDefinition_003.ai`, 207's
/// `level_definition.ai`, PvP arena 4's `_LevelDefintion_PvP_004.ai`, a
/// challenge's `Challenge_041_Level_Definition_01.ai`) or `leveldef` then
/// nothing, a digit or `pvp` (`_LevelDef_205.ai`, `Hub2_LevelDef.ai`,
/// `_levelDef_PvP_202.ai`). The disc spells them 79 ways over 87 levels:
/// AS.51's review found the first rule, `Level_Definition_` then more,
/// listing 5 of the 20 story levels' (level_200's among those it missed)
/// and no hub's, arena's or challenge's; by this one every story level,
/// hub stage, PvP arena and challenge lists its own, and no arena challenge
/// has one by name (AS.55).
pub fn is_level_definition(file: &str) -> bool {
    let stem = file
        .rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect::<String>();
    stem.match_indices("leveldef").any(|(at, m)| {
        let rest = &stem[at + m.len()..];
        rest.starts_with("in")
            || rest.starts_with("pvp")
            || rest.chars().next().is_none_or(|c| c.is_ascii_digit())
    })
}

/// The macro a cutscene object is defined by: its `object def` and
/// `actor` are what [`cutscene_objects`] reads.
const DEFINE_OBJECT: &str = "Cutscene - Define Object";

/// `config` with the cutscene objects a level's own placements define
/// listed every tick (openspec AS.82): each running placement of the
/// level's file whose script binds `Cutscene - Define Object`'s `object
/// def` to a file variable, and the script of each template that call
/// names as the object's `actor`. Flynn's and Cali's (`Object_Flynn_Def.ai`
/// and `Object_Cali_Def.ai`, awake from the load: 15 placements on 9
/// levels, none on chapter 1) define their objects into a file structure
/// at their STARTUP and take themselves off; the cutscenes add those
/// structures to their scenes, so unlisted, the object read no actor and
/// the first walk on it never ended. No file structure on the disc holds
/// field values of its own (1,565 over every level, hub, `global.bld` and
/// character file, none with a field list), so what the scripts write is
/// all an object holds. The templates' scripts are their sizes
/// (`NPC_Size.ai`, 110 %), which their cutscenes' clones run.
pub fn with_cutscene_objects(
    mut config: Config,
    program: &Program,
    files: &Files,
    level: u16,
) -> Config {
    let Whitelist::Only { scripts, .. } = &mut config.whitelist else {
        return config;
    };
    for name in cutscene_objects(program, files, level) {
        if !scripts.contains(&name) {
            scripts.push(name);
        }
    }
    config
}

/// The file names [`with_cutscene_objects`] lists for `level`: each
/// definition's, then its templates', in the program's order.
pub fn cutscene_objects(program: &Program, files: &Files, level: u16) -> Vec<String> {
    use guac_script::{Elem, Ins, Operand};
    let only = |rhs: Option<u32>| -> Option<Elem> {
        let mut rhs = rhs?;
        while let Operand::Expr {
            a: Some(inner),
            op: None,
            b: None,
        } = &program.operands[rhs as usize]
        {
            rhs = *inner;
        }
        match &program.operands[rhs as usize] {
            Operand::Path { elems, .. } if elems.len() == 1 => Some(elems[0]),
            _ => None,
        }
    };
    let mut out: Vec<String> = Vec::new();
    for s in program.scripts.iter().filter(|s| s.unit == level) {
        let runs = files.placed.iter().any(|p| {
            p.obj.unit == level
                && p.activity_state == RUNNING
                && p.script.as_deref() == Some(s.name.as_str())
        });
        if !runs {
            continue;
        }
        let (mut defines, mut actors) = (false, Vec::new());
        for op in &s.ops {
            let Ins::Parameter(p) = &program.ops[*op as usize] else {
                continue;
            };
            let Some(def) = p.def else {
                continue;
            };
            if !program.defs[def as usize]
                .display
                .eq_ignore_ascii_case(DEFINE_OBJECT)
            {
                continue;
            }
            match (program.sym(p.name), only(p.rhs)) {
                ("object def", Some(Elem::Global(_))) => defines = true,
                ("actor", Some(Elem::Object(o))) => actors.push(o),
                _ => {}
            }
        }
        if !defines {
            continue;
        }
        let templates = actors.iter().filter_map(|o| {
            files
                .placed
                .iter()
                .find(|p| p.obj == *o)
                .and_then(|p| p.script.as_deref())
        });
        for path in std::iter::once(s.name.as_str()).chain(templates) {
            let file = file_name(path).to_owned();
            if !out.contains(&file) {
                out.push(file);
            }
        }
    }
    out
}

/// Whether a script's source path names one of [`CAMERA_SCRIPTS`], by its
/// file name, any case.
pub fn is_camera_script(path: &str) -> bool {
    guac_extract::scripts::listed(&CAMERA_SCRIPTS, path)
}

/// Where the level's music stands against its sounds ([`hold_music`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Music {
    /// The book lacks the level's sounds: this many music placements held.
    Waits(usize),
    /// The level's sounds read: this many held ones let go, to start now.
    Starts(usize),
    /// The book without the level's sounds and none on their way: this many
    /// held ones let go, to run silent from now (openspec AR.30).
    Silent(usize),
    /// Nothing held, and nothing to hold: the music runs as it is.
    Runs,
}

/// The level's music scripts ([`MUSIC_SCRIPTS`]) waiting for the level's
/// sounds (openspec AR.23): while the book lacks the sounds of the file
/// linked as `level` (a first run hands chapter 1 over ahead of its sounds
/// stage, AR.21), each placement of the level running one is suspended
/// before it ticks; once the book holds them, those held are let go, and
/// each runs its STARTUP at its next tick, the level's music starting then
/// as it would at the level's start (the intro, explore 5 s on, on the
/// intro's bar). Their activity follows the activity rule while held; a
/// placement a script suspended stays as it is.
///
/// Why not take up what the scripts started before the sounds: a sound
/// info the book lacks plays as any object, a sequence of no length, and
/// reads 0 for every member, so the music's own bookkeeping goes wrong
/// before the sounds land (AR.23's headless run: the intro's copy kept a
/// volume of 0 its delayed start wrote back, explore's delayed start
/// dropped as the copy read done), and a bed given its recording then
/// plays silent or never starts. A hypothesis, nothing settling it: the
/// original's sounds are on its disc from the start (stand-in ledger:
/// `sound-play`).
pub fn hold_music(vm: &mut Vm, world: &mut ScriptWorld, files: &Files, level: u16) -> Music {
    hold_music_while(vm, world, files, level, true)
}

/// [`hold_music`] keyed on whether the level's sounds are on their way
/// (openspec AR.30, AR.27's hold): while the book lacks them the music waits
/// only while `coming` (a first run's sounds stage still to land), never for
/// good. With none coming it isn't held, and what was is let go to run
/// silent, as a level whose sounds the extract lacks ran before AR.23.
pub fn hold_music_while(
    vm: &mut Vm,
    world: &mut ScriptWorld,
    files: &Files,
    level: u16,
    coming: bool,
) -> Music {
    let booked = world.holds_sounds_of(level);
    if booked || !coming {
        let held = std::mem::take(&mut world.music_held);
        for &h in &held {
            vm.set_suspended(h, false);
        }
        return match (held.len(), booked) {
            (0, _) => Music::Runs,
            (n, true) => Music::Starts(n),
            (n, false) => {
                world.music_silent += n;
                Music::Silent(n)
            }
        };
    }
    for p in files.placed.iter().filter(|p| p.obj.unit == level) {
        if !p.script.as_deref().is_some_and(is_music_script) {
            continue;
        }
        let Some(&h) = world.handles().get(&p.obj) else {
            continue;
        };
        let Some(id) = vm.placement_of(h) else {
            continue;
        };
        if world.music_held.contains(&h) || vm.is_suspended(id) {
            continue;
        }
        vm.set_suspended(h, true);
        world.music_held.push(h);
    }
    Music::Waits(world.music_held.len())
}

/// Whether a script's source path names one of [`MUSIC_SCRIPTS`], by its
/// file name, any case.
pub fn is_music_script(path: &str) -> bool {
    guac_extract::scripts::listed(&MUSIC_SCRIPTS, path)
}

/// The VM's config for the TV's run (openspec AH.27): AH.17's with AH.20's
/// pickups and AH.21's breakables and the members they reach (the
/// creatures' and the loop's: the destructibles system makes a breakable a
/// combat target), AG.60's start scripts on the props and AG.28's camera
/// volumes and activator, the level-up's and the death actor's
/// ([`EFFECT_SCRIPTS`], AH.39) and the levels' music ([`MUSIC_SCRIPTS`],
/// AR.16, AS.54), the first fight's placements left inert until AH.25's entry
/// brings them; AS.11's every-level table ([`members::LEVELS`], openspec
/// AS.37) registered with the rest.
pub fn smash_config(mode: Mode) -> Config {
    let mut c = hero_config(mode);
    members::register(&mut c.members, members::CREATURES);
    members::register(&mut c.members, members::LOOP);
    // The camera's again, over the creatures' cells for the viewport.
    members::register(&mut c.members, members::CAMERA);
    // What AH.8's route reaches past those (openspec AH.28).
    members::register(&mut c.members, members::ROUTE);
    // And the level-up's flourish (openspec AH.39).
    members::register(&mut c.members, members::LEVEL_UP);
    // What AS.11's every-level run reaches past those (openspec AS.37): a
    // table the VM lane grows reaches the TV only once it's named here.
    members::register(&mut c.members, members::LEVELS);
    // The sounds' members on the host, over the cells the tables above
    // registered for them (openspec AR.10), and what chapter 1's music
    // reaches past them (openspec AR.11), again after those tables.
    music_members(&mut c);
    if let Whitelist::Only {
        placements,
        scripts,
        first_tick,
    } = &mut c.whitelist
    {
        placements.extend(PICKUPS.iter().map(|s| (*s).to_owned()));
        placements.extend(BREAKABLES.iter().map(|s| (*s).to_owned()));
        scripts.extend(START_PROPS.iter().map(|s| (*s).to_owned()));
        scripts.extend(CAMERA_SCRIPTS.iter().map(|s| (*s).to_owned()));
        scripts.extend(GOAT_SCRIPTS.iter().map(|s| (*s).to_owned()));
        scripts.extend(ROUTE_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // Level 214's bombs (openspec AS.57).
        scripts.extend(BOMB_SCRIPTS.iter().map(|s| (*s).to_owned()));
        placements.extend(BOMB_HINTS.iter().map(|s| (*s).to_owned()));
        // The locked doors and their keys (openspec AS.84).
        scripts.extend(DOOR_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // The interiors on every level (openspec AS.96).
        scripts.extend(INTERIOR_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // Level 214's ship (openspec AS.60).
        placements.extend(SHIP.iter().map(|s| (*s).to_owned()));
        scripts.extend(EFFECT_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // The levels' music (openspec AR.16, AS.54): their rolls draw on
        // the session's generator, as the original's do.
        scripts.extend(MUSIC_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // Chapter 1's end (openspec AS.15).
        scripts.extend(LEVEL_END_SCRIPTS.iter().map(|s| (*s).to_owned()));
        // Its interface placement the scheduler holds (openspec AT.44).
        scripts.extend(LEVEL_UI.iter().map(|s| (*s).to_owned()));
        // Eon's lines (openspec AR.29).
        scripts.extend(LINE_SCRIPTS.iter().map(|s| (*s).to_owned()));
        first_tick.extend(START_PROPS_FIRST_TICK.iter().map(|s| (*s).to_owned()));
    }
    c
}

/// The interface placements the original's scheduler holds (openspec
/// AT.44), by script, every tick: the UI controller, awake in the June
/// record's list (AH.59: `june_sched.txt`, 133rd of group 1's 148), which
/// waits for the first hero to touch its volume, then starts the
/// adventure's title (3 s, the title's own macro) and takes itself off the
/// same tick (AG.7 read it taken off at AF's fresh state). By script, not
/// placement: level 212's `212_UI_Controller` runs the same file (AT.44's
/// review: on no other level). The interface's other placements in the
/// level and `global.bld`'s (`Dialogue_UI`, `UI_PlayerHUD_Macros` and the
/// rest) are templates (`_activityState` 5): the scheduler never holds
/// one, so none ticks, and their macros run where the scripts call them,
/// the UI's tick and the game systems' update set among them, as the run
/// has them already ([`init_chain`] adds running placements, 4, alone).
pub const LEVEL_UI: [&str; 1] = ["213_UI_Controller.ai"];

/// Eon's lines' activators (openspec AR.17, AR.29), by script, every tick
/// on the TV's listing: each waits for the hero to touch it while no other
/// line holds the level's (`Global(393)`), then plays its line. Level 213
/// holds two. `Eon_1_Move_along_path`, as the level starts, through the
/// dialogue UI (its `string reference` and `voice reference` tags a lone
/// string and sound each, a set of one): his box with his portrait over
/// the level, the player's HUD hidden while it's up as the original's is
/// (AG.6's `ag6-explore1-c1-place` shots at flips 30,073 and 30,160, shown
/// again at 30,247), his voice on the dialogue UI's placement; the
/// activator off on 'text box remove''s first tick (AR.36), so the line it
/// held lets go. `Eon_2_Keep_Going`, on the ledge path, a voice alone (no
/// string: `Sound - Play VO`), off as its copy ends.
pub const LINE_SCRIPTS: [&str; 1] = ["Eon_Line_Activator.ai"];

/// One walker's scripts on the whitelist (openspec AG.27, AH.42's way) and
/// what they ask of the gameplay viewport beyond the TV's own run
/// (AH.107; stand-in ledger: `on-screen`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkerScripts {
    pub load_file: String,
    /// The unit his file links as.
    pub unit: u16,
    /// `/` and the file name of each of his scripts the run lists to walk:
    /// the scripts of his file's placements running at his join (his
    /// definition, his tuning actor) and his minion's ([`Self::minion`]).
    pub scripts: Vec<String>,
    /// `/` and the file name of every other script his file holds whose
    /// asks of the gameplay viewport are all read: what his attacks spawn
    /// (a projectile, a shockwave, a summoned helper), listed beside
    /// [`Self::scripts`], since his attack buttons reach his scripts as
    /// Eruptor's do (openspec AU.1; Eruptor's are [`ERUPTOR_SCRIPTS`]).
    pub attack_scripts: Vec<String>,
    /// The rest of his file's scripts, each with the points it asks against
    /// the gameplay viewport past the TV's run and his walk's that no one
    /// has read yet (AH.150's [`crate::on_screen::READ`]): each stays off
    /// the listing, so what spawns it stays inert, until they're read.
    pub held_scripts: Vec<(String, usize)>,
    /// His minion's script (`/` and its file name): the template his
    /// definition's `Portal - Define Minion` names, which the join's
    /// activation spawns as him. Its file is his own, named as the disc
    /// names it, often not for his load file (`012_WaterDragon2.ai` for
    /// `012_SeaDragon`, the base's `015_Yeti.ai` in `015_Yeti_Legendary`'s).
    /// `None` when the definition names no template of his file: he keeps
    /// his card.
    pub minion: Option<String>,
    /// That template's object: what the join sets his palette's clip
    /// lengths on (its name is often not his load file: `015_Yeti_L`).
    pub template: Option<Obj>,
    /// The points his scripts ask against the gameplay viewport that
    /// neither Eruptor's run nor the level's reach already (AH.107's
    /// readers) and no one has read for what they gate yet (AH.150's
    /// [`crate::on_screen::READ`]): the host would answer each "on screen",
    /// so a walker with any keeps his card.
    pub asks: usize,
    /// Those new asks read as gating nothing in play (AH.150): the host's
    /// every-point answer stands for them, holding no one.
    pub cosmetic: usize,
    /// Those new asks read as gating play (AH.150): the host answers them
    /// off the gameplay camera's view, holding no one.
    pub answered: usize,
}

/// The file name of a script's source path, either separator.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Each of `walkers`' scripts on the whitelist and the new asks they reach,
/// each counted by the disc's reads ([`crate::on_screen::READ`]).
pub fn walker_scripts(
    program: &Program,
    files: &Files,
    walkers: &[(String, u16)],
) -> Vec<WalkerScripts> {
    walker_scripts_reading(program, files, walkers, crate::on_screen::READ)
}

/// [`walker_scripts`] with the asks counted by `reads`.
pub fn walker_scripts_reading(
    program: &Program,
    files: &Files,
    walkers: &[(String, u16)],
    reads: &[crate::on_screen::Read],
) -> Vec<WalkerScripts> {
    use crate::on_screen::{Verdict, read_of};
    use std::collections::{BTreeMap, BTreeSet};
    // What the TV's own run reaches, AH.107's count: not a walker's to add.
    let base = smash_config(Mode::Play).whitelist;
    let mut known: BTreeSet<crate::on_screen::Ask> = BTreeSet::new();
    for (id, s) in program.scripts.iter().enumerate() {
        if !walkers.iter().any(|(_, u)| *u == s.unit) && base.lists("", &s.name) {
            known.extend(crate::on_screen::reached(program, id as ScriptId).0);
        }
    }
    walkers
        .iter()
        .map(|(load, unit)| {
            let mut names: Vec<String> = files
                .placed
                .iter()
                .filter(|p| p.obj.unit == *unit && p.activity_state == RUNNING)
                .filter_map(|p| p.script.as_deref())
                .map(|path| format!("/{}", file_name(path)))
                .collect();
            let template = defined_minion(program, files, *unit, &names);
            let minion = template
                .and_then(|t| t.script.as_deref())
                .map(|path| format!("/{}", file_name(path)));
            names.extend(minion.clone());
            let walks = |file: &str| {
                names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&format!("/{file}")))
            };
            // His walk's asks hold his card; each of his other scripts'
            // (what his attacks spawn, openspec AU.1) holds that script.
            let mut asks: BTreeSet<crate::on_screen::Ask> = BTreeSet::new();
            let mut others: BTreeMap<String, BTreeSet<crate::on_screen::Ask>> = BTreeMap::new();
            for (id, s) in program
                .scripts
                .iter()
                .enumerate()
                .filter(|(_, s)| s.unit == *unit)
            {
                let file = file_name(&s.name);
                let reached = crate::on_screen::reached(program, id as ScriptId).0;
                if walks(file) {
                    asks.extend(reached);
                } else {
                    others
                        .entry(format!("/{file}"))
                        .or_default()
                        .extend(reached);
                }
            }
            names.sort_by_key(|n| n.to_ascii_lowercase());
            names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
            let new: Vec<Option<Verdict>> = asks
                .difference(&known)
                .map(|a| read_of(program, a, reads).map(|r| r.verdict))
                .collect();
            let count = |v: Option<Verdict>| new.iter().filter(|x| **x == v).count();
            let (mut attack_scripts, mut held_scripts) = (Vec::new(), Vec::new());
            for (name, reached) in others {
                let unread = reached
                    .iter()
                    .filter(|a| !known.contains(a) && !asks.contains(a))
                    .filter(|a| read_of(program, a, reads).is_none())
                    .count();
                match unread {
                    0 => attack_scripts.push(name),
                    n => held_scripts.push((name, n)),
                }
            }
            attack_scripts.sort_by_key(|n| n.to_ascii_lowercase());
            held_scripts.sort_by_key(|(n, _)| n.to_ascii_lowercase());
            WalkerScripts {
                load_file: load.clone(),
                unit: *unit,
                scripts: names,
                attack_scripts,
                minion,
                template: template.map(|t| t.obj),
                asks: count(None),
                cosmetic: count(Some(Verdict::Cosmetic)),
                answered: count(Some(Verdict::Gates)),
                held_scripts,
            }
        })
        .collect()
}

/// The template of the minion `unit`'s definition defines: the placement its
/// `Portal - Define Minion` call binds as `minion`, one of his own file's
/// with a script, as the join's activation spawns him ([`join_activate`]).
/// Read off the scripts `listed` (his running placements'), the first such
/// call; `None` when none names one.
fn defined_minion<'f>(
    program: &Program,
    files: &'f Files,
    unit: u16,
    listed: &[String],
) -> Option<&'f Placed> {
    use guac_script::{Elem, Ins, Operand};
    let ours = |name: &str| {
        let file = file_name(name);
        listed.iter().any(|n| {
            n.strip_prefix('/')
                .is_some_and(|n| n.eq_ignore_ascii_case(file))
        })
    };
    program
        .scripts
        .iter()
        .filter(|s| s.unit == unit && ours(&s.name))
        .flat_map(|s| s.ops.iter())
        .find_map(|op| {
            let Ins::Parameter(p) = &program.ops[*op as usize] else {
                return None;
            };
            let def = &program.defs[p.def? as usize];
            if program.sym(p.name) != "minion"
                || !def.display.eq_ignore_ascii_case("Portal - Define Minion")
            {
                return None;
            }
            let mut rhs = p.rhs?;
            while let Operand::Expr {
                a: Some(inner),
                op: None,
                b: None,
            } = &program.operands[rhs as usize]
            {
                rhs = *inner;
            }
            let Operand::Path { elems, .. } = &program.operands[rhs as usize] else {
                return None;
            };
            let [Elem::Object(obj)] = elems.as_slice() else {
                return None;
            };
            files
                .placed
                .iter()
                .find(|pl| pl.obj == *obj && pl.obj.unit == unit && pl.script.is_some())
        })
}

/// `config` with the scripts of every walker whose new asks of the gameplay
/// viewport are all read (AG.27, AH.150) listed to run every tick, with
/// those of his attacks' whose own are (openspec AU.1,
/// [`WalkerScripts::attack_scripts`]); a walker with one not read yet, or
/// whose definition names no minion, stays off the whitelist, so what it
/// would wake stays inert.
pub fn with_walkers(mut config: Config, walkers: &[WalkerScripts]) -> Config {
    if let Whitelist::Only { scripts, .. } = &mut config.whitelist {
        for w in walkers.iter().filter(|w| w.asks == 0 && w.minion.is_some()) {
            scripts.extend(w.scripts.iter().cloned());
            // What his attacks spawn, each whose asks are read (openspec
            // AU.1).
            scripts.extend(w.attack_scripts.iter().cloned());
        }
    }
    config
}

/// `config` without the levels' music scripts ([`MUSIC_SCRIPTS`], openspec
/// AR.16): what a probe sets the music's draws apart by. The TV's run lists
/// them since AR.16 read the original's music controller rolling the same
/// placed environment sounds' repeat timers.
pub fn without_music(mut config: Config) -> Config {
    if let Whitelist::Only { scripts, .. } = &mut config.whitelist {
        scripts.retain(|s| !MUSIC_SCRIPTS.contains(&s.as_str()));
    }
    config
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

/// The TV's listing for the level whose archive is `name`, its own file
/// linked as `level`: chapter 1's ([`fight_config`], the first fight's
/// placements by name) on chapter 1, AH.27's ([`smash_config`]) on any
/// other, whose placements named as chapter 1's fight's are its own (level
/// 214 holds an `Enemy_Chompy(4)` and a `Setup_001_Controller`), with its
/// first encounter's placements and clones by name where the TV runs one
/// ([`encounter`], openspec AS.54) and its intro ([`intro`], AS.47), a hub
/// stage's own scripts ([`HUB_SCRIPTS`], AS.46); either with
/// [`with_level`] (AS.51).
pub fn level_config(mode: Mode, files: &Files, level: u16, name: &str) -> Config {
    let mut config = if guac_extract::levels::FIRST_RUN.contains(&name) {
        fight_config(mode)
    } else {
        smash_config(mode)
    };
    if let (Some(e), Whitelist::Only { placements, .. }) = (encounter(name), &mut config.whitelist)
    {
        placements.extend(
            e.placements
                .iter()
                .chain(e.clones)
                .chain(e.steered)
                .map(|s| (*s).to_owned()),
        );
    }
    // Its intro (openspec AS.47).
    if let (Some(i), Whitelist::Only { placements, .. }) = (intro(name), &mut config.whitelist) {
        placements.push(i.to_owned());
    }
    // What takes the way on past its fights (openspec AS.59).
    if let Whitelist::Only { placements, .. } = &mut config.whitelist {
        placements.extend(passages(name).iter().map(|s| (*s).to_owned()));
    }
    // A hub stage's sky, its stage's controller, its background and its
    // way on (openspec AS.46, AS.47, AU.6); the filter cells Cali's dialogue
    // reaches (the menus' blurred backdrop) are every level's since AS.53
    // (`members::LEVELS`), the level end's sequencing reaching them too.
    if is_hub(name)
        && let Whitelist::Only { scripts, .. } = &mut config.whitelist
    {
        scripts.extend(HUB_SCRIPTS.iter().map(|s| (*s).to_owned()));
    }
    with_level(config, files, level)
}

/// What takes the way on through a level past its fights (openspec AS.59),
/// by level and placement name, listed and never walked. Level 214's from
/// its first island to its second: once both Elf Nears are beaten the jump
/// pad's activator pans to the jump pad (`PvP_JumpPad_NORMAL(2)`) and wakes
/// it, which launches a hero at its own pitch and speed along his facing,
/// his stick steering him up onto the ledge holding the first ship part
/// (`Ship_part_1(1)`), or with it let go straight up and back onto it (the
/// mover update takes the launch's ground speed off: AS.75); taken after
/// a press, the part goes and wakes the sail (`214_Recover_First_Part_IGC`),
/// whose cutscene, played to its end since AS.82 (or skipped by a
/// player's △), sets him down on the second island's ship; Setup_003 waits
/// for it.
pub const PASSAGES: [(&str, &[&str]); 1] = [(
    "level_214_tutotial_2",
    &[
        "Jumpad_Activator_First_Island",
        "PvP_JumpPad_NORMAL(2)",
        "Ship_part_1(1)",
        "214_Recover_First_Part_IGC",
    ],
)];

/// The placements [`PASSAGES`] lists for the level whose archive is
/// `level`.
pub fn passages(level: &str) -> &'static [&'static str] {
    PASSAGES
        .iter()
        .find(|(l, _)| *l == level)
        .map_or(&[], |(_, p)| p)
}

/// The VM over `program` with every running placement of `files` added
/// unit by unit (`global.bld`'s, then `init_setup.bld`'s, then the level's,
/// by `order`'s units), each under the handle `handles` gives its object,
/// in the order the original's load adds them ([`load_order`]); every
/// placement's object is named, scripted or not. A running placement whose
/// script the program lacks is left out, logged.
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
        for p in load_order(files, unit) {
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

/// The placements of the file linked as `unit` in the order the original's
/// load adds them to its scheduler (openspec AH.195; the design doc's The
/// frame's order): `global.bld`'s [`crate::frame::AWAKE_HEAD`] names first,
/// in that order (AH.59: the globals as its scheduler held them); then
/// those awake from the load ([`awake`] with no hero: no activation range
/// and no buffer), then those that wait asleep for the activity rule, each
/// in the file's root-list order ([`Placed::load`]), not the order its
/// objects lie in. AF.8's fresh state (AH.195's read) holds level 213 so:
/// the 29 awake from its load in root-list order, behind them 54 of the
/// 56 that woke in one frame 13.18 s before it, in root-list order too
/// (`Propellar_Prop` and `Sliding_Rock(5)` at their end, an earlier
/// frame's doing), and the 468 still asleep in it. One woken later joins
/// the awake list's end ([`Vm::set_activity`]), as a character file's
/// running placements do at his join ([`join_request`], in this order too:
/// of the 67 character files the part links, Cynder's alone lies otherwise,
/// his `Minion_Include_Ghost` first among its objects and last on its root
/// list, behind his definition and tuning actor; AH.195's review).
pub fn load_order(files: &Files, unit: u16) -> Vec<&Placed> {
    let head = |p: &Placed| {
        crate::frame::AWAKE_HEAD
            .iter()
            .position(|n| *n == p.name)
            .unwrap_or(usize::MAX)
    };
    let mut placed: Vec<&Placed> = files.placed.iter().filter(|p| p.obj.unit == unit).collect();
    placed.sort_by_key(|p| (head(p), !awake(p.range, p.buffer, false, None), p.load));
    placed
}

/// The portal manager, whose macros the join calls as its own tick would
/// ([`join_request`], [`join_activate`]; stand-in ledger: `join`).
pub const PORTAL_MANAGER: &str = "Spyro_PortalManager";

/// The join, its first half (openspec AH.17; stand-in ledger: `join`):
/// what the portal and the portal manager do when a figure goes on the
/// portal, by the original's own macros with only their scheduling ours.
/// The portal manager asks for the figure's character file through
/// `Portal - Load Portal Object Stream` with the first player's slot's
/// loaded data (slot 0 of `portal control data.$loaded objects`), as the
/// portal control's tag handling does; then the character file's running
/// placements (its definition, its tuning actor) join the VM, listed for
/// every tick, under `handles`, in the order its load adds them
/// ([`load_order`], openspec AH.195): the definition's first tick registers
/// the minion into that loaded data (`Portal - Define Minion`). The portal's
/// tag, the level streaming and the entry beam aren't run (AJ's, AM's):
/// the stream's request is answered done once the definition has
/// registered its minion ([`answer_stream_done`]).
pub fn join_request(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    files: &Files,
    unit: u16,
    handles: &HashMap<Obj, Handle>,
) -> Result<Vec<PlacementId>, Halt> {
    let pm = portal_manager(vm, host.world)?;
    let loaded = slot_zero(vm)?;
    vm.call_for(
        pm,
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
    for p in load_order(files, unit) {
        // His file loaded again after a swap took it off (openspec AH.105,
        // [`join_deactivate`]): each placement as the file has it, as a
        // stream loaded anew makes it.
        if let Some(&h) = handles.get(&p.obj)
            && host.world.actors.get(&h).is_some_and(|a| a.removed)
        {
            host.world.actors.insert(h, actor_of(p));
        }
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
///
/// `hero` must name no actor the run has held (openspec AH.191): the
/// original's activation clones the template afresh, and his setup's
/// `Combat - Creature Definition` makes his description (his movement
/// parameters off his setup's numbers) only when none in `combat control
/// data`'s `creature descriptions` has him as its `creature`. Under the
/// last minion's handle he found the last one's and walked at its numbers,
/// so a held handle is refused before anything runs.
///
/// The activation at once, with no entry beam: the headless runs' join. The
/// TV's is [`join_enter`], the beam first (openspec AH.224).
pub fn join_activate(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    hero: Handle,
) -> Result<PlacementId, Halt> {
    let ready = join_ready(vm, host, hero)?;
    activate(vm, host, &ready, Value::Entity(hero))?;
    joined(vm, host, ready.pm, hero)
}

/// What a join has once its minion may activate ([`join_ready`]).
struct Ready {
    pm: PlacementId,
    minion: Value,
    desc: Value,
}

/// The join up to its activation: a held handle refused, the figure's
/// links made ([`crate::figure::enter`]) and the minion's template
/// answering as `hero`.
fn join_ready(vm: &mut Vm, host: &mut WorldHost<'_, '_>, hero: Handle) -> Result<Ready, Halt> {
    if host.world.actors.contains_key(&hero) {
        return Err(outside(format!(
            "{hero:#x} names an actor of the run's already: a minion joins as a new one"
        )));
    }
    crate::figure::enter(vm, host.world)?;
    ready_of(vm, host, hero)
}

/// The portal manager and the minion and description the definition
/// registered in the first player's slot, its template answering as `hero`.
fn ready_of(vm: &mut Vm, host: &mut WorldHost<'_, '_>, hero: Handle) -> Result<Ready, Halt> {
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
    answer_stream_done(vm, host, pm, loaded)?;
    Ok(Ready { pm, minion, desc })
}

/// The join's stream answered done (openspec AS.53), once his definition
/// has registered its minion through it: the portal's request for `loaded`,
/// which names no level to load, taken off its queue by the portal's own
/// `Remove Portal Object Stream`, the queue's update dropped with it. His
/// file is linked and its placements joined already; left queued, the
/// stream loader would load it, find it named nothing and queue it again
/// every other step (stand-in ledger: `level-request`).
fn answer_stream_done(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    pm: PlacementId,
    loaded: Value,
) -> Result<(), Halt> {
    const DONE: &str = "Remove Portal Object Stream";
    if vm.program().macro_named(DONE).is_some() {
        vm.call_for(pm, DONE, &[("use loaded data", loaded)], &[], host)?;
    }
    Ok(())
}

/// `Game - Activate/Deactivate Minion` for `ready`'s minion at `at`, as
/// `Portal - Entry Activate Minion` calls it.
fn activate(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    ready: &Ready,
    at: Value,
) -> Result<(), Halt> {
    let player = Value::Entity(host.world.controllers[0]);
    vm.call_for(
        ready.pm,
        "Game - Activate/Deactivate Minion",
        &[
            ("minion", ready.minion),
            ("spawn location", at),
            ("player", player),
            ("keep minion ref in player set", Value::Int(0)),
            ("input minion data", ready.desc),
            ("restore hp", Value::Int(0)),
            ("in level transition", Value::Int(1)),
        ],
        &[],
        host,
    )?;
    Ok(())
}

/// After the activation: the portal in adventure mode, the minion's script
/// and every scripted clone the activation made on the VM, the minion's the
/// activator, and the level's intro let go with his minion in the level to
/// take (openspec AS.47, [`release_intro`]). Returns his placement.
fn joined(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    pm: PlacementId,
    hero: Handle,
) -> Result<PlacementId, Halt> {
    release_intro(vm, host.world);
    adventure_mode(vm, host, pm)?;
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

/// The macros the original's entry beam runs (openspec AH.224), all in
/// `global.bld`'s `Spyro_PortalEntry_Macros.ai`.
pub const ENTRY_INIT: &str = "Portal - Init Minion Enter Effect";
pub const ENTRY_MANAGE: &str = "Portal - Manage Portal Entry Beams";
pub const ENTRY_CLEAN_UP: &str = "Portal - Entry Effect Clean Up";

/// Where a TV join stands after [`join_enter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// His minion activated at once (a part without the entry beam's
    /// macros): his placement.
    Activated(PlacementId),
    /// His entry beam runs ([`entry_beams`]); his minion activates as its
    /// column lands.
    Beam,
}

/// The join's second half on the TV (openspec AH.224): as [`join_activate`]
/// up to the activation, then the original's entry rather than the
/// activation at once. `Portal - Enter Object` makes his entry beam through
/// [`ENTRY_INIT`] (his minion, the first player's controller, his
/// description, no element forced, spawned where `hero` stands: a follow
/// actor there, the beam's `spawn loc`), and the portal manager's state
/// machine runs every beam each tick ([`entry_beams`]): the column dropping
/// onto his spot over 0.25 s (`PortalEntry_Column`), then at its landing
/// the crater, the glow plane, the shock ring and the landing flash, his
/// minion activated there (`Portal - Entry Activate Minion`, which calls
/// `Game - Activate/Deactivate Minion` as [`join_activate`] does) and his
/// element's entry macros (`PortalEntry_<element>.ai`) after. A part without
/// those macros, or whose `portal control data` keeps no `entry beams`,
/// activates him at once ([`Entry::Activated`]), and so does `beam` false
/// (a run whose beam stopped before). The scheduling is ours: the beam
/// made at the join's second half, run from that step on
/// (stand-in ledger: `join`).
pub fn join_enter(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    hero: Handle,
    beam: bool,
) -> Result<Entry, Halt> {
    let ready = join_ready(vm, host, hero)?;
    let program = vm.program_arc();
    let beams = entry_beam_count(vm).is_some();
    let init = program.macro_named(ENTRY_INIT).is_some();
    let manage = program.macro_named(ENTRY_MANAGE).is_some();
    if !(beam && beams && init && manage) {
        activate(vm, host, &ready, Value::Entity(hero))?;
        return joined(vm, host, ready.pm, hero).map(Entry::Activated);
    }
    let player = Value::Entity(host.world.controllers[0]);
    let mut args = vec![
        ("spawn minion", ready.minion),
        ("spawn location", Value::Entity(hero)),
        ("minion controller", player),
        ("minion desc", ready.desc),
    ];
    // `Portal - Enter Object` asks for no element (-1); left out, it reads
    // 0, the first element's icon.
    let force = parameter_named(&program, ENTRY_INIT, "force element");
    if let Some(force) = force.as_deref() {
        args.push((force, Value::Int(-1)));
    }
    vm.call_for(ready.pm, ENTRY_INIT, &args, &[], host)?;
    Ok(Entry::Beam)
}

/// How many entry beams `portal control data` holds; `None` when the
/// program keeps no such set.
pub fn entry_beam_count(vm: &mut Vm) -> Option<usize> {
    let g = vm.program().global_named("portal control data")?;
    let pcd = vm.global(g)?;
    let beams = vm.field(pcd, "entry beams")?;
    vm.set(beams).map(<[Value]>::len)
}

/// One tick of the entry beams (openspec AH.224), as the portal manager's
/// `Portal Control - Global - Update Entry Beams` runs them: nothing while
/// `portal control data` holds none, else [`ENTRY_MANAGE`] in the portal
/// manager's name. `waiting`, the hero whose beam hasn't activated him yet:
/// once this tick's beam has (his actor in the run), the rest of his join
/// ([`join_activate`]'s, the portal in adventure mode and his minion on the
/// VM, the activator) and his placement returned.
pub fn entry_beams(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    waiting: Option<Handle>,
) -> Result<Option<PlacementId>, Halt> {
    if entry_beam_count(vm).unwrap_or(0) == 0 {
        return Ok(None);
    }
    let pm = portal_manager(vm, host.world)?;
    let program = vm.program_arc();
    let (Some(param), Some(g)) = (
        parameter_named(&program, ENTRY_MANAGE, "manage portal data"),
        program.global_named("portal control data"),
    ) else {
        return Ok(None);
    };
    let Some(pcd) = vm.global(g) else {
        return Ok(None);
    };
    vm.call_for(pm, ENTRY_MANAGE, &[(param.as_str(), pcd)], &[], host)?;
    match waiting {
        Some(hero) if host.world.actors.contains_key(&hero) => joined(vm, host, pm, hero).map(Some),
        _ => Ok(None),
    }
}

/// His minion activated at once where his entry beam stopped short of
/// activating him (a stop in it, which [`entry_beams`] returned): every
/// beam cleaned up ([`clear_entry_beams`], a stop there logged), then the
/// activation and the rest of the join as [`join_activate`]'s, where `hero`
/// stands.
pub fn activate_without_beam(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    hero: Handle,
) -> Result<PlacementId, Halt> {
    if let Err(h) = clear_entry_beams(vm, host) {
        warn!(%h, "the entry beam's clean-up stopped");
    }
    let ready = ready_of(vm, host, hero)?;
    activate(vm, host, &ready, Value::Entity(hero))?;
    joined(vm, host, ready.pm, hero)
}

/// Every entry beam cleaned up as the original's [`ENTRY_CLEAN_UP`] does
/// (its column, its follow actor and what it tracks off, the beam out of
/// the set): a hero taken off before his beam activated him leaves none to
/// activate his minion after he's gone.
pub fn clear_entry_beams(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<(), Halt> {
    let Some(n) = entry_beam_count(vm).filter(|n| *n > 0) else {
        return Ok(());
    };
    let pm = portal_manager(vm, host.world)?;
    let program = vm.program_arc();
    let (Some(param), Some(g)) = (
        parameter_named(&program, ENTRY_CLEAN_UP, "entry beam"),
        program.global_named("portal control data"),
    ) else {
        return Ok(());
    };
    for _ in 0..n {
        let Some(pcd) = vm.global(g) else {
            return Ok(());
        };
        let Some(beam) = vm
            .field(pcd, "entry beams")
            .and_then(|s| vm.set(s).and_then(|e| e.first().copied()))
        else {
            return Ok(());
        };
        vm.call_for(pm, ENTRY_CLEAN_UP, &[(param.as_str(), beam)], &[], host)?;
    }
    Ok(())
}

/// What a hero's deactivation took off ([`join_deactivate`]), by handle,
/// each sorted, and where its macro stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deactivated {
    /// Whether his minion ran, and the deactivation's macro ran for it.
    pub minion: bool,
    /// His file's placements that were on.
    pub placements: Vec<Handle>,
    /// The clones of his file's templates that were on: his minion, his
    /// helpers, what his attacks threw and left.
    pub clones: Vec<Handle>,
    /// Where the deactivation stopped short of the original's (openspec
    /// AH.165): its macro stopping (a shutdown macro of his on a member
    /// nothing models), the exit's links finding no place in the program, or
    /// a SHUTDOWN of what it took off stopping (AH.200). The rest ran: the
    /// portal manager runs on and his stream is unloaded.
    pub halted: Option<Halt>,
}

/// The deactivate path (openspec AH.105; stand-in ledger: `join`): the
/// hero's figure off the portal mid-level, the level's scripts running on.
/// The original's in-game portal check marks his character stream unloaded,
/// then `Portal - Exit Object` calls `Game - Activate/Deactivate Minion`
/// with no minion: his minion out of the minion, target, pause and combat
/// sets, the projectiles his creature owns removed (`Remove Projectile`),
/// his hat, low-health actor and camera target removed, his shutdown macros
/// run, his controller inactive, his minion faded out over 0.06 s and
/// removed, the enemies where he stood alerted (`Vanish Alert`). That macro
/// runs here as `Portal - Exit Object` calls it (unless his minion never
/// activated); then, natively, the exit's links undone
/// ([`crate::figure::exit`]); his minion taken off the VM, no longer the
/// activator, and its SHUTDOWN run (openspec AH.200: the original runs it
/// as the fade takes him off, his `Minion - Shutdown` taking his
/// description out of `combat control data`'s set); then the stream's
/// unloading: every placement of unit `unit` and every clone of its
/// templates, `hero` among them, taken off the world and the VM, his
/// sequences stopped, and the SHUTDOWN of each run. Not waited for: the
/// fade (it ignores the pause the TV holds the level in), so his SHUTDOWN
/// runs at the take-off, where the original's ran 5 to 20 frames after the
/// figure left. Not run: the stream's bookkeeping, the exit beam and its
/// pause (AG.29 holds the level), and the marker actor it leaves where he
/// stood (AG.81 stands the next one there).
///
/// A stop in the macro, the links or a SHUTDOWN is kept in
/// [`Deactivated::halted`] (the first) and the rest still runs (openspec
/// AH.165): the macro is the embedder's call
/// in the portal manager's name ([`Vm::call_for`]), so the portal manager
/// ticks on, and his stream is unloaded whatever stopped, so nothing of his
/// file ticks on in the run or runs twice when his figure comes back. What
/// the macro didn't reach stays as it left it (his minion in a set it would
/// have taken him out of).
pub fn join_deactivate(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    files: &Files,
    unit: u16,
    hero: Handle,
) -> Deactivated {
    let minion = vm.placement_of(hero).is_some_and(|m| !vm.removed(m));
    let mut halted = match minion {
        true => deactivate_minion(vm, host).err(),
        // Taken off while his entry beam ran (openspec AH.224): the beam
        // goes with him, so nothing activates his minion after he's gone.
        false => clear_entry_beams(vm, host).err(),
    };
    if let Err(h) = crate::figure::exit(vm, host.world) {
        match &halted {
            None => halted = Some(h),
            Some(_) => warn!(%h, "his figure's exit links didn't undo either"),
        }
    }
    // His minion off first, no longer the activator (the next one's is),
    // and his SHUTDOWN run while his actor stands (openspec AH.200): the
    // original's keyframer takes the minion off ahead of the stream, and his
    // `Minion - Shutdown` takes him out of `Minions_Loaded` and his
    // description out of `combat control data`'s set.
    vm.set_activator(hero, false);
    vm.remove(hero);
    shut_down(vm, host, &mut halted);
    let (mut placements, clones) = {
        let world = &mut *host.world;
        let mut placements = Vec::new();
        for p in files.placed.iter().filter(|p| p.obj.unit == unit) {
            let Some(&h) = world.objects.get(&p.obj) else {
                continue;
            };
            if let Some(a) = world.actors.get_mut(&h)
                && !a.removed
            {
                a.removed = true;
                placements.push(h);
            }
            vm.remove(h);
        }
        let mut clones: Vec<Handle> = world
            .actors
            .iter()
            .filter(|(_, a)| a.obj.is_none() && !a.removed)
            .filter(|(_, a)| a.template.is_some_and(|t| t.unit == unit))
            .map(|(h, _)| *h)
            .collect();
        clones.sort_unstable();
        for &h in &clones {
            if let Some(a) = world.actors.get_mut(&h) {
                a.removed = true;
            }
            vm.remove(h);
        }
        let gone: std::collections::HashSet<Handle> =
            clones.iter().chain(&placements).copied().collect();
        for s in &mut world.sequences {
            if gone.contains(&s.actor) || s.actor == hero {
                s.stopped = true;
            }
        }
        (placements, clones)
    };
    // Then the SHUTDOWN of what the stream's unloading took off.
    shut_down(vm, host, &mut halted);
    placements.sort_unstable();
    Deactivated {
        minion,
        placements,
        clones,
        halted,
    }
}

/// The SHUTDOWN of each placement taken off with one to run ([`Vm::shut_down`]),
/// each stop kept in `halted` when it holds none yet, logged otherwise: the
/// deactivation's own work, which stops nothing else (openspec AH.165).
fn shut_down(vm: &mut Vm, host: &mut WorldHost<'_, '_>, halted: &mut Option<Halt>) {
    let mut keep = |h: Halt| match halted {
        None => *halted = Some(h),
        Some(_) => warn!(%h, "a SHUTDOWN of his stopped too"),
    };
    loop {
        match vm.shut_down(host) {
            Ok(report) => {
                report.stopped.into_iter().for_each(&mut keep);
                break;
            }
            // Strict mode's stop: the one it ran is off the queue, so the
            // rest run on.
            Err(h) => keep(h),
        }
    }
}

/// `Game - Activate/Deactivate Minion` with no minion, as `Portal - Exit
/// Object` calls it mid-level: his player's controller, `keep minion ref in
/// player set` 1, the slot's minion description, no level transition.
fn deactivate_minion(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<(), Halt> {
    let pm = portal_manager(vm, host.world)?;
    let loaded = slot_zero(vm)?;
    let info = vm.field(loaded, "object info").unwrap_or(Value::Null);
    let desc = vm
        .field(info, "minion additional info")
        .unwrap_or(Value::Null);
    let player = Value::Entity(host.world.controllers[0]);
    vm.call_for(
        pm,
        "Game - Activate/Deactivate Minion",
        &[
            ("minion", Value::Null),
            ("player", player),
            ("keep minion ref in player set", Value::Int(1)),
            ("input minion data", desc),
            ("restore hp", Value::Int(0)),
            ("in level transition", Value::Int(0)),
        ],
        &[],
        host,
    )?;
    Ok(())
}

/// The scripts hide the player's HUD: `UI Containers`' `player hud
/// container` holds a `hide state` over 0, as `UI - Adjust UI Visibility`
/// leaves it for `Game - Cutscene`'s hide UI and the dialogue's UI (on
/// AH.8's route 1 or 2 through the pit's intro and Norticus' scene, 0
/// otherwise). False until the UI's scripts have made it.
pub fn player_hud_hidden(vm: &Vm) -> bool {
    let Some(containers) = vm
        .program()
        .global_named("UI Containers")
        .and_then(|g| vm.global_peek(g))
        .map(|v| vm.value(v))
    else {
        return false;
    };
    vm.field(containers, "player hud container")
        .and_then(|hud| vm.field(vm.value(hud), "hide state"))
        .and_then(|state| vm.value(state).as_f32())
        .is_some_and(|state| state > 0.0)
}

/// The hero's health as his scripts hold it: his minion's
/// `description.creature info.hp`.
pub fn health(vm: &Vm, minion: PlacementId) -> Option<i32> {
    let d = vm.local(minion, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_i32()
}

/// The hero's health set where his scripts hold it, as a bite leaves it: a
/// test's, or AH.62's test hook doing a bite's work. False when he has no
/// health to set.
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

/// An experience orb out in the level (openspec AH.20, AH.25's report):
/// its actor, its worth, where it is and how it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orb {
    pub actor: Handle,
    pub worth: i32,
    pub at: Vec3,
    pub state: OrbState,
}

/// Where an orb is in its life (openspec AH.281).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbState {
    /// Still coming down (in the control's `balls launching`): a hero
    /// takes one only once it's down.
    Falling,
    /// Down, waiting for a hero to come within reach.
    Down,
    /// Taken and on its way in: a joined player's `xp balls collecting`
    /// (or the enemies' collecting set) holds it until it arrives, its
    /// actor moving in, and the arrival takes it off.
    Flying,
}

/// Every orb out (`combat control data`'s `xp ball control`: `all active
/// balls` and `balls launching`) and every one flying in (each joined
/// player's `player joined data.xp balls collecting`, and the control's
/// `XP Enemies are collecting`), each once, the active first; none before
/// the combat data is made. Taking an orb moves it out of the active sets
/// into the collecting one, so without those the TV stopped drawing it the
/// step it was taken and never drew its flight (AH.281).
pub fn orbs(vm: &Vm, world: &ScriptWorld) -> Vec<Orb> {
    let control = vm
        .program()
        .global_named("combat control data")
        .and_then(|g| vm.global_peek(g))
        .map(|c| vm.value(c))
        .and_then(|c| vm.field(c, "xp ball control"))
        .map(|v| vm.value(v));
    let Some(control) = control else {
        return Vec::new();
    };
    let balls = |set: Option<Value>| -> Vec<(Handle, i32)> {
        let Some(set) = set else {
            return Vec::new();
        };
        vm.set(set)
            .map(<[Value]>::to_vec)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|b| {
                let b = vm.value(b);
                let ball = vm.field(b, "xp ball").map(|v| vm.value(v))?;
                let Value::Entity(orb) = vm.field(ball, "orb").map(|v| vm.value(v))? else {
                    return None;
                };
                let worth = vm
                    .field(ball, "xp val")
                    .and_then(|v| vm.value(v).as_i32())?;
                Some((orb, worth))
            })
            .collect()
    };
    let field = |v: Value, name: &str| vm.field(v, name).map(|v| vm.value(v));
    let players: Vec<Value> = vm
        .program()
        .global_named("portal control data")
        .and_then(|g| vm.global_peek(g))
        .map(|v| vm.value(v))
        .and_then(|pcd| field(pcd, "player joined data"))
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default();
    let mut collecting: Vec<(Handle, i32)> = players
        .into_iter()
        .flat_map(|p| balls(field(vm.value(p), "xp balls collecting")))
        .collect();
    collecting.extend(balls(field(control, "XP Enemies are collecting")));
    listed_orbs(
        &balls(field(control, "all active balls")),
        &balls(field(control, "balls launching")),
        &collecting,
        world,
    )
}

/// The orbs the sets hold, each once in the order given (the active, then
/// those launching, then those flying in), its state by the set that has
/// it (collecting over launching over the rest); one whose actor is gone
/// isn't listed.
fn listed_orbs(
    active: &[(Handle, i32)],
    launching: &[(Handle, i32)],
    collecting: &[(Handle, i32)],
    world: &ScriptWorld,
) -> Vec<Orb> {
    let has = |set: &[(Handle, i32)], h: Handle| set.iter().any(|b| b.0 == h);
    let mut out: Vec<Orb> = Vec::new();
    for &(actor, worth) in active.iter().chain(launching).chain(collecting) {
        if out.iter().any(|o| o.actor == actor) {
            continue;
        }
        let Some(a) = world.actor(actor).filter(|a| !a.removed) else {
            continue;
        };
        let state = if has(collecting, actor) {
            OrbState::Flying
        } else if has(launching, actor) {
            OrbState::Falling
        } else {
            OrbState::Down
        };
        out.push(Orb {
            actor,
            worth,
            at: a.position,
            state,
        });
    }
    out
}

/// Where the kind's running count stands (openspec AH.21), none before the
/// level's loot data is made.
pub fn loot_index(vm: &Vm, world: &ScriptWorld, rarity: Rarity) -> Option<f32> {
    let count = loot_count(vm, world, rarity)?;
    vm.field(count, "DropRateIndex")
        .and_then(|v| vm.value(v).as_f32())
}

/// The combat manager's next alert `steps` steps on, as a recorded state
/// has its 0.25 s check's phase (openspec AH.18's headless runs, AH.25's
/// dev entry): `combat control data`'s `alert next time` set between this
/// step's game timer and the next's, its check running before the
/// placements' ticks. False before the combat data is made.
pub fn alert_in(vm: &mut Vm, steps: u32) -> bool {
    let Some(g) = vm.program().global_named("combat control data") else {
        return false;
    };
    let Some(control) = vm.global(g).map(|v| vm.value(v)) else {
        return false;
    };
    let Some(now) = vm
        .field(control, "game timer")
        .and_then(|v| vm.value(v).as_f32())
    else {
        return false;
    };
    let next = now + (steps as f32 - 0.5) / 60.0;
    vm.set_field(control, "alert next time", Value::Float(next))
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
    let mut added: Vec<PlacementId> = world
        .adopt(&program)
        .into_iter()
        .map(|(h, name, sid)| vm.add(h, &name, sid))
        .collect();
    // A running placement a script reset after it was taken off runs its
    // script again from its STARTUP under its handle (openspec AS.60), as a
    // character's file joining again does (AH.105).
    for (h, o) in std::mem::take(&mut world.revived) {
        let Some(p) = world.templates.get(&o) else {
            continue;
        };
        let off = vm.placement_of(h).is_none_or(|id| vm.removed(id));
        if p.activity_state != RUNNING || !off {
            continue;
        }
        if let Some(sid) = world.script_of(o, &program) {
            added.push(vm.add_at(h, &p.name, sid, o));
        }
    }
    added
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
pub fn running(vm: &Vm, world: &ScriptWorld, suffix: &str) -> Option<PlacementId> {
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

/// The portal mode the level plays in once a player is in (openspec AH.28),
/// a specialization of `Portal - Set Portal Mode`.
pub const ADVENTURE_MODE: &str = "Portal Mode - Adventure Mode";

/// The portal's mode as `portal control data` holds it: the specialization
/// its `current mode` names, by name (`Portal Mode - Bootup` until a level
/// starts play).
pub fn portal_mode(vm: &mut Vm) -> Option<String> {
    let g = vm.program().global_named("portal control data")?;
    let pcd = vm.global(g)?;
    let mode = vm.field(pcd, "current mode").map(|v| vm.value(v))?;
    let Value::Object(obj) = mode else {
        return None;
    };
    vm.program()
        .defs
        .iter()
        .find(|d| d.obj == obj)
        .map(|d| d.display.clone())
}

/// The portal put in adventure mode as the level's first minion joins
/// (openspec AH.28): the original's level load clones the level
/// initializer, whose `Game - Init Minions in Level` sets it once a player
/// plays outside an arena, which no box runs, so the join calls the mode's
/// own macro (stand-in ledger: `join`). Until then the UI's tick leaves
/// every dialogue hidden, and the cannon fires through one. Nothing when
/// the program has no such macro or the portal is in it already.
fn adventure_mode(vm: &mut Vm, host: &mut WorldHost<'_, '_>, pm: PlacementId) -> Result<(), Halt> {
    let defined = vm
        .program()
        .defs
        .iter()
        .any(|d| d.display.eq_ignore_ascii_case(ADVENTURE_MODE));
    if !defined || portal_mode(vm).as_deref() == Some(ADVENTURE_MODE) {
        return Ok(());
    }
    // A call skips the macro's interface, so its one parameter is handed
    // the default the interface gives it (`portal control data`), under the
    // name the file spells it with (trailing spaces and all).
    let program = vm.program_arc();
    let (Some(param), Some(g)) = (
        parameter_named(&program, ADVENTURE_MODE, "portal data in"),
        program.global_named("portal control data"),
    ) else {
        return Ok(());
    };
    let Some(pcd) = vm.global(g) else {
        return Ok(());
    };
    vm.call_for(pm, ADVENTURE_MODE, &[(param.as_str(), pcd)], &[], host)?;
    Ok(())
}

/// The name a macro's interface spells a parameter with, found by that name
/// trimmed (the file keeps trailing spaces in some), any case.
pub(crate) fn parameter_named(
    program: &Program,
    macro_name: &str,
    trimmed: &str,
) -> Option<String> {
    use guac_script::Ins;
    let def = program
        .defs
        .iter()
        .find(|d| d.placed.is_some() && d.display.eq_ignore_ascii_case(macro_name))?;
    let (sid, pos) = def.placed?;
    let ops = &program.scripts.get(sid as usize)?.ops;
    let &iface = ops.get(pos as usize + 1)?;
    if program.ops[iface as usize] != Ins::Interface {
        return None;
    }
    let n = program.branch(iface) as usize;
    ops.get(pos as usize + 2..=pos as usize + 1 + n)?
        .iter()
        .find_map(|&o| match &program.ops[o as usize] {
            Ins::Parameter(p) if program.sym(p.name).trim().eq_ignore_ascii_case(trimmed) => {
                Some(program.sym(p.name).to_owned())
            }
            _ => None,
        })
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
    /// one whose script `program` doesn't hold is dropped, and so is one
    /// taken off before it joins (the as-214-walk review: level 214's skip
    /// takes its cutscene's second ship off the tick that clones it, whose
    /// script, started anyway, cloned a set of parts nothing took off). Its
    /// script never started, so no SHUTDOWN is owed either (a reading: what
    /// the original does with a clone taken off the tick it's made isn't
    /// recorded).
    pub fn adopt(&mut self, program: &Program) -> Vec<(Handle, String, ScriptId)> {
        std::mem::take(&mut self.scripted)
            .into_iter()
            .filter_map(|(h, t)| {
                if self.removed(h) {
                    return None;
                }
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

    /// A template's placement name, an actor's or an emitter's.
    pub fn name_of(&self, t: Obj) -> Option<String> {
        self.templates
            .get(&t)
            .map(|p| p.name.clone())
            .or_else(|| self.emitters.get(&t).cloned())
    }

    /// The emitter an actor is, by its name (openspec AH.87): a clone's
    /// template's, or a placement's own; what its particles draw. None for
    /// anything else.
    pub fn emitter_of(&self, h: Handle) -> Option<&str> {
        let a = self.actors.get(&h)?;
        let t = a.template.or(a.obj)?;
        self.emitters.get(&t).map(String::as_str)
    }

    /// The emitter placement an actor is a clone of (or is), by file and
    /// object (openspec AS.17): what finds a clone of a level's own emitter
    /// in that level's particles part, whose names repeat (chapter 1 places
    /// two `Wood Plank on Fire`). None for anything but an emitter's.
    pub fn emitter_placement(&self, h: Handle) -> Option<Obj> {
        let a = self.actors.get(&h)?;
        let t = a.template.or(a.obj)?;
        self.emitters.contains_key(&t).then_some(t)
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
    /// The entry beam that stopped (openspec AH.224): the beams run no more
    /// in this run, so a stop isn't met again every step.
    pub entry_stopped: Option<Halt>,
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
    let mut player_loop = PlayerLoop::read(&program);
    // The camera's frame runs on the camera's members (openspec AG.28),
    // which AH.16's listing doesn't register: its run steps without it.
    if config
        .members
        .get(guac_script::MemberId::named("actorinfo", "players"))
        .is_none()
    {
        player_loop.camera = None;
        player_loop.cam_target = None;
    }
    let vm = init_chain(program, files, &handles, order, config);
    let mut script_world = ScriptWorld::new(files, &handles, objects);
    // What a listed script wakes off the whitelist is logged and listed
    // (decision 6), as the headless runs list it.
    script_world.set_inert(vm.inert());
    // The asks read as gating play, answered off the camera's view
    // (openspec AH.150).
    script_world.answer_from_view(crate::on_screen::answered(
        vm.program(),
        crate::on_screen::READ,
    ));
    world.insert_resource(script_world);
    world.insert_resource(ScriptRuntime {
        vm,
        generator: Generator::seeded(guac_script::session_seed()),
        player_loop,
        steps: 0,
        stopped: Vec::new(),
        entry_stopped: None,
    });
}

/// The scripts' system in `FixedUpdate`'s [`Frame::Scripts`]: nothing runs
/// until a level's script part is installed.
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        crate::frame::ensure(app);
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

    const GRUNT: Obj = Obj { unit: 4, index: 7 };
    const SPRITE: Obj = Obj { unit: 4, index: 9 };
    const NODE: Obj = Obj { unit: 4, index: 8 };

    /// One actor placement with a waypoint, a sprite that's only an `_ID`,
    /// and the world over them.
    fn world() -> (ScriptWorld, Handle, [Handle; 7]) {
        let files = Files {
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: vec![Placed {
                id: 41,
                buffer: 12.5,
                waypoints: vec![NODE],
                ..Placed::named(GRUNT, "Grunt", Some("C:/x/Grunt.ai"), RUNNING)
            }],
            ids: HashMap::from([(GRUNT, 41), (SPRITE, 77)]),
            points: HashMap::new(),
            turns: HashMap::new(),
        };
        let handles = headless_handles(&files);
        let objects = headless_objects(&handles);
        let grunt = handles[&GRUNT];
        (ScriptWorld::new(&files, &handles, objects), grunt, objects)
    }

    /// AH.281: an orb taken (only in a collecting set now, out of the
    /// active ones) is still listed, flying, until its actor goes; each orb
    /// once, its state by the set that holds it.
    #[test]
    fn an_orb_flying_in_stays_listed_until_its_actor_goes() {
        let (mut world, _, _) = world();
        let at = |x: f32| Actor {
            position: Vec3::new(x, 0.0, 0.0),
            ..Actor::blank()
        };
        for (h, x) in [(900, 1.0), (901, 2.0), (902, 3.0), (903, 4.0)] {
            world.actors.insert(h, at(x));
        }
        let (down, falling, flying, gone) = ((900, 1), (901, 1), (902, 1), (903, 1));
        world.actors.get_mut(&903).unwrap().removed = true;
        let listed = listed_orbs(&[down, falling], &[falling], &[flying, gone, down], &world);
        let states: Vec<(Handle, OrbState, f32)> =
            listed.iter().map(|o| (o.actor, o.state, o.at.x)).collect();
        assert_eq!(
            states,
            [
                (900, OrbState::Flying, 1.0),
                (901, OrbState::Falling, 2.0),
                (902, OrbState::Flying, 3.0),
            ],
            "the taken one listed at its actor; the gone one not"
        );
        // Taken off the collecting set on arrival: no longer listed.
        assert!(listed_orbs(&[], &[], &[], &world).is_empty());
        assert_eq!(
            listed_orbs(&[down], &[], &[], &world)[0].state,
            OrbState::Down
        );
    }

    /// AH.87: a clone of an emitter answers with the emitter's name, as
    /// a placement that's one does; a clone of anything else with none.
    /// The files list every emitter by name, and a template's name reads
    /// an emitter's too.
    #[test]
    fn a_clone_of_an_emitter_names_it_and_nothing_else_does() {
        const TRAIL: Obj = Obj { unit: 4, index: 12 };
        let files = Files {
            placed: vec![Placed::named(GRUNT, "Grunt", None, RUNNING)],
            emitters: HashMap::from([(TRAIL, "009_LavaBall_Trail".to_owned())]),
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let clone_of = |world: &mut ScriptWorld, h: Handle, t: Obj| {
            world.actors.insert(
                h,
                Actor {
                    template: Some(t),
                    ..Actor::blank()
                },
            );
        };
        clone_of(&mut world, 900, TRAIL);
        clone_of(&mut world, 901, GRUNT);
        assert_eq!(world.emitter_of(900), Some("009_LavaBall_Trail"));
        assert_eq!(world.emitter_of(901), None);
        assert_eq!(world.emitter_of(handles[&GRUNT]), None);
        assert_eq!(world.emitter_of(902), None, "no actor");
        world.actors.insert(
            903,
            Actor {
                obj: Some(TRAIL),
                ..Actor::blank()
            },
        );
        assert_eq!(world.emitter_of(903), Some("009_LavaBall_Trail"));
        assert_eq!(world.name_of(TRAIL).as_deref(), Some("009_LavaBall_Trail"));
        assert_eq!(world.name_of(GRUNT).as_deref(), Some("Grunt"));
        // AS.17: the placement it's a clone of (or is), by file and object.
        assert_eq!(world.emitter_placement(900), Some(TRAIL));
        assert_eq!(world.emitter_placement(903), Some(TRAIL));
        assert_eq!(world.emitter_placement(901), None, "a clone of an actor");
        assert_eq!(world.emitter_placement(handles[&GRUNT]), None);
        assert_eq!(world.emitter_placement(902), None, "no actor");
    }

    /// The emitters a file places are read by name beside its actors.
    #[test]
    fn the_files_list_each_emitter_by_name() {
        let s = guac_formats::level::builder::sample();
        let igz = Igz::parse(&s.bytes).unwrap();
        let files = Files::read(&[(3, &igz)]).unwrap();
        let names: Vec<&str> = files.emitters.values().map(String::as_str).collect();
        assert_eq!(names, ["fire_house(1)"]);
        let (obj, _) = files.emitters.iter().next().unwrap();
        assert_eq!(obj.unit, 3);
        assert_eq!(
            Some(obj.index as usize),
            igz.object_at(s.emitter).map(|(i, _)| i)
        );
    }

    /// AS.52: each waypoint a file holds is read with its turn beside its
    /// place, and the world the scripts read it in starts with that turn
    /// (the outro's camera looks from its waypoint as the waypoint faces).
    #[test]
    fn the_files_read_each_waypoints_place_and_turn() {
        let s = guac_formats::level::builder::sample();
        let igz = Igz::parse(&s.bytes).unwrap();
        let files = Files::read(&[(3, &igz)]).unwrap();
        let obj = |l| Obj {
            unit: 3,
            index: igz.object_at(l).unwrap().0 as u32,
        };
        let [w0, w1, w2] = s.waypoints.map(obj);
        assert_eq!(files.points[&w1], Vec3::new(12.0, 43.0, -90.0));
        let turn = |facing| Angles {
            tilt: 0.0,
            facing,
            roll: 0.0,
        };
        assert_eq!(
            [w0, w1, w2].map(|w| files.turns[&w]),
            [turn(0.0), turn(45.0), turn(90.0)]
        );
        let handles = HashMap::new();
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        assert_eq!(world.point_turns[&w1], turn(45.0));
    }

    fn at(namespace: &str, name: &str, receiver: Option<Value>) -> Access {
        Access {
            placement: 1,
            receiver,
            member: MemberId::named(namespace, name),
            other_point: None,
            via: None,
            op: None,
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

    /// A world whose interface holds a file string for each of `texts`, in
    /// order: the world and each string's object.
    fn world_with_texts(texts: &[&str]) -> (ScriptWorld, Vec<Value>) {
        let (mut w, ..) = world();
        let mut keeper = guac_interface::load::load(&[], None).keeper;
        let values = texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                keeper.model_mut().add_file_string(
                    Obj {
                        unit: 1,
                        index: 9000 + i as u32,
                    },
                    t,
                    guac_interface::Str {
                        units: Vec::new(),
                        flags: 0,
                        integer_digits: 0,
                        decimal_digits: 0,
                    },
                )
            })
            .collect();
        w.set_interface(keeper);
        (w, values)
    }

    /// AS.15: a request whose text names a level on the disc is a level
    /// change, held (the member reads it on later steps, taken once); one
    /// naming none (a stream's path, a level not on the disc, a string the
    /// world can't read without the interface's model) reads none the next
    /// step, kept among the streams with its text; the stream comes and
    /// goes under a held change.
    #[test]
    fn a_level_change_is_held_and_a_stream_read_as_loaded() {
        let level = at("system", "load level", None);
        let (mut w, v) = world_with_texts(&["Level_214_Tutotial_2", "debug/debug", "Level_999"]);
        let (to_214, debug, missing) = (v[0], v[1], v[2]);
        w.set_levels(Arc::new(
            [
                "level_214_tutotial_2".to_owned(),
                "level_213_tutotial".to_owned(),
            ]
            .into(),
        ));
        for stream in [debug, missing] {
            w.set(&level, stream, &mut NoEngine).unwrap();
            w.end_step();
            assert_eq!(w.get(&level, &mut NoEngine), Ok(Value::Null), "{stream:?}");
            assert_eq!(w.level_change(), None);
        }
        assert_eq!(
            w.streams.iter().cloned().collect::<Vec<_>>(),
            [Some("debug/debug".to_owned()), Some("Level_999".to_owned())]
        );
        w.set(&level, to_214, &mut NoEngine).unwrap();
        for _ in 0..3 {
            w.end_step();
            assert_eq!(w.get(&level, &mut NoEngine), Ok(to_214), "held");
        }
        assert_eq!(
            w.level_change(),
            Some(&LevelChange {
                name: "level_214_tutotial_2".into(),
                text: "Level_214_Tutotial_2".into(),
                value: to_214,
            })
        );
        assert_eq!(w.requests, [debug, missing, to_214], "taken once each");
        // No interface's model: the text unread, a stream.
        let (mut bare, ..) = world();
        bare.set_levels(Arc::new(["level_214_tutotial_2".to_owned()].into()));
        bare.set(&level, to_214, &mut NoEngine).unwrap();
        bare.end_step();
        assert_eq!(bare.get(&level, &mut NoEngine), Ok(Value::Null));
        assert_eq!(
            (bare.level_change(), bare.streams.back()),
            (None, Some(&None))
        );
        // No level list: nothing changes level.
        let (mut unlisted, v) = world_with_texts(&["Level_214_Tutotial_2"]);
        unlisted.ask_level(v[0]);
        unlisted.end_step();
        assert_eq!(unlisted.level_change(), None);
    }

    /// AS.15's review: the TV's level end is never read as a stream. Its
    /// text unread (no interface's model, as a first run's chapter 1 runs)
    /// or the disc's list missing, it's held (`system.load level` reads it
    /// on), taken once and said why; read again with its text or the list
    /// it's a level change; its text naming no level stays said. A script's
    /// own request unread is a stream as before.
    #[test]
    fn a_level_end_that_cant_be_read_is_held_and_said_never_a_stream() {
        let level = at("system", "load level", None);
        let list = || Arc::new(["level_214_tutotial_2".to_owned()].into());
        let end = Value::Object(Obj { unit: 1, index: 77 });
        let unread = |w: &ScriptWorld| w.level_end().and_then(|e| e.unread.clone());
        // No model: held, said, then read off the part.
        let (mut w, ..) = world();
        w.set_levels(list());
        w.ask_level_end(end, Some("Level_214".into()));
        for _ in 0..3 {
            w.end_step();
            assert_eq!(w.get(&level, &mut NoEngine), Ok(end), "held");
        }
        assert_eq!(unread(&w), Some(Unread::NoText));
        assert_eq!(
            (w.level_change(), w.streams.len(), w.requests.len()),
            (None, 0, 1)
        );
        w.read_level_end(None, None);
        assert_eq!(unread(&w), Some(Unread::NoText), "still no text");
        w.read_level_end(Some("Level_214_Tutotial_2".into()), None);
        assert_eq!(unread(&w), None);
        assert_eq!(
            w.level_change().map(|c| c.name.as_str()),
            Some("level_214_tutotial_2")
        );
        w.end_step();
        assert_eq!(w.requests.len(), 1, "taken once");
        // No list: held, then the list read again.
        let (mut w, v) = world_with_texts(&["Level_214_Tutotial_2", ""]);
        w.ask_level_end(v[0], None);
        w.end_step();
        assert_eq!(unread(&w), Some(Unread::NoList));
        w.read_level_end(None, Some(list()));
        assert_eq!(
            w.level_change().map(|c| c.name.as_str()),
            Some("level_214_tutotial_2")
        );
        // Its text empty (the replay's branch): said, no change.
        let (mut w, v) = world_with_texts(&["Level_214_Tutotial_2", ""]);
        w.set_levels(list());
        w.ask_level_end(v[1], None);
        w.end_step();
        assert_eq!(unread(&w), Some(Unread::NotALevel(String::new())));
        assert_eq!(w.level_change(), None);
        assert_eq!(
            Unread::NotALevel(String::new()).to_string(),
            "its text is empty"
        );
        // A script's own request unread: a stream.
        let (mut w, ..) = world();
        w.set_levels(list());
        w.ask_level(end);
        w.end_step();
        assert_eq!(
            (w.get(&level, &mut NoEngine), w.streams.len()),
            (Ok(Value::Null), 1)
        );
        assert_eq!(w.level_end(), None);
    }

    /// openspec AS.47: an attach node's root is the actor it's on; an actor
    /// named as one is its own; nothing reads nothing. Read-only: a write
    /// goes nowhere.
    #[test]
    fn an_attach_nodes_root_is_the_actor_its_on() {
        let (mut w, grunt, _) = world();
        let root = |w: &mut ScriptWorld, v: Option<Value>| {
            w.get(&at("abstractattachnode", "root", v), &mut NoEngine)
        };
        let node = w.objs.value(Object::Node(grunt, 2));
        assert_eq!(root(&mut w, Some(node)), Ok(Value::Entity(grunt)));
        assert_eq!(
            root(&mut w, Some(Value::Entity(grunt))),
            Ok(Value::Entity(grunt))
        );
        assert_eq!(root(&mut w, Some(Value::Null)), Ok(Value::Null));
        assert!(
            w.set(
                &at("abstractattachnode", "root", Some(node)),
                Value::Null,
                &mut NoEngine
            )
            .is_ok()
        );
        assert_eq!(root(&mut w, Some(node)), Ok(Value::Entity(grunt)));
    }

    /// AS.15: a request's text as an archive's name: lower-cased, a level
    /// folder and the extension taken off; nothing for blank text.
    #[test]
    fn a_requests_text_names_its_archive() {
        assert_eq!(
            level_name("Level_214_Tutotial_2").as_deref(),
            Some("level_214_tutotial_2")
        );
        assert_eq!(
            level_name(" PvP_Level_200_Two_Towers ").as_deref(),
            Some("pvp_level_200_two_towers")
        );
        assert_eq!(
            level_name("level/Level_200.bld").as_deref(),
            Some("level_200")
        );
        assert_eq!(level_name("debug/debug").as_deref(), Some("debug/debug"));
        assert_eq!(level_name("  "), None);
    }

    /// AS.51: a level's own listing gains its definition and the level's
    /// spellings of the listed scripts, a first tick's as a first tick,
    /// and nothing for a script listed as its file spells it, one listed
    /// nowhere or another file's placement; chapter 1's listing names its
    /// first fight's placements, another level's doesn't.
    #[test]
    fn a_levels_listing_gains_its_definition_and_its_spellings() {
        let level = 4;
        let placed = |i: u32, name: &str, script: &str, unit: u16| Placed {
            ..Placed::named(Obj { unit, index: i }, name, Some(script), RUNNING)
        };
        let files = Files {
            placed: vec![
                placed(
                    1,
                    "__Level_Definition",
                    "C:/l/Level_Definition_214.ai",
                    level,
                ),
                placed(2, "pop_to_Camera_X_1", "C:/l/Pop_to_Camera_X.ai", level),
                placed(3, "Boulder", "C:/l/titan_carriable_object.ai", level),
                placed(4, "Scaler", "C:/l/Scale.ai", level),
                placed(5, "Unlisted", "C:/l/Persephone.ai", level),
                placed(6, "Elsewhere", "C:/g/Pop_to_Camera_Y.ai", 1),
            ],
            ..Files::default()
        };
        let base = smash_config(Mode::Play);
        let with = with_level(base.clone(), &files, level);
        let lists = |c: &Config, script: &str| c.whitelist.listing("", script);
        use guac_script::Listed;
        assert_eq!(lists(&base, "C:/l/Level_Definition_214.ai"), Listed::Off);
        assert_eq!(lists(&with, "C:/l/Level_Definition_214.ai"), Listed::Every);
        assert_eq!(lists(&base, "C:/l/Pop_to_Camera_X.ai"), Listed::Off);
        assert_eq!(lists(&with, "C:/l/Pop_to_Camera_X.ai"), Listed::Every);
        assert_eq!(
            lists(&with, "C:/l/titan_carriable_object.ai"),
            Listed::FirstTick
        );
        assert_eq!(lists(&with, "C:/l/Persephone.ai"), Listed::Off);
        let Whitelist::Only {
            scripts,
            first_tick,
            ..
        } = &with.whitelist
        else {
            panic!("a listing");
        };
        let Whitelist::Only {
            scripts: base_scripts,
            first_tick: base_first,
            ..
        } = &base.whitelist
        else {
            panic!("a listing");
        };
        assert_eq!(scripts.len(), base_scripts.len() + 2, "{scripts:?}");
        assert_eq!(first_tick.len(), base_first.len() + 1);
        // Chapter 1's fight is chapter 1's.
        let fight = level_config(Mode::Play, &files, level, guac_extract::levels::LEVELS[0]);
        let other = level_config(Mode::Play, &files, level, "level_212");
        assert!(fight.whitelist.lists(FIRST_FIGHT[0], "C:/x/Any.ai"));
        assert!(!other.whitelist.lists(FIRST_FIGHT[0], "C:/x/Any.ai"));
        assert!(other.whitelist.lists("", "C:/l/Level_Definition_214.ai"));
        // Level 214's first encounter is 214's (openspec AS.54): its
        // placements and its clones' template by name, chapter 1's fight's
        // names off; no other level lists them.
        let e = encounter("level_214_tutotial_2").expect("214's encounter");
        let l214 = level_config(Mode::Play, &files, level, e.level);
        for name in e.placements.iter().chain(e.clones) {
            assert!(l214.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(!other.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(
                !fight.whitelist.lists(name, "C:/x/Any.ai") || FIRST_FIGHT.contains(name),
                "{name}"
            );
        }
        assert!(!l214.whitelist.lists("Enemy_Chompy(4)", "C:/x/Any.ai"));
        // Its intro, 214's alone (openspec AS.47, AS.60).
        assert!(l214.whitelist.lists("214_Intro_IGC", "C:/x/Any.ai"));
        assert!(!other.whitelist.lists("214_Intro_IGC", "C:/x/Any.ai"));
        // A spawner of a fight past the TV's (the third island's).
        assert!(
            !l214
                .whitelist
                .lists("Enemy_Chompy_Spawner(4)", "C:/x/Any.ai")
        );
        assert!(encounter("level_212").is_none());
    }

    /// openspec AS.82: a level's cutscene objects' definitions listed every
    /// tick, with their templates' scripts: a running placement whose
    /// script binds `Cutscene - Define Object`'s `object def` to a file
    /// structure (Flynn's `Object_Flynn_Def.ai`), and the script of the
    /// template it names as the actor (`NPC_Size_130.ai`). A cutscene
    /// defining its own object into a variable of its own, a definition no
    /// placement runs, one another file's placement runs, and their
    /// templates' scripts stay off.
    #[test]
    fn a_levels_cutscene_objects_are_listed_with_their_templates() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{El, Writer};
        use guac_script::{Listed, Role, Unit};

        const DEF: &str = "C:/l/NPC_IGC/Flynn/Object_Flynn_Def.ai";
        const SIZE: &str = "C:/l/NPC_IGC/NPC_Size_130.ai";
        const CUTSCENE: &str = "C:/l/cutscenes/214_Intro_IGC.ai";
        const MABU: &str = "C:/l/cutscenes/214_Intro_Mabu_IGC.ai";
        const IDLE: &str = "C:/l/NPC_IGC/Cali/Object_Cali_Def.ai";
        const CALI: &str = "C:/l/NPC_IGC/NPC_Size.ai";
        let mut b = IgzBuilder::new();
        let (flynn, mabu, cali);
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            flynn = w.b.object(0, "tfbActorInfo", 0xF8);
            mabu = w.b.object(0, "tfbActorInfo", 0xF8);
            cali = w.b.object(0, "tfbActorInfo", 0xF8);
            let ty = w.op(Class::DefineStructure, "Cutscene Object Def");
            let actor_field = w.variable("actor", "ScriptReference", None);
            let object = w.b.object(0, "ScriptStructure", 0x20);
            w.text(object, 0x08, "Object_Flynn_IGC");
            w.refer(object, 0x1C, ty);
            let other = w.b.object(0, "ScriptStructure", 0x20);
            w.text(other, 0x08, "Object_Cali_IGC");
            w.refer(other, 0x1C, ty);
            let define = w.op(Class::DefineMacro, DEFINE_OBJECT);
            let local = w.variable("mabu object", "ScriptReference", None);
            let script = |w: &mut Writer<'_>, path: &str, actor, into: &[El<'_>], body: bool| {
                w.script(
                    path,
                    |w| {
                        let a = w.reference(&[El::Local(actor)], None);
                        let o = w.reference(into, None);
                        w.call(define, &[("actor", a), ("object def", o)]);
                    },
                    |w| {
                        if body {
                            let none = w.int(0);
                            let p = w.open_macro(
                                define,
                                &[
                                    ("actor", "ScriptReference", none),
                                    ("object def", "ScriptReference", none),
                                ],
                            );
                            w.end_block(p);
                            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
                            let p = w.push(ty);
                            w.push(actor_field);
                            w.end_block(p);
                            w.end_block(types);
                        }
                    },
                    |_| {},
                );
            };
            script(w, DEF, flynn, &[El::Local(object)], true);
            script(w, CUTSCENE, mabu, &[El::Local(local)], false);
            script(w, IDLE, cali, &[El::Local(other)], false);
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
        let program = Program::link(&[unit]).unwrap();
        let obj = |l| Obj {
            unit: 0,
            index: igz.object_at(l).unwrap().0 as u32,
        };
        let at = |i: u32, name: &str, script: &str, state: u32| {
            Placed::named(
                Obj {
                    unit: 0,
                    index: 900 + i,
                },
                name,
                Some(script),
                state,
            )
        };
        let files = Files {
            placed: vec![
                at(1, "Flynn_NPC_Def", DEF, RUNNING),
                Placed::named(obj(flynn), "Flynn_NPC", Some(SIZE), 5),
                at(2, "214_Intro_IGC", CUTSCENE, RUNNING),
                Placed::named(obj(mabu), "214_Intro_Mabu_Actor_RED", Some(MABU), 5),
                // Cali's definition is a template here: no placement runs it.
                at(3, "Cali_NPC_Def", IDLE, 5),
                Placed::named(obj(cali), "Cali_NPC", Some(CALI), 5),
            ],
            ..Files::default()
        };
        assert_eq!(
            cutscene_objects(&program, &files, 0),
            ["Object_Flynn_Def.ai", "NPC_Size_130.ai"]
        );
        // Another file's level: none of this one's.
        assert!(cutscene_objects(&program, &files, 3).is_empty());
        let base = smash_config(Mode::Play);
        let with = with_cutscene_objects(base.clone(), &program, &files, 0);
        for (script, listed) in [
            (DEF, Listed::Every),
            (SIZE, Listed::Every),
            (CUTSCENE, Listed::Off),
            (MABU, Listed::Off),
            (IDLE, Listed::Off),
            (CALI, Listed::Off),
        ] {
            assert_eq!(with.whitelist.listing("", script), listed, "{script}");
            assert_eq!(base.whitelist.listing("", script), Listed::Off, "{script}");
        }
        // Listed once however often it's asked.
        let twice = with_cutscene_objects(with.clone(), &program, &files, 0);
        assert_eq!(twice.whitelist, with.whitelist);
    }

    /// openspec AS.58: level 214's second fight by name: its controller and
    /// helper, the Eon hint its controller waits on, the four undead
    /// Chompies and the two Elf Nears, where island 2's Chompies running the
    /// same script stay off.
    #[test]
    fn level_214s_second_fight_is_listed_by_name() {
        let files = Files::default();
        let l214 = level_config(Mode::Play, &files, 4, "level_214_tutotial_2");
        for name in [
            "Setup_002_Controller",
            "Setup_002_Controller_Helper",
            "Explain_Elemental",
            "Enemy_Chompy2_E",
            "Enemy_Chompy2_F",
            "Enemy_Chompy2_G",
            "Enemy_Chompy2_H",
            "Enemy_Elf_Near_S2(3)",
            "Enemy_Elf_Near_S2(4)",
        ] {
            assert!(l214.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
        }
        assert!(
            !l214
                .whitelist
                .lists("Enemy_Chompy2_E(5)", "C:/x/S2_Enemy_Chompy.ai")
        );
    }

    /// openspec AS.75: the second island's first fight by name; AS.68: its
    /// homing charge's template listed, so its clones run, and steered,
    /// never walked.
    #[test]
    fn level_214s_third_fight_is_listed_by_name_with_its_homing_charge_steered() {
        let files = Files::default();
        let l214 = level_config(Mode::Play, &files, 4, "level_214_tutotial_2");
        for name in [
            "Setup_003_Controller",
            "Enemy_Chompy2_E(1)",
            "Enemy_Chompy2_E(2)",
            "Enemy_Chompy2_E(3)",
            "Enemy_Chompy2_E(4)",
            "Enemy_TreeRex(2)",
        ] {
            assert!(l214.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
        }
        assert!(
            l214.whitelist.lists(
                "TreeRex_special_homing_actor",
                "C:/x/Enemy_TreeRex_special_homingcharge_actor.ai"
            ),
            "the homing charge listed"
        );
        let e = encounter("level_214_tutotial_2").unwrap();
        assert!(e.steered.contains(&"TreeRex_special_homing_actor"));
        assert!(
            !e.placements.contains(&"TreeRex_special_homing_actor")
                && !e.clones.contains(&"TreeRex_special_homing_actor"),
            "never walked"
        );
        // A Tree Rex of a fight past the TV's (Setup_005's yard).
        assert!(
            !l214
                .whitelist
                .lists("Enemy_TreeRex(3)", "C:/x/S2_Enemy_TreeRex.ai")
        );
    }

    /// openspec AS.84: level 214's fourth fight by name, its Mabu among the
    /// walked (his capsule meets the level), on 214's listing alone; the
    /// locked doors and their keys by script, every tick.
    #[test]
    fn level_214s_fourth_fight_and_its_locked_doors_are_listed() {
        let files = Files::default();
        let l214 = level_config(Mode::Play, &files, 4, "level_214_tutotial_2");
        let other = level_config(Mode::Play, &files, 4, "level_212");
        let e = encounter("level_214_tutotial_2").unwrap();
        for name in [
            "Setup_004_Controller",
            "Setup_004_NPC_Mabu",
            "Enemy_Elf_Near_A",
            "Enemy_Elf_Near_B",
            "Enemy_Elf_Near_C",
            "Enemy_TreeRex(1)",
            "Enemy_Chompy_Spawner(2)",
        ] {
            assert!(l214.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(!other.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(e.placements.contains(&name), "{name} walked");
        }
        for script in DOOR_SCRIPTS {
            let path =
                format!("C:/tfb/Content/Levels/Includes/GameElement_Door_and_Key/Scripts/{script}");
            assert_eq!(
                l214.whitelist.listing("a placement", &path),
                guac_script::Listed::Every,
                "{script}"
            );
        }
    }

    /// openspec AS.96: the interiors' scripts on every level's listing,
    /// chapter 1's and any other's, by script, every tick, and the portal's
    /// warp answered by the TV in every run its hero joins (stand-in ledger:
    /// `warp`); the init chain's run, which has no hero, answers none.
    #[test]
    fn the_interiors_run_on_every_levels_listing_and_the_warp_is_answered() {
        let files = Files::default();
        for level in ["level_213_tutotial", "level_214_tutotial_2", "level_201"] {
            let c = level_config(Mode::Play, &files, 4, level);
            for script in INTERIOR_SCRIPTS {
                let path = format!("C:/tfb/Content/Levels/Includes/Interiors/Scripts/{script}");
                assert_eq!(
                    c.whitelist.listing("a placement", &path),
                    guac_script::Listed::Every,
                    "{level} {script}"
                );
            }
            assert!(
                c.answered.iter().any(|m| m == crate::interiors::WARP),
                "{level}"
            );
        }
        assert!(
            hero_config(Mode::Play)
                .answered
                .iter()
                .any(|m| m == crate::interiors::WARP)
        );
        assert!(config(Mode::Play).answered.is_empty());
    }

    /// openspec AS.59: level 214's way from its first island to its second
    /// by name, on 214's listing alone and in no encounter (never walked).
    #[test]
    fn level_214s_way_to_its_second_island_is_listed_by_name() {
        let files = Files::default();
        let l214 = level_config(Mode::Play, &files, 4, "level_214_tutotial_2");
        let other = level_config(Mode::Play, &files, 4, "level_212");
        let e = encounter("level_214_tutotial_2").unwrap();
        for name in passages("level_214_tutotial_2") {
            assert!(l214.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(!other.whitelist.lists(name, "C:/x/Any.ai"), "{name}");
            assert!(!e.placements.contains(name), "{name} isn't walked");
        }
        assert!(passages("level_214_tutotial_2").contains(&"214_Recover_First_Part_IGC"));
        assert!(passages("level_212").is_empty());
    }

    /// openspec AU.28: the hub's people are the placements running Flynn's
    /// or Cali's own script, by their actors; their cutscene templates' and
    /// another placement's aren't (a clone has no placement of its own).
    #[test]
    fn the_hubs_people_are_flynn_and_cali_by_their_scripts() {
        let dir = "C:/tfb/Content/Levels/Level_Hub_2";
        let obj = |index| Obj { unit: 3, index };
        let files = Files {
            placed: vec![
                Placed::named(
                    obj(10),
                    "Flynn",
                    Some(&format!("{dir}/Scripts/Flynn.ai")),
                    4,
                ),
                Placed::named(obj(11), "Cali", Some(&format!("{dir}/Scripts/Cali.ai")), 4),
                Placed::named(
                    obj(12),
                    "CS_Flynn",
                    Some(&format!("{dir}/Cutscenes/Scripts/CS_Flynn.ai")),
                    5,
                ),
                Placed::named(obj(13), "Mabu", Some(&format!("{dir}/Scripts/Mabu.ai")), 4),
            ],
            ..Files::default()
        };
        let handles: HashMap<Obj, Handle> =
            (10..14).map(|i| (obj(i), 0x100 + u64::from(i))).collect();
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        assert_eq!(world.people(), [0x10A, 0x10B]);
        assert!(is_hub_person(&format!("{dir}/Scripts/Flynn.ai")));
        assert!(!is_hub_person(&format!(
            "{dir}/Cutscenes/Scripts/CS_Flynn.ai"
        )));
    }

    /// openspec AS.46, AS.47: a hub stage's sky, its stage's controller and
    /// its background by script, every tick, on each hub stage's listing
    /// and no other level's; each stage's intro by its placement, as level
    /// 214's.
    #[test]
    fn a_hub_stages_own_scripts_and_its_intro_are_listed() {
        let files = Files::default();
        let listing = |c: &Config, script: &str| {
            c.whitelist.listing(
                "a placement",
                &format!("C:/tfb/Content/Levels/Level_Hub_2/Scripts/{script}"),
            )
        };
        for (stage, placement) in [
            ("level_hub_2_stage_1", "CS_Intro_Controller"),
            ("level_hub_2_stage_2", "CS_Hub_Intro_Stage2"),
            ("level_hub_2_stage_3", "CS_Hub_Intro_Stage3"),
            ("level_hub_2_stage_4", "CS_Hub_Intro_Stage4"),
        ] {
            assert!(is_hub(stage));
            let c = level_config(Mode::Play, &files, 4, stage);
            for script in [
                "SkyDome_Controller.ai",
                "Hub2_Stage_1_Controller.ai",
                "Hub2_Stage_4_Controller.ai",
                "BackgroundController.ai",
                "Hub2_Background_Island.ai",
                "Hub2_Background_Actor.ai",
                "AtmZone_Controller.ai",
                "Flynn.ai",
                "Cali.ai",
            ] {
                assert_eq!(
                    listing(&c, script),
                    guac_script::Listed::Every,
                    "{stage} {script}"
                );
            }
            // Flynn's and Cali's own scripts, not their cutscene templates'.
            for template in ["CS_Flynn.ai", "CS_Cali.ai"] {
                assert_eq!(
                    listing(&c, template),
                    guac_script::Listed::Off,
                    "{stage} {template}"
                );
            }
            // The members Cali's dialogue reaches, as value cells (every
            // level's, AS.53's table).
            let filter = guac_script::MemberId::named("viewportinfo", "filter effect");
            assert_eq!(
                c.members.get(filter).map(|(_, place)| place),
                Some(guac_script::Place::ValueCell),
                "{stage}"
            );
            assert_eq!(intro(stage), Some(placement));
            assert!(c.whitelist.lists(placement, "C:/x/Any.ai"), "{stage}");
        }
        for other in ["level_213_tutotial", "level_214_tutotial_2", "level_201"] {
            assert!(!is_hub(other));
            let c = level_config(Mode::Play, &files, 4, other);
            for script in HUB_SCRIPTS {
                assert_eq!(
                    listing(&c, script),
                    guac_script::Listed::Off,
                    "{other} {script}"
                );
            }
            assert!(!c.whitelist.lists("CS_Intro_Controller", "C:/x/Any.ai"));
            // The filter cells every level's, the level end's sequencing
            // reaching them (AS.53).
            let filter = guac_script::MemberId::named("viewportinfo", "filter effect");
            assert_eq!(
                c.members.get(filter).map(|(_, place)| place),
                Some(guac_script::Place::ValueCell),
                "{other}"
            );
        }
    }

    /// openspec AS.54: a level's first encounter walks its placements that
    /// have movement numbers (its spawners), in the level's own file alone,
    /// and each clone made later of one of its templates walks as it's
    /// made; a clone of another template doesn't, nor does a placement it
    /// doesn't name. AS.68: nor does a template it steers, or its clone.
    #[test]
    fn a_levels_encounter_walks_its_placements_and_its_templates_clones() {
        const SCRIPT: &str = "C:/tfb/Content/Levels/_Enemies/Chompy/scripts/S2_Enemy_Chompy.ai";
        let o = |unit, index| Obj { unit, index };
        let (spawner, legs, template, other, elsewhere, charge) =
            (o(4, 10), o(4, 11), o(4, 12), o(4, 13), o(2, 14), o(4, 15));
        let moving = |obj, name: &str, state| Placed {
            physics: Some(Physics::default()),
            ..Placed::named(obj, name, Some(SCRIPT), state)
        };
        let files = Files {
            placed: vec![
                moving(spawner, "Enemy_Chompy_Spawner(1)", RUNNING),
                Placed::named(legs, "Enemy_Chompy_Spawner(5)", Some(SCRIPT), RUNNING),
                moving(template, "_Template_Undead_Chompy", 5),
                moving(other, "Enemy_Chompy", 5),
                moving(elsewhere, "Enemy_Chompy_Spawner(1)", RUNNING),
                moving(charge, "TreeRex_special_homing_actor", 5),
            ],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let e = encounter("level_214_tutotial_2").unwrap();
        assert_eq!(
            world.walk_encounter(4, e),
            (1, 1),
            "the spawner, the template"
        );
        assert_eq!(world.creatures(), [handles[&spawner]]);
        let mut spawn = headless_spawner(1 << 40);
        let mut clone_of = |world: &mut ScriptWorld, t: Obj| {
            let Answer::Run {
                result: Some(Value::Entity(h)),
                ..
            } = world.spawn(
                Value::Object(t),
                None,
                None,
                handles[&spawner],
                &mut spawn,
                &mut NoEngine,
            )
            else {
                panic!("spawned");
            };
            h
        };
        let chompy = clone_of(&mut world, template);
        let plain = clone_of(&mut world, other);
        let steered = clone_of(&mut world, charge);
        assert!(world.actor(chompy).is_some_and(|a| a.walker.is_some()));
        assert!(world.actor(plain).is_some_and(|a| a.walker.is_none()));
        assert!(world.actor(steered).is_some_and(|a| a.walker.is_none()));
        assert_eq!(world.creatures(), [handles[&spawner], chompy]);
    }

    /// AS.51's review: a level definition by its file name, every way the
    /// disc spells one, and none of the scripts named like one that aren't
    /// (a surface's definition, a camera's, a default).
    #[test]
    fn a_level_definition_is_known_by_every_spelling_the_disc_has() {
        for f in [
            "Level_Definition_214.ai",
            "level_definition_208.ai",
            "Level_Definition.ai",
            "LevelDefinition_003.ai",
            "008_LevelDefinition.ai",
            "Level_200_Level_Definition.ai",
            "203_level_definition.ai",
            "215_Level_Definition_DAVID.ai",
            "_LevelDef_205.ai",
            "212_LevelDef.ai",
            "Hub2_LevelDef.ai",
            "_LevelDefinition_PVP_Level_001.ai",
            "_LevelDefintion_PvP_004.ai",
            "_levelDefinition_PvP-009.ai",
            "_levelDef_PvP_202.ai",
            "209_LevelDef.ai",
            "032_Leveldef.ai",
            "Challenge_Level_001_LevelDef.ai",
            "Challenge_041_Level_Definition_01.ai",
            "_Challenge_Level_Definition.ai",
            "036_Level_definition.ai",
        ] {
            assert!(is_level_definition(f), "{f}");
        }
        for f in [
            "DefineDeathSurface.ai",
            "Challenge_019_Camera_define.ai",
            "Level_Default_Camera.ai",
            "Pop_to_Camera_X.ai",
            "Definition.ai",
        ] {
            assert!(!is_level_definition(f), "{f}");
        }
    }

    /// AS.15's dev entry: a placement woken has no range and no buffer, so
    /// the activity rule keeps it awake; no actor, no wake.
    #[test]
    fn a_placement_woken_has_no_range_or_buffer() {
        let (mut w, grunt, _) = world();
        assert!(w.wake(grunt));
        let a = w.actor(grunt).unwrap();
        assert_eq!((a.range, a.buffer), (0.0, 0.0));
        assert!(!w.wake(0xDEAD));
    }

    /// AH.220: a script that unlocks an achievement (`set system.game
    /// achievement = 23`, as `System - Achievements - Complete Achievement
    /// by ID` does) runs on past the write, in strict mode and in play mode
    /// alike, under the TV's members: the host keeps each id written in
    /// order and reads the last back.
    #[test]
    fn an_achievements_unlock_is_kept_and_its_placement_runs_on() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{El, Writer};
        use guac_script::{Role, Unit};

        const PATH: &str = "C:/tfb/Synthetic/Unlocker.ai";
        let mut b = IgzBuilder::new();
        let unlocker;
        {
            let mut w = Writer::new(&mut b);
            unlocker = w.b.object(0, "tfbActorInfo", 0xF8);
            let ticks = w.variable("ticks", "ValueInfo", Some("IntMeasurement"));
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (macros, _) = w.open(Class::MacroSection, "MACROS");
            w.end_block(macros);
            // The tick: the unlock, then a count past it.
            let id = w.int(23);
            w.set(&[El::Member("system", "game achievement")], id);
            w.inc(&[El::Local(ticks)]);
            w.end_block(pre);
            w.end(PATH);
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
        let obj = Obj {
            unit: 0,
            index: igz.object_at(unlocker).unwrap().0 as u32,
        };
        let files = Files {
            lights: HashMap::new(),
            placed: vec![Placed::named(obj, "Unlocker", Some(PATH), RUNNING)],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
            emitters: HashMap::new(),
        };
        let handles = headless_handles(&files);
        for mode in [Mode::Strict, Mode::Play] {
            let mut c = smash_config(mode);
            c.whitelist = Whitelist::All;
            let mut vm = init_chain(program.clone(), &files, &handles, &[0], c);
            let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            let id = vm.placement_of(handles[&obj]).unwrap();
            for _ in 0..3 {
                let mut spawn = headless_spawner(1 << 40);
                let mut host = WorldHost {
                    world: &mut world,
                    engine: &mut NoEngine,
                    generator: &mut Generator::seeded(1),
                    spawn: &mut spawn,
                };
                let report = vm
                    .step(&mut host)
                    .unwrap_or_else(|h| panic!("{mode:?}: {h}"));
                assert!(report.stopped.is_empty(), "{mode:?}: {:?}", report.stopped);
            }
            assert_eq!(vm.stopped(id), None, "{mode:?}");
            assert_eq!(vm.local(id, "ticks"), Some(Value::Int(3)), "{mode:?}");
            assert_eq!(world.achievements, [23, 23, 23], "{mode:?}");
            assert_eq!(
                world.get(&at("system", "game achievement", None), &mut NoEngine),
                Ok(Value::Int(23)),
                "{mode:?}"
            );
        }
        // None unlocked reads 0; the next written is the one read.
        let (mut w, ..) = world();
        let achievement = at("system", "game achievement", None);
        assert_eq!(w.get(&achievement, &mut NoEngine), Ok(Value::Int(0)));
        w.set(&achievement, Value::Int(4), &mut NoEngine).unwrap();
        w.set(&achievement, Value::Int(9), &mut NoEngine).unwrap();
        assert_eq!(w.achievements, [4, 9]);
        assert_eq!(w.get(&achievement, &mut NoEngine), Ok(Value::Int(9)));
    }

    /// AH.39: the TV's run lists the level-up's flourish and the death
    /// actor by file name, and the flourish's rumble as it strikes is a cell
    /// of the TV's members: a synthetic `LevelUp.ai` placement writing it
    /// each tick, and a `MinionDeathActor.ai` one counting, run on under the
    /// TV's own listing, strict and play alike; one under another file name
    /// stays inert.
    #[test]
    fn the_level_up_and_the_death_actor_run_on_the_tvs_listing() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{El, Writer};
        use guac_script::{Role, Unit};

        const LEVEL_UP: &str = "C:/tfb/Synthetic/LevelUp.ai";
        const DEATH: &str = "C:/tfb/Synthetic/MinionDeathActor.ai";
        const OTHER: &str = "C:/tfb/Synthetic/Unlisted.ai";
        let mut b = IgzBuilder::new();
        let mut actors = Vec::new();
        {
            let mut w = Writer::new(&mut b);
            for (path, rumbles) in [(LEVEL_UP, true), (DEATH, false), (OTHER, true)] {
                let actor = w.b.object(0, "tfbActorInfo", 0xF8);
                actors.push(actor);
                let ticks = w.variable("ticks", "ValueInfo", Some("IntMeasurement"));
                let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
                let (start, _) = w.open(Class::StartUp, "STARTUP");
                w.end_block(start);
                let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
                w.end_block(shut);
                let (macros, _) = w.open(Class::MacroSection, "MACROS");
                w.end_block(macros);
                if rumbles {
                    let seconds = w.float(0.5);
                    w.set(&[El::Member("soundinfo", "rumble duration")], seconds);
                }
                w.inc(&[El::Local(ticks)]);
                w.end_block(pre);
                w.end(path);
            }
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
        let objs: Vec<Obj> = actors
            .iter()
            .map(|a| Obj {
                unit: 0,
                index: igz.object_at(*a).unwrap().0 as u32,
            })
            .collect();
        let files = Files {
            lights: HashMap::new(),
            placed: vec![
                Placed::named(objs[0], "LevelUp", Some(LEVEL_UP), RUNNING),
                Placed::named(objs[1], "MinionDeath_Actor", Some(DEATH), RUNNING),
                Placed::named(objs[2], "Unlisted", Some(OTHER), RUNNING),
            ],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
            emitters: HashMap::new(),
        };
        let handles = headless_handles(&files);
        for mode in [Mode::Strict, Mode::Play] {
            let mut vm = init_chain(program.clone(), &files, &handles, &[0], smash_config(mode));
            let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            let ids: Vec<PlacementId> = objs
                .iter()
                .map(|o| vm.placement_of(handles[o]).unwrap())
                .collect();
            for _ in 0..3 {
                let mut spawn = headless_spawner(1 << 40);
                let mut host = WorldHost {
                    world: &mut world,
                    engine: &mut NoEngine,
                    generator: &mut Generator::seeded(1),
                    spawn: &mut spawn,
                };
                let report = vm
                    .step(&mut host)
                    .unwrap_or_else(|h| panic!("{mode:?}: {h}"));
                assert!(report.stopped.is_empty(), "{mode:?}: {:?}", report.stopped);
            }
            for (id, ticks) in ids.iter().zip([Some(3), Some(3), None]) {
                assert_eq!(vm.stopped(*id), None, "{mode:?}");
                assert_eq!(
                    vm.local(*id, "ticks").and_then(|v| v.as_i32()),
                    ticks,
                    "{mode:?}"
                );
            }
            assert!(vm.inert().contains(&handles[&objs[2]]), "{mode:?}");
        }
    }

    /// AS.60: a reset of a file placement a script took off brings it back
    /// as its file has it (level 214's intro resets the ship it took off),
    /// listed for the embedder to run its script again; a reset of one
    /// standing leaves it where it is, nothing listed.
    #[test]
    fn a_reset_brings_a_placement_taken_off_back() {
        let (mut w, grunt, _) = world();
        let file = w.actor(grunt).unwrap().position;
        w.actors.get_mut(&grunt).unwrap().position = file + Vec3::X;
        assert_eq!(
            w.op(
                &call(Class::Reset, Some(Value::Entity(grunt))),
                &mut |_| 0,
                &mut NoEngine
            ),
            Ok(Answer::Done)
        );
        assert!(w.revived.is_empty(), "a standing one isn't revived");
        assert_eq!(w.actor(grunt).unwrap().position, file + Vec3::X);
        w.op(
            &call(Class::Remove, Some(Value::Entity(grunt))),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap();
        assert!(w.removed(grunt));
        w.op(
            &call(Class::Reset, Some(Value::Entity(grunt))),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap();
        assert!(!w.removed(grunt), "back on");
        assert_eq!(
            w.actor(grunt).unwrap().position,
            file,
            "where its file has it"
        );
        assert_eq!(w.revived, [(grunt, GRUNT)]);
    }

    /// AS.67: a clone's reset of itself stands it again as it was made: at
    /// the place it was cloned (`abstractplacement.origin`), its motion
    /// gone and its template's numbers back, keeping what its spawner gave
    /// it (its id); the VM sends its script round. Another placement's reset
    /// of it leaves it where it stands, and one taken off isn't made again.
    #[test]
    fn a_clone_resetting_itself_stands_again_where_it_was_cloned() {
        let (mut w, grunt, _) = world();
        let at_grunt = OpCall {
            operands: vec![
                ("_LHS", Some(Value::Object(GRUNT))),
                ("_RHS", Some(Value::Entity(grunt))),
            ],
            ..call(Class::Spawn, None)
        };
        w.op(&at_grunt, &mut |_| 900, &mut NoEngine).unwrap();
        let made = w.actor(900).unwrap().clone();
        assert_eq!(made.origin, made.position);
        {
            let a = w.actors.get_mut(&900).unwrap();
            a.id = 7;
            a.position += Vec3::new(3.0, -1.0, 2.0);
            a.velocity = Vec3::new(0.0, 0.0, 9.0);
            a.launched = true;
            a.scale = [50.0; 3];
            a.physics.gravity_acceleration = 99.0;
        }
        let moved = w.actor(900).unwrap().position;
        let reset = |by: Handle| OpCall {
            placement: by,
            ..call(Class::Reset, Some(Value::Entity(900)))
        };
        w.op(&reset(1), &mut |_| 0, &mut NoEngine).unwrap();
        assert_eq!(w.actor(900).unwrap().position, moved, "another's reset");
        w.op(&reset(900), &mut |_| 0, &mut NoEngine).unwrap();
        let a = w.actor(900).unwrap();
        assert_eq!(a.position, made.origin, "where it was cloned");
        assert_eq!(a.origin, made.origin);
        assert_eq!(a.velocity, Vec3::ZERO);
        assert!(!a.launched);
        assert_eq!(a.scale, made.scale);
        assert_eq!(a.physics, made.physics);
        assert_eq!(a.id, 7, "its spawner's id kept");
        assert_eq!(a.template, Some(GRUNT));
        assert!(!a.removed);
        assert!(w.revived.is_empty(), "the VM restarts it, not the embedder");
        w.op(
            &call(Class::Remove, Some(Value::Entity(900))),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap();
        w.op(&reset(900), &mut |_| 0, &mut NoEngine).unwrap();
        assert!(w.removed(900), "one taken off isn't made again");
    }

    /// The AS.67 review: a file placement's reset of itself stands it again
    /// as its file has it (its place, its buffer), its motion gone, and one
    /// the engine walks still walks, its body to be made again. Nothing is
    /// listed for the embedder to run again: the VM sends its script round.
    #[test]
    fn a_file_placement_resetting_itself_stands_as_its_file_has_it_and_still_walks() {
        let (mut w, grunt, _) = world();
        let made = w.actor(grunt).unwrap().clone();
        assert!(w.walk(grunt));
        {
            let a = w.actors.get_mut(&grunt).unwrap();
            a.position += Vec3::new(4.0, 0.5, -2.0);
            a.velocity = Vec3::new(1.0, 0.0, 0.0);
            a.buffer = 0.0;
        }
        let by_itself = OpCall {
            placement: grunt,
            ..call(Class::Reset, Some(Value::Entity(grunt)))
        };
        w.op(&by_itself, &mut |_| 0, &mut NoEngine).unwrap();
        let a = w.actor(grunt).unwrap();
        assert_eq!(a.position, made.position, "its file's place");
        assert_eq!(a.velocity, Vec3::ZERO);
        assert_eq!(a.buffer, 12.5, "its file's buffer");
        assert!(
            a.walker.as_ref().is_some_and(|k| k.body.is_none()),
            "still walked, its body made again"
        );
        assert!(w.revived.is_empty());
    }

    /// The as-214-walk review: a scripted template's clone joins the run
    /// with its script, but not one taken off before it joins (level 214's
    /// skip takes its cutscene's second ship off the tick that clones it).
    #[test]
    fn a_clone_taken_off_before_it_joins_never_starts_its_script() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::Writer;
        use guac_script::{Role, Unit};

        const PATH: &str = "C:/tfb/Synthetic/Ship.ai";
        let mut b = IgzBuilder::new();
        let ship;
        {
            let mut w = Writer::new(&mut b);
            ship = w.b.object(0, "tfbActorInfo", 0xF8);
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            w.end_block(start);
            w.end_block(pre);
            w.end(PATH);
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
        let program = Program::link(&[unit]).unwrap();
        let obj = Obj {
            unit: 0,
            index: igz.object_at(ship).unwrap().0 as u32,
        };
        let files = Files {
            lights: HashMap::new(),
            placed: vec![Placed::named(obj, "Ship", Some(PATH), 5)],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
            emitters: HashMap::new(),
        };
        let handles = headless_handles(&files);
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let (on, off) = (900, 901);
        for (h, removed) in [(on, false), (off, true)] {
            w.actors.insert(
                h,
                Actor {
                    removed,
                    template: Some(obj),
                    ..Actor::blank()
                },
            );
            w.scripted.push((h, obj));
        }
        let joined: Vec<Handle> = w.adopt(&program).into_iter().map(|(h, ..)| h).collect();
        assert_eq!(joined, [on]);
        assert!(
            w.scripted.is_empty(),
            "the taken-off one isn't kept for later"
        );
    }

    /// AS.60's embedder half (the as-214-walk review): a running placement
    /// a reset brings back runs its script again only once the run has
    /// taken its old one off; one the world lists taken off while its
    /// script still runs (a native take-off) gets no second script under
    /// its handle.
    #[test]
    fn a_revived_placement_runs_its_script_again_only_once_its_old_one_is_off() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::Writer;
        use guac_script::{Role, Unit};

        const PATH: &str = "C:/tfb/Synthetic/Ship.ai";
        let mut b = IgzBuilder::new();
        let ship;
        {
            let mut w = Writer::new(&mut b);
            ship = w.b.object(0, "tfbActorInfo", 0xF8);
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            w.end_block(start);
            w.end_block(pre);
            w.end(PATH);
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
        let obj = Obj {
            unit: 0,
            index: igz.object_at(ship).unwrap().0 as u32,
        };
        let files = Files {
            lights: HashMap::new(),
            placed: vec![Placed::named(obj, "Ship", Some(PATH), RUNNING)],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
            emitters: HashMap::new(),
        };
        let handles = headless_handles(&files);
        let mut c = smash_config(Mode::Play);
        c.whitelist = Whitelist::All;
        let mut vm = init_chain(program, &files, &handles, &[0], c);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let h = handles[&obj];
        let first = vm.placement_of(h).expect("it runs");
        let reset = |world: &mut ScriptWorld| {
            world
                .op(
                    &call(Class::Reset, Some(Value::Entity(h))),
                    &mut |_| 0,
                    &mut NoEngine,
                )
                .unwrap();
        };
        // The world lists it off, its script still running: back, no second.
        world.actors.get_mut(&h).unwrap().removed = true;
        reset(&mut world);
        assert!(!world.removed(h));
        assert!(add_scripted(&mut vm, &mut world).is_empty());
        assert_eq!(vm.placement_of(h), Some(first));
        // Taken off by the run too: back, its script again from its STARTUP.
        world
            .op(
                &call(Class::Remove, Some(Value::Entity(h))),
                &mut |_| 0,
                &mut NoEngine,
            )
            .unwrap();
        assert!(vm.remove(h));
        reset(&mut world);
        let added = add_scripted(&mut vm, &mut world);
        assert_eq!(added.len(), 1);
        assert_eq!(vm.placement_of(h), Some(added[0]));
        assert!(!vm.removed(added[0]));
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
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: vec![Placed {
                id: 41,
                buffer: 12.5,
                ..Placed::named(GRUNT, "Grunt", None, RUNNING)
            }],
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
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
        // The pit's intro cutscene runs since AH.28, Eon's lines since
        // AR.29; the hints and the cutscene manager's placement stay off
        // (decision 6).
        assert!(
            c.whitelist
                .lists("Eon_4_help_Norticus", "C:/x/Pan_Show_Norticu.ai")
        );
        assert!(
            c.whitelist
                .lists("Eon_2_Keep_Going", "C:/x/Eon_Line_Activator.ai")
        );
        for (name, script) in [
            ("Hint_Combat", "C:/x/Hints_Passive.ai"),
            ("CutsceneManager", "C:/x/Spyro_CutsceneManager.ai"),
        ] {
            assert!(!c.whitelist.lists(name, script), "{name}");
        }
        for r in members::CREATURES {
            assert!(
                c.members
                    .get(MemberId::named(r.namespace, r.name))
                    .is_some()
            );
        }
    }

    /// The TV's listing (openspec AH.27): the pickups and the breakables
    /// by name, with the creatures' and the loop's members; AG.60's start
    /// scripts on the props every tick; the first fight's Chompies,
    /// Setup_001, the chest and the castle wall's script off it, inert, and
    /// the Titan boulders' its first tick alone (their cylinders and pebbles,
    /// the feat held), so nothing takes them off; the fight's own listing
    /// AH.27's with the four; AH.8's route's scripts and members (AH.28).
    /// AS.57: level 214's bombs, the castle walls they open, the gesture
    /// manager that reads the throw and the pan to the first bomb run every
    /// tick by script, the bomb hints by name; the hints' script on any
    /// other placement (chapter 1's fifteen) stays off. AS.68: the
    /// countdown's numbers run. AS.60: its ship by name.
    #[test]
    fn level_214s_bombs_and_their_walls_run_on_the_tvs_listing() {
        let c = smash_config(Mode::Play);
        for script in BOMB_SCRIPTS {
            let path = format!("C:/tfb/Content/Levels/Includes/{script}");
            assert_eq!(
                c.whitelist.listing("a placement", &path),
                guac_script::Listed::Every,
                "{script}"
            );
        }
        let hints = "C:/tfb/Content/Levels/Level_214/Scripts/Hints_Passive.ai";
        for name in BOMB_HINTS {
            assert!(c.whitelist.lists(name, hints), "{name}");
        }
        assert!(!c.whitelist.lists("Hint_Use_attacks", hints));
        assert!(c.whitelist.lists(
            "Bomb_Timer",
            "C:/tfb/Content/Levels/Includes/GameElement_Bomb/Scripts/BombTImer.ai"
        ));
        // AS.60: its ship by name, the ships' script on any other off.
        let ships = "C:/tfb/Content/Levels/Level_214/Scripts/Ships.ai";
        for name in SHIP {
            assert!(c.whitelist.lists(name, ships), "{name}");
        }
        assert!(!c.whitelist.lists("Hub_Ship", ships));
    }

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
        assert!(!c.whitelist.lists(
            "Breakable_Stone_Wall_Castle_206(1)",
            "C:/tfb/Content/Levels/Includes/GameElement_Breakable_Wall/Scripts/Breakable_Wall_213.ai",
        ));
        // The chest is a creature: its script and its locks' stay off.
        for (name, script) in [
            ("Chest(1)", "Spyro_GenericStuff/scripts/Chest.ai"),
            ("Chest_Lock", "Treasure_Chest/Chest_Lock.ai"),
            ("Lock_Flash", "Treasure_Chest/Lock_Flash.ai"),
        ] {
            let path = format!("C:/tfb/Content/Levels/Includes/{script}");
            assert!(!c.whitelist.lists(name, &path), "{name}");
        }
        assert_eq!(
            c.whitelist.listing(
                "Titan_CarriableObject_Boulder_v3_(4)",
                "C:/tfb/Content/Levels/Includes/Titan_Includes/Titan_CarribleObject/Scripts/Titan_Carriable_Object.ai",
            ),
            guac_script::Listed::FirstTick
        );
        for script in START_PROPS {
            let path = format!("C:/tfb/Content/Levels/Includes/{script}");
            assert_eq!(
                c.whitelist.listing("a prop", &path),
                guac_script::Listed::Every,
                "{script}"
            );
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
        // AH.8's route (openspec AH.28): its scripts every tick, its
        // members registered.
        for script in ROUTE_SCRIPTS {
            let path = format!("C:/tfb/Content/Levels/Includes/{script}");
            assert_eq!(
                c.whitelist.listing("a route placement", &path),
                guac_script::Listed::Every,
                "{script}"
            );
        }
        for r in members::ROUTE {
            assert!(
                c.members
                    .get(MemberId::named(r.namespace, r.name))
                    .is_some(),
                "{}",
                r.path()
            );
        }
        // What the characters' attacks reach (openspec AU.2): a cell each.
        for r in members::ATTACKS {
            let (_, place) = c
                .members
                .get(MemberId::named(r.namespace, r.name))
                .unwrap_or_else(|| panic!("{}", r.path()));
            assert_eq!(place, guac_script::Place::ValueCell, "{}", r.path());
        }
        // The sounds' members on the host (openspec AR.10), over the cells
        // the earlier tables registered for some of them.
        for r in members::SOUNDS {
            let (_, place) = c
                .members
                .get(MemberId::named(r.namespace, r.name))
                .unwrap_or_else(|| panic!("{}", r.path()));
            assert_eq!(place, guac_script::Place::Host, "{}", r.path());
        }
        // AS.11's every-level table (openspec AS.37): its host's and cells
        // registered, as the gap test registers them.
        for r in members::LEVELS {
            let id = MemberId::named(r.namespace, r.name);
            assert_eq!(
                c.members.get(id).is_some(),
                r.keeper != members::Keeper::Vm,
                "{}",
                r.path()
            );
        }
        // Chapter 1's music every tick (openspec AR.16), the TV's and the
        // fight's alike; a probe's `without_music` takes them off alone.
        let music = |c: &Config, script: &str| {
            let path = format!("C:/tfb/Content/Levels/Level_213/Scripts/{script}");
            c.whitelist.listing("a music placement", &path)
        };
        let without = without_music(fight_config(Mode::Play));
        for script in MUSIC_SCRIPTS {
            assert_eq!(music(&c, script), guac_script::Listed::Every, "{script}");
            assert_eq!(
                music(&fight, script),
                guac_script::Listed::Every,
                "{script}"
            );
            assert_ne!(
                music(&without, script),
                guac_script::Listed::Every,
                "{script}"
            );
        }
        for script in ROUTE_SCRIPTS {
            let path = format!("C:/tfb/Content/Levels/Includes/{script}");
            assert_eq!(
                without.whitelist.listing("a route placement", &path),
                guac_script::Listed::Every,
                "{script}"
            );
        }
    }

    /// The TV's activity rule from the points it's given (openspec AH.27):
    /// none, every ranged placement asleep; the level's start before a hero
    /// is in, as from a hero standing there; a placement taken off (a
    /// breakable broken) left out of it for good.
    #[test]
    fn the_activity_rule_from_given_points_and_a_placement_taken_off() {
        let (mut w, grunt, _) = world();
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer, g.managed) = (30.0, 1.0, true);
        assert_eq!(w.activity_at(&[]), vec![(grunt, false)], "no one");
        let start = Vec3::new(9.0, 0.0, 0.0);
        assert_eq!(w.activity_at(&[start]), vec![(grunt, true)]);
        let mut hero = him(start);
        assert_eq!(w.activity(&mut hero), w.activity_at(&[start]));
        let obj = *w.objects.iter().find(|(_, h)| **h == grunt).unwrap().0;
        assert!(w.take_off(obj));
        assert!(w.taken_off(obj));
        assert!(w.activity_at(&[start]).is_empty(), "left out for good");
        assert!(!w.take_off(Obj { unit: 9, index: 9 }));
    }

    /// openspec AS.83: the asleep placements' checks after the walk measure
    /// from where the step's pass measured. Asleep at the pass (no range, a
    /// buffer: its scripts' to wake), the Grunt wakes only once a write
    /// during the walk brings him in range (the elf helper's 500 ft, here
    /// past the 29.5 ft he stands off; 20 ft doesn't), fired as the pass
    /// fires; taken as awake, he isn't woken twice, and the next pass keeps
    /// him awake; with no one to measure from, or unmanaged, he isn't.
    #[test]
    fn the_checks_after_the_walk_wake_what_a_write_brought_into_range() {
        let (mut w, grunt, _) = world();
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer, g.managed) = (0.0, 250.0, true);
        let at = Vec3::new(9.0, 0.0, 0.0);
        assert_eq!(w.activity_at(&[at]), vec![(grunt, false)]);
        assert!(w.wakes(&[grunt]).is_empty(), "nothing written");
        w.actors.get_mut(&grunt).unwrap().range = 20.0;
        assert!(w.wakes(&[grunt]).is_empty(), "20 ft, out of range");
        assert!(w.fired.is_empty());
        w.actors.get_mut(&grunt).unwrap().range = 500.0;
        assert!(w.wakes(&[]).is_empty(), "only those asked");
        assert_eq!(w.wakes(&[grunt, 0xdead]), [grunt]);
        assert_eq!(w.fired, [grunt]);
        assert!(w.wakes(&[grunt]).is_empty(), "awake now");
        assert_eq!(w.activity_at(&[at]), vec![(grunt, true)]);
        // From where the pass measured: none in, the range reaches no one.
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer) = (0.0, 250.0);
        assert_eq!(w.activity_at(&[]), vec![(grunt, false)]);
        w.actors.get_mut(&grunt).unwrap().range = 500.0;
        assert!(w.wakes(&[grunt]).is_empty(), "no one to measure from");
        w.actors.get_mut(&grunt).unwrap().managed = false;
        w.activity_from = vec![at];
        assert!(w.wakes(&[grunt]).is_empty(), "unmanaged");
    }

    /// AH.25: a placement the activity rule wakes is fired from then on, in
    /// the order woken; asleep again it stays fired. The fired triggers are
    /// those with a range going by a trigger's name (AH.4's rule).
    #[test]
    fn what_the_activity_rule_wakes_is_fired_and_triggers_go_by_name() {
        let (mut w, grunt, _) = world();
        let g = w.actors.get_mut(&grunt).unwrap();
        (g.range, g.buffer, g.managed) = (30.0, 0.0, true);
        assert!(
            w.activity_at(&[Vec3::new(50.0, 0.0, 0.0)])
                .iter()
                .all(|a| !a.1)
        );
        assert!(w.fired.is_empty());
        w.activity_at(&[Vec3::new(9.0, 0.0, 0.0)]);
        assert_eq!(w.fired, [grunt]);
        w.activity_at(&[Vec3::new(50.0, 0.0, 0.0)]);
        w.activity_at(&[Vec3::new(9.0, 0.0, 0.0)]);
        assert_eq!(w.fired, [grunt], "once");
        for name in [
            "Setup_001_Trigger",
            "Activate_Norticus and Chompies",
            "Transition 7-1",
            "Pop_to_Camera_5",
            "trg_lvl_213_lush_peak",
            "Synthetic_trigger",
        ] {
            assert!(is_trigger(name), "{name}");
        }
        for name in [
            "Enemy_Chompy(1)",
            "Setup_001_Controller",
            "Transitional",
            "pottery_1(8)",
        ] {
            assert!(!is_trigger(name), "{name}");
        }
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
            lights: HashMap::new(),
            emitters: HashMap::new(),
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
            turns: HashMap::new(),
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

    /// AH.195's synthetic level ([`guac_formats::script::builder::load_order`])
    /// linked alone, read as the TV reads a part, and its VM over every
    /// script, strict: the files, the handles, the VM and the world.
    fn load_order_run() -> (Files, HashMap<Obj, Handle>, Vm, ScriptWorld) {
        use guac_formats::script::Graph;
        let bytes = guac_formats::script::builder::load_order();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = guac_script::Unit {
            name: "level",
            role: guac_script::Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let files = Files::read(&[(0, &igz)]).unwrap();
        let handles = headless_handles(&files);
        let mut c = Config::new(Mode::Strict);
        c.whitelist = Whitelist::All;
        let vm = init_chain(program, &files, &handles, &[0], c);
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        (files, handles, vm, world)
    }

    /// One step from where the activity rule is measured (metres).
    fn load_order_step(vm: &mut Vm, world: &mut ScriptWorld, from: Vec3) {
        vm.set_activity(&world.activity_at(&[from]));
        let mut spawn = headless_spawner(1 << 40);
        let mut host = WorldHost {
            world,
            engine: &mut NoEngine,
            generator: &mut Generator::seeded(1),
            spawn: &mut spawn,
        };
        vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
    }

    /// Each placement's `seen` (its place in the last step's walk), in
    /// `names`' order.
    fn load_order_seen(
        vm: &Vm,
        files: &Files,
        handles: &HashMap<Obj, Handle>,
        names: &[&str],
    ) -> Vec<Option<Value>> {
        names
            .iter()
            .map(|n| {
                let p = files.placed.iter().find(|p| p.name == *n).unwrap();
                vm.local(vm.placement_of(handles[&p.obj]).unwrap(), "seen")
            })
            .collect()
    }

    fn ints(v: &[i32]) -> Vec<Option<Value>> {
        v.iter().map(|&n| Some(Value::Int(n))).collect()
    }

    /// openspec AH.195: a level's placements join the VM in the order its
    /// load adds them, its file's root list's, not the order its objects lie
    /// in, those awake from the load ahead of those that wait asleep: the
    /// opener (second on the list, third in the file) ahead of the reader
    /// (last on the list, first in the file), so the reader finds itself open
    /// the step the opener opens it, as the original's `Sliding_Rock(5)`
    /// reads the `open` `Sheep_1` sets the frame it's set.
    #[test]
    fn a_levels_placements_tick_in_its_root_lists_order_not_its_objects() {
        use guac_formats::script::builder::{
            LOAD_OBJECTS, LOAD_OPENER, LOAD_READER, LOAD_ROOT, LOAD_SLEEPER_A, LOAD_SLEEPER_B,
        };
        let (files, handles, mut vm, mut world) = load_order_run();
        let names: Vec<&str> = files.placed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, LOAD_OBJECTS, "the files' placements in object order");
        for p in &files.placed {
            let k = LOAD_ROOT.iter().position(|n| *n == p.name).unwrap();
            assert_eq!(p.load, k as u32, "{}", p.name);
        }
        let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            order,
            [LOAD_OPENER, LOAD_READER, LOAD_SLEEPER_A, LOAD_SLEEPER_B],
            "awake from the load first, each part in the root list's order"
        );
        load_order_step(&mut vm, &mut world, Vec3::new(-100.0, 0.0, 0.0));
        let all = [LOAD_OPENER, LOAD_READER, LOAD_SLEEPER_A, LOAD_SLEEPER_B];
        let seen = load_order_seen(&vm, &files, &handles, &all);
        assert_eq!(seen[..2], ints(&[0, 1]), "the opener, then the reader");
        assert_eq!(seen[2..], [None, None], "the sleepers asleep, out of range");
        let reader = files.placed.iter().find(|p| p.name == LOAD_READER).unwrap();
        let reader = vm.placement_of(handles[&reader.obj]).unwrap();
        assert_eq!(
            vm.local(reader, "Saw"),
            Some(Value::Int(1)),
            "open its first tick"
        );
    }

    /// openspec AH.195: a placement woken later joins the awake placements'
    /// end, and those waking in one pass wake in the order they went to
    /// sleep, not the pass's (its handles') or the file's: the sleepers,
    /// asleep from the load in the root list's order, wake A then B though
    /// the pass names B first; then B, out of range first, and A after it
    /// wake B then A.
    #[test]
    fn a_placement_woken_later_takes_its_place_behind_the_awake() {
        use guac_formats::script::builder::{
            LOAD_OPENER, LOAD_READER, LOAD_SLEEPER_A, LOAD_SLEEPER_AT, LOAD_SLEEPER_B,
        };
        let (files, handles, mut vm, mut world) = load_order_run();
        let all = [LOAD_OPENER, LOAD_READER, LOAD_SLEEPER_A, LOAD_SLEEPER_B];
        let far = Vec3::new(-100.0, 0.0, 0.0);
        load_order_step(&mut vm, &mut world, far);
        let [a, b] = LOAD_SLEEPER_AT.map(Vec3::from_array);
        let both = (a + b) / 2.0;
        let pass = world.activity_at(&[both]);
        let named: Vec<&str> = pass
            .iter()
            .map(|(h, _)| {
                files
                    .placed
                    .iter()
                    .find(|p| handles[&p.obj] == *h)
                    .unwrap()
                    .name
                    .as_str()
            })
            .collect();
        assert_eq!(
            named,
            [LOAD_READER, LOAD_SLEEPER_B, LOAD_OPENER, LOAD_SLEEPER_A],
            "the pass names them by handle"
        );
        load_order_step(&mut vm, &mut world, both);
        assert_eq!(
            load_order_seen(&vm, &files, &handles, &all),
            ints(&[2, 3, 4, 5]),
            "woken behind the awake, A then B as they slept"
        );
        // B out of range alone (A 2.5 m off, B 3.5 m, the range 3.048 m),
        // then A too; then both back.
        let toward_a = a - Vec3::new(2.5, 0.0, 0.0);
        load_order_step(&mut vm, &mut world, toward_a);
        assert_eq!(
            load_order_seen(&vm, &files, &handles, &all),
            ints(&[6, 7, 8, 9]),
            "B's last tick as it goes to sleep (openspec AR.22)"
        );
        load_order_step(&mut vm, &mut world, far);
        assert_eq!(
            load_order_seen(&vm, &files, &handles, &all),
            ints(&[10, 11, 12, 9]),
            "A's last tick, B asleep"
        );
        load_order_step(&mut vm, &mut world, both);
        assert_eq!(
            load_order_seen(&vm, &files, &handles, &all),
            ints(&[13, 14, 16, 15]),
            "B, then A, as they went to sleep"
        );
    }

    /// openspec AH.146: a placed gem waiting asleep for the activity rule
    /// takes its worth at its STARTUP from what the loot system's first tick
    /// set, though both are on the level's first step and the gem lies ahead
    /// of the system in the file and on its root list: the system, awake
    /// from the load, ticks first, and the gem, woken by the hero standing
    /// on it that step, joins behind it (the original's scheduler holds
    /// `LootSystem_Init` awake from the load and `Emerald`, `Copper_Coins`
    /// and `Emerald(2)` woken 42.5 s later by their 160 ft range, AH.195's
    /// read at AF.8's fresh state). The gem holds the worth as its id.
    #[test]
    fn a_placed_gem_takes_its_worth_after_the_loot_systems_first_tick() {
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{LOOT_GEM, LOOT_GEM_WORTH, LOOT_SYSTEM, placed_loot};
        let bytes = placed_loot();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = guac_script::Unit {
            name: "level",
            role: guac_script::Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let files = Files::read(&[(0, &igz)]).unwrap();
        let names: Vec<&str> = files.placed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, [LOOT_GEM, LOOT_SYSTEM], "the gem first in the file");
        let handles = headless_handles(&files);
        let mut c = fight_config(Mode::Strict);
        c.whitelist = Whitelist::All;
        let mut vm = init_chain(program, &files, &handles, &[0], c);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let gem = files.placed.iter().find(|p| p.name == LOOT_GEM).unwrap();
        let gem = handles[&gem.obj];
        let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
        assert_eq!(order, [LOOT_SYSTEM, LOOT_GEM], "awake from the load first");
        // The level's first step with the hero on the gem.
        load_order_step(&mut vm, &mut world, Vec3::ZERO);
        let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
        assert_eq!(order, [LOOT_SYSTEM, LOOT_GEM], "woken behind the awake");
        assert_eq!(world.actor(gem).unwrap().id, LOOT_GEM_WORTH, "its worth");
    }

    /// openspec AH.59: `init_chain` adds the placements of a unit that
    /// [`crate::frame::AWAKE_HEAD`] names in its order, the rest after them
    /// in file order: `global.bld`'s as the original's scheduler holds them,
    /// whatever their order in the file.
    #[test]
    fn init_chain_adds_globals_in_the_recorded_order() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::Writer;
        const PATH: &str = "C:/tfb/Synthetic/Global.ai";
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            w.end_block(pre);
            w.end(PATH);
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = guac_script::Unit {
            name: "global",
            role: guac_script::Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        // global.bld's file order.
        let names = [
            "Macros_UI",
            "Spyro_PortalManager",
            "Spyro_PortalControl",
            "Pause Controller",
            "Macros_Animation",
            "Macros_GameSystems",
            "Macros_Music",
            "movementUpdate",
            "Macros_Sound",
        ];
        let files = Files {
            placed: names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let obj = Obj {
                        unit: 0,
                        index: 100 + i as u32,
                    };
                    Placed::named(obj, name, Some(PATH), RUNNING)
                })
                .collect(),
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut c = Config::new(Mode::Strict);
        c.whitelist = Whitelist::All;
        let vm = init_chain(program, &files, &handles, &[0], c);
        let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            order,
            [
                "Macros_Animation",
                "Macros_GameSystems",
                "Macros_Music",
                "Macros_Sound",
                "Macros_UI",
                "Spyro_PortalManager",
                "Pause Controller",
                "Spyro_PortalControl",
                "movementUpdate"
            ]
        );
    }

    /// openspec AT.44: level 213's UI controller is in the scheduler's list
    /// as the June record holds it, every tick on the TV's listing and the
    /// fight's, by its script, so level 212's running the same file is too
    /// (AT.44's review), Eon's lines' activators on it (AR.29); an
    /// interface template (5, as
    /// `Dialogue_UI` is) never joins the run, whatever the listing, while a
    /// running placement (4) does, in its file's root-list order.
    #[test]
    fn the_levels_ui_controller_ticks_and_no_interface_template_joins() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::Writer;
        const CONTROLLER: &str =
            "C:/tfb/Content/Levels/Level_213/cutscenes/scripts/213_UI_Controller.ai";
        const DIALOGUE: &str =
            "C:/tfb/Content/Levels/Includes/Spyro_GenericStuff/scripts/Dialogue_UI.ai";
        for c in [smash_config(Mode::Play), fight_config(Mode::Play)] {
            assert_eq!(
                c.whitelist.listing("213_UI_Controller", CONTROLLER),
                guac_script::Listed::Every
            );
            assert_eq!(
                c.whitelist.listing("212_UI_Controller", CONTROLLER),
                guac_script::Listed::Every,
                "level 212's, the same file"
            );
            assert_eq!(
                c.whitelist.listing(
                    "213_UI_Controller",
                    "C:/tfb/Content/Levels/Level_213/Scripts/Another.ai"
                ),
                guac_script::Listed::Off,
                "by its script, not its placement's name"
            );
            // Eon's lines (openspec AR.29): on the TV's listing.
            assert_eq!(
                c.whitelist.listing(
                    "Eon_2_Keep_Going",
                    "C:/tfb/Content/Levels/Level_213/Scripts/Eon_Line_Activator.ai"
                ),
                guac_script::Listed::Every
            );
        }
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            for path in [CONTROLLER, DIALOGUE] {
                let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
                w.end_block(pre);
                w.end(path);
            }
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = guac_script::Unit {
            name: "level",
            role: guac_script::Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let obj = |index| Obj { unit: 0, index };
        let mut template = Placed::named(obj(100), "Dialogue_UI", Some(DIALOGUE), 5);
        template.load = 0;
        let mut controller =
            Placed::named(obj(101), "213_UI_Controller", Some(CONTROLLER), RUNNING);
        controller.load = 1;
        let files = Files {
            placed: vec![template, controller],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut c = Config::new(Mode::Strict);
        c.whitelist = Whitelist::All;
        let vm = init_chain(program, &files, &handles, &[0], c);
        let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
        assert_eq!(order, ["213_UI_Controller"]);
        assert_eq!(
            vm.placement_of(handles[&obj(100)]),
            None,
            "a template never joins"
        );
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

    /// AG.211 on the world alone: a fall-back's hand-back lasts until a
    /// hero's scripts join (`hero_template`), the scripts' next write the
    /// view's again; one of a view the scripts never had is nothing; a stop
    /// of the camera's scripts after a fall-back makes it for good, a join
    /// after that leaving every write going nowhere.
    #[test]
    fn a_fall_backs_hand_back_ends_at_a_join_unless_a_stop_came_after_it() {
        use crate::viewport::Source;
        let (mut w, grunt, _) = world();
        let source = |w: &ScriptWorld| (w.viewport().source, w.view_back_at_a_join());
        w.hand_back_view_until_a_join();
        assert_eq!(source(&w), (Source::Engine, false), "nothing to hand back");
        assert!(w.view.take(), "the scripts place it");
        w.hand_back_view_until_a_join();
        assert_eq!(source(&w), (Source::HandedBack, true));
        assert!(!w.view.take(), "a write meanwhile goes nowhere");
        w.hero_template(GRUNT, grunt);
        assert_eq!(
            source(&w),
            (Source::Engine, false),
            "the join gives it back"
        );
        assert!(w.view.take(), "their next write takes it");
        w.hand_back_view_until_a_join();
        w.hand_back_view();
        assert_eq!(
            source(&w),
            (Source::HandedBack, false),
            "the stop's for good"
        );
        w.hero_template(GRUNT, grunt);
        assert_eq!(source(&w), (Source::HandedBack, false), "a join after it");
        assert!(!w.view.take());
    }

    /// A macro's parameter found by its name trimmed (openspec AH.28): the
    /// file spells `Portal Mode - Adventure Mode`'s `portal data in` with
    /// trailing spaces, and a call names it as the file does; a macro or a
    /// parameter the program lacks is none.
    #[test]
    fn a_parameter_is_found_by_its_name_trimmed() {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::Writer;
        use guac_script::{Role, Unit};

        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (macros, _) = w.open(Class::MacroSection, "MACROS");
            let zero = w.int(0);
            let (at, _) = w.define(ADVENTURE_MODE, &[("portal data in  ", zero)]);
            w.end_block(at);
            w.end_block(macros);
            w.end_block(pre);
            let idle = w.behaviour("idle");
            w.end_block(idle);
            w.end("C:/tfb/Synthetic/PortalControl.ai");
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Program::link(&[unit]).unwrap();
        assert_eq!(
            parameter_named(&program, ADVENTURE_MODE, "portal data in").as_deref(),
            Some("portal data in  ")
        );
        assert_eq!(
            parameter_named(&program, "portal mode - adventure mode", "PORTAL DATA IN").as_deref(),
            Some("portal data in  "),
            "any case"
        );
        assert_eq!(
            parameter_named(&program, ADVENTURE_MODE, "portal data"),
            None
        );
        assert_eq!(
            parameter_named(&program, "Portal Mode - Bootup", "portal data in"),
            None
        );
    }

    /// [`builder::ui_containers`]' program.
    ///
    /// [`builder::ui_containers`]: guac_formats::script::builder::ui_containers
    fn ui_containers() -> Arc<Program> {
        use guac_formats::script::Graph;
        use guac_formats::script::builder;
        use guac_script::{Role, Unit};

        let bytes = builder::ui_containers();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// A container of [`ui_containers`]' `UI Containers`, the global made
    /// first if nothing has.
    fn ui_container(vm: &mut Vm, name: &str) -> Value {
        use guac_formats::script::builder::UI_CONTAINERS;
        let g = vm.program().global_named(UI_CONTAINERS).unwrap();
        let containers = vm.global(g).map(|v| vm.value(v)).unwrap();
        vm.field(containers, name).map(|v| vm.value(v)).unwrap()
    }

    /// openspec AR.25: every config a run is built on registers the
    /// sounds' and the music's members, each as their table keeps it, and
    /// the placement's clones the music update checks, since the player
    /// loop runs the music controller's sound system update in any run that
    /// holds `Macros_Music`; the TV's after every table that registered a
    /// sound member as a value cell.
    #[test]
    fn every_config_registers_the_music_systems_members() {
        use guac_script::MemberId;
        use guac_script::members::{Keeper, MUSIC, SOUNDS};
        for (name, c) in [
            ("config", config(Mode::Strict)),
            ("hero_config", hero_config(Mode::Strict)),
            ("smash_config", smash_config(Mode::Strict)),
            ("fight_config", fight_config(Mode::Strict)),
        ] {
            for r in SOUNDS.iter().chain(MUSIC) {
                let id = MemberId::named(r.namespace, r.name);
                let got = c.members.get(id).map(|(_, place)| place);
                let want = match r.keeper {
                    Keeper::Cell => Some(guac_script::Place::ValueCell),
                    Keeper::Host | Keeper::Interface => Some(guac_script::Place::Host),
                    Keeper::Vm => continue,
                };
                assert_eq!(got, want, "{name}: {}.{}", r.namespace, r.name);
            }
            // The music update's check on a placement's clones, past the
            // init chain's table (the hero's registers it).
            let clones = MemberId::named("abstractplacement", "clones");
            assert_eq!(
                c.members.get(clones).map(|(_, place)| place),
                Some(guac_script::Place::Host),
                "{name}: abstractplacement.clones"
            );
        }
    }

    /// The music hold's run (openspec AR.23, AR.30): one unit of three
    /// placements, a music controller's, another script's and a second music
    /// one a script suspended, each counting its STARTUPs (`starts`) and
    /// ticks (`count`); their placements, in that order.
    fn music_fixture() -> (Vm, ScriptWorld, Files, [PlacementId; 3]) {
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{El, Writer};
        use guac_script::{Role, Unit};

        const MUSIC: &str = "C:/tfb/Content/Levels/Level_213/Scripts/Music_Level_Controller.ai";
        const OTHER: &str = "C:/tfb/Synthetic/Other.ai";
        let mut b = IgzBuilder::new();
        let actors;
        {
            let mut w = Writer::new(&mut b);
            actors = [
                w.b.object(0, "tfbActorInfo", 0xF8),
                w.b.object(0, "tfbActorInfo", 0xF8),
                w.b.object(0, "tfbActorInfo", 0xF8),
            ];
            for path in [MUSIC, OTHER] {
                let starts = w.variable("starts", "ValueInfo", Some("IntMeasurement"));
                let count = w.variable("count", "ValueInfo", Some("IntMeasurement"));
                let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
                let (start, _) = w.open(Class::StartUp, "STARTUP");
                w.inc(&[El::Local(starts)]);
                w.end_block(start);
                let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
                w.end_block(shut);
                let (macros, _) = w.open(Class::MacroSection, "MACROS");
                w.end_block(macros);
                w.end_block(pre);
                let idle = w.behaviour("idle");
                w.inc(&[El::Local(count)]);
                w.end_block(idle);
                w.end(path);
            }
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
        let [music, other, suspended] = actors.map(|l| Obj {
            unit: 0,
            index: igz.object_at(l).unwrap().0 as u32,
        });
        let files = Files {
            placed: vec![
                Placed::named(music, "dynamic_sound_controller", Some(MUSIC), RUNNING),
                Placed::named(other, "Other", Some(OTHER), RUNNING),
                Placed::named(suspended, "suspended", Some(MUSIC), RUNNING),
            ],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut c = Config::new(Mode::Strict);
        c.whitelist = Whitelist::All;
        let mut vm = init_chain(program, &files, &handles, &[0], c);
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let id = |o: &Obj| {
            let vm = &vm;
            vm.placement_of(handles[o]).unwrap()
        };
        let ids = [id(&music), id(&other), id(&suspended)];
        assert!(vm.set_suspended(handles[&suspended], true), "a script's");
        (vm, world, files, ids)
    }

    /// One step of [`music_fixture`]'s run.
    fn music_step(vm: &mut Vm, world: &mut ScriptWorld) {
        let mut spawn = headless_spawner(1 << 40);
        let mut host = WorldHost {
            world,
            engine: &mut NoEngine,
            generator: &mut Generator::seeded(1),
            spawn: &mut spawn,
        };
        vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
    }

    /// A book holding one archive's sounds, none in it.
    fn book_of(archive: &str) -> Arc<guac_sound::SoundBook> {
        Arc::new(guac_sound::SoundBook::new([guac_ir::sound::Sounds {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([1; 32]),
            },
            archive: archive.into(),
            sounds: Vec::new(),
            data: Vec::new(),
            refused: Vec::new(),
        }]))
    }

    /// openspec AR.23: the level's music scripts wait for the level's
    /// sounds. With no book, or one lacking the level's archive, each
    /// placement of the level running one of [`MUSIC_SCRIPTS`] is held
    /// before its first tick (another script's runs on; one a script
    /// suspended isn't taken as held); once the book holds the level's
    /// sounds the held ones go, each running its STARTUP at its next tick,
    /// and after that there's nothing to hold.
    #[test]
    fn the_levels_music_waits_for_its_sounds_then_starts() {
        const OTHER: &str = "C:/tfb/Synthetic/Other.ai";
        let (mut vm, mut world, files, [music_id, other_id, suspended_id]) = music_fixture();
        let step = music_step;
        let book = book_of;
        let stems = HashMap::from([(0u16, "level_213_tutotial".to_owned())]);
        assert_eq!(hold_music(&mut vm, &mut world, &files, 0), Music::Waits(1));
        assert_eq!(world.music_held(), 1, "reported");
        for _ in 0..3 {
            step(&mut vm, &mut world);
        }
        assert_eq!(
            vm.local(music_id, "starts"),
            None,
            "held before its first tick"
        );
        assert_eq!(vm.local(other_id, "count"), Some(Value::Int(3)));
        assert_eq!(
            hold_music(&mut vm, &mut world, &files, 0),
            Music::Waits(1),
            "once"
        );
        world.set_sounds(book("permanent/global.bld"), stems.clone());
        assert_eq!(
            hold_music(&mut vm, &mut world, &files, 0),
            Music::Waits(1),
            "the book lacks the level's"
        );
        world.set_sounds(book("level/level_213_tutotial.bld"), stems);
        assert_eq!(hold_music(&mut vm, &mut world, &files, 0), Music::Starts(1));
        assert_eq!(world.music_held(), 0, "none held now");
        step(&mut vm, &mut world);
        assert_eq!(vm.local(music_id, "starts"), Some(Value::Int(1)));
        assert_eq!(
            vm.local(music_id, "count"),
            Some(Value::Int(1)),
            "from then"
        );
        assert!(vm.is_suspended(suspended_id), "a script's suspension stays");
        assert_eq!(vm.local(suspended_id, "starts"), None);
        assert_eq!(hold_music(&mut vm, &mut world, &files, 0), Music::Runs);
        assert!(is_music_script(
            "c:/tfb/content/levels/level_213/scripts/snd_robo_ducking.ai"
        ));
        assert!(!is_music_script(OTHER));
    }

    /// openspec AS.47: a level's intro is held before its first tick, by its
    /// placement in the level's own file (another file's named alike, and
    /// any other placement, run on), and let go to run its STARTUP at its
    /// next tick; nothing's let go twice.
    #[test]
    fn a_levels_intro_waits_for_a_hero() {
        let (mut vm, mut world, mut files, [intro_id, other_id, _]) = music_fixture();
        files.placed[0].name = "214_Intro_IGC".into();
        assert!(
            !hold_intro(&mut vm, &mut world, &files, 1, "214_Intro_IGC"),
            "another file's"
        );
        assert!(
            !hold_intro(&mut vm, &mut world, &files, 0, "Intro"),
            "no such placement"
        );
        assert!(hold_intro(&mut vm, &mut world, &files, 0, "214_Intro_IGC"));
        for _ in 0..3 {
            music_step(&mut vm, &mut world);
        }
        assert_eq!(
            vm.local(intro_id, "starts"),
            None,
            "held before its first tick"
        );
        assert_eq!(vm.local(other_id, "count"), Some(Value::Int(3)));
        assert!(release_intro(&mut vm, &mut world));
        assert!(!release_intro(&mut vm, &mut world), "once");
        music_step(&mut vm, &mut world);
        assert_eq!(vm.local(intro_id, "starts"), Some(Value::Int(1)));
        // Level 214's on the TV's listing since AS.60, no other level's.
        assert_eq!(intro("level_214_tutotial_2"), Some("214_Intro_IGC"));
        assert_eq!(intro("level_212"), None);
    }

    /// openspec AR.30 (AR.27's key): the music waits only while the level's
    /// sounds are on their way. With none coming it isn't held; held ones are
    /// let go to run silent the moment none are coming, each running its
    /// STARTUP at its next tick; and a book holding them with none coming is
    /// the start as ever.
    #[test]
    fn the_levels_music_waits_only_while_its_sounds_are_on_their_way() {
        let stems = HashMap::from([(0u16, "level_214_tutotial_2".to_owned())]);
        // None coming, no book: nothing held, the music ticks from the start.
        let (mut vm, mut world, files, [music_id, ..]) = music_fixture();
        assert_eq!(
            hold_music_while(&mut vm, &mut world, &files, 0, false),
            Music::Runs
        );
        assert_eq!(world.music_held(), 0);
        music_step(&mut vm, &mut world);
        assert_eq!(vm.local(music_id, "starts"), Some(Value::Int(1)));
        assert_eq!(world.music_silent(), 0, "none held, none let go");
        // Coming: held; then none coming without the book: let go, silent.
        let (mut vm, mut world, files, [music_id, _, suspended_id]) = music_fixture();
        assert_eq!(
            hold_music_while(&mut vm, &mut world, &files, 0, true),
            Music::Waits(1)
        );
        music_step(&mut vm, &mut world);
        assert_eq!(vm.local(music_id, "starts"), None, "held");
        world.set_sounds(book_of("permanent/global.bld"), stems.clone());
        assert_eq!(
            hold_music_while(&mut vm, &mut world, &files, 0, false),
            Music::Silent(1)
        );
        assert_eq!(world.music_held(), 0);
        assert_eq!(world.music_silent(), 1, "said");
        music_step(&mut vm, &mut world);
        assert_eq!(vm.local(music_id, "starts"), Some(Value::Int(1)));
        assert!(vm.is_suspended(suspended_id), "a script's suspension stays");
        assert_eq!(
            hold_music_while(&mut vm, &mut world, &files, 0, false),
            Music::Runs
        );
        // The book holding them: never held, coming or not.
        let (mut vm, mut world, files, _) = music_fixture();
        world.set_sounds(book_of("level/level_214_tutotial_2.bld"), stems);
        for coming in [true, false] {
            assert_eq!(
                hold_music_while(&mut vm, &mut world, &files, 0, coming),
                Music::Runs,
                "coming {coming}"
            );
        }
    }

    /// The HUD hides while the scripts hide the player's (mergefix4,
    /// 2026-10-26: the pit's intro and Norticus' scene hold it at 1 or 2):
    /// over 0 hides it, 0 or under shows it, as does the announcements'
    /// container's state or no `UI Containers` made yet.
    #[test]
    fn the_player_hud_hides_while_its_containers_hide_state_is_over_0() {
        use guac_formats::script::builder::{UI_ANNOUNCE, UI_HIDE_STATE, UI_PLAYER_HUD};

        let mut vm = Vm::new(ui_containers(), Config::new(Mode::Strict));
        assert!(!player_hud_hidden(&vm), "nothing's made it yet");
        let (hud, announce) = (
            ui_container(&mut vm, UI_PLAYER_HUD),
            ui_container(&mut vm, UI_ANNOUNCE),
        );
        assert!(!player_hud_hidden(&vm), "made, at 0");
        for (state, hidden) in [(1, true), (2, true), (1, true), (0, false), (-1, false)] {
            assert!(vm.set_field(hud, UI_HIDE_STATE, Value::Int(state)));
            assert_eq!(player_hud_hidden(&vm), hidden, "{state}");
        }
        assert!(vm.set_field(hud, UI_HIDE_STATE, Value::Int(0)));
        assert!(vm.set_field(announce, UI_HIDE_STATE, Value::Int(1)));
        assert!(!player_hud_hidden(&vm), "the announcements' alone");
    }
}
