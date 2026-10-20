//! The level on the TV (openspec AG.19, design decision 9; level-scene's
//! "The level loads behind the intro" and "The level never stalls the TV").
//! The arithmetic is the library's ([`guacamole::level_scene`]); this is
//! the loading, off the frame:
//!
//! - The extraction says where a level is ([`LevelSource`]) once the Giants
//!   pick finds its extract current or the level stage has written it; the
//!   level loads from there at once, behind whatever the portal shows.
//! - Reading: on a task, the scene and lights parts, each drawn segment's
//!   mesh, each texture through the remaster's views, the keys it draws.
//! - Uploading: the frame hands the renderer at most [`UPLOAD_BYTES`] of
//!   textures and meshes, then [`MATERIALS_PER_FRAME`] materials, a frame,
//!   so no frame carries the whole level to the GPU; the warm compiles its
//!   keys meanwhile, one a frame on its hidden stage ([`crate::warm`]).
//! - Waiting: the level is spawned hidden and is ready only once the render
//!   world has every texture, mesh and material resident ([`watch_residency`])
//!   and every key compiled, so the frame it first shows uploads and
//!   compiles nothing of its own.
//!
//! Nothing shows it in the product yet (AG.21 and AG.29 take the TV into
//! it). The dev entry does: with `GUAC_LEVEL_VIEW` set in a `dev-tools` build,
//! the level covers the TV once it's ready, drawn at AF's fresh-state camera
//! by the remaster's camera into the TV's own target (the window, or the
//! shots' image).

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, Projection, RenderTarget};
use bevy::image::CompressedImageFormats;
use bevy::pbr::PreparedMaterial;
use bevy::prelude::*;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::mesh::RenderMesh;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::BlendState;
use bevy::render::renderer::RenderDevice;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use crossbeam_channel::{Receiver, Sender};
use guac_extract::intros::IrFormat;
use guac_extract::levels::{LEVELS, LIGHTS, SCENE};
use guac_ir::DumpIdentity;
use guac_ir::model::{self, Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::Scene;
use guacamole::character::warm::WarmKey;
use guacamole::character::{
    CharacterMaterial, CharacterUniform, ShadowLookup, reference_material, remaster, static_mesh,
};
use guacamole::level_scene::{
    self as arithmetic, Drawn, LevelFog, LevelLights, fresh_state_projection,
    fresh_state_transform, level_lights, view_scene,
};
use serde::Serialize;
use tracing::{info, warn};

use crate::extraction::Extraction;
use crate::motion::ReducedMotion;
use crate::turntable::{plain, read_ir};
use crate::tv::{Redrawn, TvCamera};
use crate::warm::Warm;

/// Where a level's extract is: what the extraction sends once the level's
/// parts are in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelSource {
    pub dir: PathBuf,
    pub ir: IrFormat,
    pub identity: DumpIdentity,
    /// The level's archive (`level_213_tutotial`).
    pub level: String,
}

impl LevelSource {
    /// Chapter 1 in the extract at `dir`: the one level this build plays.
    pub fn chapter_1(dir: PathBuf, ir: IrFormat, identity: DumpIdentity) -> Self {
        Self {
            dir,
            ir,
            identity,
            level: LEVELS[0].to_owned(),
        }
    }
}

/// Where the extraction puts a [`LevelSource`] (`ExtractJob::level`).
pub type LevelSender = Sender<LevelSource>;

/// The TV's end of it.
#[derive(Resource)]
pub struct LevelFeed(pub Receiver<LevelSource>);

/// A channel for [`LevelSource`]s: the sender for the extraction's job, the
/// feed for the app.
pub fn level_channel() -> (LevelSender, LevelFeed) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (tx, LevelFeed(rx))
}

/// The render layer the level draws on: clear of the default one the TV's
/// cameras see and of the turntables' (16 up).
pub(crate) const LEVEL_LAYER: usize = 2;
/// The dev view's camera draws after the TV's own (order 0), over it.
const VIEW_ORDER: isize = 1;
/// How it writes over the TV: replacing it. Bevy alpha-blends a second
/// camera on a target by default, and the level's opaque parts write the
/// alpha their maps hold, so the portal would show through them.
const VIEW_OUTPUT: CameraOutputMode = CameraOutputMode::Write {
    blend_state: Some(BlendState::REPLACE),
    clear_color: ClearColorConfig::None,
};
/// Texture and mesh bytes handed to the renderer a frame.
pub(crate) const UPLOAD_BYTES: usize = 16 << 20;
/// Materials made a frame, each a bind group the renderer prepares.
pub(crate) const MATERIALS_PER_FRAME: usize = 64;

/// A texture as a role samples it: (extract path, sRGB view, sampler).
type ImageKey = (String, bool, Sampler);

/// What the reading task hands the frame.
struct Built {
    level: String,
    drawn: Drawn,
    meshes: Vec<Mesh>,
    images: Vec<(ImageKey, Image)>,
    keys: Vec<(WarmKey, usize)>,
    lights: LevelLights,
    /// The sky's bounds, for the log: (min, max).
    sky: Option<(Vec3, Vec3)>,
    read: Duration,
    took: Duration,
}

/// Reads `source`'s level for drawing: its scene and lights parts (each
/// checked to be this dump's and this level's), the drawn segments' meshes,
/// the textures they bind, the keys they draw at the fresh state's camera.
fn read_level(source: &LevelSource, formats: CompressedImageFormats) -> Result<Built, String> {
    let started = Instant::now();
    let part = |p: &str| source.dir.join(source.ir.level_path(&source.level, p));
    let scene: guac_ir::level::Scene = read_ir(source.ir, &part(SCENE))?;
    let lights: guac_ir::level::Lights = read_ir(source.ir, &part(LIGHTS))?;
    for (what, identity, level) in [
        ("scene", &scene.identity, &scene.level),
        ("lights", &lights.identity, &lights.level),
    ] {
        if *identity != source.identity || *level != source.level {
            return Err(format!("the {what} part is another dump's or level's"));
        }
    }
    let read = started.elapsed();
    let lights = level_lights(&lights).map_err(|e| e.to_string())?;
    // Every scene's drawn part in one list, materials numbered across them.
    let mut drawn = Drawn::default();
    for graph in scene.scenes {
        drawn.append(arithmetic::drawn(graph).map_err(|e| e.to_string())?);
    }
    let scene = view_scene(
        &lights,
        &LevelFog::PLACEHOLDER,
        fresh_state_transform().to_matrix().inverse(),
    );
    let keys = arithmetic::keys(&drawn, &scene).map_err(|e| e.to_string())?;
    let mut images: Vec<(ImageKey, Image)> = Vec::new();
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
            return Err(format!(
                "the scene names a texture outside the extract: {}",
                slot.texture
            ));
        }
        let ktx2 = std::fs::read(source.dir.join(&slot.texture))
            .map_err(|e| format!("{}: {e}", slot.texture))?;
        let image = remaster::texture(&ktx2, slot.role, &slot.sampler, formats)
            .map_err(|e| format!("{}: {e}", slot.texture))?;
        images.push((key, image));
    }
    let meshes = drawn
        .segments
        .iter()
        .enumerate()
        .map(|(i, s)| static_mesh(s).map_err(|e| format!("segment {}: {e}", drawn.sources[i])))
        .collect::<Result<Vec<_>, _>>()?;
    let sky = drawn
        .sky
        .iter()
        .flat_map(|&i| {
            drawn.segments[i]
                .positions
                .iter()
                .map(|p| Vec3::from_array(*p))
        })
        .fold(None, |b: Option<(Vec3, Vec3)>, p| {
            Some(b.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))))
        });
    // The meshes hold the vertices now.
    drawn
        .segments
        .iter_mut()
        .for_each(|s| *s = empty(s.material));
    Ok(Built {
        level: source.level.clone(),
        drawn,
        meshes,
        images,
        keys,
        lights,
        sky,
        read,
        took: started.elapsed(),
    })
}

/// A segment with nothing in it but its material.
fn empty(material: u16) -> model::Segment {
    model::Segment {
        material,
        source: 0,
        positions: Vec::new(),
        normals: Vec::new(),
        tangents: None,
        uvs: Vec::new(),
        colors: None,
        joints: Vec::new(),
        weights: Vec::new(),
        indices: Vec::new(),
    }
}

/// The render world's word on the level's assets: what isn't resident yet.
#[derive(Default)]
pub(crate) struct Pending {
    images: HashSet<AssetId<Image>>,
    meshes: HashSet<AssetId<Mesh>>,
    materials: HashSet<AssetId<CharacterMaterial>>,
}

impl Pending {
    fn len(&self) -> usize {
        self.images.len() + self.meshes.len() + self.materials.len()
    }
}

/// Shared by the two worlds: the level's assets the render world hasn't
/// made resident yet, each struck off as it is.
#[derive(Resource, Clone, Default)]
pub(crate) struct LevelWatch(Arc<Mutex<Pending>>);

impl LevelWatch {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Render world, after the frame's prepare: strikes off each level texture
/// that's on the GPU, each mesh that's in the mesh allocator's buffers and
/// each material whose bind group is made (which needs its textures first).
fn watch_residency(
    watch: Res<LevelWatch>,
    images: Res<RenderAssets<GpuImage>>,
    meshes: Res<RenderAssets<RenderMesh>>,
    allocator: Res<MeshAllocator>,
    materials: Res<ErasedRenderAssets<PreparedMaterial>>,
) {
    let mut pending = watch.lock();
    if pending.len() == 0 {
        return;
    }
    pending.images.retain(|id| images.get(*id).is_none());
    pending
        .meshes
        .retain(|id| meshes.get(*id).is_none() || allocator.mesh_vertex_slice(id).is_none());
    pending.materials.retain(|id| materials.get(*id).is_none());
}

/// What `/api/_test/tv` reports of the level.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct LevelReport {
    /// `idle` (no level yet), `reading`, `uploading`, `waiting` (on the
    /// renderer and the warm), `ready`, `shown` (the dev entry) or `failed`.
    pub state: String,
    pub level: Option<String>,
    /// Drawn: segments, materials, textures and the pipeline keys.
    pub segments: usize,
    pub materials: usize,
    pub textures: usize,
    pub keys: usize,
    /// The scene's materials and segments not drawn (AG.24's effects).
    pub left_out: usize,
    pub segments_left_out: usize,
    pub sky_segments: usize,
    /// Textures, meshes and materials not resident yet; keys not compiled.
    pub not_resident: usize,
    pub keys_waiting: usize,
    /// From the source landing: the task's read of the parts, the task
    /// done, everything handed to the renderer, ready.
    pub read_ms: Option<u64>,
    pub built_ms: Option<u64>,
    pub uploaded_ms: Option<u64>,
    pub ready_ms: Option<u64>,
    /// From the Giants pick to ready.
    pub pick_to_ready_ms: Option<u64>,
    pub shown: bool,
    /// What stopped it: the reading task's error, a part's file or the
    /// lights it lacks.
    pub reason: Option<String>,
}

/// A level being handed to the renderer.
struct Uploading {
    built: Box<Built>,
    images: VecDeque<(ImageKey, Image)>,
    meshes: VecDeque<Mesh>,
    image_handles: Vec<(ImageKey, Handle<Image>)>,
    mesh_handles: Vec<Handle<Mesh>>,
    materials: Vec<Handle<CharacterMaterial>>,
}

/// A level the renderer has: its entities (hidden until shown), the
/// materials and what rebuilds their uniforms when the camera moves.
pub(crate) struct Loaded {
    pub(crate) root: Entity,
    materials: Vec<Handle<CharacterMaterial>>,
    /// Each material's spec model and alpha reference, for its uniform;
    /// `None` for one that didn't build.
    reference: Vec<Option<(guac_ir::shading::Material, f32)>>,
    /// The materials with scroll tracks, by index.
    scrolling: Vec<(usize, Vec<Scroll>)>,
    lights: LevelLights,
    pub(crate) keys: Vec<WarmKey>,
    /// The camera the uniforms are lit for, as `view_from_world`.
    lit_for: Mat4,
    camera: Option<Entity>,
    shown_at: Option<Duration>,
}

enum Stage {
    Idle,
    Reading(Task<Result<Built, String>>),
    Uploading(Box<Uploading>),
    Waiting(Box<Loaded>),
    Ready(Box<Loaded>),
    Shown(Box<Loaded>),
    Failed,
}

impl Stage {
    fn name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Reading(_) => "reading",
            Self::Uploading(_) => "uploading",
            Self::Waiting(_) => "waiting",
            Self::Ready(_) => "ready",
            Self::Shown(_) => "shown",
            Self::Failed => "failed",
        }
    }

    fn loaded(&self) -> Option<&Loaded> {
        match self {
            Self::Waiting(l) | Self::Ready(l) | Self::Shown(l) => Some(l),
            _ => None,
        }
    }

    fn loaded_mut(&mut self) -> Option<&mut Loaded> {
        match self {
            Self::Waiting(l) | Self::Ready(l) | Self::Shown(l) => Some(l),
            _ => None,
        }
    }
}

/// The level and how far it's got.
#[derive(Resource)]
pub(crate) struct Level {
    /// A renderer to upload to; without one everything counts as resident.
    gpu: bool,
    source: Option<LevelSource>,
    stage: Stage,
    since: Option<Instant>,
    /// The fog the materials take; a placeholder until AG.37.
    fog: LevelFog,
    report: LevelReport,
}

impl Level {
    pub(crate) fn new(gpu: bool) -> Self {
        Self {
            gpu,
            source: None,
            stage: Stage::Idle,
            since: None,
            fog: LevelFog::PLACEHOLDER,
            report: LevelReport {
                state: "idle".into(),
                ..Default::default()
            },
        }
    }

    pub(crate) fn report(&self) -> &LevelReport {
        &self.report
    }

    /// The level, once the renderer has it.
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> Option<&Loaded> {
        self.stage.loaded()
    }

    fn ms(&self) -> Option<u64> {
        self.since.map(|t| t.elapsed().as_millis() as u64)
    }

    fn fail(&mut self, why: String) {
        warn!(level = ?self.source.as_ref().map(|s| &s.level), reason = %why, "the level didn't load");
        self.report.reason = Some(why);
        self.stage = Stage::Failed;
    }
}

/// `GUAC_LEVEL_VIEW` (`dev-tools` builds): show the level once it's ready.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LevelView(pub(crate) bool);

/// Marks the camera the level is drawn from, the one its materials are lit
/// for ([`light_level`]).
#[derive(Component, Debug)]
pub(crate) struct LevelCamera;

/// The level's systems, after the turntables', whose warm compiles the
/// level's keys.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LevelSystems;

/// The level loader: [`Level`] and its watch in both worlds. Needs the
/// warm ([`crate::turntable::TurntablePlugin`]), `Assets` for images,
/// meshes and [`CharacterMaterial`], and [`ReducedMotion`].
pub(crate) struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        let watch = LevelWatch::default();
        let gpu = app.get_sub_app(RenderApp).is_some();
        let view = cfg!(feature = "dev-tools") && std::env::var_os("GUAC_LEVEL_VIEW").is_some();
        if view {
            info!("GUAC_LEVEL_VIEW: the level shows once it's ready, at AF's fresh-state camera");
        }
        app.insert_resource(Level::new(gpu))
            .insert_resource(LevelView(view))
            .insert_resource(watch.clone())
            .add_systems(
                Update,
                (
                    receive_level,
                    step_level,
                    show_level,
                    light_level,
                    scroll_level,
                )
                    .chain()
                    .in_set(LevelSystems)
                    .after(crate::turntable::TurntableSystems),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(watch)
                .add_systems(Render, watch_residency.in_set(RenderSystems::Cleanup));
        }
    }
}

/// A new source starts the level loading; the same one again changes
/// nothing.
fn receive_level(
    mut commands: Commands,
    feed: Option<Res<LevelFeed>>,
    mut level: ResMut<Level>,
    device: Option<Res<RenderDevice>>,
) {
    let Some(source) = feed.and_then(|f| f.0.try_iter().last()) else {
        return;
    };
    if level.source.as_ref() == Some(&source) {
        return;
    }
    if let Some(old) = level.stage.loaded() {
        commands.entity(old.root).despawn();
        if let Some(camera) = old.camera {
            commands.entity(camera).despawn();
        }
    }
    info!(dir = %source.dir.display(), level = %source.level, "the level loads");
    // Headless (no GPU) the images keep their blocks as given.
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    let task = {
        let source = source.clone();
        AsyncComputeTaskPool::get().spawn(async move { read_level(&source, formats) })
    };
    level.report = LevelReport {
        state: "reading".into(),
        level: Some(source.level.clone()),
        ..Default::default()
    };
    level.source = Some(source);
    level.stage = Stage::Reading(task);
    level.since = Some(Instant::now());
}

/// What [`step_level`] makes and spawns with.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct LevelSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<CharacterMaterial>>,
    watch: Res<'w, LevelWatch>,
    extraction: Option<Res<'w, Extraction>>,
}

/// One step of the load a frame: the task's result taken, a frame's share
/// handed to the renderer, the level spawned hidden once all of it is,
/// ready once the renderer has it all and the warm has its keys.
fn step_level(mut level: ResMut<Level>, mut warm: ResMut<Warm>, mut s: LevelSpawner) {
    let level = &mut *level;
    match std::mem::replace(&mut level.stage, Stage::Idle) {
        Stage::Reading(mut task) => match check_ready(&mut task) {
            None => level.stage = Stage::Reading(task),
            Some(Err(why)) => level.fail(why),
            Some(Ok(built)) => {
                let since = level.since.map(|t| t.elapsed());
                info!(
                    level = %built.level,
                    segments = built.drawn.segments.len(),
                    materials = built.drawn.materials.len(),
                    textures = built.images.len(),
                    keys = built.keys.len(),
                    left_out = built.drawn.left_out_materials,
                    segments_left_out = built.drawn.left_out_segments,
                    sky_segments = built.drawn.sky.len(),
                    sky = ?built.sky,
                    lights_left_out = ?built.lights.left_out,
                    read_ms = built.read.as_millis() as u64,
                    built_ms = built.took.as_millis() as u64,
                    "the level read"
                );
                let r = &mut level.report;
                r.segments = built.drawn.segments.len();
                r.materials = built.drawn.materials.len();
                r.textures = built.images.len();
                r.keys = built.keys.len();
                r.left_out = built.drawn.left_out_materials;
                r.segments_left_out = built.drawn.left_out_segments;
                r.sky_segments = built.drawn.sky.len();
                r.read_ms = Some(built.read.as_millis() as u64);
                r.built_ms = since.map(|d| d.as_millis() as u64);
                // The warm compiles the keys while the frames upload.
                let scene = Arc::new(level_scene(&built.lights, &level.fog));
                let entries: Vec<(WarmKey, model::Material)> = built
                    .keys
                    .iter()
                    .map(|(k, m)| (*k, built.drawn.materials[*m].clone()))
                    .collect();
                warm.ask_level(&entries, scene);
                let mut built = Box::new(built);
                let images = std::mem::take(&mut built.images).into();
                let meshes = std::mem::take(&mut built.meshes).into();
                level.stage = Stage::Uploading(Box::new(Uploading {
                    built,
                    images,
                    meshes,
                    image_handles: Vec::new(),
                    mesh_handles: Vec::new(),
                    materials: Vec::new(),
                }));
            }
        },
        Stage::Uploading(mut up) => {
            if upload(&mut up, level, &mut s) {
                let loaded = spawn_level(*up, &mut s);
                level.report.uploaded_ms = level.ms();
                level.stage = wait(Box::new(loaded), level, &warm, &s);
            } else {
                level.stage = Stage::Uploading(up);
            }
        }
        Stage::Waiting(loaded) => level.stage = wait(loaded, level, &warm, &s),
        other => level.stage = other,
    }
    level.report.state = level.stage.name().into();
}

/// Ready once the renderer has every part resident and the warm every key
/// compiled; waiting, with what's left counted, until then.
fn wait(loaded: Box<Loaded>, level: &mut Level, warm: &Warm, s: &LevelSpawner) -> Stage {
    let not_resident = if level.gpu { s.watch.lock().len() } else { 0 };
    let keys_waiting = loaded
        .keys
        .iter()
        .filter(|k| !warm.covers(std::iter::once(*k)))
        .count();
    level.report.not_resident = not_resident;
    level.report.keys_waiting = keys_waiting;
    if not_resident > 0 || keys_waiting > 0 {
        return Stage::Waiting(loaded);
    }
    level.report.ready_ms = level.ms();
    level.report.pick_to_ready_ms = s
        .extraction
        .as_ref()
        .and_then(|e| e.started)
        .map(|t| t.elapsed().as_millis() as u64);
    info!(
        level = ?level.report.level,
        ready_ms = level.report.ready_ms,
        pick_to_ready_ms = level.report.pick_to_ready_ms,
        "the level is ready"
    );
    Stage::Ready(loaded)
}

/// The scene the warm keys the level's materials in: its lights and fog
/// at the fresh state's camera (only the switches it turns on key a
/// pipeline).
fn level_scene(lights: &LevelLights, fog: &LevelFog) -> Scene {
    view_scene(lights, fog, fresh_state_transform().to_matrix().inverse())
}

/// Hands the renderer this frame's share: textures, then meshes, up to
/// [`UPLOAD_BYTES`] (always at least one), then materials,
/// [`MATERIALS_PER_FRAME`] a frame. True once everything is handed over.
fn upload(up: &mut Uploading, level: &Level, s: &mut LevelSpawner) -> bool {
    let mut bytes = 0;
    while bytes < UPLOAD_BYTES {
        if let Some((key, image)) = up.images.pop_front() {
            bytes += image.data.as_ref().map_or(0, Vec::len);
            let h = s.images.add(image);
            if level.gpu {
                s.watch.lock().images.insert(h.id());
            }
            up.image_handles.push((key, h));
        } else if let Some(mesh) = up.meshes.pop_front() {
            bytes += mesh_bytes(&mesh);
            let h = s.meshes.add(mesh);
            if level.gpu {
                s.watch.lock().meshes.insert(h.id());
            }
            up.mesh_handles.push(h);
        } else {
            break;
        }
    }
    if bytes > 0 {
        return false;
    }
    let scene = level_scene(&up.built.lights, &level.fog);
    let start = up.materials.len();
    let end = (start + MATERIALS_PER_FRAME).min(up.built.drawn.materials.len());
    for m in &up.built.drawn.materials[start..end] {
        let texture = |t: &model::TextureSlot| -> Handle<Image> {
            let key = (t.texture.clone(), remaster::is_srgb(t.role), t.sampler);
            up.image_handles
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, h)| h.clone())
                .unwrap_or_default()
        };
        // The keys were made from these materials in the same scene on the
        // task, so this can't refuse one; a material that does draws as
        // Bevy's default rather than fail the level.
        let material = match CharacterMaterial::from_ir(m, &scene, ShadowLookup::NONE, texture) {
            Ok(material) => s.materials.add(material),
            Err(e) => {
                warn!(material = %m.name, reason = %e, "a level material didn't build");
                Handle::default()
            }
        };
        if level.gpu && material != Handle::default() {
            s.watch.lock().materials.insert(material.id());
        }
        up.materials.push(material);
    }
    up.materials.len() == up.built.drawn.materials.len()
}

/// A mesh's vertex and index bytes, as the renderer counts them.
fn mesh_bytes(mesh: &Mesh) -> usize {
    let vertex: usize = mesh
        .attributes()
        .map(|(a, _)| a.format.size() as usize)
        .sum();
    vertex * mesh.count_vertices() + mesh.indices().map_or(0, |i| i.len() * 2)
}

/// The level's entities, hidden under one root on the level's layer.
fn spawn_level(up: Uploading, s: &mut LevelSpawner) -> Loaded {
    let Uploading {
        built,
        mesh_handles,
        materials,
        ..
    } = up;
    let root = s
        .commands
        .spawn((
            Name::new(format!("level {}", built.level)),
            Transform::IDENTITY,
            Visibility::Hidden,
        ))
        .id();
    for (seg, mesh) in built.drawn.segments.iter().zip(mesh_handles) {
        s.commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(materials[usize::from(seg.material)].clone()),
            Transform::IDENTITY,
            RenderLayers::layer(LEVEL_LAYER),
            ChildOf(root),
        ));
    }
    let reference = built
        .drawn
        .materials
        .iter()
        .map(|m| {
            reference_material(m)
                .ok()
                .map(|r| (r, m.states.alpha_test.unwrap_or(0.0)))
        })
        .collect();
    let scrolling = built
        .drawn
        .materials
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.scrolls.is_empty())
        .map(|(i, m)| (i, m.scrolls.clone()))
        .collect();
    Loaded {
        root,
        materials,
        reference,
        scrolling,
        lights: built.lights,
        keys: built.keys.iter().map(|(k, _)| *k).collect(),
        lit_for: fresh_state_transform().to_matrix().inverse(),
        camera: None,
        shown_at: None,
    }
}

/// The dev entry: once the level is ready, the remaster's camera at AF's
/// fresh-state camera, drawing the level over the TV into its target.
fn show_level(
    mut commands: Commands,
    view: Res<LevelView>,
    mut level: ResMut<Level>,
    tv: Query<Option<&RenderTarget>, With<TvCamera>>,
    time: Res<Time>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    if !view.0 || !matches!(level.stage, Stage::Ready(_)) {
        return;
    }
    let Ok(target) = tv.single() else {
        return;
    };
    let target = target.cloned().unwrap_or_default();
    let Stage::Ready(mut loaded) = std::mem::replace(&mut level.stage, Stage::Idle) else {
        unreachable!("matched as ready")
    };
    let camera = commands
        .spawn((
            remaster::camera(),
            Camera {
                order: VIEW_ORDER,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                output_mode: VIEW_OUTPUT,
                ..default()
            },
            target,
            Projection::Perspective(fresh_state_projection()),
            fresh_state_transform(),
            RenderLayers::layer(LEVEL_LAYER),
            LevelCamera,
            Name::new("level view"),
        ))
        .id();
    commands.entity(loaded.root).insert(Visibility::Inherited);
    loaded.camera = Some(camera);
    loaded.shown_at = Some(time.elapsed());
    level.stage = Stage::Shown(loaded);
    level.report.shown = true;
    level.report.state = "shown".into();
    info!(level = ?level.report.level, "the level shows (GUAC_LEVEL_VIEW)");
    redrawn.write(Redrawn::Level);
}

/// The materials light in view space: when the level's camera moves, every
/// material's uniform is lit again from where it now stands.
fn light_level(
    mut level: ResMut<Level>,
    cameras: Query<&GlobalTransform, (With<LevelCamera>, Changed<GlobalTransform>)>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let fog = level.fog;
    let Some(loaded) = level.stage.loaded_mut() else {
        return;
    };
    let view = camera.to_matrix().inverse();
    if view.abs_diff_eq(loaded.lit_for, 1e-6) {
        return;
    }
    loaded.lit_for = view;
    let scene = view_scene(&loaded.lights, &fog, view);
    for (h, reference) in loaded.materials.iter().zip(&loaded.reference) {
        let Some((reference, alpha)) = reference else {
            continue;
        };
        if let Some(mut m) = materials.get_mut(h) {
            let rows = (m.uniform.uv_u, m.uniform.uv_v);
            m.uniform = CharacterUniform::new(reference, &scene, ShadowLookup::NONE, *alpha);
            (m.uniform.uv_u, m.uniform.uv_v) = rows;
        }
    }
}

/// Parts with a scroll track slide their texture coordinates from the
/// moment the level shows; under reduced motion each holds its start.
fn scroll_level(
    level: Res<Level>,
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let Stage::Shown(loaded) = &level.stage else {
        return;
    };
    let seconds = match (reduced.0, loaded.shown_at) {
        (false, Some(at)) => time.elapsed().saturating_sub(at).as_secs_f64(),
        _ => 0.0,
    };
    for (i, scrolls) in &loaded.scrolling {
        let rows = CharacterUniform::uv_rows(&scroll::transforms(scrolls, seconds));
        // Read first: a write marks the material for upload again.
        if let Some(mut m) = materials.get_mut(&loaded.materials[*i])
            && (m.uniform.uv_u, m.uniform.uv_v) != rows
        {
            (m.uniform.uv_u, m.uniform.uv_v) = rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turntable::tests::{app, extract, identity};
    use crate::warm::PipelineWatch;
    use guac_formats::texture::{Format, Texture};
    use guac_ir::level::{
        EffectMaterial, LevelMaterial, Lights, PlacedLight, SceneGraph, SkyGroup,
    };
    use guac_ir::model::{
        Blend, Filter, Glow, Mask, Params, RenderStates, TextureRole, TextureSlot, Wrap,
    };
    use guac_ir::{RigLight, Sha256};
    use guacamole::character::warm::MeshShape;
    use std::path::Path;

    const LEVEL: &str = "level_213_tutotial";

    fn slot(role: TextureRole, texture: &str) -> TextureSlot {
        TextureSlot {
            role,
            texture: format!("textures/{texture}.ktx2"),
            image: texture.into(),
            sampler: Sampler {
                wrap_u: Wrap::Repeat,
                wrap_v: Wrap::Repeat,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: None,
            },
        }
    }

    /// A made-up tfbmulti2 material on these masks, its maps one texture
    /// each.
    fn multi2(name: &str, pixel: Mask, vertex: Mask, maps: &[TextureRole]) -> model::Material {
        model::Material {
            name: name.into(),
            pixel,
            vertex,
            textures: maps
                .iter()
                .map(|r| slot(*r, &format!("{name}{r:?}")))
                .collect(),
            params: Params {
                fixed_alpha: [-1.0, -1.0, 1.0, -1.0],
                layer1_extra: [1.0, 1.0, 1.0, 0.25],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: Some([0.0, 0.0, 1.0, 0.0]),
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: Glow::None,
            },
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: true,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        }
    }

    fn segment(material: u16, shape: MeshShape, at: f32) -> model::Segment {
        let mut s = shape.triangle(material);
        s.positions = vec![[at, 0.0, 0.0], [at + 1.0, 0.0, 0.0], [at, 0.0, 1.0]];
        s
    }

    fn rig_light(name: &str, rotation: [f32; 3]) -> RigLight {
        RigLight {
            name: name.into(),
            diffuse: [90, 80, 70, 255],
            ambient: [20, 20, 20, 255],
            specular: [60, 60, 60, 255],
            multiplier: 1.0,
            position: [0.0; 3],
            rotation,
        }
    }

    /// Made-up lights at the world rig's angles, and a point light.
    fn lights() -> Lights {
        Lights {
            identity: identity(),
            level: LEVEL.into(),
            directional: vec![
                rig_light("_worldLight1", [-45.0, 0.0, 0.0]),
                rig_light("_worldLight2", [45.0, 180.0, 0.0]),
                rig_light("_worldLight3", [-45.0, 90.0, 0.0]),
                rig_light("_causticLight", [-45.0, 0.0, 0.0]),
            ],
            placed: vec![PlacedLight {
                name: "beam".into(),
                light_type: 1,
                visible: true,
                position: [0.0; 3],
                rotation: [0.0; 3],
                falloff_rate: 0.0,
                cutoff_angle: 0.0,
                attenuation: [0.0, 0.5, 0.0],
                diffuse: [80, 40, 20, 255],
                ambient: [0, 0, 0, 255],
                specular: [0, 0, 0, 255],
                multiplier: 1.0,
            }],
        }
    }

    /// A made-up chapter 1 beside `extract`'s models: a ground layered and
    /// normal-mapped on two mesh shapes, a scrolling sky, and grass, which
    /// isn't drawn; its five textures.
    fn write_level(dir: &Path, lights: &Lights) -> LevelSource {
        let plain = MeshShape {
            uv_sets: 2,
            tangents: true,
            colours: false,
            skinned: false,
        };
        let ground = multi2(
            "ground",
            Mask {
                base: 0x1A,
                ignore: 0x3FF,
            },
            Mask {
                base: 0x101A,
                ignore: 0x1BFF,
            },
            &[
                TextureRole::Layer1ColorMap,
                TextureRole::Layer3ColorMap,
                TextureRole::Layer1BaseNormalMap,
                TextureRole::Layer1BaseSpecMap,
            ],
        );
        let mut sky = multi2(
            "sky",
            Mask {
                base: 0,
                ignore: 0x3BFF,
            },
            Mask {
                base: 0x1000,
                ignore: 0x3BFF,
            },
            &[TextureRole::Layer1ColorMap],
        );
        // Sliding along u, half a texture along at the start.
        sky.scrolls = vec![Scroll {
            layer: 0,
            scale: [1.0, 1.0],
            velocity: [0.25, 0.0],
            amplitude: [0.5, 0.0],
            frequency: [1.0, 0.0],
            phase: [std::f32::consts::FRAC_PI_2, 0.0],
            rotation_phase: 0.0,
        }];
        let grass = LevelMaterial::Effect(EffectMaterial {
            name: "grass_HIRES".into(),
            effect: 0xAA6,
            pixel: None,
            vertex: None,
            textures: Vec::new(),
            states: sky.states,
            scrolls: Vec::new(),
            tracks: Vec::new(),
            unread: Vec::new(),
        });
        let scene = guac_ir::level::Scene {
            identity: identity(),
            level: LEVEL.into(),
            scenes: vec![SceneGraph {
                world: "Levels/Test/Test_ART_MASTER.ma".into(),
                materials: vec![
                    LevelMaterial::Multi2(ground.clone()),
                    grass,
                    LevelMaterial::Multi2(sky.clone()),
                ],
                segments: vec![
                    segment(0, plain, 0.0),
                    segment(
                        0,
                        MeshShape {
                            colours: true,
                            ..plain
                        },
                        2.0,
                    ),
                    segment(
                        1,
                        MeshShape {
                            uv_sets: 1,
                            ..plain
                        },
                        4.0,
                    ),
                    segment(
                        2,
                        MeshShape {
                            uv_sets: 1,
                            tangents: false,
                            ..plain
                        },
                        6.0,
                    ),
                    segment(0, plain, 8.0),
                ],
                sky: vec![SkyGroup {
                    name: "sky".into(),
                    segments: vec![3],
                }],
            }],
        };
        let ir = IrFormat::Ron;
        let at = dir.join("levels").join(LEVEL);
        std::fs::create_dir_all(&at).unwrap();
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::write(
            dir.join(ir.level_path(LEVEL, SCENE)),
            guac_ir::to_ron(&scene).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join(ir.level_path(LEVEL, LIGHTS)),
            guac_ir::to_ron(lights).unwrap(),
        )
        .unwrap();
        // A 4x4 BC1 block of one colour: built here, none of the disc's.
        let ktx2 = Texture {
            format: Format::Bc1RgbaUnorm,
            width: 4,
            height: 4,
            levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
        }
        .to_ktx2();
        for m in [&ground, &sky] {
            for t in &m.textures {
                std::fs::write(dir.join(&t.texture), &ktx2).unwrap();
            }
        }
        LevelSource {
            dir: dir.to_path_buf(),
            ir,
            identity: identity(),
            level: LEVEL.into(),
        }
    }

    /// The turntables' test app with the level loader, fed `source`.
    fn level_app(dir: &Path, source: LevelSource, gpu: bool) -> App {
        let mut app = app(extract(dir));
        app.add_message::<Redrawn>().add_plugins(LevelPlugin);
        if gpu {
            app.insert_resource(Level::new(true))
                .insert_resource(Warm::new(true));
        }
        let (tx, feed) = level_channel();
        tx.send(source).unwrap();
        app.insert_resource(feed);
        app
    }

    fn state(app: &App) -> String {
        app.world().resource::<Level>().report().state.clone()
    }

    /// Frames until the level's state is one of `states`, up to 5 s; the
    /// render world played by `between` each frame.
    fn until(app: &mut App, states: &[&str], mut between: impl FnMut(&mut App)) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !states.contains(&state(app).as_str()) {
            assert!(Instant::now() < deadline, "stuck at {}", state(app));
            between(app);
            app.update();
        }
    }

    /// AG.19: the level is ready only once the renderer has every texture,
    /// mesh and material resident and the warm has compiled every key it
    /// draws; meanwhile it stays spawned hidden, waiting.
    #[test]
    fn the_level_is_ready_only_once_every_part_is_resident() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, true);
        until(&mut app, &["waiting"], |_| {});
        let report = app.world().resource::<Level>().report().clone();
        // Four ground maps and the sky's, four meshes (the grass's isn't
        // drawn), two materials: none resident with no render world.
        assert_eq!(
            (report.textures, report.segments, report.materials),
            (5, 4, 2)
        );
        assert_eq!((report.left_out, report.segments_left_out), (1, 1));
        assert_eq!(report.sky_segments, 1);
        assert_eq!(report.not_resident, 5 + 4 + 2, "{report:?}");
        assert_eq!(report.keys, 3);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(state(&app), "waiting", "nothing resident");

        // The render world strikes off all but one texture, then that: the
        // level still waits, now on the warm alone.
        let watch = app.world().resource::<LevelWatch>().clone();
        let last = {
            let mut p = watch.lock();
            p.meshes.clear();
            p.materials.clear();
            let last = *p.images.iter().next().unwrap();
            p.images.retain(|id| *id == last);
            last
        };
        app.update();
        assert_eq!(state(&app), "waiting");
        assert_eq!(app.world().resource::<Level>().report().not_resident, 1);
        watch.lock().images.remove(&last);
        app.update();
        let report = app.world().resource::<Level>().report().clone();
        assert_eq!((report.state.as_str(), report.not_resident), ("waiting", 0));
        assert!(report.keys_waiting > 0, "{report:?}");

        // Each key the warm draws compiles: ready.
        let pipelines = app.world().resource::<PipelineWatch>().clone();
        until(&mut app, &["ready"], |_| pipelines.compile_all());
        let level = app.world().resource::<Level>();
        let report = level.report();
        assert_eq!((report.not_resident, report.keys_waiting), (0, 0));
        assert!(report.ready_ms.is_some() && !report.shown);
        let root = level.loaded().unwrap().root;
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden),
            "nothing shows it"
        );
    }

    /// AG.19: the keys the level asks the warm for are exactly the keys its
    /// entities draw (each segment's material key on its mesh's shape),
    /// each once.
    #[test]
    fn the_warm_list_equals_the_levels_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        app.update();
        let asked = app.world().resource::<Warm>().level_listed().to_vec();
        let unique: HashSet<WarmKey> = asked.iter().copied().collect();
        assert_eq!(unique.len(), asked.len(), "each key once: {asked:?}");
        assert_eq!(asked.len(), 3);
        let loaded: HashSet<WarmKey> = app
            .world()
            .resource::<Level>()
            .loaded()
            .unwrap()
            .keys
            .iter()
            .copied()
            .collect();
        assert_eq!(loaded, unique);

        let world = app.world_mut();
        let mut q = world.query::<(&Mesh3d, &MeshMaterial3d<CharacterMaterial>, &RenderLayers)>();
        let (meshes, materials) = (
            world.resource::<Assets<Mesh>>(),
            world.resource::<Assets<CharacterMaterial>>(),
        );
        let mut drawn = HashSet::new();
        let mut count = 0;
        for (mesh, material, layers) in q.iter(world) {
            if *layers != RenderLayers::layer(LEVEL_LAYER) {
                continue;
            }
            count += 1;
            let mesh = meshes.get(&mesh.0).unwrap();
            let shape = MeshShape {
                uv_sets: [
                    Mesh::ATTRIBUTE_UV_0,
                    Mesh::ATTRIBUTE_UV_1,
                    guacamole::character::mesh::ATTRIBUTE_UV_2,
                ]
                .into_iter()
                .filter(|a| mesh.attribute(a.id).is_some())
                .count() as u8,
                tangents: mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
                colours: mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some(),
                skinned: mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX).is_some(),
            };
            drawn.insert(WarmKey {
                material: materials.get(&material.0).unwrap().key,
                mesh: shape,
            });
        }
        assert_eq!(count, 4, "every drawn segment, the grass's not among them");
        assert_eq!(drawn, unique);
        // Headless, nothing compiles: every key counts as compiled.
        assert_eq!(app.world().resource::<Warm>().report().state, "done");
    }

    /// A level the materials can't be lit for fails, saying why, and one
    /// whose parts name another dump is refused.
    #[test]
    fn a_level_without_its_world_lights_fails_and_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dark = lights();
        dark.directional.retain(|l| l.name != "_worldLight2");
        let source = write_level(tmp.path(), &dark);
        let mut app = level_app(tmp.path(), source.clone(), false);
        until(&mut app, &["failed"], |_| {});
        let reason = app.world().resource::<Level>().report().reason.clone();
        assert_eq!(reason.as_deref(), Some("the level has no _worldLight2"));
        // Another dump's level is refused by its parts' identity.
        let mut other = source;
        other.identity.manifest_hash = Sha256([9; 32]);
        let (tx, feed) = level_channel();
        tx.send(other).unwrap();
        app.insert_resource(feed);
        app.update();
        assert_eq!(state(&app), "reading");
        until(&mut app, &["failed"], |_| {});
        assert_eq!(
            app.world().resource::<Level>().report().reason.as_deref(),
            Some("the scene part is another dump's or level's")
        );
    }

    /// The dev entry: once ready, the level shows from AF's fresh-state
    /// camera over the TV, lit for it; moved, the camera relights every
    /// material from where it stands. Under reduced motion a scroll holds
    /// its start.
    #[test]
    fn the_dev_view_shows_the_level_and_a_moved_camera_relights_it() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        app.insert_resource(LevelView(true))
            .insert_resource(ReducedMotion(true));
        app.world_mut().spawn(TvCamera);
        until(&mut app, &["shown"], |_| {});
        app.update();
        let (root, camera, materials) = {
            let loaded = app.world().resource::<Level>().loaded().unwrap();
            (
                loaded.root,
                loaded.camera.unwrap(),
                loaded.materials.clone(),
            )
        };
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited)
        );
        let c = app.world().entity(camera);
        let cam = c.get::<Camera>().unwrap();
        assert_eq!(cam.order, VIEW_ORDER);
        assert!(
            matches!(
                cam.output_mode,
                CameraOutputMode::Write {
                    blend_state: Some(BlendState::REPLACE),
                    ..
                }
            ),
            "it replaces the TV, not blends over it"
        );
        assert_eq!(c.get::<Transform>(), Some(&fresh_state_transform()));
        assert_eq!(
            c.get::<RenderLayers>(),
            Some(&RenderLayers::layer(LEVEL_LAYER))
        );
        assert!(app.world().resource::<Level>().report().shown);

        let lit = level_lights(&l).unwrap();
        let travel = |app: &App| {
            let m = app
                .world()
                .resource::<Assets<CharacterMaterial>>()
                .get(&materials[0])
                .unwrap();
            m.uniform.light_travel[0].truncate()
        };
        let at = |t: &Transform| {
            view_scene(&lit, &LevelFog::PLACEHOLDER, t.to_matrix().inverse()).lights[0].travel
        };
        assert!(travel(&app).abs_diff_eq(Vec3::from_array(at(&fresh_state_transform())), 1e-6));
        // Turned to face the other way: the key light's travel turns too.
        let moved = Transform::from_xyz(0.0, 50.0, 0.0).looking_to(Vec3::NEG_Z, Vec3::Y);
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = moved;
        let sky_rows = |app: &App| {
            let m = app
                .world()
                .resource::<Assets<CharacterMaterial>>()
                .get(&materials[1])
                .unwrap();
            (m.uniform.uv_u, m.uniform.uv_v)
        };
        app.update();
        app.update();
        assert!(travel(&app).abs_diff_eq(Vec3::from_array(at(&moved)), 1e-6));
        // The sky's u slides from half a texture along, its track's start,
        // not the identity's 0.
        let (u, _) = sky_rows(&app);
        assert!(
            u[0].abs_diff_eq(Vec4::new(1.0, 0.0, 0.5, 0.0), 1e-6),
            "{u:?}"
        );
    }
}
