//! The first walk on the TV (openspec AG.21, design decisions 1 and 13;
//! level-scene's The first character placed walks into the level, Open
//! question 4's (a): the original's flow minus its story menus, until the
//! game's own menus land; stand-in ledger: `entering`).
//!
//! - Who: the first figure placed that walks ([`guacamole::walkers`]):
//!   Eruptor on his measured numbers, every other modelled character on his
//!   own scripts where the level's part holds them and they ask nothing the
//!   TV can't answer (AG.27). Any other figure, a character placed before
//!   him included, keeps the portal view and its card; a character kept by
//!   the part, a join that failed in the level or a part that didn't load
//!   is listed with why (`/api/_test/tv`'s `kept`).
//! - When: once his intro is over (or none plays), the TV on the portal
//!   view, the level loaded with its solids ([`Level::walkable`]) and his
//!   model read with every pipeline it draws compiled. Until then the portal
//!   view holds.
//! - What: he stands at `213_Init_Location`'s first waypoint facing 0°,
//!   settled onto the level's collision, posed on AE's evaluator by his
//!   locomotion (AG.30: his idle, walk, run and fall by the weights
//!   [`crate::hero::HeroGaits`] steps, his hero side's clips read with his
//!   model); the camera at camera 0's rest pose follows him on AG.17's
//!   chain until he joins the run, from then on the camera's scripts have
//!   the view (AG.28: the `Pop_To_Camera_*` volumes, the planes' blend and
//!   the sliding rocks' activator through `viewportinfo.default`), the
//!   chain the view's again for good once one of them or the portal manager
//!   stops in play, and once his own script stops until the next hero's
//!   scripts join (AG.190, AG.211; [`crate::hero`],
//!   stand-in ledger: `camera`); the level's input layer takes the pads
//!   ([`crate::pad`]). The camera is the level's
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
//!   ([`LevelPads::hold`]): no step runs, so nothing reaches his scripts,
//!   and no attack pressed before or under the hold is kept for a step
//!   after it ([`crate::hero::HeroActions::release`]). No intro plays once
//!   the level has started ([`crate::intro_player`]).
//! - The hero taken off takes his scripts with him by the deactivate path
//!   ([`crate::swap`], openspec AH.105): his minion, his file's placements
//!   and what his scripts made go, the level's scripts running on as they
//!   were (what they broke stays broken, a creature where it stood, a clone
//!   of the level's in flight landing), and the next hero in joins that run
//!   as the first did (AH.24's join), his scripts the one writer of his walk
//!   from then on, his health the one his figure's stored data kept.
//! - A character who can't walk (one the part lacks or keeps, AG.27)
//!   counts as one on the portal but can't come in: alone, it leaves the
//!   level asking for a Skylander.
//! - A hero whose script stops in play walks on in the level on AG.16's
//!   controller ([`crate::hero_scripts`], stand-in ledger: `walker`), his
//!   walk and facing the controller's alone and the view the TV's chain's
//!   (AH.151, AG.190), `/api/_test/tv`'s `hero.scripts` and
//!   `hero.camera.scripts` both `off` with his reason, until the next hero
//!   in joins the run and the camera's scripts take the view back (AG.211).
//!
//! Defeat (openspec AH.26, AH design decision 14; combat's A defeated hero,
//! tv-hud's No character left):
//! - His health out ([`follow_defeat`]): from the frame his scripts read
//!   none, the hero is dying; the death sequence his scripts run (the
//!   portal manager's, AH.20: the knock, then his minion off 2.2 s on)
//!   ends it, or [`DEFEAT_BACKSTOP`] of game time where none runs (a part
//!   without one, a portal manager stopped, the test hook's health of 0
//!   with no bite). Then he leaves as a hero taken off does, his figure
//!   beaten for the rest of the level ([`Play::beaten`]: by its instance,
//!   the placing profile and the figure), its experience and gold its own.
//! - A beaten figure counts for nothing on the portal: alone it holds the
//!   level under the needs-rest alert, asking for it to come off, then the
//!   call for a Skylander; placed again, the needs-rest alert again; any
//!   other character comes in where he fell. While he's dying, a character
//!   placed beside him waits without the too-many hold and comes in once
//!   he's out (ours: no oracle run placed one then).
//! - With no character left, Restart chapter is offered from
//!   [`RESTART_AFTER`] after his health ran out ([`Play::restart_offered`],
//!   drawn by [`crate::defeat`]); confirmed, [`restart`] ends the level
//!   ([`LevelEnded`]: every figure's progress written first, AH.33) and
//!   enters it again from its start as a new entry does, the run installed
//!   afresh (every figure's health full, the breakables and creatures back),
//!   the first character on the portal coming in.
//!   Ours (stand-in ledger: `no-character`).

use std::sync::Arc;
use std::time::Duration;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Projection, RenderTarget};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormats;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use guac_ir::clip::Clip;
use guac_ir::model::{Sampler, Scroll};
use guac_ir::nodes::AttachNodes;
use guac_ir::palette::Palette;
use guac_ir::scroll;
use guac_ir::shading::Scene;
use guac_ir::{Category, Model};
use guac_motion::camera::{CameraDef, GameCamera};
use guac_motion::hero::Hero;
use guac_motion::locomotion::{self, Locomotion, Loops};
use guac_motion::solid::{HavokSolid, SolidInfo};
use guacamole::character::attach::{self, SpawnedAttachments};
use guacamole::character::warm::{self, WarmKey};
use guacamole::character::{
    CharacterMaterial, CharacterUniform, ShadowLookup, reference_material, remaster,
    spawn_character,
};
use guacamole::level_scene::{START_FACING, hero_scene};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use guacamole::walkers::{self, Kept, Origin, Walk};
use serde::Serialize;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::Placement;
use tracing::{info, warn};

use crate::cards::{PortalCards, PortalChanged};
use crate::driver::PortalChange;
use crate::hero::{FollowCamera, HeroBody, HeroGaits, HeroPlugin, following, spawn_hero};
use crate::hero_scripts::{self, HeroRun, Join, ScriptSource};
use crate::intro::IntroQueue;
use crate::level::{
    LEVEL_LAYER, Level, LevelCamera, LevelSystems, LitAlso, VIEW_ORDER, view_camera,
};
use crate::motion::ReducedMotion;
use crate::pad::LevelPads;
use crate::physics::AvianSeam;
use crate::prepare::Prepare;
use crate::progress::LevelEnded;
use crate::screen::TvScreen;
use crate::turntable::{Loaded, Shelf, Turntables, load_model, read_hero, read_ir};
use crate::tv_state::{Current, Redrawn, TvCamera};
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

/// A figure on the portal as one profile's own (AH.1): the placing
/// profile, none for a figure placed with no profile unlocked, and the
/// figure. What a defeat marks for the rest of the level (combat's A
/// defeated hero), as the original's portal manager marks a toy's tag.
pub(crate) type Instance = (Option<String>, String);

/// The profile-and-figure pair `placement` stands for: what a defeat keys.
pub(crate) fn instance(placement: &Placement) -> Instance {
    (
        placement.owner.as_ref().map(|o| o.profile_id.clone()),
        placement.figure_id.as_str().to_owned(),
    )
}

/// The portal as the flow reads it: its characters in the order they were
/// placed, each with whether it can come in as the hero (a figure that walks,
/// AG.27's until every character does), and how many magic items and toys
/// Giants can't use stand beside them. A character beaten in the level
/// (AH.26) is no character here, only counted; and whether the hero's
/// health has run out, so a character placed beside him waits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Lineup {
    pub(crate) characters: Vec<(SlotIndex, bool)>,
    pub(crate) magic: usize,
    pub(crate) unusable: usize,
    /// Characters on the portal whose instance a defeat in this level
    /// took out: what keeps the needs-rest alert up.
    pub(crate) beaten: usize,
    /// The hero is dying: his health ran out and he isn't out yet.
    pub(crate) dying: bool,
}

impl Lineup {
    /// `order`'s slots read off `cards` by `shelf`'s roster, `beaten`'s
    /// instances counted apart.
    fn of(
        order: &[SlotIndex],
        cards: &PortalCards,
        shelf: &Shelf,
        source: Option<&ScriptSource>,
        kept: &[KeptCard],
        beaten: &[Instance],
    ) -> Self {
        let mut lineup = Self::default();
        for slot in order {
            let Some(placement) = cards.slots[slot.as_usize()].as_ref() else {
                continue;
            };
            let id = placement.figure_id.as_str();
            match kind(shelf, id) {
                Kind::Character if beaten.contains(&instance(placement)) => lineup.beaten += 1,
                Kind::Character => {
                    let walks = shelf.loads.get(id).is_some_and(|load| {
                        walking(load, source).is_some() && !kept.iter().any(|k| k.slot == *slot)
                    });
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
    /// A figure beaten in the level is on the portal and no character can
    /// take the hero's place (AH.26: the original's "needs to rest").
    NeedsRest,
}

impl Alert {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Alert::WrongToy => "wrong_toy",
            Alert::TooManySkylanders => "too_many_skylanders",
            Alert::TooManyItems => "too_many_items",
            Alert::PlaceSkylander => "place_a_skylander",
            Alert::NeedsRest => "needs_rest",
        }
    }
}

/// The alert `lineup` asks for. `inside`: the level has started (shown,
/// paused or held); `hero`: his slot while he's in it; `would_start`: before
/// the start, the level would show now but for the portal. A toy Giants
/// can't use comes first, then too many characters (one beside a dying hero
/// waits), then too many magic items, then, with no hero and none to come
/// in, the needs-rest alert while a beaten figure stands on the portal, else
/// the call for a Skylander.
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
        // He's on his way out: one beside him is the next in.
        Some(_) if lineup.dying => others.len() >= 2,
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
    if hero.is_some() || others.first().is_some_and(|walks| *walks) {
        return None;
    }
    Some(match lineup.beaten {
        0 => Alert::PlaceSkylander,
        _ => Alert::NeedsRest,
    })
}

/// The clips his locomotion plays beside his model's idle: his hero side's
/// walk, run and falling (AG.14), on his model's skeleton.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeroClips {
    pub(crate) walk: Option<Clip>,
    pub(crate) run: Option<Clip>,
    pub(crate) fall: Option<Clip>,
    /// The slots his hero side gives the walk, the run and the fall in his
    /// model's animation set, his palette's.
    pub(crate) slots: [Option<u16>; 3],
}

impl HeroClips {
    fn of(hero: guac_ir::hero::Hero) -> Self {
        let l = hero.locomotion;
        let slot = |s: &Option<guac_ir::hero::SlotClip>| s.as_ref().map(|s| s.slot);
        Self {
            slots: [slot(&l.walk), slot(&l.run), slot(&l.falling)],
            walk: l.walk.map(|s| s.clip),
            run: l.run.map(|s| s.clip),
            fall: l.falling.map(|s| s.clip),
        }
    }

    /// The palette slots of the four his gait reports and his scripts'
    /// picks name (openspec AH.49, AH.198): `idle` by name, his walk, run
    /// and fall at his hero side's slots where `palette` holds them there
    /// ([`guacamole::picks::Four::at`]), which is where his scripts play
    /// them.
    pub(crate) fn four(&self, palette: &Palette, idle: &str) -> guacamole::picks::Four {
        fn named(c: &Option<Clip>, slot: Option<u16>) -> Option<(&str, Option<u16>)> {
            c.as_ref().map(|c| (c.name.as_str(), slot))
        }
        guacamole::picks::Four::at(
            palette,
            [
                Some((idle, None)),
                named(&self.walk, self.slots[0]),
                named(&self.run, self.slots[1]),
                named(&self.fall, self.slots[2]),
            ],
        )
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

/// His model, his locomotion's clips, his palette (every slot's clip,
/// which his scripts play: AH.24) and his model's attach nodes (AH.50),
/// read together off the shelf.
pub(crate) struct HeroModel {
    loaded: Loaded,
    /// His solid as the disc authors it (AE.22): what a walker on his own
    /// scripts (AG.27) is built from, at the scale they set.
    solid: guac_ir::actor::Solid,
    clips: Arc<HeroClips>,
    palette: Option<Arc<Palette>>,
    nodes: Option<Arc<AttachNodes>>,
}

/// `load`'s model, its hero side, its palette and its attach nodes, on a
/// task. A palette that doesn't read leaves his scripts' clips unplayed, and
/// attach nodes that don't read leave every node at his pivot, each logged;
/// he walks all the same.
fn read_hero_model(
    shelf: &Shelf,
    load: &str,
    formats: CompressedImageFormats,
) -> Result<HeroModel, String> {
    let loaded = load_model(shelf, load, formats)?;
    let hero = read_hero(shelf, load).map_err(|e| format!("his hero side: {e}"))?;
    let palette = match read_palette(shelf, load) {
        Ok(p) => Some(Arc::new(p)),
        Err(why) => {
            warn!(load_file = load, reason = %why, "his palette didn't read: the clips his scripts start play nothing");
            None
        }
    };
    let nodes = match read_nodes(shelf, load) {
        Ok(n) => Some(Arc::new(n)),
        Err(why) => {
            warn!(load_file = load, reason = %why, "his attach nodes didn't read: what his scripts hang on them sits at his pivot");
            None
        }
    };
    let solid = hero.solid;
    Ok(HeroModel {
        loaded,
        solid,
        clips: Arc::new(HeroClips::of(hero)),
        palette,
        nodes,
    })
}

/// `load`'s attach nodes off the extract (AH.50's part).
fn read_nodes(shelf: &Shelf, load: &str) -> Result<AttachNodes, String> {
    let rel = shelf.ir.nodes_path(load);
    let nodes: AttachNodes = read_ir(shelf.ir, &shelf.dir.join(&rel))?;
    if nodes.identity != shelf.identity {
        return Err(format!("{rel} names another dump"));
    }
    if nodes.load_file != load {
        return Err(format!("{rel} holds {}'s attach nodes", nodes.load_file));
    }
    Ok(nodes)
}

/// `load`'s animation palette off the extract (AH.22's part).
fn read_palette(shelf: &Shelf, load: &str) -> Result<Palette, String> {
    let rel = shelf.ir.palette_path(load);
    let palette: Palette = read_ir(shelf.ir, &shelf.dir.join(&rel))?;
    if palette.identity != shelf.identity {
        return Err(format!("{rel} names another dump"));
    }
    if palette.owner != load {
        return Err(format!("{rel} holds {}'s palette", palette.owner));
    }
    Ok(palette)
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
    who: Who,
    model: Model3d,
}

/// Whose numbers the hero-to-be walks on.
enum Who {
    /// Measured (Eruptor's, [`walkers::WALKERS`]).
    Measured(Walk),
    /// His own scripts' (AG.27): built once his model is read, off his
    /// solid.
    Scripts,
}

/// A character kept on his card by a join that failed in the level
/// (AG.27), until his figure leaves the portal.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeptCard {
    slot: SlotIndex,
    kept: Kept,
}

/// Whether a character walks (AG.27): on measured numbers (a table the
/// build holds), on his own scripts (the level's part holds them and they
/// ask nothing the TV can't answer), or not (he keeps his card).
fn walking(load: &str, source: Option<&ScriptSource>) -> Option<Who> {
    if let Some(w) = walkers::walker(load) {
        return Some(Who::Measured(Walk::measured(w)));
    }
    match source?.walks(load) {
        Ok(Some(Origin::Scripts)) => Some(Who::Scripts),
        // A fighter without a measured table, or a character the part
        // holds no word on, doesn't walk.
        _ => None,
    }
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
    /// His palette: the clips his scripts start play from it (AH.24).
    palette: Option<Arc<Palette>>,
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
    /// Characters whose scripts didn't run through in the level (AG.27):
    /// they keep their cards, listed with why, until their figures leave
    /// the portal.
    kept: Vec<KeptCard>,
    /// The instances beaten in this level (AH.26): out until it ends.
    beaten: Vec<Instance>,
    /// The hero's health ran out: since when, on the level's clock (game
    /// time).
    dying: Option<Duration>,
    /// When the last hero's health ran out, on the TV's real clock, while no
    /// hero has come in since: Restart chapter's offer counts from it.
    beaten_at: Option<Duration>,
}

/// How long a beaten hero's death sequence may run before the TV takes him
/// out itself: past the original's 2.2 s (AH.11's 133-134 records from his
/// reaction to his minion off, which his scripts hold on the VM, AH.20's
/// `a_defeated_hero_knocks_them_back_and_leaves`), for a run where none
/// ends it (a part without the portal manager's, one stopped in play, the
/// test hook's health of 0 with no bite to kill him).
pub(crate) const DEFEAT_BACKSTOP: Duration = Duration::from_secs(3);

/// Restart chapter comes up this long after the hero's health ran out
/// (tv-hud's No character left: AH.11's `defeat2`, at 0 health by flip
/// 1,112 and fading in by 1,412).
pub(crate) const RESTART_AFTER: Duration = Duration::from_secs(5);

/// Asks for the level again from its start (AH.26's Restart chapter,
/// confirmed): [`restart`] does it.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartLevel;

/// Why a hero in the level with no run reads `off` ([`HeroReport::scripts`]):
/// [`hero_scripts::ask_join`] gave him none, the level's part holding no
/// file of his or not linking (the level's `scripts_reason` says which).
/// AH.25's review found such a hero read none, which every off check let
/// through, on a Mac run whose part didn't link.
pub(crate) const NO_RUN: &str =
    "no run: the level's part holds no scripts of his or didn't link; he walks on the stand-in";

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
    /// In the level: the solid his controller collides with, and what it
    /// was built from (openspec AG.170).
    pub solid: Option<SolidReport>,
    /// In the level: his locomotion as its last step left it (AG.30).
    pub gait: Option<GaitReport>,
    /// In the level (AH.24): his scripts' join (`asked`, `requested`,
    /// `joined`, or `off` with why); `off` too, with [`NO_RUN`], for a
    /// character whose scripts the level's part doesn't hold or whose part
    /// didn't link, who has no run and walks on AG.16's controller
    /// (stand-in ledger: `walker`; AH.106, so a reader holding a run to his
    /// scripts stops on it as on any other off). None before he's in.
    pub scripts: Option<String>,
    pub scripts_reason: Option<String>,
    /// His health and full health as his scripts hold them, once they run,
    /// and the behaviour his minion is in.
    pub health: Option<i32>,
    pub health_max: Option<i32>,
    pub behaviour: Option<String>,
    /// His figure's experience and gold as the TV holds them (AH.1): what
    /// his figure's words hold after the last step once his scripts run
    /// (AH.23), the placing profile's row as the placement read it before,
    /// fresh for a figure no profile placed.
    pub experience: Option<u32>,
    pub gold: Option<u32>,
}

/// The solid a hero stands on as `/api/_test/tv` reports it (openspec
/// AG.170): the shape his controller collides with, in metres, and the
/// `CollisionInfo` and scale it was built from.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct SolidReport {
    /// `sphere`, `capsule`, `box` or `cylinder`.
    pub shape: String,
    /// A sphere's, capsule's or cylinder's radius (0.05 m inside the
    /// authored half width, as the original builds them), a capsule's half
    /// segment, a box's half extents, a cylinder's half height, and a box's
    /// or cylinder's convex radius, each where the shape has one.
    pub radius: Option<f32>,
    pub half_segment: Option<f32>,
    pub half_extents: Option<[f32; 3]>,
    pub half_height: Option<f32>,
    pub margin: Option<f32>,
    /// Its centre from his pivot in his own frame (his heading and lean
    /// turn it), and its own turn, a quaternion `[x, y, z, w]`.
    pub centre: [f32; 3],
    pub rotation: [f32; 4],
    /// What it was built from, for a hero with a run: the `CollisionInfo`
    /// (its kind, full extents in feet, offset in metres and own angles in
    /// degrees) and the scale (a fraction).
    pub built_from: Option<CollisionReport>,
}

/// The `CollisionInfo` a hero's solid was built from ([`SolidReport`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct CollisionReport {
    pub kind: String,
    pub width: f32,
    pub height: f32,
    pub length: f32,
    pub offset: [f32; 3],
    pub angles: [f32; 3],
    pub scale: f32,
}

impl SolidReport {
    /// `solid` and, for a hero with a run, the `CollisionInfo` and scale it
    /// was built from.
    pub(crate) fn of(solid: &HavokSolid, from: Option<(&SolidInfo, f32)>) -> Self {
        use guac_motion::seam::Shape;
        let mut r = Self {
            centre: solid.centre.to_array(),
            rotation: solid.rotation.to_array(),
            built_from: from.map(|(info, scale)| CollisionReport {
                kind: format!("{:?}", info.kind).to_lowercase(),
                width: info.width,
                height: info.height,
                length: info.length,
                offset: info.offset,
                angles: info.rotation,
                scale,
            }),
            ..Self::default()
        };
        match solid.shape {
            Shape::Sphere { radius } => {
                r.shape = "sphere".into();
                r.radius = Some(radius);
            }
            Shape::Capsule {
                radius,
                half_segment,
            } => {
                r.shape = "capsule".into();
                r.radius = Some(radius);
                r.half_segment = Some(half_segment);
            }
            Shape::Box {
                half_extents,
                margin,
            } => {
                r.shape = "box".into();
                r.half_extents = Some(half_extents.to_array());
                r.margin = Some(margin);
            }
            Shape::Cylinder {
                radius,
                half_height,
                margin,
            } => {
                r.shape = "cylinder".into();
                r.radius = Some(radius);
                r.half_height = Some(half_height);
                r.margin = Some(margin);
            }
        }
        r
    }
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
    /// What his gaits show: his scripts' picks while they pick his clips
    /// (openspec AH.49), else AG.30's (stand-in ledger: `locomotion`).
    pub(crate) fn of_gaits(g: &HeroGaits) -> Self {
        let Some(p) = &g.picked else {
            return Self {
                step: g.steps,
                ..Self::of(&g.locomotion)
            };
        };
        Self {
            clip: p.top().to_owned(),
            speed_ft: p.speed,
            speed: p.speed * guac_motion::tuning::FOOT,
            run_share: p.run_share(),
            weights: p.weights(),
            rates: p.rates(),
            air: p.air().to_owned(),
            moving: p.moving(),
            held: false,
            step: g.steps,
        }
    }

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
    /// Who has the view (openspec AG.28): `on` while the camera's scripts on
    /// the VM drive it; `off` once one of them stopped in play and the TV's
    /// own camera has followed him since from where they left it, with why
    /// (stand-in ledger: `camera`); `none` before a hero joins their run or
    /// with no camera system in the level's part, with why. A run judging
    /// the scripts' camera holds them to `on` to the end.
    pub scripts: Option<String>,
    pub scripts_reason: Option<String>,
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
            scripts: None,
            scripts_reason: None,
        })
    }

    /// [`CameraReport::of`] with who has the view.
    pub(crate) fn held(
        transform: &Transform,
        projection: &Projection,
        follow: &FollowCamera,
    ) -> Option<Self> {
        let (scripts, reason) = follow.hold.report();
        Some(Self {
            scripts: Some(scripts.to_owned()),
            scripts_reason: reason,
            ..Self::of(transform, projection)?
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

    /// The instances beaten in this level (AH.26).
    pub(crate) fn beaten(&self) -> &[Instance] {
        &self.beaten
    }

    /// The hero's health has run out and he isn't out yet.
    pub(crate) fn dying(&self) -> bool {
        self.dying.is_some()
    }

    /// Restart chapter is on offer at `now` (the TV's real clock): the
    /// portal calling for a character with none to come in (so no hero in
    /// the level: [`judge`] calls only then), [`RESTART_AFTER`] since the
    /// last hero's health ran out with none in since.
    pub(crate) fn restart_offered(&self, now: Duration) -> bool {
        matches!(self.alert, Some(Alert::NeedsRest | Alert::PlaceSkylander))
            && self.beaten_at.is_some_and(|at| now >= at + RESTART_AFTER)
    }

    /// The slot the hero's figure stands in and its placement, while he's
    /// in the level: what the HUD reads his figure's numbers by.
    pub(crate) fn hero_placement(&self) -> Option<(Entity, SlotIndex, &Placement)> {
        let h = self.on_stage()?;
        let c = self.candidate.as_ref()?;
        Some((h.hero, c.slot, &c.placement))
    }

    /// The level's camera, while the level shows.
    pub(crate) fn level_camera(&self) -> Option<Entity> {
        self.inside.as_ref().map(|i| i.camera)
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
        bodies: &Query<(&HeroBody, Option<&HeroGaits>, Option<&HeroRun>)>,
        cameras: &Query<(&Transform, &Projection, &FollowCamera)>,
        runtime: Option<&ScriptRuntime>,
        earning: Option<&crate::progress::Earning>,
    ) -> Option<HeroReport> {
        let c = self.candidate.as_ref()?;
        let body = self.on_stage().and_then(|h| bodies.get(h.hero).ok());
        let hero = body.map(|(b, ..)| &b.hero);
        let gait = body.and_then(|(_, g, _)| g).map(GaitReport::of_gaits);
        let run = body.and_then(|(_, _, r)| r);
        let numbers = match (run.map(|r| &r.join), runtime) {
            (Some(Join::Joined(minion)), Some(rt)) => hero_scripts::hero_numbers(&rt.vm, *minion),
            _ => hero_scripts::HeroNumbers::default(),
        };
        // The TV's values for his slot: what his figure's words hold after
        // the last step (AH.23), else the row as the placement read it.
        let progress = earning
            .and_then(|e| e.slots.get(c.slot.as_usize()))
            .and_then(Option::as_ref)
            .map(|e| e.progress)
            .or_else(|| c.placement.owner.as_ref().map(|o| o.progress));
        // In the level with him in it: not while the portal holds it with
        // no hero (AG.29), when a replacement is only the hero-to-be.
        let inside = self.on_stage().is_some();
        let camera = self
            .inside
            .as_ref()
            .and_then(|i| cameras.get(i.camera).ok())
            .and_then(|(t, p, f)| CameraReport::held(t, p, f));
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
            heading: hero.map(crate::hero::heading),
            camera,
            solid: hero.map(|h| SolidReport::of(h.solid(), run.map(|r| (&r.shape, r.scale)))),
            gait,
            // In the level with no run: off, as any other fall-back is.
            scripts: match run {
                Some(r) => Some(r.join.name().to_owned()),
                None => hero.map(|_| "off".to_owned()),
            },
            scripts_reason: match run {
                Some(r) => match &r.join {
                    Join::Off(why) => Some(why.clone()),
                    _ => None,
                },
                None => hero.map(|_| NO_RUN.to_owned()),
            },
            health: numbers.health,
            health_max: numbers.health_max,
            behaviour: numbers.behaviour,
            experience: inside.then(|| progress.map_or(0, |p| p.xp)),
            gold: inside.then(|| progress.map_or(0, |p| p.gold)),
        })
    }
}

/// The walk's mode and the hero, their part of `/api/_test/tv` (openspec
/// AQ.8).
#[derive(SystemParam)]
pub(crate) struct PlayShots<'w, 's> {
    play: Option<Res<'w, Play>>,
    pads: Option<Res<'w, LevelPads>>,
    heroes: Query<
        'w,
        's,
        (
            &'static HeroBody,
            Option<&'static HeroGaits>,
            Option<&'static HeroRun>,
        ),
    >,
    cameras: Query<
        'w,
        's,
        (
            &'static Transform,
            &'static Projection,
            &'static FollowCamera,
        ),
    >,
    runtime: Option<Res<'w, ScriptRuntime>>,
    earning: Option<Res<'w, crate::progress::Earning>>,
    source: Option<Res<'w, ScriptSource>>,
}

impl PlayShots<'_, '_> {
    /// The characters kept on their cards and why (AG.27): the part's
    /// (left out of the extract, asking what the TV can't answer) and the
    /// level's (a join that didn't run through).
    pub(crate) fn kept(&self) -> Vec<Kept> {
        let mut out = self
            .source
            .as_deref()
            .map(ScriptSource::kept)
            .unwrap_or_default();
        if let Some(play) = self.play.as_deref() {
            out.extend(play.kept.iter().map(|k| k.kept.clone()));
        }
        out
    }

    pub(crate) fn mode(&self) -> Mode {
        match (self.play.as_deref(), self.pads.as_deref()) {
            (Some(play), Some(level_pads)) => play.mode(level_pads),
            _ => Mode::Portal,
        }
    }

    pub(crate) fn hero(&self) -> Option<HeroReport> {
        self.play.as_deref().and_then(|p| {
            p.report(
                &self.heroes,
                &self.cameras,
                self.runtime.as_deref(),
                self.earning.as_deref(),
            )
        })
    }
}

#[cfg(test)]
impl Play {
    /// The hero-to-be's model's way to the level, if one's chosen.
    pub(crate) fn candidate_model(&self) -> Option<&'static str> {
        self.candidate.as_ref().map(|c| c.model.name())
    }

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

    /// A hero beaten at `at` (the real clock) with no character left, the
    /// level started through `camera` and held under `alert`: for the tests
    /// of Restart chapter's choice (AH.26).
    pub(crate) fn beaten_for_tests(camera: Entity, alert: Alert, at: Duration) -> Self {
        let mut play = Self::alerting_for_tests(alert, Some(camera));
        play.beaten.push((None, "000009-0000".into()));
        play.beaten_at = Some(at);
        play
    }
}

/// The server's word on which slot's figure is the hero in a level
/// (openspec AH.36): the slot reset refuses that slot (AH.3). Absent when
/// the server never came up.
#[derive(Resource, Clone)]
pub(crate) struct LevelHeroOut(pub(crate) guac_server::progress::LevelHero);

/// The hero's slot while he's in the level, `None` while no level shows or
/// the portal holds it with no hero in it, told the server whenever it
/// changes: the level starting with him, another character taking over,
/// the level ending (AH.36).
fn tell_level_hero(play: Res<Play>, out: Option<Res<LevelHeroOut>>) {
    let Some(out) = out else {
        return;
    };
    let hero = play.on_stage().and(play.candidate.as_ref()).map(|c| c.slot);
    if out.0.get() != hero {
        info!(
            slot = hero.map(SlotIndex::display),
            "the hero's slot told the server"
        );
        out.0.set(hero);
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
            .add_message::<RestartLevel>()
            .add_message::<LevelEnded>()
            .add_systems(
                Update,
                (
                    restart,
                    follow_portal,
                    keep_unlinked,
                    choose_hero,
                    load_hero,
                    leave,
                    keep_cards,
                    follow_defeat,
                    judge_portal,
                    enter,
                    pose_hero,
                    rest_the_cards,
                    tell_level_hero,
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
    bodies: Query<(&HeroBody, Option<&HeroRun>)>,
) {
    for PortalChanged(change) in heard.read() {
        let slot = match change {
            PortalChange::Placed { slot, .. } | PortalChange::Cleared { slot } => *slot,
        };
        play.order.retain(|s| *s != slot);
        play.kept.retain(|k| k.slot != slot);
        if matches!(change, PortalChange::Placed { .. }) {
            play.order.push(slot);
        }
        if !play.candidate.as_ref().is_some_and(|c| c.slot == slot) {
            continue;
        }
        let gone = play.candidate.take().expect("checked above");
        let play = &mut *play;
        match play.inside.as_mut() {
            None => info!(slot = slot.display(), "the waiting hero left the portal"),
            Some(inside) => {
                // Lifted while dying, he's beaten all the same: his health
                // ran out, and he'd come back with none.
                if play.dying.take().is_some() && inside.hero.is_some() {
                    info!(
                        slot = slot.display(),
                        "the dying hero left the portal: beaten"
                    );
                    play.beaten.push(instance(&gone.placement));
                }
                take_off(inside, &mut level, &mut commands, &bodies, slot);
            }
        }
    }
}

/// A beaten hero out of the level (AH.26, the module docs): from the frame
/// his scripts' health reads none he's dying, and once his minion is off
/// the run (the death sequence's end) or [`DEFEAT_BACKSTOP`] of game time
/// on, he leaves as a hero taken off does, his figure beaten for the rest
/// of the level, the portal still holding it.
fn follow_defeat(
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut commands: Commands,
    bodies: Query<(&HeroBody, Option<&HeroRun>)>,
    runtime: Option<Res<ScriptRuntime>>,
    (game, real): (Res<Time<Virtual>>, Res<Time<Real>>),
) {
    let Some(hero) = play.on_stage().map(|h| h.hero) else {
        play.dying = None;
        return;
    };
    let run = bodies.get(hero).ok().and_then(|(_, run)| run);
    let minion = match run.map(|r| &r.join) {
        Some(Join::Joined(minion)) => Some(*minion),
        _ => None,
    };
    let vm = runtime.as_deref().map(|rt| &rt.vm);
    let (health, off) = match (vm, minion) {
        (Some(vm), Some(m)) => (guacamole::scripts::health(vm, m), vm.removed(m)),
        _ => (None, false),
    };
    if play.dying.is_none() && health.is_some_and(|h| h <= 0) {
        info!(?health, "the hero's health ran out: dying");
        play.dying = Some(game.elapsed());
        play.beaten_at = Some(real.elapsed());
    }
    let Some(since) = play.dying else {
        return;
    };
    let backstop = game.elapsed() >= since + DEFEAT_BACKSTOP;
    if !off && !backstop {
        return;
    }
    let play = &mut *play;
    let Some(c) = play.candidate.take() else {
        return;
    };
    play.dying = None;
    play.beaten.push(instance(&c.placement));
    info!(
        slot = c.slot.display(),
        figure = %c.placement.name,
        by = if off { "his death sequence" } else { "the backstop" },
        "beaten: his figure is out for the rest of the level"
    );
    if let Some(inside) = play.inside.as_mut() {
        debug_assert_eq!(inside.hero.as_ref().map(|h| h.hero), Some(hero));
        take_off(inside, &mut level, &mut commands, &bodies, c.slot);
    }
}

/// The hero off the portal mid-level: his entity and drawn model go, the
/// level stops lighting his materials, and where he stood and the way he
/// faced are kept for the next one in. His scripts go with him first, by
/// the deactivate path ([`crate::swap`], openspec AH.105), while his entity
/// is still there for it to read: the level's scripts run on, and the next
/// one joins that run (AH.24). The level holds from the next
/// [`judge_portal`] (no hero in it).
fn take_off(
    inside: &mut Inside,
    level: &mut Level,
    commands: &mut Commands,
    bodies: &Query<(&HeroBody, Option<&HeroRun>)>,
    slot: SlotIndex,
) {
    let Some(gone) = inside.hero.take() else {
        return;
    };
    if let Ok((body, run)) = bodies.get(gone.hero) {
        inside.left = Some((body.hero.pivot, body.hero.facing));
        if let Some(run) = run {
            commands.run_system_cached_with(
                crate::swap::deactivate,
                crate::swap::Leaving {
                    hero: gone.hero,
                    run: run.clone(),
                },
            );
        }
    }
    level.forget(&gone.lit);
    commands.entity(gone.hero).despawn();
    info!(slot = slot.display(), left = ?inside.left, "the hero left the portal: the level holds");
}

/// `slot`'s figure as the hero-to-be, if it walks.
fn candidate(
    slot: SlotIndex,
    cards: &PortalCards,
    shelf: &Shelf,
    source: Option<&ScriptSource>,
    kept: &[KeptCard],
) -> Option<Candidate> {
    let placement = cards.slots[slot.as_usize()].as_ref()?;
    let load = shelf.loads.get(placement.figure_id.as_str())?;
    if kept.iter().any(|k| k.slot == slot) {
        return None;
    }
    let who = walking(load, source)?;
    Some(Candidate {
        slot,
        placement: placement.clone(),
        load_file: load.clone(),
        who,
        model: Model3d::Shelfless,
    })
}

/// Who walks next, once the shelf says which character each figure is:
/// before the level shows, the first figure placed that walks; in it with no
/// hero, the one character on the portal if it walks (two hold it under the
/// too-many alert, [`judge`]; a figure beaten in the level is none, AH.26).
fn choose_hero(
    cards: Res<PortalCards>,
    turntables: Res<Turntables>,
    mut play: ResMut<Play>,
    source: Option<Res<ScriptSource>>,
) {
    if play.candidate.is_some() {
        return;
    }
    let Some(shelf) = turntables.shelf() else {
        return;
    };
    let source = source.as_deref();
    let chosen = match &play.inside {
        None => play
            .order
            .iter()
            .find_map(|slot| candidate(*slot, &cards, shelf, source, &play.kept)),
        Some(inside) if inside.hero.is_none() => {
            match Lineup::of(&play.order, &cards, shelf, source, &play.kept, &play.beaten)
                .characters
                .as_slice()
            {
                [(slot, true)] => candidate(*slot, &cards, shelf, source, &play.kept),
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
/// on, its three point lights, no shadows: openspec AH.65), which alone key
/// a pipeline; the level's lights and fog are uniforms and its buffer, set
/// when he's drawn.
pub(crate) fn key_scene() -> Scene {
    Scene {
        fog: true,
        point_lights: vec![guacamole::level_scene::DARK; guacamole::level_scene::MAX_POINT_LIGHTS],
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
            // His glowing parts' masks in the level too (AH.65).
            Some(Ok(read)) => match warm::level_keys(&read.loaded.model, &key_scene()) {
                Err(why) => Model3d::Failed(why.to_string()),
                Ok(keys) => {
                    if !warm.covers(keys.iter().map(|(k, _)| k)) {
                        warm.ask_hero(&read.loaded.model, &keys, Arc::new(key_scene()));
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
/// level ends ([`end_level`]).
fn leave(
    mut commands: Commands,
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut pads: ResMut<LevelPads>,
    current: Option<Res<Current>>,
    mut ended: MessageWriter<LevelEnded>,
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
    end_level(&mut commands, &mut play, &mut level, &mut pads, &mut ended);
}

/// Restart chapter confirmed (AH.26; tv-hud's No character left): the level
/// ends ([`end_level`]) and the TV enters it again from its start as any
/// entry does, the first character on the portal that walks the hero, no
/// intro (none is asked: nothing was placed). The run installed afresh puts
/// every breakable and creature back and every figure's health full; the
/// figures' progress is theirs, written first. The portal view shows while
/// his model is read again (stand-in ledger: `no-character`).
fn restart(
    mut asked: MessageReader<RestartLevel>,
    mut commands: Commands,
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut pads: ResMut<LevelPads>,
    mut ended: MessageWriter<LevelEnded>,
) {
    if asked.read().count() == 0 || play.inside.is_none() {
        return;
    }
    info!("Restart chapter: the level again from its start");
    end_level(&mut commands, &mut play, &mut level, &mut pads, &mut ended);
}

/// The level's end, whatever ends it: every figure's progress written whole
/// first ([`LevelEnded`], openspec AH.33), then the hero, his camera and the
/// pads' level layer go, the level hidden again, the alert and the level's
/// lists with them, and the run installed again from the part for the next
/// entry (AH.24).
fn end_level(
    commands: &mut Commands,
    play: &mut Play,
    level: &mut Level,
    pads: &mut LevelPads,
    ended: &mut MessageWriter<LevelEnded>,
) {
    let Some(inside) = play.inside.take() else {
        return;
    };
    ended.write(LevelEnded);
    level.hide(commands, inside.camera);
    if let Some(hero) = inside.hero {
        commands.entity(hero.hero).despawn();
    }
    commands.entity(inside.camera).despawn();
    pads.hide();
    play.candidate = None;
    play.alert = None;
    // The level's own lists go with it (AG.183): the next entry's run is a
    // fresh one, which a walker whose join failed in this one tries again,
    // and a figure beaten in it is no longer (AH.26), as a level change
    // clears the original's.
    play.kept.clear();
    play.beaten.clear();
    play.dying = None;
    play.beaten_at = None;
    // The scripts start over with the level's next entry (AH.24).
    commands.queue(hero_scripts::reinstall);
}

/// A character on the portal with no measured numbers while the level's
/// script part didn't load keeps his card, listed with why (AG.183, found
/// at the AG.27 merge check: he kept it silently, in no list, the TV
/// waiting on nothing): there are no scripts of his to walk on. One with
/// measured numbers (Eruptor) walks on them, reading `off` ([`NO_RUN`]).
fn keep_unlinked(
    mut play: ResMut<Play>,
    cards: Res<PortalCards>,
    turntables: Res<Turntables>,
    level: Res<Level>,
    source: Option<Res<ScriptSource>>,
) {
    if source.is_some() {
        return;
    }
    let (Some(why), Some(shelf)) = (level.scripts_reason(), turntables.shelf()) else {
        return;
    };
    let play = &mut *play;
    for &slot in &play.order {
        if play.kept.iter().any(|k| k.slot == slot) {
            continue;
        }
        let Some(placement) = cards.slots[slot.as_usize()].as_ref() else {
            continue;
        };
        let id = placement.figure_id.as_str();
        let Some(load) = shelf.loads.get(id) else {
            continue;
        };
        if kind(shelf, id) != Kind::Character || walkers::walker(load).is_some() {
            continue;
        }
        let reason = format!("the level's scripts didn't load, so he has none to walk on: {why}");
        warn!(load_file = %load, %reason, "he keeps his card");
        play.kept.push(KeptCard {
            slot,
            kept: Kept {
                load_file: load.clone(),
                reason,
                slot: Some(slot.display()),
            },
        });
    }
}

/// A walker whose scripts didn't run through (AG.27: his setup stopped on a
/// member, his file wouldn't join, his minion didn't activate) keeps his
/// card: he comes out of the level as a hero taken off the portal does,
/// listed with why until his figure leaves, and the portal calls for a
/// Skylander as it does for any other empty hand.
fn keep_cards(
    mut play: ResMut<Play>,
    mut level: ResMut<Level>,
    mut commands: Commands,
    bodies: Query<(&HeroBody, Option<&HeroRun>)>,
    keeps: Query<(Entity, &hero_scripts::KeepsCard, &HeroRun)>,
) {
    for (e, keeps, run) in &keeps {
        let play = &mut *play;
        let Some(inside) = play.inside.as_mut() else {
            continue;
        };
        if inside.hero.as_ref().map(|h| h.hero) != Some(e) {
            continue;
        }
        warn!(load_file = %run.load_file, reason = %keeps.0, "he keeps his card");
        play.kept.push(KeptCard {
            slot: run.slot,
            kept: Kept {
                load_file: run.load_file.clone(),
                reason: keeps.0.clone(),
                slot: Some(run.slot.display()),
            },
        });
        take_off(inside, &mut level, &mut commands, &bodies, run.slot);
        play.candidate = None;
    }
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
    source: Option<Res<ScriptSource>>,
) {
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    let alert = match turntables.shelf().filter(|_| on_portal) {
        Some(shelf) => {
            let mut lineup = Lineup::of(
                &play.order,
                &cards,
                shelf,
                source.as_deref(),
                &play.kept,
                &play.beaten,
            );
            lineup.dying = play.dying.is_some();
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
    /// The level's script part, once linked: whether his scripts join.
    source: Option<Res<'w, ScriptSource>>,
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
        who,
        load_file,
        model: Model3d::Ready(read),
        ..
    }) = play.candidate.as_ref()
    else {
        return;
    };
    // Measured numbers, or his own scripts' (AG.27) over his solid as the
    // disc authors it.
    let walker = match who {
        Who::Measured(w) => w.clone(),
        Who::Scripts => Walk::scripted(load_file, &read.solid),
    };
    let walker = &walker;
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
    let palette = read.palette.clone();
    let rig = read.nodes.clone().map(|nodes| hero_scripts::HandRig {
        model: drawn.model.clone(),
        clips: clips.clone(),
        nodes,
        scale: walker.scale,
    });
    let mut loops = clips.loops(&read.loaded.model);
    if walker.origin == Origin::Scripts {
        // His locomotion is the VM's (AH.49): no gait speed of ours picks
        // his clip; until his setup defines his walk and run he plays his
        // idle, and where the VM's picks stop AG.30's mixes those (AG.219).
        loops.walk = None;
        loops.run = None;
    }
    s.commands
        .entity(hero_entity)
        .insert(HeroGaits::new(Locomotion::new(walker.gaits, loops)));
    // His own scripts, where the level's part holds them (AH.24): at the
    // start, and for a replacement into the run his predecessor left
    // (AH.105).
    hero_scripts::ask_join(
        &mut s.commands,
        hero_entity,
        walker,
        palette.clone(),
        rig,
        s.source.as_deref(),
        c.slot,
        Some(drawn.root),
    );
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
        palette,
    };
    s.redrawn.write(Redrawn::Level);
    // A hero in: no Restart chapter on offer until the next defeat (AH.26).
    play.beaten_at = None;
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
    let follow = FollowCamera::new(
        GameCamera::new(CameraDef::LEVEL_213_START, feet),
        Vec::new(),
    );
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
    guacamole::character::spawn_alpha_clear(&mut s.commands, camera, LEVEL_LAYER);
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
    /// The model's root: what his scripts' scale (AG.27) sets.
    root: Entity,
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
    walker: &Walk,
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
        root: body.root,
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
    scripts: Option<Res<ScriptWorld>>,
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
    let step = (fixed.overstep_fraction(), fixed.timestep().as_secs_f64());
    // What his scripts play on him (AH.24), each eased back from its time
    // this step by what's left of the step at its rate, as his locomotion's.
    let over: Vec<(&Clip, f64)> = match (scripts.as_deref(), inside.palette.as_deref()) {
        (Some(world), Some(palette)) => script_clips(world, inside.hero, palette, step),
        _ => Vec::new(),
    };
    let pose = match gaits.get(inside.hero).ok() {
        Some(g) => gait_pose(
            &inside.model,
            &inside.clips,
            g,
            inside.palette.as_deref(),
            step,
            still,
            &over,
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
/// next, a step `step` seconds long: the clips his scripts' locomotion
/// weights on him out of `palette` (openspec AH.49), else AG.30's,
/// each where [`drawn_clips`] puts it; his scripts' others laid over.
fn gait_pose(
    model: &Model,
    clips: &HeroClips,
    gaits: &HeroGaits,
    palette: Option<&Palette>,
    (alpha, step): (f32, f64),
    held: bool,
    over: &[(&Clip, f64)],
) -> Vec<Transform> {
    let playing = match (&gaits.picked, palette) {
        (Some(p), Some(palette)) => {
            let mut playing = p.posed(&gaits.picked_before, palette, (alpha, step));
            // Under reduced motion his idle holds its start, as AG.30's
            // does.
            if held && let Some(idle) = p.four.0[0].and_then(|s| palette.clip(s as u16)) {
                for c in playing.iter_mut().filter(|c| std::ptr::eq(c.0, idle)) {
                    c.1 = 0.0;
                }
            }
            playing
        }
        _ => gait_clips(model, clips, drawn_clips(gaits, (alpha, step), held)),
    };
    guacamole::character::mesh::blended_over(model, &playing, over)
}

/// His locomotion's clips as the step left them (AH.50: what his attach
/// nodes are posed over), each at its time and weight, none eased.
pub(crate) fn locomotion_clips<'a>(
    model: &'a Model,
    clips: &'a HeroClips,
    gaits: &HeroGaits,
) -> Vec<(&'a Clip, f64, f64)> {
    gait_clips(model, clips, drawn_clips(gaits, (1.0, 0.0), false))
}

/// Each of his locomotion's clips with a time and weight from `drawn` (in
/// [`locomotion::Clip::ALL`]'s order), the idle his model's.
fn gait_clips<'a>(
    model: &'a Model,
    clips: &'a HeroClips,
    drawn: [(f64, f32); 4],
) -> Vec<(&'a Clip, f64, f64)> {
    locomotion::Clip::ALL
        .iter()
        .zip(drawn)
        .filter_map(|(&gait, (at, weight))| {
            let clip = match gait {
                locomotion::Clip::Idle => Some(&model.idle),
                _ => clips.get(gait),
            }?;
            Some((clip, at, f64::from(weight)))
        })
        .collect()
}

/// The clips his scripts play on `hero` (AH.24), out of `palette`, each at
/// its time `alpha` of the way from its last step to its next, a step
/// `step` seconds long: its time this step less what's left of the step at
/// its rate, none before 0. Oldest first, so a later one lies over it.
pub(crate) fn script_clips<'p>(
    world: &ScriptWorld,
    hero: Entity,
    palette: &'p Palette,
    (alpha, step): (f32, f64),
) -> Vec<(&'p Clip, f64)> {
    let back = f64::from(1.0 - alpha) * step;
    hero_scripts::playing_on(world, hero.to_bits())
        .into_iter()
        .filter_map(|(slot, time, rate)| {
            let clip = palette.clip(u16::try_from(slot).ok()?)?;
            let at = (f64::from(time) - back * f64::from(rate) * 0.01).max(0.0);
            Some((clip, at))
        })
        .collect()
}

/// Each clip of his locomotion as drawn `alpha` of the way from its last
/// step to its next, a step `step` seconds long, in
/// [`locomotion::Clip::ALL`]'s order: its time this step less what's left
/// of the step at its rate (from its restart, where it restarted), none
/// before 0, and its weight eased from the step before's; the idle at its
/// start when `held`. His scripts' picks' four while they pick (openspec
/// AH.49), their weights each one's share of all they weight.
pub(crate) fn drawn_clips(
    gaits: &HeroGaits,
    (alpha, step): (f32, f64),
    held: bool,
) -> [(f64, f32); 4] {
    if let Some(p) = &gaits.picked {
        let mut four = p.drawn_four(&gaits.picked_before, (alpha, step));
        if held {
            four[0].0 = 0.0;
        }
        return four;
    }
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

    use skylander_rpcs3_control::FigureProgress;

    use super::*;
    use crate::test_support::level::{FLOOR, START, WALL, lights, write_level};
    use crate::test_support::play::*;
    use crate::test_support::test_pad;
    use crate::test_support::turntable::placed;

    /// AH.49: while his scripts pick his clips, his gait report, its log and
    /// the drawn log read their picks: the heaviest of the four, his walk's
    /// speed, the run's share of the walk and run as written, the idle at
    /// its own weight and the gaits the rest by theirs, the rates in
    /// hundredths; drawn, eased from the step before's weights. AG.30's
    /// numbers (standing in his idle) never show.
    #[test]
    fn his_gaits_read_his_scripts_picks_while_they_pick() {
        use guacamole::picks::{Four, Picked, Weighted};
        let w = |slot, weight, rate| Weighted {
            slot,
            time: 0.5,
            rate,
            weight,
        };
        let loops = Loops {
            idle: 4.0 / 3.0,
            walk: Some(1.0667),
            run: Some(0.6667),
            fall: Some(0.8),
        };
        let mut g = HeroGaits::new(Locomotion::new(
            guac_motion::locomotion::Gaits::ERUPTOR,
            loops,
        ));
        g.picked = Some(Picked {
            clips: vec![w(0, 40.0, 100.0), w(1, 60.0, 124.0), w(3, 40.0, 117.0)],
            four: Four([Some(0), Some(1), Some(3), Some(57)]),
            speed: 14.0,
        });
        g.picked_before = vec![(0, 100.0), (1, 0.0), (3, 0.0)];
        g.steps = 7;
        let r = GaitReport::of_gaits(&g);
        assert_eq!(
            (r.clip.as_str(), r.air.as_str(), r.step),
            ("idle", "ground", 7)
        );
        assert!((r.speed_ft - 14.0).abs() < 1e-6 && r.moving);
        assert!((r.run_share - 0.4).abs() < 1e-6);
        for (got, want) in r.weights.iter().zip([0.4, 0.36, 0.24, 0.0]) {
            assert!((got - want).abs() < 1e-6, "{:?}", r.weights);
        }
        assert_eq!(r.rates, [1.24, 1.17]);
        // Halfway through the step: idle 70, walk 30, run 20: the idle its
        // own 0.7, the gaits the rest by their weights.
        let drawn = drawn_clips(&g, (0.5, 1.0 / 60.0), false);
        for (got, want) in drawn.iter().map(|d| d.1).zip([0.7, 0.18, 0.12, 0.0]) {
            assert!((got - want).abs() < 1e-6, "{drawn:?}");
        }
        // Without picks, AG.30's: his idle alone.
        g.picked = None;
        let r = GaitReport::of_gaits(&g);
        assert_eq!((r.clip.as_str(), r.weights), ("idle", [1.0, 0.0, 0.0, 0.0]));
    }

    /// AH.198's review: the four his picks and his hand-over name sit at his
    /// hero side's slots, where his scripts play them, though his palette
    /// holds his run at an earlier slot too (DinoRang's 2 and 3 on the dump,
    /// 11 of the 67 so), which the first by name would take.
    #[test]
    fn his_four_are_at_his_hero_sides_slots() {
        use guac_ir::palette::{PaletteSlot, SlotEntry};
        use guacamole::picks::Four;
        let clips = HeroClips::of(crate::test_support::play::hero_side(WALKER));
        let (walk, run, fall) = (clips.walk.clone(), clips.run.clone(), clips.fall.clone());
        let (walk, run, fall) = (walk.unwrap(), run.unwrap(), fall.unwrap());
        assert_eq!(clips.slots.map(|s| s.is_some()), [true; 3]);
        let mut idle = walk.clone();
        idle.name = "Synthetic_Idle.ma".into();
        let entry = |slot: u16, clip: &Clip| PaletteSlot {
            slot,
            name: None,
            label: None,
            entry: SlotEntry::Clip {
                additive: false,
                partial: false,
                layered: false,
                motion_extraction: 0,
                clip: clip.clone(),
            },
        };
        let [w, r, f] = clips.slots.map(Option::unwrap);
        assert!(
            r > 1 && w < r - 1,
            "room for an earlier run: {:?}",
            clips.slots
        );
        let mut entries = vec![
            entry(0, &idle),
            entry(w, &walk),
            entry(r - 1, &run),
            entry(r, &run),
            entry(f, &fall),
        ];
        entries.sort_by_key(|e| e.slot);
        let palette = Palette {
            identity: crate::test_support::turntable::identity(),
            owner: WALKER.into(),
            slots: 64,
            entries,
            refused: Vec::new(),
        };
        let by_name = Four::of(
            &palette,
            [
                Some("Synthetic_Idle.ma"),
                Some(walk.name.as_str()),
                Some(run.name.as_str()),
                Some(fall.name.as_str()),
            ],
        );
        assert_eq!(by_name.0[2], Some(u32::from(r - 1)), "the first by name");
        let four = clips.four(&palette, "Synthetic_Idle.ma");
        assert_eq!(four, Four([0, w, r, f].map(|s| Some(u32::from(s)))));
    }

    fn alert(app: &App) -> Option<Alert> {
        app.world().resource::<Play>().alert()
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

        // This level has no script part, so he has no run: he reads `off`
        // with why, as any hero on the stand-in (stand-in ledger: `walker`)
        // does (AH.106; AH.25's review found him reading none, which every
        // off check let through).
        let report = {
            let mut state: bevy::ecs::system::SystemState<(
                Query<(&HeroBody, Option<&HeroGaits>, Option<&HeroRun>)>,
                Query<(&Transform, &Projection, &FollowCamera)>,
                Res<Play>,
            )> = bevy::ecs::system::SystemState::new(app.world_mut());
            let (bodies, cameras, play) = state.get(app.world()).unwrap();
            play.report(&bodies, &cameras, None, None)
                .expect("the hero")
        };
        assert!(report.position.is_some(), "in the level");
        assert_eq!(
            (report.scripts.as_deref(), report.scripts_reason.as_deref()),
            (Some("off"), Some(NO_RUN))
        );

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
                .all(|w| *w == guacamole::character::LIGHTS_FROM_SCENE),
            "{frames:?}"
        );
        // AH.65: the walk's camera runs the glow chain off its frame's alpha,
        // so it carries the clear the level's glow masks write over; without
        // it every part's own alpha would read as a mask and the whole frame
        // would glow.
        let camera = app
            .world_mut()
            .query_filtered::<Entity, (With<FollowCamera>, With<guacamole::character::FrameGlow>)>()
            .single(app.world())
            .expect("the walk's camera, glowing");
        let clears: Vec<(Entity, RenderLayers)> = app
            .world_mut()
            .query_filtered::<(&ChildOf, &RenderLayers), With<MeshMaterial3d<guacamole::character::AlphaClear>>>()
            .iter(app.world())
            .map(|(c, l)| (c.parent(), l.clone()))
            .collect();
        assert!(
            clears.contains(&(camera, RenderLayers::layer(LEVEL_LAYER))),
            "{clears:?}"
        );
    }

    /// The scripts' clones as entities: whether each draws, how many parts
    /// hang under it, whether it's shown and whether an attack made it.
    fn clones(app: &mut App) -> Vec<(bool, usize, Visibility, bool)> {
        let mut q = app.world_mut().query::<(
            &guacamole::scripts::ScriptClone,
            Option<&crate::hero_scripts::CloneDrawn>,
            Option<&Children>,
            Option<&Visibility>,
        )>();
        q.iter(app.world())
            .map(|(_, d, c, v)| {
                (
                    d.is_some_and(|d| d.draws),
                    c.map_or(0, |c| c.len()),
                    v.copied().unwrap_or(Visibility::Inherited),
                    d.is_some_and(|d| d.attack),
                )
            })
            .collect()
    }

    /// AH.24: Eruptor, his scripts in the level's part, throws on the
    /// bottom button. His scripts join as he enters, his health and full
    /// health then off them (the synthetic minion's 120 of 150) and his
    /// figure's experience and gold off the placement, his helper drawn
    /// and no attack's; the press starts his throw's clip on him (drawn
    /// over his locomotion) and spawns the ball, drawn from the spawn
    /// part's model and an attack's, which flies, lands on the floor and is
    /// hidden once gone; held, he throws once; his second attack's
    /// button starts its clip. Leaving the level installs the scripts again.
    #[test]
    fn eruptor_throws_from_his_scripts_on_the_bottom_button() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        write_attack_parts(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready"
                && a.world()
                    .contains_resource::<guacamole::scripts::ScriptRuntime>()
        });
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        let numbers = |app: &App| {
            let e = app.world().resource::<Play>().hero().unwrap();
            let run = app.world().entity(e).get::<HeroRun>().unwrap().clone();
            let rt = app.world().resource::<guacamole::scripts::ScriptRuntime>();
            match run.join {
                Join::Joined(m) => crate::hero_scripts::hero_numbers(&rt.vm, m),
                _ => Default::default(),
            }
        };
        let n = numbers(&app);
        assert_eq!(
            (n.health, n.health_max),
            (
                Some(guac_formats::script::builder::ATTACK_HEALTH),
                Some(guac_formats::script::builder::ATTACK_HEALTH_MAX)
            )
        );
        let throws = |app: &App| -> Vec<u32> {
            let log = app.world().resource::<crate::hero_scripts::ScriptLog>();
            log.since(0)
                .iter()
                .flat_map(|s| s.started.iter().map(|q| q.slot))
                .collect()
        };
        let spawned = |app: &App| -> (usize, usize) {
            let log = app.world().resource::<crate::hero_scripts::ScriptLog>();
            let steps = log.since(0);
            let count =
                |f: fn(&crate::hero_scripts::ScriptStep) -> &Vec<crate::hero_scripts::Spawned>| {
                    steps
                        .iter()
                        .flat_map(f)
                        .filter(|c| c.name == guac_formats::script::builder::ATTACK_BALL)
                        .count()
                };
            (count(|s| &s.spawned), count(|s| &s.gone))
        };
        assert_eq!(spawned(&app), (0, 0));
        app.update();
        assert_eq!(
            clones(&mut app),
            [(true, 1, Visibility::Inherited, false)],
            "his helper, drawn, no attack's"
        );
        // One writer (design decision 5): his walk is his scripts', and the
        // synthetic minion's walk nothing, so the stick moves him nowhere.
        let still = hero(&app).pivot;
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(hero(&app).pivot, still, "the stick isn't his walk's writer");
        test_pad::stick(&mut app, pad, 0.0, 0.0);
        app.update();
        // Held: one throw, its clip on him and the ball drawn in flight.
        test_pad::button(&mut app, pad, GamepadButton::South, true);
        let deadline = Instant::now() + Duration::from_secs(5);
        while spawned(&app).0 == 0 {
            assert!(Instant::now() < deadline, "no ball");
            app.update();
        }
        let throw = guac_formats::script::builder::ATTACK_THROW_SLOT as u32;
        let arrival = guac_formats::script::builder::ATTACK_ARRIVAL_SLOT as u32;
        assert_eq!(throws(&app), [arrival, throw]);
        app.update();
        let drawn: Vec<_> = clones(&mut app)
            .into_iter()
            .filter(|&(d, .., attack)| d && attack)
            .collect();
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert_eq!(drawn[0].1, 1, "the model's one segment under it");
        let e = app.world().resource::<Play>().hero().unwrap();
        let world = app.world().resource::<ScriptWorld>();
        assert_eq!(
            crate::hero_scripts::playing_on(world, e.to_bits()).len(),
            1,
            "his throw plays on him"
        );
        // Drawn over his idle: the made-up throw holds the top joint at the
        // walk clip's x, where his idle sways it between 0 and 0.5.
        assert!((top_x(&app) - WALK_X).abs() < 1e-4, "{}", top_x(&app));
        // His hands posed by it too (AH.50): node 7 on the top joint, at
        // (WALK_X, 2, HAND_Z) in his model at his scale (1.5), node 8 behind.
        let nodes = world.nodes(e.to_bits()).expect("his nodes posed").to_vec();
        let scale = walkers::walker("009_Eruptor").unwrap().scale;
        assert_eq!(nodes.len(), 9);
        let want = Vec3::new(WALK_X, 2.0, HAND_Z) * scale;
        assert!(nodes[7].at.distance(want) < 1e-4, "{:?}", nodes[7]);
        assert!(nodes[8].at.distance(want - Vec3::Z * 2.0 * HAND_Z * scale) < 1e-4);
        assert_eq!(nodes[0].at, Vec3::ZERO, "on his origin");
        while spawned(&app).1 == 0 {
            assert!(Instant::now() < deadline, "the ball never landed");
            app.update();
        }
        app.update();
        let hidden: Vec<_> = clones(&mut app)
            .into_iter()
            .filter(|(_, parts, v, _)| *parts == 0 && *v == Visibility::Hidden)
            .collect();
        assert!(!hidden.is_empty(), "gone, hidden");
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(spawned(&app), (1, 1), "held: one throw");
        test_pad::button(&mut app, pad, GamepadButton::South, false);
        // The second attack's button (a right shoulder): its clip.
        test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
        for _ in 0..3 {
            app.update();
        }
        let second = guac_formats::script::builder::ATTACK_SECOND_SLOT as u32;
        assert_eq!(throws(&app), [arrival, throw, second]);
        // Leaving the level installs the run again, from its start.
        let steps = app
            .world()
            .resource::<guacamole::scripts::ScriptRuntime>()
            .steps;
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        until(&mut app, "the portal again", |a| mode(a) == Mode::Portal);
        app.update();
        let after = app
            .world()
            .resource::<guacamole::scripts::ScriptRuntime>()
            .steps;
        assert!(after < steps, "{after} steps after leaving, {steps} before");
    }

    /// The review of AH.24: a hero whose scripts went off after joining (a
    /// join that failed, his script stopped in play) walks on AG.16's
    /// controller (stand-in ledger: `walker`) and his attack buttons do
    /// nothing, though his file's placements the VM still ticks would throw
    /// on them: no clip, no clone.
    #[test]
    fn a_hero_whose_scripts_went_off_does_nothing_on_the_attack_buttons() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        write_attack_parts(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready"
                && a.world()
                    .contains_resource::<guacamole::scripts::ScriptRuntime>()
        });
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..3 {
            app.update();
        }
        let e = app.world().resource::<Play>().hero().unwrap();
        app.world_mut()
            .entity_mut(e)
            .get_mut::<HeroRun>()
            .unwrap()
            .join = Join::Off("the test put him on the stand-in".into());
        app.world_mut()
            .entity_mut(e)
            .remove::<crate::hero::ScriptedHero>();
        let log = |app: &App| {
            let steps = app
                .world()
                .resource::<crate::hero_scripts::ScriptLog>()
                .since(0);
            let started = steps.iter().map(|s| s.started.len()).sum::<usize>();
            let spawned = steps.iter().map(|s| s.spawned.len()).sum::<usize>();
            (started, spawned)
        };
        let before = log(&app);
        for button in [
            GamepadButton::South,
            GamepadButton::West,
            GamepadButton::RightTrigger,
        ] {
            test_pad::press(&mut app, pad, button);
            for _ in 0..10 {
                app.update();
            }
        }
        test_pad::button(&mut app, pad, GamepadButton::South, true);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(log(&app), before, "no clip and no clone off");
        assert_eq!(join(&app).as_deref(), Some("off"));
    }

    /// A walker on the shelf (AG.27): toy 26 ([`WALKER_TOY`]), the
    /// synthetic character the script part holds.
    const WALKER: &str = "026_Synthetic";

    /// The walker's made-up app: the attack parts, then the part holding the
    /// walker stopping as `fault` says, the level, a pad.
    fn walker_app(
        dir: &std::path::Path,
        fault: guac_formats::script::builder::WalkerFault,
    ) -> (App, Entity) {
        walker_app_with(dir, fault, false)
    }

    /// [`walker_app`], over AH.151's synthetic mover update when `mover`.
    fn walker_app_with(
        dir: &std::path::Path,
        fault: guac_formats::script::builder::WalkerFault,
        mover: bool,
    ) -> (App, Entity) {
        walker_app_writing(
            dir,
            fault,
            match mover {
                true => crate::test_support::hero_scripts::write_walker_moving,
                false => crate::test_support::hero_scripts::write_walker,
            },
        )
    }

    /// [`walker_app`] over the parts `write` writes.
    fn walker_app_writing(
        dir: &std::path::Path,
        fault: guac_formats::script::builder::WalkerFault,
        write: fn(&std::path::Path, guac_formats::script::builder::WalkerFault),
    ) -> (App, Entity) {
        walker_app_unmodelling(dir, fault, write, None)
    }

    /// [`walker_app_writing`] with `GUAC_TEST_UNMODEL` asking for `unmodel`
    /// (AG.221), as `main` hands the TV the key.
    fn walker_app_unmodelling(
        dir: &std::path::Path,
        fault: guac_formats::script::builder::WalkerFault,
        write: fn(&std::path::Path, guac_formats::script::builder::WalkerFault),
        unmodel: Option<&str>,
    ) -> (App, Entity) {
        let (mut app, tx) = play_app_walkers(dir, &[(WALKER_TOY, WALKER)]);
        if let Some(member) = unmodel {
            app.insert_resource(guacamole::unmodel::Unmodel(member.into()));
        }
        write_attack_parts(dir);
        // His palette, Eruptor's under his name: what the join sets on his
        // minion's template.
        let rel = IrFormat::Ron.palette_path("009_Eruptor");
        let mut palette: Palette =
            guac_ir::from_ron(&std::fs::read_to_string(dir.join(rel)).unwrap()).unwrap();
        palette.owner = WALKER.into();
        std::fs::write(
            dir.join(IrFormat::Ron.palette_path(WALKER)),
            guac_ir::to_ron(&palette).unwrap(),
        )
        .unwrap();
        // His attach list, Eruptor's under his name: what his hands pose.
        let rel = IrFormat::Ron.nodes_path("009_Eruptor");
        let mut nodes: AttachNodes =
            guac_ir::from_ron(&std::fs::read_to_string(dir.join(rel)).unwrap()).unwrap();
        nodes.load_file = WALKER.into();
        std::fs::write(
            dir.join(IrFormat::Ron.nodes_path(WALKER)),
            guac_ir::to_ron(&nodes).unwrap(),
        )
        .unwrap();
        write(dir, fault);
        tx.send(write_level(dir, &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready"
                && a.world()
                    .contains_resource::<guacamole::scripts::ScriptRuntime>()
        });
        (app, pad)
    }

    /// The kept cards the TV lists.
    fn kept(app: &mut App) -> Vec<Kept> {
        let mut shots = bevy::ecs::system::SystemState::<PlayShots>::new(app.world_mut());
        let shots = shots.get(app.world()).expect("the walk's params");
        shots.kept()
    }

    /// AG.27: a character with his own scripts in the part, no table of
    /// measured numbers, walks on what his setup wrote: his speed level and
    /// turn class into his description's movement parameters (ground max
    /// speed 18.5 ft/s, ground turn rate 720°/s) and his default scale
    /// (120 %) onto his actor, his solid and his drawn model standing at that
    /// scale, and the mover (his minion's tick, here) walking him along his
    /// facing at the speed it wrote, his scripts the only writer of it. His
    /// minion's script isn't named for his load file: his definition names
    /// it.
    #[test]
    fn a_walker_walks_on_what_his_own_setup_wrote() {
        use guac_formats::script::builder::{
            WALKER_MOVEMENT, WALKER_SCALE, WALKER_SPEED, WALKER_TURN, WalkerFault,
        };
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app(tmp.path(), WalkerFault::None);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        assert!(kept(&mut app).is_empty());
        for _ in 0..3 {
            app.update();
        }
        let e = app.world().resource::<Play>().hero().unwrap();
        let run = app.world().entity(e).get::<HeroRun>().unwrap().clone();
        assert_eq!(
            (run.origin, run.attacks),
            (Origin::Scripts, false),
            "his numbers are his scripts', his attack buttons do nothing"
        );
        // What the setup wrote: his movement parameters into his
        // description, his scale onto his actor.
        let Join::Joined(minion) = run.join else {
            panic!("{:?}", run.join);
        };
        let movement =
            guacamole::walkers::Movement::read(&app.world().resource::<ScriptRuntime>().vm, minion)
                .expect("his movement parameters");
        assert_eq!(movement.ground_max_speed, WALKER_SPEED);
        assert_eq!(movement.ground_turn_rate, WALKER_TURN);
        // Each member the fall-back reads is one his setup wrote, his
        // description's gravity not among them (AG.173: every mover falls
        // at the mover update's own).
        let written = WALKER_MOVEMENT.map(|(name, _)| name);
        let read = guacamole::walkers::Movement::MEMBERS;
        assert!(read.iter().all(|m| written.contains(m)), "{read:?}");
        assert!(!read.iter().any(|m| m.starts_with("gravity")), "{read:?}");
        let world = app.world().resource::<guacamole::scripts::ScriptWorld>();
        let actor = world.actor(e.to_bits()).expect("his actor");
        assert_eq!(actor.scale, [WALKER_SCALE; 3]);
        // His palette sits on the template his definition defines, which
        // isn't named for him.
        let template = app
            .world()
            .resource::<ScriptSource>()
            .walker(WALKER)
            .and_then(|w| w.template)
            .expect("his minion's template");
        assert!(world.palette(template).is_some(), "his palette's lengths");
        // His solid and his drawn model stand at that scale.
        let scale = WALKER_SCALE / 100.0;
        assert_eq!(run.scale, scale);
        let body = app.world().entity(e).get::<HeroBody>().unwrap();
        assert_eq!(
            body.hero.solid(),
            &run.shape.havok([WALKER_SCALE; 3]).unwrap(),
            "the solid is built at the scale his scripts set"
        );
        let root = run.root.expect("his drawn model's root");
        assert_eq!(
            app.world().entity(root).get::<Transform>().unwrap().scale,
            Vec3::splat(scale)
        );
        // His hands posed at that scale too (AG.181): each attach node his
        // model's pose puts it at, times his scripts' scale, not 1.0.
        let rig = run.rig.as_ref().expect("his attach nodes");
        assert_eq!(rig.scale, scale);
        let gaits = app.world().entity(e).get::<HeroGaits>().unwrap();
        let base = locomotion_clips(&rig.model, &rig.clips, gaits);
        let unscaled = guacamole::hands::posed(&rig.model.skeleton, &rig.nodes, &base, &[], 1.0);
        let world = app.world().resource::<guacamole::scripts::ScriptWorld>();
        let nodes = world.nodes(e.to_bits()).expect("his nodes posed");
        assert!(unscaled.iter().any(|n| n.at.length() > 0.1));
        for (posed, at_one) in nodes.iter().zip(&unscaled) {
            assert!(
                posed.at.distance(at_one.at * scale) < 1e-4,
                "{:?} for {:?} at {scale}",
                posed.at,
                at_one.at
            );
        }
        // The mover walks him along his facing at the speed it wrote: the
        // scripts' velocity, nothing of the controller's.
        assert!(
            app.world()
                .entity(e)
                .contains::<crate::hero::ScriptedHero>()
        );
        let from = hero(&app).pivot;
        let frames = 12;
        for _ in 0..frames {
            app.update();
        }
        let moved = (hero(&app).pivot - from).length();
        let want = WALKER_SPEED * guac_motion::tuning::FOOT * frames as f32 / 60.0;
        assert!(
            (moved - want).abs() < 0.05 * want,
            "moved {moved} m in {frames} steps, the mover wrote {want}"
        );
    }

    /// The `CollisionInfo` [`WALKER_SOLID`]'s writes leave on his actor, as
    /// the host keeps them (the offset in metres), turned as his file's
    /// is (his setup writes no turn).
    fn written_solid() -> SolidInfo {
        use guac_formats::script::builder::{WALKER_FILE_SOLID, WALKER_SOLID};
        guacamole::script_engine::Solid {
            shape: WALKER_SOLID.shape,
            width: WALKER_SOLID.width,
            height: WALKER_SOLID.height,
            length: WALKER_SOLID.length,
            offset: [0.0, WALKER_SOLID.y_offset * guac_motion::tuning::FOOT, 0.0],
            angles: WALKER_FILE_SOLID.2,
            ..Default::default()
        }
        .info()
    }

    /// His solid as the disc's hero side authors it, his file's too.
    fn file_solid() -> SolidInfo {
        Walk::scripted(WALKER, &hero_side(WALKER).solid).shape
    }

    /// The synthetic walker `write` writes, placed, in the level and
    /// joined, a few steps on: the app and his entity.
    fn joined_walker(
        dir: &std::path::Path,
        fault: guac_formats::script::builder::WalkerFault,
        write: fn(&std::path::Path, guac_formats::script::builder::WalkerFault),
    ) -> (App, Entity) {
        let (mut app, pad) = walker_app_writing(dir, fault, write);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..3 {
            app.update();
        }
        let e = app.world().resource::<Play>().hero().unwrap();
        (app, e)
    }

    /// AG.170: a walker whose setup writes a solid unlike his file's (a 7 x
    /// 10.5 x 3 ft capsule half a foot down, over the 4 x 6 x 3 ft sphere his
    /// file and the disc's hero side hold) stands and collides on the
    /// written one: his controller's solid is it at his scripts' 120 %, his
    /// pivot rides the offset's 0.18 m over the floor (the sphere's rides
    /// it), and the wall 6 m ahead holds him the capsule's 1.28 m off, not
    /// the sphere's 0.73 m. `/api/_test/tv`'s hero reports it.
    #[test]
    fn a_walker_stands_and_collides_on_the_solid_his_setup_wrote() {
        use guac_formats::script::builder::{WALKER_SCALE, WalkerFault};
        use guac_motion::seam::Shape;
        use guac_motion::tuning::FOOT;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, e) = joined_walker(
            tmp.path(),
            WalkerFault::None,
            crate::test_support::hero_scripts::write_walker_solid,
        );
        let written = written_solid();
        assert_ne!(written, file_solid(), "unlike his file's");
        let built = written.havok([WALKER_SCALE; 3]).unwrap();
        let Shape::Capsule { radius, .. } = built.shape else {
            panic!("{built:?}")
        };
        assert!((radius + 0.05 - 7.0 * FOOT * 1.2 / 2.0).abs() < 1e-5);
        // Standing: his lowest point on the floor where the offset puts it,
        // his pivot 0.5 ft x 120 % over it, as the sphere's rides 5 mm over.
        // Built where he stood on the sphere, the capsule reaches 0.18 m into
        // the floor, and the engine lifts him out at 1 m/s (none of the 66
        // characters' rebuilt solids reach under his pivot on the dump).
        for _ in 0..12 {
            app.update();
        }
        let lift = hero(&app).pivot.y - FLOOR;
        let offset = 0.5 * FOOT * 1.2;
        assert!(
            (lift - offset - 0.005).abs() < 2e-3,
            "{lift} m over the floor"
        );
        // Into the wall along his facing: held off it by the capsule's
        // radius and the triangles' 0.05 m, 5 mm short.
        for _ in 0..120 {
            app.update();
        }
        let at = hero(&app);
        let gap = START[2] + WALL - (at.pivot.z + radius + 0.05);
        assert!(
            (gap - 0.005).abs() < 1e-3,
            "{gap} m off the wall at {:?}",
            at.pivot
        );
        // What he stood and collided on: the written CollisionInfo at his
        // scripts' 120 %.
        let run = app.world().entity(e).get::<HeroRun>().unwrap().clone();
        assert_eq!(run.shape, written, "the CollisionInfo his setup wrote");
        assert_eq!(hero(&app).solid(), &built, "at his scripts' 120 %");
        // The hook's hero: that solid, and what it was built from.
        let report = {
            let mut state: bevy::ecs::system::SystemState<(
                Query<(&HeroBody, Option<&HeroGaits>, Option<&HeroRun>)>,
                Query<(&Transform, &Projection, &FollowCamera)>,
                Res<Play>,
            )> = bevy::ecs::system::SystemState::new(app.world_mut());
            let (bodies, cameras, play) = state.get(app.world()).unwrap();
            play.report(&bodies, &cameras, None, None)
                .expect("the hero")
        };
        // As the hook serves it.
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["solid"]["shape"], "capsule");
        assert_eq!(json["solid"]["built_from"]["width"], 7.0);
        let solid = report.solid.expect("his solid reported");
        assert_eq!(solid.shape, "capsule");
        assert_eq!(solid.radius, Some(radius));
        assert_eq!(solid.centre, built.centre.to_array());
        let from = solid.built_from.expect("what it was built from");
        assert_eq!(
            (
                from.kind.as_str(),
                from.width,
                from.height,
                from.length,
                from.scale
            ),
            ("capsule", 7.0, 10.5, 3.0, 1.2)
        );
        assert_eq!(from.offset, written.offset);
    }

    /// AG.170: a walker whose scripts set his scale again once his setup is
    /// done (150 %, eight ticks of his behaviour after his setup's 120 %)
    /// has his solid built again at it from what his setup wrote (the 7 x
    /// 10.5 ft capsule), not from his file's sphere, his drawn model and
    /// his hands at it too.
    #[test]
    fn a_walker_whose_scale_changes_after_his_setup_has_his_solid_rebuilt_at_it() {
        use guac_formats::script::builder::{WALKER_REGROW, WALKER_SCALE, WalkerFault};
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, e) = joined_walker(
            tmp.path(),
            WalkerFault::None,
            crate::test_support::hero_scripts::write_walker_regrow,
        );
        let written = written_solid();
        assert_eq!(
            hero(&app).solid(),
            &written.havok([WALKER_SCALE; 3]).unwrap()
        );
        until(&mut app, "his scale set again", |a| {
            a.world()
                .entity(e)
                .get::<HeroRun>()
                .is_some_and(|r| r.scale != WALKER_SCALE / 100.0)
        });
        let run = app.world().entity(e).get::<HeroRun>().unwrap().clone();
        let scale = WALKER_REGROW / 100.0;
        assert_eq!(run.scale, scale);
        let rebuilt = written.havok([WALKER_REGROW; 3]).unwrap();
        assert_ne!(rebuilt, file_solid().havok([WALKER_REGROW; 3]).unwrap());
        assert_eq!(hero(&app).solid(), &rebuilt, "the written one at 150 %");
        let root = run.root.expect("his drawn model's root");
        assert_eq!(
            app.world().entity(root).get::<Transform>().unwrap().scale,
            Vec3::splat(scale)
        );
        assert_eq!(run.rig.as_ref().map(|r| r.scale), Some(scale));
    }

    /// AG.170: once his script stops in play (AH.151's fall-back) he stands
    /// on the solid his scripts last set, the written capsule at their
    /// 120 %, nothing building it again, and the controller's edge probe
    /// reads its 7 ft width, not his file's 4 ft.
    #[test]
    fn a_walker_fallen_back_stands_on_the_solid_his_scripts_set() {
        use guac_formats::script::builder::{WALKER_SCALE, WalkerFault};
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, e) = joined_walker(
            tmp.path(),
            WalkerFault::InPlay,
            crate::test_support::hero_scripts::write_walker_solid,
        );
        until(&mut app, "his script stopping", |a| {
            join(a).as_deref() == Some("off")
        });
        for _ in 0..3 {
            app.update();
        }
        let written = written_solid();
        let h = hero(&app);
        assert_eq!(h.solid(), &written.havok([WALKER_SCALE; 3]).unwrap());
        assert_eq!(h.tuning().width, 7.0, "the width his scripts set");
        let run = app.world().entity(e).get::<HeroRun>().unwrap();
        assert_eq!((run.shape, run.scale), (written, 1.2));
    }

    /// AG.27: a walker whose setup stops on a member nothing models keeps
    /// his card, listed with why: he comes out of the level, the controller
    /// never walks him on a number of ours, and he isn't picked again while
    /// his figure stands on the portal; the figure leaving clears the list.
    /// He comes out by the deactivate path (AG.180, found at the AG.27
    /// merge check: this held under the run installed again as under the
    /// deactivation): his definition taken off the run that goes on, no
    /// activator left, his slot exited, the portal manager's deactivation
    /// run once.
    #[test]
    fn a_walker_whose_setup_stops_keeps_his_card_listed() {
        use guac_formats::script::builder::{ATTACK_DEACTIVATIONS, WALKER_DEF, WalkerFault};
        use guac_script::Value;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app(tmp.path(), WalkerFault::InSetup);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his card kept", |a| {
            !a.world().resource::<Play>().kept.is_empty()
        });
        app.update();
        let (def_obj, _) = placement_named(&app, WALKER_DEF);
        assert!(
            app.world().resource::<ScriptWorld>().taken_off(def_obj),
            "his definition off the run"
        );
        let order = app.world().resource::<ScriptRuntime>().vm.tick_order();
        assert!(order.iter().all(|(a, _)| !a), "no activator: {order:?}");
        assert_eq!(entered(&app), Some(Value::Int(0)), "his slot exited");
        assert_eq!(
            file_int(&app, ATTACK_DEACTIVATIONS),
            Some(Value::Int(1)),
            "the portal manager's deactivation"
        );
        let list = kept(&mut app);
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0].load_file, WALKER);
        assert!(
            list[0].reason.contains("unmodelled member"),
            "why: {}",
            list[0].reason
        );
        // His figure's slot with it (AH.166): what the e2e harness holds as
        // his scripts gone off, the hero gone before any read.
        assert_eq!(list[0].slot, Some(1));
        assert!(
            !list[0].reason.contains("no movement parameters"),
            "kept by the join's stage, not by a missing read: {}",
            list[0].reason
        );
        assert!(
            app.world().resource::<Play>().hero().is_none(),
            "he's out of the level"
        );
        // Not picked again while his figure stands there.
        for _ in 0..30 {
            app.update();
        }
        assert!(app.world().resource::<Play>().hero().is_none());
        assert!(app.world().resource::<Play>().candidate.is_none());
        // His figure off the portal clears it.
        clear(&mut app, 0);
        app.update();
        assert!(kept(&mut app).is_empty());
    }

    /// AG.183 (found at the AG.27 merge check): with the level's script
    /// part not loading, a character with no measured numbers keeps his
    /// card listed with why, his figure's slot with it, where he kept it in
    /// no list; and the TV leaving the level clears the level's list, the
    /// next entry's run a fresh one.
    #[test]
    fn a_walker_on_a_level_whose_scripts_didnt_load_keeps_his_card_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app_walkers(tmp.path(), &[(WALKER_TOY, WALKER)]);
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        until(&mut app, "its scripts' reason", |a| {
            a.world()
                .resource::<Level>()
                .report()
                .scripts_reason
                .is_some()
        });
        assert!(kept(&mut app).is_empty(), "nothing placed");
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        for _ in 0..10 {
            app.update();
        }
        let list = kept(&mut app);
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(
            (list[0].load_file.as_str(), list[0].slot),
            (WALKER, Some(1))
        );
        assert!(list[0].reason.contains("didn't load"), "{}", list[0].reason);
        assert!(app.world().resource::<Play>().candidate.is_none());
        assert_eq!(mode(&app), Mode::Portal);
        clear(&mut app, 0);
        app.update();
        assert!(kept(&mut app).is_empty(), "his figure gone");
    }

    /// AG.183: the level's kept list goes when the TV leaves the level.
    #[test]
    fn the_levels_kept_cards_go_when_the_tv_leaves_it() {
        use guac_formats::script::builder::WalkerFault;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app(tmp.path(), WalkerFault::InSetup);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his card kept", |a| {
            !a.world().resource::<Play>().kept.is_empty()
        });
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        assert!(app.world().resource::<Play>().kept.is_empty());
    }

    /// AH.165 on the TV: a walker whose deactivation stops (his shutdown,
    /// which the portal manager's deactivation runs, on a member nothing
    /// models) still comes off the run, and Eruptor placed next joins it
    /// with the portal manager ticking on: before, the stop left the
    /// walker's file running and stopped the portal manager, whose tick runs
    /// combat, the hurts and the health write, for the rest of the level
    /// while the next hero read `joined`.
    #[test]
    fn a_walker_whose_deactivation_stops_leaves_the_portal_manager_ticking() {
        use guac_formats::script::builder::{WALKER_DEF, WalkerFault};
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app(tmp.path(), WalkerFault::InDeactivation);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..3 {
            app.update();
        }
        let (def_obj, def) = placement_named(&app, WALKER_DEF);
        let old_def = app
            .world()
            .resource::<ScriptRuntime>()
            .vm
            .placement_of(def)
            .unwrap();
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        app.update();
        let vm = &app.world().resource::<ScriptRuntime>().vm;
        assert!(
            vm.log()
                .iter()
                .any(|l| l.text.contains(guacamole::scripts::PORTAL_MANAGER)
                    && l.text.contains("unmodelled member")),
            "his shutdown stopped in the deactivation"
        );
        assert!(vm.removed(old_def), "his definition off the run");
        assert!(app.world().resource::<ScriptWorld>().taken_off(def_obj));
        let before = pm_seconds(&app);
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "Eruptor's scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..10 {
            app.update();
        }
        let after = pm_seconds(&app);
        assert!(
            after > before,
            "the portal manager ticks on: {before} -> {after}"
        );
        let w = app.world();
        let vm = &w.resource::<ScriptRuntime>().vm;
        let pm = guacamole::scripts::portal_manager_placement(vm, w.resource::<ScriptWorld>());
        assert!(vm.stopped(pm.unwrap()).is_none());
    }

    /// AG.27 with AH.107: a walker whose scripts ask a point against the
    /// gameplay viewport (the host answers every point "on screen"), or
    /// whose definition defines no minion, keeps his card from the portal
    /// on, before the level: never a hero, listed with why, and the portal
    /// calls for a Skylander as for any hand that can't play.
    #[test]
    fn a_walker_asking_the_viewport_keeps_his_card_at_the_portal() {
        use guac_formats::script::builder::WalkerFault;
        for (fault, why) in [
            (WalkerFault::Asks, "viewport"),
            (WalkerFault::NoMinion, "no minion"),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, _pad) = walker_app(tmp.path(), fault);
            place(&mut app, 0, WALKER_TOY, "Synthetic");
            for _ in 0..30 {
                app.update();
            }
            assert!(app.world().resource::<Play>().candidate.is_none());
            assert_eq!(mode(&app), Mode::Portal);
            let list = kept(&mut app);
            assert_eq!(list.len(), 1, "{list:?}");
            assert_eq!(list[0].load_file, WALKER);
            assert!(list[0].reason.contains(why), "{}", list[0].reason);
            assert_eq!(list[0].slot, None, "the part's own list: no join");
        }
    }

    /// AG.27: attacks stay Eruptor's alone. The walker's attack buttons do
    /// nothing though his scripts run (stand-in ledger: `who-attacks`): no
    /// action reaches the VM's pad.
    #[test]
    fn a_walkers_attack_buttons_do_nothing() {
        use guac_formats::script::builder::WalkerFault;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app(tmp.path(), WalkerFault::None);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for button in [GamepadButton::South, GamepadButton::West] {
            test_pad::press(&mut app, pad, button);
            for _ in 0..5 {
                app.update();
            }
        }
        let held = app
            .world()
            .resource::<crate::hero_scripts::ScriptLog>()
            .since(0)
            .iter()
            .any(|s| s.actions != [false; 4]);
        assert!(!held, "no attack reached his scripts");
    }

    /// AG.27: a walker whose script stops in play (after his setup ran
    /// through) walks on, on AG.16's controller, at what his scripts had
    /// set: the movement parameters they wrote into his description become
    /// the controller's numbers (never his actor's engine ones), falling as
    /// every mover falls (AG.173: 136.21 ft/s² to 120, not the 96 to 90 his
    /// description holds), his scale stays theirs, and he's listed nowhere
    /// (his card isn't kept). AH.151: over a mover update that hands the
    /// engine, each tick, the walk his stopped minion no longer keeps (still,
    /// as the original's #608-#610 do), the stick still takes him to his top
    /// speed: the update's writes to his walk are dropped and counted.
    #[test]
    fn a_walker_whose_script_stops_in_play_walks_on_at_what_it_set() {
        use guac_formats::script::builder::{MOVER_PLACEMENT, WALKER_MOVEMENT, WalkerFault};
        use guacamole::scripts::{HeroWrite, ScriptWorld};
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walker_app_with(tmp.path(), WalkerFault::InPlay, true);
        place(&mut app, 0, WALKER_TOY, "Synthetic");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        until(&mut app, "his script stopping", |a| {
            join(a).as_deref() == Some("off")
        });
        let e = app.world().resource::<Play>().hero().expect("still in");
        let reason = match &app.world().entity(e).get::<HeroRun>().unwrap().join {
            Join::Off(why) => why.clone(),
            other => panic!("{other:?}"),
        };
        assert!(reason.contains("unmodelled member"), "{reason}");
        assert!(
            kept(&mut app).is_empty(),
            "he walks on, his card isn't kept"
        );
        assert!(
            !app.world()
                .entity(e)
                .contains::<crate::hero::ScriptedHero>()
        );
        let run = app.world().entity(e).get::<HeroRun>().unwrap();
        let [speed, push, let_go, turn, air_let_go, ..] = WALKER_MOVEMENT.map(|(_, v)| v);
        assert_eq!(
            *hero(&app).tuning(),
            guac_motion::tuning::Tuning {
                max_speed: speed,
                push,
                let_go,
                air_let_go,
                turn_rate: turn,
                gravity: 136.21,
                terminal: 120.0,
                width: run.shape.width,
            },
            "what the scripts wrote into his description, a mover's fall"
        );
        assert_eq!(run.scale, 1.2, "his scale stays his scripts'");
        // The stick held: up to his top speed on the controller, though
        // the update writes him still each tick.
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        let mut top = 0.0f32;
        for _ in 0..60 {
            app.update();
            top = top.max(hero(&app).walk.velocity.length());
        }
        let walk = hero(&app).walk.velocity.length();
        assert!((walk - speed).abs() < 1e-3, "{walk} ft/s, top {top}");
        let world = app.world().resource::<ScriptWorld>();
        assert!(world.walks_alone(e.to_bits()));
        let (_, mover) = placement_named(&app, MOVER_PLACEMENT);
        let dropped = world.hero_writes();
        assert!(
            dropped
                .get(&(mover, HeroWrite::Walk))
                .is_some_and(|&n| n >= 60),
            "the update's writes to his walk dropped: {dropped:?}"
        );
    }

    /// AG.221: `GUAC_TEST_UNMODEL` takes a registered member out of the TV's
    /// run, so a walker whose minion reaches it in play stops there and
    /// falls back as the disc's walkers did before AG.160: the same walker
    /// without the key runs on, joined, and the key naming no registered
    /// member leaves the run as it was.
    #[test]
    fn a_walker_stops_in_play_on_the_member_the_key_takes_out() {
        use guac_formats::script::builder::WalkerFault;
        const MEMBER: &str = "redoctaneperipheralscriptobject.size of region 1";
        let fault = WalkerFault::InPlayOn("redoctaneperipheralscriptobject", "size of region 1");
        let run = |unmodel: Option<&str>| {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, pad) = walker_app_unmodelling(
                tmp.path(),
                fault,
                crate::test_support::hero_scripts::write_walker,
                unmodel,
            );
            place(&mut app, 0, WALKER_TOY, "Synthetic");
            until(&mut app, "the level", |a| mode(a) != Mode::Portal);
            test_pad::press(&mut app, pad, GamepadButton::East);
            until(&mut app, "his scripts", |a| {
                join(a).as_deref() == Some("joined")
            });
            // Well past the tick his minion reaches the member on.
            for _ in 0..90 {
                app.update();
            }
            let e = app.world().resource::<Play>().hero().expect("still in");
            app.world().entity(e).get::<HeroRun>().unwrap().join.clone()
        };
        match run(Some(MEMBER)) {
            Join::Off(why) => assert!(
                // The registry has forgotten its name: the halt names its hashes.
                why.contains("#97 OpSetValue: unmodelled member 04b51d94.d81e088c"),
                "{why}"
            ),
            other => panic!("with the member out: {other:?}"),
        }
        let none = run(None);
        assert!(matches!(none, Join::Joined(_)), "the member in: {none:?}");
        // Refused: not registered, so the run is the one without the key.
        assert!(matches!(run(Some("synthetic.nothing")), Join::Joined(_)));
        assert!(matches!(run(Some("")), Join::Joined(_)));
    }

    /// AG.219: a walker whose script stops in play walks drawn on his own
    /// scripts' locomotion: over a part whose locomotion script keeps his
    /// walk (slot 1) and run (slot 3) as the original's keeps a walker's
    /// (`LOCOMOTION_STATES`), AG.30's locomotion that draws him on AG.16's
    /// controller mixes his hero side's walk and run by speed at their
    /// speeds, his run whole at his top speed at its rate, his gait report's
    /// run rate a number; over the part without them it draws his idle at
    /// any speed (AG.27's, no walk or run of ours).
    #[test]
    fn a_walker_fallen_back_walks_and_runs_on_his_scripts_definitions() {
        use guac_formats::script::builder::{LOCOMOTION_STATES, WALKER_SPEED, WalkerFault};
        use guac_motion::locomotion::{Clip as Gait, Gaits};
        let fallen_back = |write: fn(&std::path::Path, WalkerFault)| {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, pad) = walker_app_writing(tmp.path(), WalkerFault::InPlay, write);
            // His palette holding his hero side's walk and run at their
            // slots, as a walker's does (the synthetic one his attacks').
            let rel = tmp.path().join(IrFormat::Ron.palette_path(WALKER));
            let mut palette: Palette =
                guac_ir::from_ron(&std::fs::read_to_string(&rel).unwrap()).unwrap();
            let side = crate::test_support::play::hero_side(WALKER).locomotion;
            for c in [side.walk, side.run].into_iter().flatten() {
                palette.entries.push(guac_ir::palette::PaletteSlot {
                    slot: c.slot,
                    name: None,
                    label: None,
                    entry: guac_ir::palette::SlotEntry::Clip {
                        additive: false,
                        partial: false,
                        layered: false,
                        motion_extraction: 0,
                        clip: c.clip,
                    },
                });
            }
            palette.entries.sort_by_key(|e| e.slot);
            std::fs::write(&rel, guac_ir::to_ron(&palette).unwrap()).unwrap();
            place(&mut app, 0, WALKER_TOY, "Synthetic");
            until(&mut app, "the level", |a| mode(a) != Mode::Portal);
            test_pad::press(&mut app, pad, GamepadButton::East);
            until(&mut app, "his scripts", |a| {
                join(a).as_deref() == Some("joined")
            });
            until(&mut app, "his script stopping", |a| {
                join(a).as_deref() == Some("off")
            });
            test_pad::stick(&mut app, pad, 0.0, 1.0);
            for _ in 0..90 {
                app.update();
            }
            let e = app.world().resource::<Play>().hero().expect("in");
            let report = app
                .world()
                .entity(e)
                .get::<HeroGaits>()
                .map(GaitReport::of_gaits)
                .expect("his gaits");
            (gaits(&app), report, hero(&app).walk.velocity.length())
        };
        let (own, report, walk) =
            fallen_back(crate::test_support::hero_scripts::write_walker_locomotion);
        assert!((walk - WALKER_SPEED).abs() < 1e-3, "{walk}");
        let (w, r) = (LOCOMOTION_STATES[1].1, LOCOMOTION_STATES[2].1);
        assert_eq!(
            *own.gaits(),
            Gaits {
                walk_speed: w,
                run_speed: r
            }
        );
        assert!(own.loops().walk.is_some() && own.loops().run.is_some());
        assert_eq!(own.weights(), [0.0, 0.0, 1.0, 0.0], "his run whole");
        let rate = own.play(Gait::Run).rate;
        assert!((rate - WALKER_SPEED / r).abs() < 1e-5, "{rate}");
        assert_eq!(report.clip, "run");
        assert!(report.rates.iter().all(|r| r.is_finite()), "{report:?}");
        let (before, report, _) =
            fallen_back(crate::test_support::hero_scripts::write_walker_moving);
        assert_eq!(before.weights(), [1.0, 0.0, 0.0, 0.0], "his idle, before");
        assert_eq!(report.clip, "idle");
    }

    /// AH.24: a character whose attacks don't run (here no script part
    /// linked) walks on AG.16's controller (stand-in ledger: `walker`), and
    /// its attack buttons do nothing: no join, no clip, no clone.
    #[test]
    fn a_character_whose_attacks_dont_run_does_nothing_on_the_attack_buttons() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level", |a| level_state(a) == "ready");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "walking", |a| mode(a) == Mode::Level);
        assert_eq!(join(&app), None, "no scripts to join");
        for button in [
            GamepadButton::South,
            GamepadButton::West,
            GamepadButton::East,
            GamepadButton::RightTrigger,
            GamepadButton::LeftTrigger2,
        ] {
            test_pad::press(&mut app, pad, button);
            for _ in 0..10 {
                app.update();
            }
        }
        assert!(clones(&mut app).is_empty());
        assert!(
            app.world()
                .resource::<crate::hero_scripts::ScriptLog>()
                .since(0)
                .is_empty()
        );
        // The stick still walks him.
        let from = hero(&app).pivot;
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..30 {
            app.update();
        }
        assert!(hero(&app).pivot.distance(from) > 1.0);
    }

    // ---- AH.23: his figure in his scripts on the TV --------------------

    /// AH.23 on the TV: his figure in his scripts. Placed by Alice at 999
    /// experience, Eruptor joins with his figure's tag on the portal holding
    /// it; the synthetic second attack's award (a point, through `Game -
    /// Award Experience`) lands in his figure's words, the TV reads it back
    /// after the step onto his slot's values, `/api/_test/tv`'s report reads
    /// it, and it goes to the map under the generation he was placed under
    /// and from there to the row: 1,000, level 2. Placed with no profile
    /// unlocked, the same award lands in his words and on the TV and
    /// nothing goes to the map.
    #[test]
    fn a_scripts_award_reaches_his_figure_and_the_row() {
        use guac_server::progress::{Drainer, ProgressSink, ProgressionSlot};
        use skylander_rpcs3_control::{FigureProgress, PlacementOwner};

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (store, alice, fresh) = rt.block_on(async {
            let store =
                guac_server::profiles::ProfileStore::open_in_memory(&guac_server::setup::MIGRATOR)
                    .await
                    .unwrap();
            let alice = store.create("Alice", "1234", "#e63946").await.unwrap();
            let fresh = store
                .instances()
                .fresh(&alice, &format!("{ERUPTOR:06x}-0000"))
                .await
                .unwrap();
            (store, alice, fresh.progress)
        });
        for owned in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, tx) = play_app(tmp.path());
            write_attack_parts(tmp.path());
            write_figure_rows(tmp.path());
            let sink = ProgressSink::new();
            app.insert_resource(crate::progress::ProgressOut(sink.clone()));
            tx.send(write_level(tmp.path(), &lights())).unwrap();
            let pad = test_pad::connect(&mut app, None, None);
            until(&mut app, "the level and its scripts", |a| {
                level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
            });
            let mut change = placed(0, ERUPTOR, "Eruptor");
            if owned && let PortalChange::Placed { placement, .. } = &mut change {
                placement.owner = Some(PlacementOwner {
                    profile_id: alice.clone(),
                    initial: "A".into(),
                    color: "#e63946".into(),
                    progress: FigureProgress { xp: 999, ..fresh },
                });
            }
            crate::test_support::turntable::set(&mut app, [change.clone()]);
            app.world_mut().write_message(PortalChanged(change));
            until(&mut app, "the level", |a| mode(a) != Mode::Portal);
            test_pad::press(&mut app, pad, GamepadButton::East);
            until(&mut app, "his scripts", |a| {
                join(a).as_deref() == Some("joined")
            });
            let held = |app: &App| {
                let e = app.world().resource::<crate::progress::Earning>();
                e.slots[0].as_ref().unwrap().progress
            };
            // `/api/_test/tv`'s hero, as the mirror builds it.
            let reported = |app: &mut App| {
                let mut state: bevy::ecs::system::SystemState<(
                    Query<(&HeroBody, Option<&HeroGaits>, Option<&HeroRun>)>,
                    Query<(&Transform, &Projection, &FollowCamera)>,
                    Option<Res<ScriptRuntime>>,
                    Option<Res<crate::progress::Earning>>,
                    Res<Play>,
                )> = bevy::ecs::system::SystemState::new(app.world_mut());
                let (bodies, cameras, runtime, earning, play) = state.get(app.world()).unwrap();
                play.report(&bodies, &cameras, runtime.as_deref(), earning.as_deref())
                    .and_then(|r| r.experience)
            };
            let start = if owned { 999 } else { 0 };
            let e = app.world().resource::<Play>().hero().unwrap();
            let run = app.world().entity(e).get::<HeroRun>().unwrap().clone();
            let (data, generation) = run.figure.expect("his figure's tag on the portal");
            assert_eq!(generation, if owned { fresh.generation } else { 0 });
            let rt_vm = &app.world().resource::<ScriptRuntime>().vm;
            let layout =
                guacamole::level_scripts::figure_layout(tmp.path(), IrFormat::Ron).unwrap();
            assert_eq!(
                guacamole::figure::read(rt_vm, &layout, data).map(|v| v.xp),
                Some(start),
                "his words made from his slot's values"
            );
            assert_eq!(held(&app).xp, start);
            assert_eq!(reported(&mut app), Some(start));
            let _ = sink.waiting();
            test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
            for _ in 0..3 {
                app.update();
            }
            let rt_vm = &app.world().resource::<ScriptRuntime>().vm;
            assert_eq!(
                guacamole::figure::read(rt_vm, &layout, data).map(|v| v.xp),
                Some(start + 1),
                "the award in his words"
            );
            assert_eq!(held(&app).xp, start + 1, "read back onto his slot");
            assert_eq!(reported(&mut app), Some(start + 1));
            let waiting = sink.waiting();
            if !owned {
                assert_eq!(waiting, vec![], "an ownerless figure writes nothing");
                continue;
            }
            assert_eq!(waiting.len(), 1, "{waiting:?}");
            assert_eq!(
                (waiting[0].2.progress.xp, waiting[0].2.progress.generation),
                (1_000, fresh.generation)
            );
            let row = rt.block_on(async {
                let (events, _) = tokio::sync::broadcast::channel(16);
                let slot = ProgressionSlot::default();
                slot.publish(guac_server::progress::Progression {
                    level_xp: vec![0, 1_000],
                    gold_cap: 65_000,
                });
                Drainer::new(sink.clone(), store.clone(), events, slot.clone())
                    .drain()
                    .await;
                let row = store
                    .instances()
                    .read(&alice, &format!("{ERUPTOR:06x}-0000"))
                    .await
                    .unwrap()
                    .unwrap();
                (row.progress, slot.get().unwrap().level_for(row.progress.xp))
            });
            assert_eq!((row.0.xp, row.1), (1_000, 2), "the row at level 2");
        }
    }

    // ---- AH.27: the breakables on the TV --------------------------------

    /// Where the synthetic barrel stands: 3 m ahead of [`START`] along his
    /// facing, on the made-up floor.
    const BARREL_AT: [f32; 3] = [START[0], FLOOR, START[2] + 3.0];

    /// AH.27's parts beside [`write_attack_parts`]'s, after
    /// [`write_level`]'s: the level's entry AH.27's breakable at
    /// [`BARREL_AT`], its placement in the level's placements part at the
    /// same object with its solid (a prop that stands for its solid alone),
    /// and the figure's rows.
    fn write_smash_parts(dir: &std::path::Path) {
        use guac_formats::script::builder::{SMASH_BARREL, SMASH_BARREL_SIZE};
        write_attack_parts(dir);
        let barrel = crate::test_support::hero_scripts::write_smash(dir, BARREL_AT);
        write_figure_rows(dir);
        let [width, height, length] = SMASH_BARREL_SIZE;
        let solid = guac_ir::actor::Solid {
            shape: guac_ir::actor::Shape::Cylinder,
            solid: true,
            world: false,
            actor_surface: false,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width,
            height,
            length,
            weight: 1.0,
            rebound: 0.0,
        };
        let placement = guac_ir::level::Placement {
            object: barrel,
            id: 0,
            name: SMASH_BARREL.into(),
            script: None,
            position: BARREL_AT,
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: None,
            solid: Some(solid),
            movement: None,
            waypoints: None,
            activity_state: 4,
            activation_range: 160.0,
        };
        crate::test_support::level::write_props(dir, Vec::new(), vec![placement]);
    }

    /// The level's props as `/api/_test/tv` reports them.
    fn props_report(app: &App) -> crate::level::PropsReport {
        app.world()
            .resource::<Level>()
            .snapshot()
            .props
            .expect("the level's props")
    }

    /// AH.27 on the TV: Eruptor, Alice's at 5 gold, smashes the synthetic
    /// barrel. The TV's listing runs it (the activity rule waking it, its
    /// solid a prop in Avian's world that the scripts' engine maps to its
    /// placement, so the ball stops on it short of its middle); a press
    /// throws, the ball takes the barrel off and its prop goes down for
    /// good (hidden, its solid out of Avian's world, `/api/_test/tv`'s
    /// props listing it taken off); its coin pays 3 into his figure's
    /// words, read back onto his slot and sent under his generation: 8.
    /// Taken off and placed again, he comes back to the level as he left
    /// it (openspec AH.105: the run went on): the barrel still off, the same
    /// placement, nothing to break, the gold his figure's. The HUD's gold
    /// reads the row's 5, then 8 once the coin's in (AH.71).
    #[test]
    fn eruptor_smashes_the_synthetic_barrel_and_its_coin_reaches_the_row() {
        use guac_formats::script::builder::{
            SMASH_BARREL, SMASH_FAR, SMASH_FAR_COIN, SMASH_GOLD, SMASH_SETUP,
        };
        use skylander_rpcs3_control::{FigureProgress, PlacementOwner};

        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        let source = write_level(tmp.path(), &lights());
        write_smash_parts(tmp.path());
        let sink = guac_server::progress::ProgressSink::new();
        app.insert_resource(crate::progress::ProgressOut(sink.clone()));
        tx.send(source).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
        });
        // The activity rule from the level's start before he's in: the
        // barrel 3 m off it awake, the far one 63 m off asleep.
        for _ in 0..3 {
            app.update();
        }
        let placed_named = |app: &App, name: &str| {
            let source = app.world().resource::<ScriptSource>();
            let unit = source.level_unit().unwrap();
            let world = app.world().resource::<ScriptWorld>();
            let obj = world
                .handles()
                .keys()
                .find(|o| o.unit == unit && world.name(world.handles()[o]) == name)
                .copied()
                .unwrap();
            (obj, world.handles()[&obj])
        };
        let active = |app: &App, name: &str| {
            let (_, h) = placed_named(app, name);
            app.world()
                .resource::<ScriptWorld>()
                .actor(h)
                .unwrap()
                .active
        };
        assert!(active(&app, SMASH_BARREL), "from the start, before he's in");
        assert!(!active(&app, SMASH_FAR), "out of its range");
        // A scripted one out of range sleeps in the VM too: never ticked.
        let asleep = |app: &App, name: &str| {
            let (_, h) = placed_named(app, name);
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            vm.asleep(vm.placement_of(h).expect("it runs"))
        };
        assert!(asleep(&app, SMASH_FAR_COIN), "out of its range");
        let generation = 4;
        let owned = |slot: u8| {
            let mut change = placed(slot, ERUPTOR, "Eruptor");
            if let PortalChange::Placed { placement, .. } = &mut change {
                placement.owner = Some(PlacementOwner {
                    profile_id: "alice".into(),
                    initial: "A".into(),
                    color: "#e63946".into(),
                    progress: FigureProgress {
                        xp: 0,
                        gold: 5,
                        playtime_secs: 0,
                        generation,
                    },
                });
            }
            change
        };
        let change = owned(0);
        crate::test_support::turntable::set(&mut app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        let (barrel_obj, barrel) = placed_named(&app, SMASH_BARREL);
        let (setup_obj, _) = placed_named(&app, SMASH_SETUP);
        assert!(
            app.world().resource::<ScriptWorld>().taken_off(setup_obj),
            "the setup took itself off at the start"
        );
        assert!(!active(&app, SMASH_FAR), "out of his range");
        assert!(asleep(&app, SMASH_FAR_COIN), "out of his range");
        assert!(
            app.world()
                .resource::<ScriptWorld>()
                .actor(barrel)
                .unwrap()
                .active,
            "awake: he's within its range"
        );
        assert!(props_report(&app).names.contains(&SMASH_BARREL.to_owned()));
        assert!(props_report(&app).taken_off.is_empty());
        // Its solid is its placement to the scripts' engine, both ways.
        let prop_body = guac_motion::seam::BodyId(guacamole::props::PROP_BODIES + barrel_obj.index);
        let standing = |app: &App| {
            let w = app.world();
            crate::hero_scripts::standing_props(
                w.resource::<Level>(),
                w.resource::<ScriptWorld>(),
                w.get_resource::<ScriptSource>(),
            )
        };
        let props = standing(&app);
        assert_eq!(
            crate::hero_scripts::body_of(&props, barrel),
            Some(prop_body)
        );
        assert_eq!(
            crate::hero_scripts::actor_of(&props, prop_body),
            Some(barrel)
        );
        assert_eq!(
            crate::hero_scripts::actor_of(&props, guac_motion::seam::BodyId(7)),
            None
        );
        // So the scripts' engine answers.
        {
            use guacamole::script_engine::Engine;
            let mut state: bevy::ecs::system::SystemState<(
                Query<(Entity, &'static mut HeroBody)>,
                AvianSeam,
            )> = bevy::ecs::system::SystemState::new(app.world_mut());
            let (mut heroes, seam) = state.get_mut(app.world_mut()).unwrap();
            let engine = crate::hero_scripts::TvEngine::new(&mut heroes, &seam, 0.0, &props);
            assert_eq!(engine.body(barrel), Some(prop_body));
            assert_eq!(engine.actor_of(prop_body), Some(barrel));
        }
        let _ = sink.waiting();
        let hud_gold = |app: &App| {
            let hud = app.world().resource::<crate::hud::Hud>();
            hud.shown.as_ref().map(|v| v.gold)
        };
        assert_eq!(hud_gold(&app), Some(5), "the HUD's gold: the row's");
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "the barrel broken", |a| {
            a.world().resource::<ScriptWorld>().taken_off(barrel_obj)
        });
        // The ball stopped on its solid, short of its middle.
        let world = app.world().resource::<ScriptWorld>();
        let ball = world
            .clones()
            .into_iter()
            .find(|&c| world.name(c) == guac_formats::script::builder::ATTACK_BALL)
            .unwrap();
        let at = world.actor(ball).unwrap().position;
        assert!(world.removed(ball));
        assert!(
            at.z < BARREL_AT[2] - 0.3 && at.y > FLOOR + 0.3,
            "it met the barrel's face: {at}"
        );
        for _ in 0..3 {
            app.update();
        }
        let r = props_report(&app);
        assert_eq!(r.taken_off, [SMASH_BARREL]);
        assert!(!r.names.contains(&SMASH_BARREL.to_owned()), "down: {r:?}");
        let mut bodies = app.world_mut().query::<&crate::physics::SeamBody>();
        assert!(
            bodies.iter(app.world()).all(|b| b.id != prop_body),
            "its solid out of Avian's world"
        );
        assert_eq!(
            crate::hero_scripts::body_of(&standing(&app), barrel),
            None,
            "nor the engine's"
        );
        until(&mut app, "its coin's gold on his slot", |a| {
            let e = a.world().resource::<crate::progress::Earning>();
            e.slots[0].as_ref().unwrap().progress.gold == 5 + SMASH_GOLD as u32
        });
        let waiting = sink.waiting();
        assert_eq!(
            waiting
                .last()
                .map(|w| (w.2.progress.gold, w.2.progress.generation)),
            Some((5 + SMASH_GOLD as u32, generation)),
            "{waiting:?}"
        );
        app.update();
        assert_eq!(
            hud_gold(&app),
            Some(5 + SMASH_GOLD as u32),
            "the HUD's gold: the row's"
        );
        // Off and on again: the level as he left it.
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        app.update();
        let change = owned(2);
        crate::test_support::turntable::set(&mut app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
        until(&mut app, "his scripts again", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..5 {
            app.update();
        }
        let world = app.world().resource::<ScriptWorld>();
        assert!(world.taken_off(barrel_obj), "still off: the run went on");
        // The setup ran once, at the level's start, and stays off.
        let setup = world.handles()[&setup_obj];
        let vm = &app.world().resource::<ScriptRuntime>().vm;
        let p = vm.placement_of(setup).unwrap();
        assert_eq!(vm.local(p, "ran"), Some(guac_script::Value::Int(1)));
        assert!(world.taken_off(setup_obj) && vm.removed(p));
        // The level's placements are the run's own still.
        let again = world.handles()[&barrel_obj];
        assert_eq!(again, barrel, "the same run");
        let r = props_report(&app);
        assert!(!r.names.contains(&SMASH_BARREL.to_owned()));
        let e = app.world().resource::<crate::progress::Earning>();
        assert_eq!(
            e.slots[2].as_ref().unwrap().progress.gold,
            5 + SMASH_GOLD as u32,
            "his figure's gold came back with him"
        );

        // A second swap: the barrel still off, the run the same.
        clear(&mut app, 2);
        until(&mut app, "the call again", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        app.update();
        let change = owned(0);
        crate::test_support::turntable::set(&mut app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
        until(&mut app, "his scripts a third time", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..5 {
            app.update();
        }
        let world = app.world().resource::<ScriptWorld>();
        assert!(world.taken_off(barrel_obj), "still off after two swaps");
        assert_eq!(world.handles()[&barrel_obj], barrel, "the same run");
        assert!(world.taken_off(setup_obj));
        assert!(!props_report(&app).names.contains(&SMASH_BARREL.to_owned()));

        // The activity rule follows him once he's in, not the level's start:
        // set down 33 m short of the far barrel (no floor there; a step or
        // two of falling), it and the far coin wake, the coin in the VM too.
        assert!(!active(&app, SMASH_FAR), "out of his range");
        let e = app.world().resource::<Play>().hero().unwrap();
        let near_far = Vec3::new(START[0], FLOOR + 1.0, START[2] + 30.0);
        app.world_mut()
            .entity_mut(e)
            .get_mut::<HeroBody>()
            .unwrap()
            .hero
            .pivot = near_far;
        until(&mut app, "the far barrel woken by him", |a| {
            active(a, SMASH_FAR)
        });
        assert!(!asleep(&app, SMASH_FAR_COIN), "the far coin runs");
    }

    // ---- AG.29 over AH.24: the portal's holds and his scripts ------------

    /// The steps the run has made since it was installed.
    fn run_steps(app: &App) -> u64 {
        app.world().resource::<ScriptRuntime>().steps
    }

    /// What the scripts did after the log's step `since`: the palette slots
    /// started on the hero, and how many clones were made and taken off.
    fn attacks_since(app: &App, since: u64) -> (Vec<u32>, usize, usize) {
        let steps = app
            .world()
            .resource::<crate::hero_scripts::ScriptLog>()
            .since(since);
        (
            steps
                .iter()
                .flat_map(|s| s.started.iter().map(|q| q.slot))
                .collect(),
            steps.iter().map(|s| s.spawned.len()).sum(),
            steps.iter().map(|s| s.gone.len()).sum(),
        )
    }

    /// AG.29 over AH.24: every alert's hold keeps his attacks from his
    /// scripts. Eruptor joined, each alert in turn (a toy Giants can't use,
    /// a second character, a second magic item) holds the level: the run
    /// makes no step, Start pauses nothing, and the attack buttons pressed
    /// under it, tapped inside a frame or held through it start no clip and
    /// spawn nothing, under it or once it's off; a fresh press then throws.
    #[test]
    fn every_alerts_hold_keeps_his_attacks_from_his_scripts() {
        use guac_formats::script::builder::ATTACK_THROW_SLOT;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = joined_eruptor(tmp.path());
        let throw = ATTACK_THROW_SLOT as u32;
        let holds: [(&[(u8, u32, &str)], Alert); 3] = [
            (&[(7, FRYNO, "Fryno")], Alert::WrongToy),
            (&[(3, ONE, "One")], Alert::TooManySkylanders),
            (
                &[(5, ITEM, "Five"), (6, SIDEKICK, "Six")],
                Alert::TooManyItems,
            ),
        ];
        for (figures, want) in holds {
            for &(slot, toy, name) in figures {
                place(&mut app, slot, toy, name);
            }
            until(&mut app, "the hold", |a| {
                mode(a) == Mode::Held && alert(a) == Some(want)
            });
            // The hold's first frame runs the steps already due (AG.20).
            app.update();
            let (since, steps) = (log_last(&app), run_steps(&app));
            for button in [
                GamepadButton::South,
                GamepadButton::West,
                GamepadButton::RightTrigger,
                GamepadButton::Start,
            ] {
                test_pad::press(&mut app, pad, button);
                app.update();
            }
            test_pad::button(&mut app, pad, GamepadButton::East, true);
            test_pad::button(&mut app, pad, GamepadButton::East, false);
            app.update();
            test_pad::button(&mut app, pad, GamepadButton::South, true);
            for _ in 0..30 {
                app.update();
            }
            assert_eq!(mode(&app), Mode::Held);
            assert_eq!(
                app.world().resource::<LevelPads>().seat().unwrap().pause(),
                None,
                "{want:?}: Start paused nothing"
            );
            assert_eq!(run_steps(&app), steps, "{want:?}: no step under it");
            assert_eq!(attacks_since(&app, since), (vec![], 0, 0), "{want:?}");
            for &(slot, ..) in figures {
                clear(&mut app, slot);
            }
            until(&mut app, "the walk again", |a| mode(a) == Mode::Level);
            for _ in 0..40 {
                app.update();
            }
            assert!(run_steps(&app) > steps + 30, "{want:?}: the run on again");
            assert_eq!(
                attacks_since(&app, since).0,
                Vec::<u32>::new(),
                "{want:?}: South held through it throws nothing"
            );
            assert_eq!(attacks_since(&app, since).1, 0, "{want:?}");
            test_pad::button(&mut app, pad, GamepadButton::South, false);
            app.update();
            test_pad::press(&mut app, pad, GamepadButton::South);
            until(&mut app, "a fresh press throws", |a| {
                attacks_since(a, since).0 == [throw] && attacks_since(a, since).2 == 1
            });
            for _ in 0..40 {
                app.update();
            }
        }
    }

    /// AG.29 over AH.24 on the deactivate path (openspec AH.105): the hero
    /// taken off takes his scripts off the run (his minion and his file's
    /// placements off it, the synthetic portal manager's deactivation run
    /// once, his helper drawn no more), the level's run going on (its steps
    /// counting on, the portal manager's timer carrying on, no step while
    /// the level holds past its first frame); Eruptor placed again comes in
    /// where he stood and joins that run, his file's request the first step
    /// the level runs again (no init chain to wait for): his minion the one
    /// activator, ticking first, his health and full health off his
    /// scripts, his helper drawn again (one), his walk his scripts' (the
    /// stick moves him nowhere) and the bottom button throws. The TV's hero,
    /// the one whose scripts run and the one his scripts move are the same
    /// entity.
    #[test]
    fn the_hero_in_where_the_last_stood_joins_his_scripts() {
        use guac_formats::script::builder::{
            ATTACK_DEACTIVATIONS, ATTACK_HEALTH, ATTACK_HEALTH_MAX, ATTACK_THROW_SLOT,
        };
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = joined_eruptor(tmp.path());
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(clones(&mut app).len(), 1, "his helper");
        let first = app.world().resource::<Play>().hero().unwrap();
        let stood = hero(&app).pivot;
        let before = run_steps(&app);
        let pm_timer = |app: &App| {
            let w = app.world();
            let vm = &w.resource::<ScriptRuntime>().vm;
            let pm = guacamole::scripts::portal_manager_placement(vm, w.resource::<ScriptWorld>());
            vm.active_seconds(pm.expect("the portal manager runs"))
                .unwrap()
        };
        let timer = pm_timer(&app);
        let deactivations = |app: &App| {
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            let g = vm.program().global_named(ATTACK_DEACTIVATIONS).unwrap();
            vm.global_peek(g)
        };
        assert_eq!(deactivations(&app), None, "none yet");
        let old_minion = {
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            vm.placement_of(first.to_bits()).unwrap()
        };
        let since = log_last(&app);
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        app.update();
        let held = run_steps(&app);
        assert!(
            held >= before,
            "the run went on: {held} steps, {before} before"
        );
        assert!(
            pm_timer(&app) >= timer,
            "the portal manager's timer carries on"
        );
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(run_steps(&app), held, "no step under the hold");
        assert!(
            clones(&mut app).iter().all(|(draws, ..)| !draws),
            "his helper went with him"
        );
        {
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            assert!(vm.removed(old_minion), "his minion off the run");
            assert_eq!(deactivations(&app), Some(guac_script::Value::Int(1)));
        }
        let mut runs = app.world_mut().query::<&HeroRun>();
        assert_eq!(runs.iter(app.world()).count(), 0);
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "his scripts again", |a| {
            join(a).as_deref() == Some("joined")
        });
        let next = app.world().resource::<Play>().hero().unwrap();
        assert_ne!(next, first);
        assert!(hero(&app).pivot.distance(stood) < 1e-3, "where he stood");
        let mut runs = app.world_mut().query::<(Entity, &HeroRun)>();
        let runs: Vec<Entity> = runs.iter(app.world()).map(|(e, _)| e).collect();
        assert_eq!(runs, [next], "one run, on the TV's hero");
        assert!(
            app.world()
                .entity(next)
                .contains::<crate::hero::ScriptedHero>()
        );
        let joins: Vec<Option<&str>> = app
            .world()
            .resource::<crate::hero_scripts::ScriptLog>()
            .since(since)
            .iter()
            .map(|s| s.join)
            .collect();
        // The old run's last steps (the hold's first frame running those
        // due, with no hero), then his file's request the first step he's
        // in, his activation the next.
        let after: Vec<Option<&str>> = joins
            .iter()
            .copied()
            .filter(Option::is_some)
            .skip_while(|j| *j == Some("joined"))
            .take(2)
            .collect();
        assert_eq!(after, [Some("requested"), Some("joined")], "{joins:?}");
        assert!(pm_timer(&app) > timer, "the same portal manager, on");
        let run = app.world().entity(next).get::<HeroRun>().unwrap().clone();
        let Join::Joined(minion) = run.join else {
            unreachable!("joined above")
        };
        let vm = &app.world().resource::<ScriptRuntime>().vm;
        let order = vm.tick_order();
        assert_eq!(order[0], (true, "009_Eruptor".to_owned()), "his first");
        assert_eq!(order.iter().filter(|(a, _)| *a).count(), 1, "the one");
        assert!(vm.is_activator(minion) && !vm.is_activator(old_minion));
        let numbers = crate::hero_scripts::hero_numbers(vm, minion);
        assert_eq!(
            (numbers.health, numbers.health_max),
            (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX))
        );
        app.update();
        let drawn: Vec<_> = clones(&mut app)
            .into_iter()
            .filter(|(draws, ..)| *draws)
            .collect();
        assert_eq!(
            drawn,
            [(true, 1, Visibility::Inherited, false)],
            "his helper again, one"
        );
        // His scripts the one writer: the stick moves him nowhere.
        let still = hero(&app).pivot;
        test_pad::stick(&mut app, pad, 0.0, 1.0);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(hero(&app).pivot, still, "the stick isn't his walk's writer");
        test_pad::stick(&mut app, pad, 0.0, 0.0);
        let since = log_last(&app);
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "his throw", |a| {
            attacks_since(a, since).0 == [ATTACK_THROW_SLOT as u32]
                && attacks_since(a, since).1 == 1
        });
        let world = app.world().resource::<ScriptWorld>();
        assert_eq!(
            crate::hero_scripts::playing_on(world, next.to_bits()).len(),
            1,
            "his throw plays on the TV's hero"
        );
    }

    /// His health and full health as his scripts hold them, once joined.
    fn health(app: &App) -> (Option<i32>, Option<i32>) {
        let e = app.world().resource::<Play>().hero().expect("in the level");
        let run = app.world().entity(e).get::<HeroRun>().expect("his run");
        let Join::Joined(minion) = run.join else {
            panic!("not joined: {}", run.join.name());
        };
        let numbers =
            crate::hero_scripts::hero_numbers(&app.world().resource::<ScriptRuntime>().vm, minion);
        (numbers.health, numbers.health_max)
    }

    /// AH.62 over AH.20 and AG.29, on the deactivate path (openspec
    /// AH.105): his health goes off the portal with his figure and comes
    /// back with it, kept by his scripts as the original's portal manager
    /// keeps a figure's (the synthetic file keeps it where his minion
    /// writes it each tick). Bitten to 100 of his 150 (the test hook doing a
    /// bite's work), taken off and put back in the same level, his scripts
    /// read 100 once he's joined again and keep it; the TV leaving the level
    /// (a level change) installs the run again with nothing kept, so he
    /// comes back in full.
    #[test]
    fn his_health_goes_off_the_portal_and_back_with_his_figure() {
        use guac_formats::script::builder::{ATTACK_HEALTH, ATTACK_HEALTH_MAX, ATTACK_KEPT_HEALTH};
        let kept = |app: &App| {
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            let g = vm.program().global_named(ATTACK_KEPT_HEALTH).unwrap();
            vm.global_peek(g)
        };
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = joined_eruptor(tmp.path());
        app.update();
        assert_eq!(health(&app), (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX)));
        let asked = crate::hero_scripts::AskedHealth::default();
        app.insert_resource(asked.clone());
        *asked.0.lock().unwrap() = Some(100);
        app.update();
        assert_eq!(health(&app), (Some(100), Some(ATTACK_HEALTH_MAX)), "bitten");
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            kept(&app),
            Some(guac_script::Value::Int(100)),
            "kept in the run that went on"
        );
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "his scripts again", |a| {
            join(a).as_deref() == Some("joined")
        });
        assert_eq!(
            health(&app),
            (Some(100), Some(ATTACK_HEALTH_MAX)),
            "as he left"
        );
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(health(&app).0, Some(100), "his scripts keep it");
        // A level change: the TV leaves the level and enters it again.
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert!(
            kept(&app).is_none_or(|v| v == guac_script::Value::Int(0)),
            "cleared with the level"
        );
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Portal {
            game: "Skylanders Giants".into(),
        };
        until(&mut app, "the level again", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts in the new entry", |a| {
            join(a).as_deref() == Some("joined")
        });
        assert_eq!(
            health(&app),
            (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX)),
            "in full"
        );
    }

    // ---- AH.26, AH.33, AH.57, AH.71: defeat, the HUD, the level's end ----

    /// `profile`'s Eruptor in `slot`, placed with `progress`.
    fn owned_eruptor(slot: u8, profile: &str, progress: FigureProgress) -> PortalChange {
        let mut change = placed(slot, ERUPTOR, "Eruptor");
        if let PortalChange::Placed { placement, .. } = &mut change {
            placement.owner = Some(skylander_rpcs3_control::PlacementOwner {
                profile_id: profile.into(),
                initial: profile[..1].to_uppercase(),
                color: "#e63946".into(),
                progress,
            });
        }
        change
    }

    /// `change` onto the portal, as the driver tells the TV.
    fn put(app: &mut App, change: PortalChange) {
        crate::test_support::turntable::set(app, [change.clone()]);
        app.world_mut().write_message(PortalChanged(change));
    }

    /// The joined hero's health set where his scripts hold it, as a bite
    /// leaves it (AH.62's hook's work).
    fn set_health(app: &mut App, health: i32) {
        let e = app.world().resource::<Play>().hero().expect("in the level");
        let Join::Joined(minion) = app.world().entity(e).get::<HeroRun>().unwrap().join else {
            panic!("not joined");
        };
        let mut rt = app.world_mut().resource_mut::<ScriptRuntime>();
        assert!(guacamole::scripts::set_health(&mut rt.vm, minion, health));
    }

    /// Frames until `done`, at most `frames` of them: a loop the tests can
    /// run past `until`'s 5 s of the wall clock (game seconds at 60 frames
    /// each).
    fn frames_until(app: &mut App, frames: usize, what: &str, done: impl Fn(&App) -> bool) {
        for _ in 0..frames {
            if done(app) {
                return;
            }
            app.update();
        }
        assert!(done(app), "{what}: not after {frames} frames");
    }

    fn game_secs(app: &App) -> f32 {
        app.world()
            .resource::<Time<Virtual>>()
            .elapsed()
            .as_secs_f32()
    }

    fn real_secs(app: &App) -> f32 {
        app.world().resource::<Time<Real>>().elapsed().as_secs_f32()
    }

    /// Start's pause held `frames` frames, then the bottom button resumes:
    /// the game clock that far behind the real one, as on a TV that has sat
    /// paused, so a test can tell which clock a wait counts on.
    fn sit_paused(app: &mut App, pad: Entity, frames: usize) {
        let lag = real_secs(app) - game_secs(app);
        test_pad::press(app, pad, GamepadButton::Start);
        app.update();
        assert_eq!(mode(app), Mode::Paused);
        for _ in 0..frames {
            app.update();
        }
        test_pad::press(app, pad, GamepadButton::South);
        until(app, "resumed", |a| mode(a) == Mode::Level);
        let behind = real_secs(app) - game_secs(app) - lag;
        assert!(
            behind >= frames as f32 / 60.0,
            "the game clock {behind} s further behind"
        );
    }

    fn hud(app: &App) -> crate::hud::HudValues {
        app.world()
            .resource::<crate::hud::Hud>()
            .shown
            .clone()
            .expect("the HUD shows")
    }

    /// AH.26's beaten figure ruled: alone on the portal it asks for rest,
    /// counting for nothing; another character that walks comes in beside
    /// it; one beside a dying hero waits, and two beside him hold the
    /// level as two beside any hero do.
    #[test]
    fn a_beaten_figure_counts_for_nothing_and_one_beside_a_dying_hero_waits() {
        let beaten = |characters: &[(u8, bool)], n: usize| Lineup {
            beaten: n,
            ..lineup(characters, 0, 0)
        };
        assert_eq!(
            judge(&beaten(&[], 1), true, None, false),
            Some(Alert::NeedsRest)
        );
        assert_eq!(judge(&beaten(&[(4, true)], 1), true, None, false), None);
        assert_eq!(
            judge(&beaten(&[(4, false)], 1), true, None, false),
            Some(Alert::NeedsRest),
            "one that can't walk can't take over"
        );
        assert_eq!(
            judge(&beaten(&[], 0), true, None, false),
            Some(Alert::PlaceSkylander)
        );
        let dying = |characters: &[(u8, bool)]| Lineup {
            dying: true,
            ..lineup(characters, 0, 0)
        };
        let hero = Some(slot(0));
        assert_eq!(
            judge(&dying(&[(0, true), (3, true)]), true, hero, false),
            None
        );
        assert_eq!(
            judge(
                &dying(&[(0, true), (3, true), (5, true)]),
                true,
                hero,
                false
            ),
            Some(Alert::TooManySkylanders)
        );
        assert_eq!(judge(&dying(&[(0, true)]), true, hero, false), None);
    }

    /// AH.26 on the synthetic part, which runs no death sequence: his health
    /// out, he's dying, the level running on; the backstop takes him out 3 s
    /// of game time on, where he stood kept, his figure beaten for the
    /// level and the portal asking for it to come off (the level held, the
    /// HUD gone). Off, the call for a Skylander; put back, the needs-rest
    /// alert again and he stays out; another profile's Eruptor comes in
    /// where he fell, his scripts joining at full health.
    #[test]
    fn a_beaten_hero_leaves_and_his_figure_is_out_for_the_level() {
        use guac_formats::script::builder::{ATTACK_HEALTH, ATTACK_HEALTH_MAX};
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = joined_eruptor(tmp.path());
        app.update();
        assert!(app.world().resource::<crate::hud::Hud>().shown.is_some());
        // 6 s paused first: the backstop counts game time, not the real
        // clock that ran on under the pause.
        sit_paused(&mut app, pad, 360);
        set_health(&mut app, 0);
        app.update();
        let play = app.world().resource::<Play>();
        assert!(play.dying(), "his health ran out");
        assert_eq!(mode(&app), Mode::Level, "the level runs on while he dies");
        let since = game_secs(&app);
        let at = hero(&app).pivot;
        frames_until(&mut app, 400, "he's out", |a| {
            a.world().resource::<Play>().hero().is_none()
        });
        let took = game_secs(&app) - since;
        assert!(
            (3.0..3.0 + 3.0 / 60.0).contains(&took),
            "the backstop's 3 s of game time: {took}"
        );
        app.update();
        let play = app.world().resource::<Play>();
        assert_eq!(play.beaten(), [(None, format!("{ERUPTOR:06x}-0000"))]);
        assert!(!play.dying());
        assert_eq!(alert(&app), Some(Alert::NeedsRest));
        assert_eq!(mode(&app), Mode::Held);
        assert!(app.world().resource::<crate::hud::Hud>().shown.is_none());
        // Off: the call for a Skylander. Back on: rest, and he stays out.
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        place(&mut app, 2, ERUPTOR, "Eruptor");
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(alert(&app), Some(Alert::NeedsRest));
        assert!(app.world().resource::<Play>().hero().is_none());
        assert_eq!(app.world().resource::<Play>().candidate_model(), None);
        // Bob's Eruptor: another instance, in where Alice's fell.
        put(&mut app, owned_eruptor(4, "bob", FigureProgress::default()));
        until(&mut app, "Bob's scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        assert_eq!(alert(&app), None);
        let pivot = hero(&app).pivot;
        assert!(
            (pivot - at).xz().length() < 0.15,
            "in where he fell: {pivot} against {at}"
        );
        assert_eq!(health(&app), (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX)));
        assert_eq!(
            app.world().resource::<Play>().beaten().len(),
            1,
            "Alice's still out"
        );
        // Bob's off (no defeat): Alice's asks for rest again, and no
        // Restart chapter comes up, however long it holds.
        clear(&mut app, 4);
        until(&mut app, "rest", |a| alert(a) == Some(Alert::NeedsRest));
        for _ in 0..420 {
            app.update();
        }
        let now = app.world().resource::<Time<Real>>().elapsed();
        assert!(!app.world().resource::<Play>().restart_offered(now));
        assert!(!app.world().resource::<crate::input::ModalOpen>().0);
    }

    /// AH.26: where his scripts' death sequence takes his minion off (the
    /// original's 2.2 s on, AH.20's `a_defeated_hero_knocks_them_back_and_leaves`
    /// on the disc), he's out the frame it does, ahead of the backstop.
    #[test]
    fn a_beaten_hero_leaves_the_frame_his_minion_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = joined_eruptor(tmp.path());
        set_health(&mut app, 0);
        app.update();
        let since = game_secs(&app);
        for _ in 0..30 {
            app.update();
        }
        assert!(app.world().resource::<Play>().hero().is_some(), "dying");
        let e = app.world().resource::<Play>().hero().unwrap();
        assert!(
            app.world_mut()
                .resource_mut::<ScriptRuntime>()
                .vm
                .remove(e.to_bits()),
            "his minion off the run"
        );
        app.update();
        assert!(app.world().resource::<Play>().hero().is_none(), "out");
        assert!(game_secs(&app) - since < 1.0);
        assert_eq!(app.world().resource::<Play>().beaten().len(), 1);
    }

    /// AH.26: a dying hero lifted off the portal before he's out is beaten
    /// all the same (his health ran out); put back alone, he asks for rest.
    #[test]
    fn a_dying_hero_lifted_is_beaten_all_the_same() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = joined_eruptor(tmp.path());
        set_health(&mut app, 0);
        app.update();
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        assert_eq!(
            app.world().resource::<Play>().beaten(),
            [(None, format!("{ERUPTOR:06x}-0000"))]
        );
        assert!(!app.world().resource::<Play>().dying());
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "rest", |a| alert(a) == Some(Alert::NeedsRest));
    }

    /// AH.26: a character placed beside the dying hero waits (no too-many
    /// hold, the level running so his death runs out) and comes in where he
    /// fell once he's out, the needs-rest alert never showing.
    #[test]
    fn a_character_placed_beside_a_dying_hero_waits_and_comes_in() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _pad) = joined_eruptor(tmp.path());
        set_health(&mut app, 0);
        app.update();
        let at = hero(&app).pivot;
        put(&mut app, owned_eruptor(3, "bob", FigureProgress::default()));
        let mut alerts = Vec::new();
        frames_until(&mut app, 600, "Bob's scripts", |a| {
            join(a).as_deref() == Some("joined")
                && a.world()
                    .resource::<Play>()
                    .hero_placement()
                    .is_some_and(|(_, s, _)| s == slot(3))
        });
        for _ in 0..3 {
            alerts.push(alert(&app));
            app.update();
        }
        assert_eq!(alerts, [None; 3]);
        assert!((hero(&app).pivot - at).xz().length() < 0.15);
        assert_eq!(app.world().resource::<Play>().beaten().len(), 1);
    }

    /// The figures' progress written whole before the level goes (AH.33):
    /// the TV leaving the level asks the drainer for the flush, and on the
    /// store the row then holds the playtime short of its minute, which no
    /// drain before the flush wrote.
    #[test]
    fn the_level_ending_writes_every_figures_progress_first() {
        use guac_server::progress::{Drainer, ProgressSink, ProgressionSlot};

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let figure = format!("{ERUPTOR:06x}-0000");
        let (store, alice, fresh) = rt.block_on(async {
            let store =
                guac_server::profiles::ProfileStore::open_in_memory(&guac_server::setup::MIGRATOR)
                    .await
                    .unwrap();
            let alice = store.create("Alice", "1234", "#e63946").await.unwrap();
            let fresh = store.instances().fresh(&alice, &figure).await.unwrap();
            (store, alice, fresh.progress)
        });
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        write_attack_parts(tmp.path());
        write_figure_rows(tmp.path());
        let sink = ProgressSink::new();
        app.insert_resource(crate::progress::ProgressOut(sink.clone()));
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
        });
        put(&mut app, owned_eruptor(0, &alice, fresh));
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..120 {
            app.update();
        }
        let (events, _) = tokio::sync::broadcast::channel(16);
        let slot = ProgressionSlot::default();
        let mut drainer = Drainer::new(sink.clone(), store.clone(), events, slot);
        let row = |rt: &tokio::runtime::Runtime| {
            rt.block_on(async {
                store
                    .instances()
                    .read(&alice, &figure)
                    .await
                    .unwrap()
                    .unwrap()
                    .progress
            })
        };
        rt.block_on(drainer.drain());
        assert_eq!(row(&rt).playtime_secs, 0, "short of a minute: not written");
        assert!(!sink.flush_asked());
        // The level ends: the TV off the portal view.
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert!(sink.flush_asked(), "the level's end asks for the flush");
        let played = app.world().resource::<crate::progress::Earning>().slots[0]
            .as_ref()
            .unwrap()
            .progress
            .playtime_secs;
        assert!(played >= 2, "{played}");
        rt.block_on(drainer.drain());
        assert_eq!(row(&rt).playtime_secs, played, "the flush wrote it");
    }

    /// AH.26's Restart chapter, keeping the rows: Alice's Eruptor earns a
    /// point, is beaten, and once Restart chapter is on offer (5 s on, by
    /// the real clock) and confirmed, the level ends (its flush asked,
    /// AH.33) and is entered again from its start: him at the start in a
    /// fresh run at full health, no one beaten, his figure's experience and
    /// gold as they were and the row the drainer writes the same.
    #[test]
    fn restart_chapter_starts_the_level_again_keeping_every_figures_progress() {
        use guac_formats::script::builder::{ATTACK_HEALTH, ATTACK_HEALTH_MAX};
        use guac_server::progress::{Drainer, ProgressSink, ProgressionSlot};

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let figure = format!("{ERUPTOR:06x}-0000");
        let (store, alice, fresh) = rt.block_on(async {
            let store =
                guac_server::profiles::ProfileStore::open_in_memory(&guac_server::setup::MIGRATOR)
                    .await
                    .unwrap();
            let alice = store.create("Alice", "1234", "#e63946").await.unwrap();
            let fresh = store.instances().fresh(&alice, &figure).await.unwrap();
            (store, alice, fresh.progress)
        });
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        write_attack_parts(tmp.path());
        write_figure_rows(tmp.path());
        let sink = ProgressSink::new();
        app.insert_resource(crate::progress::ProgressOut(sink.clone()));
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
        });
        let start = FigureProgress {
            xp: 41,
            gold: 7,
            ..fresh
        };
        put(&mut app, owned_eruptor(0, &alice, start));
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        let at_start = hero(&app).pivot;
        test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
        let earned = |app: &App| {
            app.world().resource::<crate::progress::Earning>().slots[0]
                .as_ref()
                .unwrap()
                .progress
        };
        until(&mut app, "his point", |a| earned(a).xp == 42);
        // 6 s paused, so the offer's 5 s are told apart from game time.
        sit_paused(&mut app, pad, 360);
        set_health(&mut app, 0);
        let zero = real_secs(&app);
        let offered = |app: &App| {
            let now = app.world().resource::<Time<Real>>().elapsed();
            app.world().resource::<Play>().restart_offered(now)
        };
        frames_until(&mut app, 400, "beaten", |a| {
            alert(a) == Some(Alert::NeedsRest)
        });
        assert!(!offered(&app), "not yet: 5 s from his health's end");
        frames_until(&mut app, 400, "the offer", offered);
        let waited = real_secs(&app) - zero;
        assert!(
            (5.0..5.1).contains(&waited),
            "5 s of the real clock from his health's end: {waited}"
        );
        assert!(app.world().resource::<crate::input::ModalOpen>().0);
        let before = earned(&app);
        assert_eq!((before.xp, before.gold), (42, 7));
        assert!(!sink.flush_asked());
        app.world_mut().write_message(RestartLevel);
        app.update();
        // The flush is read the frame it's written or the next, whichever
        // side of the walk's systems the progress' run.
        app.update();
        assert!(sink.flush_asked(), "every figure's progress written first");
        assert_eq!(mode(&app), Mode::Portal, "the level ended");
        until(&mut app, "the level again", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts in the new run", |a| {
            join(a).as_deref() == Some("joined")
        });
        app.update();
        let play = app.world().resource::<Play>();
        assert!(play.beaten().is_empty(), "no one beaten in the new run");
        assert!(!play.restart_offered(Duration::MAX));
        assert!(!app.world().resource::<crate::input::ModalOpen>().0);
        assert_eq!(alert(&app), None);
        assert_eq!(
            health(&app),
            (Some(ATTACK_HEALTH), Some(ATTACK_HEALTH_MAX)),
            "full"
        );
        assert!(
            (hero(&app).pivot - at_start).length() < 0.01,
            "at the start"
        );
        let after = earned(&app);
        assert_eq!((after.xp, after.gold), (42, 7), "his progress kept");
        let row = rt.block_on(async {
            let (events, _) = tokio::sync::broadcast::channel(16);
            Drainer::new(
                sink.clone(),
                store.clone(),
                events,
                ProgressionSlot::default(),
            )
            .drain()
            .await;
            store
                .instances()
                .read(&alice, &figure)
                .await
                .unwrap()
                .unwrap()
                .progress
        });
        assert_eq!((row.xp, row.gold), (42, 7), "the row kept");
    }

    /// AH.26's Restart chapter puts the level's breakables back, the props'
    /// report with them: AH.27's synthetic barrel broken, the level started
    /// again, it stands and nothing reads as taken off (the Mac's run,
    /// 2026-10-26, read the two coins the dev entry had taken off still
    /// listed after a restart).
    #[test]
    fn a_restart_puts_the_broken_barrel_back() {
        use guac_formats::script::builder::SMASH_BARREL;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        let source = write_level(tmp.path(), &lights());
        write_smash_parts(tmp.path());
        tx.send(source).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
        });
        put(
            &mut app,
            owned_eruptor(0, "alice", FigureProgress::default()),
        );
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "the barrel down", |a| {
            props_report(a).taken_off == [SMASH_BARREL]
        });
        assert!(!props_report(&app).names.contains(&SMASH_BARREL.to_owned()));
        app.world_mut().write_message(RestartLevel);
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal, "the level ended");
        until(&mut app, "the level again", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts in the new run", |a| {
            join(a).as_deref() == Some("joined")
        });
        for _ in 0..3 {
            app.update();
        }
        let r = props_report(&app);
        assert!(r.taken_off.is_empty(), "{r:?}");
        assert!(
            r.names.contains(&SMASH_BARREL.to_owned()),
            "it stands: {r:?}"
        );
    }

    /// The HUD on the TV app (AH.26, AH.57, AH.71) over AH.27's synthetic
    /// barrel, Alice's Eruptor off the first slot (the HUD reads the hero's
    /// own slot's values) at 999 experience and 5 gold on a three-level
    /// table: his scripts' 120 of 150, level 1 and 999 toward 1,000; a bite
    /// to 100 shows 100 of 150 and its 20 by him, red, then white, gone a
    /// second on, the bar's edge flashing white; 30 (20 %) no warning, 29
    /// the warning; the barrel's coin 5 to 8; the second attack's point
    /// level 2, 1,000 toward 2,200; the table cut to two levels, MAX for
    /// both, and a point past it held at the cap.
    #[test]
    fn the_hud_follows_a_bite_a_coin_and_a_level_up() {
        use crate::hud::LevelShown;
        use guac_formats::script::builder::SMASH_GOLD;
        use guac_server::progress::{Progression, ProgressionSlot};

        let tmp = tempfile::tempdir().unwrap();
        let (mut app, tx) = play_app(tmp.path());
        let source = write_level(tmp.path(), &lights());
        write_smash_parts(tmp.path());
        let table = ProgressionSlot::default();
        table.publish(Progression {
            level_xp: vec![0, 1_000, 2_200],
            gold_cap: 65_000,
        });
        app.insert_resource(crate::progress::ProgressionIn(table.clone()));
        let asked = crate::hero_scripts::AskedHealth::default();
        app.insert_resource(asked.clone());
        tx.send(source).unwrap();
        let pad = test_pad::connect(&mut app, None, None);
        until(&mut app, "the level and its scripts", |a| {
            level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
        });
        let row = FigureProgress {
            xp: 999,
            gold: 5,
            playtime_secs: 0,
            generation: 4,
        };
        put(&mut app, owned_eruptor(2, "alice", row));
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        test_pad::press(&mut app, pad, GamepadButton::East);
        until(&mut app, "his scripts", |a| {
            join(a).as_deref() == Some("joined")
        });
        app.update();
        let v = hud(&app);
        assert_eq!(v.health, Some((120, 150)));
        assert_eq!(
            (v.level, v.xp, v.xp_next, v.gold),
            (LevelShown::Level(1), 999, Some(1_000), 5)
        );
        assert_eq!(v.xp_text(), "999 / 1,000 XP");
        assert!(!v.low());
        // A bite.
        let bite = |app: &mut App, health: i32| {
            *asked.0.lock().unwrap() = Some(health);
            app.update();
            app.update();
        };
        bite(&mut app, 100);
        let shown = app.world().resource::<crate::hud::Hud>();
        assert_eq!(hud(&app).health, Some((100, 150)));
        assert_eq!(
            shown.damage.iter().map(|d| d.amount).collect::<Vec<_>>(),
            [20]
        );
        let now = app.world().resource::<Time<Real>>().elapsed();
        assert!(shown.damage[0].red(now) && shown.flashing(now));
        for _ in 0..20 {
            app.update();
        }
        let shown = app.world().resource::<crate::hud::Hud>();
        let now = app.world().resource::<Time<Real>>().elapsed();
        assert!(!shown.damage[0].red(now), "white by now");
        assert!(!shown.flashing(now));
        for _ in 0..45 {
            app.update();
        }
        assert!(app.world().resource::<crate::hud::Hud>().damage.is_empty());
        // The warning under 20 %.
        bite(&mut app, 30);
        assert!(!hud(&app).low(), "30 of 150 is 20 %");
        bite(&mut app, 29);
        assert!(hud(&app).low());
        // A coin.
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "the coin's gold on the HUD", |a| {
            hud(a).gold == 5 + SMASH_GOLD as u32
        });
        // A level-up.
        test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
        until(&mut app, "level 2", |a| {
            hud(a).level == LevelShown::Level(2)
        });
        let v = hud(&app);
        assert_eq!((v.xp, v.xp_next), (1_000, Some(2_200)));
        assert_eq!(v.xp_text(), "1,000 / 2,200 XP");
        // At the cap: MAX, and a point past it held there.
        table.publish(Progression {
            level_xp: vec![0, 1_000],
            gold_cap: 65_000,
        });
        app.update();
        let v = hud(&app);
        assert_eq!(
            (v.level, v.xp_text(), v.level_text()),
            (LevelShown::Max, "MAX".into(), "LEVEL MAX".into())
        );
        test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(hud(&app).xp, 1_000, "held at the cap");
        let e = app.world().resource::<crate::progress::Earning>();
        assert_eq!(e.slots[2].as_ref().unwrap().progress.xp, 1_000);
        // The experience hook (AH.26's, for the Mac's level-up): 2,199 in
        // his figure's words on the three-level table reads back as level 2
        // one short of 2,200, and the next point is the level-up to MAX.
        table.publish(Progression {
            level_xp: vec![0, 1_000, 2_200],
            gold_cap: 65_000,
        });
        let xp = crate::hero_scripts::AskedExperience::default();
        app.insert_resource(xp.clone());
        *xp.0.lock().unwrap() = Some(2_199);
        until(&mut app, "the hook's experience read back", |a| {
            hud(a).xp == 2_199
        });
        let v = hud(&app);
        assert_eq!(
            (v.level, v.xp_text()),
            (LevelShown::Level(2), "2,199 / 2,200 XP".into())
        );
        test_pad::press(&mut app, pad, GamepadButton::RightTrigger);
        until(&mut app, "the level-up to the cap", |a| {
            hud(a).level == LevelShown::Max
        });
        assert_eq!(hud(&app).xp, 2_200);
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
        crate::test_support::turntable::set(&mut app, [cleared.clone()]);
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

    // ---- AH.36: the hero's slot, told the server ------------------------

    /// AH.36: the slot the server's slot reset refuses is the hero's while
    /// he's in the level: none before the level starts, his when he comes
    /// in, none while the portal holds the level with no hero in it, the
    /// replacement's once he's in, and none once the TV leaves the level.
    #[test]
    fn the_heros_slot_is_told_the_server_while_hes_in_the_level() {
        let tmp = tempfile::tempdir().unwrap();
        let hero = guac_server::progress::LevelHero::default();
        let out = LevelHeroOut(hero.clone());
        let (mut app, tx) = play_app_with(tmp.path(), move |app| {
            app.insert_resource(out);
        });
        tx.send(write_level(tmp.path(), &lights())).unwrap();
        until(&mut app, "the level", |a| level_state(a) == "ready");
        assert_eq!(hero.get(), None, "no level yet");
        place(&mut app, 0, ERUPTOR, "Eruptor");
        until(&mut app, "the level", |a| mode(a) != Mode::Portal);
        assert_eq!(hero.get(), Some(slot(0)), "the level starts with him");
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        assert_eq!(hero.get(), None, "held with no hero in it");
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "the replacement in", |a| {
            a.world().resource::<Play>().hero().is_some()
        });
        assert_eq!(hero.get(), Some(slot(2)), "the replacement's");
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.update();
        app.update();
        assert_eq!(mode(&app), Mode::Portal);
        assert_eq!(hero.get(), None, "the level ended");
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
            ..Lineup::default()
        }
    }

    /// The roster's categories as the flow reads them: figures and Giants
    /// are characters, items and sidekicks magic items, an adventure piece
    /// a pack, and a figure the roster doesn't hold one Giants can't use.
    #[test]
    fn the_roster_says_what_each_figure_is() {
        let tmp = tempfile::tempdir().unwrap();
        let mut shelf = crate::test_support::turntable::extract(tmp.path());
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
        crate::test_support::physics::spawn_body(&mut commands, &body).unwrap();
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

    /// AG.81: a replacement stands where the last one stood from his first
    /// step, put down at that pivot rather than dropped onto it (the
    /// original drops its replacement from 0.41 m up, AG.100's): every one
    /// of his first 30 steps on the ground at that place, his locomotion on
    /// the ground with no weight on his fall.
    #[test]
    fn a_replacement_stands_his_first_steps_where_the_last_stood() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = walking(tmp.path());
        test_pad::stick(&mut app, pad, 0.0, 0.0);
        for _ in 0..30 {
            app.update();
        }
        clear(&mut app, 0);
        until(&mut app, "the call", |a| {
            alert(a) == Some(Alert::PlaceSkylander)
        });
        let left = app
            .world()
            .resource::<Play>()
            .inside
            .as_ref()
            .unwrap()
            .left
            .unwrap();
        place(&mut app, 2, ERUPTOR, "Eruptor");
        until(&mut app, "the next one in", |a| mode(a) == Mode::Level);
        let next = app.world().resource::<Play>().hero().unwrap();
        for step in 0..30 {
            let h = hero(&app);
            assert!(
                h.footing.supported,
                "step {step}: off the ground at {}",
                h.pivot
            );
            assert!(
                h.pivot.distance(left.0) < 1e-3,
                "step {step}: {} against {}",
                h.pivot,
                left.0
            );
            let gaits = app.world().entity(next).get::<HeroGaits>().unwrap();
            assert_eq!(gaits.locomotion.air().name(), "ground", "step {step}");
            assert_eq!(gaits.locomotion.weights()[3], 0.0, "step {step}: his fall");
            app.update();
        }
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
