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
//!   halo. The image is the plate's size in pixels and follows it.
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

/// The bezel plate's size in pixels, which each turntable's image takes.
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

/// A model read off the extract, ready to spawn.
struct Loaded {
    model: Arc<Model>,
    /// Each texture as a role samples it: (extract path, sRGB view, sampler).
    images: Vec<((String, bool, Sampler), Image)>,
    framing: Framing,
    took: Duration,
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
fn load_model(
    shelf: &Shelf,
    load: &str,
    formats: CompressedImageFormats,
) -> Result<Loaded, String> {
    let started = Instant::now();
    let rel = shelf.ir.model_path(load);
    let model = read_model(shelf, load)?;
    let mut images: Vec<((String, bool, Sampler), Image)> = Vec::new();
    for slot in model.materials.iter().flat_map(|m| &m.textures) {
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
    let mut colour = Vec::with_capacity(model.materials.len());
    let mut glow = Vec::with_capacity(model.materials.len());
    let mut scrolling = Vec::new();
    for m in &model.materials {
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

/// The plates' size in pixels, from whichever is laid out.
fn follow_plates(plates: Query<&ComputedNode, With<CardPlate>>, mut size: ResMut<PlateSize>) {
    let Some(laid) = plates
        .iter()
        .map(|n| n.size().ceil().as_uvec2())
        .find(|s| s.x > 0 && s.y > 0)
    else {
        return;
    };
    let square = UVec2::splat(laid.x.max(laid.y));
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
        for (joint, p) in stage.joints.iter().zip(pose) {
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
pub(crate) mod tests {
    use super::*;
    use crate::driver::PortalChange;
    use bevy::render::render_resource::TextureFormat;
    use guac_formats::texture::{Format, Texture};
    use guac_ir::clip::{Clip, RotationTrack, Track, VecTrack};
    use guac_ir::model::{
        self, Blend, Filter, Glow, Mask, Params, RenderStates, Segment, TextureRole, TextureSlot,
        Wrap,
    };
    use guac_ir::{Element, FigureKind, Language, RigLight, RosterFigure, Sha256, Text};
    use skylander_core::{FigureId, SlotIndex};

    pub(crate) fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([7; 32]),
        }
    }

    /// A 4x4 BC1 KTX2, every texel one colour: built here, none of the disc's.
    fn ktx2() -> Vec<u8> {
        Texture {
            format: Format::Bc1RgbaUnorm,
            width: 4,
            height: 4,
            levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
        }
        .to_ktx2()
    }

    /// A made-up model on `load`: a quad on two joints, a colour map alone,
    /// a two-key idle swaying the top joint. `002_two`'s also draws a
    /// blended, two-sided, alpha-tested material with normal and spec maps
    /// on a quad with vertex colours, and its first material on one too, so
    /// the two models draw three keys between them (AE.14's warm list).
    pub(crate) fn model(load: &str) -> Model {
        let joint = |name: &str, parent, y: f32| model::Joint {
            name: name.into(),
            parent,
            reference: model::Transform {
                translation: [0.0, y, 0.0],
                ..model::Transform::IDENTITY
            },
            inverse_bind: Mat4::from_translation(Vec3::new(0.0, -y, 0.0)).to_cols_array_2d(),
        };
        let sampler = Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        };
        let material = model::Material {
            name: "made_up".into(),
            pixel: Mask {
                base: 0,
                ignore: 0x2FF,
            },
            vertex: Mask {
                base: 0,
                ignore: 0x1AFF,
            },
            textures: vec![TextureSlot {
                role: TextureRole::Layer1ColorMap,
                texture: "textures/made_up.ktx2".into(),
                image: "made_up".into(),
                sampler,
            }],
            params: Params {
                fixed_alpha: [-1.0; 4],
                layer1_extra: [1.0, 1.0, 1.0, 0.25],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: None,
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: Glow::Amount(64),
            },
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: false,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        };
        let top = Track {
            joint: 1,
            translation: VecTrack {
                constant: [0.0, 2.0, 0.0],
                spline: Some(guac_ir::clip::VecSpline {
                    channels: [true, false, false],
                    degree: 1,
                    knots: vec![0, 0, 30, 30],
                    points: vec![[0.0; 3], [0.5, 0.0, 0.0]],
                }),
            },
            rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
            scale: VecTrack {
                constant: [1.0; 3],
                spline: None,
            },
        };
        let quad = |material: u16, colours: bool| Segment {
            material,
            source: 1,
            positions: vec![
                [-0.5, 0.0, 0.0],
                [0.5, 0.0, 0.0],
                [-0.5, 2.0, 0.0],
                [0.5, 2.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]),
            uvs: vec![vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]],
            colors: colours.then(|| vec![[200, 128, 64, 255]; 4]),
            joints: vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]],
            weights: vec![[255, 0, 0, 0]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
        };
        let (materials, segments) = if load == "002_two" {
            let mut fins = material.clone();
            fins.name = "made_up_fins".into();
            fins.pixel.base = 0x18;
            fins.vertex.base = 0x18;
            for role in [
                TextureRole::Layer1BaseNormalMap,
                TextureRole::Layer1BaseSpecMap,
            ] {
                fins.textures.push(TextureSlot {
                    role,
                    ..fins.textures[0].clone()
                });
            }
            fins.states = RenderStates {
                blend: Blend::Alpha,
                depth_write: false,
                two_sided: true,
                alpha_test: Some(0.5),
            };
            fins.params.glow = Glow::None;
            (
                vec![material, fins],
                vec![quad(0, false), quad(1, true), quad(0, true)],
            )
        } else {
            (vec![material], vec![quad(0, false)])
        };
        Model {
            identity: identity(),
            load_file: load.into(),
            name: "MadeUp.mdl".into(),
            skeleton: model::Skeleton {
                joints: vec![joint("foot", None, 0.0), joint("top", Some(0), 2.0)],
            },
            materials,
            segments,
            idle: Clip {
                name: "MadeUp_Idle.ma".into(),
                duration: 1.0,
                frame_duration: 1.0 / 30.0,
                frames: 31,
                tracks: vec![top],
            },
        }
    }

    /// Made-up lights at the sprite rig's angles.
    pub(crate) fn rig() -> Rig {
        let light = |name: &str, rotation| RigLight {
            name: name.into(),
            diffuse: [90, 90, 90, 255],
            ambient: [20, 20, 20, 255],
            specular: [60, 60, 60, 255],
            multiplier: 1.0,
            position: [0.0; 3],
            rotation,
        };
        Rig {
            identity: identity(),
            lights: vec![
                light("_spriteLight1", [-45.0, 0.0, 0.0]),
                light("_spriteLight2", [45.0, 180.0, 0.0]),
                light("_spriteLight3", [-45.0, 90.0, 0.0]),
            ],
        }
    }

    fn figure(toy: u32, name: &str, category: Category, load: &str) -> RosterFigure {
        let text = |key: &str, en: &str| Text {
            key: key.into(),
            texts: vec![en.into()],
        };
        RosterFigure {
            toy_type: toy,
            variant: 0,
            kind: FigureKind::Base,
            deco: None,
            name: text("name", name),
            variant_word: None,
            element: Some(Element::Fire),
            category,
            load_file: load.into(),
            portrait: None,
            sky1: false,
        }
    }

    /// An extract holding two good models (toys 1 and 2), one damaged (3),
    /// one that didn't extract (4) and an item (5); its shelf.
    pub(crate) fn extract(dir: &Path) -> Shelf {
        let ir = IrFormat::Ron;
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::create_dir_all(dir.join("ir")).unwrap();
        for load in ["001_one", "002_two"] {
            let ron = guac_ir::to_ron(&model(load)).unwrap();
            std::fs::write(dir.join(ir.model_path(load)), ron).unwrap();
        }
        std::fs::write(dir.join(ir.model_path("003_three")), "not a model").unwrap();
        std::fs::write(dir.join("textures/made_up.ktx2"), ktx2()).unwrap();
        std::fs::write(dir.join(ir.rig_path()), guac_ir::to_ron(&rig()).unwrap()).unwrap();
        let roster = Roster {
            identity: identity(),
            languages: vec![Language {
                id: guac_ir::roster::ENGLISH,
                name: "ENGLISH".into(),
            }],
            figures: vec![
                figure(1, "One", Category::Figure, "001_one"),
                figure(2, "Two", Category::Giant, "002_two"),
                figure(3, "Three", Category::Figure, "003_three"),
                figure(4, "Four", Category::Figure, "004_four"),
                figure(5, "Five", Category::Item, "005_five"),
            ],
        };
        let failed = [ModelFailure {
            load_file: "004_four".into(),
            reason: "GUAC-E-MODEL-UNREADABLE: made up".into(),
        }];
        Shelf::new(&roster, dir, ir, &failed)
    }

    pub(crate) fn placed(slot: u8, toy: u32, name: &str) -> PortalChange {
        let figure_id = FigureId::new(format!("{toy:06x}-0000"));
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        PortalChange::Placed {
            slot: SlotIndex::new(slot).unwrap(),
            placement: Placement {
                figure_id,
                toy_type,
                variant,
                name: name.into(),
                element: Some(skylander_core::Element::Fire),
                portrait: None,
                owner: None,
            },
        }
    }

    /// The assets the turntable spawns into, on an app with `Time`.
    pub(crate) fn add_turntable(app: &mut App) {
        app.init_asset::<Mesh>()
            .init_asset::<SkinnedMeshInverseBindposes>()
            .init_asset::<CharacterMaterial>()
            .init_resource::<ReducedMotion>()
            .add_plugins(TurntablePlugin);
    }

    pub(crate) fn app(shelf: Shelf) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        add_turntable(&mut app);
        let (tx, feed) = shelf_channel();
        tx.send(Arc::new(shelf)).unwrap();
        app.init_resource::<PortalCards>().insert_resource(feed);
        app
    }

    pub(crate) fn set(app: &mut App, changes: impl IntoIterator<Item = PortalChange>) {
        let cards = app
            .world()
            .resource::<PortalCards>()
            .clone()
            .reduce(changes);
        app.insert_resource(cards);
    }

    /// Frames until no slot is loading, up to 5 s.
    pub(crate) fn settle(app: &mut App) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let t = app.world().resource::<Turntables>();
            let loading = (0..8).any(|s| matches!(t.card_state(s as u8), Some(CardState::Loading)));
            if !loading {
                // One more, so what the last poll spawned is in the world.
                app.update();
                return;
            }
            assert!(Instant::now() < until, "still loading");
        }
    }

    pub(crate) fn states(app: &App) -> Vec<Option<&'static str>> {
        let t = app.world().resource::<Turntables>();
        (0..8)
            .map(|s| t.card_state(s).map(CardState::name))
            .collect()
    }

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
    /// meshes, skins and materials it added.
    fn footprint(app: &mut App) -> [usize; 6] {
        let world = app.world_mut();
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
}
