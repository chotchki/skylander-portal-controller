//! The turntable on the TV (openspec AE.13, design decision 14; the
//! figure-turntable spec): each occupied slot whose figure has a character
//! model in the extract gets a scene of its own, outside the UI tree, drawn
//! by a camera of its own into an image the slot keeps. The card's image node
//! shows that image once the model is up and the figure's portrait until
//! then, and for good when there's no model to show (`tv::show_turntables`).
//!
//! - Reconciled from [`PortalCards`] by (slot, placement): a placement that
//!   changes respawns that slot's scene alone, and a UI rebuild (the portal
//!   entered again, a cell respawned) never touches one.
//! - Each slot draws on render layers of its own (the model, and its glow
//!   masks for the game's glow), so its camera sees nothing of another's.
//! - The model is read off the extract on a task, off the frame: the IR,
//!   its textures through the remaster's views, the framing from its idle
//!   loop's sweep ([`character::turntable`]). A model that won't read, or a
//!   figure with no model, keeps the portrait and is logged by figure and why;
//!   no other slot notices.
//! - The camera is the remaster's (HDR, TonyMcMapface, 4x MSAA) at the
//!   framing's fixed pitch, lit by the disc's sprite rig fixed to it, fog on
//!   past everything, no shadows or point lights. It clears to the darker stop
//!   of the figure's element (the plate behind a portrait), opaque, so blended
//!   and additive parts blend on what the card shows and the glow keeps its
//!   halo. The image is the largest plate's size in pixels and follows it.
//! - The model turns about its vertical axis once every
//!   [`TURN_SECONDS`] playing its idle, and a part with a scroll track
//!   slides its texture coordinates on the same clock (AE.21,
//!   [`guac_ir::scroll`]); under [`ReducedMotion`] it holds at [`FrozenYaw`]
//!   in its idle's first pose with each scroll at its start, so a shot
//!   repeats.

use std::collections::HashMap;
use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Projection, RenderTarget};
use bevy::image::CompressedImageFormats;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::ui::ComputedNode;
use crossbeam_channel::{Receiver, Sender};
use guac_extract::intros::IrFormat;
use guac_extract::stages::ModelFailure;
use guac_ir::model::{Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::Scene;
use guac_ir::{Category, DumpIdentity, Model, Rig, Roster};
use guacamole::character::attach::{self, SpawnedAttachments};
use guacamole::character::turntable::{Framing, SAMPLES, Sweep, TURN_SECONDS, rig_scene};
use guacamole::character::warm::{self, WarmKey};
use guacamole::character::{
    CharacterMaterial, CharacterUniform, GlowView, ShadowLookup, remaster, spawn_character,
    spawn_glow_camera, spawn_glow_masks,
};
use serde::de::DeserializeOwned;
use skylander_core::SLOT_COUNT;
use skylander_rpcs3_control::Placement;
use tracing::{info, warn};

use crate::cards::PortalCards;
use crate::extraction::Extraction;
use crate::motion::ReducedMotion;
use crate::theme::TvTheme;
use crate::warm::Warm;

/// What the extract holds for the turntable (published with the roster's
/// models, AE.13): where it is, in which IR format, the character load file
/// each figure draws, which of those didn't extract and the light rig.
#[derive(Debug)]
pub struct Shelf {
    pub dir: PathBuf,
    pub ir: IrFormat,
    pub identity: DumpIdentity,
    /// Figure id → its character load file, for the roster's figures and
    /// Giants (items, adventure packs and sidekicks keep portraits in AE).
    pub loads: HashMap<String, String>,
    /// Figure id → its category, for every figure on the roster: what the
    /// portal's flow tells characters, magic items and toys the game can't
    /// use apart by (AG.29, AG.63, AG.73).
    pub kinds: HashMap<String, Category>,
    /// Load files the model stage reported, with why.
    pub failed: HashMap<String, String>,
    /// The sprite rig (`ir/rig`), or why it didn't read.
    pub rig: Result<Rig, String>,
}

impl Shelf {
    /// The shelf for `roster`'s extract at `dir`, its rig read from there.
    pub fn new(roster: &Roster, dir: &Path, ir: IrFormat, failed: &[ModelFailure]) -> Self {
        let loads = roster
            .figures
            .iter()
            .filter(|f| matches!(f.category, Category::Figure | Category::Giant))
            .map(|f| (f.id(), f.load_file.clone()))
            .collect();
        let kinds = roster
            .figures
            .iter()
            .map(|f| (f.id(), f.category))
            .collect();
        let rig = read_ir::<Rig>(ir, &dir.join(ir.rig_path())).and_then(|rig| {
            (rig.identity == roster.identity)
                .then_some(rig)
                .ok_or_else(|| format!("{} names another dump", ir.rig_path()))
        });
        Self {
            dir: dir.to_path_buf(),
            ir,
            identity: roster.identity.clone(),
            loads,
            kinds,
            failed: failed
                .iter()
                .map(|f| (f.load_file.clone(), f.reason.clone()))
                .collect(),
            rig,
        }
    }
}

/// Where the extraction puts a [`Shelf`] for the TV (`ExtractJob::shelf`).
pub type ShelfSender = Sender<Arc<Shelf>>;

/// The TV's end: the newest shelf replaces the one before (`receive_shelf`).
#[derive(Resource)]
pub struct ShelfFeed(pub Receiver<Arc<Shelf>>);

/// A channel for [`Shelf`]s: the sender for the extraction's job, the feed
/// for the app.
pub fn shelf_channel() -> (ShelfSender, ShelfFeed) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (tx, ShelfFeed(rx))
}

/// The idle's frozen turn under [`ReducedMotion`], radians: 0 faces the
/// camera. Shots set it from `GUAC_SHOT_YAW` to walk a turn.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct FrozenYaw(pub(crate) f32);

/// The largest bezel plate's size in pixels, which each turntable's image
/// takes ([`follow_plates`]).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlateSize(pub(crate) UVec2);

impl Default for PlateSize {
    /// Until a plate is laid out: the plate at 4K.
    fn default() -> Self {
        Self(UVec2::splat(256))
    }
}

/// A card's state as `/api/_test/tv` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardState {
    /// Its model is on the card.
    Model,
    /// Its portrait is, for good: no model, or one that couldn't be shown.
    Portrait,
    /// Its portrait is while the model is read or the extract makes it.
    Loading,
}

impl CardState {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Portrait => "portrait",
            Self::Loading => "loading",
        }
    }
}

/// Every slot's turntable and the shelf they draw from.
#[derive(Resource, Default)]
pub(crate) struct Turntables {
    slots: [Option<Turntable>; SLOT_COUNT],
    shelf: Option<Arc<Shelf>>,
}

impl Turntables {
    /// The image a slot's card shows in place of its portrait, once its
    /// model is up.
    pub(crate) fn image(&self, slot: u8) -> Option<Handle<Image>> {
        match &self.slots.get(usize::from(slot))?.as_ref()?.state {
            State::Model(stage) => Some(stage.image.clone()),
            _ => None,
        }
    }

    /// The cameras of the cards whose models are up, the warm's apart; each
    /// one's glow camera is its child.
    pub(crate) fn card_cameras(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots.iter().flatten().filter_map(|t| match &t.state {
            State::Model(stage) => Some(stage.camera),
            _ => None,
        })
    }

    /// What `slot`'s card shows; `None` for an empty slot.
    pub(crate) fn card_state(&self, slot: u8) -> Option<CardState> {
        let t = self.slots.get(usize::from(slot))?.as_ref()?;
        Some(match &t.state {
            State::Model(_) => CardState::Model,
            State::Portrait(_) => CardState::Portrait,
            State::Loading(_) | State::Compiling { .. } => CardState::Loading,
            State::NoShelf { extracting: true } => CardState::Loading,
            State::NoShelf { extracting: false } => CardState::Portrait,
        })
    }

    #[cfg(test)]
    pub(crate) fn stage_entities(&self, slot: u8) -> Option<(Entity, Entity)> {
        match &self.slots[usize::from(slot)].as_ref()?.state {
            State::Model(s) => Some((s.root, s.camera)),
            _ => None,
        }
    }

    /// A stage's body joints and what it carries: each attachment's root
    /// and the entities of its drawn parts.
    #[cfg(test)]
    pub(crate) fn carried(&self, slot: u8) -> Option<(Vec<Entity>, Vec<Entity>, Vec<Entity>)> {
        match &self.slots[usize::from(slot)].as_ref()?.state {
            State::Model(s) => Some((
                s.joints.clone(),
                s.attachments.roots.clone(),
                s.attachments.parts.iter().map(|p| p.entity).collect(),
            )),
            _ => None,
        }
    }

    /// The shelf the cards draw from, once one has landed.
    pub(crate) fn shelf(&self) -> Option<&Arc<Shelf>> {
        self.shelf.as_ref()
    }

    /// Why `slot`'s card keeps its portrait, when it does for good.
    pub(crate) fn portrait_reason(&self, slot: u8) -> Option<&str> {
        match &self.slots[usize::from(slot)].as_ref()?.state {
            State::Portrait(why) => Some(why),
            _ => None,
        }
    }
}

struct Turntable {
    placement: Placement,
    state: State,
}

enum State {
    /// No shelf yet: models still coming while an extraction runs.
    NoShelf {
        extracting: bool,
    },
    Loading(Task<Result<Loaded, String>>),
    /// Read, and waiting for the warm to compile the pipelines it draws
    /// with (`crate::warm`), so it never comes up on one that isn't.
    Compiling {
        loaded: Box<Loaded>,
        keys: Vec<WarmKey>,
        since: Instant,
    },
    Model(Stage),
    /// Why there's no model on the card.
    Portrait(String),
}

/// A slot's scene: the model under its root, posed through its joints, and
/// the camera drawing it into `image`.
struct Stage {
    root: Entity,
    joints: Vec<Entity>,
    camera: Entity,
    image: Handle<Image>,
    size: UVec2,
    model: Arc<Model>,
    /// What it carries (AE.23): a skinned attachment's joints follow its
    /// own idle.
    attachments: SpawnedAttachments,
    framing: Framing,
    backdrop: Color,
    /// When it came up, for its turn, idle and scrolls.
    since: Duration,
    scrolling: Vec<Scrolling>,
}

/// A part a scroll track moves: its tracks, and the colour and glow-mask
/// materials whose texture-coordinate transforms follow them.
struct Scrolling {
    scrolls: Vec<Scroll>,
    materials: Vec<Handle<CharacterMaterial>>,
}

/// A model read off the extract, ready to spawn: a card's, or the hero's
/// ([`crate::play`]).
pub(crate) struct Loaded {
    pub(crate) model: Arc<Model>,
    /// Each texture as a role samples it: (extract path, sRGB view, sampler).
    pub(crate) images: Vec<((String, bool, Sampler), Image)>,
    framing: Framing,
    pub(crate) took: Duration,
}

/// The turntable cameras draw before the TV's (order 0) samples their
/// images, as the titles' do; each one's glow camera before it.
const CAMERA_ORDER: isize = -1;
const GLOW_ORDER: isize = -2;

/// Render layers: two a slot from here (the model, then its glow masks),
/// clear of the default layer every other camera draws.
const FIRST_LAYER: usize = 16;

/// The warm's scene ([`crate::warm`]) takes the pair past the eight slots'.
pub(crate) const WARM_SLOT: usize = SLOT_COUNT;

pub(crate) fn model_layer(slot: usize) -> usize {
    FIRST_LAYER + 2 * slot
}

pub(crate) fn glow_layer(slot: usize) -> usize {
    FIRST_LAYER + 2 * slot + 1
}

/// Orders the turntable's systems; the card's image follows them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TurntableSystems;

/// The turntables and their warm ([`crate::warm`]): needs `Assets` for
/// images, meshes, inverse bindposes and [`CharacterMaterial`] (the running
/// TV's `CharacterShadingPlugin`, or the tests' own), [`PortalCards`] and
/// [`ReducedMotion`].
pub(crate) struct TurntablePlugin;

impl Plugin for TurntablePlugin {
    fn build(&self, app: &mut App) {
        crate::warm::build(app);
        app.init_resource::<Turntables>()
            .init_resource::<FrozenYaw>()
            .init_resource::<PlateSize>()
            .add_systems(
                Update,
                (
                    receive_shelf,
                    reconcile,
                    poll_loads,
                    crate::warm::step,
                    follow_plates,
                    resize_targets,
                    turn,
                )
                    .chain()
                    .in_set(TurntableSystems),
            );
    }
}

fn receive_shelf(
    feed: Option<Res<ShelfFeed>>,
    mut turntables: ResMut<Turntables>,
    mut warm: ResMut<Warm>,
) {
    let Some(feed) = feed else { return };
    if let Some(shelf) = feed.0.try_iter().last() {
        info!(
            dir = %shelf.dir.display(),
            models = shelf.loads.len(),
            failed = shelf.failed.len(),
            rig = shelf.rig.is_ok(),
            "turntable shelf"
        );
        warm.start(&shelf);
        turntables.shelf = Some(shelf);
    }
}

/// Each slot follows its placement in `PortalCards`; a slot waiting for
/// the shelf starts once it's there.
fn reconcile(
    mut commands: Commands,
    cards: Res<PortalCards>,
    mut turntables: ResMut<Turntables>,
    extraction: Option<Res<Extraction>>,
    device: Option<Res<RenderDevice>>,
) {
    let extracting = extraction.is_some_and(|e| e.feed.is_some());
    // Headless (no GPU) the images keep their blocks as given.
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    let Turntables { slots, shelf } = &mut *turntables;
    for (i, slot) in slots.iter_mut().enumerate() {
        let wanted = cards.slots[i].as_ref();
        if slot.as_ref().map(|t| &t.placement) != wanted {
            if let Some(old) = slot.take() {
                old.state.clear(&mut commands);
            }
            *slot = wanted.map(|p| Turntable {
                placement: p.clone(),
                state: State::NoShelf { extracting },
            });
        }
        let Some(t) = slot.as_mut() else { continue };
        if let State::NoShelf { .. } = t.state {
            t.state = match shelf {
                Some(shelf) => begin(i, &t.placement, shelf, formats),
                None => State::NoShelf { extracting },
            };
        }
    }
}

impl State {
    /// Despawns what the state spawned; a load in flight is dropped.
    fn clear(self, commands: &mut Commands) {
        if let Self::Model(stage) = self {
            commands.entity(stage.root).despawn();
            commands.entity(stage.camera).despawn();
        }
    }
}

/// What a figure's card does on a shelf: read its model, or keep its
/// portrait and say why.
fn begin(slot: usize, p: &Placement, shelf: &Arc<Shelf>, formats: CompressedImageFormats) -> State {
    let id = p.figure_id.as_str();
    let Some(load) = shelf.loads.get(id) else {
        info!(
            slot = slot + 1,
            figure = %p.name,
            id,
            "no character model on the disc; the card keeps its portrait"
        );
        return State::Portrait("no character model on the disc".into());
    };
    let refused = |why: String| {
        warn!(slot = slot + 1, figure = %p.name, id, load_file = %load, reason = %why,
            "the turntable can't show this model; the card keeps its portrait");
        State::Portrait(why)
    };
    if let Some(why) = shelf.failed.get(load) {
        return refused(format!("the model didn't extract: {why}"));
    }
    if let Err(why) = &shelf.rig {
        return refused(format!("no light rig: {why}"));
    }
    let (shelf, load) = (shelf.clone(), load.clone());
    let task = AsyncComputeTaskPool::get().spawn(async move { load_model(&shelf, &load, formats) });
    State::Loading(task)
}

/// A load file's model, textures and framing off the extract.
pub(crate) fn load_model(
    shelf: &Shelf,
    load: &str,
    formats: CompressedImageFormats,
) -> Result<Loaded, String> {
    let started = Instant::now();
    let rel = shelf.ir.model_path(load);
    let model = read_model(shelf, load)?;
    if let Some(why) = &model.attachments_unsettled {
        // The extract keeps why; the card draws the body alone (AE.23).
        warn!(load, %why, "what the figure carries didn't settle: its body alone");
    }
    let mut images: Vec<((String, bool, Sampler), Image)> = Vec::new();
    let drawn = attach::drawn(&model);
    for slot in drawn.materials.iter().flat_map(|m| &m.textures) {
        let key = (
            slot.texture.clone(),
            remaster::is_srgb(slot.role),
            slot.sampler,
        );
        if images.iter().any(|(k, _)| *k == key) {
            continue;
        }
        if !plain(&slot.texture) {
            return Err(format!("{rel} names a texture outside the extract"));
        }
        let path = shelf.dir.join(&slot.texture);
        let ktx2 = std::fs::read(&path).map_err(|e| format!("{}: {e}", slot.texture))?;
        let image = remaster::texture(&ktx2, slot.role, &slot.sampler, formats)
            .map_err(|e| format!("{}: {e}", slot.texture))?;
        images.push((key, image));
    }
    let sweep = Sweep::of_idle(&model, SAMPLES)
        .ok_or_else(|| format!("{rel} has no vertex to frame, or one that isn't finite"))?;
    Ok(Loaded {
        model: Arc::new(model),
        images,
        framing: Framing::fit(&sweep),
        took: started.elapsed(),
    })
}

/// `load`'s model off the shelf's extract, checked to be that load file's
/// on that dump.
pub(crate) fn read_model(shelf: &Shelf, load: &str) -> Result<Model, String> {
    let rel = shelf.ir.model_path(load);
    let model: Model = read_ir(shelf.ir, &shelf.dir.join(&rel))?;
    if model.identity != shelf.identity {
        return Err(format!("{rel} names another dump"));
    }
    if model.load_file != load {
        return Err(format!("{rel} holds {}'s model", model.load_file));
    }
    Ok(model)
}

/// `load`'s hero side off the shelf's extract (AG.14: its locomotion clips,
/// solid and disc movement), checked to be that load file's on that dump.
pub(crate) fn read_hero(shelf: &Shelf, load: &str) -> Result<guac_ir::hero::Hero, String> {
    let rel = shelf.ir.hero_path(load);
    let hero: guac_ir::hero::Hero = read_ir(shelf.ir, &shelf.dir.join(&rel))?;
    if hero.identity != shelf.identity {
        return Err(format!("{rel} names another dump"));
    }
    if hero.load_file != load {
        return Err(format!("{rel} holds {}'s hero side", hero.load_file));
    }
    Ok(hero)
}

/// A relative path of plain names, so a model can't send the loader outside
/// its extract.
pub(crate) fn plain(rel: &str) -> bool {
    let path = Path::new(rel);
    !rel.is_empty() && path.components().all(|c| matches!(c, PathPart::Normal(_)))
}

/// `path` out of the IR's envelope, or why not.
pub(crate) fn read_ir<T: DeserializeOwned>(ir: IrFormat, path: &Path) -> Result<T, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    match ir {
        IrFormat::Ron => {
            let text = std::str::from_utf8(&bytes).map_err(|e| format!("{name}: {e}"))?;
            guac_ir::from_ron(text).map_err(|e| format!("{name}: {e}"))
        }
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| format!("{name}: {e}")),
    }
}

/// Everything `poll_loads` spawns with.
#[derive(bevy::ecs::system::SystemParam)]
struct Spawner<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    materials: ResMut<'w, Assets<CharacterMaterial>>,
    theme: Option<Res<'w, TvTheme>>,
    plate: Res<'w, PlateSize>,
    time: Res<'w, Time>,
}

/// A finished load becomes the slot's scene once the pipelines it draws
/// with are compiled (the warm draws any that aren't first), or its
/// portrait with why.
fn poll_loads(mut turntables: ResMut<Turntables>, mut warm: ResMut<Warm>, mut spawner: Spawner) {
    let Turntables { slots, shelf } = &mut *turntables;
    for (i, slot) in slots.iter_mut().enumerate() {
        let Some(t) = slot.as_mut() else { continue };
        let (result, waited) = match &mut t.state {
            State::Loading(task) => match check_ready(task) {
                Some(result) => (result, None),
                None => continue,
            },
            State::Compiling { keys, .. } if warm.covers(keys.iter()) => {
                match std::mem::replace(&mut t.state, State::NoShelf { extracting: false }) {
                    State::Compiling { loaded, since, .. } => (Ok(*loaded), Some(since.elapsed())),
                    _ => unreachable!("matched as compiling"),
                }
            }
            _ => continue,
        };
        let p = &t.placement;
        let scene = shelf
            .as_ref()
            .ok_or_else(|| "the shelf went away".to_string())
            .and_then(|s| s.rig.clone())
            .and_then(|rig| rig_scene(&rig).map_err(|e| e.to_string()));
        // What it draws with: a key the warm hasn't compiled goes to the
        // front of its queue and the model waits for it. A model whose
        // keys don't resolve fails in `spawn_stage` with why.
        if let (Ok(loaded), Ok(scene)) = (&result, &scene)
            && let Ok(keys) = warm::keys(&loaded.model, scene)
            && !warm.covers(keys.iter().map(|(k, _)| k))
        {
            warm.ask(&loaded.model, &keys, scene);
            info!(
                slot = i + 1,
                figure = %p.name,
                keys = keys.len(),
                "the turntable waits for its pipelines to compile"
            );
            let Ok(loaded) = result else {
                unreachable!("matched as read")
            };
            t.state = State::Compiling {
                loaded: Box::new(loaded),
                keys: keys.into_iter().map(|(k, _)| k).collect(),
                since: Instant::now(),
            };
            continue;
        }
        let spawned = result.and_then(|loaded| {
            let took = loaded.took;
            let load_file = loaded.model.load_file.clone();
            scene
                .and_then(|scene| spawn_stage(&mut spawner, i, p, loaded, &scene))
                .map(|stage| (stage, took, load_file))
        });
        t.state = match spawned {
            Ok((stage, took, load_file)) => {
                info!(
                    slot = i + 1,
                    figure = %p.name,
                    load_file,
                    read_ms = took.as_millis() as u64,
                    compiling_ms = waited.map(|w| w.as_millis() as u64),
                    size = %stage.size,
                    "turntable up"
                );
                State::Model(stage)
            }
            Err(why) => {
                warn!(slot = i + 1, figure = %p.name, id = p.figure_id.as_str(), reason = %why,
                    "the turntable can't show this model; the card keeps its portrait");
                State::Portrait(why)
            }
        };
    }
}

/// The model under a root at rest, its materials in the rig's scene, its
/// glow masks, and the camera.
fn spawn_stage(
    s: &mut Spawner,
    slot: usize,
    p: &Placement,
    loaded: Loaded,
    scene: &Scene,
) -> Result<Stage, String> {
    let Loaded {
        model,
        images,
        framing,
        ..
    } = loaded;
    let handles: Vec<((String, bool, Sampler), Handle<Image>)> = images
        .into_iter()
        .map(|(k, image)| (k, s.images.add(image)))
        .collect();
    let texture = |t: &guac_ir::model::TextureSlot| -> Handle<Image> {
        let key = (t.texture.clone(), remaster::is_srgb(t.role), t.sampler);
        handles
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, h)| h.clone())
            .unwrap_or_default()
    };
    // The body's materials, then what it carries (AE.23).
    let drawn = attach::drawn(&model);
    let mut colour = Vec::with_capacity(drawn.materials.len());
    let mut glow = Vec::with_capacity(drawn.materials.len());
    let mut scrolling = Vec::new();
    for m in &drawn.materials {
        let material = CharacterMaterial::from_ir(m, scene, ShadowLookup::NONE, texture)
            .map_err(|e| format!("material {}: {e}", m.name))?;
        let mask = material.glow_mask().map(|g| s.materials.add(g));
        let part = s.materials.add(material);
        if !m.scrolls.is_empty() {
            scrolling.push(Scrolling {
                scrolls: m.scrolls.clone(),
                materials: std::iter::once(part.clone()).chain(mask.clone()).collect(),
            });
        }
        glow.push(mask);
        colour.push(part);
    }
    let spawned = spawn_character(
        &mut s.commands,
        &model,
        Transform::IDENTITY,
        0.0,
        &mut s.meshes,
        &mut s.bindposes,
        &colour,
    )
    .map_err(|e| e.to_string())?;
    for segment in &spawned.segments {
        s.commands
            .entity(*segment)
            .insert(RenderLayers::layer(model_layer(slot)));
    }
    spawn_glow_masks(&mut s.commands, &model, &spawned, &glow, glow_layer(slot));
    let attachments = attach::spawn_attachments(
        &mut s.commands,
        &model,
        &drawn,
        spawned.root,
        &spawned.joints,
        0.0,
        &mut s.meshes,
        &mut s.bindposes,
        &colour,
    )
    .map_err(|e| e.to_string())?;
    for part in &attachments.parts {
        s.commands
            .entity(part.entity)
            .insert(RenderLayers::layer(model_layer(slot)));
    }
    attach::spawn_glow_masks(&mut s.commands, &attachments, &glow, glow_layer(slot));
    let backdrop = match &s.theme {
        Some(theme) => theme.element(p.element).unwrap_or(theme.plate)[1],
        None => Color::BLACK,
    };
    let size = s.plate.0;
    let (camera, image) = spawn_camera(
        &mut s.commands,
        &mut s.images,
        slot,
        size,
        &framing,
        backdrop,
    );
    Ok(Stage {
        root: spawned.root,
        joints: spawned.joints,
        camera,
        image,
        size,
        model,
        attachments,
        framing,
        backdrop,
        since: s.time.elapsed(),
        scrolling,
    })
}

/// The remaster's camera at `framing` drawing `slot`'s layers (a card's,
/// or [`WARM_SLOT`]) into a new image `size` big, with its glow camera.
pub(crate) fn spawn_camera(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    slot: usize,
    size: UVec2,
    framing: &Framing,
    backdrop: Color,
) -> (Entity, Handle<Image>) {
    let image = images.add(remaster::target(size));
    let projection = Projection::Perspective(framing.projection());
    let name = match slot {
        WARM_SLOT => "turntable warm".to_string(),
        _ => format!("turntable {}", slot + 1),
    };
    let camera = commands
        .spawn((
            remaster::camera(),
            Camera {
                order: CAMERA_ORDER,
                clear_color: ClearColorConfig::Custom(backdrop),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            projection.clone(),
            framing.transform(),
            RenderLayers::layer(model_layer(slot)),
            Name::new(name),
        ))
        .id();
    spawn_glow_camera(
        commands,
        images,
        camera,
        GlowView {
            size,
            projection,
            msaa: remaster::MSAA,
            order: GLOW_ORDER,
        },
        glow_layer(slot),
    );
    (camera, image)
}

/// Marks the bezel plate a card's portrait, or its turntable, shows in.
#[derive(Component, Debug, Clone)]
pub(crate) struct CardPlate {
    pub(crate) slot: u8,
    pub(crate) portrait: Option<Handle<Image>>,
}

/// The plates' size in pixels: the largest laid-out plate's, so no card
/// draws its image scaled up. Plates of one size lay out a pixel apart by
/// where they sit (85.33 px at 720p rounds to 85 or 86), so the first the
/// query yields, which follows the order the cards' models came up in,
/// isn't one size every run.
fn follow_plates(plates: Query<&ComputedNode, With<CardPlate>>, mut size: ResMut<PlateSize>) {
    let Some(laid) = plates
        .iter()
        .map(|n| n.size().ceil().as_uvec2())
        .filter(|s| s.x > 0 && s.y > 0)
        .map(|s| s.x.max(s.y))
        .max()
    else {
        return;
    };
    let square = UVec2::splat(laid);
    if size.0 != square {
        size.0 = square;
    }
}

/// A scene drawn at another size gets a new camera and image at the plate's.
fn resize_targets(mut turntables: ResMut<Turntables>, mut spawner: Spawner) {
    let size = spawner.plate.0;
    for (i, slot) in turntables.slots.iter_mut().enumerate() {
        let Some(Turntable {
            state: State::Model(stage),
            ..
        }) = slot.as_mut()
        else {
            continue;
        };
        if stage.size == size {
            continue;
        }
        spawner.commands.entity(stage.camera).despawn();
        let (camera, image) = spawn_camera(
            &mut spawner.commands,
            &mut spawner.images,
            i,
            size,
            &stage.framing,
            stage.backdrop,
        );
        (stage.camera, stage.image, stage.size) = (camera, image, size);
    }
}

/// Each model turns, plays its idle and slides its scrolling parts; under
/// reduced motion it holds the idle's first pose and the scrolls' starts at
/// the frozen turn.
fn turn(
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    frozen: Res<FrozenYaw>,
    turntables: Res<Turntables>,
    mut transforms: Query<&mut Transform>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    for t in turntables.slots.iter().flatten() {
        let State::Model(stage) = &t.state else {
            continue;
        };
        let (seconds, yaw) = if reduced.0 {
            (0.0, frozen.0)
        } else {
            let run = time.elapsed().saturating_sub(stage.since).as_secs_f64();
            let turns = (run / f64::from(TURN_SECONDS)).fract() as f32;
            (run, turns * std::f32::consts::TAU)
        };
        if let Ok(mut root) = transforms.get_mut(stage.root) {
            let rotation = Quat::from_rotation_y(yaw);
            if root.rotation != rotation {
                root.rotation = rotation;
            }
        }
        let pose = guacamole::character::mesh::pose(&stage.model, seconds);
        let carried = stage
            .model
            .attachments
            .iter()
            .zip(&stage.attachments.joints)
            .filter(|(_, joints)| !joints.is_empty())
            .flat_map(|(a, joints)| joints.iter().zip(attach::attachment_pose(a, seconds)));
        for (joint, p) in stage.joints.iter().zip(pose).chain(carried) {
            if let Ok(mut tr) = transforms.get_mut(*joint)
                && *tr != p
            {
                *tr = p;
            }
        }
        for part in &stage.scrolling {
            let rows = CharacterUniform::uv_rows(&scroll::transforms(&part.scrolls, seconds));
            for h in &part.materials {
                // Read first: a write marks the material for upload again.
                if let Some(mut m) = materials.get_mut(h)
                    && (m.uniform.uv_u, m.uniform.uv_v) != rows
                {
                    (m.uniform.uv_u, m.uniform.uv_v) = rows;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::PortalChange;
    use crate::test_support::turntable::{
        add_turntable, app, extract, identity, model, placed, rig, set, settle, states,
    };
    use bevy::render::render_resource::TextureFormat;

    use guac_ir::Sha256;
    use skylander_core::SlotIndex;

    fn alive(app: &App, e: Entity) -> bool {
        app.world().get_entity(e).is_ok()
    }

    #[test]
    fn a_placement_spawns_a_scene_a_new_one_replaces_it_and_a_clear_removes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        set(&mut app, [placed(0, 1, "One")]);
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
        let t = app.world().resource::<Turntables>();
        let (root, camera) = t.stage_entities(0).unwrap();
        let image = t.image(0).unwrap();
        // The camera draws the slot's own layer into the slot's image.
        let world = app.world();
        assert_eq!(
            world.get::<RenderLayers>(camera),
            Some(&RenderLayers::layer(model_layer(0)))
        );
        match world.get::<RenderTarget>(camera) {
            Some(RenderTarget::Image(target)) => assert_eq!(target.handle, image),
            other => panic!("{other:?}"),
        }
        let target = world.resource::<Assets<Image>>().get(&image).unwrap();
        assert_eq!(target.size(), PlateSize::default().0);
        assert_eq!(
            target.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        // Its segment is on that layer too, under the root.
        let mut q = app
            .world_mut()
            .query::<(&RenderLayers, &ChildOf, &MeshMaterial3d<CharacterMaterial>)>();
        let on: Vec<_> = q
            .iter(app.world())
            .filter(|(l, c, _)| c.parent() == root && **l == RenderLayers::layer(model_layer(0)))
            .collect();
        assert_eq!(on.len(), 1);

        // Another figure in the slot: the old scene goes, a new one comes.
        set(&mut app, [placed(0, 2, "Two")]);
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
        let (root2, camera2) = app
            .world()
            .resource::<Turntables>()
            .stage_entities(0)
            .unwrap();
        assert!(root2 != root && !alive(&app, root) && !alive(&app, camera));
        assert!(alive(&app, root2) && alive(&app, camera2));

        // The same figure placed again by another profile is a new placement
        // (the key is the placement, not the figure): its scene starts over.
        let mut again = placed(0, 2, "Two");
        if let PortalChange::Placed { placement, .. } = &mut again {
            placement.owner = Some(skylander_rpcs3_control::PlacementOwner {
                profile_id: "p2".into(),
                initial: "B".into(),
                color: "#3366ff".into(),
                progress: Default::default(),
            });
        }
        set(&mut app, [again]);
        settle(&mut app);
        let (root3, camera3) = app
            .world()
            .resource::<Turntables>()
            .stage_entities(0)
            .unwrap();
        assert!(root3 != root2 && !alive(&app, root2) && !alive(&app, camera2));
        let (root2, camera2) = (root3, camera3);

        // Cleared: nothing of it left.
        set(
            &mut app,
            [PortalChange::Cleared {
                slot: SlotIndex::new(0).unwrap(),
            }],
        );
        app.update();
        app.update();
        assert_eq!(states(&app)[0], None);
        assert!(!alive(&app, root2) && !alive(&app, camera2));
        let mut q = app.world_mut().query::<&Camera>();
        assert_eq!(q.iter(app.world()).count(), 0, "both cameras gone");
    }

    #[test]
    fn a_model_that_cant_be_shown_keeps_its_portrait_and_leaves_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        set(&mut app, [placed(0, 1, "One"), placed(1, 3, "Three")]);
        settle(&mut app);
        let (root, camera) = app
            .world()
            .resource::<Turntables>()
            .stage_entities(0)
            .unwrap();
        // The damaged one, a failed extraction, an item: portraits, each why.
        set(&mut app, [placed(2, 4, "Four"), placed(3, 5, "Five")]);
        settle(&mut app);
        assert_eq!(
            states(&app)[..4],
            [
                Some("model"),
                Some("portrait"),
                Some("portrait"),
                Some("portrait")
            ]
        );
        let t = app.world().resource::<Turntables>();
        assert!(
            t.portrait_reason(1).unwrap().starts_with("003_three.ron:"),
            "{:?}",
            t.portrait_reason(1)
        );
        assert!(t.portrait_reason(2).unwrap().contains("made up"));
        assert_eq!(t.portrait_reason(3), Some("no character model on the disc"));
        assert_eq!(t.image(1), None);
        // Slot 1 never noticed.
        assert_eq!(t.stage_entities(0), Some((root, camera)));
        assert!(alive(&app, root) && alive(&app, camera));
    }

    #[test]
    fn without_a_shelf_a_card_loads_while_an_extraction_runs_and_starts_when_one_lands() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = extract(tmp.path());
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        add_turntable(&mut app);
        let (tx, feed) = shelf_channel();
        app.init_resource::<PortalCards>().insert_resource(feed);
        set(&mut app, [placed(0, 1, "One")]);
        app.update();
        // Nothing extracting: the portrait.
        assert_eq!(states(&app)[0], Some("portrait"));
        // An extraction under way: loading.
        let (_running, feed) = crossbeam_channel::unbounded();
        app.insert_resource(Extraction {
            feed: Some(feed),
            ..Default::default()
        });
        set(&mut app, [placed(0, 2, "Two")]);
        app.update();
        assert_eq!(states(&app)[0], Some("loading"));
        // The models land: it reads its own.
        tx.send(Arc::new(shelf)).unwrap();
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
    }

    #[test]
    fn what_a_figure_carries_hangs_on_its_bone_through_the_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        set(&mut app, [placed(2, 2, "Two")]);
        settle(&mut app);
        assert_eq!(states(&app)[2], Some("model"));
        let (joints, roots, parts) = app.world().resource::<Turntables>().carried(2).unwrap();
        assert_eq!((roots.len(), parts.len()), (2, 2));
        // Each part on the slot's layer, the knife under the top joint and
        // the jewel under the knife.
        let world = app.world();
        for p in &parts {
            assert_eq!(
                world.get::<RenderLayers>(*p),
                Some(&RenderLayers::layer(model_layer(2)))
            );
        }
        assert_eq!(world.get::<ChildOf>(roots[0]).unwrap().parent(), joints[1]);
        assert_eq!(world.get::<ChildOf>(roots[1]).unwrap().parent(), roots[0]);
        // As the top joint slides through the idle, the knife holds its
        // offset from it.
        let m = model("002_two");
        let knife =
            guacamole::character::mesh::joint_transform(&m.attachments[0].offset).to_matrix();
        let at = |app: &App| {
            let w = app.world();
            let joint = w.get::<GlobalTransform>(joints[1]).unwrap().to_matrix();
            let root = w.get::<GlobalTransform>(roots[0]).unwrap().to_matrix();
            (joint, root)
        };
        // The joint's own transform, under the body: only the idle moves it
        // (the turn turns the stage above it).
        let posed = |app: &App| app.world().get::<Transform>(joints[1]).copied().unwrap();
        let (j0, r0) = at(&app);
        let p0 = posed(&app);
        assert!(r0.abs_diff_eq(j0 * knife, 1e-5));
        std::thread::sleep(Duration::from_millis(300));
        app.update();
        let (j1, r1) = at(&app);
        let p1 = posed(&app);
        assert!(
            p1.translation.distance(p0.translation) > 1e-3,
            "the top joint slid through the idle: {} then {}",
            p0.translation,
            p1.translation
        );
        assert!(r1.abs_diff_eq(j1 * knife, 1e-5));
        assert!(!r1.abs_diff_eq(r0, 1e-4), "the knife moved with the joint");
    }

    /// A key from the environment, else the workspace's `.env.dev`.
    fn env_or_dotenv(key: &str) -> Option<String> {
        if let Ok(value) = std::env::var(key) {
            return Some(value);
        }
        let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
        let text = std::fs::read_to_string(dotenv).ok()?;
        text.lines().find_map(|line| {
            let (k, v) = line.trim().split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
        })
    }

    /// AE.23 on the real disc: every figure with a character model comes up
    /// on a turntable (eight at a time, headless, no renderer), framed to a
    /// finite camera that holds what it carries, every segment of its body
    /// and of what it carries drawn on its slot's layer, none on its
    /// portrait. The extract is written to a throwaway folder under
    /// `GUAC_EXTRACT_DIR`, outside the tree, and removed after.
    ///
    /// `cargo test -p guacamole --bin guacamole every_figures_turntable --
    /// --ignored --nocapture`
    #[test]
    #[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and writes under GUAC_EXTRACT_DIR"]
    fn every_figures_turntable_loads_frames_and_draws() {
        use guac_extract::known::KNOWN_DUMPS;
        use guac_extract::{models, roster};
        let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
        let root = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
        let tmp = tempfile::Builder::new()
            .prefix("ae23-turntables-")
            .tempdir_in(&root)
            .unwrap();
        let dir = tmp.path();
        let known = &KNOWN_DUMPS[0];
        let identity = DumpIdentity {
            title_id: known.title_id.into(),
            app_ver: known.app_ver.into(),
            manifest_hash: Sha256([0; 32]),
        };
        let started = Instant::now();
        let roster = roster::read(&dump, identity.clone()).unwrap();
        let usrdir = dump.join("PS3_GAME/USRDIR");
        let level_of = |archive: &str| {
            let bytes = std::fs::read(usrdir.join(archive)).unwrap();
            models::level(&bytes, archive).unwrap()
        };
        let (global, bootstrap) = (level_of(roster::GLOBAL), level_of(roster::BOOTSTRAP));
        let globals = models::Globals::new(&global, &bootstrap).unwrap();
        let ir = IrFormat::Postcard;
        for sub in ["models", "textures", "ir"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let mut read: HashMap<String, Model> = HashMap::new();
        for load in models::character_loads(&roster) {
            let level = models::archive(&models::archive_path(&usrdir, load), load)
                .and_then(|bytes| models::level(&bytes, load))
                .unwrap();
            let m = models::read(&level, load, identity.clone(), Some(&globals)).unwrap();
            for (rel, ktx2) in &m.textures {
                std::fs::write(dir.join(rel), ktx2).unwrap();
            }
            std::fs::write(
                dir.join(ir.model_path(load)),
                guac_ir::to_postcard(&m.model).unwrap(),
            )
            .unwrap();
            read.insert(load.to_owned(), m.model);
        }
        let rig = models::rig(&global, identity.clone()).unwrap();
        std::fs::write(dir.join(ir.rig_path()), guac_ir::to_postcard(&rig).unwrap()).unwrap();
        let extracted = started.elapsed();
        let shelf = Shelf::new(&roster, dir, ir, &[]);
        let mut figures: Vec<&guac_ir::RosterFigure> = roster
            .figures
            .iter()
            .filter(|f| shelf.loads.contains_key(&f.id()))
            .collect();
        figures.sort_by_key(|f| f.id());
        let mut app = app(shelf);
        let started = Instant::now();
        let (mut up, mut carried_parts, mut body_parts) = (0, 0, 0);
        let mut slowest = Duration::ZERO;
        for batch in figures.chunks(SLOT_COUNT) {
            let changes = batch.iter().enumerate().map(|(i, f)| {
                let figure_id = skylander_core::FigureId::new(f.id());
                let (toy_type, variant) = figure_id.subtype_parts().unwrap();
                PortalChange::Placed {
                    slot: SlotIndex::new(i as u8).unwrap(),
                    placement: Placement {
                        figure_id,
                        toy_type,
                        variant,
                        name: f.load_file.clone(),
                        element: None,
                        portrait: None,
                        owner: None,
                    },
                }
            });
            let at = Instant::now();
            set(&mut app, changes);
            let until = Instant::now() + Duration::from_secs(120);
            loop {
                app.update();
                let t = app.world().resource::<Turntables>();
                let waiting = (0..batch.len())
                    .any(|s| matches!(t.card_state(s as u8), Some(CardState::Loading)));
                if !waiting {
                    app.update();
                    break;
                }
                assert!(Instant::now() < until, "still loading");
            }
            slowest = slowest.max(at.elapsed());
            for (i, f) in batch.iter().enumerate() {
                let slot = i as u8;
                let t = app.world().resource::<Turntables>();
                assert_eq!(
                    t.card_state(slot),
                    Some(CardState::Model),
                    "{} ({}): {:?}",
                    f.id(),
                    f.load_file,
                    t.portrait_reason(slot)
                );
                let (root, camera) = t.stage_entities(slot).unwrap();
                let (_, roots, parts) = t.carried(slot).unwrap();
                let m = &read[&f.load_file];
                let drawn = attach::drawn(m);
                let carried: usize = attach::segments(m, &drawn).iter().map(Vec::len).sum();
                assert_eq!(roots.len(), m.attachments.len(), "{}", f.id());
                assert_eq!(parts.len(), carried, "{}", f.id());
                // Every body and carried segment on the slot's layer.
                let world = app.world_mut();
                let layer = RenderLayers::layer(model_layer(i));
                let on = world
                    .query::<(&Mesh3d, &RenderLayers)>()
                    .iter(world)
                    .filter(|(_, l)| **l == layer)
                    .count();
                assert_eq!(on, m.segments.len() + carried, "{}", f.id());
                // A finite camera, framed on a sweep holding what it carries.
                let eye = world.get::<Transform>(camera).unwrap().translation;
                assert!(eye.is_finite(), "{}", f.id());
                assert!(world.get::<Transform>(root).is_some());
                let sweep = Sweep::of_idle(m, SAMPLES).unwrap();
                for p in attach::points(m, 0.0) {
                    assert!(p.x.hypot(p.z) <= sweep.radius + 1e-4, "{}", f.id());
                }
                up += 1;
                carried_parts += carried;
                body_parts += m.segments.len();
            }
            set(
                &mut app,
                (0..batch.len()).map(|i| PortalChange::Cleared {
                    slot: SlotIndex::new(i as u8).unwrap(),
                }),
            );
            app.update();
            app.update();
        }
        println!(
            "{} figures over {} load files (extract {:.1} s): {up} up on a turntable, {body_parts} body and {carried_parts} carried segments drawn, every batch of eight up within {:.2} s, {:.1} s in all",
            figures.len(),
            read.len(),
            extracted.as_secs_f64(),
            slowest.as_secs_f64(),
            started.elapsed().as_secs_f64()
        );
        assert_eq!((figures.len(), up), (99, 99));
    }

    #[test]
    fn the_model_turns_and_holds_still_under_reduced_motion() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        set(&mut app, [placed(0, 1, "One")]);
        settle(&mut app);
        let (root, _) = app
            .world()
            .resource::<Turntables>()
            .stage_entities(0)
            .unwrap();
        let yaw = |app: &App| {
            let r = app.world().get::<Transform>(root).unwrap().rotation;
            r.to_euler(EulerRot::YXZ).0
        };
        let before = yaw(&app);
        std::thread::sleep(Duration::from_millis(50));
        app.update();
        assert!(yaw(&app) != before, "it turns");
        app.insert_resource(ReducedMotion(true))
            .insert_resource(FrozenYaw(0.5));
        app.update();
        let held = yaw(&app);
        assert!((held - 0.5).abs() < 1e-6, "{held}");
        std::thread::sleep(Duration::from_millis(50));
        app.update();
        assert_eq!(yaw(&app), held);
    }

    #[test]
    fn a_scrolling_part_slides_its_coordinates_and_holds_its_start_under_reduced_motion() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = extract(tmp.path());
        // One's colour map slides up v at a texture every 4 s, as Ninjini's
        // genie does at 0.77.
        let mut m = model("001_one");
        m.materials[0].scrolls = vec![Scroll {
            layer: 0,
            scale: [1.0, 1.0],
            velocity: [0.0, 0.25],
            amplitude: [0.0, 0.0],
            frequency: [0.0, 0.0],
            phase: [0.0, 0.0],
            rotation_phase: 0.0,
        }];
        let path = tmp.path().join(IrFormat::Ron.model_path("001_one"));
        std::fs::write(path, guac_ir::to_ron(&m).unwrap()).unwrap();
        let mut app = app(shelf);
        set(&mut app, [placed(0, 1, "One")]);
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
        // Its colour material and its glow mask (glow 64), both moving.
        let offsets = |app: &App| -> Vec<Vec4> {
            let mats = app.world().resource::<Assets<CharacterMaterial>>();
            mats.iter().map(|(_, m)| m.uniform.uv_v[0]).collect()
        };
        let before = offsets(&app);
        assert_eq!(before.len(), 2);
        std::thread::sleep(Duration::from_millis(50));
        app.update();
        let after = offsets(&app);
        for (b, a) in before.iter().zip(&after) {
            assert!(a.z > b.z && a.z < 1.0, "{b} -> {a}");
            assert_eq!((a.x, a.y, a.w), (0.0, 1.0, 0.0));
        }
        assert_eq!(after[0], after[1]);
        // Every other set, and u, as stored.
        let mats = app.world().resource::<Assets<CharacterMaterial>>();
        for (_, m) in mats.iter() {
            assert_eq!(m.uniform.uv_u[0], Vec4::X);
            assert_eq!(&m.uniform.uv_u[1..], [Vec4::X; 2]);
            assert_eq!(&m.uniform.uv_v[1..], [Vec4::Y; 2]);
        }
        // Held at its start, and still there a moment later.
        app.insert_resource(ReducedMotion(true));
        app.update();
        assert_eq!(offsets(&app), [Vec4::Y; 2]);
        std::thread::sleep(Duration::from_millis(50));
        app.update();
        assert_eq!(offsets(&app), [Vec4::Y; 2]);
    }

    #[test]
    fn a_shelf_maps_character_figures_to_their_load_files() {
        let tmp = tempfile::tempdir().unwrap();
        let shelf = extract(tmp.path());
        assert_eq!(shelf.loads.len(), 4, "the item has none");
        assert_eq!(shelf.loads["000002-0000"], "002_two");
        assert_eq!(shelf.failed["004_four"], "GUAC-E-MODEL-UNREADABLE: made up");
        assert_eq!(shelf.rig.as_ref().unwrap().lights.len(), 3);
        // A rig of another dump isn't taken.
        let mut other = rig();
        other.identity.app_ver = "01.01".into();
        std::fs::write(
            tmp.path().join("ir/rig.ron"),
            guac_ir::to_ron(&other).unwrap(),
        )
        .unwrap();
        let roster = Roster {
            identity: identity(),
            languages: Vec::new(),
            figures: Vec::new(),
        };
        let shelf = Shelf::new(&roster, tmp.path(), IrFormat::Ron, &[]);
        assert_eq!(shelf.rig.unwrap_err(), "ir/rig.ron names another dump");
        assert!(!plain("../x.ktx2") && !plain("/x") && !plain("") && plain("textures/a.ktx2"));
    }

    /// What a scene leaves in the world: entities, cameras, and the images,
    /// meshes, skins and materials it added, once Bevy has let go of what
    /// nothing holds. It frees an asset in the `PreUpdate` after its last
    /// handle drops, each type's tracking unordered beside the others', so
    /// a material's textures can go a frame after the material: a settle
    /// of two updates (the loads landing in the frame that began them)
    /// still counted a replaced scene's textures. Freed here instead, the
    /// materials first.
    fn footprint(app: &mut App) -> [usize; 6] {
        let world = app.world_mut();
        world
            .run_system_cached(Assets::<CharacterMaterial>::track_assets)
            .unwrap();
        world
            .run_system_cached(Assets::<Image>::track_assets)
            .unwrap();
        world
            .run_system_cached(Assets::<Mesh>::track_assets)
            .unwrap();
        world
            .run_system_cached(Assets::<SkinnedMeshInverseBindposes>::track_assets)
            .unwrap();
        let cameras = world.query::<&Camera>().iter(world).count();
        [
            world.entities().count_spawned() as usize,
            cameras,
            world.resource::<Assets<Image>>().len(),
            world.resource::<Assets<Mesh>>().len(),
            world
                .resource::<Assets<SkinnedMeshInverseBindposes>>()
                .len(),
            world.resource::<Assets<CharacterMaterial>>().len(),
        ]
    }

    /// Review of AE.13: place, replace, resize and remove, three rounds, and
    /// the world ends each round where it started; an image, camera, mesh or
    /// material a scene leaked would grow it by a round each.
    #[test]
    fn place_replace_resize_and_remove_leave_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        settle(&mut app);
        let before = footprint(&mut app);
        for round in 0..3 {
            set(
                &mut app,
                [
                    placed(0, 1, "One"),
                    placed(1, 2, "Two"),
                    placed(2, 3, "Three"),
                ],
            );
            settle(&mut app);
            assert_eq!(
                states(&app)[..3],
                [Some("model"), Some("model"), Some("portrait")]
            );
            let up = footprint(&mut app);
            assert_eq!(up[1], 4, "two scenes, each a camera and its glow camera");
            // Swapped: both scenes start over.
            set(&mut app, [placed(0, 2, "Two"), placed(1, 1, "One")]);
            settle(&mut app);
            assert_eq!(footprint(&mut app), up, "round {round}: a replace");
            // Another plate size: a new camera and image each, the old gone.
            for size in [128, 256] {
                app.insert_resource(PlateSize(UVec2::splat(size)));
                app.update();
                app.update();
                let t = app.world().resource::<Turntables>();
                let image = t.image(0).unwrap();
                let images = app.world().resource::<Assets<Image>>();
                assert_eq!(images.get(&image).unwrap().size(), UVec2::splat(size));
            }
            assert_eq!(footprint(&mut app), up, "round {round}: a resize");
            set(
                &mut app,
                (0..3).map(|s| PortalChange::Cleared {
                    slot: SlotIndex::new(s).unwrap(),
                }),
            );
            for _ in 0..3 {
                app.update();
            }
            assert_eq!(footprint(&mut app), before, "round {round}: cleared");
        }
    }

    /// The images take the largest plate's size whichever plate the query
    /// yields first: two plates of one size laid out a pixel apart, in
    /// both orders.
    #[test]
    fn the_plate_size_is_the_largest_plates_in_either_order() {
        for heights in [[85.0, 86.0], [86.0, 85.0]] {
            let mut world = World::new();
            world.init_resource::<PlateSize>();
            for (slot, height) in heights.into_iter().enumerate() {
                world.spawn((
                    CardPlate {
                        slot: slot as u8,
                        portrait: None,
                    },
                    ComputedNode {
                        size: Vec2::new(85.0, height),
                        ..default()
                    },
                ));
            }
            world.run_system_cached(follow_plates).unwrap();
            assert_eq!(
                world.resource::<PlateSize>().0,
                UVec2::splat(86),
                "{heights:?}"
            );
        }
    }
}
