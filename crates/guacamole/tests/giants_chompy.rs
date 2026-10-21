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
//! AH.19's are the hurt and kill chain's: bites pushing him by his
//! resistance and his hurt reaction, a turned-away bite, a Lava Lob's kill,
//! a stunned Chompy hit again (a lob staging and the disc's own `Combat -
//! Hurt Creature` on a schedule), a bite in his eruption and the chain's
//! rolls over 1,000 calls a case.
//!
//! Dev aids, not checks: `AH18_LIVE_TABLE=<aj-vm's live member table>`
//! answers every member the registry doesn't hold with a value cell of its
//! registered type and prints which the run reached; in a fight,
//! `AH19_TRACE=<steps from F0>` with `AH19_TRACE_OUT=<file outside the
//! tree>` writes that step's ops as script and position, and `AH19_BALLS`
//! prints what flies from F0 + 85 to F0 + 150.

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
use guac_script::members::{COMBAT, CREATURES, HERO as HERO_MEMBERS, INIT_CHAIN, LOOP};
use guac_script::{
    Generator, Halt, Handle, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role,
    Stall, StallDetector, StallLimit, Unit, Value, Vm, random_literal,
};
use guacamole::level_scene;
use guacamole::player_loop::PlayerLoop;
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
    /// The portal manager's tick as the player loop runs it (AH.20).
    player_loop: PlayerLoop,
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
        let slid = self.with_host(|vm, host| vm.slide(host))?;
        assert!(slid.stopped.is_empty(), "strict mode stops by halting");
        let joined = self.minion.is_some();
        let player_loop = self.player_loop.clone();
        self.with_host(|vm, host| player_loop.step(vm, host, joined))?;
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
    let player_loop = PlayerLoop::read(&program);
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
            player_loop,
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
    /// AH.10's `stun` staging (openspec AH.19): the four's health and full
    /// health set to 100 at F0, then South down a step and up a step 90
    /// times from F0 + 120.
    stun: bool,
    /// The four's health and full health set to 100 at F0 and nothing
    /// else (AH.10's `erupt_bit`).
    stun_health: bool,
    /// AH.11's pokes (openspec AH.19): his critical chance (`total crit`)
    /// and his armour set at F0.
    crit: Option<i32>,
    armour: Option<i32>,
    /// The save's experience multiplier set at F0 (openspec AH.20: the
    /// oracle's save reads 1.1, AH.4; a fresh save and guacamole 1).
    xp_multiplier: Option<f32>,
    /// His health set at F0 (openspec AH.20: AH.11's `defeat2` set it to 40
    /// or 60 for a bite or few to beat him).
    hero_health: Option<i32>,
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
    /// The hurt chain's numbers on it (openspec AH.19).
    hurt: Hurt,
    /// `$description.$creature info.$hp`.
    health: f32,
    /// Across the ground, m/s.
    speed: f32,
    /// How many its lunge has bitten (`actors hit`).
    bitten: usize,
}

/// What the hurt chain keeps on a creature's description (openspec AH.19,
/// the oracle harness's chain paths): its knockback's timer (the game time
/// it ends), resistance and last strength, its hurt reaction's mode (1 while
/// it runs) and the hurt state.
#[derive(Debug, Clone, Copy, Default)]
struct Hurt {
    knock_timer: f32,
    resist: f32,
    strength: f32,
    mode: i32,
    mode_timer: f32,
    state: i32,
    /// `$creature info.$invulnerable`.
    invulnerable: i32,
}

#[derive(Debug, Clone)]
struct Snap {
    /// Steps from the opening's start (AH.10's F0).
    k: i64,
    hero: Vec3,
    health: f32,
    /// His walk across the ground, m/s, and the hurt chain's numbers on him.
    hero_speed: f32,
    hero_hurt: Hurt,
    /// `combat control data`'s game timer: the clock a knockback's timer
    /// counts on.
    game_timer: f32,
    /// His player's `enemies defeated`.
    defeated: i32,
    /// His minion taken off (openspec AH.20: a defeated hero leaving).
    hero_gone: bool,
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
    /// The first player's joined data's `minion description` (his total
    /// critical chance, armour and the rest his description sums).
    fn minion_description(&mut self) -> Value {
        let g = self
            .vm
            .program()
            .global_named("portal control data")
            .unwrap();
        let pcd = self.vm.global(g).map(|v| self.vm.value(v)).unwrap();
        let joined = self
            .vm
            .field(pcd, "player joined data")
            .map(|v| self.vm.value(v))
            .unwrap();
        let first = self.vm.set(joined).unwrap()[0];
        let first = self.vm.value(first);
        self.vm
            .field(first, "minion description")
            .map(|v| self.vm.value(v))
            .unwrap()
    }

    /// `combat control data`'s game timer.
    fn game_timer(&self) -> f32 {
        let Some(g) = self.vm.program().global_named("combat control data") else {
            return 0.0;
        };
        let Some(c) = self.vm.global_peek(g) else {
            return 0.0;
        };
        self.vm
            .field(c, "game timer")
            .and_then(|v| self.vm.value(v).as_f32())
            .unwrap_or(0.0)
    }

    /// The hurt chain's numbers on a placement's `description`.
    fn hurt(&self, id: PlacementId) -> Hurt {
        let f = |fields: &[&str]| {
            path(self, id, "description", fields)
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
        };
        Hurt {
            knock_timer: f(&["knockback timer"]),
            resist: f(&["state tracking", "knockback current resist"]),
            strength: f(&["state tracking", "knockback last knock strength"]),
            mode: f(&["state tracking", "mode"]) as i32,
            mode_timer: f(&["state tracking", "mode timer"]),
            state: f(&["state"]) as i32,
            invulnerable: f(&["creature info", "invulnerable"]) as i32,
        }
    }

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
                    hurt: self.hurt(id),
                    health: path(self, id, "description", &["creature info", "hp"])
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0),
                    speed: Vec3::new(a.velocity.x, 0.0, a.velocity.z).length() / FEET_PER_METRE,
                    bitten: self
                        .vm
                        .local(id, "actors hit")
                        .and_then(|v| self.vm.set(self.vm.value(v)).map(<[Value]>::len))
                        .unwrap_or(0),
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
            hero_speed: {
                let v = self.hero().walk.velocity;
                Vec3::new(v.x, 0.0, v.z).length() / FEET_PER_METRE
            },
            hero_hurt: self.minion.map(|m| self.hurt(m)).unwrap_or_default(),
            game_timer: self.game_timer(),
            defeated: self
                .minion
                .and_then(|m| {
                    path(
                        self,
                        m,
                        "my Minion Description",
                        &[
                            "player joined data",
                            "player stat tracking",
                            "enemies defeated",
                        ],
                    )
                })
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0) as i32,
            hero_gone: self.minion.is_some_and(|m| self.vm.removed(m)),
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
            let m = run.minion.unwrap();
            if let Some(crit) = script.crit {
                let d = run.minion_description();
                assert!(run.vm.set_field(d, "total crit", Value::Int(crit)));
            }
            if let Some(hp) = script.hero_health {
                assert!(scripts::set_health(&mut run.vm, m, hp));
            }
            if let Some(mult) = script.xp_multiplier {
                let g = run
                    .vm
                    .program()
                    .global_named("combat control data")
                    .unwrap();
                let c = run.vm.global(g).map(|v| run.vm.value(v)).unwrap();
                assert!(
                    run.vm
                        .set_field(c, "accolade xp multiplier", Value::Float(mult))
                );
            }
            if let Some(armour) = script.armour {
                let c = path(&run, m, "description", &["creature info"]).unwrap();
                assert!(run.vm.set_field(c, "armor", Value::Int(armour)));
            }
            if script.stun || script.stun_health {
                for n in CHOMPIES {
                    let id = run.placement(n);
                    let d = run
                        .vm
                        .local(id, "description")
                        .map(|v| run.vm.value(v))
                        .unwrap();
                    let c = run
                        .vm
                        .field(d, "creature info")
                        .map(|v| run.vm.value(v))
                        .unwrap();
                    assert!(run.vm.set_field(c, "hp", Value::Int(100)));
                    let cp = run
                        .vm
                        .field(c, "character parameters")
                        .map(|v| run.vm.value(v))
                        .unwrap();
                    assert!(run.vm.set_field(cp, "hp max", Value::Int(100)));
                }
            }
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
        pad.actions[0] =
            at(script.lob) || script.stun && (120..300).contains(&f) && (f - 120) % 2 == 0;
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
        // A dev aid: `AH19_BALLS` prints what flies each step from F0 + 85
        // to F0 + 150 and where the Chompies stand.
        if std::env::var("AH19_BALLS").is_ok() && (85..150).contains(&f) {
            let flying: Vec<String> = run
                .world
                .clones()
                .into_iter()
                .filter_map(|h| {
                    let a = run.world.actor(h)?;
                    (a.launched && !a.removed)
                        .then(|| format!("{} {:.3}", run.world.name(h), a.position))
                })
                .collect();
            let chompies: Vec<String> = CHOMPIES
                .iter()
                .map(|n| format!("{n} {:.3}", run.at(n)))
                .collect();
            println!(
                "F0 + {f}: flying {flying:?}; hero {:.3}; {chompies:?}",
                run.hero().pivot
            );
        }
        // A dev aid: `AH19_TRACE=<steps from F0>` writes that step's ops,
        // each as script and position, to `AH19_TRACE_OUT`.
        let traced = std::env::var("AH19_TRACE")
            .ok()
            .and_then(|t| t.parse::<i64>().ok())
            == Some(f);
        if traced {
            run.vm.trace(Some(2_000_000));
        }
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        if traced {
            let program = run.vm.program_arc();
            let ops = run.vm.traced();
            run.vm.trace(None);
            let text: String = ops
                .iter()
                .map(|(s, at)| format!("{} {at}\n", program.scripts[*s as usize].name))
                .collect();
            let out = std::env::var("AH19_TRACE_OUT").expect("AH19_TRACE_OUT");
            std::fs::write(out, text).unwrap();
        }
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
    // AH.10: standing still to F0 + 900 cost 73-104 over its 20 drives,
    // four Chompies at him in each; 50 in `still_p1` (the opening a flip
    // late, four) and 89 in `still_p2` (two late, three). Three come at him
    // here (anchor B's wander for (2) isn't set), and since AH.19 a bite's
    // push carries him the original's 3 m clear, so it costs what three
    // make of it: 4-8 bites, 50-104 (the 22 drives' range).
    assert!(!bites.is_empty(), "no bite landed");
    assert!((4..=8).contains(&bites.len()), "{bites:?}");
    assert!((50.0..=104.0).contains(&cost), "{cost}");
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
/// resolves, a frame and a half over (whole frames, and a roll's top);
/// and how many
/// drives it read.
fn ah10_limits() -> (Vec<StallLimit>, usize) {
    ah10_limits_from(&["ah10-main-1792553632"], |_| true)
}

/// [`ah10_limits`] over the drives in AH.10's capture folders `folders`
/// that `keep` takes, by name (AH.19: the stun and eruption stagings give
/// a stunned Chompy's 'disabled').
fn ah10_limits_from(folders: &[&str], keep: impl Fn(&str) -> bool) -> (Vec<StallLimit>, usize) {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR").expect("GUAC_CAPTURE_DIR: AH.10's recordings");
    let mut dirs: Vec<PathBuf> = Vec::new();
    for f in folders {
        let main = Path::new(&root).join("BLUS30968").join(f);
        dirs.extend(
            std::fs::read_dir(&main)
                .unwrap_or_else(|e| panic!("{}: {e}", main.display()))
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("chain-records.json").is_file())
                .filter(|p| keep(&p.file_name().unwrap().to_string_lossy())),
        );
    }
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
            // A frame and a half over: whole frames counted, and a stay
            // with a roll in it can run a frame past the drives' longest
            // (AH.20: a Chompy's alert, its wait a roll up to 0.5 s, 76
            // steps against AH.10's longest 75 frames).
            longest_stay: (frames as f32 + 1.5) / 60.0,
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
        ..Script::default()
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
    // -29.880 to -29.894; his gone 0.47-0.50 s after it, the slide of his
    // alpha over 0.5 s to 0 (AH.19's slides over time; the review's reader:
    // 27-30 frames).
    assert_eq!(end - advance, 121);
    assert!((15.79..=15.86).contains(&mouth.x), "{mouth:?}");
    assert!((-29.90..=-29.87).contains(&mouth.z), "{mouth:?}");
    let gone = norticus
        .iter()
        .find(|n| n.0.as_deref() == Some("(gone)"))
        .map(|n| n.1 - end)
        .expect("he goes");
    assert!((27..=30).contains(&gone), "{gone}");
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
        ..Script::default()
    });
    assert!(snaps.len() > 600);
    let name = |m: &MemberId| run.vm.config().members.name(*m);
    let registered = |m: &MemberId| {
        INIT_CHAIN
            .iter()
            .chain(HERO_MEMBERS)
            .chain(CREATURES)
            .chain(COMBAT)
            .chain(LOOP)
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

/// A development probe (openspec AH.19): standing still from anchor B,
/// each step a bite lands or a push runs, his health, walk and the hurt
/// chain's numbers on him; `AH19_LOB` adds AH.10's lob and eruption.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_the_pushes() {
    println!("load before: {}", load());
    let lob = std::env::var("AH19_LOB").is_ok();
    let stun = std::env::var("AH19_STUN").is_ok();
    let number = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<i32>().ok());
    let (run, snaps) = fight(Script {
        lob: lob.then_some(90),
        erupt: lob.then_some(160),
        stun,
        crit: number("AH19_CRIT"),
        armour: number("AH19_ARMOUR"),
        steps: if stun { 600 } else { 900 },
        ..Script::default()
    });
    for (i, w) in snaps.windows(2).enumerate() {
        let (a, b) = (&w[0], &w[1]);
        if b.health != a.health {
            println!("F0 + {}: his health {} -> {}", b.k, a.health, b.health);
            for s in &snaps[i..(i + 12).min(snaps.len())] {
                println!(
                    "   F0 + {:4}: speed {:7.3} res {:7.3} kt {:9.4} ks {:6.2} mode {} mt {:9.4} state {}",
                    s.k,
                    s.hero_speed,
                    s.hero_hurt.resist,
                    s.hero_hurt.knock_timer,
                    s.hero_hurt.strength,
                    s.hero_hurt.mode,
                    s.hero_hurt.mode_timer,
                    s.hero_hurt.state
                );
            }
        }
        for (c, name) in CHOMPIES.iter().enumerate() {
            let (x, y) = (&a.chompies[c], &b.chompies[c]);
            if y.health != x.health {
                println!(
                    "  {} F0 + {}: health {} -> {}",
                    name, b.k, x.health, y.health
                );
                for s in &snaps[i..(i + 13).min(snaps.len())] {
                    let y = &s.chompies[c];
                    println!(
                        "     F0 + {:4}: speed {:7.3} res {:7.3} kt {:9.4} ks {:6.2} mode {} mt {:9.4} state {} beh {:?}",
                        s.k,
                        y.speed,
                        y.hurt.resist,
                        y.hurt.knock_timer,
                        y.hurt.strength,
                        y.hurt.mode,
                        y.hurt.mode_timer,
                        y.hurt.state,
                        y.beh
                    );
                }
            }
        }
    }
    let registered = |m: &MemberId| {
        INIT_CHAIN
            .iter()
            .chain(HERO_MEMBERS)
            .chain(CREATURES)
            .chain(COMBAT)
            .any(|r| MemberId::named(r.namespace, r.name) == *m)
            || guac_script::Builtin::of(*m).is_some()
    };
    let mut past: Vec<String> = run
        .vm
        .uses()
        .iter()
        .filter(|(m, _)| !registered(m))
        .map(|(m, (r, w))| format!("{} r{r} w{w}", run.vm.config().members.name(*m)))
        .collect();
    past.sort();
    println!("asked, unregistered: {past:#?}");
    println!("load after: {}", load());
}

// ---- AH.19: the hurt and kill chain on the VM ----------------------------

/// A push as the snaps show it: the step its knockback began (the timer
/// set or set again), its knock time (the timer less the game time when it
/// was set: the step's, for a hurt in a placement's tick after the UI's
/// moved it; for one in the projectile manager, the step before's, the
/// native player loop running ahead of the UI's tick here, AH.57), the
/// resistance before and after, and the speed each step from it until the
/// timer is cleared.
#[derive(Debug, Clone)]
struct Push {
    k: i64,
    knock: f32,
    /// The knock time against the step before's game time.
    knock_before: f32,
    resist_before: f32,
    resist_after: f32,
    speeds: Vec<f32>,
}

/// The pushes in `snaps` of whatever `of` reads from a step.
fn pushes(snaps: &[Snap], of: impl Fn(&Snap) -> (Hurt, f32)) -> Vec<Push> {
    let mut out: Vec<Push> = Vec::new();
    for (i, w) in snaps.windows(2).enumerate() {
        let ((a, _), (b, _)) = (of(&w[0]), of(&w[1]));
        if b.knock_timer > 0.0 && b.knock_timer != a.knock_timer {
            let speeds = snaps[i + 1..]
                .iter()
                .take_while(|s| of(s).0.knock_timer == b.knock_timer)
                .map(|s| of(s).1)
                .collect();
            out.push(Push {
                k: w[1].k,
                knock: b.knock_timer - w[1].game_timer,
                knock_before: b.knock_timer - w[0].game_timer,
                resist_before: a.resist,
                resist_after: b.resist,
                speeds,
            });
        }
    }
    out
}

/// Each step his health fell: the step and what it cost.
fn bites_of(snaps: &[Snap]) -> Vec<(i64, f32)> {
    snaps
        .windows(2)
        .filter(|w| w[1].health < w[0].health)
        .map(|w| (w[1].k, w[0].health - w[1].health))
        .collect()
}

/// A step's speed `d` m/s off what an 80 ft/s push slowed by `decel` ft/s²
/// for `frames` frames reads.
fn slowed(frames: usize, decel: f32) -> f32 {
    (80.0 - decel * frames as f32 / 60.0) / FEET_PER_METRE
}

/// Bites push him by his resistance, as AH.10 recorded (combat: Knockback,
/// No grace after a hit): 0.13125 s at 24.4 m/s with none, 0.125 with
/// some, his resistance up 12.5 a bite and gone 1.63-1.65 s after a lone
/// one; his hurt reaction 0.6 s, the push slowing 80 ft/s² while it runs
/// (his deceleration put back by it) and eased from nothing to 600 ft/s²
/// over the push without it (a bite inside his reaction's window). The push
/// moves him from the step after the bite, 8 steps as AH.10's 8 frames
/// from the bite's own: the mover update ticks before the Chompies here
/// (AH.57).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn bites_push_him_by_his_resistance_and_hurt_him_for_0_6_s() {
    println!("load before: {}", load());
    let (_, snaps) = fight(Script {
        steps: 900,
        ..Script::default()
    });
    let bites = bites_of(&snaps);
    let pushes = pushes(&snaps, |s| (s.hero_hurt, s.hero_speed));
    println!("bites {bites:?}");
    assert!(bites.len() >= 4, "{bites:?}");
    for (k, cost) in &bites {
        assert!((11.0..=14.0).contains(cost), "F0 + {k}: {cost}");
    }
    let mut lone = 0;
    let mut reacted = 0;
    let mut eased = 0;
    for p in &pushes {
        println!(
            "push at F0 + {}: knock {:.5} s, resistance {:.3} -> {:.3}, {} steps: {:?}",
            p.k,
            p.knock,
            p.resist_before,
            p.resist_after,
            p.speeds.len(),
            p.speeds
        );
        assert!(bites.iter().any(|b| b.0 == p.k), "a push with no bite");
        let want = ((25.0 - p.resist_before) / 100.0 * 0.525).max(0.125);
        assert!((p.knock - want).abs() < 2e-4, "{} against {want}", p.knock);
        if p.resist_before == 0.0 {
            assert!((p.knock - 0.13125).abs() < 2e-4);
        } else {
            assert!((p.knock - 0.125).abs() < 2e-4);
        }
        // His resistance rises by half the strength, 12.5, on what the
        // combat manager's falling left of it that step.
        let rose = p.resist_after - p.resist_before;
        if p.resist_before == 0.0 {
            assert!((rose - 12.5).abs() < 1e-3, "{rose}");
        } else {
            assert!((12.3..=12.501).contains(&rose), "{rose}");
        }
        // Still the step of the bite, moving from the next.
        let at = snaps.iter().position(|s| s.k == p.k).unwrap();
        assert!(snaps[at].hero_speed < 1e-3);
        let s = &p.speeds[1..];
        let reaction = snaps[at + 1].hero_hurt.mode == 1 && snaps[at].hero_hurt.mode == 0;
        // Into a wall a push stops after a frame (AH.10): only open-ground
        // pushes are held to their profile.
        if s.iter().filter(|v| **v > 1.0).count() < 7 {
            continue;
        }
        assert_eq!(s.len(), 8, "8 steps moving");
        if reaction {
            reacted += 1;
            // The first step slows by the push's slide (its first 60th of
            // 0.13125 s of x²: 9.7 ft/s²), then 80 ft/s² a step: AH.10's
            // 23.956-21.535 m/s from its second frame, each a frame early.
            assert!((s[0] - slowed(1, 600.0 * (1.0 / 7.875f32).powi(2))).abs() < 0.01);
            // The ground's slope under him can take a little off the
            // across-the-ground speed of a step: within 0.2 m/s.
            for (j, v) in s.iter().enumerate().skip(1) {
                let want = s[0] - (80.0 / FEET_PER_METRE) * j as f32 / 60.0;
                assert!((v - want).abs() < 0.2, "step {j}: {v} against {want}");
            }
            // At 80 ft/s² a step for at least five steps: from its second
            // step, or from its third where the reaction's write lands a
            // step late (AH.20's run, F0 + 801, a second Chompy's mover in
            // the update that step: AH.57's tick order).
            let level = s
                .windows(2)
                .filter(|w| ((w[0] - w[1]) - (80.0 / FEET_PER_METRE) / 60.0).abs() < 0.01)
                .count();
            assert!(level >= 5, "{s:?}");
        } else {
            eased += 1;
            // AH.10's 24.330, 24.121, 23.664, 22.794, 21.374, 19.482,
            // 16.939 m/s: the decel slid in as x².
            for w in s.windows(3) {
                assert!(w[0] - w[1] < w[1] - w[2] + 1e-3, "{s:?}");
            }
            assert!((s[0] - 24.33).abs() < 0.02, "{}", s[0]);
            assert!((s[6] - 16.9).abs() < 0.3, "{}", s[6]);
        }
        // A lone push: nothing more for 1.7 s, the run lasting that long.
        let last = snaps.last().map_or(0, |s| s.k);
        if p.resist_before == 0.0
            && p.k + 102 <= last
            && !pushes.iter().any(|q| q.k > p.k && q.k <= p.k + 102)
        {
            let gone = snaps[at..]
                .iter()
                .find(|s| s.hero_hurt.resist == 0.0)
                .map(|s| (s.k - p.k) as f32 / 60.0);
            println!("  lone: resistance gone {gone:?} s on");
            let gone = gone.expect("its resistance falls back");
            assert!((1.62..=1.66).contains(&gone), "{gone}");
            lone += 1;
        }
    }
    assert!(lone > 0, "a lone push");
    assert!(reacted > 0, "a push in his reaction");
    // His hurt reaction: 0.6 s of his script's time (AH.10: 0.581-0.585 s,
    // 36-37 frames), none started by a bite inside it or within about a
    // second of its end.
    let mut runs: Vec<(i64, i64)> = Vec::new();
    for w in snaps.windows(2) {
        if w[1].hero_hurt.mode == 1 && w[0].hero_hurt.mode != 1 {
            runs.push((w[1].k, i64::MAX));
        }
        if w[1].hero_hurt.mode != 1
            && w[0].hero_hurt.mode == 1
            && let Some(r) = runs.last_mut()
        {
            r.1 = w[1].k;
        }
    }
    println!("his reactions {runs:?}; eased pushes {eased}");
    for &(a, b) in &runs {
        if b == i64::MAX {
            continue;
        }
        assert!((35..=37).contains(&(b - a)), "{a}..{b}");
    }
    for &(k, _) in &bites {
        let inside = runs.iter().any(|&(a, b)| k >= a && k < b);
        let soon = runs
            .iter()
            .any(|&(_, b)| b != i64::MAX && k >= b && k < b + 58);
        let started = runs.iter().any(|&(a, _)| a == k + 1);
        if inside || soon {
            assert!(!started, "a bite at F0 + {k} started his reaction");
        }
    }
    println!("load after: {}", load());
}

/// Armour turns a bite away whole (combat: Armour can turn a hit away):
/// with his armour at 200 a bite lands only on the roll's top, so standing
/// still bites reach him and take nothing, and nothing pushes him or starts
/// his reaction (AH.10's turned-away bites).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_turned_away_bite_neither_hurts_nor_pushes() {
    println!("load before: {}", load());
    let (_, snaps) = fight(Script {
        armour: Some(200),
        steps: 900,
        ..Script::default()
    });
    let bitten: usize = snaps
        .windows(2)
        .map(|w| {
            (0..4)
                .map(|c| {
                    w[1].chompies[c]
                        .bitten
                        .saturating_sub(w[0].chompies[c].bitten)
                })
                .sum::<usize>()
        })
        .sum();
    let bites = bites_of(&snaps);
    let pushes = pushes(&snaps, |s| (s.hero_hurt, s.hero_speed));
    println!(
        "{bitten} bites reached him; {bites:?} landed; pushes {}",
        pushes.len()
    );
    assert!(bitten >= 3, "bites reach him");
    assert_eq!(
        pushes.len(),
        bites.len(),
        "a push only with a bite that lands"
    );
    assert!(bites.len() < bitten, "most are turned away");
    // His reaction starts the step after a bite that lands, never else.
    assert!(
        snaps
            .windows(2)
            .filter(|w| !bites.iter().any(|b| b.0 == w[1].k - 1))
            .all(|w| !(w[1].hero_hurt.mode == 1 && w[0].hero_hurt.mode == 0)),
        "a reaction with no bite landing"
    );
    println!("load after: {}", load());
}

/// A Lava Lob beats the Chompies its burst reaches (creatures: One Lava
/// Lob; combat: A hit does its attack's damage): each 1 -> 0 at the step
/// it lands, 'death' then, gone 0.8 s on, his player credited each. Thrown
/// at F0 + 20, it lands among the four while they stand noticing him;
/// AH.10's at F0 + 90 lands short of where ours does (AH.58).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_lava_lob_beats_the_chompies_its_burst_reaches() {
    println!("load before: {}", load());
    let (_, snaps) = fight(Script {
        lob: Some(20),
        steps: 300,
        ..Script::default()
    });
    let hit = snaps
        .windows(2)
        .find(|w| (0..4).any(|c| w[1].chompies[c].health < w[0].chompies[c].health))
        .expect("the lob lands");
    let k = hit[1].k;
    let beaten: Vec<usize> = (0..4)
        .filter(|&c| hit[0].chompies[c].health == 1.0 && hit[1].chompies[c].health <= 0.0)
        .collect();
    println!(
        "the lob lands at F0 + {k}: {:?} beaten, defeated {} -> {}",
        beaten.iter().map(|&c| CHOMPIES[c]).collect::<Vec<_>>(),
        hit[0].defeated,
        hit[1].defeated
    );
    assert!(!beaten.is_empty());
    assert_eq!(hit[1].defeated - hit[0].defeated, beaten.len() as i32);
    for &c in &beaten {
        assert_eq!(hit[1].chompies[c].beh.as_deref(), Some("death"));
        // A lob does 8, 12 critical: either takes the 1.
        assert!(
            hit[1].chompies[c].health == -7.0
                || hit[1].chompies[c].health == -11.0
                || hit[1].chompies[c].health == 0.0,
            "{}",
            hit[1].chompies[c].health
        );
        let gone = snaps
            .iter()
            .find(|s| s.k > k && s.chompies[c].gone)
            .map(|s| s.k - k)
            .expect("gone");
        assert!((48..=50).contains(&gone), "{gone}");
    }
    println!("load after: {}", load());
}

/// A stunned Chompy hit again (AH.10's `stun` staging: the four's health
/// poked to 100, a lob every 20 steps): each lob hurts for 8 (12
/// critical); the first stuns it 0.6 s ('disabled'), a lob inside the stun
/// doesn't start it again, nor one within a second of its end or too weak
/// to push it; a lob pushes it 0.15 s from 24.4 m/s at the hit, slowing
/// (eased) to about 17 m/s at its ninth step. Our push runs a tenth step
/// (the combat manager's knockback check before the game timer moves here:
/// AH.57).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.10's recordings; run on the Mac"]
fn a_stunned_chompy_hit_again_stays_stunned_to_its_first_end() {
    println!("load before: {}", load());
    let (limits, read) =
        ah10_limits_from(&["ah10-main-1792553632", "ah10-staged-1792556887"], |n| {
            !n.starts_with("still_p") && !n.starts_with("fight_")
        });
    println!("AH.10's {read} drives");
    let (_, snaps) = fight(Script {
        stun: true,
        steps: 600,
        ..Script::default()
    });
    let mut hits = 0;
    let mut stuns = 0;
    for (c, name) in CHOMPIES.iter().enumerate() {
        let pushes = pushes(&snaps, |s| (s.chompies[c].hurt, s.chompies[c].speed));
        for (i, w) in snaps.windows(2).enumerate() {
            let (a, b) = (&w[0].chompies[c], &w[1].chompies[c]);
            if b.health >= a.health {
                continue;
            }
            hits += 1;
            let k = w[1].k;
            let lost = a.health - b.health;
            assert!(lost == 8.0 || lost == 12.0, "{} F0 + {k}: {lost}", name);
            let push = pushes.iter().find(|p| p.k == k);
            let stunned = b.hurt.mode == 1 && a.hurt.mode != 1;
            let since_end = snaps[..=i]
                .windows(2)
                .rev()
                .find(|x| x[0].chompies[c].hurt.mode == 1 && x[1].chompies[c].hurt.mode != 1)
                .map(|x| (k - x[1].k) as f32 / 60.0);
            println!(
                "{} F0 + {k}: -{lost}, resist {:.2}, push {:?}, stunned {stunned} (in one {}, {since_end:?} s after the last)",
                name,
                a.hurt.resist,
                push.map(|p| (p.knock_before, p.speeds.len())),
                a.hurt.mode == 1
            );
            if a.hurt.mode == 1 {
                assert!(!stunned);
                assert_eq!(b.hurt.mode_timer, a.hurt.mode_timer, "not started again");
            } else if since_end.is_some_and(|t| t < 1.0) || a.hurt.resist >= 25.0 {
                assert!(!stunned, "within 1 s of its end or too weak to push it");
            } else {
                assert!(stunned);
                assert_eq!(b.beh.as_deref(), Some("disabled"));
                stuns += 1;
                let end = snaps[i + 1..]
                    .iter()
                    .find(|s| s.chompies[c].hurt.mode != 1)
                    .map(|s| s.k - k)
                    .expect("its stun ends");
                assert!((35..=37).contains(&end), "{end}");
            }
            if a.hurt.resist < 25.0 {
                let p = push.expect("a push");
                let want = ((25.0 - a.hurt.resist) / 100.0 * 0.6).max(0.125);
                // A resistance in its last falling step is gone by the hit:
                // the combat manager lets it fall before the projectile
                // manager lands the hit, both ahead of the step (AH.20's
                // run, 0.12 the step before).
                let gone = 25.0 / 100.0 * 0.6;
                assert!(
                    (p.knock_before - want).abs() < 2e-4
                        || a.hurt.resist < 0.5 && (p.knock_before - gone).abs() < 2e-4,
                    "{} against {want}",
                    p.knock_before
                );
                if a.hurt.resist == 0.0 && p.speeds.iter().all(|v| *v > 1.0) {
                    assert!((p.speeds[0] - 24.384).abs() < 1e-3);
                    assert_eq!(p.speeds.len(), 10, "{:?}", p.speeds);
                    assert!((16.4..=17.2).contains(&p.speeds[8]), "{:?}", p.speeds);
                }
            } else {
                assert!(push.is_none(), "too weak to push it");
            }
        }
    }
    println!("{hits} hits, {stuns} stuns");
    // How many lobs land rides on AH.58's aim and the session's draws
    // (AH.19's run 6, AH.20's 4 once the route's coins drew).
    assert!(hits >= 4 && stuns >= 2);
    let stalled = stalls(&snaps, &limits);
    assert!(stalled.is_empty(), "{stalled:?}");
    println!("load after: {}", load());
}

/// In his eruption (combat: Armour can turn a hit away): a bite in its
/// wind-up hurts him without a push or his reaction; one once it's out
/// takes nothing (AH.10's `erupt_bit`: his invulnerable flag on from the
/// eruption's spawn, 43 steps into its clip, to the clip's end). The four
/// poked to 100 so they live through it, West every 2 s from F0 + 150.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn in_his_eruption_a_bite_hurts_without_a_push_then_takes_nothing() {
    println!("load before: {}", load());
    let (_, snaps) = fight(Script {
        stun_health: true,
        erupt_every: Some(150),
        steps: 900,
        ..Script::default()
    });
    // Each eruption's invulnerable window, and its wind-up the 43 steps
    // before (AH.17: the lava 43 steps into the clip).
    let mut windows: Vec<(i64, i64)> = Vec::new();
    for w in snaps.windows(2) {
        if w[1].hero_hurt.invulnerable > 0 && w[0].hero_hurt.invulnerable == 0 {
            windows.push((w[1].k, i64::MAX));
        }
        if w[1].hero_hurt.invulnerable == 0
            && w[0].hero_hurt.invulnerable > 0
            && let Some(x) = windows.last_mut()
        {
            x.1 = w[1].k;
        }
    }
    println!("invulnerable {windows:?}");
    assert!(windows.len() >= 3, "eruptions");
    for &(a, b) in &windows {
        if b != i64::MAX {
            // From the lava's spawn to the clip's end: 86 - 43 steps.
            assert!((42..=44).contains(&(b - a)), "{a}..{b}");
        }
    }
    let mut wind_up = 0;
    let mut out = 0;
    for w in snaps.windows(2) {
        let reached: usize = (0..4)
            .map(|c| {
                w[1].chompies[c]
                    .bitten
                    .saturating_sub(w[0].chompies[c].bitten)
            })
            .sum();
        if reached == 0 {
            continue;
        }
        let k = w[1].k;
        let lost = w[0].health - w[1].health;
        let pushed = w[1].hero_hurt.knock_timer != w[0].hero_hurt.knock_timer;
        let reaction = w[1].hero_hurt.mode == 1 && w[0].hero_hurt.mode == 0;
        let inside = windows.iter().any(|&(a, b)| k >= a && k < b);
        let before = windows.iter().any(|&(a, _)| k >= a - 43 && k < a);
        println!(
            "F0 + {k}: {reached} reached him, -{lost}, pushed {pushed}, reaction {reaction}, out {inside}, wind-up {before}"
        );
        if inside {
            out += 1;
            assert_eq!(lost, 0.0, "nothing while it's out");
            assert!(!pushed && !reaction);
        } else if before {
            wind_up += 1;
            assert!(lost >= 11.0, "it hurts");
            assert!(
                !pushed && !reaction,
                "no push and no reaction in its wind-up"
            );
        }
    }
    println!("{wind_up} in a wind-up, {out} with one out");
    assert!(wind_up + out > 0, "a bite in an eruption");
    println!("load after: {}", load());
}

/// The hurt chain's rolls (combat: A hit does its attack's damage, Armour
/// can turn a hit away; script-runtime: Random rolls), the disc's own
/// `Combat - Hurt Creature` called 1,000 times a case over the session's
/// generator, each call's first draw read beforehand: Eruptor's Lava Lob on
/// a Chompy critical exactly when its roll is under his chance of 6
/// (37-83 of 1,000), every one at a chance of 100 (12 each); a Chompy's
/// bite on him turned away exactly when its roll is under his armour's
/// half, 3 (14-46), and at armour 200 landing only on a roll of 100 (1-20).
/// The calls run back to back, no step between, so nothing the fight does
/// meanwhile (a Chompy walking off the pit's edge to its death) moves the
/// numbers.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_hurt_chains_rolls_come_out_as_the_originals() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (mut run, _) = fight(Script {
        steps: 30,
        ..Script::default()
    });
    let m = run.minion.unwrap();
    let him = HERO;
    let (chompy, _) = run.placed("Enemy_Chompy(1)");
    let cid = run.placement("Enemy_Chompy(1)");
    let his_info = path(&run, m, "description", &["creature info"]).unwrap();
    let owner = his_info;
    let its_info = path(&run, cid, "description", &["creature info"]).unwrap();
    let set_hp = |run: &mut Run, info: Value| {
        assert!(run.vm.set_field(info, "hp", Value::Int(1_000_000)));
        let cp = run
            .vm
            .field(info, "character parameters")
            .map(|v| run.vm.value(v))
            .unwrap();
        assert!(run.vm.set_field(cp, "hp max", Value::Int(1_000_000)));
    };
    set_hp(&mut run, its_info);
    set_hp(&mut run, his_info);
    let hp = |run: &Run, info: Value| {
        run.vm
            .field(info, "hp")
            .and_then(|v| run.vm.value(v).as_f32())
            .unwrap()
    };
    let armour = run.vm.field(his_info, "armor").map(|v| run.vm.value(v));
    println!("his armour {armour:?}");
    assert_eq!(armour.and_then(|v| v.as_f32()), Some(6.0));
    let hurt = |run: &mut Run, args: &[(&str, Value)], target: Value| -> (f32, u32) {
        let first = run.generator.clone().next_u32();
        let before = hp(run, target);
        run.with_host(|vm, host| vm.call_with(Some(m), "Combat - Hurt Creature", args, &[], host))
            .unwrap_or_else(|h| panic!("{h}"));
        let lost = before - hp(run, target);
        (lost, first)
    };
    let lob = || {
        vec![
            ("damage", Value::Float(8.0)),
            ("target", Value::Entity(chompy)),
            ("hurt owner info", owner),
            ("critical hit", Value::Int(-1)),
            ("knockback strength", Value::Int(0)),
            ("hurt location actor", Value::Entity(him)),
        ]
    };
    // His critical chance as his joined data sums it.
    let total_crit = |run: &mut Run, crit: i32| {
        let d = run.minion_description();
        assert!(run.vm.set_field(d, "total crit", Value::Int(crit)));
    };
    for chance in [6, 100] {
        total_crit(&mut run, chance);
        let mut crits = 0;
        for _ in 0..1000 {
            let (lost, first) = hurt(&mut run, &lob(), its_info);
            let roll = random_literal(Value::Float(100.0), first)
                .unwrap()
                .as_f32()
                .unwrap();
            let critical = chance as f32 - roll > 0.0;
            if lost != if critical { 12.0 } else { 8.0 } {
                let d = path(&run, cid, "description", &[]).unwrap();
                println!(
                    "{:?} behaviour {:?} removed {} hp {}; description {:?}",
                    CHOMPIES[0],
                    run.vm.behaviour(cid),
                    run.vm.removed(cid),
                    hp(&run, its_info),
                    run.vm.fields(d)
                );
                println!("info {:?}", run.vm.fields(its_info));
            }
            assert_eq!(lost, if critical { 12.0 } else { 8.0 }, "roll {roll}");
            crits += i32::from(critical);
        }
        println!("crit chance {chance}: {crits} of 1,000 critical");
        if chance == 6 {
            assert!((37..=83).contains(&crits), "{crits}");
        } else {
            assert_eq!(crits, 1000);
        }
    }
    let bite = vec![
        ("damage", Value::Float(11.0)),
        ("target", Value::Entity(him)),
        ("critical hit", Value::Int(-1)),
        ("knockback strength", Value::Int(25)),
        ("hurt location actor", Value::Entity(chompy)),
    ];
    for armour in [6, 200] {
        assert!(run.vm.set_field(his_info, "armor", Value::Int(armour)));
        let mut away = 0;
        for _ in 0..1000 {
            let (lost, first) = hurt(&mut run, &bite, his_info);
            let roll = random_literal(Value::Int(100), first)
                .unwrap()
                .as_f32()
                .unwrap();
            let turned = armour as f32 * 0.5 - roll > 0.0;
            assert_eq!(lost, if turned { 0.0 } else { 11.0 }, "roll {roll}");
            away += i32::from(turned);
        }
        println!("armour {armour}: {away} of 1,000 turned away");
        if armour == 6 {
            assert!((14..=46).contains(&away), "{away}");
        } else {
            assert!((980..=999).contains(&away), "{away}");
        }
    }
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// A Chompy hit on a schedule (combat: No grace after a hit, Knockback),
/// the disc's own `Combat - Hurt Creature` called with a Lava Lob's
/// numbers (8, On Fire at 50, strength 25, his creature info the owner)
/// at set steps, the Chompy's health poked to 100: the first hit stuns it
/// 0.6 s; one inside a stun doesn't start it again; one within a second of
/// its end stuns nothing; hits there build its resistance past 25, and one
/// after that second, too weak to push it, neither pushes nor stuns; one
/// with the resistance gone stuns again; each push its knock time from the
/// strength less the resistance, 0.15 s at none (AH.10's `stun` staging).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_chompy_hit_on_a_schedule_is_stunned_by_the_rule() {
    println!("load before: {}", load());
    let (mut run, _) = fight(Script {
        stun_health: true,
        steps: 30,
        ..Script::default()
    });
    let m = run.minion.unwrap();
    let owner = path(&run, m, "description", &["creature info"]).unwrap();
    let (chompy, _) = run.placed("Enemy_Chompy(2)");
    let on_fire = {
        let p = run.vm.program_arc();
        let d = p.macro_named("State - On Fire").expect("On Fire");
        Value::Object(p.defs[p.resolve(d).unwrap() as usize].obj)
    };
    let args = [
        ("damage", Value::Float(8.0)),
        ("target", Value::Entity(chompy)),
        ("damage state", on_fire),
        ("damage state magnitude", Value::Int(50)),
        ("knockback strength", Value::Int(25)),
        ("hurt owner info", owner),
        ("hurt location actor", Value::Entity(HERO)),
    ];
    // A stun at 27; 66 inside the second after it ends; 90, 94 and 98 in
    // that second too, building its resistance past 25 with no stun; 135,
    // out of both, too weak; 250 a stun again, 253 inside it; 420 fresh.
    let schedule = [27u32, 66, 90, 94, 98, 135, 250, 253, 420];
    let mut snaps = vec![run.snap(0)];
    for k in 1..=480u32 {
        if schedule.contains(&k) {
            run.with_host(|vm, host| {
                vm.call_with(Some(m), "Combat - Hurt Creature", &args, &[], host)
            })
            .unwrap_or_else(|h| panic!("{h}"));
        }
        run.step().unwrap_or_else(|h| panic!("{h}"));
        snaps.push(run.snap(k as i64));
    }
    let c = 1;
    let pushes = pushes(&snaps, |s| (s.chompies[c].hurt, s.chompies[c].speed));
    let mut seen = std::collections::BTreeSet::new();
    for (i, w) in snaps.windows(2).enumerate() {
        let (a, b) = (&w[0].chompies[c], &w[1].chompies[c]);
        if b.health >= a.health {
            continue;
        }
        let k = w[1].k;
        let push = pushes.iter().find(|p| p.k == k);
        let stunned = b.hurt.mode == 1 && a.hurt.mode != 1;
        let since_end = snaps[..=i]
            .windows(2)
            .rev()
            .find(|x| x[0].chompies[c].hurt.mode == 1 && x[1].chompies[c].hurt.mode != 1)
            .map(|x| (k - x[1].k) as f32 / 60.0);
        println!(
            "F0 + {k}: -{}, resist {:.2}, push {:?}, stunned {stunned} (in one {}, {since_end:?} s after the last)",
            a.health - b.health,
            a.hurt.resist,
            push.map(|p| (p.knock_before, p.speeds.len())),
            a.hurt.mode == 1
        );
        assert_eq!(a.health - b.health, 8.0);
        if a.hurt.mode == 1 {
            seen.insert("inside");
            assert!(!stunned);
            assert_eq!(b.hurt.mode_timer, a.hurt.mode_timer, "not started again");
        } else if since_end.is_some_and(|t| t < 1.0) {
            seen.insert("within 1 s");
            assert!(!stunned);
        } else if a.hurt.resist >= 25.0 {
            seen.insert("too weak");
            assert!(!stunned && push.is_none());
        } else {
            seen.insert("stunned");
            assert!(stunned);
            assert_eq!(b.beh.as_deref(), Some("disabled"));
            let end = snaps[i + 1..]
                .iter()
                .find(|s| s.chompies[c].hurt.mode != 1)
                .map(|s| s.k - k)
                .expect("its stun ends");
            assert!((35..=37).contains(&end), "{end}");
        }
        if a.hurt.resist < 25.0 {
            // Called ahead of the step: the game time it set the timer on
            // is the step before's.
            let p = push.expect("a push");
            let want = ((25.0 - a.hurt.resist) / 100.0 * 0.6).max(0.125);
            assert!(
                (p.knock_before - want).abs() < 2e-4,
                "{} against {want}",
                p.knock_before
            );
        }
    }
    println!("cases {seen:?}");
    assert_eq!(seen.len(), 4, "every case staged");
    println!("load after: {}", load());
}

/// One orb as a run sees it (openspec AH.20): its worth and the steps from
/// F0 it came out (listed among the active or launching), came down (left
/// the launching set), was taken (into his player's `xp balls collecting`)
/// and arrived (left it, its worth paid); his distance to it, in space and
/// across the ground, ft, at the end of the step before it was taken (what
/// the take read), and across the ground at the end of the step it came
/// down.
#[derive(Debug, Clone, Default)]
struct OrbLife {
    worth: i32,
    out: i64,
    down: Option<i64>,
    taken: Option<i64>,
    arrived: Option<i64>,
    taken_from: Option<(f32, f32)>,
    down_ground: Option<f32>,
}

/// What the orbs' sets hold after a step: each orb's entity, its worth
/// and where it is.
#[derive(Debug, Clone, Default)]
struct OrbSets {
    active: Vec<(u64, i32, Vec3)>,
    launching: Vec<u64>,
    collecting: Vec<u64>,
}

impl Run {
    /// `combat control data.$xp ball control`.
    fn xp_control(&self) -> Option<Value> {
        let g = self.vm.program().global_named("combat control data")?;
        let c = self.vm.value(self.vm.global_peek(g)?);
        self.vm
            .field(c, "xp ball control")
            .map(|v| self.vm.value(v))
    }

    /// Each ball of `set` (a set of `Xp Ball Movement`): its orb's entity
    /// and worth.
    fn balls(&self, set: Value) -> Vec<(u64, i32)> {
        let items = self.vm.set(set).map(<[Value]>::to_vec).unwrap_or_default();
        items
            .into_iter()
            .filter_map(|b| {
                let b = self.vm.value(b);
                let xb = self.vm.field(b, "xp ball").map(|v| self.vm.value(v))?;
                let orb = match self.vm.field(xb, "orb").map(|v| self.vm.value(v)) {
                    Some(Value::Entity(e)) => e,
                    _ => return None,
                };
                let worth = self
                    .vm
                    .field(xb, "xp val")
                    .and_then(|v| self.vm.value(v).as_i32())?;
                Some((orb, worth))
            })
            .collect()
    }

    fn orb_sets(&self) -> OrbSets {
        let Some(xbc) = self.xp_control() else {
            return OrbSets::default();
        };
        let set = |name: &str| self.vm.field(xbc, name).map(|v| self.vm.value(v));
        let mut active: Vec<(u64, i32, Vec3)> = Vec::new();
        for n in ["all active balls", "balls launching"] {
            for (orb, worth) in set(n).map(|s| self.balls(s)).unwrap_or_default() {
                if !active.iter().any(|a| a.0 == orb) {
                    let at = self.world.actor(orb).map_or(Vec3::ZERO, |a| a.position);
                    active.push((orb, worth, at));
                }
            }
        }
        let launching = set("balls launching")
            .map(|s| self.balls(s).into_iter().map(|b| b.0).collect())
            .unwrap_or_default();
        let collecting = self
            .minion
            .and_then(|m| {
                path(
                    self,
                    m,
                    "my Minion Description",
                    &["player joined data", "xp balls collecting"],
                )
            })
            .map(|s| self.balls(s).into_iter().map(|b| b.0).collect())
            .unwrap_or_default();
        OrbSets {
            active,
            launching,
            collecting,
        }
    }

    /// His player's `player stat tracking.$exp collected`: what
    /// `Game - Award Experience` has paid this session (the figure's own
    /// experience is AH.23's buffer).
    fn exp_collected(&self) -> i32 {
        self.minion
            .and_then(|m| {
                path(
                    self,
                    m,
                    "my Minion Description",
                    &[
                        "player joined data",
                        "player stat tracking",
                        "exp collected",
                    ],
                )
            })
            .and_then(|v| v.as_i32())
            .unwrap_or(0)
    }
}

/// Follows every orb from step `from` (F0 + `from`) for `steps` steps,
/// `walk` moving the hero before each (given the orbs as the last step left
/// them), and returns each orb's life by its entity and each step's
/// experience paid.
fn follow_orbs(
    run: &mut Run,
    from: i64,
    steps: i64,
    mut walk: impl FnMut(&mut Run, i64, &OrbSets),
) -> (BTreeMap<u64, OrbLife>, Vec<(i64, i32)>) {
    let mut lives: BTreeMap<u64, OrbLife> = BTreeMap::new();
    let mut paid = Vec::new();
    let mut before = run.orb_sets();
    let mut exp_before = run.exp_collected();
    for f in from..from + steps {
        walk(run, f, &before);
        let hero_at_start = run.hero().pivot;
        run.step().unwrap_or_else(|h| panic!("F0 + {f}: {h}"));
        let now = run.orb_sets();
        for (orb, worth, at) in &now.active {
            let life = lives.entry(*orb).or_insert(OrbLife {
                worth: *worth,
                out: f,
                ..OrbLife::default()
            });
            if life.down.is_none() && !now.launching.contains(orb) {
                life.down = Some(f);
                let d = *at - run.hero().pivot;
                life.down_ground = Some(feet(Vec3::new(d.x, 0.0, d.z).length()));
            }
        }
        for orb in &now.collecting {
            let life = lives.get_mut(orb).expect("a taken orb came out");
            if life.taken.is_none() {
                life.taken = Some(f);
                // Where it was, and he, as the take read them: the step
                // before's end.
                let at = before
                    .active
                    .iter()
                    .find(|a| a.0 == *orb)
                    .map_or(Vec3::ZERO, |a| a.2);
                let d = at - hero_at_start;
                life.taken_from = Some((feet(d.length()), feet(Vec3::new(d.x, 0.0, d.z).length())));
            }
        }
        for orb in &before.collecting {
            if !now.collecting.contains(orb) {
                lives.get_mut(orb).unwrap().arrived = Some(f);
            }
        }
        let exp = run.exp_collected();
        if exp != exp_before {
            paid.push((f, exp - exp_before));
        }
        exp_before = exp;
        before = now;
    }
    (lives, paid)
}

/// A lob's kill and its orbs against AH.10's (openspec AH.20, pickups'
/// Experience comes out as orbs and A hero collects orbs): each Chompy
/// beaten makes its experience times the save's multiplier in 1-point
/// orbs; they come out two a step from the step after the kill or the
/// one after, and down within 1.2 s (AH.10's 4-72 frames); one down
/// within 20 ft of him across the ground is taken the step after it comes
/// down, never in the air, and flies in at 55 ft/s, paying its worth the
/// step it arrives (AH.10: 8.4 ft in 10 frames, 20 ft in 22); those out of
/// reach stay until he walks within 20 ft across the ground. Returns the
/// lives, the experience paid and how many Chompies the lob beat.
/// A lob's orbs followed ([`lob_and_collect`]): each orb's life, the
/// experience paid by step, how many the lob beat and the step it did.
struct Lobbed {
    lives: BTreeMap<u64, OrbLife>,
    paid: Vec<(i64, i32)>,
    beaten: usize,
    kill: i64,
}

fn lob_and_collect(mult: Option<f32>) -> Lobbed {
    let (mut run, _) = fight(Script {
        lob: Some(20),
        xp_multiplier: mult,
        steps: 40,
        ..Script::default()
    });
    let health = |run: &Run, c: usize| {
        let id = run.placement(CHOMPIES[c]);
        path(run, id, "description", &["creature info", "hp"])
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0)
    };
    let mut beaten = [false; 4];
    let mut kill: Option<i64> = None;
    // Where he stands, then once every orb is down a walk to each left,
    // 4 m/s across the ground.
    let mut target: Option<u64> = None;
    let (lives, paid) = follow_orbs(&mut run, 40, 900, |run, f, sets| {
        for (c, b) in beaten.iter_mut().enumerate() {
            if !*b && health(run, c) <= 0.0 {
                *b = true;
                // Its health read 0 at the end of the step before.
                kill.get_or_insert(f - 1);
            }
        }
        if f < 200 || sets.active.iter().any(|a| sets.launching.contains(&a.0)) {
            return;
        }
        if target
            .is_none_or(|t| !sets.active.iter().any(|a| a.0 == t) || sets.collecting.contains(&t))
        {
            let h = run.hero().pivot;
            target = sets
                .active
                .iter()
                .filter(|a| !sets.collecting.contains(&a.0))
                .min_by(|a, b| (a.2 - h).length().total_cmp(&(b.2 - h).length()))
                .map(|a| a.0);
        }
        let Some(at) = target.and_then(|t| sets.active.iter().find(|a| a.0 == t)) else {
            return;
        };
        let h = run.hero().pivot;
        let d = Vec3::new(at.2.x - h.x, 0.0, at.2.z - h.z);
        if d.length() > 0.1 {
            let to = h + d.normalize() * (4.0 / 60.0) - Vec3::Y;
            let facing = run.hero().facing;
            run.put(to, facing);
        }
    });
    let n = beaten.iter().filter(|b| **b).count();
    let kill = kill.expect("the lob beats a Chompy");
    println!("the lob beats {n} at F0 + {kill}");
    Lobbed {
        lives,
        paid,
        beaten: n,
        kill,
    }
}

fn check_orbs(l: &Lobbed, per_chompy: usize) {
    let Lobbed {
        lives,
        paid,
        beaten,
        kill,
    } = l;
    let (beaten, kill) = (*beaten, *kill);
    assert_eq!(lives.len(), per_chompy * beaten, "orbs per Chompy beaten");
    assert!(lives.values().all(|l| l.worth == 1), "every one worth 1");
    let mut outs: BTreeMap<i64, usize> = BTreeMap::new();
    for l in lives.values() {
        *outs.entry(l.out).or_default() += 1;
    }
    println!("out per step {outs:?}");
    assert!(outs.values().all(|&n| n <= 2), "two a step at most");
    let first = *outs.keys().next().unwrap();
    assert!(
        first == kill + 1 || first == kill + 2,
        "the first out at F0 + {first}, the kill at F0 + {kill}"
    );
    let mut taken_where_they_lay = 0;
    let mut walked_to = 0;
    for (orb, l) in lives {
        let down = l.down.expect("every orb comes down");
        let fall = down - l.out;
        println!(
            "orb {orb:#x}: out F0 + {} ({} after the first), down {fall} steps on {:.2} ft across, taken {:?} from {:?}, arrived {:?}",
            l.out,
            l.out - first,
            l.down_ground.unwrap(),
            l.taken,
            l.taken_from,
            l.arrived
        );
        assert!((1..=72).contains(&fall), "down within 1.2 s: {fall}");
        let taken = l.taken.expect("every orb taken, as he walks in");
        assert!(taken > down, "never taken in the air");
        let (space, ground) = l.taken_from.unwrap();
        assert!(ground <= 20.0, "within 20 ft across the ground: {ground}");
        if l.down_ground.unwrap() <= 20.0 {
            // Down within reach: taken the next step, as it lay.
            assert_eq!(taken, down + 1, "taken as it comes down");
            taken_where_they_lay += 1;
        } else {
            // Walked to: out of reach as it came down.
            walked_to += 1;
        }
        let arrived = l.arrived.expect("every orb taken arrives");
        let want = (space / 55.0 * 60.0 - 1e-3).ceil() as i64;
        assert!(
            arrived - taken == want || arrived - taken == want + 1,
            "{space:.2} ft in {} steps, want {want}",
            arrived - taken
        );
        assert!(
            paid.iter().any(|(f, _)| *f == arrived),
            "its worth paid the step it arrives"
        );
    }
    let total: i32 = paid.iter().map(|p| p.1).sum();
    assert_eq!(total as usize, lives.len(), "every orb's worth paid");
    println!(
        "{taken_where_they_lay} taken where they came down, {walked_to} walked to; {total} experience"
    );
    assert!(walked_to > 0, "some out of reach as they come down");
}

/// AH.10's orbs at guacamole's multiplier, 1 (a fresh save): four of 1 a
/// Chompy, so the lob's two make eight and 8 experience.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_beaten_chompys_orbs_come_down_and_fly_to_him_as_ah10s() {
    println!("load before: {}", load());
    check_orbs(&lob_and_collect(None), 4);
    println!("load after: {}", load());
}

/// At the oracle's save's multiplier, 1.1 (AH.4's read, one accolade
/// done): a Chompy's 4.4 makes five orbs of 1, the fraction one more
/// (AH.10: the twenty fights' 80 Chompies made 400).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn at_the_oracles_multiplier_a_chompy_makes_five_orbs() {
    println!("load before: {}", load());
    check_orbs(&lob_and_collect(Some(1.1)), 5);
    println!("load after: {}", load());
}

/// A big orb on the disc's own macro (openspec AH.20; pickups' A big orb):
/// `Xp Balls - Create Exp Balls` called as `Combat - Kill Creature` calls
/// it, for a creature worth 50, 1,000 times on the session's seeded
/// generator, the orbs left waiting to come out (the macro fades the
/// oldest past 50 itself): its first orb is 25 between 207 and 288 times
/// (25 in 101, mean +- 3 sd) and 50 otherwise, every kill's orbs summing
/// to 50.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_big_orb_splits_by_the_discs_roll() {
    println!("load before: {}", load());
    // At anchor B, the four awake and their descriptions built.
    let (mut run, _) = fight(Script {
        steps: 1,
        ..Script::default()
    });
    let id = run.placement("Enemy_Chompy(1)");
    let info = path(&run, id, "description", &["creature info"]).unwrap();
    let params = run
        .vm
        .field(info, "character parameters")
        .map(|v| run.vm.value(v))
        .unwrap();
    let started = Instant::now();
    let mut first: BTreeMap<i32, usize> = BTreeMap::new();
    let mut splits: BTreeMap<Vec<i32>, usize> = BTreeMap::new();
    let next_id = |run: &Run| {
        let xbc = run.xp_control().unwrap();
        run.vm
            .field(xbc, "Next Ball ID")
            .and_then(|v| run.vm.value(v).as_i32())
            .unwrap()
    };
    for _ in 0..1000 {
        // Kill Creature zeroes the experience after the call.
        assert!(run.vm.set_field(params, "exp value", Value::Int(50)));
        // This kill's orbs: their ids from the next one on (the pool keeps
        // older ones, the clean-up leaving it, and prunes it to 50 itself).
        let from = next_id(&run);
        run.with_host(|vm, host| {
            vm.call_with(
                Some(id),
                "Xp Balls - Create Exp Balls",
                &[
                    ("incoming creature info", info),
                    ("spawn loc", Value::Null),
                    ("force count", Value::Int(-1)),
                    ("force value", Value::Int(-1)),
                    ("force h rate", Value::Int(-1)),
                    ("time mult", Value::Float(1.0)),
                    ("elemental zone bonus", Value::Int(0)),
                ],
                &[],
                host,
            )
        })
        .unwrap_or_else(|h| panic!("{h}"));
        let xbc = run.xp_control().unwrap();
        let mut balls: BTreeMap<i32, i32> = BTreeMap::new();
        for n in ["ball movement pool", "balls launching"] {
            let set = run.vm.field(xbc, n).map(|v| run.vm.value(v)).unwrap();
            for b in run.vm.set(set).map(<[Value]>::to_vec).unwrap_or_default() {
                let b = run.vm.value(b);
                let xb = run.vm.field(b, "xp ball").map(|v| run.vm.value(v)).unwrap();
                let get = |n: &str| {
                    run.vm
                        .field(xb, n)
                        .and_then(|v| run.vm.value(v).as_i32())
                        .unwrap()
                };
                if get("my ID") >= from {
                    balls.insert(get("my ID"), get("xp val"));
                }
            }
        }
        let worths: Vec<i32> = balls.into_values().collect();
        assert_eq!(worths.iter().sum::<i32>(), 50, "{worths:?}");
        *first.entry(worths[0]).or_default() += 1;
        *splits.entry(worths.clone()).or_default() += 1;
    }
    let xbc = run.xp_control().unwrap();
    let pool = run
        .vm
        .field(xbc, "ball movement pool")
        .and_then(|v| run.vm.set(run.vm.value(v)).map(<[Value]>::len))
        .unwrap();
    println!("the pool at the end: {pool} (its own pruning past 50)");
    assert!(pool <= 60, "{pool}");
    println!(
        "first orb {first:?}; splits {splits:?}; {:.1?}",
        started.elapsed()
    );
    assert_eq!(first.keys().copied().collect::<Vec<_>>(), [25, 50]);
    assert!((207..=288).contains(&first[&25]), "{}", first[&25]);
    println!("load after: {}", load());
}

/// An AH.11 coin drive read back (openspec AH.20): each record's game time
/// (s from its first, by the oracle's clamped frame times), the hero's
/// pivot, the loot entries listed (by placement) and his figure's gold.
struct CoinDrive {
    name: String,
    t: Vec<f32>,
    hero: Vec<Vec3>,
    loot: Vec<Vec<String>>,
    gold: Vec<f32>,
}

impl CoinDrive {
    fn read(dir: &Path) -> Self {
        let json = |n: &str| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(dir.join(n)).unwrap()).unwrap()
        };
        let rec = json("recording.json");
        let records = json("chain-records.json");
        let mut t = vec![0.0f32];
        for r in rec
            .as_array()
            .unwrap()
            .iter()
            .take(records.as_array().unwrap().len() - 1)
        {
            let c = &r["clock"];
            let dt = if c["logic_stopped"].as_bool().unwrap() {
                0.0
            } else {
                let pr = c["presentation_rate"].as_f64().unwrap() as f32;
                let d = (c["frame_delta"].as_f64().unwrap() as f32).clamp(0.5 * pr, 3.0 * pr);
                d * c["logic_scale"].as_f64().unwrap() as f32
                    * c["object_scale"].as_f64().unwrap() as f32
            };
            t.push(t.last().unwrap() + dt);
        }
        let (mut hero, mut loot, mut gold) = (Vec::new(), Vec::new(), Vec::new());
        for r in records.as_array().unwrap() {
            let items = r["items"].as_array().unwrap();
            let h = items.iter().find(|i| i["kind"] == "hero").unwrap();
            let p: Vec<f32> = h["body"]["position"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            hero.push(Vec3::new(p[0], p[1], p[2]));
            gold.push(h["vars"]["gold"].as_f64().unwrap() as f32);
            loot.push(
                items
                    .iter()
                    .filter(|i| i["kind"] == "coin")
                    .map(|i| i["placement"].as_str().unwrap().to_owned())
                    .collect(),
            );
        }
        CoinDrive {
            name: dir.file_name().unwrap().to_string_lossy().into_owned(),
            t,
            hero,
            loot,
            gold,
        }
    }

    /// Where he was at game time `at`, between the records either side.
    fn hero_at(&self, at: f32) -> Vec3 {
        let k = self.t.partition_point(|&x| x <= at).max(1) - 1;
        if k + 1 >= self.t.len() {
            return *self.hero.last().unwrap();
        }
        let span = (self.t[k + 1] - self.t[k]).max(1e-6);
        let x = ((at - self.t[k]) / span).clamp(0.0, 1.0);
        self.hero[k].lerp(self.hero[k + 1], x)
    }

    /// Each coin taken in the drive: the game time of the record its entry
    /// is first gone from, and his distance to it in space then (ft).
    fn takes(&self, coin_at: impl Fn(&str) -> Vec3) -> BTreeMap<String, (f32, f32)> {
        let mut out = BTreeMap::new();
        for k in 1..self.loot.len() {
            for c in &self.loot[k - 1] {
                if !self.loot[k].contains(c) {
                    let d = feet((coin_at(c) - self.hero[k - 1]).length());
                    out.insert(c.clone(), (self.t[k], d));
                }
            }
        }
        out
    }

    /// Each rise of his gold: game time and by how much.
    fn gold_rises(&self) -> Vec<(f32, f32)> {
        (1..self.gold.len())
            .filter(|&k| self.gold[k] > self.gold[k - 1])
            .map(|k| (self.t[k], self.gold[k] - self.gold[k - 1]))
            .collect()
    }
}

impl Run {
    /// The loot in play (`Macros_UI`'s `LootInPlay`) and his player's
    /// `loot i collect`: each entry's coin, by its placement's name.
    fn coins(&self) -> (Vec<String>, Vec<String>) {
        let ui_obj = self
            .files
            .placed
            .iter()
            .find(|p| p.name == "Macros_UI")
            .unwrap()
            .obj;
        let ui = self.vm.placement_of(self.handles[&ui_obj]).unwrap();
        let names = |set: Option<Value>| -> Vec<String> {
            let items = set
                .and_then(|s| self.vm.set(self.vm.value(s)).map(<[Value]>::to_vec))
                .unwrap_or_default();
            items
                .into_iter()
                .filter_map(|e| {
                    let e = self.vm.value(e);
                    let a = self.vm.field(e, "lootActor").map(|v| self.vm.value(v))?;
                    let Value::Entity(h) = a else {
                        return None;
                    };
                    let obj = self.handles.iter().find(|(_, x)| **x == h)?.0;
                    self.files
                        .placed
                        .iter()
                        .find(|p| p.obj == *obj)
                        .map(|p| p.name.clone())
                })
                .collect()
        };
        let in_play = names(self.vm.local(ui, "LootInPlay"));
        let collecting = names(self.minion.and_then(|m| {
            path(
                self,
                m,
                "my Minion Description",
                &["player joined data", "loot i collect"],
            )
        }));
        (in_play, collecting)
    }

    /// His player's `player stat tracking.$loot collected`: what
    /// `Game - Award Money` has paid this session (the figure's own gold is
    /// AH.23's buffer).
    fn loot_collected(&self) -> i32 {
        self.minion
            .and_then(|m| {
                path(
                    self,
                    m,
                    "my Minion Description",
                    &[
                        "player joined data",
                        "player stat tracking",
                        "loot collected",
                    ],
                )
            })
            .and_then(|v| v.as_i32())
            .unwrap_or(0)
    }
}

/// AH.11's coin drives replayed headless (openspec AH.20; pickups' Coins):
/// the hero put each step where the drive had him at that game time, from
/// the level's start with his scripts joined. A coin is taken at a check
/// every 0.25 s (15 steps) once within 12 ft of him in space, and not at
/// the check before; its gold paid 0.48 s (29 steps) on, one taken while
/// another slides starting both again so they pay together; within a
/// check's 0.25 s of the drive's own take, by the same coins and the same
/// gold. `Coin_spinning(1)` was taken before anchor F, so it's left out of
/// the fall line's drives.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.11's recordings; run on the Mac"]
fn spinning_coins_fly_to_him_and_pay_as_ah11s() {
    println!("load before: {}", load());
    let root = env_or_dotenv("GUAC_CAPTURE_DIR").expect("GUAC_CAPTURE_DIR: AH.11's recordings");
    let base = Path::new(&root).join("BLUS30968");
    let mut drives = Vec::new();
    for f in ["ah11-coinsw-1792571832", "ah11-coinsf-1792572230"] {
        let mut d: Vec<PathBuf> = std::fs::read_dir(base.join(f))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("chain-records.json").is_file())
            .collect();
        d.sort();
        drives.extend(d);
    }
    assert_eq!(drives.len(), 6);
    for dir in drives {
        let drive = CoinDrive::read(&dir);
        let (mut run, _) = joined(Mode::Strict);
        let coin_at = |run: &Run, c: &str| run.placed(c).1;
        let theirs = drive.takes(|c| {
            run.files
                .placed
                .iter()
                .find(|p| p.name == c && p.obj.unit == LEVEL)
                .map_or(Vec3::ZERO, |p| Vec3::from_array(p.position))
        });
        let skip = |c: &str| c == "Coin_spinning(1)" || !scripts::PICKUPS.contains(&c);
        let gold_theirs: f32 = drive
            .gold_rises()
            .iter()
            .filter(|(t, _)| *t > 0.0)
            .map(|g| g.1)
            .sum();
        // Two seconds at the drive's start first, for what was taken before
        // it (the fall line's first coin, which wakes as he comes within its
        // range and can't be taken for 0.7 s) to be taken and paid.
        for _ in 0..120 {
            let hero = run.bench.heroes.get_mut(&HERO).unwrap();
            hero.pivot = drive.hero[0];
            hero.walk.velocity = Vec3::ZERO;
            run.step().unwrap_or_else(|h| panic!("{}: {h}", drive.name));
        }
        // Ours: the hero where the drive had him, each step.
        let steps = (drive.t.last().unwrap() * 60.0) as i64 + 60;
        let mut ours: BTreeMap<String, (i64, f32)> = BTreeMap::new();
        let mut paid: Vec<(i64, i32)> = Vec::new();
        let (mut before, _) = run.coins();
        let mut paid_before = run.loot_collected();
        let mut checks: Vec<i64> = Vec::new();
        let timer = |run: &Run| {
            let m = run.minion.unwrap();
            path(
                run,
                m,
                "my Minion Description",
                &["player joined data", "timer"],
            )
            .and_then(|v| v.as_f32())
            .unwrap_or(0.0)
        };
        let mut timer_before = timer(&run);
        for s in 0..steps {
            let at = drive.hero_at(s as f32 / 60.0);
            let hero = run.bench.heroes.get_mut(&HERO).unwrap();
            hero.pivot = at;
            hero.walk.velocity = Vec3::ZERO;
            let from = at;
            run.step()
                .unwrap_or_else(|h| panic!("{}: step {s}: {h}", drive.name));
            let t = timer(&run);
            if t != timer_before {
                checks.push(s);
            }
            timer_before = t;
            let (now, _) = run.coins();
            for c in &before {
                if !now.contains(c) {
                    let d = feet((coin_at(&run, c) - from).length());
                    ours.insert(c.clone(), (s, d));
                }
            }
            before = now;
            let p = run.loot_collected();
            if p != paid_before {
                paid.push((s, p - paid_before));
            }
            paid_before = p;
        }
        let ours: BTreeMap<String, (i64, f32)> =
            ours.into_iter().filter(|(c, _)| !skip(c)).collect();
        let theirs: BTreeMap<String, (f32, f32)> =
            theirs.into_iter().filter(|(c, _)| !skip(c)).collect();
        println!(
            "{}: theirs {theirs:?}, gold +{gold_theirs}; ours {ours:?}, paid {paid:?}",
            drive.name
        );
        assert_eq!(
            ours.keys().collect::<Vec<_>>(),
            theirs.keys().collect::<Vec<_>>(),
            "the same coins taken"
        );
        for (c, (s, d)) in &ours {
            assert!(*d <= 12.0, "{c}: taken {d:.2} ft off");
            assert!(checks.contains(s), "{c}: taken at a check");
            // Not within 12 ft at the check before.
            let prev = checks.iter().rev().find(|x| **x < *s).copied();
            if let Some(p) = prev {
                let then = drive.hero_at(p as f32 / 60.0);
                let d0 = feet((coin_at(&run, c) - then).length());
                assert!(
                    d0 > 12.0,
                    "{c}: within reach at the check before ({d0:.2} ft)"
                );
            }
            let (t, _) = theirs[c];
            let ours_t = *s as f32 / 60.0;
            assert!(
                (ours_t - t).abs() <= 0.25 + 1.0 / 60.0 + 0.03,
                "{c}: ours {ours_t:.3} s, theirs {t:.3} s"
            );
        }
        // Paid 0.48 s after the last take before it: 29 steps.
        let mut takes: Vec<i64> = ours.values().map(|v| v.0).collect();
        takes.sort();
        for (s, n) in &paid {
            let last = *takes
                .iter()
                .rev()
                .find(|t| *t < s)
                .expect("a take before it");
            assert_eq!(
                s - last,
                29,
                "{}: paid {n} at {s}, the last take {last}",
                drive.name
            );
        }
        let ours_gold: i32 = paid.iter().map(|p| p.1).sum();
        assert_eq!(ours_gold as f32, gold_theirs, "{}: the gold", drive.name);
    }
    println!("load after: {}", load());
}

/// A defeated hero's sequence (openspec AH.20, the player loop's death
/// sequence; combat's A defeated hero), his health set to 20 at F0 so a
/// second bite beats him: his hurt reaction from the step after the
/// killing bite, 8 steps on every creature within 30 ft knocked back at
/// strength 25 and none past it (AH.11's `defeat2`: 8 records, 0.13 s, the
/// Chompies 5.6-20.9 ft off), his mode -1 28 steps after the knock (AH.11's
/// 28 records), and his minion taken off 133-134 steps after the bite
/// (AH.11's 133-134 records): the kill strict through the death's
/// data-mining snapshot.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_defeated_hero_knocks_them_back_and_leaves() {
    println!("load before: {}", load());
    let (_, snaps) = fight(Script {
        hero_health: Some(20),
        steps: 900,
        ..Script::default()
    });
    let at = |p: &dyn Fn(&Snap) -> bool| snaps.iter().find(|s| p(s)).map(|s| s.k);
    let bite = at(&|s| s.health <= 0.0).expect("beaten");
    let reaction = at(&|s| s.k > bite && s.hero_hurt.mode == 1).unwrap();
    let knock = at(&|s| s.k > bite && s.chompies.iter().any(|c| c.hurt.strength == 25.0)).unwrap();
    let out = at(&|s| s.k > bite && s.hero_hurt.mode == -1).unwrap();
    let gone = at(&|s| s.hero_gone).expect("he leaves");
    let before = snaps.iter().find(|s| s.k == knock - 1).unwrap();
    let knocked = snaps.iter().find(|s| s.k == knock).unwrap();
    let off: Vec<f32> = before
        .chompies
        .iter()
        .map(|c| feet((c.pos - before.hero).length()))
        .collect();
    println!(
        "beaten at F0 + {bite}; reaction F0 + {reaction}; knock F0 + {knock} ({off:.1?} ft off); mode -1 F0 + {out}; gone F0 + {gone} ({:.3} s after the bite)",
        (gone - bite) as f32 / 60.0
    );
    assert_eq!(reaction, bite + 1);
    assert_eq!(knock - reaction, 8, "0.13 s on");
    for (c, d) in off.iter().enumerate() {
        let k = knocked.chompies[c].hurt.strength == 25.0;
        assert_eq!(k, *d < 30.0, "{}: {d:.1} ft, knocked {k}", CHOMPIES[c]);
    }
    assert!((27..=29).contains(&(out - knock)), "{}", out - knock);
    // In steps, as AH.11's records count (its 133-134 records read 2.18-2.20
    // s on the oracle's frame times; 134 of ours, 2.233 s at 60 a second).
    assert!((133..=134).contains(&(gone - bite)), "{}", gone - bite);
    println!("load after: {}", load());
}
