//! The player loop (openspec AH.20, design decisions 2 and 4): the portal
//! manager's tick as guacamole runs it, which AJ deletes once the portal
//! manager's script runs whole (stand-in ledger: `player-loop`). Its
//! stretches are the portal manager's own ops, run as its tick reaches them
//! ([`Vm::run_span`]); only which stretches run, for whom and when is ours.
//!
//! Each step's walk ([`PlayerLoop::walk`], openspec AH.59, the original's
//! order as [`crate::frame`] has it) runs the loop's slices where their
//! placements sit in the scheduler's list: the game systems' tick at
//! `Macros_GameSystems`' place, the music controller's reset of the mover
//! update at `Macros_Music`'s (the mover update leaves the list and ticks
//! at its end once the walk is through, after every other awake placement),
//! and the portal manager's tick at `Spyro_PortalManager`'s, after the UI's,
//! whose tick moves the game timer. The portal manager's, once a step:
//!
//! - the gameplay camera's frame (openspec AG.28): `Camera - System -
//!   Update` over `camera control data`, the tick's first statement, from
//!   the level's start, run apart ([`Vm::run_span_apart`]): stopped in
//!   play, the TV's camera takes the view and the rest runs on;
//! - the head: its `script timer local` set from its clock (the state
//!   system's immunity after a hurt reaction and a dropped coin's age read
//!   it);
//! - the HUD's banners, each player's, as the tick's own loop steps them
//!   ahead of the level-init gate ([`crate::hud_messages`], openspec
//!   AH.222): a level-up's end raising his health to the new level's full
//!   health;
//! - once a hero has joined: the death sequence for a defeated hero, the
//!   hurt list's upkeep (every 0.25 s the hurts older than the last check
//!   dropped), the combat manager (its shared alert every 0.25 s of game
//!   time) and the projectile manager;
//! - then for each player playing, his camera target stepped after his
//!   last move (`Camera - Update Player Cam Target`, the statement ahead of
//!   his orbs, run apart as the camera's frame is), and while nothing
//!   pauses the game the stretch from `XP Balls - Collect XP Balls` to the
//!   health kept on the figure: orbs in reach taken and flying in, each
//!   paying its worth
//!   through `Game - Award Experience` as it arrives, the loot check every
//!   0.25 s of game time (`Loot - Player Check For Loot`), the coins taken
//!   sliding in and paying through `Game - Award Money`
//!   (`Loot - Player Collecting Loot`), and the hero's health written to
//!   his figure's stored data when it changes;
//! - the tail: `script timer last frame` set, then the clock a tick on.
//!
//! The game systems' ([`PlayerLoop::game_systems_tick`]), once a step: of its
//! update macros (`System Update Controls`' `update while unpaused macros`)
//! the ones [`UPDATES`] names, each once in the set's order (openspec AH.21:
//! the destructibles system waking a breakable in range as a target and
//! shutting a broken one down for its loot, the actor remover taking its
//! debris 1.5 s on); then, once a hero has joined, its orbs (`Xp Balls - Xp
//! Ball Control`): the orbs popping out two a frame, coming down and
//! bouncing. The rest of that tick (its other update macros, which run the
//! encounter's camera pans and the difficulty tracking, and the playtime)
//! isn't run.
//!
//! Not run: the HUD's banners' drawing (stepped by [`crate::hud_messages`];
//! stand-in ledger: `hud-messages`) and its low-health warning (the HUD is
//! AH.26's; stand-in ledger: `hud`), the join prompt and a player's join,
//! the champion details and the two-player camera, each player's update
//! macros, and the level-init gate: `Game - Init Minions in Level` raises
//! `Level Init State` in the original's level load, which no box runs, so
//! the loop runs from the level's start as if it had.
//!
//! The stretches are found by what they call ([`PlayerLoop::read`]), each
//! checked; a portal manager without one (a synthetic part's) runs without
//! it.

use std::ops::Range;

use guac_script::{
    Elem, Halt, Handle, Host, Ins, Mode, Operand, PlacementId, Program, ScriptId, StepReport,
    Value, Vm,
};
use tracing::warn;

use crate::scripts::{ScriptWorld, WorldHost, portal_manager, running};

/// The source path the portal manager's script ends with, as linked.
const SCRIPT: &str = "Spyro_PortalManager.ai";
/// The game systems' script, whose tick moves the orbs.
const GAME_SYSTEMS: &str = "GameSystems.ai";
/// The music controller's script, whose tick ends resetting the mover
/// update (openspec AH.59).
const MUSIC: &str = "MusicController.ai";
/// The mover update's script: every scripted mover's velocity handed to the
/// engine.
const MOVER: &str = "movementUpdate.ai";

/// Where the loop's slices sit in a step's walk (openspec AH.59): the
/// placements running the game systems', the music controller's and the
/// portal manager's scripts, and the mover update the music controller
/// resets; `None` where the program runs no such placement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Places {
    pub game_systems: Option<PlacementId>,
    pub music: Option<PlacementId>,
    pub portal_manager: Option<PlacementId>,
    pub mover: Option<Handle>,
}

impl Places {
    /// The places as `vm` runs `world`'s placements.
    pub fn of(vm: &Vm, world: &ScriptWorld) -> Self {
        let mover = running(vm, world, MOVER).and_then(|id| vm.handle_of(id));
        Places {
            game_systems: running(vm, world, GAME_SYSTEMS),
            music: running(vm, world, MUSIC),
            portal_manager: portal_manager(vm, world).ok(),
            mover,
        }
    }
}

/// The game systems' update macros the loop runs, by name (openspec AH.21):
/// the destructibles system's (a breakable woken in range made a target,
/// one broken shut down, the bounce and a delayed effect) and the actor
/// remover's (debris gone 1.5 s on). The set's other update macros (the
/// encounter's camera pans, the HUD's) stay off.
pub const UPDATES: [&str; 4] = [
    "Destructibles - System Update",
    "Destructibles - Bounce",
    "Destructibles - Delayed Particle",
    "Actor Remover Sytem Update Macro",
];

/// The portal manager's tick as the loop runs it: each stretch's positions
/// in its script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerLoop {
    /// None when the program has no portal manager: the loop runs nothing.
    pub script: Option<ScriptId>,
    /// `Camera - System - Update` over `camera control data`: the tick's
    /// first statement, ahead of its clock's local (openspec AG.28).
    pub camera: Option<Range<u32>>,
    /// Per player playing, ahead of his orbs: the statement around `Camera -
    /// Update Player Cam Target`, his camera target stepped after his last
    /// move (openspec AG.28).
    pub cam_target: Option<Range<u32>>,
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

/// `ops[range]` split at each statement's end ([`statement_end`]), the
/// last held to the range.
fn statements(program: &Program, ops: &[u32], range: Range<u32>) -> Vec<Range<u32>> {
    let mut out = Vec::new();
    let mut p = range.start;
    while p < range.end {
        let next = statement_end(program, ops, p).max(p + 1).min(range.end);
        out.push(p..next);
        p = next;
    }
    out
}

/// The blocks inside statement `s`: its op's block less an `else` closing
/// it, then that else's block, and so on down an else-if chain.
fn blocks(program: &Program, ops: &[u32], s: &Range<u32>) -> Vec<Range<u32>> {
    let mut out = Vec::new();
    let mut start = s.start + 1;
    let mut end = (start + program.branch(ops[s.start as usize])).min(s.end);
    loop {
        let last = end.checked_sub(1).map(|e| ops[e as usize]);
        match last {
            Some(op) if end > start && program.ops[op as usize] == Ins::Skip => {
                let skip = program.branch(op);
                out.push(start..end - 1);
                if skip == 0 {
                    return out;
                }
                start = end;
                end = (end + skip).min(s.end);
            }
            _ => {
                out.push(start..end);
                return out;
            }
        }
    }
}

/// Inside statement `r`, the outermost statement holding position `q` but
/// not `without`: a stretch nested in a block another stretch shares.
fn stretch_holding(
    program: &Program,
    ops: &[u32],
    r: &Range<u32>,
    q: u32,
    without: u32,
) -> Option<Range<u32>> {
    let mut within = blocks(program, ops, r);
    loop {
        let s = within
            .iter()
            .flat_map(|b| statements(program, ops, b.clone()))
            .find(|s| s.contains(&q))?;
        if !s.contains(&without) {
            return Some(s);
        }
        if s.start == q {
            return None;
        }
        within = blocks(program, ops, &s);
    }
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
        out.camera = body
            .iter()
            .find(|r| is(r, "Camera - System - Update"))
            .cloned();
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
            // His camera target, in the playing branch ahead of the orbs:
            // the statement that holds its call and not theirs.
            if let (Some(q), Some(from)) = (
                calls_in(program, ops, r, "Camera - Update Player Cam Target"),
                collect,
            ) && q < from
            {
                out.cam_target = stretch_holding(program, ops, r, q, from);
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
            ("the camera's frame", &out.camera),
            ("a player's camera target", &out.cam_target),
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

    /// One step's walk (the module docs; `guac_script`'s crate docs, The
    /// tick): every placement's tick in the scheduler's order, the loop's
    /// slices in their placements' places, `joined` once a hero's scripts
    /// run. In play mode a slice that stops is logged into `stopped` and
    /// the walk goes on, as a placement's tick that stops; in strict mode it
    /// ends the step. The frame's slides are the caller's, first.
    pub fn walk(
        &self,
        vm: &mut Vm,
        host: &mut WorldHost<'_, '_>,
        joined: bool,
        stopped: &mut Vec<Halt>,
    ) -> Result<StepReport, Halt> {
        self.walk_with(vm, host, joined, stopped, &mut |_, _, _| Ok(()))
    }

    /// [`PlayerLoop::walk`], with `then` run after the loop's slice in each
    /// held placement's place: a test's own call where a placement of the
    /// original's order would make it (a bite at the portal manager's
    /// place: after the hero's tick and the UI's, before the mover update).
    pub fn walk_with(
        &self,
        vm: &mut Vm,
        host: &mut WorldHost<'_, '_>,
        joined: bool,
        stopped: &mut Vec<Halt>,
        then: &mut guac_script::Native<'_>,
    ) -> Result<StepReport, Halt> {
        let places = Places::of(vm, host.world);
        let play = vm.config().mode == Mode::Play;
        let mut native = |vm: &mut Vm, id: PlacementId, host: &mut dyn Host| {
            let ran = self
                .native(vm, host, &places, id, joined)
                .and_then(|()| then(vm, id, host));
            match ran {
                Err(h) if play => {
                    stopped.push(h);
                    Ok(())
                }
                other => other,
            }
        };
        vm.step_with(host, &mut native)
    }

    /// The scripts' part of a step as a headless run takes it: the frame's
    /// slides, then [`PlayerLoop::walk`]. The TV's puts the join between
    /// the two.
    pub fn frame(
        &self,
        vm: &mut Vm,
        host: &mut WorldHost<'_, '_>,
        joined: bool,
        stopped: &mut Vec<Halt>,
    ) -> Result<StepReport, Halt> {
        self.frame_with(vm, host, joined, stopped, &mut |_, _, _| Ok(()))
    }

    /// [`PlayerLoop::frame`] walking with [`PlayerLoop::walk_with`]'s `then`.
    pub fn frame_with(
        &self,
        vm: &mut Vm,
        host: &mut WorldHost<'_, '_>,
        joined: bool,
        stopped: &mut Vec<Halt>,
        then: &mut guac_script::Native<'_>,
    ) -> Result<StepReport, Halt> {
        let slid = vm.slide(host)?;
        stopped.extend(slid.stopped);
        self.walk_with(vm, host, joined, stopped, then)
    }

    /// What the walk runs in held placement `id`'s place: the game systems'
    /// tick, the music controller's reset of the mover update (it rejoins
    /// the awake list's end once the walk is through) or the portal
    /// manager's tick; nothing for any other held placement.
    pub fn native(
        &self,
        vm: &mut Vm,
        host: &mut dyn Host,
        places: &Places,
        id: PlacementId,
        joined: bool,
    ) -> Result<(), Halt> {
        if places.game_systems == Some(id) {
            self.game_systems_tick(vm, host, id, joined)
        } else if places.music == Some(id) {
            if let Some(mover) = places.mover {
                vm.requeue(mover);
            }
            Ok(())
        } else if places.portal_manager == Some(id) {
            self.portal_manager_tick(vm, host, id, joined)
        } else {
            Ok(())
        }
    }

    /// The portal manager's tick as the loop runs it (the module docs), for
    /// `pm` once its own first tick has run (the VM ticks it then).
    pub fn portal_manager_tick(
        &self,
        vm: &mut Vm,
        host: &mut dyn Host,
        pm: PlacementId,
        joined: bool,
    ) -> Result<(), Halt> {
        if !vm.is_held(pm) {
            return Ok(());
        }
        // The camera's frame stops alone (openspec AG.28): stopped in play,
        // the view is the TV's and the rest of the tick runs on.
        if let Some(r) = &self.camera {
            vm.run_span_apart(pm, r.start, r.end, host)?;
        }
        let now = vm.active_seconds(pm).unwrap_or(0.0);
        // False only when no op of the program names the local: nothing
        // reads it then (a synthetic portal manager's).
        vm.set_local(pm, "script timer local", Value::Float(now));
        // The HUD's banners, each player's, joined or not (openspec AH.222;
        // stand-in ledger: `hud-messages`).
        let since = now
            - vm.local(pm, "script timer last frame")
                .and_then(|v| vm.value(v).as_f32())
                .unwrap_or(0.0);
        for player in players_of(vm, pm) {
            crate::hud_messages::step(vm, host, pm, player, since)?;
        }
        if joined {
            for r in [&self.death, &self.hurts, &self.combat, &self.projectiles]
                .into_iter()
                .flatten()
            {
                vm.run_span(pm, r.start, r.end, host)?;
            }
            if self.player.is_some() || self.cam_target.is_some() {
                let paused = global_number(vm, "Pause Layer") > 0.0;
                for player in players_of(vm, pm) {
                    vm.set_local(pm, "player data scratch", player);
                    vm.set_field(player, "script timer", Value::Float(now));
                    let playing = vm
                        .field(player, "playing")
                        .and_then(|v| vm.value(v).as_f32())
                        .unwrap_or(0.0);
                    if playing == 0.0 {
                        continue;
                    }
                    if let Some(r) = &self.cam_target {
                        vm.run_span_apart(pm, r.start, r.end, host)?;
                    }
                    if let Some(r) = &self.player
                        && !paused
                    {
                        vm.run_span(pm, r.start, r.end, host)?;
                    }
                }
            }
        }
        if let Some(r) = &self.last_frame {
            vm.run_span(pm, r.start, r.end, host)?;
        }
        vm.count_tick(pm);
        Ok(())
    }

    /// Why the camera's frame or a player's camera target stopped in play,
    /// if one did (openspec AG.28): then the TV's camera has the view.
    pub fn camera_stopped<'v>(&self, vm: &'v Vm, pm: PlacementId) -> Option<&'v Halt> {
        [&self.camera, &self.cam_target]
            .into_iter()
            .flatten()
            .find_map(|r| vm.span_stopped(pm, r.start))
    }

    /// The game systems' tick as the loop runs it, for `gs` in its own
    /// place (ahead of the UI's and the portal manager's, AH.59's
    /// recordings; `global.bld` lists it after them, which isn't the order
    /// it runs in): the update macros the loop runs ([`UPDATES`]), then once
    /// a hero has joined its orbs; its clock counted each step, as its own
    /// tick would. Nothing until its own first tick has run.
    pub fn game_systems_tick(
        &self,
        vm: &mut Vm,
        host: &mut dyn Host,
        gs: PlacementId,
        joined: bool,
    ) -> Result<(), Halt> {
        if !vm.is_held(gs) {
            return Ok(());
        }
        run_updates(vm, host, gs)?;
        if joined && let Some((_, r)) = &self.orbs {
            vm.run_span(gs, r.start, r.end, host)?;
        }
        vm.count_tick(gs);
        Ok(())
    }
}

/// The update macros of `System Update Controls`' `update while unpaused
/// macros` the loop runs ([`UPDATES`]), each once in the set's order as the
/// game systems' tick reaches the set, for `gs`; one a macro takes out of
/// the set before its turn doesn't run.
fn run_updates(vm: &mut Vm, host: &mut dyn Host, gs: PlacementId) -> Result<(), Halt> {
    let listed = update_macros(vm);
    for name in listed {
        if update_macros(vm).contains(&name) {
            vm.call_with(Some(gs), name, &[], &[], host)?;
        }
    }
    Ok(())
}

/// The set as it stands, by the names the loop runs: read again before each
/// call, since an update macro takes itself or another out when it's done
/// (the bounce with nothing bouncing, the remover with nothing to remove).
pub fn update_macros(vm: &mut Vm) -> Vec<&'static str> {
    let Some(g) = vm.program().global_named("System Update Controls") else {
        return Vec::new();
    };
    let Some(controls) = vm.global(g).map(|v| vm.value(v)) else {
        return Vec::new();
    };
    let Some(set) = vm.field(controls, "update while unpaused macros") else {
        return Vec::new();
    };
    let elems = vm.set(set).map(<[Value]>::to_vec).unwrap_or_default();
    let program = vm.program();
    elems
        .into_iter()
        .filter_map(|e| match vm.value(e) {
            Value::Object(o) => program.def_of(o),
            _ => None,
        })
        .filter_map(|d| {
            let d = program.resolve(d).unwrap_or(d);
            let display = program.defs.get(d as usize)?.display.as_str();
            UPDATES.iter().copied().find(|u| *u == display)
        })
        .collect()
}

/// The portal manager's `player data`, each player's structure.
fn players_of(vm: &Vm, pm: PlacementId) -> Vec<Value> {
    vm.local(pm, "player data")
        .map(|v| vm.value(v))
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .map(|p| vm.value(p))
        .collect()
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
    use guac_formats::igz::Location;
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
    /// The camera's macros the synthetic tick calls (openspec AG.28), each
    /// counting its calls the same way: the frame, the tick's first
    /// statement, and a playing player's camera target, ahead of his orbs.
    const CAMERA_CALLED: [(&str, &str); 2] = [
        ("Camera - System - Update", "camera"),
        ("Camera - Update Player Cam Target", "cam target"),
    ];

    /// The synthetic file's banner macros (openspec AH.222), each counting
    /// its calls in a value cell of its own: the level-up's init, and
    /// `Clear Message`, which here does the level-up's shutdown itself.
    const BANNER: [(&str, &str); 2] = [
        ("Replace Message - Level Up - Init", "banner init"),
        (
            "MainUI - Player HUD - Replace Message - Clear Message",
            "banner clear",
        ),
    ];
    /// Level 2's full health by combat's rule for a 290 base (`Round Number
    /// Up(290 × 1.111)`), what the synthetic level-up's end writes.
    const LEVEL_2_FULL: i32 = 323;

    /// The HUD's banner queue in the synthetic file (openspec AH.222), in
    /// the original's shape: a player's queue, its clock, the banner
    /// showing, his full health and his flag; a banner's state, timer and
    /// definition; a definition's init macro; `UI Animations`' slam and
    /// its frames (the test puts them in); the level-up's definition, which
    /// STARTUP points at the init. `Clear Message` does what the original's
    /// does with a level-up's: his health and full health to
    /// [`LEVEL_2_FULL`] and his flag cleared (the definition's shutdown),
    /// then the banner off the queue and none showing.
    struct Banners {
        info_ty: Location,
        def_ty: Location,
        anims_ty: Location,
        keyframer_ty: Location,
        frame_ty: Location,
        hp: Location,
        clock: Location,
        queue: Location,
        showing: Location,
        hp_max: Location,
        invulnerable: Location,
        state: Location,
        timer: Location,
        def: Location,
        init: Location,
        slam: Location,
        frames: Location,
        time: Location,
        anims: Location,
        level_up: Location,
        params: [Location; 5],
        defs: [Location; 2],
    }

    impl Banners {
        fn new(w: &mut Writer<'_>, hp: Location) -> Self {
            let info_ty = w.op(
                Class::DefineStructure,
                "UI - Player HUD Replace Message Info",
            );
            let def_ty = w.op(
                Class::DefineStructure,
                "UI - Player HUD Replace Message Def",
            );
            let anims_ty = w.op(Class::DefineStructure, "UI Animation Set");
            let keyframer_ty = w.op(Class::DefineStructure, "Keyframer Definition");
            let frame_ty = w.op(Class::DefineStructure, "Keyframer Frame");
            let queue = w.variable("hud replace messages", "ScriptSet", None);
            w.refer(queue, 0x24, info_ty);
            let slam = w.variable("slam no fade in", "ScriptStructure", None);
            w.refer(slam, 0x24, keyframer_ty);
            let frames = w.variable("frames", "ScriptSet", None);
            w.refer(frames, 0x24, frame_ty);
            let anims = w.b.object(0, "ScriptStructure", 0x20);
            w.text(anims, 0x08, "UI Animations");
            w.refer(anims, 0x1C, anims_ty);
            let level_up = w.b.object(0, "ScriptStructure", 0x20);
            w.text(level_up, 0x08, "hud replace message def - level up");
            w.refer(level_up, 0x1C, def_ty);
            let params = [
                ("cur player joined data", "ScriptReference"),
                ("message info", "ScriptReference"),
                ("clear message", "ScriptReference"),
                ("clear for player joined data", "ScriptReference"),
                ("fade time", "ValueInfo"),
            ]
            .map(|(n, c)| {
                let contents = (c == "ValueInfo").then_some(FLOAT);
                w.variable(n, c, contents)
            });
            let defs = BANNER.map(|(name, _)| w.op(Class::DefineMacro, name));
            Banners {
                info_ty,
                def_ty,
                anims_ty,
                keyframer_ty,
                frame_ty,
                hp,
                clock: w.variable("hud replace message timer", "ValueInfo", Some(FLOAT)),
                queue,
                showing: w.variable("cur hud replace message", "ScriptReference", None),
                hp_max: w.variable("hp max", "ValueInfo", Some(INT)),
                invulnerable: w.variable("invulnerable", "ValueInfo", Some(INT)),
                state: w.variable("state", "ValueInfo", Some(INT)),
                timer: w.variable("timer", "ValueInfo", Some(INT)),
                def: w.variable("def", "ScriptReference", None),
                init: w.variable("init macro", "ScriptReference", None),
                slam,
                frames,
                time: w.variable("time", "ValueInfo", Some(FLOAT)),
                anims,
                level_up,
                params,
                defs,
            }
        }

        /// A player's: his queue, its clock, the banner showing, his full
        /// health and his flag.
        fn player_fields(&self) -> [Location; 5] {
            [
                self.clock,
                self.queue,
                self.showing,
                self.hp_max,
                self.invulnerable,
            ]
        }

        fn structures(&self, w: &mut Writer<'_>) {
            for (ty, fields) in [
                (self.info_ty, vec![self.state, self.timer, self.def]),
                (self.def_ty, vec![self.init]),
                (self.anims_ty, vec![self.slam]),
                (self.keyframer_ty, vec![self.frames]),
                (self.frame_ty, vec![self.time]),
            ] {
                let p = w.push(ty);
                for f in fields {
                    w.push(f);
                }
                w.end_block(p);
            }
        }

        /// The slam's four frames (the test times them) and the level-up's
        /// definition naming its init.
        fn startup(&self, w: &mut Writer<'_>) {
            for _ in 0..4 {
                w.inc(&[
                    El::Local(self.anims),
                    El::Local(self.slam),
                    El::Local(self.frames),
                    El::Member("setvariant", "count"),
                ]);
            }
            let l = w.value(&[El::Local(self.level_up), El::Local(self.init)]);
            let r = w.reference(&[El::Local(self.defs[0])], None);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, l);
            w.refer(o, 0x20, r);
            w.push(o);
        }

        fn macros(&self, w: &mut Writer<'_>) {
            let [cur, info, clear, clear_for, fade] = self.params;
            let null = w.reference(&[El::Member("abstractscriptgroup", "null")], None);
            let zero = w.float(0.0);
            // The init: counted.
            let p = w.open_macro(
                self.defs[0],
                &[
                    ("cur player joined data", "ScriptReference", null),
                    ("message info", "ScriptReference", null),
                ],
            );
            let _ = (cur, info);
            w.inc(&[El::Member("test", BANNER[0].1)]);
            w.end_block(p);
            // `Clear Message`: the level-up's end, then the banner off.
            let p = w.open_macro(
                self.defs[1],
                &[
                    ("clear message", "ScriptReference", null),
                    ("clear for player joined data", "ScriptReference", null),
                    ("fade time", "ValueInfo", zero),
                ],
            );
            let _ = fade;
            let full = w.int(LEVEL_2_FULL);
            w.set(&[El::Local(clear_for), El::Local(self.hp_max)], full);
            let full = w.rhs(INT, &[El::Local(clear_for), El::Local(self.hp_max)]);
            w.set(&[El::Local(clear_for), El::Local(self.hp)], full);
            let off = w.int(0);
            w.set(&[El::Local(clear_for), El::Local(self.invulnerable)], off);
            let l = w.value(&[El::Local(clear_for), El::Local(self.showing)]);
            let r = w.reference(&[El::Member("abstractscriptgroup", "null")], None);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, l);
            w.refer(o, 0x20, r);
            w.push(o);
            let lhs = w.stack(
                guac_formats::script::StackClass::Set,
                &[El::Local(clear_for), El::Local(self.queue)],
            );
            let rhs = w.reference(&[El::Local(clear)], None);
            let o = w.op(Class::ChangeMembership, "set [set]||to");
            w.refer(o, 0x1C, lhs);
            w.word(o, 0x20, 1);
            w.refer(o, 0x24, rhs);
            w.push(o);
            w.inc(&[El::Member("test", BANNER[1].1)]);
            w.end_block(p);
        }
    }

    /// A portal manager in the original's shape, our ops: its STARTUP
    /// makes two players, the first playing, each with the HUD's banner
    /// queue ([`Banners`]); its tick's body calls the
    /// camera's frame, sets its clock's local, ends at a gate that stays
    /// shut (the VM's own first tick stops there), then the death
    /// sequence's check (finalizing a dying player), the hurts' upkeep every
    /// 0.25 s, the combat and projectile managers, the per-player loop (the
    /// camera target while he plays, orbs, the loot check every 0.25 s, loot
    /// collecting, health remembered) and the last frame's time.
    fn portal_manager() -> Arc<Program> {
        portal_manager_with(|_| {})
    }

    /// [`portal_manager`]'s file with `extra`'s scripts after it.
    fn portal_manager_with(extra: impl FnOnce(&mut Writer<'_>)) -> Arc<Program> {
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
            let banners = Banners::new(w, hp);
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
            let camera: Vec<_> = CAMERA_CALLED
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
            banners.startup(w);
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            banners.macros(w);
            for (def, (_, cell)) in defs
                .iter()
                .zip(CALLED)
                .chain(camera.iter().zip(CAMERA_CALLED))
            {
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
            for f in banners.player_fields() {
                w.push(f);
            }
            w.end_block(p);
            banners.structures(w);
            w.end_block(types);
            // The tick's body.
            w.call(camera[0], &[]);
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
            let one = w.int(1);
            let plays = w.check(&[El::Local(scratch), El::Local(playing)], 1, one);
            w.call(camera[1], &[]);
            w.close(plays);
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
            extra(w);
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
            Self::with(Mode::Strict, &[])
        }

        /// The bench in `mode`, the counters named in `unmodelled` left
        /// unregistered: a macro counting on one stops on a member nothing
        /// models.
        fn with(mode: Mode, unmodelled: &[&str]) -> Self {
            let program = portal_manager();
            let player_loop = PlayerLoop::read(&program);
            let files = Files {
                placed: vec![Placed::named(PM, PORTAL_MANAGER, Some(PATH), 4)],
                ..Files::default()
            };
            let handles = HashMap::from([(PM, PM_HANDLE)]);
            let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
            let mut config = Config::new(mode);
            for (_, cell) in CALLED.iter().chain(&CAMERA_CALLED) {
                if unmodelled.contains(cell) {
                    continue;
                }
                config
                    .members
                    .value_cell("test", cell, MemberSpec::new(Kind::Int, 0.0, 0.0));
            }
            config
                .members
                .value_cell("test", "hurts", MemberSpec::new(Kind::Int, 0.0, 0.0));
            for (_, cell) in BANNER {
                if unmodelled.contains(&cell) {
                    continue;
                }
                config
                    .members
                    .value_cell("test", cell, MemberSpec::new(Kind::Int, 0.0, 0.0));
            }
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

        /// One step as the TV's: the slides, then the walk with the loop's
        /// slices in their places.
        fn step(&mut self, joined: bool) {
            walk(
                &mut self.vm,
                &mut self.world,
                &self.player_loop,
                &mut self.generator,
                joined,
            );
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

        /// The award's part of a level-up for the first player (openspec
        /// AH.222): the slam's four frames timed as the disc's `UI
        /// Animations` holds them (0.166 s), him bitten to 278 of 290 and
        /// kept from harm, the level-up's banner queued as `Add Message`
        /// queues it. Returns the player.
        fn queue_level_up(&mut self) -> Value {
            let p0 = self.player(0);
            let anims = {
                let g = self.vm.program().global_named("UI Animations").unwrap();
                self.vm.global(g).unwrap()
            };
            let frames = self
                .vm
                .field(anims, "slam no fade in")
                .and_then(|s| self.vm.field(s, "frames"))
                .unwrap();
            let four = self.vm.set(frames).map(<[Value]>::to_vec).unwrap();
            assert_eq!(four.len(), 4, "STARTUP's four frames");
            for f in four {
                assert!(self.vm.set_field(f, "time", Value::Float(0.166)));
            }
            assert!(self.vm.set_field(p0, "hp", Value::Int(278)));
            assert!(self.vm.set_field(p0, "hp max", Value::Int(290)));
            assert!(self.vm.set_field(p0, "invulnerable", Value::Int(1)));
            let mut engine = NoEngine;
            let mut spawn = headless_spawner(0x2000);
            let mut host = WorldHost {
                world: &mut self.world,
                engine: &mut engine,
                generator: &mut self.generator,
                spawn: &mut spawn,
            };
            let queue = self.vm.field(p0, "hud replace messages").unwrap();
            let banner = self.vm.grow(queue, &mut host).unwrap();
            let def = {
                let g = self
                    .vm
                    .program()
                    .global_named("hud replace message def - level up")
                    .unwrap();
                self.vm.global(g).unwrap()
            };
            assert!(self.vm.set_reference(banner, "def", self.vm.value(def)));
            p0
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

    /// The camera's stretches (openspec AG.28): its frame the tick's first
    /// statement, and in the per-player loop the statement holding the
    /// camera target's call and not the orbs'; the frame runs every step
    /// from the level's start, joined or not, the camera target once a
    /// step for each player playing once a hero has joined.
    #[test]
    fn the_cameras_frame_runs_every_step_and_his_target_while_he_plays() {
        let program = portal_manager();
        let l = PlayerLoop::read(&program);
        let ops = &program.scripts[l.script.unwrap() as usize].ops;
        let camera = l.camera.clone().unwrap();
        assert_eq!(
            called(&program, ops[camera.start as usize]),
            Some("Camera - System - Update")
        );
        assert_eq!(
            tick_body(&program, ops)[0],
            camera,
            "the tick's first statement"
        );
        let target = l.cam_target.clone().unwrap();
        let player = l.player.clone().unwrap();
        assert!(target.end <= player.start, "{target:?} ahead of {player:?}");
        assert!(target.clone().any(
            |q| called(&program, ops[q as usize]) == Some("Camera - Update Player Cam Target")
        ));
        let mut bench = Bench::new();
        let (frames, targets) = (bench.count("camera"), bench.count("cam target"));
        for _ in 0..3 {
            bench.step(false);
        }
        assert_eq!(bench.count("camera"), frames + 3, "unjoined, each step");
        assert_eq!(
            bench.count("cam target"),
            targets,
            "no player while unjoined"
        );
        for _ in 0..2 {
            bench.step(true);
        }
        assert_eq!(bench.count("camera"), frames + 5);
        assert_eq!(
            bench.count("cam target"),
            targets + 2,
            "the one playing, once a step"
        );
    }

    /// A camera stretch stopped in play stops alone (openspec AG.28): the
    /// player's camera target on a member nothing models stops that stretch
    /// for good, which `camera_stopped` names (the TV then hands the view
    /// back), and the portal manager, the camera's frame and his orbs run on
    /// every step after.
    #[test]
    fn a_camera_target_stopped_in_play_stops_that_stretch_alone() {
        let mut bench = Bench::with(Mode::Play, &["cam target"]);
        let (frames, orbs) = (bench.count("camera"), bench.count("collect"));
        assert!(
            bench
                .player_loop
                .camera_stopped(&bench.vm, bench.pm)
                .is_none()
        );
        for _ in 0..3 {
            bench.step(true);
        }
        let why = bench
            .player_loop
            .camera_stopped(&bench.vm, bench.pm)
            .expect("the camera target's stop");
        assert!(why.to_string().contains("unmodelled member"), "{why}");
        // The camera target's stretch, not the frame's.
        let (frame, target) = (
            bench.player_loop.camera.clone().unwrap(),
            bench.player_loop.cam_target.clone().unwrap(),
        );
        assert!(bench.vm.span_stopped(bench.pm, target.start).is_some());
        assert!(bench.vm.span_stopped(bench.pm, frame.start).is_none());
        assert!(
            bench.vm.stopped(bench.pm).is_none(),
            "the portal manager runs on"
        );
        assert_eq!(bench.count("camera"), frames + 3, "the frame runs on");
        assert_eq!(bench.count("collect"), orbs + 3, "his orbs run on");
    }

    const GS: Obj = Obj { unit: 0, index: 2 };
    const GS_HANDLE: u64 = 0x30;
    const GS_PATH: &str = "C:/tfb/Content/Levels/Includes/Global/scripts/GameSystems.ai";
    /// The game systems' update macros the synthetic file defines, each
    /// counting its calls in a value cell: two the loop runs, one it
    /// doesn't. The actor remover takes itself out of the set as it runs.
    const UPDATED: [(&str, &str); 3] = [
        ("Destructibles - System Update", "destructibles"),
        ("Actor Remover Sytem Update Macro", "remover"),
        ("Difficulty Tracking Macro", "difficulty"),
    ];

    /// A game systems file in the original's shape, our ops: `System Update
    /// Controls` a file structure whose `update while unpaused macros` its
    /// STARTUP fills with the three update macros. With `first_takes_remover`
    /// the destructibles' macro takes the actor remover out of the set each
    /// call, before the remover's turn.
    fn game_systems(first_takes_remover: bool) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let controls_ty = w.op(Class::DefineStructure, "Update Controls");
            let updates = w.variable("update while unpaused macros", "ScriptSet", None);
            let controls = w.b.object(0, "ScriptStructure", 0x20);
            w.text(controls, 0x08, "System Update Controls");
            w.refer(controls, 0x1C, controls_ty);
            let defs: Vec<_> = UPDATED
                .iter()
                .map(|(name, _)| w.op(Class::DefineMacro, name))
                .collect();
            let membership = |w: &mut Writer<'_>, def, combine| {
                let lhs = w.stack(
                    guac_formats::script::StackClass::Set,
                    &[El::Local(controls), El::Local(updates)],
                );
                let rhs = w.reference(&[El::Local(def)], None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, combine);
                w.refer(o, 0x24, rhs);
                w.push(o);
            };
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            for &d in &defs {
                membership(w, d, 0);
            }
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for (i, (def, (_, cell))) in defs.iter().zip(UPDATED).enumerate() {
                let p = w.push(*def);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.inc(&[El::Member("test", cell)]);
                if i == 1 {
                    membership(w, *def, 1);
                }
                if i == 0 && first_takes_remover {
                    membership(w, defs[1], 1);
                }
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            let p = w.push(controls_ty);
            w.push(updates);
            w.end_block(p);
            w.end_block(types);
            w.end_block(pre);
            w.end(GS_PATH);
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

    /// The game systems' update macros as the loop runs them (openspec
    /// AH.21): from its first tick on, each listed one in the set once a
    /// step, the destructibles' every step, the actor remover once (it takes
    /// itself out), the difficulty tracking never; before its first tick,
    /// nothing; the orbs only once a hero has joined (none here).
    #[test]
    fn the_game_systems_run_the_listed_update_macros() {
        let (mut vm, mut world, player_loop, gs) = game_systems_run(false);
        assert_eq!(player_loop.orbs, None);
        let mut generator = Generator::seeded(1);
        game_systems_step(&mut vm, &mut world, &player_loop, &mut generator);
        assert!(vm.is_held(gs), "its first tick taken");
        assert_eq!(update_calls(&vm), [0; 3], "nothing before it");
        assert_eq!(
            update_macros(&mut vm),
            [
                "Destructibles - System Update",
                "Actor Remover Sytem Update Macro"
            ],
            "the listed ones, in the set's order"
        );
        for _ in 0..10 {
            game_systems_step(&mut vm, &mut world, &player_loop, &mut generator);
        }
        assert_eq!(update_calls(&vm), [10, 1, 0]);
        assert_eq!(update_macros(&mut vm), ["Destructibles - System Update"]);
        let clock = vm.active_seconds(gs).unwrap();
        assert!((clock - 11.0 / 60.0).abs() < 1e-5, "{clock}");
    }

    /// One the set loses before its turn doesn't run (the set read again
    /// before each call): the destructibles' macro takes the actor remover
    /// out each call, so the remover, listed as the step began, never runs.
    #[test]
    fn an_update_macro_taken_out_before_its_turn_doesnt_run() {
        let (mut vm, mut world, player_loop, _) = game_systems_run(true);
        let mut generator = Generator::seeded(1);
        for _ in 0..11 {
            game_systems_step(&mut vm, &mut world, &player_loop, &mut generator);
        }
        assert_eq!(update_calls(&vm), [10, 0, 0]);
        assert_eq!(update_macros(&mut vm), ["Destructibles - System Update"]);
    }

    /// [`game_systems`]' file on a VM of its own, in strict mode with its
    /// first tick listed and each update macro's count a value cell: the
    /// VM, its world, the loop read off it and the placement.
    fn game_systems_run(first_takes_remover: bool) -> (Vm, ScriptWorld, PlayerLoop, PlacementId) {
        let program = game_systems(first_takes_remover);
        let player_loop = PlayerLoop::read(&program);
        let files = Files {
            placed: vec![Placed::named(GS, "GameSystems", Some(GS_PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(GS, GS_HANDLE)]);
        let world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        for (_, cell) in UPDATED {
            config
                .members
                .value_cell("test", cell, MemberSpec::new(Kind::Int, 0.0, 0.0));
        }
        config.whitelist = Whitelist::with_first_ticks(&[], &[], &["GameSystems.ai"]);
        let sid = program.script_named("GameSystems.ai").unwrap();
        let mut vm = Vm::new(program, config);
        let gs = vm.add_at(GS_HANDLE, "GameSystems", sid, GS);
        (vm, world, player_loop, gs)
    }

    /// A step as the loop takes it, a hero joined: the game systems' tick
    /// in its place.
    fn game_systems_step(
        vm: &mut Vm,
        world: &mut ScriptWorld,
        player_loop: &PlayerLoop,
        generator: &mut Generator,
    ) {
        walk(vm, world, player_loop, generator, true);
    }

    /// One step's slides and walk over `world`, strict: any stop panics.
    fn walk(
        vm: &mut Vm,
        world: &mut ScriptWorld,
        player_loop: &PlayerLoop,
        generator: &mut Generator,
        joined: bool,
    ) {
        let mut engine = NoEngine;
        let mut spawn = headless_spawner(0x1000);
        let mut host = WorldHost {
            world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        vm.slide(&mut host).unwrap();
        let mut stopped = Vec::new();
        player_loop
            .walk(vm, &mut host, joined, &mut stopped)
            .unwrap_or_else(|h| panic!("{h}"));
        assert!(stopped.is_empty(), "strict mode stops by halting");
    }

    /// Each of [`UPDATED`]'s calls so far.
    fn update_calls(vm: &Vm) -> [i32; 3] {
        UPDATED.map(
            |(_, cell)| match vm.value_cell(None, MemberId::named("test", cell)) {
                Some(Value::Int(i)) => i,
                _ => 0,
            },
        )
    }

    /// openspec AH.59: a step's walk in the original's order, on the
    /// synthetic portal manager's file grown by a counting script (each
    /// placement running it reads the combat manager's calls so far into
    /// `seen` and the step's count of ticks into `place`, then counts on)
    /// and a music controller with nothing to tick. Added as `init_chain`
    /// adds them (the music controller, the UI, the portal manager, the
    /// mover update, a level's Chompy, then the hero's minion made the
    /// activator at his join), they walk the hero first, the UI ahead of the
    /// portal manager's slice, the Chompy after it, and the mover update,
    /// which the music controller's place sends to the end, last.
    #[test]
    fn a_step_walks_in_the_originals_order_with_the_loops_slices_in_place() {
        let program = portal_manager_with(|w| {
            let counter = w.b.object(0, "ValueInfo", 0x24);
            w.text(counter, 0x08, "Counter");
            w.meta_word(counter, 0x1C, INT);
            w.word(counter, 0x20, 0);
            let seen = w.variable("seen", "ValueInfo", Some(INT));
            let place = w.variable("place", "ValueInfo", Some(INT));
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let calls = w.rhs(INT, &[El::Member("test", "combat")]);
            w.set(&[El::Local(seen)], calls);
            let at = w.rhs(INT, &[El::Local(counter)]);
            w.set(&[El::Local(place)], at);
            w.inc(&[El::Local(counter)]);
            w.end_block(pre);
            w.end("C:/tfb/Content/Levels/Synthetic/Count.ai");
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            w.end_block(start);
            w.end_block(pre);
            w.end("C:/tfb/Content/Levels/Includes/Global/scripts/MusicController.ai");
        });
        let player_loop = PlayerLoop::read(&program);
        // Each placement's file path, as the places are found by it.
        let named: [(Obj, u64, &str, &str); 6] = [
            (
                Obj { unit: 0, index: 11 },
                0x11,
                "Macros_Music",
                "MusicController.ai",
            ),
            (
                Obj { unit: 0, index: 12 },
                0x12,
                "Macros_UI",
                "Global/scripts/UI.ai",
            ),
            (PM, PM_HANDLE, PORTAL_MANAGER, "Spyro_PortalManager.ai"),
            (
                Obj { unit: 0, index: 13 },
                0x13,
                "movementUpdate",
                "movementUpdate.ai",
            ),
            (
                Obj { unit: 1, index: 1 },
                0x21,
                "Enemy_Chompy(1)",
                "S2_Enemy_Chompy.ai",
            ),
            (
                Obj { unit: 2, index: 1 },
                0x31,
                "009_Eruptor",
                "009_Eruptor.ai",
            ),
        ];
        let files = Files {
            placed: named
                .iter()
                .map(|&(obj, _, name, path)| {
                    Placed::named(obj, name, Some(&format!("C:/tfb/{path}")), 4)
                })
                .collect(),
            ..Files::default()
        };
        let handles: HashMap<Obj, u64> = named.iter().map(|&(o, h, ..)| (o, h)).collect();
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        for (_, cell) in CALLED.iter().chain(&CAMERA_CALLED) {
            config
                .members
                .value_cell("test", cell, MemberSpec::new(Kind::Int, 0.0, 0.0));
        }
        config
            .members
            .value_cell("test", "hurts", MemberSpec::new(Kind::Int, 0.0, 0.0));
        config.whitelist = Whitelist::with_first_ticks(
            &[
                "Macros_UI",
                "movementUpdate",
                "Enemy_Chompy(1)",
                "009_Eruptor",
            ],
            &[],
            &["Spyro_PortalManager.ai", "MusicController.ai"],
        );
        let pm_script = program.script_named("Spyro_PortalManager.ai").unwrap();
        let music_script = program.script_named("MusicController.ai").unwrap();
        let count = program.script_named("Count.ai").unwrap();
        let mut vm = Vm::new(program, config);
        let mut ids = HashMap::new();
        for &(obj, h, name, _) in &named {
            let sid = match name {
                PORTAL_MANAGER => pm_script,
                "Macros_Music" => music_script,
                _ => count,
            };
            ids.insert(name, vm.add_at(h, name, sid, obj));
        }
        assert!(vm.set_activator(0x31, true));
        let places = Places::of(&vm, &world);
        assert_eq!(places.game_systems, None, "no game systems here");
        assert_eq!(places.music, Some(ids["Macros_Music"]));
        assert_eq!(places.portal_manager, Some(ids[PORTAL_MANAGER]));
        assert_eq!(places.mover, Some(0x13));
        let mut generator = Generator::seeded(1);
        // The first step takes the held placements' first ticks.
        walk(&mut vm, &mut world, &player_loop, &mut generator, false);
        assert!(vm.is_held(ids[PORTAL_MANAGER]) && vm.is_held(ids["Macros_Music"]));
        let read = |vm: &Vm, name: &str, var: &str| match vm.local(ids[name], var) {
            Some(Value::Int(i)) => i,
            other => panic!("{name}'s {var}: {other:?}"),
        };
        for step in 1..4 {
            walk(&mut vm, &mut world, &player_loop, &mut generator, true);
            let base = 4 * step;
            let places: Vec<i32> = [
                "009_Eruptor",
                "Macros_UI",
                "Enemy_Chompy(1)",
                "movementUpdate",
            ]
            .iter()
            .map(|n| read(&vm, n, "place") - base)
            .collect();
            assert_eq!(places, [0, 1, 2, 3], "step {step}: hero, UI, Chompy, mover");
            let calls = step;
            assert_eq!(
                read(&vm, "009_Eruptor", "seen"),
                calls - 1,
                "before the slice"
            );
            assert_eq!(
                read(&vm, "Macros_UI", "seen"),
                calls - 1,
                "the UI ahead of it"
            );
            assert_eq!(read(&vm, "Enemy_Chompy(1)", "seen"), calls, "after it");
            assert_eq!(read(&vm, "movementUpdate", "seen"), calls);
        }
        assert_eq!(
            vm.tick_order()
                .into_iter()
                .map(|(g, n)| format!("{}{n}", if g { "*" } else { "" }))
                .collect::<Vec<_>>(),
            [
                "*009_Eruptor",
                "Macros_Music",
                "Macros_UI",
                PORTAL_MANAGER,
                "Enemy_Chompy(1)",
                "movementUpdate"
            ]
        );
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

    /// AH.222: a level-up's banner, queued as the original's award queues
    /// it, raises his health to the new level's full health once it ends,
    /// on the original's timing: none showing until the next step, shown
    /// the step after (its definition's init run once), held while its
    /// icon slams in (the slam's first three 0.166 s frames) and 1.8 s
    /// more, then `Clear Message`, which ends it as a level-up's ends (full
    /// health 323, his health to it, his flag cleared) and takes it off.
    /// Until then his health and flag stay as the award left them, bitten
    /// to 278 of 290 and kept from harm. The queue's clock runs on by the
    /// portal manager's time, joined or not.
    #[test]
    fn a_level_ups_banner_raises_his_health_to_full_when_it_ends() {
        let mut b = Bench::new();
        let p0 = b.queue_level_up();
        let int = |b: &Bench, name: &str| b.vm.field(p0, name).and_then(|v| b.vm.value(v).as_i32());
        let clock = |b: &Bench| {
            b.vm.field(p0, "hud replace message timer")
                .and_then(|v| b.vm.value(v).as_f32())
                .unwrap_or(f32::NAN)
        };
        let showing = |b: &Bench| {
            b.vm.field(p0, "cur hud replace message")
                .map(|v| b.vm.value(v))
                .unwrap_or(Value::Null)
        };
        let (mut shown, mut slammed, mut ended) = (None, None, None);
        let start = clock(&b);
        for k in 1..=200 {
            b.step(k > 100);
            if shown.is_none() && b.count("banner init") == 1 {
                shown = Some(k);
            }
            let state = match showing(&b) {
                Value::Null => None,
                m => b.vm.field(m, "state").and_then(|v| b.vm.value(v).as_i32()),
            };
            if slammed.is_none() && state == Some(2) {
                slammed = Some(k);
            }
            if ended.is_none() && b.count("banner clear") == 1 {
                ended = Some(k);
            } else if ended.is_none() {
                assert_eq!(
                    (int(&b, "hp"), int(&b, "hp max"), int(&b, "invulnerable")),
                    (Some(278), Some(290), Some(1)),
                    "step {k}: as the award left him"
                );
            }
            if k == 1 {
                assert!(showing(&b) != Value::Null, "showing from the next step");
                assert_eq!(b.count("banner init"), 0, "shown the step after");
            }
        }
        // The queue's clock: the portal manager's time, every step.
        assert!(
            (clock(&b) - start - 200.0 / 60.0).abs() < 1e-3,
            "{} s on",
            clock(&b) - start
        );
        // Shown 2 steps on, slammed in 30 later (0.498 s), ended 109 after
        // that (1.8 s on the clock's single-precision sums of 1/60).
        assert_eq!((shown, slammed, ended), (Some(2), Some(32), Some(141)));
        assert_eq!(b.count("banner init"), 1);
        assert_eq!(b.count("banner clear"), 1);
        assert_eq!(
            (int(&b, "hp"), int(&b, "hp max"), int(&b, "invulnerable")),
            (Some(LEVEL_2_FULL), Some(LEVEL_2_FULL), Some(0))
        );
        assert_eq!(showing(&b), Value::Null, "none showing");
        let queue = b.vm.field(p0, "hud replace messages").unwrap();
        assert_eq!(b.vm.set(queue).map(<[Value]>::len), Some(0), "taken off");
    }

    /// AH.222: a banner whose own macro stops in play mode (here its init,
    /// counting on a member nothing models) is parked there, logged, and
    /// the portal manager runs on: its calls go on each step, the banner
    /// never ends, and nothing it would have done is done.
    #[test]
    fn a_banner_whose_call_stops_in_play_is_parked_and_the_portal_manager_runs_on() {
        let mut b = Bench::with(Mode::Play, &["banner init"]);
        let p0 = b.queue_level_up();
        let combat = b.count("combat");
        for k in 1..=200 {
            b.step(true);
            if k == 2 {
                let banner =
                    b.vm.field(p0, "cur hud replace message")
                        .map(|v| b.vm.value(v))
                        .unwrap();
                assert_eq!(
                    b.vm.field(banner, "state").map(|v| b.vm.value(v)),
                    Some(Value::Int(crate::hud_messages::PARKED)),
                    "parked the step its init stopped"
                );
            }
        }
        assert_eq!(b.vm.stopped(b.pm), None, "the portal manager runs on");
        assert_eq!(b.count("combat"), combat + 200, "its calls with it");
        assert_eq!(b.count("banner clear"), 0, "never ended");
        let int = |name: &str| b.vm.field(p0, name).and_then(|v| b.vm.value(v).as_i32());
        assert_eq!((int("hp"), int("invulnerable")), (Some(278), Some(1)));
    }
}
