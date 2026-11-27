//! The hero put and a foe held for a look (openspec AS.1090), in
//! `test-hooks` builds only: what the headless bench does with `put` and the
//! census's put-back, for a driver on the TV.
//!
//! `POST /api/_test/tv/put` takes `{"hero": {"at": [x, y, z], "facing": f}}`
//! (metres and degrees: his pivot set down there, dropped onto what's under
//! it, at rest, before the next step's scripts, as the dev entry's set-down),
//! `{"hold": {"name": "<placement>", "at": [x, y, z], "facing": f}}`
//! (optionally `"hp": n`), either or both, or `{"release": true}`. A held
//! placement of the level's run is woken (its range and buffer 0, as a
//! script wakes one) and set at its place and facing, at rest, before each
//! step's scripts until released or another is held; with `hp` its health is
//! put back to `hp` before each step, and every step that left it lower is
//! counted as a hurt ([`Hurt`]). Anything else is a 400. `GET` on the same
//! path reads [`PutShot`] back.
//!
//! The first ask withholds the story save for the rest of the process: the
//! session begins again with no host, so nothing a put run does is written
//! ([`withholds_saves`]).

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use guac_script::{Handle, PlacementId, Value, Vm};
use guacamole::frame::Frame;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::hero::HeroBody;
use crate::hero_scripts::{HeroRun, Join, ScriptLog, ScriptSource};
use crate::physics::AvianSeam;

/// The most hurts kept; past it the oldest go.
const HURTS_KEPT: usize = 4096;

/// A place and a facing: metres in the level's frame, degrees.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Place {
    pub(crate) at: [f32; 3],
    pub(crate) facing: f32,
}

/// A placement held at a place, its health kept at `hp` when given.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hold {
    pub(crate) name: String,
    pub(crate) at: [f32; 3],
    pub(crate) facing: f32,
    #[serde(default)]
    pub(crate) hp: Option<f32>,
}

/// `POST /api/_test/tv/put`'s body.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PutAsk {
    #[serde(default)]
    pub(crate) hero: Option<Place>,
    #[serde(default)]
    pub(crate) hold: Option<Hold>,
    #[serde(default)]
    pub(crate) release: bool,
}

impl PutAsk {
    /// An ask the TV can act on: something asked, a release never with a
    /// hold, finite numbers, a placement's name of 1 to 64 characters with
    /// no control character, and a health over 0.
    pub(crate) fn valid(&self) -> bool {
        let finite = |p: [f32; 3], f: f32| p.iter().all(|v| v.is_finite()) && f.is_finite();
        let hero = self.hero.is_none_or(|p| finite(p.at, p.facing));
        let hold = self.hold.as_ref().is_none_or(|h| {
            (1..=64).contains(&h.name.len())
                && !h.name.chars().any(char::is_control)
                && finite(h.at, h.facing)
                && h.hp.is_none_or(|v| v.is_finite() && v > 0.0)
        });
        let something = self.hero.is_some() || self.hold.is_some() || self.release;
        something && hero && hold && !(self.release && self.hold.is_some())
    }
}

/// The hero's last set-down: the scripts' step it came before, where and
/// facing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct PutDone {
    pub(crate) step: u64,
    pub(crate) at: [f32; 3],
    pub(crate) facing: f32,
}

/// A step that left the held placement's health under what the hold put
/// back: the scripts' step, its health before it and after.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Hurt {
    pub(crate) step: u64,
    pub(crate) from: f32,
    pub(crate) to: f32,
}

/// The held placement as the last step left it, before the hold set it
/// again.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct HeldShot {
    pub(crate) name: String,
    pub(crate) at: [f32; 3],
    pub(crate) facing: f32,
    pub(crate) awake: bool,
    pub(crate) behaviour: Option<String>,
    pub(crate) hp: Option<f32>,
}

/// What's asked and what's been done, as `GET /api/_test/tv/put` reads it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PutShot {
    /// An ask has been taken: the story save is withheld from then on.
    pub(crate) asked: bool,
    /// The hero's set-down asked and not yet done.
    pub(crate) hero_asked: Option<Place>,
    pub(crate) hero_put: Option<PutDone>,
    pub(crate) hold: Option<Hold>,
    pub(crate) held: Option<HeldShot>,
    /// The hold's hurts, oldest first; a new hold starts them again.
    pub(crate) hurts: Vec<Hurt>,
    /// Why the last hold was let go unasked (no such placement).
    pub(crate) refused: Option<String>,
    /// The health the hold last put back.
    #[serde(skip)]
    pub(crate) topped: Option<f32>,
}

impl PutShot {
    /// `ask` taken: what it asks replaces what was asked.
    pub(crate) fn take(&mut self, ask: PutAsk) {
        self.asked = true;
        if let Some(p) = ask.hero {
            self.hero_asked = Some(p);
        }
        if ask.release {
            self.hold = None;
            self.held = None;
            self.topped = None;
        }
        if let Some(h) = ask.hold {
            self.hold = Some(h);
            self.held = None;
            self.hurts.clear();
            self.refused = None;
            self.topped = None;
        }
    }

    /// The health read before this step's hold puts it back: a hurt at the
    /// step before when it's under what was put back then.
    pub(crate) fn read(&mut self, step: u64, hp: Option<f32>) {
        if let (Some(set), Some(now)) = (self.topped, hp)
            && now < set
        {
            if self.hurts.len() == HURTS_KEPT {
                self.hurts.remove(0);
            }
            self.hurts.push(Hurt {
                step: step.saturating_sub(1),
                from: set,
                to: now,
            });
        }
    }
}

/// The put's state, shared by its route and the TV's app.
#[derive(Resource, Clone, Default)]
pub(crate) struct Puts(pub(crate) Arc<Mutex<PutShot>>);

impl Puts {
    fn with<T>(&self, f: impl FnOnce(&mut PutShot) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The process's one put state, shared by the route, the TV's app and the
/// story save's host filter.
pub(crate) fn shared() -> &'static Puts {
    static PUTS: std::sync::LazyLock<Puts> = std::sync::LazyLock::new(Puts::default);
    &PUTS
}

/// Whether a story save session begun now keeps its saves to itself: once
/// a put has been asked, nothing the run does is written.
pub(crate) fn withholds_saves(puts: Option<&Puts>) -> bool {
    puts.is_some_and(|p| p.with(|s| s.asked))
}

pub(crate) struct PutPlugin;

impl Plugin for PutPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.add_systems(FixedUpdate, hold.in_set(Frame::Pads));
    }
}

/// A placement's health as its scripts hold it (`description.creature
/// info.hp`, else a local `health`), and the cell's kind for the write.
fn health(vm: &Vm, id: PlacementId) -> Option<f32> {
    let number = |v: Value| match v {
        Value::Int(i) => Some(i as f32),
        Value::Float(f) => Some(f),
        _ => None,
    };
    let described = vm
        .local(id, "description")
        .map(|v| vm.value(v))
        .and_then(|d| vm.field(d, "creature info").map(|v| vm.value(v)))
        .and_then(|c| vm.field(c, "hp").map(|v| vm.value(v)))
        .and_then(number);
    described.or_else(|| vm.local(id, "health").map(|v| vm.value(v)).and_then(number))
}

/// `hp` written where [`health`] reads it, in the kind the cell holds.
fn set_health(vm: &mut Vm, id: PlacementId, hp: f32) -> bool {
    let as_kind = |old: Value| match old {
        Value::Int(_) => Some(Value::Int(hp.round() as i32)),
        Value::Float(_) => Some(Value::Float(hp)),
        _ => None,
    };
    let info = vm
        .local(id, "description")
        .map(|v| vm.value(v))
        .and_then(|d| vm.field(d, "creature info").map(|v| vm.value(v)));
    if let Some(c) = info
        && let Some(v) = vm.field(c, "hp").map(|v| vm.value(v)).and_then(as_kind)
    {
        return vm.set_field(c, "hp", v);
    }
    match vm.local(id, "health").map(|v| vm.value(v)).and_then(as_kind) {
        Some(v) => vm.set_local(id, "health", v),
        None => false,
    }
}

/// One step of the put, before the step's scripts: the hero's set-down when
/// one is asked and he's joined, then the hold.
fn hold(
    puts: Res<Puts>,
    runtime: Option<ResMut<ScriptRuntime>>,
    world: Option<ResMut<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    log: Res<ScriptLog>,
    seam: AvianSeam,
    mut heroes: Query<(&HeroRun, &mut HeroBody)>,
) {
    let (Some(mut runtime), Some(mut world), Some(level)) = (
        runtime,
        world,
        source.as_deref().and_then(ScriptSource::level_unit),
    ) else {
        return;
    };
    let step = log.last() + 1;
    let mut shot = puts.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(place) = shot.hero_asked
        && let Some((_, mut body)) = heroes
            .iter_mut()
            .find(|(r, _)| matches!(r.join, Join::Joined(_)))
    {
        crate::dev_entry::set_down(&mut body, &seam, Vec3::from_array(place.at), place.facing);
        shot.hero_asked = None;
        shot.hero_put = Some(PutDone {
            step,
            at: body.hero.pivot.to_array(),
            facing: body.hero.facing,
        });
        info!(step, at = ?place.at, facing = place.facing, "the put sets him down");
    }
    let Some(asked) = shot.hold.clone() else {
        return;
    };
    let Some(h) = world.placed_in(level, &asked.name) else {
        let why = format!("{} is no placement of this level", asked.name);
        warn!(reason = %why, "the put lets its hold go");
        shot.hold = None;
        shot.held = None;
        shot.refused = Some(why);
        return;
    };
    let id = runtime.vm.placement_of(h);
    let hp = id.and_then(|id| health(&runtime.vm, id));
    shot.read(step, hp);
    shot.held = Some(held_shot(&asked.name, h, &world, &runtime.vm, hp));
    world.wake(h);
    world.place(h, Vec3::from_array(asked.at), asked.facing);
    shot.topped = match (asked.hp, id) {
        (Some(want), Some(id)) if set_health(&mut runtime.vm, id, want) => Some(want),
        _ => None,
    };
}

fn held_shot(name: &str, h: Handle, world: &ScriptWorld, vm: &Vm, hp: Option<f32>) -> HeldShot {
    let actor = world.actor(h);
    HeldShot {
        name: name.to_owned(),
        at: actor.map_or([0.0; 3], |a| a.position.to_array()),
        facing: actor.map_or(0.0, |a| a.angles.facing),
        awake: actor.is_some_and(|a| a.active),
        behaviour: vm
            .placement_of(h)
            .and_then(|id| vm.behaviour(id))
            .map(str::to_owned),
        hp,
    }
}

/// `POST /api/_test/tv/put` (the module docs): 202 with the ask taken, 400
/// for any other body; `GET` reads the state back. Generic so a test needs
/// no `AppState`.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(puts: &Puts) -> axum::Router<S> {
    use axum::extract::rejection::JsonRejection;
    let (asks, reads) = (puts.clone(), puts.clone());
    axum::Router::new().route(
        "/api/_test/tv/put",
        axum::routing::post(move |body: Result<axum::Json<PutAsk>, JsonRejection>| {
            let ask = body.ok().map(|axum::Json(b)| b).filter(PutAsk::valid);
            let taken = ask.is_some();
            if let Some(ask) = ask {
                asks.with(|s| s.take(ask));
            }
            async move {
                if taken {
                    axum::http::StatusCode::ACCEPTED
                } else {
                    axum::http::StatusCode::BAD_REQUEST
                }
            }
        })
        .get(move || {
            let shot = reads.with(|s| s.clone());
            async move { axum::Json(shot) }
        }),
    )
}

#[cfg(test)]
mod tests {
    use bevy::input::gamepad::GamepadButton;
    use guac_formats::script::builder::FIGHT_CREATURE;

    use super::*;
    use crate::test_support::level::{FLOOR, START};
    use crate::test_support::play::{fight_app_with, hero, until};
    use crate::test_support::test_pad;

    async fn post(routes: &axum::Router, body: &str) -> u16 {
        use tower::ServiceExt;
        routes
            .clone()
            .oneshot(
                axum::http::Request::post("/api/_test/tv/put")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// openspec AS.1090: the route takes a hero's place, a hold or a
    /// release, whole and finite, and nothing else; `GET` reads back what it
    /// took, and the first ask it takes marks the run as one whose saves are
    /// withheld.
    #[tokio::test]
    async fn the_put_route_takes_a_whole_ask_and_reads_it_back() {
        use tower::ServiceExt;
        let puts = Puts::default();
        let routes = routes::<()>(&puts);
        for bad in [
            "{}",
            r#"{"release":false}"#,
            r#"{"hero":{"at":[1,2],"facing":0}}"#,
            r#"{"hero":{"at":[1,2,3]}}"#,
            r#"{"hold":{"name":"","at":[1,2,3],"facing":0}}"#,
            r#"{"hold":{"name":"Enemy_Chompy(1)","at":[1,2,3],"facing":0,"hp":0}}"#,
            r#"{"hold":{"name":"Enemy_Chompy(1)","at":[1,2,3],"facing":0},"release":true}"#,
            r#"{"hero":{"at":[1,2,3],"facing":0},"seed":4}"#,
            "not json",
        ] {
            assert_eq!(post(&routes, bad).await, 400, "{bad}");
        }
        assert!(!withholds_saves(Some(&puts)), "a refused ask marks nothing");
        let ask = r#"{"hero":{"at":[1,2,3],"facing":90},"hold":{"name":"Enemy_Chompy(1)","at":[4,5,6],"facing":270,"hp":1000}}"#;
        assert_eq!(post(&routes, ask).await, 202);
        assert!(withholds_saves(Some(&puts)));
        assert!(!withholds_saves(None));
        let got = routes
            .clone()
            .oneshot(
                axum::http::Request::get("/api/_test/tv/put")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(got.into_body(), 1 << 16).await.unwrap();
        let read: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(read["asked"], true);
        assert_eq!(read["hero_asked"]["at"], serde_json::json!([1.0, 2.0, 3.0]));
        assert_eq!(read["hold"]["name"], "Enemy_Chompy(1)");
        assert_eq!(read["hold"]["hp"], 1000.0);
        assert_eq!(post(&routes, r#"{"release":true}"#).await, 202);
        assert!(puts.with(|s| s.hold.is_none() && s.hero_asked.is_some()));
    }

    /// A hurt is a step that left the health under what the hold put back,
    /// at the step before the read; a health at or over it, or none, is no
    /// hurt, and a new hold starts the count again.
    #[test]
    fn a_hurt_is_a_step_that_left_the_health_under_what_was_put_back() {
        let mut s = PutShot::default();
        s.read(10, Some(5.0));
        assert!(s.hurts.is_empty(), "nothing put back yet");
        s.topped = Some(100.0);
        s.read(11, Some(100.0));
        s.read(12, None);
        s.read(13, Some(120.0));
        assert!(s.hurts.is_empty(), "{:?}", s.hurts);
        s.read(14, Some(76.0));
        assert_eq!(
            s.hurts,
            vec![Hurt {
                step: 13,
                from: 100.0,
                to: 76.0
            }]
        );
        s.take(PutAsk {
            hold: Some(Hold {
                name: "x".into(),
                at: [0.0; 3],
                facing: 0.0,
                hp: None,
            }),
            ..PutAsk::default()
        });
        assert!(s.hurts.is_empty() && s.topped.is_none());
    }

    /// openspec AS.1090 on the TV's app: asked once his scripts have
    /// joined, the hero is set down where asked, facing as asked; the
    /// synthetic creature is held where asked, off its own place, awake,
    /// through the steps after, its health put back to the asked; his first
    /// button's ball lands on it there and is counted as a hurt, and it
    /// isn't beaten for it.
    #[test]
    fn the_hero_is_put_and_the_creature_held_its_hurts_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let puts = Puts::default();
        let ours = puts.clone();
        let (mut app, pad) = fight_app_with(tmp.path(), move |app| {
            app.insert_resource(ours).add_plugins(PutPlugin);
        });
        let held = [START[0] + 4.0, FLOOR, START[2] + 3.2];
        let me = [START[0] + 4.0, FLOOR + 0.5, START[2]];
        puts.with(|s| {
            s.take(PutAsk {
                hero: Some(Place {
                    at: me,
                    facing: 0.0,
                }),
                hold: Some(Hold {
                    name: FIGHT_CREATURE.into(),
                    at: held,
                    facing: 180.0,
                    hp: Some(1000.0),
                }),
                release: false,
            })
        });
        until(&mut app, "him set down", |_| puts.with(|s| s.hero_put.is_some()));
        for _ in 0..10 {
            app.update();
        }
        let h = hero(&app);
        assert!(
            h.pivot.with_y(0.0).distance(Vec3::from_array(me).with_y(0.0)) < 0.05,
            "{:?}",
            h.pivot
        );
        assert!((h.facing - 0.0).abs() < 0.5, "{}", h.facing);
        let shot = puts.with(|s| s.held.clone()).expect("held");
        assert!(shot.awake, "{shot:?}");
        assert!(
            Vec3::from_array(shot.at).distance(Vec3::from_array(held)) < 0.05,
            "{shot:?}"
        );
        assert_eq!(shot.hp, Some(1000.0), "{shot:?}");
        assert!(puts.with(|s| s.hurts.is_empty()));
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "a hurt counted", |_| puts.with(|s| !s.hurts.is_empty()));
        for _ in 0..3 {
            app.update();
        }
        let (hurt, shot) = puts.with(|s| (s.hurts[0], s.held.clone().unwrap()));
        assert!(hurt.to < hurt.from && hurt.from == 1000.0, "{hurt:?}");
        assert_eq!(shot.hp, Some(1000.0), "put back: {shot:?}");
        assert!(
            Vec3::from_array(shot.at).distance(Vec3::from_array(held)) < 0.05,
            "held through the hit: {shot:?}"
        );
        puts.with(|s| s.take(PutAsk {
            release: true,
            ..PutAsk::default()
        }));
        app.update();
        assert!(puts.with(|s| s.held.is_none()));
    }
}
