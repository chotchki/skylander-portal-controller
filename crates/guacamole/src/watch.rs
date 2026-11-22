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
use guac_script::{Handle, Value, Vm};
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
    /// The actor its script's `player` local names, with its place: a
    /// placement that picks a player (a hat box, a chest) shows whom
    /// (openspec AS.224).
    pub player: Option<String>,
    /// Its script timer (`system.script timer`) and its `player ref timer`
    /// local, seconds (openspec AS.224).
    pub timer: Option<f32>,
    pub player_ref_timer: Option<f32>,
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
    /// The members of the `Minions` set in its order, each as `name
    /// (removed) at x,y,z` (openspec AS.224), the seated hero at his
    /// pivot (the as-ship-parts review).
    pub minions: Vec<String>,
    /// The joined hero's own state this frame (openspec AS.408); none until
    /// he's joined.
    pub hero: Option<HeroWatch>,
}

/// The joined hero as the watch reads him (openspec AS.408): what his
/// scripts are in beside what his controller did, so a stop shows whether
/// his scripts held him (a freeze) or he pushed and the seam held him.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct HeroWatch {
    /// His placement's behaviour, lowercased.
    pub behaviour: Option<String>,
    /// His walk across the ground as his controller asked it this frame,
    /// m/s, and what the move made of it against what it met, m/s.
    pub walk: f32,
    pub solved: f32,
    /// The edge probe holding his controller off.
    pub held_off: bool,
    /// Every awake actor but him within [`NEAR`] of his pivot across the
    /// ground, nearest first, as `name at d m`: what may pin him.
    pub near: Vec<String>,
    /// The seam bodies within [`TOUCH`] of his solid where he stands, one
    /// each, nearest first, as `name d m` (negative inside): what his move
    /// meets (an actor's body by its placement, a prop's, the level's).
    pub touching: Vec<String>,
    /// How the engine walks him ([`ScriptWorld::walk_read`]): `mover`,
    /// `written`, `pad` or `still`, with his ground top speed.
    pub engine: Option<String>,
}

/// How near the seam's bodies count as against him for
/// [`HeroWatch::touching`], metres.
pub(crate) const TOUCH: f32 = 0.25;

/// [`HeroWatch::touching`] of the planes the seam gave at his solid: one per
/// body, its nearest plane's distance, nearest first, each named by `name`.
pub(crate) fn touching(
    planes: &[guac_motion::seam::ContactPlane],
    name: impl Fn(guac_motion::seam::BodyId) -> String,
) -> Vec<String> {
    let mut by: Vec<(guac_motion::seam::BodyId, f32)> = Vec::new();
    for p in planes {
        match by.iter_mut().find(|(b, _)| *b == p.body) {
            Some((_, d)) => *d = d.min(p.distance),
            None => by.push((p.body, p.distance)),
        }
    }
    by.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.0.cmp(&b.0.0)));
    by.into_iter()
        .map(|(b, d)| format!("{} {d:.2} m", name(b)))
        .collect()
}

/// How near an actor stands to the hero for [`HeroWatch::near`], metres.
const NEAR: f32 = 2.5;

/// [`HeroWatch`] for the hero joined as `h`, whose controller is `hero`.
pub(crate) fn hero_watch(
    vm: &Vm,
    world: &ScriptWorld,
    h: Option<Handle>,
    hero: &guac_motion::hero::Hero,
) -> HeroWatch {
    let across = |v: Vec3| v.x.hypot(v.z);
    let mut near: Vec<(f32, String)> = world
        .actors()
        .filter(|(o, a)| Some(*o) != h && a.active && !a.removed)
        .filter_map(|(o, a)| {
            let d = across(a.position - hero.pivot);
            (d <= NEAR).then(|| (d, world.name(o).to_owned()))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    HeroWatch {
        behaviour: h
            .and_then(|h| vm.placement_of(h))
            .and_then(|id| vm.behaviour(id))
            .map(str::to_ascii_lowercase),
        walk: across(hero.walk.velocity),
        solved: across(hero.solved),
        held_off: hero.held_off,
        near: near
            .into_iter()
            .map(|(d, n)| format!("{n} at {d:.2} m"))
            .collect(),
        touching: Vec::new(),
        engine: h.and_then(|h| world.walk_read(h)),
    }
}

/// Each name in `names` off `vm` and `world`, the level's file linked as
/// `level`, with decision 17's list and `Cutscene State`. `seated` is the
/// joined hero's handle, read at `hero`'s pivot: his actor's own place is
/// where he joined, since the scripts read him off the engine.
pub(crate) fn read(
    vm: &Vm,
    world: &ScriptWorld,
    level: u16,
    names: &[String],
    hero: Option<(Vec3, f32)>,
    seated: Option<Handle>,
    figure: Option<(u32, u32)>,
) -> WatchShot {
    let seat = seated.zip(hero.map(|(at, _)| at));
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
                    player: None,
                    timer: None,
                    player_ref_timer: None,
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
                player: vm
                    .placement_of(h)
                    .and_then(|id| vm.local(id, "player"))
                    .map(|v| describe(vm, world, v, seat)),
                timer: vm.placement_of(h).and_then(|id| vm.active_seconds(id)),
                player_ref_timer: vm
                    .placement_of(h)
                    .and_then(|id| vm.local(id, "player ref timer"))
                    .and_then(|v| vm.value(v).as_f32()),
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
    let minions = vm
        .program()
        .global_named("Minions")
        .and_then(|g| vm.global_peek(g))
        .and_then(|v| vm.set(v))
        .map(|s| s.iter().map(|m| describe(vm, world, *m, seat)).collect())
        .unwrap_or_default();
    WatchShot {
        placements,
        equivalence: anchors::equivalence(vm, world, level, hero, figure),
        cutscene_state,
        minions,
        hero: None,
    }
}

/// An actor a script holds as `name (removed) at x,y,z`.
fn describe(vm: &Vm, world: &ScriptWorld, v: Value, seat: Option<(Handle, Vec3)>) -> String {
    match vm.value(v) {
        Value::Entity(h) => match world.actor(h) {
            Some(a) => {
                let at = seat.filter(|(s, _)| *s == h).map_or(a.position, |(_, p)| p);
                format!(
                    "{}{} at {:.1},{:.1},{:.1}",
                    world.name(h),
                    if a.removed { " (removed)" } else { "" },
                    at.x,
                    at.y,
                    at.z
                )
            }
            None => format!("{h:#x} (no actor)"),
        },
        other => format!("{other:?}"),
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
    seam: crate::physics::AvianSeam<'w, 's>,
    embodied: Option<Res<'w, crate::creatures::Embodied>>,
}

impl WatchShots<'_, '_> {
    /// A seam body by what it is: an actor's by its placement's name, a
    /// prop's by its object, else the level's.
    fn body_name(&self, world: &ScriptWorld, id: guac_motion::seam::BodyId) -> String {
        if let Some(h) = self.embodied.as_deref().and_then(|e| e.whose(id)) {
            return match world.actor(h) {
                Some(_) => world.name(h).to_owned(),
                None => "hero".to_owned(),
            };
        }
        match id.0.checked_sub(guacamole::props::PROP_BODIES) {
            Some(i) if id.0 < crate::creatures::ACTOR_BODIES => format!("prop {i}"),
            _ => format!("level {}", id.0),
        }
    }

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
        let seated = joined.and_then(|(r, _)| match r.join {
            Join::Joined(id) => rt.vm.handle_of(id),
            _ => None,
        });
        let figure = joined.and_then(|(r, _)| {
            self.earning
                .as_deref()?
                .slots
                .get(r.slot.as_usize())?
                .as_ref()
                .map(|e| (e.progress.xp, e.progress.gold))
        });
        let mut shot = read(&rt.vm, world, level, &names, hero, seated, figure);
        shot.hero = joined.map(|(_, b)| {
            let mut w = hero_watch(&rt.vm, world, seated, &b.hero);
            let solid = b.hero.solid();
            let pose = solid.pose(guac_motion::seam::Pose::new(
                b.hero.pivot,
                b.hero.rotation(),
            ));
            let planes = guac_motion::seam::PhysicsSeam::contacts(
                &self.seam,
                &solid.in_world(),
                pose,
                TOUCH,
                &b.exclude,
            );
            w.touching = touching(&planes, |id| self.body_name(world, id));
            w
        });
        Some(shot)
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
        let shot = read(
            &rt.vm,
            world,
            level,
            &names,
            Some(hero),
            None,
            Some((7, 11)),
        );
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
                player: None,
                timer: None,
                player_ref_timer: None,
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
        // His own read, his scripts' behaviour among it (openspec AS.408).
        let me = s.hero.as_ref().expect("his own read");
        assert!(me.behaviour.is_some(), "{me:?}");
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

    /// The as-ship-parts review: the joined hero in `Minions` reads at his
    /// pivot, not at his actor's own place (where he joined, which the
    /// scripts never read for a hero); anyone else at the actor's.
    #[test]
    fn the_seated_hero_reads_at_his_pivot() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        app.update();
        let (_, h) = placement_named(&app, FIGHT_CREATURE);
        let w = app.world();
        let (vm, world) = (
            &w.resource::<ScriptRuntime>().vm,
            w.resource::<ScriptWorld>(),
        );
        let own = world.actor(h).unwrap().position;
        let pivot = own + Vec3::new(40.0, 5.0, -30.0);
        let at = |p: Vec3| format!("at {:.1},{:.1},{:.1}", p.x, p.y, p.z);
        let seated = describe(vm, world, Value::Entity(h), Some((h, pivot)));
        assert!(seated.ends_with(&at(pivot)), "{seated}");
        let other = describe(vm, world, Value::Entity(h), Some((h ^ 1, pivot)));
        assert!(other.ends_with(&at(own)), "{other}");
        let none = describe(vm, world, Value::Entity(h), None);
        assert!(none.ends_with(&at(own)), "{none}");
    }

    /// openspec AS.408: the joined hero's own read: his behaviour as his
    /// scripts hold it, his asked and solved walks across the ground, and
    /// the awake actors within 2.5 m of him nearest first, him and anyone
    /// farther or asleep left out.
    #[test]
    fn the_heros_watch_reads_his_scripts_his_walk_and_whos_by_him() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        for _ in 0..30 {
            app.update();
        }
        let (_, creature) = placement_named(&app, FIGHT_CREATURE);
        let w = app.world_mut();
        let mut q = w.query::<&HeroBody>();
        let mut hero = q.single(w).unwrap().hero.clone();
        let w = app.world();
        let (vm, world) = (
            &w.resource::<ScriptRuntime>().vm,
            w.resource::<ScriptWorld>(),
        );
        let c = world.actor(creature).unwrap().position;
        hero.pivot = c + Vec3::new(1.2, 0.0, 1.6);
        hero.walk.velocity = Vec3::new(3.0, -9.0, 4.0);
        hero.solved = Vec3::new(0.0, 2.0, 0.5);
        hero.held_off = true;
        let watch = hero_watch(vm, world, None, &hero);
        assert_eq!((watch.walk, watch.solved), (5.0, 0.5));
        assert!(watch.held_off);
        assert_eq!(watch.behaviour, None, "no placement named");
        assert_eq!(
            watch.near.first().map(String::as_str),
            Some(format!("{} at 2.00 m", world.name(creature)).as_str()),
            "{:?}",
            watch.near
        );
        // Himself left out, and his placement's behaviour read.
        let as_him = hero_watch(vm, world, Some(creature), &hero);
        assert!(
            as_him
                .near
                .iter()
                .all(|n| !n.starts_with(&world.name(creature))),
            "{:?}",
            as_him.near
        );
        assert_eq!(
            as_him.behaviour.as_deref(),
            vm.placement_of(creature)
                .and_then(|id| vm.behaviour(id))
                .map(str::to_ascii_lowercase)
                .as_deref()
        );
        assert!(as_him.behaviour.is_some());
        // How the engine walks him, read off his actor (openspec AS.756).
        assert_eq!(as_him.engine, world.walk_read(creature));
        assert!(as_him.engine.is_some());
        assert_eq!(watch.engine, None);
        // Past 2.5 m nobody.
        hero.pivot = c + Vec3::new(2.0, 0.0, 1.6);
        let far = hero_watch(vm, world, None, &hero);
        assert!(
            far.near
                .iter()
                .all(|n| !n.starts_with(&world.name(creature))),
            "{:?}",
            far.near
        );
    }

    /// openspec AS.756: what the seam has against him reads one line a
    /// body, its nearest plane's gap (inside negative), nearest first.
    #[test]
    fn what_touches_the_hero_reads_one_line_a_body_nearest_first() {
        use guac_motion::seam::{BodyId, ContactPlane};
        let plane = |body: u32, distance: f32| ContactPlane {
            body: BodyId(body),
            triangle: None,
            material: None,
            normal: Vec3::Y,
            point: Vec3::ZERO,
            distance,
        };
        let planes = [plane(7, 0.03), plane(9, 0.01), plane(7, -0.1)];
        let lines = touching(&planes, |b| format!("b{}", b.0));
        assert_eq!(lines, ["b7 -0.10 m", "b9 0.01 m"]);
        assert!(touching(&[], |_| String::new()).is_empty());
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
            None,
        );
        assert_eq!(shot.placements[0].state, "gone");
    }
}
