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
//!   settled onto the level's collision, posed on AE's evaluator by his
//!   locomotion (AG.30: his idle, walk, run and fall by the weights
//!   [`crate::hero::HeroGaits`] steps, his hero side's clips read with his
//!   model); the camera at camera 0's rest pose follows him on AG.17's
//!   chain, no triggers ([`crate::hero`]); the level's input layer takes
//!   the pads ([`crate::pad`]). The camera is the level's
//!   ([`crate::level::view_camera`]: its output onto the TV's target with
//!   the replacing write the start's pass compiled, AG.44), first on the
//!   target, so the level fills the TV under its UI
//!   ([`crate::level::tv_over_level`], AG.50): the portal view hidden,
//!   Start's overlay and the controller message laid over the level.
//! - He's drawn in the level's lights and fog without its point light
//!   ([`guacamole::level_scene::hero_scene`]), so his keys are the ones the
//!   start's pass compiled from the census for his card; any that aren't
//!   go through the warm, which counts them as outside the census. His
//!   lights are the world's, as the level's are, so his camera's moves
//!   rewrite none of his materials.
//!
//! The TV leaves the level when it leaves the portal view (the game
//! quit, a server error) or the level gives way to another load.
//!
//! The portal and the level (AG.29, with AG.63's too-many hold and AG.73's
//! wrong-toy alert; level-scene's One hero at a time, Removing the hero
//! pauses the level and A figure Giants can't use holds the level): what a
//! figure is comes off the extract's roster ([`kind`]), and [`judge`] says
//! which alert the portal asks for, drawn full screen by [`crate::alert`].
//! - A second character beside the hero, or two on the portal when the
//!   level would start, holds it under the too-many alert until one comes
//!   off; the hero taken off, the other comes in where he stood.
//! - The hero taken off with no character left holds the level behind the
//!   place-a-Skylander screen; the next character placed comes in where he
//!   stood, facing his way, and the level carries on from where it held.
//! - A second magic item or sidekick holds it under the too-many-items
//!   alert; one alone, or an adventure-pack piece, changes nothing.
//! - A toy Giants can't use holds the portal view or the level under the
//!   wrong-toy alert until it comes off.
//! - While the level holds, `Time<Virtual>` stops and the pads move no one
//!   ([`LevelPads::hold`]). No intro plays once the level has started
//!   ([`crate::intro_player`]).
//! - A character that doesn't walk yet (AG.27) counts as one on the portal
//!   but can't come in: alone, it leaves the level asking for a Skylander.

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
use guac_ir::clip::Clip;
use guac_ir::model::{Sampler, Scroll};
use guac_ir::scroll;
use guac_ir::shading::Scene;
use guac_ir::{Category, Model};
use guac_motion::camera::{CameraDef, GameCamera};
use guac_motion::hero::Hero;
use guac_motion::locomotion::{self, Locomotion, Loops};
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
use crate::hero::{FollowCamera, HeroBody, HeroGaits, HeroPlugin, following, spawn_hero};
use crate::intro::IntroQueue;
use crate::level::{
    LEVEL_LAYER, Level, LevelCamera, LevelSystems, LitAlso, VIEW_ORDER, view_camera,
};
use crate::motion::ReducedMotion;
use crate::pad::LevelPads;
use crate::physics::AvianSeam;
use crate::prepare::Prepare;
use crate::screen::TvScreen;
use crate::turntable::{Loaded, Shelf, Turntables, load_model, read_hero};
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
    /// The level, held by the portal (AG.29): an alert, or no hero in it.
    Held,
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Portal => "portal",
            Mode::Level => "level",
            Mode::Paused => "paused",
            Mode::Held => "held",
        }
    }
}

/// What a figure on the portal is to Giants, by the extract's roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A Skylander: a figure or a Giant.
    Character,
    /// A magic item, or a sidekick, which Giants counts as one (AG.6's
    /// Barkley beside the cannon).
    MagicItem,
    /// An adventure pack's piece: counted toward nothing, it changes nothing
    /// (its location is AM's).
    Pack,
    /// Off Giants' roster: a later game's character, a trap, a vehicle.
    Unusable,
}

/// A figure the roster doesn't hold is one Giants can't use: the roster is
/// the whole of what the disc knows.
pub(crate) fn kind(shelf: &Shelf, figure_id: &str) -> Kind {
    match shelf.kinds.get(figure_id) {
        Some(Category::Figure | Category::Giant) => Kind::Character,
        Some(Category::Item | Category::Sidekick) => Kind::MagicItem,
        Some(Category::AdventurePack) => Kind::Pack,
        None => Kind::Unusable,
    }
}

/// The portal as the flow reads it: its characters in the order they were
/// placed, each with whether it can come in as the hero (a figure that walks,
/// AG.27's until every character does), and how many magic items and toys
/// Giants can't use stand beside them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Lineup {
    pub(crate) characters: Vec<(SlotIndex, bool)>,
    pub(crate) magic: usize,
    pub(crate) unusable: usize,
}

impl Lineup {
    /// `order`'s slots read off `cards` by `shelf`'s roster.
    fn of(order: &[SlotIndex], cards: &PortalCards, shelf: &Shelf) -> Self {
        let mut lineup = Self::default();
        for slot in order {
            let Some(placement) = cards.slots[slot.as_usize()].as_ref() else {
                continue;
            };
            let id = placement.figure_id.as_str();
            match kind(shelf, id) {
                Kind::Character => {
                    let walks = shelf
                        .loads
                        .get(id)
                        .is_some_and(|load| walkers::walker(load).is_some());
                    lineup.characters.push((*slot, walks));
                }
                Kind::MagicItem => lineup.magic += 1,
                Kind::Pack => {}
                Kind::Unusable => lineup.unusable += 1,
            }
        }
        lineup
    }
}

/// What the portal shows over the TV, full screen (level-scene's portal
/// alerts; [`crate::alert`] draws them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alert {
    /// A toy Giants can't use is on the portal (AG.73).
    WrongToy,
    /// A second character beside the hero, or two when the level would
    /// start (AG.63).
    TooManySkylanders,
    /// A second magic item or sidekick while the level shows.
    TooManyItems,
    /// The hero came off and no character can take his place.
    PlaceSkylander,
}

impl Alert {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Alert::WrongToy => "wrong_toy",
            Alert::TooManySkylanders => "too_many_skylanders",
            Alert::TooManyItems => "too_many_items",
            Alert::PlaceSkylander => "place_a_skylander",
        }
    }
}

/// The alert `lineup` asks for. `inside`: the level has started (shown,
/// paused or held); `hero`: his slot while he's in it; `would_start`: before
/// the start, the level would show now but for the portal. A toy Giants
/// can't use comes first, then too many characters, then too many magic
/// items, then the call for a Skylander.
pub(crate) fn judge(
    lineup: &Lineup,
    inside: bool,
    hero: Option<SlotIndex>,
    would_start: bool,
) -> Option<Alert> {
    if lineup.unusable > 0 {
        return Some(Alert::WrongToy);
    }
    if !inside {
        return (would_start && lineup.characters.len() >= 2).then_some(Alert::TooManySkylanders);
    }
    let others: Vec<bool> = lineup
        .characters
        .iter()
        .filter(|(slot, _)| Some(*slot) != hero)
        .map(|(_, walks)| *walks)
        .collect();
    let too_many = match hero {
        Some(_) => !others.is_empty(),
        None => others.len() >= 2,
    };
    if too_many {
        return Some(Alert::TooManySkylanders);
    }
    if lineup.magic >= 2 {
        return Some(Alert::TooManyItems);
    }
    // A lone character that walks comes in without a screen (its arrival is
    // AG.72's); one that doesn't walk yet can't, so the call stays up.
    (hero.is_none() && !others.first().is_some_and(|walks| *walks)).then_some(Alert::PlaceSkylander)
}

/// The clips his locomotion plays beside his model's idle: his hero side's
/// walk, run and falling (AG.14), on his model's skeleton.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeroClips {
    pub(crate) walk: Option<Clip>,
    pub(crate) run: Option<Clip>,
    pub(crate) fall: Option<Clip>,
}

impl HeroClips {
    fn of(hero: guac_ir::hero::Hero) -> Self {
        let l = hero.locomotion;
        Self {
            walk: l.walk.map(|s| s.clip),
            run: l.run.map(|s| s.clip),
            fall: l.falling.map(|s| s.clip),
        }
    }

    /// The loops [`Locomotion`] keeps, the idle's from `model`.
    fn loops(&self, model: &Model) -> Loops {
        Loops {
            idle: model.idle.duration,
            walk: self.walk.as_ref().map(|c| c.duration),
            run: self.run.as_ref().map(|c| c.duration),
            fall: self.fall.as_ref().map(|c| c.duration),
        }
    }

    fn get(&self, gait: locomotion::Clip) -> Option<&Clip> {
        match gait {
            locomotion::Clip::Idle => None,
            locomotion::Clip::Walk => self.walk.as_ref(),
            locomotion::Clip::Run => self.run.as_ref(),
            locomotion::Clip::Fall => self.fall.as_ref(),
        }
    }
}

/// His model and his locomotion's clips, read together off the shelf.
pub(crate) struct HeroModel {
    loaded: Loaded,
    clips: Arc<HeroClips>,
}

/// `load`'s model and its hero side, on a task.
fn read_hero_model(
    shelf: &Shelf,
    load: &str,
    formats: CompressedImageFormats,
) -> Result<HeroModel, String> {
    let loaded = load_model(shelf, load, formats)?;
    let hero = read_hero(shelf, load).map_err(|e| format!("his hero side: {e}"))?;
    Ok(HeroModel {
        loaded,
        clips: Arc::new(HeroClips::of(hero)),
    })
}

/// The hero's model on its way to the level.
enum Model3d {
    /// No shelf yet to read it off.
    Shelfless,
    Reading(Task<Result<HeroModel, String>>),
    /// Read; waiting for the warm to compile what it draws with.
    Compiling {
        model: Box<HeroModel>,
        keys: Vec<WarmKey>,
    },
    Ready(Box<HeroModel>),
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

/// The hero in the level and what draws him.
struct OnStage {
    hero: Entity,
    model: Arc<Model>,
    clips: Arc<HeroClips>,
    joints: Vec<Entity>,
    attachments: SpawnedAttachments,
    /// Parts with scroll tracks, and their materials.
    scrolling: Vec<(Vec<Scroll>, Handle<CharacterMaterial>)>,
    /// His materials the level lights, which it stops lighting when he
    /// leaves.
    lit: Vec<Handle<CharacterMaterial>>,
    /// The TV's virtual clock when he came in, for his idle and scrolls.
    since: Duration,
}

/// The level shown, the hero in it or not.
struct Inside {
    camera: Entity,
    hero: Option<OnStage>,
    /// Where the last hero stood and the way he faced as he came off: where
    /// the next one comes in (AG.6's replacements: within 0.15 m and 0.18°).
    left: Option<(Vec3, f32)>,
}

/// The walk: who's placed in what order, who walks, the level shown and the
/// portal's alert.
#[derive(Resource, Default)]
pub(crate) struct Play {
    /// Occupied slots in the order their figures were placed.
    order: Vec<SlotIndex>,
    candidate: Option<Candidate>,
    inside: Option<Inside>,
    alert: Option<Alert>,
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
    /// In the level: his locomotion as its last step left it (AG.30).
    pub gait: Option<GaitReport>,
}

/// His locomotion as `/api/_test/tv` reports it (AG.30): no clip is
/// picked, so every clip's weight, and the heaviest.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct GaitReport {
    /// The heaviest: `idle`, `walk`, `run` or `fall`.
    pub clip: String,
    /// What his last step read: his walk's length after its push and
    /// before its slowing, ft/s and m/s.
    pub speed_ft: f32,
    pub speed: f32,
    /// The run's share of the walk and run.
    pub run_share: f32,
    /// Idle, walk, run and fall, summing to 1.
    pub weights: [f32; 4],
    /// The walk's and the run's playback rates, 1 their authored speed.
    pub rates: [f32; 2],
    /// `ground`, `falling` or `landing`.
    pub air: String,
    /// The stand/move switch's side.
    pub moving: bool,
    /// His last step's edge probe held him (AG.58).
    pub held: bool,
    /// Game steps since he came in.
    pub step: u64,
}

impl GaitReport {
    pub(crate) fn of(l: &Locomotion) -> Self {
        Self {
            clip: l.top().name().to_owned(),
            speed_ft: l.speed(),
            speed: l.speed() * guac_motion::tuning::FOOT,
            run_share: l.run_share(),
            weights: l.weights(),
            rates: [
                l.play(locomotion::Clip::Walk).rate,
                l.play(locomotion::Clip::Run).rate,
            ],
            air: l.air().name().to_owned(),
            moving: l.moving(),
            held: l.held(),
            step: l.steps(),
        }
    }
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
    /// The TV's mode: the level's, held while the portal holds it, paused
    /// while it waits for Start or a pad.
    pub(crate) fn mode(&self, pads: &LevelPads) -> Mode {
        match &self.inside {
            None => Mode::Portal,
            Some(_) if pads.held() => Mode::Held,
            Some(_) if pads.paused() => Mode::Paused,
            Some(_) => Mode::Level,
        }
    }

    /// The level has started: shown, paused or held. No intro plays from
    /// then on (character-intros).
    pub(crate) fn started(&self) -> bool {
        self.inside.is_some()
    }

    /// What [`crate::alert`] draws full screen and the mirror reports, as
    /// [`judge`] last ruled.
    pub(crate) fn alert(&self) -> Option<Alert> {
        self.alert
    }

    fn on_stage(&self) -> Option<&OnStage> {
        self.inside.as_ref()?.hero.as_ref()
    }

    /// The hero's entity, while he's in the level: the walk's shots'.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(crate) fn hero(&self) -> Option<Entity> {
        self.on_stage().map(|h| h.hero)
    }

    /// The hero's entity, the camera's and his joints', while he's in the
    /// level: what `/api/_test/tv/drawn` reads each frame (AG.55).
    pub(crate) fn drawn(&self) -> Option<(Entity, Entity, &[Entity])> {
        let inside = self.inside.as_ref()?;
        let h = inside.hero.as_ref()?;
        Some((h.hero, inside.camera, h.joints.as_slice()))
    }

    /// Who walks, or will once the level shows, where he stands and the
    /// camera following him.
    pub(crate) fn report(
        &self,
        bodies: &Query<(&HeroBody, Option<&HeroGaits>)>,
        cameras: &Query<(&Transform, &Projection), With<FollowCamera>>,
    ) -> Option<HeroReport> {
        let c = self.candidate.as_ref()?;
        let body = self.on_stage().and_then(|h| bodies.get(h.hero).ok());
        let hero = body.map(|(b, _)| &b.hero);
        let gait = body
            .and_then(|(_, g)| g)
            .map(|g| GaitReport::of(&g.locomotion));
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
            gait,
        })
    }
}

#[cfg(test)]
impl Play {
    /// The level started with no hero in it, seen through `camera`: for the
    /// tests of what reads [`Play::started`].
    pub(crate) fn started_for_tests(camera: Entity) -> Self {
        Self {
            inside: Some(Inside {
                camera,
                hero: None,
                left: None,
            }),
            ..default()
        }
    }

    /// `alert` ruled, the level started or not, as [`judge_portal`] leaves
    /// it: for the tests of what draws and reports it.
    pub(crate) fn alerting_for_tests(alert: Alert, started: Option<Entity>) -> Self {
        let mut play = match started {
            Some(camera) => Self::started_for_tests(camera),
            None => Self::default(),
        };
        play.alert = Some(alert);
        play
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
                    judge_portal,
                    enter,
                    pose_hero,
                    rest_the_cards,
                )
                    .chain()
                    .in_set(PlaySystems)
                    .after(LevelSystems),
            );
    }
}

/// The slots in the order their figures were placed. The hero's figure
/// leaving the portal, or another placed over it, drops him: before the
/// level shows he never walks in; in it he leaves where he stands
/// ([`take_off`]).
fn follow_portal(
    mut heard: MessageReader<PortalChanged>,
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut commands: Commands,
    bodies: Query<&HeroBody>,
) {
    for PortalChanged(change) in heard.read() {
        let slot = match change {
            PortalChange::Placed { slot, .. } | PortalChange::Cleared { slot } => *slot,
        };
        play.order.retain(|s| *s != slot);
        if matches!(change, PortalChange::Placed { .. }) {
            play.order.push(slot);
        }
        if !play.candidate.as_ref().is_some_and(|c| c.slot == slot) {
            continue;
        }
        play.candidate = None;
        match play.inside.as_mut() {
            None => info!(slot = slot.display(), "the waiting hero left the portal"),
            Some(inside) => take_off(inside, &mut level, &mut commands, &bodies, slot),
        }
    }
}

/// The hero off the portal mid-level: his entity and drawn model go, the
/// level stops lighting his materials, and where he stood and the way he
/// faced are kept for the next one in. The level holds from the next
/// [`judge_portal`] (no hero in it).
fn take_off(
    inside: &mut Inside,
    level: &mut Level,
    commands: &mut Commands,
    bodies: &Query<&HeroBody>,
    slot: SlotIndex,
) {
    let Some(gone) = inside.hero.take() else {
        return;
    };
    if let Ok(body) = bodies.get(gone.hero) {
        inside.left = Some((body.hero.pivot, body.hero.facing));
    }
    level.forget(&gone.lit);
    commands.entity(gone.hero).despawn();
    info!(slot = slot.display(), left = ?inside.left, "the hero left the portal: the level holds");
}

/// `slot`'s figure as the hero-to-be, if it walks.
fn candidate(slot: SlotIndex, cards: &PortalCards, shelf: &Shelf) -> Option<Candidate> {
    let placement = cards.slots[slot.as_usize()].as_ref()?;
    let load = shelf.loads.get(placement.figure_id.as_str())?;
    let walker = walkers::walker(load)?;
    Some(Candidate {
        slot,
        placement: placement.clone(),
        load_file: load.clone(),
        walker,
        model: Model3d::Shelfless,
    })
}

/// Who walks next, once the shelf says which character each figure is:
/// before the level shows, the first figure placed that walks; in it with no
/// hero, the one character on the portal if it walks (two hold it under the
/// too-many alert, [`judge`]).
fn choose_hero(cards: Res<PortalCards>, turntables: Res<Turntables>, mut play: ResMut<Play>) {
    if play.candidate.is_some() {
        return;
    }
    let Some(shelf) = turntables.shelf() else {
        return;
    };
    let chosen = match &play.inside {
        None => play
            .order
            .iter()
            .find_map(|slot| candidate(*slot, &cards, shelf)),
        Some(inside) if inside.hero.is_none() => {
            match Lineup::of(&play.order, &cards, shelf).characters.as_slice() {
                [(slot, true)] => candidate(*slot, &cards, shelf),
                _ => None,
            }
        }
        Some(_) => None,
    };
    if let Some(c) = chosen {
        info!(
            slot = c.slot.display(),
            figure = %c.placement.name,
            load_file = %c.load_file,
            in_level = play.inside.is_some(),
            "the hero, once his model is ready"
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
                        .spawn(async move { read_hero_model(&shelf, &load, formats) }),
                )
            }
        }
        Model3d::Reading(task) => match check_ready(task) {
            None => return,
            Some(Err(why)) => Model3d::Failed(why),
            Some(Ok(read)) => match warm::keys(&read.loaded.model, &key_scene()) {
                Err(why) => Model3d::Failed(why.to_string()),
                Ok(keys) => {
                    let scene = key_scene();
                    if !warm.covers(keys.iter().map(|(k, _)| k)) {
                        warm.ask(&read.loaded.model, &keys, &scene);
                    }
                    info!(
                        load_file = %c.load_file,
                        keys = keys.len(),
                        read_ms = read.loaded.took.as_millis() as u64,
                        walk = read.clips.walk.as_ref().map(|c| c.name.as_str()),
                        run = read.clips.run.as_ref().map(|c| c.name.as_str()),
                        fall = read.clips.fall.as_ref().map(|c| c.name.as_str()),
                        "the hero's model read"
                    );
                    Model3d::Compiling {
                        model: Box::new(read),
                        keys: keys.into_iter().map(|(k, _)| k).collect(),
                    }
                }
            },
        },
        Model3d::Compiling { keys, .. } if warm.covers(keys.iter()) => {
            let Model3d::Compiling { model, .. } =
                std::mem::replace(&mut c.model, Model3d::Shelfless)
            else {
                unreachable!("matched as compiling")
            };
            Model3d::Ready(model)
        }
        _ => return,
    };
    if let Model3d::Failed(why) = &next {
        warn!(load_file = %c.load_file, reason = %why, "the hero's model can't be drawn; the portal view stays");
    }
    c.model = next;
}

/// The level gone from under the hero, or the TV off the portal view: the
/// hero, his camera and the pads' level layer go, the level hidden again,
/// the alert with them.
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
    if let Some(hero) = inside.hero {
        commands.entity(hero.hero).despawn();
    }
    commands.entity(inside.camera).despawn();
    pads.hide();
    play.candidate = None;
    play.alert = None;
}

/// Before the level has started, the TV would enter it now but for the
/// portal: the hero-to-be's model ready with its keys, no intro playing or
/// waiting, the start's pass done and the level ready with its solids. The
/// TV on the portal view is the caller's to check.
fn would_enter(
    play: &Play,
    level: &Level,
    prepare: Option<&Prepare>,
    queue: Option<&IntroQueue>,
) -> bool {
    let ready = play
        .candidate
        .as_ref()
        .is_some_and(|c| matches!(c.model, Model3d::Ready(_)));
    let preparing = prepare.is_some_and(|p| p.preparing());
    let intros = queue.is_some_and(|q| q.playing().is_some() || q.queued().next().is_some());
    ready && !preparing && !intros && level.walkable().is_some()
}

/// The portal's alert ([`judge`]) on the portal view and over the level, and
/// the level held ([`LevelPads::hold`]) while one shows or no hero is in it.
/// A replacement whose model can't be drawn counts as one that can't come
/// in, so the call for a Skylander stays up.
fn judge_portal(
    mut play: ResMut<Play>,
    cards: Res<PortalCards>,
    turntables: Res<Turntables>,
    level: Res<Level>,
    current: Option<Res<Current>>,
    (prepare, queue): (Option<Res<Prepare>>, Option<Res<IntroQueue>>),
    mut pads: ResMut<LevelPads>,
) {
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    let alert = match turntables.shelf().filter(|_| on_portal) {
        Some(shelf) => {
            let mut lineup = Lineup::of(&play.order, &cards, shelf);
            if let Some(c) = play.candidate.as_ref()
                && matches!(c.model, Model3d::Failed(_))
            {
                for (slot, walks) in &mut lineup.characters {
                    *walks &= *slot != c.slot;
                }
            }
            let hero = play.on_stage().and(play.candidate.as_ref()).map(|c| c.slot);
            let would_start = play.inside.is_none()
                && would_enter(&play, &level, prepare.as_deref(), queue.as_deref());
            judge(&lineup, play.inside.is_some(), hero, would_start)
        }
        None => None,
    };
    if play.alert != alert {
        info!(
            alert = alert.map(Alert::name),
            was = play.alert.map(Alert::name),
            "the portal's alert"
        );
        play.alert = alert;
    }
    let held = play
        .inside
        .as_ref()
        .is_some_and(|i| alert.is_some() || i.hero.is_none());
    if pads.held() != held {
        pads.hold(held);
    }
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

/// The hero into the level once every condition holds and the portal asks
/// for nothing: at the start, the TV entering the level with him (the
/// camera following, the pads the level's); or, the level held with no hero
/// in it, where the last one stood and the way he faced, the level carrying
/// on from where it held (level-scene's Removing the hero pauses the
/// level).
fn enter(
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut pads: ResMut<LevelPads>,
    seam: AvianSeam,
    current: Option<Res<Current>>,
    (prepare, queue): (Option<Res<Prepare>>, Option<Res<IntroQueue>>),
    tv: Query<Option<&RenderTarget>, With<TvCamera>>,
    mut s: EnterSpawner,
) {
    let replacing = match &play.inside {
        None => false,
        Some(inside) if inside.hero.is_none() => true,
        Some(_) => return,
    };
    let Some(Candidate {
        walker,
        model: Model3d::Ready(_),
        ..
    }) = play.candidate.as_ref()
    else {
        return;
    };
    let walker = *walker;
    if play.alert.is_some() {
        return;
    }
    let (at, facing, target) = if replacing {
        let Some((at, facing)) = play.inside.as_ref().and_then(|i| i.left) else {
            warn!("no place the last hero left: the level holds");
            return;
        };
        (at, facing, None)
    } else {
        let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
        if !on_portal || !would_enter(&play, &level, prepare.as_deref(), queue.as_deref()) {
            return;
        }
        let Ok(target) = tv.single() else {
            return;
        };
        let Some(start) = level.walkable().map(|l| l.start) else {
            return;
        };
        (
            start,
            START_FACING,
            Some(target.cloned().unwrap_or_default()),
        )
    };
    let Some((lights, fog)) = level.lighting().map(|(l, f)| (l.clone(), f)) else {
        return;
    };
    let Some(solid) = walker.solid() else {
        warn!(load_file = walker.load_file, "the walker has no solid");
        return;
    };
    let mut hero = Hero::new(solid, walker.tuning, at, facing);
    if replacing {
        // Where the last one stood is a spot a hero fits: a settle's cast
        // from a metre up could meet an overhang beside it first (the Mac
        // run's second replacement came in 0.97 m up).
        hero.stand(&seam, &[]);
    } else {
        hero.settle(&seam, &[]);
    }
    let feet = hero.pivot;
    let Some(c) = play.candidate.as_mut() else {
        unreachable!("matched above")
    };
    let Model3d::Ready(read) = std::mem::replace(&mut c.model, Model3d::Shelfless) else {
        unreachable!("matched as ready")
    };
    // In the world's frame, as the level's lights are.
    let scene = hero_scene(&lights, &fog, Mat4::IDENTITY);
    let hero_entity = spawn_hero(&mut s.commands, hero);
    let drawn = match draw_hero(&mut s, hero_entity, walker, &read.loaded, &scene) {
        Ok(drawn) => drawn,
        Err(why) => {
            warn!(load_file = walker.load_file, reason = %why, "the hero's model can't be drawn; he stays out");
            s.commands.entity(hero_entity).despawn();
            c.model = Model3d::Failed(why);
            return;
        }
    };
    let clips = read.clips.clone();
    let loops = clips.loops(&read.loaded.model);
    s.commands
        .entity(hero_entity)
        .insert(HeroGaits::new(Locomotion::new(walker.gaits, loops)));
    c.model = Model3d::Ready(read);
    let now = s.time.elapsed();
    let lit = drawn.lit.iter().map(|l| l.material.clone()).collect();
    level.light_also(drawn.lit, &mut s.materials);
    let on_stage = OnStage {
        hero: hero_entity,
        model: drawn.model,
        clips,
        joints: drawn.joints,
        attachments: drawn.attachments,
        scrolling: drawn.scrolling,
        lit,
        since: now,
    };
    s.redrawn.write(Redrawn::Level);
    let Some(target) = target else {
        info!(
            load_file = walker.load_file,
            at = ?at,
            feet = ?feet,
            facing,
            "a hero comes in where the last one stood"
        );
        if let Some(inside) = play.inside.as_mut() {
            inside.hero = Some(on_stage);
        }
        return;
    };
    let follow = FollowCamera {
        camera: GameCamera::new(CameraDef::LEVEL_213_START, feet),
        planes: Vec::new(),
    };
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
    level.show(&mut s.commands, camera, now);
    pads.show();
    info!(
        load_file = walker.load_file,
        start = ?at,
        feet = ?feet,
        facing,
        "the TV enters the level"
    );
    play.inside = Some(Inside {
        camera,
        hero: Some(on_stage),
        left: None,
    });
}

/// The cards' turntables draw nothing while the level covers the portal
/// view, and draw again once it's left (AG.55: Eruptor's card and its glow,
/// drawn behind the level, took 1.6 ms of every 7.8 ms frame of the render
/// world in the 1280x720 window): each card camera with its model up, and
/// its glow camera, its child. A card still compiling keeps its camera.
fn rest_the_cards(
    play: Res<Play>,
    turntables: Res<Turntables>,
    children: Query<&Children>,
    mut cameras: Query<&mut Camera>,
) {
    let draw = play.inside.is_none();
    for card in turntables.card_cameras() {
        let glows = children.get(card).into_iter().flat_map(|c| c.iter());
        for e in std::iter::once(card).chain(glows) {
            if let Ok(mut camera) = cameras.get_mut(e)
                && camera.is_active != draw
            {
                camera.is_active = draw;
            }
        }
    }
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

/// His pose (AG.30): his locomotion's clips at their weights and times,
/// eased between fixed steps as his transform is (each weight from the step
/// before's toward this one's, each clip's time back from this step's by
/// what's left of the step at its rate), so it holds while the level waits;
/// his scrolling parts and what he carries on the TV's virtual clock since
/// he came in. Under reduced motion (dev shots) his idle, his scrolls and
/// what he carries hold their start; his walk, run and fall still play, as
/// the stick's and not ambient motion, so a walk's shots show them.
fn pose_hero(
    play: Res<Play>,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    reduced: Option<Res<ReducedMotion>>,
    gaits: Query<&HeroGaits>,
    mut transforms: Query<&mut Transform>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let Some(inside) = play.on_stage() else {
        return;
    };
    let still = reduced.is_some_and(|r| r.0);
    let seconds = if still {
        0.0
    } else {
        time.elapsed().saturating_sub(inside.since).as_secs_f64()
    };
    let pose = match gaits.get(inside.hero).ok() {
        Some(g) => gait_pose(
            &inside.model,
            &inside.clips,
            g,
            (fixed.overstep_fraction(), fixed.timestep().as_secs_f64()),
            still,
        ),
        None => guacamole::character::mesh::pose(&inside.model, seconds),
    };
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

/// His joints `alpha` of the way from his locomotion's last step to its
/// next, a step `step` seconds long, each clip where [`drawn_clips`] puts
/// it.
fn gait_pose(
    model: &Model,
    clips: &HeroClips,
    gaits: &HeroGaits,
    (alpha, step): (f32, f64),
    held: bool,
) -> Vec<Transform> {
    let drawn = drawn_clips(gaits, (alpha, step), held);
    let playing: Vec<(&Clip, f64, f64)> = locomotion::Clip::ALL
        .iter()
        .zip(drawn)
        .filter_map(|(&gait, (at, weight))| {
            let clip = match gait {
                locomotion::Clip::Idle => Some(&model.idle),
                _ => clips.get(gait),
            }?;
            Some((clip, at, f64::from(weight)))
        })
        .collect();
    guacamole::character::mesh::blended(model, &playing)
}

/// Each clip of his locomotion as drawn `alpha` of the way from its last
/// step to its next, a step `step` seconds long, in
/// [`locomotion::Clip::ALL`]'s order: its time this step less what's left
/// of the step at its rate (from its restart, where it restarted), none
/// before 0, and its weight eased from the step before's; the idle at its
/// start when `held`.
pub(crate) fn drawn_clips(
    gaits: &HeroGaits,
    (alpha, step): (f32, f64),
    held: bool,
) -> [(f64, f32); 4] {
    let now = gaits.locomotion.weights();
    let back = f64::from(1.0 - alpha) * step;
    std::array::from_fn(|i| {
        let gait = locomotion::Clip::ALL[i];
        let weight = gaits.before[i] + (now[i] - gaits.before[i]) * alpha;
        let p = gaits.locomotion.play(gait);
        let at = if held && gait == locomotion::Clip::Idle {
            0.0
        } else {
            (p.time - back * f64::from(p.rate)).max(0.0)
        };
        (at, weight)
    })
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
    /// The made-up extract's: a character that doesn't walk, a magic item.
    const ONE: u32 = 1;
    const ITEM: u32 = 5;
    /// Added to the roster by [`play_app`].
    const SIDEKICK: u32 = 6;
    /// On no roster: a figure Giants can't use (a later game's).
    const FRYNO: u32 = 0x1c2;

    /// Where the made-up hero side holds the top joint along x in the walk,
    /// the run and the fall: a pose tells their weights. The idle sways it
    /// from 0 to 0.5 over its second.
    const WALK_X: f32 = 1.0;
    const RUN_X: f32 = 2.0;
    const FALL_X: f32 = 3.0;

    /// A made-up hero side for `load` on the turntables' made-up model's
    /// skeleton: its walk, run and falling clips Eruptor's lengths (1.0667,
    /// 0.6667 and 0.8 s), each holding the top joint at its own x.
    pub(crate) fn hero_side(load: &str) -> guac_ir::hero::Hero {
        use guac_ir::actor::{Movement, Shape, Solid};
        use guac_ir::clip::{RotationTrack, Track, VecTrack};
        use guac_ir::hero::{Locomotion, SlotClip};
        let slot = |slot: u16, name: &str, duration: f32, x: f32| {
            Some(SlotClip {
                slot,
                label: None,
                clip: Clip {
                    name: name.into(),
                    duration,
                    frame_duration: 1.0 / 30.0,
                    frames: (duration * 30.0).round() as u32 + 1,
                    tracks: vec![Track {
                        joint: 1,
                        translation: VecTrack {
                            constant: [x, 2.0, 0.0],
                            spline: None,
                        },
                        rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                        scale: VecTrack {
                            constant: [1.0; 3],
                            spline: None,
                        },
                    }],
                },
            })
        };
        guac_ir::hero::Hero {
            identity: crate::turntable::tests::identity(),
            load_file: load.into(),
            actor: load.into(),
            scale: [100.0; 3],
            solid: Solid {
                shape: Shape::Sphere,
                solid: true,
                world: true,
                actor_surface: false,
                offset: [0.0; 3],
                rotation: [0.0; 3],
                width: 4.0,
                height: 6.0,
                length: 3.0,
                weight: 10.0,
                rebound: 0.0,
            },
            movement: Movement {
                ground_max_speed: 18.0,
                ground_acceleration: 160.0,
                ground_deceleration: 80.0,
                ground_turn_rate: 360.0,
                air_max_speed: 30.0,
                air_acceleration: 160.0,
                air_deceleration: 20.0,
                air_turn_rate: 360.0,
                gravity_max_speed: 120.0,
                gravity_acceleration: 128.0,
                jump_max_speed: 0.0,
                jump_acceleration: 0.0,
                jump_initial_speed: 0.0,
                motion_type: 0,
                tilts_with_ground: true,
                time_scale: 1.0,
            },
            locomotion: Locomotion {
                walk: slot(1, "MadeUp_Walk.ma", 1.0667, WALK_X),
                jog: None,
                run: slot(3, "MadeUp_Run.ma", 0.6667, RUN_X),
                falling: slot(57, "MadeUp_Falling.ma", 0.8, FALL_X),
                landing: None,
            },
        }
    }

    /// The turntables' made-up extract with Eruptor's load file on toy 9,
    /// a made-up model and hero side of its own, and a chapter 1 on
    /// [`write_level`]'s walk; the TV on the portal view.
    fn play_app(dir: &std::path::Path) -> (App, crate::level::LevelSender) {
        let mut shelf = extract(dir);
        let ron = guac_ir::to_ron(&model("009_Eruptor")).unwrap();
        std::fs::write(dir.join(IrFormat::Ron.model_path("009_Eruptor")), ron).unwrap();
        let hero = IrFormat::Ron.hero_path("009_Eruptor");
        std::fs::create_dir_all(dir.join(&hero).parent().unwrap()).unwrap();
        std::fs::write(
            dir.join(hero),
            guac_ir::to_ron(&hero_side("009_Eruptor")).unwrap(),
        )
        .unwrap();
        shelf
            .loads
            .insert(format!("{ERUPTOR:06x}-0000"), "009_Eruptor".into());
        // On the roster beside the made-up extract's: Eruptor, and a
        // sidekick, which Giants counts as a magic item.
        shelf
            .kinds
            .insert(format!("{ERUPTOR:06x}-0000"), Category::Figure);
        shelf
            .kinds
            .insert(format!("{SIDEKICK:06x}-0000"), Category::Sidekick);
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
        app.world_mut().spawn((Camera2d, TvCamera));
        let (tx, feed) = level_channel();
        app.insert_resource(feed);
        (app, tx)
    }

    fn place(app: &mut App, slot: u8, toy: u32, name: &str) {
        let change = placed(slot, toy, name);
        crate::turntable::tests::set(app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
    }

    fn clear(app: &mut App, slot: u8) {
        let change = PortalChange::Cleared {
            slot: SlotIndex::new(slot).unwrap(),
        };
        crate::turntable::tests::set(app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
    }

    fn alert(app: &App) -> Option<Alert> {
        app.world().resource::<Play>().alert()
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
    /// level's replacing write, under the TV's UI (AG.50), the pads go to the
    /// level's layer, and the first pad walks him against the camera's view
    /// until the wall stops him.
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
        // The TV's UI over it: its camera after the level's on one target,
        // laid over it, so Start's overlay and the controller message show.
        let level_target = entity.get::<RenderTarget>().cloned();
        let c = c.clone();
        let mut tv = app
            .world_mut()
            .query_filtered::<(&Camera, Option<&RenderTarget>), With<TvCamera>>();
        let (tv, tv_target) = tv.single(app.world()).unwrap();
        assert!(c.order < tv.order, "{} under {}", c.order, tv.order);
        assert!(crate::level::draws_over(
            (tv, tv_target),
            (&c, level_target.as_ref())
        ));

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

    /// level-scene's The first character placed walks into the level and
    /// Two characters before the start: of two figures that walk placed in
    /// one frame, the one placed first is the hero-to-be, not the one in the
    /// lower slot nor the one placed last; two on the portal hold the portal
    /// view under the too-many alert instead of the level, and once the
    /// other comes off he walks in.
    #[test]
    fn the_first_placed_of_two_walkers_is_the_hero() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 2, ERUPTOR, "Eruptor");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the alert", |a| {
            alert(a) == Some(Alert::TooManySkylanders)
        });
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal, "held on the portal view");
        }
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 2, "the first placed");
        clear(&mut app, 0);
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        assert_eq!(alert(&app), None);
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 2);
        assert_eq!(
            (hero(&app).pivot.x, hero(&app).pivot.z),
            (START[0], START[2])
        );
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

    /// His idle loops on the TV's clock, and holds its start under reduced
    /// motion; walking, his gaits play under it all the same.
    #[test]
    fn under_reduced_motion_his_idle_holds_its_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        let joints = |app: &App| -> Vec<Transform> {
            let inside = app.world().resource::<Play>().on_stage().unwrap();
            inside
                .joints
                .iter()
                .map(|j| *app.world().entity(*j).get::<Transform>().unwrap())
                .collect()
        };
        let model = app
            .world()
            .resource::<Play>()
            .on_stage()
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
        // Walking, his gaits play: the stick's, not ambient motion.
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..40 {
            app.update();
        }
        assert_eq!(gaits(&app).top(), locomotion::Clip::Run);
        assert!((top_x(&app) - RUN_X).abs() < 1e-5, "{}", top_x(&app));
        // Let go and at rest again, his idle's start.
        test_pad::stick(&mut app, pad, 0.0, 0.0);
        for _ in 0..40 {
            app.update();
        }
        assert_eq!(joints(&app), at_start, "his idle's start again");
    }

    fn gaits(app: &App) -> Locomotion {
        let e = app.world().resource::<Play>().hero().expect("in the level");
        let g = app.world().entity(e).get::<HeroGaits>().expect("his gaits");
        g.locomotion.clone()
    }

    /// The top joint's x as drawn: the weights times each clip's x, the
    /// idle's sway its own.
    fn top_x(app: &App) -> f32 {
        let inside = app.world().resource::<Play>().on_stage().unwrap();
        let joint = inside.joints[1];
        app.world()
            .entity(joint)
            .get::<Transform>()
            .unwrap()
            .translation
            .x
    }

    /// The idle's sway at `t` seconds: 0 to 0.5 over its looped second.
    fn idle_x(t: f64) -> f32 {
        let frame = (t.rem_euclid(1.0) * 30.0).min(30.0);
        (0.5 * frame / 30.0) as f32
    }

    /// AG.30 on the TV: standing he plays his idle alone; the stick full on
    /// takes him off it over 0.25 s (15 steps) while the walk and then the
    /// run take over, the run joining the fifth step he moves and alone
    /// from the eighth, playing at 102.4 % as he runs; let go, he's back in
    /// his idle whole 35 steps on, his walk keeping its last rate as it
    /// fades. Each step's weights pose his model: at a step's end the drawn
    /// top joint is the step before's weights on each clip (the drawn pose
    /// eases a step behind, as his transform does).
    #[test]
    fn his_locomotion_takes_him_from_his_idle_to_his_run_and_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        for _ in 0..30 {
            app.update();
        }
        let g = gaits(&app);
        assert_eq!(g.weights(), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(g.top(), locomotion::Clip::Idle);
        let idle_t = g.play(locomotion::Clip::Idle).time - 1.0 / 60.0;
        assert!(
            (top_x(&app) - idle_x(idle_t)).abs() < 1e-5,
            "{}",
            top_x(&app)
        );

        test_pad::stick(&mut app, pad, 0.0, 1.0);
        let mut steps = Vec::new();
        for _ in 0..40 {
            app.update();
            let g = gaits(&app);
            steps.push((g.weights(), g.speed(), top_x(&app)));
        }
        let moved = steps.iter().position(|s| s.1 > 0.0).unwrap();
        let joins = steps.iter().position(|s| s.0[2] > 0.0).unwrap();
        let alone = steps
            .iter()
            .position(|s| s.0[1] == 0.0 && s.0[2] > 0.0)
            .unwrap();
        let idle_out = steps.iter().position(|s| s.0[0] == 0.0).unwrap();
        eprintln!("moved {moved}, the run joins {joins}, alone {alone}, the idle out {idle_out}");
        assert_eq!((joins - moved, alone - moved, idle_out - moved), (4, 7, 15));
        let g = gaits(&app);
        assert_eq!(g.top(), locomotion::Clip::Run);
        assert!((g.speed() - 21.5).abs() < 1e-4, "{}", g.speed());
        assert!((g.play(locomotion::Clip::Run).rate - 1.0237).abs() < 1e-3);
        assert!((top_x(&app) - RUN_X).abs() < 1e-5, "{}", top_x(&app));
        // Mid set-off the drawn pose is the step before's weights.
        let (w, _, x) = steps[moved + 6];
        let (before, ..) = steps[moved + 5];
        let walk_or_run = before[1] * WALK_X + before[2] * RUN_X;
        assert!(w[0] > 0.0 && w[2] > 0.0, "{w:?}");
        assert!(
            x > walk_or_run && x < walk_or_run + 0.5 * before[0],
            "{x} {before:?}"
        );

        test_pad::stick(&mut app, pad, 0.0, 0.0);
        let mut back = Vec::new();
        for _ in 0..40 {
            app.update();
            let g = gaits(&app);
            back.push((g.weights(), g.play(locomotion::Clip::Walk).rate));
        }
        let whole = back.iter().position(|s| s.0[0] == 1.0).unwrap();
        let all_walk = back.iter().position(|s| s.0[2] == 0.0).unwrap();
        eprintln!("let go: all walk {all_walk}, the idle whole {whole} steps on");
        assert_eq!(all_walk, 8);
        assert_eq!(whole, 35);
        let rest = back.iter().position(|s| s.1 == back[whole].1).unwrap();
        assert!(back[rest..=whole].iter().all(|s| s.1 == back[rest].1));
        app.update();
        assert_eq!(gaits(&app).top(), locomotion::Clip::Idle);
    }

    /// His pose between steps, at 144 Hz: each weight eased from the step
    /// before's toward this one's by how far the display is into the next
    /// step, each clip back from its time by what's left of the step.
    #[test]
    fn his_pose_eases_between_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 144.0,
        )));
        for _ in 0..10 {
            app.update();
        }
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        let mut between = 0;
        for _ in 0..30 {
            app.update();
            let alpha = app.world().resource::<Time<Fixed>>().overstep_fraction();
            let step = app
                .world()
                .resource::<Time<Fixed>>()
                .timestep()
                .as_secs_f64();
            let e = app.world().resource::<Play>().hero().unwrap();
            let g = app.world().entity(e).get::<HeroGaits>().unwrap();
            let now = g.locomotion.weights();
            let w: Vec<f32> = (0..4)
                .map(|i| g.before[i] + (now[i] - g.before[i]) * alpha)
                .collect();
            let idle = g.locomotion.play(locomotion::Clip::Idle);
            let t = (idle.time - f64::from(1.0 - alpha) * step * f64::from(idle.rate)).max(0.0);
            let want = w[0] * idle_x(t) + w[1] * WALK_X + w[2] * RUN_X + w[3] * FALL_X;
            assert!(
                (top_x(&app) - want).abs() < 1e-4,
                "{} {want} {w:?}",
                top_x(&app)
            );
            if alpha > 0.05 && alpha < 0.95 && g.before != now {
                between += 1;
            }
        }
        eprintln!("{between} display frames between steps with the weights moving");
        assert!(between >= 5, "{between}");
    }

    /// AG.55: at 144 and at 60 frames a second over the 60 Hz step, every
    /// drawn frame of a run moves his drawn transform and the camera's by
    /// the frame's share of the walk (no frame repeating the one before, none
    /// a double step) and his run's drawn time by the frame times its rate.
    /// His joints aren't read here (the made-up run holds one pose): his
    /// pose's easing is `his_pose_eases_between_steps`'.
    #[test]
    fn every_drawn_frame_advances_him_the_camera_and_his_run_at_144_and_60() {
        for rate in [144.0, 60.0] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, tx) = play_app(tmp.path());
            tx.send(write_level(tmp.path(), &lights())).unwrap();
            let pad = test_pad::connect(&mut app, None, None);
            place(&mut app, 0, ERUPTOR, "Eruptor");
            until(&mut app, "the level", |a| mode(a) == Mode::Level);
            let frame = 1.0 / rate;
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                frame,
            )));
            test_pad::stick(&mut app, pad, 0.0, 1.0);
            // Into the run alone: his idle out 15 steps after he sets off.
            for _ in 0..(40.0 * rate / 60.0) as usize {
                app.update();
            }
            let drawn = |app: &App| {
                let (hero, camera, _) = app.world().resource::<Play>().drawn().unwrap();
                let t = |e: Entity| {
                    app.world()
                        .entity(e)
                        .get::<Transform>()
                        .unwrap()
                        .translation
                };
                let fixed = app.world().resource::<Time<Fixed>>();
                let alpha = fixed.overstep_fraction();
                let step = fixed.timestep().as_secs_f64();
                let g = app.world().entity(hero).get::<HeroGaits>().unwrap();
                let run = drawn_clips(g, (alpha, step), false)[2];
                let speed = g.locomotion.speed();
                let rate = g.locomotion.play(locomotion::Clip::Run).rate;
                (t(hero), t(camera), run, speed, rate)
            };
            let mut before = drawn(&app);
            assert_eq!(gaits(&app).top(), locomotion::Clip::Run, "{rate} Hz");
            for i in 0..(rate as usize) {
                app.update();
                let now = drawn(&app);
                // 21.5 ft/s, metres, over the frame.
                let share = f64::from(now.3) * 0.3048 * frame;
                let moved = f64::from(now.0.distance(before.0));
                let looked = f64::from(now.1.distance(before.1));
                assert!(
                    moved > 0.5 * share && moved < 1.5 * share,
                    "{rate} Hz frame {i}: he moved {moved} m against {share}"
                );
                assert!(
                    looked > 0.5 * share && looked < 1.5 * share,
                    "{rate} Hz frame {i}: the camera moved {looked} m against {share}"
                );
                assert_eq!(now.2.1, 1.0, "{rate} Hz frame {i}: the run alone");
                let ran = now.2.0 - before.2.0;
                let want = frame * f64::from(now.4);
                assert!(
                    (ran - want).abs() < 1e-6,
                    "{rate} Hz frame {i}: the run drew {ran} s on, not {want}"
                );
                before = now;
            }
        }
    }

    /// AG.55: his card's turntable and its glow camera draw on the portal
    /// view, rest while he's inside and draw again when the level's left.
    #[test]
    fn the_cards_rest_behind_the_level() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        let active = |app: &mut App| -> Vec<bool> {
            let cards: Vec<Entity> = app
                .world()
                .resource::<Turntables>()
                .card_cameras()
                .collect();
            let mut out = Vec::new();
            for card in cards {
                let e = app.world().entity(card);
                out.push(e.get::<Camera>().unwrap().is_active);
                for glow in e.get::<Children>().into_iter().flat_map(|c| c.iter()) {
                    if let Some(c) = app.world().entity(glow).get::<Camera>() {
                        out.push(c.is_active);
                    }
                }
            }
            out
        };
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his card's model", |a| {
            a.world().resource::<Turntables>().card_state(0)
                == Some(crate::turntable::CardState::Model)
        });
        assert_eq!(active(&mut app), [true, true], "the card and its glow draw");
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        app.update();
        assert_eq!(active(&mut app), [false, false], "behind the level");
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(active(&mut app), [true, true], "the level left");
    }

    /// Without his hero side (a hand-made extract, or one cut short) he has
    /// no clips to walk in: the portal view holds and says why.
    #[test]
    fn without_his_hero_side_the_portal_view_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        std::fs::remove_file(tmp.path().join(IrFormat::Ron.hero_path("009_Eruptor"))).unwrap();
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("failed"));
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        let Model3d::Failed(why) = &c.model else {
            unreachable!("failed")
        };
        assert!(why.starts_with("his hero side: 009_Eruptor.ron:"), "{why}");
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal);
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
    /// placed first, keeps the portal view; Eruptor placed beside it holds
    /// the portal view under the too-many alert (two characters when the
    /// level would start), and walks in once it comes off. The TV leaving
    /// the portal view leaves the level, hidden again.
    #[test]
    fn another_character_keeps_the_portal_view_and_leaving_the_portal_leaves_the_level() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ONE, "One");
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(alert(&app), None, "no level would start");
        assert!(app.world().resource::<Play>().candidate.is_none());
        place(&mut app, 1, ERUPTOR, "Eruptor");
        until(&mut app, "the alert", |a| {
            alert(a) == Some(Alert::TooManySkylanders)
        });
        assert_eq!(mode(&app), Mode::Portal);
        clear(&mut app, 0);
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 1);
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(alert(&app), None);
        assert_eq!(level_state(&app), "ready", "hidden, ready to show again");
        assert!(!app.world().resource::<LevelPads>().shows());
        assert!(!app.world().resource::<LevelPads>().held());
        let mut heroes = app.world_mut().query::<&HeroBody>();
        assert_eq!(heroes.iter(app.world()).count(), 0);
        let mut cameras = app.world_mut().query::<&FollowCamera>();
        assert_eq!(cameras.iter(app.world()).count(), 0);
    }

    // ---- AG.29, AG.63, AG.73: the portal and the level -------------------

    fn slot(n: u8) -> SlotIndex {
        SlotIndex::new(n).unwrap()
    }

    /// A lineup of `characters` (slot, walks), `magic` items and
    /// `unusable` toys.
    fn lineup(characters: &[(u8, bool)], magic: usize, unusable: usize) -> Lineup {
        Lineup {
            characters: characters.iter().map(|(s, w)| (slot(*s), *w)).collect(),
            magic,
            unusable,
        }
    }

    /// The roster's categories as the flow reads them: figures and Giants
    /// are characters, items and sidekicks magic items, an adventure piece
    /// a pack, and a figure the roster doesn't hold one Giants can't use.
    #[test]
    fn the_roster_says_what_each_figure_is() {
        let tmp = tempfile::tempdir().unwrap();
        let mut shelf = crate::turntable::tests::extract(tmp.path());
        shelf.kinds.insert("000006-0000".into(), Category::Sidekick);
        shelf
            .kinds
            .insert("000007-0000".into(), Category::AdventurePack);
        for (id, want) in [
            ("000001-0000", Kind::Character),
            ("000002-0000", Kind::Character),
            ("000005-0000", Kind::MagicItem),
            ("000006-0000", Kind::MagicItem),
            ("000007-0000", Kind::Pack),
            ("0001c2-0000", Kind::Unusable),
        ] {
            assert_eq!(kind(&shelf, id), want, "{id}");
        }
    }

    /// AG.63's hold, ruled: in the level a second character beside the hero
    /// holds it, and so do two with the hero off; one off ends it; the hero
    /// off with one that walks left lets him come in, with one that doesn't
    /// (AG.27) or none it calls for a Skylander.
    #[test]
    fn a_second_character_holds_and_one_off_ends_it() {
        let hero = Some(slot(0));
        assert_eq!(judge(&lineup(&[(0, true)], 0, 0), true, hero, false), None);
        for second in [true, false] {
            assert_eq!(
                judge(&lineup(&[(0, true), (3, second)], 0, 0), true, hero, false),
                Some(Alert::TooManySkylanders),
                "walks: {second}"
            );
        }
        // The hero off, the other in.
        assert_eq!(judge(&lineup(&[(3, true)], 0, 0), true, None, false), None);
        assert_eq!(
            judge(&lineup(&[(3, true), (5, true)], 0, 0), true, None, false),
            Some(Alert::TooManySkylanders)
        );
        assert_eq!(
            judge(&lineup(&[(3, false)], 0, 0), true, None, false),
            Some(Alert::PlaceSkylander)
        );
        assert_eq!(
            judge(&lineup(&[], 0, 0), true, None, false),
            Some(Alert::PlaceSkylander)
        );
    }

    /// Items, adventure-pack pieces and sidekicks never hold the level for
    /// too many Skylanders and never answer the call for one; a second
    /// magic item (a sidekick counted as one) holds it under its own alert.
    #[test]
    fn items_packs_and_sidekicks_never_hold_it_for_too_many_skylanders() {
        let hero = Some(slot(0));
        assert_eq!(judge(&lineup(&[(0, true)], 1, 0), true, hero, false), None);
        assert_eq!(
            judge(&lineup(&[(0, true)], 2, 0), true, hero, false),
            Some(Alert::TooManyItems)
        );
        assert_eq!(
            judge(&lineup(&[], 1, 0), true, None, false),
            Some(Alert::PlaceSkylander)
        );
        // Before the start, items beside a character change nothing.
        assert_eq!(judge(&lineup(&[(0, true)], 2, 0), false, None, true), None);
        // Too many of both: too many Skylanders first (our order: AG.6 never
        // put both on at once).
        assert_eq!(
            judge(&lineup(&[(0, true), (1, true)], 2, 0), true, hero, false),
            Some(Alert::TooManySkylanders)
        );
    }

    /// Two characters hold the start only once the level would show.
    #[test]
    fn two_before_the_start_hold_once_the_level_would_start() {
        let two = lineup(&[(0, true), (1, false)], 0, 0);
        assert_eq!(judge(&two, false, None, false), None);
        assert_eq!(
            judge(&two, false, None, true),
            Some(Alert::TooManySkylanders)
        );
        assert_eq!(judge(&lineup(&[(0, true)], 0, 0), false, None, true), None);
    }

    /// AG.73 ruled: a toy Giants can't use holds whatever shows, before the
    /// start or in the level, before every other alert.
    #[test]
    fn a_toy_giants_cant_use_comes_first() {
        let hero = Some(slot(0));
        for (l, inside, hero, start) in [
            (lineup(&[], 0, 1), false, None, false),
            (lineup(&[(0, true)], 0, 1), false, None, true),
            (lineup(&[(0, true)], 0, 1), true, hero, false),
            (lineup(&[(0, true), (1, true)], 2, 1), true, hero, false),
            (lineup(&[], 0, 1), true, None, false),
        ] {
            assert_eq!(
                judge(&l, inside, hero, start),
                Some(Alert::WrongToy),
                "{l:?}"
            );
        }
    }

    /// Eruptor walking the level, the first pad pushed forward: the app, the
    /// pad and his pivot.
    fn walking(tmp: &std::path::Path) -> (App, Entity) {
        let (mut app, tx) = play_app(tmp);
        tx.send(write_level(tmp, &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) == Mode::Level);
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..20 {
            app.update();
        }
        (app, pad)
    }

    /// The level stands still: from the frame after the hold began (that
    /// frame still runs the steps already due, as a pause's does, AG.20)
    /// and over `frames` frames, his pivot and the virtual clock don't move,
    /// the stick held as it is.
    fn stands_still(app: &mut App, frames: usize) -> Vec3 {
        app.update();
        let at = hero(app).pivot;
        let clock = app.world().resource::<Time<Virtual>>().elapsed();
        for _ in 0..frames {
            app.update();
            assert_eq!(hero(app).pivot, at, "he moved under the hold");
        }
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(app.world().resource::<Time<Virtual>>().elapsed(), clock);
        at
    }

    /// AG.63: a second character placed beside the walking hero holds the
    /// level under the too-many alert, him and the clock still, the stick
    /// still pushed; once it comes off he carries on from where he stopped.
    /// No intro is asked for (the level has started).
    #[test]
    fn a_second_character_holds_the_level_until_it_comes_off() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = walking(tmp.path());
        place(&mut app, 3, ONE, "One");
        until(&mut app, "the hold", |a| mode(a) == Mode::Held);
        assert_eq!(alert(&app), Some(Alert::TooManySkylanders));
        assert!(app.world().resource::<Play>().started());
        let at = stands_still(&mut app, 30);
        clear(&mut app, 3);
        until(&mut app, "the walk again", |a| mode(a) == Mode::Level);
        assert_eq!(alert(&app), None);
        for _ in 0..10 {
            app.update();
        }
        let on = hero(&app).pivot;
        assert!(on.distance(at) > 0.1, "he walks on: {at} -> {on}");
        assert!(on.distance(at) < 3.0, "from where he stopped: {at} -> {on}");
    }

    /// AG.63: with a second Eruptor holding the level, the hero taken off
    /// brings the other in where he stood and the way he faced, the camera
    /// the same, and the level carries on.
    #[test]
    fn the_hero_off_brings_the_other_in_where_he_stood() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walking(tmp.path());
        // A turn first, so his facing isn't the start's.
        test_pad::stick(&mut app, pad, 1.0, 0.0);
        for _ in 0..20 {
            app.update();
        }
        let camera = app
            .world()
            .resource::<Play>()
            .inside
            .as_ref()
            .unwrap()
            .camera;
        place(&mut app, 4, ERUPTOR, "Eruptor");
        until(&mut app, "the hold", |a| {
            alert(a) == Some(Alert::TooManySkylanders)
        });
        stands_still(&mut app, 5);
        let left = hero(&app);
        clear(&mut app, 0);
        until(&mut app, "the other in", |a| {
            a.world().resource::<Play>().hero().is_some()
                && a.world()
                    .resource::<Play>()
                    .candidate
                    .as_ref()
                    .is_some_and(|c| c.slot.as_u8() == 4)
        });
        let now = hero(&app);
        eprintln!(
            "left at {} facing {}, in at {} facing {}",
            left.pivot, left.facing, now.pivot, now.facing
        );
        assert!(now.pivot.distance(left.pivot) < 1e-3, "{}", now.pivot);
        assert_eq!(now.facing, left.facing);
        assert_ne!(left.facing, START_FACING);
        let inside = app.world().resource::<Play>().inside.as_ref().unwrap();
        assert_eq!(inside.camera, camera, "the same camera");
        until(&mut app, "the walk again", |a| mode(a) == Mode::Level);
        assert_eq!(alert(&app), None);
        let mut heroes = app.world_mut().query::<&HeroBody>();
        assert_eq!(heroes.iter(app.world()).count(), 1, "one hero");
    }

    /// AG.29: the hero taken off with nothing else on the portal holds the
    /// level behind the call for a Skylander, no hero in it; an item placed
    /// then never answers it, nor a character that doesn't walk yet; the
    /// next one that walks comes in where he stood, facing his way, an
    /// overhang beside the spot notwithstanding, and the level carries on
    /// from where it held.
    #[test]
    fn removing_the_hero_holds_the_level_until_the_next_comes_in() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = walking(tmp.path());
        let camera = app
            .world()
            .resource::<Play>()
            .inside
            .as_ref()
            .unwrap()
            .camera;
        let level_lit = app
            .world()
            .resource::<Level>()
            .loaded()
            .unwrap()
            .also_count();
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        assert_eq!(mode(&app), Mode::Held);
        assert!(app.world().resource::<Play>().hero().is_none());
        let mut heroes = app.world_mut().query::<&HeroBody>();
        assert_eq!(heroes.iter(app.world()).count(), 0, "he's gone");
        assert_eq!(
            app.world()
                .resource::<Level>()
                .loaded()
                .unwrap()
                .also_count(),
            0,
            "his {level_lit} materials no longer lit with the level"
        );
        assert!(
            app.world().resource::<Level>().report().shown,
            "still shown"
        );
        let left = app
            .world()
            .resource::<Play>()
            .inside
            .as_ref()
            .unwrap()
            .left
            .unwrap();
        app.update();
        let clock = app.world().resource::<Time<Virtual>>().elapsed();
        place(&mut app, 5, ITEM, "Five");
        for _ in 0..30 {
            app.update();
            assert_eq!(
                alert(&app),
                Some(Alert::PlaceSkylander),
                "an item isn't a hero"
            );
        }
        assert_eq!(
            app.world().resource::<Time<Virtual>>().elapsed(),
            clock,
            "held"
        );
        // A character that doesn't walk yet (AG.27) can't answer it either.
        place(&mut app, 3, ONE, "One");
        for _ in 0..30 {
            app.update();
            assert_eq!(alert(&app), Some(Alert::PlaceSkylander), "One doesn't walk");
            assert!(app.world().resource::<Play>().hero().is_none());
        }
        clear(&mut app, 3);
        // An overhang beside his spot, clear of a hero standing there and in
        // reach of one dropped from a metre up: he's put down, not dropped.
        let mut overhang = guac_motion::scenarios::Soup::default();
        let (x, y, z) = (left.0.x, left.0.y + 1.845, left.0.z + 0.5);
        overhang.quad(
            [
                Vec3::new(x - 2.0, y, z),
                Vec3::new(x + 2.0, y, z),
                Vec3::new(x + 2.0, y, z + 2.5),
                Vec3::new(x - 2.0, y, z + 2.5),
            ],
            Vec3::NEG_Y,
        );
        let body = guac_motion::seam::Body {
            id: guac_motion::seam::BodyId(0xa629),
            kind: guac_motion::seam::BodyKind::Static,
            pose: guac_motion::seam::Pose::IDENTITY,
            collider: guac_motion::seam::Collider::Mesh(Arc::new(overhang.mesh())),
        };
        let mut commands = app.world_mut().commands();
        crate::physics::spawn_body(&mut commands, &body).unwrap();
        app.world_mut().flush();
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "the next one in", |a| mode(a) == Mode::Level);
        let now = hero(&app);
        assert!(
            now.pivot.distance(left.0) < 1e-3,
            "{} against {}",
            now.pivot,
            left.0
        );
        assert_eq!(now.facing, left.1);
        let inside = app.world().resource::<Play>().inside.as_ref().unwrap();
        assert_eq!(inside.camera, camera, "the level carries on");
        assert_eq!(
            app.world()
                .resource::<Level>()
                .loaded()
                .unwrap()
                .also_count(),
            level_lit,
            "his materials lit with the level"
        );
    }

    /// AG.29: a replacement whose model can't be drawn (his hero side gone
    /// from the extract) can't come in, so the call for a Skylander stays up
    /// rather than a hold with nothing on it.
    #[test]
    fn a_replacement_that_cant_be_drawn_leaves_the_call_up() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = walking(tmp.path());
        std::fs::remove_file(tmp.path().join(IrFormat::Ron.hero_path("009_Eruptor"))).unwrap();
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("failed"));
        for _ in 0..30 {
            app.update();
            assert_eq!(alert(&app), Some(Alert::PlaceSkylander));
            assert_eq!(mode(&app), Mode::Held);
        }
    }

    /// AG.29: a figure placed over the hero's own slot takes him off as a
    /// removal does: he leaves where he stood, and with only a character
    /// that doesn't walk yet on the portal the level calls for a Skylander.
    #[test]
    fn a_figure_placed_over_the_heros_slot_takes_him_off() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = walking(tmp.path());
        place(&mut app, 0, ONE, "One");
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        assert_eq!(mode(&app), Mode::Held);
        assert!(app.world().resource::<Play>().hero().is_none());
        let mut heroes = app.world_mut().query::<&HeroBody>();
        assert_eq!(heroes.iter(app.world()).count(), 0, "he's gone");
        assert!(
            app.world()
                .resource::<Play>()
                .inside
                .as_ref()
                .unwrap()
                .left
                .is_some(),
            "where he stood kept"
        );
    }

    /// AG.29: every figure off holds the level; Start does nothing under the
    /// call; the TV stays in the level.
    #[test]
    fn remove_all_holds_and_start_does_nothing_under_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walking(tmp.path());
        place(&mut app, 5, ITEM, "Five");
        for _ in 0..5 {
            app.update();
        }
        clear(&mut app, 0);
        clear(&mut app, 5);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        test_pad::press(&mut app, pad, GamepadButton::Start);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(mode(&app), Mode::Held);
        let seat = app.world().resource::<LevelPads>().seat().unwrap().pause();
        assert_eq!(seat, None, "Start under the hold paused nothing");
        assert!(app.world().resource::<Level>().report().shown);
    }

    /// AG.29: an item placed first never starts the level or becomes the
    /// hero; Eruptor after it does, beside it.
    #[test]
    fn an_item_first_never_starts_the_level() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ITEM, "Five");
        place(&mut app, 1, SIDEKICK, "Six");
        for _ in 0..60 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal);
            assert_eq!(alert(&app), None);
        }
        assert!(app.world().resource::<Play>().candidate.is_none());
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        let c = app.world().resource::<Play>().candidate.as_ref().unwrap();
        assert_eq!(c.slot.as_u8(), 2);
    }

    /// AG.29: an item placed mid-level changes nothing: no alert, he walks
    /// on; a sidekick beside it is a second magic item and holds the level
    /// under the too-many-items alert until one comes off.
    #[test]
    fn an_item_mid_level_changes_nothing_and_a_second_holds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = walking(tmp.path());
        place(&mut app, 5, ITEM, "Five");
        let from = hero(&app).pivot;
        for _ in 0..20 {
            app.update();
            assert_eq!(mode(&app), Mode::Level);
            assert_eq!(alert(&app), None);
        }
        assert!(hero(&app).pivot.distance(from) > 0.1, "he walks on");
        place(&mut app, 6, SIDEKICK, "Six");
        until(&mut app, "the hold", |a| {
            alert(a) == Some(Alert::TooManyItems)
        });
        assert_eq!(mode(&app), Mode::Held);
        stands_still(&mut app, 20);
        clear(&mut app, 5);
        until(&mut app, "the walk again", |a| mode(a) == Mode::Level);
    }

    /// AG.73: a toy Giants can't use, placed mid-level, holds it under the
    /// wrong-toy alert, him unmoved and the pad's buttons doing nothing;
    /// taken off, he walks on.
    #[test]
    fn a_toy_giants_cant_use_holds_the_level_until_it_comes_off() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walking(tmp.path());
        place(&mut app, 7, FRYNO, "Fryno");
        until(&mut app, "the alert", |a| alert(a) == Some(Alert::WrongToy));
        assert_eq!(mode(&app), Mode::Held);
        for button in [GamepadButton::Start, GamepadButton::South] {
            test_pad::press(&mut app, pad, button);
        }
        let at = stands_still(&mut app, 30);
        assert_eq!(
            app.world().resource::<LevelPads>().seat().unwrap().pause(),
            None
        );
        clear(&mut app, 7);
        until(&mut app, "the walk again", |a| mode(a) == Mode::Level);
        for _ in 0..10 {
            app.update();
        }
        assert!(hero(&app).pivot.distance(at) > 0.1, "he walks on");
    }

    /// AG.73 at the start: the toy holds the portal view under its alert,
    /// Eruptor placed beside it doesn't enter while it's there, and he does
    /// once it's off.
    #[test]
    fn a_toy_giants_cant_use_holds_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, FRYNO, "Fryno");
        until(&mut app, "the alert", |a| alert(a) == Some(Alert::WrongToy));
        place(&mut app, 1, ERUPTOR, "Eruptor");
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        for _ in 0..30 {
            app.update();
            assert_eq!(mode(&app), Mode::Portal, "held on the portal view");
            assert_eq!(alert(&app), Some(Alert::WrongToy));
        }
        clear(&mut app, 0);
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        assert_eq!(alert(&app), None);
    }

    /// AG.29, intros: two characters before the start both play their intros
    /// in order and only then does the too-many alert show, the level not
    /// entered; once one comes off the other walks in.
    #[test]
    fn two_before_the_start_wait_for_their_intros_then_hold() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        place(&mut app, 1, ONE, "One");
        for (slot, toy, movie) in [(0, ERUPTOR, "eruptor"), (1, ONE, "one")] {
            app.world_mut()
                .resource_mut::<IntroQueue>()
                .apply(crate::intro::IntroEvent::Placed {
                    slot: SlotIndex::new(slot).unwrap(),
                    figure_id: format!("{toy:06x}-0000"),
                    movie: Some(movie.into()),
                });
        }
        until(&mut app, "his model", |a| hero_model(a) == Some("ready"));
        for skip in 0..2 {
            for _ in 0..20 {
                app.update();
                assert_eq!(alert(&app), None, "intro {skip} plays");
            }
            app.world_mut()
                .resource_mut::<IntroQueue>()
                .apply(crate::intro::IntroEvent::Skip);
        }
        until(&mut app, "the alert", |a| {
            alert(a) == Some(Alert::TooManySkylanders)
        });
        assert_eq!(mode(&app), Mode::Portal);
        clear(&mut app, 1);
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    }
}
