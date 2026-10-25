//! AG.27's disc tests: every modelled character on his own scripts, headless
//! over level 213 with guacamole's host (the walkers' part as the TV links
//! it, `level_scripts::load` over a throwaway extract made by the real
//! stages), each joined by AH.17's join, his setup on the VM writing the
//! `movement parameters` and the scale the mover update reads; AH.165's
//! and AH.169's swaps of a walker; AG.162's whitelist against the TV's
//! listing; and a walker's fall-back once his script stops in play, as the
//! TV takes it (AH.151, AG.173, AG.190, AG.211: Ninjini and the seven giants,
//! whose minions stop at `Minion.ai` #842 only in a run that takes
//! `collisioninfo.object collisions` out of its registry, [`PLAY_842`]: since
//! AG.160 modelled it nothing on the dump stops in play). Ignored by default; they read
//! `GUAC_GIANTS_DUMP` and write the extract under `GUAC_EXTRACT_DIR` (a
//! folder `ag27-verify` there, shared by the tests of one run; a run that
//! made it removes it as its process exits, one that found it current
//! leaves it, so two runs at once over one `GUAC_EXTRACT_DIR` can lose it
//! from under each other: a lane gives its own), from the process env
//! else `.env.dev`, and AG.12's and AE.22's recordings under
//! `GUAC_CAPTURE_DIR`. `AH18_LIVE_TABLE` (aj-vm's live member table, process
//! env, optional) names a halt's member and is what a probe's discovery
//! reads; without it the probe runs on the members it names (AG.182: the
//! plain run of every RPCS3-free disc test passes):
//!
//! `cargo test -p guacamole --release --test giants_walkers -- --ignored --nocapture --test-threads=1`

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::levels;
use guac_extract::{DirState, ExtractRoot, open, stages};
use guac_formats::igz::Igz;
use guac_ir::palette::{LevelPalettes, Palette};
use guac_ir::roster::{Category, Roster};
use guac_motion::hero::Hero;
use guac_motion::parry::ParryWorld;
use guac_motion::proxy::Surfaces;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::{Generator, Halt, Handle, Mode, Obj, PlacementId, Program, Value, Vm};
use guacamole::figure::{self, Layout, Tag, Values};
use guacamole::level_scripts;
use guacamole::on_screen;
use guacamole::player_loop::PlayerLoop;
use guacamole::script_engine::Pad;
use guacamole::scripts::{
    self, Files, ScriptWorld, WalkerScripts, WorldHost, headless_handles, headless_objects,
    headless_spawner,
};
use guacamole::walkers::{self, Movement, Walk};

const LEVEL_NAME: &str = "level_213_tutotial";

/// `halt` with an unmodelled member's id replaced by its name in aj-vm's
/// live table (`AH18_LIVE_TABLE`; `class.member [type]`), the id kept beside
/// it; as it is without the table.
fn named(halt: &str) -> String {
    let Some(at) = halt.find("unmodelled member ") else {
        return halt.to_owned();
    };
    let id = halt[at + 18..].split_whitespace().next().unwrap_or("");
    let Some(table) = std::env::var("AH18_LIVE_TABLE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return halt.to_owned();
    };
    for row in table.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = row.split('\t').collect();
        if c.len() < 4 {
            continue;
        }
        let ns = c[0].to_ascii_lowercase();
        let ns = ns.strip_prefix("tfb").unwrap_or(&ns).to_owned();
        let name = c[2].to_ascii_lowercase();
        let name = name.trim_start_matches('-').to_owned();
        let m = guac_script::MemberId::named(&ns, &name);
        if format!("{:08x}.{:08x}", m.namespace, m.name) == id {
            return format!("{halt} = {ns}.{name} [{}]", c[3]);
        }
    }
    halt.to_owned()
}

fn capture_dir() -> PathBuf {
    PathBuf::from(env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR"))
}

fn read_ron<T: serde::de::DeserializeOwned>(dir: &Path, rel: &str) -> T {
    let text = std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    guac_ir::from_ron(&text).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The one-minute load average, as `uptime` prints it.
fn load1() -> f32 {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.split_whitespace()
        .find_map(|w| w.parse::<f32>().ok())
        .unwrap_or(f32::NAN)
}

/// The extract root this process made ([`extract`]), removed as it exits.
static MADE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Removes [`MADE`]'s root: run by the C runtime's `atexit` as the test
/// process exits, after every test of the run (the harness's main returns,
/// or exits with a failure).
extern "C" fn remove_made() {
    let made = MADE.lock().ok().and_then(|mut m| m.take());
    if let Some(root) = made {
        match std::fs::remove_dir_all(&root) {
            Ok(()) => eprintln!("removed the extract this run made: {}", root.display()),
            Err(e) => eprintln!("the extract this run made stays at {}: {e}", root.display()),
        }
    }
}

unsafe extern "C" {
    /// The C runtime's exit hook (libSystem on macOS).
    fn atexit(f: extern "C" fn()) -> std::ffi::c_int;
}

/// The extract the tests read: made by the real stages (all but the intros)
/// into `GUAC_EXTRACT_DIR/ag27-verify`, kept between the tests of one run.
/// One this run made goes when the process exits (AG.182: a run left its
/// 1.9 GB behind); one it found current stays, for the run after.
fn extract() -> PathBuf {
    // One test makes it while the others of a parallel run wait.
    static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    let dump = dump();
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let at = base.join("ag27-verify");
    let root = ExtractRoot::checked(at.clone()).unwrap();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    let dir = opened.dir.path().to_path_buf();
    if opened.state == DirState::Current {
        return dir;
    }
    {
        let mut made = MADE.lock().unwrap();
        if made.is_none() {
            *made = Some(at);
            // SAFETY: `atexit` takes a plain function pointer, which it calls
            // once at exit; `remove_made` touches only `MADE` and the files.
            let registered = unsafe { atexit(remove_made) };
            assert_eq!(registered, 0, "the exit hook");
        }
    }
    println!(
        "making the extract ({:?}); load {:.2}",
        opened.state,
        load1()
    );
    let started = Instant::now();
    let done = stages::run(&mut opened.dir, None, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "  stages: roster {:.2} s, models {:.2} s, level {:.2} s, scripts {:.2} s ({} entries, {} scripts, {} walkers, {} left out: {:?}, failed {:?}), {:.1} s in all; load {:.2}",
        done.roster.took.as_secs_f64(),
        done.models.took.as_secs_f64(),
        done.levels.took.as_secs_f64(),
        done.scripts.took.as_secs_f64(),
        done.scripts.entries,
        done.scripts.scripts,
        done.scripts.walkers,
        done.scripts.walkers_left_out.len(),
        done.scripts.walkers_left_out,
        done.scripts.failed,
        started.elapsed().as_secs_f64(),
        load1()
    );
    dir
}

/// A member the registry lacks, as a value cell: namespace, name, its
/// registered type and bounds (aj-vm's live table).
type Cell = (&'static str, &'static str, guac_script::Kind, f32, f32);

/// How a run is made: its mode, the members the registry lacks it adds as
/// value cells by name (`cells`), and whether it adds every one aj-vm's
/// live table names (`discover`, `AH18_LIVE_TABLE`, the common run's
/// `discovery`) so the run goes on past each. Strict with neither is the
/// check; with them, what the scripts would do once those are modelled.
/// `without` takes members the registry holds out of it again (namespace,
/// name; `Members::without`, a test-only switch): a script reaching one
/// stops in play as before it was modelled, the TV's fall-back taking him.
#[derive(Debug, Clone, Copy)]
struct Opts {
    mode: Mode,
    discover: bool,
    cells: &'static [Cell],
    without: &'static [(&'static str, &'static str)],
}

const STRICT: Opts = Opts {
    mode: Mode::Strict,
    discover: false,
    cells: &[],
    without: &[],
};
const DISCOVER: Opts = Opts {
    mode: Mode::Strict,
    discover: true,
    cells: &[],
    without: &[],
};
/// Tree Rex strict on the registry: the two members his walk reached past it
/// until AH.49 and AG.160 modelled them, his minion's first tick writing
/// AH.49's blend percent (`112_TitanLife.ai` #501) and the mover's first
/// step with the stick on asking his object collisions (`Minion.ai` #842,
/// where four others stopped), are the host's now: no value cell stands in
/// for either.
const TREE_REX_CELLS: Opts = STRICT;

/// `o`'s value cells into `config`.
fn add_cells(config: &mut guac_script::Config, o: Opts) {
    if o.discover {
        assert!(
            std::env::var_os("AH18_LIVE_TABLE").is_some(),
            "a discovery run reads aj-vm's live table: set AH18_LIVE_TABLE"
        );
        discovery(config);
    }
    for &(ns, name, kind, min, max) in o.cells {
        config
            .members
            .value_cell(ns, name, guac_script::MemberSpec::new(kind, min, max));
    }
    for &(ns, name) in o.without {
        assert!(
            config.members.without(ns, name),
            "{ns}.{name}: not registered, so not taken out"
        );
    }
}

/// The level and the part as the TV's load has them.
struct Level {
    dir: PathBuf,
    program: Arc<Program>,
    files: Files,
    order: Vec<u16>,
    fighters: Vec<(String, u16)>,
    walkers: Vec<WalkerScripts>,
    left_out: Vec<(String, String)>,
    layout: Layout,
    roster: Roster,
    level_bytes: Vec<u8>,
    palettes: LevelPalettes,
    /// The level's own file: the last in link order.
    level_unit: u16,
    linked: (usize, u64, Duration, Duration),
}

impl Level {
    fn open() -> Level {
        let dir = extract();
        let started = Instant::now();
        let linked = level_scripts::load(&dir, LEVEL_NAME).unwrap_or_else(|e| panic!("{e}"));
        let link = (linked.files, linked.bytes, linked.took(), started.elapsed());
        let layout = level_scripts::figure_layout(&dir, IrFormat::Ron).unwrap();
        let roster: Roster = read_ron(&dir, IrFormat::Ron.roster_path());
        let usr = dump().join("PS3_GAME/USRDIR");
        let level_bytes = guac_extract::scripts::entry(
            &usr,
            &format!("{}/{LEVEL_NAME}.bld", levels::LEVEL_FOLDER),
        )
        .expect("the level's archive");
        let igz = Igz::parse(&level_bytes).unwrap();
        let palettes = levels::palettes(&igz, levels::LEVELS[0], identity())
            .expect("the level's palettes")
            .ir;
        Level {
            dir,
            program: Arc::new(linked.program),
            files: linked.placements,
            order: linked.order,
            fighters: linked.fighters,
            walkers: linked.walkers,
            left_out: linked.left_out,
            layout,
            roster,
            level_unit: (linked.files - 1) as u16,
            level_bytes,
            palettes,
            linked: link,
        }
    }

    fn unit_of(&self, load: &str) -> Option<u16> {
        self.fighters
            .iter()
            .find(|(l, _)| l == load)
            .map(|(_, u)| *u)
            .or_else(|| {
                self.walkers
                    .iter()
                    .find(|w| w.load_file == load)
                    .map(|w| w.unit)
            })
    }

    /// The load file's first figure (its base where it has one, else the
    /// first alternate deco): its toy type and variant word.
    fn toy(&self, load: &str) -> Option<(u32, u16)> {
        let mut of: Vec<_> = self
            .roster
            .figures
            .iter()
            .filter(|f| {
                f.load_file == load && matches!(f.category, Category::Figure | Category::Giant)
            })
            .collect();
        of.sort_by_key(|f| f.variant);
        of.first().map(|f| (f.toy_type, f.variant))
    }
}

/// One character joined into the level on his own, headless.
struct W {
    vm: Vm,
    world: ScriptWorld,
    bench: Bench,
    generator: Generator,
    files: Files,
    handles: HashMap<Obj, Handle>,
    next: Handle,
    minion: Option<PlacementId>,
    steps: u32,
    player_loop: PlayerLoop,
    layout: Layout,
    unit: u16,
    walk: Walk,
    scale: f32,
    /// The seconds a step lasts (a replay sets the recording's own).
    dt: f32,
    toy: (u32, u16),
    mode: Mode,
    /// What play mode stopped, each step's in turn (strict halts instead).
    stops: Vec<String>,
    /// His fall-back as the TV takes it (openspec AH.151, AG.190), once his
    /// minion's script stops in play: the step and why; from then on AG.16's
    /// controller walks him on the stick, on what his scripts had set.
    off: Option<(u32, String)>,
    /// What the fall-back does (the TV's both): his walk handed back to the
    /// controller in the host, and the view to the TV's chain.
    hold_walk: bool,
    hand_back_view: bool,
    /// The view the TV draws, when the run steers by it (`tv_view`): the
    /// scripts' viewport, or once handed back the chain's until they write
    /// it again after a join (AG.211), which the stick reads the step after
    /// (`bench.view`).
    tv_view: bool,
    drawn: Option<guac_motion::camera::View>,
    chain: Option<guac_motion::camera::GameCamera>,
}

impl W {
    /// `load`'s run: his unit's placements waiting to join, the level's
    /// collision and props, the hero at the level's start. `scripts` lists
    /// his scripts on the whitelist (a fighter's are listed already).
    fn new(l: &Level, load: &str, o: Opts) -> W {
        let unit = l.unit_of(load).unwrap_or_else(|| panic!("{load}: no unit"));
        let walker = l.walkers.iter().find(|w| w.load_file == load);
        // As the TV lists them: every walker whose new asks of the viewport
        // are all read (AH.150), and this one (any unread let through, so
        // his scripts run).
        let mut config = scripts::with_walkers(scripts::smash_config(o.mode), &l.walkers);
        add_cells(&mut config, o);
        if let Some(w) = walker {
            let mut w = w.clone();
            w.asks = 0;
            config = scripts::with_walkers(config, &[w]);
        }
        let handles = headless_handles(&l.files);
        let objects = headless_objects(&handles);
        let vm = scripts::init_chain(l.program.clone(), &l.files, &handles, &l.order, config);
        let mut world = ScriptWorld::new(&l.files, &handles, objects);
        world.set_inert(vm.inert());
        // As the TV's: the asks read as gating play off the camera's view
        // (openspec AH.150).
        world.answer_from_view(on_screen::answered(&l.program, on_screen::READ));
        // His palette on his template, as the TV's join sets it.
        let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(load));
        // As the TV's join: on the minion's template his definition names, a
        // fighter's the one named for him.
        if let Some(template) = walker.and_then(|w| w.template).or_else(|| {
            l.files
                .placed
                .iter()
                .find(|p| p.obj.unit == unit && p.name == load)
                .map(|p| p.obj)
        }) {
            world.set_palette_of(template, &palette);
        }
        // The level's placements' palettes by their models.
        for p in l.files.placed.iter().filter(|p| p.obj.unit == l.level_unit) {
            if let Some(model) = &p.model
                && let Some(pal) = l.palettes.palettes.iter().find(|x| &x.owner == model)
            {
                world.set_palette_of(p.obj, pal);
            }
        }
        let igz = Igz::parse(&l.level_bytes).unwrap();
        let (mesh, start) = chapter_1(&igz);
        let parry = ParryWorld::new(&[mesh]).expect("the level's collision");
        let walk = if load == "009_Eruptor" {
            Walk::measured(walkers::walker(load).unwrap())
        } else {
            let hero: guac_ir::Hero = read_ron(&l.dir, &IrFormat::Ron.hero_path(load));
            Walk::scripted(load, &hero.solid)
        };
        let solid = walk.solid().expect("his solid");
        let mut hero = Hero::new(solid, walk.tuning, start, 0.0);
        hero.settle(&parry, &[]);
        let props = {
            let level = level_props(&igz);
            let by_object = level
                .props
                .iter()
                .map(|p| {
                    (
                        p.object,
                        handles[&Obj {
                            unit: l.level_unit,
                            index: p.object,
                        }],
                    )
                })
                .collect();
            Some(StandingProps {
                level,
                handles: by_object,
                standing: HashSet::new(),
                holds: HashMap::new(),
            })
        };
        let mut bench = Bench {
            heroes: HashMap::from([(HERO, hero)]),
            world: parry,
            view: 15.0,
            hold_view: true,
            pads: [Pad::default(); 2],
            bodies: HashMap::new(),
            props,
        };
        world.activity_at(&[bench.heroes[&HERO].pivot]);
        bench.stand_props(&world);
        let next = objects[6].max(HERO) + 1;
        let toy = l.toy(load).unwrap_or_else(|| panic!("{load}: no figure"));
        let scale = walk.scale;
        W {
            vm,
            world,
            bench,
            generator: Generator::seeded(5489),
            files: l.files.clone(),
            handles,
            next,
            minion: None,
            steps: 0,
            player_loop: PlayerLoop::read(&l.program),
            layout: l.layout.clone(),
            unit,
            walk,
            scale,
            dt: 1.0 / 60.0,
            toy,
            mode: o.mode,
            stops: Vec::new(),
            off: None,
            hold_walk: true,
            hand_back_view: true,
            tv_view: false,
            drawn: None,
            chain: None,
        }
    }

    /// The run steering by the view the TV draws (openspec AG.28): the
    /// camera's scripts' viewport, the chain's once handed back, the step
    /// before's heading what the stick reads.
    fn on_the_tvs_view(mut self) -> Self {
        self.tv_view = true;
        self.bench.hold_view = false;
        self
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

    /// One step in the TV's order (`guacamole::frame`; the common run's,
    /// without the hands, which only an attack reads), then his scripted
    /// scale ([`hero_scripts::apply_scripted_scale`]'s rule) before the
    /// engine's frame.
    fn step(&mut self) -> Result<(), Halt> {
        let dt = self.dt;
        self.vm.set_tick_seconds(dt);
        let pads = self.bench.pads;
        self.world.read_pads(pads);
        for (h, active) in self.world.activity(&mut self.bench) {
            self.vm.set_asleep(h, !active);
        }
        // Off his scripts he's off the player loop's joined run, as on the
        // TV (`hero_scripts::step_scripts`' `settled`).
        let joined = self.minion.is_some() && self.off.is_none();
        let player_loop = self.player_loop.clone();
        let mut stopped = Vec::new();
        let r = self.with_host(|vm, host| player_loop.frame(vm, host, joined, &mut stopped));
        let play = self.mode == Mode::Play;
        assert!(play || stopped.is_empty(), "strict mode stops by halting");
        self.stops
            .extend(stopped.iter().map(|h| named(&h.to_string())));
        self.world.end_step();
        scripts::add_scripted(&mut self.vm, &mut self.world);
        self.bench.stand_props(&self.world);
        let report = r?;
        assert!(
            play || report.stopped.is_empty(),
            "strict mode stops by halting"
        );
        self.stops
            .extend(report.stopped.iter().map(|h| named(&h.to_string())));
        self.apply_scale();
        // His fall-back where the TV's `step_scripts` takes one: Eruptor at
        // any stop, a walker once his setup ran through and wrote his
        // movement parameters; one whose setup stopped keeps his card on the
        // TV, out of the level, and here steps on as before.
        if self.off.is_none()
            && let Some(m) = self.minion
            && let Some(h) = self.vm.stopped(m)
        {
            let why = named(&h.to_string());
            let stage = match self.vm.set_up(m) {
                true => walkers::Stage::Playing,
                false => walkers::Stage::Joining,
            };
            let fallback = walkers::fallback(self.walk.origin, stage, &why);
            let moves = Movement::read(&self.vm, m).is_some();
            if !matches!(fallback, walkers::Fallback::Card(_))
                && (moves || fallback == walkers::Fallback::Stand)
            {
                self.fall_back(why);
            }
        }
        let before = self.hero().pivot;
        let input = guac_motion::hero::Input::stick(self.bench.pads[0].stick, self.bench.view);
        for hero in self.bench.heroes.values_mut() {
            match self.off {
                None => hero.step_engine(&self.bench.world, dt, &[HERO_BODY], Surfaces::NONE),
                Some(_) => {
                    hero.step_among(&self.bench.world, &input, dt, &[HERO_BODY], Surfaces::NONE)
                }
            }
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;
        self.bench.place_bodies(&self.world);
        if self.tv_view {
            let feet = self.hero().pivot;
            // The scripts' view again once they write it after a join
            // (AG.211), as the TV's camera takes it at `CameraScripts::On`.
            if self.world.viewport().scripted() {
                self.chain = None;
            }
            let drawn = match self.chain.as_mut() {
                Some(chain) => {
                    chain.step(before, dt, &[]);
                    chain.view()
                }
                None => guacamole::viewport::scripts_view(self.world.viewport(), Some(feet)),
            };
            self.bench.view = drawn.heading();
            self.drawn = Some(drawn);
        }
        Ok(())
    }

    /// His fall-back as the TV's `step_scripts` takes it once his minion's
    /// script stops in play: AG.16's controller on the movement parameters
    /// his scripts wrote (Eruptor's on his measured table), his walk the
    /// controller's alone in the host (AH.151, as `hold_walk` says) and the
    /// view handed back where the scripts have it (AG.190, as
    /// `hand_back_view` says), in every run as on the TV, the TV's chain
    /// taking it where it stood in a run steering by the view
    /// ([`W::on_the_tvs_view`]).
    fn fall_back(&mut self, why: String) {
        self.off = Some((self.steps, why));
        let width = self.walk.shape.width;
        if self.walk.origin == walkers::Origin::Scripts
            && let Some(m) = self.minion.and_then(|m| Movement::read(&self.vm, m))
        {
            self.bench
                .heroes
                .get_mut(&HERO)
                .unwrap()
                .set_tuning(m.tuning(width));
        }
        if self.hold_walk {
            self.world.hand_back_walk(HERO);
        }
        // As the TV's `step_scripts`: the view handed back until the next
        // join (AG.211) whenever the scripts have it (`CameraScripts::On`
        // there), so the asks AH.150 answers off it read AH.20's answer
        // meanwhile in every run; the chain built only where the run steers
        // by the view the TV draws.
        if self.hand_back_view && self.world.viewport().scripted() {
            if self.tv_view {
                let feet = self.hero().pivot;
                let drawn = self.drawn.unwrap_or_else(|| {
                    guacamole::viewport::scripts_view(self.world.viewport(), Some(feet))
                });
                self.chain = Some(guacamole::viewport::chain_from(&drawn, feet));
            }
            self.world.hand_back_view_until_a_join();
        }
    }

    /// The name of the placement whose handle is `h`, if one of the part's.
    fn placement_named(&self, h: Handle) -> String {
        self.handles
            .iter()
            .find(|(_, x)| **x == h)
            .and_then(|(o, _)| self.files.placed.iter().find(|p| p.obj == *o))
            .map_or_else(|| format!("{h:#x}"), |p| p.name.clone())
    }

    /// A scripted walker's scale is his scripts' alone: his solid is built
    /// at the one his actor holds (`hero_scripts::apply_scripted_scale`).
    fn apply_scale(&mut self) {
        if self.walk.origin != walkers::Origin::Scripts || self.minion.is_none() {
            return;
        }
        let Some(a) = self.world.actor(HERO) else {
            return;
        };
        let Some(scale) = walkers::uniform_scale(a.scale) else {
            return;
        };
        if (scale - self.scale).abs() < 1e-6 {
            return;
        }
        if let Some(solid) = self.walk.shape.havok([scale * 100.0; 3]) {
            self.bench.heroes.get_mut(&HERO).unwrap().set_solid(solid);
            self.scale = scale;
        }
    }

    /// The join as the TV does it: his figure's tag on the portal, his
    /// file's placements, the step, the minion's activation.
    fn join(&mut self) -> Result<(), Halt> {
        let files = self.files.clone();
        let handles = self.handles.clone();
        let layout = self.layout.clone();
        let tag = Tag {
            toy_id: self.toy.0,
            subtype: u32::from(self.toy.1),
            serial: 1,
            values: Values::default(),
        };
        self.with_host(|vm, host| figure::put_on_portal(vm, host, &layout, &tag))?;
        let unit = self.unit;
        self.with_host(|vm, host| scripts::join_request(vm, host, &files, unit, &handles))?;
        self.step()?;
        let minion = self.with_host(|vm, host| scripts::join_activate(vm, host, HERO))?;
        self.minion = Some(minion);
        Ok(())
    }

    /// The next hero in where this one stood, as the TV's swap brings him
    /// (openspec AH.105): `load`'s palette on his minion's template with its
    /// clips' animation info (AH.49's, as the TV's join sets it), his
    /// controller on his own numbers at this one's place and facing, then
    /// the join. Takes this one off first ([`W::deactivate`]) if he's on.
    fn swap_in(&mut self, l: &Level, load: &str) -> Result<(), Halt> {
        if self.minion.is_some() {
            self.deactivate();
        }
        let unit = l.unit_of(load).unwrap_or_else(|| panic!("{load}: no unit"));
        let walker = l.walkers.iter().find(|w| w.load_file == load);
        let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(load));
        if let Some(template) = walker.and_then(|w| w.template).or_else(|| {
            l.files
                .placed
                .iter()
                .find(|p| p.obj.unit == unit && p.name == load)
                .map(|p| p.obj)
        }) {
            self.world.set_palette_of(template, &palette);
        }
        let walk = if load == "009_Eruptor" {
            Walk::measured(walkers::walker(load).unwrap())
        } else {
            let hero: guac_ir::Hero = read_ron(&l.dir, &IrFormat::Ron.hero_path(load));
            Walk::scripted(load, &hero.solid)
        };
        let (pivot, facing) = (self.hero().pivot, self.hero().facing);
        let mut hero = Hero::new(walk.solid().expect("his solid"), walk.tuning, pivot, facing);
        hero.settle(&self.bench.world, &[HERO_BODY]);
        self.bench.heroes.insert(HERO, hero);
        self.unit = unit;
        self.toy = l.toy(load).unwrap_or_else(|| panic!("{load}: no figure"));
        self.scale = walk.scale;
        self.walk = walk;
        self.join()
    }

    /// His figure off the portal mid-level as the TV's swap takes it
    /// (openspec AH.105, AH.165): the deactivate path, whatever stops in it.
    fn deactivate(&mut self) -> scripts::Deactivated {
        let files = self.files.clone();
        let unit = self.unit;
        let gone =
            self.with_host(|vm, host| scripts::join_deactivate(vm, host, &files, unit, HERO));
        self.minion = None;
        // The TV's take-off ends a fall-back with the hero it despawns: the
        // next join is a hero on his scripts again. The TV's camera stays
        // (its chain, when it has the view).
        self.off = None;
        gone
    }

    /// The portal manager's placement.
    fn portal_manager(&self) -> PlacementId {
        scripts::portal_manager_placement(&self.vm, &self.world).expect("the portal manager runs")
    }

    /// His file's placements running now, by handle, each with its
    /// placement.
    fn file_running(&self) -> Vec<(Handle, PlacementId)> {
        let mut out: Vec<(Handle, PlacementId)> = self
            .files
            .placed
            .iter()
            .filter(|p| p.obj.unit == self.unit)
            .filter_map(|p| self.handles.get(&p.obj).copied())
            .filter_map(|h| Some((h, self.vm.placement_of(h)?)))
            .filter(|(_, id)| !self.vm.removed(*id))
            .collect();
        out.sort_unstable();
        out
    }

    /// The init chain's two steps, the join and `settle` steps; the
    /// character as far as he got, when and the halt otherwise.
    fn joined(l: &Level, load: &str, o: Opts, settle: u32) -> Result<W, Box<(W, String, Halt)>> {
        let mut w = W::new(l, load, o);
        let fail = |w: W, when: String, h: Halt| Err(Box::new((w, when, h)));
        if let Err(h) = w.step() {
            return fail(w, "init step 1".to_owned(), h);
        }
        if let Err(h) = w.step() {
            return fail(w, "init step 2".to_owned(), h);
        }
        if let Err(h) = w.join() {
            return fail(w, "join".to_owned(), h);
        }
        for k in 0..settle {
            if let Err(h) = w.step() {
                return fail(w, format!("settle step {k}"), h);
            }
        }
        Ok(w)
    }

    fn hero(&self) -> &Hero {
        &self.bench.heroes[&HERO]
    }

    /// His `movement parameters` as his scripts hold them in his
    /// description (what the mover update reads), each member's value by
    /// name: ground and air speed, acceleration, deceleration and turn
    /// rate, gravity, move type.
    fn movement(&self) -> Option<BTreeMap<String, f32>> {
        let m = self.minion?;
        let d = self.vm.local(m, "description").map(|v| self.vm.value(v))?;
        let mp = self
            .vm
            .field(d, "movement parameters")
            .map(|v| self.vm.value(v))?;
        Some(
            self.vm
                .fields(mp)
                .into_iter()
                .filter_map(|(n, f)| {
                    let c = self.vm.value(f);
                    let v = self.vm.field(c, "value").map(|v| self.vm.value(v))?;
                    Some((n.to_owned(), v.as_f32()?))
                })
                .collect(),
        )
    }

    /// [`W::movement`] as AG.12 lists it: ground (speed, acceleration,
    /// deceleration, turn rate), air (the same), gravity (acceleration,
    /// max speed) and move type.
    fn parameters(&self) -> Option<[f32; 11]> {
        let m = self.movement()?;
        let g = |n: &str| m.get(n).copied();
        Some([
            g("ground max speed")?,
            g("ground acceleration")?,
            g("ground deceleration")?,
            g("ground turn rate")?,
            g("air max speed")?,
            g("air acceleration")?,
            g("air deceleration")?,
            g("air turn rate")?,
            g("gravity acceleration")?,
            g("gravity max speed")?,
            g("move type")?,
        ])
    }

    /// His actor's scale, percent, and its `ActorParameters` as the host
    /// holds them (the engine's, which stay the unconstrained 10,000).
    fn actor(&self) -> Option<([f32; 3], f32)> {
        let a = self.world.actor(HERO)?;
        Some((a.scale, a.physics.ground_max_speed))
    }

    /// The walk his scripts hand the engine, ft/s (the scripts' own unit,
    /// a recording's `mover.length`).
    fn horizontal_walk(&self) -> f32 {
        let v = self.hero().walk.velocity;
        v.x.hypot(v.z)
    }
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_probe_of_the_part() {
    let l = Level::open();
    println!(
        "part: {} files, {} bytes; read+parse+link {:.2?} (load wall {:.2?}); {} walkers listed, {} left out: {:?}; order {:?}; level unit {}",
        l.linked.0,
        l.linked.1,
        l.linked.2,
        l.linked.3,
        l.walkers.len(),
        l.left_out.len(),
        l.left_out,
        l.order,
        l.level_unit
    );
    for w in &l.walkers {
        println!(
            "  {} unit {} asks {} scripts {:?}",
            w.load_file, w.unit, w.asks, w.scripts
        );
    }
}

/// AG.162: the extract's whitelist holds each walker the TV lists (no new
/// viewport ask, a minion his definition names) once, in link order, with
/// exactly the scripts the TV's listing runs for him, and no other walker.
/// The unit tests only count the names and `giants_fight_stage` only finds
/// each in his archive, so a name swapped for another script of his file
/// fails here alone.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn the_whitelist_holds_each_listed_walkers_scripts() {
    use guac_extract::scripts::{WHITELIST, character_archive};
    let l = Level::open();
    let listed: Vec<&WalkerScripts> = l
        .walkers
        .iter()
        .filter(|w| w.asks == 0 && w.minion.is_some())
        .collect();
    let archives: Vec<String> = l
        .walkers
        .iter()
        .map(|w| character_archive(&w.load_file))
        .collect();
    let entries: Vec<_> = WHITELIST
        .iter()
        .filter(|e| archives.iter().any(|a| a == e.archive))
        .collect();
    assert_eq!(
        entries.iter().map(|e| e.archive).collect::<Vec<_>>(),
        listed
            .iter()
            .map(|w| character_archive(&w.load_file))
            .collect::<Vec<_>>(),
        "the whitelist's walkers are the TV's listed ones, in link order"
    );
    let key = |s: &String| s.to_ascii_lowercase();
    for (w, e) in listed.iter().zip(&entries) {
        let mut ours: Vec<String> = e.scripts.iter().map(|s| format!("/{s}")).collect();
        ours.sort_by_key(key);
        let mut tv = w.scripts.clone();
        tv.sort_by_key(key);
        assert_eq!(ours, tv, "{}", w.load_file);
    }
    println!(
        "{} walkers listed, {} on the whitelist, {} scripts",
        listed.len(),
        entries.len(),
        entries.iter().map(|e| e.scripts.len()).sum::<usize>()
    );
}

/// Each walker's viewport asks the TV's own run doesn't reach: which script
/// holds them and in which definition.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_probe_of_the_asks() {
    let l = Level::open();
    let p = &*l.program;
    let base = scripts::smash_config(Mode::Play).whitelist;
    let mut known = std::collections::BTreeSet::new();
    for (id, s) in p.scripts.iter().enumerate() {
        if !l.walkers.iter().any(|w| w.unit == s.unit) && base.lists("", &s.name) {
            known.extend(guacamole::on_screen::reached(p, id as u32).0);
        }
    }
    for w in l.walkers.iter().filter(|w| w.asks > 0) {
        println!("{} (asks {}):", w.load_file, w.asks);
        for (id, s) in p
            .scripts
            .iter()
            .enumerate()
            .filter(|(_, s)| s.unit == w.unit)
        {
            let file = s.name.rsplit(['/', '\\']).next().unwrap();
            if !w
                .scripts
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&format!("/{file}")))
            {
                continue;
            }
            for a in guacamole::on_screen::reached(p, id as u32).0 {
                if known.contains(&a) {
                    continue;
                }
                println!(
                    "  from {file}: ask in {} #{} def {:?}",
                    p.scripts[a.script as usize].name,
                    a.position,
                    a.def.map(|d| p.defs[d as usize].display.clone())
                );
            }
        }
    }
}

const TREE_REX: &str = "112_TitanLife";
const FOOT: f32 = 0.3048;

fn angle(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

fn json_f32(v: &serde_json::Value) -> f32 {
    v.as_f64().unwrap() as f32
}

fn json_vec3(v: &serde_json::Value) -> Vec3 {
    Vec3::new(json_f32(&v[0]), json_f32(&v[1]), json_f32(&v[2]))
}

/// A record's pad as our step reads it: its left stick's bytes.
fn record_pad(r: &serde_json::Value) -> Pad {
    let byte = |b: &serde_json::Value| b.as_u64().unwrap() as u8;
    let left = &r["pad"]["left"];
    Pad {
        stick: Stick::from_bytes(byte(&left["x"]), byte(&left["y"])),
        actions: [false; 4],
        connected: true,
    }
}

/// AG.12's recordings: each walk's label and its records.
fn ag12_walks() -> Vec<(String, Vec<serde_json::Value>)> {
    let dir = capture_dir().join("BLUS30968/ag12-walks-1792580915");
    let mut labels: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("t_"))
        .collect();
    labels.sort();
    labels
        .into_iter()
        .map(|label| {
            let text = std::fs::read_to_string(dir.join(&label).join("recording.json")).unwrap();
            let records: serde_json::Value = serde_json::from_str(&text).unwrap();
            (label, records.as_array().unwrap().clone())
        })
        .collect()
}

/// How a replay of one recording went: ours against theirs per step.
#[derive(Debug, Default)]
struct Replayed {
    steps: usize,
    /// The record's frame times, s.
    dt: (f32, f32),
    /// Largest speed difference over every step, over the steps theirs is
    /// at a plateau (within 1e-3 ft/s of the step before and after), ft/s.
    speed: f32,
    plateau: f32,
    plateau_steps: usize,
    /// Largest walk heading difference while both move, degrees.
    heading: f32,
    /// Largest and last facing difference, degrees.
    facing: f32,
    facing_last: f32,
    /// Largest and last horizontal distance between ours and theirs, both
    /// from where each started, m.
    drift: f32,
    drift_last: f32,
    /// Where we stood against the record's first position, m.
    start: f32,
    /// The steps at which each first reached 90 % of its top speed.
    ninety: (Option<usize>, Option<usize>),
}

/// A recording replayed on a fresh `load` run as `o` says: step k reads
/// record k's pad against the view record k's camera holds and is set
/// beside record k + 1 (O5 samples a frame's state before its pad read), the
/// step's seconds the record's own frame time (`own_dt`) or 1/60.
fn replay(l: &Level, load: &str, o: Opts, records: &[serde_json::Value], own_dt: bool) -> Replayed {
    let mut w = W::joined(l, load, o, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    let start = w.hero().pivot;
    let theirs0 = json_vec3(&records[0]["hero"]["position"]);
    let mut out = Replayed {
        steps: records.len() - 1,
        dt: (f32::MAX, 0.0),
        start: Vec3::new(start.x - theirs0.x, 0.0, start.z - theirs0.z).length(),
        ..Replayed::default()
    };
    let mut ours_v = Vec::new();
    let mut theirs_v = Vec::new();
    for k in 0..records.len() - 1 {
        let r = &records[k];
        let pr = json_f32(&r["clock"]["presentation_rate"]);
        let d = json_f32(&r["clock"]["frame_delta"]).clamp(0.5 * pr, 3.0 * pr);
        if k > 0 {
            out.dt = (out.dt.0.min(d), out.dt.1.max(d));
        }
        w.dt = if own_dt { d } else { 1.0 / 60.0 };
        w.bench.view = json_f32(&r["camera"]["rotation"][1]);
        w.bench.pads[0] = record_pad(r);
        w.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let next = &records[k + 1];
        let m = &next["mover"];
        let speed_t = json_f32(&m["length"]);
        let speed_o = w.horizontal_walk();
        ours_v.push(speed_o);
        theirs_v.push(speed_t);
        out.speed = out.speed.max((speed_o - speed_t).abs());
        if speed_o > 1e-3 && speed_t > 1e-3 {
            let wv = w.hero().walk.velocity;
            let h_o = guacamole::script_engine::heading_of(wv);
            out.heading = out.heading.max(angle(h_o, json_f32(&m["heading"])).abs());
        }
        if own_dt && std::env::var("AG27_TRACE").is_ok() {
            println!(
                "  k {k}: stick {:?}, facing ours {:.3} theirs {:.3}, speed ours {:.4} theirs {:.4}",
                r["stick"],
                w.hero().facing,
                json_f32(&next["hero"]["rotation"][1]),
                speed_o,
                speed_t
            );
        }
        let f = angle(w.hero().facing, json_f32(&next["hero"]["rotation"][1])).abs();
        out.facing = out.facing.max(f);
        out.facing_last = f;
        let p = w.hero().pivot - start;
        let t = json_vec3(&next["hero"]["position"]) - theirs0;
        let drift = Vec3::new(p.x - t.x, 0.0, p.z - t.z).length();
        out.drift = out.drift.max(drift);
        out.drift_last = drift;
    }
    for k in 1..theirs_v.len() - 1 {
        if (theirs_v[k] - theirs_v[k - 1]).abs() < 1e-3
            && (theirs_v[k + 1] - theirs_v[k]).abs() < 1e-3
        {
            out.plateau = out.plateau.max((ours_v[k] - theirs_v[k]).abs());
            out.plateau_steps += 1;
        }
    }
    let top = |v: &[f32]| {
        let t = v.iter().copied().fold(0.0f32, f32::max);
        v.iter().position(|s| *s >= 0.9 * t && t > 0.0)
    };
    out.ninety = (top(&ours_v), top(&theirs_v));
    out
}

/// AG.27 Verify (1): Tree Rex's scripts set AG.12's numbers, and his walk
/// replays against AG.12's O5 recording, strict on the registry
/// ([`TREE_REX_CELLS`]: the two members his scripts reached past it were
/// value cells here until AH.49 and AG.160 modelled them).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.12's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn tree_rex_walks_on_ag12s_numbers() {
    println!("load before: {:.2}", load1());
    let started = Instant::now();
    let l = Level::open();
    // Strict over the registry as it stands: where his scripts stop.
    match W::joined(&l, TREE_REX, STRICT, 120) {
        Ok(_) => println!("strict, the registry as it stands: his scripts run through"),
        Err(b) => {
            let (_, when, h) = *b;
            println!(
                "strict, the registry as it stands: {when}: {}",
                named(&h.to_string())
            );
        }
    }
    let w = W::joined(&l, TREE_REX, TREE_REX_CELLS, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    let minion = w.minion.unwrap();
    let params = w.parameters().expect("his movement parameters");
    let (scale, engine_speed) = w.actor().expect("his actor");
    let a = w.world.actor(HERO).unwrap();
    println!(
        "Tree Rex ({TREE_REX}), unit {}, set up {}, description's movement parameters (ground speed, accel, decel, turn; air the same; gravity accel, max; move type) {:?}, actor scale {:?}, actor's engine ground speed {engine_speed}, solid {:?}; drawn solid {:?}",
        w.unit,
        w.vm.set_up(minion),
        params,
        scale,
        a.solid,
        w.hero().solid()
    );
    assert!(w.vm.set_up(minion), "his setup ran through");
    assert_eq!(
        params,
        [
            20.0, 160.0, 80.0, 360.0, 20.0, 160.0, 50.0, 1440.0, 128.0, 120.0, 0.0
        ],
        "AG.12's numbers"
    );
    assert_eq!(scale, [100.0; 3]);
    assert_eq!(
        (a.solid.width, a.solid.height, a.solid.length, a.solid.shape),
        (14.0, 21.25, 3.0, 2),
        "the 14 x 21.25 ft capsule"
    );
    assert_eq!(w.scale, 1.0);
    assert_eq!(
        w.hero().solid(),
        &w.walk.shape.havok([100.0; 3]).unwrap(),
        "the capsule at 100 %"
    );
    let walks = ag12_walks();
    assert_eq!(walks.len(), 14, "AG.12's 14 rings");
    let mut worst = Replayed::default();
    // The worst facing over the rings that swing the stick 180°.
    let mut swings = 0.0f32;
    for (label, records) in &walks {
        if label.starts_with("t_south") {
            // South at rest and while walking: a walker's attack buttons
            // reach no script (AH.29), so the lunge the oracle's Tree Rex
            // makes is not compared.
            continue;
        }
        let turns = label.starts_with("t_turns");
        for own in [false, true] {
            let r = replay(&l, TREE_REX, TREE_REX_CELLS, records, own);
            println!(
                "{label} {}: {} steps, record dt {:.4}-{:.4} s; start {:.4} m off; speed within {:.5} ft/s (plateau {:.5} over {} steps); walk heading within {:.4}°; facing within {:.4}° (last {:.4}°); position drift {:.5} m (last {:.5}); 90 % of top at step {:?} (theirs {:?})",
                if own { "own dt" } else { "1/60 s" },
                r.steps,
                r.dt.0,
                r.dt.1,
                r.start,
                r.speed,
                r.plateau,
                r.plateau_steps,
                r.heading,
                r.facing,
                r.facing_last,
                r.drift,
                r.drift_last,
                r.ninety.0,
                r.ninety.1
            );
            assert_eq!(r.ninety.0, r.ninety.1, "{label}: 90 % of top speed");
            assert!(r.plateau < 1e-3, "{label}: plateau {}", r.plateau);
            if own {
                // The record's own frame times: his walk is theirs to the
                // float, step by step.
                assert!(r.speed < 1e-3, "{label}: speed {}", r.speed);
                assert!(r.drift < 1e-3, "{label}: drift {}", r.drift);
                assert!(r.heading < 0.1, "{label}: heading {}", r.heading);
                // His facing is the oracle's to the bit at every step of
                // every ring, the 180° swings of `t_full` and `t_half` among
                // them (AG.172: a target straight behind turns him the
                // positive way, as the oracle's Tree Rex turned; on the
                // [-180, 180) normalized angle these were 119-180° off),
                // but `t_half`-W-01 and -02, 3 ulps (9.2e-5°) short from
                // step 357 on (AG.185).
                if matches!(label.as_str(), "t_half-W-01" | "t_half-W-02") {
                    assert!(r.facing < 1e-4, "{label}: facing {}", r.facing);
                } else {
                    assert_eq!(r.facing, 0.0, "{label}: facing");
                }
                worst.speed = worst.speed.max(r.speed);
                worst.plateau = worst.plateau.max(r.plateau);
                worst.heading = worst.heading.max(r.heading);
                if turns {
                    worst.facing = worst.facing.max(r.facing);
                } else {
                    swings = swings.max(r.facing);
                }
                worst.drift = worst.drift.max(r.drift);
            } else {
                assert!(r.drift < 0.05, "{label}: drift {}", r.drift);
            }
        }
    }
    println!(
        "worst of the own-dt replays: speed {:.6} ft/s, plateau {:.6}, walk heading {:.4}°, turning recordings' facing {:e}°, the 180° swings' facing {:e}°, position drift {:.6} m",
        worst.speed, worst.plateau, worst.heading, worst.facing, swings, worst.drift
    );
    // The check discriminates: Eruptor's scripts (21.5 ft/s, 1440°/s) on
    // the same pads miss Tree Rex's plateau by his 1.5 ft/s at full stick.
    let (_, records) = &walks[0];
    let e = replay(&l, "009_Eruptor", STRICT, records, true);
    println!(
        "Eruptor on {}'s pads: speed within {:.4} ft/s, drift {:.4} m",
        walks[0].0, e.speed, e.drift
    );
    assert!(e.speed > 1.0 && e.drift > 0.1, "{e:?}");
    println!("{:.2?}; load after: {:.2}", started.elapsed(), load1());
}

/// Prints `v`'s fields down to `depth`, floats and ints as they are.
fn dump_fields(vm: &Vm, v: Value, depth: usize, indent: usize, only: &dyn Fn(&str) -> bool) {
    for (name, f) in vm.fields(v) {
        let value = vm.value(f);
        let shown = match value {
            Value::Float(x) => format!("{x}"),
            Value::Int(i) => format!("{i}"),
            other => format!("{other:?}"),
        };
        if depth > 0 && matches!(value, Value::Struct(_)) && only(name) {
            println!("{:indent$}{name}:", "", indent = indent);
            dump_fields(vm, value, depth - 1, indent + 2, &|_| true);
        } else if indent > 0 {
            println!("{:indent$}{name} = {shown}", "", indent = indent);
        }
    }
}

/// His description's movement and locomotion fields once his setup ran
/// (`AG27_LOAD`, Tree Rex by default): with every member aj-vm's live table
/// names a value cell when `AH18_LIVE_TABLE` is set, strict with
/// `AG27_STRICT`, otherwise on Tree Rex's two named cells
/// ([`TREE_REX_CELLS`]; strict for another load), so the plain run passes
/// (AG.182).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_probe_of_the_parameters() {
    let l = Level::open();
    let load = std::env::var("AG27_LOAD").unwrap_or_else(|_| TREE_REX.to_owned());
    let opts = if std::env::var("AG27_STRICT").is_ok() {
        STRICT
    } else if std::env::var_os("AH18_LIVE_TABLE").is_some() {
        DISCOVER
    } else {
        println!(
            "AH18_LIVE_TABLE unset: no discovery; {load} on {}",
            if load == TREE_REX {
                "the registry, strict (TREE_REX_CELLS: no value cell since AH.49 and AG.160)"
            } else {
                "the registry as it stands (strict)"
            }
        );
        if load == TREE_REX {
            TREE_REX_CELLS
        } else {
            STRICT
        }
    };
    let w = W::joined(&l, &load, opts, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    let m = w.minion.unwrap();
    let d = w.vm.local(m, "description").map(|v| w.vm.value(v)).unwrap();
    println!("description fields:");
    dump_fields(&w.vm, d, 2, 0, &|n| {
        n.contains("movement") || n.contains("locomotion")
    });
    let _ = &w;
}

/// The literal arguments of every call site in `unit`'s scripts the VM has
/// run (`Vm::call_sites`): each `param <name> := <number>` its interface
/// binds, by the site's position, for the parameters named in `names`.
fn call_arguments(vm: &Vm, unit: u16, names: &[&str]) -> Vec<(String, u32, String, f32, u64)> {
    use guac_script::{Ins, LiteralClass, Operand};
    let program = vm.program();
    let mut out = Vec::new();
    let mut sites: Vec<_> = vm.call_sites().iter().collect();
    sites.sort();
    for (&(script, at), &n) in sites {
        let info = &program.scripts[script as usize];
        if info.unit != unit {
            continue;
        }
        let mut k = at as usize + 1;
        if !matches!(
            info.ops.get(k).map(|o| &program.ops[*o as usize]),
            Some(Ins::Interface)
        ) {
            continue;
        }
        k += 1;
        while let Some(Ins::Parameter(p)) = info.ops.get(k).map(|o| &program.ops[*o as usize]) {
            let name = program.sym(p.name);
            if names.contains(&name)
                && let Some(mut rhs) = p.rhs
            {
                // A bare value is wrapped in an expression of its own.
                while let Operand::Expr {
                    a: Some(inner),
                    op: None,
                    b: None,
                } = &program.operands[rhs as usize]
                {
                    rhs = *inner;
                }
                if let Operand::Path {
                    literal: Some(lit), ..
                } = &program.operands[rhs as usize]
                {
                    let v = match lit.class {
                        LiteralClass::Float => f32::from_bits(lit.bits),
                        LiteralClass::Int => lit.bits as i32 as f32,
                        _ => f32::NAN,
                    };
                    out.push((
                        info.name.rsplit('/').next().unwrap().to_owned(),
                        at,
                        name.to_owned(),
                        v,
                        n,
                    ));
                }
            }
            k += 1;
        }
    }
    out
}

/// Every field under `v` (down to `depth`, each structure once) holding a
/// number within 1e-3 of one of `wanted`, by path.
fn find_numbers(
    vm: &Vm,
    v: Value,
    path: &str,
    depth: usize,
    wanted: &[f32],
    seen: &mut HashSet<u32>,
    out: &mut Vec<String>,
) {
    if let Value::Struct(s) = vm.value(v) {
        if !seen.insert(s) || depth == 0 {
            return;
        }
        for (name, f) in vm.fields(vm.value(v)) {
            let value = vm.value(f);
            let p = format!("{path}.{name}");
            if let Some(x) = value.as_f32()
                && wanted.iter().any(|w| (x - w).abs() < 1e-3)
            {
                out.push(format!("{p} = {x}"));
            }
            find_numbers(vm, value, &p, depth - 1, wanted, seen, out);
        }
    }
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_probe_of_the_locomotion_cells() {
    let l = Level::open();
    let load = std::env::var("AG27_LOAD").unwrap_or_else(|_| "009_Eruptor".to_owned());
    let w = W::joined(&l, &load, STRICT, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    let m = w.minion.unwrap();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let d = w.vm.local(m, "description").unwrap();
    find_numbers(
        &w.vm,
        d,
        "description",
        8,
        &[3.658, 6.401, 0.25, 12.0019, 21.0017, 13.124],
        &mut seen,
        &mut out,
    );
    for name in [
        "minion locomotion states",
        "returned locomotion states",
        "locomotion states",
        "locomotion data",
        "minion locomotion data",
        "character locomotion states",
    ] {
        if let Some(v) = w.vm.local(m, name) {
            println!("local {name}: {:?}", w.vm.value(v));
            find_numbers(
                &w.vm,
                v,
                name,
                8,
                &[3.658, 6.401, 0.25, 12.0019, 21.0017, 13.124],
                &mut seen,
                &mut out,
            );
        }
    }
    for g in 0..w.vm.program().globals.len() {
        if let Some(v) = w.vm.global_peek(g as u32) {
            let obj = w.vm.program().globals[g].0;
            find_numbers(
                &w.vm,
                v,
                &format!("global {obj:?}"),
                8,
                &[3.658, 6.401, 0.25],
                &mut seen,
                &mut out,
            );
            if let Some(x) = w.vm.value(v).as_f32()
                && [3.658, 6.401, 0.25].iter().any(|t| (x - t).abs() < 1e-3)
            {
                out.push(format!("global {obj:?} = {x}"));
            }
        }
    }
    for o in &out {
        println!("  {o}");
    }
    for (script, at, name, v, n) in call_arguments(
        &w.vm,
        w.unit,
        &[
            "walk speed",
            "run speed",
            "jog speed",
            "locomotion switch time",
        ],
    ) {
        println!("  call site {script} #{at} ran {n}x: {name} := {v}");
    }
    let program = w.vm.program();
    for (def, n) in w.vm.calls() {
        let name = &program.defs[*def as usize].display;
        if name.contains("Locomotion") {
            println!("  called {name}: {n}");
            for var in [
                "walk speed",
                "run speed",
                "jog speed",
                "locomotion switch time",
                "movement speed",
                "states id",
            ] {
                if let Some(v) = w.vm.frame(*def, var) {
                    println!("    frame {var} = {:?}", w.vm.value(v));
                }
            }
        }
    }
}

/// AG.219's reading on the dump: each walker's locomotion states as his own
/// scripts define them (`walkers::locomotion_states`, off `global.bld`'s
/// locomotion script's `locomotions` for him once his setup has run), and
/// AG.30's gaits for him off them by his hero side's clips' names
/// (`walkers::ScriptedGaits`): Eruptor's his Define call's 3.658 and 6.401
/// m/s as AG.11 measured them (`Gaits::ERUPTOR`), and every walker's walk,
/// run or both his hero side's and his scripts'.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn every_walkers_stand_in_gaits_are_his_own_locomotion_states() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let mut loads: Vec<String> = vec!["009_Eruptor".to_owned()];
    loads.extend(l.walkers.iter().map(|w| w.load_file.clone()));
    let mut kinds: BTreeMap<(bool, bool), Vec<String>> = BTreeMap::new();
    for load in &loads {
        let w = W::joined(&l, load, PLAY, 120)
            .map_err(|b| format!("{load}: {}", b.1))
            .unwrap();
        let states = walkers::locomotion_states(&w.vm, &w.world, HERO)
            .unwrap_or_else(|| panic!("{load}: his locomotion states"));
        let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(load));
        let hero: guac_ir::Hero = read_ron(&l.dir, &IrFormat::Ron.hero_path(load));
        let g = walkers::ScriptedGaits::of(
            &states,
            |slot| palette.clip(slot as u16).map(|c| c.name.as_str()),
            at(&hero.locomotion.walk).map(|(n, s)| (n, s.map(u32::from))),
            at(&hero.locomotion.run).map(|(n, s)| (n, s.map(u32::from))),
        )
        .unwrap_or_else(|| panic!("{load}: no walk or run of his scripts' {states:?}"));
        println!(
            "{load}: states {:?}; walk {} run {} at {:?}",
            states
                .iter()
                .map(|s| (
                    s.slot,
                    s.speed,
                    s.slot
                        .and_then(|x| palette.clip(x as u16))
                        .map(|c| c.name.as_str())
                ))
                .collect::<Vec<_>>(),
            g.walk,
            g.run,
            g.gaits
        );
        if load == "009_Eruptor" {
            assert_eq!(g.gaits, guac_motion::locomotion::Gaits::ERUPTOR);
            assert!(g.walk && g.run);
        }
        kinds.entry((g.walk, g.run)).or_default().push(load.clone());
    }
    for ((walk, run), who) in &kinds {
        println!("walk {walk} run {run}: {} {who:?}", who.len());
    }
    assert_eq!(kinds.values().map(Vec::len).sum::<usize>(), loads.len());
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}

/// AG.30's locomotion for `load` as the TV builds it at his fall-back: a
/// walker's off his own scripts' locomotion states (AG.219,
/// `walkers::stand_in`), Eruptor's on his measured gaits, both from his
/// join's rest; and the four his picks are read by.
fn stand_in_of(
    w: &W,
    l: &Level,
    load: &str,
) -> (guac_motion::locomotion::Locomotion, guacamole::picks::Four) {
    use guac_motion::locomotion::{Gaits, Locomotion, Loops};
    let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(load));
    let hero: guac_ir::Hero = read_ron(&l.dir, &IrFormat::Ron.hero_path(load));
    let states = walkers::locomotion_states(&w.vm, &w.world, HERO).expect("his states");
    let four = four_of(&palette, &hero, &states);
    let side = &hero.locomotion;
    let duration = |c: &Option<guac_ir::hero::SlotClip>| c.as_ref().map(|c| c.clip.duration);
    let joined = Loops {
        idle: four.0[0]
            .and_then(|s| palette.clip(s as u16))
            .map_or(1.0, |c| c.duration),
        walk: None,
        run: None,
        fall: duration(&side.falling),
    };
    let own = match w.walk.origin {
        walkers::Origin::Scripts => walkers::stand_in(
            &states,
            &palette,
            side.walk.as_ref().map(|c| (&c.clip, Some(c.slot))),
            side.run.as_ref().map(|c| (&c.clip, Some(c.slot))),
            joined,
        )
        .expect("his own stand-in"),
        walkers::Origin::Measured => Locomotion::new(
            Gaits::ERUPTOR,
            Loops {
                walk: duration(&side.walk),
                run: duration(&side.run),
                ..joined
            },
        ),
    };
    (own, four)
}

/// AG.219's AG.30 locomotion draws what his own scripts draw (AH.198's
/// review; stand-in ledger: `locomotion`): every walker and Eruptor on his
/// scripts, on the registry as it stands, a part stick for 120 steps then a
/// full one for 180, his scripts' picks read each step beside AG.30's
/// locomotion built as his fall-back builds it ([`stand_in_of`]) and
/// stepped on the same frames from his join's rest; on every step at a
/// speed held 30 steps, moving and not held at an edge, its four weights
/// and the rate of each gait the picks weight are the picks' within 1e-4.
/// Left out: Wrecking Ball (`023_ForceFieldGrub`), whose scripts start a
/// weighted idle each step (AG.222).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn every_walkers_stand_in_draws_what_his_scripts_draw() {
    use guac_motion::locomotion::{Clip, Frame};
    use guacamole::picks::Picked;
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let mut loads: Vec<String> = vec!["009_Eruptor".to_owned()];
    loads.extend(l.walkers.iter().map(|w| w.load_file.clone()));
    let (mut held, mut worst_w, mut worst_r) = (0usize, 0.0f32, 0.0f32);
    for load in loads.iter().filter(|x| x.as_str() != "023_ForceFieldGrub") {
        let mut w = joined_on_the_tv(&l, load, PLAY, 120, true, true);
        let (mut own, four) = stand_in_of(&w, &l, load);
        let mut speeds: Vec<f32> = Vec::new();
        let (mut n, mut most_w, mut most_r) = (0usize, 0.0f32, 0.0f32);
        for k in 0..300u32 {
            let push = if k < 120 { 0.45 } else { 1.0 };
            w.bench.pads[0] = Pad {
                stick: guac_motion::scenarios::stick(push, 0.0),
                actions: [false; 4],
                connected: true,
            };
            w.step().unwrap();
            let h = w.hero();
            let speed = h.gait_speed;
            own.step(w.dt, Frame::of(h));
            speeds.push(speed);
            let steady = speeds.len() > 30
                && speeds[speeds.len() - 31..]
                    .iter()
                    .all(|s| (s - speed).abs() < 1e-3);
            if !steady || speed < 0.5 || h.probe_held {
                continue;
            }
            let p = Picked::read(&w.world, HERO, four, speed).expect("his picks");
            let (pw, sw) = (p.weights(), own.weights());
            let dw = pw
                .iter()
                .zip(sw)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            let rates = p.rates();
            let dr = [(1, Clip::Walk), (2, Clip::Run)]
                .iter()
                .filter(|(i, _)| pw[*i] > 1e-3)
                .map(|&(i, c)| (rates[i - 1] - own.play(c).rate).abs())
                .fold(0.0f32, f32::max);
            assert!(
                dw < 1e-4,
                "{load} step {k} at {speed}: picks {pw:?}, stand-in {sw:?}"
            );
            assert!(
                dr < 1e-4,
                "{load} step {k} at {speed}: rates {rates:?} against the stand-in's"
            );
            n += 1;
            most_w = most_w.max(dw);
            most_r = most_r.max(dr);
        }
        assert!(n > 60, "{load}: {n} steady steps");
        println!(
            "{load}: {n} steady steps, weights within {most_w:.2e}, rates within {most_r:.2e}"
        );
        held += 1;
        worst_w = worst_w.max(most_w);
        worst_r = worst_r.max(most_r);
    }
    assert_eq!(held, loads.len() - 1);
    println!(
        "{held} held: weights within {worst_w:.2e}, rates within {worst_r:.2e}; {:.1?} in all, load {:.2}",
        started.elapsed(),
        load1()
    );
}

/// AG.27 Verify (2): Eruptor, joined through the part with every walker
/// linked beside him, walks on AG.8's numbers with his scripts driving
/// (21.5 ft/s, 1440°/s: the numbers the file `giants_eruptor` holds on the
/// open ground, here on level 213's own), and his locomotion calls run.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn eruptors_ag8_numbers_hold_with_every_walker_linked() {
    println!("load before: {:.2}", load1());
    let started = Instant::now();
    let l = Level::open();
    let top = 21.5 * FOOT;
    for strength in [1.0_f32, 0.5, 0.2] {
        let mut w = W::joined(&l, "009_Eruptor", STRICT, 120)
            .map_err(|b| {
                let (_, when, h) = *b;
                format!("{when}: {}", named(&h.to_string()))
            })
            .unwrap();
        let mut track = Vec::new();
        for _ in 0..150u32 {
            // 90° to the left of the view's forward (heading 105°, the way
            // AG.12's Tree Rex walks, over open ground in the level).
            w.bench.pads[0] = Pad {
                stick: guac_motion::scenarios::stick(strength, 90.0),
                actions: [false; 4],
                connected: true,
            };
            w.step().unwrap();
            let h = w.hero();
            track.push((h.pivot, h.facing, h.velocity));
        }
        let turned = (1440.0 * strength / 60.0).min(105.0);
        let first = track.iter().position(|t| t.2 != Vec3::ZERO).unwrap();
        let ninety = track
            .iter()
            .position(|t| Vec3::new(t.2.x, 0.0, t.2.z).length() >= 0.9 * top * strength)
            .unwrap();
        let (a, b) = (track[first + 10].0, track[first + 40].0);
        let metres = Vec3::new(b.x - a.x, 0.0, b.z - a.z).length();
        println!(
            "stick {strength}: first facing {:.4} (1440°/s × S: {turned:.4}); moving from step {first}, 90 % at step {ninety} ({:.4} s on), {metres:.5} m over half a game second from step 10 (21.5 ft/s × S / 2 = {:.5})",
            track[0].1,
            (ninety - first) as f32 / 60.0,
            top * strength / 2.0
        );
        assert!(
            (track[0].1 - turned).abs() < 1e-3,
            "first frame facing {}",
            track[0].1
        );
        assert_eq!(first, 0, "the step the stick is read");
        assert!((metres - top * strength / 2.0).abs() < 1e-3, "{metres}");
        if strength == 1.0 {
            assert!(((ninety - first) as f32 / 60.0 - 0.1167).abs() < 1e-3);
        }
        if strength == 1.0 {
            let params = w.parameters();
            println!("  his description's movement parameters: {params:?}");
        }
    }
    // A full reversal at top speed: straight back, his facing round in
    // 0.13-0.15 s.
    let mut w = W::joined(&l, "009_Eruptor", STRICT, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    let mut track = Vec::new();
    for k in 0..150u32 {
        w.bench.pads[0] = Pad {
            stick: guac_motion::scenarios::stick(1.0, if k < 90 { 90.0 } else { 270.0 }),
            actions: [false; 4],
            connected: true,
        };
        w.step().unwrap();
        track.push((w.hero().walk.velocity, w.hero().facing));
    }
    let (before, after) = (track[89].0, track[91].0);
    let round = (90..150)
        .find(|&k| (track[k].1 - 285.0).abs() < 1e-3)
        .unwrap();
    let swing = (round - 89) as f32 / 60.0;
    println!("reversal: walk {before:?} -> {after:?}, facing round at step {round}, {swing:.4} s");
    assert!(
        (after + before).length() < 1e-3,
        "straight back at top speed"
    );
    assert!((0.13..=0.15).contains(&swing), "{swing}");
    // His locomotion calls ran on the VM, with the script's own arguments:
    // Define Locomotion States (walk 3.658, run 6.401 m/s), Initialize
    // Locomotions (switch time 0.25 s).
    let args = call_arguments(
        &w.vm,
        w.unit,
        &["walk speed", "run speed", "locomotion switch time"],
    );
    let has = |name: &str, v: f32| {
        args.iter()
            .any(|a| a.2 == name && (a.3 - v).abs() < 1e-6 && a.4 == 1)
    };
    println!("locomotion call arguments: {args:?}");
    assert!(has("walk speed", 3.658), "{args:?}");
    assert!(has("run speed", 6.401), "{args:?}");
    assert!(has("locomotion switch time", 0.25), "{args:?}");
    let program = w.vm.program();
    let called = |name: &str| {
        w.vm.calls()
            .iter()
            .filter(|(d, _)| program.defs[**d as usize].display == name)
            .map(|(_, n)| *n)
            .sum::<u64>()
    };
    let counts = [
        "Minion - Define Locomotion States",
        "Anim - Add Locomotion State",
        "Minion - Initialize Locomotions",
    ]
    .map(called);
    println!(
        "calls run: Define {} Add {} Initialize {}",
        counts[0], counts[1], counts[2]
    );
    assert!(counts[0] >= 1 && counts[1] >= 2 && counts[2] >= 1);
    println!("{:.2?}; load after: {:.2}", started.elapsed(), load1());
}

/// AE.22's live read of a figure: the hero's scale and `CollisionInfo`.
#[derive(Debug, Clone, PartialEq)]
struct Live {
    figure: String,
    scale: [f32; 3],
    size: [f32; 3],
    properties: u32,
}

/// AE.22's reads of `load`'s figures, one per figure the sweep read.
fn live_reads(load: &str) -> Vec<Live> {
    let root = capture_dir().join("BLUS30968/ae22-full-1792487613");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&root).unwrap().flatten() {
        let path = e.path().join("figure.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        if v["figure"]["load_file"].as_str() != Some(load) {
            continue;
        }
        let t = &v["tuning"];
        if t.is_null() {
            continue;
        }
        let c = &t["collision"];
        out.push(Live {
            figure: v["figure"]["id"].as_str().unwrap_or("").to_owned(),
            scale: [0, 1, 2].map(|i| json_f32(&t["scale"][i])),
            size: [
                json_f32(&c["width"]),
                json_f32(&c["height"]),
                json_f32(&c["length"]),
            ],
            properties: c["properties"].as_u64().unwrap_or(0) as u32,
        });
    }
    out.sort_by(|a, b| a.figure.cmp(&b.figure));
    out
}

/// What one character's 600 ticks came to.
#[derive(Debug, Default)]
struct Audit {
    load: String,
    /// Where it stopped, if it did: the phase and the halt with the member
    /// named.
    stopped: Option<(String, String)>,
    ticks: u32,
    set_up: bool,
    params: Option<[f32; 11]>,
    scale: Option<[f32; 3]>,
    /// The disc's solid (what the TV builds him from) and the solid his
    /// scripts hold on his actor, width, height, length (feet).
    solid: Option<[f32; 3]>,
    scripted_solid: Option<[f32; 3]>,
    /// His walk's top speed (ft/s) and how far he went on the full-stick
    /// stretch (m).
    top: f32,
    walked: f32,
    /// His minion's behaviour at the end.
    behaviour: Option<String>,
    /// The walk the TV's fall-back would give him if his scripts stopped
    /// now: his description's ground max speed (`Movement::read`), ft/s.
    fallback: Option<f32>,
    /// Every member his run reached, by name (AG.160's count).
    reached: Vec<String>,
}

impl Audit {
    /// How his 600 ticks went, headless.
    fn outcome(&self) -> &'static str {
        match (&self.stopped, self.set_up, self.params) {
            (Some(_), false, _) => "stops before his setup is through",
            (Some(_), true, _) => "stops after his setup",
            (None, false, _) => "runs, his setup never through",
            (None, true, Some(p)) if (self.top - p[0]).abs() < 1e-3 => "walks at his top speed",
            (None, true, _) => "set up, not at his top speed",
        }
    }
}

/// `load` joined and run 600 ticks: 120 standing, 240 with the stick full
/// toward 90° left of the view, 240 released.
fn audit(l: &Level, load: &str, o: Opts) -> Audit {
    let mut a = Audit {
        load: load.to_owned(),
        ..Audit::default()
    };
    let mut w = match W::joined(l, load, o, 0) {
        Ok(w) => w,
        Err(b) => {
            let (w, when, h) = *b;
            a.stopped = Some((when, named(&h.to_string())));
            a.read(&w);
            return a;
        }
    };
    let mut at = w.hero().pivot;
    for k in 0..600u32 {
        w.bench.pads[0] = Pad {
            stick: if (120..360).contains(&k) {
                guac_motion::scenarios::stick(1.0, 90.0)
            } else {
                Stick::CENTRED
            },
            actions: [false; 4],
            connected: true,
        };
        if k == 120 {
            at = w.hero().pivot;
        }
        if let Err(h) = w.step() {
            a.stopped = Some((format!("tick {k}"), named(&h.to_string())));
            break;
        }
        a.ticks = k + 1;
        a.top = a.top.max(w.horizontal_walk());
        if k == 359 {
            let p = w.hero().pivot;
            a.walked = Vec3::new(p.x - at.x, 0.0, p.z - at.z).length();
        }
    }
    a.read(&w);
    a
}

impl Audit {
    fn read(&mut self, w: &W) {
        self.set_up = w.minion.is_some_and(|m| w.vm.set_up(m));
        self.behaviour = w.minion.and_then(|m| w.vm.behaviour(m)).map(str::to_owned);
        self.fallback = w
            .minion
            .and_then(|m| Movement::read(&w.vm, m))
            .map(|m| m.ground_max_speed);
        self.params = w.parameters();
        if let Some(act) = w.world.actor(HERO) {
            self.scale = Some(act.scale);
            self.scripted_solid = Some([act.solid.width, act.solid.height, act.solid.length]);
        }
        self.solid = Some([w.walk.shape.width, w.walk.shape.height, w.walk.shape.length]);
        self.reached =
            w.vm.reached()
                .iter()
                .map(|m| w.vm.config().members.name(*m))
                .collect();
    }
}

/// AG.27 Verify (4): every other modelled character, one placement each,
/// strict mode over the real level for 600 ticks.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, AE.22's reads under GUAC_CAPTURE_DIR, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn every_other_modelled_character_walks_or_is_listed() {
    let started = Instant::now();
    let l = Level::open();
    println!(
        "the part: {} files, {} bytes read; read+parse+link {:.2?}; {} walkers, {} left out; load {:.2}",
        l.linked.0,
        l.linked.1,
        l.linked.2,
        l.walkers.len(),
        l.left_out.len(),
        load1()
    );
    let scripts_dir = l.dir.join("scripts");
    let mut on_disk = 0u64;
    let mut entries = 0;
    for e in std::fs::read_dir(&scripts_dir).unwrap().flatten() {
        on_disk += e.metadata().unwrap().len();
        entries += 1;
    }
    println!("scripts/ in the extract: {entries} files, {on_disk} bytes");
    let mut table = Vec::new();
    let mut loads: Vec<String> = l.walkers.iter().map(|w| w.load_file.clone()).collect();
    loads.retain(|x| x != TREE_REX);
    loads.insert(0, TREE_REX.to_owned());
    for load in &loads {
        let t = Instant::now();
        let a = audit(&l, load, STRICT);
        let live = live_reads(load);
        let matches = |s: Option<[f32; 3]>| {
            s.is_some_and(|s| {
                !live.is_empty()
                    && live
                        .iter()
                        .all(|v| v.size.iter().zip(s).all(|(x, y)| (x - y).abs() < 1e-3))
            })
        };
        let dims_ok = matches(a.solid);
        let script_dims_ok = matches(a.scripted_solid);
        let scale_ok = a.scale.is_some_and(|s| {
            !live.is_empty()
                && live
                    .iter()
                    .all(|v| v.scale.iter().zip(s).all(|(x, y)| (x - y).abs() < 1e-3))
        });
        let entry = l.walkers.iter().find(|w| &w.load_file == load).unwrap();
        println!(
            "{load}: {} [{}] ({:.1?}, load {:.2}); minion {:?}, asks {}; ticks {}, set up {}, params {:?}, scale {:?} vs live {:?} [{}], disc solid {:?} [{}] and scripted solid {:?} [{}] vs live {:?}; top {:.3} ft/s, walked {:.2} m, behaviour {:?}, fall-back speed {:?}",
            a.outcome(),
            match &a.stopped {
                Some((when, why)) => format!("STOPPED at {when}: {why}"),
                None => "RAN".to_owned(),
            },
            t.elapsed(),
            load1(),
            entry.minion,
            entry.asks,
            a.ticks,
            a.set_up,
            a.params,
            a.scale,
            live.iter().map(|v| v.scale).collect::<Vec<_>>(),
            scale_ok,
            a.solid,
            dims_ok,
            a.scripted_solid,
            script_dims_ok,
            live.iter().map(|v| v.size).collect::<Vec<_>>(),
            a.top,
            a.walked,
            a.behaviour,
            a.fallback
        );
        let kept = entry.asks > 0 || entry.minion.is_none();
        table.push((a, live, scale_ok, dims_ok, script_dims_ok, kept));
    }
    // Headless every walker's scripts run (asks let through); on the TV one
    // asking the viewport or with no minion keeps his card.
    let mut outcomes: BTreeMap<&str, (Vec<String>, usize)> = BTreeMap::new();
    for (a, .., kept) in &table {
        let o = outcomes.entry(a.outcome()).or_default();
        o.0.push(a.load.clone());
        o.1 += usize::from(*kept);
    }
    for (o, (who, kept)) in &outcomes {
        println!(
            "  {o}: {} ({kept} of them kept on their cards on the TV) - {who:?}",
            who.len()
        );
    }
    let ran = table.iter().filter(|t| t.0.stopped.is_none()).count();
    println!(
        "== {} characters: {ran} ran 600 ticks, {} stopped; scale matches AE.22's live read on {}, the disc's solid on {}, the scripted solid on {}; {} ran without his setup completing; {:.1?}, load {:.2}",
        table.len(),
        table.len() - ran,
        table.iter().filter(|t| t.2).count(),
        table.iter().filter(|t| t.3).count(),
        table.iter().filter(|t| t.4).count(),
        table
            .iter()
            .filter(|t| t.0.stopped.is_none() && !t.0.set_up)
            .count(),
        started.elapsed(),
        load1()
    );
    let mut by_member: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (a, ..) in &table {
        if let Some((when, why)) = &a.stopped {
            let member = why.split("= ").last().unwrap_or(why).to_owned();
            by_member
                .entry(member)
                .or_default()
                .push(format!("{} ({when})", a.load));
        }
    }
    for (m, who) in &by_member {
        println!("  stopped on {m}: {} - {who:?}", who.len());
    }
    // Every member the 66 runs reached, by namespace (AG.160), against
    // AH.34's ceiling of 324 over the fight's whitelist.
    let mut reached: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (a, ..) in &table {
        for m in &a.reached {
            let (ns, name) = m.split_once('.').unwrap_or(("?", m));
            reached
                .entry(ns.to_owned())
                .or_default()
                .insert(name.to_owned());
        }
    }
    let total: usize = reached.values().map(|s| s.len()).sum();
    println!(
        "members reached over the 66 runs: {total} in {} namespaces",
        reached.len()
    );
    for (ns, names) in &reached {
        println!("  {ns}: {}", names.len());
    }
    // AG.160: none stops, each on what his own scripts set.
    assert_eq!(ran, table.len(), "{by_member:?}");
    // AG.171: Wham-Shell and Drill Sergeant, set up and standing still on a
    // full stick before (their mover's controller lost to a stale `its`),
    // walk at their scripts' top speed.
    for load in ["013_Crab", "021_DrillBot"] {
        let (a, ..) = table
            .iter()
            .find(|t| t.0.load == load)
            .unwrap_or_else(|| panic!("{load} in the table"));
        assert_eq!(a.outcome(), "walks at his top speed", "{load}: {a:?}");
        assert!(a.walked > 10.0, "{load} walked {} m", a.walked);
    }
    // Wherever a character's setup ran through, his scale and the solid his
    // scripts hold are AE.22's live read.
    for (a, _, scale_ok, _, script_ok, _) in &table {
        if a.set_up {
            assert!(*scale_ok, "{}: scale {:?}", a.load, a.scale);
            assert!(*script_ok, "{}: solid {:?}", a.load, a.scripted_solid);
        }
    }
}

#[test]
#[ignore = "a probe: reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR"]
fn a_probe_of_a_walker_that_does_not_move() {
    let l = Level::open();
    let load = std::env::var("AG27_LOAD").unwrap_or_else(|_| "013_Crab".to_owned());
    let mut w = W::joined(&l, &load, STRICT, 120)
        .map_err(|b| {
            let (_, when, h) = *b;
            format!("{when}: {}", named(&h.to_string()))
        })
        .unwrap();
    for k in 0..20 {
        w.bench.pads[0] = Pad {
            stick: guac_motion::scenarios::stick(1.0, 90.0),
            actions: [false; 4],
            connected: true,
        };
        w.step().unwrap();
        if k == 10 {
            let m = w.minion.unwrap();
            let d = w.vm.local(m, "description").map(|v| w.vm.value(v)).unwrap();
            println!(
                "pad connected {:?}, behaviour {:?}",
                w.world.pads()[0],
                w.vm.behaviour(m)
            );
            dump_fields(&w.vm, d, 2, 0, &|n| {
                n.contains("velocity") || n.contains("input") || n.contains("controller")
            });
            println!(
                "walk {:?}, hero velocity {:?}",
                w.hero().walk.velocity,
                w.hero().velocity
            );
        }
    }
}

/// What one character's swap came to (openspec AH.165): taken off after
/// `before` steps and put back, then `after` steps on a full stick.
#[derive(Debug, Default)]
struct Swapped {
    load: String,
    /// Where the run stopped short of the swap (strict), if it did.
    short: Option<String>,
    /// Play mode's stops before the take-off.
    stops: Vec<String>,
    set_up: bool,
    /// His file's placements running at the take-off, and what the
    /// deactivation took off.
    running: usize,
    placements: usize,
    clones: usize,
    /// Where the deactivation stopped, if it did.
    halted: Option<String>,
    /// The portal manager stopped once he was off.
    pm_stopped: bool,
    /// After he's put back: the portal manager's ticks over the steps
    /// after, whether each placement taken off stayed off and silent,
    /// whether his file runs again once (a new placement under each handle,
    /// the same count), his new minion set up, his top speed on the stick
    /// once his arrival is over and his description's ground top speed.
    pm_ticks: f32,
    old_silent: bool,
    once: bool,
    set_up_again: bool,
    top_again: f32,
    ground_max: Option<f32>,
    /// The step his fall-back after the put-back took him off his scripts,
    /// if one did (AH.151: his walk AG.16's controller's from there).
    off_again: Option<u32>,
}

/// `load` joined (`o`), `before` steps standing, taken off by the deactivate
/// path and put back at once (the TV holds the level between: no step),
/// then `before` steps standing again (his arrival: AH.105's review saw the
/// replacement held 'disabled' about 60 steps) and `after` on a full stick.
fn swap(l: &Level, load: &str, o: Opts, before: u32, after: u32) -> Swapped {
    let mut out = Swapped {
        load: load.to_owned(),
        ..Swapped::default()
    };
    let mut w = match W::joined(l, load, o, before) {
        Ok(w) => w,
        Err(b) => {
            let (_, when, h) = *b;
            out.short = Some(format!("{when}: {}", named(&h.to_string())));
            return out;
        }
    };
    out.stops = std::mem::take(&mut w.stops);
    out.set_up = w.minion.is_some_and(|m| w.vm.set_up(m));
    let pm = w.portal_manager();
    let old = w.file_running();
    out.running = old.len();
    let first = w.minion.expect("joined");
    let gone = w.deactivate();
    out.placements = gone.placements.len();
    out.clones = gone.clones.len();
    out.halted = gone.halted.map(|h| named(&h.to_string()));
    out.pm_stopped = w.vm.stopped(pm).is_some();
    let silent: Vec<(PlacementId, f32)> = old
        .iter()
        .map(|&(_, id)| id)
        .chain([first])
        .map(|id| (id, w.vm.active_seconds(id).unwrap_or(0.0)))
        .collect();
    if let Err(h) = w.join() {
        out.short = Some(format!("the join again: {}", named(&h.to_string())));
        return out;
    }
    let ticked = w.vm.active_seconds(pm).unwrap_or(0.0);
    for k in 0..before + after {
        w.bench.pads[0] = Pad {
            stick: match k >= before {
                true => guac_motion::scenarios::stick(1.0, 90.0),
                false => Stick::CENTRED,
            },
            actions: [false; 4],
            connected: true,
        };
        if let Err(h) = w.step() {
            out.short = Some(format!("after the join again: {}", named(&h.to_string())));
            return out;
        }
        if k >= before {
            out.top_again = out.top_again.max(w.horizontal_walk());
        }
    }
    out.ground_max = w.parameters().map(|p| p[0]);
    out.pm_ticks = (w.vm.active_seconds(pm).unwrap_or(0.0) - ticked) * 60.0;
    out.old_silent = silent
        .iter()
        .all(|&(id, s)| w.vm.removed(id) && w.vm.active_seconds(id) == Some(s));
    let now = w.file_running();
    out.once =
        now.len() == old.len() && now.iter().zip(&old).all(|(n, o)| n.0 == o.0 && n.1 != o.1);
    out.set_up_again = w.minion.is_some_and(|m| w.vm.set_up(m));
    out.off_again = w.off.as_ref().map(|o| o.0);
    out
}

/// The walkers whose minions stop in play at `Minion.ai` #842 once the stick
/// goes on in a run that takes `collisioninfo.object collisions` out of its
/// registry ([`PLAY_842`]), as every run did before AG.160 modelled it:
/// Ninjini's two and [`GIANTS`]. On the registry as it stands none does.
fn falling_back() -> Vec<&'static str> {
    [NINJINI, "109_Djini_Alt"]
        .into_iter()
        .chain(GIANTS)
        .collect()
}

/// A swap table's put-back walks held (AH.151): every walker set up again
/// at his scripts' top speed after the put-back, on his scripts within
/// 1e-3 ft/s or, fallen back, on AG.16's controller within
/// [`WALK_ULPS`] of their number; the ones that fall back exactly
/// `expected` (the table's run's: none on the registry as it stands,
/// [`falling_back`]'s on [`PLAY_842`]).
fn put_back_walks(table: &[Swapped], expected: &[&str]) {
    let mut walking = 0;
    for s in table.iter().filter(|s| s.set_up_again) {
        let top = s.ground_max.expect("his movement parameters");
        match s.off_again {
            Some(_) => assert!(
                ulps(s.top_again, top) <= WALK_ULPS,
                "{}: fallen back at {} against his {top}",
                s.load,
                s.top_again
            ),
            None => assert!(
                (s.top_again - top).abs() < 1e-3,
                "{}: {} against his {top}",
                s.load,
                s.top_again
            ),
        }
        walking += 1;
    }
    let fell: Vec<&str> = table
        .iter()
        .filter(|s| s.off_again.is_some())
        .map(|s| s.load.as_str())
        .collect();
    println!(
        "put back: {walking} set up again at their top speed, {} of them fallen back: {fell:?}",
        fell.len()
    );
    assert_eq!(fell, expected, "fallen back after the put-back");
}

/// AH.165 on the dump (found at the AG.27 merge check: a walker joined,
/// taken off and placed again into the same run was never tested): headless
/// as the TV's swap takes it (the deactivate path, his figure's tag on the
/// portal again, AH.17's join), (1) Whirlwind, whose setup runs through, in
/// strict mode: nothing stops, his file's placements come off and back on
/// under their handles once, the ones taken off silent, his new minion set
/// up and walking at his top speed again, the portal manager ticking every
/// step after; (2) every walker the TV runs, in play mode as the TV runs
/// them, each taken off and put back the same way, the deactivations that
/// stop listed, the portal manager ticking on and his file running once
/// after every one, every one set up again on his scripts at his top speed
/// (AG.160), and the nine [`falling_back`] names again without
/// `collisioninfo.object collisions` ([`PLAY_842`]), each falling back after
/// his put-back at his stop at `Minion.ai` #842 (AH.151); (3) Eruptor taken
/// off at points through each of his three attacks, strict: whether his
/// deactivation stops anywhere.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_walker_taken_off_and_put_back_runs_his_file_once() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    // (1)
    let whirlwind = swap(&l, "000_AirDragon", STRICT, 120, 120);
    println!("(1) {whirlwind:?}");
    assert_eq!(whirlwind.short, None);
    assert_eq!(whirlwind.halted, None);
    assert!(whirlwind.set_up && whirlwind.set_up_again);
    assert!(!whirlwind.pm_stopped);
    assert!(
        (whirlwind.pm_ticks - 240.0).abs() < 1e-2,
        "{}",
        whirlwind.pm_ticks
    );
    assert!(whirlwind.old_silent && whirlwind.once);
    let top = whirlwind.ground_max.expect("his movement parameters");
    assert!(
        (whirlwind.top_again - top).abs() < 1e-3,
        "{} against {top}",
        whirlwind.top_again
    );
    println!("(1) {:.1?}, load {:.2}", started.elapsed(), load1());
    // (2)
    let tv: Vec<&WalkerScripts> = l
        .walkers
        .iter()
        .filter(|w| w.asks == 0 && w.minion.is_some())
        .collect();
    let mut table = Vec::new();
    for w in &tv {
        let t = Instant::now();
        let s = swap(&l, &w.load_file, PLAY, 60, 60);
        println!(
            "{}: set up {}, {} stops before ({:?}); running {}, off {} placements and {} clones; deactivation {}; pm stopped {}, ticks after {:.2}; old silent {}, once {}, set up again {}, top again {:?} of {:?} ({:.1?}, load {:.2})",
            s.load,
            s.set_up,
            s.stops.len(),
            s.stops.first(),
            s.running,
            s.placements,
            s.clones,
            s.halted.as_deref().unwrap_or("ran through"),
            s.pm_stopped,
            s.pm_ticks,
            s.old_silent,
            s.once,
            s.set_up_again,
            s.top_again,
            s.ground_max,
            t.elapsed(),
            load1()
        );
        table.push(s);
    }
    let halted: Vec<(&str, &str)> = table
        .iter()
        .filter_map(|s| Some((s.load.as_str(), s.halted.as_deref()?)))
        .collect();
    let walking = table
        .iter()
        .filter(|s| s.ground_max.is_some_and(|g| (s.top_again - g).abs() < 1e-3))
        .count();
    println!(
        "(2) {} walkers the TV runs: {} set up, {} set up again, {walking} at their top speed after; {} deactivations stopped {:?}; {:.1?}, load {:.2}",
        table.len(),
        table.iter().filter(|s| s.set_up).count(),
        table.iter().filter(|s| s.set_up_again).count(),
        halted.len(),
        halted,
        started.elapsed(),
        load1()
    );
    // The put-back walks: on the registry as it stands every walker set up
    // again walks on his scripts (AG.160).
    put_back_walks(&table, &[]);
    // And the nine whose stop at #842 a run without the member makes again,
    // each falling back after his put-back as the TV takes it (AH.151).
    let mut off = Vec::new();
    for load in falling_back() {
        let s = swap(&l, load, PLAY_842, 60, 60);
        println!(
            "{load} without object collisions: set up again {}, off again at {:?}, top again {:?} of {:?}",
            s.set_up_again, s.off_again, s.top_again, s.ground_max
        );
        off.push(s);
    }
    put_back_walks(&off, &falling_back());
    table.extend(off);
    for s in &table {
        assert_eq!(s.short, None, "{}", s.load);
        assert_eq!(s.set_up, s.set_up_again, "{}", s.load);
        assert!(!s.pm_stopped, "{}: the portal manager stopped", s.load);
        assert!(
            (s.pm_ticks - 120.0).abs() < 1e-2,
            "{}: the portal manager ticked {}",
            s.load,
            s.pm_ticks
        );
        assert!(s.old_silent && s.once, "{}: his file twice", s.load);
    }
    // (3)
    let mut eruptor = Vec::new();
    for action in 0..3usize {
        for after_press in [1u32, 10, 30, 60] {
            let mut w = W::joined(&l, "009_Eruptor", STRICT, 120)
                .map_err(|b| {
                    let (_, when, h) = *b;
                    format!("{when}: {}", named(&h.to_string()))
                })
                .unwrap();
            for k in 0..after_press {
                let mut actions = [false; 4];
                actions[action] = k == 0;
                w.bench.pads[0] = Pad {
                    stick: Stick::CENTRED,
                    actions,
                    connected: true,
                };
                w.step().unwrap();
            }
            let pm = w.portal_manager();
            let gone = w.deactivate();
            eruptor.push((
                action,
                after_press,
                gone.clones.len(),
                gone.halted.map(|h| named(&h.to_string())),
                w.vm.stopped(pm).is_some(),
            ));
        }
    }
    for e in &eruptor {
        println!(
            "(3) Eruptor, attack {} taken off {} steps after its press: {} clones off, deactivation {}, pm stopped {}",
            e.0 + 1,
            e.1,
            e.2,
            e.3.as_deref().unwrap_or("ran through"),
            e.4
        );
    }
    println!(
        "(3) {} take-offs, {} stopped; {:.1?} in all, load {:.2}",
        eruptor.len(),
        eruptor.iter().filter(|e| e.3.is_some()).count(),
        started.elapsed(),
        load1()
    );
    assert!(eruptor.iter().all(|e| !e.4));
}

/// AH.169 (the review of AH.165): the walkers the TV kept at the portal for
/// what their scripts ask of the viewport until AH.150 read the asks
/// ([`ASKING`]), Tree Rex among them, the AG.27 merge check's example of a
/// shutdown that stops, each joined in play mode as the TV joins them,
/// taken off and put back as
/// [`a_walker_taken_off_and_put_back_runs_his_file_once`]'s (2) takes the
/// others: which deactivations stop on the dump, and the portal manager
/// ticking on and his file running once after every one, whatever stopped;
/// every one set up again on his scripts (AG.160), and the seven giants
/// again without `collisioninfo.object collisions` ([`PLAY_842`]), each
/// falling back after his put-back (AH.151).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn the_asking_walkers_come_off_whatever_their_deactivation_does() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let asking: Vec<&WalkerScripts> = l
        .walkers
        .iter()
        .filter(|w| ASKING.contains(&w.load_file.as_str()) && w.minion.is_some())
        .collect();
    let mut table = Vec::new();
    for w in &asking {
        let s = swap(&l, &w.load_file, PLAY, 60, 60);
        println!(
            "{} (asks {}): set up {}, {} stops before ({:?}); running {}, off {} placements and {} clones; deactivation {}; pm stopped {}, ticks after {:.2}; old silent {}, once {}, set up again {}, top again {:?} of {:?}, short {:?}",
            s.load,
            w.asks + w.cosmetic + w.answered,
            s.set_up,
            s.stops.len(),
            s.stops.first(),
            s.running,
            s.placements,
            s.clones,
            s.halted.as_deref().unwrap_or("ran through"),
            s.pm_stopped,
            s.pm_ticks,
            s.old_silent,
            s.once,
            s.set_up_again,
            s.top_again,
            s.ground_max,
            s.short
        );
        table.push(s);
    }
    let halted: Vec<(&str, &str)> = table
        .iter()
        .filter_map(|s| Some((s.load.as_str(), s.halted.as_deref()?)))
        .collect();
    println!(
        "{} asking walkers: {} set up, {} deactivations stopped {:?}; {:.1?}, load {:.2}",
        table.len(),
        table.iter().filter(|s| s.set_up).count(),
        halted.len(),
        halted,
        started.elapsed(),
        load1()
    );
    // Tree Rex strict on the registry (his setup runs through since AH.49
    // and AG.160), standing and walking 120 steps each side of the swap.
    let rex = swap(&l, TREE_REX, TREE_REX_CELLS, 120, 120);
    println!(
        "{TREE_REX} strict: {rex:?}; {:.1?}, load {:.2}",
        started.elapsed(),
        load1()
    );
    assert_eq!(table.len(), ASKING.len(), "the fourteen");
    // The put-back walks: on the registry as it stands every one set up
    // again walks on his scripts (AG.160).
    put_back_walks(&table, &[]);
    // The seven giants without object collisions: each falls back after his
    // put-back at his stop at #842, as the TV takes it (AH.151).
    let mut off = Vec::new();
    for load in GIANTS {
        let s = swap(&l, load, PLAY_842, 60, 60);
        println!(
            "{load} without object collisions: set up again {}, off again at {:?}, top again {:?} of {:?}",
            s.set_up_again, s.off_again, s.top_again, s.ground_max
        );
        off.push(s);
    }
    put_back_walks(&off, &GIANTS);
    table.extend(off);
    if rex.short.is_none() {
        assert!(rex.set_up && rex.set_up_again, "{rex:?}");
        assert!(!rex.pm_stopped && rex.old_silent && rex.once, "{rex:?}");
        assert!((rex.pm_ticks - 240.0).abs() < 1e-2, "{rex:?}");
    }
    for s in &table {
        assert_eq!(s.short, None, "{}", s.load);
        assert!(!s.pm_stopped, "{}: the portal manager stopped", s.load);
        assert!(
            (s.pm_ticks - 120.0).abs() < 1e-2,
            "{}: the portal manager ticked {}",
            s.load,
            s.pm_ticks
        );
        assert!(s.old_silent && s.once, "{}: his file twice", s.load);
    }
}

/// The fourteen walkers whose scripts ask the gameplay viewport past the
/// TV's own run (AH.107's reader, AG.27's review): the ten AG.27's verify
/// held and the review's four, each minion the one his definition names.
const ASKING: [&str; 14] = [
    "002_SkyTurtle",
    "021_DrillBot",
    "025_BamBazooker",
    "029_ShadowMaid",
    "101_BugmanGiant",
    "102_Titan_Earth",
    "102_Titan_Earth_Alt",
    "104_OilBlazer",
    "107_Whale",
    "110_RobotGiant",
    "110_RobotGiant_Legendary",
    "112_TitanLife",
    "112_TitanLife_Alt",
    "114_Mummy",
];

/// How each of [`ASKING`] comes out on the TV's terms: joined in play mode
/// as the TV joins him, listed with every walker as the TV lists them, 600
/// ticks as AG.27's audit (120 standing, 240 on a full stick, 240 let go).
#[derive(Debug, Default)]
struct OnTheTv {
    load: String,
    set_up: bool,
    /// His minion's stop, if it stopped.
    stopped: Option<String>,
    /// Every stop play mode logged, each step's in turn.
    stops: Vec<String>,
    /// His walk's top speed on the full stick, ft/s, and how far it took
    /// him, metres: on his scripts, or once they stop on AG.16's controller
    /// at what they set (the fall-back, AH.151).
    top: f32,
    walked: f32,
    /// His description's ground max speed as his scripts wrote it.
    ground_max: Option<f32>,
    /// The step his fall-back took him off his scripts, if one did.
    off: Option<u32>,
    /// Who stood for the viewport at the end: handed back from a fall-back
    /// on, as the TV's (AG.190), whether the run steers by it or not.
    view: guacamole::viewport::Source,
}

impl OnTheTv {
    /// The TV's call on him (`hero_scripts`' fall-back, AG.27): a join that
    /// never ran through keeps his card, a stop after it walks him on what
    /// his scripts set, none walks him on his scripts.
    fn verdict(&self) -> String {
        match (self.set_up, &self.stopped) {
            (false, _) => format!(
                "keeps his card: his join never ran through ({})",
                self.stops.first().map_or("no stop", String::as_str)
            ),
            (true, Some(why)) => format!(
                "joins; his script stops in play ({why}), falling back at step {:?}, and he walks on what it set: top {:?} ft/s (his {:?}), {:.2} m",
                self.off, self.top, self.ground_max, self.walked
            ),
            (true, None) => format!(
                "joins and walks on his scripts: top {:.3} ft/s (his {:?}), {:.2} m",
                self.top, self.ground_max, self.walked
            ),
        }
    }
}

fn on_the_tv(l: &Level, load: &str, o: Opts) -> OnTheTv {
    let mut out = OnTheTv {
        load: load.to_owned(),
        ..OnTheTv::default()
    };
    let mut w = match W::joined(l, load, o, 0) {
        Ok(w) => w,
        Err(b) => {
            let (w, when, h) = *b;
            out.stops = w.stops.clone();
            out.stops.push(format!("{when}: {}", named(&h.to_string())));
            return out;
        }
    };
    let mut at = w.hero().pivot;
    for k in 0..600u32 {
        w.bench.pads[0] = Pad {
            stick: if (120..360).contains(&k) {
                guac_motion::scenarios::stick(1.0, 90.0)
            } else {
                Stick::CENTRED
            },
            actions: [false; 4],
            connected: true,
        };
        if k == 120 {
            at = w.hero().pivot;
        }
        if let Err(h) = w.step() {
            out.stops
                .push(format!("tick {k}: {}", named(&h.to_string())));
            break;
        }
        if (120..360).contains(&k) {
            out.top = out.top.max(w.horizontal_walk());
        }
        if k == 359 {
            let p = w.hero().pivot;
            out.walked = Vec3::new(p.x - at.x, 0.0, p.z - at.z).length();
        }
    }
    let minion = w.minion.expect("joined");
    out.set_up = w.vm.set_up(minion);
    out.stopped = w.vm.stopped(minion).map(|h| named(&h.to_string()));
    out.stops.extend(w.stops.iter().cloned());
    out.ground_max = w.parameters().map(|p| p[0]);
    out.off = w.off.as_ref().map(|o| o.0);
    out.view = w.world.viewport().source;
    out
}

/// AH.150's headless check: each of the fourteen's asks of the gameplay
/// viewport read for what it gates (`on_screen::READ`, none left unread),
/// every walker listed as the TV lists them with no ask let through, and
/// each of the fourteen joined in play mode as the TV joins him, the why
/// never the ask: since AG.160 every one joins and walks on his scripts at
/// their top speed. The seven giants again without `collisioninfo.object
/// collisions` ([`PLAY_842`]): each one's minion stops at `Minion.ai` #842
/// and he falls back on what his scripts set, at the step and over the
/// distance he did before AG.160. Then Drill Sergeant and Tree Rex at their
/// scripts' top speed, both strict on the registry (Tree Rex's two value
/// cells gone since AH.49 and AG.160 modelled their members).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn the_fourteen_asking_walkers_join_or_keep_their_cards_for_their_own_reasons() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let p = &*l.program;
    let base = scripts::smash_config(Mode::Play).whitelist;
    let mut known = std::collections::BTreeSet::new();
    for (id, s) in p.scripts.iter().enumerate() {
        if !l.walkers.iter().any(|w| w.unit == s.unit) && base.lists("", &s.name) {
            known.extend(on_screen::reached(p, id as u32).0);
        }
    }
    // Each walker's new asks, by their reads.
    let unread: Vec<&str> = l
        .walkers
        .iter()
        .filter(|w| w.asks > 0)
        .map(|w| w.load_file.as_str())
        .collect();
    assert!(unread.is_empty(), "asks no one has read: {unread:?}");
    let listing = scripts::with_walkers(scripts::smash_config(Mode::Play), &l.walkers).whitelist;
    let expected: [(&str, usize, usize); 14] = [
        ("002_SkyTurtle", 0, 2),
        ("021_DrillBot", 0, 1),
        ("025_BamBazooker", 0, 1),
        ("029_ShadowMaid", 0, 3),
        ("101_BugmanGiant", 1, 2),
        ("102_Titan_Earth", 1, 0),
        ("102_Titan_Earth_Alt", 1, 0),
        ("104_OilBlazer", 1, 0),
        ("107_Whale", 1, 0),
        ("110_RobotGiant", 1, 0),
        ("110_RobotGiant_Legendary", 1, 0),
        ("112_TitanLife", 1, 0),
        ("112_TitanLife_Alt", 1, 0),
        ("114_Mummy", 1, 0),
    ];
    for (load, cosmetic, answered) in expected {
        let w = l
            .walkers
            .iter()
            .find(|w| w.load_file == load)
            .unwrap_or_else(|| panic!("{load}: a walker"));
        let name = l
            .roster
            .figures
            .iter()
            .find(|f| f.load_file == load)
            .and_then(|f| f.name.texts.first())
            .map_or("?", String::as_str);
        println!(
            "{load} ({name}): asks {} unread, {} cosmetic, {} answered off the view; minion {:?}",
            w.asks, w.cosmetic, w.answered, w.minion
        );
        let mut seen = std::collections::BTreeSet::new();
        for (id, s) in p
            .scripts
            .iter()
            .enumerate()
            .filter(|(_, s)| s.unit == w.unit)
        {
            let file = s.name.rsplit(['/', '\\']).next().unwrap();
            if !w
                .scripts
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&format!("/{file}")))
            {
                continue;
            }
            for a in on_screen::reached(p, id as u32).0 {
                if known.contains(&a) || !seen.insert(a) {
                    continue;
                }
                let r = on_screen::read_of(p, &a, on_screen::READ).expect("read");
                println!(
                    "  {} #{} in {:?}, op {}: {:?}, {}",
                    r.file, a.position, r.def, a.op, r.verdict, r.gates
                );
            }
        }
        assert_eq!((w.cosmetic, w.answered), (cosmetic, answered), "{load}");
        let minion = p
            .scripts
            .iter()
            .find(|s| {
                s.unit == w.unit
                    && w.minion.as_deref().is_some_and(|m| {
                        s.name
                            .to_ascii_lowercase()
                            .ends_with(&m.to_ascii_lowercase())
                    })
            })
            .map(|s| s.name.clone())
            .unwrap_or_else(|| panic!("{load}: his minion's script"));
        assert!(listing.lists("", &minion), "{load}: on the TV's listing");
    }
    // The TV's run answers the gating ones off the camera's view, the
    // shake's not.
    let mut tv = bevy::ecs::world::World::new();
    scripts::install_with(
        &mut tv,
        l.program.clone(),
        &l.files,
        &l.order,
        scripts::with_walkers(scripts::smash_config(Mode::Play), &l.walkers),
    );
    let every = on_screen::every_ask(p);
    let (mut gating, mut cosmetic) = (0, 0);
    for a in &every {
        let Some(r) = on_screen::read_of(p, a, on_screen::READ) else {
            continue;
        };
        let answered = tv.resource::<ScriptWorld>().answers_from_view(a.op);
        match r.verdict {
            on_screen::Verdict::Gates => {
                assert!(
                    answered,
                    "{r:?} in unit {}",
                    p.scripts[a.script as usize].unit
                );
                gating += 1;
            }
            on_screen::Verdict::Cosmetic => {
                assert!(!answered, "{r:?}");
                cosmetic += 1;
            }
        }
    }
    println!(
        "the TV's run: {gating} gating asks answered off the view, {cosmetic} cosmetic on the stand-in's answer; reads {:.1?}, load {:.2}",
        started.elapsed(),
        load1()
    );
    assert_eq!((gating, cosmetic), (9, 1));
    // As the TV joins them.
    let mut table = Vec::new();
    for load in ASKING {
        let t = Instant::now();
        let r = on_the_tv(&l, load, PLAY);
        println!(
            "{load}: {} ({:.1?}, load {:.2}); stops {:?}",
            r.verdict(),
            t.elapsed(),
            load1(),
            r.stops
        );
        table.push(r);
    }
    for r in &table {
        println!(
            "  {}: set up {}, off at {:?}, top {:?} ft/s, walked {:?} m, his {:?}",
            r.load, r.set_up, r.off, r.top, r.walked, r.ground_max
        );
    }
    for r in &table {
        let why = r.verdict();
        assert!(!why.contains("viewport"), "{}: {why}", r.load);
        if !r.set_up {
            continue;
        }
        let top = r.ground_max.expect("his movement");
        assert!((r.top - top).abs() < 1e-3, "{}: {why}", r.load);
        assert_eq!(r.off, None, "{}: {why}", r.load);
    }
    // The seven giants again on a run without object collisions, so each
    // one's minion stops at `Minion.ai` #842 as before AG.160 modelled it:
    // the fall-back's step and how far the stick at 90° took him, m (the
    // fallback merge's integration `7f64e622` read them on its natural
    // stop, 2026-10-25), on AG.16's controller at their number to f32's
    // steps (AH.151) and the view handed back as the TV's (AG.190).
    let fallen: [(&str, u32, f32); 7] = [
        ("101_BugmanGiant", 124, 19.72424),
        ("102_Titan_Earth", 124, 13.7622175),
        ("102_Titan_Earth_Alt", 124, 13.7622175),
        ("107_Whale", 124, 19.288462),
        ("110_RobotGiant", 125, 23.729307),
        ("110_RobotGiant_Legendary", 125, 23.729307),
        ("114_Mummy", 124, 20.23483),
    ];
    assert_eq!(fallen.map(|f| f.0), GIANTS);
    for (load, step, walked) in fallen {
        let t = Instant::now();
        let r = on_the_tv(&l, load, PLAY_842);
        let why = r.verdict();
        println!(
            "{load} without object collisions: {why} ({:.1?}, load {:.2}); stops {:?}",
            t.elapsed(),
            load1(),
            r.stops
        );
        let top = r.ground_max.expect("his movement");
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|s| s.contains("Minion.ai #842")),
            "{load}: {why}"
        );
        assert!(ulps(r.top, top) <= WALK_ULPS, "{load}: {why}");
        assert_eq!(r.view, guacamole::viewport::Source::HandedBack, "{load}");
        assert_eq!(r.off, Some(step), "{load}: his fall-back's step");
        assert_eq!(r.walked, walked, "{load}: walked");
    }
    let drill = table.iter().find(|r| r.load == "021_DrillBot").unwrap();
    assert!(
        drill.set_up && drill.stopped.is_none(),
        "{}",
        drill.verdict()
    );
    // AG.160: as the TV joins them, every one of the fourteen joins and
    // walks on his scripts, none stopping in play.
    for r in &table {
        assert!(
            r.set_up && r.stopped.is_none(),
            "{}: {}",
            r.load,
            r.verdict()
        );
    }
    // Strict: Drill Sergeant and Tree Rex on the registry (since AH.49 and
    // AG.160 no member of Tree Rex's is a value cell).
    for (load, o) in [("021_DrillBot", STRICT), (TREE_REX, TREE_REX_CELLS)] {
        let t = Instant::now();
        let a = audit(&l, load, o);
        println!(
            "{load} strict{}: {} [{}], top {:.4} ft/s, walked {:.2} m, params {:?} ({:.1?}, load {:.2})",
            if o.cells.is_empty() {
                ""
            } else {
                " on value cells"
            },
            a.outcome(),
            a.stopped
                .as_ref()
                .map_or("RAN".to_owned(), |s| format!("{s:?}")),
            a.top,
            a.walked,
            a.params,
            t.elapsed(),
            load1()
        );
        assert_eq!(a.outcome(), "walks at his top speed", "{load}: {a:?}");
        if load == TREE_REX {
            assert!((a.top - 20.0).abs() < 1e-3, "his 20 ft/s: {}", a.top);
        }
    }
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}

/// Ninjini, on the TV's listing (no viewport ask): his minion stops in play
/// at `Minion.ai` #842 in a run without `collisioninfo.object collisions`
/// ([`PLAY_842`]), as in every run until AG.160 modelled it.
const NINJINI: &str = "109_Djini";
/// Play mode on the registry as it stands, as the TV runs the scripts.
const PLAY: Opts = Opts {
    mode: Mode::Play,
    discover: false,
    cells: &[],
    without: &[],
};
/// [`PLAY`] with `collisioninfo.object collisions` taken out of the registry
/// (`Members::without`, test-only): the member AG.160 modelled, which
/// `Minion - Titan Abilities` asks at `Minion.ai` #842 on the first step he
/// moves, so Ninjini's two and the seven giants stop in play there as they
/// did on the fallback merge's tree (`126da6a4`) and the fall-back takes
/// each the TV's way (his minion's script stopped, his locomotion's reading
/// what it reads then). Nothing else in the run changes.
const PLAY_842: Opts = Opts {
    without: &[("collisioninfo", "object collisions")],
    ..PLAY
};

/// `load` joined as the TV joins him (play mode, steering by the view the
/// TV draws; `o`, [`PLAY`] or [`PLAY_842`]), `settle` steps standing after,
/// his fall-back taking his walk (`hold_walk`) and the view
/// (`hand_back_view`) as the TV's does or not.
fn joined_on_the_tv(
    l: &Level,
    load: &str,
    o: Opts,
    settle: u32,
    hold_walk: bool,
    hand_back_view: bool,
) -> W {
    assert_eq!(o.mode, Mode::Play, "as the TV runs them");
    let mut w = W::new(l, load, o).on_the_tvs_view();
    w.hold_walk = hold_walk;
    w.hand_back_view = hand_back_view;
    w.step().unwrap();
    w.step().unwrap();
    w.join().unwrap();
    for _ in 0..settle {
        w.step().unwrap();
    }
    w
}

/// One full-stick walk after a walker's script stops in play.
#[derive(Debug, Default)]
struct OffWalk {
    stop: Option<(u32, String)>,
    /// His walk's length after the stop: the most and the last, ft/s.
    top: f32,
    last: f32,
    /// How far he went after it, m, over how many steps.
    walked: f32,
    steps: u32,
    /// The TV's eye's distance from his feet the step it went to the chain
    /// and at the end, and how far the eye moved; the scripts' viewport's
    /// eye from his feet at the end.
    gap: (f32, f32),
    eye_moved: f32,
    scripts_gap: f32,
    /// What the scripts wrote to him off his scripts, by writer: his walk
    /// and facing (dropped) and his place (kept).
    written: Vec<(String, String, u32)>,
    /// What play mode stopped over the run, each once.
    stops: Vec<String>,
}

/// `w` on a full stick straight ahead for `steps` steps, his fall-back
/// where his minion's script stops in play, as the TV's.
fn walk_off(w: &mut W, steps: u32) -> OffWalk {
    let mut out = OffWalk::default();
    let mut from = None;
    let mut eye0 = None;
    for _ in 0..steps {
        w.bench.pads[0] = Pad {
            stick: guac_motion::scenarios::stick(1.0, 0.0),
            actions: [false; 4],
            connected: true,
        };
        w.step().unwrap();
        if w.off.is_none() {
            continue;
        }
        let p = w.hero().pivot;
        let eye = w.drawn.map(|d| d.eye).unwrap_or(Vec3::NAN);
        if from.is_none() {
            from = Some(p);
            eye0 = Some(eye);
            out.gap.0 = eye.distance(p);
        }
        out.steps += 1;
        out.last = w.horizontal_walk();
        out.top = out.top.max(out.last);
        out.walked = Vec3::new(p.x - from.unwrap().x, 0.0, p.z - from.unwrap().z).length();
        out.gap.1 = eye.distance(p);
        out.eye_moved = eye.distance(eye0.unwrap());
        out.scripts_gap = w.world.viewport().eye.distance(p);
    }
    out.stop = w.off.clone();
    out.stops = w.stops.clone();
    out.stops.dedup();
    out.written = w
        .world
        .hero_writes()
        .iter()
        .map(|((by, what), n)| (w.placement_named(*by), format!("{what:?}"), *n))
        .collect();
    out
}

/// AH.151's and AG.190's Verify headless: Ninjini (`109_Djini`) on the TV's
/// listing in play mode, steering by the view the TV draws, without
/// `collisioninfo.object collisions` ([`PLAY_842`]: nothing on the dump
/// stops in play since AG.160 modelled it), stops in play once the stick
/// goes on (`Minion.ai` #842, where every run stopped him before AG.160)
/// and falls back on AG.16's controller at what his scripts set: at his 24
/// ft/s with the mover update's writes to his walk dropped, the view the
/// chain's from where it stood, following him. Beside it, each half undone:
/// without the hold the update writes him still each step (one step's
/// push, 2.667 ft/s); without the hand-back the scripts' view stays where
/// his camera target stopped. And on the registry as it stands his scripts
/// run on through the same walk.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_walker_off_his_scripts_walks_at_his_top_speed_with_the_view_following() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    // AG.160: the registry as it stands, no stop on the same walk.
    let mut w = joined_on_the_tv(&l, NINJINI, PLAY, 120, true, true);
    let on = walk_off(&mut w, 240);
    println!(
        "{NINJINI} on the registry as it stands: stop {:?}, play mode's stops {:?}; load {:.2}",
        on.stop,
        on.stops,
        load1()
    );
    assert_eq!((on.stop, on.stops.len()), (None, 0), "his scripts run on");
    let mut runs = Vec::new();
    for (hold, hand_back) in [(true, true), (false, true), (true, false)] {
        let mut w = joined_on_the_tv(&l, NINJINI, PLAY_842, 120, hold, hand_back);
        let m = w.minion.expect("joined");
        assert!(w.vm.set_up(m), "his setup ran through");
        assert!(w.off.is_none(), "on his scripts standing: {:?}", w.off);
        assert_eq!(
            w.world.viewport().source,
            guacamole::viewport::Source::Scripts,
            "the camera's scripts have the view"
        );
        let top = Movement::read(&w.vm, m)
            .expect("his movement parameters")
            .ground_max_speed;
        let walk = walk_off(&mut w, 240);
        println!(
            "{NINJINI} hold walk {hold}, hand back view {hand_back}: stop {:?}; after it {} steps, walk top {:.4} last {:.4} ft/s (his description's {top}), {:.2} m walked; the TV's eye {:.3} m from his feet at the hand-back, {:.3} at the end, moved {:.2} m; the scripts' eye {:.3} m from him at the end; written {:?}; play mode's stops {:?}; load {:.2}",
            walk.stop,
            walk.steps,
            walk.top,
            walk.last,
            walk.walked,
            walk.gap.0,
            walk.gap.1,
            walk.eye_moved,
            walk.scripts_gap,
            walk.written,
            walk.stops,
            load1()
        );
        runs.push((hold, hand_back, top, walk));
    }
    let (_, _, top, both) = &runs[0];
    let (when, why) = both.stop.as_ref().expect("his script stops in play");
    // Where he stops without the member (AG.28 took his stop at
    // `MusicController.ai` #1227 away, AG.160 this one but for the switch).
    assert!(
        why.contains("Minion.ai #842") && why.contains("unmodelled member 075b657b.bb4361e7"),
        "{why}"
    );
    assert!(
        *when > 120,
        "after his join's 120 steps standing: step {when}"
    );
    assert!((both.top - top).abs() < 1e-3, "at his top speed: {both:?}");
    assert!(both.walked > 10.0, "{both:?}");
    assert!(
        (both.gap.1 - both.gap.0).abs() < 0.3 && both.eye_moved > 10.0,
        "the view following him: {both:?}"
    );
    let walk_writes: u32 = both
        .written
        .iter()
        .filter(|(_, what, _)| what == "Walk")
        .map(|(.., n)| n)
        .sum();
    assert!(
        walk_writes > 100,
        "the scripts' writes to his walk dropped: {both:?}"
    );
    // Unheld, the update writes him still each step under the controller's
    // push: one step's, 160 ft/s² over 1/60 s.
    let (.., no_hold) = &runs[1];
    assert!(
        (no_hold.last - 160.0 / 60.0).abs() < 0.01 && no_hold.walked < 5.0,
        "unheld, the update stills him: {no_hold:?}"
    );
    // Not handed back, the scripts' view trails him: his camera target
    // isn't stepped off the joined run.
    let (.., no_hand_back) = &runs[2];
    assert!((no_hand_back.top - top).abs() < 1e-3);
    assert!(
        no_hand_back.gap.1 - no_hand_back.gap.0 > 2.0,
        "the scripts' view left behind: {no_hand_back:?}"
    );
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}

/// The seven giants AH.150 lets join (Swarm, both Crushers, Thumpback, both
/// Bouncers, Eye-Brawl): in a run without `collisioninfo.object collisions`
/// ([`PLAY_842`]) each one's minion stops in play at `Minion.ai` #842 a few
/// steps into the stick, as Ninjini's does and as every run's did until
/// AG.160 modelled it.
const GIANTS: [&str; 7] = [
    "101_BugmanGiant",
    "102_Titan_Earth",
    "102_Titan_Earth_Alt",
    "107_Whale",
    "110_RobotGiant",
    "110_RobotGiant_Legendary",
    "114_Mummy",
];

/// How far apart two numbers of one sign are in f32 steps (units in the
/// last place): the precision a number the controller computes in f32 is
/// held to against one the scripts wrote.
fn ulps(a: f32, b: f32) -> u32 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs() as u32
}

/// What one of [`GIANTS`] came to at his stop at `Minion.ai` #842 (the
/// fallback merge's integration, `7f64e622`, headless, 2026-10-25, its
/// natural stop; [`PLAY_842`]'s since AG.160): his stop's step, his walk's
/// top (ft/s), how far he walked after the stop and how far the TV's eye
/// moved with him (m), and the writes to his walk dropped. Each number as
/// f32 prints it.
struct GiantOff {
    load: &'static str,
    stop: u32,
    top: f32,
    walked: f32,
    eye_moved: f32,
    writes: u32,
}

const GIANTS_OFF: [GiantOff; 7] = [
    GiantOff {
        load: "101_BugmanGiant",
        stop: 124,
        top: 20.000008,
        walked: 13.043735,
        eye_moved: 13.218856,
        writes: 714,
    },
    GiantOff {
        load: "102_Titan_Earth",
        stop: 124,
        top: 20.000008,
        walked: 13.015394,
        eye_moved: 13.038585,
        writes: 714,
    },
    GiantOff {
        load: "102_Titan_Earth_Alt",
        stop: 124,
        top: 20.000008,
        walked: 13.015394,
        eye_moved: 13.038585,
        writes: 714,
    },
    GiantOff {
        load: "107_Whale",
        stop: 124,
        top: 20.000013,
        walked: 12.958654,
        eye_moved: 12.986823,
        writes: 714,
    },
    GiantOff {
        load: "110_RobotGiant",
        stop: 125,
        top: 24.000002,
        walked: 12.702467,
        eye_moved: 12.890366,
        writes: 711,
    },
    GiantOff {
        load: "110_RobotGiant_Legendary",
        stop: 125,
        top: 24.000002,
        walked: 12.702467,
        eye_moved: 12.890366,
        writes: 711,
    },
    GiantOff {
        load: "114_Mummy",
        stop: 124,
        top: 20.000008,
        walked: 13.736573,
        eye_moved: 14.038888,
        writes: 714,
    },
];

/// The most f32 steps a fallen-back walk's top is from what his scripts
/// set: the controller's walk is f32 throughout and its length a hypot of
/// two f32 parts (7 the most headless, Thumpback's; the Mac TV's walks 12
/// at most, AG.212).
const WALK_ULPS: u32 = 8;

/// AH.151's and AG.190's on-dump case since AH.150 (the fallback merge's
/// check): each of [`GIANTS`] joined as the TV joins him, steering by the
/// view the TV draws, without `collisioninfo.object collisions`
/// ([`PLAY_842`]: since AG.160 nothing stops on the registry as it
/// stands), 120 steps standing, then a full stick for 240: his stop at
/// `Minion.ai` #842, his walk on AG.16's controller at what his scripts set
/// (within [`WALK_ULPS`] of it), the update's writes to his walk dropped,
/// the view the chain's from where it stood until a join, following him;
/// against [`GIANTS_OFF`]'s numbers.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn the_seven_giants_fall_back_at_their_scripts_speed_with_the_view_following() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    for want in &GIANTS_OFF {
        let load = want.load;
        let t = Instant::now();
        let mut w = joined_on_the_tv(&l, load, PLAY_842, 120, true, true);
        let m = w.minion.expect("joined");
        assert!(w.vm.set_up(m), "{load}: his setup ran through");
        assert!(
            w.world.viewport().scripted(),
            "{load}: the scripts have the view standing"
        );
        let top = Movement::read(&w.vm, m)
            .expect("his movement parameters")
            .ground_max_speed;
        let walk = walk_off(&mut w, 240);
        let walk_writes: u32 = walk
            .written
            .iter()
            .filter(|(_, what, _)| what == "Walk")
            .map(|(.., n)| n)
            .sum();
        println!(
            "{load}: the view {:?} after (back at a join {}); stop {:?}; after it {} steps, walk top {:?} last {:?} ft/s (his description's {top:?}, {} f32 steps off), walked {:?} m; the TV's eye {:?} m from his feet at the hand-back, {:?} at the end, moved {:?} m; the scripts' eye {:?} m from him at the end; {walk_writes} walk writes dropped; written {:?}; play mode's stops {:?} ({:.1?}, load {:.2})",
            w.world.viewport().source,
            w.world.view_back_at_a_join(),
            walk.stop,
            walk.steps,
            walk.top,
            walk.last,
            ulps(walk.top, top),
            walk.walked,
            walk.gap.0,
            walk.gap.1,
            walk.eye_moved,
            walk.scripts_gap,
            walk.written,
            walk.stops,
            t.elapsed(),
            load1()
        );
        // His fall-back: his scripts' walk on the controller, theirs the one
        // writer dropped, the view the chain's until a join and following
        // him.
        assert!(
            ulps(walk.top, top) <= WALK_ULPS,
            "{load}: top {} against his {top}",
            walk.top
        );
        assert!(walk_writes > 100, "{load}: {walk_writes} writes dropped");
        assert_eq!(
            w.world.viewport().source,
            guacamole::viewport::Source::HandedBack,
            "{load}"
        );
        assert!(w.world.view_back_at_a_join(), "{load}: until a join");
        assert!(
            (walk.gap.1 - walk.gap.0).abs() < 0.3 && (walk.eye_moved - walk.walked).abs() < 0.5,
            "{load}: the view following him: {walk:?}"
        );
        let (when, why) = walk.stop.clone().expect("his script stops in play");
        assert!(
            why.contains("Minion.ai #842") && why.contains("unmodelled member 075b657b.bb4361e7"),
            "{load}: {why}"
        );
        assert_eq!(when, want.stop, "{load}: his stop's step");
        assert_eq!(walk.top, want.top, "{load}: his top");
        assert_eq!(walk.walked, want.walked, "{load}: walked");
        assert_eq!(walk.eye_moved, want.eye_moved, "{load}: the eye moved");
        assert_eq!(walk_writes, want.writes, "{load}: writes dropped");
    }
    println!(
        "the seven stop at #842 and fall back; {:.1?} in all, load {:.2}",
        started.elapsed(),
        load1()
    );
}

/// A record of AG.39's free in the air: unsupported 0.4 ft or more over the
/// ground point, his manifold's closest point 2 cm or more off.
fn free_in_the_air(r: &serde_json::Value) -> bool {
    let pivot = json_vec3(&r["hero"]["position"]);
    let ground = json_vec3(&r["contacts"]["ground"]["location"]);
    let high = r["contacts"]["support"].as_u64() != Some(2) && pivot.y - ground.y >= 0.4 / 3.28084;
    let first = r["contacts"]["planes"]
        .as_array()
        .and_then(|p| p.first())
        .and_then(|p| p["distance"].as_f64());
    high && first.is_none_or(|d| d >= 0.02)
}

/// His controller set to a record of AG.39's: pivot, facing, lean, footing
/// as its contacts read it, and his walk as what moved his body into it.
fn seed_in_the_air(w: &mut W, r: &serde_json::Value) {
    use guac_motion::ground::{LEAN_SETTLED, angle as tilt};
    let pivot = json_vec3(&r["hero"]["position"]);
    let rotation = r["hero"]["rotation"].as_array().unwrap();
    let rotation = [0, 1, 2].map(|i| rotation[i].as_f64().unwrap() as f32);
    let ground = &r["contacts"]["ground"];
    let normal = json_vec3(&ground["normal"]);
    let hero = w.bench.heroes.get_mut(&HERO).unwrap();
    hero.pivot = pivot;
    hero.facing = rotation[1].rem_euclid(360.0);
    hero.up = guac_motion::solid::euler(rotation) * Vec3::Y;
    hero.footing = guac_motion::walk::Footing {
        supported: r["contacts"]["support"].as_u64() == Some(2),
        height: Some(pivot.y - json_vec3(&ground["location"]).y),
        slope: if tilt(hero.up, normal) <= LEAN_SETTLED {
            hero.up
        } else {
            normal
        },
        ground: Some(normal),
    };
    hero.walk.velocity = json_vec3(&r["hero"]["velocity"]) / FOOT;
    hero.walk.landing = None;
}

/// AG.173's Verify: a walker whose script stopped in play (Ninjini, his
/// stop at `Minion.ai` #842 in a run without `collisioninfo.object
/// collisions`, [`PLAY_842`], taken as the TV takes it) falls on the
/// fall-back as AG.10 measured Eruptor's on
/// the same drop: each of AG.39's 40 falls off the ledge route into the
/// corridor, him set to its second free record and fed each record's stick
/// and view at our 1/60 s, his downward speed gaining 136.21 ft/s² over the
/// step before up in the air, and landing on the record theirs does or the
/// one either side. Beside it, the description's 128 he fell at before.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.39's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn a_stopped_walkers_fall_is_ag10s_on_the_same_drop() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let root = capture_dir().join("BLUS30968/ag39-falls-1792502813");
    let mut recordings: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    let mut groups: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    groups.sort();
    for g in groups {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&g)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("recording.json").is_file())
            .collect();
        dirs.sort();
        for d in dirs {
            let text = std::fs::read_to_string(d.join("recording.json")).unwrap();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            let label = d.strip_prefix(&root).unwrap().display().to_string();
            recordings.push((label, v.as_array().unwrap().clone()));
        }
    }
    let mut table = Vec::new();
    // Ninjini on the fall-back, then falling at his description's 128 as he
    // did before AG.173, then Eruptor's fall-back on the same seeds (his
    // stop forced: his scripts never stop on the dump), the controller's own
    // landings (stand-in ledger: `walker`).
    for (load, o, gravity) in [
        (NINJINI, PLAY_842, None),
        (NINJINI, PLAY_842, Some(128.0)),
        ("009_Eruptor", PLAY, None),
    ] {
        // His stop as the TV's: joined, standing, then the stick on.
        let mut w = W::new(&l, load, o);
        w.step().unwrap();
        w.step().unwrap();
        w.join().unwrap();
        for _ in 0..120 {
            w.step().unwrap();
        }
        for _ in 0..30 {
            w.bench.pads[0] = Pad {
                stick: guac_motion::scenarios::stick(1.0, 0.0),
                actions: [false; 4],
                connected: true,
            };
            w.step().unwrap();
        }
        if load == "009_Eruptor" {
            assert!(w.off.is_none(), "Eruptor's scripts run on");
            w.fall_back("forced: the control".to_owned());
        }
        let stop = w.off.clone().expect("his script stops in play");
        let mut tuning = *w.hero().tuning();
        if let Some(g) = gravity {
            tuning.gravity = g;
            w.bench.heroes.get_mut(&HERO).unwrap().set_tuning(tuning);
        }
        let (mut falls, mut near, mut never) = (0, 0, 0);
        let (mut free, mut worst) = (0usize, 0.0f32);
        let mut gains = Vec::new();
        let mut recorded: Vec<f64> = Vec::new();
        for (label, records) in &recordings {
            let mut k = 1;
            while k + 1 < records.len() {
                let second = free_in_the_air(&records[k])
                    && free_in_the_air(&records[k - 1])
                    && (k < 2 || !free_in_the_air(&records[k - 2]));
                if !second {
                    k += 1;
                    continue;
                }
                let theirs = (k + 1..records.len())
                    .find(|&i| records[i]["contacts"]["support"].as_u64() == Some(2));
                let end = theirs.map_or(records.len() - 1, |t| (t + 3).min(records.len() - 1));
                // The drop's own gains over the same records: free with the
                // two before, over the frame time two records back (AG.10's).
                for i in (k + 1).max(2)..=end {
                    if (i - 2..=i).all(|j| free_in_the_air(&records[j])) {
                        let vy = |j: usize| json_vec3(&records[j]["hero"]["velocity"]).y / FOOT;
                        let dt = json_f32(&records[i - 2]["clock"]["frame_delta"]);
                        recorded.push(f64::from((vy(i - 1) - vy(i)) / dt));
                    }
                }
                seed_in_the_air(&mut w, &records[k]);
                let mut ours = None;
                for (i, record) in records.iter().enumerate().take(end).skip(k) {
                    w.bench.view = record["camera"]["rotation"][1].as_f64().unwrap() as f32;
                    w.bench.pads[0] = record_pad(record);
                    let high = w.hero().footing.high();
                    let vy = w.hero().walk.velocity.y;
                    w.step().unwrap_or_else(|h| panic!("{label} step {i}: {h}"));
                    let h = w.hero();
                    if high && h.footing.high() {
                        let gain = (vy - h.walk.velocity.y) / w.dt;
                        worst = worst.max((gain - 136.21).abs());
                        gains.push(gain);
                        free += 1;
                    }
                    if h.footing.supported {
                        ours = Some(i + 1);
                        break;
                    }
                }
                falls += 1;
                near +=
                    usize::from(matches!((ours, theirs), (Some(a), Some(b)) if a.abs_diff(b) <= 1));
                never += usize::from(ours.is_none());
                k = end.max(k + 1);
            }
        }
        // In f64: an f32 sum of 400-odd gains drifts by 1e-3 over them.
        let mean_of = |g: &[f64]| g.iter().sum::<f64>() / g.len().max(1) as f64;
        let ours: Vec<f64> = gains.iter().map(|&g| f64::from(g)).collect();
        let (mean, theirs) = (mean_of(&ours), mean_of(&recorded));
        println!(
            "{load} off his scripts ({stop:?}), falling at {} ft/s² to {}: {falls} falls, {near} landing within a record of theirs, {never} not by theirs + 3; {free} free steps gaining {mean:.4} ft/s² on average, the most from 136.21 {worst:.4}; the drops' own {} records gaining {theirs:.4}; load {:.2}",
            tuning.gravity,
            tuning.terminal,
            recorded.len(),
            load1()
        );
        table.push((gravity, tuning, falls, near, never, worst, mean - theirs));
    }
    let (_, tuning, falls, near, never, worst, off) = table[0];
    assert_eq!((tuning.gravity, tuning.terminal), (136.21, 120.0));
    assert_eq!(falls, 40, "AG.39's 40");
    assert!(worst < 1e-2, "{worst} ft/s² off AG.10's");
    assert!(off.abs() < 1e-2, "{off} ft/s² off the drops' own gain");
    assert!(
        table[1].5 > 8.0 && table[1].6.abs() > 8.0,
        "the description's 128 misses it"
    );
    println!("on the fall-back: {near} of {falls} within a record, {never} not landing");
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}

/// The four clips his gait reports name, by his palette's slots: his
/// scripts' standing state's clip for the idle (his model's on all 67),
/// then his hero side's walk, run and fall at the hero side's slots where
/// his palette holds them there (`Four::at`; the TV names them off his
/// model's idle and his hero side, `HeroClips::four`).
fn four_of(
    palette: &Palette,
    hero: &guac_ir::Hero,
    states: &[walkers::LocomotionState],
) -> guacamole::picks::Four {
    let idle = states
        .iter()
        .find(|s| s.speed == 0.0)
        .and_then(|s| s.slot)
        .and_then(|s| palette.clip(s as u16))
        .map(|c| (c.name.as_str(), None));
    let l = &hero.locomotion;
    guacamole::picks::Four::at(palette, [idle, at(&l.walk), at(&l.run), at(&l.falling)])
}

/// A hero side's locomotion slot's clip name and its slot.
fn at(c: &Option<guac_ir::hero::SlotClip>) -> Option<(&str, Option<u16>)> {
    c.as_ref().map(|c| (c.clip.name.as_str(), Some(c.slot)))
}

/// AH.198 and AG.219 headless on the dump, the TV's hand-over in the
/// harness: each hero walked on a full stick on his scripts, their picks
/// read every step (`Picked::read`, as `pick_gaits` reads them), his
/// fall-back then taking him to AG.30's locomotion as the TV's does (a
/// walker's built off his own scripts' locomotion states,
/// `walkers::stand_in`; Eruptor's his measured gaits) and carrying it on
/// from the picks' last (`Locomotion::take_over`), stepped on his walk on
/// the controller after. Ninjini and Swarm stop at `Minion.ai` #842 the
/// TV's way ([`PLAY_842`]); Eruptor, Tree Rex (a walker with a walk and a
/// run), Dino-Rang (his run at two slots of his palette, his scripts
/// playing his hero side's) and Double Trouble (one clip for his walk and
/// run, at three) are forced off as AG.173's control is, nothing on the dump stopping
/// them. Beside each walker, AG.27's locomotion (no walk or run of ours) on
/// the same walk: his idle at any speed.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_fallen_back_heros_stand_in_carries_on_from_his_picks() {
    use guac_motion::locomotion::{Clip, EASE, Frame, Gaits, Locomotion, Loops, SET_OFF};
    use guacamole::picks::Picked;
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    for (load, o, force) in [
        (NINJINI, PLAY_842, false),
        ("101_BugmanGiant", PLAY_842, false),
        ("009_Eruptor", PLAY, true),
        (TREE_REX, PLAY, true),
        // His run at slots 2 and 3, his scripts playing 3 (AH.198's review:
        // 11 of the 67 so); and his walk and run one clip, at 1, 2 and 3.
        ("006_DinoRang", PLAY, true),
        ("018_Wizard", PLAY, true),
    ] {
        let mut w = joined_on_the_tv(&l, load, o, 120, true, true);
        let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(load));
        let hero: guac_ir::Hero = read_ron(&l.dir, &IrFormat::Ron.hero_path(load));
        let states = walkers::locomotion_states(&w.vm, &w.world, HERO).expect("his states");
        let four = four_of(&palette, &hero, &states);
        let side = &hero.locomotion;
        let duration = |c: &Option<guac_ir::hero::SlotClip>| c.as_ref().map(|c| c.clip.duration);
        let joined = Loops {
            idle: four.0[0]
                .and_then(|s| palette.clip(s as u16))
                .map_or(1.0, |c| c.duration),
            walk: None,
            run: None,
            fall: duration(&side.falling),
        };
        let mut last: Option<Picked> = None;
        let mut stand_in: Option<(Locomotion, Locomotion)> = None;
        let (mut on, mut after) = (0u32, Vec::new());
        for k in 0..240u32 {
            if force && k == 60 {
                w.fall_back(format!("forced: {load}'s scripts ran on"));
            }
            w.bench.pads[0] = Pad {
                stick: guac_motion::scenarios::stick(1.0, 0.0),
                actions: [false; 4],
                connected: true,
            };
            w.step().unwrap();
            let speed = w.hero().gait_speed;
            match (&w.off, &mut stand_in) {
                (None, _) => {
                    last = Picked::read(&w.world, HERO, four, speed);
                    on += 1;
                }
                (Some(_), None) => {
                    // The TV's hand-over, the step his scripts went off.
                    let p = last.as_ref().expect("picks up to his fall-back");
                    let mut own = match w.walk.origin {
                        walkers::Origin::Scripts => walkers::stand_in(
                            &states,
                            &palette,
                            side.walk.as_ref().map(|c| (&c.clip, Some(c.slot))),
                            side.run.as_ref().map(|c| (&c.clip, Some(c.slot))),
                            joined,
                        )
                        .expect("his own stand-in"),
                        walkers::Origin::Measured => Locomotion::new(
                            Gaits::ERUPTOR,
                            Loops {
                                walk: duration(&side.walk),
                                run: duration(&side.run),
                                ..joined
                            },
                        ),
                    };
                    let h = p.handover();
                    own.take_over(h);
                    // Each gait the picks weight carries on where it played.
                    for (i, c) in [(1, Clip::Walk), (2, Clip::Run)] {
                        if p.weights()[i] > 0.0 {
                            let given = h.plays[i].expect("where it played");
                            assert_eq!(own.play(c), given, "{load}: {c:?}");
                        }
                    }
                    let ours = Locomotion::new(w.walk.gaits, joined);
                    let (from, picked) = (own.weights(), p.weights());
                    let frame = Frame::of(w.hero());
                    let before = own.weights();
                    own.step(w.dt, frame);
                    let mut theirs = ours;
                    theirs.step(w.dt, frame);
                    after.push((from, picked, before, own.weights(), theirs.weights(), speed));
                    stand_in = Some((own, theirs));
                }
                (Some(_), Some((own, theirs))) => {
                    let frame = Frame::of(w.hero());
                    let before = own.weights();
                    own.step(w.dt, frame);
                    theirs.step(w.dt, frame);
                    after.push((
                        before,
                        [0.0; 4],
                        before,
                        own.weights(),
                        theirs.weights(),
                        speed,
                    ));
                }
            }
        }
        let (own, theirs) = stand_in.expect("fallen back");
        let (from, picked, ..) = after[0];
        // No pop at the hand-over: over a set-off's time from it no weight
        // moves a step more than an eased set-off's steepest step (its
        // rate's peak, 1/(1 - EASE) over SET_OFF). Later the edge probe
        // that holds him at the walk's end moves the run's weight to the
        // walk in a frame, as the original's does (AG.58).
        let steepest = 1.0 / (1.0 - EASE) / SET_OFF * w.dt;
        let set_off = (SET_OFF / w.dt).ceil() as usize + 1;
        let jump = after[..set_off]
            .iter()
            .map(|a| {
                a.2.iter()
                    .zip(a.3)
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max);
        // Once a set-off's slide is through, never his idle at his top
        // speed (the edge probe holds him still at the walk's end, AG.36,
        // where it comes back in).
        let top = after.iter().map(|a| a.5).fold(0.0f32, f32::max);
        let at_top: Vec<_> = after[set_off..]
            .iter()
            .filter(|a| (a.5 - top).abs() < 1e-3)
            .collect();
        let idle_most = at_top.iter().map(|a| a.3[0]).fold(0.0f32, f32::max);
        println!(
            "{load}: {on} steps on his scripts, off {:?}; states {states:?}; picks' last {picked:?}, the stand-in from {from:?}; after {} steps: {:?} at {:.4} ft/s, rates walk {:.4} run {:.4} (gaits {:?}, loops {:?}); the most a weight moved a step over the {set_off} from the hand-over {jump:.6} (an eased set-off's {steepest:.6}); at his top {top:.4} ft/s on {} steps after a set-off: the idle at most {idle_most:.6}, his last there {:?}; AG.27's stand-in {:?}; load {:.2}",
            w.off.as_ref().map(|o| o.0),
            after.len(),
            own.weights(),
            after.last().unwrap().5,
            own.play(guac_motion::locomotion::Clip::Walk).rate,
            own.play(guac_motion::locomotion::Clip::Run).rate,
            own.gaits(),
            own.loops(),
            at_top.len(),
            at_top.last().map(|a| a.3),
            theirs.weights(),
            load1()
        );
        assert!(on > 0, "{load}: on his scripts first");
        assert!(at_top.len() > 60, "{load}: at his top speed a while");
        for (a, b) in from.iter().zip(picked) {
            assert!(
                (a - b).abs() < 1e-6,
                "{load}: {from:?} against the picks' {picked:?}"
            );
        }
        assert!(
            jump <= steepest + 1e-6,
            "{load}: a step's {jump} past {steepest}"
        );
        assert!(idle_most == 0.0, "{load}: his idle at a run: {idle_most}");
        let w4 = at_top.last().unwrap().3;
        assert!(
            (w4.iter().sum::<f32>() - 1.0).abs() < 1e-6,
            "{load}: {w4:?}"
        );
        assert!(
            [Clip::Walk, Clip::Run]
                .iter()
                .all(|&c| own.play(c).rate.is_finite()),
            "{load}"
        );
        if w.walk.origin == walkers::Origin::Scripts {
            let theirs_at_top = at_top.iter().map(|a| a.4[0]).fold(1.0f32, f32::min);
            assert_eq!(theirs_at_top, 1.0, "{load}: AG.27's draws his idle");
        }
    }
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}

/// A field of `v` as the VM holds it.
fn vm_field(vm: &Vm, v: Value, name: &str) -> Option<Value> {
    vm.field(v, name).map(|f| vm.value(f))
}

/// The first player's camera points as the portal manager keeps them
/// (`player data`'s `cam control data`): the intent point, the velocity
/// point and the camera target, each where its actor stands.
fn cam_points(w: &W) -> [Option<Vec3>; 3] {
    let at = |name: &str| -> Option<Vec3> {
        let pm = scripts::portal_manager_placement(&w.vm, &w.world)?;
        let players = w.vm.local(pm, "player data").map(|v| w.vm.value(v))?;
        let p0 = w.vm.value(*w.vm.set(players)?.first()?);
        let cc = vm_field(&w.vm, p0, "cam control data")?;
        match vm_field(&w.vm, cc, name)? {
            Value::Entity(h) => w.world.actor(h).map(|a| a.position),
            _ => None,
        }
    };
    [at("intent target"), at("velocity target"), at("cam target")]
}

/// Each camera the level defined, by id: where its follower (`cam infos`'
/// `camera`) stands.
fn cam_followers(w: &W) -> Vec<(i64, Vec3)> {
    let Some(g) = w.vm.program().global_named("camera control data") else {
        return Vec::new();
    };
    let Some(cc) = w.vm.global_peek(g).map(|v| w.vm.value(v)) else {
        return Vec::new();
    };
    let Some(infos) = vm_field(&w.vm, cc, "cam infos") else {
        return Vec::new();
    };
    let all = w.vm.set(infos).map(<[Value]>::to_vec).unwrap_or_default();
    all.into_iter()
        .filter_map(|e| {
            let e = w.vm.value(e);
            let id = vm_field(&w.vm, e, "cam def")
                .and_then(|d| vm_field(&w.vm, d, "id"))
                .and_then(|v| v.as_f32())? as i64;
            match vm_field(&w.vm, e, "camera")? {
                Value::Entity(h) => w.world.actor(h).map(|a| (id, a.position)),
                _ => None,
            }
        })
        .collect()
}

/// AG.211 headless on the dump: Ninjini falls back as the TV takes it (his
/// minion's stop at `Minion.ai` #842 in a run without `collisioninfo.object
/// collisions`, [`PLAY_842`], whose registry the next hero joins on too;
/// the view to the chain), walks off on
/// the controller, comes off the portal, and the next hero (`AG211_NEXT`,
/// Whirlwind by default, who walks on his scripts) comes in where he stood
/// and joins the run: the join's activation makes his camera target afresh
/// on his minion, the camera's frame never stopped, so the scripts take
/// the view back at their first write and it follows him on their camera,
/// walked back the way Ninjini came (the way Ninjini went ends at the drop
/// AG.36's edge probe held him at).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn a_hero_put_in_after_a_fall_back_has_the_scripts_view_again() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let next = std::env::var("AG211_NEXT").unwrap_or_else(|_| "000_AirDragon".to_owned());
    let mut w = joined_on_the_tv(&l, NINJINI, PLAY_842, 120, true, true);
    let walk = walk_off(&mut w, 240);
    let feet = w.hero().pivot;
    let chain_eye = w.drawn.expect("drawn").eye;
    println!(
        "{NINJINI}: stop {:?}, {:.2} m walked; the view {:?} (back at a join {}), the chain's eye {chain_eye:?} {:?} m from his feet, the scripts' {:?} m; his camera points {:?} m from him",
        walk.stop.as_ref().map(|s| s.0),
        walk.walked,
        w.world.viewport().source,
        w.world.view_back_at_a_join(),
        chain_eye.distance(feet),
        w.world.viewport().eye.distance(feet),
        cam_points(&w).map(|p| p.map(|p| p.distance(feet)))
    );
    let off_view = w.world.viewport().source;
    let off_until_join = w.world.view_back_at_a_join();
    w.swap_in(&l, &next).unwrap();
    let joined_feet = w.hero().pivot;
    let points = cam_points(&w).map(|p| p.map(|p| p.distance(joined_feet)));
    println!(
        "{next} joined where he stood ({:?} m off): the view {:?}, his camera points {points:?} m from his feet, the cameras' followers {:?}",
        joined_feet.distance(feet),
        w.world.viewport().source,
        cam_followers(&w)
            .iter()
            .map(|(id, p)| (*id, p.distance(joined_feet)))
            .collect::<Vec<_>>()
    );
    let joined_view = w.world.viewport().source;
    // His palette's animation info on his minion's template, as the TV's
    // join sets it (AH.49: `set_palette_of`).
    let template = l
        .walkers
        .iter()
        .find(|x| x.load_file == next)
        .and_then(|x| x.template)
        .expect("his minion's template");
    let palette: Palette = read_ron(&l.dir, &IrFormat::Ron.palette_path(&next));
    let infos = scripts::clip_infos(&palette);
    let held: Vec<scripts::ClipInfo> = (0..infos.len() as u32)
        .map(|slot| w.world.clip_info(template, slot))
        .collect();
    let flagged = infos
        .iter()
        .filter(|i| **i != scripts::ClipInfo::default())
        .count();
    // The TV's camera, its chain the view's, is the same across the swap.
    let chain_kept = w.chain.is_some();
    let mut track = Vec::new();
    for k in 0..300u32 {
        w.bench.pads[0] = Pad {
            stick: if (60..180).contains(&k) {
                guac_motion::scenarios::stick(1.0, 180.0)
            } else {
                Stick::CENTRED
            },
            actions: [false; 4],
            connected: true,
        };
        let before = w.drawn.expect("drawn");
        w.step().unwrap();
        let p = w.hero().pivot;
        let d = w.drawn.expect("drawn");
        let v = *w.world.viewport();
        track.push((
            v.source,
            d.eye.distance(before.eye),
            d.eye.distance(p),
            d.eye.distance(v.eye),
            p,
            w.horizontal_walk(),
            w.off.is_some(),
        ));
        if k < 4 || k % 30 == 0 {
            println!(
                "  {k}: the view {:?}, the drawn eye {:?} m from his feet and {:?} m from the scripts', moved {:?} m; walk {:?} ft/s",
                v.source,
                d.eye.distance(p),
                d.eye.distance(v.eye),
                d.eye.distance(before.eye),
                w.horizontal_walk()
            );
        }
    }
    let first = &track[0];
    let walked_back = {
        let (a, b) = (track[59].4, track[179].4);
        Vec3::new(b.x - a.x, 0.0, b.z - a.z).length()
    };
    let eye_back = track[179].2;
    // His top beside what his description holds: in this harness a walker
    // put in after another reads the first's (AH.191; the TV's swap gives
    // Whirlwind his own 23 ft/s), so not compared here.
    let top = track[60..180].iter().map(|t| t.5).fold(0.0f32, f32::max);
    let ground = w.parameters().map(|p| p[0]);
    println!(
        "first step: the view {:?}, the drawn eye moved {:?} m from the chain's, {:?} m from his feet; walked back {walked_back:?} m at top {top:?} ft/s (his {ground:?}), the eye {eye_back:?} m from his feet at the end of it; every step the scripts' {}; {:.1?} in all, load {:.2}",
        first.0,
        first.1,
        first.2,
        track
            .iter()
            .all(|t| t.0 == guacamole::viewport::Source::Scripts && t.3 < 1e-6),
        started.elapsed(),
        load1()
    );
    // His fall-back hands the view back until a join; the next hero's join
    // gives it back before his first step (`ScriptWorld::hero_template`),
    // his camera points made afresh on his minion.
    use guacamole::viewport::Source;
    assert_eq!((off_view, off_until_join), (Source::HandedBack, true));
    assert!(chain_kept, "the chain the view's across the take-off");
    assert_eq!(joined_view, Source::Engine);
    assert!(!w.world.view_back_at_a_join());
    assert!(flagged > 0, "{next}'s palette flags some clip");
    assert_eq!(
        held, infos,
        "{next}'s animation info, as the TV's join sets it"
    );
    assert_eq!(points, [Some(0.0); 3], "his camera points on his minion");
    // From his first step on the view is the scripts' and the TV draws it.
    assert!(
        track.iter().all(|t| t.0 == Source::Scripts && t.3 < 1e-6),
        "the scripts' view every step"
    );
    // Walked back the way Ninjini came, it follows him at their distance.
    let standing = track[59].2;
    assert!(walked_back > 10.0, "walked back {walked_back} m");
    assert!(
        (eye_back - standing).abs() < 0.3,
        "the eye {eye_back} m from his feet, {standing} standing"
    );
    let eye_walked = track[60..180].iter().map(|t| t.1).sum::<f32>();
    assert!(eye_walked > 10.0, "the eye moved {eye_walked} m with him");
    // From where (the fallback merge's tree, 2026-10-25, on his natural stop
    // there): their eye at the first write 0.263 m from where the chain's
    // stood, 16.750 m from his feet (the chain's 16.564 from Ninjini's).
    let (_, why) = walk.stop.as_ref().expect("his script stops in play");
    assert!(why.contains("Minion.ai #842"), "{why}");
    assert_eq!(walk.stop.as_ref().map(|s| s.0), Some(124));
    assert_eq!(
        first.1, 0.26322174,
        "the first step's move from the chain's eye"
    );
    assert_eq!(first.2, 16.750097, "the eye from his feet");
}

/// Tree Rex's live clips (each one his scripts play on him, in the order
/// started) matched to a record's by length: the record's i-th control is
/// ours of the same length, the n-th of its length for the n-th, as both
/// lists keep the order the clips started in.
fn matched_clips(w: &W, record: &serde_json::Value) -> Vec<(usize, Option<usize>)> {
    let ours: Vec<usize> = w
        .world
        .sequences()
        .iter()
        .enumerate()
        .filter(|(_, s)| s.actor == HERO && s.plays())
        .map(|(i, _)| i)
        .collect();
    let mut used = HashSet::new();
    record["clips"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let period = json_f32(&c["period"]);
            let found = ours.iter().copied().find(|&s| {
                !used.contains(&s) && (w.world.sequences()[s].length - period).abs() < 1e-3
            });
            if let Some(s) = found {
                used.insert(s);
            }
            (i, found)
        })
        .collect()
}

/// AG.161 Verify: Tree Rex's locomotion picked on the VM (`global.bld`'s
/// locomotion script every tick, AH.49) against AG.12's recordings, record
/// by record: each of the 12 rings walked on his own scripts at the
/// records' own frame times from his settled spawn (as
/// [`tree_rex_walks_on_ag12s_numbers`] walks them), his clips' times and
/// rates set to the first record's, then at every step each control the
/// next record holds (his head turn, idle, walk, run and two leans) against
/// ours of its length: its weight within 0.1 %, its rate within 0.1 % where
/// it weighs anything.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.12's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn tree_rexs_clips_on_his_scripts_against_ag12s_recordings() {
    println!("load before: {:.2}", load1());
    let started = Instant::now();
    let l = Level::open();
    let (mut records_n, mut within, mut rates, mut rates_within) = (0, 0, 0, 0);
    let mut worst = (0.0f32, String::new());
    let mut missed: BTreeMap<String, usize> = BTreeMap::new();
    for (label, records) in &ag12_walks() {
        if label.starts_with("t_south") {
            continue;
        }
        let mut w = W::joined(&l, TREE_REX, TREE_REX_CELLS, 120)
            .map_err(|b| {
                let (_, when, h) = *b;
                format!("{when}: {}", named(&h.to_string()))
            })
            .unwrap();
        // Each clip at the first record's time and rate.
        for (i, ours) in matched_clips(&w, &records[0]) {
            let c = &records[0]["clips"][i];
            if let Some(s) = ours {
                let q = &mut w.world.sequences_mut()[s];
                q.time = (json_f32(&c["period"]) - json_f32(&c["remaining"])).clamp(0.0, q.length);
                q.rate = json_f32(&c["rate"]);
            }
        }
        let (mut ring_records, mut ring_within) = (0, 0);
        for k in 0..records.len() - 1 {
            let r = &records[k];
            let pr = json_f32(&r["clock"]["presentation_rate"]);
            w.dt = json_f32(&r["clock"]["frame_delta"]).clamp(0.5 * pr, 3.0 * pr);
            w.bench.view = json_f32(&r["camera"]["rotation"][1]);
            w.bench.pads[0] = record_pad(r);
            w.step().unwrap_or_else(|h| panic!("{label} step {k}: {h}"));
            let next = &records[k + 1];
            let mut miss: f32 = 0.0;
            for (i, ours) in matched_clips(&w, next) {
                let c = &next["clips"][i];
                let (weight, rate) = (json_f32(&c["weight"]), json_f32(&c["rate"]));
                let mine = ours.map(|s| {
                    let q = &w.world.sequences()[s];
                    (q.blend, q.rate)
                });
                let off = mine.map_or(f32::NAN, |m| (m.0 - weight).abs());
                // A control with no clip of ours stays a miss.
                miss = if miss.is_nan() || off.is_nan() {
                    f32::NAN
                } else {
                    miss.max(off)
                };
                if weight > 0.0 {
                    rates += 1;
                    if mine.is_some_and(|m| (m.1 - rate).abs() <= 0.1) {
                        rates_within += 1;
                    }
                }
            }
            records_n += 1;
            ring_records += 1;
            if miss <= 0.1 {
                within += 1;
                ring_within += 1;
            } else {
                *missed.entry(label.clone()).or_default() += 1;
            }
            if miss.is_nan() || miss > worst.0 {
                worst = (miss, format!("{label} record {}", k + 1));
            }
        }
        println!("{label}: {ring_within} of {ring_records} records' clips within 0.1 %");
    }
    println!(
        "Tree Rex's clips: {within} of {records_n} records within 0.1 % on every control (worst {:.4} %: {}); rates {rates_within} of {rates} within 0.1 %; past by ring {missed:?}; {:.1?}, load {:.2}",
        worst.0,
        worst.1,
        started.elapsed(),
        load1()
    );
    assert!(records_n > 4_000, "{records_n}");
    assert_eq!(within, records_n, "{missed:?}");
    assert_eq!(rates_within, rates);
}
