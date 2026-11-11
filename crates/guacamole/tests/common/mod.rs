//! What the headless disc tests over level 213 share (openspec AH.18-AH.21):
//! the script part linked off `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`), the VM over guacamole's host, Eruptor's controller on the
//! parry side of the seam over the level's own collision, and the step the
//! TV's order runs. Each test binary uses what it needs of it.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guac_extract::levels;
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_ir::{DumpIdentity, Sha256};
use guac_motion::hero::Hero;
use guac_motion::parry::ParryWorld;
use guac_motion::proxy::Surfaces;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, PhysicsSeam, Pose, Quat, Vec3};
use guac_motion::tuning::{Tuning, eruptor_solid};
use guac_script::{
    Generator, Halt, Handle, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role,
    Unit, Value, Vm,
};
use guac_server::profiles::ProfileStore;
use guac_server::progress::{Drainer, ProgressSink, ProgressionSlot};
use guacamole::figure::{self, Layout, Tag, Values};
use guacamole::level_scene;
use guacamole::player_loop::PlayerLoop;
use guacamole::props::{self, LevelProps, PROP_BODIES};
use guacamole::script_engine::{Engine, NodePose, Pad};
use guacamole::scripts::{
    self, Files, ScriptWorld, WorldHost, headless_handles, headless_objects, headless_spawner,
};
use skylander_core::FigureId;
use skylander_rpcs3_control::FigureProgress;

/// Eruptor's figure as guacamole's store keys it.
pub const ERUPTOR_FIGURE: &str = "000009-0000";

/// `key` from the environment, else from the repo's `.env.dev`.
pub fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        return Some(v);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

pub fn dump() -> PathBuf {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let root = PathBuf::from(root);
    assert!(root.join("PS3_GAME/USRDIR").is_dir(), "no PS3_GAME/USRDIR");
    root
}

pub fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

/// The script part's archives in link order, as `level_scripts` links them:
/// the objectives screen's entry before the level's since AS.53 (the screens
/// part's, linked with no placement run).
pub const ARCHIVES: [(&str, Role); 6] = [
    ("permanent/bootstrap.bld", Role::Bootstrap),
    ("permanent/global.bld", Role::Global),
    ("character/init_setup.bld", Role::Other),
    ("character/009_eruptor.bld", Role::Other),
    ("misc/ui_objectivesscreen.bld", Role::Other),
    ("level/level_213_tutotial.bld", Role::Other),
];

/// The units whose running placements start with the level, in order.
pub const ORDER: [u16; 3] = [1, 2, 5];
/// Eruptor's file: its placements join with him.
pub const ERUPTOR: u16 = 3;
/// The level's file.
pub const LEVEL: u16 = 5;

/// The first hero's handle in a headless run: past every placement's and
/// object's. The next hero after a take-off joins under one of his own
/// ([`Run::hero`]), as the TV's next hero is an entity of his own
/// (openspec AH.191).
pub const HERO: Handle = 0x7000_0000;
/// The hero's body in the seam; each creature's counts up from past it.
pub const HERO_BODY: BodyId = BodyId(9_000);

/// The engine a headless run owns: Eruptor's controller and the creatures'
/// bodies on the parry side of the seam over the level's collision, and
/// with [`Setup::props`] the level's props standing by AG.25's rule.
pub struct Bench {
    pub heroes: HashMap<Handle, Hero>,
    pub world: ParryWorld,
    pub view: f32,
    /// Whether `view` stands for the viewport's turn over the camera's
    /// scripts (openspec AG.28): the runs that steer the stick by a recorded
    /// camera's view, every run but those judging the scripts' camera.
    pub hold_view: bool,
    pub pads: [Pad; 2],
    /// The bodies in the seam now, by actor: the heroes and the awake
    /// creatures, as the TV keeps them (openspec AS.85: an actor asleep or
    /// taken off has none, so the scripts' engine knows it by none).
    pub bodies: HashMap<Handle, BodyId>,
    /// Each actor's body once it has had one, kept while it has none, so
    /// one waking again is the same body, as the TV's are.
    pub ids: HashMap<Handle, BodyId>,
    pub props: Option<StandingProps>,
}

/// The level's props in a headless run (openspec AH.21), as the TV stands
/// them: each solid in the seam while its actor stands (awake by the
/// activity rule or its scripts, not taken off) as the scripts leave it
/// (openspec AG.60: the actor is the prop's one holder); by AG.25's rule
/// for a prop no actor stands for.
pub struct StandingProps {
    pub level: LevelProps,
    /// Each prop's placement handle, by its object.
    pub handles: HashMap<u32, Handle>,
    /// The props whose solid is in the seam now, by object.
    pub standing: HashSet<u32>,
    /// The body each one's solid is in the seam as.
    pub holds: HashMap<u32, Body>,
}

impl StandingProps {
    /// The seam made to hold what stands with the hero at `hero`, each as
    /// its actor in `world` holds it; returns the objects taken down.
    pub fn stand(&mut self, seam: &mut ParryWorld, hero: Vec3, world: &ScriptWorld) -> Vec<u32> {
        let mut down = Vec::new();
        for p in &self.level.props {
            let actor = self.handles.get(&p.object).and_then(|h| world.actor(*h));
            let stands = match actor {
                Some(a) => a.active && !a.removed,
                None => p.stands(hero),
            };
            let body = match (stands, actor) {
                (false, _) => None,
                (true, Some(a)) => props::actor_body(p, a),
                (true, None) => p.body.clone(),
            };
            match body {
                Some(b) => {
                    if self.holds.get(&p.object) != Some(&b) {
                        seam.set_body(&b).unwrap();
                        self.holds.insert(p.object, b);
                    }
                }
                None => {
                    if let Some(b) = self.holds.remove(&p.object) {
                        seam.remove_body(b.id);
                        down.push(p.object);
                    }
                }
            }
        }
        self.standing = self.holds.keys().copied().collect();
        down
    }

    fn object_of(&self, actor: Handle) -> Option<u32> {
        self.handles
            .iter()
            .find(|(_, h)| **h == actor)
            .map(|(o, _)| *o)
    }
}

impl Engine for Bench {
    fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
        self.heroes.get_mut(&h)
    }
    fn heroes(&self) -> Vec<Handle> {
        self.heroes.keys().copied().collect()
    }
    fn seam(&self) -> &dyn PhysicsSeam {
        &self.world
    }
    fn view_heading(&self) -> f32 {
        self.view
    }
    fn holds_view(&self) -> bool {
        self.hold_view
    }
    fn pad(&self, n: usize) -> Pad {
        self.pads.get(n).copied().unwrap_or_default()
    }
    fn body(&self, actor: Handle) -> Option<BodyId> {
        if let Some(b) = self.bodies.get(&actor) {
            return Some(*b);
        }
        let props = self.props.as_ref()?;
        let object = props.object_of(actor)?;
        props
            .standing
            .contains(&object)
            .then_some(BodyId(PROP_BODIES + object))
    }
    fn actor_of(&self, body: BodyId) -> Option<Handle> {
        if let Some((h, _)) = self.bodies.iter().find(|(_, b)| **b == body) {
            return Some(*h);
        }
        let props = self.props.as_ref()?;
        let object = body.0.checked_sub(PROP_BODIES)?;
        props
            .standing
            .contains(&object)
            .then(|| props.handles.get(&object).copied())
            .flatten()
    }
}

impl Bench {
    /// The props standing by where the hero is, less what the scripts took
    /// off; the objects taken down.
    pub fn stand_props(&mut self, world: &ScriptWorld) -> Vec<u32> {
        let Some(hero) = self.heroes.values().next().map(|h| h.pivot) else {
            return Vec::new();
        };
        match &mut self.props {
            Some(p) => p.stand(&mut self.world, hero, world),
            None => Vec::new(),
        }
    }

    /// The hero under `new` from here, where he was `old`'s: his body in the
    /// seam the same one (openspec AH.191: the next hero after a take-off
    /// joins as an actor of his own).
    pub fn rekey(&mut self, old: Handle, new: Handle) {
        if let Some(h) = self.heroes.remove(&old) {
            self.heroes.insert(new, h);
        }
        if let Some(b) = self.bodies.remove(&old) {
            self.bodies.insert(new, b);
        }
    }

    /// Each body where its actor stands after the frame: the hero's solid
    /// and each awake creature's, as the seam holds solids (0.05 m in).
    pub fn place_bodies(&mut self, world: &ScriptWorld) {
        for (h, hero) in &self.heroes {
            // Hung he has no body, as the TV's (AS.115).
            if hero.hung {
                if let Some(b) = self.bodies.remove(h) {
                    self.world.remove_body(b);
                }
                continue;
            }
            let id = *self.bodies.entry(*h).or_insert(HERO_BODY);
            let solid = *hero.solid();
            self.world
                .set_body(&Body {
                    id,
                    kind: BodyKind::Kinematic,
                    pose: solid.pose(Pose::new(hero.pivot, hero.rotation())),
                    collider: Collider::Convex(solid.in_world()),
                })
                .unwrap();
        }
        let walkers = world.creature_bodies();
        let awake: Vec<Handle> = walkers.iter().map(|w| w.0).collect();
        for (h, solid, pose) in walkers {
            let next = BodyId(HERO_BODY.0 + 1 + (self.heroes.len() + self.ids.len()) as u32);
            let id = *self.ids.entry(h).or_insert(next);
            self.bodies.insert(h, id);
            self.world
                .set_body(&Body {
                    id,
                    kind: BodyKind::Kinematic,
                    pose,
                    collider: Collider::Convex(solid.in_world()),
                })
                .unwrap();
        }
        // A creature asleep or taken off is out of the world, and has no
        // body for the scripts' engine either, as the TV's (AS.85).
        let gone: Vec<(Handle, BodyId)> = self
            .bodies
            .iter()
            .filter(|(h, _)| !self.heroes.contains_key(h) && !awake.contains(h))
            .map(|(h, b)| (*h, *b))
            .collect();
        for (h, b) in gone {
            self.world.remove_body(b);
            self.bodies.remove(&h);
        }
    }
}

/// A headless run: the VM, the host's world, the engine, the generator.
pub struct Run {
    pub vm: Vm,
    pub world: ScriptWorld,
    pub bench: Bench,
    pub generator: Generator,
    pub files: Files,
    pub handles: HashMap<Obj, Handle>,
    pub next: Handle,
    /// The hero's handle: [`HERO`], then after each take-off one of his own.
    pub hero: Handle,
    pub minion: Option<PlacementId>,
    pub steps: u32,
    /// The portal manager's tick as the player loop runs it (AH.20).
    pub player_loop: PlayerLoop,
    /// Where the figure's values sit in its tag's save area, AH.2's rows off
    /// the disc (AH.23).
    pub layout: Layout,
    /// The figure's tag data once he's joined (AH.23).
    pub figure: Option<Value>,
    /// What poses his hands each step (AH.50), and his idle's time at step
    /// 0: his attach nodes off his model's attach list by his idle and what
    /// his scripts play on him.
    pub rig: guacamole::hands::Rig,
    pub idle0: f64,
    /// A recorded pose's lean ([`lean_of`]) and where he stood in it: while
    /// his pivot holds there, his posed nodes are turned by it before what
    /// hangs from them is set. A run set down at a recorded pose leans by
    /// it until AH.68 tilts him (guacamole poses him upright, the oracle
    /// tilts his model to the ground; stand-in ledger: `upright`).
    pub lean: Option<(Vec3, Quat)>,
    /// With `GUAC_SURFACE_CENSUS` set, the clones seen standing for actors
    /// (openspec AS.168, `Actor::stands_for_actors`), those the hero came
    /// within 3 m of and the people that came within a metre of one (a
    /// person's ground is what the ray caster meets, AS.166), each logged
    /// once as it's first seen.
    pub surfaces: (HashSet<Handle>, HashSet<Handle>, HashSet<(Handle, Handle)>),
    /// With `GUAC_HANG_CENSUS` set (openspec AS.189), what the hero hung
    /// from while he hangs, and each let-go: the step, what let him go and
    /// where, logged again 60 steps on.
    pub hangs: (Option<String>, Vec<(u32, String, Vec3)>),
    /// The hero whose entry beam hasn't activated him yet (openspec AS.218):
    /// the TV's step runs the beams with him waiting, so his join takes its
    /// own minion and an enemy minion's beam landing then still gets its AI.
    pub entering: Option<Handle>,
    /// His figure's toy id: Eruptor's 9, or [`Setup::character`]'s.
    pub toy: u16,
}

/// His model's turn off his facing alone for a recorded `[tilt, heading,
/// roll]` (degrees, as a record's `rotation` holds them): his whole turn as
/// AH.50 composes it, less the heading his nodes are already turned by.
pub fn lean_of(rotation: [f32; 3]) -> Quat {
    let whole =
        guac_extract::models::euler_transform(&guac_formats::igz::layouts::EulerTransform {
            position: [0.0; 3],
            rotation,
        });
    Quat::from_rotation_y(rotation[1].to_radians()).inverse() * Quat::from_array(whole.rotation)
}

/// Eruptor's toy, as his figure's tag names it: the toy id 9, the base
/// subtype, a serial of our own.
pub fn eruptor_tag(values: Values) -> Tag {
    Tag {
        toy_id: 9,
        subtype: 0,
        serial: 1,
        values,
    }
}

impl Run {
    /// His attach nodes posed as the frame left his clips (his idle at its
    /// time this step), and what hangs from them set there (AH.50).
    pub fn pose_hands(&mut self) {
        let idle = self.idle0 + f64::from(self.steps) / 60.0;
        self.rig.pose(&mut self.world, self.hero, idle);
        if let Some((at, lean)) = self.lean
            && self.hero().pivot.distance(at) < 0.01
            && let Some(nodes) = self.world.nodes(self.hero)
        {
            let leaned = nodes
                .iter()
                .map(|n| NodePose {
                    at: lean * n.at,
                    turn: lean * n.turn,
                })
                .collect();
            self.world.set_nodes(self.hero, leaned);
        }
        self.world.place_hung(&mut self.bench);
    }

    pub fn with_host<T>(&mut self, f: impl FnOnce(&mut Vm, &mut WorldHost<'_, '_>) -> T) -> T {
        let mut spawn = headless_spawner(self.next);
        let t = {
            let mut host = WorldHost {
                world: &mut self.world,
                engine: &mut self.bench,
                generator: &mut self.generator,
                spawn: &mut spawn,
            };
            f(&mut self.vm, &mut host)
        };
        self.next = spawn(Value::Null);
        t
    }

    /// One step, in the TV's order (`guacamole::frame`, openspec AH.59):
    /// the pads read, every managed placement woken or put to sleep by the
    /// activity rule, the frame's slides, the walk (his minion first, every
    /// awake placement as it woke with the player loop's slices in their
    /// placements' places, the mover update last), the clones made outside
    /// it added, the props stood; then the engine's frame: the hero moved by
    /// the walk his scripts left, the sequences on, the creatures walked,
    /// what the scripts threw flying, his hands posed and what hangs from
    /// them set there (AH.50), the bodies placed where their actors stand.
    pub fn step(&mut self) -> Result<(), Halt> {
        self.step_with(&mut |_, _, _| Ok(()))
    }

    /// [`Run::step`], with `then` run in each held placement's place in the
    /// walk after the player loop's slice there (`PlayerLoop::walk_with`):
    /// a test's call where the original's order would make it.
    pub fn step_with(&mut self, then: &mut guac_script::Native<'_>) -> Result<(), Halt> {
        let pads = self.bench.pads;
        self.world.read_pads(pads);
        self.vm.set_activity(&self.world.activity(&mut self.bench));
        // The frame's slides and the walk in the original's order, the
        // player loop's slices in their places (openspec AH.59).
        let joined = self.minion.is_some();
        let player_loop = self.player_loop.clone();
        let mut stopped = Vec::new();
        // The frame's slides, then the entry beams between them and the walk
        // as the TV's step runs them (openspec AH.224): a level's own, an
        // enemy minion's summoned through one (AS.200). The headless join
        // activates the hero with none.
        let slid = self.with_host(|vm, host| vm.slide(host))?;
        assert!(slid.stopped.is_empty(), "strict mode stops by halting");
        if scripts::entry_beam_count(&mut self.vm).unwrap_or(0) > 0 {
            let waiting = self.entering;
            if let Some(m) = self.with_host(|vm, host| scripts::entry_beams(vm, host, waiting))? {
                self.minion = Some(m);
                self.entering = None;
            }
        }
        let r =
            self.with_host(|vm, host| player_loop.walk_with(vm, host, joined, &mut stopped, then));
        assert!(stopped.is_empty(), "strict mode stops by halting");
        // The level's end where the host doesn't run the original's, as the
        // TV's step makes it (openspec AS.15).
        self.with_host(guacamole::level_end::step)?;
        guacamole::level_switch::step(&self.vm, &mut self.world);
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        // The props stand by where he is as the step began, after the
        // scripts' step as the TV's do: a breakable broken this step is out
        // of the seam before anything moves.
        self.bench.stand_props(&self.world);
        let report = r?;
        assert!(report.stopped.is_empty(), "strict mode stops by halting");
        let dt = 1.0 / 60.0;
        let push = self.bench.pads[0].stick.push(self.bench.view);
        for (h, hero) in &mut self.bench.heroes {
            self.world.step_scripted(
                *h,
                hero,
                &self.bench.world,
                push,
                dt,
                &[HERO_BODY],
                Surfaces::NONE,
            );
            hero.catch_fall();
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;

        self.pose_hands();
        self.bench.place_bodies(&self.world);
        Ok(())
    }

    /// [`Run::step`] on a run in play mode (openspec AH.28): the scripts
    /// this step stopped, each logged by the VM and the run going on as the
    /// TV's does, where a strict run would halt.
    pub fn step_play(&mut self) -> Result<Vec<Halt>, Halt> {
        self.step_play_with(&mut |_, _, _| Ok(()))
    }

    /// [`Run::step_play`], with `then` in the walk's native places, the TV's
    /// way of reaching in (openspec AS.130).
    pub fn step_play_with(
        &mut self,
        then: &mut guac_script::Native<'_>,
    ) -> Result<Vec<Halt>, Halt> {
        let pads = self.bench.pads;
        self.world.read_pads(pads);
        self.vm.set_activity(&self.world.activity(&mut self.bench));
        let joined = self.minion.is_some();
        let player_loop = self.player_loop.clone();
        let mut stopped = Vec::new();
        // The frame's slides, then the entry beams, as `step_with`'s.
        match self.with_host(|vm, host| vm.slide(host)) {
            Ok(slid) => stopped.extend(slid.stopped),
            Err(h) => stopped.push(h),
        }
        if scripts::entry_beam_count(&mut self.vm).unwrap_or(0) > 0 {
            let waiting = self.entering;
            match self.with_host(|vm, host| scripts::entry_beams(vm, host, waiting)) {
                Ok(Some(m)) => {
                    self.minion = Some(m);
                    self.entering = None;
                }
                Ok(None) => {}
                Err(h) => stopped.push(h),
            }
        }
        let r =
            self.with_host(|vm, host| player_loop.walk_with(vm, host, joined, &mut stopped, then));
        if let Err(h) = self.with_host(guacamole::level_end::step) {
            stopped.push(h);
        }
        guacamole::level_switch::step(&self.vm, &mut self.world);
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        self.bench.stand_props(&self.world);
        stopped.extend(r?.stopped);
        let dt = 1.0 / 60.0;
        let push = self.bench.pads[0].stick.push(self.bench.view);
        for (h, hero) in &mut self.bench.heroes {
            self.world.step_scripted(
                *h,
                hero,
                &self.bench.world,
                push,
                dt,
                &[HERO_BODY],
                Surfaces::NONE,
            );
            hero.catch_fall();
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;
        self.pose_hands();
        self.bench.place_bodies(&self.world);
        self.census_surfaces();
        self.census_hangs();
        Ok(stopped)
    }

    /// `GUAC_HANG_CENSUS`'s lines (openspec AS.189): each let-go of the
    /// hero, what he hung from, where he's let go and where he is 60 steps
    /// on, so a hang that sets him down on or in something shows as a lift.
    fn census_hangs(&mut self) {
        if std::env::var_os("GUAC_HANG_CENSUS").is_none() {
            return;
        }
        let Some(hero) = self.bench.heroes.get(&self.hero) else {
            return;
        };
        let (pivot, hung) = (hero.pivot, hero.hung);
        if hung {
            let from = self.world.actor(self.hero).map_or_else(
                || "?".to_owned(),
                |a| {
                    let owner = self.world.node_owner(a.attach).or(match a.attach {
                        Value::Entity(h) => Some(h),
                        _ => None,
                    });
                    owner.map_or_else(|| format!("{:?}", a.attach), |o| self.world.name(o))
                },
            );
            self.hangs.0 = Some(from);
        } else if let Some(from) = self.hangs.0.take() {
            println!(
                "LET-GO step {} from {from} at ({:.2}, {:.2}, {:.2})",
                self.steps, pivot.x, pivot.y, pivot.z
            );
            self.hangs.1.push((self.steps + 60, from, pivot));
        }
        let now = self.steps;
        self.hangs.1.retain(|(at, from, was)| {
            if *at != now {
                return true;
            }
            println!(
                "LET-GO+60 step {now} from {from} at ({:.2}, {:.2}, {:.2}): {:.2} m on, {:+.2} m up",
                pivot.x,
                pivot.y,
                pivot.z,
                pivot.distance(*was),
                pivot.y - was.y
            );
            false
        });
    }

    /// `GUAC_SURFACE_CENSUS`'s lines (openspec AS.168): each clone standing
    /// for actors the first step it does, by its template's name and where,
    /// and the first step the hero comes within 3 m of its shape's reach.
    fn census_surfaces(&mut self) {
        if std::env::var_os("GUAC_SURFACE_CENSUS").is_none() {
            return;
        }
        let hero = self.bench.heroes.get(&self.hero).map(|h| h.pivot);
        let people: Vec<(Handle, Vec3)> = self
            .world
            .people()
            .into_iter()
            .filter_map(|p| self.world.actor(p).map(|a| (p, a.position)))
            .collect();
        for (h, solid, pose) in self.world.creature_bodies() {
            let Some(a) = self.world.actor(h) else {
                continue;
            };
            if !a.stands_for_actors() {
                continue;
            }
            let name = a
                .template
                .and_then(|t| self.world.template(t))
                .map_or_else(|| "?".to_owned(), |p| p.name.clone());
            let at = pose.position;
            if self.surfaces.0.insert(h) {
                println!(
                    "SURFACE {name} at ({:.2}, {:.2}, {:.2}) step {}",
                    at.x, at.y, at.z, self.steps
                );
            }
            // Within 3 m of its shape's reach from its centre.
            let reach = match solid.shape {
                guac_motion::seam::Shape::Box { half_extents, .. } => half_extents.length(),
                guac_motion::seam::Shape::Sphere { radius, .. } => radius,
                guac_motion::seam::Shape::Capsule {
                    radius,
                    half_segment,
                    ..
                } => radius + half_segment,
                guac_motion::seam::Shape::Cylinder {
                    radius,
                    half_height,
                    ..
                } => radius.hypot(half_height),
            };
            if let Some(p) = hero
                && p.distance(at) < 3.0 + reach
                && self.surfaces.1.insert(h)
            {
                println!(
                    "SURFACE-NEAR {name} at ({:.2}, {:.2}, {:.2}) step {}, the hero at ({:.2}, {:.2}, {:.2})",
                    at.x, at.y, at.z, self.steps, p.x, p.y, p.z
                );
            }
            for &(person, p) in &people {
                if p.distance(at) < 1.0 + reach && self.surfaces.2.insert((h, person)) {
                    println!(
                        "SURFACE-PERSON {name} at ({:.2}, {:.2}, {:.2}) step {}, {} at ({:.2}, {:.2}, {:.2})",
                        at.x,
                        at.y,
                        at.z,
                        self.steps,
                        self.world.name(person),
                        p.x,
                        p.y,
                        p.z
                    );
                }
            }
        }
    }

    /// The join at this step with a fresh figure on the portal, as the TV
    /// joins him ([`Run::join_with`]).
    pub fn join(&mut self) -> Result<(), Halt> {
        self.join_with(Values::default())
    }

    /// The join at this step: his figure's tag on the portal holding
    /// `values` (AH.23), the request now, the activation after the
    /// definition's first tick.
    pub fn join_with(&mut self, values: Values) -> Result<(), Halt> {
        self.join_tag(Tag {
            toy_id: u32::from(self.toy),
            ..eruptor_tag(values)
        })
    }

    /// [`Run::join_with`] with the figure's tag given: a toy's own serial
    /// (openspec AH.105: the portal manager keeps a figure's stored data by
    /// its tag).
    pub fn join_tag(&mut self, tag: Tag) -> Result<(), Halt> {
        let files = self.files.clone();
        let handles = self.handles.clone();
        let layout = self.layout.clone();
        let data = self.with_host(|vm, host| figure::put_on_portal(vm, host, &layout, &tag))?;
        self.figure = Some(data);
        self.with_host(|vm, host| scripts::join_request(vm, host, &files, ERUPTOR, &handles))?;
        self.step()?;
        let hero = self.hero;
        let minion = self.with_host(|vm, host| scripts::join_activate(vm, host, hero))?;
        self.minion = Some(minion);
        Ok(())
    }

    /// The TV's join (openspec AH.224, AS.218): the tag and the request as
    /// [`Run::join_tag`]'s, then his entry beam made after the definition's
    /// first tick ([`scripts::join_enter`]); [`Run::step`] runs it with him
    /// waiting until it activates him ([`Run::entering`]). Returns whether
    /// the dump's part runs a beam.
    pub fn enter(&mut self) -> Result<bool, Halt> {
        let tag = Tag {
            toy_id: u32::from(self.toy),
            ..eruptor_tag(Values::default())
        };
        let files = self.files.clone();
        let handles = self.handles.clone();
        let layout = self.layout.clone();
        let data = self.with_host(|vm, host| figure::put_on_portal(vm, host, &layout, &tag))?;
        self.figure = Some(data);
        self.with_host(|vm, host| scripts::join_request(vm, host, &files, ERUPTOR, &handles))?;
        self.step()?;
        let hero = self.hero;
        let entry = self.with_host(|vm, host| scripts::join_enter(vm, host, hero, true))?;
        Ok(match entry {
            scripts::Entry::Beam => {
                self.entering = Some(hero);
                true
            }
            scripts::Entry::Activated(m) => {
                self.minion = Some(m);
                false
            }
        })
    }

    /// His figure off the portal mid-level (openspec AH.105): the
    /// deactivate path, the run going on; [`Run::join_with`] puts the next
    /// one on, as an actor of his own (AH.191). Strict: a stop in it is the
    /// run's (openspec AH.165: the TV unloads him whatever stopped).
    pub fn deactivate(&mut self) -> Result<scripts::Deactivated, Halt> {
        let files = self.files.clone();
        let hero = self.hero;
        let gone =
            self.with_host(|vm, host| scripts::join_deactivate(vm, host, &files, ERUPTOR, hero));
        let next = self.next;
        self.next += 1;
        self.bench.rekey(hero, next);
        self.hero = next;
        self.minion = None;
        self.entering = None;
        self.figure = None;
        match gone.halted {
            Some(h) => Err(h),
            None => Ok(gone),
        }
    }

    /// The hero set down at `at` (dropped onto what's under it) facing
    /// `facing`, at rest: a scripted hero's teleport.
    pub fn put(&mut self, at: Vec3, facing: f32) {
        let hero = self.bench.heroes.get_mut(&self.hero).unwrap();
        hero.pivot = at + Vec3::Y;
        hero.facing = facing;
        hero.walk.velocity = Vec3::ZERO;
        hero.settle(&self.bench.world, &[HERO_BODY]);
    }

    /// The combat manager's next alert `steps` steps on: `combat control
    /// data`'s `alert next time` set between this step's game timer and the
    /// next's (its check runs before the placements' ticks).
    pub fn alert_in(&mut self, steps: u32) {
        assert!(scripts::alert_in(&mut self.vm, steps), "the combat data");
    }

    /// A level placement set where a recorded state has it, at rest.
    pub fn set_at(&mut self, name: &str, at: Vec3, facing: f32) {
        let (h, _) = self.placed(name);
        assert!(self.world.place(h, at, facing), "{name}");
    }

    /// The hero's health as his scripts hold it (`description.creature
    /// info.hp` on his minion), once he's joined.
    pub fn health(&self) -> Option<f32> {
        let m = self.minion?;
        let d = self.vm.local(m, "description").map(|v| self.vm.value(v))?;
        let c = self
            .vm
            .field(d, "creature info")
            .map(|v| self.vm.value(v))?;
        self.vm.field(c, "hp").map(|v| self.vm.value(v))?.as_f32()
    }

    pub fn hero(&self) -> &Hero {
        &self.bench.heroes[&self.hero]
    }

    pub fn hero_mut(&mut self) -> &mut Hero {
        self.bench
            .heroes
            .get_mut(&self.hero)
            .expect("his controller")
    }

    /// His figure's values as its tag's words hold them, once he's joined.
    pub fn figure_values(&self) -> Option<Values> {
        figure::read(&self.vm, &self.layout, self.figure?)
    }

    /// A level placement's handle and where its file puts it, metres.
    pub fn placed(&self, name: &str) -> (Handle, Vec3) {
        let p = self
            .files
            .placed
            .iter()
            .find(|p| p.name == name && p.obj.unit == LEVEL)
            .unwrap_or_else(|| panic!("no {name}"));
        (self.handles[&p.obj], Vec3::from_array(p.position))
    }

    pub fn placement(&self, name: &str) -> PlacementId {
        let (h, _) = self.placed(name);
        self.vm
            .placement_of(h)
            .unwrap_or_else(|| panic!("{name} isn't run"))
    }

    /// Where the engine has the actor now, metres (its file's place until
    /// something moves it).
    pub fn at(&self, name: &str) -> Vec3 {
        let (h, _) = self.placed(name);
        self.world.actor(h).unwrap().position
    }
}

/// A level's collision meshes, each a static body, and where the hero
/// starts (openspec AS.51: any level by its archive).
pub fn level_bodies(level: &Igz<'_>, name: &str) -> (Vec<Body>, Vec3) {
    let placements = levels::placements(level, name, identity())
        .expect("the placements")
        .ir;
    let collision = levels::collision(level, name, identity())
        .expect("the collision")
        .ir;
    let start = level_scene::start_point(&placements).expect("the start");
    let bodies = collision
        .meshes
        .iter()
        .enumerate()
        .map(|(i, m)| Body {
            id: BodyId(level_scene::LEVEL_BODIES + i as u32),
            kind: BodyKind::Static,
            pose: Pose::IDENTITY,
            collider: Collider::Mesh(Arc::new(
                Mesh::new(
                    m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                    m.triangles.iter().map(|t| t.map(u32::from)).collect(),
                    m.materials.clone(),
                )
                .unwrap(),
            )),
        })
        .collect();
    (bodies, start)
}

/// A level's props through the extractor's own readers, as the TV's load
/// builds them.
pub fn level_props_of(level: &Igz<'_>, name: &str) -> LevelProps {
    let props = levels::props(level, name, identity())
        .expect("the props")
        .ir;
    let placements = levels::placements(level, name, identity())
        .expect("the placements")
        .ir;
    LevelProps::new(&props, &placements).expect("the props")
}

/// The level's collision mesh and where the hero starts.
pub fn chapter_1(level: &Igz<'_>) -> (Body, Vec3) {
    let l = levels::LEVELS[0];
    let placements = levels::placements(level, l, identity())
        .expect("the placements")
        .ir;
    let collision = levels::collision(level, l, identity())
        .expect("the collision")
        .ir;
    let start = level_scene::start_point(&placements).expect("the start");
    let m = &collision.meshes[0];
    let mesh = Body {
        id: BodyId(level_scene::LEVEL_BODIES),
        kind: BodyKind::Static,
        pose: Pose::IDENTITY,
        collider: Collider::Mesh(Arc::new(
            Mesh::new(
                m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                m.triangles.iter().map(|t| t.map(u32::from)).collect(),
                m.materials.clone(),
            )
            .unwrap(),
        )),
    };
    (mesh, start)
}

/// Chapter 1's props through the extractor's own readers, as the TV's load
/// builds them.
pub fn level_props(level: &Igz<'_>) -> LevelProps {
    let l = levels::LEVELS[0];
    let props = levels::props(level, l, identity()).expect("the props").ir;
    let placements = levels::placements(level, l, identity())
        .expect("the placements")
        .ir;
    LevelProps::new(&props, &placements).expect("the props")
}

/// The skinned models the TV draws chapter 1's first fight and the clones
/// of its skinned templates with (openspec AG.150,
/// `creature_models::skinned_models`), by path, through the extractor's own
/// readers, as the TV's load builds them.
pub fn skinned_models() -> Vec<(String, guac_ir::Model)> {
    let bytes = level_bytes();
    let level = Igz::parse(&bytes).unwrap();
    let l = levels::LEVELS[0];
    let props = levels::props(&level, l, identity()).expect("the props").ir;
    let placements = levels::placements(&level, l, identity())
        .expect("the placements")
        .ir;
    guacamole::creature_models::skinned_models(&placements, &props, &identity())
}

/// [`skinned_models`] for the level whose archive is `level` (openspec
/// AS.63: any level's skinned templates' clones).
pub fn skinned_models_of(level: &str) -> Vec<(String, guac_ir::Model)> {
    let usr = dump().join("PS3_GAME/USRDIR");
    let bytes = guac_extract::scripts::entry(&usr, &format!("level/{level}.bld")).unwrap();
    let igz = Igz::parse(&bytes).unwrap();
    let props = levels::props(&igz, level, identity())
        .expect("the props")
        .ir;
    let placements = levels::placements(&igz, level, identity())
        .expect("the placements")
        .ir;
    guacamole::creature_models::skinned_models(&placements, &props, &identity())
}

/// Whether the TV draws the clones of the level placement `name` (openspec
/// AS.63's rule, `props::clones_draw`): its static model among the props'
/// spawn models (`run` stands the level's props, `Setup::props`), or its
/// skinned one among `skinned`, the creature models' paths, unless
/// `props::UNDRAWN_SPAWNS` names it (the TV's `clone_parts`).
pub fn clones_drawn(run: &Run, skinned: &[String], name: &str) -> bool {
    let props = run.bench.props.as_ref().expect("the props stood");
    !guacamole::props::UNDRAWN_SPAWNS.contains(&name)
        && run.files.placed.iter().any(|p| {
            p.obj.unit == LEVEL
                && p.name == name
                && p.model.as_ref().is_some_and(|m| {
                    props
                        .level
                        .spawn_models
                        .iter()
                        .any(|&i| &props.level.models[i].path == m)
                        || skinned.contains(m)
                })
        })
}

/// Chapter 1's palettes part through the extractor's own reader: what the
/// TV poses its creatures and skinned clones by.
pub fn level_palettes() -> guac_ir::palette::LevelPalettes {
    let bytes = level_bytes();
    let level = Igz::parse(&bytes).unwrap();
    levels::palettes(&level, levels::LEVELS[0], identity())
        .expect("the palettes")
        .ir
}

/// Chapter 1's `level.bld` entry off the dump.
fn level_bytes() -> Vec<u8> {
    let usr = dump().join("PS3_GAME/USRDIR");
    guac_extract::scripts::entry(&usr, ARCHIVES[LEVEL as usize].0).unwrap()
}

/// Each slot's clip length at 100 %, 0 where a slot is empty or a network.
pub fn lengths(p: &guac_ir::palette::Palette) -> Vec<f32> {
    let mut out = vec![0.0; usize::from(p.slots)];
    for e in &p.entries {
        if let guac_ir::palette::SlotEntry::Clip { clip, .. } = &e.entry {
            out[usize::from(e.slot)] = clip.duration;
        }
    }
    out
}

/// Eruptor's rig read from his archive with the globals his model's
/// shaders need: his model, his palette and his attach nodes, at his scale
/// in the level (AH.50), his random networks laid over his palette as the
/// TV reads it (openspec AS.196: his hurt).
pub fn eruptor_rig(part: &[Vec<u8>]) -> guacamole::hands::Rig {
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).expect("the globals");
    let read = guac_extract::models::read(&part[3], "009_Eruptor", identity(), Some(&globals))
        .expect("Eruptor's model");
    guacamole::hands::Rig {
        model: read.model,
        palette: read.palette.with_networks(&read.networks),
        nodes: read.nodes,
        scale: guac_motion::tuning::ERUPTOR_SCALE,
    }
}

/// A character's rig off his file ([`Setup::character`]), as Eruptor's is.
pub fn character_rig(part: &[Vec<u8>], load_file: &str) -> guacamole::hands::Rig {
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).expect("the globals");
    let read = guac_extract::models::read(&part[3], load_file, identity(), Some(&globals))
        .expect("his model");
    guacamole::hands::Rig {
        model: read.model,
        palette: read.palette.with_networks(&read.networks),
        nodes: read.nodes,
        scale: guac_motion::tuning::ERUPTOR_SCALE,
    }
}

/// Each live-table member the registry doesn't hold, as a value cell of its
/// registered type, by name: the discovery aid.
pub fn discovery(config: &mut guac_script::Config) -> HashMap<MemberId, String> {
    let Ok(path) = std::env::var("AH18_LIVE_TABLE") else {
        return HashMap::new();
    };
    let table = std::fs::read_to_string(path).expect("the live table");
    let mut names = HashMap::new();
    for row in table.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = row.split('\t').collect();
        let ns = c[0].to_ascii_lowercase();
        let ns = ns.strip_prefix("tfb").unwrap_or(&ns).to_owned();
        let name = c[2].to_ascii_lowercase();
        let name = name.trim_start_matches('-').to_owned();
        let id = MemberId::named(&ns, &name);
        if config.members.get(id).is_some() || guac_script::Builtin::of(id).is_some() {
            continue;
        }
        let min: f32 = c.get(8).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let max: f32 = c.get(9).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let kind = match c[3] {
            "FloatMeasurement" | "PairedFloatMeasurement" => Kind::Float,
            "IntMeasurement" | "EnumMeasurement" | "PairedEnumMeasurement" => Kind::Int,
            "ColorMeasurement" => Kind::Color,
            "ScreenMeasurement" => Kind::Screen,
            _ => Kind::Reference,
        };
        config
            .members
            .value_cell(&ns, &name, MemberSpec::new(kind, min, max));
        names.insert(id, format!("{ns}.{name} [{}]", c[3]));
    }
    names
}

/// The run over the dump: the init chain's VM, the host's world over every
/// file, the hero at the level's start on its collision, the fight's
/// creatures walkers; the join is [`Run::join`]'s.
/// What a headless run holds besides the level's collision and the hero.
#[derive(Debug, Clone, Copy, Default)]
pub struct Setup {
    /// The level's props in the seam by AG.25's rule (openspec AH.21).
    pub props: bool,
    /// The sounds part over the dump ([`sound_part`]): each start of a
    /// sound plays it on the game clock, as the TV's run does (openspec
    /// AR.9); without it a sound plays nothing, as before AR.9.
    pub sounds: bool,
    /// Chapter 1's music scripts taken off the run (`scripts::without_music`,
    /// openspec AR.16): a probe's way to set their draws apart; the TV's run
    /// lists them.
    pub no_music: bool,
    /// The interface's model over the dump ([`interface_part`]): the run's
    /// config registers its members and the world holds it, as the TV's run
    /// does once the extract holds the part (openspec AT.9); without it the
    /// members are cells, as before AT.9.
    pub interface: bool,
    /// Scripts listed past the TV's listing, by file name (a probe's way to
    /// run what the TV doesn't yet).
    pub scripts: &'static [&'static str],
    /// Placements listed past the TV's listing, by name (the same, for a
    /// script others share that only these should run).
    pub placements: &'static [&'static str],
    /// A hub stage's ship part streams linked after the level, their
    /// scripts listed, as the TV's load links them (openspec AS.157,
    /// `ship_parts::streams`); they join the run once its save is in.
    pub streams: bool,
    /// Streams left out of those, by archive: as one whose parts didn't
    /// extract is.
    pub streams_left_out: &'static [&'static str],
    /// Another character in Eruptor's place (openspec AS.156: Crusher, the
    /// earth Giant whose attacks break a super hard rock): his file linked
    /// as Eruptor's unit, his scripts listed as the TV lists a walker's
    /// (`scripts::with_walkers`), his figure's toy on the portal. His body
    /// in the seam stays Eruptor's controller's.
    pub character: Option<Character>,
    /// The wake rule in the TV's place (openspec AS.246: a probe's way to
    /// run a level on the original's).
    pub wake: Wake,
}

/// Which sleepers a run wakes with their declared locals made anew
/// ([`Setup::wake`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Wake {
    /// The TV's (`scripts::ANEW_ON_WAKE`).
    #[default]
    Tv,
    /// The original's: every sleeper's (`AnewOnWake::Every`).
    Every,
    /// None's: every sleeper keeps them.
    Kept,
}

/// A character a run joins in Eruptor's place ([`Setup::character`]).
#[derive(Debug, Clone, Copy)]
pub struct Character {
    /// His file under the dump's `USRDIR`, e.g. `character/102_titan_earth.bld`.
    pub archive: &'static str,
    /// His load file, as the roster names it, e.g. `102_Titan_Earth`.
    pub load_file: &'static str,
    /// His toy id.
    pub toy: u16,
}

/// Crusher, the earth Giant.
pub const CRUSHER: Character = Character {
    archive: "character/102_titan_earth.bld",
    load_file: "102_Titan_Earth",
    toy: 102,
};

/// Tree Rex, the life Giant (openspec AS.183: the life area's Giant wall).
pub const TREE_REX: Character = Character {
    archive: "character/112_titanlife.bld",
    load_file: "112_TitanLife",
    toy: 112,
};

/// The interface records of the script part's archives (`ARCHIVES`, by
/// unit), read off the dump in memory by guac-extract's own reader, once a
/// test binary (openspec AT.9): what the TV's run reads off the extract's
/// interface part and the level's sprites part.
pub fn interface_records() -> Vec<guac_ir::interface::Interface> {
    static RECORDS: std::sync::OnceLock<Vec<guac_ir::interface::Interface>> =
        std::sync::OnceLock::new();
    RECORDS
        .get_or_init(|| {
            let usr = dump().join("PS3_GAME/USRDIR");
            ARCHIVES
                .iter()
                .map(|(archive, _)| {
                    let bld = std::fs::read(usr.join(archive))
                        .unwrap_or_else(|e| panic!("{archive}: {e}"));
                    let arc = std::fs::read(usr.join(archive).with_extension("arc")).ok();
                    guac_extract::interface::read(archive, &bld, arc.as_deref(), identity())
                        .unwrap_or_else(|e| panic!("{archive}'s interface: {e}"))
                        .ir
                })
                .collect()
        })
        .clone()
}

/// The interface records of a part linking `archives` (by unit), read off
/// the dump in memory, uncached: a level past chapter 1's (openspec AS.51).
pub fn interface_records_of(archives: &[String]) -> Vec<guac_ir::interface::Interface> {
    let usr = dump().join("PS3_GAME/USRDIR");
    archives
        .iter()
        .map(|archive| {
            let bld = std::fs::read(usr.join(archive)).unwrap_or_else(|e| panic!("{archive}: {e}"));
            let arc = std::fs::read(usr.join(archive).with_extension("arc")).ok();
            guac_extract::interface::read(archive, &bld, arc.as_deref(), identity())
                .unwrap_or_else(|e| panic!("{archive}'s interface: {e}"))
                .ir
        })
        .collect()
}

/// The interface's model over the script part's files (`igzs`, by unit) and
/// [`interface_records`], as the TV's install loads it.
pub fn interface_part(igzs: &[Igz<'_>]) -> guac_interface::load::Loaded {
    interface_part_with(igzs, interface_records())
}

/// [`interface_part`] over `records`.
pub fn interface_part_with(
    igzs: &[Igz<'_>],
    records: Vec<guac_ir::interface::Interface>,
) -> guac_interface::load::Loaded {
    let keys: Vec<guac_interface::load::Keys> = igzs
        .iter()
        .enumerate()
        .map(|(u, igz)| guac_interface::load::Keys::read(u as u16, igz))
        .collect();
    let imports = guac_interface::load::Imports {
        exports: guac_formats::exports::Exports::new(&igzs[0], &igzs[1]).unwrap(),
        bootstrap: 0,
        global: 1,
    };
    let pairs: Vec<_> = keys.iter().zip(&records).collect();
    guac_interface::load::load(&pairs, Some(&imports))
}

/// The sounds part of `global.bld`, Eruptor's archive and chapter 1, read
/// off the dump in memory by guac-extract's own reader (nothing written),
/// once a test binary, by unit (openspec AR.9): what the TV's run reads off
/// the extract's sounds part.
pub fn sound_part() -> (Arc<guac_sound::SoundBook>, HashMap<u16, String>) {
    static PART: std::sync::OnceLock<(Arc<guac_sound::SoundBook>, HashMap<u16, String>)> =
        std::sync::OnceLock::new();
    PART.get_or_init(|| {
        let usr = dump().join("PS3_GAME/USRDIR");
        let exports = guac_extract::sounds::exports(&usr).expect("global's exports");
        let mut stems = HashMap::new();
        let mut parts = Vec::new();
        for unit in [1u16, ERUPTOR, LEVEL] {
            let archive = ARCHIVES[unit as usize].0;
            let bld = std::fs::read(usr.join(archive)).unwrap_or_else(|e| panic!("{archive}: {e}"));
            let arc = std::fs::read(usr.join(archive).with_extension("arc")).ok();
            let read =
                guac_extract::sounds::read(archive, &bld, arc.as_deref(), &exports, identity())
                    .unwrap_or_else(|e| panic!("{archive}'s sounds: {e}"));
            stems.insert(unit, guac_sound::stem(archive));
            parts.push(read.ir);
        }
        (Arc::new(guac_sound::SoundBook::new(parts)), stems)
    })
    .clone()
}

/// The sounds part of a part linking `archives` (by unit): `global.bld`'s,
/// Eruptor's and the level's, read off the dump in memory, uncached: a level
/// past chapter 1's, whose own sounds the TV reads off its level's record
/// (openspec AR.30).
pub fn sound_part_of(archives: &[String]) -> (Arc<guac_sound::SoundBook>, HashMap<u16, String>) {
    let usr = dump().join("PS3_GAME/USRDIR");
    let exports = guac_extract::sounds::exports(&usr).expect("global's exports");
    let mut stems = HashMap::new();
    let mut parts = Vec::new();
    for unit in [1u16, ERUPTOR, LEVEL] {
        let archive = &archives[unit as usize];
        let bld = std::fs::read(usr.join(archive)).unwrap_or_else(|e| panic!("{archive}: {e}"));
        let arc = std::fs::read(usr.join(archive).with_extension("arc")).ok();
        let read = guac_extract::sounds::read(archive, &bld, arc.as_deref(), &exports, identity())
            .unwrap_or_else(|e| panic!("{archive}'s sounds: {e}"));
        stems.insert(unit, guac_sound::stem(archive));
        parts.push(read.ir);
    }
    (Arc::new(guac_sound::SoundBook::new(parts)), stems)
}

pub fn run_on(mode: Mode) -> (Run, HashMap<MemberId, String>) {
    run_on_with(mode, Setup::default())
}

pub fn run_on_with(mode: Mode, setup: Setup) -> (Run, HashMap<MemberId, String>) {
    run_on_level(mode, levels::LEVELS[0], setup)
}

/// [`run_on_with`] over the level whose archive is `level` (openspec AS.51):
/// its file linked last, the TV's listing for it (`scripts::level_config`),
/// the hero at its start on its collision; chapter 1's first fight and goats
/// walk on chapter 1 alone.
pub fn run_on_level(mode: Mode, level: &str, setup: Setup) -> (Run, HashMap<MemberId, String>) {
    let dump = dump();
    let usr = dump.join("PS3_GAME/USRDIR");
    let chapter_1 = level == levels::LEVELS[0];
    let level_archive = format!("level/{level}.bld");
    let on_disc: Vec<String> = std::fs::read_dir(usr.join("level"))
        .expect("the dump's levels")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    // A hub stage's ship part streams after the level (openspec AS.157).
    let streams: Vec<String> = match setup.streams {
        true => guacamole::ship_parts::streams(level, on_disc.iter().map(String::as_str)),
        false => Vec::new(),
    }
    .into_iter()
    .filter(|s| !setup.streams_left_out.contains(&s.as_str()))
    .collect();
    let archives: Vec<String> = ARCHIVES
        .iter()
        .enumerate()
        .map(|(u, (a, _))| match u as u16 {
            LEVEL => level_archive.clone(),
            ERUPTOR => setup.character.map_or(*a, |c| c.archive).to_owned(),
            _ => (*a).to_owned(),
        })
        .chain(streams.iter().map(|s| format!("level/{s}.bld")))
        .collect();
    let stream_units: Vec<u16> = (ARCHIVES.len()..archives.len()).map(|u| u as u16).collect();
    let roles: Vec<Role> = ARCHIVES
        .iter()
        .map(|(_, r)| *r)
        .chain(stream_units.iter().map(|_| Role::Other))
        .collect();
    let part: Vec<Vec<u8>> = archives
        .iter()
        .map(|a| guac_extract::scripts::entry(&usr, a).unwrap_or_else(|e| panic!("{a}: {e}")))
        .collect();
    let igzs: Vec<Igz<'_>> = part.iter().map(|b| Igz::parse(b).unwrap()).collect();
    let graphs: Vec<Graph<'_>> = igzs.iter().map(|i| Graph::read(i).unwrap()).collect();
    let units: Vec<Unit<'_, '_>> = igzs
        .iter()
        .zip(&graphs)
        .zip(&roles)
        .map(|((igz, graph), role)| Unit {
            name: "part",
            role: *role,
            igz,
            graph,
        })
        .collect();
    let program = Arc::new(Program::link(&units).unwrap());
    let player_loop = PlayerLoop::read(&program);
    let layout = layout();
    let read: Vec<(u16, &Igz<'_>)> = [1u16, 2, LEVEL, ERUPTOR]
        .iter()
        .chain(&stream_units)
        .map(|&u| (u, &igzs[u as usize]))
        .collect();
    let files = Files::read(&read).unwrap();
    let handles = headless_handles(&files);
    let objects = headless_objects(&handles);
    // Its cutscene objects' definitions, as the TV's install lists them
    // (openspec AS.82).
    let mut config = scripts::with_cutscene_objects(
        scripts::level_config(mode, &files, LEVEL, level),
        &program,
        &files,
        LEVEL,
    );
    config = guacamole::ship_parts::with_streams(config, &files, &stream_units);
    if setup.no_music {
        config = scripts::without_music(config);
    }
    if setup.interface {
        config = guacamole::interface_host::with_interface(config);
    }
    if let guac_script::Whitelist::Only {
        scripts,
        placements,
        ..
    } = &mut config.whitelist
    {
        scripts.extend(setup.scripts.iter().map(|s| (*s).to_owned()));
        placements.extend(setup.placements.iter().map(|s| (*s).to_owned()));
    }
    let walker = setup.character.map(|c| {
        scripts::walker_scripts(&program, &files, &[(c.load_file.to_owned(), ERUPTOR)])
            .pop()
            .expect("his scripts")
    });
    if let Some(w) = &walker {
        config = scripts::with_walkers(config, std::slice::from_ref(w));
    }
    match setup.wake {
        Wake::Tv => {}
        Wake::Every => config.anew_on_wake = guac_script::AnewOnWake::Every,
        Wake::Kept => config.anew_on_wake = guac_script::AnewOnWake::Listed(Vec::new()),
    }
    let found = discovery(&mut config);
    let mut vm = scripts::init_chain(program, &files, &handles, &ORDER, config);
    let mut world = ScriptWorld::new(&files, &handles, objects);
    world.set_inert(vm.inert());
    world.ship_parts.units = stream_units;
    world.ship_parts.ids = streams
        .iter()
        .filter_map(|s| guacamole::ship_parts::part_id(s))
        .collect();
    // Its intro waits for the hero, as the TV's install holds it (openspec
    // AS.47); one a test lists past the TV's listing (`Setup::scripts`) too.
    let intro: Option<String> = scripts::intro(level).map(str::to_owned).or_else(|| {
        files
            .placed
            .iter()
            .find(|p| {
                p.obj.unit == LEVEL
                    && p.script.as_deref().is_some_and(|s| {
                        s.ends_with("_Intro_IGC.ai") && setup.scripts.iter().any(|l| s.ends_with(l))
                    })
            })
            .map(|p| p.name.clone())
    });
    if let Some(intro) = intro {
        scripts::hold_intro(&mut vm, &mut world, &files, LEVEL, &intro);
    }
    if setup.sounds {
        let (book, stems) = match chapter_1 {
            true => sound_part(),
            false => sound_part_of(&archives),
        };
        world.set_sounds(book, stems);
    }
    if setup.interface {
        let records = match chapter_1 {
            true => interface_records(),
            false => interface_records_of(&archives[..ARCHIVES.len()]),
        };
        world.set_interface(interface_part_with(&igzs, records).keeper);
    }
    world.set_levels(Arc::new(on_disc.into_iter().collect()));
    world.set_level(level);
    let template = match &walker {
        Some(w) => w.template.expect("his template"),
        None => {
            files
                .placed
                .iter()
                .find(|p| p.name == "009_Eruptor")
                .expect("Eruptor's template")
                .obj
        }
    };
    let rig = match setup.character {
        Some(c) => character_rig(&part, c.load_file),
        None => eruptor_rig(&part),
    };
    world.set_palette_of(template, &rig.palette);
    // The creatures' palettes, by the model each placement names.
    let palettes = levels::palettes(&igzs[LEVEL as usize], level, identity())
        .expect("the level's palettes")
        .ir;
    for p in files.placed.iter().filter(|p| p.obj.unit == LEVEL) {
        if let Some(model) = &p.model
            && let Some(pal) = palettes.palettes.iter().find(|x| &x.owner == model)
        {
            world.set_palette_of(p.obj, pal);
        }
    }
    // The streams' templates' too (openspec AS.157: the engines turn).
    for (&u, name) in world.ship_parts.units.clone().iter().zip(&streams) {
        let palettes = levels::palettes(&igzs[u as usize], name, identity())
            .expect("a stream's palettes")
            .ir;
        for p in files.placed.iter().filter(|p| p.obj.unit == u) {
            if let Some(model) = &p.model
                && let Some(pal) = palettes.palettes.iter().find(|x| &x.owner == model)
            {
                world.set_palette_of(p.obj, pal);
            }
        }
    }
    if chapter_1 {
        world.walk_first_fight(LEVEL);
        world.walk_goats(LEVEL);
    }
    for e in scripts::encounters(level) {
        world.walk_encounter(LEVEL, e);
    }
    let (meshes, start) = level_bodies(&igzs[LEVEL as usize], level);
    let parry = ParryWorld::new(&meshes).expect("the level's collision");
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, start, 0.0);
    hero.settle(&parry, &[]);
    let props = setup.props.then(|| {
        let level = level_props_of(&igzs[LEVEL as usize], level);
        let by_object = level
            .props
            .iter()
            .map(|p| {
                let obj = Obj {
                    unit: LEVEL,
                    index: p.object,
                };
                (p.object, handles[&obj])
            })
            .collect();
        StandingProps {
            level,
            handles: by_object,
            standing: HashSet::new(),
            holds: HashMap::new(),
        }
    });
    let mut bench = Bench {
        heroes: HashMap::from([(HERO, hero)]),
        world: parry,
        view: 15.0,
        hold_view: true,
        pads: [Pad::default(); 2],
        bodies: HashMap::new(),
        ids: HashMap::new(),
        props,
    };
    // The activity rule from where he stands, as the first step's will
    // (the VM's sleep is that step's to set): a prop stands by its actor.
    world.activity_at(&[bench.heroes[&HERO].pivot]);
    bench.stand_props(&world);
    let next = objects[6].max(HERO) + 1;
    (
        Run {
            vm,
            world,
            bench,
            generator: Generator::seeded(5489),
            files,
            handles,
            next,
            hero: HERO,
            minion: None,
            steps: 0,
            player_loop,
            layout,
            figure: None,
            rig,
            idle0: 0.0,
            lean: None,
            surfaces: Default::default(),
            hangs: Default::default(),
            entering: None,
            toy: setup.character.map_or(9, |c| c.toy),
        },
        found,
    )
}

/// AH.2's progression tables off the disc, and the figure's layout from
/// their rows.
pub fn progression() -> guac_ir::progression::Progression {
    let usr = dump().join("PS3_GAME/USRDIR");
    guac_extract::progression::read(&usr, identity()).expect("the progression tables")
}

pub fn layout() -> Layout {
    Layout::of(&progression()).expect("the figure's layout")
}

pub fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The discovery aid's report: the value-cell members the run reached and
/// how often each was read and written.
pub fn report(run: &Run, found: &HashMap<MemberId, String>) {
    let mut rows: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for (m, (r, w)) in run.vm.uses() {
        if let Some(n) = found.get(m) {
            rows.insert(n.clone(), (*r, *w));
        }
    }
    println!("unregistered members reached: {}", rows.len());
    for (n, (r, w)) in rows {
        println!("  {n}: reads {r} writes {w}");
    }
}

/// The run joined and settled at the spawn: the init chain's two steps,
/// the join, then 120 steps for his spawn sequence to play out.
pub fn joined(mode: Mode) -> (Run, HashMap<MemberId, String>) {
    joined_with(mode, Setup::default())
}

/// [`joined`] over a run set up as `setup` says.
pub fn joined_with(mode: Mode, setup: Setup) -> (Run, HashMap<MemberId, String>) {
    let (mut run, found) = run_on_with(mode, setup);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    (run, found)
}

/// The TV's half of AH.1's path, the store's half on an in-memory
/// database of guacamole's set: a profile's fresh row, `sent` handed over
/// under its generation and drained; the row after.
pub fn row_after(sent: &[Values]) -> FigureProgress {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let store = ProfileStore::open_in_memory(&guac_server::setup::MIGRATOR)
            .await
            .unwrap();
        let bob = store.create("Bob", "1234", "#457b9d").await.unwrap();
        let fresh = store.instances().fresh(&bob, ERUPTOR_FIGURE).await.unwrap();
        let (tx, _events) = tokio::sync::broadcast::channel(256);
        let sink = ProgressSink::new();
        let slot = ProgressionSlot::default();
        let p = progression();
        slot.publish((&p).into());
        let mut drainer = Drainer::new(sink.clone(), store.clone(), tx, slot);
        let figure = FigureId::new(ERUPTOR_FIGURE);
        for v in sent {
            sink.send(
                &bob,
                &figure,
                FigureProgress {
                    xp: v.xp,
                    gold: v.gold,
                    playtime_secs: v.playtime_secs,
                    generation: fresh.progress.generation,
                },
            );
        }
        drainer.drain().await;
        store
            .instances()
            .read(&bob, ERUPTOR_FIGURE)
            .await
            .unwrap()
            .unwrap()
            .progress
    })
}

/// One dialogue box a scene's line made on the interface's model (openspec
/// AT.44): the step it was made and taken off, its speaker's portrait
/// (the template its portrait is a clone of) and how many letters its text
/// box's line holds as the scripts set it.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogueBox {
    pub made: u32,
    pub gone: Option<u32>,
    pub portrait: Option<String>,
    pub letters: usize,
}

/// One voice a line started (openspec AR.17): its sound, its actor's name,
/// the step it started and ended, its length on the game clock (the
/// recording's frames) and the length the build measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub sound: String,
    pub actor: String,
    pub started: u64,
    pub ended: Option<u64>,
    pub length: f32,
    pub built: f32,
}

/// The template a dialogue box's backdrop is a clone of: the box's
/// container holds it, the portrait and the text box hang on it.
pub const DIALOGUE_BACKDROP: &str = "UI_Dialogue_FullBackdrop";

/// What a run's scene lines did, step by step: each dialogue box made and
/// taken off on the model, each voice (a `vo_` sound) started.
#[derive(Debug, Clone, Default)]
pub struct Lines {
    pub boxes: Vec<DialogueBox>,
    pub voices: Vec<Voice>,
    open: Vec<(guac_interface::SpriteId, usize)>,
}

impl Lines {
    /// What changed since the last step: boxes made or taken off, the
    /// letters a box's line holds, voices started and ended.
    pub fn record(&mut self, run: &Run) {
        let step = run.steps;
        if let Some(k) = run.world.interface() {
            let m = k.model();
            let backdrops: Vec<guac_interface::SpriteId> = k
                .instances()
                .into_iter()
                .filter(|&id| m.sprite(id).name == DIALOGUE_BACKDROP)
                .collect();
            for &id in &backdrops {
                let sprite = m.sprite(id);
                let portrait = sprite.children.iter().find_map(|&c| {
                    let s = m.sprite(c);
                    (!s.name.contains("Text")).then(|| s.name.clone())
                });
                let letters = sprite
                    .children
                    .iter()
                    .map(|&c| m.letter_count(c, &guac_interface::NoWorld))
                    .max()
                    .unwrap_or(0);
                match self.open.iter().find(|(o, _)| *o == id) {
                    Some(&(_, at)) => {
                        let b = &mut self.boxes[at];
                        b.letters = b.letters.max(letters);
                        b.portrait = b.portrait.take().or(portrait);
                    }
                    None => {
                        self.open.push((id, self.boxes.len()));
                        self.boxes.push(DialogueBox {
                            made: step,
                            gone: None,
                            portrait,
                            letters,
                        });
                    }
                }
            }
            self.open.retain(|&(id, at)| {
                let alive = backdrops.contains(&id);
                if !alive {
                    self.boxes[at].gone = Some(step);
                }
                alive
            });
        }
        let w = &run.world;
        for p in w.plays() {
            let Some(name) = p.name.as_deref().filter(|n| n.starts_with("vo_")) else {
                continue;
            };
            if self
                .voices
                .iter()
                .any(|v| v.sound == name && v.started == p.started)
            {
                continue;
            }
            let s = &w.sequences()[p.sequence as usize];
            self.voices.push(Voice {
                sound: name.to_owned(),
                actor: w.name(s.actor).to_owned(),
                started: p.started,
                ended: None,
                length: p.length,
                built: p.params.rumble_duration,
            });
        }
        for e in w.sounds_ended() {
            if let Some(v) = self
                .voices
                .iter_mut()
                .find(|v| Some(v.sound.as_str()) == e.name.as_deref() && v.started == e.started)
            {
                v.ended = Some(e.ended);
            }
        }
    }
}

/// The cutscene in play's macro's name, if any (as `level_end` reads it).
pub fn cutscene_name(run: &Run) -> Option<String> {
    let vm = &run.vm;
    let data = vm.global_peek(vm.program().global_named("cs control data")?)?;
    let current = vm.field(data, "current cs").filter(|v| *v != Value::Null)?;
    let def = vm.field(current, "cutscene definition")?;
    match vm.field(def, "define macro")? {
        Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
        _ => None,
    }
}

/// The scene of the cutscene in play, as its definition lists them: its
/// index (none outside a cutscene) and how many there are.
pub fn scene_of(run: &Run) -> Option<(usize, usize)> {
    let vm = &run.vm;
    let data = vm.global_peek(vm.program().global_named("cs control data")?)?;
    let current = vm.field(data, "current cs").filter(|v| *v != Value::Null)?;
    let scenes = vm.set(vm.field(vm.field(current, "cutscene definition")?, "scenes")?)?;
    let this = vm.field(vm.field(current, "cur scene data")?, "scene definition")?;
    Some((scenes.iter().position(|s| *s == this)?, scenes.len()))
}

/// The actor each object of the scene in play reads (its `obj data`'s
/// `actor`), by name: `None` for one reading nothing (openspec AS.82: a
/// cutscene object no definition has filled).
pub fn scene_actors(run: &Run) -> Vec<Option<String>> {
    let vm = &run.vm;
    let Some(data) = vm
        .program()
        .global_named("cs control data")
        .and_then(|g| vm.global_peek(g))
    else {
        return Vec::new();
    };
    let objects = vm
        .field(data, "current cs")
        .filter(|v| *v != Value::Null)
        .and_then(|cs| vm.field(cs, "cur scene data"))
        .and_then(|scene| vm.field(scene, "scene objects"))
        .and_then(|set| vm.set(set).map(<[Value]>::to_vec))
        .unwrap_or_default();
    objects
        .into_iter()
        .map(
            |o| match vm.field(o, "obj data").and_then(|d| vm.field(d, "actor")) {
                Some(Value::Entity(h)) => Some(run.world.name(h).to_owned()),
                Some(Value::Object(obj)) => vm.program().name_of(obj).map(str::to_owned),
                _ => None,
            },
        )
        .collect()
}

/// Level 214's intro skipped as a player skips it (openspec AS.81): △
/// pulsed every 10 steps while its cutscene plays (the cutscene system
/// takes it past the cutscene's first second), until the intro takes
/// itself off and hands the hero over; what stopped on the way. For the
/// tests past the intro, which the TV skipped a frame in until AS.81.
pub fn skip_214s_intro(run: &mut Run) -> Vec<Halt> {
    let intro = run.placement("214_Intro_IGC");
    let pad = |down: bool| guacamole::script_engine::Pad {
        stick: guac_motion::stick::Stick::new(0.0, 0.0),
        actions: [false, false, false, down],
        connected: true,
    };
    let mut stops = Vec::new();
    for k in 0..600u32 {
        if run.vm.removed(intro) {
            break;
        }
        run.bench.pads[0] = pad(cutscene_name(run).is_some() && k % 10 == 0);
        stops.extend(run.step_play().unwrap());
    }
    run.bench.pads[0] = pad(false);
    assert!(
        run.vm.removed(intro),
        "the intro skipped by step {}",
        run.steps
    );
    stops
}

/// What stepped while the game's pause let the level rest (openspec
/// AS.205): the awake creatures and clones (the level's enemies, what they
/// threw, the heroes) whose scripts ticked, that moved or whose clips went
/// on over the window, and of those with scripts how many stood awake and
/// how many rested.
#[derive(Debug, Default)]
pub struct UnderPause {
    pub stepped: Vec<String>,
    pub awake: usize,
    pub resting: usize,
}

/// `steps` steps of `step` measured as [`UnderPause`] says.
pub fn under_pause(run: &mut Run, steps: u32, mut step: impl FnMut(&mut Run)) -> UnderPause {
    let actors = |run: &Run| -> Vec<Handle> {
        let mut v: Vec<Handle> = run
            .world
            .creatures()
            .into_iter()
            .chain(run.world.clones())
            .filter(|h| run.world.actor(*h).is_some_and(|a| a.active && !a.removed))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let state = |run: &Run, h: Handle| {
        let clock = run
            .vm
            .placement_of(h)
            .and_then(|id| run.vm.active_seconds(id));
        let at = run.world.actor(h).map(|a| a.position);
        let clips: Vec<f32> = run
            .world
            .sequences()
            .iter()
            .filter(|s| s.actor == h && !s.done && !s.stopped)
            .map(|s| s.time)
            .collect();
        (clock, at, clips)
    };
    let before: Vec<(Handle, _)> = actors(run)
        .into_iter()
        .map(|h| (h, state(run, h)))
        .collect();
    for _ in 0..steps {
        step(run);
    }
    let mut out = UnderPause::default();
    for (h, was) in before {
        if run.world.actor(h).is_none_or(|a| a.removed) {
            continue;
        }
        let now = state(run, h);
        let moved = match (was.1, now.1) {
            (Some(a), Some(b)) => a.distance(b) > 1e-4,
            (a, b) => a != b,
        };
        if was.0 != now.0 || moved || was.2 != now.2 {
            out.stepped
                .push(format!("{} {was:?} -> {now:?}", run.world.name(h)));
        }
        // Those with scripts: each one rests.
        if let Some(id) = run.vm.placement_of(h) {
            out.awake += 1;
            out.resting += usize::from(run.vm.resting(id));
        }
    }
    out
}
