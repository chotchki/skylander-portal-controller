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

/// The script part's archives in link order, as `level_scripts` links them.
pub const ARCHIVES: [(&str, Role); 5] = [
    ("permanent/bootstrap.bld", Role::Bootstrap),
    ("permanent/global.bld", Role::Global),
    ("character/init_setup.bld", Role::Other),
    ("character/009_eruptor.bld", Role::Other),
    ("level/level_213_tutotial.bld", Role::Other),
];

/// The units whose running placements start with the level, in order.
pub const ORDER: [u16; 3] = [1, 2, 4];
/// Eruptor's file: its placements join with him.
pub const ERUPTOR: u16 = 3;
/// The level's file.
pub const LEVEL: u16 = 4;

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
    pub bodies: HashMap<Handle, BodyId>,
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
            let next = BodyId(HERO_BODY.0 + 1 + self.bodies.len() as u32);
            let id = *self.bodies.entry(h).or_insert(next);
            self.world
                .set_body(&Body {
                    id,
                    kind: BodyKind::Kinematic,
                    pose,
                    collider: Collider::Convex(solid.in_world()),
                })
                .unwrap();
        }
        // A creature asleep or taken off is out of the world.
        let gone: Vec<(Handle, BodyId)> = self
            .bodies
            .iter()
            .filter(|(h, _)| !self.heroes.contains_key(h) && !awake.contains(h))
            .map(|(h, b)| (*h, *b))
            .collect();
        for (_, b) in gone {
            self.world.remove_body(b);
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
        for (h, active) in self.world.activity(&mut self.bench) {
            self.vm.set_asleep(h, !active);
        }
        // The frame's slides and the walk in the original's order, the
        // player loop's slices in their places (openspec AH.59).
        let joined = self.minion.is_some();
        let player_loop = self.player_loop.clone();
        let mut stopped = Vec::new();
        let r =
            self.with_host(|vm, host| player_loop.frame_with(vm, host, joined, &mut stopped, then));
        assert!(stopped.is_empty(), "strict mode stops by halting");
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        // The props stand by where he is as the step began, after the
        // scripts' step as the TV's do: a breakable broken this step is out
        // of the seam before anything moves.
        self.bench.stand_props(&self.world);
        let report = r?;
        assert!(report.stopped.is_empty(), "strict mode stops by halting");
        let dt = 1.0 / 60.0;
        for hero in self.bench.heroes.values_mut() {
            hero.step_engine(&self.bench.world, dt, &[HERO_BODY], Surfaces::NONE);
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
        let pads = self.bench.pads;
        self.world.read_pads(pads);
        for (h, active) in self.world.activity(&mut self.bench) {
            self.vm.set_asleep(h, !active);
        }
        let joined = self.minion.is_some();
        let player_loop = self.player_loop.clone();
        let mut stopped = Vec::new();
        let r = self.with_host(|vm, host| {
            player_loop.frame_with(vm, host, joined, &mut stopped, &mut |_, _, _| Ok(()))
        });
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        self.bench.stand_props(&self.world);
        stopped.extend(r?.stopped);
        let dt = 1.0 / 60.0;
        for hero in self.bench.heroes.values_mut() {
            hero.step_engine(&self.bench.world, dt, &[HERO_BODY], Surfaces::NONE);
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;
        self.pose_hands();
        self.bench.place_bodies(&self.world);
        Ok(stopped)
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
        self.join_tag(eruptor_tag(values))
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
/// in the level (AH.50).
pub fn eruptor_rig(part: &[Vec<u8>]) -> guacamole::hands::Rig {
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).expect("the globals");
    let read = guac_extract::models::read(&part[3], "009_Eruptor", identity(), Some(&globals))
        .expect("Eruptor's model");
    guacamole::hands::Rig {
        model: read.model,
        palette: read.palette,
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
    /// Scripts listed past the fight's (openspec AH.28's probes), every
    /// tick, by script.
    pub scripts: &'static [&'static str],
}

pub fn run_on(mode: Mode) -> (Run, HashMap<MemberId, String>) {
    run_on_with(mode, Setup::default())
}

pub fn run_on_with(mode: Mode, setup: Setup) -> (Run, HashMap<MemberId, String>) {
    let dump = dump();
    let usr = dump.join("PS3_GAME/USRDIR");
    let part: Vec<Vec<u8>> = ARCHIVES
        .iter()
        .map(|(a, _)| guac_extract::scripts::entry(&usr, a).unwrap_or_else(|e| panic!("{a}: {e}")))
        .collect();
    let igzs: Vec<Igz<'_>> = part.iter().map(|b| Igz::parse(b).unwrap()).collect();
    let graphs: Vec<Graph<'_>> = igzs.iter().map(|i| Graph::read(i).unwrap()).collect();
    let units: Vec<Unit<'_, '_>> = igzs
        .iter()
        .zip(&graphs)
        .zip(ARCHIVES)
        .map(|((igz, graph), (_, role))| Unit {
            name: "part",
            role,
            igz,
            graph,
        })
        .collect();
    let program = Arc::new(Program::link(&units).unwrap());
    let player_loop = PlayerLoop::read(&program);
    let layout = layout();
    let read: Vec<(u16, &Igz<'_>)> = [1u16, 2, LEVEL, ERUPTOR]
        .iter()
        .map(|&u| (u, &igzs[u as usize]))
        .collect();
    let files = Files::read(&read).unwrap();
    let handles = headless_handles(&files);
    let objects = headless_objects(&handles);
    let mut config = scripts::fight_config(mode);
    if let guac_script::Whitelist::Only { scripts, .. } = &mut config.whitelist {
        scripts.extend(setup.scripts.iter().map(|s| (*s).to_owned()));
    }
    let found = discovery(&mut config);
    let vm = scripts::init_chain(program, &files, &handles, &ORDER, config);
    let mut world = ScriptWorld::new(&files, &handles, objects);
    world.set_inert(vm.inert());
    let template = files
        .placed
        .iter()
        .find(|p| p.name == "009_Eruptor")
        .expect("Eruptor's template")
        .obj;
    let rig = eruptor_rig(&part);
    world.set_palette_of(template, &rig.palette);
    // The creatures' palettes, by the model each placement names.
    let palettes = levels::palettes(&igzs[LEVEL as usize], levels::LEVELS[0], identity())
        .expect("the level's palettes")
        .ir;
    for p in files.placed.iter().filter(|p| p.obj.unit == LEVEL) {
        if let Some(model) = &p.model
            && let Some(pal) = palettes.palettes.iter().find(|x| &x.owner == model)
        {
            world.set_palette_of(p.obj, pal);
        }
    }
    world.walk_first_fight(LEVEL);
    world.walk_goats(LEVEL);
    let (mesh, start) = chapter_1(&igzs[LEVEL as usize]);
    let parry = ParryWorld::new(&[mesh]).expect("the level's collision");
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, start, 0.0);
    hero.settle(&parry, &[]);
    let props = setup.props.then(|| {
        let level = level_props(&igzs[LEVEL as usize]);
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
