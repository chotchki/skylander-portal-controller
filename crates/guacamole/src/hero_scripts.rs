//! The hero's own scripts on the TV (openspec AH.24, AH design decisions 1,
//! 5 and 21; hero-attacks, pad-input's The face buttons attack): Eruptor's
//! attacks from his scripts on the VM, in the level from the spawn.
//!
//! - The run: once the level's script part links, the VM runs the TV's
//!   listing ([`guacamole::scripts::smash_config`], openspec AH.27): the init
//!   chain, the UI's and the movement update's ticks, a fighter's or a
//!   walker's scripts once he joins ([`guacamole::scripts::with_walkers`],
//!   AG.27), AH.20's pickups and AH.21's breakables (the loot system's init,
//!   the spinning coins in reach, the archetype setups, the destructibles
//!   system through the player loop), AG.60's start scripts on the props
//!   and AG.28's camera volumes and activator
//!   ([`guacamole::scripts::CAMERA_SCRIPTS`]). No creature runs but the
//!   first fight's, which AH.25's dev entry lists: what the scripts wake
//!   past that stays inert (decision 6). The level's placements take their
//!   palettes off the level's palettes part, as the headless runs do.
//! - The activity rule ([`step_scripts`], AG.7's rule): each step, each
//!   managed placement woken or put to sleep by the hero's pivot, by the
//!   level's start before he's in (as the props stand), so a breakable out
//!   of its range has no ground and sleeps, as AH.21 runs it.
//! - The join: the hero placed on the TV whose file the part holds, a
//!   fighter ([`guac_extract::scripts::FIGHTERS`], Eruptor) to attack and
//!   walk or a walker (AG.27, every other modelled character) to walk
//!   alone, joins by AH.17's join ([`guacamole::scripts::join_request`],
//!   then on the next step [`guacamole::scripts::join_activate`];
//!   stand-in ledger: `join`), the step the TV enters the level; from then
//!   on his walk, facing and clips are his scripts' and his step the engine's
//!   alone ([`ScriptedHero`]), a walker's attack buttons reaching no script
//!   (hero-attacks' Which characters attack). Eruptor with no run (the part
//!   lacking his file), a join that fails or his script stopped in play
//!   walks on AG.16's controller, logged (stand-in ledger: `walker`); a
//!   walker whose join fails keeps his card ([`KeepsCard`]), one whose
//!   script stops in play walks on it at what his scripts had set.
//! - A step ([`step_scripts`], [`Frame::Scripts`]): the frame's slides,
//!   his join's next half, then the walking pad's stick and, once he's in,
//!   its attacks ([`HeroActions`]) read as the first player's pad, then the
//!   walk in the original's order ([`guacamole::frame`], openspec AH.59): his
//!   minion first, every awake placement as it woke with the player loop's
//!   slices in their placements' places, the mover update last; the clones
//!   of scripted templates made outside it added; then the camera's scripts
//!   judged ([`CameraScripts`], AG.28): theirs the view once a hero has
//!   joined and they place the viewport, handed back for good once one of
//!   them or the portal manager stops (AG.191), and from his fall-back until
//!   the next hero's scripts join (AG.190, AG.211). After the hero's step, the
//!   engine's frame ([`advance_scripts`], [`Frame::Actors`]): the sequences
//!   on, what the scripts threw flying through the seam.
//! - His script stopped in play (AG.27's fall-back, stand-in ledger: `walker`):
//!   AG.16's controller walks him from that step on Eruptor's
//!   measured table or a walker's `movement parameters`, falling as every
//!   mover does (AG.173), his walk and facing the controller's alone in the
//!   host (AH.151, [`ScriptWorld::hand_back_walk`]: the mover update would
//!   hand the engine the velocity his stopped minion no longer keeps), and
//!   the view handed to the TV's chain where it stood (AG.190: nothing
//!   steps his camera target off the joined run) until the next hero's
//!   scripts join the run (AG.211: their activation makes his camera target
//!   afresh on his minion, the camera's frame never stopped, so their next
//!   write takes the view back, [`ScriptWorld::hand_back_view_until_a_join`]);
//!   a take-off is no fall-back and leaves the scripts the view. His clips
//!   AG.30's from there (stand-in ledger: `locomotion`), carried on from
//!   where his scripts' picks left him (AH.198, [`pick_gaits`]), a walker's
//!   mixing the walk and run his own scripts define (AG.219).
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
//! mid-level (AG.29, [`crate::play`]) takes his scripts off the run by the
//! deactivate path ([`crate::swap`], openspec AH.105), the level's scripts
//! running on; the hero who comes in where he stood joins that run as the
//! first did.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_transform_interpolation::prelude::TransformInterpolation;
use guac_ir::Model;
use guac_ir::nodes::AttachNodes;
use guac_ir::palette::Palette;
use guac_motion::hero::Hero;
use guac_motion::locomotion::Locomotion;
use guac_motion::seam::{BodyId, PhysicsSeam};
use guac_motion::solid::{SolidInfo, euler};
use guac_script::{Halt, Handle, Mode, Obj, PlacementId, Program, Value};
use guacamole::figure::{self, Layout};
use guacamole::frame::Frame;
use guacamole::script_engine::{Engine, Pad};
use guacamole::scripts::{
    self, Files, ScriptClone, ScriptRuntime, ScriptWorld, WalkerScripts, WorldHost, smash_config,
};
#[cfg(feature = "unmodel")]
use guacamole::unmodel::{UNMODEL_KEY, Unmodel, take_out};
use guacamole::walkers::{self, Fallback, Kept, Movement, Origin, Stage};
use serde::Serialize;
use tracing::{info, warn};

use crate::hero::{
    CameraScripts, FollowCamera, HeroActions, HeroBody, HeroGaits, HeroInput, ScriptedHero,
};
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
    /// Each walker's scripts (AG.27): the other modelled characters whose
    /// own scripts the part holds.
    walkers: Vec<WalkerScripts>,
    /// The characters the part was made without, with why: each keeps his
    /// card.
    left_out: Vec<(String, String)>,
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

    /// What the host reads off the part's files.
    pub(crate) fn files(&self) -> &Files {
        &self.files
    }

    /// The unit `load_file`'s scripts link as, if he's a fighter.
    pub(crate) fn fighter(&self, load_file: &str) -> Option<u16> {
        self.fighters
            .iter()
            .find(|(f, _)| f == load_file)
            .map(|(_, u)| *u)
    }

    /// The walker `load_file` is, if the part holds his scripts (the file
    /// names the load file in the case the disc writes it).
    pub(crate) fn walker(&self, load_file: &str) -> Option<&WalkerScripts> {
        self.walkers
            .iter()
            .find(|w| w.load_file.eq_ignore_ascii_case(load_file))
    }

    /// Whether `load_file` walks on his own scripts (AG.27): why not when
    /// the part holds none of his or his scripts ask the gameplay viewport
    /// something no one has read yet (AH.150's reads). A fighter (Eruptor)
    /// always does, and a character the part holds no word on isn't a
    /// walker of this part's (`Ok(None)`: he walks only on measured
    /// numbers, if any are).
    pub(crate) fn walks(&self, load_file: &str) -> Result<Option<Origin>, String> {
        if self.fighter(load_file).is_some() {
            return Ok(Some(Origin::Measured));
        }
        if let Some(w) = self.walker(load_file) {
            return match (w.asks, &w.minion) {
                (_, None) => Err("his definition names no minion his file holds".to_owned()),
                (0, Some(_)) => Ok(Some(Origin::Scripts)),
                (n, Some(_)) => Err(format!(
                    "his scripts ask {n} points against the gameplay viewport not read for what they gate, which the TV would answer on screen for every point"
                )),
            };
        }
        match self
            .left_out
            .iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(load_file))
        {
            Some((_, why)) => Err(format!("the extract left his scripts out: {why}")),
            None => Ok(None),
        }
    }

    /// Every character the part keeps on his card, with why (AG.27).
    pub(crate) fn kept(&self) -> Vec<Kept> {
        let left = self.left_out.iter().map(|(l, why)| Kept {
            load_file: l.clone(),
            reason: format!("the extract left his scripts out: {why}"),
            slot: None,
        });
        let held = self.walkers.iter().filter_map(|w| {
            Some(Kept {
                load_file: w.load_file.clone(),
                reason: self.walks(&w.load_file).err()?,
                slot: None,
            })
        });
        left.chain(held).collect()
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
            walkers: Vec::new(),
            left_out: Vec::new(),
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
    pub(crate) walkers: Vec<WalkerScripts>,
    pub(crate) left_out: Vec<(String, String)>,
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
        walkers: part.walkers,
        left_out: part.left_out,
        layout: part.layout,
        palettes: part.palettes,
    };
    install_from(world, &source);
    world.insert_resource(source);
    world.insert_resource(ScriptSteps::default());
}

/// A new run's camera: idle until a hero joins it (openspec AG.28), the
/// placements running the camera's level scripts listed so a stop among them
/// hands the view back.
fn fresh_camera(world: &mut World, source: &ScriptSource) {
    world.insert_resource(CameraScripts::default());
    let script_world = world.resource::<ScriptWorld>();
    let placements = source
        .files
        .placed
        .iter()
        .filter(|p| p.script.as_deref().is_some_and(scripts::is_camera_script))
        .filter_map(|p| script_world.handles().get(&p.obj).copied())
        .collect();
    world.insert_resource(CameraPlacements(placements));
}

/// The placements running the camera's level scripts
/// ([`scripts::CAMERA_SCRIPTS`]), by handle.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct CameraPlacements(Vec<Handle>);

/// The run from `source`, its entities and an earlier run's gone first: the
/// TV's listing ([`scripts::smash_config`], openspec AH.27: Eruptor's
/// scripts, the pickups, the breakables, AG.60's start scripts and AG.28's
/// camera volumes and activator; the first fight's with the dev entry's,
/// [`guacamole::scripts::fight_config`]) with the walkers' (AG.27,
/// [`scripts::with_walkers`]), each of the level's placements with a model
/// the level's palettes part holds given its palette, the camera's scripts
/// idle until a hero joins ([`fresh_camera`]).
fn install_from(world: &mut World, source: &ScriptSource) {
    // The dev entry's fight brings the first fight's placements (AH.25).
    let fight = crate::dev_entry::fight_listed(world);
    let config = match fight {
        true => guacamole::scripts::fight_config(Mode::Play),
        false => smash_config(Mode::Play),
    };
    #[allow(unused_mut)]
    let mut config = scripts::with_walkers(config, &source.walkers);
    #[cfg(feature = "unmodel")]
    if let Some(Unmodel(request)) = world.get_resource::<Unmodel>() {
        match take_out(&mut config, request) {
            Ok(name) => info!("{UNMODEL_KEY}: {name} taken out of the run's registry"),
            Err(why) => tracing::error!("{UNMODEL_KEY}: {why}; the run installed unchanged"),
        }
    }
    scripts::install_with(
        world,
        source.program.clone(),
        &source.files,
        &source.order,
        config,
    );
    fresh_camera(world, source);
    let Some(level) = source.level_unit() else {
        return;
    };
    let mut script_world = world.resource_mut::<ScriptWorld>();
    if fight {
        script_world.walk_first_fight(level);
    }
    script_world.walk_goats(level);
    for p in source.files.placed.iter().filter(|p| p.obj.unit == level) {
        if let Some(model) = &p.model
            && let Some((_, lengths)) = source.palettes.iter().find(|(owner, _)| owner == model)
        {
            script_world.set_palette(p.obj, lengths.clone());
        }
    }
}

/// The run installed again from its part: what leaving the level does, the
/// level's next entry a fresh start, every figure's stored data (its health
/// kept across a swap, AH.62) gone with the level, as a level change clears
/// the portal manager's.
pub(crate) fn reinstall(world: &mut World) {
    let Some(source) = world.get_resource::<ScriptSource>().cloned() else {
        return;
    };
    install_from(world, &source);
    world.insert_resource(ScriptSteps::default());
    info!("the scripts installed again for the level's next entry");
}

/// The steps a run makes before a hero's file joins it: the init chain's
/// two, as AH.17's headless runs make them (the portal's loaded objects are
/// set by then). The run on the portal view has made them long before the
/// first hero comes in, and a swap's next hero joins the run the last one
/// left (openspec AH.105).
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
    /// His scripts don't run: why. He walks on AG.16's controller
    /// (stand-in ledger: `walker`).
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
    /// Whether the attack buttons are his scripts' (a fighter's, AH.29;
    /// stand-in ledger: `who-attacks`): a walker's do nothing.
    pub(crate) attacks: bool,
    /// Where his numbers are from (AG.27): a measured walker's scripts
    /// write the numbers he already carries, a scripted walker's are all he
    /// has.
    pub(crate) origin: Origin,
    /// His solid as the disc authors it, before his scale: his scale is his
    /// scripts' alone (AH design decision 5), so his solid and his drawn
    /// model take it from the VM ([`apply_scripted_scale`]).
    pub(crate) shape: SolidInfo,
    /// The scale his solid and his drawn model stand at now.
    pub(crate) scale: f32,
    /// His drawn model's root, which that scale sets.
    pub(crate) root: Option<Entity>,
}

/// A walker whose scripts didn't run through (AG.27): he keeps his card,
/// the reason listed. [`crate::play`] takes him out of the level.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeepsCard(pub(crate) String);

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
/// true when they will (he then walks on them, [`ScriptedHero`]). A fighter
/// (Eruptor) joins to attack and walk, a walker (AG.27) to walk alone.
pub(crate) fn ask_join(
    commands: &mut Commands,
    hero: Entity,
    walk: &walkers::Walk,
    palette: Option<Arc<Palette>>,
    rig: Option<HandRig>,
    source: Option<&ScriptSource>,
    slot: SlotIndex,
    root: Option<Entity>,
) -> bool {
    let load_file = walk.load_file.as_str();
    let found = source.and_then(|s| {
        s.fighter(load_file)
            .map(|u| (u, true))
            .or_else(|| match s.walks(load_file) {
                Ok(Some(_)) => s.walker(load_file).map(|w| (w.unit, false)),
                _ => None,
            })
    });
    let Some((unit, attacks)) = found else {
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
            attacks,
            origin: walk.origin,
            shape: walk.shape,
            scale: walk.scale,
            root,
        },
        ScriptedHero,
    ));
    info!(
        load_file,
        unit, attacks, "his scripts join at the next step"
    );
    true
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
/// handle and its body: the level's file's placements by their object; then
/// the heroes' and the creatures' bodies the TV keeps there
/// ([`crate::creatures`], openspec AH.25).
pub(crate) fn standing_props(
    level: &Level,
    world: &ScriptWorld,
    source: Option<&ScriptSource>,
) -> Vec<(Handle, BodyId)> {
    let Some(unit) = source.and_then(ScriptSource::level_unit) else {
        return world.embodied.clone();
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
        .chain(world.embodied.iter().copied())
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
    /// (the level left and entered again).
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
        app.init_resource::<ScriptLog>()
            .add_systems(
                FixedUpdate,
                (
                    (step_scripts, apply_scripted_scale)
                        .chain()
                        .in_set(Frame::Scripts),
                    (advance_scripts, draw_clones).chain().in_set(Frame::Actors),
                    pick_gaits
                        .in_set(Frame::Locomotion)
                        .before(crate::hero::step_gaits),
                    pose_hands.in_set(Frame::Hands),
                )
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>),
            )
            .add_systems(
                Update,
                set_asked_health.run_if(resource_exists::<AskedHealth>),
            );
    }
}

/// A scripted walker's scale is his scripts' alone (AH design decision 5,
/// AG.27): once they run, his actor's scale on the VM is the one his solid
/// is built at, his drawn model stands at and his hands are posed at
/// ([`pose_hands`], AG.181: they stood at 1.0 while he was drawn at his
/// scripts' 1.2). A scale that isn't a uniform one (a script setting one
/// axis) is left as it was; a measured walker's stays the measured one.
pub(crate) fn apply_scripted_scale(
    world: Res<ScriptWorld>,
    mut heroes: Query<(Entity, &mut HeroRun, &mut HeroBody)>,
    mut transforms: Query<&mut Transform>,
) {
    for (e, mut run, mut body) in &mut heroes {
        if run.origin != Origin::Scripts || !matches!(run.join, Join::Joined(_)) {
            continue;
        }
        let Some(a) = world.actor(e.to_bits()) else {
            continue;
        };
        let Some(scale) = walkers::uniform_scale(a.scale) else {
            continue;
        };
        if (scale - run.scale).abs() < 1e-6 {
            continue;
        }
        let Some(solid) = run.shape.havok([scale * 100.0; 3]) else {
            continue;
        };
        info!(load_file = %run.load_file, from = run.scale, to = scale, "his scripts set his scale");
        body.hero.set_solid(solid);
        run.scale = scale;
        if let Some(rig) = run.rig.as_mut() {
            rig.scale = scale;
        }
        if let Some(root) = run.root
            && let Ok(mut t) = transforms.get_mut(root)
        {
            t.scale = Vec3::splat(scale);
        }
    }
}

/// AH.62's test hook (`test-hooks` builds): a health asked for over `POST
/// /api/_test/tv/health`, set where the joined hero's scripts hold it in the
/// frame's `Update` (after its fixed steps, so the next frame's read it),
/// doing a bite's work as AH.11's `swap1` poked the original's hero to 200.
#[derive(Resource, Clone, Default)]
pub(crate) struct AskedHealth(pub(crate) Arc<Mutex<Option<i32>>>);

/// The most a health asked for may be: past any figure's full health at
/// the level cap (AH.2).
const HEALTH_ASKED_MAX: i32 = 99_999;

/// The route's body; its range is the route's to check.
#[derive(serde::Deserialize)]
struct HealthBody {
    health: i32,
}

/// `POST /api/_test/tv/health` (`{"health": 200}`): 202 with it kept for
/// the next frame; 400, keeping nothing, for a health outside 1 to 99,999
/// or any other body. Generic so a test needs no `AppState`.
pub(crate) fn health_routes<S: Clone + Send + Sync + 'static>(
    asked: &AskedHealth,
) -> axum::Router<S> {
    use axum::extract::rejection::JsonRejection;
    let asked = asked.0.clone();
    axum::Router::new().route(
        "/api/_test/tv/health",
        axum::routing::post(move |body: Result<axum::Json<HealthBody>, JsonRejection>| {
            let kept = match body {
                Ok(axum::Json(b)) if (1..=HEALTH_ASKED_MAX).contains(&b.health) => {
                    *asked.lock().unwrap_or_else(|e| e.into_inner()) = Some(b.health);
                    true
                }
                _ => false,
            };
            async move {
                if kept {
                    axum::http::StatusCode::ACCEPTED
                } else {
                    axum::http::StatusCode::BAD_REQUEST
                }
            }
        }),
    )
}

/// The health [`AskedHealth`] holds, set on the hero whose scripts joined;
/// with none joined it's dropped with a warning.
fn set_asked_health(
    asked: Res<AskedHealth>,
    runtime: Option<ResMut<ScriptRuntime>>,
    runs: Query<&HeroRun>,
) {
    let Some(health) = asked.0.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let joined = runs.iter().find_map(|r| match r.join {
        Join::Joined(m) => Some(m),
        _ => None,
    });
    let set = match (joined, runtime) {
        (Some(minion), Some(mut rt)) => scripts::set_health(&mut rt.vm, minion, health),
        _ => false,
    };
    if set {
        info!(health, "his health set over the test hook");
    } else {
        warn!(
            health,
            "no hero whose scripts run to take the health asked for"
        );
    }
}

/// The heading the TV's view looked along as the step before left it
/// (the camera's scripts' or the chain's, openspec AG.28); 0 with none.
pub(crate) fn view_heading(cameras: &Query<&FollowCamera>) -> f32 {
    cameras.iter().next().map_or(0.0, |f| f.drawn.heading())
}

/// One step of the run, before the hero's (the module docs).
pub(crate) fn step_scripts(
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
    (level, real, mut camera, camera_placements): (
        Res<Level>,
        Res<Time<Real>>,
        ResMut<CameraScripts>,
        Option<Res<CameraPlacements>>,
    ),
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
        // His palette on his minion's template: a walker's the one his
        // definition names (AG.27), Eruptor's the one named for him.
        if let Some(r) = run.as_deref()
            && let Some(palette) = &r.palette
            && let Some(template) = source
                .as_ref()
                .and_then(|s| s.walker(&r.load_file))
                .and_then(|w| w.template)
                .or_else(|| {
                    files
                        .placed
                        .iter()
                        .find(|p| p.obj.unit == unit && p.name == r.load_file)
                        .map(|p| p.obj)
                })
        {
            world.set_palette_of(template, palette);
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
    // A walker's (AG.27) do nothing either way: attacks stay Eruptor's
    // alone (AH.29; stand-in ledger: `who-attacks`).
    let attacks = run.as_deref().is_none_or(|r| r.attacks);
    let held = if joined.is_some() && attacks {
        held
    } else {
        [false; 4]
    };
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
    // The camera's scripts (openspec AG.28): theirs the view once a hero has
    // joined their run and they place the viewport, the chain's for good once
    // one of them stops in play, and from a hero's fall-back until the next
    // hero joins (AG.190, AG.211).
    let now = camera_state(
        &camera,
        &runtime,
        &mut world,
        camera_placements.as_deref().map_or(&[], |c| &c.0),
        joined.is_some(),
    );
    if now != *camera {
        match &now {
            CameraScripts::On => info!("the camera's scripts have the view"),
            CameraScripts::Stopped(why) => warn!(reason = %why, "the camera's scripts stopped"),
            CameraScripts::HeroOff(_) | CameraScripts::Idle(_) => {}
        }
        *camera = now;
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
        // His setup ran through when his minion's first tick did (STARTUP's);
        // a stop before that is the join's, after it the play's.
        let stage = match joined {
            Some(m) if runtime.vm.set_up(m) => Stage::Playing,
            _ => Stage::Joining,
        };
        // What he does now (AG.27; stand-in ledger: `walker`): Eruptor on
        // the measured numbers he carries; a walker whose script stopped in
        // play on the movement parameters his scripts had written into his
        // description (never his actor's engine numbers, 10,000 ft/s); one
        // whose join never completed keeps his card.
        let movement = joined.and_then(|m| Movement::read(&runtime.vm, m));
        let fallback = match (walkers::fallback(r.origin, stage, &why), movement) {
            (Fallback::OnWhatTheySet, None) => Fallback::Card(format!(
                "{why}; his scripts had set no movement parameters to walk on"
            )),
            (f, _) => f,
        };
        match &fallback {
            Fallback::Stand => {
                warn!(load_file = %r.load_file, reason = %why, "he walks on the stand-in now; his attack buttons do nothing");
            }
            Fallback::OnWhatTheySet => {
                warn!(load_file = %r.load_file, reason = %why, "he walks on what his scripts had set now; his attack buttons do nothing");
                if let (Some(m), Ok((_, mut body))) = (movement, heroes.get_mut(e)) {
                    body.hero.set_tuning(m.tuning(r.shape.width));
                }
            }
            Fallback::Card(reason) => {
                warn!(load_file = %r.load_file, reason = %reason, "his scripts don't run: he keeps his card");
                commands.entity(e).insert(KeepsCard(reason.clone()));
            }
        }
        if matches!(fallback, Fallback::Stand | Fallback::OnWhatTheySet) {
            // His walk and facing the controller's alone from here (openspec
            // AH.151): the mover update would hand the engine, over his walk,
            // the velocity his stopped minion no longer keeps.
            world.hand_back_walk(e.to_bits());
            // And the view the TV's (openspec AG.190): nothing steps his
            // camera target once he's off the player loop's joined run, until
            // the next hero's scripts join it (AG.211).
            if *camera == CameraScripts::On {
                let reason =
                    format!("his scripts stopped, so nothing steps his camera target: {why}");
                warn!(load_file = %r.load_file, %reason, "the camera's scripts lose the view until a hero joins their run");
                world.hand_back_view_until_a_join();
                *camera = CameraScripts::HeroOff(reason);
            }
        }
        r.join = Join::Off(why);
        commands.entity(e).remove::<ScriptedHero>();
    }
    // The log's own count, on across the run's installs: a run installed
    // again (the level left and entered again) starts its steps from 1, and
    // a reader's `since` would skip them.
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

/// The camera's scripts' state after a step (openspec AG.28): stopped for
/// good once the portal manager's camera frame, a player's camera target,
/// any stretch of the portal manager's (AG.191: its tick and the frame in
/// it stop with it) or a camera volume's or activator's script stops in
/// play (the view handed back to the TV's camera); driving once a hero has
/// joined their run and they place the viewport, from then on; idle, with
/// why, before that or with no camera system in the part. A hero falling
/// back hands the view back too ([`step_scripts`], AG.190), until the next
/// hero's scripts join and the camera's scripts write it again (AG.211:
/// [`ScriptWorld::hero_template`] gives it back); a take-off doesn't.
fn camera_state(
    now: &CameraScripts,
    runtime: &ScriptRuntime,
    world: &mut ScriptWorld,
    placements: &[Handle],
    joined: bool,
) -> CameraScripts {
    if let CameraScripts::Stopped(_) = now {
        return now.clone();
    }
    if runtime.player_loop.camera.is_none() {
        return CameraScripts::Idle("the level's part runs no camera system".to_owned());
    }
    let Some(pm) = scripts::portal_manager_placement(&runtime.vm, world) else {
        return CameraScripts::Idle("the level's part runs no portal manager".to_owned());
    };
    // The portal manager stopped by any stretch of its own (openspec
    // AG.191): its tick, the camera's frame in it, never runs again. A
    // stretch run apart puts its own stop back, so only another's stays.
    let stopped = runtime
        .player_loop
        .camera_stopped(&runtime.vm, pm)
        .map(|h| format!("the camera's frame stopped: {h}"))
        .or_else(|| {
            runtime
                .vm
                .stopped(pm)
                .map(|h| format!("the portal manager stopped, the camera's frame with it: {h}"))
        })
        .or_else(|| {
            placements
                .iter()
                .filter_map(|h| runtime.vm.placement_of(*h))
                .find_map(|id| runtime.vm.stopped(id))
                .map(|h| format!("a camera script stopped: {h}"))
        });
    if let Some(why) = stopped {
        world.hand_back_view();
        return CameraScripts::Stopped(why);
    }
    match now {
        CameraScripts::On => CameraScripts::On,
        _ if joined && world.viewport().scripted() => CameraScripts::On,
        idle => idle.clone(),
    }
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
pub(crate) fn advance_scripts(
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

/// What his scripts' locomotion left on him this step, for each hero whose
/// scripts run (openspec AH.49): his gaits' `picked`, and the step before's
/// weights kept for his pose's easing. With it, AG.30's picker is off for him
/// ([`crate::hero::step_gaits`]); with none (no locomotion of his scripts
/// weights anything on him, or they've stopped and he walks on AG.30's,
/// stand-in ledger: `locomotion`), it picks as before, from where the picks
/// left him (AH.198, [`HeroGaits::pick`]), a walker's on the walk and run
/// his own scripts define (AG.219, [`walkers::stand_in`]), where AG.27 gave
/// him none.
pub(crate) fn pick_gaits(
    world: Res<ScriptWorld>,
    runtime: Option<Res<ScriptRuntime>>,
    mut heroes: Query<(
        Entity,
        &HeroRun,
        &HeroBody,
        &mut HeroGaits,
        Has<ScriptedHero>,
    )>,
) {
    for (e, run, body, mut gaits, scripted) in &mut heroes {
        let own = |was: &Locomotion| {
            let (Some(rt), Some(palette), Some(rig)) = (&runtime, &run.palette, &run.rig) else {
                return None;
            };
            if run.origin != Origin::Scripts {
                return None;
            }
            let states = walkers::locomotion_states(&rt.vm, &world, e.to_bits())?;
            let own = walkers::stand_in(
                &states,
                palette,
                rig.clips.walk.as_ref().map(|c| (c, rig.clips.slots[0])),
                rig.clips.run.as_ref().map(|c| (c, rig.clips.slots[1])),
                *was.loops(),
            )?;
            info!(
                load_file = %run.load_file,
                walk = ?own.loops().walk,
                run = ?own.loops().run,
                gaits = ?own.gaits(),
                "AG.30's locomotion draws him on his scripts' walk and run"
            );
            Some(own)
        };
        let four = match (&run.palette, &run.rig) {
            (Some(p), Some(rig)) => rig.clips.four(p, &rig.model.idle.name),
            _ => guacamole::picks::Four::default(),
        };
        gaits.pick(
            scripted
                .then(|| {
                    guacamole::picks::Picked::read(&world, e.to_bits(), four, body.hero.gait_speed)
                })
                .flatten(),
            own,
        );
    }
}

/// His attach nodes posed by the clips on him as this step left them, his
/// locomotion's (his scripts' picks, else AG.30's; his idle alone
/// with neither) with his scripts' laid over, and what hangs from them set
/// there (the module docs; AH.50).
pub(crate) fn pose_hands(
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
        let base = match (gaits, &run.palette) {
            (Some(g), Some(palette)) if g.picked.is_some() => g
                .picked
                .as_ref()
                .map(|p| p.posed(&[], palette, (1.0, 0.0)))
                .unwrap_or_default(),
            (Some(g), _) => play::locomotion_clips(&rig.model, &rig.clips, g),
            (None, _) => vec![(&rig.model.idle, 0.0, 1.0)],
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
/// steps, hidden once taken off. One made before the level's loaded (the
/// start's scripts run from the part's link, openspec AG.60) waits for its
/// models.
pub(crate) fn draw_clones(
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
    let Some(models) = level.spawn_models() else {
        return;
    };
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
                    .and_then(|m| models.get(m));
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
/// first; his locomotion's aren't among them (openspec AH.49:
/// [`crate::script_engine::Sequence::laid_over`]).
pub(crate) fn playing_on(world: &ScriptWorld, hero: Handle) -> Vec<(u32, f32, f32)> {
    world
        .sequences()
        .iter()
        .filter(|s| s.actor == hero && s.plays() && s.laid_over())
        .filter_map(|s| world.clip_of(s).map(|(_, slot)| (slot, s.time, s.rate)))
        .collect()
}

/// The level's loot counts as its scripts hold them (AH.21's running count:
/// a break drops when its kind's count lands at or under the kind's rate).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LootShot {
    pub common: Option<f32>,
    pub uncommon: Option<f32>,
    pub rare: Option<f32>,
}

/// The loot counts' part of `/api/_test/tv` (openspec AQ.8).
#[derive(SystemParam)]
pub(crate) struct LootShots<'w> {
    runtime: Option<Res<'w, ScriptRuntime>>,
    world: Option<Res<'w, ScriptWorld>>,
}

impl LootShots<'_> {
    /// None until the level's loot data is made.
    pub(crate) fn shot(&self) -> Option<LootShot> {
        match (self.runtime.as_deref(), self.world.as_deref()) {
            (Some(rt), Some(w)) => {
                use guacamole::scripts::{Rarity, loot_index};
                let shot = LootShot {
                    common: loot_index(&rt.vm, w, Rarity::Common),
                    uncommon: loot_index(&rt.vm, w, Rarity::Uncommon),
                    rare: loot_index(&rt.vm, w, Rarity::Rare),
                };
                (shot != LootShot::default()).then_some(shot)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod camera_tests;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use guac_formats::script::builder::{
        ATTACK_ARRIVAL_SLOT, ATTACK_BALL, ATTACK_HEALTH, ATTACK_HEALTH_MAX, ATTACK_HELPER,
        ATTACK_SECOND_SLOT, ATTACK_THROW_SLOT,
    };
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios;
    use guac_script::{Generator, Vm};
    use guacamole::level_scripts;
    use guacamole::scripts::{headless_handles, headless_objects};

    use super::*;
    use crate::test_support::hero_scripts::{LEVEL, progression_rows, write_attack, write_smash};

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

    /// AH.62's hook: a health from 1 to 99,999 is kept for the next frame;
    /// anything else is a 400 and keeps nothing.
    #[tokio::test]
    async fn the_health_hook_keeps_a_health_in_range_and_refuses_the_rest() {
        use tower::ServiceExt;
        let asked = AskedHealth::default();
        let routes = health_routes::<()>(&asked);
        for (body, status, kept) in [
            (r#"{"health":200}"#, 202, Some(200)),
            (r#"{"health":1}"#, 202, Some(1)),
            (r#"{"health":0}"#, 400, None),
            (r#"{"health":100000}"#, 400, None),
            (r#"{"hp":200}"#, 400, None),
            ("200", 400, None),
        ] {
            *asked.0.lock().unwrap() = None;
            let response = routes
                .clone()
                .oneshot(
                    axum::http::Request::post("/api/_test/tv/health")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{body}");
            assert_eq!(*asked.0.lock().unwrap(), kept, "{body}");
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
                walkers: Vec::new(),
                left_out: Vec::new(),
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

    /// AG.60: a clone the scripts make before the level's loaded (the
    /// start's, run from the part's link) isn't settled as drawing nothing:
    /// it waits, undecided, for the level's models.
    #[test]
    fn a_clone_made_before_the_level_loads_waits_for_its_models() {
        use bevy::ecs::system::RunSystemOnce;
        let tmp = tempfile::tempdir().unwrap();
        write_attack(tmp.path());
        let files = level_scripts::load(tmp.path(), LEVEL).unwrap().placements;
        let mut app = App::new();
        let clone = app.world_mut().spawn(ScriptClone(Value::Null)).id();
        // Its actor: a placement's, answering for the clone's entity.
        let mut handles = headless_handles(&files);
        handles.insert(files.placed[0].obj, clone.to_bits());
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        assert!(world.actor(clone.to_bits()).is_some());
        app.insert_resource(world)
            .insert_resource(Level::new(false));
        app.world_mut().run_system_once(draw_clones).unwrap();
        assert!(app.world().entity(clone).get::<CloneDrawn>().is_none());
    }
}
