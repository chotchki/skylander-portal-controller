//! The hero's swap on the TV (openspec AH.105; level-scene's Removing the
//! hero pauses the level, combat's A hero's health): his figure coming off
//! the portal mid-level takes his scripts off the run by the deactivate
//! path ([`guacamole::scripts::join_deactivate`]), the level's scripts
//! running on, and the next hero joins that run by AH.17's join, his
//! minion the activator, ticking first.
//!
//! - What goes: what the original's portal manager takes off when a figure
//!   leaves (its `Game - Activate/Deactivate Minion` with no minion, run as
//!   `Portal - Exit Object` calls it mid-level: his projectiles, his sets,
//!   his hat and camera target, his controller), then his character file's
//!   stream: each of its placements and each clone of its templates still
//!   on (his minion, his helpers, what his attacks left), his clips
//!   stopped. Its scheduling is ours (stand-in ledger: `join`).
//! - What stays: everything of the level's, a creature mid-behaviour with
//!   its timers and health, what its scripts took off (a breakable broken,
//!   a coin taken) and a clone of the level's in flight (a coin, debris, a
//!   thrown rock), which lands; and his figure's stored data in the portal
//!   manager, which brings him back with the health he left with (AH.11's
//!   `swap2`: 200 off and on, 200 of 290), a figure with another tag in
//!   full. Leaving the level installs the run again and clears it, as a
//!   level change clears the original's.
//!
//! It runs from his take-off ([`crate::play`]) as a one-shot system, while
//! his entity is still there for the engine to read (the enemies by where
//! he stood alerted), before the entity goes. A stop in it (a shutdown macro
//! of his on a member nothing models) is logged and the rest runs (openspec
//! AH.165): the portal manager ticks on and his stream is unloaded, so his
//! file runs once when his figure comes back. A walker kept on his card
//! (AG.27) comes off the same way.

use bevy::prelude::*;
use guac_script::Value;
use guacamole::scripts::{self, ScriptClone, ScriptRuntime, ScriptWorld, WorldHost};
use tracing::{info, warn};

use crate::hero::{FollowCamera, HeroBody};
use crate::hero_scripts::{HeroRun, Join, ScriptSource, TvEngine, standing_props, view_heading};
use crate::level::Level;
use crate::physics::AvianSeam;

/// The hero leaving the portal mid-level: his entity, still spawned, and
/// his run.
pub(crate) struct Leaving {
    pub(crate) hero: Entity,
    pub(crate) run: HeroRun,
}

/// The deactivate path for `leaving` (the module docs). Nothing of his is
/// in the run before his join's request: then it does nothing.
pub(crate) fn deactivate(
    In(leaving): In<Leaving>,
    mut commands: Commands,
    runtime: Option<ResMut<ScriptRuntime>>,
    world: Option<ResMut<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    seam: AvianSeam,
    cameras: Query<&FollowCamera>,
    mut heroes: Query<(Entity, &'static mut HeroBody)>,
    level: Res<Level>,
) {
    let (Some(mut runtime), Some(mut world), Some(source)) = (runtime, world, source) else {
        return;
    };
    let run = &leaving.run;
    if run.join == Join::Asked {
        info!(load_file = %run.load_file, "he left before his scripts joined: nothing of his in the run");
        return;
    }
    let props = standing_props(&level, &world, Some(&source));
    let view = view_heading(&cameras);
    let mut engine = TvEngine::new(&mut heroes, &seam, view, &props);
    let ScriptRuntime { vm, generator, .. } = &mut *runtime;
    let mut spawn = |template: Value| -> guac_script::Handle {
        commands.spawn(ScriptClone(template)).id().to_bits()
    };
    let gone = {
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        scripts::join_deactivate(
            vm,
            &mut host,
            source.files(),
            run.unit,
            leaving.hero.to_bits(),
        )
    };
    if let Some(h) = &gone.halted {
        warn!(%h, load_file = %run.load_file, "his deactivation stopped short; his stream is unloaded and the portal manager runs on");
    }
    info!(
        load_file = %run.load_file,
        minion = gone.minion,
        placements = gone.placements.len(),
        clones = gone.clones.len(),
        "his scripts off the run; the level's run on"
    );
    scripts::add_scripted(vm, &mut world);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use guac_formats::script::builder::{
        ATTACK_BALL, ATTACK_DEACTIVATIONS, ATTACK_HEALTH, ATTACK_HELPER, ATTACK_KEPT_HEALTH,
        SWAP_CREATURE, SWAP_HEALTH, SWAP_HEALTH_AWAKE, SWAP_ROCK, SWAP_THROW, WALKER_DEF,
        WALKER_LOAD, WALKER_SCALE, WALKER_SPEED, WALKER_TURN, WalkerFault,
    };
    use guac_motion::hero::Hero;
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios;
    use guac_motion::seam::PhysicsSeam;
    use guac_motion::stick::Stick;
    use guac_script::{Generator, Halt, Handle, Mode, Obj, PlacementId, Value, Vm, Whitelist};
    use guacamole::level_scripts;
    use guacamole::script_engine::{Engine, Pad};
    use guacamole::scripts::{
        self, Deactivated, Files, ScriptWorld, WorldHost, headless_handles, headless_objects,
    };

    use crate::test_support::hero_scripts::{LEVEL, progression_rows, write_swap, write_walker};

    /// The hero's handle, the one and the next alike: a headless run reuses
    /// it where the TV's next hero is an entity of his own.
    const HERO: Handle = 0x7000_0000;
    /// Where the creature stands: 5 m ahead of him along his facing.
    const CREATURE_AT: [f32; 3] = [0.0, 0.0, 5.0];
    /// The step the swap comes at: the creature's rock thrown 5 steps
    /// before (the creature ticks from step 1), his ball 2.
    const SWAP_AT: u32 = SWAP_THROW as u32 + 5;

    /// One hero on the parry twin's open ground.
    struct Bench {
        hero: Hero,
        world: ParryWorld,
    }

    impl Engine for Bench {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            (h == HERO).then_some(&mut self.hero)
        }
        fn heroes(&self) -> Vec<Handle> {
            vec![HERO]
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.world
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
    }

    /// AH.105's synthetic part linked as the TV links it, run headless in
    /// strict mode on the fight's listing (the creature by its placement's
    /// name) with its rock, stepped as the TV steps.
    struct Run {
        vm: Vm,
        world: ScriptWorld,
        bench: Bench,
        generator: Generator,
        files: Files,
        fighter: u16,
        level: u16,
        next: Handle,
        steps: u32,
        /// The player loop the TV walks with, for a walker's run: its native
        /// ticks count the portal manager's (none while it's stopped).
        player_loop: Option<guacamole::player_loop::PlayerLoop>,
        /// Whether his minion has activated (the loop's `joined`).
        joined: bool,
        /// The first pad's stick each step reads.
        stick: Stick,
    }

    impl Run {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            write_swap(tmp.path(), CREATURE_AT);
            let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
            let mut config = scripts::fight_config(Mode::Strict);
            if let Whitelist::Only { placements, .. } = &mut config.whitelist {
                placements.push(SWAP_ROCK.to_owned());
            }
            let fighter = linked
                .fighters
                .iter()
                .find(|(f, _)| f == "009_Eruptor")
                .unwrap()
                .1;
            Self::over(linked, config, fighter)
        }

        /// AG.27's synthetic walker's part, stopping as `fault` says, run
        /// as the TV lists it (every walker that asks nothing new), strict:
        /// the hero's unit his.
        fn walker(fault: WalkerFault) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            write_walker(tmp.path(), fault);
            let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
            let config =
                scripts::with_walkers(scripts::smash_config(Mode::Strict), &linked.walkers);
            let unit = linked
                .walkers
                .iter()
                .find(|w| w.load_file == WALKER_LOAD)
                .unwrap()
                .unit;
            let mut run = Self::over(linked, config, unit);
            run.player_loop = Some(guacamole::player_loop::PlayerLoop::read(run.vm.program()));
            run
        }

        /// The run over `linked` listed by `config`, the hero's file `unit`.
        fn over(linked: level_scripts::Linked, config: guac_script::Config, unit: u16) -> Self {
            let files = linked.placements;
            let handles = headless_handles(&files);
            let objects = headless_objects(&handles);
            let vm = scripts::init_chain(
                Arc::new(linked.program),
                &files,
                &handles,
                &linked.order,
                config,
            );
            let mut world = ScriptWorld::new(&files, &handles, objects);
            let template = files
                .placed
                .iter()
                .find(|p| p.name == "009_Eruptor")
                .unwrap()
                .obj;
            let mut lengths = vec![0.0; 40];
            lengths[20] = 0.5;
            lengths[32] = 2.0;
            world.set_palette(template, lengths);
            let ground = scenarios::bodies(&scenarios::open_ground(), &[]);
            let parry = ParryWorld::new(&ground).unwrap();
            let hero = scenarios::eruptor(&parry, bevy::math::Vec3::new(0.0, 1.0, 0.0), 0.0);
            Self {
                vm,
                world,
                bench: Bench { hero, world: parry },
                generator: Generator::seeded(5489),
                files,
                fighter: unit,
                level: *linked.order.last().unwrap(),
                next: objects[6].max(HERO) + 1,
                steps: 0,
                player_loop: None,
                joined: false,
                stick: Stick::CENTRED,
            }
        }

        fn host<T>(&mut self, f: impl FnOnce(&mut Vm, &mut WorldHost<'_, '_>) -> T) -> T {
            let mut spawn = scripts::headless_spawner(self.next);
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

        /// One step as the TV's: the activity rule, the pads, every tick,
        /// the clones added, his engine step, the engine's frame.
        fn step(&mut self, actions: [bool; 4]) {
            for (h, active) in self.world.activity(&mut self.bench) {
                self.vm.set_asleep(h, !active);
            }
            self.world.read_pads([
                Pad {
                    stick: self.stick,
                    actions,
                    connected: true,
                },
                Pad::default(),
            ]);
            let (player_loop, joined) = (self.player_loop.clone(), self.joined);
            let r = self.host(|vm, host| match &player_loop {
                Some(l) => l.frame(vm, host, joined, &mut Vec::new()),
                None => vm.step(host),
            });
            self.world.end_step();
            scripts::add_scripted(&mut self.vm, &mut self.world);
            r.unwrap_or_else(|h| panic!("{h}"));
            let dt = 1.0 / 60.0;
            self.bench
                .hero
                .step_engine(&self.bench.world, dt, &[], &Default::default());
            self.world.advance(dt, &mut self.bench);
            self.steps += 1;
        }

        /// AH.17's join as the TV makes it: his figure's tag on the portal
        /// (AH.23), his file's placements this step (it's stepped), his
        /// minion activated after it, before the next step.
        fn join(&mut self) -> PlacementId {
            self.try_join().unwrap_or_else(|h| panic!("{h}"))
        }

        /// [`Run::join`], a stop in the request or the activation returned
        /// as the TV's join gets it (the request's: no step after it).
        fn try_join(&mut self) -> Result<PlacementId, Halt> {
            let layout = guacamole::figure::Layout::of(&progression_rows()).unwrap();
            let tag = guacamole::figure::Tag {
                toy_id: 9,
                subtype: 0,
                serial: 1,
                values: guacamole::figure::Values::default(),
            };
            self.host(|vm, host| guacamole::figure::put_on_portal(vm, host, &layout, &tag))
                .unwrap_or_else(|h| panic!("{h}"));
            let files = self.files.clone();
            let unit = self.fighter;
            let handles = self.world.handles().clone();
            self.host(|vm, host| scripts::join_request(vm, host, &files, unit, &handles))?;
            self.step([false; 4]);
            let minion = self.host(|vm, host| scripts::join_activate(vm, host, HERO))?;
            self.joined = true;
            Ok(minion)
        }

        /// What of his file runs: each of its placements and each clone of
        /// its templates the VM holds and hasn't taken off.
        fn his_running(&self) -> Vec<Handle> {
            let unit = self.fighter;
            let placed = self
                .files
                .placed
                .iter()
                .filter(|p| p.obj.unit == unit)
                .filter_map(|p| self.world.handles().get(&p.obj).copied());
            let clones = self.world.clones().into_iter().filter(|&c| {
                self.world
                    .actor(c)
                    .is_some_and(|a| a.template.is_some_and(|t| t.unit == unit))
            });
            placed
                .chain(clones)
                .filter(|&h| {
                    self.vm
                        .placement_of(h)
                        .is_some_and(|id| !self.vm.removed(id))
                })
                .collect()
        }

        /// The deactivate path for him, run through.
        fn deactivate(&mut self) -> Deactivated {
            let gone = self.deactivate_as_it_goes();
            assert_eq!(gone.halted, None);
            gone
        }

        /// The deactivate path for him, whatever stops in it.
        fn deactivate_as_it_goes(&mut self) -> Deactivated {
            let files = self.files.clone();
            let unit = self.fighter;
            self.joined = false;
            self.host(|vm, host| scripts::join_deactivate(vm, host, &files, unit, HERO))
        }

        /// The portal manager's placement and its seconds of ticks (its
        /// native tick counts each step it runs; none once it's stopped).
        fn portal_manager(&self) -> (PlacementId, f32) {
            let pm = scripts::portal_manager_placement(&self.vm, &self.world).expect("it runs");
            (pm, self.vm.active_seconds(pm).unwrap())
        }

        /// A placement of a file by its name: its object and handle.
        fn placed(&self, unit: u16, name: &str) -> (Obj, Handle) {
            let p = self
                .files
                .placed
                .iter()
                .find(|p| p.name == name && p.obj.unit == unit)
                .unwrap_or_else(|| panic!("no {name}"));
            (p.obj, self.world.handles()[&p.obj])
        }

        /// The clones made so far whose template is named `name`.
        fn clones_named(&self, name: &str) -> Vec<Handle> {
            self.world
                .clones()
                .into_iter()
                .filter(|&c| self.world.name(c) == name)
                .collect()
        }

        /// The creature's behaviour, ticks, health and seconds of ticks.
        fn creature(&self) -> (Option<String>, Value, Value, f32) {
            let (_, h) = self.placed(self.level, SWAP_CREATURE);
            let id = self.vm.placement_of(h).expect("it runs");
            let local = |name: &str| {
                self.vm
                    .local(id, name)
                    .map_or(Value::Null, |v| self.vm.value(v))
            };
            (
                self.vm.behaviour(id).map(str::to_owned),
                local("ticks"),
                local("health"),
                self.vm.active_seconds(id).unwrap(),
            )
        }

        /// The first player's slot's `entered` (`Portal - Enter Object`'s
        /// and `Portal - Exit Object`'s mark).
        fn entered(&mut self) -> Option<Value> {
            let g = self.vm.program().global_named("portal control data")?;
            let pcd = self.vm.global(g)?;
            let slot = self
                .vm
                .field(pcd, "loaded objects")
                .and_then(|s| self.vm.set(s).and_then(|e| e.first().copied()))?;
            self.vm.field(slot, "entered").map(|v| self.vm.value(v))
        }

        /// A file number's value, by its name.
        fn file_int(&self, name: &str) -> Option<Value> {
            let g = self.vm.program().global_named(name)?;
            self.vm.global_peek(g)
        }
    }

    /// His run joined with the creature counting: the init chain's two
    /// steps, the join, then steps to `to` (the creature ticking from the
    /// first), his press of the first attack at `press`.
    fn joined_to(to: u32, press: Option<u32>) -> (Run, PlacementId) {
        let mut run = Run::new();
        run.step([false; 4]);
        run.step([false; 4]);
        let minion = run.join();
        while run.steps < to {
            let down = press == Some(run.steps);
            run.step([down, false, false, false]);
        }
        (run, minion)
    }

    /// A swap mid-behaviour (openspec AH.105, on the synthetic part): the
    /// creature, awake in `battle` with its rock in flight and his ball in
    /// flight, his figure off and an Eruptor on again (the deactivate path,
    /// then AH.17's join). Its behaviour, its count, its health and its
    /// seconds of ticks carry on through the join's step rather than
    /// starting over (its health 5, not its first 7); its rock flies on and
    /// lands, taken off by its own script on the floor; his minion, his
    /// helper, his ball and his file's placements are off the run (the
    /// synthetic portal manager's deactivation run once), a placement of
    /// his file running again under its handle once he's back.
    #[test]
    fn a_swap_keeps_the_levels_scripts_and_takes_his_off() {
        let (mut run, old) = joined_to(SWAP_AT, Some(SWAP_AT - 2));
        let rocks = run.clones_named(SWAP_ROCK);
        assert_eq!(rocks.len(), 1, "the rock thrown");
        let rock = rocks[0];
        assert!(run.world.actor(rock).unwrap().launched, "in flight");
        let balls = run.clones_named(ATTACK_BALL);
        assert_eq!(balls.len(), 1, "his ball thrown");
        let ball = balls[0];
        assert!(!run.world.removed(ball), "in flight");
        let helper = run.clones_named(ATTACK_HELPER);
        assert_eq!(helper.len(), 1);
        let (def_obj, def) = run.placed(run.fighter, "009_Eruptor_Def");
        let def_before = run.vm.placement_of(def).unwrap();
        let before = run.creature();
        assert_eq!(before.0.as_deref(), Some("battle"));
        assert_eq!(before.2, Value::Int(SWAP_HEALTH_AWAKE));
        assert_eq!(run.file_int(ATTACK_DEACTIVATIONS), None);

        assert_eq!(run.entered(), Some(Value::Int(1)));
        let gone = run.deactivate();
        assert!(gone.minion);
        assert_eq!(run.entered(), Some(Value::Int(0)), "his slot exited");
        assert!(
            run.world
                .sequences()
                .iter()
                .filter(|s| s.actor == HERO)
                .all(|s| s.stopped || s.done),
            "his clips stopped"
        );
        assert!(gone.clones.contains(&HERO) && gone.clones.contains(&ball));
        assert!(gone.clones.contains(&helper[0]));
        assert!(!gone.clones.contains(&rock), "the level's");
        assert!(gone.placements.contains(&def));
        assert!(run.vm.removed(old) && run.vm.removed(def_before));
        assert!(run.world.removed(ball) && run.world.removed(helper[0]));
        assert!(run.world.taken_off(def_obj));
        assert!(!run.world.removed(rock), "flying on");
        assert_eq!(
            run.file_int(ATTACK_DEACTIVATIONS),
            Some(Value::Int(1)),
            "the portal manager's deactivation"
        );
        let minion = run.join();
        assert_eq!(run.entered(), Some(Value::Int(1)), "entered again");
        let after = run.creature();
        assert_eq!(after.0, before.0, "its behaviour");
        let (Value::Int(b), Value::Int(a)) = (before.1, after.1) else {
            panic!("ticks: {:?} {:?}", before.1, after.1)
        };
        assert_eq!(a, b + 1, "its count on (the join's step)");
        assert_eq!(after.2, before.2, "its health");
        assert_ne!(after.2, Value::Int(SWAP_HEALTH), "not started over");
        assert!(
            (after.3 - before.3 - 1.0 / 60.0).abs() < 1e-5,
            "its timer on: {} -> {}",
            before.3,
            after.3
        );
        // His file again: its placement under the same handle, a new one.
        let def_again = run.vm.placement_of(def).unwrap();
        assert_ne!(def_again, def_before);
        assert!(!run.vm.removed(def_again) && !run.world.taken_off(def_obj));
        assert_ne!(minion, old);
        // The rock lands where it falls and its script takes it off.
        let mut landed = None;
        for _ in 0..60 {
            run.step([false; 4]);
            if run.world.removed(rock) {
                landed = Some(run.world.actor(rock).unwrap().position);
                break;
            }
        }
        let at = landed.expect("the rock landed");
        assert!(at.y < 0.6, "on the floor: {at}");
        assert!(at.z > CREATURE_AT[2] + 0.5, "ahead of the creature: {at}");
        assert_eq!(run.clones_named(ATTACK_BALL).len(), 1, "no throw since");
    }

    /// The next hero's minion ticks first (openspec AH.59, AH.105): after
    /// the swap his is the one activator, ahead of every level placement,
    /// the one that left neither ticking nor an activator.
    #[test]
    fn the_next_heros_minion_ticks_first() {
        let (mut run, old) = joined_to(SWAP_AT, None);
        assert_eq!(run.vm.tick_order()[0], (true, "009_Eruptor".to_owned()));
        run.deactivate();
        assert!(
            run.vm.tick_order().iter().all(|(a, _)| !a),
            "no activator while none is in"
        );
        let minion = run.join();
        let order = run.vm.tick_order();
        assert_eq!(order[0], (true, "009_Eruptor".to_owned()));
        assert_eq!(order.iter().filter(|(a, _)| *a).count(), 1);
        assert!(run.vm.is_activator(minion) && !run.vm.is_activator(old));
        assert!(run.vm.removed(old));
        // His minion's tick comes before the creature's: his walk of the
        // step, the creature's place in it after.
        let creature = order.iter().position(|(_, n)| n == SWAP_CREATURE).unwrap();
        assert!(creature > 0, "{order:?}");
    }

    /// A figure put back has the health it left with (openspec AH.105;
    /// combat's A hero's health): bitten to 77, a step for his minion to
    /// keep it (the synthetic file's keep, standing for the portal
    /// manager's stored data), off and on again: 77 from his new minion's
    /// first tick on. A fresh run (a level change) has nothing kept: full.
    #[test]
    fn a_figure_put_back_has_the_health_it_left_with() {
        let (mut run, old) = joined_to(10, None);
        assert_eq!(scripts::health(&run.vm, old), Some(ATTACK_HEALTH));
        assert!(scripts::set_health(&mut run.vm, old, 77));
        run.step([false; 4]);
        assert_eq!(run.file_int(ATTACK_KEPT_HEALTH), Some(Value::Int(77)));
        run.deactivate();
        let minion = run.join();
        run.step([false; 4]);
        assert_eq!(scripts::health(&run.vm, minion), Some(77), "as he left");
        for _ in 0..10 {
            run.step([false; 4]);
        }
        assert_eq!(scripts::health(&run.vm, minion), Some(77), "kept");
        let (fresh, minion) = joined_to(10, None);
        assert_eq!(
            scripts::health(&fresh.vm, minion),
            Some(ATTACK_HEALTH),
            "a level change: full"
        );
    }

    /// The walker's run joined: the init chain's two steps, the join, then
    /// `steps` steps.
    fn walker_joined(fault: WalkerFault, steps: u32) -> (Run, PlacementId) {
        let mut run = Run::walker(fault);
        run.step([false; 4]);
        run.step([false; 4]);
        let minion = run.join();
        for _ in 0..steps {
            run.step([false; 4]);
        }
        (run, minion)
    }

    /// His actor's scale, percent.
    fn scale_of(run: &Run) -> [f32; 3] {
        run.world.actor(HERO).expect("his actor").scale
    }

    /// AH.165 (found at the AG.27 merge check: nothing tested it): a walker
    /// joined, taken off and placed again into the same run. His file's
    /// placements come off and back on under their handles, each running
    /// once (the ones taken off tick no more); his new minion sets up from
    /// its own first tick (`set_up` false at the activation, true after
    /// it) and its setup sets his scale again on the fresh actor the
    /// activation spawns as him; the portal manager ticks on throughout.
    #[test]
    fn a_walker_taken_off_and_put_back_runs_his_file_once() {
        let mut run = Run::walker(WalkerFault::None);
        run.step([false; 4]);
        run.step([false; 4]);
        let first = run.join();
        assert!(!run.vm.set_up(first), "his minion hasn't ticked yet");
        run.step([false; 4]);
        assert!(run.vm.set_up(first));
        assert_eq!(scale_of(&run), [WALKER_SCALE; 3]);
        for _ in 0..5 {
            run.step([false; 4]);
        }
        let (def_obj, def) = run.placed(run.fighter, WALKER_DEF);
        let old_def = run.vm.placement_of(def).unwrap();
        let (pm, ticked) = run.portal_manager();
        let gone = run.deactivate();
        assert!(gone.minion && gone.placements.contains(&def));
        assert!(run.vm.removed(old_def) && run.vm.removed(first));
        assert!(run.world.taken_off(def_obj));
        assert_eq!(run.file_int(ATTACK_DEACTIVATIONS), Some(Value::Int(1)));
        assert!(run.vm.tick_order().iter().all(|(a, _)| !a), "no activator");
        assert_eq!(run.entered(), Some(Value::Int(0)));
        let second = run.join();
        assert_ne!(second, first);
        let again = run.vm.placement_of(def).unwrap();
        assert_ne!(again, old_def);
        assert!(!run.vm.removed(again) && !run.world.taken_off(def_obj));
        assert!(
            !run.vm.set_up(second),
            "a minion of his own, not set up yet"
        );
        assert_ne!(scale_of(&run), [WALKER_SCALE; 3], "a fresh actor as him");
        let (def_was, first_was) = (
            run.vm.active_seconds(old_def).unwrap(),
            run.vm.active_seconds(first).unwrap(),
        );
        for _ in 0..10 {
            run.step([false; 4]);
        }
        assert!(run.vm.set_up(second));
        assert_eq!(scale_of(&run), [WALKER_SCALE; 3], "his setup's scale again");
        assert_eq!(
            run.vm.active_seconds(old_def).unwrap(),
            def_was,
            "the definition taken off ticks no more"
        );
        assert_eq!(run.vm.active_seconds(first).unwrap(), first_was);
        assert!(run.vm.active_seconds(again).unwrap() > 0.0);
        let order = run.vm.tick_order();
        assert_eq!(order.iter().filter(|(a, _)| *a).count(), 1, "{order:?}");
        let (pm_again, now) = run.portal_manager();
        assert_eq!(pm_again, pm);
        assert!(
            now > ticked,
            "the portal manager ticks on: {ticked} -> {now}"
        );
        assert!(run.vm.stopped(pm).is_none());
    }

    /// AH.165: a walker whose deactivation stops (his shutdown, which the
    /// portal manager's deactivation runs, on a member nothing models) is
    /// still unloaded, and the portal manager ticks on: before, the stop
    /// returned before the unloading (his file's placements and minion left
    /// running, his slot entered) and stopped the portal manager, whose
    /// tick runs combat, hurts and the health write, for the rest of the
    /// level. Placed again, his file runs once.
    #[test]
    fn a_walker_whose_deactivation_stops_is_still_taken_off() {
        let (mut run, first) = walker_joined(WalkerFault::InDeactivation, 5);
        let (def_obj, def) = run.placed(run.fighter, WALKER_DEF);
        let old_def = run.vm.placement_of(def).unwrap();
        let (pm, ticked) = run.portal_manager();
        let gone = run.deactivate_as_it_goes();
        let halt = gone.halted.as_ref().expect("his shutdown stops");
        assert!(halt.to_string().contains("unmodelled member"), "{halt}");
        assert!(gone.minion && gone.placements.contains(&def) && gone.clones.contains(&HERO));
        assert!(run.vm.removed(old_def) && run.vm.removed(first));
        assert!(run.world.taken_off(def_obj) && run.world.removed(HERO));
        assert!(run.vm.tick_order().iter().all(|(a, _)| !a), "no activator");
        assert_eq!(run.entered(), Some(Value::Int(0)), "his slot exited");
        assert_eq!(run.file_int(ATTACK_DEACTIVATIONS), Some(Value::Int(1)));
        assert!(run.vm.stopped(pm).is_none(), "the portal manager runs on");
        let second = run.join();
        for _ in 0..10 {
            run.step([false; 4]);
        }
        let (_, now) = run.portal_manager();
        assert!(now > ticked, "it ticks: {ticked} -> {now}");
        let again = run.vm.placement_of(def).unwrap();
        assert_ne!(again, old_def);
        assert!(run.vm.set_up(second));
        assert_eq!(run.vm.tick_order().iter().filter(|(a, _)| *a).count(), 1);
    }

    /// AH.169 (the review of AH.165: the request's and the activation's
    /// calls in the portal manager's name were pinned by nothing, either
    /// back on `call_with` passing every test): a walker whose join stops in
    /// its request or its activation (a macro of his the portal manager's
    /// macro runs, on a member nothing models) gets the stop back, the
    /// portal manager not stopped; the step the TV runs before his take-off,
    /// then the deactivate path (as `keep_cards` takes him off) runs
    /// through, and after it nothing of his file runs, no activator is
    /// left, his slot is exited and the portal manager ticks on.
    #[test]
    fn a_walker_whose_join_stops_leaves_the_portal_manager_ticking() {
        for fault in [WalkerFault::InRequest, WalkerFault::InActivation] {
            let mut run = Run::walker(fault);
            run.step([false; 4]);
            run.step([false; 4]);
            let (pm, _) = run.portal_manager();
            let halt = run.try_join().expect_err("his macro stops");
            assert!(
                halt.to_string().contains("unmodelled member"),
                "{fault:?}: {halt}"
            );
            assert_eq!(halt.placement_name, scripts::PORTAL_MANAGER, "{fault:?}");
            let text = halt.to_string();
            assert!(
                run.vm.log().iter().any(|l| l.text == text),
                "{fault:?}: the stop logged"
            );
            assert!(run.vm.stopped(pm).is_none(), "{fault:?}: not stopped");
            run.step([false; 4]);
            let gone = run.deactivate_as_it_goes();
            assert_eq!(gone.halted, None, "{fault:?}");
            let (def_obj, _) = run.placed(run.fighter, WALKER_DEF);
            assert!(run.world.taken_off(def_obj), "{fault:?}");
            let running = run.his_running();
            assert!(
                running.is_empty(),
                "{fault:?}: nothing of his runs: {running:?}"
            );
            assert!(
                run.vm.tick_order().iter().all(|(a, _)| !a),
                "{fault:?}: no activator"
            );
            assert_eq!(run.entered(), Some(Value::Int(0)), "{fault:?}");
            let (_, ticked) = run.portal_manager();
            for _ in 0..10 {
                run.step([false; 4]);
            }
            let (pm_again, now) = run.portal_manager();
            assert_eq!(pm_again, pm);
            assert!(run.vm.stopped(pm).is_none(), "{fault:?}");
            assert!(
                now - ticked > 9.5 / 60.0,
                "{fault:?}: the portal manager ticks on: {ticked} -> {now}"
            );
            let running = run.his_running();
            assert!(running.is_empty(), "{fault:?}: nor after: {running:?}");
        }
    }
    /// AG.171 (found at AG.27's verify: Wham-Shell and Drill Sergeant set
    /// up and stood still): a parameter bound to `its` keeps its own
    /// variable, whatever a `find subset` left `its` on. The synthetic
    /// walker's mover walks him only once his lookup hands his description
    /// back first in the set it binds to `its`, the call coming right after
    /// a `find subset` over his creature info that reads its right side
    /// through nothing; read as that candidate, `its` put his creature info
    /// first in the set and he stood still, as the two did when
    /// `PortalControl.ai`'s legendary lookup left `its` on a toy table row
    /// before `Movement - Add Mover` looked up their joined data.
    #[test]
    fn a_walker_walks_through_a_find_subset_left_its_on_a_candidate() {
        let (mut run, minion) = walker_joined(WalkerFault::None, 1);
        assert!(run.vm.set_up(minion));
        let decoys = run
            .vm
            .local(minion, "decoys")
            .map(|v| run.vm.value(v))
            .and_then(|v| run.vm.set(v).map(<[Value]>::len));
        assert_eq!(decoys, Some(1), "the candidate `its` is left on");
        let from = run.bench.hero.pivot;
        let steps = 12;
        for _ in 0..steps {
            run.step([false; 4]);
        }
        let walk = run.bench.hero.walk.velocity;
        assert!(
            (walk.x.hypot(walk.z) - WALKER_SPEED).abs() < 1e-3,
            "his walk {walk:?}, his setup wrote {WALKER_SPEED} ft/s"
        );
        let moved = (run.bench.hero.pivot - from).length();
        let want = WALKER_SPEED * guac_motion::tuning::FOOT * steps as f32 / 60.0;
        assert!(
            (moved - want).abs() < 0.02 * want,
            "moved {moved} m in {steps} steps, the mover wrote {want}"
        );
    }
    /// AG.172 (found at AG.27's verify: Tree Rex turned the other way round
    /// from the oracle's at AG.12's 180° swings): a target straight behind
    /// turns him the positive way, either way round. The synthetic walker
    /// turns as `Movement - Face and Move` does, through
    /// `system.normalized angle`; held to the right until he faces it
    /// (-90°), the stick swung full left asks for 90°, 180° on (and from
    /// 90°, -90° asks for -180°): he turns up by his 720°/s x the stick
    /// over the tick both times. On the [-180, 180) angle he turned down.
    #[test]
    fn a_walker_straight_behind_turns_the_positive_way() {
        let (right, left) = (Stick::from_bytes(255, 128), Stick::from_bytes(0, 128));
        for (first, then) in [(right, left), (left, right)] {
            let (mut run, _) = walker_joined(WalkerFault::None, 1);
            run.stick = first;
            for _ in 0..30 {
                run.step([false; 4]);
            }
            let faced = first.push(0.0).heading;
            let before = run.bench.hero.facing;
            assert!(
                (before - faced).rem_euclid(360.0) < 1e-4,
                "he faces the first stick's {faced}°: {before}"
            );
            run.stick = then;
            run.step([false; 4]);
            let turned = (run.bench.hero.facing - before + 180.0).rem_euclid(360.0) - 180.0;
            let want = WALKER_TURN * then.push(0.0).strength / 60.0;
            assert!(
                (turned - want).abs() < 1e-3,
                "from {before}° toward {}°: turned {turned}°, want +{want}°",
                then.push(0.0).heading
            );
        }
    }
}
