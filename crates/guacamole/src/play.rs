//! The first walk on the TV (openspec AG.21, design decisions 1 and 13;
//! level-scene's The first character placed walks into the level, Open
//! question 4's (a): the original's flow minus its story menus, a stand-in
//! until the game's own menus land).
//!
//! - Who: the first figure placed that walks ([`guacamole::walkers`]),
//!   Eruptor alone until AG.27. Any other figure, a character placed before
//!   him included, keeps the portal view and its card.
//! - When: once his intro is over (or none plays), the TV on the portal
//!   view, the level loaded with its solids ([`Level::walkable`]) and his
//!   model read with every pipeline it draws compiled. Until then the portal
//!   view holds.
//! - What: he stands at `213_Init_Location`'s first waypoint facing 0°,
//!   settled onto the level's collision, his idle looping on AE's
//!   evaluator; the camera at camera 0's rest pose follows him on AG.17's
//!   chain, no triggers ([`crate::hero`]); the level's input layer takes
//!   the pads ([`crate::pad`]). The camera is the level's
//!   ([`crate::level::view_camera`]: its output onto the TV's target with
//!   the replacing write the start's pass compiled, AG.44), so the level
//!   covers the whole TV.
//! - He's drawn in the level's lights and fog without its point light
//!   ([`guacamole::level_scene::hero_scene`]), so his keys are the ones the
//!   start's pass compiled from the census for his card; any that aren't
//!   go through the warm, which counts them as outside the census. His
//!   lights are the world's, as the level's are, so his camera's moves
//!   rewrite none of his materials.
//!
//! The TV leaves the level when it leaves the portal view (the game
//! quit, a server error) or the level gives way to another load. What a
//! removal, a second character, an item or an intro does once the level
//! shows is AG.29's.

use std::sync::Arc;
use std::time::Duration;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Projection, RenderTarget};
use bevy::image::CompressedImageFormats;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use guac_ir::Model;
use guac_ir::model::{Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::Scene;
use guac_motion::camera::{CameraDef, GameCamera};
use guac_motion::hero::Hero;
use guacamole::character::attach::{self, SpawnedAttachments};
use guacamole::character::warm::{self, WarmKey};
use guacamole::character::{
    CharacterMaterial, CharacterUniform, ShadowLookup, reference_material, remaster,
    spawn_character,
};
use guacamole::level_scene::{START_FACING, hero_scene};
use guacamole::walkers::{self, Walker};
use serde::Serialize;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::Placement;
use tracing::{info, warn};

use crate::cards::{PortalCards, PortalChanged};
use crate::driver::PortalChange;
use crate::hero::{FollowCamera, HeroBody, HeroPlugin, following, spawn_hero};
use crate::intro::IntroQueue;
use crate::level::{
    LEVEL_LAYER, Level, LevelCamera, LevelSystems, LitAlso, VIEW_ORDER, view_camera,
};
use crate::motion::ReducedMotion;
use crate::pad::LevelPads;
use crate::physics::AvianSeam;
use crate::prepare::Prepare;
use crate::screen::TvScreen;
use crate::turntable::{Loaded, Turntables, load_model};
use crate::tv::{Current, Redrawn, TvCamera};
use crate::warm::Warm;

/// Where the TV is, as `/api/_test/tv` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The TV's own screens: the portal view and what leads to it.
    Portal,
    /// The level, walking.
    Level,
    /// The level, waiting (Start, or no pad walking the hero).
    Paused,
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Portal => "portal",
            Mode::Level => "level",
            Mode::Paused => "paused",
        }
    }
}

/// The hero's model on its way to the level.
enum Model3d {
    /// No shelf yet to read it off.
    Shelfless,
    Reading(Task<Result<Loaded, String>>),
    /// Read; waiting for the warm to compile what it draws with.
    Compiling {
        loaded: Box<Loaded>,
        keys: Vec<WarmKey>,
    },
    Ready(Box<Loaded>),
    /// Why it can't walk: the level isn't entered for it.
    Failed(String),
}

impl Model3d {
    fn name(&self) -> &'static str {
        match self {
            Model3d::Shelfless => "shelfless",
            Model3d::Reading(_) => "reading",
            Model3d::Compiling { .. } => "compiling",
            Model3d::Ready(_) => "ready",
            Model3d::Failed(_) => "failed",
        }
    }
}

/// The figure that walks once the level shows.
struct Candidate {
    slot: SlotIndex,
    placement: Placement,
    load_file: String,
    walker: &'static Walker,
    model: Model3d,
}

/// The level shown with the hero in it.
struct Inside {
    hero: Entity,
    camera: Entity,
    model: Arc<Model>,
    joints: Vec<Entity>,
    attachments: SpawnedAttachments,
    /// Parts with scroll tracks, and their materials.
    scrolling: Vec<(Vec<Scroll>, Handle<CharacterMaterial>)>,
    /// The TV's virtual clock when he came in, for his idle and scrolls.
    since: Duration,
}

/// The walk: who's placed in what order, who walks and the level shown.
#[derive(Resource, Default)]
pub(crate) struct Play {
    /// Occupied slots in the order their figures were placed.
    order: Vec<SlotIndex>,
    candidate: Option<Candidate>,
    inside: Option<Inside>,
}

/// What `/api/_test/tv` reports of the walk.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct HeroReport {
    /// 1-based, as the phone numbers slots.
    pub slot: u8,
    pub figure_id: String,
    pub load_file: String,
    /// His model: `shelfless`, `reading`, `compiling`, `ready` or `failed`,
    /// with why.
    pub model: String,
    pub reason: Option<String>,
    /// In the level: his pivot (the bottom of his solid) in the level's
    /// frame, metres, and the way he faces, degrees.
    pub position: Option<[f32; 3]>,
    pub heading: Option<f32>,
    /// In the level: the camera it shows through, as drawn.
    pub camera: Option<CameraReport>,
}

/// The level's camera as the TV draws it: its eased transform and its
/// picture, so the rest pose can be read off the TV (gameplay-camera's The
/// camera starts behind the hero).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct CameraReport {
    /// Where it stands, the level's frame, metres.
    pub eye: [f32; 3],
    /// The way it looks across the ground (degrees, 0 along +z and 90 along
    /// +x, as his heading) and above level (degrees, negative down).
    pub heading: f32,
    pub pitch: f32,
    /// The picture: its height in degrees, the near plane and the far clip
    /// in metres.
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraReport {
    /// `None` for a camera that isn't a perspective one.
    pub(crate) fn of(transform: &Transform, projection: &Projection) -> Option<Self> {
        let Projection::Perspective(p) = projection else {
            return None;
        };
        let f = transform.forward();
        Some(Self {
            eye: transform.translation.to_array(),
            heading: f.x.atan2(f.z).to_degrees().rem_euclid(360.0),
            pitch: f.y.atan2(f.x.hypot(f.z)).to_degrees(),
            fov_y: p.fov.to_degrees(),
            near: p.near,
            far: p.far,
        })
    }
}

impl Play {
    /// The TV's mode: the level's, paused while it waits.
    pub(crate) fn mode(&self, pads: &LevelPads) -> Mode {
        match (&self.inside, pads.paused()) {
            (None, _) => Mode::Portal,
            (Some(_), false) => Mode::Level,
            (Some(_), true) => Mode::Paused,
        }
    }

    /// The hero's entity, while the level shows: the walk's shots'.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) fn hero(&self) -> Option<Entity> {
        self.inside.as_ref().map(|i| i.hero)
    }

    /// Who walks, or will once the level shows, where he stands and the
    /// camera following him.
    pub(crate) fn report(
        &self,
        bodies: &Query<&HeroBody>,
        cameras: &Query<(&Transform, &Projection), With<FollowCamera>>,
    ) -> Option<HeroReport> {
        let c = self.candidate.as_ref()?;
        let hero = self
            .inside
            .as_ref()
            .and_then(|i| bodies.get(i.hero).ok())
            .map(|b| &b.hero);
        let camera = self
            .inside
            .as_ref()
            .and_then(|i| cameras.get(i.camera).ok())
            .and_then(|(t, p)| CameraReport::of(t, p));
        Some(HeroReport {
            slot: c.slot.as_u8() + 1,
            figure_id: c.placement.figure_id.as_str().to_owned(),
            load_file: c.load_file.clone(),
            model: c.model.name().to_owned(),
            reason: match &c.model {
                Model3d::Failed(why) => Some(why.clone()),
                _ => None,
            },
            position: hero.map(|h| h.pivot.to_array()),
            heading: hero.map(|h| h.facing.rem_euclid(360.0)),
            camera,
        })
    }
}

/// The walk's systems, after the level's.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlaySystems;

/// The walk: [`Play`], the hero's step and his camera ([`HeroPlugin`]).
/// Needs the level loader, the turntables (their shelf and warm), the
/// physics seam, the intro queue, the input router's [`LevelPads`] and the
/// TV's [`Current`] screen and camera.
pub(crate) struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HeroPlugin)
            .init_resource::<Play>()
            .add_systems(
                Update,
                (
                    follow_portal,
                    choose_hero,
                    load_hero,
                    leave,
                    enter,
                    pose_hero,
                )
                    .chain()
                    .in_set(PlaySystems)
                    .after(LevelSystems),
            );
    }
}

/// The slots in the order their figures were placed; the waiting hero
/// dropped when his figure leaves or another replaces it before the level
/// shows (after, that's AG.29's).
fn follow_portal(mut heard: MessageReader<PortalChanged>, mut play: ResMut<Play>) {
    for PortalChanged(change) in heard.read() {
        let slot = match change {
            PortalChange::Placed { slot, .. } | PortalChange::Cleared { slot } => *slot,
        };
        play.order.retain(|s| *s != slot);
        if matches!(change, PortalChange::Placed { .. }) {
            play.order.push(slot);
        }
        if play.inside.is_none() && play.candidate.as_ref().is_some_and(|c| c.slot == slot) {
            info!(slot = slot.display(), "the waiting hero left the portal");
            play.candidate = None;
        }
    }
}

/// The first figure placed that walks is the hero, once the shelf says
/// which character each figure is.
fn choose_hero(cards: Res<PortalCards>, turntables: Res<Turntables>, mut play: ResMut<Play>) {
    if play.candidate.is_some() {
        return;
    }
    let Some(shelf) = turntables.shelf() else {
        return;
    };
    let chosen = play.order.iter().find_map(|slot| {
        let placement = cards.slots[slot.as_usize()].as_ref()?;
        let load = shelf.loads.get(placement.figure_id.as_str())?;
        let walker = walkers::walker(load)?;
        Some(Candidate {
            slot: *slot,
            placement: placement.clone(),
            load_file: load.clone(),
            walker,
            model: Model3d::Shelfless,
        })
    });
    if let Some(c) = chosen {
        info!(
            slot = c.slot.display(),
            figure = %c.placement.name,
            load_file = %c.load_file,
            "the hero, once the level shows"
        );
        play.candidate = Some(c);
    }
}

/// The scene the hero's keys are made in: the hero scene's switches (fog
/// on, no point light, no shadows), which alone key a pipeline; the
/// level's lights and fog are uniforms, set when he's drawn.
fn key_scene() -> Scene {
    Scene {
        fog: true,
        ..Scene::new()
    }
}

/// The hero's model off the shelf, on a task; then what it draws with
/// compiled, the warm asked for any key it hasn't.
fn load_hero(
    mut play: ResMut<Play>,
    turntables: Res<Turntables>,
    mut warm: ResMut<Warm>,
    device: Option<Res<RenderDevice>>,
) {
    let Some(c) = play.candidate.as_mut() else {
        return;
    };
    let next = match &mut c.model {
        Model3d::Shelfless => {
            let Some(shelf) = turntables.shelf().cloned() else {
                return;
            };
            if let Some(why) = shelf.failed.get(&c.load_file) {
                Model3d::Failed(format!("the model didn't extract: {why}"))
            } else {
                // Headless (no GPU) the images keep their blocks as given.
                let formats = device.map_or(CompressedImageFormats::BC, |d| {
                    CompressedImageFormats::from_features(d.features())
                });
                let load = c.load_file.clone();
                Model3d::Reading(
                    AsyncComputeTaskPool::get()
                        .spawn(async move { load_model(&shelf, &load, formats) }),
                )
            }
        }
        Model3d::Reading(task) => match check_ready(task) {
            None => return,
            Some(Err(why)) => Model3d::Failed(why),
            Some(Ok(loaded)) => match warm::keys(&loaded.model, &key_scene()) {
                Err(why) => Model3d::Failed(why.to_string()),
                Ok(keys) => {
                    let scene = key_scene();
                    if !warm.covers(keys.iter().map(|(k, _)| k)) {
                        warm.ask(&loaded.model, &keys, &scene);
                    }
                    info!(
                        load_file = %c.load_file,
                        keys = keys.len(),
                        read_ms = loaded.took.as_millis() as u64,
                        "the hero's model read"
                    );
                    Model3d::Compiling {
                        loaded: Box::new(loaded),
                        keys: keys.into_iter().map(|(k, _)| k).collect(),
                    }
                }
            },
        },
        Model3d::Compiling { keys, .. } if warm.covers(keys.iter()) => {
            let Model3d::Compiling { loaded, .. } =
                std::mem::replace(&mut c.model, Model3d::Shelfless)
            else {
                unreachable!("matched as compiling")
            };
            Model3d::Ready(loaded)
        }
        _ => return,
    };
    if let Model3d::Failed(why) = &next {
        warn!(load_file = %c.load_file, reason = %why, "the hero's model can't be drawn; the portal view stays");
    }
    c.model = next;
}

/// The level gone from under the hero, or the TV off the portal view: the
/// hero, his camera and the pads' level layer go, the level hidden again.
fn leave(
    mut commands: Commands,
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut pads: ResMut<LevelPads>,
    current: Option<Res<Current>>,
) {
    let Some(inside) = play.inside.as_ref() else {
        return;
    };
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    let shown = level.shown_through() == Some(inside.camera);
    if on_portal && shown {
        return;
    }
    info!(on_portal, shown, "the TV leaves the level");
    let inside = play.inside.take().expect("checked above");
    level.hide(&mut commands, inside.camera);
    commands.entity(inside.hero).despawn();
    commands.entity(inside.camera).despawn();
    pads.hide();
    play.candidate = None;
}

/// What [`enter`] spawns with.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct EnterSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    materials: ResMut<'w, Assets<CharacterMaterial>>,
    time: Res<'w, Time>,
    redrawn: MessageWriter<'w, Redrawn>,
}

/// The TV into the level, once every condition holds: the hero settled at
/// the start, his model on him, the camera following, the pads the level's.
fn enter(
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut pads: ResMut<LevelPads>,
    seam: AvianSeam,
    current: Option<Res<Current>>,
    prepare: Option<Res<Prepare>>,
    queue: Option<Res<IntroQueue>>,
    tv: Query<Option<&RenderTarget>, With<TvCamera>>,
    mut s: EnterSpawner,
) {
    if play.inside.is_some() {
        return;
    }
    let Some(Candidate {
        walker,
        model: Model3d::Ready(_),
        ..
    }) = play.candidate.as_ref()
    else {
        return;
    };
    let walker = *walker;
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    let preparing = prepare.is_some_and(|p| p.preparing());
    let intros = queue.is_some_and(|q| q.playing().is_some() || q.queued().next().is_some());
    if !on_portal || preparing || intros {
        return;
    }
    let Ok(target) = tv.single() else {
        return;
    };
    let Some(start) = level.walkable().map(|l| l.start) else {
        return;
    };
    let Some((lights, fog)) = level.lighting().map(|(l, f)| (l.clone(), f)) else {
        return;
    };
    let Some(solid) = walker.solid() else {
        warn!(load_file = walker.load_file, "the walker has no solid");
        return;
    };
    let mut hero = Hero::new(solid, walker.tuning, start, START_FACING);
    hero.settle(&seam, &[]);
    let feet = hero.pivot;
    let Some(c) = play.candidate.as_mut() else {
        unreachable!("matched above")
    };
    let Model3d::Ready(loaded) = std::mem::replace(&mut c.model, Model3d::Shelfless) else {
        unreachable!("matched as ready")
    };
    let follow = FollowCamera {
        camera: GameCamera::new(CameraDef::LEVEL_213_START, feet),
        planes: Vec::new(),
    };
    // In the world's frame, as the level's lights are.
    let scene = hero_scene(&lights, &fog, Mat4::IDENTITY);
    let hero_entity = spawn_hero(&mut s.commands, hero);
    let drawn = match draw_hero(&mut s, hero_entity, walker, &loaded, &scene) {
        Ok(drawn) => drawn,
        Err(why) => {
            warn!(load_file = walker.load_file, reason = %why, "the hero's model can't be drawn; the portal view stays");
            s.commands.entity(hero_entity).despawn();
            c.model = Model3d::Failed(why);
            return;
        }
    };
    c.model = Model3d::Ready(loaded);
    let target = target.cloned().unwrap_or_default();
    let camera = s
        .commands
        .spawn((
            view_camera(target, VIEW_ORDER),
            RenderLayers::layer(LEVEL_LAYER),
            LevelCamera,
            Name::new("level camera"),
        ))
        .insert(following(follow))
        .id();
    let now = s.time.elapsed();
    level.show(&mut s.commands, camera, now);
    level.light_also(drawn.lit, &mut s.materials);
    pads.show();
    s.redrawn.write(Redrawn::Level);
    info!(
        load_file = walker.load_file,
        start = ?start,
        feet = ?feet,
        facing = START_FACING,
        "the TV enters the level"
    );
    play.inside = Some(Inside {
        hero: hero_entity,
        camera,
        model: drawn.model,
        joints: drawn.joints,
        attachments: drawn.attachments,
        scrolling: drawn.scrolling,
        since: now,
    });
}

/// What [`draw_hero`] made.
struct DrawnHero {
    model: Arc<Model>,
    joints: Vec<Entity>,
    attachments: SpawnedAttachments,
    scrolling: Vec<(Vec<Scroll>, Handle<CharacterMaterial>)>,
    lit: Vec<LitAlso>,
}

/// The hero's model on his entity at his scale, in the level's layer, its
/// materials (and what it carries) in `scene`, posed at its idle's start.
fn draw_hero(
    s: &mut EnterSpawner,
    hero: Entity,
    walker: &Walker,
    loaded: &Loaded,
    scene: &Scene,
) -> Result<DrawnHero, String> {
    let model = loaded.model.clone();
    let handles: Vec<((String, bool, Sampler), Handle<Image>)> = loaded
        .images
        .iter()
        .map(|(k, image)| (k.clone(), s.images.add(image.clone())))
        .collect();
    let texture = |t: &guac_ir::model::TextureSlot| -> Handle<Image> {
        let key = (t.texture.clone(), remaster::is_srgb(t.role), t.sampler);
        handles
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, h)| h.clone())
            .unwrap_or_default()
    };
    let drawn = attach::drawn(&model);
    let mut colour = Vec::with_capacity(drawn.materials.len());
    let mut lit = Vec::with_capacity(drawn.materials.len());
    let mut scrolling = Vec::new();
    for m in &drawn.materials {
        // Lit in the world's frame once the level takes them
        // (`Level::light_also`).
        let material = CharacterMaterial::from_ir(m, scene, ShadowLookup::NONE, texture)
            .map_err(|e| format!("material {}: {e}", m.name))?;
        let reference = reference_material(m).map_err(|e| format!("material {}: {e}", m.name))?;
        let handle = s.materials.add(material);
        if !m.scrolls.is_empty() {
            scrolling.push((m.scrolls.clone(), handle.clone()));
        }
        lit.push(LitAlso {
            material: handle.clone(),
            reference,
            alpha: m.states.alpha_test.unwrap_or(0.0),
        });
        colour.push(handle);
    }
    let body = spawn_character(
        &mut s.commands,
        &model,
        Transform::from_scale(Vec3::splat(walker.scale)),
        0.0,
        &mut s.meshes,
        &mut s.bindposes,
        &colour,
    )
    .map_err(|e| e.to_string())?;
    let attachments = attach::spawn_attachments(
        &mut s.commands,
        &model,
        &drawn,
        body.root,
        &body.joints,
        0.0,
        &mut s.meshes,
        &mut s.bindposes,
        &colour,
    )
    .map_err(|e| e.to_string())?;
    let layer = RenderLayers::layer(LEVEL_LAYER);
    for e in body
        .segments
        .iter()
        .chain(attachments.parts.iter().map(|p| &p.entity))
    {
        s.commands.entity(*e).insert(layer.clone());
    }
    s.commands.entity(hero).insert((
        Visibility::default(),
        Name::new(format!("hero {}", walker.load_file)),
    ));
    s.commands.entity(body.root).insert(ChildOf(hero));
    Ok(DrawnHero {
        model,
        joints: body.joints,
        attachments,
        scrolling,
        lit,
    })
}

/// His idle, looped on the TV's virtual clock (so it holds while the level
/// waits), and his scrolling parts; under reduced motion both hold their
/// start. Locomotion is AG.30's.
fn pose_hero(
    play: Res<Play>,
    time: Res<Time>,
    reduced: Option<Res<ReducedMotion>>,
    mut transforms: Query<&mut Transform>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let Some(inside) = play.inside.as_ref() else {
        return;
    };
    let seconds = if reduced.is_some_and(|r| r.0) {
        0.0
    } else {
        time.elapsed().saturating_sub(inside.since).as_secs_f64()
    };
    let pose = guacamole::character::mesh::pose(&inside.model, seconds);
    let carried = inside
        .model
        .attachments
        .iter()
        .zip(&inside.attachments.joints)
        .filter(|(_, joints)| !joints.is_empty())
        .flat_map(|(a, joints)| joints.iter().zip(attach::attachment_pose(a, seconds)));
    for (joint, p) in inside.joints.iter().zip(pose).chain(carried) {
        if let Ok(mut t) = transforms.get_mut(*joint)
            && *t != p
        {
            *t = p;
        }
    }
    for (scrolls, handle) in &inside.scrolling {
        let rows = CharacterUniform::uv_rows(&scroll::transforms(scrolls, seconds));
        // Read first: a write marks the material for upload again.
        if let Some(mut m) = materials.get_mut(handle)
            && (m.uniform.uv_u, m.uniform.uv_v) != rows
        {
            (m.uniform.uv_u, m.uniform.uv_v) = rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bevy::input::gamepad::GamepadButton;
    use bevy::time::TimeUpdateStrategy;
    use guac_extract::intros::IrFormat;
    use guac_motion::camera::View;

    use super::*;
    use crate::level::tests::{FLOOR, START, WALL, lights, write_level};
    use crate::level::{LevelPlugin, level_channel};
    use crate::pad::test_pad;
    use crate::physics::SeamPlugin;
    use crate::turntable::tests::{app, extract, model, placed};

    const ERUPTOR: u32 = 9;

    /// The turntables' made-up extract with Eruptor's load file on toy 9,
    /// a made-up model of its own, and a chapter 1 on [`write_level`]'s
    /// walk; the TV on the portal view.
    fn play_app(dir: &std::path::Path) -> (App, crate::level::LevelSender) {
        let mut shelf = extract(dir);
        let ron = guac_ir::to_ron(&model("009_Eruptor")).unwrap();
        std::fs::write(dir.join(IrFormat::Ron.model_path("009_Eruptor")), ron).unwrap();
        shelf
            .loads
            .insert(format!("{ERUPTOR:06x}-0000"), "009_Eruptor".into());
        let mut app = app(shelf);
        app.add_message::<Redrawn>()
            .add_message::<PortalChanged>()
            .add_plugins((bevy::input::InputPlugin, crate::input::InputRouter))
            .add_plugins((LevelPlugin, SeamPlugin, PlayPlugin))
            .init_resource::<IntroQueue>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .insert_resource(Current {
                screen: TvScreen::Portal {
                    game: "Skylanders Giants".into(),
                },
                since: std::time::Instant::now(),
                clients: 1,
            });
        // Avian's plugins finish building here, as `App::run` would.
        app.finish();
        app.cleanup();
        app.world_mut().spawn(TvCamera);
        let (tx, feed) = level_channel();
        app.insert_resource(feed);
        (app, tx)
    }

    fn place(app: &mut App, slot: u8, toy: u32, name: &str) {
        let change = placed(slot, toy, name);
        crate::turntable::tests::set(app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
    }

    fn mode(app: &App) -> Mode {
        let pads = app.world().resource::<LevelPads>();
        app.world().resource::<Play>().mode(pads)
    }

    fn level_state(app: &App) -> String {
        app.world().resource::<Level>().report().state.clone()
    }

    fn hero_model(app: &App) -> Option<&'static str> {
        app.world()
            .resource::<Play>()
            .candidate
            .as_ref()
            .map(|c| c.model.name())
    }

    /// Frames until `done`, up to 5 s of the wall clock (the reading tasks
    /// run on it).
    fn until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(app) {
            assert!(
                Instant::now() < deadline,
                "{what}: still {:?}, the level {}, the hero's model {:?}",
                mode(app),
                level_state(app),
                hero_model(app)
            );
            app.update();
        }
    }

    fn hero(app: &App) -> Hero {
        let e = app.world().resource::<Play>().hero().expect("in the level");
        app.world()
            .entity(e)
            .get::<HeroBody>()
            .unwrap()
            .hero
            .clone()
    }

    fn view(app: &mut App) -> View {
        let mut cameras = app.world_mut().query::<&FollowCamera>();
        cameras.single(app.world()).unwrap().camera.view()
    }

    /// AG.21: placing Eruptor, the level loaded, takes the TV into it: the
    /// level shows through the following camera on the TV's target with the
    /// level's replacing write, the pads go to the level's layer, and the
    /// first pad walks him against the camera's view until the wall stops
    /// him.
    #[test]
    fn a_character_placed_enters() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level", |a| level_state(a) == "ready");
        assert_eq!(mode(&app), Mode::Portal, "nothing placed: the portal");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        let report = app.world().resource::<Level>().report().clone();
        assert!(report.shown, "{report:?}");
        assert_eq!(report.state, "shown");
        assert!(app.world().resource::<LevelPads>().shows());
        let camera = app.world().resource::<Level>().shown_through().unwrap();
        let entity = app.world().entity(camera);
        assert!(entity.contains::<LevelCamera>() && entity.contains::<FollowCamera>());
        let c = entity.get::<Camera>().unwrap();
        assert_eq!(c.order, VIEW_ORDER);
        assert!(matches!(
            c.output_mode,
            bevy::camera::CameraOutputMode::Write {
                blend_state: Some(b),
                ..
            } if b == bevy::render::render_resource::BlendState::REPLACE
        ));
        assert_eq!(
            entity.get::<RenderLayers>(),
            Some(&RenderLayers::layer(LEVEL_LAYER))
        );

        // The first pad, pushed forward: along the view's 15° until the
        // wall 6 m ahead along +z holds him 5 mm off it.
        let from = hero(&app).pivot;
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..30 {
            app.update();
        }
        let walked = hero(&app).pivot - from;
        let heading = walked.x.atan2(walked.z).to_degrees();
        assert!((heading - 15.0).abs() < 0.05, "{heading}");
        for _ in 0..120 {
            app.update();
        }
        let at = hero(&app);
        // His 0.8644 m sphere meets the level's triangles grown by their
        // 0.05 m radius (AG.16's `solid`).
        let radius = 0.8644 + 0.05;
        let gap = START[2] + WALL - (at.pivot.z + radius);
        eprintln!("against the wall: {gap} m off it at {:?}", at.pivot);
        assert!((gap - 0.005).abs() < 1e-3, "{gap}");
        assert!(at.pivot.x - from.x > 3.0, "slid along it: {:?}", at.pivot);

        // Start pauses it: the mode says so.
        test_pad::press(&mut app, pad, GamepadButton::Start);
        app.update();
        assert_eq!(mode(&app), Mode::Paused);
    }

    /// The camera following him rewrites no material, standing or walking:
    /// the level's and the hero's lights are the world's, which the shader
    /// turns by its view. (Lit in view space, the eased camera's float noise
    /// alone rewrote every material on 131 of 144 frames at rest.)
    #[test]
    fn a_following_camera_lights_nothing_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 144.0,
        )));
        let relit = app.world().resource::<Level>().report().relit;
        let eye = view(&mut app).eye;
        for _ in 0..144 {
            app.update();
        }
        test_pad::stick(&mut app, pad, 1.0, 1.0);
        for _ in 0..144 {
            app.update();
        }
        assert!(
            view(&mut app).eye.distance(eye) > 1.0,
            "the camera followed"
        );
        assert_eq!(app.world().resource::<Level>().report().relit, relit);
        // Everything the level's camera draws, the hero and the level, is
        // lit in the world's frame (his card's turntable keeps its rig's
        // view space).
        let mut materials = app
            .world_mut()
            .query::<(&MeshMaterial3d<CharacterMaterial>, &RenderLayers)>();
        let assets = app.world().resource::<Assets<CharacterMaterial>>();
        let frames: Vec<f32> = materials
            .iter(app.world())
            .filter(|(_, l)| **l == RenderLayers::layer(LEVEL_LAYER))
            .map(|(m, _)| assets.get(&m.0).unwrap().uniform.misc.w)
            .collect();
        assert!(
            frames.len() >= 5,
            "the level's four segments and his: {frames:?}"
        );
        assert!(
            frames
                .iter()
                .all(|w| *w == guacamole::character::LIGHTS_IN_WORLD),
            "{frames:?}"
        );
    }

    /// AG.21: the start pose. Eruptor at `213_Init_Location`'s first
    /// waypoint facing 0°, settled 5 mm above the floor, drawn there at his
    /// 150 %; the camera at camera 0's rest pose off his feet (55 ft at 32°
    /// behind the view along 15°, aimed 9 ft up), looking along 15°. Placed
    /// before the level loads, so he walks in the frame its floor spawns.
    #[test]
    fn the_start_pose() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        let h = hero(&app);
        eprintln!("settled at {:?}, facing {}", h.pivot, h.facing);
        assert_eq!((h.pivot.x, h.pivot.z), (START[0], START[2]));
        assert!((h.pivot.y - (FLOOR + 0.005)).abs() < 1e-3, "{:?}", h.pivot);
        assert_eq!(h.facing, START_FACING);
        assert_eq!(*h.tuning(), guac_motion::tuning::Tuning::ERUPTOR);
        assert_eq!(*h.solid(), guac_motion::tuning::eruptor_solid());
        let v = view(&mut app);
        let off = v.eye - h.pivot;
        let want = Vec3::new(-3.679547, 8.883567, -13.732257);
        assert!((off - want).length() < 1e-4, "{off}");
        assert!((v.heading() - 15.0).abs() < 1e-4, "{}", v.heading());
        assert!((v.pitch() + 23.360138).abs() < 1e-4, "{}", v.pitch());
        // The camera the TV draws through stands there too: its transform
        // the eye looking at the aim, its picture camera 0's (53.96° top to
        // bottom, 0.1 m to its 800 ft far clip).
        let mut cameras = app
            .world_mut()
            .query_filtered::<(&Transform, &Projection), With<FollowCamera>>();
        let (t, p) = cameras.single(app.world()).unwrap();
        let drawn = CameraReport::of(t, p).expect("a perspective camera");
        let off = Vec3::from_array(drawn.eye) - h.pivot;
        assert!((off - want).length() < 1e-4, "{drawn:?}");
        assert!((drawn.heading - 15.0).abs() < 1e-3, "{drawn:?}");
        assert!((drawn.pitch + 23.360138).abs() < 1e-3, "{drawn:?}");
        assert!((drawn.fov_y - 53.96).abs() < 1e-3, "{drawn:?}");
        assert!((drawn.fov_y - v.fov_y).abs() < 1e-4, "{drawn:?}");
        assert_eq!(drawn.near, guac_motion::camera::NEAR);
        assert!((drawn.far - 243.84).abs() < 1e-4, "{drawn:?}");
        // Drawn: his model on him at his scale, its parts on the level's
        // layer, its idle posing the joints.
        let entity = app.world().resource::<Play>().hero().unwrap();
        let drawn = app.world().entity(entity).get::<Transform>().unwrap();
        assert_eq!(drawn.translation, h.pivot);
        let children = app.world().entity(entity).get::<Children>().unwrap();
        let root = children[0];
        let scale = app.world().entity(root).get::<Transform>().unwrap().scale;
        assert_eq!(scale, Vec3::splat(1.5));
        let mut layers = app
            .world_mut()
            .query_filtered::<&RenderLayers, With<Mesh3d>>();
        let on_level = layers
            .iter(app.world())
            .filter(|l| **l == RenderLayers::layer(LEVEL_LAYER))
            .count();
        assert!(on_level >= 1, "{on_level}");
        // Nothing moves him while no pad walks him: the level waits for one
        // from its first frame of input.
        app.update();
        assert_eq!(mode(&app), Mode::Paused, "no pad: the controller message");
        assert_eq!(hero(&app).pivot, h.pivot);
    }

    /// AG.21: with the level still loading when Eruptor is placed, the
    /// portal view holds, and so it does through his intro; the TV enters
    /// once both are done.
    #[test]
    fn with_the_level_still_loading_the_portal_view_holds_then_enters() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal, "no level yet");
        }
        // His intro plays while the level loads.
        app.world_mut()
            .resource_mut::<IntroQueue>()
            .apply(crate::intro::IntroEvent::Placed {
                slot: SlotIndex::new(0).unwrap(),
                figure_id: format!("{ERUPTOR:06x}-0000"),
                movie: Some("eruptor".into()),
            });
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal, "his intro plays");
        }
        app.world_mut()
            .resource_mut::<IntroQueue>()
            .apply(crate::intro::IntroEvent::Skip);
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    }

    /// level-scene's The first character placed walks into the level: of
    /// two figures that walk placed in one frame, the one placed first is
    /// the hero, not the one in the lower slot nor the one placed last.
    #[test]
    fn the_first_placed_of_two_walkers_is_the_hero() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 2, ERUPTOR, "Eruptor");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 2, "the first placed");
    }

    /// The start's pass holds the TV's screens (AE.25): while it runs the
    /// level isn't entered, his model and the level ready or not; once it's
    /// done he walks in.
    #[test]
    fn the_starts_pass_holds_the_level_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        app.insert_resource(Prepare::new(true));
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal, "the pass runs");
        }
        app.insert_resource(Prepare::new(false));
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    }

    /// His idle loops on the TV's clock while he walks, and holds its start
    /// under reduced motion.
    #[test]
    fn under_reduced_motion_his_idle_holds_its_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        let joints = |app: &App| -> Vec<Transform> {
            let inside = app.world().resource::<Play>().inside.as_ref().unwrap();
            inside
                .joints
                .iter()
                .map(|j| *app.world().entity(*j).get::<Transform>().unwrap())
                .collect()
        };
        let model = app
            .world()
            .resource::<Play>()
            .inside
            .as_ref()
            .unwrap()
            .model
            .clone();
        let n = joints(&app).len();
        let at_start: Vec<Transform> = guacamole::character::mesh::pose(&model, 0.0)
            .into_iter()
            .take(n)
            .collect();
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(mode(&app), Mode::Level, "walking: {pad:?}");
        assert_ne!(joints(&app), at_start, "the idle plays");
        app.insert_resource(ReducedMotion(true));
        for _ in 0..20 {
            app.update();
            assert_eq!(joints(&app), at_start, "held at its start");
        }
    }

    /// Taken off before the level shows, he never walks in; placed again,
    /// he does.
    #[test]
    fn a_hero_taken_off_before_the_level_shows_never_enters() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        let cleared = PortalChange::Cleared {
            slot: SlotIndex::new(0).unwrap(),
        };
        crate::turntable::tests::set(&mut app, [cleared.clone()]);
        app.world_mut().write_message(PortalChanged(cleared));
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(hero_model(&app), None);
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    }

    /// Open question 1's (a) until AG.27: a character that doesn't walk yet,
    /// placed first, keeps the portal view; Eruptor placed after it walks
    /// in. The TV leaving the portal view leaves the level, hidden again.
    #[test]
    fn another_character_keeps_the_portal_view_and_leaving_the_portal_leaves_the_level() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, 1, "One");
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(mode(&app), Mode::Portal);
        assert!(app.world().resource::<Play>().candidate.is_none());
        place(&mut app, 1, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 1);
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(level_state(&app), "ready", "hidden, ready to show again");
        assert!(!app.world().resource::<LevelPads>().shows());
        let mut heroes = app.world_mut().query::<&HeroBody>();
        assert_eq!(heroes.iter(app.world()).count(), 0);
        let mut cameras = app.world_mut().query::<&FollowCamera>();
        assert_eq!(cameras.iter(app.world()).count(), 0);
    }
}
