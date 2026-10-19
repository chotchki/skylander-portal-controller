//! The pre-warm (openspec AE.14, design decision 15; the figure-turntable
//! spec's "the turntable never stalls the TV"). macOS compiles a render
//! pipeline inside the frame that first draws with it, so a model whose
//! material keys nothing has drawn yet would stall the TV the frame it came
//! up. So:
//!
//! - When a shelf lands (a current extract at start or on the pick, or an
//!   extraction's models once they're in), every model on it is read on a
//!   task for the keys it draws ([`warm::keys`]): the warm list.
//! - A hidden turntable, built as a card's is (the remaster's camera into
//!   an image, its glow camera, a render-layer pair past the eight slots'),
//!   draws one triangle per key not drawn before, one key a frame, so no
//!   frame compiles more than one key's colour and glow pipelines. The
//!   render world says when each triangle's pipeline is compiled
//!   ([`watch_pipelines`]); once every key is, the hidden turntable goes.
//! - A card's turntable whose model is read waits, on its portrait, until
//!   every key it draws is compiled ([`Warm::covers`]); a key the warm
//!   hasn't reached goes to the front of its queue ([`Warm::ask`]). So a
//!   model never comes up on a pipeline that isn't compiled, whether it's
//!   placed before the warm is through or a key escaped the list.
//!
//! Without a renderer (the headless tests) nothing compiles: the list is
//! still read, and every key counts as compiled.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::pbr::SpecializedMaterialPipelineCache;
use bevy::prelude::*;
use bevy::render::render_resource::{
    CachedPipelineState, Extent3d, PipelineCache, PipelineDescriptor, TextureDimension,
    TextureFormat,
};
use bevy::render::sync_world::MainEntity;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderCacheError;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use guac_ir::DumpIdentity;
use guac_ir::model::{self, Model};
use guac_ir::shading::Scene;
use guacamole::character::turntable::{Framing, Sweep, rig_scene};
use guacamole::character::warm::{self, WarmKey};
use guacamole::character::{CharacterMaterial, ShadowLookup, spawn_character, spawn_glow_masks};
use serde::Serialize;
use tracing::{debug, info, warn};

use crate::turntable::{Shelf, WARM_SLOT, glow_layer, model_layer, read_model, spawn_camera};

/// The hidden turntable's image: its size changes no pipeline.
const STAGE_SIZE: u32 = 64;

/// How long a drawn key may go unconfirmed before the warm stops waiting
/// for it (and a model waiting on it comes up anyway).
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);

/// A key and the material that first draws it.
#[derive(Debug, Clone)]
struct Entry {
    key: WarmKey,
    material: model::Material,
}

/// What a shelf's models draw.
struct List {
    entries: Vec<Entry>,
    models: usize,
    /// Load files whose model didn't read, with why.
    unreadable: Vec<(String, String)>,
    took: Duration,
}

/// The warm list's keys, read off `shelf`'s models in load-file order.
fn read_list(shelf: &Shelf, scene: &Scene) -> List {
    let started = Instant::now();
    let mut loads: Vec<&String> = shelf
        .loads
        .values()
        .filter(|l| !shelf.failed.contains_key(*l))
        .collect();
    loads.sort();
    loads.dedup();
    let mut list = List {
        entries: Vec::new(),
        models: 0,
        unreadable: Vec::new(),
        took: Duration::ZERO,
    };
    let mut seen = HashSet::new();
    for load in loads {
        let read = read_model(shelf, load).and_then(|model| {
            warm::keys(&model, scene)
                .map(|keys| (model, keys))
                .map_err(|e| e.to_string())
        });
        match read {
            Ok((model, keys)) => {
                list.models += 1;
                for (key, material) in keys {
                    if seen.insert(key) {
                        list.entries.push(Entry {
                            key,
                            material: model.materials[material].clone(),
                        });
                    }
                }
            }
            Err(why) => list.unreadable.push((load.clone(), why)),
        }
    }
    list.took = started.elapsed();
    list
}

/// The render world's word on a drawn triangle's pipeline.
#[derive(Debug, Clone, PartialEq)]
enum Compiled {
    Waiting,
    Ready,
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Watched {
    /// Mesh entities whose pipeline the render world looks up.
    checks: HashMap<Entity, Compiled>,
    /// Render pipelines compiled, as of the render world's last frame.
    ready: usize,
}

/// Shared by the two worlds: what the warm asks the render world to look
/// up, its answers and how many pipelines are compiled.
#[derive(Resource, Clone, Default)]
pub(crate) struct PipelineWatch(Arc<Mutex<Watched>>);

impl PipelineWatch {
    fn lock(&self) -> MutexGuard<'_, Watched> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Render pipelines compiled so far (all of them, not only the warm's),
    /// as of the render world's last frame.
    pub(crate) fn ready(&self) -> usize {
        self.lock().ready
    }
}

/// Render world, after the frame's draws: each watched mesh's pipeline,
/// looked up in every view it's specialized for, and the count of compiled
/// pipelines, each new one logged (debug) by its label and fragment shader
/// so a long frame can be put down to what compiled in it.
fn watch_pipelines(
    watch: Res<PipelineWatch>,
    cache: Res<PipelineCache>,
    specialized: Option<Res<SpecializedMaterialPipelineCache>>,
    mut compiled: Local<Vec<bool>>,
) {
    let mut ready = 0;
    for (i, p) in cache.pipelines().enumerate() {
        let ok = matches!(p.state, CachedPipelineState::Ok(_));
        ready += usize::from(ok);
        if compiled.len() <= i {
            compiled.resize(i + 1, false);
        }
        if ok && !compiled[i] {
            compiled[i] = true;
            if let PipelineDescriptor::RenderPipelineDescriptor(d) = &p.descriptor {
                let fragment = d.fragment.as_ref();
                debug!(
                    id = i,
                    label = d.label.as_deref().unwrap_or(""),
                    shader = %fragment
                        .and_then(|f| f.shader.path())
                        .map(|p| p.to_string())
                        .unwrap_or_default(),
                    entry = fragment.and_then(|f| f.entry_point.as_deref()).unwrap_or(""),
                    defs = fragment.map_or(0, |f| f.shader_defs.len()),
                    "pipeline compiled"
                );
            }
        }
    }
    let mut watched = watch.lock();
    watched.ready = ready;
    let Some(specialized) = specialized else {
        return;
    };
    for (mesh, state) in watched.checks.iter_mut() {
        if *state != Compiled::Waiting {
            continue;
        }
        let main = MainEntity::from(*mesh);
        let Some(id) = specialized
            .values()
            .find_map(|view| view.get(&main).copied())
        else {
            continue;
        };
        *state = match cache.get_render_pipeline_state(id) {
            CachedPipelineState::Ok(_) => Compiled::Ready,
            // Retried by the cache until the shader's in.
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => continue,
            CachedPipelineState::Err(e) => Compiled::Failed(e.to_string()),
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => continue,
        };
    }
}

/// A key drawn on the hidden turntable, waiting for the render world.
struct Drawing {
    key: WarmKey,
    /// Its colour segment and, unless the glow pass leaves it out, its
    /// glow mask.
    checks: Vec<Entity>,
    since: Instant,
}

/// The hidden turntable: its camera (the glow camera its child), the
/// texture every slot binds, a root per key drawn and the keys the render
/// world hasn't confirmed yet.
struct Stage {
    camera: Entity,
    /// A texture's pixels and format change no pipeline.
    texture: Handle<Image>,
    roots: Vec<Entity>,
    drawing: Vec<Drawing>,
}

/// What `/api/_test/tv` reports of the warm.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct WarmReport {
    /// `idle` (no shelf yet), `reading` (the list), `compiling`, `done`.
    pub state: String,
    /// Models read for the list, and those that wouldn't read.
    pub models: usize,
    pub unreadable: usize,
    /// Every key known: compiled, failed or still to draw.
    pub keys: usize,
    pub compiled: usize,
    pub failed: usize,
    /// From the shelf landing (or the first key asked) to the last key
    /// compiled so far.
    pub took_ms: Option<u64>,
}

#[derive(Resource)]
pub(crate) struct Warm {
    /// A renderer to compile on; without one every key counts as compiled.
    gpu: bool,
    /// The extract the last list came off, so the same one isn't read twice.
    from: Option<(PathBuf, DumpIdentity)>,
    reading: Option<Task<List>>,
    /// The scene the cards light their models in, and the dump.
    scene: Option<(Scene, DumpIdentity)>,
    queue: VecDeque<Entry>,
    /// Queued or drawing.
    pending: HashSet<WarmKey>,
    /// Compiled, failed or given up on: what a turntable waits for.
    done: HashSet<WarmKey>,
    stage: Option<Stage>,
    started: Option<Instant>,
    report: WarmReport,
    /// The last list's keys, in its order.
    listed: Vec<WarmKey>,
}

impl Warm {
    pub(crate) fn new(gpu: bool) -> Self {
        Self {
            gpu,
            from: None,
            reading: None,
            scene: None,
            queue: VecDeque::new(),
            pending: HashSet::new(),
            done: HashSet::new(),
            stage: None,
            started: None,
            listed: Vec::new(),
            report: WarmReport {
                state: "idle".into(),
                ..Default::default()
            },
        }
    }

    /// Reads `shelf`'s warm list on a task, unless it's the extract the
    /// last list came off. A shelf without a rig shows no models.
    pub(crate) fn start(&mut self, shelf: &Arc<Shelf>) {
        let Ok(rig) = &shelf.rig else {
            return;
        };
        let scene = match rig_scene(rig) {
            Ok(scene) => scene,
            Err(e) => {
                warn!("the warm has no scene: {e}");
                return;
            }
        };
        let from = (shelf.dir.clone(), shelf.identity.clone());
        if self.from.as_ref() == Some(&from) {
            return;
        }
        self.from = Some(from);
        self.scene = Some((scene.clone(), shelf.identity.clone()));
        self.started = Some(Instant::now());
        let shelf = shelf.clone();
        self.reading =
            Some(AsyncComputeTaskPool::get().spawn(async move { read_list(&shelf, &scene) }));
        self.update_report();
    }

    /// Whether every one of `keys` is compiled (or failed, or given up on).
    pub(crate) fn covers<'a>(&self, keys: impl IntoIterator<Item = &'a WarmKey>) -> bool {
        !self.gpu || keys.into_iter().all(|k| self.done.contains(k))
    }

    /// Puts `keys` (with `model`'s material index that draws each) ahead
    /// of everything else on the queue: a turntable is waiting on them.
    pub(crate) fn ask(&mut self, model: &Model, keys: &[(WarmKey, usize)], scene: &Scene) {
        if self.scene.is_none() {
            self.scene = Some((scene.clone(), model.identity.clone()));
        }
        self.started.get_or_insert_with(Instant::now);
        for (key, material) in keys.iter().rev() {
            if self.done.contains(key) {
                continue;
            }
            let entry = match self.queue.iter().position(|e| e.key == *key) {
                Some(at) => self.queue.remove(at),
                // Drawing already.
                None if self.pending.contains(key) => None,
                None => Some(Entry {
                    key: *key,
                    material: model.materials[*material].clone(),
                }),
            };
            if let Some(entry) = entry {
                self.pending.insert(entry.key);
                self.queue.push_front(entry);
            }
        }
        self.update_report();
    }

    pub(crate) fn report(&self) -> &WarmReport {
        &self.report
    }

    fn finish(&mut self, key: WarmKey) {
        self.pending.remove(&key);
        self.done.insert(key);
    }

    fn update_report(&mut self) {
        let r = &mut self.report;
        r.keys = self.done.len() + self.pending.len();
        r.state = if self.reading.is_some() {
            "reading"
        } else if !self.queue.is_empty() || self.stage.is_some() {
            "compiling"
        } else if self.from.is_some() || !self.done.is_empty() {
            "done"
        } else {
            "idle"
        }
        .into();
    }

    /// The last list's keys, in its order.
    #[cfg(test)]
    pub(crate) fn listed(&self) -> &[WarmKey] {
        &self.listed
    }
}

/// Adds the warm to the turntables: [`Warm`] in the app, the watch in both
/// worlds. [`step`] runs in the turntables' chain.
pub(crate) fn build(app: &mut App) {
    let watch = PipelineWatch::default();
    let gpu = app.get_sub_app(RenderApp).is_some();
    app.insert_resource(Warm::new(gpu))
        .insert_resource(watch.clone());
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .insert_resource(watch)
            .add_systems(Render, watch_pipelines.in_set(RenderSystems::Cleanup));
    }
}

/// What [`step`] spawns with.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct WarmSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    materials: ResMut<'w, Assets<CharacterMaterial>>,
    watch: Option<Res<'w, PipelineWatch>>,
}

/// Each frame: the list once read goes on the queue; what the render world
/// confirmed comes off the stage; the next key goes on it (the stage's
/// camera alone the first frame); the stage goes when nothing's left.
pub(crate) fn step(mut warm: ResMut<Warm>, mut s: WarmSpawner) {
    let warm = &mut *warm;
    if let Some(list) = warm.reading.as_mut().and_then(check_ready) {
        warm.reading = None;
        warm.listed = list.entries.iter().map(|e| e.key).collect();
        let mut new = 0;
        for entry in list.entries.iter() {
            if !warm.done.contains(&entry.key) && warm.pending.insert(entry.key) {
                warm.queue.push_back(entry.clone());
                new += 1;
            }
        }
        warm.report.models += list.models;
        warm.report.unreadable += list.unreadable.len();
        info!(
            models = list.models,
            unreadable = list.unreadable.len(),
            keys = warm.listed.len(),
            new,
            read_ms = list.took.as_millis() as u64,
            "the warm list read"
        );
        for (load, why) in &list.unreadable {
            info!(load_file = %load, reason = %why, "the warm skips a model that won't read");
        }
    }
    if !warm.gpu {
        while let Some(entry) = warm.queue.pop_front() {
            warm.finish(entry.key);
        }
        warm.update_report();
        return;
    }
    let Some(watch) = s.watch.as_deref().cloned() else {
        return;
    };
    confirm(warm, &watch);
    if !warm.queue.is_empty() {
        match warm.stage.as_mut() {
            None => warm.stage = spawn_stage(&mut s),
            Some(_) => {
                if let Some(entry) = warm.queue.pop_front() {
                    draw(warm, entry, &mut s, &watch);
                }
            }
        }
    } else if warm.stage.as_ref().is_some_and(|st| st.drawing.is_empty()) {
        let Some(stage) = warm.stage.take() else {
            return;
        };
        s.commands.entity(stage.camera).despawn();
        for root in stage.roots {
            s.commands.entity(root).despawn();
        }
        warm.report.took_ms = warm.started.map(|t| t.elapsed().as_millis() as u64);
        info!(
            keys = warm.done.len(),
            compiled = warm.report.compiled,
            failed = warm.report.failed,
            took_ms = warm.report.took_ms,
            "the warm compiled every key the models draw"
        );
    }
    warm.update_report();
}

/// Takes what the render world confirmed off the stage.
fn confirm(warm: &mut Warm, watch: &PipelineWatch) {
    let Some(stage) = warm.stage.as_mut() else {
        return;
    };
    let mut watched = watch.lock();
    let mut finished = Vec::new();
    stage.drawing.retain(|d| {
        let found: Vec<Option<Compiled>> = d
            .checks
            .iter()
            .map(|e| watched.checks.get(e).cloned())
            .collect();
        let failed = found.iter().flatten().find_map(|c| match c {
            Compiled::Failed(why) => Some(why.clone()),
            _ => None,
        });
        let ready = found.iter().all(|c| c == &Some(Compiled::Ready));
        let late = d.since.elapsed() > CONFIRM_TIMEOUT;
        if !(ready || failed.is_some() || late) {
            return true;
        }
        for e in &d.checks {
            watched.checks.remove(e);
        }
        finished.push((d.key, failed, late && !ready));
        false
    });
    drop(watched);
    for (key, failed, late) in finished {
        match (failed, late) {
            (Some(why), _) => {
                warn!(?key, %why, "a warm pipeline failed to compile");
                warm.report.failed += 1;
            }
            (None, true) => {
                warn!(
                    ?key,
                    "a warm pipeline wasn't confirmed in time; going on without it"
                );
                warm.report.failed += 1;
            }
            (None, false) => warm.report.compiled += 1,
        }
        warm.finish(key);
    }
}

/// The hidden turntable's camera, framing a unit triangle at the origin.
fn spawn_stage(s: &mut WarmSpawner) -> Option<Stage> {
    let framing = Framing::fit(&Sweep {
        radius: 0.5,
        bottom: -0.5,
        top: 0.5,
    });
    let (camera, _) = spawn_camera(
        &mut s.commands,
        &mut s.images,
        WARM_SLOT,
        UVec2::splat(STAGE_SIZE),
        &framing,
        Color::BLACK,
    );
    let texture = s.images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    Some(Stage {
        camera,
        texture,
        roots: Vec::new(),
        drawing: Vec::new(),
    })
}

/// One key on the stage: a triangle of its mesh shape with its material,
/// and its glow mask unless the glow pass leaves the part out, each
/// watched.
fn draw(warm: &mut Warm, entry: Entry, s: &mut WarmSpawner, watch: &PipelineWatch) {
    let (Some(stage), Some((scene, identity))) = (warm.stage.as_mut(), warm.scene.as_ref()) else {
        warm.finish(entry.key);
        return;
    };
    let model = warm::triangle_model(identity, &entry.material, entry.key.mesh);
    let texture = stage.texture.clone();
    let built = CharacterMaterial::from_ir(&model.materials[0], scene, ShadowLookup::NONE, |_| {
        texture.clone()
    })
    .map_err(|e| e.to_string())
    .and_then(|material| {
        if material.key != entry.key.material {
            warn!(drawn = ?material.key, listed = ?entry.key.material, "the warm drew another key");
        }
        let glow = material.glow_mask().map(|g| s.materials.add(g));
        let colour = s.materials.add(material);
        spawn_character(
            &mut s.commands,
            &model,
            Transform::IDENTITY,
            0.0,
            &mut s.meshes,
            &mut s.bindposes,
            &[colour],
        )
        .map(|spawned| (spawned, glow))
        .map_err(|e| e.to_string())
    });
    let (spawned, glow) = match built {
        Ok(built) => built,
        Err(why) => {
            warn!(key = ?entry.key, %why, "the warm couldn't draw a key");
            warm.report.failed += 1;
            warm.finish(entry.key);
            return;
        }
    };
    for segment in &spawned.segments {
        s.commands
            .entity(*segment)
            .insert(RenderLayers::layer(model_layer(WARM_SLOT)));
    }
    let masks = spawn_glow_masks(
        &mut s.commands,
        &model,
        &spawned,
        &[glow],
        glow_layer(WARM_SLOT),
    );
    // Its glow mask too, unless the glow pass leaves the part out.
    let checks: Vec<Entity> = spawned.segments[..1]
        .iter()
        .chain(&masks)
        .copied()
        .collect();
    let mut watched = watch.lock();
    for e in &checks {
        watched.checks.insert(*e, Compiled::Waiting);
    }
    stage.roots.push(spawned.root);
    stage.drawing.push(Drawing {
        key: entry.key,
        checks,
        since: Instant::now(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turntable::tests::{app, extract, placed, set, settle, states};
    use crate::turntable::{CardState, Turntables};
    use guacamole::character::key::Pass;
    use guacamole::character::warm::MeshShape;

    /// Frames until the warm list is read, up to 5 s.
    fn read(app: &mut App) {
        let until = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Warm>().listed().is_empty() {
            assert!(Instant::now() < until, "the list never read");
            app.update();
        }
    }

    /// What every character mesh on a slot's layers draws: its material's
    /// key and its mesh's shape, the colour pass's and the glow pass's.
    fn drawn(app: &mut App) -> (HashSet<WarmKey>, HashSet<WarmKey>) {
        let world = app.world_mut();
        let mut q = world.query::<(&Mesh3d, &MeshMaterial3d<CharacterMaterial>, &RenderLayers)>();
        let (meshes, materials) = (
            world.resource::<Assets<Mesh>>(),
            world.resource::<Assets<CharacterMaterial>>(),
        );
        let (mut colour, mut glow) = (HashSet::new(), HashSet::new());
        for (mesh, material, layers) in q.iter(world) {
            let slot = (0..8).find(|s| {
                *layers == RenderLayers::layer(model_layer(*s))
                    || *layers == RenderLayers::layer(glow_layer(*s))
            });
            assert!(slot.is_some(), "only the slots' meshes: {layers:?}");
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
            };
            let mut key = materials.get(&material.0).unwrap().key;
            let pass = std::mem::take(&mut key.pass);
            let into = match pass {
                Pass::Colour => &mut colour,
                Pass::GlowMask => &mut glow,
            };
            into.insert(WarmKey {
                material: key,
                mesh: shape,
            });
        }
        (colour, glow)
    }

    /// AE.14: the warm list is exactly the keys the roster's models draw on
    /// their turntables, each once, the glow pass's included: every figure
    /// of the made-up extract placed (two models drawing three keys between
    /// them, one they share, a damaged model, one that didn't extract and
    /// an item).
    #[test]
    fn the_warm_list_equals_the_rosters_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        read(&mut app);
        let listed = app.world().resource::<Warm>().listed().to_vec();
        let report = app.world().resource::<Warm>().report().clone();
        assert_eq!((report.models, report.unreadable), (2, 1), "{report:?}");
        let unique: HashSet<WarmKey> = listed.iter().copied().collect();
        assert_eq!(unique.len(), listed.len(), "each key once: {listed:?}");
        assert_eq!(listed.len(), 3);

        set(
            &mut app,
            [
                placed(0, 1, "One"),
                placed(1, 2, "Two"),
                placed(2, 3, "Three"),
                placed(3, 4, "Four"),
                placed(4, 5, "Five"),
            ],
        );
        settle(&mut app);
        assert_eq!(
            states(&app)[..5],
            [
                Some("model"),
                Some("model"),
                Some("portrait"),
                Some("portrait"),
                Some("portrait")
            ]
        );
        let (colour, glow) = drawn(&mut app);
        assert_eq!(colour, unique);
        // The glow pass draws the same keys but the fins': no glow attribute
        // and no depth written, so the glow pass leaves them out.
        let masked: HashSet<WarmKey> = unique
            .iter()
            .copied()
            .filter(|k| k.material.states.depth_write)
            .collect();
        assert_eq!(masked.len(), 2);
        assert_eq!(glow, masked, "the glow pass draws the same keys");
        // Headless, nothing compiles: every key counts as compiled.
        let report = app.world().resource::<Warm>().report().clone();
        assert_eq!((report.state.as_str(), report.keys), ("done", 3));
    }

    /// Plays the render world: every pipeline the warm watches compiles.
    fn compile_all(app: &App) {
        let watch = app.world().resource::<PipelineWatch>();
        for state in watch.lock().checks.values_mut() {
            *state = Compiled::Ready;
        }
    }

    fn roots(app: &App) -> usize {
        let warm = app.world().resource::<Warm>();
        warm.stage.as_ref().map_or(0, |s| s.roots.len())
    }

    /// AE.14: with a renderer, a model whose keys aren't compiled waits on
    /// its portrait; its keys go ahead of the list, one a frame on the
    /// hidden turntable after its camera, and it comes up once the render
    /// world says they're compiled. The stage goes once every key is.
    #[test]
    fn a_turntable_waits_for_its_keys_which_go_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        app.insert_resource(Warm::new(true));
        set(&mut app, [placed(0, 2, "Two")]);
        // Read, then waiting on its three keys, drawn before nothing else
        // is: the list may land first, but the asked keys go to the front.
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let t = app.world().resource::<Turntables>();
            let w = app.world().resource::<Warm>();
            if !w.queue.is_empty() || w.stage.is_some() {
                assert_eq!(t.card_state(0), Some(CardState::Loading));
                break;
            }
            assert!(Instant::now() < until, "nothing asked");
        }
        let asked: Vec<WarmKey> = app
            .world()
            .resource::<Warm>()
            .queue
            .iter()
            .map(|e| e.key)
            .collect();
        assert_eq!(asked.len(), 3, "{asked:?}");
        // The stage's camera, then a key a frame.
        while roots(&app) == 0 {
            app.update();
        }
        assert_eq!(roots(&app), 1);
        app.update();
        assert_eq!(roots(&app), 2);
        app.update();
        assert_eq!(roots(&app), 3);
        assert_eq!(
            app.world().resource::<Turntables>().card_state(0),
            Some(CardState::Loading),
            "nothing compiled yet"
        );
        assert_eq!(app.world().resource::<Warm>().report().state, "compiling");
        compile_all(&app);
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
        let warm = app.world().resource::<Warm>();
        assert!(warm.stage.is_none(), "the stage went");
        assert_eq!(
            (
                warm.report().compiled,
                warm.report().failed,
                warm.report().keys
            ),
            (3, 0, 3)
        );
        assert_eq!(warm.report().state, "done");
        // Nothing of it left: only the slot's own camera and its glow camera.
        let mut cameras = app.world_mut().query::<&Camera>();
        assert_eq!(cameras.iter(app.world()).count(), 2);
    }

    /// A key the render world reports failing doesn't hold a model back
    /// forever: it's done (and counted failed), so the turntable comes up.
    #[test]
    fn a_key_that_fails_to_compile_lets_the_model_come_up() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        app.insert_resource(Warm::new(true));
        set(&mut app, [placed(0, 1, "One")]);
        let until = Instant::now() + Duration::from_secs(5);
        while roots(&app) == 0 {
            assert!(Instant::now() < until, "nothing drawn");
            app.update();
        }
        let watch = app.world().resource::<PipelineWatch>().clone();
        for state in watch.lock().checks.values_mut() {
            *state = Compiled::Failed("made up".into());
        }
        settle(&mut app);
        assert_eq!(states(&app)[0], Some("model"));
        assert!(app.world().resource::<Warm>().report().failed >= 1);
    }
}
