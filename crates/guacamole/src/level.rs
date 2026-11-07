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

use avian3d::prelude::{Position, Rotation};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{CameraOutputMode, ClearColorConfig, Projection, RenderTarget};
use bevy::ecs::system::SystemParam;
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
use bevy_transform_interpolation::prelude::TransformInterpolation;
use crossbeam_channel::{Receiver, Sender};
use guac_extract::intros::IrFormat;
use guac_extract::levels::{
    ATMOSPHERE, COLLISION, FIRST_RUN, LEVELS, LIGHTS, PLACEMENTS, PROPS, SCENE,
};
use guac_ir::DumpIdentity;
use guac_ir::model::{self, Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::{PointLight, Scene};
use guac_motion::seam::{Body, BodyId, BodyKind};
use guac_script::Obj;
use guacamole::character::warm::WarmKey;
use guacamole::character::{
    CharacterMaterial, CharacterUniform, FrameGlow, LEVEL_LIGHTS, ScenePointLights,
    reference_material, remaster, spawn_alpha_clear, static_mesh,
};
use guacamole::frame::{self, Frame};
use guacamole::level_scene::{
    self as arithmetic, AtStart, Drawn, LevelFog, LevelLights, fresh_state_projection,
    fresh_state_transform, fresh_state_view, hero_scene, view_scene, world_scene,
};
use guacamole::level_scripts;
use guacamole::props::{self, LeftOut, LevelProps, Prop};
use guacamole::script_engine::Actor;
use guacamole::scripts::ScriptWorld;
use serde::Serialize;
use tracing::{info, warn};

use crate::extraction::Extraction;
use crate::hero::HeroBody;
use crate::hero_scripts::ScriptSource;
use crate::motion::ReducedMotion;
use crate::physics::{PreparedBody, SeamBody};
use crate::turntable::{plain, read_ir};
use crate::tv_state::{Redrawn, ScreenRoot, TvCamera};
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
    /// Chapter 1 in the extract at `dir`: what a Giants pick hands the TV.
    pub fn chapter_1(dir: PathBuf, ir: IrFormat, identity: DumpIdentity) -> Self {
        Self::named(dir, ir, identity, LEVELS[0])
    }

    /// `level` in the extract at `dir`, its parts in (openspec AS.13: a
    /// level asked for by name, [`crate::level_choice`]).
    pub fn named(dir: PathBuf, ir: IrFormat, identity: DumpIdentity, level: &str) -> Self {
        Self {
            dir,
            ir,
            identity,
            level: level.to_owned(),
        }
    }

    /// Whether this level is chapter 1, whose listing names its first
    /// fight's placements (openspec AS.51); every level runs its own
    /// scripts.
    pub fn is_chapter_1(&self) -> bool {
        FIRST_RUN.contains(&self.level.as_str())
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

/// The sounds part whole in the extract at `dir` after the level loaded
/// from it (openspec AR.21): a first run hands chapter 1 over before its
/// sounds stage reads, so the level's load found none or some of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundsReady {
    pub dir: PathBuf,
}

/// Where the extraction puts a [`SoundsReady`] (`ExtractJob::sounds`).
pub type SoundsSender = Sender<SoundsReady>;

/// The TV's end of it.
#[derive(Resource)]
pub struct SoundsFeed(pub Receiver<SoundsReady>);

/// A channel for [`SoundsReady`]s, beside the level's.
pub fn sounds_channel() -> (SoundsSender, SoundsFeed) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (tx, SoundsFeed(rx))
}

/// The render layer the level draws on: clear of the default one the TV's
/// cameras see and of the turntables' (16 up).
pub(crate) const LEVEL_LAYER: usize = 2;
/// The level's camera (the dev view's, the walk's) draws before the TV's own
/// ([`crate::tv_state::TV_ORDER`]), under its UI (AG.50): at order 1 it covered Start's
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

/// Whether a level's sounds are on their way (openspec AR.30): the Giants
/// pick's extraction yet to end its sounds stage, which a first run hands
/// chapter 1 over ahead of (AT.26). A level asked for by name is fetched
/// once that extraction has ended, its own sounds with it (AR.30), so none
/// of its are ever on their way; with no extraction (no pick yet, a test),
/// none are.
pub(crate) fn sounds_coming(extraction: Option<&Extraction>) -> bool {
    use crate::extraction::ExtractStatus as S;
    extraction.is_some_and(|e| {
        matches!(
            e.status,
            S::Starting
                | S::Checking { .. }
                | S::Roster { .. }
                | S::Models { .. }
                | S::Level { .. }
                | S::Scripts { .. }
                | S::Particles { .. }
                | S::Sprites { .. }
                | S::Sounds { .. }
        )
    })
}

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
    /// The level's materials and its props' the TV can't draw yet, left
    /// out by name (openspec AS.13), each once with its segments summed.
    refused: Vec<arithmetic::Refused>,
    /// Each drawn material's sort bias: the sky's blended parts first, in
    /// the scene's order (AG.79, `level_scene::depth_biases`).
    depth_bias: Vec<f32>,
    walk: Walk,
    /// The props (AG.25): each model's segments as (an index into `meshes`,
    /// past the level's own, and its material in `drawn.materials`).
    prop_models: Vec<Vec<(usize, u16)>>,
    /// The spawn part's models (AH.24), by path: each one's segments as the
    /// props' are, past theirs; then the level's templates' the TV draws
    /// the clones of (AG.60), the props' own segments.
    spawn_models: Vec<(String, Vec<(usize, u16)>)>,
    /// The particle part's emitters (AH.87), each generator's material in
    /// `drawn.materials`, `None` for one that draws nothing.
    particles: Vec<(Arc<guac_ir::particle::Emitter>, Vec<Option<usize>>)>,
    /// The level's own emitters (openspec AS.17), in its particles part's
    /// order, each generator's material as the particle part's are; what
    /// that part refused, by name with why; and why it didn't read, if it
    /// didn't.
    placed: Vec<(Arc<guac_ir::particle::Placed>, Vec<Option<usize>>)>,
    placed_refused: Vec<(String, String)>,
    placed_unread: Option<String>,
    /// The first fight's creatures' models (openspec AH.25) and the skinned
    /// templates' (AG.150, AS.63's `props::clones_draw`), by path, with each
    /// one's palette off the level's palettes part: their textures among
    /// `images`.
    creatures: Vec<CreatureModel>,
    read: Duration,
    took: Duration,
}

/// A creature's model as the TV draws it (openspec AH.25,
/// [`guacamole::creature_models`]): its skinned parts, its idle and the
/// clips its scripts play by palette slot.
#[derive(Debug, Clone)]
pub(crate) struct CreatureModel {
    pub(crate) path: String,
    pub(crate) model: Arc<guac_ir::Model>,
    /// None when the level's palettes part doesn't read or holds none for
    /// it: it holds its idle.
    pub(crate) palette: Option<Arc<guac_ir::palette::Palette>>,
    /// Its attach list off the level's nodes part (openspec AH.138), its
    /// nodes on its joints; none when the part doesn't read or holds none
    /// for it: what hangs on its nodes stands at its pivot.
    pub(crate) nodes: Option<Arc<guac_ir::nodes::AttachNodes>>,
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
    /// The first fight's creatures' models (AH.25) and the skinned
    /// templates' (AG.150), by path.
    creatures: Vec<(String, guac_ir::Model)>,
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
    let creatures =
        guacamole::creature_models::skinned_models(&placements, &props, &source.identity);
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
        creatures,
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
    // Each texture's layout, read once off its header (AS.13: a normal map
    // the shader can't read refuses its material).
    let formats_read = Mutex::new(HashMap::new());
    let format_of = |texture: &str| -> Option<u32> {
        if !plain(texture) {
            return None;
        }
        let mut read = formats_read.lock().unwrap_or_else(|e| e.into_inner());
        *read
            .entry(texture.to_owned())
            .or_insert_with(|| ktx2_format(&source.dir.join(texture)))
    };
    // The census's level table is held to this derivation on the disc
    // (`tests/giants_census.rs`).
    let AtStart {
        mut drawn,
        lights,
        fog,
        mut keys,
        refused,
    } = arithmetic::at_start(scene, &lights, &atmosphere, &format_of).map_err(|e| e.to_string())?;
    // The props' models draw beside the level: their materials after its
    // own, their keys in its scene (the census's props table holds them,
    // `tests/giants_census.rs`), their meshes after its segments'. What
    // the TV can't draw of them yet is left out as the level's is.
    let level_materials = drawn.materials.len();
    let scene = level_scene(&lights, &fog);
    let refused = merged(
        refused
            .into_iter()
            .chain(walk.props.refuse(&scene, &format_of)),
    );
    let (props, prop_ranges) = walk.props.drawn();
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
    drawn.extend_materials(&props);
    // The level's placements whose clones the TV draws (AG.60, AS.63's
    // `props::clones_draw`): their models are among the props', drawn by
    // path as the spawn part's are.
    let level_spawns: Vec<(String, Vec<(usize, u16)>)> = walk
        .props
        .spawn_models
        .iter()
        .map(|&i| (walk.props.models[i].path.clone(), prop_models[i].clone()))
        .collect();
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
    let mut spawn_models = spawns
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
    spawn_models.extend(level_spawns);
    drawn.extend_materials(&spawned);
    // What the scripts' emitters draw (AH.87): the particle part's
    // generators' materials after the spawn models', keyed in the same
    // scene (the census's particle tables hold them). A part that doesn't
    // read draws no particles; the level goes on.
    let emitters = match read_particles(source) {
        Ok(e) => e,
        Err(why) => {
            warn!(reason = %why, "the particle part didn't read: the scripts' emitters draw nothing");
            Vec::new()
        }
    };
    let (sparks, index) = guacamole::particles::drawn(&emitters);
    let particle_materials = drawn.materials.len();
    for (k, m) in arithmetic::keys(&sparks, &scene).map_err(|e| format!("the particles: {e}"))? {
        if !keys.iter().any(|(kept, _)| *kept == k) {
            keys.push((k, particle_materials + m));
        }
    }
    drawn.extend_materials(&sparks);
    let particles = index
        .into_iter()
        .filter_map(|(name, generators)| {
            let emitter = emitters
                .iter()
                .flat_map(|a| &a.emitters)
                .find(|e| e.name == name)?;
            let materials = generators
                .iter()
                .map(|(_, m)| m.map(|m| particle_materials + usize::from(m)))
                .collect();
            Some((Arc::new(emitter.clone()), materials))
        })
        .collect();
    // The level's own emitters (openspec AS.17): its particles part, their
    // materials after the spawn emitters', keyed in the same scene. A part
    // that doesn't read (an extract that hasn't read it yet included) draws
    // none of them; the level goes on.
    let placed_part = match read_ir::<guac_ir::particle::LevelEmitters>(
        source.ir,
        &part(guac_extract::levels::PARTICLES),
    )
    .and_then(|p| {
        if p.identity != source.identity || p.level != source.level {
            return Err("the particles part is another dump's or level's".to_owned());
        }
        Ok(p)
    }) {
        Ok(p) => Ok(p),
        Err(why) => {
            warn!(reason = %why, "the level's particles part didn't read: its own emitters draw nothing");
            Err(why)
        }
    };
    let mut placed = Vec::new();
    let mut placed_refused = Vec::new();
    if let Ok(level_emitters) = &placed_part {
        let (own, index) = guacamole::particles::drawn_placed(level_emitters);
        let own_materials = drawn.materials.len();
        for (k, m) in arithmetic::keys(&own, &scene).map_err(|e| format!("its emitters: {e}"))? {
            if !keys.iter().any(|(kept, _)| *kept == k) {
                keys.push((k, own_materials + m));
            }
        }
        drawn.extend_materials(&own);
        placed = level_emitters
            .emitters
            .iter()
            .zip(index)
            .map(|(p, generators)| {
                let materials = generators
                    .into_iter()
                    .map(|m| m.map(|m| own_materials + usize::from(m)))
                    .collect();
                (Arc::new(p.clone()), materials)
            })
            .collect();
        placed_refused.clone_from(&level_emitters.refused);
    }
    let placed_unread = placed_part.err();
    // The first fight's creatures (AH.25) and the skinned templates' clones
    // (AG.150): their models with their palettes, their textures read with
    // the level's. A palettes part that doesn't read leaves each holding its
    // idle.
    let palettes: Option<guac_ir::palette::LevelPalettes> = match read_ir(
        source.ir,
        &source.dir.join(guac_extract::levels::part_path(
            source.ir,
            &source.level,
            guac_extract::levels::PALETTES,
        )),
    ) {
        Ok(p) => Some(p),
        Err(why) => {
            warn!(reason = %why, "the level's palettes didn't read: its creatures hold their idles");
            None
        }
    };
    // Their attach lists (AH.138): a nodes part that doesn't read leaves
    // what hangs on their nodes at their pivots.
    let nodes: Option<guac_ir::nodes::LevelNodes> = match read_ir(
        source.ir,
        &part(guac_extract::levels::NODES),
    ) {
        Ok(n) => Some(n),
        Err(why) => {
            warn!(reason = %why, "the level's nodes didn't read: what hangs on a creature's node stands at its pivot");
            None
        }
    };
    let creatures: Vec<CreatureModel> = std::mem::take(&mut walk.creatures)
        .into_iter()
        .map(|(path, model)| CreatureModel {
            palette: palettes
                .as_ref()
                .and_then(|p| p.palettes.iter().find(|x| x.owner == path))
                .map(|p| Arc::new(p.clone())),
            nodes: nodes
                .as_ref()
                .and_then(|n| n.get(&path))
                .map(|n| Arc::new(n.clone())),
            path,
            model: Arc::new(model),
        })
        .collect();
    let creature_materials: Vec<&model::Material> = creatures
        .iter()
        .flat_map(|c| c.model.materials.iter())
        .collect();
    let mut images: Vec<(ImageKey, Image)> = Vec::new();
    let slots = drawn
        .materials
        .iter()
        .chain(creature_materials)
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
        refused,
        depth_bias,
        walk,
        prop_models,
        spawn_models,
        particles,
        placed,
        placed_refused,
        placed_unread,
        creatures,
        read,
        took: started.elapsed(),
    })
}

/// A KTX2's `vkFormat`, off its header; `None` for a file that doesn't
/// read or isn't one.
fn ktx2_format(path: &std::path::Path) -> Option<u32> {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    (head[..12] == KTX2_IDENTIFIER)
        .then(|| u32::from_le_bytes([head[12], head[13], head[14], head[15]]))
}

/// The 12 bytes every KTX2 file opens with.
const KTX2_IDENTIFIER: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];

/// `refused` with each material once, in the order first met, its segments
/// summed: a prop's material its models share is refused in each.
fn merged(refused: impl IntoIterator<Item = arithmetic::Refused>) -> Vec<arithmetic::Refused> {
    let mut out: Vec<arithmetic::Refused> = Vec::new();
    for r in refused {
        match out
            .iter_mut()
            .find(|o| o.material == r.material && o.why == r.why)
        {
            Some(o) => o.segments += r.segments,
            None => out.push(r),
        }
    }
    out
}

/// The walkers the extract's scripts part holds (AG.27), whose archives
/// give the spawn and particle parts what their attacks spawn (openspec
/// AU.1); none for an extract made before them.
fn listed_walkers(source: &LevelSource) -> Vec<String> {
    guac_extract::scripts::read_walkers(&source.dir)
        .map(|w| w.listed)
        .unwrap_or_default()
}

/// One spawn archive's file of a part: `None`, logged, for a walker's that
/// doesn't read (his clones draw nothing; the rest still do), the error for
/// the base's (`global.bld`'s and a fighter's), which fail the part.
fn walker_tolerant<T>(
    archive: &str,
    base: &[String],
    rel: &str,
    read: Result<T, String>,
) -> Result<Option<T>, String> {
    match read {
        Ok(t) => Ok(Some(t)),
        Err(why) if !base.iter().any(|b| b == archive) => {
            warn!(file = rel, reason = %why, "a walker's spawn file didn't read: what his attacks spawn draws nothing");
            Ok(None)
        }
        Err(why) => Err(why),
    }
}

/// The extract's spawn part (AH.22): every model each spawn archive's
/// placements name, as props draw them, every walker's archive's among
/// them (openspec AU.1).
fn read_spawns(source: &LevelSource) -> Result<Vec<guacamole::props::PropModel>, String> {
    let base = guac_extract::scripts::spawn_archives();
    let mut out = Vec::new();
    for archive in guac_extract::scripts::spawn_archives_with(&listed_walkers(source)) {
        let rel = source.ir.spawn_path(&archive);
        let read = read_ir::<guac_ir::spawn::SpawnModels>(source.ir, &source.dir.join(&rel))
            .and_then(|spawns| match spawns.identity == source.identity {
                true => Ok(spawns),
                false => Err(format!("{rel} is another dump's")),
            });
        let Some(spawns) = walker_tolerant(&archive, &base, &rel, read)? else {
            continue;
        };
        out.extend(guacamole::props::spawn_models(&spawns).map_err(|e| format!("{rel}: {e}"))?);
    }
    Ok(out)
}

/// The extract's particle part (AH.87): every emitter each spawn archive
/// places, with its generators, every walker's among them (openspec AU.1).
fn read_particles(source: &LevelSource) -> Result<Vec<guac_ir::particle::Emitters>, String> {
    let base = guac_extract::particles::archives();
    let mut out = Vec::new();
    for archive in guac_extract::particles::archives_with(&listed_walkers(source)) {
        let rel = guac_extract::particles::path(source.ir, &archive);
        let read = read_ir::<guac_ir::particle::Emitters>(source.ir, &source.dir.join(&rel))
            .and_then(|e| match e.identity == source.identity {
                true => Ok(e),
                false => Err(format!("{rel} is another dump's")),
            });
        if let Some(e) = walker_tolerant(&archive, &base, &rel, read)? {
            out.push(e);
        }
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
    /// The scene's materials and segments not drawn (tfbshadow's, and an
    /// effect missing what its stand-in reads, AG.24; stand-in ledger: `grass-warp`).
    pub left_out: usize,
    pub segments_left_out: usize,
    /// The level's and its props' materials the TV can't draw yet, each
    /// by name with why and its segments left out with it (openspec AS.13).
    pub refused: Vec<RefusedMaterial>,
    /// The level asked for by name and how far it's got (openspec AS.13,
    /// [`crate::level_choice`]): kept from one load to the next.
    pub asked: Option<crate::level_choice::AskedReport>,
    /// Drawn materials that are grass shells and warps (AG.24): the
    /// level's, its props' and the clones'.
    pub grass_shells: usize,
    pub warps: usize,
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
    /// When the dev view showed it, on the TV's real clock in ms: the frame
    /// log's (`/api/_test/tv/frames`), so a compile after it counts against
    /// it (openspec AS.14).
    pub shown_ms: Option<f64>,
    /// The point lights the frame draws the level with (openspec AH.65),
    /// the lit ones of the level's buffer's three.
    pub lights: Vec<FrameLight>,
    /// The script part, once linked (AH.22), or why it didn't.
    pub scripts: Option<ScriptsReport>,
    pub scripts_reason: Option<String>,
    /// From the source landing to the sounds part read again into the run
    /// once it landed after the load (AR.21), and the archives it held.
    pub sounds_again_ms: Option<u64>,
    pub sounds_again_archives: Option<usize>,
    /// What stopped it: the reading task's error, a part's file or the
    /// lights it lacks.
    pub reason: Option<String>,
}

/// A material the TV can't draw yet, as `/api/_test/tv` reports it
/// (openspec AS.13).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct RefusedMaterial {
    pub material: String,
    pub why: String,
    pub segments: usize,
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
    /// broken), by name: they stand nowhere again in this run; a run
    /// installed afresh (the level entered again) has them back.
    pub taken_off: Vec<String>,
    /// Standing props the scripts have moved, turned, scaled or reshaped
    /// off their file's (openspec AG.60), by name, and how many solids were
    /// rebuilt for a changed shape since the load.
    pub scripted: Vec<String>,
    pub rebuilt: usize,
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
/// leaves it; once the level's scripts run, as each prop's actor stands,
/// drawn and solid where the scripts leave it (openspec AG.60).
pub(crate) struct StandingProps {
    props: Vec<Prop>,
    /// Per prop: the entity its model hangs under, if it draws.
    draws: Vec<Option<Entity>>,
    /// Per prop: its file's solid, ready to spawn, its entity while it
    /// stands and the body that entity holds.
    bodies: Vec<Option<PreparedBody>>,
    spawned: Vec<Option<Entity>>,
    holds: Vec<Option<Body>>,
    standing: Vec<bool>,
    shown: Vec<bool>,
    report: PropsReport,
}

/// What stands a prop once the level's scripts run (openspec AG.60): each
/// prop's actor by its object, and how a draw's transform and a standing
/// body's pose are set (whether that body was there to move).
pub(crate) struct ByActors<'a> {
    pub(crate) actor_of: &'a dyn Fn(u32) -> Option<&'a Actor>,
    pub(crate) place: &'a mut dyn FnMut(Entity, Transform),
    pub(crate) pose: &'a mut dyn FnMut(Entity, guac_motion::seam::Pose) -> bool,
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
                // Eased between steps: the scripts move and turn some (a
                // spinning coin, the propeller) every step (AG.60).
                let parent = commands
                    .spawn((
                        Name::new(format!("prop {}", p.name)),
                        p.transform,
                        TransformInterpolation,
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
            holds: vec![None; n],
            standing: vec![false; n],
            shown: vec![false; n],
            report,
        }
    }

    /// Stands up the props within range of `at` and takes down the rest
    /// (AG.25's rule), or, once the level's scripts run, each as its actor
    /// stands (openspec AG.60: the actor is the prop's one holder, AH design
    /// decision 21): awake by the activity rule or its scripts and not taken
    /// off (AH.21: a breakable broken goes at once and never stands again),
    /// drawn where the scripts leave its place, turn and scale unless its
    /// tint is see-through, solid as they leave it. Each one's draw shown or
    /// hidden and placed; its solid spawned into Avian's world, moved there,
    /// rebuilt when its shape changed, or despawned.
    pub(crate) fn stand(&mut self, at: Vec3, mut by: Option<ByActors>, commands: &mut Commands) {
        self.report.at = Some(at.to_array());
        let mut changed = false;
        for (i, p) in self.props.iter().enumerate() {
            let actor = by.as_ref().and_then(|b| (b.actor_of)(p.object));
            let off = actor.is_some_and(|a| a.removed);
            let listed = self.report.taken_off.contains(&p.name);
            if off && !listed {
                self.report.taken_off.push(p.name.clone());
            } else if listed && actor.is_some_and(|a| !a.removed) {
                // Back in a run installed afresh (the level ended and
                // entered again, AH.26's Restart chapter).
                self.report.taken_off.retain(|n| *n != p.name);
            }
            let stands = match actor {
                Some(a) => a.active && !a.removed,
                None => p.stands(at),
            };
            let shows = stands && actor.is_none_or(props::actor_shows);
            if stands != self.standing[i] {
                changed = true;
                self.standing[i] = stands;
            }
            let place = actor.map_or(p.transform, props::actor_transform);
            let scripted = place != p.transform;
            if let Some(draw) = self.draws[i] {
                if shows != self.shown[i] {
                    commands.entity(draw).insert(if shows {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    });
                    self.shown[i] = shows;
                    changed = true;
                }
                if let Some(b) = by.as_mut() {
                    (b.place)(draw, place);
                }
            }
            let body = match (stands, actor) {
                (false, _) => None,
                (true, Some(a)) => props::actor_body(p, a),
                (true, None) => p.body.clone(),
            };
            if self.report.scripted.contains(&p.name) != (stands && (scripted || body != p.body)) {
                changed = true;
                if stands && (scripted || body != p.body) {
                    self.report.scripted.push(p.name.clone());
                } else {
                    self.report.scripted.retain(|n| *n != p.name);
                }
            }
            match (body, self.spawned[i]) {
                (None, None) => {}
                (None, Some(e)) => {
                    commands.entity(e).despawn();
                    self.spawned[i] = None;
                    self.holds[i] = None;
                    changed = true;
                }
                (Some(b), held) => {
                    let same_shape = self.holds[i]
                        .as_ref()
                        .is_some_and(|h| h.collider == b.collider && h.kind == b.kind);
                    match held {
                        Some(e) if same_shape => {
                            if self.holds[i].as_ref().map(|h| h.pose) != Some(b.pose)
                                && let Some(by) = by.as_mut()
                                && (by.pose)(e, b.pose)
                            {
                                self.holds[i] = Some(b);
                            }
                        }
                        _ => {
                            if let Some(e) = held {
                                commands.entity(e).despawn();
                                self.report.rebuilt += 1;
                            }
                            // The file's solid was built off the frame; one
                            // the scripts changed is built here, a convex
                            // shape at a time.
                            let prepared = if Some(&b) == p.body.as_ref() {
                                self.bodies[i].clone()
                            } else {
                                PreparedBody::new(&b)
                                    .inspect_err(|e| warn!(prop = %p.name, error = %e, "a solid the scripts left won't build"))
                                    .ok()
                            };
                            self.spawned[i] = prepared.map(|b| b.spawn(commands));
                            self.holds[i] = self.spawned[i].map(|_| b);
                            // A solid rebuilt sensing or blocking counts anew.
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            let standing: Vec<usize> = (0..self.props.len())
                .filter(|&i| self.standing[i])
                .collect();
            let kind = |i: &usize| self.holds[*i].as_ref().map(|b| b.kind);
            let r = &mut self.report;
            r.standing = standing.len();
            r.drawn = standing.iter().filter(|&&i| self.shown[i]).count();
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
        self.holds.fill(None);
        self.standing.fill(false);
        self.shown.fill(false);
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
            .zip(&self.holds)
            .filter_map(|(p, held)| held.as_ref().map(|b| (p.object, b.id)))
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

/// An emitter as the level draws it (AH.87): its generators off the
/// particle part, each with its material, `None` for one that draws
/// nothing.
#[derive(Debug, Clone)]
pub(crate) struct EmitterDraw {
    pub(crate) emitter: Arc<guac_ir::particle::Emitter>,
    pub(crate) materials: Vec<Option<Handle<CharacterMaterial>>>,
}

/// One of a level's own emitters as the level draws it (openspec AS.17):
/// where its file places it and its generators with their materials.
#[derive(Debug, Clone)]
pub(crate) struct PlacedDraw {
    pub(crate) placed: Arc<guac_ir::particle::Placed>,
    pub(crate) draw: EmitterDraw,
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
    /// The particle part's emitters by name: what an emitter's clone draws
    /// (AH.87).
    emitters: HashMap<String, EmitterDraw>,
    /// The level's own emitters (openspec AS.17), in its particles part's
    /// order: what stands in the level, and what a clone of one of its
    /// templates draws ([`Level::level_emitter`]), by object in its file.
    placed: Vec<PlacedDraw>,
    placed_by_object: HashMap<u32, usize>,
    /// What its particles part refused, by name with why, and why the part
    /// didn't read, if it didn't.
    placed_refused: Vec<(String, String)>,
    placed_unread: Option<String>,
    /// The first fight's creatures' models (AH.25) and the skinned
    /// templates' (AG.150), and every texture the level read, by its view,
    /// for their materials.
    creatures: Vec<CreatureModel>,
    images: Vec<(ImageKey, Handle<Image>)>,
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
    /// The sounds part landed whole after the level's load read it (AR.21):
    /// read again once the run is installed.
    sounds_landed: bool,
    /// That read, on a task, with the program it was read for.
    sounds_again: Option<Task<(Arc<guac_script::Program>, guacamole::sounds::SoundPart)>>,
    /// The level last asked for by name (openspec AS.13), kept from one load
    /// to the next.
    asked: Option<crate::level_choice::AskedReport>,
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
            sounds_landed: false,
            sounds_again: None,
            asked: None,
        }
    }

    /// The level last asked for by name and how far it's got (openspec
    /// AS.13).
    pub(crate) fn asked(&self) -> Option<&crate::level_choice::AskedReport> {
        self.asked.as_ref()
    }

    pub(crate) fn set_asked(&mut self, asked: crate::level_choice::AskedReport) {
        self.asked = Some(asked);
    }

    /// Changes the ask's report, once there's one.
    pub(crate) fn update_asked(&mut self, f: impl FnOnce(&mut crate::level_choice::AskedReport)) {
        if let Some(a) = self.asked.as_mut() {
            f(a);
        }
    }

    /// The load's report as it stands (the props as of the load's spawn).
    pub(crate) fn report(&self) -> &LevelReport {
        &self.report
    }

    /// The load's report to set by hand: for the tests of what follows it
    /// (the asked level's header, openspec AS.14).
    #[cfg(test)]
    pub(crate) fn report_mut(&mut self) -> &mut LevelReport {
        &mut self.report
    }

    /// A level asked for off the extract at `dir`, nothing read yet: for the
    /// tests of what reads its files (the sounds' audio, openspec AR.9).
    #[cfg(test)]
    pub(crate) fn from_extract_for_tests(dir: PathBuf) -> Self {
        let mut l = Self::new(false);
        l.source = Some(LevelSource::chapter_1(
            dir,
            IrFormat::Ron,
            guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
        ));
        l
    }

    /// The extract the level reads from, once one's been asked for: where
    /// its sounds' audio is (openspec AR.9).
    pub(crate) fn extract_dir(&self) -> Option<&std::path::Path> {
        self.source.as_ref().map(|s| s.dir.as_path())
    }

    /// Why the level's script part didn't load, once its load said so.
    pub(crate) fn scripts_reason(&self) -> Option<&str> {
        self.report.scripts_reason.as_deref()
    }

    /// The report, with the props as they stand now.
    pub(crate) fn snapshot(&self) -> LevelReport {
        let mut r = self.report.clone();
        r.props = self.stage.loaded().map(|l| l.props.report().clone());
        r.asked = self.asked.clone();
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

    /// The ready level, its solids spawned and its script part linked and
    /// installed or failed (openspec AS.49: a hero entering before the run
    /// would walk it with none): what the hero can be spawned into.
    pub(crate) fn walkable(&self) -> Option<&Loaded> {
        match &self.stage {
            Stage::Ready(l) if self.scripts.is_none() => Some(l),
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

    /// The spawn part's models by path, once the level's loaded, and the
    /// level's placements' static ones by AS.63's rule
    /// (`props::clones_draw`): what the scripts' clones draw (AH.24).
    pub(crate) fn spawn_models(
        &self,
    ) -> Option<&HashMap<String, Vec<(Handle<Mesh>, Handle<CharacterMaterial>)>>> {
        self.stage.loaded().map(|l| &l.spawns)
    }

    /// The particle part's emitters by name and the level's root, once the
    /// level's loaded: what an emitter's clone draws and what its particles
    /// hang under (AH.87).
    pub(crate) fn emitters(&self) -> Option<(&HashMap<String, EmitterDraw>, Entity)> {
        self.stage.loaded().map(|l| (&l.emitters, l.root))
    }

    /// The level's own emitters (openspec AS.17) and its root, once the
    /// level's loaded: what stands in it by AG.7's rule.
    pub(crate) fn placed_emitters(&self) -> Option<(&[PlacedDraw], Entity)> {
        self.stage.loaded().map(|l| (l.placed.as_slice(), l.root))
    }

    /// What a clone of the level's own emitter at `object` in its file
    /// draws (openspec AS.17), once the level's loaded; none for an object
    /// its particles part doesn't hold.
    pub(crate) fn level_emitter(&self, object: u32) -> Option<&EmitterDraw> {
        let l = self.stage.loaded()?;
        l.placed_by_object.get(&object).map(|&i| &l.placed[i].draw)
    }

    /// What the level's particles part refused, by name with why, and why
    /// the part didn't read, if it didn't (openspec AS.17).
    pub(crate) fn placed_refused(&self) -> Option<(&[(String, String)], Option<&str>)> {
        self.stage
            .loaded()
            .map(|l| (l.placed_refused.as_slice(), l.placed_unread.as_deref()))
    }

    /// The image the particle part's emitter `name` draws with (its first
    /// textured generator's, its sRGB view where the level read both), once
    /// the level's loaded: what the HUD's level-up arrow is drawn from
    /// (openspec AH.248), the image AH.244 stores decoded at its own size.
    pub(crate) fn emitter_image(&self, name: &str) -> Option<Handle<Image>> {
        let l = self.stage.loaded()?;
        let draw = l.emitters.get(name)?;
        let path = draw
            .emitter
            .generators
            .iter()
            .find_map(|g| g.texture.as_ref())
            .map(|t| t.texture.as_str())?;
        let mut views = l.images.iter().filter(|((p, ..), _)| p == path);
        let first = views.clone().next().map(|(_, h)| h.clone());
        views
            .find(|((_, srgb, _), _)| *srgb)
            .map(|(_, h)| h.clone())
            .or(first)
    }

    /// The creature model at `path` (AH.25) and a lookup of the level's
    /// textures by view for its materials, once the level's loaded.
    pub(crate) fn creature(
        &self,
        path: &str,
    ) -> Option<(&CreatureModel, &[((String, bool, Sampler), Handle<Image>)])> {
        let l = self.stage.loaded()?;
        let c = l.creatures.iter().find(|c| c.path == path)?;
        Some((c, &l.images))
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
            let kept = (m.uniform.uv_u, m.uniform.uv_v, m.uniform.effect);
            m.uniform = CharacterUniform::new(&lit.reference, scene, lit.alpha).lit_by_scene();
            m.scene_lights = LEVEL_LIGHTS;
            (m.uniform.uv_u, m.uniform.uv_v, m.uniform.effect) = kept;
        }
    }
}

/// `GUAC_LEVEL_VIEW` (`dev-tools` builds): show the level once it's ready.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LevelView(pub(crate) bool);

/// Marks the camera the level is drawn from.
#[derive(Component, Debug)]
pub(crate) struct LevelCamera;

/// Marks the level's shadow light, whose cascades a character's shadow is
/// drawn into (openspec AG.23).
#[derive(Component, Debug)]
pub(crate) struct ShadowLight;

/// Marks what the level's sky segments hang from, kept at the level
/// camera's eye (openspec AS.45, [`keep_sky_about_the_eye`]).
#[derive(Component, Debug)]
pub(crate) struct LevelSky;

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
        frame::ensure(app);
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
                (
                    receive_level,
                    step_level,
                    show_level,
                    scroll_level,
                    receive_sounds,
                    sounds_again,
                )
                    .chain()
                    .in_set(LevelSystems)
                    .after(crate::turntable::TurntableSystems),
            )
            // The props stand by where the hero is as his step begins: what
            // the scripts broke this step out before anything moves.
            .add_systems(FixedUpdate, stand_props.in_set(Frame::Solids))
            // Before the assets' events go out, so the frame draws it lit.
            .add_systems(
                PostUpdate,
                (light_level, light_frame).before(bevy::asset::AssetEventSystems),
            )
            .add_systems(
                PostUpdate,
                keep_sky_about_the_eye.before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (
                    tv_over_level,
                    twin_level_glow.before(crate::tints::TintSystems),
                )
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
    // Every level runs its own scripts (openspec AS.51); the last level's
    // run goes as the next comes, so it drives nothing of this one
    // (AS.49: a level left with a hero in it included), and the hero comes
    // in only once this one's run is in ([`Level::walkable`]).
    level.scripts = Some({
        let (dir, name, ir) = (source.dir.clone(), source.level.clone(), source.ir);
        AsyncComputeTaskPool::get().spawn(async move {
            level_scripts::load(&dir, &name).map(|mut linked| {
                linked.layout = level_scripts::figure_layout(&dir, ir);
                linked.palettes = level_scripts::level_palettes(&dir, &name, ir);
                // The sounds its units' archives play (openspec AR.9).
                linked.sounds = Some(guacamole::sounds::load(&dir, ir, &linked.program.units));
                // The original's interface its scripts write (openspec AT.9).
                linked.interface = guacamole::interface_host::load(
                    &dir,
                    ir,
                    &linked.program.units,
                    &linked.interface_keys,
                    linked.interface_imports.as_ref(),
                );
                linked
            })
        })
    });
    commands.queue(take_down_run);
    level.report = LevelReport {
        state: "reading".into(),
        level: Some(source.level.clone()),
        ..Default::default()
    };
    level.source = Some(source);
    level.stage = Stage::Reading(task);
    level.since = Some(Instant::now());
}

/// The script run taken down (openspec AS.13, AS.49) as the next level
/// comes: its placements', clones' and objects' entities and the run itself,
/// so the next level isn't driven by the last one's (its props stood by
/// that run's actors, by object, in another level's file) before its own
/// run is in, nor reinstalled when the TV leaves the last. Its voices go too,
/// which the player follows only while a run is in (a loop would sound on
/// forever), and the particles' and sounds' reports say nothing draws or
/// plays. The camera's and the steps' records stay, read by no step without
/// a run.
fn take_down_run(world: &mut World) {
    use crate::emitters::ParticleReport;
    use crate::sound_player::{Heard, SoundReport};
    use bevy::audio::AudioSink;
    use guacamole::scripts::{ScriptClone, ScriptObject, ScriptPlacement, ScriptRuntime};
    let had = world.contains_resource::<ScriptRuntime>();
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<ScriptPlacement>, With<ScriptClone>, With<ScriptObject>)>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    let voices: Vec<Entity> = world
        .query_filtered::<(Entity, Option<&AudioSink>), With<Heard>>()
        .iter(world)
        .map(|(e, sink)| {
            if let Some(sink) = sink {
                sink.stop();
            }
            e
        })
        .collect();
    for e in voices {
        world.despawn(e);
    }
    world.remove_resource::<ScriptRuntime>();
    world.remove_resource::<ScriptWorld>();
    world.remove_resource::<ScriptSource>();
    if let Some(mut r) = world.get_resource_mut::<ParticleReport>() {
        (r.systems, r.particles, r.quads, r.level_clones) = (0, 0, 0, 0);
        r.emitters.clear();
        r.placed = crate::emitters::PlacedReport::default();
    }
    if let Some(mut r) = world.get_resource_mut::<SoundReport>() {
        *r = SoundReport {
            muted: r.muted,
            ended: std::mem::take(&mut r.ended),
            refused: std::mem::take(&mut r.refused),
            files: r.files,
            released: r.released,
            ..Default::default()
        };
    }
    if had {
        info!("the last level's script run taken down");
    }
}

/// The sounds part landing after the level loaded (openspec AR.21): news
/// for the extract the level reads from marks it to read again; any other
/// extract's is no news for it.
fn receive_sounds(feed: Option<Res<SoundsFeed>>, mut level: ResMut<Level>) {
    let Some(feed) = feed else {
        return;
    };
    for ready in feed.0.try_iter() {
        if level.extract_dir() == Some(ready.dir.as_path()) {
            info!(dir = %ready.dir.display(), "the sounds part landed after the level loaded");
            level.sounds_landed = true;
        }
    }
}

/// The sounds read again into the live run once their part has landed
/// (openspec AR.21), on a task off the frame: once the level's scripts are
/// linked and installed, the run's [`guacamole::sounds::SoundPart`] read
/// again for its units, then handed to the run and kept with its part for a
/// reinstall. A play started before stays as it began (silent where the book
/// lacked it); one after finds its recording; the level's music, held
/// while the book lacked the level's sounds, starts (AR.23,
/// [`crate::hero_scripts::hold_music`]). A read for a run since replaced is
/// dropped. Music held for sounds that then aren't coming (the sounds stage
/// ended without them) is let go to run silent (openspec AR.30).
fn sounds_again(
    mut level: ResMut<Level>,
    source: Option<ResMut<ScriptSource>>,
    world: Option<ResMut<ScriptWorld>>,
    runtime: Option<ResMut<guacamole::scripts::ScriptRuntime>>,
    extraction: Option<Res<Extraction>>,
) {
    let level = &mut *level;
    let coming = sounds_coming(extraction.as_deref());
    if let Some(task) = level.sounds_again.as_mut() {
        let Some((program, part)) = check_ready(task) else {
            return;
        };
        level.sounds_again = None;
        let (Some(mut source), Some(mut world)) = (source, world) else {
            return;
        };
        if !Arc::ptr_eq(source.program(), &program) {
            info!("the sounds read again were for a run since replaced: dropped");
            return;
        }
        log_sounds(&part, true);
        world.set_sounds(part.book.clone(), part.stems.clone());
        level.report.sounds_again_ms = level.ms();
        level.report.sounds_again_archives = Some(part.stems.len());
        source.set_sounds(part);
        // The level's music held for them starts (AR.23), or runs silent if
        // they came without the level's (AR.30).
        if let (Some(unit), Some(mut runtime)) = (source.level_unit(), runtime) {
            let music = guacamole::scripts::hold_music_while(
                &mut runtime.vm,
                &mut world,
                source.files(),
                unit,
                coming,
            );
            crate::hero_scripts::log_music(music);
        }
        return;
    }
    if !level.sounds_landed || level.scripts.is_some() {
        // Held for sounds that aren't coming now (AR.30): let go, silent.
        if !coming
            && level.scripts.is_none()
            && let (Some(source), Some(mut world), Some(mut runtime)) = (source, world, runtime)
            && world.music_held() > 0
            && let Some(unit) = source.level_unit()
        {
            let music = guacamole::scripts::hold_music_while(
                &mut runtime.vm,
                &mut world,
                source.files(),
                unit,
                false,
            );
            crate::hero_scripts::log_music(music);
        }
        return;
    }
    let (Some(source), Some(from)) = (source, level.source.as_ref()) else {
        return;
    };
    let program = source.program().clone();
    let (dir, ir) = (from.dir.clone(), from.ir);
    level.sounds_landed = false;
    level.sounds_again = Some(AsyncComputeTaskPool::get().spawn(async move {
        let part = guacamole::sounds::load(&dir, ir, &program.units);
        (program, part)
    }));
}

/// What the level's sounds read found (openspec AR.9, AR.21): the archives
/// and sounds the book holds, and each archive that didn't read; an extract
/// with no sounds part yet (a first run's, before its stage) said once, not
/// an archive at a time.
fn log_sounds(s: &guacamole::sounds::SoundPart, again: bool) {
    info!(
        archives = s.stems.len(),
        sounds = s.sounds(),
        missing = s.missing.len(),
        read_ms = s.read.as_secs_f64() * 1e3,
        again,
        "the level's sounds read"
    );
    // Bootstrap's and init_setup's never have one, nor the screens' the
    // level's end links (openspec AS.53: their sounds aren't in the part).
    let screen = |stem: &str| {
        guac_extract::screens::ARCHIVES
            .iter()
            .any(|a| guac_extract::scripts::stem(a).eq_ignore_ascii_case(stem))
    };
    let missing: Vec<&(String, String)> = s
        .missing
        .iter()
        .filter(|(stem, _)| !matches!(stem.as_str(), "bootstrap" | "init_setup") && !screen(stem))
        .collect();
    if s.stems.is_empty() && !missing.is_empty() {
        info!(
            archives = missing.len(),
            "no sounds part in the extract yet: the level plays silent until it lands"
        );
        return;
    }
    for (stem, why) in missing {
        warn!(%stem, reason = %why, "an archive's sounds didn't read: its scripts' sounds play nothing");
    }
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
                    refused = built.refused.len(),
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
                for x in &built.refused {
                    warn!(level = %built.level, material = %x.material, reason = %x.why, segments = x.segments, "a material the TV can't draw yet: left out");
                }
                r.refused = built
                    .refused
                    .iter()
                    .map(|x| RefusedMaterial {
                        material: x.material.clone(),
                        why: x.why.clone(),
                        segments: x.segments,
                    })
                    .collect();
                let effects =
                    (0..built.drawn.materials.len()).filter_map(|m| built.drawn.effect(m));
                (r.grass_shells, r.warps) = effects.fold((0, 0), |(g, w), e| match e {
                    guacamole::character::effect::Effect::Grass { .. } => (g + 1, w),
                    guacamole::character::effect::Effect::Warp => (g, w + 1),
                });
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
                walkers,
                left_out,
                layout,
                palettes,
                sounds,
                interface,
                levels,
                interface_keys,
                ..
            } = linked;
            let level_name = level
                .source
                .as_ref()
                .map_or_else(String::new, |s| s.level.clone());
            let extract = level.source.as_ref().map(|s| (s.dir.clone(), s.ir));
            if let Some(s) = &sounds {
                log_sounds(s, false);
            }
            guacamole::interface_host::log(interface.as_ref());
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
                        walkers,
                        left_out,
                        layout,
                        palettes,
                        sounds,
                        interface,
                        level: level_name,
                        levels,
                        extract,
                        interface_keys,
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
        let material = match CharacterMaterial::from_ir(m, &scene, texture) {
            Ok(mut material) => {
                material.uniform = level_uniform(material.uniform);
                // A grass shell's or a warp's pass and numbers (AG.24).
                if let Some(effect) = up.built.drawn.effect(start + k) {
                    effect.apply(&mut material);
                }
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
        image_handles,
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
    // The shadow light's cascades on the level's layer (openspec AG.23):
    // what a character's shadow is drawn into and the level's materials
    // read, shown and hidden with the level.
    s.commands.spawn((
        guacamole::character::shadow::shadow_light(Vec3::from_array(
            guacamole::character::shadow::SHADOW_TRAVEL,
        )),
        RenderLayers::layer(LEVEL_LAYER),
        ShadowLight,
        Name::new("the level's shadow light"),
        ChildOf(root),
    ));
    let sky = s
        .commands
        .spawn((
            Name::new("the level's sky"),
            LevelSky,
            Transform::IDENTITY,
            Visibility::Inherited,
            ChildOf(root),
        ))
        .id();
    for (i, (seg, mesh)) in built
        .drawn
        .segments
        .iter()
        .zip(&mesh_handles[..level_meshes])
        .enumerate()
    {
        s.commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials[usize::from(seg.material)].clone()),
            Transform::IDENTITY,
            RenderLayers::layer(LEVEL_LAYER),
            ChildOf(if built.drawn.sky.contains(&i) {
                sky
            } else {
                root
            }),
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
    props.stand(built.walk.start, None, &mut s.commands);
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
    let emitters = built
        .particles
        .iter()
        .map(|(e, generators)| {
            let materials = generators
                .iter()
                .map(|m| m.map(|m| materials[m].clone()))
                .collect();
            (
                e.name.clone(),
                EmitterDraw {
                    emitter: e.clone(),
                    materials,
                },
            )
        })
        .collect();
    let placed: Vec<PlacedDraw> = built
        .placed
        .iter()
        .map(|(p, generators)| PlacedDraw {
            placed: p.clone(),
            draw: EmitterDraw {
                emitter: Arc::new(p.emitter.clone()),
                materials: generators
                    .iter()
                    .map(|m| m.map(|m| materials[m].clone()))
                    .collect(),
            },
        })
        .collect();
    let placed_by_object = placed
        .iter()
        .enumerate()
        .map(|(i, p)| (p.placed.emitter.object, i))
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
        emitters,
        placed,
        placed_by_object,
        placed_refused: std::mem::take(&mut built.placed_refused),
        placed_unread: built.placed_unread.take(),
        creatures: std::mem::take(&mut built.creatures),
        images: image_handles,
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
pub(crate) fn stand_props(
    mut commands: Commands,
    mut level: ResMut<Level>,
    heroes: Query<&HeroBody>,
    scripts: Option<Res<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    mut draws: Query<&mut Transform, Without<SeamBody>>,
    mut poses: Query<(&mut Position, &mut Rotation), With<SeamBody>>,
) {
    let Some(loaded) = level.stage.loaded_mut() else {
        return;
    };
    let at = heroes.iter().next().map_or(loaded.start, |h| h.hero.pivot);
    let unit = source.as_deref().and_then(ScriptSource::level_unit);
    let by = match (scripts.as_deref(), unit) {
        (Some(world), Some(unit)) => Some(world).zip(Some(unit)),
        _ => None,
    };
    let actor_of = |object: u32| {
        let (world, unit) = by?;
        let h = world.handles().get(&Obj {
            unit,
            index: object,
        })?;
        world.actor(*h)
    };
    let mut place = |draw: Entity, t: Transform| {
        if let Ok(mut now) = draws.get_mut(draw)
            && *now != t
        {
            *now = t;
        }
    };
    let mut pose = |body: Entity, p: guac_motion::seam::Pose| {
        let Ok((mut position, mut rotation)) = poses.get_mut(body) else {
            return false;
        };
        position.0 = p.position;
        rotation.0 = p.rotation;
        true
    };
    let actors = by.is_some().then_some(ByActors {
        actor_of: &actor_of,
        place: &mut place,
        pose: &mut pose,
    });
    loaded.props.stand(at, actors, &mut commands);
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
/// ([`view_camera`]): the level's own, and the one the start's pass draws
/// to compile its output.
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
    (time, real): (Res<Time>, Res<Time<Real>>),
    mut redrawn: MessageWriter<Redrawn>,
) {
    if !view.0 || !matches!(level.stage, Stage::Ready(_)) {
        return;
    }
    let Ok(target) = tv.single() else {
        return;
    };
    let target = target.cloned().unwrap_or_default();
    // Chapter 1 at AF's fresh state; another level at its start as that
    // frames chapter 1's (openspec AS.13).
    let at = match (level.source.as_ref(), level.start()) {
        (Some(source), Some(start)) if !source.is_chapter_1() => arithmetic::start_view(start),
        _ => fresh_state_transform(),
    };
    let camera = commands
        .spawn((
            view_camera(target, VIEW_ORDER),
            RenderLayers::layer(LEVEL_LAYER),
            LevelCamera,
            Name::new("level view"),
        ))
        // Over the bundle's own, which a bundle can't hold twice.
        .insert(at)
        .id();
    spawn_alpha_clear(&mut commands, camera, LEVEL_LAYER);
    level.show(&mut commands, camera, time.elapsed());
    level.report.shown_ms = Some(real.elapsed().as_secs_f64() * 1e3);
    info!(level = ?level.report.level, "the level shows (GUAC_LEVEL_VIEW)");
    redrawn.write(Redrawn::Level);
}

/// The sky goes where the eye goes (openspec AS.45): the original draws a
/// level's sky through the view with its translation taken out, so a sky
/// segment's file coordinates are metres from the eye, wherever it stands
/// (AC.13's first capture: chapter 1's 14 sky draws hold 0, 0, -0.2, 0 in
/// the matrix's last column where every other static draw holds the eye's
/// 37.58, -14.97, 114.07, 114.17). Past the dome the view looks out of it
/// otherwise: level 214's stands 550 m about the origin, its second and
/// third islands 750-830 m out, all black behind them.
fn keep_sky_about_the_eye(
    camera: Query<&Transform, (With<LevelCamera>, Without<LevelSky>)>,
    mut sky: Query<&mut Transform, With<LevelSky>>,
) {
    let Some(eye) = camera.iter().next().map(|c| c.translation) else {
        return;
    };
    for mut hung in &mut sky {
        if hung.translation != eye {
            hung.translation = eye;
        }
    }
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
            // Its scroll's rows and an effect's numbers (AG.24) kept.
            let kept = (m.uniform.uv_u, m.uniform.uv_v, m.uniform.effect);
            m.uniform = level_uniform(CharacterUniform::new(reference, &scene, *alpha));
            (m.uniform.uv_u, m.uniform.uv_v, m.uniform.effect) = kept;
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
/// along. What the mask draws is its shared material's: a tint the scripts
/// lay on a clone (AH.67) moves its colour, not its glow amount, even one
/// faded in the frame it comes up ([`crate::tints::TintSystems`] run after
/// this) or already on it. A mask off a fading copy blended too, a pipeline
/// the census doesn't hold, compiled mid-route the first time the cannon's
/// prompt came up (the fixes lane's Mac step, 2026-10-31).
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
            Option<&crate::tints::TintedPart>,
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
    for (part, mesh, material, layers, skin, unculled, tinted) in &parts {
        commands.entity(part).insert(Twinned);
        if *layers != level {
            continue;
        }
        let source = tinted.map_or(&material.0, |t| &t.shared);
        let id = source.id();
        let twin = match twins.0.get(&id) {
            Some(t) => t.clone(),
            None => {
                let made = materials
                    .get(source)
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

/// Every camera with its target, and the level's: whether a UI draws over
/// the level, for the overlay's and the alert's parts of `/api/_test/tv`
/// (openspec AQ.8).
#[derive(SystemParam)]
pub(crate) struct OverLevel<'w, 's> {
    cameras: Query<'w, 's, (&'static Camera, Option<&'static RenderTarget>)>,
    level_cameras: Query<'w, 's, Entity, With<LevelCamera>>,
}

impl OverLevel<'_, '_> {
    /// The UI on `target`'s camera draws over a level camera
    /// ([`draws_over`]); false with no level camera.
    pub(crate) fn over(&self, target: Option<&ComputedUiTargetCamera>) -> bool {
        let ui = target
            .and_then(ComputedUiTargetCamera::get)
            .and_then(|c| self.cameras.get(c).ok());
        ui.is_some_and(|ui| {
            self.level_cameras
                .iter()
                .filter_map(|c| self.cameras.get(c).ok())
                .any(|level| draws_over(ui, level))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::level::{
        LEVEL, START, atmosphere, lights, multi2, segment, write_level, write_props, zone,
    };
    use crate::test_support::turntable::{app, extract};
    use crate::tv_state::TV_ORDER;
    use crate::warm::PipelineWatch;

    use guac_ir::Sha256;
    use guac_ir::level::LevelMaterial;
    use guac_ir::model::{Mask, TextureRole, TextureSlot};
    use guacamole::character::warm::MeshShape;
    use guacamole::level_scene::level_lights;
    use std::path::Path;

    /// AU.1: the spawn and particle parts read every walker's file the
    /// scripts part lists beside the base's; a walker's that doesn't read
    /// leaves his alone out (what his attacks spawn draws nothing), the
    /// base's still read, where a base file missing fails the part.
    #[test]
    fn a_walkers_spawn_files_read_beside_the_bases_and_a_missing_one_alone_is_left_out() {
        use guac_ir::particle::Emitters;
        use guac_ir::spawn::SpawnModels;
        let tmp = tempfile::tempdir().unwrap();
        let identity = guac_ir::DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([7; 32]),
        };
        let source = LevelSource::chapter_1(tmp.path().into(), IrFormat::Ron, identity.clone());
        let write = |rel: String, archive: &str, prop: Option<&str>, emitters: bool| {
            let path = tmp.path().join(&rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let text = if emitters {
                guac_ir::to_ron(&Emitters {
                    identity: identity.clone(),
                    archive: archive.into(),
                    emitters: Vec::new(),
                    refused: Vec::new(),
                })
            } else {
                guac_ir::to_ron(&SpawnModels {
                    identity: identity.clone(),
                    archive: archive.into(),
                    props: prop
                        .map(|p| guac_ir::level::Prop {
                            path: p.into(),
                            skeleton: None,
                            materials: Vec::new(),
                            segments: Vec::new(),
                            idle: None,
                        })
                        .into_iter()
                        .collect(),
                    refused: Vec::new(),
                })
            };
            std::fs::write(path, text.unwrap()).unwrap();
        };
        for (i, archive) in guac_extract::scripts::spawn_archives().iter().enumerate() {
            write(
                IrFormat::Ron.spawn_path(archive),
                archive,
                Some(&format!("base_{i}.mdl")),
                false,
            );
            write(
                guac_extract::particles::path(IrFormat::Ron, archive),
                archive,
                None,
                true,
            );
        }
        let list = guac_extract::scripts::WalkerList {
            listed: vec!["016_Spyro".into(), "112_TitanLife".into()],
            left_out: Vec::new(),
        };
        let rel = tmp.path().join(guac_extract::scripts::WALKERS_FILE);
        std::fs::create_dir_all(rel.parent().unwrap()).unwrap();
        std::fs::write(rel, serde_json::to_vec(&list).unwrap()).unwrap();
        let spyro = guac_extract::scripts::character_archive("016_Spyro");
        write(
            IrFormat::Ron.spawn_path(&spyro),
            &spyro,
            Some("spyro_fireball.mdl"),
            false,
        );
        write(
            guac_extract::particles::path(IrFormat::Ron, &spyro),
            &spyro,
            None,
            true,
        );
        // Tree Rex's two files missing: his alone are left out.
        let paths: Vec<String> = read_spawns(&source)
            .unwrap()
            .into_iter()
            .map(|m| m.path)
            .collect();
        assert_eq!(paths, ["base_0.mdl", "base_1.mdl", "spyro_fireball.mdl"]);
        let emitters = read_particles(&source).unwrap();
        assert_eq!(
            emitters
                .iter()
                .map(|e| e.archive.as_str())
                .collect::<Vec<_>>(),
            [
                "permanent/global.bld",
                "character/009_eruptor.bld",
                spyro.as_str()
            ]
        );
        // A base file missing fails the part.
        std::fs::remove_file(
            tmp.path()
                .join(IrFormat::Ron.spawn_path(&guac_extract::scripts::spawn_archives()[0])),
        )
        .unwrap();
        assert!(read_spawns(&source).is_err());
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

    /// Sends `source` to the loader and runs one update. The read runs on a
    /// task, so the update may find it finished and step on to `uploading`
    /// (or `failed`); `task_done` makes that so: the load begun by its own
    /// system, the task spun on until it is finished, then the update (no
    /// sleeps), and false leaves the update to race the task as a frame does.
    /// Either way the load is the new source's by the time this returns.
    fn send_source(app: &mut App, source: &LevelSource, task_done: bool) {
        use bevy::ecs::system::RunSystemOnce;
        let (tx, feed) = level_channel();
        tx.send(source.clone()).unwrap();
        app.insert_resource(feed);
        if task_done {
            app.world_mut().run_system_once(receive_level).unwrap();
            let level = app.world().resource::<Level>();
            let Stage::Reading(task) = &level.stage else {
                panic!("the load didn't begin reading: {}", state(app));
            };
            while !task.is_finished() {
                std::thread::yield_now();
            }
        }
        app.update();
        let level = app.world().resource::<Level>();
        assert!(level.source.as_ref() == Some(source), "the new load begun");
        assert!(
            matches!(
                level.stage,
                Stage::Reading(_) | Stage::Uploading(_) | Stage::Failed
            ),
            "a load begun, whatever stage its read has reached: {}",
            state(app)
        );
        // Its report is its own: one update in, nothing the last load
        // reached (its upload, the parts it waited on, its ready, its
        // refusal) is left in it; a refused read reports its own reason.
        if !matches!(level.stage, Stage::Failed) {
            let r = level.report();
            assert_eq!(
                (
                    r.uploaded_ms,
                    r.not_resident,
                    r.ready_ms,
                    r.reason.as_deref()
                ),
                (None, 0, None, None),
                "the last load's report let go"
            );
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

    /// AS.13 and AS.51: a level past chapter 1 takes the last level's run
    /// down as it comes (its placements, clones and objects, the run
    /// itself, its voices; the particles' and sounds' reports emptied), so
    /// that run drives nothing of it, then links and installs its own run
    /// from its own script part, its listing not chapter 1's; chapter 1
    /// asked for again links and installs its run anew.
    #[test]
    fn a_level_past_chapter_1_takes_the_last_run_down_and_runs_its_own() {
        use guac_formats::roster::builder::{disc, sample};
        use guacamole::scripts::{ScriptPlacement, ScriptRuntime};
        const ARENA: &str = "pvp_level_001_cubedungeon";
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        assert!(source.is_chapter_1());
        let d = disc(&sample());
        let (creature, _) = guac_formats::script::builder::creature();
        let entries = [&d.bootstrap, &d.global, &d.init_setup, &creature, &creature];
        for level in [LEVEL, ARENA] {
            for ((rel, _), bytes) in level_scripts::files(level).iter().zip(entries) {
                let path = tmp.path().join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, bytes).unwrap();
            }
        }
        let mut app = level_app(tmp.path(), source.clone(), false);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.world().contains_resource::<ScriptRuntime>() {
            assert!(Instant::now() < deadline, "the run never installed");
            app.update();
        }
        let placements = |app: &mut App| {
            app.world_mut()
                .query_filtered::<(), With<ScriptPlacement>>()
                .iter(app.world())
                .count()
        };
        // The fixture's part places nothing of its own: one of the run's
        // placements, as an install spawns them; a voice of its music and
        // the reports of what it plays and draws, as the player and the
        // emitters leave them.
        app.world_mut()
            .spawn(ScriptPlacement(Obj { unit: 4, index: 1 }));
        assert_eq!(placements(&mut app), 1);
        let voice = app
            .world_mut()
            .spawn(crate::sound_player::Heard::of_run(1))
            .id();
        app.insert_resource(crate::sound_player::SoundReport {
            muted: true,
            archives: 69,
            started: 7,
            voices: 1,
            files: 2,
            ..Default::default()
        });
        app.insert_resource(crate::emitters::ParticleReport {
            systems: 3,
            started: 3,
            particles: 12,
            quads: 12,
            emitters: vec!["CoinSparkle".into()],
            seen: vec!["CoinSparkle".into()],
            ..Default::default()
        });

        let arena = LevelSource::named(
            source.dir.clone(),
            source.ir,
            source.identity.clone(),
            ARENA,
        );
        assert!(!arena.is_chapter_1());
        send_source(&mut app, &arena, false);
        // The last run gone: none in, or (its part linked within the frame)
        // the arena's own.
        let w = app.world();
        let source_level = w
            .get_resource::<crate::hero_scripts::ScriptSource>()
            .map(|s| s.level().to_owned());
        assert!(
            source_level.as_deref().is_none_or(|l| l == ARENA),
            "{source_level:?}"
        );
        assert_eq!(
            w.contains_resource::<ScriptRuntime>(),
            source_level.is_some()
        );
        assert!(
            w.resource::<Level>().scripts.is_some() || source_level.is_some(),
            "its own part read"
        );
        assert_eq!(report(&app).scripts_reason, None);
        assert_eq!(placements(&mut app), 0);
        assert!(app.world().get_entity(voice).is_err(), "its voice went");
        let w = app.world();
        let sounds = w.resource::<crate::sound_player::SoundReport>();
        assert_eq!(
            (
                sounds.muted,
                sounds.archives,
                sounds.started,
                sounds.voices,
                sounds.files
            ),
            (true, 0, 0, 0, 2),
            "nothing plays; the build's muting and the player's files as they were"
        );
        let particles = w.resource::<crate::emitters::ParticleReport>();
        assert_eq!(
            (
                particles.systems,
                particles.particles,
                particles.quads,
                particles.started
            ),
            (0, 0, 0, 3),
            "nothing draws; the starts since the TV came up kept"
        );
        assert!(particles.emitters.is_empty() && particles.seen.len() == 1);
        // Its own run, from its own part: not chapter 1's listing.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.world().contains_resource::<ScriptRuntime>() {
            assert!(Instant::now() < deadline, "the arena's run never came");
            app.update();
        }
        let s = app.world().resource::<crate::hero_scripts::ScriptSource>();
        assert_eq!((s.level(), s.chapter_1()), (ARENA, false));
        assert!(
            s.program().units.last().is_some_and(|u| u.contains(ARENA)),
            "{:?}",
            s.program().units
        );

        send_source(&mut app, &source, false);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.world().contains_resource::<ScriptRuntime>() {
            assert!(Instant::now() < deadline, "chapter 1's run never came back");
            app.update();
        }
        assert_eq!(report(&app).scripts_reason, None);
        let s = app.world().resource::<crate::hero_scripts::ScriptSource>();
        assert_eq!((s.level(), s.chapter_1()), (LEVEL, true));
    }

    /// AS.16: leaving a level for another takes the first down as the next
    /// comes: its root and everything under it (its parts, its props' draws,
    /// its shadow light), its solids in Avian's world (the collision's and a
    /// standing prop's), and its textures, meshes and materials once their
    /// handles go, so the next, the same level from another extract, comes
    /// up with as many of each as the first had and none of the first's.
    #[test]
    fn leaving_a_level_takes_its_entities_solids_and_assets_down() {
        use crate::physics::SeamBody;
        use guac_ir::actor::{Shape, Solid};
        let level_in = |dir: &Path| {
            let source = write_level(dir, &lights());
            let scene: guac_ir::level::Scene = guac_ir::from_ron(
                &std::fs::read_to_string(dir.join(source.ir.level_path(LEVEL, SCENE))).unwrap(),
            )
            .unwrap();
            let shape = MeshShape {
                uv_sets: 2,
                tangents: true,
                colours: false,
                skinned: false,
            };
            let gourd = guac_ir::level::Prop {
                path: "gourd.mdl".into(),
                skeleton: None,
                materials: vec![scene.scenes[0].materials[0].clone()],
                segments: vec![segment(0, shape, 0.0)],
                idle: None,
            };
            let placed = guac_ir::level::Placement {
                object: 20,
                id: 0,
                name: "gourd".into(),
                script: None,
                position: [START[0] + 4.5, START[1], START[2]],
                rotation: [0.0; 3],
                scale: [100.0; 3],
                model: Some("gourd.mdl".into()),
                solid: Some(Solid {
                    shape: Shape::Sphere,
                    solid: true,
                    world: true,
                    actor_surface: false,
                    offset: [0.0; 3],
                    rotation: [0.0; 3],
                    width: 7.25,
                    height: 6.5,
                    length: 3.0,
                    weight: 1.0,
                    rebound: 0.0,
                }),
                movement: None,
                waypoints: None,
                activity_state: 4,
                activation_range: 160.0,
            };
            write_props(dir, vec![gourd], vec![placed]);
            source
        };
        let (first_dir, next_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = level_in(first_dir.path());
        let next = level_in(next_dir.path());
        let mut app = level_app(first_dir.path(), first, false);
        app.add_plugins(crate::physics::SeamPlugin);
        app.finish();
        app.cleanup();
        until(&mut app, &["ready"], |_| {});
        let world_counts = |app: &mut App| {
            let w = app.world_mut();
            let bodies = w
                .query_filtered::<Entity, With<SeamBody>>()
                .iter(w)
                .collect::<Vec<_>>();
            (
                bodies,
                w.resource::<Assets<Mesh>>().len(),
                w.resource::<Assets<Image>>().len(),
                w.resource::<Assets<CharacterMaterial>>().len(),
            )
        };
        let root = app.world().resource::<Level>().loaded().unwrap().root;
        let mut under = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            under.push(e);
            if let Some(children) = app.world().get::<Children>(e) {
                stack.extend(children.iter());
            }
        }
        let (bodies, meshes, images, materials) = world_counts(&mut app);
        // The collision's mesh and the gourd's sphere; the level's parts,
        // its props' draw and its shadow light under its root.
        assert_eq!(bodies.len(), 2, "{bodies:?}");
        assert!(under.len() > 3, "{under:?}");

        send_source(&mut app, &next, false);
        app.update();
        for e in under.iter().chain(&bodies) {
            assert!(
                app.world().get_entity(*e).is_err(),
                "{e} outlived its level"
            );
        }
        assert!(
            world_counts(&mut app).0.is_empty(),
            "no solid but the next level's"
        );
        until(&mut app, &["ready"], |_| {});
        for _ in 0..3 {
            app.update();
        }
        let (now_bodies, now_meshes, now_images, now_materials) = world_counts(&mut app);
        assert_eq!(now_bodies.len(), 2);
        assert!(now_bodies.iter().all(|b| !bodies.contains(b)));
        assert_eq!(
            (now_meshes, now_images, now_materials),
            (meshes, images, materials),
            "the first level's assets let go"
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

    /// AS.49: a ready level isn't one the hero can come into while its
    /// script part is still linking (he'd walk it with no run, his join
    /// off), and is once the part is in.
    #[test]
    fn a_ready_level_is_walkable_only_once_its_scripts_are_in() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        let mut level = app.world_mut().resource_mut::<Level>();
        level.scripts = None;
        assert!(level.walkable().is_some());
        level.scripts = Some(
            AsyncComputeTaskPool::get()
                .spawn(std::future::pending::<Result<level_scripts::Linked, String>>()),
        );
        assert!(level.walkable().is_none(), "its part still linking");
        level.scripts = None;
        assert!(level.walkable().is_some());
    }

    /// A second source while the first still waits on the renderer: what
    /// the first left pending goes with it, so the second waits on its own
    /// parts alone (the first's never turn up).
    #[test]
    fn a_new_source_waits_on_its_own_parts_alone() {
        for done in [false, true] {
            a_new_source_waits_on_its_own_parts(done);
        }
    }

    fn a_new_source_waits_on_its_own_parts(done: bool) {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = write_level(a.path(), &lights());
        let second = write_level(b.path(), &lights());
        let mut app = level_app(a.path(), first, true);
        until(&mut app, &["waiting"], |_| {});
        assert_eq!(report(&app).not_resident, 11);
        send_source(&mut app, &second, done);
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
                    -10.0,
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
        // The fence, with no range, a prop that stands by its actor alone
        // (openspec AG.60); `213_Init_Location` nothing to stand.
        assert_eq!(
            (r.props, r.models, r.templates, r.creatures, r.by_script),
            (3, 2, 1, 1, 0),
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
                ("prop fence".to_owned(), false),
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
                ("prop fence".to_owned(), false),
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

        // Once the level's scripts run, each prop stands as its actor does
        // (openspec AG.60): the gourd and the coin by AG.7's rule, as the
        // activity rule wakes them from where the hero stands (the start
        // here), the fence, with no range, while its actor is awake (a
        // deactivation buffer of 0); each drawn and solid where its actor
        // stands, its file's place here.
        use guacamole::script_engine::Solid as ActorSolid;
        use guacamole::scripts::{Files, Placed, ScriptWorld, headless_objects};
        let (gourd, coin, fence) = (
            Obj { unit: 4, index: 20 },
            Obj { unit: 4, index: 21 },
            Obj { unit: 4, index: 24 },
        );
        let placed = |obj, name: &str, range, dx: f32, shape, size: [f32; 3]| Placed {
            position: [START[0] + dx, START[1], START[2]],
            range,
            solid: Some(ActorSolid {
                width: size[0],
                height: size[1],
                length: size[2],
                shape,
                // The file's bit 3, as each solid's here.
                interacts_with_actors: 1,
                ..ActorSolid::default()
            }),
            ..Placed::named(obj, name, None, 4)
        };
        let files = |fence: Placed| Files {
            placed: vec![
                placed(gourd, "gourd", 160.0, 4.5, 3, [7.25, 6.5, 3.0]),
                placed(coin, "coin", 160.0, 49.25, 0, [3.0, 6.0, 3.0]),
                fence,
            ],
            ..Files::default()
        };
        let fence_file = placed(fence, "fence", 0.0, -10.0, 1, [23.0, 18.0, 3.0]);
        let handles = HashMap::from([(gourd, 0x20), (coin, 0x21), (fence, 0x24)]);
        let world_of = |files: &Files| {
            let mut w = ScriptWorld::new(files, &handles, headless_objects(&handles));
            w.activity_at(&[start]);
            w
        };
        let mut world = world_of(&files(fence_file.clone()));
        let source = crate::hero_scripts::ScriptSource::of(
            Arc::new(guac_script::Program::default()),
            files(fence_file.clone()),
            vec![2, 4],
        );
        app.insert_resource(source);
        app.insert_resource(world);
        // Updates until a fixed step has stood the props as `done` wants.
        let steps_until = |app: &mut App, done: &dyn Fn(&PropsReport) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !done(&props(app)) {
                assert!(Instant::now() < deadline, "{:?}", props(app));
                app.update();
            }
            app.update();
        };
        steps_until(&mut app, &|r| r.names == ["gourd", "fence"]);
        assert_eq!(props(&app).names, ["gourd", "fence"]);
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 20, PROP_BODIES + 24]);
        assert_eq!(down_on_the_gourd(&mut app), Some(PROP_BODIES + 20));
        assert_eq!(
            props(&app).scripted,
            Vec::<String>::new(),
            "where the file has them"
        );

        // AH.21: the level's scripts take the gourd's placement off (broken):
        // the next fixed step hides its draw and takes its solid out of
        // Avian's world, for good wherever the hero stands. Another unit's
        // object of the same index stands as before.
        world = app.world_mut().remove_resource::<ScriptWorld>().unwrap();
        assert!(world.take_off(gourd));
        assert!(!world.take_off(Obj { unit: 2, index: 20 }));
        app.insert_resource(world);
        let deadline = Instant::now() + Duration::from_secs(5);
        while props(&app).names != ["fence"] {
            assert!(Instant::now() < deadline, "the gourd never went");
            app.update();
        }
        let r = props(&app);
        assert_eq!(
            (r.standing, r.solid, r.taken_off.clone()),
            (1, 1, vec!["gourd".to_owned()])
        );
        assert_eq!(
            shown(&mut app),
            [
                ("prop coin".to_owned(), false),
                ("prop fence".to_owned(), true),
                ("prop gourd".to_owned(), false)
            ]
        );
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 24]);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(down_on_the_gourd(&mut app), Some(1), "the floor, for good");

        // AG.60: a script moving and turning the fence's actor moves its
        // draw and its solid with it, the solid moved rather than rebuilt.
        let fence_draw = |app: &mut App| {
            let mut q = app.world_mut().query::<(&Name, &Transform)>();
            q.iter(app.world())
                .find(|(n, _)| n.as_str() == "prop fence")
                .map(|(_, t)| *t)
                .unwrap()
        };
        let fence_body = |app: &mut App| {
            let mut q = app.world_mut().query::<(
                &crate::physics::SeamBody,
                &avian3d::prelude::Position,
                &avian3d::prelude::Rotation,
            )>();
            q.iter(app.world())
                .find(|(b, ..)| b.id.0 == PROP_BODIES + 24)
                .map(|(_, p, r)| (p.0, r.0))
                .unwrap()
        };
        let (was_at, _) = fence_body(&mut app);
        world = app.world_mut().remove_resource::<ScriptWorld>().unwrap();
        let moved = Vec3::new(START[0] - 10.0, START[1], START[2] + 3.0);
        assert!(world.place(0x24, moved, 90.0));
        app.insert_resource(world);
        steps_until(&mut app, &|r| !r.scripted.is_empty());
        let t = fence_draw(&mut app);
        assert_eq!(t.translation, moved);
        assert_eq!(t.rotation, guac_motion::solid::euler([0.0, 90.0, 0.0]));
        let (now_at, turned) = fence_body(&mut app);
        assert!(
            (now_at - was_at - Vec3::Z * 3.0).length() < 1e-4,
            "{was_at} -> {now_at}"
        );
        assert!((turned.to_axis_angle().1.to_degrees() - 90.0).abs() < 1e-3);
        let r = props(&app);
        assert_eq!(
            (r.scripted.clone(), r.rebuilt),
            (vec!["fence".to_owned()], 0)
        );

        // Its start script making its solid a cylinder at half its size and
        // its tint see-through: the solid rebuilt, the draw hidden while the
        // solid stands.
        let mut changed = fence_file.clone();
        changed.scale = [50.0; 3];
        changed.tint = 0x8080_8000;
        changed.solid = Some(ActorSolid {
            shape: 4,
            ..changed.solid.unwrap()
        });
        let mut world = world_of(&files(changed.clone()));
        assert!(world.take_off(gourd), "broken still");
        app.insert_resource(world);
        steps_until(&mut app, &|r| r.rebuilt > 0);
        let r = props(&app);
        assert_eq!((r.names.clone(), r.rebuilt), (vec!["fence".to_owned()], 1));
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 24]);
        assert!(!shown(&mut app).contains(&("prop fence".to_owned(), true)));
        assert_eq!(fence_draw(&mut app).scale, Vec3::splat(0.5));
        assert_eq!((r.solid, r.sensors), (1, 0));

        // Falling down, its script turns its `interacts with actors` off
        // (`WoodFence_Spawner.ai`'s 'Fall Down'): the solid rebuilt sensing,
        // stopping nothing until the script removes it.
        changed.solid = Some(ActorSolid {
            interacts_with_actors: 0,
            ..changed.solid.unwrap()
        });
        let mut world = world_of(&files(changed));
        assert!(world.take_off(gourd), "broken still");
        app.insert_resource(world);
        steps_until(&mut app, &|r| r.rebuilt > 1);
        let r = props(&app);
        assert_eq!((r.solid, r.sensors, r.rebuilt), (0, 1, 2));
        assert_eq!(bodies(&mut app), [1, PROP_BODIES + 24]);
    }

    /// AG.60, AS.63: a level template draws its clones from its model
    /// among the props', found by path as the spawn part's are, whatever
    /// its name; a marker that shows nothing (`props::see_through`) and a
    /// pebble `props::UNDRAWN_SPAWNS` names draw none.
    #[test]
    fn a_templates_model_is_there_for_its_clones() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let scene: guac_ir::level::Scene = guac_ir::from_ron(
            &std::fs::read_to_string(tmp.path().join(source.ir.level_path(LEVEL, SCENE))).unwrap(),
        )
        .unwrap();
        let shape = MeshShape {
            uv_sets: 2,
            tangents: true,
            colours: false,
            skinned: false,
        };
        let model = |path: &str| guac_ir::level::Prop {
            path: path.into(),
            skeleton: None,
            materials: vec![scene.scenes[0].materials[0].clone()],
            segments: vec![segment(0, shape, 0.0)],
            idle: None,
        };
        let template = |object, name: &str, model: &str| guac_ir::level::Placement {
            object,
            id: 0,
            name: name.into(),
            script: None,
            position: START,
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: Some(model.into()),
            solid: None,
            movement: None,
            waypoints: None,
            activity_state: 5,
            activation_range: 0.0,
        };
        let mut marker = model("inviso.mdl");
        if let guac_ir::level::LevelMaterial::Multi2(m) = &mut marker.materials[0] {
            m.states.blend = guac_ir::model::Blend::Alpha;
            m.params.material_tint[3] = 0;
        }
        write_props(
            tmp.path(),
            vec![
                model("egg.mdl"),
                model("lock.mdl"),
                model("pebble.mdl"),
                marker,
            ],
            vec![
                template(20, "Bird_nest_egg", "egg.mdl"),
                template(21, "Chest_Lock", "lock.mdl"),
                template(22, guacamole::props::UNDRAWN_SPAWNS[0], "pebble.mdl"),
                template(23, "Canon_Barrel_Marker", "inviso.mdl"),
            ],
        );
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        let level = app.world().resource::<Level>();
        let models = level.spawn_models().expect("loaded");
        assert_eq!(models.get("egg.mdl").map(Vec::len), Some(1));
        assert_eq!(models.get("lock.mdl").map(Vec::len), Some(1));
        assert!(!models.contains_key("pebble.mdl"));
        assert!(!models.contains_key("inviso.mdl"));
    }

    /// AH.87: the particle part's emitters load with the level, each
    /// textured generator on a material of its own keyed among the level's,
    /// an untextured one on none; without the part, or with another dump's,
    /// the level loads all the same and no emitter draws.
    #[test]
    fn the_particle_parts_emitters_load_with_their_materials() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source.clone(), false);
        until(&mut app, &["ready"], |_| {});
        let level = app.world().resource::<Level>();
        assert!(level.emitters().expect("loaded").0.is_empty());

        crate::test_support::level::write_particles(tmp.path());
        let mut app = level_app(tmp.path(), source.clone(), false);
        until(&mut app, &["ready"], |_| {});
        let level = app.world().resource::<Level>();
        let (emitters, _) = level.emitters().expect("loaded");
        let trail = emitters.get("Trail").expect("the fighter's emitter");
        assert_eq!(trail.emitter.generators.len(), 2);
        let materials = app.world().resource::<Assets<CharacterMaterial>>();
        let m = materials
            .get(trail.materials[0].as_ref().expect("textured"))
            .expect("built");
        assert_eq!(
            (m.key.shader.pixel(), m.key.shader.vertex()),
            (0x0000, 0x1000)
        );
        assert!(trail.materials[1].is_none(), "untextured draws nothing");
        let keys = &level.loaded().unwrap().keys;
        let particle = guacamole::census::PARTICLES[0].key(false).unwrap();
        assert!(keys.contains(&particle), "its key among the level's");

        // Another dump's part: read, refused, nothing drawn.
        for archive in guac_extract::particles::archives() {
            let path = tmp.path().join(guac_extract::particles::path(
                guac_extract::intros::IrFormat::Ron,
                &archive,
            ));
            let mut e: guac_ir::particle::Emitters =
                guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
            e.identity.title_id = "BLUS00000".into();
            std::fs::write(&path, guac_ir::to_ron(&e).unwrap()).unwrap();
        }
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        let level = app.world().resource::<Level>();
        assert!(level.emitters().expect("loaded").0.is_empty());
    }

    /// AG.21: the level's solids come and go with it. A new source's load
    /// takes the old one's static bodies out of Avian's world, so the hero
    /// never stands on a level that's gone.
    #[test]
    fn a_new_load_takes_the_old_levels_solids_with_it() {
        for done in [false, true] {
            a_new_load_takes_the_old_solids(done);
        }
    }

    fn a_new_load_takes_the_old_solids(done: bool) {
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
        send_source(&mut app, &second, done);
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
            Some(
                "the level has no 213_Init_Location or CS_Hub_Intro_Final_Positions with a waypoint, nor a placement running InitializeLevel.ai"
            )
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
        let mut u = CharacterUniform::new(&guac_ir::shading::Material::new(), &Scene::new(), 0.0);
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
            CharacterMaterial::from_ir(ir, &scene, |_| Handle::default()).unwrap()
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

    /// AG.23: the level comes with its shadow light, under its root (shown and
    /// hidden with it) on its layer, drawing cascades along the original's
    /// shadow projection (`character::shadow::SHADOW_TRAVEL`).
    #[test]
    fn the_level_comes_with_its_shadow_light() {
        let tmp = tempfile::tempdir().unwrap();
        let l = lights();
        let source = write_level(tmp.path(), &l);
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        let (root, materials) = {
            let loaded = app.world().resource::<Level>().loaded().unwrap();
            (loaded.root, loaded.materials.clone())
        };
        let world = app.world_mut();
        let mut q = world.query_filtered::<(
            &DirectionalLight,
            &Transform,
            &RenderLayers,
            &ChildOf,
        ), With<ShadowLight>>();
        let found: Vec<(bool, Vec3, RenderLayers, Entity)> = q
            .iter(world)
            .map(|(d, t, r, c)| (d.shadow_maps_enabled, *t.forward(), r.clone(), c.parent()))
            .collect();
        assert_eq!(found.len(), 1, "one shadow light");
        let (shadows, forward, layers, parent) = &found[0];
        assert!(*shadows);
        assert_eq!(*layers, RenderLayers::layer(LEVEL_LAYER));
        assert_eq!(*parent, root);
        // Along the original's shadow projection, not the key light's travel.
        let along = Vec3::from_array(guacamole::character::shadow::SHADOW_TRAVEL).normalize();
        assert!(forward.dot(along) > 1.0 - 1e-5, "{forward:?}");
        let key =
            world_scene(&level_lights(&l, &zone()).unwrap(), &LevelFog::NONE).lights[0].travel;
        assert!(
            forward.dot(Vec3::from_array(key).normalize()) < 0.99,
            "not the key's"
        );
        // The scene the level's materials are made in receives it (the
        // fixture's ground owns it off, as its masks say).
        assert!(world_scene(&level_lights(&l, &zone()).unwrap(), &LevelFog::NONE).shadows);
        let assets = world.resource::<Assets<CharacterMaterial>>();
        let ground = assets.get(&materials[0]).unwrap();
        assert!(!ground.key.shader.switches().shadows);
    }

    /// The fixes lane's Mac step: a part already on a tinted copy blending
    /// when its mask comes up (a clone fading in, AH.67) gets the mask of its
    /// shared material, on the census's key (its own states, unblended), not
    /// one off the blending copy.
    #[test]
    fn a_part_on_a_fading_copy_gets_its_shared_materials_glow_mask() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        let key = guacamole::census::LEVEL[1].key(false).unwrap();
        let ir = guacamole::census::material_for(&key);
        let scene = Scene {
            fog: true,
            ..Scene::new()
        };
        let lit = CharacterMaterial::from_ir(&ir, &scene, |_| Handle::default()).unwrap();
        assert_eq!(lit.key.states.blend, guac_ir::model::Blend::Opaque);
        let fading = crate::tints::tinted(&lit, 0x8080_8040);
        assert_ne!(fading.key.states, lit.key.states, "the copy blends");
        let (shared, copy) = {
            let mut assets = app.world_mut().resource_mut::<Assets<CharacterMaterial>>();
            (assets.add(lit.clone()), assets.add(fading))
        };
        let part = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::<Mesh>::default()),
                MeshMaterial3d(copy),
                RenderLayers::layer(LEVEL_LAYER),
                crate::tints::TintedPart {
                    shared: shared.clone(),
                },
            ))
            .id();
        app.update();
        app.update();
        let world = app.world_mut();
        let mut q = world
            .query_filtered::<(&ChildOf, &MeshMaterial3d<CharacterMaterial>), With<GlowTwin>>();
        let twins: Vec<(Entity, Handle<CharacterMaterial>)> = q
            .iter(world)
            .map(|(c, m)| (c.parent(), m.0.clone()))
            .collect();
        assert_eq!(twins.len(), 1, "{twins:?}");
        assert_eq!(twins[0].0, part);
        let assets = world.resource::<Assets<CharacterMaterial>>();
        let mask = assets.get(&twins[0].1).unwrap();
        assert_eq!(mask.key.pass, guacamole::character::Pass::LevelGlow);
        assert_eq!(mask.key.states, lit.key.states, "the shared one's states");
        assert_eq!(mask.key, lit.level_glow().unwrap().key, "the census's key");
    }

    /// Fades the parts [`FadeNow`] names the frame they come up, as the
    /// clones' tint does (a copy blending, its shared one kept by command).
    #[derive(Resource, Default)]
    struct FadeNow(Vec<Entity>);

    fn fade_now(
        mut commands: Commands,
        fade: Res<FadeNow>,
        mut parts: Query<(
            &mut MeshMaterial3d<CharacterMaterial>,
            Option<&crate::tints::TintedPart>,
        )>,
        mut materials: ResMut<Assets<CharacterMaterial>>,
    ) {
        for &e in &fade.0 {
            crate::tints::tint_part(e, 0x8080_8040, &mut commands, &mut parts, &mut materials);
        }
    }

    /// The fixes lane's Mac step: a part a tint fades the very frame it
    /// comes up (a clone fading in, AH.67) gets its shared material's mask,
    /// on the census's key: the masks are made before the tints are laid.
    #[test]
    fn a_part_faded_the_frame_it_comes_up_gets_its_shared_materials_glow_mask() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let mut app = level_app(tmp.path(), source, false);
        app.init_resource::<FadeNow>()
            .add_systems(PostUpdate, fade_now.in_set(crate::tints::TintSystems));
        let key = guacamole::census::LEVEL[1].key(false).unwrap();
        let ir = guacamole::census::material_for(&key);
        let scene = Scene {
            fog: true,
            ..Scene::new()
        };
        let lit = CharacterMaterial::from_ir(&ir, &scene, |_| Handle::default()).unwrap();
        let shared = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(lit.clone());
        let part = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::<Mesh>::default()),
                MeshMaterial3d(shared.clone()),
                RenderLayers::layer(LEVEL_LAYER),
            ))
            .id();
        app.world_mut().resource_mut::<FadeNow>().0.push(part);
        app.update();
        app.update();
        let world = app.world_mut();
        let faded = world
            .get::<MeshMaterial3d<CharacterMaterial>>(part)
            .unwrap()
            .0
            .clone();
        assert_ne!(faded, shared, "faded on a copy");
        let mut q = world
            .query_filtered::<(&ChildOf, &MeshMaterial3d<CharacterMaterial>), With<GlowTwin>>();
        let twins: Vec<Handle<CharacterMaterial>> = q
            .iter(world)
            .filter(|(c, _)| c.parent() == part)
            .map(|(_, m)| m.0.clone())
            .collect();
        assert_eq!(twins.len(), 1);
        let assets = world.resource::<Assets<CharacterMaterial>>();
        assert_eq!(
            assets.get(&twins[0]).unwrap().key,
            lit.level_glow().unwrap().key,
            "the census's key"
        );
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
        for done in [false, true] {
            a_level_without_its_world_lights_fails(done);
        }
    }

    fn a_level_without_its_world_lights_fails(done: bool) {
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
        send_source(&mut app, &other, done);
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
        // The sky went with it (AS.45), its segment under it, and the rest
        // of the level stayed where the file puts it.
        let mut sky = app
            .world_mut()
            .query_filtered::<(&GlobalTransform, &Children), With<LevelSky>>();
        let (hung, held) = sky.single(app.world()).unwrap();
        assert_eq!(
            hung.translation(),
            moved.translation,
            "the sky's left behind"
        );
        assert_eq!(held.len(), 1, "one sky segment");
        let segment = held[0];
        let drawn = app.world().get::<GlobalTransform>(segment).unwrap();
        assert_eq!(drawn.translation(), moved.translation);
        let mut parts = app
            .world_mut()
            .query_filtered::<(&ChildOf, &GlobalTransform), With<Mesh3d>>();
        let still = parts
            .iter(app.world())
            .filter(|(c, _)| c.parent() == root)
            .map(|(_, t)| t.translation())
            .collect::<Vec<_>>();
        assert!(
            !still.is_empty() && still.iter().all(|t| *t == Vec3::ZERO),
            "{still:?}"
        );
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
        for t in leaves
            .textures
            .iter()
            .filter(|t| t.texture != shared.texture)
        {
            std::fs::write(
                tmp.path().join(&t.texture),
                crate::test_support::level::map_ktx2(t.role),
            )
            .unwrap();
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

    /// AS.13: the load reads each normal map's layout off its KTX2's header
    /// and leaves out by name a tfbmulti2 material whose normal map isn't
    /// DXT5 (the hub's felt is DXT1), with its segments, in the level's
    /// scene and in its props' models alike: one entry, its segments summed
    /// over all three, and the report naming it. The rest load drawn.
    #[test]
    fn the_load_reads_each_normal_maps_layout_and_leaves_a_dxt1_one_out() {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        // What the level draws before the felt joins it.
        let drawn_before = read_level(&source, CompressedImageFormats::BC)
            .unwrap()
            .drawn
            .segments
            .len();
        let path = tmp.path().join(source.ir.level_path(LEVEL, SCENE));
        let mut scene: guac_ir::level::Scene =
            guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let graph = &mut scene.scenes[0];
        let LevelMaterial::Multi2(ground) = graph.materials[0].clone() else {
            panic!("the ground");
        };
        let mut felt = ground.clone();
        felt.name = "felt".into();
        for t in felt.textures.iter_mut().filter(|t| !t.role.is_color()) {
            t.texture = "textures/felt_nrm.ktx2".into();
            t.image = "felt_nrm".into();
        }
        // Laid out as a colour map is: DXT1.
        std::fs::write(
            tmp.path().join("textures/felt_nrm.ktx2"),
            crate::test_support::level::map_ktx2(TextureRole::Layer1ColorMap),
        )
        .unwrap();
        let shape = MeshShape {
            uv_sets: 2,
            tangents: true,
            colours: false,
            skinned: false,
        };
        let felt_at = u16::try_from(graph.materials.len()).unwrap();
        graph.materials.push(LevelMaterial::Multi2(felt.clone()));
        graph.segments.push(segment(felt_at, shape, 10.0));
        std::fs::write(&path, guac_ir::to_ron(&scene).unwrap()).unwrap();
        // Two props, each drawing the felt beside the ground.
        let prop = |path: &str| guac_ir::level::Prop {
            path: path.into(),
            skeleton: None,
            materials: vec![
                LevelMaterial::Multi2(ground.clone()),
                LevelMaterial::Multi2(felt.clone()),
            ],
            segments: vec![segment(0, shape, 0.0), segment(1, shape, 1.0)],
            idle: None,
        };
        let placed = |object: u32, name: &str, model: &str| guac_ir::level::Placement {
            object,
            id: 0,
            name: name.into(),
            script: None,
            position: [START[0] + 2.0, START[1], START[2]],
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: Some(model.into()),
            solid: None,
            movement: None,
            waypoints: None,
            activity_state: 4,
            activation_range: 160.0,
        };
        write_props(
            tmp.path(),
            vec![prop("table_a.mdl"), prop("table_b.mdl")],
            vec![
                placed(20, "table_a", "table_a.mdl"),
                placed(21, "table_b", "table_b.mdl"),
            ],
        );

        let built = read_level(&source, CompressedImageFormats::BC).unwrap();
        assert_eq!(
            built
                .refused
                .iter()
                .map(|r| (r.material.as_str(), r.segments))
                .collect::<Vec<_>>(),
            [("felt", 3)],
            "once, the level's segment and each prop's"
        );
        assert_eq!(
            built.refused[0].why,
            "its normal map felt_nrm isn't DXT5 (vkFormat 133), the layout the shader reads one in"
        );
        assert!(built.drawn.materials.iter().all(|m| m.name != "felt"));
        assert_eq!(
            built.drawn.segments.len(),
            drawn_before,
            "the felt's segment left out"
        );
        for m in &built.walk.props.models {
            assert_eq!(
                m.drawn
                    .materials
                    .iter()
                    .map(|x| x.name.as_str())
                    .collect::<Vec<_>>(),
                [ground.name.as_str()],
                "{}",
                m.path
            );
            assert_eq!(m.drawn.segments.len(), 1, "{}", m.path);
        }

        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        assert_eq!(
            report(&app).refused,
            [RefusedMaterial {
                material: "felt".into(),
                why: built.refused[0].why.clone(),
                segments: 3,
            }]
        );
    }

    /// AG.24: a grass shell and a warp in the scene load drawn (their
    /// passes among the keys, the warp's ripple map through both views),
    /// tfbshadow's `player_shadow` left out, and none of them adds a solid:
    /// the level's bodies are its collision part's alone. They come after a
    /// frame's share of other materials, as chapter 1's do, so each is
    /// uploaded with its own effect, not the one at its place in the share.
    #[test]
    fn the_grass_and_the_warp_load_drawn_and_add_no_solid() {
        use guac_ir::level::{EffectMaterial, UnitTexture};
        use guacamole::character::effect::Effect;
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        let plain = read_level(&source, CompressedImageFormats::BC).unwrap();
        let path = tmp.path().join(source.ir.level_path(LEVEL, SCENE));
        let mut scene: guac_ir::level::Scene =
            guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let graph = &mut scene.scenes[0];
        let LevelMaterial::Multi2(ground) = graph.materials[0].clone() else {
            panic!("the ground");
        };
        let file = ground.textures[0].clone();
        let unit = |unit: i16, image: &str| UnitTexture {
            unit,
            texture: file.texture.clone(),
            image: image.into(),
            sampler: file.sampler,
        };
        let effect = |effect: u32, name: &str, textures: Vec<UnitTexture>| {
            LevelMaterial::Effect(EffectMaterial {
                name: name.into(),
                effect,
                pixel: None,
                vertex: None,
                textures,
                states: guac_ir::model::RenderStates {
                    blend: guac_ir::model::Blend::Alpha,
                    depth_write: true,
                    two_sided: true,
                    alpha_test: None,
                },
                scrolls: Vec::new(),
                tracks: Vec::new(),
                unread: Vec::new(),
            })
        };
        // A frame's share of the ground's material first (the review of
        // AG.24: an upload reading a share's effects off its own start drew
        // chapter 1's, which sit past the first share, on the wrong parts).
        for _ in 0..MATERIALS_PER_FRAME {
            graph.materials.push(LevelMaterial::Multi2(ground.clone()));
        }
        let first = graph.materials.len() as u16;
        graph.materials.push(effect(
            0xAA6,
            "grass_HIRES",
            vec![
                unit(1, "grass_hieght.png"),
                unit(2, "grass_random.dds"),
                unit(0, "rocks_top_grass.png"),
            ],
        ));
        graph.materials.push(effect(
            0xAA9,
            "Water_Warp",
            vec![unit(0, "ripple_warp.png")],
        ));
        graph.materials.push(effect(
            0xAA8,
            "player_shadow",
            vec![unit(0, "shadow_grey3.png")],
        ));
        let shape = MeshShape {
            uv_sets: 1,
            tangents: false,
            colours: true,
            skinned: false,
        };
        for k in 0..3 {
            graph
                .segments
                .push(segment(first + k, shape, 20.0 + f32::from(k)));
        }
        std::fs::write(&path, guac_ir::to_ron(&scene).unwrap()).unwrap();

        let built = read_level(&source, CompressedImageFormats::BC).unwrap();
        let n = built.drawn.materials.len();
        assert_eq!(
            n,
            plain.drawn.materials.len() + MATERIALS_PER_FRAME + 2,
            "the share, the grass and the warp drawn"
        );
        let effects: Vec<Effect> = (0..n).filter_map(|i| built.drawn.effect(i)).collect();
        assert_eq!(effects, [Effect::Grass { shell: 0.0 }, Effect::Warp]);
        assert_eq!(
            built.drawn.left_out_materials,
            plain.drawn.left_out_materials + 1,
            "tfbshadow's left out"
        );
        let passes: Vec<guacamole::character::Pass> =
            built.keys.iter().map(|(k, _)| k.material.pass).collect();
        assert!(passes.contains(&guacamole::character::Pass::Grass));
        assert!(passes.contains(&guacamole::character::Pass::Warp));
        // The warp's one file through the colour map's view and the ripple's.
        let views: Vec<bool> = built
            .images
            .iter()
            .filter(|(k, _)| k.0 == file.texture)
            .map(|(k, _)| k.1)
            .collect();
        assert!(views.contains(&true) && views.contains(&false), "{views:?}");
        // No solid of theirs: the bodies are the collision part's.
        assert_eq!(built.walk.bodies.len(), plain.walk.bodies.len());
        assert_eq!(built.walk.triangles, plain.walk.triangles);

        // Uploaded: each drawn in its pass with its numbers, the keys the
        // warm was asked for, and a fog's relight keeping the numbers.
        let mut app = level_app(tmp.path(), source, false);
        until(&mut app, &["ready"], |_| {});
        app.update();
        let uploaded = |app: &mut App| -> Vec<(guacamole::character::Pass, Vec4)> {
            let world = app.world_mut();
            let mut q = world.query::<(&MeshMaterial3d<CharacterMaterial>, &RenderLayers)>();
            let materials = world.resource::<Assets<CharacterMaterial>>();
            let mut out: Vec<_> = q
                .iter(world)
                .filter(|(_, l)| **l == RenderLayers::layer(LEVEL_LAYER))
                .map(|(m, _)| materials.get(&m.0).unwrap())
                .filter(|m| m.key.pass != guacamole::character::Pass::Colour)
                .map(|m| (m.key.pass, m.uniform.effect))
                .collect();
            out.sort_by_key(|(p, _)| *p as u8);
            out
        };
        let want = [
            (
                guacamole::character::Pass::Grass,
                Effect::Grass { shell: 0.0 }.uniform(),
            ),
            (guacamole::character::Pass::Warp, Effect::Warp.uniform()),
        ];
        let mut want = want.to_vec();
        want.sort_by_key(|(p, _)| *p as u8);
        assert_eq!(uploaded(&mut app), want);
        let asked: HashSet<WarmKey> = app
            .world()
            .resource::<Warm>()
            .level_listed()
            .iter()
            .copied()
            .collect();
        assert!(
            asked
                .iter()
                .any(|k| k.material.pass == guacamole::character::Pass::Grass)
                && asked
                    .iter()
                    .any(|k| k.material.pass == guacamole::character::Pass::Warp),
            "{asked:?}"
        );
        app.insert_resource(LevelFog {
            colour: [90, 60, 30],
            near: 30.0,
            far: 300.0,
        });
        app.update();
        assert_eq!(app.world().resource::<Level>().report().relit, 1);
        assert_eq!(uploaded(&mut app), want, "kept through the relight");
    }

    /// AG.24: a prop's or a clone's warp (the level-up's ring) keeps its
    /// numbers through a fog's relight too.
    #[test]
    fn a_relit_clone_keeps_its_effects_numbers() {
        use guacamole::character::effect::Effect;
        let mut assets = Assets::<CharacterMaterial>::default();
        let reference = guac_ir::shading::Material::new();
        let mut ring = CharacterMaterial::new(
            guacamole::character::MaterialKey {
                shader: guacamole::character::ShaderKey::new(0, 0).unwrap(),
                states: guacamole::character::StateKey::OPAQUE,
                pass: guacamole::character::Pass::Colour,
            },
            &reference,
            &Scene::new(),
            0.0,
            guacamole::character::CharacterTextures {
                colour: Some(Handle::default()),
                ..Default::default()
            },
        )
        .unwrap();
        Effect::Warp.apply(&mut ring);
        let material = assets.add(ring);
        relight(
            &[LitAlso {
                material: material.clone(),
                reference,
                alpha: 0.0,
            }],
            &Scene::new(),
            &mut assets,
        );
        let m = assets.get(&material).unwrap();
        assert_eq!(m.uniform.effect, Effect::Warp.uniform());
        assert_eq!(m.key.pass, guacamole::character::Pass::Warp);
    }

    /// AR.21's tests' run: one unit, chapter 1's script part, with an actor
    /// and the file object of a sound on it.
    struct Landing {
        program: Arc<guac_script::Program>,
        files: guacamole::scripts::Files,
        actor: guac_script::Handle,
        sound: Obj,
        /// A placement running the level's music controller (AR.23).
        music: guac_script::Handle,
    }

    const LANDING_SOUND: u32 = 10;
    const LANDING_FILE: &str = "audio/sounds/0011223344556677.mp3";

    fn landing() -> Landing {
        use guacamole::scripts::{Files, Placed, headless_handles};
        let level = LEVELS[0];
        let unit = format!("{}/{level}/script.igz", guac_extract::levels::LEVEL_DIR);
        let program = Arc::new({
            let mut p = guac_script::Program::default();
            p.units = vec![unit];
            p
        });
        let actor_obj = Obj {
            unit: 0,
            index: 500,
        };
        let music_obj = Obj {
            unit: 0,
            index: 501,
        };
        let files = Files {
            placed: vec![
                Placed::named(actor_obj, "thrower", None, 4),
                Placed::named(
                    music_obj,
                    "dynamic_sound_controller",
                    Some("C:/tfb/Content/Levels/Level_213/Scripts/Music_Level_Controller.ai"),
                    4,
                ),
            ],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        Landing {
            program,
            actor: handles[&actor_obj],
            music: handles[&music_obj],
            files,
            sound: Obj {
                unit: 0,
                index: LANDING_SOUND,
            },
        }
    }

    impl Landing {
        /// The run's VM as an install leaves it (AR.23): the music
        /// controller's placement in it, held while the book lacks the
        /// level's sounds.
        fn runtime(&self, world: &mut ScriptWorld) -> guacamole::scripts::ScriptRuntime {
            let mut vm = guac_script::Vm::new(
                self.program.clone(),
                guac_script::Config::new(guac_script::Mode::Play),
            );
            vm.add(self.music, "dynamic_sound_controller", 0);
            guacamole::scripts::hold_music(&mut vm, world, &self.files, 0);
            guacamole::scripts::ScriptRuntime {
                vm,
                generator: guac_script::Generator::seeded(1),
                player_loop: Default::default(),
                steps: 0,
                stopped: Vec::new(),
                entry_stopped: None,
            }
        }

        fn music_held(&self, app: &App) -> bool {
            let vm = &app
                .world()
                .resource::<guacamole::scripts::ScriptRuntime>()
                .vm;
            vm.is_suspended(vm.placement_of(self.music).unwrap())
        }
    }

    impl Landing {
        /// A fresh run over it, with `book`'s sounds, as an install makes.
        fn world(&self, sounds: &guacamole::sounds::SoundPart) -> ScriptWorld {
            use guacamole::scripts::{headless_handles, headless_objects};
            let handles = headless_handles(&self.files);
            let mut world = ScriptWorld::new(&self.files, &handles, headless_objects(&handles));
            world.set_sounds(sounds.book.clone(), sounds.stems.clone());
            world
        }

        fn source(&self) -> crate::hero_scripts::ScriptSource {
            crate::hero_scripts::ScriptSource::of(self.program.clone(), self.files.clone(), vec![0])
        }

        /// How many plays a start of its sound makes on the run now.
        fn plays(&self, app: &mut App) -> usize {
            let mut w = app.world_mut().resource_mut::<ScriptWorld>();
            let before = w.plays().len();
            w.play_sound_on(self.sound, self.actor);
            w.plays().len() - before
        }
    }

    /// The sounds part landing in `dir`: one archive, chapter 1's, a sound
    /// with its recording.
    fn land_sounds(dir: &Path) {
        use guac_ir::sound::{Audio, DataHome, DataRef, Sound, SoundData, Sounds};
        const ONCE: u32 = LANDING_SOUND;
        let level = LEVELS[0];
        let file = LANDING_FILE;
        let part = Sounds {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([0; 32]),
            },
            archive: format!("{}/{level}.bld", guac_extract::levels::LEVEL_FOLDER),
            sounds: vec![Sound {
                object: ONCE,
                name: Some("snd_once".into()),
                data: DataRef::Here(DataHome::Level, 90),
                flags: 0x4000_0028,
                properties: 0,
                playback_mode: 0,
                playback_percent: 100.0,
                volume_percent: 50.0,
                volume_randomness_percent: 0.0,
                pitch_randomness_percent: 0.0,
                pan_percent: 0.0,
                attenuation_distance_full: 1.0,
                attenuation_distance_silent: 101.0,
                rumble_duration: 1.0,
                reverb_amount: 0.0,
                reverb_randomness_percent: 0.0,
                filter_percent: 0.0,
                filter_randomness_percent: 0.0,
                filter_attenuation_distance_minimum: 1.0,
                filter_attenuation_distance_maximum: 200.0,
                filter_maximum_percent: 100.0,
                sample_rate_reduction: 0.0,
            }],
            data: vec![SoundData {
                home: DataHome::Level,
                object: 90,
                name: None,
                raw_duration: 1.0,
                streamed: None,
                lip_sync: false,
                audio: Some(Audio {
                    file: file.into(),
                    rate: 48_000,
                    channels: 1,
                    samples: 48_000,
                    loop_points: None,
                }),
            }],
            refused: Vec::new(),
        };
        let rel = guac_extract::sounds::path(IrFormat::Ron, &part.archive);
        std::fs::create_dir_all(dir.join(&rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(&rel), guac_ir::to_ron(&part).unwrap()).unwrap();
    }

    /// AR.21's live run: a sounds part landing after the level's load is
    /// read again into the run once its news names the level's extract, its
    /// book replaced on the run and kept with the part for a reinstall; a
    /// start before it plays nothing, one after finds its recording; the
    /// level's music, held until then, let go (AR.23). News of
    /// another extract changes nothing, and a read for a run replaced before
    /// it lands is dropped.
    #[test]
    fn the_sounds_landing_after_the_load_are_read_again_into_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let landing = landing();
        let Landing { program, files, .. } = &landing;
        // As the load found it: no sounds part yet.
        let empty = guacamole::sounds::load(&dir, IrFormat::Ron, &program.units);
        assert!(empty.stems.is_empty() && empty.missing.len() == 1);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let (tx, feed) = sounds_channel();
        let mut world = landing.world(&empty);
        let runtime = landing.runtime(&mut world);
        app.insert_resource(Level::from_extract_for_tests(dir.clone()))
            .insert_resource(landing.source())
            .insert_resource(world)
            .insert_resource(runtime)
            .insert_resource(feed)
            .insert_resource(sounds_stage_running())
            .add_systems(Update, (receive_sounds, sounds_again).chain());
        let plays = |app: &mut App| landing.plays(app);
        assert_eq!(plays(&mut app), 0, "no book holds it yet");
        app.update();
        assert!(landing.music_held(&app), "the music waits (AR.23)");

        // The part lands.
        land_sounds(&dir);
        let file = LANDING_FILE;
        let taken_up = |app: &App| {
            app.world()
                .resource::<Level>()
                .report()
                .sounds_again_archives
        };
        // Another extract's news is none of this level's.
        tx.send(SoundsReady {
            dir: dir.join("other"),
        })
        .unwrap();
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(taken_up(&app), None);
        assert_eq!(plays(&mut app), 0);

        // A read whose run is replaced before it lands is dropped.
        tx.send(SoundsReady { dir: dir.clone() }).unwrap();
        app.update();
        assert!(app.world().resource::<Level>().sounds_again.is_some());
        let replaced = Arc::new({
            let mut p = guac_script::Program::default();
            p.units = program.units.clone();
            p
        });
        app.insert_resource(crate::hero_scripts::ScriptSource::of(
            replaced,
            files.clone(),
            vec![0],
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Level>().sounds_again.is_some() {
            assert!(Instant::now() < deadline, "the read never landed");
            app.update();
        }
        assert_eq!(taken_up(&app), None);
        assert_eq!(plays(&mut app), 0, "the replaced run's read dropped");
        assert!(landing.music_held(&app), "still waiting");

        // Its news again while the level's scripts still link: held until
        // they're in, then read into the run as it stands.
        app.insert_resource(crate::hero_scripts::ScriptSource::of(
            program.clone(),
            files.clone(),
            vec![0],
        ));
        app.world_mut().resource_mut::<Level>().scripts =
            Some(AsyncComputeTaskPool::get().spawn(std::future::pending()));
        // The news is sent before the extraction moves past the stage.
        tx.send(SoundsReady { dir: dir.clone() }).unwrap();
        app.world_mut().resource_mut::<Extraction>().status =
            crate::extraction::ExtractStatus::Interface { done: 0, total: 85 };
        for _ in 0..20 {
            app.update();
        }
        let l = app.world().resource::<Level>();
        assert!(
            l.sounds_landed && l.sounds_again.is_none(),
            "held for the link"
        );
        assert_eq!(taken_up(&app), None);
        app.world_mut().resource_mut::<Level>().scripts = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while taken_up(&app).is_none() {
            assert!(Instant::now() < deadline, "never read again");
            app.update();
        }
        assert_eq!(taken_up(&app), Some(1));
        let w = app.world().resource::<ScriptWorld>();
        assert_eq!(w.sound_book().unwrap().stems().count(), 1);
        let kept = app
            .world()
            .resource::<crate::hero_scripts::ScriptSource>()
            .sounds()
            .map(|s| s.stems.len());
        assert_eq!(kept, Some(1), "kept with the part for a reinstall");
        assert_eq!(plays(&mut app), 1, "a start after it plays");
        let w = app.world().resource::<ScriptWorld>();
        assert_eq!(w.plays().last().unwrap().file, file);
        assert!(!landing.music_held(&app), "the music starts with them");
        assert_eq!(
            app.world().resource::<ScriptWorld>().music_silent(),
            0,
            "with its sounds, not silent"
        );
    }

    /// An extraction short of its sounds stage's end: the level's sounds on
    /// their way (openspec AR.30).
    fn sounds_stage_running() -> Extraction {
        Extraction {
            status: crate::extraction::ExtractStatus::Sounds { done: 3, total: 69 },
            ..Default::default()
        }
    }

    /// AR.30's review: the sounds are on their way through every stage up to
    /// and through the sounds stage, whichever one chapter 1's hand-off
    /// lands in (a first run hands it over before its sprites, AT.26), and
    /// never past it, nor with no pick or a failed one.
    #[test]
    fn the_sounds_are_coming_until_the_sounds_stage_ends() {
        use crate::extraction::{ExtractStatus as S, Failure};
        let coming = |status: S| {
            sounds_coming(Some(&Extraction {
                status,
                ..Default::default()
            }))
        };
        let n = || (0, 1);
        for status in [
            S::Starting,
            S::Checking {
                bytes_done: 0,
                bytes_total: 1,
            },
            S::Roster {
                done: n().0,
                total: n().1,
            },
            S::Models {
                done: n().0,
                total: n().1,
            },
            S::Level {
                done: n().0,
                total: n().1,
            },
            S::Scripts {
                done: n().0,
                total: n().1,
            },
            S::Particles {
                done: n().0,
                total: n().1,
            },
            S::Sprites {
                done: n().0,
                total: n().1,
            },
            S::Sounds {
                done: n().0,
                total: n().1,
            },
        ] {
            assert!(coming(status.clone()), "{status:?}");
        }
        for status in [
            S::Idle,
            S::Interface {
                done: n().0,
                total: n().1,
            },
            S::Indexing,
            S::Converting {
                done: 0,
                total: 1,
                movie: "intro".into(),
            },
            S::Ready {
                intros: 0,
                ran: true,
            },
            S::Failed(Failure::Other {
                code: "GUAC-E-EXTRACT-IO".into(),
                message: "gone".into(),
            }),
        ] {
            assert!(!coming(status.clone()), "{status:?}");
        }
        assert!(!sounds_coming(None), "no pick");
    }

    /// openspec AR.30: the level's music, held while its sounds were on
    /// their way, is let go to run silent once the extraction moves past its
    /// sounds stage without news of them (the stage failed), never held for
    /// good; nothing plays, and nothing is held again.
    #[test]
    fn music_held_for_sounds_that_dont_come_runs_silent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let landing = landing();
        let empty = guacamole::sounds::load(&dir, IrFormat::Ron, &landing.program.units);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let (_tx, feed) = sounds_channel();
        let mut world = landing.world(&empty);
        let runtime = landing.runtime(&mut world);
        app.insert_resource(Level::from_extract_for_tests(dir.clone()))
            .insert_resource(landing.source())
            .insert_resource(world)
            .insert_resource(runtime)
            .insert_resource(feed)
            .insert_resource(sounds_stage_running())
            .add_systems(Update, (receive_sounds, sounds_again).chain());
        for _ in 0..5 {
            app.update();
        }
        assert!(landing.music_held(&app), "held while they're coming");
        app.world_mut().resource_mut::<Extraction>().status =
            crate::extraction::ExtractStatus::Interface { done: 0, total: 85 };
        app.update();
        assert!(!landing.music_held(&app), "let go once they aren't");
        assert_eq!(app.world().resource::<ScriptWorld>().music_held(), 0);
        assert_eq!(app.world().resource::<ScriptWorld>().music_silent(), 1);
        for _ in 0..5 {
            app.update();
        }
        assert!(!landing.music_held(&app));
        assert_eq!(landing.plays(&mut app), 0, "silent");
    }

    /// AR.21's other orders: news that comes before any level is asked for
    /// waits for nothing (the load reads the part whole); a read that lands
    /// after the run was installed again (the level left, `/api/quit`) goes
    /// to the run installed, and a later install takes the part it kept; the
    /// same news twice reads it twice, to the same book.
    #[test]
    fn the_sounds_landing_reach_the_run_as_installed_and_wait_for_no_level() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let landing = landing();
        let empty = guacamole::sounds::load(&dir, IrFormat::Ron, &landing.program.units);
        land_sounds(&dir);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let (tx, feed) = sounds_channel();
        app.insert_resource(Level::new(false))
            .insert_resource(landing.source())
            .insert_resource(landing.world(&empty))
            .insert_resource(feed)
            .add_systems(Update, (receive_sounds, sounds_again).chain());
        let level = |app: &App| {
            let l = app.world().resource::<Level>();
            (
                l.sounds_landed,
                l.sounds_again.is_some(),
                l.report.sounds_again_archives,
            )
        };
        // No level asked for yet: the news is no one's.
        tx.send(SoundsReady { dir: dir.clone() }).unwrap();
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(level(&app), (false, false, None));
        // Then chapter 1 asked for off that extract, as `receive_level` sets
        // it on the same level.
        {
            let mut l = app.world_mut().resource_mut::<Level>();
            l.source = Level::from_extract_for_tests(dir.clone()).source;
            l.since = Some(Instant::now());
        }
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(level(&app), (false, false, None), "nothing kept for it");
        assert_eq!(landing.plays(&mut app), 0);

        // The news for it, the run installed again while the read is out:
        // the read lands on the run as it now stands.
        tx.send(SoundsReady { dir: dir.clone() }).unwrap();
        app.update();
        assert_eq!(level(&app), (false, true, None));
        crate::hero_scripts::reinstall(app.world_mut());
        let run = app.world().resource::<ScriptWorld>().run();
        let deadline = Instant::now() + Duration::from_secs(5);
        while level(&app).2.is_none() {
            assert!(Instant::now() < deadline, "never read again");
            app.update();
        }
        let w = app.world().resource::<ScriptWorld>();
        assert_eq!(w.run(), run, "no install of its own");
        assert_eq!(w.sound_book().unwrap().stems().count(), 1);
        assert_eq!(landing.plays(&mut app), 1);

        // Installed again after it: the part kept on the source.
        crate::hero_scripts::reinstall(app.world_mut());
        let w = app.world().resource::<ScriptWorld>();
        assert_ne!(w.run(), run);
        assert_eq!(w.sound_book().unwrap().stems().count(), 1);
        assert_eq!(landing.plays(&mut app), 1, "the install's run plays it");

        // The same news again: read again, to the same book.
        app.world_mut()
            .resource_mut::<Level>()
            .report
            .sounds_again_archives = None;
        tx.send(SoundsReady { dir: dir.clone() }).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while level(&app).2.is_none() {
            assert!(Instant::now() < deadline, "the second news never read");
            app.update();
        }
        assert_eq!(level(&app), (false, false, Some(1)));
        assert_eq!(landing.plays(&mut app), 1);
        let w = app.world().resource::<ScriptWorld>();
        assert_eq!(w.plays().last().unwrap().file, LANDING_FILE);
    }
}
