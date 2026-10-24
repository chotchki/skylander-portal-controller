//! The dev entry (openspec AH.25, AH design decision 17): the first fight
//! on the TV before the route to it runs (AH.28), in `dev-tools` builds
//! only (`test-hooks` turns it on too; a release build has none,
//! [`plugin`]; stand-in ledger: `dev-entry`).
//!
//! - Asked: `GUAC_DEV_ENTRY` in the process env at start (`b` or `start`),
//!   or `POST /api/_test/tv/entry` (`{"anchor": "b"}`, optionally
//!   `"seed": n`) in a `test-hooks` build, before the hero comes into the
//!   level: once he's in it's refused. Asked, the TV's run lists the first
//!   fight's placements (the four Chompies, Setup_001's controller,
//!   Norticus: [`guacamole::scripts::fight_config`]) and walks its
//!   creatures, installed again at once if the level's part has linked.
//! - Anchor B: once the hero's scripts have joined and his arrival has
//!   played out (120 steps, as the headless runs settle him), he's set down
//!   along AH.8's route ([`guacamole::anchors::ROUTE`]) a step at a time,
//!   each set-down before that step's scripts, so the route's triggers fire
//!   in the original's order and wake what they woke there; then at anchor
//!   B's pose. The creatures are set where anchor B has them there and
//!   again at F0, 120 steps on, with the rest of the state anchor B holds
//!   that the set-downs don't make, off AH.4's reads ([`ANCHOR_FIELDS`]):
//!   the combat manager's next alert in its phase, the level's loot counts,
//!   and the session's generator seeded when asked. The follow camera rests
//!   on camera 0's definition at each set-down, looking along the oracle's
//!   view at anchor B there.
//! - Start: the fight listed and nothing else, the hero where the level
//!   starts him: what the e2e's synthetic creature needs.
//! - Norticus' scene, once the four are beaten, ended 5 s into it as the
//!   headless runs end it (`anchors::SCENE_STEPS`), and the hero freed from
//!   its hold: the cutscene manager ends it in the original, off the
//!   whitelist (AH.56).
//! - At F0 the entry reads decision 17's equivalence list off the run
//!   ([`guacamole::anchors::equivalence`]) and reports it with F0's step in
//!   the scripts' log (`/api/_test/tv`'s `entry`), so a driver starts AH.8's
//!   opening there and holds the list against the oracle's reads.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use guac_motion::camera::{CameraDef, GameCamera, LookFrom};
use guac_script::Generator;
use guacamole::anchors::{
    self, ANCHOR_B, ANCHOR_B_LOOT, ANCHOR_B_POSE, ANCHOR_B_VIEW, Equivalence, FIRST_ALERT, OPENING,
    ROUTE, STRIP_AT, TO_STRIP,
};
use guacamole::frame::Frame;
use guacamole::scripts::{self, Rarity, ScriptRuntime, ScriptWorld};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::hero::{FollowCamera, HeroBody, viewed};
use crate::hero_scripts::{HeroRun, Join, ScriptLog, ScriptSource};
use crate::physics::AvianSeam;
use crate::play::Play;
use crate::progress::Earning;

/// Where the entry takes the hero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Anchor {
    /// The level's start, as AG.21 enters it: the fight listed, nothing set.
    Start,
    /// Anchor B (AH.8): the pit's floor after the Chompies' scene.
    B,
}

impl Anchor {
    /// `GUAC_DEV_ENTRY`'s value.
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "b" | "B" | "anchor_b" => Some(Anchor::B),
            "start" => Some(Anchor::Start),
            _ => None,
        }
    }
}

/// The anchor's state that the set-downs don't make, set from AH.4's reads
/// at anchor B (stand-in ledger: `dev-entry`, which names these): the
/// four Chompies' and Norticus' places and facings (`actors-anchor.json`),
/// the combat manager's next alert in its 0.25 s check's phase (F0 + 24),
/// the destructibles' three loot counts (`fight-anchor.json`), and the
/// generator's seed when asked.
pub(crate) const ANCHOR_FIELDS: [&str; 4] = [
    "creatures' places and facings",
    "the combat manager's next alert",
    "the destructibles' loot counts",
    "the generator's seed (asked)",
];

/// Steps after his scripts join before the route starts: his arrival
/// played out, as the headless runs settle him.
pub(crate) const SETTLE: u32 = 120;

/// A route to an anchor as the entry runs it: steps from its start.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub(crate) anchor: Anchor,
    /// Steps after his join before the route's step 0.
    pub(crate) settle: u32,
    /// Each set-down: its step, his pivot (metres) and facing (degrees).
    pub(crate) set_downs: Vec<(u32, Vec3, f32)>,
    /// The step the creatures are set at (and again at F0), and where.
    pub(crate) creatures_at: u32,
    pub(crate) creatures: Vec<(String, Vec3, f32)>,
    /// F0's step: the one the anchor's state is in place for.
    pub(crate) f0: u32,
    /// The combat manager's next alert, steps from F0.
    pub(crate) first_alert: Option<u32>,
    /// The loot counts set at F0: common, uncommon, rare.
    pub(crate) loot: Option<[f32; 3]>,
    /// The way the follow camera looks once set down there (a heading).
    pub(crate) view: Option<f32>,
}

impl Plan {
    pub(crate) fn of(anchor: Anchor) -> Self {
        match anchor {
            Anchor::Start => Plan {
                anchor,
                settle: 0,
                set_downs: Vec::new(),
                creatures_at: 0,
                creatures: Vec::new(),
                f0: 0,
                first_alert: None,
                loot: None,
                view: None,
            },
            Anchor::B => Plan {
                anchor,
                settle: SETTLE,
                set_downs: TO_STRIP
                    .iter()
                    .copied()
                    .chain(ROUTE.iter().map(|(k, at, f)| (k + STRIP_AT, *at, *f)))
                    .map(
                        |(k, at, f)| match k == STRIP_AT + ROUTE[ROUTE.len() - 1].0 {
                            // Anchor B itself at its read's own pose.
                            true => (k, Vec3::from_array(ANCHOR_B_POSE.0), ANCHOR_B_POSE.1),
                            false => (k, Vec3::from_array(at), f),
                        },
                    )
                    .collect(),
                creatures_at: STRIP_AT + ROUTE[ROUTE.len() - 1].0,
                creatures: ANCHOR_B
                    .iter()
                    .map(|(n, at, f)| ((*n).to_owned(), Vec3::from_array(*at), *f))
                    .collect(),
                f0: STRIP_AT + OPENING,
                first_alert: Some(FIRST_ALERT),
                loot: Some(ANCHOR_B_LOOT),
                view: Some(ANCHOR_B_VIEW),
            },
        }
    }
}

/// An entry asked for: its route, and the seed the session's generator
/// takes at F0, if one was asked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Asked {
    pub(crate) plan: Plan,
    pub(crate) seed: Option<u32>,
}

/// Entries asked over `POST /api/_test/tv/entry`, waiting for a frame.
#[derive(Resource, Clone, Default)]
pub(crate) struct EntryAsks(pub(crate) Arc<Mutex<Option<Asked>>>);

/// How far the entry has got: asked, on its way, or at its anchor, where
/// it does nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// Nothing asked.
    #[default]
    None,
    /// Asked: waiting for the hero's scripts to join.
    Waiting,
    /// His arrival playing out, then the set-downs.
    Route,
    /// At the anchor from F0 on.
    Anchored,
}

/// The entry asked, how far it has got and what it read at F0; a new ask
/// starts it over.
#[derive(Resource, Debug, Default)]
pub(crate) struct DevEntry {
    pub(crate) asked: Option<Asked>,
    phase: Phase,
    /// The hero the route runs on, and the steps run since his join.
    hero: Option<Entity>,
    ran: u32,
    /// F0's step in the scripts' log, and the list read there.
    f0: Option<u64>,
    equivalence: Option<Equivalence>,
    /// Steps Norticus has been in his scene, and the log's step the entry
    /// ended it at ([`anchors::SCENE_STEPS`]; AH.56 runs what ends it).
    scene: u32,
    scene_ended: Option<u64>,
    /// Why the last ask was refused.
    refused: Option<String>,
}

/// The entry's part of `/api/_test/tv`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct EntryShot {
    pub anchor: Option<Anchor>,
    pub phase: Phase,
    /// The route's last step the entry acted on (from its step 0: F0's once
    /// at the anchor), once it's under way.
    pub route_step: Option<u32>,
    /// F0's step in `/api/_test/tv/scripts`' log: the step the anchor's
    /// state is in place for.
    pub f0_step: Option<u64>,
    pub equivalence: Option<Equivalence>,
    /// What the entry set off AH.4's reads, past the set-downs.
    pub set: Vec<&'static str>,
    /// The log's step it ended Norticus' scene at, 5 s into it.
    pub scene_ended_step: Option<u64>,
    pub refused: Option<String>,
}

impl DevEntry {
    /// Whether `/api/_test/tv` reports it: once asked, or refused.
    pub(crate) fn shown(&self) -> bool {
        self.asked.is_some() || self.refused.is_some()
    }

    pub(crate) fn shot(&self) -> EntryShot {
        let plan = self.asked.as_ref().map(|a| &a.plan);
        EntryShot {
            anchor: plan.map(|p| p.anchor),
            phase: self.phase,
            route_step: plan
                .filter(|_| self.phase != Phase::Waiting)
                .and_then(|p| self.ran.checked_sub(p.settle + 1)),
            f0_step: self.f0,
            equivalence: self.equivalence.clone(),
            set: match plan.map(|p| p.anchor) {
                Some(Anchor::B) => ANCHOR_FIELDS.to_vec(),
                _ => Vec::new(),
            },
            scene_ended_step: self.scene_ended,
            refused: self.refused.clone(),
        }
    }
}

/// Whether the TV's run lists the first fight: an entry was asked. Read
/// when the run is installed.
pub(crate) fn fight_listed(world: &World) -> bool {
    world
        .get_resource::<DevEntry>()
        .is_some_and(|e| e.asked.is_some())
}

/// `GUAC_DEV_ENTRY`'s ask, in a dev build only.
pub(crate) fn asked_at_start(dev_build: bool, value: Option<&str>) -> Option<Asked> {
    if !dev_build {
        return None;
    }
    let anchor = Anchor::parse(value?)?;
    Some(Asked {
        plan: Plan::of(anchor),
        seed: None,
    })
}

/// The entry's plugin in a dev build (`cfg!(feature = "dev-tools")`, which
/// `test-hooks` turns on), none otherwise: a release build has no entry.
pub(crate) fn plugin(dev_build: bool, asks: EntryAsks) -> Option<DevEntryPlugin> {
    dev_build.then_some(DevEntryPlugin { asks })
}

pub(crate) struct DevEntryPlugin {
    asks: EntryAsks,
}

impl Plugin for DevEntryPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        let env = std::env::var("GUAC_DEV_ENTRY").ok();
        let at_start = asked_at_start(true, env.as_deref());
        if env.is_some() && at_start.is_none() {
            warn!(value = ?env, "GUAC_DEV_ENTRY isn't `b` or `start`: no entry");
        }
        if let Some(a) = &at_start {
            info!(anchor = ?a.plan.anchor, "the dev entry asked at start");
        }
        app.insert_resource(self.asks.clone())
            .insert_resource(DevEntry {
                phase: if at_start.is_some() {
                    Phase::Waiting
                } else {
                    Phase::None
                },
                asked: at_start,
                ..default()
            })
            .add_systems(Update, take_asks)
            .add_systems(FixedUpdate, drive.in_set(Frame::Pads));
    }
}

/// An ask taken: refused once the hero's in the level, else the fight
/// listed and the run installed again on it if it's made.
fn take_asks(
    mut commands: Commands,
    asks: Res<EntryAsks>,
    mut entry: ResMut<DevEntry>,
    play: Option<Res<Play>>,
    source: Option<Res<ScriptSource>>,
) {
    let Some(asked) = asks.0.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    if play.is_some_and(|p| p.started()) {
        let why = "the hero's already in the level: ask before he comes in".to_owned();
        warn!(reason = %why, "the dev entry refused");
        entry.refused = Some(why);
        return;
    }
    info!(anchor = ?asked.plan.anchor, seed = ?asked.seed, "the dev entry asked");
    let listed = entry.asked.is_some();
    *entry = DevEntry {
        asked: Some(asked),
        phase: Phase::Waiting,
        ..default()
    };
    if source.is_some() && !listed {
        commands.queue(crate::hero_scripts::reinstall);
    }
}

/// One step of the entry, before the step's scripts (the module docs).
fn drive(
    mut entry: ResMut<DevEntry>,
    runtime: Option<ResMut<ScriptRuntime>>,
    world: Option<ResMut<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    log: Res<ScriptLog>,
    earning: Option<Res<Earning>>,
    seam: AvianSeam,
    mut heroes: Query<(Entity, &HeroRun, &mut HeroBody)>,
    mut cameras: Query<(&mut FollowCamera, &mut Transform)>,
) {
    let DevEntry {
        asked: Some(asked), ..
    } = &*entry
    else {
        return;
    };
    let plan = asked.plan.clone();
    let seed = asked.seed;
    let (Some(mut runtime), Some(mut world), Some(level)) = (
        runtime,
        world,
        source.as_deref().and_then(ScriptSource::level_unit),
    ) else {
        return;
    };
    if entry.phase == Phase::Anchored {
        let minion = heroes.iter().find_map(|(_, r, _)| match r.join {
            Join::Joined(m) => Some(m),
            _ => None,
        });
        end_the_scene(&mut entry, &mut runtime, &world, level, minion, &log);
        return;
    }
    let joined = heroes
        .iter()
        .find(|(_, r, _)| matches!(r.join, Join::Joined(_)))
        .map(|(e, r, _)| (e, r.slot));
    let Some((hero, slot)) = joined else {
        return;
    };
    if entry.hero != Some(hero) {
        // His activation ran in the step before this one.
        entry.hero = Some(hero);
        entry.ran = 1;
        entry.phase = Phase::Route;
        info!(anchor = ?plan.anchor, "the dev entry's route starts once he's settled");
    }
    let ran = entry.ran;
    entry.ran += 1;
    let Some(k) = ran.checked_sub(plan.settle) else {
        return;
    };
    let Ok((_, _, mut body)) = heroes.get_mut(hero) else {
        return;
    };
    if let Some((_, at, facing)) = plan.set_downs.iter().find(|s| s.0 == k) {
        set_down(&mut body, &seam, *at, *facing);
        let view = plan
            .view
            .filter(|_| k == plan.set_downs.last().map_or(0, |s| s.0));
        rest_camera(&mut cameras, body.hero.pivot, view);
        info!(step = k, at = ?at, facing, "the dev entry sets him down");
    }
    if k == plan.creatures_at || k == plan.f0 {
        for (name, at, facing) in &plan.creatures {
            match world.placed_in(level, name) {
                Some(h) if world.place(h, *at, *facing) => {}
                _ => warn!(%name, "the dev entry found no such placement to set"),
            }
        }
    }
    // F0 (the anchor's state, then the list read): at its step, or at the
    // first the route acts on (the level's start's F0 is his join).
    if k < plan.f0 {
        return;
    }
    if let Some(steps) = plan.first_alert
        && !scripts::alert_in(&mut runtime.vm, steps)
    {
        warn!("no combat data to set the next alert in");
    }
    if let Some(loot) = plan.loot {
        for (rarity, index) in [Rarity::Common, Rarity::Uncommon, Rarity::Rare]
            .into_iter()
            .zip(loot)
        {
            if !scripts::set_loot_index(&mut runtime.vm, &world, rarity, index) {
                warn!(?rarity, "no loot data to set the count in");
            }
        }
    }
    if let Some(seed) = seed {
        runtime.generator = Generator::seeded(seed);
    }
    let figure = earning
        .as_deref()
        .and_then(|e| e.slots.get(slot.as_usize()))
        .and_then(Option::as_ref)
        .map(|e| (e.progress.xp, e.progress.gold));
    let pose = Some((body.hero.pivot, body.hero.facing));
    let list = anchors::equivalence(&runtime.vm, &world, level, pose, figure);
    entry.f0 = Some(log.last() + 1);
    entry.equivalence = Some(list);
    entry.phase = Phase::Anchored;
    info!(f0 = ?entry.f0, "the dev entry is at its anchor");
}

/// Norticus' scene ended 5 s into it ([`anchors::SCENE_STEPS`]), as the
/// headless runs end it until AH.56 runs what does in the original (the
/// cutscene manager, off the whitelist): his `Cutscene State` back to 0,
/// which sends Norticus on into the cave, and the hero's minion freed from
/// the scene's hold (his `description.state` back to 0, which his
/// `disabled` behaviour waits on to return him to play).
fn end_the_scene(
    entry: &mut DevEntry,
    runtime: &mut ScriptRuntime,
    world: &ScriptWorld,
    level: u16,
    minion: Option<guac_script::PlacementId>,
    log: &ScriptLog,
) {
    if entry.scene_ended.is_some() || entry.asked.as_ref().map(|a| a.plan.anchor) != Some(Anchor::B)
    {
        return;
    }
    let vm = &mut runtime.vm;
    let in_scene = world
        .placed_in(level, anchors::NORTICUS)
        .and_then(|h| vm.placement_of(h))
        .and_then(|id| vm.behaviour(id))
        == Some("cutscene go");
    if !in_scene {
        return;
    }
    entry.scene += 1;
    if entry.scene < anchors::SCENE_STEPS {
        return;
    }
    match vm.program().global_named(anchors::CUTSCENE_STATE) {
        Some(g) if vm.set_global(g, guac_script::Value::Int(0)) => {
            entry.scene_ended = Some(log.last() + 1);
            info!("the dev entry ends Norticus' scene, 5 s into it");
        }
        _ => warn!("no cutscene state to end Norticus' scene with"),
    }
    let description = minion
        .and_then(|m| vm.local(m, "description"))
        .map(|d| vm.value(d));
    match description {
        Some(d) if vm.set_field(d, "state", guac_script::Value::Int(0)) => {
            info!("the dev entry frees him from the scene");
        }
        _ => warn!("no hero's description to free from the scene"),
    }
}

/// The hero set down at `at` (his pivot, dropped onto what's under it)
/// facing `facing`, at rest, as a scripted hero's teleport.
pub(crate) fn set_down(body: &mut HeroBody, seam: &AvianSeam, at: Vec3, facing: f32) {
    let HeroBody {
        hero,
        exclude,
        before,
    } = body;
    hero.pivot = at + Vec3::Y;
    hero.facing = facing;
    hero.walk.velocity = Vec3::ZERO;
    hero.settle(seam, exclude);
    *before = hero.pivot;
}

/// The follow camera at rest on camera 0's definition over his feet,
/// looking along `view` (a heading) when given, its drawn place there.
fn rest_camera(
    cameras: &mut Query<(&mut FollowCamera, &mut Transform)>,
    feet: Vec3,
    view: Option<f32>,
) {
    let def = match view {
        Some(h) => CameraDef {
            look_from: LookFrom::Heading((h + 180.0).rem_euclid(360.0)),
            ..CameraDef::LEVEL_213_START
        },
        None => CameraDef::LEVEL_213_START,
    };
    for (mut follow, mut transform) in cameras.iter_mut() {
        follow.camera = GameCamera::new(def, feet);
        *transform = viewed(&follow.camera.view());
    }
}

/// `POST /api/_test/tv/entry`'s body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryBody {
    anchor: Anchor,
    seed: Option<u32>,
}

/// `POST /api/_test/tv/entry` (`{"anchor": "b"}` or `"start"`, optionally
/// `"seed": n`): 202 with the ask kept for the next frame, 400 for any other
/// body. A refusal (the hero already in) shows in `/api/_test/tv`'s
/// `entry`. Generic so a test needs no `AppState`.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(asks: &EntryAsks) -> axum::Router<S> {
    use axum::extract::rejection::JsonRejection;
    let asks = asks.0.clone();
    axum::Router::new().route(
        "/api/_test/tv/entry",
        axum::routing::post(move |body: Result<axum::Json<EntryBody>, JsonRejection>| {
            let kept = match body {
                Ok(axum::Json(b)) => {
                    *asks.lock().unwrap_or_else(|e| e.into_inner()) = Some(Asked {
                        plan: Plan::of(b.anchor),
                        seed: b.seed,
                    });
                    true
                }
                Err(_) => false,
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

#[cfg(test)]
mod tests {
    use guac_formats::script::builder::{
        FIGHT_CONTROLLER, FIGHT_CREATURE, FIGHT_DONE, FIGHT_TRIGGER_A, FIGHT_TRIGGER_B,
        FIGHT_WAIT_B,
    };
    use guacamole::scripts::FIRST_FIGHT;

    use super::*;
    use crate::test_support::play::{TRIGGERS, fight_app, until};

    /// A build without `dev-tools` (which `test-hooks` turns on) passes
    /// `false`: no plugin, so no [`DevEntry`] and nothing lists the fight,
    /// and `GUAC_DEV_ENTRY` asks for nothing; a dev build's takes `b` and
    /// `start` and nothing else.
    #[test]
    fn the_entry_is_absent_from_a_build_without_either_feature() {
        assert!(plugin(false, EntryAsks::default()).is_none());
        assert_eq!(asked_at_start(false, Some("b")), None);
        assert_eq!(asked_at_start(false, Some("start")), None);
        let anchor = |v| asked_at_start(true, Some(v)).map(|a| a.plan.anchor);
        assert_eq!(anchor("b"), Some(Anchor::B));
        assert_eq!(anchor("anchor_b"), Some(Anchor::B));
        assert_eq!(anchor("start"), Some(Anchor::Start));
        assert_eq!(anchor("pit"), None);
        assert_eq!(asked_at_start(true, None), None);
        let mut world = World::new();
        assert!(!fight_listed(&world), "no entry resource");
        world.insert_resource(DevEntry::default());
        assert!(!fight_listed(&world), "nothing asked");
        world.resource_mut::<DevEntry>().asked = asked_at_start(true, Some("b"));
        assert!(fight_listed(&world));
        // Anchor B's plan is AH.8's route and AH.4's reads: the camera
        // volumes on the way to the strip, then the route from the strip,
        // each set-down after the last.
        let b = Plan::of(Anchor::B);
        assert_eq!(b.set_downs.len(), TO_STRIP.len() + ROUTE.len());
        assert!(b.set_downs.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(b.set_downs[TO_STRIP.len()].0, STRIP_AT);
        assert_eq!(b.creatures_at, STRIP_AT + 360);
        let last = b.set_downs.last().unwrap();
        assert_eq!(
            (last.1.to_array(), last.2),
            ANCHOR_B_POSE,
            "anchor B's own pose"
        );
        assert_eq!(
            (b.settle, b.f0, b.first_alert),
            (SETTLE, STRIP_AT + OPENING, Some(24))
        );
        assert_eq!(b.loot, Some(ANCHOR_B_LOOT));
        assert_eq!(b.creatures.len(), 5);
    }

    /// A route through the synthetic level's triggers: set down at `first`
    /// at its step 0 and at `second` at its step 20, F0 at 40, his arrival
    /// 5 steps long.
    fn route(first: [f32; 3], second: [f32; 3]) -> Asked {
        Asked {
            plan: Plan {
                anchor: Anchor::B,
                settle: 5,
                set_downs: vec![
                    (0, Vec3::from_array(first), 90.0),
                    (20, Vec3::from_array(second), 270.0),
                ],
                creatures_at: 20,
                creatures: Vec::new(),
                f0: 40,
                first_alert: None,
                loot: None,
                view: None,
            },
            seed: Some(77),
        }
    }

    fn entry(app: &App) -> EntryShot {
        app.world().resource::<DevEntry>().shot()
    }

    /// The controller's behaviour and the triggers fired, in the order they
    /// woke, once the entry is at its anchor.
    fn run(asked: Asked) -> (App, Option<String>, Vec<String>) {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), Some(asked));
        until(&mut app, "the entry at its anchor", |a| {
            entry(a).phase == Phase::Anchored
        });
        let w = app.world();
        let level = w.resource::<ScriptSource>().level_unit().unwrap();
        let world = w.resource::<ScriptWorld>();
        let vm = &w.resource::<ScriptRuntime>().vm;
        let controller = world
            .placed_in(level, FIGHT_CONTROLLER)
            .and_then(|h| vm.placement_of(h))
            .and_then(|id| vm.behaviour(id))
            .map(str::to_owned);
        let fired = world.fired_triggers(level);
        (app, controller, fired)
    }

    /// AH.25: the entry sets the hero down a step at a time, each set-down
    /// before its step's scripts, so the triggers fire in the recorded
    /// order and wake what waits on them in turn: through A then B the
    /// synthetic Setup_001's controller reaches its end, the triggers firing
    /// A then B; the other way B fires first unseen, then A, and it waits on
    /// B for good.
    #[test]
    fn the_triggers_fire_in_the_recorded_order_on_a_synthetic_level() {
        let (_, controller, fired) = run(route(TRIGGERS[0], TRIGGERS[1]));
        assert_eq!(controller.as_deref(), Some(FIGHT_DONE));
        assert_eq!(fired, [FIGHT_TRIGGER_A, FIGHT_TRIGGER_B]);
        let (_, controller, fired) = run(route(TRIGGERS[1], TRIGGERS[0]));
        assert_eq!(controller.as_deref(), Some(FIGHT_WAIT_B));
        assert_eq!(fired, [FIGHT_TRIGGER_B, FIGHT_TRIGGER_A]);
    }

    /// Decision 17's list read off the entry's state at F0 and reported:
    /// his pose where the last set-down put him (dropped onto the floor,
    /// facing its way), his figure's experience and gold (a placement no
    /// profile owns: fresh), each of the first fight's placements' behaviour
    /// (the synthetic level holds the creature and the controller of them,
    /// the rest none) and the alerted flag where one has a description
    /// (none here), the triggers fired by name, and the loot counts (none:
    /// the synthetic part makes no loot data); F0's step the log's next
    /// when it was read.
    #[test]
    fn decision_17s_list_reads_off_the_entrys_state() {
        let (app, _, _) = run(route(TRIGGERS[0], TRIGGERS[1]));
        let shot = entry(&app);
        assert_eq!(shot.anchor, Some(Anchor::B));
        assert!(shot.f0_step.is_some());
        let list = shot.equivalence.expect("read at F0");
        let (at, facing) = list.hero.expect("his pose");
        let b = Vec3::from_array(TRIGGERS[1]);
        assert!(
            Vec3::from_array(at).with_y(0.0).distance(b.with_y(0.0)) < 1e-3,
            "{at:?}"
        );
        assert!((at[1] - b[1]).abs() < 0.1, "on the floor: {at:?}");
        assert!((facing - 270.0).abs() < 1e-3, "{facing}");
        assert_eq!((list.experience, list.gold), (Some(0), Some(0)));
        assert_eq!(
            list.placements
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            FIRST_FIGHT
        );
        let behaviour = |name: &str| {
            list.placements
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.behaviour.clone())
        };
        assert_eq!(behaviour(FIGHT_CONTROLLER).as_deref(), Some(FIGHT_DONE));
        assert!(behaviour(FIGHT_CREATURE).is_some(), "it runs");
        assert!(
            list.placements
                .iter()
                .filter(|p| p.name != FIGHT_CONTROLLER && p.name != FIGHT_CREATURE)
                .all(|p| p.behaviour.is_none() && p.alerted.is_none())
        );
        assert!(list.placements.iter().all(|p| p.alerted.is_none()));
        assert_eq!(list.triggers_fired, [FIGHT_TRIGGER_A, FIGHT_TRIGGER_B]);
        assert_eq!(list.loot, [None, None, None]);
        // The mirror's part reads the same.
        assert_eq!(shot.phase, Phase::Anchored);
        assert_eq!(shot.route_step, Some(40));
    }

    /// The level's start: the fight listed and nothing set, at its anchor
    /// the step after his join, where the level starts him.
    #[test]
    fn the_levels_start_is_at_its_anchor_once_he_joins() {
        let tmp = tempfile::tempdir().unwrap();
        let asked = Asked {
            plan: Plan::of(Anchor::Start),
            seed: None,
        };
        let (mut app, _) = fight_app(tmp.path(), Some(asked));
        until(&mut app, "the entry at its anchor", |a| {
            entry(a).phase == Phase::Anchored
        });
        let shot = entry(&app);
        assert_eq!(shot.anchor, Some(Anchor::Start));
        assert_eq!(shot.route_step, Some(1));
        assert!(shot.set.is_empty());
        let (at, _) = shot.equivalence.unwrap().hero.unwrap();
        let start = Vec3::from_array(crate::test_support::level::START);
        assert!(Vec3::from_array(at).with_y(0.0).distance(start.with_y(0.0)) < 1e-3);
        assert!(fight_listed(app.world()));
    }

    /// Each set-down lands at its own step, before that step's scripts: a
    /// route whose F0 is its last set-down's step reads him there at F0,
    /// set down that step.
    #[test]
    fn a_set_down_lands_at_its_step_before_its_scripts() {
        let mut asked = route(TRIGGERS[0], TRIGGERS[1]);
        asked.plan.f0 = 20;
        let (app, _, _) = run(asked);
        let (at, facing) = entry(&app).equivalence.unwrap().hero.unwrap();
        let b = Vec3::from_array(TRIGGERS[1]);
        assert!(
            Vec3::from_array(at).with_y(0.0).distance(b.with_y(0.0)) < 1e-3,
            "{at:?}"
        );
        assert!((facing - 270.0).abs() < 1e-3, "{facing}");
    }

    /// Once he's in the level an ask is refused, and the run keeps its
    /// listing: nothing of the fight runs.
    #[test]
    fn an_ask_with_the_hero_in_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        let asks = app.world().resource::<EntryAsks>().clone();
        *asks.0.lock().unwrap() = Some(route(TRIGGERS[0], TRIGGERS[1]));
        app.update();
        let entry = app.world().resource::<DevEntry>();
        assert!(entry.asked.is_none());
        assert!(
            entry
                .shot()
                .refused
                .is_some_and(|r| r.contains("in the level"))
        );
        assert!(!fight_listed(app.world()));
    }
}
