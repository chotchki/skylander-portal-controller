//! AH.17's disc tests: Eruptor's scripts on the VM over level 213 with
//! guacamole's host, headless: the init chain (AH.16), the join's stand-in
//! (his definition's first tick, then the portal manager's activation of
//! his minion), and his own script, the movement update and the UI's tick
//! every step, his walk handed to his controller's engine on the parry
//! side of the seam over the level's own collision. Ignored by default;
//! they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --release --test giants_eruptor -- --ignored --nocapture --test-threads=1`
//!
//! A dev aid, not a check: `AH17_LIVE_TABLE=<aj-vm's live member table>`
//! answers every member the registry doesn't hold with a value cell of its
//! registered type and prints which the run reached, so a run lists what's
//! left to model instead of stopping at the first; `AH17_USES=1` prints
//! every member the members test's run read or wrote, with the counts.

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
use guac_motion::scenarios;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, PhysicsSeam, Pose, Vec3};
use guac_motion::stick::Stick;
use guac_motion::tuning::{Tuning, eruptor_solid};
use guac_script::members::{COMBAT, HERO as HERO_MEMBERS, INIT_CHAIN};

use guac_script::{
    Generator, Halt, Handle, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role,
    StallDetector, StallLimit, Unit, Value, Vm,
};
use guacamole::level_scene;
use guacamole::player_loop::PlayerLoop;
use guacamole::script_engine::{Engine, Pad};
use guacamole::scripts::{
    self, Files, ScriptWorld, WorldHost, headless_handles, headless_objects, headless_spawner,
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

/// The hero's handle in a headless run: past every placement's and object's.
const HERO: Handle = 0x7000_0000;

/// The engine a headless run owns: Eruptor's controller on the parry side
/// of the seam over the level's collision, the camera's view and the pads.
struct Bench {
    heroes: HashMap<Handle, Hero>,
    world: ParryWorld,
    view: f32,
    pads: [Pad; 2],
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
    /// What poses his hands each step (AH.50), and his idle's time at step
    /// 0: his attach nodes off his model's attach list by his idle and what
    /// his scripts play on him.
    rig: guacamole::hands::Rig,
    idle0: f64,
}

impl Run {
    /// His attach nodes posed as the frame left his clips (his idle at its
    /// time this step), and what hangs from them set there (AH.50).
    fn pose_hands(&mut self) {
        let idle = self.idle0 + f64::from(self.steps) / 60.0;
        self.rig.pose(&mut self.world, HERO, idle);
        self.world.place_hung(&mut self.bench);
    }

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

    /// One step: the pads read, the projectiles managed (once he's in),
    /// every placement's tick, the clones of scripted templates added, then
    /// the engine's frame: the hero moved by the walk his scripts left, the
    /// sequences on, what the scripts threw flying.
    fn step(&mut self) -> Result<(), Halt> {
        let pads = self.bench.pads;
        self.world.read_pads(pads);
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
            hero.step_engine(&self.bench.world, dt, &[], Surfaces::NONE);
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;
        self.pose_hands();
        Ok(())
    }
}

/// The level's collision mesh and where the hero starts.
fn chapter_1(dump: &Path) -> (Body, Vec3) {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let level = levels::LEVELS[0];
    let archive = std::fs::read(levels::archive_path(&usrdir, level)).expect("the level archive");
    let bld = levels::level_bld(&archive).expect("its level.bld");
    let igz = Igz::parse(&bld).expect("the level parses");
    let placements = levels::placements(&igz, level, identity())
        .expect("the placements")
        .ir;
    let collision = levels::collision(&igz, level, identity())
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

/// Eruptor's rig as his archive holds it, read with the globals his
/// model's shaders need: his model, his palette and his attach nodes, at his
/// scale in the level (AH.50).
fn eruptor_rig(part: &[Vec<u8>]) -> guacamole::hands::Rig {
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

/// Each live-table member the registry doesn't hold, as a value cell of its
/// registered type, by name: the discovery aid.
fn discovery(config: &mut guac_script::Config) -> HashMap<MemberId, String> {
    let Ok(path) = std::env::var("AH17_LIVE_TABLE") else {
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

/// Every live-table member by id (`AH17_NAMES`), to name a halt's.
fn names_of_live_table() -> Option<HashMap<MemberId, String>> {
    let table = std::fs::read_to_string(std::env::var("AH17_NAMES").ok()?).ok()?;
    let mut names = HashMap::new();
    for row in table.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = row.split('\t').collect();
        let ns = c[0].to_ascii_lowercase();
        let ns = ns.strip_prefix("tfb").unwrap_or(&ns).to_owned();
        let name = c[2].to_ascii_lowercase();
        let name = name.trim_start_matches('-').to_owned();
        names.insert(
            MemberId::named(&ns, &name),
            format!("{ns}.{name} [{}]", c[3]),
        );
    }
    Some(names)
}

/// The run over the dump, the init chain's VM and the host's world, the
/// hero on the level's collision or, `open`, on the parry twin's open
/// ground (AG.4's scenarios: level ground at y = 0, material 1); the join
/// is [`Run::join`]'s.
fn run_on(mode: Mode, open: bool) -> (Run, HashMap<MemberId, String>) {
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
    let read: Vec<(u16, &Igz<'_>)> = [1u16, 2, 4, ERUPTOR]
        .iter()
        .map(|&u| (u, &igzs[u as usize]))
        .collect();
    let files = Files::read(&read).unwrap();
    let handles = headless_handles(&files);
    let objects = headless_objects(&handles);
    let mut config = scripts::hero_config(mode);
    let found = discovery(&mut config);
    let vm = scripts::init_chain(program, &files, &handles, &ORDER, config);
    let mut world = ScriptWorld::new(&files, &handles, objects);
    let template = files
        .placed
        .iter()
        .find(|p| p.name == "009_Eruptor")
        .expect("Eruptor's template")
        .obj;
    let rig = eruptor_rig(&part);
    world.set_palette(template, lengths(&rig.palette));
    let (parry, start) = if open {
        let ground = scenarios::bodies(&scenarios::open_ground(), &[]);
        (ParryWorld::new(&ground).unwrap(), Vec3::new(0.0, 1.0, 0.0))
    } else {
        let (mesh, start) = chapter_1(&dump);
        (
            ParryWorld::new(&[mesh]).expect("the level's collision"),
            start,
        )
    };
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, start, 0.0);
    hero.settle(&parry, &[]);
    let bench = Bench {
        heroes: HashMap::from([(HERO, hero)]),
        world: parry,
        view: 15.0,
        pads: [Pad::default(); 2],
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
            rig,
            idle0: 0.0,
        },
        found,
    )
}

impl Run {
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

/// The run joined and settled: the init chain's two steps, the join, then
/// 120 steps for his spawn sequence to play out (0.77 s).
fn joined() -> Run {
    joined_on(false)
}

/// [`joined`] on the level or, `open`, the parry twin's open ground.
fn joined_on(open: bool) -> Run {
    let (mut run, _) = run_on(Mode::Strict, open);
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    run
}

/// A clone the scripts made, by its template's name.
#[derive(Debug, Clone)]
struct Spawned {
    step: u32,
    name: String,
    handle: Handle,
    /// Each step it flew: where it ended and its velocity, ft/s.
    flight: Vec<(u32, Vec3, Vec3)>,
    /// The step a remove took it off, and where it was.
    gone: Option<(u32, Vec3)>,
}

/// Steps `run` `steps` times under the input script (`scripted`'s form),
/// the step numbered from 0; what the scripts spawned, how each flew and
/// when it went.
fn press(run: &mut Run, script: &str, steps: u32) -> Vec<Spawned> {
    let mut seen: Vec<Spawned> = Vec::new();
    let before = run.world.clones();
    for k in 0..steps {
        press_step(run, script, k, &before, &mut seen);
    }
    seen
}

/// Step `k` of [`press`]: the pad, the step, and each clone made since
/// `before` noted in `seen`.
fn press_step(run: &mut Run, script: &str, k: u32, before: &[Handle], seen: &mut Vec<Spawned>) {
    run.bench.pads[0] = scripted(script, k);
    run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
    for c in run.world.clones() {
        if before.contains(&c) {
            continue;
        }
        let a = run.world.actor(c).unwrap();
        let i = match seen.iter().position(|s| s.handle == c) {
            Some(i) => i,
            None => {
                let name = a
                    .template
                    .and_then(|t| run.world.name_of(t))
                    .unwrap_or_default();
                seen.push(Spawned {
                    step: k,
                    name,
                    handle: c,
                    flight: Vec::new(),
                    gone: None,
                });
                seen.len() - 1
            }
        };
        let s = &mut seen[i];
        if s.gone.is_some() {
            continue;
        }
        if a.removed {
            s.gone = Some((k, a.position));
        } else if a.launched {
            s.flight.push((k, a.position, a.velocity));
        }
    }
}

/// A recording under `$GUAC_CAPTURE_DIR/BLUS30968/`, if it's there.
fn recording(path: &str) -> Option<serde_json::Value> {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR")?;
    let text = std::fs::read_to_string(Path::new(&root).join("BLUS30968").join(path)).ok()?;
    serde_json::from_str(&text).ok()
}

fn vec3(v: &serde_json::Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap() as f32;
    Vec3::new(f(0), f(1), f(2))
}

/// A behaviour an actor entered (none: no script, or gone) and the game
/// frame it entered it on.
type Stay = (Option<String>, i64);

/// The oracle's drives the stall detector's limits on him and his clones
/// come from: AH.9's from anchor A (`erupt`, `lob`) and AH.50's from the
/// spawn (`west`, `south`), ten each.
const LIMIT_DRIVES: [(&str, &str); 4] = [
    ("ah9-main-1792545846", "erupt"),
    ("ah9-main-1792545846", "lob"),
    ("ah50-main-1792638637", "west"),
    ("ah50-main-1792638637", "south"),
];

/// The stall detector's limits on him and the clones his attacks make (the
/// eruption's `LavaBlobbing`, `Going Black` and `Fading`, AH.50), from
/// [`LIMIT_DRIVES`]: each stay of an actor in one behaviour with a change
/// on both sides, a clone's spawn and its last listing counting as one,
/// its behaviour's longest by the names each drive's `chain.json` resolves,
/// lowercased as the VM names them, a frame and a half over (whole frames,
/// and a frame's time on the oracle's clock); by the placement's name, the
/// clones by their template's. The hero holds one behaviour throughout every
/// drive, so none is his. And how many drives it read.
fn eruptor_limits() -> (Vec<StallLimit>, usize) {
    let mut read = 0;
    let mut longest: BTreeMap<(String, String), i64> = BTreeMap::new();
    for (folder, kind) in LIMIT_DRIVES {
        for i in 0..10 {
            let drive = format!("{folder}/{kind}-{i:02}");
            let (Some(records), Some(chain)) = (
                recording(&format!("{drive}/chain-records.json")),
                recording(&format!("{drive}/chain.json")),
            ) else {
                continue;
            };
            read += 1;
            let names = &chain["resolved"]["behaviours"];
            // Per actor (its kind and address): its behaviour per listing,
            // with the game frame, and whether it's a clone.
            let mut stays: BTreeMap<(String, i64), Vec<Stay>> = BTreeMap::new();
            let mut clones: BTreeMap<(String, i64), bool> = BTreeMap::new();
            for rec in records.as_array().into_iter().flatten() {
                let frame = rec["game_frame"].as_i64().unwrap();
                for it in rec["items"].as_array().into_iter().flatten() {
                    let k = it["kind"].as_str().unwrap_or_default();
                    let template = match k {
                        "hero" => "009_Eruptor",
                        "eruption" => "009_LavaAttack",
                        "lava_ball" => "009_LavaProjectile",
                        "fire_blob_splash" => "009_FireBlobSplash",
                        "lava_collider" => "009_LavaCollider",
                        _ => continue,
                    };
                    let key = (template.to_owned(), it["address"].as_i64().unwrap());
                    clones.insert(key.clone(), k != "hero");
                    let b = it["behaviour"].as_i64().filter(|&a| a != 0).map(|a| {
                        names[a.to_string()]
                            .as_str()
                            .unwrap_or_else(|| panic!("{drive}: behaviour {a} unnamed"))
                            .to_lowercase()
                    });
                    let s = stays.entry(key).or_default();
                    if s.last().is_none_or(|l| l.0 != b) {
                        s.push((b, frame));
                    }
                }
            }
            let last = records
                .as_array()
                .and_then(|r| r.last())
                .and_then(|r| r["game_frame"].as_i64())
                .unwrap_or(0);
            for (key, mut s) in stays {
                let clone = clones[&key];
                // A clone's last listing ends its last stay, unless it's
                // still listed at the drive's end.
                let gone = records
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|r| {
                        r["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|i| i["address"].as_i64() == Some(key.1))
                    })
                    .filter_map(|r| r["game_frame"].as_i64())
                    .max()
                    .map(|f| f + 1);
                if clone && let Some(g) = gone.filter(|&g| g <= last) {
                    s.push((None, g));
                }
                let skip = usize::from(!clone);
                for w in s.windows(2).skip(skip) {
                    if let Some(b) = &w[0].0 {
                        let e = longest.entry((key.0.clone(), b.clone())).or_insert(0);
                        *e = (*e).max(w[1].1 - w[0].1);
                    }
                }
            }
        }
    }
    let limits = longest
        .into_iter()
        .map(|((placement, behaviour), frames)| StallLimit {
            placement,
            behaviour,
            longest_stay: (frames as f32 + 1.5) / 60.0,
        })
        .collect();
    (limits, read)
}

/// His scripts for 600 steps from the join, standing: no unmodelled member,
/// the stall detector clean against the oracle's records (none of them
/// moves him off his one behaviour, so none holds him to one).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn his_scripts_run_600_steps_standing() {
    println!("load before: {}", load());
    let started = Instant::now();
    let mut run = joined();
    let minion = run.minion.unwrap();
    let (limits, read) = eruptor_limits();
    println!("{read} recordings: limits {limits:?}");
    assert!(
        limits.iter().all(|l| l.placement != "009_Eruptor"),
        "a limit on him: {limits:?}"
    );
    let mut detector = StallDetector::new(limits);
    for k in 0..600 {
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let now = run.steps as f32 / 60.0;
        let stall = detector.observe(now, "009_Eruptor", run.vm.behaviour(minion));
        assert_eq!(stall, None);
    }
    println!(
        "minion behaviour {:?}, {} steps, {} ops, {:.2?}",
        run.vm.behaviour(minion),
        run.steps,
        run.vm.executed(),
        started.elapsed()
    );
    println!("load after: {}", load());
    assert_eq!(run.vm.behaviour(minion), Some("default"));
}

/// The members his scripts reach over a walk, two throws and an eruption,
/// 600 steps: each one the VM asked a value of registered (strict mode halts
/// on any other), the rest reached only on a path through nothing (a
/// viewport effect the camera's cell doesn't hold); the count against AH.34's
/// ceiling.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_members_he_reaches_stay_under_the_ceiling() {
    println!("load before: {}", load());
    let mut run = joined();
    press(
        &mut run,
        "stick:0:1@0-120,a1@130,a1@170,a2@220,stick:1:0@400-460,stick:-0.5:-0.5@470-560",
        600,
    );
    let name = |m: &MemberId| run.vm.config().members.name(*m);
    let registered = |m: &MemberId| {
        INIT_CHAIN
            .iter()
            .chain(HERO_MEMBERS)
            .chain(COMBAT)
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
    let mut all: Vec<String> = reached.iter().map(name).collect();
    all.sort();
    let past: Vec<String> = asked
        .iter()
        .filter(|m| !registered(m))
        .map(|m| name(m))
        .collect();
    println!(
        "members reached {} ({} the VM's own), {} asked a value; reached only through nothing: \
         {through_nothing:?}",
        reached.len(),
        reached
            .iter()
            .filter(|m| guac_script::Builtin::of(**m).is_some())
            .count(),
        asked.len()
    );
    println!("reached: {all:?}");
    if std::env::var("AH17_USES").is_ok() {
        let mut u: Vec<(String, u64, u64)> = run
            .vm
            .uses()
            .iter()
            .map(|(m, (r, w))| (name(m), *r, *w))
            .collect();
        u.sort();
        for (n, r, w) in u {
            println!("  uses {n}: reads {r} writes {w}");
        }
    }
    println!("load after: {}", load());
    assert!(past.is_empty(), "{past:?}");
    assert!(
        reached.len() <= 324,
        "past AH.34's ceiling: {}",
        reached.len()
    );
}

/// AH.50's South press at the spawn (`ah50-main-1792638637/<drive>`): the
/// first throw's release as the oracle has it, in his frame from his pivot
/// (facing 0 at the spawn, so the world's axes): the collider's place at
/// the record before the ball's first (where the ball leaves from), his
/// idle's and the throw's times there, the ball's first record and the
/// last two of its flight, and his pivot.
struct OracleThrow {
    idle: f64,
    throw: f64,
    collider: Vec3,
    first: Vec3,
    last: Vec3,
    burst: Vec3,
    pivot: Vec3,
}

fn oracles_south(drive: &str) -> Option<OracleThrow> {
    let records = recording(&format!("ah50-main-1792638637/{drive}/chain-records.json"))?;
    let rec = recording(&format!("ah50-main-1792638637/{drive}/recording.json"))?;
    let chain = recording(&format!("ah50-main-1792638637/{drive}/chain.json"))?;
    let names = &chain["resolved"]["clip_names"];
    let at = |r: &serde_json::Value, kind: &str| -> Option<(i64, Vec3)> {
        r["items"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == kind)
            .and_then(|i| Some((i["address"].as_i64()?, vec3(&i["body"]["position"]))))
    };
    let records = records.as_array()?;
    let first_at = records.iter().position(|r| at(r, "lava_ball").is_some())?;
    let (ball, _) = at(&records[first_at], "lava_ball")?;
    let flight: Vec<Vec3> = records[first_at..]
        .iter()
        .filter_map(|r| {
            r["items"]
                .as_array()?
                .iter()
                .find(|i| i["kind"] == "lava_ball" && i["address"].as_i64() == Some(ball))
                .map(|i| vec3(&i["body"]["position"]))
        })
        .collect();
    let release = &records[first_at - 1];
    let pivot = at(release, "hero")?.1;
    let frame = release["game_frame"].as_i64()?;
    let clips = rec
        .as_array()?
        .iter()
        .find(|r| r["game_frame"].as_i64() == Some(frame))?["clips"]
        .clone();
    let time_of = |stem: &str| -> Option<f64> {
        clips.as_array()?.iter().find_map(|c| {
            let name = names[c["id"].as_i64()?.to_string()].as_str()?;
            (name.ends_with(stem) && c["weight"].as_f64()? > 0.0)
                .then(|| c["period"].as_f64().unwrap() - c["remaining"].as_f64().unwrap())
        })
    };
    Some(OracleThrow {
        idle: time_of("Eruptor_Idle.ma")?,
        throw: time_of("Eruptor_RunAttackLeft.ma")?,
        collider: at(release, "lava_collider")?.1 - pivot,
        first: flight[0] - pivot,
        last: flight[flight.len() - 2] - pivot,
        burst: flight[flight.len() - 1] - pivot,
        pivot,
    })
}

/// A press of South from rest at the spawn: the throw's clip at once, the
/// ball four steps into it (AH.7's and AH.9's five game frames after the
/// press, the pad read a frame later there), 80 ft/s at 65° less a frame of
/// gravity, 3° left of his facing, from his left hand posed by his idle and
/// the throw (AH.50: where the oracle's collider stands on that hand at the
/// release, his idle set to the oracle's time there), and its burst on the
/// clearing's floor where the oracle's comes down.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn south_throws_a_ball_from_his_hand_that_bursts_on_the_clearing_floor() {
    println!("load before: {}", load());
    let oracle = oracles_south("south-00");
    let mut run = joined();
    let pivot = run.bench.heroes[&HERO].pivot;
    // The ball leaves at step 14 from the collider his hand left at step
    // 13's pose, the run's steps then 14 on from now.
    if let Some(o) = &oracle {
        run.idle0 = o.idle - f64::from(run.steps + 14) / 60.0;
    }
    let seen = press(&mut run, "a1@10", 120);
    let throw = run
        .world
        .sequences()
        .iter()
        .find(|q| q.actor == HERO && q.rate == 150.0)
        .expect("a throw")
        .clone();
    assert_eq!((throw.length, throw.rate), (0.5, 150.0), "his left lob");
    let ball = seen
        .iter()
        .find(|s| s.name == "009_LavaProjectile")
        .expect("a ball");
    assert_eq!(ball.step, 14, "four steps into the throw");
    let (_, first, v) = ball.flight[0];
    // Its first record less the frame's move (ft/s to m).
    let launch = first - v * (0.3048 / 60.0) - pivot;
    let speed = v.length();
    let pitch = guacamole::script_engine::pitch_of(v);
    let heading = guacamole::script_engine::heading_of(v);
    let (gone, burst) = ball.gone.expect("it burst");
    let across = Vec3::new(burst.x - pivot.x, 0.0, burst.z - pivot.z).length();
    println!(
        "ball from step {} for {} records, launched at {launch:?} from his pivot, first {first:?} at \
         {speed:.3} ft/s, {pitch:.3} deg up, heading {heading:.3}; burst at step {gone} at {burst:?}, \
         {across:.3} m across from his pivot",
        ball.step,
        ball.flight.len()
    );
    assert!((speed - 76.99).abs() < 0.02, "{speed}");
    assert!((pitch - 63.95).abs() < 0.02, "{pitch}");
    assert!((heading + 3.0).abs() < 0.01, "{heading}");
    // From his left hand: up and ahead of him, never at his pivot.
    assert!(
        launch.x > 0.0 && launch.y > 1.0 && launch.z > 1.0,
        "{launch:?}"
    );
    // On the floor: the gap under it to the level's ground no more than
    // the Floor check's quarter foot.
    let down = guac_motion::ground::ray(
        &run.bench.world,
        burst + Vec3::Y * 0.1,
        Vec3::NEG_Y,
        10.0,
        &[],
    )
    .expect("floor under the burst");
    let gap = (burst.y - 0.712 - down.point.y) / 0.3048;
    assert!(gap <= 0.25, "{gap} ft off the floor");
    if let Some(o) = &oracle {
        let flat = |p: Vec3| Vec3::new(p.x, 0.0, p.z).length();
        println!(
            "oracle (south-00): released with the throw at {:.4} s and his idle at {:.4} s from \
             {:?}, first {:?}; last in flight {:.3} m across, burst {:.3} m across at {:.3} m up; \
             his pivot {:?} (ours {pivot:?})",
            o.throw,
            o.idle,
            o.collider,
            o.first,
            flat(o.last),
            flat(o.burst),
            o.burst.y,
            o.pivot
        );
        // The hand moves about 1.9 m a frame there (0.025 s of the clip), so
        // the oracle's throw a tenth of a millisecond off ours moves it
        // millimetres: within a centimetre of the oracle's collider.
        assert!((launch - o.collider).length() < 0.01, "{launch:?}");
        assert!(
            across > flat(o.last) - 0.05 && across < flat(o.burst) + 0.05,
            "{across}"
        );
    }
    println!("load after: {}", load());
}

/// A press of West from rest: the eruption's clip (slot 32, 2 s at 140 %)
/// at once, the eruption 43 steps into it and gone 112 steps later, 1.867 s,
/// as AH.9's (44 game frames after the press, 1.867 s of game time).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn west_erupts_43_steps_into_the_clip_for_1_867_s() {
    println!("load before: {}", load());
    let mut run = joined();
    // The eruption's behaviours each step against the oracle's longest
    // stays (AH.50): its stays, and the detector clean.
    let (limits, read) = eruptor_limits();
    let names: Vec<&str> = limits
        .iter()
        .filter(|l| l.placement == "009_LavaAttack")
        .map(|l| l.behaviour.as_str())
        .collect();
    println!("{read} recordings: the eruption's limits {limits:?}");
    if read > 0 {
        assert_eq!(names, ["fading", "going black", "lavablobbing"]);
    }
    let mut detector = StallDetector::new(limits.clone());
    let mut stays: Vec<(Option<String>, u32)> = Vec::new();
    let mut stalls = Vec::new();
    let before = run.world.clones();
    let mut seen: Vec<Spawned> = Vec::new();
    for k in 0..240u32 {
        press_step(&mut run, "a2@10", k, &before, &mut seen);
        let now = run.steps as f32 / 60.0;
        let eruption = seen
            .iter()
            .find(|s| s.name == "009_LavaAttack")
            .map(|s| s.handle);
        if let Some(h) = eruption {
            let b = if run.world.removed(h) {
                None
            } else {
                run.vm
                    .placement_of(h)
                    .and_then(|id| run.vm.behaviour(id))
                    .map(str::to_owned)
            };
            stalls.extend(detector.observe(now, "009_LavaAttack", b.as_deref()));
            if stays.last().is_none_or(|l| l.0 != b) {
                stays.push((b, k));
            }
        }
    }
    // The step it's made, its script hasn't ticked yet (the oracle lists it
    // from the frame after, in its first behaviour): from its first.
    let stays: Vec<(Option<String>, u32)> = stays
        .windows(2)
        .map(|w| (w[0].0.clone(), w[1].1 - w[0].1))
        .skip_while(|s| s.0.is_none())
        .collect();
    println!("the eruption's stays (steps): {stays:?}; stalls {stalls:?}");
    assert!(stalls.is_empty(), "{stalls:?}");
    // The oracle's: LavaBlobbing 66-67 frames, Going Black 30-31, Fading
    // 15-16, taken off from it (never `Wait to Remove`).
    let names: Vec<Option<&str>> = stays.iter().map(|s| s.0.as_deref()).collect();
    assert_eq!(
        names,
        [Some("lavablobbing"), Some("going black"), Some("fading")]
    );
    for ((_, steps), range) in stays.iter().zip([66..=67, 30..=31, 15..=16]) {
        assert!(range.contains(steps), "{stays:?}");
    }
    let clip = run.world.sequences()[1].clone();
    assert_eq!((clip.length, clip.rate), (2.0, 140.0));
    let e = seen
        .iter()
        .find(|s| s.name == "009_LavaAttack")
        .expect("an eruption");
    let (gone, _) = e.gone.expect("it ends");
    let life = (gone - e.step) as f32 / 60.0;
    println!(
        "eruption from step {} ({} into the clip), gone at step {gone}: {} steps, {life:.4} s; {} spawns",
        e.step,
        e.step - 10,
        gone - e.step,
        seen.len()
    );
    assert_eq!(e.step - 10, 43);
    assert!((life - 1.867).abs() < 0.5 / 60.0, "{life}");
    println!("load after: {}", load());
}

/// The pad at step `k` with action `n` (1 or 2) pressed every other step
/// over [from, to): down on `from`, `from + 2`, ..., up between (AH.9's
/// `lobs` and `erupts`, O4's schedule).
fn pulsed(n: usize, from: u32, to: u32, k: u32) -> Pad {
    let mut pad = Pad {
        connected: true,
        ..Pad::default()
    };
    pad.actions[n - 1] = (from..to).contains(&k) && (k - from).is_multiple_of(2);
    pad
}

/// The steps each clip of `rate` on him started on and the steps each
/// ended on (done or stopped after that step's engine frame), from step 0
/// of `pads`.
fn clip_steps(run: &mut Run, rate: f32, steps: u32, pads: impl Fn(u32) -> Pad) -> ClipSteps {
    let mut out = ClipSteps::default();
    let mut seen = run.world.sequences().len();
    let mut playing: Vec<usize> = Vec::new();
    let before = run.world.clones();
    let mut clones: Vec<Handle> = Vec::new();
    for k in 0..steps {
        run.bench.pads[0] = pads(k);
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let sequences = run.world.sequences();
        for (i, s) in sequences.iter().enumerate().skip(seen) {
            if s.actor == HERO && s.rate == rate {
                out.started.push(k);
                playing.push(i);
            }
        }
        seen = sequences.len();
        playing.retain(|&i| {
            let s = &sequences[i];
            let over = s.done || s.stopped;
            if over {
                out.ended.push(k);
            }
            !over
        });
        for c in run.world.clones() {
            if before.contains(&c) || clones.contains(&c) {
                continue;
            }
            clones.push(c);
            let name = run
                .world
                .actor(c)
                .and_then(|a| a.template)
                .and_then(|t| run.world.name_of(t))
                .unwrap_or_default();
            out.spawned.push((k, name));
        }
    }
    out
}

#[derive(Debug, Default)]
struct ClipSteps {
    started: Vec<u32>,
    ended: Vec<u32>,
    /// Each clone made: the step and its template's name.
    spawned: Vec<(u32, String)>,
}

/// AH.9's cadences, headless (the AH.24 review): South pressed every other
/// step throws every 20 steps, hands in turn, each ball 4 steps into its
/// throw (AH.9's `lobs`: 20 game frames apart in 41 of 50 gaps, 22 in the
/// rest, never fewer; the ball 4 frames into the throw in all 60); West
/// pressed every other step erupts again 59 to 61 steps after the last
/// eruption's clip ended, its eruption 43 steps into the clip (AH.9's
/// `erupts`: 59 or 61 frames after, 43 in all 30); held, each fires once
/// (AH.7's 2 s of South, AH.9's `hold_west`).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn pressed_every_other_step_he_throws_and_erupts_at_ah9s_cadence() {
    println!("load before: {}", load());
    let mut run = joined();
    let lobs = clip_steps(&mut run, 150.0, 300, |k| pulsed(1, 10, 130, k));
    let gaps: Vec<u32> = lobs.started.windows(2).map(|w| w[1] - w[0]).collect();
    let balls: Vec<u32> = lobs
        .spawned
        .iter()
        .filter(|s| s.1 == "009_LavaProjectile")
        .map(|s| s.0)
        .collect();
    let into: Vec<i64> = lobs
        .started
        .iter()
        .map(|&t| {
            balls
                .iter()
                .find(|&&b| b >= t)
                .map_or(-1, |&b| (b - t) as i64)
        })
        .collect();
    let slots: Vec<f32> = run
        .world
        .sequences()
        .iter()
        .filter(|s| s.actor == HERO && s.rate == 150.0)
        .map(|s| s.length)
        .collect();
    println!(
        "South every other step from step 10 to 130: throws at {:?}, gaps {gaps:?}, balls at \
         {balls:?} ({into:?} into their throws), clip lengths {slots:?}",
        lobs.started
    );
    assert_eq!(lobs.started.len(), 6, "six throws in the two seconds");
    assert!(gaps.iter().all(|&g| g == 20 || g == 22), "{gaps:?}");
    assert!(into.iter().all(|&i| i == 4), "{into:?}");
    assert_eq!(balls.len(), lobs.started.len());

    let mut run = joined();
    let erupts = clip_steps(&mut run, 140.0, 330, |k| pulsed(2, 10, 310, k));
    let after: Vec<u32> = erupts
        .started
        .iter()
        .skip(1)
        .map(|&s| s - erupts.ended.iter().filter(|&&e| e < s).max().unwrap())
        .collect();
    let eruptions: Vec<u32> = erupts
        .spawned
        .iter()
        .filter(|s| s.1 == "009_LavaAttack")
        .map(|s| s.0)
        .collect();
    println!(
        "West every other step from step 10 to 310: eruptions' clips at {:?}, ended {:?}, the next \
         {after:?} steps after the last ended; eruptions at {eruptions:?}",
        erupts.started, erupts.ended
    );
    assert_eq!(erupts.started.len(), 3, "three in five seconds");
    assert!(after.iter().all(|&a| (59..=61).contains(&a)), "{after:?}");
    for (s, e) in erupts.started.iter().zip(&eruptions) {
        assert_eq!(e - s, 43, "the eruption 43 steps into its clip");
    }

    // Held: one of each.
    let mut run = joined();
    let held = clip_steps(&mut run, 150.0, 140, |k| {
        let mut pad = pulsed(1, 0, 0, k);
        pad.actions[0] = (10..130).contains(&k);
        pad
    });
    let mut run = joined();
    let held_west = clip_steps(&mut run, 140.0, 320, |k| {
        let mut pad = pulsed(2, 0, 0, k);
        pad.actions[1] = (10..310).contains(&k);
        pad
    });
    println!(
        "South held 2 s: throws at {:?}; West held 5 s: eruptions at {:?}",
        held.started, held_west.started
    );
    assert_eq!(held.started.len(), 1);
    assert_eq!(held_west.started.len(), 1);
    println!("load after: {}", load());
}

/// hero-movement's Top speed and Turning with his scripts driving him on
/// the parry twin's open ground, as AG.4's scenarios hold the stand-in to:
/// 90 % of top speed 0.1167 s from his first move, 6.553 m a game second
/// within a millimetre at full stick and 21.5 ft/s × the strength at part
/// stick; his first frame facing the stick's 15° at full stick and turning
/// at 720°/s at half; a full reversal walking
/// straight back the next frame at top speed, his facing round in
/// 0.13-0.15 s (1440°/s). His scripts move him a frame after his facing
/// turns, the movement update ticking before his own script (AG.38).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_walk_holds_top_speed_and_turning_with_the_vm_driving() {
    println!("load before: {}", load());
    let top = 21.5 * 0.3048;
    for strength in [1.0_f32, 0.5, 0.2] {
        let mut run = joined_on(true);
        let mut track = Vec::new();
        for k in 0..150u32 {
            run.bench.pads[0] = scenarios_stick(strength, k);
            run.step().unwrap();
            let h = &run.bench.heroes[&HERO];
            track.push((h.pivot, h.facing, h.velocity));
        }
        // 1440°/s × S: at full stick his first frame reaches the stick's
        // 15°, at half it turns 12° of it (720°/s, Part stick).
        let turned = (1440.0 * strength / 60.0).min(15.0);
        assert!(
            (track[0].1 - turned).abs() < 1e-4,
            "first frame facing {}",
            track[0].1
        );
        let first = track.iter().position(|t| t.2 != Vec3::ZERO).unwrap();
        let ninety = track
            .iter()
            .position(|t| Vec3::new(t.2.x, 0.0, t.2.z).length() >= 0.9 * top * strength)
            .unwrap();
        let (a, b) = (track[first + 59].0, track[first + 119].0);
        let metres = Vec3::new(b.x - a.x, 0.0, b.z - a.z).length();
        println!(
            "stick {strength}: moving from step {first}, 90 % at step {ninety} ({:.4} s on), \
             {metres:.5} m in the second game second, level {}",
            (ninety - first) as f32 / 60.0,
            track[first + 59].0.y == track[first + 119].0.y
        );
        assert_eq!(first, 1, "a frame after the stick");
        assert!((metres - top * strength).abs() < 1e-3, "{metres}");
        if strength == 1.0 {
            assert!(((ninety - first) as f32 / 60.0 - 0.1167).abs() < 1e-3);
        }
    }
    let mut run = joined_on(true);
    let script = "stick:0:1@0-90,stick:0:-1@90-150";
    let mut track = Vec::new();
    for k in 0..150u32 {
        run.bench.pads[0] = scripted(script, k);
        run.step().unwrap();
        let h = &run.bench.heroes[&HERO];
        track.push((h.walk.velocity, h.facing));
    }
    let before = track[89].0;
    let after = track[91].0;
    let round = (90..150)
        .find(|&k| (track[k].1 - 195.0).abs() < 1e-3)
        .unwrap();
    let swing = (round - 89) as f32 / 60.0;
    println!("reversal: walk {before:?} -> {after:?}, facing round at step {round}, {swing:.4} s");
    assert!(
        (after + before).length() < 1e-3,
        "straight back at top speed"
    );
    assert!((0.13..=0.15).contains(&swing), "{swing}");
    println!("load after: {}", load());
}

/// The pad AG.4's scenarios push: forward at `strength`.
fn scenarios_stick(strength: f32, _k: u32) -> Pad {
    Pad {
        stick: scenarios::stick(strength, 0.0),
        actions: [false; 4],
        connected: true,
    }
}

/// The pad at step `k` of an input script: `stick:x:y@a-b` holds the stick
/// over steps [a, b), `aN@k` holds action N for 6 steps from k.
fn scripted(script: &str, k: u32) -> Pad {
    let mut pad = Pad {
        connected: true,
        ..Pad::default()
    };
    for item in script.split(',').filter(|s| !s.is_empty()) {
        let (what, when) = item.split_once('@').unwrap();
        if let Some(xy) = what.strip_prefix("stick:") {
            let (x, y) = xy.split_once(':').unwrap();
            let (a, b) = when.split_once('-').unwrap();
            let (a, b): (u32, u32) = (a.parse().unwrap(), b.parse().unwrap());
            if (a..b).contains(&k) {
                pad.stick = Stick::new(x.parse().unwrap(), y.parse().unwrap());
            }
        } else if let Some(n) = what.strip_prefix('a') {
            let n: usize = n.parse().unwrap();
            let at: u32 = when.parse().unwrap();
            if (at..at + 6).contains(&k) {
                pad.actions[n - 1] = true;
            }
        }
    }
    pad
}

/// A development probe: an input script (`AH17_INPUT`) from 120 steps after
/// the join, for `AH17_STEPS` steps, the hero and what the scripts spawned
/// printed.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_input() {
    let script = std::env::var("AH17_INPUT").unwrap_or_default();
    let steps: u32 = std::env::var("AH17_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let (mut run, found) = run_on(Mode::Strict, std::env::var("AH17_OPEN").is_ok());
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    let mut result = Ok(());
    let mut actors_before: Vec<Handle> = run.world.clones();
    let mut live_before: Vec<Handle> = actors_before.clone();
    for k in 0..steps {
        run.bench.pads[0] = scripted(&script, k);
        run.vm.trace(Some(400_000));
        let calls_at: Option<u32> = std::env::var("AH17_CALLS")
            .ok()
            .and_then(|v| v.parse().ok());
        let before: HashMap<u32, u64> = run.vm.calls().clone();
        let stepped = run.step();
        if calls_at.is_some_and(|c| c == k || c + 1 == k) {
            let program = run.vm.program_arc();
            let mut called: Vec<(String, u64)> = run
                .vm
                .calls()
                .iter()
                .filter_map(|(d, c)| {
                    let n = c - before.get(d).copied().unwrap_or(0);
                    (n > 0).then(|| (program.defs[*d as usize].display.clone(), n))
                })
                .collect();
            called.sort();
            for (n, c) in called {
                println!("   step {k} called {c}x {n}");
            }
        }
        if let Err(h) = &stepped
            && let Some(names) = names_of_live_table()
        {
            for (id, name) in &names {
                if h.to_string()
                    .contains(&format!("{:08x}.{:08x}", id.namespace, id.name))
                {
                    println!("  the halt's member: {name}");
                }
            }
        }
        if let Err(h) = stepped {
            let traced = run.vm.traced();
            let program = run.vm.program_arc();
            let keep: usize = std::env::var("AH17_TRACE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(40);
            for (sid, pos) in traced.iter().rev().take(keep).rev() {
                let name = program.scripts[*sid as usize]
                    .name
                    .rsplit('/')
                    .next()
                    .unwrap();
                println!("  trace {name} #{pos}");
            }
            result = Err(h);
            break;
        }
        let clones = run.world.clones();
        for c in clones.iter().filter(|c| !actors_before.contains(c)) {
            let a = run.world.actor(*c).unwrap();
            let name = a
                .template
                .and_then(|t| run.world.name_of(t))
                .unwrap_or_default();
            println!("step {k}: spawned {name} ({c:#x}) at {:?}", a.position);
        }
        for c in &clones {
            let a = run.world.actor(*c).unwrap();
            if a.launched && !a.removed && std::env::var("AH17_FLY").is_ok() {
                println!(
                    "step {k}: flying {c:#x} at {:?} v {:?} ({:.2} ft/s) contacts {}",
                    a.position,
                    a.velocity,
                    a.velocity.length(),
                    a.contacts.len()
                );
            }
        }
        for c in live_before.iter().filter(|c| run.world.removed(**c)) {
            let a = run.world.actor(*c).unwrap();
            let name = a
                .template
                .and_then(|t| run.world.name_of(t))
                .unwrap_or_default();
            if a.launched || std::env::var("AH17_REMOVED").is_ok() {
                println!(
                    "step {k}: removed {name} {c:#x} at {:?} launched {}",
                    a.position, a.launched
                );
            }
        }
        live_before = clones
            .iter()
            .copied()
            .filter(|c| !run.world.removed(*c))
            .collect();
        actors_before = clones;
        if std::env::var("AH17_PHYS").is_ok() && k == 0 {
            for p in run.files.placed.iter().filter(|p| p.obj.unit == ERUPTOR) {
                println!(
                    "template {}: physics {:?} solid {:?} scale {:?}",
                    p.name, p.physics, p.solid, p.scale
                );
            }
        }
        let h = &run.bench.heroes[&HERO];
        let every: u32 = std::env::var("AH17_EVERY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        if k % every == 0 {
            println!(
                "step {k}: pivot {:?} facing {:.3} walk {:?} {:.4} ft/s",
                h.pivot,
                h.facing,
                h.walk.velocity,
                h.walk.velocity.length()
            );
        }
    }
    for (i, q) in run.world.sequences().iter().enumerate() {
        println!("seq {i}: {q:?}");
    }
    report(&run, &found);
    result.unwrap_or_else(|h| panic!("{h}"));
}

/// A development probe: Eruptor's attach list, and nodes 7 and 8 posed by
/// his throw clips, in his model's frame.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_nodes() {
    let dump = dump();
    let usr = dump.join("PS3_GAME/USRDIR");
    let part: Vec<Vec<u8>> = ARCHIVES
        .iter()
        .map(|(a, _)| guac_extract::scripts::entry(&usr, a).unwrap_or_else(|e| panic!("{a}: {e}")))
        .collect();
    let igz = Igz::parse(&part[3]).unwrap();
    let portal = guac_formats::model::portal_model(&igz).unwrap();
    let portal = guac_formats::model::in_play(&igz, portal, "009_Eruptor").unwrap();
    let hero = guac_formats::model::hero_actor(&igz, &portal, "009_Eruptor").unwrap();
    let model = hero.info.model.unwrap_or(portal.model);
    let points = guac_formats::attachments::attach_points(&igz, model).unwrap();
    let globals = guac_extract::models::Globals::new(&part[1], &part[0]).expect("the globals");
    let read = guac_extract::models::read(&part[3], "009_Eruptor", identity(), Some(&globals))
        .expect("Eruptor's model");
    let skeleton = &read.model.skeleton;
    let mut joints = Vec::new();
    for (i, p) in points.iter().enumerate() {
        let bone = p.bone_name.unwrap_or_default();
        let j = skeleton.joints.iter().position(|j| j.name == bone);
        println!(
            "node {i}: {:?} bone {bone} ({}) joint {j:?} at {:?} turned {:?}",
            p.name, p.bone_index, p.transform.position, p.transform.rotation
        );
        joints.push((j, guac_extract::models::euler_transform(&p.transform)));
    }
    let clips: HashMap<u16, guac_ir::clip::Clip> = read
        .palette
        .entries
        .iter()
        .filter_map(|e| match &e.entry {
            guac_ir::palette::SlotEntry::Clip { clip, .. } => Some((e.slot, clip.clone())),
            _ => None,
        })
        .collect();
    for slot in [20u16, 21] {
        let Some(clip) = clips.get(&slot) else {
            println!("slot {slot}: no clip");
            continue;
        };
        println!(
            "slot {slot}: {} {:.4} s, {} frames",
            clip.name, clip.duration, clip.frames
        );
        for k in 0..8 {
            let t = f64::from(k) / 60.0 * 1.5;
            let pose = clip.model_pose(skeleton, t);
            for n in [7usize, 8] {
                let (Some(j), off) = joints[n] else { continue };
                let jt = &pose[j];
                let at = Vec3::from_array(jt.translation)
                    + bevy_quat(jt.rotation)
                        * (Vec3::from_array(off.translation) * Vec3::from_array(jt.scale));
                println!("  step {k} t {t:.4}: node {n} at {at:?}");
            }
        }
    }
}

fn bevy_quat(q: [f32; 4]) -> guac_motion::seam::Quat {
    guac_motion::seam::Quat::from_array(q)
}

/// A development probe (AH.24): what a South and a West press spawn, each
/// clone's template with its script and model, and his health and his
/// figure's data as his scripts hold them.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_spawns() {
    let mut run = joined();
    let minion = run.minion.unwrap();
    for (name, v) in [
        ("description", "creature info"),
        ("my Minion Description", "player joined data"),
    ] {
        let d = run.vm.local(minion, name);
        println!("local {name}: {d:?}");
        if let Some(d) = d {
            let f = run.vm.field(d, v);
            println!("  .{v}: {f:?}");
            if let Some(f) = f {
                for (n, x) in run.vm.fields(f) {
                    println!("    .{n} = {x:?} -> {:?}", run.vm.value(x));
                }
            }
        }
    }
    let seen = press(&mut run, "a1@10,a2@100", 300);
    for s in &seen {
        let a = run.world.actor(s.handle).unwrap();
        let t = a
            .template
            .and_then(|t| run.files.placed.iter().find(|p| p.obj == t));
        println!(
            "step {} {} script {:?} model {:?} flew {} gone {:?} scale {:?}",
            s.step,
            s.name,
            t.and_then(|p| p.script.clone()),
            t.and_then(|p| p.model.clone()),
            s.flight.len(),
            s.gone.map(|g| g.0),
            a.scale
        );
    }
    for p in run.files.placed.iter().filter(|p| p.obj.unit == ERUPTOR) {
        println!(
            "placement {} state {} script {:?} model {:?}",
            p.name, p.activity_state, p.script, p.model
        );
    }
    for (i, q) in run.world.sequences().iter().enumerate() {
        println!(
            "seq {i}: actor {:#x} len {} rate {} done {}",
            q.actor, q.length, q.rate, q.done
        );
    }
}

/// A development probe (AH.24): the eruption's clones step by step after
/// they spawn: where each is, its scale, what it hangs from, its velocity.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_eruption() {
    let mut run = joined();
    let before = run.world.clones();
    for k in 0..200u32 {
        run.bench.pads[0] = scripted("a2@5", k);
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        if !(48..=60).contains(&k) && k % 20 != 0 {
            continue;
        }
        for c in run.world.clones() {
            if before.contains(&c) {
                continue;
            }
            let a = run.world.actor(c).unwrap();
            let name = a
                .template
                .and_then(|t| run.world.name_of(t))
                .unwrap_or_default();
            if !name.starts_with("009_Lava") {
                continue;
            }
            println!(
                "step {k} {name} {c:#x} at {:?} scale {:?} angles {:?} attach {:?} v {:?} launched {} removed {}",
                a.position, a.scale, a.angles, a.attach, a.velocity, a.launched, a.removed
            );
        }
    }
}

/// AH.50's drives with throws in them, each a fresh RPCS3 from its state
/// with O6's actor chain on `009_LavaCollider`: from the spawn, South twice
/// (a hand each); from anchor A, AH.9's lob at the barrel; from anchor B,
/// AH.10's lob at F0 + 90 at a Chompy.
const HAND_DRIVES: [(&str, usize); 3] = [("south", 10), ("lob_a", 10), ("fight_lob", 5)];

/// AG.1's step residual: what an O5 trace's own steps leave unexplained.
const STEP_RESIDUAL: f32 = 1.24e-5;

/// Eruptor's nodes 7 and 8 (`lHand`, `rHand`) posed off his model's attach
/// list (AH.50): each record of AH.50's drives a throw plays in, the
/// `009_LavaCollider` the throw hangs on the hand it throws from, in his
/// frame (his whole turn, the tilt and roll the ground under him gives his
/// model as well as his facing), against that hand posed by the clips the
/// record names at their times (his locomotion's, the throw laid over them)
/// at his scale: within AG.1's step residual, the lean his turn plays (6 %
/// at most, no clip of our locomotion's yet) left out and moving no joint
/// up to his hands. Named, AH.49's: the frames after a throw's clip has gone
/// from the list while the engine still blends it out (the collider's hand
/// off the idle's by up to 3.4 m, back within the residual inside 13
/// frames). A record with a clip the end pause named nothing for (a bite's
/// hurt reaction) is left out.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.50's recordings; run on the Mac"]
fn his_hands_posed_from_his_attach_list_stand_where_the_oracles_collider_does() {
    println!("load before: {}", load());
    let dump = dump();
    let usr = dump.join("PS3_GAME/USRDIR");
    let part: Vec<Vec<u8>> = ARCHIVES
        .iter()
        .map(|(a, _)| guac_extract::scripts::entry(&usr, a).unwrap_or_else(|e| panic!("{a}: {e}")))
        .collect();
    let rig = eruptor_rig(&part);
    assert_eq!(
        rig.nodes.nodes[7..9]
            .iter()
            .map(|n| (n.name.as_deref(), n.bone.as_deref()))
            .collect::<Vec<_>>(),
        [
            (Some("lHand"), Some("J_palm_L")),
            (Some("rHand"), Some("J_palm_R"))
        ]
    );
    let mut clips: HashMap<String, &guac_ir::clip::Clip> = rig
        .palette
        .entries
        .iter()
        .filter_map(|e| match &e.entry {
            guac_ir::palette::SlotEntry::Clip { clip, .. } => Some((clip.name.clone(), clip)),
            _ => None,
        })
        .collect();
    clips.insert(rig.model.idle.name.clone(), &rig.model.idle);
    let only = std::env::var("AH50_REC").ok();
    let (mut drives, mut records, mut leaning, mut unnamed) = (0, 0, 0, 0);
    let (mut worst, mut worst_lean, mut worst_out): (f32, f32, f32) = (0.0, 0.0, 0.0);
    let mut longest_out = 0;
    for (kind, n) in HAND_DRIVES {
        for i in 0..n {
            let drive = format!("ah50-main-1792638637/{kind}-{i:02}");
            if only.as_ref().is_some_and(|o| !drive.ends_with(o.as_str())) {
                continue;
            }
            let (Some(chain_records), Some(rec), Some(chain)) = (
                recording(&format!("{drive}/chain-records.json")),
                recording(&format!("{drive}/recording.json")),
                recording(&format!("{drive}/chain.json")),
            ) else {
                continue;
            };
            drives += 1;
            let names = &chain["resolved"]["clip_names"];
            let by_frame: HashMap<i64, &serde_json::Value> = rec
                .as_array()
                .unwrap()
                .iter()
                .map(|r| (r["game_frame"].as_i64().unwrap(), r))
                .collect();
            let mut hand: Option<usize> = None;
            let mut drive_worst: f32 = 0.0;
            // Frames since a throw's clip left the list while its hand is
            // still off the idle's.
            let mut out = 0;
            for r in chain_records.as_array().unwrap() {
                let g = r["game_frame"].as_i64().unwrap();
                let items = r["items"].as_array().unwrap();
                let he = items.iter().find(|i| i["kind"] == "hero").unwrap();
                let Some(col) = items.iter().find(|i| i["kind"] == "lava_collider") else {
                    continue;
                };
                let Some(frame) = by_frame.get(&g) else {
                    continue;
                };
                let mut base = Vec::new();
                let mut over = Vec::new();
                let mut lean = false;
                let mut named = true;
                for c in frame["clips"].as_array().unwrap() {
                    let w = c["weight"].as_f64().unwrap();
                    if w <= 0.0 {
                        continue;
                    }
                    let id = c["id"].as_i64().unwrap().to_string();
                    let Some(name) = names[&id].as_str().and_then(|n| n.rsplit('/').next()) else {
                        named = false;
                        continue;
                    };
                    let t = c["period"].as_f64().unwrap() - c["remaining"].as_f64().unwrap();
                    if name.contains("Lean") {
                        // Not his locomotion's on our side yet (AH.49).
                        lean = true;
                        continue;
                    }
                    let clip = clips
                        .get(name)
                        .unwrap_or_else(|| panic!("{drive}: {name} not in his palette"));
                    if name.contains("Attack") {
                        hand = Some(if name.contains("Right") { 8 } else { 7 });
                        over.push((*clip, t));
                    } else {
                        base.push((*clip, t, w));
                    }
                }
                // Hung from the first throw on: before it, it lies where
                // his join dropped it.
                let Some(node) = hand else {
                    continue;
                };
                if !named {
                    unnamed += 1;
                    continue;
                }
                let posed = guacamole::hands::posed(
                    &rig.model.skeleton,
                    &rig.nodes,
                    &base,
                    &over,
                    rig.scale,
                );
                let angles = vec3(&he["body"]["rotation"]);
                let turn = guac_extract::models::euler_transform(
                    &guac_formats::igz::layouts::EulerTransform {
                        position: [0.0; 3],
                        rotation: angles.to_array(),
                    },
                );
                let turn = guac_motion::seam::Quat::from_array(turn.rotation);
                let at = vec3(&he["body"]["position"]) + turn * posed[node];
                let off = (at - vec3(&col["body"]["position"])).length();
                records += 1;
                if lean {
                    leaning += 1;
                    worst_lean = worst_lean.max(off);
                }
                if over.is_empty() && off > STEP_RESIDUAL {
                    out += 1;
                    longest_out = longest_out.max(out);
                    worst_out = worst_out.max(off);
                } else {
                    out = 0;
                    drive_worst = drive_worst.max(off);
                }
            }
            println!("{drive}: worst {drive_worst:.2e} m");
            worst = worst.max(drive_worst);
        }
    }
    println!(
        "{drives} drives, {records} records: worst {worst:.2e} m; {leaning} with a lean, worst there \
         {worst_lean:.2e} m; a throw blending out off by {worst_out:.3} m at most, {longest_out} frames \
         at most; {unnamed} with a clip the end pause didn't name"
    );
    println!("load after: {}", load());
    if drives > 0 {
        assert!(worst <= STEP_RESIDUAL, "{worst}");
        assert!(worst_lean <= STEP_RESIDUAL, "{worst_lean}");
        assert!(longest_out <= 13, "{longest_out}");
    }
}
