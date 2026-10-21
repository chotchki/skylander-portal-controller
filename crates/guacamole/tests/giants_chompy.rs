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
use guac_script::{
    Generator, Halt, Handle, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role,
    Unit, Value, Vm,
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

/// The step AH.8's opening starts at: 120 steps after anchor B's pose.
const OPENING: u32 = 480;

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
