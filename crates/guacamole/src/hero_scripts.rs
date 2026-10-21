//! The hero's own scripts on the TV (openspec AH.24, AH design decisions 1,
//! 5 and 21; hero-attacks, pad-input's The face buttons attack): Eruptor's
//! attacks from his scripts on the VM, in the level from the spawn.
//!
//! - The run: once the level's script part links, the VM runs AH.17's
//!   listing ([`guacamole::scripts::hero_config`]): the init chain, the UI's
//!   and the movement update's ticks, and a fighter's scripts once he joins.
//!   Nothing else on the level runs (no creature, no breakable): what his
//!   scripts wake past that stays inert (decision 6).
//! - The join: the hero placed on the TV is a fighter whose file the part
//!   holds ([`guac_extract::scripts::FIGHTERS`]) and his scripts join by
//!   AH.17's stand-in ([`guacamole::scripts::join_request`], then on the next
//!   step [`guacamole::scripts::join_activate`]), the step the TV enters the
//!   level; from then on his walk, facing and clips are his scripts' and his
//!   step the engine's alone ([`ScriptedHero`]). A character whose scripts
//!   the part doesn't hold walks on AG.16's stand-in and its attack buttons
//!   do nothing (hero-attacks' Which characters attack); a join that fails,
//!   or his script stopped in play, puts him back on it, logged.
//! - A step ([`step_scripts`], before the hero's): his join's next half,
//!   then the walking pad's stick and, once he's in, its attacks
//!   ([`HeroActions`]) read as the first player's pad, the portal manager's
//!   projectile manager (once he's in), every placement's tick, the clones
//!   of scripted templates added. After the hero's step, the engine's frame
//!   ([`advance_scripts`]): the sequences on, what the scripts threw flying
//!   through the seam.
//! - The engine the host reads ([`TvEngine`], decision 21): the hero's
//!   controller by query on his entity, Avian's side of the seam, the
//!   follow camera's view; nothing copied in or out.
//! - What's drawn: his clips the scripts start, layered over his locomotion
//!   ([`crate::play`]), and each clone whose template names a model the
//!   extract's spawn part holds ([`draw_clones`]), at its actor's place,
//!   turn and scale, eased between steps.
//! - His hands: where on his model a thrown ball leaves isn't posed yet
//!   (AH.50), so his attach nodes 7 and 8 (`lHand`, `rHand`) sit at AH.7's
//!   measured launch point in his frame, mirrored ([`hands`]): the stand-in
//!   row "A node's place on the model".
//!
//! Leaving the level installs the run again from the part, the level's
//! start as the next entry finds it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_transform_interpolation::prelude::TransformInterpolation;
use guac_ir::palette::{Palette, SlotEntry};
use guac_motion::hero::Hero;
use guac_motion::seam::PhysicsSeam;
use guac_motion::solid::euler;
use guac_script::{Halt, Handle, Mode, PlacementId, Program, Value};
use guacamole::script_engine::{Engine, Pad};
use guacamole::scripts::{
    self, Files, ScriptClone, ScriptRuntime, ScriptWorld, WorldHost, hero_config,
};
use serde::Serialize;
use tracing::{info, warn};

use crate::hero::{FollowCamera, HeroActions, HeroBody, HeroInput, HeroSystems, ScriptedHero};
use crate::level::{LEVEL_LAYER, Level};
use crate::pad::LevelPads;
use crate::physics::AvianSeam;

/// The level's script part as it linked, kept to install the run again.
#[derive(Resource, Clone)]
pub(crate) struct ScriptSource {
    program: Arc<Program>,
    files: Files,
    order: Vec<u16>,
    /// Each fighter's load file and the unit his file links as.
    fighters: Vec<(String, u16)>,
}

impl ScriptSource {
    /// The unit `load_file`'s scripts link as, if he's a fighter.
    pub(crate) fn fighter(&self, load_file: &str) -> Option<u16> {
        self.fighters
            .iter()
            .find(|(f, _)| f == load_file)
            .map(|(_, u)| *u)
    }
}

/// Installs the run over a linked part (the level's load), keeping the part
/// to install it again when the TV leaves the level.
pub(crate) fn install(
    world: &mut World,
    program: Arc<Program>,
    files: Files,
    order: Vec<u16>,
    fighters: Vec<(String, u16)>,
) {
    let source = ScriptSource {
        program,
        files,
        order,
        fighters,
    };
    install_from(world, &source);
    world.insert_resource(source);
    world.insert_resource(ScriptSteps::default());
}

/// The run from `source`, its entities and an earlier run's gone first.
fn install_from(world: &mut World, source: &ScriptSource) {
    scripts::install_with(
        world,
        source.program.clone(),
        &source.files,
        &source.order,
        hero_config(Mode::Play),
    );
}

/// The run installed again from its part: what leaving the level does.
pub(crate) fn reinstall(world: &mut World) {
    let Some(source) = world.get_resource::<ScriptSource>().cloned() else {
        return;
    };
    install_from(world, &source);
    world.insert_resource(ScriptSteps::default());
    info!("the scripts installed again for the level's next entry");
}

/// Where a hero's join is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Join {
    /// Placed and in: his file's placements join at the next step.
    Asked,
    /// His file's placements in; his minion activates at the next step.
    Requested,
    /// His scripts run as this placement.
    Joined(PlacementId),
    /// His scripts don't run: why. He walks on the stand-in.
    Off(String),
}

impl Join {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Join::Asked => "asked",
            Join::Requested => "requested",
            Join::Joined(_) => "joined",
            Join::Off(_) => "off",
        }
    }
}

/// The hero whose scripts join the run: his load file, his file's unit, his
/// palette (each slot's clip, which his scripts' sequences play and the TV
/// draws) and his join.
#[derive(Component, Debug, Clone)]
pub(crate) struct HeroRun {
    pub(crate) load_file: String,
    pub(crate) unit: u16,
    pub(crate) palette: Option<Arc<Palette>>,
    pub(crate) join: Join,
}

/// Asks for `hero`'s scripts to join the run, if the part holds his file;
/// true when they will (he then walks on them, [`ScriptedHero`]).
pub(crate) fn ask_join(
    commands: &mut Commands,
    hero: Entity,
    load_file: &str,
    palette: Option<Arc<Palette>>,
    source: Option<&ScriptSource>,
) -> bool {
    let Some(unit) = source.and_then(|s| s.fighter(load_file)) else {
        info!(
            load_file,
            "his scripts aren't in the level's part: he walks on the stand-in, his attack buttons do nothing"
        );
        return false;
    };
    commands.entity(hero).insert((
        HeroRun {
            load_file: load_file.to_owned(),
            unit,
            palette,
            join: Join::Asked,
        },
        ScriptedHero,
    ));
    info!(load_file, unit, "his scripts join at the next step");
    true
}

/// Where AH.7's first lob left his left hand, his frame (metres, +Z his
/// facing): its first record less its frame's move, from his pivot at the
/// spawn facing 0 (AH.17's derivation). His right hand mirrors it.
pub(crate) const ERUPTOR_LEFT_HAND: Vec3 = Vec3::new(0.6011, 0.7710, 1.4838);

/// His attach nodes where the stand-in puts them, by index (AH.50 poses
/// them off his model instead): Eruptor's `lHand` (7) and `rHand` (8) at
/// AH.7's launch point and its mirror, the rest at his pivot.
pub(crate) fn hands(load_file: &str) -> Option<Vec<Vec3>> {
    (load_file == "009_Eruptor").then(|| {
        let mut nodes = vec![Vec3::ZERO; 9];
        nodes[7] = ERUPTOR_LEFT_HAND;
        nodes[8] = Vec3::new(
            -ERUPTOR_LEFT_HAND.x,
            ERUPTOR_LEFT_HAND.y,
            ERUPTOR_LEFT_HAND.z,
        );
        nodes
    })
}

/// Each slot's clip length at 100 %, 0 where a slot holds no clip.
pub(crate) fn lengths(palette: &Palette) -> Vec<f32> {
    let mut out = vec![0.0; usize::from(palette.slots)];
    for e in &palette.entries {
        if let SlotEntry::Clip { clip, .. } = &e.entry
            && let Some(l) = out.get_mut(usize::from(e.slot))
        {
            *l = clip.duration;
        }
    }
    out
}

/// What the scripts reach that the TV's entities own (decision 21): the
/// heroes' controllers on their entities, Avian's side of the seam, the
/// follow camera's view as the step before left it.
pub(crate) struct TvEngine<'a, 'w, 's> {
    heroes: &'a mut Query<'w, 's, (Entity, &'static mut HeroBody)>,
    seam: &'a dyn PhysicsSeam,
    view: f32,
}

impl Engine for TvEngine<'_, '_, '_> {
    fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
        let e = Entity::try_from_bits(h)?;
        self.heroes
            .get_mut(e)
            .ok()
            .map(|(_, b)| &mut b.into_inner().hero)
    }
    fn heroes(&self) -> Vec<Handle> {
        self.heroes.iter().map(|(e, _)| e.to_bits()).collect()
    }
    fn seam(&self) -> &dyn PhysicsSeam {
        self.seam
    }
    fn view_heading(&self) -> f32 {
        self.view
    }
    fn pad(&self, _: usize) -> Pad {
        Pad::default()
    }
}

/// The steps the run has made since it was installed, for the log.
#[derive(Resource, Debug, Default)]
pub(crate) struct ScriptSteps {
    /// Started sequences and clones already logged.
    sequences: usize,
    clones: HashSet<Handle>,
    gone: HashSet<Handle>,
    /// The clones made while a clip his scripts started once he'd joined
    /// played on him: his attacks' (a throw's ball, an eruption's lava), not
    /// the level's or his join's, which plays his arrival's clip on him
    /// while it spawns his helpers (Eruptor's `009_LavaCollider` draws the
    /// original's invisible `design_inviso.mdl`).
    attacks: HashSet<Handle>,
    /// The hero's clips playing, by index into the started sequences, with
    /// their slots and whether each started once he'd joined: logged as
    /// ended once played out or stopped.
    playing: Vec<(usize, u32, bool)>,
}

/// One step of the run as `/api/_test/tv/scripts` hands it out (AH.24):
/// the attacks the first player's pad held, what the scripts started on the
/// hero, what they spawned and what they took off.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct ScriptStep {
    /// The run's step, from 1.
    pub step: u64,
    pub actions: [bool; 4],
    /// The hero's join: `asked`, `requested`, `joined` or `off`; none
    /// without a hero.
    pub join: Option<&'static str>,
    /// Clips started on the hero this step: palette slot, length (s at
    /// 100 %) and rate (percent).
    pub started: Vec<Started>,
    /// The palette slots of his clips that played out (or were stopped)
    /// over this step's engine frame.
    pub ended: Vec<u32>,
    /// Clones made this step, by their template's name, where.
    pub spawned: Vec<Spawned>,
    /// Clones taken off this step.
    pub gone: Vec<Spawned>,
    /// Scripts play mode stopped this step.
    pub stopped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Started {
    pub slot: u32,
    pub length: f32,
    pub rate: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Spawned {
    pub name: String,
    pub at: [f32; 3],
}

/// Steps the log keeps: over half an hour at 60 a second.
const STEPS_KEPT: usize = 120_000;

/// The run's steps, shared with `/api/_test/tv/scripts`.
#[derive(Resource, Clone, Default)]
pub(crate) struct ScriptLog(pub(crate) Arc<Mutex<std::collections::VecDeque<ScriptStep>>>);

impl ScriptLog {
    fn push(&self, step: ScriptStep) {
        let mut log = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if log.len() == STEPS_KEPT {
            log.pop_front();
        }
        log.push_back(step);
    }

    /// The last step logged; 0 with none.
    pub(crate) fn last(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .back()
            .map_or(0, |s| s.step)
    }

    /// The steps after `since`, oldest first.
    pub(crate) fn since(&self, since: u64) -> Vec<ScriptStep> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|s| s.step > since)
            .cloned()
            .collect()
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ScriptSystems {
    /// Every placement's tick, before the hero's step.
    Step,
    /// The engine's frame and the clones drawn, after it.
    Advance,
}

/// The run on the TV: its step before the hero's, the engine's frame and
/// the clones' draws after.
pub(crate) struct HeroScriptsPlugin;

impl Plugin for HeroScriptsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptLog>()
            .configure_sets(
                FixedUpdate,
                (
                    ScriptSystems::Step.before(HeroSystems::Step),
                    ScriptSystems::Advance
                        .after(HeroSystems::Step)
                        .before(HeroSystems::Gait),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    step_scripts.in_set(ScriptSystems::Step),
                    (advance_scripts, draw_clones)
                        .chain()
                        .in_set(ScriptSystems::Advance),
                )
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>),
            );
    }
}

/// The follow camera's heading as the step before left it; 0 with none.
fn view_heading(cameras: &Query<&FollowCamera>) -> f32 {
    cameras
        .iter()
        .next()
        .map_or(0.0, |f| f.camera.view().heading())
}

/// One step of the run, before the hero's (the module docs).
fn step_scripts(
    mut commands: Commands,
    mut runtime: ResMut<ScriptRuntime>,
    mut world: ResMut<ScriptWorld>,
    source: Option<Res<ScriptSource>>,
    mut actions: ResMut<HeroActions>,
    input: Res<HeroInput>,
    pads: Res<LevelPads>,
    seam: AvianSeam,
    cameras: Query<&FollowCamera>,
    mut heroes: Query<(Entity, &'static mut HeroBody)>,
    mut runs: Query<(Entity, &mut HeroRun)>,
    mut steps: Option<ResMut<ScriptSteps>>,
    log: Res<ScriptLog>,
) {
    let held = actions.take();
    let settled = runs
        .iter()
        .next()
        .is_some_and(|(_, r)| matches!(r.join, Join::Joined(_)));
    let walking = pads.seat().and_then(|s| s.walking()).is_some();
    let view = view_heading(&cameras);
    let run = runs.iter_mut().next();
    let (hero, mut run) = match run {
        Some((e, r)) => (Some(e), Some(r)),
        None => (None, None),
    };
    let ScriptRuntime { vm, generator, .. } = &mut *runtime;
    let mut spawn =
        |template: Value| -> Handle { commands.spawn(ScriptClone(template)).id().to_bits() };
    let mut engine = TvEngine {
        heroes: &mut heroes,
        seam: &seam,
        view,
    };
    let mut stopped: Vec<Halt> = Vec::new();
    let mut off: Option<String> = None;
    // The frame's slides, once before any script this step (openspec
    // AH.19), then the portal manager's clock, the head of its tick.
    {
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        match vm.slide(&mut host) {
            Ok(r) => stopped.extend(r.stopped),
            Err(h) => stopped.push(h),
        }
    }
    if let Err(h) = scripts::portal_manager_clock(vm, &world) {
        stopped.push(h);
    }
    // The minion activates at the step after his file's placements joined,
    // before anything of this step: AH.17's order.
    if let (Some(h), Some(r)) = (hero, run.as_deref_mut())
        && r.join == Join::Requested
    {
        let handle = h.to_bits();
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        match scripts::join_activate(vm, &mut host, handle) {
            Ok(minion) => {
                if let Some(nodes) = hands(&r.load_file) {
                    world.set_nodes(handle, nodes);
                }
                info!(load_file = %r.load_file, ?minion, "his scripts run");
                r.join = Join::Joined(minion);
            }
            Err(h) => off = Some(format!("his minion didn't activate: {h}")),
        }
    }
    let files = source.as_ref().map(|s| &s.files);
    let asked = match (hero, run.as_deref()) {
        (Some(_), Some(r)) if r.join == Join::Asked => Some(r.unit),
        _ => None,
    };
    if let (Some(unit), Some(files)) = (asked, files) {
        let handles = world.handles().clone();
        if let Some(r) = run.as_deref()
            && let Some(palette) = &r.palette
            && let Some(template) = files
                .placed
                .iter()
                .find(|p| p.obj.unit == unit && p.name == r.load_file)
        {
            world.set_palette(template.obj, lengths(palette));
        }
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        match scripts::join_request(vm, &mut host, files, unit, &handles) {
            Ok(added) => {
                info!(
                    unit,
                    placements = added.len(),
                    "his file's placements joined"
                );
                if let Some(r) = run.as_deref_mut() {
                    r.join = Join::Requested;
                }
            }
            Err(h) => off = Some(format!("his file's placements didn't join: {h}")),
        }
    }
    let joined = run.as_deref().and_then(|r| match r.join {
        Join::Joined(m) => Some(m),
        _ => None,
    });
    // The pads after the join, as the headless runs read them. The attack
    // buttons are his scripts' only once they run (hero-attacks' Which
    // characters attack): a hero not joined yet, or whose join failed or
    // whose script stopped, reads none, so nothing of his file the VM still
    // ticks throws for him.
    let held = if joined.is_some() { held } else { [false; 4] };
    world.read_pads([
        Pad {
            stick: input.0.stick,
            actions: held,
            connected: walking,
        },
        Pad::default(),
    ]);
    let mut host = WorldHost {
        world: &mut world,
        engine: &mut engine,
        generator,
        spawn: &mut spawn,
    };
    if joined.is_some()
        && let Err(h) = scripts::manage_projectiles(vm, &mut host)
    {
        stopped.push(h);
    }
    match vm.step(&mut host) {
        Ok(r) => stopped.extend(r.stopped),
        Err(h) => stopped.push(h),
    }
    world.end_step();
    scripts::add_scripted(vm, &mut world);
    runtime.steps += 1;
    for h in &stopped {
        warn!(%h, "a script stopped");
    }
    if let Some(m) = joined
        && let Some(h) = runtime.vm.stopped(m)
    {
        off = Some(format!("his script stopped: {h}"));
    }
    if let (Some(e), Some(r), Some(why)) = (hero, run.as_deref_mut(), off) {
        warn!(load_file = %r.load_file, reason = %why, "he walks on the stand-in now; his attack buttons do nothing");
        r.join = Join::Off(why);
        commands.entity(e).remove::<ScriptedHero>();
    }
    let step = runtime.steps;
    let mut entry = ScriptStep {
        step,
        actions: held,
        join: run.as_deref().map(|r| r.join.name()),
        stopped: stopped.iter().map(|h| h.to_string()).collect(),
        ..ScriptStep::default()
    };
    runtime.stopped.extend(stopped);
    if let Some(steps) = steps.as_deref_mut() {
        record(
            &world,
            hero.map(Entity::to_bits),
            settled,
            steps,
            &mut entry,
        );
    }
    log.push(entry);
}

/// What changed in `world` since the last step logged: the clips started on
/// `hero` (`settled` when he'd joined before this step), the clones made (an
/// attack's while a clip started once he'd joined plays) and the clones
/// taken off.
fn record(
    world: &ScriptWorld,
    hero: Option<Handle>,
    settled: bool,
    steps: &mut ScriptSteps,
    entry: &mut ScriptStep,
) {
    let sequences = world.sequences();
    let from = steps.sequences.min(sequences.len());
    for (i, s) in sequences.iter().enumerate().skip(from) {
        if Some(s.actor) != hero {
            continue;
        }
        if let Some((_, slot)) = world.clip_of(s) {
            entry.started.push(Started {
                slot,
                length: s.length,
                rate: s.rate,
            });
            steps.playing.push((i, slot, settled));
        }
    }
    steps.sequences = sequences.len();
    let named = |h: Handle| -> Spawned {
        let a = world.actor(h);
        Spawned {
            name: a
                .and_then(|a| a.template)
                .and_then(|t| world.template(t))
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            at: a.map_or([0.0; 3], |a| a.position.to_array()),
        }
    };
    for c in world.clones() {
        if steps.clones.insert(c) {
            entry.spawned.push(named(c));
            if steps.playing.iter().any(|&(.., settled)| settled) {
                steps.attacks.insert(c);
            }
        }
        if world.removed(c) && steps.gone.insert(c) {
            entry.gone.push(named(c));
        }
    }
}

/// The engine's frame after the hero's step: the sequences on and what the
/// scripts threw moved through the seam.
fn advance_scripts(
    time: Res<Time>,
    mut world: ResMut<ScriptWorld>,
    seam: AvianSeam,
    cameras: Query<&FollowCamera>,
    mut heroes: Query<(Entity, &'static mut HeroBody)>,
    steps: Option<ResMut<ScriptSteps>>,
    log: Res<ScriptLog>,
) {
    let view = view_heading(&cameras);
    let mut engine = TvEngine {
        heroes: &mut heroes,
        seam: &seam,
        view,
    };
    world.advance(time.delta_secs(), &mut engine);
    // His clips that played out over this frame, onto this step's entry.
    let Some(mut steps) = steps else {
        return;
    };
    let sequences = world.sequences();
    let mut ended = Vec::new();
    steps.playing.retain(|&(i, slot, _)| {
        let over = sequences.get(i).is_none_or(|s| s.done || s.stopped);
        if over {
            ended.push(slot);
        }
        !over
    });
    if !ended.is_empty()
        && let Some(entry) = log.0.lock().unwrap_or_else(|e| e.into_inner()).back_mut()
    {
        entry.ended.extend(ended);
    }
}

/// A clone drawn: its model's parts hang under it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CloneDrawn {
    /// Whether it draws anything (a template with no model, or one the
    /// spawn part doesn't hold, draws nothing).
    pub(crate) draws: bool,
    /// Whether an attack of his made it (spawned while a clip his scripts
    /// started played on him).
    pub(crate) attack: bool,
}

/// Where a clone draws: its actor's place, its turn (AF.9's Euler angles)
/// and its scale.
pub(crate) fn clone_transform(a: &guacamole::script_engine::Actor) -> Transform {
    Transform {
        translation: a.position,
        rotation: euler([a.angles.tilt, a.angles.facing, a.angles.roll]),
        scale: Vec3::from_array(a.scale) / 100.0,
    }
}

/// Each clone the scripts made, drawn from its template's model off the
/// extract's spawn part while it's on: placed each step, eased between
/// steps, hidden once taken off.
fn draw_clones(
    mut commands: Commands,
    world: Res<ScriptWorld>,
    level: Res<Level>,
    steps: Option<Res<ScriptSteps>>,
    mut clones: Query<(
        Entity,
        &ScriptClone,
        Option<&CloneDrawn>,
        Option<&mut Transform>,
    )>,
) {
    let models = level.spawn_models();
    for (entity, _, drawn, transform) in &mut clones {
        let Some(actor) = world.actor(entity.to_bits()) else {
            continue;
        };
        let place = clone_transform(actor);
        match (drawn, transform) {
            (None, _) => {
                let model = actor
                    .template
                    .and_then(|t| world.template(t))
                    .and_then(|p| p.model.as_deref())
                    .and_then(|m| models.and_then(|ms| ms.get(m)));
                // A model whose effects draw nothing (its parts left out)
                // draws nothing here either.
                let draws = model.is_some_and(|parts| !parts.is_empty()) && !actor.removed;
                let attack = steps
                    .as_ref()
                    .is_some_and(|s| s.attacks.contains(&entity.to_bits()));
                commands.entity(entity).insert((
                    place,
                    TransformInterpolation,
                    if draws {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                    CloneDrawn { draws, attack },
                ));
                if let (Some(parts), true) = (model, draws) {
                    for (mesh, material) in parts {
                        commands.spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::IDENTITY,
                            RenderLayers::layer(LEVEL_LAYER),
                            ChildOf(entity),
                        ));
                    }
                }
            }
            (Some(d), Some(mut t)) => {
                if actor.removed {
                    if d.draws {
                        commands
                            .entity(entity)
                            .insert((Visibility::Hidden, CloneDrawn { draws: false, ..*d }));
                        commands.entity(entity).despawn_related::<Children>();
                    }
                } else if *t != place {
                    *t = place;
                }
            }
            (Some(_), None) => {}
        }
    }
}

/// The hero's numbers as his scripts hold them (AH.24's report): his health
/// (`description.creature info.hp`) and full health (`player joined data`'s
/// `hp max`), and the behaviour his minion runs.
pub(crate) fn hero_numbers(vm: &guac_script::Vm, minion: PlacementId) -> HeroNumbers {
    let int = |v: Option<Value>| match v.map(|v| vm.value(v)) {
        Some(Value::Int(i)) => Some(i),
        Some(Value::Float(f)) => Some(f as i32),
        _ => None,
    };
    let description = vm.local(minion, "description");
    let creature = description.and_then(|d| vm.field(d, "creature info"));
    let joined = vm
        .local(minion, "my Minion Description")
        .and_then(|d| vm.field(d, "player joined data"));
    HeroNumbers {
        health: int(creature.and_then(|c| vm.field(c, "hp"))),
        health_max: int(joined.and_then(|j| vm.field(j, "hp max"))),
        behaviour: vm.behaviour(minion).map(str::to_owned),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct HeroNumbers {
    pub health: Option<i32>,
    pub health_max: Option<i32>,
    pub behaviour: Option<String>,
}

/// The clips the scripts play on the hero now, for his pose: each one's
/// palette slot, its time (seconds into it) and its rate (percent), oldest
/// first.
pub(crate) fn playing_on(world: &ScriptWorld, hero: Handle) -> Vec<(u32, f32, f32)> {
    world
        .sequences()
        .iter()
        .filter(|s| s.actor == hero && !s.done && !s.stopped && s.length > 0.0)
        .filter_map(|s| world.clip_of(s).map(|(_, slot)| (slot, s.time, s.rate)))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use guac_formats::script::builder::{
        self as attack_builder, ATTACK_ARRIVAL_SLOT, ATTACK_BALL, ATTACK_HEALTH, ATTACK_HEALTH_MAX,
        ATTACK_HELPER, ATTACK_SECOND_SLOT, ATTACK_THROW_SLOT,
    };
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios;
    use guac_script::{Generator, Vm};
    use guacamole::level_scripts;
    use guacamole::scripts::{headless_handles, headless_objects};

    use super::*;

    pub(crate) const LEVEL: &str = "level_213_tutotial";

    /// AH.24's synthetic script part in an extract at `dir`: the roster
    /// builder's bootstrap entry, AH.24's attack (global, init_setup and the
    /// fighter's) and AH.13's creature as the level's, at the paths the TV
    /// links them from.
    pub(crate) fn write_attack(dir: &Path) {
        let d = guac_formats::roster::builder::disc(&guac_formats::roster::builder::sample());
        let a = attack_builder::attack();
        let (creature, _) = attack_builder::creature();
        let bytes = [
            &d.bootstrap,
            &a.global,
            &a.init_setup,
            &a.fighter,
            &creature,
        ];
        for ((rel, _), bytes) in level_scripts::files(LEVEL).iter().zip(bytes) {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    /// A headless engine: heroes on the parry twin's open ground.
    struct Bench {
        heroes: HashMap<Handle, Hero>,
        world: ParryWorld,
    }

    impl Engine for Bench {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            self.heroes.get_mut(&h)
        }
        fn heroes(&self) -> Vec<Handle> {
            self.heroes.keys().copied().collect()
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.world
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
    }

    const HERO: Handle = 0x7000_0000;

    /// The synthetic part linked as the TV links it, run headless in strict
    /// mode with the TV's listing.
    struct Headless {
        vm: Vm,
        world: ScriptWorld,
        bench: Bench,
        generator: Generator,
        files: Files,
        fighter: u16,
        next: Handle,
    }

    impl Headless {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            write_attack(tmp.path());
            let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
            let files = linked.placements;
            let handles = headless_handles(&files);
            let objects = headless_objects(&handles);
            let vm = scripts::init_chain(
                Arc::new(linked.program),
                &files,
                &handles,
                &linked.order,
                hero_config(Mode::Strict),
            );
            let mut world = ScriptWorld::new(&files, &handles, objects);
            // His palette as the TV sets it from his: the throw half a
            // second, the second attack two.
            let template = files
                .placed
                .iter()
                .find(|p| p.name == "009_Eruptor")
                .unwrap()
                .obj;
            let mut lengths = vec![0.0; 40];
            lengths[ATTACK_ARRIVAL_SLOT as usize] = 0.1;
            lengths[ATTACK_THROW_SLOT as usize] = 0.5;
            lengths[ATTACK_SECOND_SLOT as usize] = 2.0;
            world.set_palette(template, lengths);
            let ground = scenarios::bodies(&scenarios::open_ground(), &[]);
            let parry = ParryWorld::new(&ground).unwrap();
            let hero = scenarios::eruptor(&parry, Vec3::new(0.0, 1.0, 0.0), 0.0);
            let fighter = linked
                .fighters
                .iter()
                .find(|(f, _)| f == "009_Eruptor")
                .unwrap()
                .1;
            Self {
                vm,
                world,
                bench: Bench {
                    heroes: HashMap::from([(HERO, hero)]),
                    world: parry,
                },
                generator: Generator::seeded(5489),
                files,
                fighter,
                next: objects[6].max(HERO) + 1,
            }
        }

        fn host<T>(&mut self, f: impl FnOnce(&mut Vm, &mut WorldHost<'_, '_>) -> T) -> T {
            let mut spawn = scripts::headless_spawner(self.next);
            let t = {
                let mut host = WorldHost {
                    world: &mut self.world,
                    engine: &mut self.bench,
                    generator: &mut self.generator,
                    spawn: &mut spawn,
                };
                f(&mut self.vm, &mut host)
            };
            self.next = spawn(Value::Null);
            t
        }

        /// One step as the TV's: the pads, the projectiles, every tick, the
        /// clones added, the hero's engine step, the engine's frame.
        fn step(&mut self, actions: [bool; 4]) {
            self.world.read_pads([
                Pad {
                    actions,
                    connected: true,
                    ..Pad::default()
                },
                Pad::default(),
            ]);
            let r = self.host(|vm, host| vm.step(host));
            self.world.end_step();
            scripts::add_scripted(&mut self.vm, &mut self.world);
            r.unwrap_or_else(|h| panic!("{h}"));
            let dt = 1.0 / 60.0;
            for hero in self.bench.heroes.values_mut() {
                hero.step_engine(&self.bench.world, dt, &[], &Default::default());
            }
            self.world.advance(dt, &mut self.bench);
        }

        fn join(&mut self) -> PlacementId {
            let files = self.files.clone();
            let unit = self.fighter;
            let handles = self.world.handles().clone();
            self.host(|vm, host| scripts::join_request(vm, host, &files, unit, &handles))
                .unwrap_or_else(|h| panic!("{h}"));
            self.step([false; 4]);
            self.host(|vm, host| scripts::join_activate(vm, host, HERO))
                .unwrap_or_else(|h| panic!("{h}"))
        }
    }

    /// The synthetic attack the e2e suite throws with runs through the
    /// TV's join and listing in strict mode: his minion joins with his
    /// health and full health set, starts slot 6 on him and spawns his
    /// helper, unmoved; a press
    /// of his first attack starts
    /// slot 20 on him and spawns the ball over him, which flies up and on
    /// at 40 ft/s, falls by its 200 ft/s² onto the ground and is gone the
    /// step after it touches; his second attack starts slot 32.
    #[test]
    fn the_synthetic_attack_throws_through_the_join() {
        let mut run = Headless::new();
        run.step([false; 4]);
        run.step([false; 4]);
        let minion = run.join();
        for _ in 0..3 {
            run.step([false; 4]);
        }
        let numbers = hero_numbers(&run.vm, minion);
        assert_eq!(
            (numbers.health, numbers.health_max),
            (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX))
        );
        assert_eq!(numbers.behaviour.as_deref(), Some("default"));
        let pivot = run.bench.heroes[&HERO].pivot;
        let helpers: Vec<_> = run
            .world
            .clones()
            .into_iter()
            .filter(|&c| c != HERO)
            .filter_map(|c| run.world.actor(c))
            .map(|a| {
                let name = a.template.and_then(|t| run.world.template(t));
                (name.map(|p| p.name.clone()), a.position, a.launched)
            })
            .collect();
        assert_eq!(
            helpers,
            [(Some(ATTACK_HELPER.to_owned()), pivot, false)],
            "his helper at his pivot"
        );
        // Held a step: one throw.
        let before = run.world.clones().len();
        run.step([true, false, false, false]);
        let slots: Vec<u32> = run
            .world
            .sequences()
            .iter()
            .filter(|q| q.actor == HERO)
            .filter_map(|q| run.world.clip_of(q).map(|(_, s)| s))
            .collect();
        assert_eq!(
            slots,
            [ATTACK_ARRIVAL_SLOT as u32, ATTACK_THROW_SLOT as u32],
            "his arrival's slot, then his throw's"
        );
        let new: Vec<Handle> = run.world.clones().into_iter().skip(before).collect();
        assert_eq!(new.len(), 1, "one ball");
        let ball = new[0];
        let a = run.world.actor(ball).unwrap();
        let name = a
            .template
            .and_then(|t| run.world.template(t))
            .map(|p| p.name.clone());
        assert_eq!(name.as_deref(), Some(ATTACK_BALL));
        assert!(a.launched, "sent flying");
        let up = (a.position.y - pivot.y) / guacamole::script_engine::METRES_PER_FOOT;
        assert!(up > 4.0 && up < 6.0, "{up} ft over his pivot");
        // Holding on throws no more; it lands and is gone.
        let mut gone = None;
        for k in 0..120 {
            run.step([true, false, false, false]);
            if run.world.removed(ball) {
                gone = Some(k);
                break;
            }
        }
        let a = run.world.actor(ball).unwrap();
        assert!(gone.is_some(), "it never landed: {:?}", a.position);
        let across = Vec3::new(a.position.x - pivot.x, 0.0, a.position.z - pivot.z).length();
        assert!(across > 1.0, "{across} m ahead");
        assert_eq!(
            run.world.clones().len(),
            before + 1,
            "held, no second throw"
        );
        // The second attack's press: its slot.
        run.step([false; 4]);
        run.step([false, true, false, false]);
        let last = run.world.sequences().last().unwrap();
        assert_eq!(
            run.world.clip_of(last).map(|(_, s)| s),
            Some(ATTACK_SECOND_SLOT as u32)
        );
    }
}
