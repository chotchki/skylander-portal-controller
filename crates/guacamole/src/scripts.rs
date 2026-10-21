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
    Program, Value, Vm, Whitelist,
};
use tracing::{info, warn};

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

/// An actor placement of a file the scripts load with.
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
                });
            }
        }
        Ok(out)
    }
}

/// The host's members, by what each answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    Platform,
    Configuration,
    GameLanguage,
    ConsoleLanguage,
    TimeScale,
    LoadLevel,
    Buffer,
    Id,
    Waypoints,
    Controller(usize),
    Physical(usize),
    Portal,
}

impl Member {
    fn table() -> HashMap<MemberId, Member> {
        let mut t = HashMap::new();
        let mut add = |ns: &str, name: &str, m: Member| {
            t.insert(MemberId::named(ns, name), m);
        };
        add("system", "hardware platform", Member::Platform);
        add("system", "game configuration", Member::Configuration);
        add("system", "game language", Member::GameLanguage);
        add("system", "console language", Member::ConsoleLanguage);
        add("system", "game time scale", Member::TimeScale);
        add("system", "load level", Member::LoadLevel);
        add("placement", "deactivation buffer", Member::Buffer);
        add("abstractplacement", "id", Member::Id);
        add("spritematrixmeasurement", "id", Member::Id);
        add("actorinfo", "waypoints", Member::Waypoints);
        for n in 0..2 {
            add(
                "scriptcontroller",
                &format!("controller {}", n + 1),
                Member::Controller(n),
            );
        }
        for n in 0..4 {
            add(
                "scriptcontroller",
                &format!("physical controller {}", n + 1),
                Member::Physical(n),
            );
        }
        add("redoctaneperipheralsystemobject", "portal", Member::Portal);
        t
    }
}

/// A placement or clone, as the host keeps it.
#[derive(Debug, Clone, PartialEq)]
struct Kept {
    /// Its object in a file; a clone has none.
    obj: Option<Obj>,
    id: i32,
    /// The deactivation buffer its file gives it, and its now.
    buffer_loaded: f32,
    buffer: f32,
    waypoints: Vec<Value>,
    removed: bool,
}

/// What a clone handle is asked for: the template it copies.
pub type Spawner<'a> = dyn FnMut(Value) -> Handle + 'a;

/// The engine state the scripts read and write (the module docs).
#[derive(Resource, Debug)]
pub struct ScriptWorld {
    members: HashMap<MemberId, Member>,
    ids: HashMap<Obj, i32>,
    kept: HashMap<Handle, Kept>,
    objects: HashMap<Obj, Handle>,
    controllers: [Handle; 2],
    physical: [Handle; 4],
    portal: Handle,
    time_scale: f32,
    load_level: Value,
    /// The levels the scripts asked for, as each step took them.
    pub requests: Vec<Value>,
}

impl ScriptWorld {
    /// Over `files`, each placement under the handle `handles` gives its
    /// object, and the pads and portal under `objects`' seven handles.
    pub fn new(files: &Files, handles: &HashMap<Obj, Handle>, objects: [Handle; 7]) -> Self {
        let mut kept = HashMap::new();
        for p in &files.placed {
            if let Some(&h) = handles.get(&p.obj) {
                kept.insert(
                    h,
                    Kept {
                        obj: Some(p.obj),
                        id: p.id,
                        buffer_loaded: p.buffer,
                        buffer: p.buffer,
                        waypoints: p.waypoints.iter().map(|w| Value::Object(*w)).collect(),
                        removed: false,
                    },
                );
            }
        }
        Self {
            members: Member::table(),
            ids: files.ids.clone(),
            kept,
            objects: handles.iter().map(|(o, h)| (*o, *h)).collect(),
            controllers: [objects[0], objects[1]],
            physical: [objects[2], objects[3], objects[4], objects[5]],
            portal: objects[6],
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

    /// Whether a remove took the placement or clone under `h` off.
    pub fn removed(&self, h: Handle) -> bool {
        self.kept.get(&h).is_some_and(|k| k.removed)
    }

    /// The handles of the clones the scripts have made, live or taken off.
    pub fn clones(&self) -> Vec<Handle> {
        let mut c: Vec<Handle> = self
            .kept
            .iter()
            .filter(|(_, k)| k.obj.is_none())
            .map(|(h, _)| *h)
            .collect();
        c.sort_unstable();
        c
    }

    fn handle(&self, v: Value) -> Option<Handle> {
        match v {
            Value::Entity(h) => Some(h),
            Value::Object(o) => self.objects.get(&o).copied(),
            _ => None,
        }
    }

    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
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
            Member::Buffer => match on.and_then(|h| self.kept.get(&h)) {
                Some(k) => Value::Float(k.buffer),
                None => return Err(HostFault::Nothing),
            },
            Member::Id => match (on.and_then(|h| self.kept.get(&h)), at.receiver) {
                (Some(k), _) => Value::Int(k.id),
                (None, Some(Value::Object(o))) => match self.ids.get(&o) {
                    Some(&id) => Value::Int(id),
                    None => return Err(HostFault::Nothing),
                },
                _ => return Err(HostFault::Nothing),
            },
            Member::Waypoints => return Err(HostFault::Nothing),
        })
    }

    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        match self.members.get(&at.member) {
            Some(Member::Waypoints) => {
                let on = at.receiver.and_then(|r| self.handle(r));
                match on.and_then(|h| self.kept.get(&h)) {
                    Some(k) => Ok(k.waypoints.clone()),
                    None => Err(HostFault::Nothing),
                }
            }
            _ => Err(HostFault::Unmodelled),
        }
    }

    fn set(&mut self, at: &Access, v: Value) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        match (m, v) {
            (Member::TimeScale, Value::Float(f)) => self.time_scale = f,
            (Member::LoadLevel, v) => self.load_level = v,
            (Member::Buffer, Value::Float(f)) => match on.and_then(|h| self.kept.get_mut(&h)) {
                Some(k) => k.buffer = f,
                None => return Err(HostFault::Nothing),
            },
            (Member::Id, Value::Int(i)) => match on.and_then(|h| self.kept.get_mut(&h)) {
                Some(k) => k.id = i,
                None => match at.receiver {
                    Some(Value::Object(o)) if self.ids.contains_key(&o) => {
                        self.ids.insert(o, i);
                    }
                    _ => return Err(HostFault::Nothing),
                },
            },
            // The rest are read-only: the write goes nowhere, as the
            // original's member with no setter.
            _ => {}
        }
        Ok(())
    }

    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(HostFault::Unmodelled);
        };
        let on = at.receiver.and_then(|r| self.handle(r));
        match m {
            Member::Buffer => match on.and_then(|h| self.kept.get_mut(&h)) {
                Some(k) => k.buffer = k.buffer_loaded,
                None => return Err(HostFault::Nothing),
            },
            Member::LoadLevel => self.load_level = Value::Null,
            _ => {}
        }
        Ok(())
    }

    fn op(&mut self, call: &OpCall, spawn: &mut Spawner<'_>) -> Result<Answer, HostFault> {
        let first = call.operands.first().and_then(|(_, v)| *v);
        let target = first.and_then(|v| self.handle(v));
        match call.class {
            Class::Control => Ok(match (first, target) {
                (_, Some(h)) if self.kept.get(&h).is_some_and(|k| k.removed) => Answer::Skip,
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
                let h = spawn(template);
                let from = self
                    .handle(template)
                    .and_then(|t| self.kept.get(&t))
                    .cloned();
                let id = from.as_ref().map_or_else(
                    || match template {
                        Value::Object(o) => self.ids.get(&o).copied().unwrap_or(0),
                        _ => 0,
                    },
                    |k| k.id,
                );
                self.kept.insert(
                    h,
                    Kept {
                        obj: None,
                        id,
                        buffer_loaded: from.as_ref().map_or(0.0, |k| k.buffer_loaded),
                        buffer: from.as_ref().map_or(0.0, |k| k.buffer),
                        waypoints: Vec::new(),
                        removed: false,
                    },
                );
                let e = Value::Entity(h);
                Ok(Answer::Run {
                    receiver: Some(e),
                    result: Some(e),
                })
            }
            Class::Remove => {
                if let Some(k) = target.and_then(|h| self.kept.get_mut(&h)) {
                    k.removed = true;
                }
                Ok(Answer::Done)
            }
            Class::Reset => {
                if let Some(k) = target.and_then(|h| self.kept.get_mut(&h)) {
                    k.buffer = k.buffer_loaded;
                }
                Ok(Answer::Done)
            }
            Class::StopSequence => Ok(Answer::Done),
            _ => Err(HostFault::Unmodelled),
        }
    }
}

/// [`Host`] over a [`ScriptWorld`], the session's generator and a spawner.
pub struct WorldHost<'a, 's> {
    pub world: &'a mut ScriptWorld,
    pub generator: &'a mut Generator,
    pub spawn: &'a mut Spawner<'s>,
}

impl Host for WorldHost<'_, '_> {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.world.get(at)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.world.set(at, value)
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.world.reset(at)
    }
    fn draw(&mut self) -> u32 {
        self.generator.next_u32()
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        self.world.op(call, self.spawn)
    }
    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        self.world.elements(at)
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
    let mut world_host = WorldHost {
        world: &mut host.world,
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
/// generator is the session's, seeded from the clock.
pub fn install(world: &mut World, program: Arc<Program>, files: &Files, order: &[u16]) {
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
    let vm = init_chain(program, files, &handles, order, config(Mode::Play));
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
                obj: GRUNT,
                name: "Grunt".into(),
                script: Some("C:/x/Grunt.ai".into()),
                activity_state: RUNNING,
                id: 41,
                buffer: 12.5,
                waypoints: vec![NODE],
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
            w.get(&at("system", "hardware platform", None)),
            Ok(Value::Int(PS3))
        );
        assert_eq!(
            w.get(&at("system", "game configuration", None)),
            Ok(Value::Int(GOLDEN_MASTER))
        );
        for name in ["game language", "console language"] {
            assert_eq!(w.get(&at("system", name, None)), Ok(Value::Int(ENGLISH)));
        }
        // Read-only: a write goes nowhere.
        w.set(&at("system", "hardware platform", None), Value::Int(3))
            .unwrap();
        assert_eq!(
            w.get(&at("system", "hardware platform", None)),
            Ok(Value::Int(PS3))
        );
        assert_eq!(
            w.get(&at("system", "pause state", None)),
            Err(HostFault::Unmodelled)
        );
    }

    #[test]
    fn the_time_scale_keeps_a_write_and_a_level_asked_for_is_taken_at_the_steps_end() {
        let (mut w, ..) = world();
        let scale = at("system", "game time scale", None);
        assert_eq!(w.get(&scale), Ok(Value::Float(FULL_SPEED)));
        w.set(&scale, Value::Float(50.0)).unwrap();
        assert_eq!(w.get(&scale), Ok(Value::Float(50.0)));
        let level = at("system", "load level", None);
        assert_eq!(w.get(&level), Ok(Value::Null));
        w.set(&level, Value::Object(SPRITE)).unwrap();
        assert_eq!(w.get(&level), Ok(Value::Object(SPRITE)));
        w.end_step();
        assert_eq!(w.requests, vec![Value::Object(SPRITE)]);
        assert_eq!(w.get(&level), Ok(Value::Null));
        w.end_step();
        assert_eq!(w.requests.len(), 1, "taken once");
    }

    #[test]
    fn a_placements_buffer_id_and_waypoints_are_its_files_by_handle_or_object() {
        let (mut w, grunt, _) = world();
        let buffer = |r| at("placement", "deactivation buffer", Some(r));
        for r in [Value::Entity(grunt), Value::Object(GRUNT)] {
            assert_eq!(w.get(&buffer(r)), Ok(Value::Float(12.5)));
            assert_eq!(
                w.get(&at("abstractplacement", "id", Some(r))),
                Ok(Value::Int(41))
            );
        }
        w.set(&buffer(Value::Entity(grunt)), Value::Float(3.0))
            .unwrap();
        assert_eq!(w.get(&buffer(Value::Object(GRUNT))), Ok(Value::Float(3.0)));
        w.reset(&buffer(Value::Entity(grunt))).unwrap();
        assert_eq!(w.get(&buffer(Value::Entity(grunt))), Ok(Value::Float(12.5)));
        w.set(&buffer(Value::Entity(grunt)), Value::Float(3.0))
            .unwrap();
        assert_eq!(
            w.op(&call(Class::Reset, Some(Value::Entity(grunt))), &mut |_| 0),
            Ok(Answer::Done)
        );
        assert_eq!(w.get(&buffer(Value::Entity(grunt))), Ok(Value::Float(12.5)));
        // A sprite has only its `_ID`; nothing else is a placement.
        let sprite_id = at("spritematrixmeasurement", "id", Some(Value::Object(SPRITE)));
        assert_eq!(w.get(&sprite_id), Ok(Value::Int(77)));
        assert_eq!(
            w.get(&buffer(Value::Object(SPRITE))),
            Err(HostFault::Nothing)
        );
        assert_eq!(
            w.get(&at("abstractplacement", "id", Some(Value::Int(3)))),
            Err(HostFault::Nothing)
        );
        let waypoints = at("actorinfo", "waypoints", Some(Value::Entity(grunt)));
        assert_eq!(w.elements(&waypoints), Ok(vec![Value::Object(NODE)]));
        assert_eq!(w.get(&waypoints), Err(HostFault::Nothing));
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
            got.push(w.get(&at(ns, name, None)).unwrap());
        }
        assert_eq!(got, objects.map(Value::Entity));
        // A tell on one runs its block with it.
        let pad = Value::Entity(objects[0]);
        assert_eq!(
            w.op(&call(Class::Control, Some(pad)), &mut |_| 0),
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
            w.op(&call(Class::Spawn, Some(Value::Object(GRUNT))), &mut spawn),
            Ok(Answer::Run {
                receiver: Some(clone),
                result: Some(clone)
            })
        );
        assert_eq!(made, vec![Value::Object(GRUNT)]);
        assert_eq!(w.clones(), vec![900]);
        assert_eq!(
            w.get(&at("abstractplacement", "id", Some(clone))),
            Ok(Value::Int(41))
        );
        assert_eq!(
            w.get(&at("placement", "deactivation buffer", Some(clone))),
            Ok(Value::Float(12.5))
        );
        let tell = call(Class::Control, Some(clone));
        assert!(matches!(w.op(&tell, &mut |_| 0), Ok(Answer::Run { .. })));
        assert_eq!(
            w.op(&call(Class::Remove, Some(clone)), &mut |_| 0),
            Ok(Answer::Done)
        );
        assert!(w.removed(900));
        assert!(!w.removed(grunt));
        assert_eq!(w.op(&tell, &mut |_| 0), Ok(Answer::Skip));
        assert_eq!(
            w.op(&call(Class::Control, None), &mut |_| 0),
            Ok(Answer::Skip)
        );
        assert_eq!(
            w.op(&call(Class::StopSequence, Some(clone)), &mut |_| 0),
            Ok(Answer::Done)
        );
        assert_eq!(
            w.op(&call(Class::StartSequence, Some(clone)), &mut |_| 0),
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
                obj: GRUNT,
                name: "Grunt".into(),
                script: None,
                activity_state: RUNNING,
                id: 41,
                buffer: 12.5,
                waypoints: Vec::new(),
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
