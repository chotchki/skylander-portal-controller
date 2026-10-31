//! `POST /api/_test/tv/watch` (openspec AH.28): the level's placements a
//! Mac driver names, read off the scripts' run every frame into
//! `/api/_test/tv`'s `watch`: each one's state (`awake`, `asleep`, `gone`,
//! or `none` for a name the level's file doesn't place), behaviour, place
//! and facing, with design decision 17's list ([`anchors::equivalence`])
//! and `Cutscene State` read the same frame. A route driven on the TV is
//! checked by what its steps did (a breakable broken, a block where its
//! pushes left it, a gate lowering, the triggers fired) with no dev entry.
//! `{"names": []}` stops the watch. A test hook: only a `test-hooks` build
//! serves the route.

use std::sync::{Arc, Mutex};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use guac_script::{Value, Vm};
use guacamole::anchors::{self, Equivalence};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::{Deserialize, Serialize};

use crate::hero::HeroBody;
use crate::hero_scripts::{HeroRun, Join, ScriptSource};
use crate::progress::Earning;

/// A route's placements fit with room (AH.8's touch 38); the cap keeps a
/// frame's read bounded whatever a driver posts.
pub(crate) const WATCH_MAX: usize = 64;

/// Past the longest placement name a level file holds; bounds the read too.
const NAME_MAX: usize = 120;

/// The names asked over `POST /api/_test/tv/watch`, shared with the
/// routes; none or empty: nothing watched.
#[derive(Resource, Clone, Default)]
pub(crate) struct WatchAsks(pub(crate) Arc<Mutex<Vec<String>>>);

impl WatchAsks {
    pub(crate) fn names(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, names: Vec<String>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = names;
    }
}

/// `POST /api/_test/tv/watch`'s body.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WatchBody {
    names: Vec<String>,
}

impl WatchBody {
    /// Its names, if each is 1 to 120 bytes and there are at most 64 (none
    /// stops the watch).
    fn names(self) -> Option<Vec<String>> {
        let ok = self.names.len() <= WATCH_MAX
            && self.names.iter().all(|n| (1..=NAME_MAX).contains(&n.len()));
        ok.then_some(self.names)
    }

    /// Takes the body's names into `asks`; false (a 400) when they aren't
    /// a watch.
    pub(crate) fn ask(self, asks: &WatchAsks) -> bool {
        match self.names() {
            Some(names) => {
                asks.set(names);
                true
            }
            None => false,
        }
    }
}

/// A watched placement as `/api/_test/tv` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WatchedShot {
    pub name: String,
    /// `awake`, `asleep` or `gone` by its actor; `none` when the level's
    /// file places nothing by the name.
    pub state: &'static str,
    /// The behaviour its script is in, lowercased as the VM spells it;
    /// none before it starts or without a script.
    pub behaviour: Option<String>,
    /// Its pivot, metres, and facing, degrees.
    pub at: Option<[f32; 3]>,
    pub facing: Option<f32>,
}

/// The watch's part of `/api/_test/tv`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WatchShot {
    pub placements: Vec<WatchedShot>,
    /// Decision 17's list read this frame (the hero's pose and his figure's
    /// experience and gold once he's joined).
    pub equivalence: Equivalence,
    /// `Cutscene State` as the run holds it; none until something made it.
    pub cutscene_state: Option<i32>,
}

/// Each name in `names` off `vm` and `world`, the level's file linked as
/// `level`, with decision 17's list and `Cutscene State`.
pub(crate) fn read(
    vm: &Vm,
    world: &ScriptWorld,
    level: u16,
    names: &[String],
    hero: Option<(Vec3, f32)>,
    figure: Option<(u32, u32)>,
) -> WatchShot {
    let placements = names
        .iter()
        .map(|name| {
            let Some((h, a)) = world
                .placed_in(level, name)
                .and_then(|h| Some((h, world.actor(h)?)))
            else {
                return WatchedShot {
                    name: name.clone(),
                    state: "none",
                    behaviour: None,
                    at: None,
                    facing: None,
                };
            };
            WatchedShot {
                name: name.clone(),
                state: if a.removed {
                    "gone"
                } else if a.active {
                    "awake"
                } else {
                    "asleep"
                },
                behaviour: vm
                    .placement_of(h)
                    .and_then(|id| vm.behaviour(id))
                    .map(str::to_ascii_lowercase),
                at: Some(a.position.to_array()),
                facing: Some(a.angles.facing.rem_euclid(360.0)),
            }
        })
        .collect();
    let cutscene_state = vm
        .program()
        .global_named("Cutscene State")
        .and_then(|g| vm.global_peek(g))
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            Value::Float(f) => Some(f as i32),
            _ => None,
        });
    WatchShot {
        placements,
        equivalence: anchors::equivalence(vm, world, level, hero, figure),
        cutscene_state,
    }
}

/// The watch's part of `/api/_test/tv`, read in the mirror's frame.
#[derive(SystemParam)]
pub(crate) struct WatchShots<'w, 's> {
    asks: Option<Res<'w, WatchAsks>>,
    runtime: Option<Res<'w, ScriptRuntime>>,
    world: Option<Res<'w, ScriptWorld>>,
    source: Option<Res<'w, ScriptSource>>,
    earning: Option<Res<'w, Earning>>,
    heroes: Query<'w, 's, (&'static HeroRun, &'static HeroBody)>,
}

impl WatchShots<'_, '_> {
    /// None until something's watched and the level's run is made.
    pub(crate) fn shot(&self) -> Option<WatchShot> {
        let names = self.asks.as_deref()?.names();
        if names.is_empty() {
            return None;
        }
        let (rt, world) = (self.runtime.as_deref()?, self.world.as_deref()?);
        let level = self.source.as_deref()?.level_unit()?;
        let joined = self
            .heroes
            .iter()
            .find(|(r, _)| matches!(r.join, Join::Joined(_)));
        let hero = joined.map(|(_, b)| (b.hero.pivot, b.hero.facing));
        let figure = joined.and_then(|(r, _)| {
            self.earning
                .as_deref()?
                .slots
                .get(r.slot.as_usize())?
                .as_ref()
                .map(|e| (e.progress.xp, e.progress.gold))
        });
        Some(read(&rt.vm, world, level, &names, hero, figure))
    }
}

#[cfg(test)]
mod tests {
    use guac_formats::script::builder::FIGHT_CREATURE;

    use super::*;
    use crate::test_support::play::{CREATURE_AT, fight_app, placement_named};

    fn body(json: &str) -> Option<Vec<String>> {
        serde_json::from_str::<WatchBody>(json).ok()?.names()
    }

    /// A watch is up to 64 names of 1 to 120 bytes; none stops it; an
    /// empty name, a 65th, a long one or another field isn't one.
    #[test]
    fn a_watch_takes_up_to_64_names_and_none_stops_it() {
        assert_eq!(
            body(r#"{"names":["a","b(1)"]}"#),
            Some(vec!["a".into(), "b(1)".into()])
        );
        assert_eq!(body(r#"{"names":[]}"#), Some(Vec::new()));
        let n = |k: usize| format!(r#"{{"names":[{}]}}"#, vec!["\"x\""; k].join(","));
        assert!(body(&n(64)).is_some());
        assert_eq!(body(&n(65)), None);
        assert_eq!(body(r#"{"names":[""]}"#), None);
        let long = format!(r#"{{"names":["{}"]}}"#, "x".repeat(NAME_MAX + 1));
        assert_eq!(body(&long), None);
        assert_eq!(body(r#"{"names":["a"],"all":true}"#), None);
        assert_eq!(body(r#"{"name":"a"}"#), None);
        let asks = WatchAsks::default();
        assert!(
            serde_json::from_str::<WatchBody>(r#"{"names":["a"]}"#)
                .unwrap()
                .ask(&asks)
        );
        assert_eq!(asks.names(), ["a"]);
        assert!(
            !serde_json::from_str::<WatchBody>(r#"{"names":[""]}"#)
                .unwrap()
                .ask(&asks)
        );
        assert_eq!(asks.names(), ["a"], "a refused watch leaves the last");
    }

    /// The watch reads a placement off the run as its actor and script
    /// hold it: AH.25's synthetic creature awake in its behaviour where its
    /// file stands it, a name the level doesn't place `none`, and the
    /// hero's pose and figure in decision 17's list.
    #[test]
    fn a_watched_placement_reads_as_the_run_holds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        for _ in 0..30 {
            app.update();
        }
        let w = app.world();
        let rt = w.resource::<ScriptRuntime>();
        let world = w.resource::<ScriptWorld>();
        let level = w.resource::<ScriptSource>().level_unit().unwrap();
        let h = world
            .placed_in(level, FIGHT_CREATURE)
            .expect("the creature");
        let names = [FIGHT_CREATURE.to_owned(), "No_Such_Placement".to_owned()];
        let hero = (Vec3::new(1.0, 2.0, 3.0), 370.0);
        let shot = read(&rt.vm, world, level, &names, Some(hero), Some((7, 11)));
        let c = &shot.placements[0];
        assert_eq!(c.name, FIGHT_CREATURE);
        let a = world.actor(h).unwrap();
        assert_eq!(c.state, if a.active { "awake" } else { "asleep" });
        assert_eq!(c.state, "awake", "the fight's creature runs");
        assert_eq!(
            c.behaviour.as_deref(),
            rt.vm
                .placement_of(h)
                .and_then(|id| rt.vm.behaviour(id))
                .map(str::to_ascii_lowercase)
                .as_deref()
        );
        assert!(c.behaviour.is_some());
        let at = c.at.unwrap();
        assert_eq!(at, a.position.to_array());
        assert!(
            (at[0] - CREATURE_AT[0]).abs() < 3.0 && (at[2] - CREATURE_AT[2]).abs() < 3.0,
            "{at:?}"
        );
        assert_eq!(
            shot.placements[1],
            WatchedShot {
                name: "No_Such_Placement".into(),
                state: "none",
                behaviour: None,
                at: None,
                facing: None,
            }
        );
        assert_eq!(shot.equivalence.hero, Some(([1.0, 2.0, 3.0], 10.0)));
        assert_eq!(
            (shot.equivalence.experience, shot.equivalence.gold),
            (Some(7), Some(11))
        );
    }

    /// The mirror's read: nothing until names are asked (or once they're
    /// cleared), then the run's placements with the joined hero's pose and
    /// his figure's experience and gold as the TV holds them.
    #[test]
    fn the_shot_reads_once_asked_with_the_joined_heros_pose() {
        use bevy::ecs::system::RunSystemOnce;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        app.update();
        let shot = |app: &mut App| {
            app.world_mut()
                .run_system_once(|w: WatchShots| w.shot())
                .unwrap()
        };
        assert_eq!(shot(&mut app), None, "no asks resource");
        let asks = WatchAsks::default();
        app.insert_resource(asks.clone());
        assert_eq!(shot(&mut app), None, "nothing asked");
        asks.set(vec![FIGHT_CREATURE.to_owned()]);
        let s = shot(&mut app).expect("asked");
        assert_eq!(s.placements.len(), 1);
        assert_eq!(s.placements[0].state, "awake");
        let (pivot, facing) = {
            let w = app.world_mut();
            let mut q = w.query::<&HeroBody>();
            let b = q.single(w).unwrap();
            (b.hero.pivot, b.hero.facing)
        };
        let (at, f) = s.equivalence.hero.expect("his pose");
        assert_eq!(at, pivot.to_array());
        assert!((f - facing.rem_euclid(360.0)).abs() < 1e-4);
        let e = app.world().resource::<Earning>().slots[0]
            .as_ref()
            .map(|e| (e.progress.xp, e.progress.gold));
        assert!(e.is_some(), "his figure earns");
        assert_eq!(
            (s.equivalence.experience, s.equivalence.gold),
            (e.map(|e| e.0), e.map(|e| e.1))
        );
        asks.set(Vec::new());
        assert_eq!(shot(&mut app), None, "cleared");
    }

    /// Once a placement is taken off it reads `gone`.
    #[test]
    fn a_placement_taken_off_reads_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        app.update();
        let level = app.world().resource::<ScriptSource>().level_unit().unwrap();
        let (obj, _) = placement_named(&app, FIGHT_CREATURE);
        assert!(app.world_mut().resource_mut::<ScriptWorld>().take_off(obj));
        let w = app.world();
        let shot = read(
            &w.resource::<ScriptRuntime>().vm,
            w.resource::<ScriptWorld>(),
            level,
            &[FIGHT_CREATURE.to_owned()],
            None,
            None,
        );
        assert_eq!(shot.placements[0].state, "gone");
    }
}
