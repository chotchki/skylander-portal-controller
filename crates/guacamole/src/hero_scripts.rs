//! The hero's own scripts on the TV (openspec AH.24, AH design decisions 1,
//! 5 and 21; hero-attacks, pad-input's The face buttons attack): Eruptor's
//! attacks from his scripts on the VM, in the level from the spawn.
//!
//! - The run: once the level's script part links, the VM runs the TV's
//!   listing ([`guacamole::scripts::smash_config`], openspec AH.27): the init
//!   chain, the UI's and the movement update's ticks, a fighter's scripts
//!   once he joins, and AH.20's pickups and AH.21's breakables (the loot
//!   system's init, the spinning coins in reach, the archetype setups, the
//!   destructibles system through the player loop). No creature runs (the
//!   first fight is AH.25's): what the scripts wake past that stays inert
//!   (decision 6). The level's placements take their palettes off the
//!   level's palettes part, as the headless runs do.
//! - The activity rule ([`step_scripts`], AG.7's rule): each step, each
//!   managed placement woken or put to sleep by the hero's pivot, by the
//!   level's start before he's in (as the props stand), so a breakable out
//!   of its range has no ground and sleeps, as AH.21 runs it.
//! - The join: the hero placed on the TV is a fighter whose file the part
//!   holds ([`guac_extract::scripts::FIGHTERS`]) and his scripts join by
//!   AH.17's stand-in ([`guacamole::scripts::join_request`], then on the next
//!   step [`guacamole::scripts::join_activate`]), the step the TV enters the
//!   level; from then on his walk, facing and clips are his scripts' and his
//!   step the engine's alone ([`ScriptedHero`]). A character whose scripts
//!   the part doesn't hold walks on AG.16's stand-in and its attack buttons
//!   do nothing (hero-attacks' Which characters attack); a join that fails,
//!   or his script stopped in play, puts him back on it, logged.
//! - A step ([`step_scripts`], [`Frame::Scripts`]): the frame's slides,
//!   his join's next half, then the walking pad's stick and, once he's in,
//!   its attacks ([`HeroActions`]) read as the first player's pad, then the
//!   walk in the original's order ([`guacamole::frame`], openspec AH.59): his
//!   minion first, every awake placement as it woke with the player loop's
//!   slices in their placements' places, the mover update last; the clones
//!   of scripted templates made outside it added. After the hero's step,
//!   the engine's frame ([`advance_scripts`], [`Frame::Actors`]): the
//!   sequences on, what the scripts threw flying through the seam.
//! - The engine the host reads ([`TvEngine`], decision 21): the hero's
//!   controller by query on his entity, Avian's side of the seam, the
//!   follow camera's view, each standing prop's body as its placement (a
//!   ball stopping on a barrel touches the barrel, not the level); nothing
//!   copied in or out.
//! - What's drawn: his clips the scripts start, layered over his locomotion
//!   ([`crate::play`]), and each clone whose template names a model the
//!   extract's spawn part holds ([`draw_clones`]), at its actor's place,
//!   turn and scale, eased between steps.
//! - His hands (AH.50): after each step's locomotion, his attach nodes are
//!   posed off his model's attach list by the clips on him, his
//!   locomotion's with his scripts' laid over them ([`pose_hands`],
//!   [`guacamole::hands`]), and what hangs from them set there: the
//!   collider his throw hangs on `lHand` or `rHand`, which the ball leaves
//!   from, as the original's engine poses it.
//!
//! Leaving the level installs the run again from the part, the level's
//! start as the next entry finds it. The hero coming off the portal
//! mid-level (AG.29, [`crate::play`]) installs it again too, his minion and
//! what his scripts spawned going with him, but with the level kept as he
//! left it ([`reinstall_keeping_level`], AH.27): what the level's scripts
//! took off (a breakable broken, a spinning coin taken) stays off and the
//! loot counts stand where they stood; the hero who comes in where he
//! stood joins that run as the first did, once it has made [`INIT_STEPS`].
//! That stands in for the original's own swap (the portal manager
//! deactivating his minion, the level's scripts running on): a coin or
//! debris in flight at the swap goes, and a creature's state would start
//! over, which AH.25's fight needs the deactivate path for
//! (`openspec/backlog.md`).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_transform_interpolation::prelude::TransformInterpolation;
use guac_ir::Model;
use guac_ir::nodes::AttachNodes;
use guac_ir::palette::Palette;
use guac_motion::hero::Hero;
use guac_motion::seam::{BodyId, PhysicsSeam};
use guac_motion::solid::euler;
use guac_script::{Halt, Handle, Mode, Obj, PlacementId, Program, Value};
use guacamole::figure::{self, Layout};
use guacamole::frame::Frame;
use guacamole::script_engine::{Engine, Pad};
use guacamole::scripts::{
    self, Files, LevelState, ScriptClone, ScriptRuntime, ScriptWorld, WorldHost, smash_config,
};
use serde::Serialize;
use tracing::{info, warn};

use crate::hero::{FollowCamera, HeroActions, HeroBody, HeroGaits, HeroInput, ScriptedHero};
use crate::level::{LEVEL_LAYER, Level};
use crate::pad::LevelPads;
use crate::physics::AvianSeam;
use crate::play::{self, HeroClips};
use crate::progress::Earning;
use skylander_core::SlotIndex;

/// The level's script part as it linked, kept to install the run again.
#[derive(Resource, Clone)]
pub(crate) struct ScriptSource {
    program: Arc<Program>,
    files: Files,
    order: Vec<u16>,
    /// Each fighter's load file and the unit his file links as.
    fighters: Vec<(String, u16)>,
    /// Where a figure's values sit in its tag's words, from the extract's
    /// progression tables (AH.23); none when the extract has no rows to lay
    /// them out by, and then no figure reaches his scripts.
    layout: Option<Layout>,
    /// The level's placements' palettes by model (openspec AH.27).
    palettes: Vec<(String, Vec<f32>)>,
}

impl ScriptSource {
    /// The unit the level's own file links as: the last whose placements
    /// run from the level's start.
    pub(crate) fn level_unit(&self) -> Option<u16> {
        self.order.last().copied()
    }

    /// The unit `load_file`'s scripts link as, if he's a fighter.
    pub(crate) fn fighter(&self, load_file: &str) -> Option<u16> {
        self.fighters
            .iter()
            .find(|(f, _)| f == load_file)
            .map(|(_, u)| *u)
    }
}

#[cfg(test)]
impl ScriptSource {
    /// A part with `files` linked as `program`, the level's unit last of
    /// `order`, no fighters.
    pub(crate) fn of(program: Arc<Program>, files: Files, order: Vec<u16>) -> Self {
        Self {
            program,
            files,
            order,
            fighters: Vec::new(),
            layout: None,
            palettes: Vec::new(),
        }
    }
}

/// A level's script part as the level's load hands it to the TV.
pub(crate) struct Part {
    pub(crate) program: Arc<Program>,
    pub(crate) files: Files,
    pub(crate) order: Vec<u16>,
    pub(crate) fighters: Vec<(String, u16)>,
    pub(crate) layout: Option<Layout>,
    pub(crate) palettes: Vec<(String, Vec<f32>)>,
}

/// Installs the run over a linked part (the level's load), keeping the part
/// to install it again when the TV leaves the level.
pub(crate) fn install(world: &mut World, part: Part) {
    let source = ScriptSource {
        program: part.program,
        files: part.files,
        order: part.order,
        fighters: part.fighters,
        layout: part.layout,
        palettes: part.palettes,
    };
    install_from(world, &source);
    world.insert_resource(source);
    world.insert_resource(ScriptSteps::default());
    world.insert_resource(Carried::default());
}

/// The run from `source`, its entities and an earlier run's gone first: the
/// TV's listing ([`scripts::smash_config`], openspec AH.27: Eruptor's
/// scripts, the pickups and the breakables), each of the level's placements
/// with a model the level's palettes part holds given its palette.
fn install_from(world: &mut World, source: &ScriptSource) {
    scripts::install_with(
        world,
        source.program.clone(),
        &source.files,
        &source.order,
        smash_config(Mode::Play),
    );
    let Some(level) = source.level_unit() else {
        return;
    };
    let mut script_world = world.resource_mut::<ScriptWorld>();
    for p in source.files.placed.iter().filter(|p| p.obj.unit == level) {
        if let Some(model) = &p.model
            && let Some((_, lengths)) = source.palettes.iter().find(|(owner, _)| owner == model)
        {
            script_world.set_palette(p.obj, lengths.clone());
        }
    }
}

/// The run installed again from its part: what leaving the level does, the
/// level's next entry a fresh start.
pub(crate) fn reinstall(world: &mut World) {
    let Some(source) = world.get_resource::<ScriptSource>().cloned() else {
        return;
    };
    install_from(world, &source);
    world.insert_resource(ScriptSteps::default());
    world.insert_resource(Carried::default());
    info!("the scripts installed again for the level's next entry");
}

/// The run installed again with the level kept as it stands (openspec
/// AH.27): what the hero's take-off does mid-level (AG.29's swap, the
/// stand-in `openspec/backlog.md` names). His minion and what his scripts
/// spawned go with him; the level's placements the scripts took off (each
/// breakable broken, each spinning coin taken) stay off and each kind's loot
/// count stays where it stood ([`LevelState`]), so the next hero walks into
/// the level as it was left, not its start.
pub(crate) fn reinstall_keeping_level(world: &mut World) {
    let Some(source) = world.get_resource::<ScriptSource>().cloned() else {
        return;
    };
    let carried = world.remove_resource::<Carried>().unwrap_or_default();
    let now = match (
        world.get_resource::<ScriptRuntime>(),
        world.get_resource::<ScriptWorld>(),
        source.level_unit(),
    ) {
        (Some(rt), Some(w), Some(level)) => LevelState::read(&rt.vm, w, level),
        _ => LevelState::default(),
    };
    // What play took off: in this run since its hero joined (none if he
    // never did), and what the runs before it carried.
    let mut played = carried.played;
    if let Some(at_join) = &carried.at_join {
        played.extend(now.since(at_join).taken_off);
    }
    played.sort_unstable_by_key(|o| (o.unit, o.index));
    played.dedup();
    // Counts the last take-off carried that no step has set back yet (the
    // portal held the level since) go on as they were, not the fresh run's
    // draw.
    let loot = match &carried.loot {
        Some(waiting) => waiting.loot.clone(),
        None => now.loot,
    };
    let kept = LevelState {
        taken_off: played.clone(),
        loot,
    };
    install_from(world, &source);
    world.insert_resource(ScriptSteps::default());
    world.resource_scope(|world, mut rt: Mut<ScriptRuntime>| {
        let mut w = world.resource_mut::<ScriptWorld>();
        kept.put_back(&mut rt.vm, &mut w);
    });
    info!(
        taken_off = kept.taken_off.len(),
        loot = ?kept.loot,
        "the scripts installed again with the level as he left it"
    );
    world.insert_resource(Carried {
        at_join: None,
        played,
        loot: Some(kept),
    });
}

/// The level a run carries across the hero's take-offs (openspec AH.27).
#[derive(Resource, Debug, Default)]
pub(crate) struct Carried {
    /// What the level had taken off when this run's hero joined: its
    /// start's own (the setups that take themselves off) and what the runs
    /// before carried. None until he joins.
    pub(crate) at_join: Option<Vec<Obj>>,
    /// What play took off in the runs before this one, put back into it.
    pub(crate) played: Vec<Obj>,
    /// The counts to set back once this run's loot data is made (its first
    /// step).
    pub(crate) loot: Option<LevelState>,
}

/// The steps a run makes before a hero's file joins it: the init chain's
/// two, as AH.17's headless runs make them (the portal's loaded objects are
/// set by then). The run on the portal view has made them long before the
/// first hero comes in; a run his take-off installed again (AG.29) has
/// made none when the next one does, the level held meanwhile.
pub(crate) const INIT_STEPS: u64 = 2;

/// Where a hero's join is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Join {
    /// Placed and in: his file's placements join at the next step once the
    /// run has made [`INIT_STEPS`].
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
/// draws), what poses his attach nodes and his join.
#[derive(Component, Debug, Clone)]
pub(crate) struct HeroRun {
    pub(crate) load_file: String,
    pub(crate) unit: u16,
    pub(crate) palette: Option<Arc<Palette>>,
    /// None when his attach nodes didn't read: every node at his pivot.
    pub(crate) rig: Option<HandRig>,
    pub(crate) join: Join,
    /// The portal slot his figure stands in: its values make his tag's words.
    pub(crate) slot: SlotIndex,
    /// His figure's tag data once his scripts join holding it (AH.23), and
    /// the generation its words were made under.
    pub(crate) figure: Option<(Value, i64)>,
}

/// What poses his attach nodes (AH.50): his model (its skeleton and idle),
/// his locomotion's other clips, his model's attach list and his scale.
#[derive(Debug, Clone)]
pub(crate) struct HandRig {
    pub(crate) model: Arc<Model>,
    pub(crate) clips: Arc<HeroClips>,
    pub(crate) nodes: Arc<AttachNodes>,
    pub(crate) scale: f32,
}

/// Asks for `hero`'s scripts to join the run, if the part holds his file;
/// true when they will (he then walks on them, [`ScriptedHero`]).
pub(crate) fn ask_join(
    commands: &mut Commands,
    hero: Entity,
    load_file: &str,
    palette: Option<Arc<Palette>>,
    rig: Option<HandRig>,
    source: Option<&ScriptSource>,
    slot: SlotIndex,
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
            rig,
            join: Join::Asked,
            slot,
            figure: None,
        },
        ScriptedHero,
    ));
    info!(load_file, unit, "his scripts join at the next step");
    true
}

/// Each slot's clip length at 100 %, 0 where a slot holds no clip.
pub(crate) fn lengths(palette: &Palette) -> Vec<f32> {
    guacamole::level_scripts::lengths(palette)
}

/// What the scripts reach that the TV's entities own (decision 21): the
/// heroes' controllers on their entities, Avian's side of the seam, the
/// follow camera's view as the step before left it, and which body in the
/// seam each standing prop is (openspec AH.27: a breakable's solid is its
/// placement to the scripts, as the headless runs map it).
pub(crate) struct TvEngine<'a, 'w, 's> {
    heroes: &'a mut Query<'w, 's, (Entity, &'static mut HeroBody)>,
    seam: &'a dyn PhysicsSeam,
    view: f32,
    props: &'a [(Handle, BodyId)],
}

impl<'a, 'w, 's> TvEngine<'a, 'w, 's> {
    /// Over the heroes' controllers, Avian's side of the seam, the view's
    /// heading and the standing props ([`standing_props`]).
    pub(crate) fn new(
        heroes: &'a mut Query<'w, 's, (Entity, &'static mut HeroBody)>,
        seam: &'a dyn PhysicsSeam,
        view: f32,
        props: &'a [(Handle, BodyId)],
    ) -> Self {
        Self {
            heroes,
            seam,
            view,
            props,
        }
    }
}

/// Each prop standing with its solid in Avian's world, as its placement's
/// handle and its body: the level's file's placements by their object.
pub(crate) fn standing_props(
    level: &Level,
    world: &ScriptWorld,
    source: Option<&ScriptSource>,
) -> Vec<(Handle, BodyId)> {
    let Some(unit) = source.and_then(ScriptSource::level_unit) else {
        return Vec::new();
    };
    level
        .standing_bodies()
        .into_iter()
        .filter_map(|(index, body)| {
            world
                .handles()
                .get(&Obj { unit, index })
                .map(|&h| (h, body))
        })
        .collect()
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
    fn body(&self, actor: Handle) -> Option<BodyId> {
        body_of(self.props, actor)
    }
    fn actor_of(&self, body: BodyId) -> Option<Handle> {
        actor_of(self.props, body)
    }
}

/// The body a standing prop's placement is, in [`standing_props`]' list.
pub(crate) fn body_of(props: &[(Handle, BodyId)], actor: Handle) -> Option<BodyId> {
    props.iter().find(|(h, _)| *h == actor).map(|(_, b)| *b)
}

/// The placement a standing prop's body is, in [`standing_props`]' list.
pub(crate) fn actor_of(props: &[(Handle, BodyId)], body: BodyId) -> Option<Handle> {
    props.iter().find(|(_, b)| *b == body).map(|(h, _)| *h)
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
    /// The log's step, from 1, counting on across the run's installs
    /// (leaving the level, the hero taken off: AG.29).
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
    /// When the step ran, on the frame log's clock (`/api/_test/tv/frames`,
    /// the TV's real time), and how long its scripts took on the frame's
    /// thread (the activity rule, the slides, the player loop, the join and
    /// every placement's tick), both ms (openspec AH.27: a long frame told
    /// apart from a long step).
    pub at_ms: f64,
    pub took_ms: f64,
    /// How long the engine's frame after it took (the sequences, what the
    /// scripts threw moved through the seam), ms.
    pub advance_ms: f64,
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

/// The run on the TV ([`guacamole::frame`]'s sets): its step in
/// [`Frame::Scripts`], the engine's frame and the clones' draws in
/// [`Frame::Actors`] after the hero's, his hands in [`Frame::Hands`].
pub(crate) struct HeroScriptsPlugin;

impl Plugin for HeroScriptsPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<ScriptLog>().add_systems(
            FixedUpdate,
            (
                step_scripts.in_set(Frame::Scripts),
                (advance_scripts, draw_clones).chain().in_set(Frame::Actors),
                pose_hands.in_set(Frame::Hands),
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
    mut earning: Option<ResMut<Earning>>,
    (level, mut carried, real): (Res<Level>, Option<ResMut<Carried>>, Res<Time<Real>>),
) {
    let began = std::time::Instant::now();
    // The activity rule (openspec AH.27, AG.7's rule as the headless runs
    // apply it): each managed placement woken or put to sleep by the
    // heroes' pivots as the step begins, by the level's start before one is
    // in, as the props stand.
    let pivots: Vec<Vec3> = heroes.iter().map(|(_, b)| b.hero.pivot).collect();
    let from: Vec<Vec3> = match pivots.is_empty() {
        true => level.start().into_iter().collect(),
        false => pivots,
    };
    for (h, active) in world.activity_at(&from) {
        runtime.vm.set_asleep(h, !active);
    }
    let props = standing_props(&level, &world, source.as_deref());
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
    let ran = runtime.steps;
    let ScriptRuntime {
        vm,
        generator,
        player_loop,
        ..
    } = &mut *runtime;
    let mut spawn =
        |template: Value| -> Handle { commands.spawn(ScriptClone(template)).id().to_bits() };
    let mut engine = TvEngine::new(&mut heroes, &seam, view, &props);
    let mut stopped: Vec<Halt> = Vec::new();
    let mut off: Option<String> = None;
    // The frame's slides, once before any script this step (openspec
    // AH.19), as the original's scripts' updater begins.
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
                info!(load_file = %r.load_file, ?minion, "his scripts run");
                r.join = Join::Joined(minion);
            }
            Err(h) => off = Some(format!("his minion didn't activate: {h}")),
        }
    }
    let files = source.as_ref().map(|s| &s.files);
    let asked = match (hero, run.as_deref()) {
        (Some(_), Some(r)) if r.join == Join::Asked && ran >= INIT_STEPS => Some(r.unit),
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
        // His figure's tag on the portal first, so his definition's first
        // tick finds it (AH.23).
        if let Some(r) = run.as_deref_mut() {
            let layout = source.as_ref().and_then(|s| s.layout.as_ref());
            let earner = earning
                .as_deref()
                .and_then(|e| e.slots.get(r.slot.as_usize()))
                .and_then(Option::as_ref);
            match (layout, earner) {
                (Some(layout), Some(e)) => {
                    match figure::put_on_portal(vm, &mut host, layout, &e.tag()) {
                        Ok(data) => {
                            info!(
                                slot = r.slot.display(),
                                xp = e.progress.xp,
                                gold = e.progress.gold,
                                "his figure's tag is on the portal"
                            );
                            r.figure = Some((data, e.progress.generation));
                        }
                        Err(h) => {
                            warn!(%h, "his figure's tag didn't go on the portal: his scripts see none")
                        }
                    }
                }
                (None, _) => {
                    info!("the extract lays out no figure's words: his scripts see no figure")
                }
                (_, None) => warn!(
                    slot = r.slot.display(),
                    "no figure stands in his slot: his scripts see none"
                ),
            }
        }
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
    // The walk in the original's order (openspec AH.59, [`guacamole::frame`]):
    // his minion first, then the awake placements as they woke, the player
    // loop's slices (AH.20) in their placements' places, its stretches past
    // the clock once he'd joined before this step, the mover update last.
    let mut host = WorldHost {
        world: &mut world,
        engine: &mut engine,
        generator,
        spawn: &mut spawn,
    };
    match player_loop.walk(vm, &mut host, settled, &mut stopped) {
        Ok(r) => stopped.extend(r.stopped),
        Err(h) => stopped.push(h),
    }
    world.end_step();
    scripts::add_scripted(vm, &mut world);
    // What his figure's words hold after the step, onto his slot's values
    // under the generation they were made from (AH.23).
    if let (Some(r), Some(layout), Some(earning)) = (
        run.as_deref(),
        source.as_ref().and_then(|s| s.layout.as_ref()),
        earning.as_deref_mut(),
    ) && let Some((data, generation)) = r.figure
        && let Some(values) = figure::read(vm, layout, data)
    {
        earning.earned(r.slot.as_usize(), values, generation);
    }
    runtime.steps += 1;
    // A run installed again with the level kept: its loot counts set back
    // once its loot data is made; and what the level had taken off when
    // its hero joined, which play's own is told from at his take-off.
    if let Some(c) = carried.as_deref_mut() {
        if let Some(state) = &c.loot
            && state.set_loot(&mut runtime.vm, &world)
        {
            info!(loot = ?state.loot, "the loot counts set back where they stood");
            c.loot = None;
        }
        if c.at_join.is_none()
            && joined.is_some()
            && let Some(level) = source.as_ref().and_then(|s| s.level_unit())
        {
            c.at_join = Some(LevelState::read(&runtime.vm, &world, level).taken_off);
        }
    }
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
    // The log's own count, on across the run's installs: a run installed
    // again mid-level (AG.29's take-off) starts its steps from 1, and a
    // reader's `since` would skip them.
    let step = log.last() + 1;
    let mut entry = ScriptStep {
        step,
        actions: held,
        join: run.as_deref().map(|r| r.join.name()),
        stopped: stopped.iter().map(|h| h.to_string()).collect(),
        at_ms: real.elapsed().as_secs_f64() * 1e3,
        took_ms: began.elapsed().as_secs_f64() * 1e3,
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
    (level, source): (Res<Level>, Option<Res<ScriptSource>>),
) {
    let began = std::time::Instant::now();
    let view = view_heading(&cameras);
    let props = standing_props(&level, &world, source.as_deref());
    let mut engine = TvEngine::new(&mut heroes, &seam, view, &props);
    world.advance(time.delta_secs(), &mut engine);
    let took = began.elapsed().as_secs_f64() * 1e3;
    if let Some(entry) = log.0.lock().unwrap_or_else(|e| e.into_inner()).back_mut() {
        entry.advance_ms = took;
    }
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

/// His attach nodes posed by the clips on him as this step left them, his
/// locomotion's (his idle alone with no gaits) with his scripts' laid over,
/// and what hangs from them set there (the module docs; AH.50).
fn pose_hands(
    mut world: ResMut<ScriptWorld>,
    seam: AvianSeam,
    cameras: Query<&FollowCamera>,
    mut heroes: Query<(Entity, &'static mut HeroBody)>,
    runs: Query<(Entity, &HeroRun, Option<&HeroGaits>)>,
    (level, source): (Res<Level>, Option<Res<ScriptSource>>),
) {
    let mut posed = false;
    for (e, run, gaits) in &runs {
        let Some(rig) = &run.rig else {
            continue;
        };
        let base = match gaits {
            Some(g) => play::locomotion_clips(&rig.model, &rig.clips, g),
            None => vec![(&rig.model.idle, 0.0, 1.0)],
        };
        let over = match &run.palette {
            Some(p) => guacamole::hands::playing(&world, e.to_bits(), p),
            None => Vec::new(),
        };
        let nodes =
            guacamole::hands::posed(&rig.model.skeleton, &rig.nodes, &base, &over, rig.scale);
        world.set_nodes(e.to_bits(), nodes);
        posed = true;
    }
    if !posed {
        return;
    }
    let view = view_heading(&cameras);
    let props = standing_props(&level, &world, source.as_deref());
    let mut engine = TvEngine::new(&mut heroes, &seam, view, &props);
    world.place_hung(&mut engine);
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
        let (creature, _) = attack_builder::creature();
        write_parts(dir, &creature);
    }

    /// AH.27's beside it: AH.24's attack with AH.27's breakable as the
    /// level's entry, the barrel at `at`; the barrel's object index.
    pub(crate) fn write_smash(dir: &Path, at: [f32; 3]) -> u32 {
        let s = attack_builder::smash(at);
        write_parts(dir, &s.level);
        s.barrel
    }

    /// The roster builder's bootstrap entry, AH.24's attack (global,
    /// init_setup and the fighter's) and `level` as the level's.
    fn write_parts(dir: &Path, level: &[u8]) {
        let d = guac_formats::roster::builder::disc(&guac_formats::roster::builder::sample());
        let a = attack_builder::attack();
        let bytes: [&[u8]; 5] = [&d.bootstrap, &a.global, &a.init_setup, &a.fighter, level];
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
        /// The level's file's unit.
        level: u16,
        next: Handle,
    }

    impl Headless {
        fn new() -> Self {
            Self::build(None)
        }

        /// AH.27's: the barrel at `at`, on the TV's listing.
        fn smash(at: [f32; 3]) -> Self {
            Self::build(Some(at))
        }

        fn build(barrel: Option<[f32; 3]>) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let config = match barrel {
                None => {
                    write_attack(tmp.path());
                    scripts::hero_config(Mode::Strict)
                }
                Some(at) => {
                    write_smash(tmp.path(), at);
                    scripts::smash_config(Mode::Strict)
                }
            };
            let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
            let files = linked.placements;
            let handles = headless_handles(&files);
            let objects = headless_objects(&handles);
            let vm = scripts::init_chain(
                Arc::new(linked.program),
                &files,
                &handles,
                &linked.order,
                config,
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
                level: *linked.order.last().unwrap(),
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

        /// One step as the TV's: the activity rule, the pads, every tick,
        /// the clones added, the hero's engine step, the engine's frame.
        fn step(&mut self, actions: [bool; 4]) {
            for (h, active) in self.world.activity(&mut self.bench) {
                self.vm.set_asleep(h, !active);
            }
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

    /// AH.23 headless on the synthetic fight: his figure's tag on the
    /// portal before his join, in strict mode on the TV's listing, holding
    /// 999 experience and 4,321 gold; his second attack's award (a point,
    /// through `Game - Award Experience`, as an orb pays) lands in its words
    /// as the scripts wrote it, the thousandth point.
    #[test]
    fn the_synthetic_fights_award_lands_in_his_figures_words() {
        use guacamole::figure::{Layout, Tag, Values};
        let mut run = Headless::new();
        run.step([false; 4]);
        run.step([false; 4]);
        let layout = Layout::of(&progression_rows()).unwrap();
        let tag = Tag {
            toy_id: 9,
            subtype: 0,
            serial: 1,
            values: Values {
                xp: 999,
                gold: 4_321,
                playtime_secs: 60,
            },
        };
        let data = run
            .host(|vm, host| figure::put_on_portal(vm, host, &layout, &tag))
            .unwrap_or_else(|h| panic!("{h}"));
        assert_eq!(figure::read(&run.vm, &layout, data), Some(tag.values));
        // Sized by the part's `Save - Set up Tag Data Fields`, a word for
        // every four bytes of the portal's regions: AH.11's live 44.
        let area = run.vm.field(data, "save data area").unwrap();
        assert_eq!(run.vm.set(area).map(<[Value]>::len), Some(44));
        // The slot `Portal - Enter Object` marks entered, as its links.
        let entered = |vm: &mut guac_script::Vm| {
            let g = vm.program().global_named("portal control data").unwrap();
            let pcd = vm.global(g).unwrap();
            let slot = vm
                .field(pcd, "loaded objects")
                .and_then(|s| vm.set(s).and_then(|e| e.first().copied()))
                .unwrap();
            vm.field(slot, "entered").map(|v| vm.value(v))
        };
        assert_eq!(entered(&mut run.vm), Some(Value::Int(0)), "not before");
        run.join();
        assert_eq!(entered(&mut run.vm), Some(Value::Int(1)), "at his join");
        run.step([false; 4]);
        run.step([false, true, false, false]);
        let after = figure::read(&run.vm, &layout, data).unwrap();
        assert_eq!(
            after,
            Values {
                xp: 1_000,
                ..tag.values
            },
            "the award in his words"
        );
    }

    /// Giants' rows as AH.2 reads them, the table ours: what lays a
    /// figure's words out in the synthetic part's tests.
    pub(crate) fn progression_rows() -> guac_ir::progression::Progression {
        use guac_ir::progression::{Progression, SaveField, SaveKind, SaveParam};
        let row = |param: &str, kind, word, bit, width, limit| SaveParam {
            param: param.into(),
            kind,
            fields: vec![SaveField {
                word,
                bit,
                member: None,
            }],
            width,
            bits_per_field: 0,
            limit,
        };
        Progression {
            identity: crate::turntable::tests::identity(),
            level_xp: vec![0, 1_000],
            save_params: vec![
                row("pp_Exp", SaveKind::Plain, 0, 0, 24, 33_000),
                row("pp_Money", SaveKind::Plain, 0, 24, 16, 65_535),
                row("pp_CumulativeTime", SaveKind::Plain, 1, 8, 32, 0),
                row("pp_Exp", SaveKind::Spillover, 28, 24, 16, 63_500),
            ],
            gold_cap: 65_000,
        }
    }

    /// The swap's stand-in on a joined hero (openspec AH.20): the health
    /// he left with set where his scripts hold it, which the TV's report
    /// reads back; kept across his swap, gone at a level change.
    #[test]
    fn a_swapped_heros_health_comes_back_as_he_left_it() {
        let mut run = Headless::new();
        run.step([false; 4]);
        run.step([false; 4]);
        let minion = run.join();
        run.step([false; 4]);
        assert_eq!(scripts::health(&run.vm, minion), Some(ATTACK_HEALTH));
        let mut kept = scripts::KeptHealth::default();
        kept.left("eruptor", 77);
        let back = kept.back("eruptor").unwrap();
        assert!(scripts::set_health(&mut run.vm, minion, back));
        assert_eq!(hero_numbers(&run.vm, minion).health, Some(77));
        run.step([false; 4]);
        assert_eq!(
            scripts::health(&run.vm, minion),
            Some(77),
            "his scripts keep it"
        );
        kept.level_changed();
        assert_eq!(kept.back("eruptor"), None);
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

    // ---- AH.27: the breakables on the TV's listing ------------------------

    /// The TV's install (openspec AH.27): the run on the TV's listing, the
    /// level's placements given their palettes by the model each names
    /// (the far barrel's), none for one that names no model, the run's
    /// listing the breakables' (the setup ticks, not inert).
    #[test]
    fn the_tvs_install_gives_the_levels_placements_their_palettes() {
        use guac_formats::script::builder::{
            SMASH_BARREL, SMASH_FAR, SMASH_FAR_MODEL, SMASH_SETUP,
        };
        let tmp = tempfile::tempdir().unwrap();
        write_smash(tmp.path(), BARREL_AT);
        let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
        let level = *linked.order.last().unwrap();
        let obj = |name: &str| {
            linked
                .placements
                .placed
                .iter()
                .find(|p| p.name == name && p.obj.unit == level)
                .unwrap()
                .obj
        };
        let (far, near, setup) = (obj(SMASH_FAR), obj(SMASH_BARREL), obj(SMASH_SETUP));
        let mut world = World::new();
        install(
            &mut world,
            Part {
                program: Arc::new(linked.program),
                files: linked.placements,
                order: linked.order,
                fighters: linked.fighters,
                layout: None,
                palettes: vec![(SMASH_FAR_MODEL.to_owned(), vec![0.0, 0.5])],
            },
        );
        let w = world.resource::<ScriptWorld>();
        assert_eq!(w.palette(far), Some(&[0.0, 0.5][..]));
        assert_eq!(w.palette(near), None);
        let rt = world.resource::<ScriptRuntime>();
        let h = w.handles()[&setup];
        assert!(!rt.vm.inert().contains(&h), "the setup runs");
    }

    /// A take-off before any step since the last (the portal held the
    /// level throughout) carries what the last one carried: the barrel put
    /// back off, and the counts still waiting to be set back as they were,
    /// not the fresh run's (the synthetic part draws none).
    #[test]
    fn a_take_off_before_any_step_carries_what_the_last_one_did() {
        use guac_formats::script::builder::SMASH_BARREL;
        use guacamole::scripts::Rarity;
        let tmp = tempfile::tempdir().unwrap();
        write_smash(tmp.path(), BARREL_AT);
        let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
        let level = *linked.order.last().unwrap();
        let barrel = linked
            .placements
            .placed
            .iter()
            .find(|p| p.name == SMASH_BARREL && p.obj.unit == level)
            .unwrap()
            .obj;
        let mut world = World::new();
        install(
            &mut world,
            Part {
                program: Arc::new(linked.program),
                files: linked.placements,
                order: linked.order,
                fighters: linked.fighters,
                layout: None,
                palettes: Vec::new(),
            },
        );
        let waiting = LevelState {
            taken_off: vec![barrel],
            loot: vec![(Rarity::Common, 0.25), (Rarity::Uncommon, 0.5)],
        };
        world.insert_resource(Carried {
            at_join: None,
            played: vec![barrel],
            loot: Some(waiting.clone()),
        });
        reinstall_keeping_level(&mut world);
        assert!(world.resource::<ScriptWorld>().taken_off(barrel));
        let carried = world.resource::<Carried>();
        assert_eq!(carried.played, [barrel]);
        assert_eq!(carried.at_join, None);
        assert_eq!(carried.loot.as_ref(), Some(&waiting), "the counts waiting");
    }

    /// Where the synthetic barrel stands: 3 m ahead of the headless hero
    /// along his facing, on the open ground, inside the ball's arc.
    const BARREL_AT: [f32; 3] = [0.0, 0.0, 3.0];

    impl Headless {
        /// A file placement's handle by its name, in the level's file.
        fn placed(&self, name: &str) -> (Obj, Handle) {
            let p = self
                .files
                .placed
                .iter()
                .find(|p| p.name == name && p.obj.unit == self.level)
                .unwrap_or_else(|| panic!("no {name}"));
            (p.obj, self.world.handles()[&p.obj])
        }

        /// The clones made so far whose template is named `name`.
        fn clones_named(&self, name: &str) -> Vec<Handle> {
            self.world
                .clones()
                .into_iter()
                .filter(|&c| self.world.name(c) == name)
                .collect()
        }

        /// The init chain's two steps, his figure's tag holding `values`,
        /// then the join and a step: his figure's tag data.
        fn joined_holding(&mut self, layout: &Layout, values: figure::Values) -> Value {
            self.step([false; 4]);
            self.step([false; 4]);
            let tag = figure::Tag {
                toy_id: 9,
                subtype: 0,
                serial: 1,
                values,
            };
            let data = self
                .host(|vm, host| figure::put_on_portal(vm, host, layout, &tag))
                .unwrap_or_else(|h| panic!("{h}"));
            self.join();
            self.step([false; 4]);
            data
        }
    }

    /// AH.27 headless on the synthetic breakable, in strict mode on the
    /// TV's listing: a press throws the ball, which takes the barrel off
    /// where it meets it, high over the ground (not on the floor past it),
    /// and goes with it; a coin comes out where the barrel stood, touches
    /// him beside it and pays its gold into his figure's words, the barrel
    /// never broken twice.
    #[test]
    fn a_throw_breaks_the_synthetic_barrel_and_its_coin_pays_his_figure() {
        use guac_formats::script::builder::{ATTACK_BALL, SMASH_BARREL, SMASH_COIN, SMASH_GOLD};
        let mut run = Headless::smash(BARREL_AT);
        let layout = Layout::of(&progression_rows()).unwrap();
        let start = figure::Values {
            xp: 10,
            gold: 5,
            playtime_secs: 0,
        };
        let data = run.joined_holding(&layout, start);
        let (barrel_obj, barrel) = run.placed(SMASH_BARREL);
        assert!(!run.world.taken_off(barrel_obj));
        run.step([true, false, false, false]);
        let ball = *run.clones_named(ATTACK_BALL).first().expect("the ball");
        let mut broke = None;
        for k in 0..60 {
            run.step([false; 4]);
            if run.world.taken_off(barrel_obj) {
                broke = Some(k);
                break;
            }
        }
        let broke = broke.expect("the barrel broke");
        assert!(run.world.removed(barrel));
        assert!(run.world.removed(ball), "the ball with it");
        let at = run.world.actor(ball).unwrap().position;
        let barrel_at = Vec3::from_array(BARREL_AT);
        assert!(at.y > 0.3, "met it over the ground: {at}");
        assert!(
            (at - barrel_at).with_y(0.0).length() < 1.5,
            "where the barrel stands: {at}"
        );
        let coins = run.clones_named(SMASH_COIN);
        assert_eq!(coins.len(), 1, "one coin, at step {broke}");
        let coin = run.world.actor(coins[0]).unwrap().position;
        assert!(coin.distance(barrel_at) < 1e-4, "where it stood: {coin}");
        for _ in 0..5 {
            run.step([false; 4]);
        }
        assert!(run.world.removed(coins[0]), "taken");
        assert_eq!(
            figure::read(&run.vm, &layout, data),
            Some(figure::Values {
                gold: start.gold + SMASH_GOLD as u32,
                ..start
            }),
            "its gold in his words"
        );
        // A second throw finds nothing to break.
        for _ in 0..40 {
            run.step([false; 4]);
        }
        run.step([true, false, false, false]);
        for _ in 0..60 {
            run.step([false; 4]);
        }
        assert_eq!(run.clones_named(SMASH_COIN).len(), 1, "no second coin");
        assert_eq!(run.clones_named(ATTACK_BALL).len(), 2);
    }

    /// AH.27's carry: the level read off a run where the barrel broke holds
    /// it, and since the join only it (the setup that took itself off at the
    /// start isn't play's); put back into a fresh run before its first step
    /// it's off there too, asleep (no ground) and out of the VM, and a throw
    /// at where it stood breaks nothing and drops no coin; a run with
    /// nothing taken off carries nothing, and with no loot data has no
    /// count to set.
    #[test]
    fn a_fresh_run_put_back_keeps_the_barrel_broken() {
        use guac_formats::script::builder::{ATTACK_BALL, SMASH_BARREL, SMASH_COIN, SMASH_SETUP};
        let mut run = Headless::smash(BARREL_AT);
        assert_eq!(
            LevelState::read(&run.vm, &run.world, run.level),
            LevelState::default()
        );
        let layout = Layout::of(&progression_rows()).unwrap();
        run.joined_holding(&layout, figure::Values::default());
        let at_join = LevelState::read(&run.vm, &run.world, run.level).taken_off;
        let (setup_obj, _) = run.placed(SMASH_SETUP);
        assert_eq!(
            at_join,
            [setup_obj],
            "the setup took itself off at the start"
        );
        let (barrel_obj, _) = run.placed(SMASH_BARREL);
        run.step([true, false, false, false]);
        for _ in 0..60 {
            run.step([false; 4]);
        }
        assert!(run.world.taken_off(barrel_obj));
        let kept = LevelState::read(&run.vm, &run.world, run.level).since(&at_join);
        assert_eq!(kept.taken_off, [barrel_obj], "play's alone, not the setup");
        assert_eq!(
            kept.since(&[barrel_obj]).taken_off,
            [],
            "off before the join: not play's"
        );
        assert!(kept.loot.is_empty(), "the synthetic part has no loot data");
        // Another file's placement taken off (his own file's) isn't the
        // level's to carry.
        let other = run
            .files
            .placed
            .iter()
            .find(|p| {
                p.obj.unit != run.level
                    && run.world.handles().contains_key(&p.obj)
                    && !run.world.taken_off(p.obj)
            })
            .expect("another file's placement")
            .obj;
        assert!(run.world.take_off(other));
        assert_eq!(
            LevelState::read(&run.vm, &run.world, run.level)
                .since(&at_join)
                .taken_off,
            [barrel_obj],
            "the level's own alone"
        );

        let mut fresh = Headless::smash(BARREL_AT);
        let (_, barrel) = fresh.placed(SMASH_BARREL);
        kept.put_back(&mut fresh.vm, &mut fresh.world);
        assert!(fresh.world.taken_off(barrel_obj));
        assert!(!fresh.world.actor(barrel).unwrap().active, "asleep");
        assert!(kept.set_loot(&mut fresh.vm, &fresh.world), "nothing to set");
        fresh.joined_holding(&layout, figure::Values::default());
        for (h, active) in fresh.world.activity(&mut fresh.bench) {
            assert!(h != barrel || !active, "never woken again");
        }
        fresh.step([true, false, false, false]);
        for _ in 0..60 {
            fresh.step([false; 4]);
        }
        assert_eq!(fresh.clones_named(ATTACK_BALL).len(), 1);
        assert!(fresh.clones_named(SMASH_COIN).is_empty(), "no coin");
        let ball = fresh.clones_named(ATTACK_BALL)[0];
        let at = fresh.world.actor(ball).unwrap().position;
        assert!(
            at.z > BARREL_AT[2],
            "it flew on past where the barrel stood: {at}"
        );
    }
}
