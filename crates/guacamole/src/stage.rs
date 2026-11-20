//! A creature staged on the TV (openspec AS.680), in `test-hooks` builds
//! only: `POST /api/_test/tv/stage` (`{"level": "<archive>", "creature":
//! "<placement>"}`, optionally `"trigger": "<placement>"`, `"distance":
//! <metres>`, `"heading": <degrees>`, `"round": <degrees>`), asked before the
//! hero comes in. `GET` on the same path reads it back.
//!
//! What it sets, and nothing else:
//! - the level asked by name, as `POST /api/_test/tv/level` asks it;
//! - the creature (and the trigger, when one's named) on the run's whitelist
//!   and walked, at the run's install ([`guacamole::staging`]);
//! - the dev entry's `by` anchor on the creature, handed over once the run
//!   is installed: the hero set down [`crate::dev_entry::BY_DISTANCE`] off
//!   it (or `distance` along `heading`) once his arrival has played out;
//! - the story save's session begun with no host, so nothing the run does is
//!   written ([`withholds_saves`]);
//! - the level's camera pinned on the creature each frame it's drawn, off
//!   its posed joints' sphere ([`guacamole::staging::frame`]), through the
//!   same pin `/api/_test/tv/camera` sets, and let go once it isn't.
//!
//! It ends ([`over`]) once its level is refused, another level is asked
//! after it, or its run is left: its pin let go, the stage cleared, so the
//! next pick in the same process lists nothing of it, saves for its host and
//! follows him. A stage for the level the TV already holds is a 409: its
//! creature is listed at the level's install, which has been.
//!
//! Its wake is the activity rule's: the creature wakes when the hero comes
//! into its range, as in play. A creature its file leaves at range 0 waits
//! for its controller, which the stage lists and walks only when it's named
//! as the trigger; the controller's own triggers then wake it.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use guacamole::scripts::ScriptWorld;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::creatures::DrawnCreatures;
use crate::dev_entry::{Anchor, Asked, EntryAsks, Plan};
use crate::hero::FollowCamera;
use crate::hero_scripts::ScriptSource;
use crate::level_choice::LevelAsks;
use crate::mirror::{CameraPin, TvMirror};

/// The level and the creature to stage, the controller that wakes it where
/// its file leaves it at range 0, and where the hero and the camera stand.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageAsk {
    pub(crate) level: String,
    pub(crate) creature: String,
    #[serde(default)]
    pub(crate) trigger: Option<String>,
    #[serde(default)]
    pub(crate) distance: Option<f32>,
    #[serde(default)]
    pub(crate) heading: Option<f32>,
    #[serde(default)]
    pub(crate) round: Option<f32>,
}

impl StageAsk {
    /// A stage the TV can act on: a level's plain name, a placement's name
    /// (and the trigger's) of 1 to 64 characters with no control character,
    /// and finite numbers.
    pub(crate) fn valid(&self) -> bool {
        let name = |n: &str| (1..=64).contains(&n.len()) && !n.chars().any(char::is_control);
        crate::level_choice::plain_name(&self.level)
            && name(&self.creature)
            && self.trigger.as_deref().is_none_or(name)
            && [self.distance, self.heading, self.round]
                .iter()
                .flatten()
                .all(|v| v.is_finite())
            && self.distance.is_none_or(|d| d > 0.0)
    }

    /// The placements the run lists for it: the creature, then its trigger.
    pub(crate) fn listed(&self) -> Vec<&str> {
        std::iter::once(self.creature.as_str())
            .chain(self.trigger.as_deref())
            .collect()
    }
}

/// The stage as `GET /api/_test/tv/stage` reads it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct StageShot {
    pub(crate) ask: Option<StageAsk>,
    /// Put on the run's list and walked at its install, by name.
    pub(crate) listed: Vec<String>,
    pub(crate) walked: Vec<String>,
    /// The staged level's run installed with it listed, and the entry
    /// handed over (each once a stage).
    pub(crate) installed: bool,
    pub(crate) entry: bool,
    /// The TV's level ask has been the staged level's.
    pub(crate) seen: bool,
    /// The level the TV holds, for the route's check.
    #[serde(skip)]
    pub(crate) holding: Option<String>,
    /// The creature awake by the activity rule, drawn, and the camera on it.
    pub(crate) awake: bool,
    pub(crate) drawn: bool,
    pub(crate) pin: Option<[f32; 5]>,
    /// The sphere framed: its centre and radius, metres.
    pub(crate) centre: Option<[f32; 3]>,
    pub(crate) radius: Option<f32>,
}

/// The stage asked, and what it's done so far, shared with its route.
#[derive(Resource, Clone, Default)]
pub(crate) struct Stage(pub(crate) Arc<Mutex<StageShot>>);

/// The process's one stage, shared by the route and the TV's app (a test
/// hook asked once a run, like the level and the entry it hands on).
pub(crate) fn shared() -> &'static Stage {
    static STAGE: std::sync::LazyLock<Stage> = std::sync::LazyLock::new(Stage::default);
    &STAGE
}

impl Stage {
    pub(crate) fn ask(&self) -> Option<StageAsk> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).ask.clone()
    }

    fn with<T>(&self, f: impl FnOnce(&mut StageShot) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Whether a story save session begun now keeps its saves to itself: while
/// a stage is asked, the staged run writes nothing a family's save holds.
pub(crate) fn withholds_saves(stage: Option<&Stage>) -> bool {
    stage.is_some_and(|s| s.ask().is_some())
}

/// `config` with the stage's placements listed, when it's for `level`; the
/// names listed.
pub(crate) fn listed(
    stage: Option<&Stage>,
    level: &str,
    config: &mut guac_script::Config,
) -> Vec<String> {
    let Some(ask) = stage.and_then(Stage::ask).filter(|a| a.level == level) else {
        return Vec::new();
    };
    guacamole::staging::list(config, &ask.listed());
    let names: Vec<String> = ask.listed().into_iter().map(str::to_owned).collect();
    if let Some(s) = stage {
        s.with(|s| s.listed = names.clone());
    }
    names
}

/// The stage's placements walked in the run just installed for `level`,
/// its level's file linked as `unit`.
pub(crate) fn walked(stage: Option<&Stage>, level: &str, unit: u16, world: &mut ScriptWorld) {
    let Some(ask) = stage.and_then(Stage::ask).filter(|a| a.level == level) else {
        return;
    };
    let walked: Vec<String> = ask
        .listed()
        .into_iter()
        .filter(|n| guacamole::staging::walk(world, unit, n).is_some())
        .map(str::to_owned)
        .collect();
    info!(level, ?walked, "the stage's placements listed and walked");
    if let Some(s) = stage {
        s.with(|s| (s.walked, s.installed) = (walked, true));
    }
}

pub(crate) struct StagePlugin;

impl Plugin for StagePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (end_stage, hand_over_entry).chain())
            .add_systems(
                PostUpdate,
                pin_on_creature
                    .before(crate::mirror::pin_camera)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// The pin the stage last set, let go if the camera still holds it (a pin
/// `/api/_test/tv/camera` set since is the route's).
fn let_go(shot: &mut StageShot, pin: &Mutex<Option<CameraPin>>) {
    if let Some([x, y, z, tilt, heading]) = shot.pin.take() {
        let ours = CameraPin {
            eye: [x, y, z],
            tilt,
            heading,
            fov_y: None,
        };
        let mut held = pin.lock().unwrap_or_else(|e| e.into_inner());
        if *held == Some(ours) {
            *held = None;
        }
    }
    (shot.centre, shot.radius) = (None, None);
}

/// Whether the stage for `staged` is over: the TV's level ask (`asked`,
/// its name and state) for it refused or failed, another level asked once
/// its own was seen, or its run installed (`installed`) and the TV's run
/// (`run`) another level's or none.
pub(crate) fn over(
    staged: &str,
    seen: bool,
    installed: bool,
    asked: Option<(&str, &str)>,
    run: Option<&str>,
) -> bool {
    let refused =
        asked.is_some_and(|(name, state)| name == staged && matches!(state, "refused" | "failed"));
    let superseded = seen && asked.is_some_and(|(name, _)| name != staged);
    refused || superseded || (installed && run != Some(staged))
}

/// The stage ended once it's [`over`]: its pin let go and cleared, so
/// nothing of it reaches the next pick (its list, its host-less saves, its
/// camera). Keeps the level the TV holds for the route.
fn end_stage(
    stage: Res<Stage>,
    mirror: Option<Res<TvMirror>>,
    source: Option<Res<ScriptSource>>,
    level: Option<Res<crate::level::Level>>,
) {
    let holding = level.as_ref().and_then(|l| l.name()).map(str::to_owned);
    stage.with(|s| s.holding = holding);
    let Some(ask) = stage.ask() else {
        return;
    };
    let asked = level
        .as_ref()
        .and_then(|l| l.asked())
        .map(|a| (a.name.as_str(), a.state.as_str()));
    let seen = stage.with(|s| {
        s.seen |= asked.is_some_and(|(name, _)| name == ask.level);
        s.seen
    });
    let installed = stage.with(|s| s.installed);
    if !over(
        &ask.level,
        seen,
        installed,
        asked,
        source.as_ref().map(|s| s.level()),
    ) {
        return;
    }
    stage.with(|s| {
        if let Some(m) = &mirror {
            let_go(s, &m.8);
        }
        *s = StageShot {
            holding: s.holding.take(),
            ..StageShot::default()
        };
    });
    info!(creature = %ask.creature, level = %ask.level, "the stage is over: ended");
}

/// The entry handed to the dev entry once the staged level's run is in.
fn hand_over_entry(
    stage: Res<Stage>,
    source: Option<Res<ScriptSource>>,
    world: Option<Res<ScriptWorld>>,
    entry: Option<Res<EntryAsks>>,
) {
    let (Some(ask), Some(source), Some(_), Some(entry)) = (stage.ask(), source, world, entry)
    else {
        return;
    };
    if source.level() != ask.level || !stage.with(|s| s.installed && !s.entry) {
        return;
    }
    *entry.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Asked {
        plan: Plan::of(Anchor::By),
        seed: None,
        interior: Some(ask.creature.clone()),
        heading: ask.heading,
        distance: ask.distance,
    });
    stage.with(|s| s.entry = true);
    info!(creature = %ask.creature, "the stage's entry handed over");
}

/// The level's camera pinned on the staged creature while it's drawn.
#[allow(clippy::too_many_arguments)]
fn pin_on_creature(
    stage: Res<Stage>,
    mirror: Option<Res<TvMirror>>,
    source: Option<Res<ScriptSource>>,
    world: Option<Res<ScriptWorld>>,
    drawn: Option<Res<DrawnCreatures>>,
    creatures: Query<&crate::creatures::DrawnCreature>,
    joints: Query<&GlobalTransform>,
    cameras: Query<&Projection, With<FollowCamera>>,
) {
    let (Some(ask), Some(mirror), Some(source), Some(world)) = (stage.ask(), mirror, source, world)
    else {
        return;
    };
    if source.level() != ask.level {
        return;
    }
    let Some(h) = source
        .level_unit()
        .and_then(|u| world.placed_in(u, &ask.creature))
    else {
        return;
    };
    let actor = world.actor(h);
    let awake = actor.is_some_and(|a| a.active && !a.removed);
    let root = drawn.as_ref().and_then(|d| d.0.get(&h).copied());
    let body = root.and_then(|r| creatures.get(r).ok());
    stage.with(|s| {
        s.awake = awake;
        s.drawn = body.is_some();
    });
    let Some(body) = body.filter(|_| awake) else {
        // Not awake or not drawn: the camera let go, and no pin read back
        // as if it framed it.
        stage.with(|s| let_go(s, &mirror.8));
        return;
    };
    let points: Vec<Vec3> = body
        .joints()
        .iter()
        .chain(root.as_ref())
        .filter_map(|e| joints.get(*e).ok())
        .map(GlobalTransform::translation)
        .collect();
    let Some((centre, radius)) = sphere(&points) else {
        return;
    };
    let (fov, aspect) = cameras
        .iter()
        .find_map(|p| match p {
            Projection::Perspective(p) => Some((p.fov.to_degrees(), p.aspect_ratio)),
            _ => None,
        })
        .unwrap_or((45.0, 16.0 / 9.0));
    let facing = actor.map_or(0.0, |a| a.angles.facing);
    let f = guacamole::staging::frame(
        centre,
        radius,
        facing,
        ask.round.unwrap_or(0.0),
        fov,
        aspect,
    );
    let pin = CameraPin {
        eye: f.eye.to_array(),
        tilt: f.tilt,
        heading: f.heading,
        fov_y: None,
    };
    *mirror.8.lock().unwrap_or_else(|e| e.into_inner()) = Some(pin);
    stage.with(|s| {
        s.pin = Some([f.eye.x, f.eye.y, f.eye.z, f.tilt, f.heading]);
        s.centre = Some(centre.to_array());
        s.radius = Some(radius);
    });
}

/// How much past its joints a creature's skin reaches, metres.
const SKIN: f32 = 0.4;

/// The sphere round `points`: their box's centre and half its diagonal,
/// plus [`SKIN`]. None for no points.
pub(crate) fn sphere(points: &[Vec3]) -> Option<(Vec3, f32)> {
    let first = *points.first()?;
    let (lo, hi) = points
        .iter()
        .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    Some(((lo + hi) / 2.0, (hi - lo).length() / 2.0 + SKIN))
}

/// `POST /api/_test/tv/stage`: 202 with the stage kept and its level asked,
/// 409 for the level the TV already holds (its install, where the creature
/// is listed, is past), 400 for any other body; `GET` reads it back.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(
    stage: &Stage,
    levels: &LevelAsks,
) -> axum::Router<S> {
    use axum::extract::rejection::JsonRejection;
    let (post_stage, get_stage, levels) = (stage.clone(), stage.clone(), levels.clone());
    axum::Router::new().route(
        "/api/_test/tv/stage",
        axum::routing::post(move |body: Result<axum::Json<StageAsk>, JsonRejection>| {
            use axum::http::StatusCode;
            let status = match body {
                Ok(axum::Json(ask)) if ask.valid() => {
                    let held = post_stage.with(|s| s.holding.as_deref() == Some(&ask.level));
                    if held {
                        StatusCode::CONFLICT
                    } else if levels.ask(&ask.level) {
                        post_stage.with(|s| {
                            *s = StageShot {
                                ask: Some(ask),
                                holding: s.holding.take(),
                                ..StageShot::default()
                            }
                        });
                        StatusCode::ACCEPTED
                    } else {
                        StatusCode::BAD_REQUEST
                    }
                }
                _ => StatusCode::BAD_REQUEST,
            };
            async move { status }
        })
        .get(move || {
            let shot = get_stage.with(|s| s.clone());
            async move { axum::Json(shot) }
        }),
    )
}

#[cfg(test)]
mod tests {
    use tower::ServiceExt;

    use super::*;

    async fn post(routes: &axum::Router<()>, body: &str) -> u16 {
        routes
            .clone()
            .oneshot(
                axum::http::Request::post("/api/_test/tv/stage")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// openspec AS.680: the route takes a level and a creature, with an
    /// optional trigger and numbers, keeps the stage and asks the level;
    /// anything else is a 400 and asks nothing.
    #[tokio::test]
    async fn the_stage_route_takes_a_level_and_a_creature() {
        let (stage, levels) = (Stage::default(), LevelAsks::default());
        let routes = routes::<()>(&stage, &levels);
        for (body, status) in [
            (r#"{"level":"level_208_ice"}"#, 400),
            (r#"{"creature":"Enemy_BombThrower(3)"}"#, 400),
            (
                r#"{"level":"Level 208","creature":"Enemy_BombThrower(3)"}"#,
                400,
            ),
            (r#"{"level":"level_208_ice","creature":""}"#, 400),
            (r#"{"level":"level_208_ice","creature":"a\nb"}"#, 400),
            (
                r#"{"level":"level_208_ice","creature":"x","distance":-1}"#,
                400,
            ),
            (r#"{"level":"level_208_ice","creature":"x","by":"y"}"#, 400),
        ] {
            assert_eq!(post(&routes, body).await, status, "{body}");
        }
        assert_eq!(stage.ask(), None);
        assert_eq!(*levels.0.lock().unwrap(), None);
        let ok = r#"{"level":"level_202_arkeyancopter","creature":"Undead_Heavy_tiny(1)","trigger":"Setup_001_Controller","distance":12.5}"#;
        assert_eq!(post(&routes, ok).await, 202);
        let ask = stage.ask().unwrap();
        assert_eq!(
            ask.listed(),
            ["Undead_Heavy_tiny(1)", "Setup_001_Controller"]
        );
        assert_eq!(ask.distance, Some(12.5));
        assert_eq!(
            levels.0.lock().unwrap().as_deref(),
            Some("level_202_arkeyancopter")
        );
        assert!(withholds_saves(Some(&stage)));
        assert!(!withholds_saves(Some(&Stage::default())));
        assert!(!withholds_saves(None));
        // The level the TV already holds: a 409, the stage kept as it was.
        stage.with(|s| s.holding = Some("level_208_ice".into()));
        let held = r#"{"level":"level_208_ice","creature":"Enemy_BombThrower(3)"}"#;
        assert_eq!(post(&routes, held).await, 409);
        assert_eq!(stage.ask().unwrap().level, "level_202_arkeyancopter");
    }

    /// The stage is over once its level is refused, another level is asked
    /// after its own, or its run is left; not while its own ask waits or
    /// its run plays.
    #[test]
    fn the_stage_is_over_when_its_level_is_refused_superseded_or_left() {
        let s = "level_208_ice";
        for (seen, installed, asked, run, want) in [
            // Posted, the TV's ask still the last level's: not yet.
            (
                false,
                false,
                Some(("level_201", "ready")),
                Some("level_201"),
                false,
            ),
            // Its ask waiting, then loading.
            (true, false, Some((s, "waiting")), Some("level_201"), false),
            // Refused (not on the disc) or failed.
            (true, false, Some((s, "refused")), Some("level_201"), true),
            (true, false, Some((s, "failed")), Some("level_201"), true),
            // Installed and playing.
            (true, true, Some((s, "ready")), Some(s), false),
            // Another level asked after it, installed or not.
            (true, false, Some(("level_201", "waiting")), Some(s), true),
            (true, true, Some(("level_201", "waiting")), Some(s), true),
            // Its run left with no ask (a quit).
            (true, true, Some((s, "ready")), None, true),
        ] {
            assert_eq!(
                over(s, seen, installed, asked, run),
                want,
                "{seen} {installed} {asked:?} {run:?}"
            );
        }
    }

    /// openspec AS.680: the run lists the stage's placements only on its own
    /// level.
    #[test]
    fn the_stage_lists_on_its_level_alone() {
        let stage = Stage::default();
        stage.with(|s| {
            s.ask = Some(StageAsk {
                level: "level_208_ice".into(),
                creature: "Enemy_BombThrower(3)".into(),
                trigger: None,
                distance: None,
                heading: None,
                round: None,
            })
        });
        let mut other = guacamole::scripts::smash_config(guac_script::Mode::Play);
        assert!(listed(Some(&stage), "level_201", &mut other).is_empty());
        assert!(!other.whitelist.lists("Enemy_BombThrower(3)", "C:/x/U.ai"));
        let mut own = guacamole::scripts::smash_config(guac_script::Mode::Play);
        assert_eq!(
            listed(Some(&stage), "level_208_ice", &mut own),
            ["Enemy_BombThrower(3)"]
        );
        assert!(own.whitelist.lists("Enemy_BombThrower(3)", "C:/x/U.ai"));
        assert_eq!(stage.0.lock().unwrap().listed, ["Enemy_BombThrower(3)"]);
    }

    /// The stage ends once its run is left, and not before: installed and
    /// then no run, its pin is let go (a pin the camera route set since is
    /// kept), its ask cleared, so saves have their host again and the next
    /// install lists nothing of it.
    #[test]
    fn the_stage_ends_once_its_run_is_left() {
        let ask = StageAsk {
            level: "level_208_ice".into(),
            creature: "Enemy_BombThrower(3)".into(),
            trigger: None,
            distance: None,
            heading: None,
            round: None,
        };
        let ours = CameraPin {
            eye: [1.0, 2.0, 3.0],
            tilt: -20.0,
            heading: 35.0,
            fov_y: None,
        };
        let route = CameraPin { tilt: -5.0, ..ours };
        for (installed, held, ends, left) in [
            // Posted, its run not in yet (the level still loading): kept.
            (false, Some(ours), false, Some(ours)),
            // Its run in and then left: ended, its pin let go.
            (true, Some(ours), true, None),
            // Ended with the route's own pin on the camera: that one kept.
            (true, Some(route), true, Some(route)),
        ] {
            let stage = Stage::default();
            stage.with(|s| {
                s.ask = Some(ask.clone());
                s.installed = installed;
                s.pin = Some([1.0, 2.0, 3.0, -20.0, 35.0]);
            });
            let mirror = TvMirror::default();
            *mirror.8.lock().unwrap() = held;
            let mut app = App::new();
            app.insert_resource(stage.clone())
                .insert_resource(mirror.clone())
                .add_systems(Update, end_stage);
            app.update();
            assert_eq!(stage.ask().is_none(), ends, "installed {installed}");
            assert_eq!(withholds_saves(Some(&stage)), !ends);
            assert_eq!(*mirror.8.lock().unwrap(), left, "installed {installed}");
            let mut config = guacamole::scripts::smash_config(guac_script::Mode::Play);
            assert_eq!(
                listed(Some(&stage), "level_208_ice", &mut config).is_empty(),
                ends
            );
        }
    }

    /// Every joint inside the sphere with the skin to spare: a limb that
    /// reached past it would be cut by the picture's edge.
    #[test]
    fn the_sphere_holds_every_joint() {
        assert_eq!(sphere(&[]), None);
        let pts = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 4.0, 0.0),
            Vec3::new(1.0, 1.0, 1.0),
        ];
        let (c, r) = sphere(&pts).unwrap();
        assert!(pts.iter().all(|p| (*p - c).length() <= r - SKIN + 1e-5));
        assert!((r - (Vec3::new(2.0, 4.0, 1.0).length() / 2.0 + SKIN)).abs() < 1e-5);
    }
}
