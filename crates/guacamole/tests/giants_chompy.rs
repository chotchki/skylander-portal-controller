//! AH.18's disc tests: chapter 1's first fight on the VM over level 213 with
//! guacamole's host, headless: the init chain (AH.16), Eruptor joined
//! (AH.17), then the four Chompies Setup_001 wakes, its controller, Norticus
//! as far as the encounter reaches him, the level's enemy system and
//! `global.bld`'s mover update every step, each managed placement woken and
//! put to sleep by the activity rule from the hero's pivot, the creatures
//! moved by guac-motion's controller on the parry side of the seam over the
//! level's own collision, the hero and the creatures bodies in it. Ignored
//! by default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`)
//! and, for the comparisons, AH.10's recordings under `GUAC_CAPTURE_DIR`:
//!
//! `cargo test -p guacamole --release --test giants_chompy -- --ignored --nocapture --test-threads=1`
//!
//! A dev aid, not a check: `AH18_LIVE_TABLE=<aj-vm's live member table>`
//! answers every member the registry doesn't hold with a value cell of its
//! registered type and prints which the run reached.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use guac_extract::levels;
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_ir::{DumpIdentity, Sha256};
use guac_motion::hero::Hero;
use guac_motion::parry::ParryWorld;
use guac_motion::proxy::Surfaces;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, PhysicsSeam, Pose, Vec3};
use guac_motion::stick::Stick;
use guac_motion::tuning::{Tuning, eruptor_solid};
use guac_script::members::{CREATURES, HERO as HERO_MEMBERS, INIT_CHAIN};
use guac_script::{
    Generator, Halt, Handle, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role,
    Stall, StallDetector, StallLimit, Unit, Value, Vm,
};
use guacamole::level_scene;
use guacamole::script_engine::{Engine, FEET_PER_METRE, Pad};
use guacamole::scripts::{
    self, FIRST_FIGHT, Files, ScriptWorld, WorldHost, headless_handles, headless_objects,
    headless_spawner,
};

/// `key` from the environment, else from the repo's `.env.dev`.
fn env_or_dotenv(key: &str) -> Option<String> {
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

fn dump() -> PathBuf {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let root = PathBuf::from(root);
    assert!(root.join("PS3_GAME/USRDIR").is_dir(), "no PS3_GAME/USRDIR");
    root
}

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

/// The script part's archives in link order, as `level_scripts` links them.
const ARCHIVES: [(&str, Role); 5] = [
    ("permanent/bootstrap.bld", Role::Bootstrap),
    ("permanent/global.bld", Role::Global),
    ("character/init_setup.bld", Role::Other),
    ("character/009_eruptor.bld", Role::Other),
    ("level/level_213_tutotial.bld", Role::Other),
];

/// The units whose running placements start with the level, in order.
const ORDER: [u16; 3] = [1, 2, 4];
/// Eruptor's file: its placements join with him.
const ERUPTOR: u16 = 3;
/// The level's file.
const LEVEL: u16 = 4;

/// The hero's handle in a headless run: past every placement's and object's.
const HERO: Handle = 0x7000_0000;
/// The hero's body in the seam; each creature's counts up from past it.
const HERO_BODY: BodyId = BodyId(9_000);

/// The engine a headless run owns: Eruptor's controller and the creatures'
/// bodies on the parry side of the seam over the level's collision.
struct Bench {
    heroes: HashMap<Handle, Hero>,
    world: ParryWorld,
    view: f32,
    pads: [Pad; 2],
    bodies: HashMap<Handle, BodyId>,
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
    fn pad(&self, n: usize) -> Pad {
        self.pads.get(n).copied().unwrap_or_default()
    }
    fn body(&self, actor: Handle) -> Option<BodyId> {
        self.bodies.get(&actor).copied()
    }
    fn actor_of(&self, body: BodyId) -> Option<Handle> {
        self.bodies
            .iter()
            .find(|(_, b)| **b == body)
            .map(|(h, _)| *h)
    }
}

impl Bench {
    /// Each body where its actor stands after the frame: the hero's solid
    /// and each awake creature's, as the seam holds solids (0.05 m in).
    fn place_bodies(&mut self, world: &ScriptWorld) {
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
        let walkers = world.walkers();
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
struct Run {
    vm: Vm,
    world: ScriptWorld,
    bench: Bench,
    generator: Generator,
    files: Files,
    handles: HashMap<Obj, Handle>,
    next: Handle,
    minion: Option<PlacementId>,
    steps: u32,
}

impl Run {
    fn with_host<T>(&mut self, f: impl FnOnce(&mut Vm, &mut WorldHost<'_, '_>) -> T) -> T {
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

    /// One step: the pads read, every managed placement woken or put to
    /// sleep by the activity rule, the combat manager and the projectiles
    /// managed as the portal manager's tick runs them (once he's in),
    /// every placement's tick, the clones of scripted templates added, then
    /// the engine's frame: the hero moved by the walk his scripts left, the
    /// sequences on, the creatures walked, what the scripts threw flying,
    /// the bodies placed where their actors stand.
    fn step(&mut self) -> Result<(), Halt> {
        let pads = self.bench.pads;
        self.world.read_pads(pads);
        for (h, active) in self.world.activity(&mut self.bench) {
            self.vm.set_asleep(h, !active);
        }
        if self.minion.is_some() {
            self.with_host(scripts::manage_combat)?;
            self.with_host(scripts::manage_projectiles)?;
        }
        let r = self.with_host(|vm, host| vm.step(host));
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        let report = r?;
        assert!(report.stopped.is_empty(), "strict mode stops by halting");
        let dt = 1.0 / 60.0;
        for hero in self.bench.heroes.values_mut() {
            hero.step_engine(&self.bench.world, dt, &[HERO_BODY], Surfaces::NONE);
        }
        self.world.advance(dt, &mut self.bench);
        self.bench.place_bodies(&self.world);
        self.steps += 1;
        Ok(())
    }

    /// The join at this step: the request now, the activation after the
    /// definition's first tick.
    fn join(&mut self) -> Result<(), Halt> {
        let files = self.files.clone();
        let handles = self.handles.clone();
        self.with_host(|vm, host| scripts::join_request(vm, host, &files, ERUPTOR, &handles))?;
        self.step()?;
        let minion = self.with_host(|vm, host| scripts::join_activate(vm, host, HERO))?;
        self.minion = Some(minion);
        Ok(())
    }

    /// The hero set down at `at` (dropped onto what's under it) facing
    /// `facing`, at rest: a scripted hero's teleport.
    fn put(&mut self, at: Vec3, facing: f32) {
        let hero = self.bench.heroes.get_mut(&HERO).unwrap();
        hero.pivot = at + Vec3::Y;
        hero.facing = facing;
        hero.walk.velocity = Vec3::ZERO;
        hero.settle(&self.bench.world, &[HERO_BODY]);
    }

    /// The combat manager's next alert `steps` steps on: `combat control
    /// data`'s `alert next time` set between this step's game timer and the
    /// next's (its check runs before the placements' ticks).
    fn alert_in(&mut self, steps: u32) {
        let g = self
            .vm
            .program()
            .global_named("combat control data")
            .unwrap();
        let control = self.vm.global(g).map(|v| self.vm.value(v)).unwrap();
        let now = self
            .vm
            .field(control, "game timer")
            .and_then(|v| self.vm.value(v).as_f32())
            .unwrap();
        let next = now + (steps as f32 - 0.5) / 60.0;
        assert!(
            self.vm
                .set_field(control, "alert next time", Value::Float(next))
        );
    }

    /// A level placement set where a recorded state has it, at rest.
    fn set_at(&mut self, name: &str, at: Vec3, facing: f32) {
        let (h, _) = self.placed(name);
        assert!(self.world.place(h, at, facing), "{name}");
    }

    /// The hero's health as his scripts hold it (`description.creature
    /// info.hp` on his minion), once he's joined.
    fn health(&self) -> Option<f32> {
        let m = self.minion?;
        let d = self.vm.local(m, "description").map(|v| self.vm.value(v))?;
        let c = self
            .vm
            .field(d, "creature info")
            .map(|v| self.vm.value(v))?;
        self.vm.field(c, "hp").map(|v| self.vm.value(v))?.as_f32()
    }

    fn hero(&self) -> &Hero {
        &self.bench.heroes[&HERO]
    }

    /// A level placement's handle and where its file puts it, metres.
    fn placed(&self, name: &str) -> (Handle, Vec3) {
        let p = self
            .files
            .placed
            .iter()
            .find(|p| p.name == name && p.obj.unit == LEVEL)
            .unwrap_or_else(|| panic!("no {name}"));
        (self.handles[&p.obj], Vec3::from_array(p.position))
    }

    fn placement(&self, name: &str) -> PlacementId {
        let (h, _) = self.placed(name);
        self.vm
            .placement_of(h)
            .unwrap_or_else(|| panic!("{name} isn't run"))
    }

    /// Where the engine has the actor now, metres (its file's place until
    /// something moves it).
    fn at(&self, name: &str) -> Vec3 {
        let (h, _) = self.placed(name);
        self.world.actor(h).unwrap().position
    }
}

/// The level's collision mesh and where the hero starts.
fn chapter_1(level: &Igz<'_>) -> (Body, Vec3) {
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

/// Each slot's clip length at 100 %, 0 where a slot is empty or a network.
fn lengths(p: &guac_ir::palette::Palette) -> Vec<f32> {
    let mut out = vec![0.0; usize::from(p.slots)];
    for e in &p.entries {
        if let guac_ir::palette::SlotEntry::Clip { clip, .. } = &e.entry {
            out[usize::from(e.slot)] = clip.duration;
        }
    }
    out
}

/// Each slot's clip length on Eruptor's model, read from his archive with
/// the globals his model's shaders need.
fn eruptor_palette(part: &[Vec<u8>]) -> Vec<f32> {
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).expect("the globals");
    let read = guac_extract::models::read(&part[3], "009_Eruptor", identity(), Some(&globals))
        .expect("Eruptor's model");
    lengths(&read.palette)
}

/// Each live-table member the registry doesn't hold, as a value cell of its
/// registered type, by name: the discovery aid.
fn discovery(config: &mut guac_script::Config) -> HashMap<MemberId, String> {
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
fn run_on(mode: Mode) -> (Run, HashMap<MemberId, String>) {
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
    let read: Vec<(u16, &Igz<'_>)> = [1u16, 2, LEVEL, ERUPTOR]
        .iter()
        .map(|&u| (u, &igzs[u as usize]))
        .collect();
    let files = Files::read(&read).unwrap();
    let handles = headless_handles(&files);
    let objects = headless_objects(&handles);
    let mut config = scripts::fight_config(mode);
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
    world.set_palette(template, eruptor_palette(&part));
    // The creatures' palettes, by the model each placement names.
    let palettes = levels::palettes(&igzs[LEVEL as usize], levels::LEVELS[0], identity())
        .expect("the level's palettes")
        .ir;
    for p in files.placed.iter().filter(|p| p.obj.unit == LEVEL) {
        if let Some(model) = &p.model
            && let Some(pal) = palettes.palettes.iter().find(|x| &x.owner == model)
        {
            world.set_palette(p.obj, lengths(pal));
        }
    }
    for name in FIRST_FIGHT {
        if let Some(p) = files
            .placed
            .iter()
            .find(|p| p.name == name && p.obj.unit == LEVEL)
            && p.physics.is_some()
        {
            world.walk(handles[&p.obj]);
        }
    }
    let (mesh, start) = chapter_1(&igzs[LEVEL as usize]);
    let parry = ParryWorld::new(&[mesh]).expect("the level's collision");
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, start, 0.0);
    hero.settle(&parry, &[]);
    let bench = Bench {
        heroes: HashMap::from([(HERO, hero)]),
        world: parry,
        view: 15.0,
        pads: [Pad::default(); 2],
        bodies: HashMap::new(),
    };
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
            minion: None,
            steps: 0,
        },
        found,
    )
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The discovery aid's report: the value-cell members the run reached and
/// how often each was read and written.
fn report(run: &Run, found: &HashMap<MemberId, String>) {
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
fn joined(mode: Mode) -> (Run, HashMap<MemberId, String>) {
    let (mut run, found) = run_on(mode);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    (run, found)
}

/// AH.8's route from the strip to anchor B as a scripted hero's teleports
/// (`ah8-route-1792539821/route_b`'s `route-record.json`, its steps' end
/// points): the step each is made, where (metres) and his facing. 16 ft
/// from `Setup_001_Trigger` and 36 ft from the Norticus trigger where the
/// trigger fired (step 106's end, `s001`); 23 ft from the Norticus trigger
/// 122 steps later (`activate`, where it fired 122 game frames after);
/// on the scaffold at the pit's rim, 26 ft above `Enemy_Chompy(1)`
/// (`scaffold`); landed on the pit floor (`cutscene`, where the 7 ft rule
/// had passed); anchor B (`anchor`, 31.7 ft from `Enemy_Chompy(2)`).
const ROUTE: [(u32, [f32; 3], f32); 5] = [
    (0, [31.75, 44.06, -5.55], 270.0),
    (122, [27.76, 43.06, -5.04], 250.0),
    (180, [10.67, 30.35, -1.93], 200.0),
    (300, [7.27, 22.39, -7.56], 200.0),
    (360, [5.445, 22.393, -12.023], 202.28),
];

/// The creatures where anchor B has them (`actors-anchor.json`): AH.8's
/// route ran the intro cutscene, which guacamole doesn't, so the fourth
/// Chompy's walk round its path is somewhere else when the hero lands.
const ANCHOR_B: [(&str, [f32; 3], f32); 5] = [
    ("Enemy_Chompy(1)", [4.5213137, 22.374975, -23.117332], 243.0),
    (
        "Enemy_Chompy(2)",
        [1.8390902, 22.380882, -20.97726],
        178.00002,
    ),
    (
        "Enemy_Chompy(3)",
        [-0.13646065, 22.384933, -21.690493],
        146.0,
    ),
    (
        "Enemy_Chompy(4)",
        [3.6175919, 22.28899, -27.261147],
        258.4939,
    ),
    (
        "1.NPC_Norticus_First_Encounter",
        [2.0715888, 22.338335, -23.759197],
        0.0,
    ),
];

/// The step AH.8's opening starts at: 120 steps after the hero lands at
/// anchor B's pose, the creatures set there again at it (F0).
const OPENING: u32 = 480;

/// The combat manager's first alert from F0, steps: anchor B's state holds
/// the 0.25 s check's phase, AH.10's first alert landing at F0 + 24 (or 25,
/// by F0's own frame time).
const FIRST_ALERT: u32 = 24;

/// A development probe: the encounter's placements and the route's
/// triggers where the files put them, then the route as AH.8 walked it
/// (Setup_001's trigger, 122 steps later the Norticus trigger, then down to
/// within 7 ft of `Enemy_Chompy(1)`), `AH18_STEPS` steps after it, the
/// behaviours each step they change; with `AH18_LIVE_TABLE` the members
/// reached that nothing registers.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_the_route() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (mut run, found) = joined(Mode::Strict);
    for name in FIRST_FIGHT {
        let p = run.files.placed.iter().find(|p| p.name == name).unwrap();
        println!(
            "{name}: model {:?}, palette {:?}",
            p.model,
            run.world.palette(p.obj).map(|l| l
                .iter()
                .enumerate()
                .filter(|x| *x.1 > 0.0)
                .map(|(i, x)| format!("{i}:{x:.3}"))
                .collect::<Vec<_>>())
        );
    }
    let names = [
        "Setup_001_Trigger",
        "Setup_001_Controller",
        "Activate_Norticus and Chompies",
        "Enemy_Chompy(1)",
        "Enemy_Chompy(2)",
        "Enemy_Chompy(3)",
        "Enemy_Chompy(4)",
        "1.NPC_Norticus_First_Encounter",
        "EnemySystem_",
    ];
    for n in names {
        let (h, at) = run.placed(n);
        let a = run.world.actor(h).unwrap();
        println!(
            "{n}: at {at:?}, range {} buffer {}, run {:?}",
            a.range,
            a.buffer,
            run.vm.placement_of(h)
        );
    }
    let steps: u32 = std::env::var("AH18_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let watched: Vec<(String, PlacementId)> = names
        .iter()
        .filter_map(|n| {
            let (h, _) = run.placed(n);
            run.vm.placement_of(h).map(|id| ((*n).to_owned(), id))
        })
        .collect();
    let mut last: HashMap<String, (Option<String>, bool)> = HashMap::new();
    // The hands where AH.7's first lob leaves them (AH.24's stand-in).
    let hand = Vec3::new(0.6011, 0.7710, 1.4838);
    let mut nodes = vec![Vec3::ZERO; 9];
    nodes[7] = hand;
    nodes[8] = Vec3::new(-hand.x, hand.y, hand.z);
    run.world.set_nodes(HERO, nodes);
    let mut result = Ok(());
    let mut scene = 0;
    for k in 0..steps {
        if let Some((_, at, facing)) = ROUTE.iter().find(|r| r.0 == k) {
            run.put(Vec3::from_array(*at), *facing);
        }
        if k == ROUTE[4].0 {
            for (name, at, facing) in ANCHOR_B {
                run.set_at(name, Vec3::from_array(at), facing);
            }
        }
        // AH.8's opening from anchor B: the stick toward `Enemy_Chompy(2)`
        // for 10 steps, then let go.
        if k == OPENING {
            let to = run.at("Enemy_Chompy(2)") - run.hero().pivot;
            run.bench.view = guacamole::script_engine::heading_of(to);
        }
        run.bench.pads[0] = if (OPENING..OPENING + 10).contains(&k) {
            stick(Stick::new(0.0, 1.0))
        } else {
            stick(Stick::CENTRED)
        };
        // AH.10's `fight`: a lob (South) at F0 + 90 and an eruption (West)
        // at F0 + 160, each held 6 steps, his face toward Chompy(2).
        if std::env::var("AH18_FIGHT").is_ok() {
            if k == OPENING + 89 || k == OPENING + 159 {
                let to = run.at("Enemy_Chompy(2)") - run.hero().pivot;
                run.bench.heroes.get_mut(&HERO).unwrap().facing =
                    guacamole::script_engine::heading_of(to).rem_euclid(360.0);
            }
            if (OPENING + 90..OPENING + 96).contains(&k) {
                run.bench.pads[0].actions[0] = true;
            }
            if (OPENING + 160..OPENING + 166).contains(&k) {
                run.bench.pads[0].actions[1] = true;
            }
            // The fourth, once it's come at him: an eruption every 2 s
            // from F0 + 700.
            if k >= OPENING + 700 && (k - OPENING - 700) % 120 < 6 {
                run.bench.pads[0].actions[1] = true;
            }
        }
        let calls_at = std::env::var("AH18_CALLS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            == Some(k);
        let before = run.vm.calls().clone();
        let tracing = std::env::var("AH18_TRACE")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            == Some(k);
        if tracing {
            run.vm.trace(Some(400_000));
        }
        if let Err(h) = run.step() {
            result = Err(h);
            break;
        }
        if calls_at {
            let program = run.vm.program_arc();
            let mut called: Vec<(String, u64)> = run
                .vm
                .calls()
                .iter()
                .filter_map(|(d, n)| {
                    let was = before.get(d).copied().unwrap_or(0);
                    (*n > was).then(|| (program.defs[*d as usize].display.clone(), n - was))
                })
                .collect();
            called.sort();
            println!("step {k} called: {called:?}");
        }
        if tracing {
            let script = std::env::var("AH18_TRACE_SCRIPT").unwrap_or_default();
            let program = run.vm.program_arc();
            let mut seen = Vec::new();
            for (sid, pos) in run.vm.traced() {
                let name = &program.scripts[sid as usize].name;
                if name.contains(&script) {
                    seen.push(pos);
                }
            }
            println!("step {k} traced {script}: {seen:?}");
        }
        for i in 1..=4 {
            let n = format!("Enemy_Chompy({i})");
            let (h, _) = run.placed(&n);
            let Some(id) = run.vm.placement_of(h) else {
                continue;
            };
            let att = run
                .vm
                .local(id, "description")
                .map(|d| run.vm.value(d))
                .and_then(|d| run.vm.field(d, "attention parameters"))
                .map(|a| run.vm.value(a));
            let read = |f: &str| {
                att.and_then(|a| run.vm.field(a, f))
                    .map(|v| run.vm.value(v))
            };
            let now = format!(
                "alerted {:?} target {:?} communicator {:?} notice {:?} radius {:?} comm {:?}",
                read("alerted"),
                read("alerted target"),
                read("communicator"),
                read("notice player radius"),
                read("attention radius"),
                read("attention communication radius"),
            );
            let key = format!("att{i}");
            if last.get(&key).map(|v| v.0.clone()) != Some(Some(now.clone())) {
                println!("step {k}: {n} {now}");
                last.insert(key, (Some(now), false));
            }
        }
        if (358..=362).contains(&k) {
            let hp = run.hero().pivot;
            let d: Vec<String> = (1..=4)
                .map(|i| {
                    let p = run.at(&format!("Enemy_Chompy({i})"));
                    format!("({i}) {:?} {:.2} ft", p, (p - hp).length() * FEET_PER_METRE)
                })
                .collect();
            println!("step {k}: hero {hp:?}; {d:?}");
            for i in 1..=4 {
                let (h, _) = run.placed(&format!("Enemy_Chompy({i})"));
                let got = run.with_host(|_, host| {
                    guac_script::Host::get(
                        host,
                        &guac_script::Access {
                            placement: 0,
                            receiver: Some(Value::Entity(h)),
                            member: MemberId::named("positionmeasurement", "distance"),
                            other_point: Some(Value::Entity(HERO)),
                        },
                    )
                });
                println!("  host distance ({i}) {got:?}");
            }
        }
        // The cutscene manager isn't on the whitelist (the stand-in row's
        // camera pans and dialogue): the scene's end set as it would be 5 s
        // after it starts (AH.10's 'Advance with player').
        if std::env::var("AH18_SCENE_END").is_ok() {
            let id = run.placement("1.NPC_Norticus_First_Encounter");
            if run.vm.behaviour(id) == Some("cutscene go") {
                scene += 1;
                if scene == 300 {
                    let g = run.vm.program().global_named("Cutscene State").unwrap();
                    assert!(run.vm.set_global(g, Value::Int(0)));
                    println!("step {k}: the scene's end set");
                }
            }
        }
        let hp = run.health();
        if last.get("hp").map(|v| v.0.clone()) != Some(Some(format!("{hp:?}"))) {
            println!("step {k}: hero health {hp:?} at {:?}", run.hero().pivot);
            last.insert("hp".to_owned(), (Some(format!("{hp:?}")), false));
        }
        for (n, id) in &watched {
            let now = (run.vm.behaviour(*id).map(str::to_owned), run.vm.asleep(*id));
            if last.get(n) != Some(&now) {
                println!("step {k}: {n} {:?} asleep {}", now.0, now.1);
                last.insert(n.clone(), now);
            }
        }
    }
    println!(
        "hero at {:?}; {} steps, {} ops, {:.2?}",
        run.hero().pivot,
        run.steps,
        run.vm.executed(),
        started.elapsed()
    );
    for n in &names[3..7] {
        let (h, _) = run.placed(n);
        let obj = run
            .files
            .placed
            .iter()
            .find(|p| run.handles[&p.obj] == h)
            .unwrap()
            .obj;
        let program = run.vm.program_arc();
        let tags: Vec<String> = program
            .tags
            .get(&obj)
            .map(|t| {
                t.iter()
                    .map(|t| {
                        format!(
                            "{} {:?}",
                            program.sym(t.name),
                            t.value.map(|o| &program.operands[o as usize])
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        println!("{n} {obj:?} tags {tags:?}");
        let sid = program.script_named("S2_Enemy_Chompy.ai").unwrap();
        let declared: Vec<String> = program.scripts[sid as usize]
            .declared
            .iter()
            .map(|(k, d)| format!("{}:{:?}", program.sym(*k), d))
            .filter(|s| {
                s.contains("range")
                    || s.contains("radius")
                    || s.contains("oil")
                    || s.contains("purple")
            })
            .collect();
        println!("  declared {declared:?}");
        let id = run.placement(n);
        println!(
            "{n}: notice {:?}, communicate {:?}, listen {:?}",
            run.vm.local(id, "notice player range"),
            run.vm.local(id, "communicate range"),
            run.vm.local(id, "listen range")
        );
    }
    for n in &names[3..8] {
        let (h, at) = run.placed(n);
        let a = run.world.actor(h).unwrap();
        println!(
            "{n}: now at {:?} (file {at:?}), facing {}, velocity {:?}, solid {:?}",
            a.position,
            a.angles.facing,
            a.velocity,
            a.havok()
        );
    }
    println!("woken off the whitelist: {:?}", run.world.woken);
    if let Some(g) = run.vm.program().global_named("Minions") {
        let v = run.vm.global(g);
        let set = v.and_then(|v| run.vm.set(v).map(<[Value]>::to_vec));
        println!("G:Minions {v:?} = {set:?}");
    }
    report(&run, &found);
    println!("load after: {}", load());
    if let Err(h) = result {
        panic!("{h}");
    }
}

#[allow(dead_code)]
fn feet(d: f32) -> f32 {
    d * FEET_PER_METRE
}

fn stick(s: Stick) -> Pad {
    Pad {
        stick: s,
        connected: true,
        ..Pad::default()
    }
}

/// What a headless fight does past anchor B, by steps from AH.8's opening
/// (AH.10's F0: the stick toward `Enemy_Chompy(2)` 10 steps).
#[derive(Debug, Clone, Copy, Default)]
struct Script {
    /// South (a lob) for 6 steps from this step, his face toward
    /// `Enemy_Chompy(2)`.
    lob: Option<u32>,
    /// West (an eruption) for 6 steps from this step.
    erupt: Option<u32>,
    /// West again every 120 steps from this step (for the fourth).
    erupt_every: Option<u32>,
    /// The scene's end set 5 s into Norticus' scene (the cutscene manager
    /// isn't on the whitelist).
    scene_end: bool,
    /// Steps past the opening's start.
    steps: u32,
}

/// A Chompy as one step leaves it.
#[derive(Debug, Clone)]
struct ChompySnap {
    beh: Option<String>,
    pos: Vec3,
    facing: f32,
    /// `$description.$attention parameters.$alerted`: 2 when it saw him.
    alerted: i32,
    /// Who called it: none, or the Chompy whose call it was.
    communicator: Value,
    /// `$stateMachine.$state tracking.$state` and `.$substate`.
    state: i32,
    substate: i32,
    /// `$description.$creature info.$invulnerable`.
    invulnerable: i32,
    gone: bool,
}

#[derive(Debug, Clone)]
struct Snap {
    /// Steps from the opening's start (AH.10's F0).
    k: i64,
    hero: Vec3,
    health: f32,
    chompies: Vec<ChompySnap>,
    norticus: (Option<String>, Vec3, bool),
    controller: Option<String>,
}

const CHOMPIES: [&str; 4] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
];
const NORTICUS: &str = "1.NPC_Norticus_First_Encounter";

fn int_of(run: &Run, v: Option<Value>) -> i32 {
    match v.map(|v| run.vm.value(v)) {
        Some(Value::Int(i)) => i,
        Some(Value::Float(f)) => f as i32,
        _ => 0,
    }
}

/// A struct field path off a placement's local, each step's value read.
fn path(run: &Run, id: PlacementId, local: &str, fields: &[&str]) -> Option<Value> {
    let mut v = run.vm.local(id, local).map(|v| run.vm.value(v))?;
    for f in fields {
        v = run.vm.field(v, f).map(|x| run.vm.value(x))?;
    }
    Some(v)
}

impl Run {
    fn snap(&self, k: i64) -> Snap {
        let chompies = CHOMPIES
            .iter()
            .map(|n| {
                let (h, _) = self.placed(n);
                let id = self.vm.placement_of(h).unwrap();
                let a = self.world.actor(h).unwrap();
                ChompySnap {
                    beh: self.vm.behaviour(id).map(str::to_owned),
                    pos: a.position,
                    facing: a.angles.facing,
                    alerted: int_of(
                        self,
                        path(
                            self,
                            id,
                            "description",
                            &["attention parameters", "alerted"],
                        ),
                    ),
                    communicator: path(
                        self,
                        id,
                        "description",
                        &["attention parameters", "communicator"],
                    )
                    .unwrap_or(Value::Null),
                    state: int_of(
                        self,
                        path(self, id, "stateMachine", &["state tracking", "state"]),
                    ),
                    substate: int_of(
                        self,
                        path(self, id, "stateMachine", &["state tracking", "substate"]),
                    ),
                    invulnerable: int_of(
                        self,
                        path(self, id, "description", &["creature info", "invulnerable"]),
                    ),
                    gone: self.vm.removed(id),
                }
            })
            .collect();
        let (nh, _) = self.placed(NORTICUS);
        let nid = self.vm.placement_of(nh).unwrap();
        let ctl = self.placement("Setup_001_Controller");
        Snap {
            k,
            hero: self.hero().pivot,
            health: self.health().unwrap_or(0.0),
            chompies,
            norticus: (
                self.vm.behaviour(nid).map(str::to_owned),
                self.world.actor(nh).unwrap().position,
                self.vm.removed(nid),
            ),
            controller: self.vm.behaviour(ctl).map(str::to_owned),
        }
    }
}

/// The run along AH.8's route to anchor B (the creatures where anchor B has
/// them), then `script` from the opening, strict: a snap each step from the
/// opening on, and the run.
fn fight(script: Script) -> (Run, Vec<Snap>) {
    let (mut run, _) = joined(Mode::Strict);
    let hand = Vec3::new(0.6011, 0.7710, 1.4838);
    let mut nodes = vec![Vec3::ZERO; 9];
    nodes[7] = hand;
    nodes[8] = Vec3::new(-hand.x, hand.y, hand.z);
    run.world.set_nodes(HERO, nodes);
    let mut snaps = Vec::new();
    let mut scene = 0;
    let end = OPENING + script.steps;
    for k in 0..end {
        if let Some((_, at, facing)) = ROUTE.iter().find(|r| r.0 == k) {
            run.put(Vec3::from_array(*at), *facing);
        }
        if k == ROUTE[4].0 {
            for (name, at, facing) in ANCHOR_B {
                run.set_at(name, Vec3::from_array(at), facing);
            }
        }
        if k == OPENING {
            // F0 is anchor B's state: the creatures where it has them and
            // the combat manager's 0.25 s check in its phase.
            for (name, at, facing) in ANCHOR_B {
                run.set_at(name, Vec3::from_array(at), facing);
            }
            run.alert_in(FIRST_ALERT);
            let to = run.at("Enemy_Chompy(2)") - run.hero().pivot;
            run.bench.view = guacamole::script_engine::heading_of(to);
        }
        let f = k as i64 - OPENING as i64;
        let mut pad = if (0..10).contains(&f) {
            stick(Stick::new(0.0, 1.0))
        } else {
            stick(Stick::CENTRED)
        };
        let at = |s: Option<u32>| s.is_some_and(|s| (s as i64..s as i64 + 6).contains(&f));
        if script.lob.is_some_and(|s| f == s as i64 - 1) {
            let to = run.at("Enemy_Chompy(2)") - run.hero().pivot;
            run.bench.heroes.get_mut(&HERO).unwrap().facing =
                guacamole::script_engine::heading_of(to).rem_euclid(360.0);
        }
        pad.actions[0] = at(script.lob);
        pad.actions[1] = at(script.erupt)
            || script
                .erupt_every
                .is_some_and(|s| f >= s as i64 && (f - s as i64) % 120 < 6);
        run.bench.pads[0] = pad;
        if script.scene_end {
            let id = run.placement(NORTICUS);
            if run.vm.behaviour(id) == Some("cutscene go") {
                scene += 1;
                if scene == 300 {
                    let g = run.vm.program().global_named("Cutscene State").unwrap();
                    assert!(run.vm.set_global(g, Value::Int(0)));
                }
            }
        }
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        if f >= -1 {
            snaps.push(run.snap(f));
        }
    }
    (run, snaps)
}

/// A nip under way: its first step, the middles then (ft), each state's
/// first step, the lunge's top speed (ft/s).
type NipSoFar = (i64, f32, Vec<(i32, i64)>, f32);

fn ft(d: f32) -> f32 {
    d * FEET_PER_METRE
}

/// Each run of steps one Chompy spends in a behaviour: its name, the step
/// it entered and the step it left (`None` while it's still in it).
fn stays(snaps: &[Snap], c: usize) -> Vec<(Option<String>, i64, Option<i64>)> {
    let mut out: Vec<(Option<String>, i64, Option<i64>)> = Vec::new();
    for s in snaps {
        let b = if s.chompies[c].gone {
            Some("(gone)".to_owned())
        } else {
            s.chompies[c].beh.clone()
        };
        match out.last_mut() {
            Some(last) if last.0 == b => {}
            _ => {
                if let Some(last) = out.last_mut() {
                    last.2 = Some(s.k);
                }
                out.push((b, s.k, None));
            }
        }
    }
    out
}

/// The 7 ft rule (creatures' Down into the pit and Watching from above):
/// the controller waits while the hero stands on the scaffold at the pit's
/// rim, 26 ft above `Enemy_Chompy(1)`, and the encounter starts the step he
/// lands on the floor: the Chompies' invulnerability off, then its
/// triumph's wait. Two placements off the whitelist woken on the way, inert
/// and listed: the spike gate (the Norticus trigger) and the intro
/// cutscene (the controller).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_four_wake_on_the_7_ft_rule_from_the_floor_not_the_rim() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (mut run, _) = joined(Mode::Strict);
    let mut log: Vec<(u32, Option<String>, Vec<i32>)> = Vec::new();
    for k in 0..360 {
        if let Some((_, at, facing)) = ROUTE.iter().find(|r| r.0 == k) {
            run.put(Vec3::from_array(*at), *facing);
        }
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let s = run.snap(k as i64);
        let inv: Vec<i32> = s.chompies.iter().map(|c| c.invulnerable).collect();
        if log
            .last()
            .is_none_or(|l| (&l.1, &l.2) != (&s.controller, &inv))
        {
            println!(
                "step {k}: controller {:?}, invulnerable {inv:?}",
                s.controller
            );
            log.push((k, s.controller, inv));
        }
    }
    // Woken at the trigger (the Chompies' ranges set), bullying Norticus,
    // then waiting for a minion within 7 ft.
    let waiting = log
        .iter()
        .find(|l| l.1.as_deref() == Some("wait for minion"))
        .expect("it waits");
    assert!(waiting.0 < 10, "{log:?}");
    assert_eq!(waiting.2, vec![1; 4], "invulnerable while it waits");
    // On the scaffold (steps 180-299) nothing changes; landed (300) it
    // starts.
    let intro = log
        .iter()
        .find(|l| l.1.as_deref() == Some("chompy - intro"))
        .expect("the 7 ft rule");
    assert_eq!(intro.0, ROUTE[3].0, "the step he lands");
    let rim = ROUTE[2].1[1] - run.at("Enemy_Chompy(1)").y;
    println!(
        "the rim {:.1} ft above Enemy_Chompy(1); the intro at step {}",
        ft(rim),
        intro.0
    );
    assert!(ft(rim) > 7.0);
    let after = log.iter().find(|l| l.0 > intro.0).expect("on");
    assert_eq!(after.2, vec![0; 4], "invulnerability off: {log:?}");
    let woken: Vec<(&str, &str)> = run
        .world
        .woken
        .iter()
        .map(|w| (w.name.as_str(), w.by.as_str()))
        .collect();
    println!("woken off the whitelist: {woken:?}");
    assert_eq!(
        woken,
        [
            (
                "Spike_Gate_Wooden_Template(1)",
                "Activate_Norticus and Chompies"
            ),
            ("Eon_4_help_Norticus", "Setup_001_Controller"),
        ]
    );
    for w in &run.world.woken {
        let id = run.vm.placement_of(w.placement).expect("a placement");
        assert_eq!(run.vm.behaviour(id), None, "{} stays inert", w.name);
    }
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// The middles of a Chompy and the hero: each pivot 3 ft up (both 3 ft
/// spheres as authored, AH.10's middles).
fn middles(c: Vec3, hero: Vec3) -> f32 {
    ft((c - hero).length())
}

/// What a standing-still fight from anchor B makes of AH.10's numbers
/// (`ah10-main`'s `still`: the opening, then nothing to F0 + 900): the
/// wake, the calls, each Chompy's sequence, the nips and the bites. Every
/// number the box names, printed and held to AH.10's.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn standing_still_they_wake_close_in_and_bite_as_ah10_recorded() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (_, snaps) = fight(Script {
        steps: 900,
        ..Script::default()
    });
    println!("{} snaps in {:.2?}", snaps.len(), started.elapsed());
    // The wake: within 30 ft of `Enemy_Chompy(2)`, alerted at the next
    // 0.25 s check (AH.10: 14-15 frames on, seen at 27.6-28.4 ft).
    let within = snaps
        .iter()
        .find(|s| middles(s.chompies[1].pos, s.hero) <= 30.0)
        .expect("he comes within 30 ft")
        .k;
    let alert = snaps
        .iter()
        .find(|s| s.chompies.iter().any(|c| c.alerted > 0))
        .expect("an alert");
    let seers: Vec<usize> = (0..4)
        .filter(|&c| {
            alert.chompies[c].alerted == 2 && alert.chompies[c].communicator == Value::Null
        })
        .collect();
    println!(
        "within 30 ft of Enemy_Chompy(2) at F0 + {within}; alerted at F0 + {} ({} frames on), seen by {:?} at {:?} ft",
        alert.k,
        alert.k - within,
        seers.iter().map(|&c| CHOMPIES[c]).collect::<Vec<_>>(),
        seers
            .iter()
            .map(|&c| middles(alert.chompies[c].pos, alert.hero))
            .collect::<Vec<_>>()
    );
    assert!(
        (14..=15).contains(&(alert.k - within)),
        "the next 0.25 s check"
    );
    for &c in &seers {
        let d = middles(alert.chompies[c].pos, alert.hero);
        assert!(d <= [30.0, 30.0, 30.0, 35.0][c], "{} at {d}", CHOMPIES[c]);
    }
    // The calls: those within 20 ft of a seer among the first three (60 of
    // the fourth) are called, the rest aren't.
    for (c, ch) in alert.chompies.iter().enumerate() {
        let caller = seers
            .iter()
            .map(|&s| (s, ft((alert.chompies[s].pos - ch.pos).length())))
            .filter(|&(s, _)| s != c)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((s, d)) = caller {
            let reach = if s == 3 { 60.0 } else { 20.0 };
            println!(
                "{} {} ft from {}: alerted {} (communicator {:?})",
                CHOMPIES[c], d, CHOMPIES[s], ch.alerted, ch.communicator
            );
            if !seers.contains(&c) {
                assert_eq!(ch.alerted > 0, d <= reach, "{}", CHOMPIES[c]);
            }
        }
    }
    // Each one's sequence.
    let mut notice = Vec::new();
    let mut waits = Vec::new();
    let mut nips = Vec::new();
    // Each nip's entry: the middles then (ft) and whether it walked in (it
    // moved over the step before, the hero still).
    let mut entries: Vec<(f32, bool)> = Vec::new();
    let mut top_speed = 0.0f32;
    let mut top_turn = 0.0f32;
    for (c, name) in CHOMPIES.iter().enumerate() {
        let st = stays(&snaps, c);
        println!(
            "{}: {:?}",
            name,
            st.iter()
                .map(|s| (s.0.clone(), s.1, s.2))
                .collect::<Vec<_>>()
        );
        // The alerted state's wait (substate 1) and notice (substate 2 to
        // 'battle').
        let mut in_wait = None;
        let mut in_notice = None;
        for w in snaps.windows(2) {
            let (a, b) = (&w[0].chompies[c], &w[1].chompies[c]);
            if b.beh.as_deref() == Some("alerted") {
                if a.beh.as_deref() != Some("alerted") {
                    in_wait = Some(w[1].k);
                }
                if a.substate < 2 && b.substate >= 2 {
                    if let Some(s) = in_wait.take() {
                        waits.push((w[1].k - s) as f32 / 60.0);
                    }
                    in_notice = Some(w[1].k);
                }
            }
            if a.beh.as_deref() == Some("alerted")
                && b.beh.as_deref() == Some("battle")
                && let Some(s) = in_notice.take()
            {
                notice.push((w[1].k - s) as f32 / 60.0);
            }
            if b.beh.as_deref() == Some("battle") && a.beh.as_deref() == Some("battle") {
                let across = Vec3::new(b.pos.x - a.pos.x, 0.0, b.pos.z - a.pos.z);
                let speed = ft(across.length()) * 60.0;
                top_speed = top_speed.max(speed);
                let turn = ((b.facing - a.facing + 180.0).rem_euclid(360.0) - 180.0).abs();
                top_turn = top_turn.max(turn);
            }
        }
        // Its nips: entry distance, steps by state tracking, end.
        let mut cur: Option<NipSoFar> = None;
        for w in snaps.windows(2) {
            let (a, b) = (&w[0].chompies[c], &w[1].chompies[c]);
            if b.beh.as_deref() == Some("attack - nip") && a.beh.as_deref() != Some("attack - nip")
            {
                cur = Some((w[1].k, middles(b.pos, w[1].hero), Vec::new(), 0.0));
                let across = Vec3::new(b.pos.x - a.pos.x, 0.0, b.pos.z - a.pos.z);
                let still = (w[1].hero - w[0].hero).length() < 1e-4;
                entries.push((
                    middles(b.pos, w[1].hero),
                    still && ft(across.length()) * 60.0 > 5.0,
                ));
            }
            if let Some((_, _, steps, lunge)) = cur.as_mut() {
                if b.beh.as_deref() == Some("attack - nip") {
                    if steps.last().is_none_or(|l| l.0 != b.state) {
                        steps.push((b.state, w[1].k));
                    }
                    if b.state == 3 {
                        let d = ft((b.pos - a.pos).length()) * 60.0;
                        *lunge = lunge.max(d);
                    }
                } else {
                    let (start, dist, steps, lunge) = cur.take().unwrap();
                    nips.push((c, start, w[1].k, dist, steps, lunge));
                }
            }
        }
    }
    println!("alerted waits {waits:?} s; notices {notice:?} s");
    println!("battle: top speed {top_speed:.2} ft/s, top turn {top_turn:.2} deg a frame");
    for n in &nips {
        println!(
            "{} nip F0+{}..F0+{}: entered at {:.2} ft between middles; steps {:?}; lunge top {:.1} ft/s",
            CHOMPIES[n.0], n.1, n.2, n.3, n.4, n.5
        );
    }
    assert!(
        !waits.is_empty() && waits.iter().all(|w| (0.0..=0.51).contains(w)),
        "{waits:?}"
    );
    // AH.10's notice is 0.748-0.769 s; the Chompy's clips end a frame or
    // two later here than in the original (AH.55), so a frame's slack.
    assert!(
        !notice.is_empty() && notice.iter().all(|n| (0.74..=0.79).contains(n)),
        "{notice:?}"
    );
    assert!(top_speed <= 20.0 + 0.05 && top_speed > 15.0, "{top_speed}");
    assert!((11.9..=12.0 + 1e-3).contains(&top_turn), "{top_turn}");
    assert!(!nips.is_empty());
    // Walking in, a nip starts at AH.10's 13.9-15.5 ft between middles (the
    // review's reading of the 20 `still` drives, 102 entries).
    let walked: Vec<f32> = entries.iter().filter(|e| e.1).map(|e| e.0).collect();
    println!("nips entered walking in at {walked:?} ft");
    assert!(
        !walked.is_empty() && walked.iter().all(|d| (13.9..=15.6).contains(d)),
        "{walked:?}"
    );
    // The lunge's top speed, AH.10's 33.7-34.2 ft/s (35 set, slowed by the
    // velocity model).
    let lunge = nips.iter().map(|n| n.5).fold(0.0f32, f32::max);
    assert!((33.0..=34.2).contains(&lunge), "lunge top {lunge}");
    // A nip's steps: 1 windup, 2 hold, 3 lunge, 10 recovery, each counted
    // to the step the next starts (AH.10's recordings by the review's
    // reading: 18-19 frames, 18-19, up to 0.419 s at 34 ft/s, 32-33 frames
    // to the nip's last frame, 0.530-0.561 s; AH.10's note's recovery,
    // 0.548-0.578 s, counts to 'battle', a frame more).
    for n in &nips {
        let steps = &n.4;
        let dur = |s: i32| {
            let i = steps.iter().position(|x| x.0 == s)?;
            let end = steps.get(i + 1).map_or(n.2, |x| x.1);
            Some((end - steps[i].1) as f32 / 60.0)
        };
        let (w, h, l, r) = (dur(1), dur(2), dur(3), dur(10));
        println!("  windup {w:?} hold {h:?} lunge {l:?} recovery {r:?}");
        // AH.10's windup 0.298-0.319 s; two frames' slack (AH.55).
        if let Some(w) = w {
            assert!((0.29..=0.34).contains(&w), "windup {w}");
        }
        if let Some(h) = h {
            assert!((0.29..=0.33).contains(&h), "hold {h}");
        }
        if let Some(l) = l {
            assert!(l <= 0.43, "lunge {l}");
        }
        // The recovery waits out its clip (slot 25, 0.567 s) and ends 1-2
        // frames after the original's, as the windup does (AH.55).
        if let Some(r) = r {
            assert!((0.53..=0.57).contains(&r), "recovery {r}");
        }
    }
    // The next windup 2 s or more after a nip ends.
    for (c, name) in CHOMPIES.iter().enumerate() {
        let mine: Vec<_> = nips.iter().filter(|n| n.0 == c).collect();
        for w in mine.windows(2) {
            let gap = (w[1].1 - w[0].2) as f32 / 60.0;
            println!("  {name} next windup {gap:.3} s after");
            assert!(gap >= 1.99, "{gap}");
        }
    }
    // Bites: each a hurt of 11 to 14; what standing still to F0 + 900 cost.
    let mut bites = Vec::new();
    for w in snaps.windows(2) {
        let lost = w[0].health - w[1].health;
        if lost > 0.0 {
            bites.push((w[1].k, lost));
        }
    }
    let cost = snaps[0].health - snaps.last().unwrap().health;
    println!("bites {bites:?}; standing still to F0 + 900 cost {cost}");
    // AH.10: standing still to F0 + 900 cost 73-104 over its 20 drives.
    assert!(!bites.is_empty(), "no bite landed");
    assert!((73.0..=104.0).contains(&cost), "{cost}");
    let (limits, read) = ah10_limits();
    assert_eq!(read, 40);
    let stalled = stalls(&snaps, &limits);
    println!("stalls against AH.10's {read} drives: {stalled:?}");
    assert!(stalled.is_empty());
    // A bite takes 11-14, two in one step 22-28 (AH.10's 26 and 25).
    assert!(
        bites
            .iter()
            .all(|b| (11.0..=14.0).contains(&b.1) || (22.0..=28.0).contains(&b.1)),
        "{bites:?}"
    );
    println!("load after: {}", load());
}

/// AH.10's limits for the stall detector: over its recordings
/// (`ah10-main-1792553632`'s 20 `still` and 20 `fight` drives), each stay
/// of a Chompy's or Norticus' in one behaviour with a change on both
/// sides, its behaviour's longest by the names each drive's `chain.json`
/// resolves, half a frame over for counting in whole frames; and how many
/// drives it read.
fn ah10_limits() -> (Vec<StallLimit>, usize) {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR").expect("GUAC_CAPTURE_DIR: AH.10's recordings");
    let main = Path::new(&root).join("BLUS30968/ah10-main-1792553632");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&main)
        .unwrap_or_else(|e| panic!("{}: {e}", main.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("chain-records.json").is_file())
        .collect();
    dirs.sort();
    let json = |p: PathBuf| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
    };
    let mut longest: BTreeMap<(String, String), i64> = BTreeMap::new();
    for dir in &dirs {
        let chain = json(dir.join("chain.json"));
        let names = &chain["resolved"]["behaviours"];
        let records = json(dir.join("chain-records.json"));
        let mut stays: BTreeMap<String, Vec<(Option<String>, i64)>> = BTreeMap::new();
        for rec in records.as_array().into_iter().flatten() {
            let frame = rec["game_frame"].as_i64().unwrap();
            for it in rec["items"].as_array().into_iter().flatten() {
                if !matches!(it["kind"].as_str(), Some("chompy" | "norticus")) {
                    continue;
                }
                let b = it["behaviour"].as_i64().filter(|&a| a != 0).map(|a| {
                    names[a.to_string()]
                        .as_str()
                        .unwrap_or_else(|| panic!("{}: behaviour {a} unnamed", dir.display()))
                        .to_lowercase()
                });
                let s = stays
                    .entry(it["placement"].as_str().unwrap().to_owned())
                    .or_default();
                if s.last().is_none_or(|l| l.0 != b) {
                    s.push((b, frame));
                }
            }
        }
        for (placement, s) in stays {
            for w in s.windows(2).skip(1) {
                if let Some(b) = &w[0].0 {
                    let e = longest.entry((placement.clone(), b.clone())).or_insert(0);
                    *e = (*e).max(w[1].1 - w[0].1);
                }
            }
        }
    }
    let limits = longest
        .into_iter()
        .map(|((placement, behaviour), frames)| StallLimit {
            placement,
            behaviour,
            longest_stay: (frames as f32 + 0.5) / 60.0,
        })
        .collect();
    (limits, dirs.len())
}

/// The stall detector over a run's snaps: each Chompy's and Norticus'
/// behaviour each step against AH.10's longest stays.
fn stalls(snaps: &[Snap], limits: &[StallLimit]) -> Vec<Stall> {
    let mut detector = StallDetector::new(limits.iter().cloned());
    let mut out = Vec::new();
    for s in snaps {
        let now = s.k as f32 / 60.0;
        for (c, ch) in s.chompies.iter().enumerate() {
            let b = if ch.gone { None } else { ch.beh.as_deref() };
            out.extend(detector.observe(now, CHOMPIES[c], b));
        }
        let b = if s.norticus.2 {
            None
        } else {
            s.norticus.0.as_deref()
        };
        out.extend(detector.observe(now, NORTICUS, b));
    }
    out
}

/// AH.10's `fight` from anchor B (a lob, South, at F0 + 90 and an
/// eruption, West, at F0 + 160, his face toward `Enemy_Chompy(2)`), an
/// eruption every 2 s from F0 + 700 for the fourth, to the end of
/// Norticus' part: each one beaten gone 0.8 s on, Norticus' scene the
/// frame the last is gone with the hero set before him, the controller
/// removed, then (the scene's end set 5 s in, the cutscene manager being
/// off the whitelist) Norticus walking into the cave mouth in 2 s. Strict
/// throughout, and the stall detector clean against AH.10's drives.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.10's recordings; run on the Mac"]
fn the_fight_ends_with_norticus_walking_into_the_cave() {
    println!("load before: {}", load());
    let (limits, read) = ah10_limits();
    println!("AH.10's {read} drives: limits {limits:?}");
    assert_eq!(read, 40);
    let started = Instant::now();
    let (run, snaps) = fight(Script {
        lob: Some(90),
        erupt: Some(160),
        erupt_every: Some(700),
        scene_end: true,
        steps: 1450,
    });
    println!("{} snaps in {:.2?}", snaps.len(), started.elapsed());
    let mut last_gone = None;
    for (c, name) in CHOMPIES.iter().enumerate() {
        let st = stays(&snaps, c);
        println!(
            "{}: {:?}",
            name,
            st.iter()
                .map(|s| (s.0.clone(), s.1, s.2))
                .collect::<Vec<_>>()
        );
        let death = st
            .iter()
            .find(|s| s.0.as_deref() == Some("death"))
            .unwrap_or_else(|| panic!("{} beaten", name));
        let gone = death.2.expect("gone");
        println!(
            "  {} beaten at F0 + {}, gone {} steps ({:.3} s) on",
            name,
            death.1,
            gone - death.1,
            (gone - death.1) as f32 / 60.0
        );
        // AH.10: 0.80-0.83 s (48-50 frames).
        assert!((48..=50).contains(&(gone - death.1)), "{}", gone - death.1);
        last_gone = last_gone.max(Some(gone));
    }
    let last_gone = last_gone.unwrap();
    let norticus: Vec<(Option<String>, i64)> = snaps.iter().fold(Vec::new(), |mut v, s| {
        let b = if s.norticus.2 {
            Some("(gone)".to_owned())
        } else {
            s.norticus.0.clone()
        };
        if v.last().is_none_or(|l: &(Option<String>, i64)| l.0 != b) {
            v.push((b, s.k));
        }
        v
    });
    println!("Norticus: {norticus:?}");
    let at = |name: &str| {
        norticus
            .iter()
            .find(|n| n.0.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("Norticus' {name}: {norticus:?}"))
            .1
    };
    let (go, advance, end) = (at("cutscene go"), at("advance with player"), at("end"));
    let controller_gone = snaps
        .iter()
        .find(|s| s.k > 0 && s.controller.is_none())
        .expect("the controller removed")
        .k;
    let snap = |k: i64| snaps.iter().find(|s| s.k == k).unwrap();
    for w in snaps.windows(2) {
        if (w[1].hero - w[0].hero).length() > 1.0 {
            println!(
                "the hero set at F0 + {}: {:?} -> {:?}",
                w[1].k, w[0].hero, w[1].hero
            );
        }
    }
    let hero = snap(go + 1).hero;
    let mouth = snap(end).norticus.1;
    // His walk from 'advance with player' (AH.10: moving 9 frames in,
    // gaining 1.33 ft/s a frame to his velocity's 30 ft/s; 'End' 121 frames
    // in at x 15.80-15.85, z -29.88 to -29.89).
    let walk: Vec<f32> = (advance..end)
        .map(|k| {
            let (a, b) = (snap(k).norticus.1, snap(k + 1).norticus.1);
            ft(Vec3::new(b.x - a.x, 0.0, b.z - a.z).length()) * 60.0
        })
        .collect();
    let first = walk.iter().position(|v| *v > 0.01);
    assert_eq!(first, Some(8), "moving the 9th step in");
    println!(
        "Norticus' walk: moving {first:?} steps in, then {:?} ft/s, top {:.2}",
        first.map(|f| walk[f..(f + 8).min(walk.len())].to_vec()),
        walk.iter().copied().fold(0.0f32, f32::max)
    );
    println!(
        "last gone F0 + {last_gone}; the controller removed F0 + {controller_gone}; 'cutscene go' \
         F0 + {go}, the hero at ({:.3}, {:.3}) a step on; 'advance with player' {} steps on; 'end' {} \
         steps ({:.3} s) after that at ({:.3}, {:.3}); gone {:?}",
        hero.x,
        hero.z,
        advance - go,
        end - advance,
        (end - advance) as f32 / 60.0,
        mouth.x,
        mouth.z,
        norticus
            .iter()
            .find(|n| n.0.as_deref() == Some("(gone)"))
            .map(|n| n.1 - end)
    );
    // AH.10: 'Cutscene Go' the frame the last is gone, the controller
    // removed the next (the tick order the placements woke in); the hero
    // set as the scene's first tick runs, the step after it's current.
    assert_eq!(go, last_gone);
    assert_eq!(controller_gone, last_gone + 1);
    let hero = snap(go + 1).hero;
    assert!(
        (hero.x - 0.94).abs() < 0.01 && (hero.z + 17.65).abs() < 0.01,
        "{hero:?}"
    );
    assert_eq!(advance - go, 300, "the scene's end set 5 s in");
    // AH.10: 'End' 121 frames on in all 20 fights, at x 15.80-15.85 and z
    // -29.880 to -29.894; his gone 0.47-0.50 s after it waits on the slide
    // of his alpha over 0.5 s, which lands at once here (AH.49).
    assert_eq!(end - advance, 121);
    assert!((15.79..=15.86).contains(&mouth.x), "{mouth:?}");
    assert!((-29.90..=-29.87).contains(&mouth.z), "{mouth:?}");
    let stalled = stalls(&snaps, &limits);
    println!("stalls: {stalled:?}");
    assert!(stalled.is_empty());
    let woken: Vec<&str> = run.world.woken.iter().map(|w| w.name.as_str()).collect();
    println!("woken off the whitelist: {woken:?}; load after: {}", load());
}

/// The members the fight reaches from the join to the end of Norticus'
/// part (the run above, 1,930 steps past the join, strict: it halts on any
/// member it asks
/// a value of that isn't modelled): each one asked registered, the rest
/// reached only on a path through nothing; the count against AH.34's
/// ceiling.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_members_the_fight_reaches_stay_under_the_ceiling() {
    println!("load before: {}", load());
    let (run, snaps) = fight(Script {
        lob: Some(90),
        erupt: Some(160),
        erupt_every: Some(700),
        scene_end: true,
        steps: 1450,
    });
    assert!(snaps.len() > 600);
    let name = |m: &MemberId| run.vm.config().members.name(*m);
    let registered = |m: &MemberId| {
        INIT_CHAIN
            .iter()
            .chain(HERO_MEMBERS)
            .chain(CREATURES)
            .any(|r| MemberId::named(r.namespace, r.name) == *m)
            || guac_script::Builtin::of(*m).is_some()
    };
    let reached = run.vm.reached();
    let asked: Vec<&MemberId> = reached
        .iter()
        .filter(|m| run.vm.uses().contains_key(*m))
        .collect();
    let mut through_nothing: Vec<String> = reached
        .iter()
        .filter(|m| !run.vm.uses().contains_key(*m) && !registered(m))
        .map(name)
        .collect();
    through_nothing.sort();
    let mut theirs: Vec<String> = asked
        .iter()
        .filter(|m| {
            CREATURES
                .iter()
                .any(|r| MemberId::named(r.namespace, r.name) == ***m)
        })
        .map(|m| name(m))
        .collect();
    theirs.sort();
    let past: Vec<String> = asked
        .iter()
        .filter(|m| !registered(m))
        .map(|m| name(m))
        .collect();
    println!(
        "{} steps; members reached {} ({} the VM's own), {} asked a value, {} of them the \
         fight's new rows {theirs:?}; reached only through nothing: {through_nothing:?}",
        run.steps,
        reached.len(),
        reached
            .iter()
            .filter(|m| guac_script::Builtin::of(**m).is_some())
            .count(),
        asked.len(),
        theirs.len()
    );
    println!("load after: {}", load());
    assert!(past.is_empty(), "{past:?}");
    assert!(
        reached.len() <= 324,
        "past AH.34's ceiling: {}",
        reached.len()
    );
}

/// A development probe: each clip's loop length, frames and frame time on
/// the Chompy's palette (slots 11, 20-25) and Eruptor's (20, 21, 32).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_clip_lengths() {
    let dump = dump();
    let usr = dump.join("PS3_GAME/USRDIR");
    let part: Vec<Vec<u8>> = ARCHIVES
        .iter()
        .map(|(a, _)| guac_extract::scripts::entry(&usr, a).unwrap())
        .collect();
    let igz = Igz::parse(&part[LEVEL as usize]).unwrap();
    let palettes = levels::palettes(&igz, levels::LEVELS[0], identity())
        .unwrap()
        .ir;
    let show = |p: &guac_ir::palette::Palette, slots: &[u16]| {
        for &s in slots {
            if let Some(e) = p.get(s)
                && let guac_ir::palette::SlotEntry::Clip { clip, .. } = &e.entry
            {
                println!(
                    "{} slot {s}: {} duration {} frames {} frame {} ({} x frame = {}, (frames-1) x frame = {})",
                    p.owner,
                    clip.name,
                    clip.duration,
                    clip.frames,
                    clip.frame_duration,
                    clip.frames,
                    clip.frames as f32 * clip.frame_duration,
                    (clip.frames as f32 - 1.0) * clip.frame_duration
                );
            }
        }
    };
    for p in &palettes.palettes {
        if p.owner.contains("ChompyUndead") {
            show(p, &[3, 6, 11, 19, 20, 21, 22, 23, 24, 25]);
        }
    }
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).unwrap();
    let read =
        guac_extract::models::read(&part[3], "009_Eruptor", identity(), Some(&globals)).unwrap();
    show(&read.palette, &[20, 21, 32]);
}
