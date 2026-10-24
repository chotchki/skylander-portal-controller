//! The level on the TV (openspec AG.19, design decision 9; level-scene's
//! "The level loads behind the intro" and "The level never stalls the TV").
//! The arithmetic is the library's ([`guacamole::level_scene`]); this is
//! the loading, off the frame:
//!
//! - The extraction says where a level is ([`LevelSource`]) once the Giants
//!   pick finds its extract current or the level stage has written it; the
//!   level loads from there at once, behind whatever the portal shows.
//! - Reading: on a task, the scene, lights and atmosphere parts, each drawn
//!   segment's mesh, each texture through the remaster's views, the keys it
//!   draws.
//! - Uploading: the frame hands the renderer at most [`UPLOAD_BYTES`] of
//!   textures and meshes, then [`MATERIALS_PER_FRAME`] materials, a frame,
//!   so no frame carries the whole level to the GPU; the warm compiles its
//!   keys meanwhile, one a frame on its hidden stage ([`crate::warm`]).
//! - Waiting: the level is spawned hidden and is ready only once the render
//!   world has every texture, mesh and material resident ([`watch_residency`])
//!   and every key compiled, so the frame it first shows uploads and
//!   compiles nothing of its own.
//! - Fog and light: the atmospheric zone the level's definition sets up at
//!   its start (AG.41) lights it and sets the [`LevelFog`] resource, which
//!   every material's fog term reads; a change relights them all
//!   ([`light_level`]). The lights are handed to the shader in the world's
//!   frame, which it turns by the view it draws for (AG.21,
//!   [`CharacterUniform::lit_in_world`]), so the camera following the hero
//!   rewrites no material: lit in view space, every frame it moved rewrote
//!   all 471 of chapter 1's.
//! - Scripts (AH.22): the level's script part, parsed and linked on a task
//!   of its own beside the reading one ([`guacamole::level_scripts`]), so it
//!   adds nothing to the time the level takes to show; the level is drawn
//!   whether it links or not, and the report says which and what it cost.
//!   Nothing runs the program yet (AH.16).
//! - The walk (AG.21): the collision and placements parts are read on the
//!   same task, the collision built into Avian colliders there
//!   ([`PreparedBody`]) and spawned with the level, where the hero's queries
//!   find them at once (Avian puts a collider in its trees, its bounds
//!   computed, as it's inserted); where the hero starts is
//!   `213_Init_Location`'s first waypoint.
//!
//! The first character placed takes the TV into it ([`crate::play`]): its
//! camera shows the level ([`Level::show`]) and the hero's materials are lit
//! with the level's ([`Level::light_also`]). The dev entry does without a
//! hero: with `GUAC_LEVEL_VIEW` set in a `dev-tools` build, the level fills
//! the TV once it's ready, drawn at AF's fresh-state camera by the
//! remaster's camera into the TV's own target (the window, or the shots'
//! image).
//!
//! Either way the level draws under the TV's UI (AG.50, [`tv_over_level`]):
//! its camera comes first on the TV's target, the TV's own after it with the
//! screen under the level hidden and nothing cleared in, so what TV UI shows
//! while the level does (Start's overlay, the controller message, an intro)
//! lands on the level, in `GlobalZIndex` order as anywhere else on the TV.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{CameraOutputMode, ClearColorConfig, Projection, RenderTarget};
use bevy::image::CompressedImageFormats;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::PreparedMaterial;
use bevy::prelude::*;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::mesh::RenderMesh;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::BlendState;
use bevy::render::renderer::RenderDevice;
use bevy::render::storage::ShaderBuffer;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use crossbeam_channel::{Receiver, Sender};
use guac_extract::intros::IrFormat;
use guac_extract::levels::{ATMOSPHERE, COLLISION, LEVELS, LIGHTS, PLACEMENTS, PROPS, SCENE};
use guac_ir::DumpIdentity;
use guac_ir::model::{self, Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::{PointLight, Scene};
use guac_motion::seam::{BodyId, BodyKind};
use guac_script::Obj;
use guacamole::character::warm::WarmKey;
use guacamole::character::{
    CharacterMaterial, CharacterUniform, FrameGlow, LEVEL_LIGHTS, ScenePointLights, ShadowLookup,
    reference_material, remaster, spawn_alpha_clear, static_mesh,
};
use guacamole::level_scene::{
    self as arithmetic, AtStart, Drawn, LevelFog, LevelLights, fresh_state_projection,
    fresh_state_transform, fresh_state_view, hero_scene, view_scene, world_scene,
};
use guacamole::level_scripts;
use guacamole::props::{LeftOut, LevelProps, Prop};
use guacamole::scripts::ScriptWorld;
use serde::Serialize;
use tracing::{info, warn};

use crate::extraction::Extraction;
use crate::hero::{HeroBody, HeroSystems};
use crate::hero_scripts::{ScriptSource, ScriptSystems};
use crate::motion::ReducedMotion;
use crate::physics::PreparedBody;
use crate::turntable::{plain, read_ir};
use crate::tv::{Redrawn, ScreenRoot, TvCamera};
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
/// The level's camera (the dev view's, the walk's) draws before the TV's own
/// ([`crate::tv::TV_ORDER`]), under its UI (AG.50): at order 1 it covered Start's
/// overlay and the controller message, so a paused level read as a hang.
/// Clear of the turntables' -1 and -2 and the titles' -1, though those draw
/// into images of their own.
pub(crate) const VIEW_ORDER: isize = -3;
/// How it writes onto the TV's target: replacing what's there. It's the
/// first camera there, where Bevy would pick that write itself; named, it's
/// one pipeline whatever the order, the one the start's pass compiles. The
/// level's opaque parts write the alpha their maps hold, so a blend would
/// let the target's last frame show through them.
const VIEW_OUTPUT: CameraOutputMode = CameraOutputMode::Write {
    blend_state: Some(BlendState::REPLACE),
    clear_color: ClearColorConfig::None,
};
/// How the TV's camera writes over a level output under it (AG.50): laid
/// over, not replacing. bevy_ui blends each node over the last on a target
/// the TV clears to no colour at all, so the target holds colours already
/// weighted by their cover, which is what a premultiplied write expects;
/// Bevy's own pick for a second camera (plain alpha blending) weighs them
/// twice, a text edge at half cover landing at a quarter.
pub(crate) const TV_OVER: CameraOutputMode = CameraOutputMode::Write {
    blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
    clear_color: ClearColorConfig::None,
};
/// Texture and mesh bytes handed to the renderer a frame.
pub(crate) const UPLOAD_BYTES: usize = 16 << 20;
/// Materials made a frame, each a bind group the renderer prepares.
pub(crate) const MATERIALS_PER_FRAME: usize = 64;

/// A texture as a material's slot samples it: (extract path, sRGB view,
/// sampler).
type ImageKey = (String, bool, Sampler);

/// What the reading task hands the frame.
struct Built {
    level: String,
    drawn: Drawn,
    meshes: Vec<Mesh>,
    images: Vec<(ImageKey, Image)>,
    keys: Vec<(WarmKey, usize)>,
    lights: LevelLights,
    /// The start's zone's fog, which the TV's [`LevelFog`] takes.
    fog: LevelFog,
    /// The sky's bounds, for the log: (min, max).
    sky: Option<(Vec3, Vec3)>,
    /// Each drawn material's sort bias: the sky's blended parts first, in
    /// the scene's order (AG.79, `level_scene::depth_biases`).
    depth_bias: Vec<f32>,
    walk: Walk,
    /// The props (AG.25): each model's segments as (an index into `meshes`,
    /// past the level's own, and its material in `drawn.materials`).
    prop_models: Vec<Vec<(usize, u16)>>,
    /// The spawn part's models (AH.24), by path: each one's segments as the
    /// props' are, past theirs.
    spawn_models: Vec<(String, Vec<(usize, u16)>)>,
    read: Duration,
    took: Duration,
}

/// What walking the level takes: its solids, built off the frame, where
/// the hero starts, and the props that stand by where he is (AG.25), each
/// one's solid built off the frame too.
struct Walk {
    bodies: Vec<PreparedBody>,
    triangles: usize,
    start: Vec3,
    props: LevelProps,
    prop_bodies: Vec<Option<PreparedBody>>,
}

/// The collision, placements and props parts as the walk takes them: the
/// collision's meshes built into colliders, the start's waypoint found, the
/// placements' props and their solids.
fn read_walk(source: &LevelSource, part: impl Fn(&str) -> PathBuf) -> Result<Walk, String> {
    let collision: guac_ir::level::Collision = read_ir(source.ir, &part(COLLISION))?;
    let placements: guac_ir::level::Placements = read_ir(source.ir, &part(PLACEMENTS))?;
    let props: guac_ir::level::Props = read_ir(source.ir, &part(PROPS))?;
    for (what, identity, level) in [
        ("collision", &collision.identity, &collision.level),
        ("placements", &placements.identity, &placements.level),
        ("props", &props.identity, &props.level),
    ] {
        if *identity != source.identity || *level != source.level {
            return Err(format!("the {what} part is another dump's or level's"));
        }
    }
    let start = arithmetic::start_point(&placements).map_err(|e| e.to_string())?;
    let solids = arithmetic::solids(&collision).map_err(|e| e.to_string())?;
    let triangles = collision.meshes.iter().map(|m| m.triangles.len()).sum();
    let bodies = solids
        .iter()
        .map(|b| PreparedBody::new(b).map_err(|e| format!("the collision: {e}")))
        .collect::<Result<_, _>>()?;
    let props = LevelProps::new(&props, &placements).map_err(|e| format!("the props: {e}"))?;
    let prop_bodies = props
        .props
        .iter()
        .map(|p| {
            p.body
                .as_ref()
                .map(|b| PreparedBody::new(b).map_err(|e| format!("{}'s solid: {e}", p.name)))
                .transpose()
        })
        .collect::<Result<_, _>>()?;
    Ok(Walk {
        bodies,
        triangles,
        start,
        props,
        prop_bodies,
    })
}

/// Reads `source`'s level for drawing: its scene, lights and atmosphere
/// parts (each checked to be this dump's and this level's), the start's
/// zone's lights and fog, the drawn segments' meshes, the textures they
/// bind, the keys they draw at the fresh state's camera.
fn read_level(source: &LevelSource, formats: CompressedImageFormats) -> Result<Built, String> {
    let started = Instant::now();
    let part = |p: &str| source.dir.join(source.ir.level_path(&source.level, p));
    let scene: guac_ir::level::Scene = read_ir(source.ir, &part(SCENE))?;
    let lights: guac_ir::level::Lights = read_ir(source.ir, &part(LIGHTS))?;
    let atmosphere: guac_ir::level::Atmosphere = read_ir(source.ir, &part(ATMOSPHERE))?;
    for (what, identity, level) in [
        ("scene", &scene.identity, &scene.level),
        ("lights", &lights.identity, &lights.level),
        ("atmosphere", &atmosphere.identity, &atmosphere.level),
    ] {
        if *identity != source.identity || *level != source.level {
            return Err(format!("the {what} part is another dump's or level's"));
        }
    }
    let read = started.elapsed();
    let mut walk = read_walk(source, part)?;
    // The census's level table is held to this derivation on the disc
    // (`tests/giants_census.rs`).
    let AtStart {
        mut drawn,
        lights,
        fog,
        mut keys,
    } = arithmetic::at_start(scene, &lights, &atmosphere).map_err(|e| e.to_string())?;
    // The props' models draw beside the level: their materials after its
    // own, their keys in its scene (the census's props table holds them,
    // `tests/giants_census.rs`), their meshes after its segments'.
    let level_materials = drawn.materials.len();
    let (props, prop_ranges) = walk.props.drawn();
    let scene = level_scene(&lights, &fog);
    for (k, m) in arithmetic::keys(&props, &scene).map_err(|e| format!("the props: {e}"))? {
        if !keys.iter().any(|(kept, _)| *kept == k) {
            keys.push((k, level_materials + m));
        }
    }
    let offset = |m: u16| {
        u16::try_from(level_materials + usize::from(m))
            .map_err(|_| "past 65,536 materials with the props'".to_owned())
    };
    let level_segments = drawn.segments.len();
    let prop_models = prop_ranges
        .iter()
        .map(|r| {
            r.clone()
                .map(|i| Ok((level_segments + i, offset(props.segments[i].material)?)))
                .collect::<Result<Vec<_>, String>>()
        })
        .collect::<Result<Vec<_>, String>>()?;
    drawn.materials.extend(props.materials.iter().cloned());
    // The meshes hold the props' vertices once built.
    walk.props.models.clear();
    // What the scripts' clones draw (AH.24): the spawn part's models after
    // the props', keyed in the same scene (the census's spawn table holds
    // them). A part that doesn't read draws no clone; the level goes on.
    let spawns = match read_spawns(source) {
        Ok(m) => m,
        Err(why) => {
            warn!(reason = %why, "the spawn models didn't read: the scripts' clones draw nothing");
            Vec::new()
        }
    };
    let (spawned, spawn_ranges) = guacamole::props::models_drawn(&spawns);
    let spawn_materials = drawn.materials.len();
    for (k, m) in
        arithmetic::keys(&spawned, &scene).map_err(|e| format!("the spawn models: {e}"))?
    {
        if !keys.iter().any(|(kept, _)| *kept == k) {
            keys.push((k, spawn_materials + m));
        }
    }
    let spawn_segments = level_segments + props.segments.len();
    let spawn_offset = |m: u16| {
        u16::try_from(spawn_materials + usize::from(m))
            .map_err(|_| "past 65,536 materials with the spawn models'".to_owned())
    };
    let spawn_models = spawns
        .iter()
        .zip(&spawn_ranges)
        .map(|(model, r)| {
            let parts = r
                .clone()
                .map(|i| {
                    Ok((
                        spawn_segments + i,
                        spawn_offset(spawned.segments[i].material)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((model.path.clone(), parts))
        })
        .collect::<Result<Vec<_>, String>>()?;
    drawn.materials.extend(spawned.materials.iter().cloned());
    let mut images: Vec<(ImageKey, Image)> = Vec::new();
    let slots = drawn
        .materials
        .iter()
        .flat_map(|m| m.textures.iter().map(move |slot| (m, slot)));
    for (material, slot) in slots {
        let srgb = remaster::samples_srgb(material, slot.role);
        let key = (slot.texture.clone(), srgb, slot.sampler);
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
        let image = remaster::texture_view(&ktx2, srgb, &slot.sampler, formats)
            .map_err(|e| format!("{}: {e}", slot.texture))?;
        images.push((key, image));
    }
    let mut meshes = drawn
        .segments
        .iter()
        .enumerate()
        .map(|(i, s)| static_mesh(s).map_err(|e| format!("segment {}: {e}", drawn.sources[i])))
        .collect::<Result<Vec<_>, _>>()?;
    for s in &props.segments {
        meshes.push(static_mesh(s).map_err(|e| format!("a prop's segment {}: {e}", s.source))?);
    }
    for s in &spawned.segments {
        meshes.push(
            static_mesh(s).map_err(|e| format!("a spawn model's segment {}: {e}", s.source))?,
        );
    }
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
    let depth_bias = arithmetic::depth_biases(&drawn);
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
        fog,
        sky,
        depth_bias,
        walk,
        prop_models,
        spawn_models,
        read,
        took: started.elapsed(),
    })
}

/// The extract's spawn part (AH.22): every model each spawn archive's
/// placements name, as props draw them.
fn read_spawns(source: &LevelSource) -> Result<Vec<guacamole::props::PropModel>, String> {
    let mut out = Vec::new();
    for archive in guac_extract::scripts::spawn_archives() {
        let rel = source.ir.spawn_path(&archive);
        let spawns: guac_ir::spawn::SpawnModels = read_ir(source.ir, &source.dir.join(&rel))?;
        if spawns.identity != source.identity {
            return Err(format!("{rel} is another dump's"));
        }
        out.extend(guacamole::props::spawn_models(&spawns).map_err(|e| format!("{rel}: {e}"))?);
    }
    Ok(out)
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

    /// Strikes off what the render world says is resident.
    fn strike(
        &mut self,
        image: impl Fn(AssetId<Image>) -> bool,
        mesh: impl Fn(AssetId<Mesh>) -> bool,
        material: impl Fn(AssetId<CharacterMaterial>) -> bool,
    ) {
        self.images.retain(|id| !image(*id));
        self.meshes.retain(|id| !mesh(*id));
        self.materials.retain(|id| !material(*id));
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
    pending.strike(
        |id| images.get(id).is_some(),
        |id| meshes.get(id).is_some() && allocator.mesh_vertex_slice(&id).is_some(),
        |id| materials.get(id).is_some(),
    );
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
    /// The collision's triangles, and where the hero starts (AG.21).
    pub triangles: usize,
    pub start: Option<[f32; 3]>,
    /// Its props: what can stand, what does where the hero is (AG.25).
    pub props: Option<PropsReport>,
    /// Textures, meshes and materials not resident yet; keys not compiled.
    pub not_resident: usize,
    pub keys_waiting: usize,
    /// From the source landing: the task's read of the parts, the task
    /// done, everything handed to the renderer, ready.
    pub read_ms: Option<u64>,
    pub built_ms: Option<u64>,
    pub uploaded_ms: Option<u64>,
    pub ready_ms: Option<u64>,
    /// Times every material was lit again (a camera's move, a fog change).
    pub relit: u64,
    /// From the Giants pick to ready.
    pub pick_to_ready_ms: Option<u64>,
    pub shown: bool,
    /// The point lights the frame draws the level with (openspec AH.65),
    /// the lit ones of the level's buffer's three.
    pub lights: Vec<FrameLight>,
    /// The script part, once linked (AH.22), or why it didn't.
    pub scripts: Option<ScriptsReport>,
    pub scripts_reason: Option<String>,
    /// What stopped it: the reading task's error, a part's file or the
    /// lights it lacks.
    pub reason: Option<String>,
}

/// A point light the frame draws the level with (openspec AH.65): where,
/// its colour decoded, its falloff's two terms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub(crate) struct FrameLight {
    pub position: [f32; 3],
    pub colour: [f32; 3],
    pub linear: f32,
    pub quadratic: f32,
}

/// What `/api/_test/tv` reports of the level's props (AG.25).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PropsReport {
    /// Placements that can stand, the models they draw, and those that
    /// stand where the hero is (or at the start, before he's in): drawn,
    /// solid (kinematic) and sensing (sensors), each standing one by name.
    pub props: usize,
    pub models: usize,
    pub standing: usize,
    pub drawn: usize,
    pub solid: usize,
    pub sensors: usize,
    pub names: Vec<String>,
    /// Where the rule measured from last.
    pub at: Option<[f32; 3]>,
    /// Props the level's scripts took off (openspec AH.21: breakables
    /// broken), by name: they stand nowhere again.
    pub taken_off: Vec<String>,
    /// The placements left out, by why (template, creature, by script,
    /// nothing to stand, a state no level file holds), and the props whose
    /// model draws nothing.
    pub templates: usize,
    pub creatures: usize,
    pub by_script: usize,
    pub nothing: usize,
    pub state: usize,
    pub undrawn: usize,
}

/// A level's props on the TV (AG.25): each one's draw (hidden under the
/// level's root until it stands) and solid (in Avian's world only while it
/// stands), stood up and taken down as the hero comes within its range and
/// leaves it.
pub(crate) struct StandingProps {
    props: Vec<Prop>,
    /// Per prop: the entity its model hangs under, if it draws.
    draws: Vec<Option<Entity>>,
    /// Per prop: its solid, ready to spawn, and its entity while it stands.
    bodies: Vec<Option<PreparedBody>>,
    spawned: Vec<Option<Entity>>,
    standing: Vec<bool>,
    report: PropsReport,
}

impl StandingProps {
    fn new(
        walk: &mut Walk,
        models: &[Vec<(usize, u16)>],
        meshes: &[Handle<Mesh>],
        materials: &[Handle<CharacterMaterial>],
        root: Entity,
        commands: &mut Commands,
    ) -> Self {
        let props = std::mem::take(&mut walk.props.props);
        let left = |why: LeftOut| {
            walk.props
                .left_out
                .iter()
                .filter(|(_, w)| *w == why)
                .count()
        };
        let report = PropsReport {
            props: props.len(),
            models: models.len(),
            templates: left(LeftOut::Template),
            creatures: left(LeftOut::Creature),
            by_script: left(LeftOut::ByScript),
            nothing: left(LeftOut::Nothing),
            state: left(LeftOut::State),
            undrawn: walk.props.undrawn.len(),
            ..Default::default()
        };
        let draws = props
            .iter()
            .map(|p| {
                let model = &models[p.model?];
                let parent = commands
                    .spawn((
                        Name::new(format!("prop {}", p.name)),
                        p.transform,
                        Visibility::Hidden,
                        ChildOf(root),
                    ))
                    .id();
                for &(mesh, material) in model {
                    commands.spawn((
                        Mesh3d(meshes[mesh].clone()),
                        MeshMaterial3d(materials[usize::from(material)].clone()),
                        Transform::IDENTITY,
                        RenderLayers::layer(LEVEL_LAYER),
                        ChildOf(parent),
                    ));
                }
                Some(parent)
            })
            .collect();
        let n = props.len();
        Self {
            props,
            draws,
            bodies: std::mem::take(&mut walk.prop_bodies),
            spawned: vec![None; n],
            standing: vec![false; n],
            report,
        }
    }

    /// Stands up the props within range of `at` and takes down the rest,
    /// and every one `taken_off` says the level's scripts took off, by its
    /// object (openspec AH.21: a breakable broken goes at once and never
    /// stands again): each one's draw shown or hidden, its solid spawned
    /// into Avian's world or despawned from it.
    pub(crate) fn stand(
        &mut self,
        at: Vec3,
        taken_off: impl Fn(u32) -> bool,
        commands: &mut Commands,
    ) {
        self.report.at = Some(at.to_array());
        let mut changed = false;
        for (i, p) in self.props.iter().enumerate() {
            let off = taken_off(p.object);
            if off && !self.report.taken_off.contains(&p.name) {
                self.report.taken_off.push(p.name.clone());
            }
            let stands = p.stands(at) && !off;
            if stands == self.standing[i] {
                continue;
            }
            changed = true;
            self.standing[i] = stands;
            if let Some(draw) = self.draws[i] {
                commands.entity(draw).insert(if stands {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
            if stands {
                self.spawned[i] = self.bodies[i].clone().map(|b| b.spawn(commands));
            } else if let Some(body) = self.spawned[i].take() {
                commands.entity(body).despawn();
            }
        }
        if changed {
            let standing: Vec<usize> = (0..self.props.len())
                .filter(|&i| self.standing[i])
                .collect();
            let kind = |i: &usize| self.props[*i].body.as_ref().map(|b| b.kind);
            let r = &mut self.report;
            r.standing = standing.len();
            r.drawn = standing
                .iter()
                .filter(|&&i| self.draws[i].is_some())
                .count();
            r.solid = standing
                .iter()
                .filter(|i| kind(i) == Some(BodyKind::Kinematic))
                .count();
            r.sensors = standing
                .iter()
                .filter(|i| kind(i) == Some(BodyKind::Sensor))
                .count();
            r.names = standing
                .iter()
                .map(|&i| self.props[i].name.clone())
                .collect();
        }
    }

    /// Every solid standing, for a new load to take away.
    fn despawn(&mut self, commands: &mut Commands) {
        for body in self.spawned.iter_mut().filter_map(Option::take) {
            commands.entity(body).despawn();
        }
        self.standing.fill(false);
    }

    pub(crate) fn report(&self) -> &PropsReport {
        &self.report
    }

    /// Each prop whose solid is in Avian's world now: its object and its
    /// body's id (what the scripts' engine maps to the prop's placement,
    /// openspec AH.27).
    fn standing_bodies(&self) -> Vec<(u32, BodyId)> {
        self.props
            .iter()
            .zip(&self.spawned)
            .filter(|(_, spawned)| spawned.is_some())
            .filter_map(|(p, _)| p.body.as_ref().map(|b| (p.object, b.id)))
            .collect()
    }
}

/// What `/api/_test/tv` reports of the level's script part (AH.22).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct ScriptsReport {
    /// The entries, the level's own included, and their bytes.
    pub files: usize,
    pub bytes: u64,
    /// What linked: scripts, ops and macro definitions.
    pub scripts: usize,
    pub ops: usize,
    pub defs: usize,
    /// Reading the entries, parsing them and their graphs, linking; and
    /// from the source landing to linked.
    pub read_ms: u64,
    pub parsed_ms: u64,
    pub linked_ms: u64,
    pub done_ms: Option<u64>,
}

/// A level being handed to the renderer.
struct Uploading {
    built: Box<Built>,
    images: VecDeque<(ImageKey, Image)>,
    meshes: VecDeque<Mesh>,
    image_handles: Vec<(ImageKey, Handle<Image>)>,
    mesh_handles: Vec<Handle<Mesh>>,
    materials: Vec<Handle<CharacterMaterial>>,
    /// The fog every material is made with.
    fog: LevelFog,
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
    /// The fog the uniforms are lit with.
    lit_fog: LevelFog,
    camera: Option<Entity>,
    shown_at: Option<Duration>,
    /// Its solids, spawned with it.
    bodies: Vec<Entity>,
    /// Its props, standing by where the hero is (AG.25).
    pub(crate) props: StandingProps,
    /// The spawn part's models by path, each segment's mesh and material:
    /// what the scripts' clones draw (AH.24).
    spawns: HashMap<String, Vec<(Handle<Mesh>, Handle<CharacterMaterial>)>>,
    /// Where the hero starts, before he's settled.
    pub(crate) start: Vec3,
    /// Materials lit with the level's but in the hero's scene
    /// ([`Level::light_also`]).
    also: Vec<LitAlso>,
    /// The point lights the level's buffer holds ([`light_frame`]), `None`
    /// until the first frame writes them.
    frame_lights: Option<[PointLight; 3]>,
}

#[cfg(test)]
impl Loaded {
    /// Materials besides its own it lights ([`Level::light_also`]).
    pub(crate) fn also_count(&self) -> usize {
        self.also.len()
    }
}

/// A material outside the level lit with it: the hero's, in
/// [`hero_scene`], relit when the level's are.
pub(crate) struct LitAlso {
    pub(crate) material: Handle<CharacterMaterial>,
    pub(crate) reference: guac_ir::shading::Material,
    pub(crate) alpha: f32,
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
    report: LevelReport,
    /// The script part's task, until it's done (AH.22).
    scripts: Option<Task<Result<level_scripts::Linked, String>>>,
}

impl Level {
    pub(crate) fn new(gpu: bool) -> Self {
        Self {
            gpu,
            source: None,
            stage: Stage::Idle,
            since: None,
            report: LevelReport {
                state: "idle".into(),
                ..Default::default()
            },
            scripts: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn report(&self) -> &LevelReport {
        &self.report
    }

    /// The report, with the props as they stand now.
    pub(crate) fn snapshot(&self) -> LevelReport {
        let mut r = self.report.clone();
        r.props = self.stage.loaded().map(|l| l.props.report().clone());
        r
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

    /// The ready level, its solids spawned: what the hero can be spawned
    /// into.
    pub(crate) fn walkable(&self) -> Option<&Loaded> {
        match &self.stage {
            Stage::Ready(l) => Some(l),
            _ => None,
        }
    }

    /// Shows the ready level through `camera`, which draws its layer: its
    /// entities visible from this frame. False when it isn't ready.
    pub(crate) fn show(&mut self, commands: &mut Commands, camera: Entity, at: Duration) -> bool {
        let Stage::Ready(mut loaded) = std::mem::replace(&mut self.stage, Stage::Idle) else {
            return false;
        };
        commands.entity(loaded.root).insert(Visibility::Inherited);
        loaded.camera = Some(camera);
        loaded.shown_at = Some(at);
        self.stage = Stage::Shown(loaded);
        self.report.shown = true;
        self.report.state = "shown".into();
        true
    }

    /// The level shown through `camera` hidden again, ready to show; the
    /// materials lit with it besides its own dropped. The camera is its
    /// owner's to despawn.
    pub(crate) fn hide(&mut self, commands: &mut Commands, camera: Entity) {
        if self.shown_through() != Some(camera) {
            return;
        }
        let Stage::Shown(mut loaded) = std::mem::replace(&mut self.stage, Stage::Idle) else {
            unreachable!("shown through a camera")
        };
        commands.entity(loaded.root).insert(Visibility::Hidden);
        loaded.camera = None;
        loaded.shown_at = None;
        loaded.also.clear();
        self.stage = Stage::Ready(loaded);
        self.report.shown = false;
        self.report.state = "ready".into();
    }

    /// The camera the level shows through, while it shows.
    pub(crate) fn shown_through(&self) -> Option<Entity> {
        match &self.stage {
            Stage::Shown(l) => l.camera,
            _ => None,
        }
    }

    /// Where the hero starts, once the level's spawned: where the scripts'
    /// activity rule measures from before a hero is in (openspec AH.27), as
    /// the props stand.
    pub(crate) fn start(&self) -> Option<Vec3> {
        self.stage.loaded().map(|l| l.start)
    }

    /// Each prop whose solid is in Avian's world now, by its object, and
    /// its body's id (openspec AH.27); none before the level's spawned.
    pub(crate) fn standing_bodies(&self) -> Vec<(u32, BodyId)> {
        self.stage
            .loaded()
            .map_or_else(Vec::new, |l| l.props.standing_bodies())
    }

    /// The spawn part's models by path, once the level's loaded: what the
    /// scripts' clones draw (AH.24).
    pub(crate) fn spawn_models(
        &self,
    ) -> Option<&HashMap<String, Vec<(Handle<Mesh>, Handle<CharacterMaterial>)>>> {
        self.stage.loaded().map(|l| &l.spawns)
    }

    /// The level's lights and fog, once loaded.
    pub(crate) fn lighting(&self) -> Option<(&LevelLights, LevelFog)> {
        self.stage.loaded().map(|l| (&l.lights, l.lit_fog))
    }

    /// Lights `materials` with the level from now on, in the hero's scene
    /// ([`hero_scene`]) in the world's frame: at once, then again whenever
    /// the level's are.
    pub(crate) fn light_also(
        &mut self,
        materials: Vec<LitAlso>,
        assets: &mut Assets<CharacterMaterial>,
    ) {
        let Some(loaded) = self.stage.loaded_mut() else {
            return;
        };
        let scene = hero_scene(&loaded.lights, &loaded.lit_fog, Mat4::IDENTITY);
        relight(&materials, &scene, assets);
        loaded.also.extend(materials);
    }

    /// Stops lighting `materials` with the level: a hero who left it
    /// (AG.29), whose materials go with him.
    pub(crate) fn forget(&mut self, materials: &[Handle<CharacterMaterial>]) {
        if let Some(loaded) = self.stage.loaded_mut() {
            loaded.also.retain(|lit| !materials.contains(&lit.material));
        }
    }
}

/// `materials`' uniforms lit in `scene`, the world's frame, each
/// texture-coordinate transform kept.
fn relight(materials: &[LitAlso], scene: &Scene, assets: &mut Assets<CharacterMaterial>) {
    for lit in materials {
        if let Some(mut m) = assets.get_mut(&lit.material) {
            let rows = (m.uniform.uv_u, m.uniform.uv_v);
            m.uniform = CharacterUniform::new(&lit.reference, scene, ShadowLookup::NONE, lit.alpha)
                .lit_by_scene();
            m.scene_lights = LEVEL_LIGHTS;
            (m.uniform.uv_u, m.uniform.uv_v) = rows;
        }
    }
}

/// `GUAC_LEVEL_VIEW` (`dev-tools` builds): show the level once it's ready.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LevelView(pub(crate) bool);

/// Marks the camera the level is drawn from.
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
            .init_resource::<LevelFog>()
            .insert_resource(LevelView(view))
            .insert_resource(watch.clone())
            .add_systems(
                Update,
                (receive_level, step_level, show_level, scroll_level)
                    .chain()
                    .in_set(LevelSystems)
                    .after(crate::turntable::TurntableSystems),
            )
            // The props stand by where the hero is as his step begins.
            .add_systems(
                FixedUpdate,
                stand_props
                    .after(ScriptSystems::Step)
                    .before(HeroSystems::Step),
            )
            // Before the assets' events go out, so the frame draws it lit.
            .add_systems(
                PostUpdate,
                (light_level, light_frame).before(bevy::asset::AssetEventSystems),
            )
            .add_systems(
                PostUpdate,
                (tv_over_level, twin_level_glow)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            )
            .init_resource::<GlowTwins>();
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(watch)
                .add_systems(Render, watch_residency.in_set(RenderSystems::Cleanup));
        }
    }
}

/// A new source starts the level loading, the last one's assets no longer
/// waited on; the same one again changes nothing.
fn receive_level(
    mut commands: Commands,
    feed: Option<Res<LevelFeed>>,
    mut level: ResMut<Level>,
    watch: Res<LevelWatch>,
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
        for body in &old.bodies {
            commands.entity(*body).despawn();
        }
    }
    if let Some(old) = level.stage.loaded_mut() {
        old.props.despawn(&mut commands);
    }
    // Its handles go with it, so what it left pending never turns up.
    *watch.lock() = Pending::default();
    info!(dir = %source.dir.display(), level = %source.level, "the level loads");
    // Headless (no GPU) the images keep their blocks as given.
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    let task = {
        let source = source.clone();
        AsyncComputeTaskPool::get().spawn(async move { read_level(&source, formats) })
    };
    level.scripts = Some({
        let (dir, name, ir) = (source.dir.clone(), source.level.clone(), source.ir);
        AsyncComputeTaskPool::get().spawn(async move {
            level_scripts::load(&dir, &name).map(|mut linked| {
                linked.layout = level_scripts::figure_layout(&dir, ir);
                linked.palettes = level_scripts::level_palettes(&dir, &name, ir);
                linked
            })
        })
    });
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
    fog: ResMut<'w, LevelFog>,
    extraction: Option<Res<'w, Extraction>>,
}

/// One step of the load a frame: the task's result taken, a frame's share
/// handed to the renderer, the level spawned hidden once all of it is,
/// ready once the renderer has it all and the warm has its keys.
fn step_level(mut level: ResMut<Level>, mut warm: ResMut<Warm>, mut s: LevelSpawner) {
    let level = &mut *level;
    poll_scripts(level, &mut s.commands);
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
                    triangles = built.walk.triangles,
                    start = ?built.walk.start,
                    lights_left_out = ?built.lights.left_out,
                    fog = ?built.fog,
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
                r.triangles = built.walk.triangles;
                r.start = Some(built.walk.start.to_array());
                r.read_ms = Some(built.read.as_millis() as u64);
                r.built_ms = since.map(|d| d.as_millis() as u64);
                // The start's fog is the TV's; the warm compiles the keys
                // while the frames upload.
                let fog = built.fog;
                *s.fog = fog;
                let scene = Arc::new(level_scene(&built.lights, &fog));
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
                    fog,
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

/// The script part's task, reported once it's done: what linked and what
/// it cost, or why it didn't, and the run installed from it (AH.16). The
/// level goes on either way.
fn poll_scripts(level: &mut Level, commands: &mut Commands) {
    let Some(task) = level.scripts.as_mut() else {
        return;
    };
    let Some(result) = check_ready(task) else {
        return;
    };
    level.scripts = None;
    let done_ms = level.ms();
    match result {
        Ok(linked) => {
            let report = ScriptsReport {
                files: linked.files,
                bytes: linked.bytes,
                scripts: linked.program.scripts.len(),
                ops: linked.program.ops.len(),
                defs: linked.program.defs.len(),
                read_ms: linked.read.as_millis() as u64,
                parsed_ms: linked.parsed.as_millis() as u64,
                linked_ms: linked.linked.as_millis() as u64,
                done_ms,
            };
            info!(
                level = ?level.report.level,
                files = report.files,
                bytes = report.bytes,
                scripts = report.scripts,
                ops = report.ops,
                defs = report.defs,
                read_ms = report.read_ms,
                parsed_ms = report.parsed_ms,
                linked_ms = report.linked_ms,
                done_ms = ?report.done_ms,
                "the level's scripts linked"
            );
            level.report.scripts = Some(report);
            let level_scripts::Linked {
                program,
                placements,
                order,
                fighters,
                layout,
                palettes,
                ..
            } = linked;
            let layout = match layout {
                Ok(l) => Some(l),
                Err(why) => {
                    warn!(reason = %why, "no figure's words to lay out: a hero's scripts see no figure");
                    None
                }
            };
            let palettes = palettes.unwrap_or_else(|why| {
                warn!(reason = %why, "the level's palettes didn't read: its placements' clips play nothing");
                Vec::new()
            });
            commands.queue(move |world: &mut World| {
                crate::hero_scripts::install(
                    world,
                    crate::hero_scripts::Part {
                        program: std::sync::Arc::new(program),
                        files: placements,
                        order,
                        fighters,
                        layout,
                        palettes,
                    },
                );
            });
        }
        Err(why) => {
            warn!(level = ?level.report.level, reason = %why, "the level's scripts didn't load; it's drawn without them");
            level.report.scripts_reason = Some(why);
        }
    }
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
    view_scene(lights, fog, fresh_state_view())
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
    let scene = world_scene(&up.built.lights, &up.fog);
    let start = up.materials.len();
    let end = (start + MATERIALS_PER_FRAME).min(up.built.drawn.materials.len());
    for (k, m) in up.built.drawn.materials[start..end].iter().enumerate() {
        let texture = |t: &model::TextureSlot| -> Handle<Image> {
            let key = (
                t.texture.clone(),
                remaster::samples_srgb(m, t.role),
                t.sampler,
            );
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
            Ok(mut material) => {
                material.uniform = level_uniform(material.uniform);
                material.scene_lights = LEVEL_LIGHTS;
                material.depth_bias = up.built.depth_bias[start + k];
                s.materials.add(material)
            }
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

/// A level material's uniform as it draws: lit in the world's frame with
/// its point lights off the level's buffer ([`LEVEL_LIGHTS`], openspec
/// AH.65), a multiplying rim's strength in light (AG.79). The upload and a
/// fog's relight both finish it here, so a relight keeps the rim's.
pub(crate) fn level_uniform(u: CharacterUniform) -> CharacterUniform {
    u.lit_by_scene().multiplies_in_light()
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
        mut built,
        mesh_handles,
        materials,
        fog,
        ..
    } = up;
    let level_meshes = built.drawn.segments.len();
    let bodies: Vec<Entity> = std::mem::take(&mut built.walk.bodies)
        .into_iter()
        .map(|b| b.spawn(&mut s.commands))
        .collect();
    let root = s
        .commands
        .spawn((
            Name::new(format!("level {}", built.level)),
            Transform::IDENTITY,
            Visibility::Hidden,
        ))
        .id();
    for (seg, mesh) in built
        .drawn
        .segments
        .iter()
        .zip(&mesh_handles[..level_meshes])
    {
        s.commands.spawn((
            Mesh3d(mesh.clone()),
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
    let mut props = StandingProps::new(
        &mut built.walk,
        &built.prop_models,
        &mesh_handles,
        &materials,
        root,
        &mut s.commands,
    );
    // Standing by the start until a hero's step says where he is.
    props.stand(built.walk.start, |_| false, &mut s.commands);
    let spawns = built
        .spawn_models
        .iter()
        .map(|(path, parts)| {
            let parts = parts
                .iter()
                .map(|&(mesh, material)| {
                    (
                        mesh_handles[mesh].clone(),
                        materials[usize::from(material)].clone(),
                    )
                })
                .collect();
            (path.clone(), parts)
        })
        .collect();
    Loaded {
        root,
        materials,
        reference,
        scrolling,
        lights: built.lights,
        keys: built.keys.iter().map(|(k, _)| *k).collect(),
        lit_fog: fog,
        camera: None,
        shown_at: None,
        bodies,
        props,
        spawns,
        start: built.walk.start,
        also: Vec::new(),
        frame_lights: None,
    }
}

/// Each fixed step before the hero's (AG.25): the props stand by where he
/// stands as the step begins, so his step meets the solids the original's
/// would; before he's in, by the level's start. After the scripts' step
/// (openspec AH.21), so a breakable they broke this step is gone before
/// anything moves.
fn stand_props(
    mut commands: Commands,
    mut level: ResMut<Level>,
    heroes: Query<&HeroBody>,
    scripts: Option<Res<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
) {
    let Some(loaded) = level.stage.loaded_mut() else {
        return;
    };
    let at = heroes.iter().next().map_or(loaded.start, |h| h.hero.pivot);
    let unit = source.as_deref().and_then(ScriptSource::level_unit);
    let taken_off = |object: u32| match (scripts.as_deref(), unit) {
        (Some(w), Some(unit)) => w.taken_off(Obj {
            unit,
            index: object,
        }),
        _ => false,
    };
    loaded.props.stand(at, taken_off, &mut commands);
}

/// The camera the level is drawn from into `target` at `order`: the
/// remaster's at AF's fresh-state camera, replacing what's there
/// ([`VIEW_OUTPUT`]), the game's glow run on its frame (openspec AH.65,
/// [`FrameGlow`]: whoever spawns it gives it its [`spawn_alpha_clear`] on
/// its layer). The walk's and the dev entry's under the TV; the
/// start's pass draws one under it too ([`crate::prepare`]), so the output's
/// pipeline for the TV's target and that write, and the TV's own write over
/// it ([`TV_OVER`]), are compiled before the level first shows (AG.42,
/// AG.50).
pub(crate) fn view_camera(target: RenderTarget, order: isize) -> impl Bundle {
    (
        remaster::camera(),
        Camera {
            order,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            output_mode: VIEW_OUTPUT,
            ..default()
        },
        target,
        Projection::Perspective(fresh_state_projection()),
        fresh_state_transform(),
        LevelOutput,
        FrameGlow,
    )
}

/// Marks a camera writing the level's output onto a target
/// ([`view_camera`]): the level's own, and the start's pass's stand-in.
#[derive(Component, Debug, Default)]
pub(crate) struct LevelOutput;

/// The TV's UI over the level (AG.50; design decision 13: the level takes
/// the cards' place on the TV, which keeps its join code): whenever a level
/// output draws under the TV's camera on its target, the TV writes over it
/// ([`TV_OVER`]); while the level itself shows there, the TV also clears to
/// no colour and hides its screen (the sky, the portal's header and cards),
/// so only TV UI shown on purpose lands on the level. Start's overlay and
/// the controller message do ([`crate::pause`]), as an intro would; the
/// level's small join code is AG.29's. Before the visibility pass, so the
/// frame the level shows or goes draws the TV the right way.
pub(crate) fn tv_over_level(
    outputs: Query<(&Camera, Option<&RenderTarget>, Has<LevelCamera>), With<LevelOutput>>,
    mut tv: Query<(&mut Camera, Option<&RenderTarget>), (With<TvCamera>, Without<LevelOutput>)>,
    mut screens: Query<&mut Visibility, With<ScreenRoot>>,
) {
    let Ok((mut camera, target)) = tv.single_mut() else {
        return;
    };
    let mut under = false;
    let mut level = false;
    for (output, at, is_level) in &outputs {
        if output.is_active && output.order < camera.order && same_target(at, target) {
            under = true;
            level |= is_level;
        }
    }
    let output = if under {
        TV_OVER
    } else {
        CameraOutputMode::default()
    };
    if !same_output(&camera.output_mode, &output) {
        camera.output_mode = output;
    }
    let clear = if level {
        ClearColorConfig::Custom(Color::NONE)
    } else {
        ClearColorConfig::Default
    };
    if !same_clear(&camera.clear_color, &clear) {
        camera.clear_color = clear;
    }
    let shown = if level {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut screen in &mut screens {
        screen.set_if_neq(shown);
    }
}

/// Whether UI drawn through the camera `ui` shows over the level drawn
/// through the camera `level` (AG.50): both drawing, on one target, `ui`
/// after it, cleared to no colour and laid over rather than replacing. The
/// rule [`tv_over_level`] keeps for the TV; the mirror reports it.
pub(crate) fn draws_over(
    (ui, ui_target): (&Camera, Option<&RenderTarget>),
    (level, level_target): (&Camera, Option<&RenderTarget>),
) -> bool {
    let clear = matches!(ui.clear_color, ClearColorConfig::Custom(c) if c.alpha() == 0.0);
    let laid_over = match ui.output_mode {
        CameraOutputMode::Write { blend_state, .. } => blend_state != Some(BlendState::REPLACE),
        CameraOutputMode::Skip => false,
    };
    ui.is_active
        && level.is_active
        && ui.order > level.order
        && same_target(ui_target, level_target)
        && clear
        && laid_over
}

/// One target, a camera without one on the primary window as Bevy has it.
fn same_target(a: Option<&RenderTarget>, b: Option<&RenderTarget>) -> bool {
    let normal = |t: Option<&RenderTarget>| {
        t.cloned()
            .unwrap_or_default()
            .normalize(Some(Entity::PLACEHOLDER))
    };
    normal(a) == normal(b)
}

fn same_clear(a: &ClearColorConfig, b: &ClearColorConfig) -> bool {
    match (a, b) {
        (ClearColorConfig::Default, ClearColorConfig::Default)
        | (ClearColorConfig::None, ClearColorConfig::None) => true,
        (ClearColorConfig::Custom(a), ClearColorConfig::Custom(b)) => a == b,
        _ => false,
    }
}

fn same_output(a: &CameraOutputMode, b: &CameraOutputMode) -> bool {
    match (a, b) {
        (CameraOutputMode::Skip, CameraOutputMode::Skip) => true,
        (
            CameraOutputMode::Write {
                blend_state: a,
                clear_color: ca,
            },
            CameraOutputMode::Write {
                blend_state: b,
                clear_color: cb,
            },
        ) => a == b && same_clear(ca, cb),
        _ => false,
    }
}

/// The dev entry: once the level is ready, the remaster's camera at AF's
/// fresh-state camera, drawing the level into the TV's target under its UI.
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
    let camera = commands
        .spawn((
            view_camera(target, VIEW_ORDER),
            RenderLayers::layer(LEVEL_LAYER),
            LevelCamera,
            Name::new("level view"),
        ))
        .id();
    spawn_alpha_clear(&mut commands, camera, LEVEL_LAYER);
    level.show(&mut commands, camera, time.elapsed());
    info!(level = ?level.report.level, "the level shows (GUAC_LEVEL_VIEW)");
    redrawn.write(Redrawn::Level);
}

/// A fog change relights every material: the level's in its scene, the
/// hero's in his ([`Level::light_also`]), each in the world's frame, which
/// the shader turns by the view it draws for, so a camera that moves
/// rewrites none (lit in view space, a following camera rewrote all of
/// chapter 1's 471 every frame it moved, and every frame at rest too, since
/// the eased camera's rotation jitters by float noise).
fn light_level(
    mut level: ResMut<Level>,
    fog: Res<LevelFog>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let level = &mut *level;
    let Some(loaded) = level.stage.loaded_mut() else {
        return;
    };
    if *fog == loaded.lit_fog {
        return;
    }
    loaded.lit_fog = *fog;
    level.report.relit += 1;
    let scene = world_scene(&loaded.lights, &fog);
    for (h, reference) in loaded.materials.iter().zip(&loaded.reference) {
        let Some((reference, alpha)) = reference else {
            continue;
        };
        if let Some(mut m) = materials.get_mut(h) {
            let rows = (m.uniform.uv_u, m.uniform.uv_v);
            m.uniform = level_uniform(CharacterUniform::new(
                reference,
                &scene,
                ShadowLookup::NONE,
                *alpha,
            ));
            (m.uniform.uv_u, m.uniform.uv_v) = rows;
        }
    }
    relight(
        &loaded.also,
        &hero_scene(&loaded.lights, &fog, Mat4::IDENTITY),
        &mut materials,
    );
}

/// The frame's three point lights into the level's buffer (openspec
/// AH.65, [`LEVEL_LIGHTS`]): the level's placed ones and those its scripts
/// lit ([`ScriptWorld::lit_lights`]), the nearest the hero (else the
/// level's start) first ([`arithmetic::frame_point_lights`]). Rewritten
/// only when they change, one buffer whatever the level holds: every level
/// material, prop, clone and the hero read their point lights off it.
fn light_frame(
    mut level: ResMut<Level>,
    scripts: Option<Res<ScriptWorld>>,
    heroes: Query<&HeroBody>,
    buffers: Option<ResMut<Assets<ShaderBuffer>>>,
) {
    let level = &mut *level;
    let Some(loaded) = level.stage.loaded_mut() else {
        return;
    };
    let focus = heroes.iter().next().map_or(loaded.start, |h| h.hero.pivot);
    let lit: Vec<PointLight> = scripts
        .as_deref()
        .map(|w| {
            w.lit_lights()
                .iter()
                .map(|(at, l)| arithmetic::script_light(*at, l))
                .collect()
        })
        .unwrap_or_default();
    let frame = arithmetic::frame_point_lights(&loaded.lights.points, &lit, focus);
    if loaded.frame_lights == Some(frame) {
        return;
    }
    loaded.frame_lights = Some(frame);
    level.report.lights = frame
        .iter()
        .filter(|p| *p != &arithmetic::DARK)
        .map(|p| FrameLight {
            position: p.position,
            colour: p.colour,
            linear: p.linear,
            quadratic: p.quadratic,
        })
        .collect();
    // Headless (no renderer) there's no buffer to write; the report holds
    // the lights.
    if let Some(mut buffers) = buffers
        && let Some(mut b) = buffers.get_mut(&LEVEL_LIGHTS)
    {
        *b = ScenePointLights::new(&frame).buffer();
    }
}

/// A level part's glow mask (openspec AH.65): a child of the part drawing
/// its mesh (on its skin, if it has one) on the level's layer with its
/// material's level glow ([`CharacterMaterial::level_glow`]).
#[derive(Component, Debug)]
pub(crate) struct GlowTwin;

/// A mesh [`twin_level_glow`] has looked at.
#[derive(Component, Debug)]
struct Twinned;

/// Each part material's level glow, made once: `None` for one that
/// doesn't glow.
#[derive(Resource, Debug, Default)]
struct GlowTwins(HashMap<AssetId<CharacterMaterial>, Option<Handle<CharacterMaterial>>>);

/// Every glowing part on the level's layer gets its glow mask
/// ([`GlowTwin`]): the level's own, its props', what its scripts spawn and
/// the hero's, as each is spawned. The mask goes with the part (a child,
/// its visibility and its skin), so a clone hidden or taken off takes it
/// along. What the mask draws is its material's when the part came up: a
/// tint the scripts lay on a clone later (AH.67) moves its colour, not its
/// glow amount.
fn twin_level_glow(
    mut commands: Commands,
    mut twins: ResMut<GlowTwins>,
    materials: Option<ResMut<Assets<CharacterMaterial>>>,
    parts: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<CharacterMaterial>,
            &RenderLayers,
            Option<&SkinnedMesh>,
            Has<NoFrustumCulling>,
        ),
        (Without<Twinned>, Without<GlowTwin>),
    >,
) {
    let Some(mut materials) = materials else {
        return;
    };
    // A material gone (a level or a hero left) takes its mask's with it.
    twins.0.retain(|id, _| materials.contains(*id));
    let level = RenderLayers::layer(LEVEL_LAYER);
    for (part, mesh, material, layers, skin, unculled) in &parts {
        commands.entity(part).insert(Twinned);
        if *layers != level {
            continue;
        }
        let id = material.0.id();
        let twin = match twins.0.get(&id) {
            Some(t) => t.clone(),
            None => {
                let made = materials
                    .get(&material.0)
                    .and_then(CharacterMaterial::level_glow)
                    .map(|m| materials.add(m));
                twins.0.insert(id, made.clone());
                made
            }
        };
        let Some(twin) = twin else {
            continue;
        };
        let mut e = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(twin),
            Transform::IDENTITY,
            level.clone(),
            GlowTwin,
            ChildOf(part),
        ));
        if let Some(skin) = skin {
            e.insert(skin.clone());
        }
        if unculled {
            e.insert(NoFrustumCulling);
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
pub(crate) mod tests {
    use super::*;
    use crate::turntable::tests::{app, extract, identity};
    use crate::tv::TV_ORDER;
    use crate::warm::PipelineWatch;
    use guac_formats::texture::{Format, Texture};
    use guac_ir::level::{
        Atmosphere, AtmosphericZone, EffectMaterial, LevelMaterial, Lights, PlacedLight,
        SceneGraph, SkyGroup, ZoneFog, ZoneLight,
    };
    use guac_ir::model::{
        Blend, Filter, Glow, Mask, Params, RenderStates, TextureRole, TextureSlot, Wrap,
    };
    use guac_ir::{RigLight, Sha256};
    use guacamole::character::warm::MeshShape;
    use guacamole::level_scene::level_lights;
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
    pub(crate) fn lights() -> Lights {
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

    /// The start's zone: made-up lights at the rig's angles, and the fog
    /// AG.13 read off the opening area at AF's fresh state, (115, 83, 49)
    /// from 100 ft over 1,000 ft, the sky dome taking none.
    fn zone() -> AtmosphericZone {
        let light = |facing, tilt| ZoneLight {
            diffuse: [90, 80, 70, 0],
            specular: [60, 60, 60, 0],
            facing,
            tilt,
        };
        AtmosphericZone {
            id: 0,
            ambient: [20, 20, 20, 0],
            lights: [light(0, -45), light(180, 45), light(90, -45)],
            fog: ZoneFog {
                on: 1,
                start: 100,
                range: 1000,
                colour: [115, 83, 49, 0],
                sky_dome_percent: 0,
                sky_dome_colour: [128, 128, 128, 0],
                yon_colour: [115, 83, 49, 0],
            },
        }
    }

    fn atmosphere() -> Atmosphere {
        Atmosphere {
            identity: identity(),
            level: LEVEL.into(),
            script: "C:/tfb/Content/Levels/Level_900/Scripts/Level_Definition_900.ai".into(),
            zones: vec![zone()],
            start: Some(0),
        }
    }

    /// A made-up chapter 1 beside `extract`'s models: a ground layered and
    /// normal-mapped on two mesh shapes, a scrolling sky, and grass, which
    /// isn't drawn; its five textures.
    pub(crate) fn write_level(dir: &Path, lights: &Lights) -> LevelSource {
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
        std::fs::write(
            dir.join(ir.level_path(LEVEL, ATMOSPHERE)),
            guac_ir::to_ron(&atmosphere()).unwrap(),
        )
        .unwrap();
        write_walk(dir);
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

    /// Where [`write_walk`]'s hero starts, and its floor's height.
    pub(crate) const START: [f32; 3] = [10.0, 40.0, -90.0];
    pub(crate) const FLOOR: f32 = 39.99;

    /// How far ahead of the start, along +z, the wall stands.
    pub(crate) const WALL: f32 = 6.0;

    /// The walk's two parts beside a made-up level: a 40 m wide floor under
    /// the start, ending [`WALL`] ahead of it along +z at a 3 m wall that
    /// shares its edge (level 213's mesh is indexed the same way), and
    /// `213_Init_Location` with its first waypoint at the start.
    pub(crate) fn write_walk(dir: &Path) {
        let ir = IrFormat::Ron;
        let [x, z] = [START[0], START[2]];
        let wall_z = z + WALL;
        let floor = guac_ir::level::CollisionMesh {
            world: "Levels/Test/Test_ART_MASTER.ma".into(),
            vertices: vec![
                [x - 20.0, FLOOR, z - 20.0],
                [x + 20.0, FLOOR, z - 20.0],
                [x + 20.0, FLOOR, wall_z],
                [x - 20.0, FLOOR, wall_z],
                [x + 20.0, FLOOR + 3.0, wall_z],
                [x - 20.0, FLOOR + 3.0, wall_z],
            ],
            // Up, then the wall's face toward the start (-z).
            triangles: vec![[0, 2, 1], [0, 3, 2], [3, 4, 2], [3, 5, 4]],
            materials: vec![1; 4],
            radius: 0.05,
            welding: 4,
        };
        let collision = guac_ir::level::Collision {
            identity: identity(),
            level: LEVEL.into(),
            meshes: vec![floor],
        };
        let placements = guac_ir::level::Placements {
            identity: identity(),
            level: LEVEL.into(),
            actors: vec![guac_ir::level::Placement {
                object: 1,
                id: 0,
                name: guacamole::level_scene::START_PLACEMENT.into(),
                script: None,
                position: [0.0; 3],
                rotation: [0.0; 3],
                scale: [100.0; 3],
                model: None,
                solid: None,
                movement: None,
                waypoints: Some(0),
                activity_state: 4,
                activation_range: 0.0,
            }],
            waypoints: vec![vec![guac_ir::level::Waypoint {
                name: "wp0".into(),
                position: START,
                rotation: [0.0; 3],
                height: 0.0,
            }]],
        };
        std::fs::create_dir_all(dir.join("levels").join(LEVEL)).unwrap();
        std::fs::write(
            dir.join(ir.level_path(LEVEL, COLLISION)),
            guac_ir::to_ron(&collision).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join(ir.level_path(LEVEL, PLACEMENTS)),
            guac_ir::to_ron(&placements).unwrap(),
        )
        .unwrap();
        // No props.
        write_props(dir, Vec::new(), Vec::new());
    }

    /// The props part holding `models`, and the placements part with
    /// `actors` after [`write_walk`]'s `213_Init_Location`.
    pub(crate) fn write_props(
        dir: &Path,
        models: Vec<guac_ir::level::Prop>,
        actors: Vec<guac_ir::level::Placement>,
    ) {
        let ir = IrFormat::Ron;
        let props = guac_ir::level::Props {
            identity: identity(),
            level: LEVEL.into(),
            props: models,
            refused: Vec::new(),
        };
        std::fs::write(
            dir.join(ir.level_path(LEVEL, PROPS)),
            guac_ir::to_ron(&props).unwrap(),
        )
        .unwrap();
        if actors.is_empty() {
            return;
        }
        let path = dir.join(ir.level_path(LEVEL, PLACEMENTS));
        let mut placements: guac_ir::level::Placements =
            guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
        placements.actors.extend(actors);
        std::fs::write(path, guac_ir::to_ron(&placements).unwrap()).unwrap();
    }

    /// The turntables' test app with the level loader, fed `source`.
    pub(crate) fn level_app(dir: &Path, source: LevelSource, gpu: bool) -> App {
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

    /// Frames, `between` played before each, until the level's report
    /// passes `done`, up to 5 s.
    fn until_report(
        app: &mut App,
        done: impl Fn(&LevelReport) -> bool,
        mut between: impl FnMut(&mut App),
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(app.world().resource::<Level>().report()) {
            assert!(Instant::now() < deadline, "stuck at {}", state(app));
            between(app);
            app.update();
        }
    }

    fn report(app: &App) -> LevelReport {
        app.world().resource::<Level>().report().clone()
    }

    /// AG.19: the level is ready only once the renderer has every texture,
    /// mesh and material resident and the warm has compiled every key it
    /// draws; meanwhile it stays spawned hidden, waiting. Here the warm
    /// finishes first, so only residency holds it.
    #[test]
    fn the_level_is_ready_only_once_every_part_is_resident() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, true);
        until(&mut app, &["waiting"], |_| {});
        let r = report(&app);
        // Four ground maps and the sky's, four meshes (the grass's isn't
        // drawn), two materials: none resident with no render world.
        assert_eq!((r.textures, r.segments, r.materials), (5, 4, 2));
        assert_eq!((r.left_out, r.segments_left_out), (1, 1));
        assert_eq!(r.sky_segments, 1);
        assert_eq!(r.not_resident, 5 + 4 + 2, "{r:?}");
        assert_eq!(r.keys, 3);

        // Every key compiled: still waiting, on the renderer alone.
        let pipelines = app.world().resource::<PipelineWatch>().clone();
        until_report(
            &mut app,
            |r| r.keys_waiting == 0,
            |_| pipelines.compile_all(),
        );
        for _ in 0..10 {
            app.update();
        }
        let r = report(&app);
        assert_eq!(
            (r.state.as_str(), r.not_resident, r.keys_waiting),
            ("waiting", 11, 0),
            "nothing resident"
        );

        // The render world strikes off all but one texture, then that.
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
        assert_eq!(
            (state(&app), report(&app).not_resident),
            ("waiting".into(), 1)
        );
        watch.lock().images.remove(&last);
        app.update();
        let level = app.world().resource::<Level>();
        let r = level.report();
        assert_eq!(
            (r.state.as_str(), r.not_resident, r.keys_waiting),
            ("ready", 0, 0)
        );
        assert!(r.ready_ms.is_some() && !r.shown);
        let root = level.loaded().unwrap().root;
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden),
            "nothing shows it"
        );
    }

    /// AH.22: the level's script part links on a task of its own and the
    /// report says what linked; without the part the level loads all the
    /// same, the reason reported.
    #[test]
    fn the_script_part_links_beside_the_load_and_a_missing_one_is_reported() {
        use guac_formats::roster::builder::{disc, sample};
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let d = disc(&sample());
        let (creature, _) = guac_formats::script::builder::creature();
        let entries = [&d.bootstrap, &d.global, &d.init_setup, &creature, &creature];
        for ((rel, _), bytes) in level_scripts::files(LEVEL).iter().zip(entries) {
            let path = tmp.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let mut app = level_app(tmp.path(), source.clone(), false);
        until_report(&mut app, |r| r.scripts.is_some(), |_| {});
        let r = report(&app);
        let linked = r.scripts.as_ref().unwrap();
        // The portal manager, three Init_Setup scripts, the creature twice.
        assert_eq!((linked.files, linked.scripts), (5, 6));
        assert!(linked.ops > 0 && linked.done_ms.is_some(), "{linked:?}");
        assert_eq!(r.scripts_reason, None);
        until(&mut app, &["ready"], |_| {});

        // The fighter's entry gone: the level is ready without the scripts.
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        until_report(&mut app, |r| r.scripts_reason.is_some(), |_| {});
        let r = report(&app);
        assert!(r.scripts.is_none());
        assert!(
            r.scripts_reason
                .as_deref()
                .is_some_and(|w| w.starts_with("scripts/bootstrap.igz: ")),
            "{r:?}"
        );
    }

    /// The other order: everything resident, the level still waits on the
    /// warm until every key it draws is compiled.
    #[test]
    fn a_resident_level_waits_on_its_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, true);
        until(&mut app, &["waiting"], |_| {});
        let watch = app.world().resource::<LevelWatch>().clone();
        *watch.lock() = Pending::default();
        for _ in 0..10 {
            app.update();
        }
        let r = report(&app);
        assert_eq!((r.state.as_str(), r.not_resident), ("waiting", 0));
        assert!(r.keys_waiting > 0, "{r:?}");
        let pipelines = app.world().resource::<PipelineWatch>().clone();
        until(&mut app, &["ready"], |_| pipelines.compile_all());
        assert_eq!(report(&app).keys_waiting, 0);
    }

    /// A second source while the first still waits on the renderer: what
    /// the first left pending goes with it, so the second waits on its own
    /// parts alone (the first's never turn up).
    #[test]
    fn a_new_source_waits_on_its_own_parts_alone() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = write_level(a.path(), &lights());
        let second = write_level(b.path(), &lights());
        let mut app = level_app(a.path(), first, true);
        until(&mut app, &["waiting"], |_| {});
        assert_eq!(report(&app).not_resident, 11);
        let (tx, feed) = level_channel();
        tx.send(second).unwrap();
        app.insert_resource(feed);
        app.update();
        assert_eq!(state(&app), "reading");
        until(&mut app, &["waiting"], |_| {});
        assert_eq!(report(&app).not_resident, 11, "the first's parts let go");
        let watch = app.world().resource::<LevelWatch>().clone();
        *watch.lock() = Pending::default();
        let pipelines = app.world().resource::<PipelineWatch>().clone();
        until(&mut app, &["ready"], |_| pipelines.compile_all());
    }

    /// AG.25: the props as entities. At the start the gourd 4.5 m off
    /// stands, drawn and solid; the coin 49.25 m off doesn't, past its
    /// 160 ft (48.768 m); a template, a creature and a placement its script
    /// wakes have no entity at all. A hero 90 m on along x stands the coin up
    /// and takes the gourd down, its solid out of Avian's world; gone, the
    /// props stand by the start again.
    #[test]
    fn the_props_stand_by_the_rule_as_entities() {
        use guac_ir::actor::{Shape, Solid};
        use guacamole::props::PROP_BODIES;
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let scene: guac_ir::level::Scene = guac_ir::from_ron(
            &std::fs::read_to_string(tmp.path().join(source.ir.level_path(LEVEL, SCENE))).unwrap(),
        )
        .unwrap();
        // The gourd on the ground's material and shape, a key the level
        // draws; the coin on the sky's, fogged as the sky isn't: a key of
        // its own.
        let materials = &scene.scenes[0].materials;
        let plain = MeshShape {
            uv_sets: 2,
            tangents: true,
            colours: false,
            skinned: false,
        };
        let bare = MeshShape {
            uv_sets: 1,
            tangents: false,
            ..plain
        };
        let model = |path: &str, skinned: bool, material: usize, shape| guac_ir::level::Prop {
            path: path.into(),
            skeleton: skinned.then(|| model::Skeleton { joints: Vec::new() }),
            materials: vec![materials[material].clone()],
            segments: vec![segment(0, shape, 0.0)],
            idle: None,
        };
        let solid = |shape, size: [f32; 3]| Solid {
            shape,
            solid: true,
            world: true,
            actor_surface: false,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width: size[0],
            height: size[1],
            length: size[2],
            weight: 1.0,
            rebound: 0.0,
        };
        let actor = |object,
                     name: &str,
                     state,
                     range,
                     dx: f32,
                     model: &str,
                     solid,
                     script: Option<&str>| {
            guac_ir::level::Placement {
                object,
                id: 0,
                name: name.into(),
                script: script.map(str::to_owned),
                position: [START[0] + dx, START[1], START[2]],
                rotation: [0.0; 3],
                scale: [100.0; 3],
                model: Some(model.into()),
                solid: Some(solid),
                movement: None,
                waypoints: None,
                activity_state: state,
                activation_range: range,
            }
        };
        let sphere = solid(Shape::Sphere, [7.25, 6.5, 3.0]);
        write_props(
            tmp.path(),
            vec![
                model("gourd.mdl", false, 0, plain),
                model("coin.mdl", false, 2, bare),
                model("chompy.mdl", true, 0, plain),
            ],
            vec![
                actor(20, "gourd", 4, 160.0, 4.5, "gourd.mdl", sphere, None),
                actor(
                    21,
                    "coin",
                    4,
                    160.0,
                    49.25,
                    "coin.mdl",
                    solid(Shape::None, [3.0, 6.0, 3.0]),
                    Some("Placed_Loot_Spinning.ai"),
                ),
                actor(22, "template", 5, 160.0, 1.0, "gourd.mdl", sphere, None),
                actor(
                    23,
                    "chompy",
                    4,
                    260.0,
                    3.0,
                    "chompy.mdl",
                    solid(Shape::Capsule, [3.0, 6.0, 3.0]),
                    Some("S2_Enemy_Chompy.ai"),
                ),
                actor(
                    24,
                    "fence",
                    4,
                    0.0,
                    2.0,
                    "gourd.mdl",
                    solid(Shape::Box, [23.0, 18.0, 3.0]),
                    Some("WoodFence_Spawner.ai"),
                ),
            ],
        );
        let mut app = level_app(tmp.path(), source, false);
        // Avian's side of the seam, so a ray says what its world holds.
        app.add_plugins(crate::physics::SeamPlugin);
        app.finish();
        app.cleanup();
        until(&mut app, &["ready"], |_| {});
        // What a ray straight down onto the gourd meets in Avian's world,
        // once a fixed step's physics schedule has run.
        let down_on_the_gourd = |app: &mut App| {
            use guac_motion::seam::PhysicsSeam;
            let mut state =
                bevy::ecs::system::SystemState::<crate::physics::AvianSeam>::new(app.world_mut());
            let seam = state.get(app.world()).unwrap();
            let at = Vec3::new(START[0] + 4.5, START[1] + 10.0, START[2]);
            seam.ray(at, -Vec3::Y, 20.0, &[]).map(|h| h.body.0)
        };
        let props = |app: &App| {
            app.world()
                .resource::<Level>()
                .snapshot()
                .props
                .expect("the props' report")
        };
        let r = props(&app);
        // By script: the fence and `213_Init_Location`.
        assert_eq!(
            (r.props, r.models, r.templates, r.creatures, r.by_script),
            (2, 2, 1, 1, 2),
            "{r:?}"
        );
        assert_eq!((r.standing, r.drawn, r.solid, r.sensors), (1, 1, 1, 0));
        // Every model's keys, standing or not: the level's three and the
        // coin's, asked of the warm before it shows.
        app.update();
        let asked: HashSet<WarmKey> = app
            .world()
            .resource::<Warm>()
            .level_listed()
            .iter()
            .copied()
            .collect();
        assert_eq!((report(&app).keys, asked.len()), (4, 4));
        assert_eq!(r.names, ["gourd"]);
        let shown = |app: &mut App| {
            let mut q = app.world_mut().query::<(&Name, &Visibility, &Children)>();
            let mut out: Vec<(String, bool)> = q
                .iter(app.world())
                .filter(|(n, ..)| n.as_str().starts_with("prop "))
                .map(|(n, v, c)| {
                    assert_eq!(c.len(), 1, "its one segment");
                    (n.as_str().to_owned(), *v == Visibility::Inherited)
                })
                .collect();
            out.sort();
            out
        };
        let bodies = |app: &mut App| {
            let mut q = app.world_mut().query::<&crate::physics::SeamBody>();
            let mut ids: Vec<u32> = q.iter(app.world()).map(|b| b.id.0).collect();
            ids.sort();
            ids
        };
        assert_eq!(
            shown(&mut app),
            [
                ("prop coin".to_owned(), false),
                ("prop gourd".to_owned(), true)
            ]
        );
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 20]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while down_on_the_gourd(&mut app) != Some(PROP_BODIES + 20) {
            assert!(
                Instant::now() < deadline,
                "the gourd never in Avian's world"
            );
            app.update();
        }

        // A hero 90 m on: the coin 40.75 m off stands, the gourd 85.5 m off
        // doesn't, its solid gone with it.
        let at = Vec3::new(START[0] + 90.0, START[1], START[2]);
        let hero = app
            .world_mut()
            .spawn(crate::hero::HeroBody {
                hero: guac_motion::hero::Hero::new(
                    guac_motion::scenarios::eruptor_solid(),
                    guac_motion::tuning::Tuning::ERUPTOR,
                    at,
                    0.0,
                ),
                exclude: Vec::new(),
                before: at,
            })
            .id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while props(&app).at != Some(at.to_array()) {
            assert!(Instant::now() < deadline, "no fixed step stood the props");
            app.update();
        }
        app.update();
        assert_eq!(props(&app).names, ["coin"]);
        assert_eq!(
            shown(&mut app),
            [
                ("prop coin".to_owned(), true),
                ("prop gourd".to_owned(), false)
            ]
        );
        assert_eq!(bodies(&mut app), [1], "the coin's solid has no shape");
        assert_eq!(
            down_on_the_gourd(&mut app),
            Some(1),
            "the floor, the gourd gone"
        );

        app.world_mut().despawn(hero);
        let start = app.world().resource::<Level>().loaded().unwrap().start;
        let deadline = Instant::now() + Duration::from_secs(5);
        while props(&app).at != Some(start.to_array()) {
            assert!(Instant::now() < deadline, "no fixed step stood the props");
            app.update();
        }
        app.update();
        assert_eq!(props(&app).names, ["gourd"]);
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 20]);
        assert_eq!(down_on_the_gourd(&mut app), Some(PROP_BODIES + 20));

        // AH.21: the level's scripts take the gourd's placement off (broken):
        // the next fixed step hides its draw and takes its solid out of
        // Avian's world, for good wherever the hero stands. Another unit's
        // object of the same index stands as before.
        use guacamole::scripts::{Files, Placed, ScriptWorld, headless_objects};
        let gourd = Obj { unit: 4, index: 20 };
        let files = Files {
            placed: vec![Placed::named(gourd, "gourd", None, 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(gourd, 0x20)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let source = crate::hero_scripts::ScriptSource::of(
            Arc::new(guac_script::Program::default()),
            files,
            vec![2, 4],
        );
        app.insert_resource(source);
        app.insert_resource(world);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(props(&app).names, ["gourd"], "not taken off yet");
        world = app.world_mut().remove_resource::<ScriptWorld>().unwrap();
        assert!(world.take_off(gourd));
        assert!(!world.take_off(Obj { unit: 2, index: 20 }));
        app.insert_resource(world);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !props(&app).names.is_empty() {
            assert!(Instant::now() < deadline, "the gourd never went");
            app.update();
        }
        let r = props(&app);
        assert_eq!(
            (r.standing, r.solid, r.taken_off.clone()),
            (0, 0, vec!["gourd".to_owned()])
        );
        assert_eq!(
            shown(&mut app),
            [
                ("prop coin".to_owned(), false),
                ("prop gourd".to_owned(), false)
            ]
        );
        assert_eq!(bodies(&mut app), [1]);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(down_on_the_gourd(&mut app), Some(1), "the floor, for good");
    }

    /// AG.21: the level's solids come and go with it. A new source's load
    /// takes the old one's static bodies out of Avian's world, so the hero
    /// never stands on a level that's gone.
    #[test]
    fn a_new_load_takes_the_old_levels_solids_with_it() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = write_level(a.path(), &lights());
        let second = write_level(b.path(), &lights());
        let mut app = level_app(a.path(), first, false);
        until(&mut app, &["ready"], |_| {});
        let bodies = |app: &mut App| {
            let mut q = app.world_mut().query::<&crate::physics::SeamBody>();
            q.iter(app.world()).count()
        };
        assert_eq!(bodies(&mut app), 1, "write_walk's one mesh");
        assert_eq!(report(&app).triangles, 4);
        let (tx, feed) = level_channel();
        tx.send(second).unwrap();
        app.insert_resource(feed);
        app.update();
        assert_eq!(state(&app), "reading");
        assert_eq!(bodies(&mut app), 0, "the old level's gone");
        until(&mut app, &["ready"], |_| {});
        assert_eq!(bodies(&mut app), 1);
    }

    /// AG.21: the walk's collision and placements parts are held to the
    /// dump and level as the drawn parts are, and a level without
    /// `213_Init_Location`'s waypoint has no start.
    #[test]
    fn the_walks_parts_are_this_dumps_and_levels() {
        use guac_ir::level::{Collision, Placements};
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let path = |part| tmp.path().join(source.ir.level_path(LEVEL, part));
        let collision: Collision =
            guac_ir::from_ron(&std::fs::read_to_string(path(COLLISION)).unwrap()).unwrap();
        let placements: Placements =
            guac_ir::from_ron(&std::fs::read_to_string(path(PLACEMENTS)).unwrap()).unwrap();
        let read = || read_level(&source, CompressedImageFormats::BC).err();
        assert_eq!(read(), None);
        let write = |part, ron: String| std::fs::write(path(part), ron).unwrap();

        let mut other = collision.clone();
        other.identity.manifest_hash = Sha256([9; 32]);
        write(COLLISION, guac_ir::to_ron(&other).unwrap());
        assert_eq!(
            read().as_deref(),
            Some("the collision part is another dump's or level's")
        );
        let mut other = collision.clone();
        other.level = "level_999".into();
        write(COLLISION, guac_ir::to_ron(&other).unwrap());
        assert_eq!(
            read().as_deref(),
            Some("the collision part is another dump's or level's")
        );
        write(COLLISION, guac_ir::to_ron(&collision).unwrap());

        let mut other = placements.clone();
        other.identity.manifest_hash = Sha256([9; 32]);
        write(PLACEMENTS, guac_ir::to_ron(&other).unwrap());
        assert_eq!(
            read().as_deref(),
            Some("the placements part is another dump's or level's")
        );
        let mut other = placements.clone();
        other.actors[0].name = "213_Not_Init".into();
        write(PLACEMENTS, guac_ir::to_ron(&other).unwrap());
        assert_eq!(
            read().as_deref(),
            Some("the level has no 213_Init_Location with a waypoint")
        );
    }

    fn asset<A: Asset>(n: u128) -> AssetId<A> {
        AssetId::Uuid {
            uuid: bevy::asset::uuid::Uuid::from_u128(n),
        }
    }

    /// The render world's word strikes off exactly what it says is resident.
    #[test]
    fn the_render_world_strikes_off_only_what_is_resident() {
        let mut p = Pending {
            images: [asset(1), asset(2)].into(),
            meshes: [asset(3), asset(4)].into(),
            materials: [asset(5), asset(6)].into(),
        };
        p.strike(|i| i == asset(1), |m| m == asset(4), |m| m == asset(6));
        assert_eq!(p.images, [asset(2)].into());
        assert_eq!(p.meshes, [asset(3)].into());
        assert_eq!(p.materials, [asset(5)].into());
        p.strike(|_| false, |_| false, |_| false);
        assert_eq!(p.len(), 3);
        p.strike(|_| true, |_| true, |_| true);
        assert_eq!(p.len(), 0);
    }

    /// AH.65: a level material's uniform as the upload and a fog's relight
    /// both finish it: its point lights off the level's buffer, its
    /// multiplying rim's strength in light (AG.79), so a relight keeps the
    /// rim's (the relight used to put it back to the bare strength).
    #[test]
    fn a_level_uniform_reads_the_scenes_lights_and_keeps_the_rims_strength() {
        let mut u = CharacterUniform::new(
            &guac_ir::shading::Material::new(),
            &Scene::new(),
            ShadowLookup::NONE,
            0.0,
        );
        u.fixed_alpha.w = 2.0;
        let level = level_uniform(u);
        assert_eq!(level.misc.w, guacamole::character::LIGHTS_FROM_SCENE);
        assert_eq!(level.rim.z, remaster::in_light(2.0));
        assert!((level.rim.z - 2f32.powf(2.2)).abs() < 1e-4);
    }

    /// AH.65: every glowing part on the level's layer gets its glow mask, a
    /// child drawing its mesh (on its skin) with its material's level glow;
    /// one material's mask made once for every part on it; a part that
    /// doesn't glow, or isn't the level's, gets none.
    #[test]
    fn every_glowing_level_part_gets_its_glow_mask() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        let key = guacamole::census::LEVEL[1].key(false).unwrap();
        let ir = guacamole::census::material_for(&key);
        let scene = Scene {
            fog: true,
            ..Scene::new()
        };
        let made = |ir: &model::Material| {
            CharacterMaterial::from_ir(ir, &scene, ShadowLookup::NONE, |_| Handle::default())
                .unwrap()
        };
        let mut dull = ir.clone();
        dull.params.glow = model::Glow::None;
        let (glowing, plain) = {
            let mut assets = app.world_mut().resource_mut::<Assets<CharacterMaterial>>();
            (assets.add(made(&ir)), assets.add(made(&dull)))
        };
        let mesh = Handle::<Mesh>::default();
        let mut part = |material: &Handle<CharacterMaterial>, layer: usize| {
            app.world_mut()
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    RenderLayers::layer(layer),
                ))
                .id()
        };
        let parts = [
            part(&glowing, LEVEL_LAYER),
            part(&glowing, LEVEL_LAYER),
            part(&plain, LEVEL_LAYER),
            part(&glowing, LEVEL_LAYER + 1),
        ];
        app.update();
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<(
            &ChildOf,
            &MeshMaterial3d<CharacterMaterial>,
            &RenderLayers,
        ), With<GlowTwin>>();
        let twins: Vec<(Entity, Handle<CharacterMaterial>, RenderLayers)> = q
            .iter(world)
            .map(|(c, m, l)| (c.parent(), m.0.clone(), l.clone()))
            .collect();
        let parents: Vec<Entity> = twins.iter().map(|t| t.0).collect();
        assert_eq!(parents.len(), 2, "{twins:?}");
        assert!(parents.contains(&parts[0]) && parents.contains(&parts[1]));
        assert_eq!(twins[0].1, twins[1].1, "one mask for the material");
        assert!(
            twins
                .iter()
                .all(|t| t.2 == RenderLayers::layer(LEVEL_LAYER))
        );
        let assets = world.resource::<Assets<CharacterMaterial>>();
        let mask = assets.get(&twins[0].1).unwrap();
        assert_eq!(mask.key.pass, guacamole::character::Pass::LevelGlow);
        assert_eq!(mask.key.shader, key.material.shader);
    }

    /// AH.65: the frame's point lights reach the level's report (and its
    /// buffer, under a renderer) once it's loaded: chapter 1's placed one,
    /// the scripts' none yet.
    #[test]
    fn the_frames_point_lights_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        app.update();
        let lit = report(&app).lights;
        assert_eq!(lit.len(), 1, "{lit:?}");
        let placed = &level_lights(&l, &zone()).unwrap().points[0];
        assert_eq!(
            (lit[0].position, lit[0].colour, lit[0].linear),
            (placed.position, placed.colour, placed.linear)
        );
    }

    /// AG.41: the fog AG.13 read reaches every level material: the level's
    /// read sets the TV's `LevelFog` to its start's zone's, 30.48 m to
    /// 335.28 m, and every material is made with it (the sky's too, which
    /// owns its fog off); a new one relights every material from the camera
    /// it was lit for.
    #[test]
    fn the_start_zones_fog_reaches_every_level_material() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        assert_eq!(*app.world().resource::<LevelFog>(), LevelFog::NONE);
        until(&mut app, &["ready"], |_| {});
        let fog = *app.world().resource::<LevelFog>();
        assert_eq!(fog, LevelFog::from_zone(&zone().fog).unwrap());
        assert_eq!(fog.colour, [115, 83, 49]);
        let materials = app
            .world()
            .resource::<Level>()
            .loaded()
            .unwrap()
            .materials
            .clone();
        let uniforms = |app: &App| -> Vec<(CharacterUniform, bool)> {
            let assets = app.world().resource::<Assets<CharacterMaterial>>();
            materials
                .iter()
                .map(|h| {
                    let m = assets.get(h).unwrap();
                    (m.uniform, m.key.shader.switches().fog)
                })
                .collect()
        };
        let fogged: Vec<bool> = uniforms(&app).iter().map(|(_, f)| *f).collect();
        assert_eq!(fogged, [true, false], "the ground fogged, the sky not");
        for (u, _) in uniforms(&app) {
            assert!(
                u.fog_range
                    .abs_diff_eq(Vec4::new(30.48, 335.28, 0.0, 0.0), 1e-4),
                "{:?}",
                u.fog_range
            );
            assert_eq!(
                u.fog_colour.truncate(),
                Vec3::from_array(remaster::linear([115, 83, 49]))
            );
        }
        // Made-up numbers, none of AG.13's.
        let fog = LevelFog {
            colour: [90, 60, 30],
            near: 30.0,
            far: 300.0,
        };
        app.insert_resource(fog);
        app.update();
        let lit = world_scene(&level_lights(&l, &zone()).unwrap(), &fog);
        for (u, _) in uniforms(&app) {
            assert_eq!(u.fog_range, Vec4::new(30.0, 300.0, 0.0, 0.0));
            assert_eq!(
                u.fog_colour.truncate(),
                Vec3::from_array(remaster::linear([90, 60, 30]))
            );
            assert!(
                u.light_travel[0]
                    .truncate()
                    .abs_diff_eq(Vec3::from_array(lit.lights[0].travel), 1e-6)
            );
            assert_eq!(u.misc.w, guacamole::character::LIGHTS_FROM_SCENE);
        }
        assert_eq!(app.world().resource::<Level>().report().relit, 1);
    }

    /// AG.79 and AH.65: what the upload hands each level material stays
    /// through a fog's relight: the sky's sort bias (its blended parts first,
    /// in the scene's order) and a multiplying rim's strength in light, which
    /// the relight once put back to the bare strength (the coins brown again
    /// past the first zone change).
    #[test]
    fn a_fog_relight_keeps_the_skys_bias_and_a_multiplying_rims_strength() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        let materials = app
            .world()
            .resource::<Level>()
            .loaded()
            .unwrap()
            .materials
            .clone();
        let drawn = |app: &App| -> Vec<(f32, f32)> {
            let assets = app.world().resource::<Assets<CharacterMaterial>>();
            materials
                .iter()
                .map(|h| {
                    let m = assets.get(h).unwrap();
                    (m.depth_bias, m.uniform.rim.z)
                })
                .collect()
        };
        // The ground, then the sky's copy: only the sky ranks.
        let biases: Vec<f32> = drawn(&app).iter().map(|d| d.0).collect();
        assert_eq!(biases, [0.0, arithmetic::sky_bias(0)]);
        // The made-up level has no multiplying rim: the ground gets one, so
        // the relight has a strength to keep.
        {
            let mut level = app.world_mut().resource_mut::<Level>();
            let loaded = level.stage.loaded_mut().unwrap();
            let (ground, _) = loaded.reference[0].as_mut().unwrap();
            ground.rim = Some(guac_ir::shading::Rim {
                weights: guac_ir::shading::Weights::MULTIPLY,
                ..guac_ir::shading::Rim::new(2.0)
            });
        }
        app.insert_resource(LevelFog {
            colour: [90, 60, 30],
            near: 30.0,
            far: 300.0,
        });
        app.update();
        assert_eq!(app.world().resource::<Level>().report().relit, 1);
        let after = drawn(&app);
        assert_eq!(after[0], (0.0, remaster::in_light(2.0)), "in light");
        assert_eq!(after[1].0, arithmetic::sky_bias(0), "the sky's still first");
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
            let shape = MeshShape::of_mesh(meshes.get(&mesh.0).unwrap());
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

    /// AG.41: a level lit and fogged by its start's zone fails without its
    /// atmosphere part, or with one that sets up no zone, saying why.
    #[test]
    fn a_level_without_its_start_zone_fails_and_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let path = tmp.path().join(source.ir.level_path(LEVEL, ATMOSPHERE));
        let mut none = atmosphere();
        none.start = None;
        std::fs::write(&path, guac_ir::to_ron(&none).unwrap()).unwrap();
        let mut app = level_app(tmp.path(), source.clone(), false);
        until(&mut app, &["failed"], |_| {});
        assert_eq!(
            app.world().resource::<Level>().report().reason.as_deref(),
            Some("the level sets up no atmospheric zone at its start")
        );
        std::fs::remove_file(&path).unwrap();
        let built = read_level(&source, CompressedImageFormats::BC);
        let why = built.err().unwrap();
        assert!(why.starts_with("atmosphere.ron: "), "{why}");
    }

    /// The TV's camera and its target, as [`draws_over`] takes them.
    fn tv_camera(app: &mut App) -> (Camera, Option<RenderTarget>) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Camera, Option<&RenderTarget>), With<TvCamera>>();
        let (c, t) = q.single(world).unwrap();
        (c.clone(), t.cloned())
    }

    fn camera_of(app: &App, camera: Entity) -> (Camera, Option<RenderTarget>) {
        let e = app.world().entity(camera);
        (
            e.get::<Camera>().unwrap().clone(),
            e.get::<RenderTarget>().cloned(),
        )
    }

    fn over(
        (ui, ui_at): &(Camera, Option<RenderTarget>),
        (l, l_at): &(Camera, Option<RenderTarget>),
    ) -> bool {
        draws_over((ui, ui_at.as_ref()), (l, l_at.as_ref()))
    }

    fn writes(camera: &Camera) -> Option<Option<BlendState>> {
        match camera.output_mode {
            CameraOutputMode::Write { blend_state, .. } => Some(blend_state),
            CameraOutputMode::Skip => None,
        }
    }

    /// AG.50: while the level shows, the TV's camera comes after the level's
    /// on one target, clears to no colour and lays its UI over the level,
    /// its screen hidden, so Start's overlay and the controller message land
    /// on the level; with the level gone, the TV is as it was. At AG.21's
    /// order (the level's camera after the TV's, replacing it) it covered
    /// them, and `draws_over` says so.
    #[test]
    fn the_tvs_ui_draws_over_the_level_and_its_screen_comes_back_after() {
        use bevy::ecs::system::RunSystemOnce;
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        app.insert_resource(LevelView(true));
        app.world_mut().spawn((
            Camera2d,
            Camera {
                order: TV_ORDER,
                ..default()
            },
            TvCamera,
        ));
        let screen = app
            .world_mut()
            .spawn((ScreenRoot, Visibility::default()))
            .id();
        app.update();
        let (tv, _) = tv_camera(&mut app);
        assert_eq!(writes(&tv), Some(None), "alone, Bevy's own write");
        assert!(matches!(tv.clear_color, ClearColorConfig::Default));

        until(&mut app, &["shown"], |_| {});
        app.update();
        let camera = app.world().resource::<Level>().shown_through().unwrap();
        let level = camera_of(&app, camera);
        let tv = tv_camera(&mut app);
        assert!(
            level.0.order < tv.0.order,
            "the level's camera under the TV's"
        );
        assert_eq!(VIEW_ORDER, level.0.order);
        assert!(same_target(level.1.as_ref(), tv.1.as_ref()));
        assert_eq!(
            writes(&tv.0),
            Some(Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING)),
            "laid over the level"
        );
        assert!(matches!(tv.0.clear_color, ClearColorConfig::Custom(c) if c == Color::NONE));
        assert!(over(&tv, &level), "the TV's UI over the level");
        assert_eq!(
            app.world().get::<Visibility>(screen),
            Some(&Visibility::Hidden),
            "the screen under the level hidden"
        );

        // AG.21's order: the level's camera after the TV's, replacing it.
        let mut covered = level.clone();
        covered.0.order = 1;
        assert!(!over(&tv, &covered), "covered again");

        // Gone (the dev entry off, or it shows the level again).
        app.insert_resource(LevelView(false));
        app.world_mut()
            .run_system_once(move |mut commands: Commands, mut l: ResMut<Level>| {
                l.hide(&mut commands, camera);
                commands.entity(camera).despawn();
            })
            .unwrap();
        app.update();
        let (tv, _) = tv_camera(&mut app);
        assert_eq!(writes(&tv), Some(None), "alone again");
        assert!(matches!(tv.clear_color, ClearColorConfig::Default));
        assert_eq!(
            app.world().get::<Visibility>(screen),
            Some(&Visibility::Inherited)
        );
    }

    /// The start's pass draws a level output under the TV with nothing of
    /// the level showing: the TV writes over it as it does over the level
    /// (the pipeline the pass compiles for it), its screen opaque and up.
    #[test]
    fn under_the_passs_output_the_tv_writes_as_over_the_level_and_shows_its_screen() {
        let mut app = App::new();
        app.add_systems(PostUpdate, tv_over_level);
        let tv = app
            .world_mut()
            .spawn((Camera::default(), RenderTarget::default(), TvCamera))
            .id();
        let screen = app
            .world_mut()
            .spawn((ScreenRoot, Visibility::default()))
            .id();
        let output = app
            .world_mut()
            .spawn(view_camera(RenderTarget::default(), -10))
            .id();
        app.update();
        let c = app.world().get::<Camera>(tv).unwrap();
        assert_eq!(
            writes(c),
            Some(Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING))
        );
        assert!(matches!(c.clear_color, ClearColorConfig::Default));
        assert_eq!(
            app.world().get::<Visibility>(screen),
            Some(&Visibility::Inherited)
        );
        // Over the TV, switched off, or on another target, it's nothing
        // under the TV.
        app.world_mut().get_mut::<Camera>(output).unwrap().order = 1;
        app.update();
        assert_eq!(writes(app.world().get::<Camera>(tv).unwrap()), Some(None));
        app.world_mut().get_mut::<Camera>(output).unwrap().order = -10;
        app.world_mut().get_mut::<Camera>(output).unwrap().is_active = false;
        app.update();
        assert_eq!(
            writes(app.world().get::<Camera>(tv).unwrap()),
            Some(None),
            "switched off"
        );
        app.world_mut().get_mut::<Camera>(output).unwrap().is_active = true;
        app.update();
        assert_eq!(
            writes(app.world().get::<Camera>(tv).unwrap()),
            Some(Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING)),
            "drawing again"
        );
        app.world_mut()
            .entity_mut(output)
            .insert(RenderTarget::Image(Handle::default().into()));
        app.update();
        assert_eq!(writes(app.world().get::<Camera>(tv).unwrap()), Some(None));
    }

    /// The rule the mirror reports: UI shows over the level only drawn
    /// after it, on its target, cleared to no colour and laid over.
    #[test]
    fn ui_draws_over_the_level_only_after_it_on_its_target_laid_over_nothing() {
        let level = (
            Camera {
                order: VIEW_ORDER,
                output_mode: VIEW_OUTPUT,
                ..default()
            },
            None,
        );
        let tv = (
            Camera {
                order: TV_ORDER,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                output_mode: TV_OVER,
                ..default()
            },
            Some(RenderTarget::default()),
        );
        assert!(over(&tv, &level));
        let with = |f: &dyn Fn(&mut Camera)| {
            let mut t = tv.clone();
            f(&mut t.0);
            over(&t, &level)
        };
        assert!(!with(&|c| c.order = VIEW_ORDER - 1), "drawn first");
        assert!(
            !with(&|c| c.clear_color = ClearColorConfig::Default),
            "an opaque clear"
        );
        assert!(!with(&|c| c.output_mode = VIEW_OUTPUT), "replacing");
        assert!(!with(&|c| c.is_active = false));
        assert!(
            with(&|c| c.output_mode = CameraOutputMode::default()),
            "Bevy's own write after another camera blends"
        );
        let elsewhere = (
            tv.0.clone(),
            Some(RenderTarget::Image(Handle::default().into())),
        );
        assert!(!over(&elsewhere, &level), "another target");
    }

    /// The dev entry: once ready, the level shows from AF's fresh-state
    /// camera over the TV, lit for it; moved, the camera relights every
    /// material from where it stands. Under reduced motion a scroll holds
    /// its start.
    #[test]
    fn the_dev_view_shows_the_level_lit_in_the_world_and_a_moved_camera_relights_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        app.insert_resource(LevelView(true))
            .insert_resource(ReducedMotion(true));
        app.world_mut().spawn(TvCamera);
        until(&mut app, &["shown"], |_| {});
        let (root, camera, materials) = {
            let loaded = app.world().resource::<Level>().loaded().unwrap();
            (
                loaded.root,
                loaded.camera.unwrap(),
                loaded.materials.clone(),
            )
        };
        let lit = level_lights(&l, &zone()).unwrap();
        let travel = |app: &App| {
            let m = app
                .world()
                .resource::<Assets<CharacterMaterial>>()
                .get(&materials[0])
                .unwrap();
            (m.uniform.light_travel[0].truncate(), m.uniform.misc.w)
        };
        // Lit in the world's frame, which the shader turns by its view.
        let world = || {
            (
                Vec3::from_array(world_scene(&lit, &LevelFog::NONE).lights[0].travel),
                guacamole::character::LIGHTS_FROM_SCENE,
            )
        };
        let rewritten = |app: &App| {
            app.world()
                .resource::<Messages<AssetEvent<CharacterMaterial>>>()
                .iter_current_update_messages()
                .any(|e| {
                    *e == AssetEvent::Modified {
                        id: materials[0].id(),
                    }
                })
        };
        let (t, w) = travel(&app);
        assert!(t.abs_diff_eq(world().0, 1e-6) && w == world().1, "{t} {w}");
        // The frame it shows rewrites no material (the sky's scroll aside).
        assert!(!rewritten(&app), "the show frame relit the level");
        app.update();
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
            "it replaces what's on the target, not blends over it"
        );
        assert_eq!(c.get::<Transform>(), Some(&fresh_state_transform()));
        assert_eq!(
            c.get::<RenderLayers>(),
            Some(&RenderLayers::layer(LEVEL_LAYER))
        );
        assert!(app.world().resource::<Level>().report().shown);
        // Turned to face the other way: nothing is lit again, since the
        // shader turns the world's lights by the view it draws for (AG.21).
        let relit = app.world().resource::<Level>().report().relit;
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
        assert!(!rewritten(&app), "a moved camera relit the level");
        app.update();
        assert_eq!(app.world().resource::<Level>().report().relit, relit);
        assert_eq!(travel(&app), (world().0, world().1));
        // The sky's u slides from half a texture along, its track's start,
        // not the identity's 0.
        let (u, _) = sky_rows(&app);
        assert!(
            u[0].abs_diff_eq(Vec4::new(1.0, 0.0, 0.5, 0.0), 1e-6),
            "{u:?}"
        );
    }

    /// AG.22: a gloss map is a weight, so the level loads it through a
    /// linear view, even where the same texture is another material's
    /// third layer, decoded; the gloss-mapped material draws on its own key.
    #[test]
    fn a_gloss_map_loads_as_stored() {
        use bevy::render::render_resource::TextureFormat;
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let path = tmp.path().join(source.ir.level_path(LEVEL, SCENE));
        let mut scene: guac_ir::level::Scene =
            guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let graph = &mut scene.scenes[0];
        let LevelMaterial::Multi2(ground) = &graph.materials[0] else {
            panic!("the ground");
        };
        let shared = ground
            .textures
            .iter()
            .find(|t| t.role == TextureRole::Layer3ColorMap)
            .unwrap()
            .clone();
        let mut leaves = multi2(
            "leaves",
            Mask {
                base: 0x9C,
                ignore: 0x3FF,
            },
            Mask {
                base: 0x109C,
                ignore: 0x1BFF,
            },
            &[
                TextureRole::Layer1ColorMap,
                TextureRole::Layer1BaseNormalMap,
                TextureRole::Layer1BaseSpecMap,
                TextureRole::Layer4ColorMap,
            ],
        );
        leaves.textures.push(TextureSlot {
            role: TextureRole::Layer3ColorMap,
            ..shared.clone()
        });
        leaves.params.fixed_alpha = [-1.0, -1.0, 1.0, 0.125];
        let shape = MeshShape {
            uv_sets: 2,
            tangents: true,
            colours: true,
            skinned: false,
        };
        graph.materials.push(LevelMaterial::Multi2(leaves.clone()));
        graph.segments.push(segment(3, shape, 10.0));
        std::fs::write(&path, guac_ir::to_ron(&scene).unwrap()).unwrap();
        let ktx2 = std::fs::read(tmp.path().join(&shared.texture)).unwrap();
        for t in leaves
            .textures
            .iter()
            .filter(|t| t.texture != shared.texture)
        {
            std::fs::write(tmp.path().join(&t.texture), &ktx2).unwrap();
        }

        let built = read_level(&source, CompressedImageFormats::BC).unwrap();
        let views: Vec<(bool, TextureFormat)> = built
            .images
            .iter()
            .filter(|(k, _)| k.0 == shared.texture)
            .map(|(k, image)| (k.1, image.texture_descriptor.format))
            .collect();
        assert_eq!(
            views,
            [
                (true, TextureFormat::Bc1RgbaUnormSrgb),
                (false, TextureFormat::Bc1RgbaUnorm)
            ]
        );
        // The leaves' other maps decode as they would anywhere.
        for t in leaves
            .textures
            .iter()
            .filter(|t| t.texture != shared.texture)
        {
            let (k, _) = built.images.iter().find(|(k, _)| k.0 == t.texture).unwrap();
            assert_eq!(k.1, remaster::is_srgb(t.role), "{:?}", t.role);
        }
        let gloss = built
            .keys
            .iter()
            .filter(|(k, _)| k.material.shader.switches().gloss)
            .count();
        assert_eq!((built.keys.len(), gloss), (4, 1));
    }
}
