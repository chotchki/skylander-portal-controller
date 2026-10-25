//! AG.27's disc tests: every modelled character on his own scripts, headless
//! over level 213 with guacamole's host (the walkers' part as the TV links
//! it, `level_scripts::load` over a throwaway extract made by the real
//! stages), each joined by AH.17's join, his setup on the VM writing the
//! `movement parameters` and the scale the mover update reads; AH.165's
//! and AH.169's swaps of a walker; and AG.162's whitelist against the TV's
//! listing. Ignored by default; they read
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
use guacamole::level_scripts::{self, lengths};
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
#[derive(Debug, Clone, Copy)]
struct Opts {
    mode: Mode,
    discover: bool,
    cells: &'static [Cell],
}

const STRICT: Opts = Opts {
    mode: Mode::Strict,
    discover: false,
    cells: &[],
};
const DISCOVER: Opts = Opts {
    mode: Mode::Strict,
    discover: true,
    cells: &[],
};
/// The two members Tree Rex's walk reaches past the registry: his minion's
/// first tick writes AH.49's blend percent (`112_TitanLife.ai` #501), and
/// the mover's first step with the stick on asks his object collisions
/// (`Minion.ai` #842, where four others stop). With these two value cells
/// his setup and all 12 replays run through.
const TREE_REX_CELLS: Opts = Opts {
    mode: Mode::Strict,
    discover: false,
    cells: &[
        (
            "animationinfo",
            "blend percent",
            guac_script::Kind::Float,
            0.0,
            100.0,
        ),
        (
            "collisioninfo",
            "object collisions",
            guac_script::Kind::Reference,
            0.0,
            0.0,
        ),
    ],
};

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
        // His palette's lengths on his template, as the TV's join sets them.
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
            world.set_palette(template, lengths(&palette));
        }
        // The level's placements' palettes by their models.
        for p in l.files.placed.iter().filter(|p| p.obj.unit == l.level_unit) {
            if let Some(model) = &p.model
                && let Some(pal) = l.palettes.palettes.iter().find(|x| &x.owner == model)
            {
                world.set_palette(p.obj, lengths(pal));
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
        }
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
        let joined = self.minion.is_some();
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
        for hero in self.bench.heroes.values_mut() {
            hero.step_engine(&self.bench.world, dt, &[HERO_BODY], Surfaces::NONE);
        }
        self.world.advance(dt, &mut self.bench);
        self.steps += 1;
        self.bench.place_bodies(&self.world);
        Ok(())
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

    /// His figure off the portal mid-level as the TV's swap takes it
    /// (openspec AH.105, AH.165): the deactivate path, whatever stops in it.
    fn deactivate(&mut self) -> scripts::Deactivated {
        let files = self.files.clone();
        let unit = self.unit;
        let gone =
            self.with_host(|vm, host| scripts::join_deactivate(vm, host, &files, unit, HERO));
        self.minion = None;
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
/// replays against AG.12's O5 recording, with the two members his scripts
/// reach past the registry value cells ([`TREE_REX_CELLS`]).
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
                "the two members TREE_REX_CELLS names as value cells"
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
    out
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
/// after every one; (3) Eruptor taken off at points through each of his
/// three attacks, strict: whether his deactivation stops anywhere.
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
    let play = Opts {
        mode: Mode::Play,
        discover: false,
        cells: &[],
    };
    let tv: Vec<&WalkerScripts> = l
        .walkers
        .iter()
        .filter(|w| w.asks == 0 && w.minion.is_some())
        .collect();
    let mut table = Vec::new();
    for w in &tv {
        let t = Instant::now();
        let s = swap(&l, &w.load_file, play, 60, 60);
        println!(
            "{}: set up {}, {} stops before ({:?}); running {}, off {} placements and {} clones; deactivation {}; pm stopped {}, ticks after {:.2}; old silent {}, once {}, set up again {}, top again {:.3} of {:?} ({:.1?}, load {:.2})",
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
/// ticking on and his file running once after every one, whatever stopped.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP, writes the extract under GUAC_EXTRACT_DIR; run on the Mac"]
fn the_asking_walkers_come_off_whatever_their_deactivation_does() {
    let started = Instant::now();
    println!("load before: {:.2}", load1());
    let l = Level::open();
    let play = Opts {
        mode: Mode::Play,
        discover: false,
        cells: &[],
    };
    let asking: Vec<&WalkerScripts> = l
        .walkers
        .iter()
        .filter(|w| ASKING.contains(&w.load_file.as_str()) && w.minion.is_some())
        .collect();
    let mut table = Vec::new();
    for w in &asking {
        let s = swap(&l, &w.load_file, play, 60, 60);
        println!(
            "{} (asks {}): set up {}, {} stops before ({:?}); running {}, off {} placements and {} clones; deactivation {}; pm stopped {}, ticks after {:.2}; old silent {}, once {}, set up again {}, short {:?}",
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
    // Tree Rex with his setup run through (the parameters probe's two
    // cells), strict, standing and walking 120 steps each side of the swap.
    let rex = swap(&l, TREE_REX, TREE_REX_CELLS, 120, 120);
    println!(
        "{TREE_REX} on his two cells, strict: {rex:?}; {:.1?}, load {:.2}",
        started.elapsed(),
        load1()
    );
    assert_eq!(table.len(), ASKING.len(), "the fourteen");
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
    /// him, metres.
    top: f32,
    walked: f32,
    /// His description's ground max speed as his scripts wrote it.
    ground_max: Option<f32>,
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
                "joins; his script stops in play ({why}) and he walks on what it set ({:?} ft/s)",
                self.ground_max
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
    out
}

/// AH.150's headless check: each of the fourteen's asks of the gameplay
/// viewport read for what it gates (`on_screen::READ`, none left unread),
/// every walker listed as the TV lists them with no ask let through, and
/// each of the fourteen joined in play mode as the TV joins him: who walks
/// on his scripts, who falls back on what they set, who keeps his card and
/// why, the why never the ask. Then Drill Sergeant and Tree Rex at their
/// scripts' top speed: Drill Sergeant strict, Tree Rex strict on his two
/// value cells (`TREE_REX_CELLS`; his setup stops on AH.49's
/// `animationinfo.blend percent` without them, AG.160).
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
    let play = Opts {
        mode: Mode::Play,
        discover: false,
        cells: &[],
    };
    let mut table = Vec::new();
    for load in ASKING {
        let t = Instant::now();
        let r = on_the_tv(&l, load, play);
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
        let why = r.verdict();
        assert!(!why.contains("viewport"), "{}: {why}", r.load);
        if r.set_up && r.stopped.is_none() {
            let top = r.ground_max.expect("his movement");
            assert!((r.top - top).abs() < 1e-3, "{}: {why}", r.load);
        }
    }
    let drill = table.iter().find(|r| r.load == "021_DrillBot").unwrap();
    assert!(
        drill.set_up && drill.stopped.is_none(),
        "{}",
        drill.verdict()
    );
    // Strict: Drill Sergeant on the registry, Tree Rex on his two cells.
    for (load, o) in [("021_DrillBot", STRICT), (TREE_REX, TREE_REX_CELLS)] {
        let t = Instant::now();
        let a = audit(&l, load, o);
        println!(
            "{load} strict{}: {} [{}], top {:.4} ft/s, walked {:.2} m, params {:?} ({:.1?}, load {:.2})",
            if o.cells.is_empty() {
                ""
            } else {
                " on his two cells"
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
    }
    println!("{:.1?} in all, load {:.2}", started.elapsed(), load1());
}
