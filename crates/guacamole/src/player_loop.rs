//! The player loop (openspec AH.20, design decisions 2 and 4): the portal
//! manager's tick as guacamole runs it, a stand-in AJ deletes once the
//! portal manager's script runs whole. Its stretches are the portal
//! manager's own ops, run as its tick reaches them ([`Vm::run_span`]); only
//! which stretches run, for whom and when is ours.
//!
//! Once a step, ahead of every placement's tick:
//!
//! - the head: its `script timer local` set from its clock (the state
//!   system's immunity after a hurt reaction and a dropped coin's age read
//!   it);
//! - once a hero has joined: the death sequence for a defeated hero, the
//!   hurt list's upkeep (every 0.25 s the hurts older than the last check
//!   dropped), the combat manager (its shared alert every 0.25 s of game
//!   time) and the projectile manager;
//! - then for each player playing while nothing pauses the game, the
//!   stretch from `XP Balls - Collect XP Balls` to the health kept on the
//!   figure: orbs in reach taken and flying in, each paying its worth
//!   through `Game - Award Experience` as it arrives, the loot check every
//!   0.25 s of game time (`Loot - Player Check For Loot`), the coins taken
//!   sliding in and paying through `Game - Award Money`
//!   (`Loot - Player Collecting Loot`), and the hero's health written to
//!   his figure's stored data when it changes;
//! - the tail: `script timer last frame` set, then the clock a tick on;
//! - once a hero has joined, the game systems' tick's orbs (`Xp Balls - Xp
//!   Ball Control`): the orbs popping out two a frame, coming down and
//!   bouncing. The rest of that tick (its update macros, which run the
//!   encounter's camera pans, and the playtime) isn't run.
//!
//! Not run: the HUD's messages and its low-health warning (the HUD is
//! AH.26's stand-in), the join prompt and a player's join, the champion
//! details and the two-player camera, each player's update macros, and the
//! level-init gate: `Game - Init Minions in Level` raises `Level Init
//! State` in the original's level load, which no box runs, so the loop runs
//! from the level's start as if it had.
//!
//! The stretches are found by what they call ([`PlayerLoop::read`]), each
//! checked; a portal manager without one (a synthetic part's) runs without
//! it.

use std::ops::Range;

use guac_script::{Elem, Halt, Ins, Operand, Program, ScriptId, Value, Vm};
use tracing::warn;

use crate::scripts::{WorldHost, portal_manager, running};

/// The source path the portal manager's script ends with, as linked.
const SCRIPT: &str = "Spyro_PortalManager.ai";
/// The game systems' script, whose tick moves the orbs.
const GAME_SYSTEMS: &str = "GameSystems.ai";

/// The portal manager's tick as the loop runs it: each stretch's positions
/// in its script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerLoop {
    /// None when the program has no portal manager: the loop runs nothing.
    pub script: Option<ScriptId>,
    /// `if portal control data.$players dying ...`: the defeated heroes'
    /// sequence.
    pub death: Option<Range<u32>>,
    /// `if system.script timer > $hurt maintain tick ...`: the hurts kept.
    pub hurts: Option<Range<u32>>,
    /// `Combat - Manage Combat`.
    pub combat: Option<Range<u32>>,
    /// `Manage Projectiles`.
    pub projectiles: Option<Range<u32>>,
    /// Per player: orbs, the award, the loot check, coins and health.
    pub player: Option<Range<u32>>,
    /// `set $script timer last frame = system.script timer`.
    pub last_frame: Option<Range<u32>>,
    /// The game systems' script and its tick's `Xp Balls - Xp Ball
    /// Control`: the orbs popping out, coming down and bouncing.
    pub orbs: Option<(ScriptId, Range<u32>)>,
}

/// The macro a `use macro` op names, by its display name.
pub fn called(program: &Program, op: u32) -> Option<&str> {
    let Ins::UseMacro { np: Some(np), .. } = program.ops.get(op as usize)? else {
        return None;
    };
    let Operand::Path { elems, .. } = program.operands.get(*np as usize)? else {
        return None;
    };
    let def = match elems.first()? {
        Elem::Op(o) => match program.ops.get(*o as usize)? {
            Ins::Define(d) => *d,
            _ => return None,
        },
        Elem::Object(obj) => program.def_of(*obj)?,
        _ => return None,
    };
    let def = program.resolve(def).unwrap_or(def);
    Some(program.defs.get(def as usize)?.display.as_str())
}

/// The end of the statement at `p` of `ops`: its op and block, and with an
/// `else` closing that block, the else's block too.
fn statement_end(program: &Program, ops: &[u32], p: u32) -> u32 {
    let len = ops.len() as u32;
    let mut end = (p + 1 + program.branch(ops[p as usize])).min(len);
    while end > p + 1 {
        let last = ops[(end - 1) as usize];
        if program.ops[last as usize] == Ins::Skip && program.branch(last) > 0 {
            end = (end + program.branch(last)).min(len);
        } else {
            break;
        }
    }
    end
}

/// The statements of PRESCRIPT's own ops after its sections, each as its
/// positions: a tick's body.
fn tick_body(program: &Program, ops: &[u32]) -> Vec<Range<u32>> {
    let Some(&pre) = ops.first() else {
        return Vec::new();
    };
    if program.ops[pre as usize] != Ins::PreScript {
        return Vec::new();
    }
    let end = (1 + program.branch(pre)).min(ops.len() as u32);
    let mut out = Vec::new();
    let mut p = 1;
    while p < end {
        let op = ops[p as usize];
        let next = statement_end(program, ops, p).max(p + 1);
        let section = matches!(
            program.ops[op as usize],
            Ins::StartUp | Ins::ShutDown | Ins::Section | Ins::Continue
        );
        if !section {
            out.push(p..next.min(end));
        }
        p = next;
    }
    out
}

fn calls_in(program: &Program, ops: &[u32], r: &Range<u32>, name: &str) -> Option<u32> {
    r.clone()
        .find(|&q| called(program, ops[q as usize]) == Some(name))
}

impl PlayerLoop {
    /// The loop over `program`'s portal manager: each stretch found by what
    /// it calls and checked, or left out (logged) when the script lacks it.
    pub fn read(program: &Program) -> Self {
        let mut out = PlayerLoop::default();
        if let Some(gs) = program.script_named(GAME_SYSTEMS) {
            let ops = &program.scripts[gs as usize].ops;
            out.orbs = tick_body(program, ops)
                .into_iter()
                .find(|r| {
                    called(program, ops[r.start as usize]) == Some("Xp Balls - Xp Ball Control")
                })
                .map(|r| (gs, r));
        }
        let Some(script) = program.script_named(SCRIPT) else {
            return out;
        };
        out.script = Some(script);
        let ops = &program.scripts[script as usize].ops;
        let body = tick_body(program, ops);
        let is = |r: &Range<u32>, name: &str| -> bool {
            called(program, ops[r.start as usize]) == Some(name)
        };
        let check = |r: &Range<u32>| {
            matches!(
                program.ops[ops[r.start as usize] as usize],
                Ins::CheckValue { .. }
            )
        };
        let death = body.iter().position(|r| {
            check(r)
                && calls_in(program, ops, r, "Portal - Finalize Minion Defeat Sequence").is_some()
        });
        if let Some(i) = death {
            out.death = Some(body[i].clone());
            // The hurts' upkeep follows it: a check.
            if let Some(r) = body.get(i + 1)
                && check(r)
            {
                out.hurts = Some(r.clone());
            }
        }
        out.combat = body
            .iter()
            .find(|r| is(r, "Combat - Manage Combat"))
            .cloned();
        out.projectiles = body.iter().find(|r| is(r, "Manage Projectiles")).cloned();
        let players = body.iter().position(|r| {
            matches!(
                program.ops[ops[r.start as usize] as usize],
                Ins::LoopValue { .. }
            ) && calls_in(program, ops, r, "XP Balls - Collect XP Balls").is_some()
        });
        if let Some(i) = players {
            let r = &body[i];
            let collect = calls_in(program, ops, r, "XP Balls - Collect XP Balls");
            let collecting = calls_in(program, ops, r, "Loot - Player Collecting Loot");
            if let (Some(from), Some(c)) = (collect, collecting) {
                let health = statement_end(program, ops, c);
                if health < r.end
                    && matches!(
                        program.ops[ops[health as usize] as usize],
                        Ins::CheckValue { .. }
                    )
                {
                    out.player = Some(from..statement_end(program, ops, health));
                }
            }
            if let Some(next) = body.get(i + 1)
                && next.end - next.start == 1
                && matches!(
                    program.ops[ops[next.start as usize] as usize],
                    Ins::SetValue { .. }
                )
            {
                out.last_frame = Some(next.clone());
            }
        }
        for (name, found) in [
            ("the death sequence", &out.death),
            ("the hurts' upkeep", &out.hurts),
            ("the combat manager", &out.combat),
            ("the projectile manager", &out.projectiles),
            ("a player's orbs, award and loot", &out.player),
            ("the last frame's time", &out.last_frame),
        ] {
            if found.is_none() {
                warn!(
                    stretch = name,
                    "the portal manager's tick lacks it; the loop runs without it"
                );
            }
        }
        out
    }

    /// One step of the loop (the module docs), `joined` once a hero's
    /// scripts run. Nothing until the portal manager's own first tick has
    /// run (the VM ticks it then).
    pub fn step(
        &self,
        vm: &mut Vm,
        host: &mut WorldHost<'_, '_>,
        joined: bool,
    ) -> Result<(), Halt> {
        let pm = portal_manager(vm, host.world)?;
        if !vm.is_held(pm) {
            return Ok(());
        }
        let now = vm.active_seconds(pm).unwrap_or(0.0);
        // False only when no op of the program names the local: nothing
        // reads it then (a synthetic portal manager's).
        vm.set_local(pm, "script timer local", Value::Float(now));
        if joined {
            for r in [&self.death, &self.hurts, &self.combat, &self.projectiles]
                .into_iter()
                .flatten()
            {
                vm.run_span(pm, r.start, r.end, host)?;
            }
            if let Some(r) = &self.player {
                let paused = global_number(vm, "Pause Layer") > 0.0;
                let players = vm
                    .local(pm, "player data")
                    .map(|v| vm.value(v))
                    .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
                    .unwrap_or_default();
                for player in players {
                    let player = vm.value(player);
                    vm.set_local(pm, "player data scratch", player);
                    vm.set_field(player, "script timer", Value::Float(now));
                    let playing = vm
                        .field(player, "playing")
                        .and_then(|v| vm.value(v).as_f32())
                        .unwrap_or(0.0);
                    if playing != 0.0 && !paused {
                        vm.run_span(pm, r.start, r.end, host)?;
                    }
                }
            }
        }
        if let Some(r) = &self.last_frame {
            vm.run_span(pm, r.start, r.end, host)?;
        }
        vm.count_tick(pm);
        // The game systems' tick's orbs, after the portal manager's (it's
        // listed after it in `global.bld`), once a hero has joined; its
        // clock counted each step, as its own tick would.
        if let Some((_, r)) = &self.orbs
            && let Some(gs) = running(vm, host.world, GAME_SYSTEMS)
            && vm.is_held(gs)
        {
            if joined {
                vm.run_span(gs, r.start, r.end, host)?;
            }
            vm.count_tick(gs);
        }
        Ok(())
    }
}

/// A `global.bld` number, 0 when it reads none.
fn global_number(vm: &mut Vm, name: &str) -> f32 {
    vm.program()
        .global_named(name)
        .and_then(|g| vm.global(g))
        .and_then(|v| vm.value(v).as_f32())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph};
    use guac_script::{
        Config, Generator, Kind, MemberId, MemberSpec, Mode, Obj, PlacementId, Program, Role, Unit,
        Value, Vm, Whitelist,
    };

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{
        Files, PORTAL_MANAGER, Placed, ScriptWorld, headless_objects, headless_spawner,
    };

    const INT: &str = "IntMeasurement";
    const FLOAT: &str = "FloatMeasurement";
    const PM: Obj = Obj { unit: 0, index: 1 };
    const PM_HANDLE: u64 = 0x10;
    const PATH: &str =
        "C:/tfb/Content/Levels/Includes/Spyro_GenericStuff/scripts/Spyro_PortalManager.ai";
    /// The macros the synthetic tick calls, each counting its calls in a
    /// value cell of the same name.
    const CALLED: [(&str, &str); 6] = [
        ("Portal - Finalize Minion Defeat Sequence", "finalize"),
        ("Combat - Manage Combat", "combat"),
        ("Manage Projectiles", "projectiles"),
        ("XP Balls - Collect XP Balls", "collect"),
        ("Loot - Player Check For Loot", "check"),
        ("Loot - Player Collecting Loot", "collecting"),
    ];

    /// A portal manager in the original's shape, our ops: its STARTUP
    /// makes two players, the first playing; its tick's body sets its
    /// clock's local, ends at a gate that stays shut (the VM's own first
    /// tick stops there), then the death sequence's check (finalizing a
    /// dying player), the hurts' upkeep every 0.25 s, the combat and
    /// projectile managers, the per-player loop (orbs, the loot check every
    /// 0.25 s, loot collecting, health remembered) and the last frame's
    /// time.
    fn portal_manager() -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let player_ty = w.op(Class::DefineStructure, "Player Joined Data");
            let playing = w.variable("playing", "ValueInfo", Some(INT));
            let timer = w.variable("timer", "ValueInfo", Some(FLOAT));
            let script_timer = w.variable("script timer", "ValueInfo", Some(FLOAT));
            let hp = w.variable("hp", "ValueInfo", Some(INT));
            let hp_kept = w.variable("hp remembered", "ValueInfo", Some(INT));
            let players = w.variable("player data", "ScriptSet", None);
            w.refer(players, 0x24, player_ty);
            let scratch = w.variable("player data scratch", "ScriptReference", None);
            let dying = w.variable("dying", "ValueInfo", Some(INT));
            let gate = w.variable("gate", "ValueInfo", Some(INT));
            let tick = w.variable("hurt maintain tick", "ValueInfo", Some(FLOAT));
            let local = w.variable("script timer local", "ValueInfo", Some(FLOAT));
            let last = w.variable("script timer last frame", "ValueInfo", Some(FLOAT));
            let clock = || El::Member("system", "script timer");
            let defs: Vec<_> = CALLED
                .iter()
                .map(|(name, _)| w.op(Class::DefineMacro, name))
                .collect();
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            for _ in 0..2 {
                w.inc(&[El::Local(players), El::Member("setvariant", "count")]);
            }
            let first = w.reference(
                &[El::Local(players), El::Member("setvariant", "first")],
                None,
            );
            let lhs = w.value(&[El::Local(scratch)]);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, lhs);
            w.refer(o, 0x20, first);
            w.push(o);
            let one = w.int(1);
            w.set(&[El::Local(scratch), El::Local(playing)], one);
            let hundred = w.int(100);
            w.set(&[El::Local(scratch), El::Local(hp)], hundred);
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for (def, (_, cell)) in defs.iter().zip(CALLED) {
                let p = w.push(*def);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.inc(&[El::Member("test", cell)]);
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            let p = w.push(player_ty);
            for f in [playing, timer, script_timer, hp, hp_kept] {
                w.push(f);
            }
            w.end_block(p);
            w.end_block(types);
            // The tick's body.
            let now = w.rhs(FLOAT, &[clock()]);
            w.set(&[El::Local(local)], now);
            let shut_gate = w.int(1);
            let g = w.check(&[El::Local(gate)], 3, shut_gate);
            w.flow_end();
            w.close(g);
            let zero = w.int(0);
            let d = w.check(&[El::Local(dying)], 4, zero);
            w.call(defs[0], &[]);
            w.close(d);
            let upkeep = w.rhs(FLOAT, &[El::Local(tick)]);
            let h = w.check(&[clock()], 4, upkeep);
            let now = w.rhs(FLOAT, &[clock()]);
            let quarter = w.float(0.25);
            let later = w.expr(now, Some((0, quarter)));
            w.set(&[El::Local(tick)], later);
            w.inc(&[El::Member("test", "hurts")]);
            w.close(h);
            w.call(defs[1], &[]);
            w.call(defs[2], &[]);
            let two = w.int(2);
            let l = w.loop_value(two);
            w.call(defs[3], &[]);
            let now = w.rhs(FLOAT, &[clock()]);
            let c = w.check(&[El::Local(scratch), El::Local(timer)], 0, now);
            let now = w.rhs(FLOAT, &[clock()]);
            let quarter = w.float(0.25);
            let later = w.expr(now, Some((0, quarter)));
            w.set(&[El::Local(scratch), El::Local(timer)], later);
            w.call(defs[4], &[]);
            w.close(c);
            w.call(defs[5], &[]);
            let kept = w.rhs(INT, &[El::Local(scratch), El::Local(hp_kept)]);
            let k = w.check(&[El::Local(scratch), El::Local(hp)], 5, kept);
            let now_hp = w.rhs(INT, &[El::Local(scratch), El::Local(hp)]);
            w.set(&[El::Local(scratch), El::Local(hp_kept)], now_hp);
            w.close(k);
            w.end_loop(l);
            let now = w.rhs(FLOAT, &[clock()]);
            w.set(&[El::Local(last)], now);
            w.end_block(pre);
            w.end(PATH);
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// The synthetic portal manager running, its first tick taken: the
    /// VM, the host's world, the loop over it and its placement.
    struct Bench {
        vm: Vm,
        world: ScriptWorld,
        player_loop: PlayerLoop,
        pm: PlacementId,
        generator: Generator,
    }

    impl Bench {
        fn new() -> Self {
            let program = portal_manager();
            let player_loop = PlayerLoop::read(&program);
            let files = Files {
                placed: vec![Placed::named(PM, PORTAL_MANAGER, Some(PATH), 4)],
                ..Files::default()
            };
            let handles = HashMap::from([(PM, PM_HANDLE)]);
            let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            let mut config = Config::new(Mode::Strict);
            for (_, cell) in CALLED {
                config
                    .members
                    .value_cell("test", cell, MemberSpec::new(Kind::Int, 0.0, 0.0));
            }
            config
                .members
                .value_cell("test", "hurts", MemberSpec::new(Kind::Int, 0.0, 0.0));
            config.whitelist = Whitelist::with_first_ticks(&[], &[], &["Spyro_PortalManager.ai"]);
            let sid = program.script_named("Spyro_PortalManager.ai").unwrap();
            let mut vm = Vm::new(program, config);
            let pm = vm.add_at(PM_HANDLE, PORTAL_MANAGER, sid, PM);
            let mut bench = Bench {
                vm,
                world,
                player_loop,
                pm,
                generator: Generator::seeded(1),
            };
            bench.step(false);
            assert!(bench.vm.is_held(pm), "its first tick taken");
            bench
        }

        /// One step as the TV's: the slides, the loop, every tick.
        fn step(&mut self, joined: bool) {
            let mut engine = NoEngine;
            let mut spawn = headless_spawner(0x1000);
            let mut host = WorldHost {
                world: &mut self.world,
                engine: &mut engine,
                generator: &mut self.generator,
                spawn: &mut spawn,
            };
            self.vm.slide(&mut host).unwrap();
            self.player_loop
                .step(&mut self.vm, &mut host, joined)
                .unwrap_or_else(|h| panic!("{h}"));
            self.vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
        }

        fn count(&self, cell: &str) -> i32 {
            match self.vm.value_cell(None, MemberId::named("test", cell)) {
                Some(Value::Int(i)) => i,
                _ => 0,
            }
        }

        fn counts(&self) -> Vec<i32> {
            CALLED
                .iter()
                .map(|(_, c)| self.count(c))
                .chain([self.count("hurts")])
                .collect()
        }

        fn local(&self, name: &str) -> f32 {
            self.vm
                .local(self.pm, name)
                .and_then(|v| v.as_f32())
                .unwrap_or(f32::NAN)
        }

        fn player(&self, i: usize) -> Value {
            let set = self.vm.local(self.pm, "player data").unwrap();
            let set = self.vm.value(set);
            self.vm.value(self.vm.set(set).unwrap()[i])
        }
    }

    /// The stretches are found by what they call, the gate and the loop's
    /// own head left out.
    #[test]
    fn the_stretches_are_found_by_what_they_call() {
        let program = portal_manager();
        let l = PlayerLoop::read(&program);
        let ops = &program.scripts[l.script.unwrap() as usize].ops;
        let name =
            |r: &Option<Range<u32>>| called(&program, ops[r.clone().unwrap().start as usize]);
        assert_eq!(name(&l.combat), Some("Combat - Manage Combat"));
        assert_eq!(name(&l.projectiles), Some("Manage Projectiles"));
        assert_eq!(name(&l.player), Some("XP Balls - Collect XP Balls"));
        // death, then the hurts right after it, then the managers, each
        // whole and in the tick's order.
        let (d, h, c, p) = (
            l.death.clone().unwrap(),
            l.hurts.clone().unwrap(),
            l.combat.clone().unwrap(),
            l.projectiles.clone().unwrap(),
        );
        assert!(d.end == h.start && h.end == c.start && c.end == p.start);
        assert_eq!(l.last_frame.clone().unwrap().len(), 1);
        // A script without the stretches: none found, nothing to run.
        let empty = PlayerLoop::read(&Program::default());
        assert_eq!(empty, PlayerLoop::default());
    }

    /// The loop's schedule over game time: before a hero joins, the clock
    /// alone; once he has, each step the combat and projectile managers
    /// and, for each player playing, orb collection and loot collecting,
    /// every 0.25 s of game time the loot check and the hurts' upkeep; the
    /// death sequence once a player is dying; health remembered; a second
    /// player playing doubling the per-player calls.
    #[test]
    fn the_loops_schedule_over_game_time() {
        let mut b = Bench::new();
        let base = b.counts();
        assert_eq!(base, vec![0; 7], "the gate ended the VM's own first tick");
        for _ in 0..30 {
            b.step(false);
        }
        assert_eq!(b.counts(), base, "not joined: nothing but the clock");
        let clock = b.vm.active_seconds(b.pm).unwrap();
        assert!((b.local("script timer local") + 1.0 / 60.0 - clock).abs() < 1e-5);
        assert_eq!(
            b.local("script timer last frame"),
            b.local("script timer local")
        );
        for _ in 0..120 {
            b.step(true);
        }
        // finalize, combat, projectiles, collect, check, collecting, hurts
        assert_eq!(b.counts(), vec![0, 120, 120, 120, 8, 120, 8]);
        let clock = b.vm.active_seconds(b.pm).unwrap();
        assert!((b.local("script timer local") + 1.0 / 60.0 - clock).abs() < 1e-5);
        // The health its stretch keeps on the first player.
        let p0 = b.player(0);
        assert_eq!(
            b.vm.field(p0, "hp remembered").map(|v| b.vm.value(v)),
            Some(Value::Int(100))
        );
        // A dying player: the sequence runs each step.
        assert!(b.vm.set_local(b.pm, "dying", Value::Int(1)));
        b.step(true);
        assert_eq!(b.count("finalize"), 1);
        assert!(b.vm.set_local(b.pm, "dying", Value::Int(0)));
        // The second player playing too: twice the per-player calls, his
        // own loot check on his own timer.
        let p1 = b.player(1);
        assert!(b.vm.set_field(p1, "playing", Value::Int(1)));
        let before = b.counts();
        for _ in 0..60 {
            b.step(true);
        }
        let after = b.counts();
        let d: Vec<i32> = after.iter().zip(&before).map(|(a, b)| a - b).collect();
        assert_eq!(d, vec![0, 60, 60, 120, 8, 120, 4]);
    }
}
