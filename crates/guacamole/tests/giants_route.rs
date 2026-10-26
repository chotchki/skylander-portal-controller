//! AH.28's disc tests: AH.8's route from the level's start to anchor B and
//! on through the first fight to the open gate, headless over level 213 with
//! guacamole's host, every step from the level's own scripts: the init
//! chain, Eruptor joined at the spawn, the level's props standing by their
//! actors (AG.60), the TV's listing (`fight_config`: the first fight's
//! placements asleep until the route's triggers wake them, AH.28's route
//! scripts), the stick driven closed loop by AH.8's route plan (`route.ron`,
//! the steps the oracle's driver carried out: points to pass and stop at,
//! headings to turn to, holds, presses and waits) against the view the bench
//! holds, a game step a frame. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`) and, for the probes'
//! comparisons, AH.8's recordings under `GUAC_CAPTURE_DIR`:
//!
//! `cargo test -p guacamole --release --test giants_route -- --ignored --nocapture --test-threads=1`
//!
//! The route is driven, not replayed: AH.8's `input.ron` is what the
//! oracle's closed loop held on the pad, and replayed open loop here
//! (`probe_the_recorded_input_open_loop`) it parts from the record's walk.
//! Every number in the plan is a reading off AH.8's record, not game bytes.
//!
//! Dev aids, not checks: `probe_the_route_from_the_start` drives the route
//! in play mode and prints each look's placements beside AH.8's census there
//! (`AH28_WATCH=<names>` prints their changes step by step, `AH28_FIGHT`
//! fights on past anchor B, `AH18_LIVE_TABLE` answers every member the
//! registry doesn't hold with a value cell and prints what the run reached);
//! `probe_ops` (`AH28_OPS=<script>#<pos>,...`) and `probe_tags`
//! (`AH28_TAGS=<placements>`) print ops and tags as the linked program holds
//! them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Halt, MemberId, Mode};
use guacamole::anchors::{ANCHOR_B_FIRED, ANCHOR_B_POSE, NORTICUS};
use guacamole::props::SKINNED_SPAWNS;
use guacamole::script_engine::Pad;
use guacamole::scripts;

mod common;

use common::*;

/// The two buttons AH.8's route presses: West erupts, North fires the
/// cannon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    West,
    North,
}

impl Button {
    /// The action it raises, by the TV's table (openspec AH.24: West 2;
    /// AH.28: North 4).
    fn action(self) -> usize {
        match self {
            Button::West => 1,
            Button::North => 3,
        }
    }
}

/// One step of AH.8's plan (`tools/oracle-harness`'s `route::Step`, the
/// ones its route used): ground points in metres (x, z).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    /// The stick full toward the point until his pivot is within the
    /// radius of it across the ground.
    Via([f32; 2], f32),
    /// To the point and still there, within the radius.
    Stop([f32; 2], f32),
    /// Turned where he stands until he faces the point within the degrees.
    Face([f32; 2], f32),
    /// The pad at rest until he has held still this many steps.
    Settle(u32),
    /// The stick full toward the point this many steps, whatever he does.
    Hold([f32; 2], u32),
    /// The button held this many steps, the stick centred.
    Press(Button, u32),
    /// The pad at rest this many steps.
    Wait(u32),
    /// A look at the run, named as AH.8's read there.
    Read(&'static str),
}

/// AH.8's route plan (`ah8-route-1792539821/route_b/route.ron`), its shots
/// left out: 113 steps from the spawn to anchor B.
const ROUTE_PLAN: [Step; 113] = [
    Step::Via([21.6, -89.6], 1.0),
    Step::Via([36.8, -83.7], 1.0),
    Step::Via([44.5, -79.6], 1.0),
    Step::Via([48.8, -77.8], 1.0),
    Step::Via([48.6, -71.5], 1.0),
    Step::Via([43.5, -73.6], 1.0),
    Step::Via([36.5, -70.5], 1.0),
    Step::Via([33.6, -73.0], 0.5),
    Step::Via([33.8, -74.6], 0.5),
    Step::Stop([28.6, -76.0], 0.15),
    Step::Face([23.6, -76.17], 1.0),
    Step::Settle(30),
    Step::Read("ledge_e1_before"),
    Step::Press(Button::West, 6),
    Step::Wait(200),
    Step::Read("ledge_e1"),
    Step::Stop([25.5, -77.25], 0.15),
    Step::Face([20.5, -77.42], 1.0),
    Step::Settle(30),
    Step::Press(Button::West, 6),
    Step::Wait(200),
    Step::Settle(60),
    Step::Read("ledge_e2"),
    Step::Via([18.0, -77.6], 0.5),
    Step::Stop([11.5, -78.0], 0.2),
    Step::Settle(60),
    Step::Face([6.66, -75.89], 1.0),
    Step::Settle(30),
    Step::Read("fence_before"),
    Step::Press(Button::West, 6),
    Step::Wait(200),
    Step::Read("fence_e1"),
    Step::Press(Button::West, 6),
    Step::Wait(240),
    Step::Read("fence_e2"),
    Step::Wait(240),
    Step::Read("fence_down"),
    Step::Via([4.0, -74.0], 0.7),
    Step::Via([-3.0, -68.0], 1.0),
    Step::Stop([-9.0, -58.0], 0.2),
    Step::Settle(60),
    Step::Read("walkway"),
    Step::Via([-10.5, -54.5], 0.7),
    Step::Stop([-12.6, -52.5], 0.2),
    Step::Face([-20.0, -52.5], 1.0),
    Step::Settle(30),
    Step::Read("drop_before"),
    Step::Hold([-24.0, -52.4], 24),
    Step::Hold([-24.0, -52.4], 96),
    Step::Settle(60),
    Step::Read("drop_after"),
    Step::Stop([-30.6, -62.4], 0.2),
    Step::Face([-33.5, -59.2], 1.0),
    Step::Settle(30),
    Step::Read("cannon_before"),
    Step::Press(Button::North, 6),
    Step::Wait(30),
    Step::Wait(270),
    Step::Read("cannon_after"),
    Step::Via([-22.0, -48.0], 1.0),
    Step::Via([-14.0, -38.0], 1.0),
    Step::Stop([-9.0, -36.5], 0.2),
    Step::Settle(60),
    Step::Read("strip"),
    Step::Face([2.17, -36.83], 2.0),
    Step::Settle(20),
    Step::Read("turtle1_before"),
    Step::Hold([12.0, -36.83], 240),
    Step::Hold([14.0, -36.83], 240),
    Step::Settle(30),
    Step::Read("turtle1_pushed"),
    Step::Via([16.5, -31.0], 1.0),
    Step::Via([21.5, -25.0], 1.0),
    Step::Stop([25.0, -22.3], 0.2),
    Step::Face([27.81, -25.04], 2.0),
    Step::Settle(20),
    Step::Read("turtle3_before"),
    Step::Hold([33.0, -30.8], 150),
    Step::Read("turtle3_pushed"),
    Step::Stop([28.2, -24.4], 0.2),
    Step::Face([31.19, -21.68], 2.0),
    Step::Settle(20),
    Step::Read("turtle2_before"),
    Step::Hold([37.5, -15.9], 150),
    Step::Read("turtle2_pushed"),
    Step::Settle(60),
    Step::Via([40.0, -12.0], 1.0),
    Step::Via([36.0, -6.0], 1.0),
    Step::Stop([31.5, -5.5], 0.3),
    Step::Settle(20),
    Step::Read("s001"),
    Step::Stop([27.5, -5.0], 0.3),
    Step::Settle(20),
    Step::Read("activate"),
    Step::Via([22.3, -7.8], 1.0),
    Step::Via([18.4, -5.9], 1.0),
    Step::Via([16.0, -2.5], 1.0),
    Step::Via([13.3, -0.5], 1.0),
    Step::Stop([10.5, -2.0], 0.2),
    Step::Face([4.53, -23.11], 2.0),
    Step::Settle(60),
    Step::Read("scaffold"),
    Step::Face([4.5, -12.0], 2.0),
    Step::Settle(20),
    Step::Hold([4.5, -12.0], 45),
    Step::Wait(30),
    Step::Wait(470),
    Step::Settle(60),
    Step::Read("landed"),
    Step::Stop([5.423, -12.084], 0.1),
    Step::Face([1.797, -20.974], 1.0),
    Step::Settle(120),
    Step::Read("anchor"),
];

/// A let-go's slowing, m/s² (AG.38: 80 ft/s²).
const LET_GO: f32 = 80.0 * 0.3048;

/// The heading from `from` toward `to` across the ground, degrees (θ faces
/// (sin θ, 0, cos θ)).
fn heading_to(from: Vec3, to: [f32; 2]) -> f32 {
    (to[0] - from.x).atan2(to[1] - from.z).to_degrees()
}

fn ground(from: Vec3, to: [f32; 2]) -> f32 {
    (to[0] - from.x).hypot(to[1] - from.z)
}

fn wrap(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// The stick that pushes at `strength` along `heading` against a view
/// looking along `view`, past the dead zone per axis
/// (`guac_motion::stick`).
fn stick_for(view: f32, heading: f32, strength: f32) -> Stick {
    let d = (heading - view).to_radians();
    let raw = |a: f32| {
        let a = a * strength;
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
        }
    };
    Stick::new(raw(-d.sin()), raw(d.cos()))
}

/// A plan step the driver couldn't finish within its budget.
#[derive(Debug, Clone)]
struct Stuck {
    index: usize,
    step: Step,
    at: Vec3,
}

/// The closed loop over AH.8's plan: each game step, the pad for it.
struct Driver {
    index: usize,
    /// Steps into the current plan step.
    k: u32,
    /// Steps he's held still in a row.
    still: u32,
    last: Option<Vec3>,
    stuck: Vec<Stuck>,
}

/// Each plan step's budget, game steps, past which the driver records it
/// stuck and goes on.
fn budget(s: Step) -> u32 {
    match s {
        Step::Via(..) | Step::Stop(..) => 1_200,
        Step::Face(..) => 600,
        Step::Settle(n) => 4 * n + 300,
        Step::Hold(_, n) | Step::Press(_, n) | Step::Wait(n) => n,
        Step::Read(_) => 0,
    }
}

enum Next {
    /// This game step's pad.
    Pad(Pad),
    /// A read here, no step made for it.
    Read(&'static str),
    Done,
}

impl Driver {
    fn new() -> Self {
        Driver {
            index: 0,
            k: 0,
            still: 0,
            last: None,
            stuck: Vec::new(),
        }
    }

    fn advance(&mut self) {
        self.index += 1;
        self.k = 0;
        self.still = 0;
    }

    /// The pad for the next game step with him at `at` facing `facing`,
    /// the view along `view`; reads and finished steps pass on at once.
    fn next(&mut self, at: Vec3, facing: f32, view: f32) -> Next {
        let speed = self
            .last
            .map(|l| (at - l).with_y(0.0).length() * 60.0)
            .unwrap_or(0.0);
        let moved = self.last.map(|l| (at - l).length()).unwrap_or(1.0);
        self.last = Some(at);
        if moved < 0.0005 {
            self.still += 1;
        } else {
            self.still = 0;
        }
        loop {
            let Some(&step) = ROUTE_PLAN.get(self.index) else {
                return Next::Done;
            };
            if self.k >= budget(step) && !matches!(step, Step::Read(_)) {
                if matches!(step, Step::Via(..) | Step::Stop(..) | Step::Face(..)) {
                    self.stuck.push(Stuck {
                        index: self.index,
                        step,
                        at,
                    });
                }
                self.advance();
                continue;
            }
            let mut pad = Pad {
                connected: true,
                ..Pad::default()
            };
            let done = match step {
                Step::Read(label) => {
                    self.advance();
                    return Next::Read(label);
                }
                Step::Via(to, r) => {
                    if ground(at, to) <= r {
                        true
                    } else {
                        pad.stick = stick_for(view, heading_to(at, to), 1.0);
                        false
                    }
                }
                Step::Stop(to, r) => {
                    // Full stick while he can still stop short of it by
                    // letting go, then let go; eased in the last 1.5 m.
                    let d = ground(at, to);
                    if d <= r && speed < 0.02 && self.still >= 2 {
                        true
                    } else {
                        let brake = speed * speed / (2.0 * LET_GO);
                        if d > r * 0.5 + brake && !(d <= r && speed > 0.02) {
                            let s = ((d - r * 0.5) / 1.5).clamp(0.3, 1.0);
                            pad.stick = stick_for(view, heading_to(at, to), s);
                        }
                        false
                    }
                }
                Step::Face(to, within) => {
                    // A tap toward the heading, then the pad at rest until
                    // he's still again, so the turn walks him nowhere.
                    let want = heading_to(at, to);
                    let err = wrap(want - facing);
                    if err.abs() <= within && self.still >= 2 {
                        true
                    } else {
                        if err.abs() > within && self.still >= 2 {
                            let s = (err.abs() / 24.0).clamp(0.12, 1.0);
                            pad.stick = stick_for(view, want, s);
                            self.still = 0;
                        }
                        false
                    }
                }
                Step::Settle(n) => self.still >= n,
                Step::Hold(to, _) => {
                    pad.stick = stick_for(view, heading_to(at, to), 1.0);
                    false
                }
                Step::Press(b, _) => {
                    pad.actions[b.action()] = true;
                    false
                }
                Step::Wait(_) => false,
            };
            if done {
                self.advance();
                continue;
            }
            self.k += 1;
            return Next::Pad(pad);
        }
    }
}

/// One step of `run`: strict, a stop halts the test; in play, the scripts
/// stopped are returned and the run goes on.
fn step(run: &mut Run, mode: Mode) -> Vec<Halt> {
    match mode {
        Mode::Strict => {
            let at = run.steps;
            run.step()
                .unwrap_or_else(|h| panic!("step {at}: {h}; load: {}", load()));
            Vec::new()
        }
        Mode::Play => run.step_play().unwrap_or_else(|h| panic!("{h}")),
    }
}

/// The run joined at the spawn on the TV's listing with the props in, his
/// spawn played out: the init chain's two steps, the join with a fresh
/// figure, 120 steps; the stops in play on the way.
fn joined(mode: Mode) -> (Run, Vec<(u32, Halt)>, HashMap<MemberId, String>) {
    let (mut run, found) = run_on_with(mode, Setup { props: true });
    let mut stops = Vec::new();
    let go = |run: &mut Run, stops: &mut Vec<(u32, Halt)>| {
        let k = run.steps;
        stops.extend(step(run, mode).into_iter().map(|h| (k, h)));
    };
    go(&mut run, &mut stops);
    go(&mut run, &mut stops);
    let files = run.files.clone();
    let handles = run.handles.clone();
    let layout = run.layout.clone();
    let tag = eruptor_tag(guacamole::figure::Values::default());
    let data = run
        .with_host(|vm, host| guacamole::figure::put_on_portal(vm, host, &layout, &tag))
        .unwrap_or_else(|h| panic!("{h}"));
    run.figure = Some(data);
    run.with_host(|vm, host| scripts::join_request(vm, host, &files, ERUPTOR, &handles))
        .unwrap_or_else(|h| panic!("{h}"));
    go(&mut run, &mut stops);
    let hero = run.hero;
    let minion = run
        .with_host(|vm, host| scripts::join_activate(vm, host, hero))
        .unwrap_or_else(|h| panic!("{h}"));
    run.minion = Some(minion);
    for _ in 0..120 {
        go(&mut run, &mut stops);
    }
    (run, stops, found)
}

/// A placement at a look: its state as AH.8's census codes it, its
/// behaviour and its actor's place.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    /// 1 awake, 2 asleep, 3 taken off: the census's codes.
    state: u8,
    /// Lowercased: the VM's spelling; the census keeps the file's case.
    behaviour: Option<String>,
    at: Option<[f32; 3]>,
}

fn ours(run: &Run, name: &str) -> Option<Seen> {
    let p = run
        .files
        .placed
        .iter()
        .find(|p| p.name == name && p.obj.unit == LEVEL)?;
    let h = run.handles[&p.obj];
    let a = run.world.actor(h)?;
    let state = if a.removed {
        3
    } else if a.active {
        1
    } else {
        2
    };
    let behaviour = run
        .vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_ascii_lowercase);
    Some(Seen {
        state,
        behaviour,
        at: Some(a.position.to_array()),
    })
}

/// The run at one of the plan's reads: the hero, his gold, the portal's
/// mode and every watched placement.
#[derive(Debug, Clone)]
struct Look {
    label: &'static str,
    step: u32,
    hero: Vec3,
    gold: u32,
    portal_mode: Option<String>,
    seen: BTreeMap<&'static str, Seen>,
}

impl Look {
    fn of(run: &mut Run, label: &'static str) -> Self {
        let seen = WATCHED
            .iter()
            .filter_map(|n| ours(run, n).map(|s| (*n, s)))
            .collect();
        Look {
            label,
            step: run.steps,
            hero: run.hero().pivot,
            gold: run.figure_values().map_or(0, |v| v.gold),
            portal_mode: scripts::portal_mode(&mut run.vm),
            seen,
        }
    }

    fn state(&self, name: &str) -> u8 {
        self.seen.get(name).map_or(0, |s| s.state)
    }

    fn behaviour(&self, name: &str) -> Option<&str> {
        self.seen.get(name).and_then(|s| s.behaviour.as_deref())
    }

    fn at(&self, name: &str) -> Vec3 {
        Vec3::from_array(self.seen[name].at.expect("a place"))
    }
}

/// The route as far as it goes: every look, the stops in play by step and
/// plan step, what got stuck.
struct Driven {
    run: Run,
    looks: Vec<Look>,
    stops: Vec<(u32, usize, Halt)>,
    stuck: Vec<Stuck>,
    /// What the scripts played on the skinned clones (openspec AG.150).
    played: Played,
    /// The steps a scene held the game (`Cutscene State` past 0), and of
    /// those the steps his minion read invulnerable: the cutscene system's
    /// control wrapper's hold on the minions while one plays.
    scene_steps: u32,
    held_steps: u32,
}

/// Whether the joined hero's minion reads invulnerable
/// (`description.creature info.invulnerable`).
fn invulnerable(run: &Run) -> bool {
    let Some(m) = run.minion else {
        return false;
    };
    let vm = &run.vm;
    vm.local(m, "description")
        .map(|v| vm.value(v))
        .and_then(|d| vm.field(d, "creature info"))
        .map(|v| vm.value(v))
        .and_then(|c| vm.field(c, "invulnerable"))
        .and_then(|v| vm.value(v).as_f32())
        .is_some_and(|f| f == 1.0)
}

/// The palette slots the scripts played on the clones of each template
/// `SKINNED_SPAWNS` names, by the template's name; and each clip one of
/// them played out of another placement's palette (none on the route).
#[derive(Debug, Default)]
struct Played {
    slots: BTreeMap<String, BTreeSet<u32>>,
    foreign: BTreeSet<(String, u32)>,
}

impl Played {
    /// What plays on the skinned clones this step.
    fn record(&mut self, run: &Run) {
        let w = &run.world;
        for s in w.sequences().iter().filter(|s| s.plays()) {
            let Some(a) = w.actor(s.actor).filter(|a| a.obj.is_none()) else {
                continue;
            };
            let Some(t) = a.template.and_then(|t| w.template(t)) else {
                continue;
            };
            if !SKINNED_SPAWNS.contains(&t.name.as_str()) {
                continue;
            }
            if let Some((owner, slot)) = w.clip_of(s) {
                if Some(owner) == a.template {
                    self.slots.entry(t.name.clone()).or_default().insert(slot);
                } else {
                    self.foreign.insert((t.name.clone(), slot));
                }
            }
        }
    }
}

impl Driven {
    fn look(&self, label: &str) -> &Look {
        self.looks
            .iter()
            .find(|l| l.label == label)
            .unwrap_or_else(|| panic!("no look {label}"))
    }
}

/// AH.8's plan driven from where `run` stands, each look read as the plan
/// reaches it; `AH28_WATCH`'s placements printed as they change.
fn drive(mut run: Run, mode: Mode) -> Driven {
    let mut driver = Driver::new();
    let mut looks = Vec::new();
    let mut stops: Vec<(u32, usize, Halt)> = Vec::new();
    let limit = run.steps + 20_000;
    let watch: Vec<String> = std::env::var("AH28_WATCH")
        .map(|w| w.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut watched: BTreeMap<String, State> = BTreeMap::new();
    let mut played = Played::default();
    let (mut scene_steps, mut held_steps) = (0, 0);
    while run.steps < limit {
        for n in &watch {
            let now = ours(&run, n).map(|s| (s.state, s.behaviour));
            if watched.get(n) != Some(&now) {
                println!(
                    "watch step {} plan step {}: {n} {now:?} at {} hero {}",
                    run.steps,
                    driver.index,
                    fmt_at(ours(&run, n).and_then(|s| s.at)),
                    fmt_at(Some(run.hero().pivot.to_array())),
                );
                watched.insert(n.clone(), now);
            }
        }
        let hero = run.hero();
        let (at, facing) = (hero.pivot, hero.facing);
        match driver.next(at, facing, run.bench.view) {
            Next::Done => break,
            Next::Read(label) => looks.push(Look::of(&mut run, label)),
            Next::Pad(pad) => {
                run.bench.pads[0] = pad;
                let k = run.steps;
                let s = step(&mut run, mode);
                stops.extend(s.into_iter().map(|h| (k, driver.index, h)));
                played.record(&run);
                if global_int(&mut run, "Cutscene State") != 0 {
                    scene_steps += 1;
                    held_steps += u32::from(invulnerable(&run));
                }
            }
        }
    }
    Driven {
        run,
        looks,
        stops,
        stuck: driver.stuck,
        played,
        scene_steps,
        held_steps,
    }
}

/// The placements the route's steps touch, in route order (AH.8's note),
/// with the cannon's pan, whose range the walk to the strip crosses.
const WATCHED: [&str; 39] = [
    "barrel_b_intact(1)",
    "barrel_b_intact(3)",
    "barrel_b_intact(4)",
    "pottery_1(8)",
    "pottery_2(5)",
    "gourd_01(10)",
    "gourd_01(12)",
    "gourd_01(13)",
    "Fence Spawner_Template(1)",
    "barrel_b_intact(6)",
    "Sheep_2",
    "Coin_spinning",
    "Coin_spinning_0",
    "Coin_spinning_0(1)",
    "Push_Block_Cannon",
    "Breakable_GateRubble_Wall(1)",
    "Pan Hint_Show_Cannon",
    "Setup_001_Controller",
    "Push_Block_Template(1)",
    "Push_Block_Template(3)",
    "Push_Block_Template(2)",
    "Setup_001_Trigger",
    "Activate_Norticus and Chompies",
    "Spike_Gate_Wooden_Template(1)",
    "1.NPC_Norticus_First_Encounter",
    "Norticus_First_Encounter(1)",
    "Coin_spinning(1)",
    "Coin_spinning(2)",
    "Coin_spinning(3)",
    "Coin_spinning(4)",
    "Coin_spinning(5)",
    "Coin_spinning(6)",
    "Eon_4_help_Norticus",
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "Hint_Combat",
    "Norticus_intro_Camera_Target",
];

/// The breakables AH.8's two eruptions on the ledge route broke.
const LEDGE: [&str; 8] = [
    "barrel_b_intact(1)",
    "barrel_b_intact(3)",
    "barrel_b_intact(4)",
    "pottery_1(8)",
    "pottery_2(5)",
    "gourd_01(10)",
    "gourd_01(12)",
    "gourd_01(13)",
];

/// The spinning coins the route takes: the walkway's three, then the six
/// down into the pit.
const COINS: [&str; 9] = [
    "Coin_spinning",
    "Coin_spinning_0",
    "Coin_spinning_0(1)",
    "Coin_spinning(1)",
    "Coin_spinning(2)",
    "Coin_spinning(3)",
    "Coin_spinning(4)",
    "Coin_spinning(5)",
    "Coin_spinning(6)",
];

const CHOMPIES: [&str; 4] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
];

fn fmt_at(a: Option<[f32; 3]>) -> String {
    a.map_or("-".into(), |a| {
        format!("({:.2}, {:.2}, {:.2})", a[0], a[1], a[2])
    })
}

/// A global number of `run`'s, 0 when it reads none.
fn global_int(run: &mut Run, name: &str) -> i32 {
    let vm = &mut run.vm;
    vm.program()
        .global_named(name)
        .and_then(|g| vm.global(g))
        .and_then(|v| vm.value(v).as_f32())
        .map_or(0, |f| f as i32)
}

/// A placement's state and behaviour, as [`Seen`] holds them.
type State = Option<(u8, Option<String>)>;

/// What the fight past anchor B did, step by step from it: each change of
/// a watched placement's state or behaviour, and of `Cutscene State`.
#[derive(Debug, Default)]
struct Fight {
    changes: Vec<(u32, &'static str, State)>,
    scene: Vec<(u32, i32)>,
    /// Where he stood once Norticus was gone after his scene, and 60 steps
    /// of the stick on after it.
    freed: Option<(Vec3, Vec3)>,
}

impl Fight {
    /// The first step `name` read `behaviour` from.
    fn first(&self, name: &str, behaviour: &str) -> Option<u32> {
        self.changes
            .iter()
            .find(|(_, n, s)| {
                *n == name && s.as_ref().and_then(|s| s.1.as_deref()) == Some(behaviour)
            })
            .map(|c| c.0)
    }

    /// The first step `name` was taken off from.
    fn gone(&self, name: &str) -> Option<u32> {
        self.changes
            .iter()
            .find(|(_, n, s)| *n == name && s.as_ref().is_some_and(|s| s.0 == 3))
            .map(|c| c.0)
    }
}

/// The fight past anchor B as AH.8's opening starts it (the stick toward
/// `Enemy_Chompy(2)` 10 steps), then an eruption every 60 steps while any
/// of the four stands, the pad at rest after; once Norticus' scene has
/// ended and he's gone, the stick held 60 steps to see the hero walk;
/// `steps` steps in all.
fn fight(run: &mut Run, mode: Mode, steps: u32) -> Fight {
    let names = [
        CHOMPIES[0],
        CHOMPIES[1],
        CHOMPIES[2],
        CHOMPIES[3],
        NORTICUS,
        "Spike_Gate_Wooden_Template(1)",
        "Setup_001_Controller",
        "Norticus_intro_Camera_Target",
    ];
    let mut out = Fight::default();
    let mut last: BTreeMap<&str, State> = BTreeMap::new();
    let mut scene = None;
    let mut ended: Option<u32> = None;
    // The stick held once Norticus is gone, not before: his walk to the
    // cave waits on where the hero stands.
    let mut walk_from: Option<u32> = None;
    for k in 0..steps {
        let at = run.hero().pivot;
        let alive = CHOMPIES
            .iter()
            .any(|n| ours(run, n).is_some_and(|s| s.state != 3));
        let mut pad = Pad {
            connected: true,
            ..Pad::default()
        };
        if k < 10 {
            pad.stick = stick_for(run.bench.view, heading_to(at, [1.797, -20.974]), 1.0);
        } else if alive && (k - 10) % 60 < 6 {
            pad.actions[1] = true;
        } else if let Some(from) = walk_from
            && k - from < 60
        {
            pad.stick = stick_for(run.bench.view, heading_to(at, [5.0, -12.0]), 1.0);
        }
        run.bench.pads[0] = pad;
        let s = step(run, mode);
        assert!(s.is_empty() || mode == Mode::Play);
        let cs = global_int(run, "Cutscene State");
        if scene != Some(cs) {
            out.scene.push((k, cs));
            if scene == Some(1) && cs == 0 && ended.is_none() && !alive {
                ended = Some(k);
            }
            scene = Some(cs);
        }
        if ended.is_some()
            && walk_from.is_none()
            && ours(run, NORTICUS).is_some_and(|s| s.state == 3)
        {
            walk_from = Some(k);
            out.freed = Some((run.hero().pivot, run.hero().pivot));
        }
        if let Some(from) = walk_from
            && k - from == 60
            && let Some((was, _)) = out.freed
        {
            out.freed = Some((was, run.hero().pivot));
        }
        for n in names {
            let now = ours(run, n).map(|s| (s.state, s.behaviour));
            if last.get(n) != Some(&now) {
                out.changes.push((k, n, now.clone()));
                last.insert(n, now);
            }
        }
    }
    out
}

/// The level's templates the route's scripts clone that the TV doesn't
/// draw (openspec AG.239), with why; every other template cloned on the
/// way draws ([`route_clones_drawn`]).
const ROUTE_UNDRAWN: [(&str, &str); 9] = [
    (
        "Boulder_Titan_Pebble_01(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_02(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_03(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_04(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_05(1)",
        "on the boulder's attach nodes",
    ),
    ("Chest_Lock", "the chest's, a creature's"),
    ("Canon_Barrel_Marker", "invisible"),
    ("Dummy_Inviso", "invisible"),
    ("Turban_hat", "Norticus' hat: the fight's, a creature's"),
];

/// Every level template `run`'s scripts have cloned that has a model is
/// drawn on the TV (its model among the props part's spawn models, or a
/// skinned one [`SKINNED_SPAWNS`] names) unless [`ROUTE_UNDRAWN`] lists it.
fn route_clones_drawn(run: &Run) {
    let mut clones: BTreeMap<String, usize> = BTreeMap::new();
    for c in run.world.clones() {
        let a = run.world.actor(c).unwrap();
        let Some(t) = a.template.and_then(|t| run.world.template(t)) else {
            continue;
        };
        if t.obj.unit == LEVEL {
            *clones.entry(t.name.clone()).or_default() += 1;
        }
    }
    println!("the level's templates cloned on the route: {clones:?}");
    let props = run.bench.props.as_ref().unwrap();
    let skinned: Vec<String> = skinned_models().into_iter().map(|(p, _)| p).collect();
    for name in clones.keys() {
        let placed = run
            .files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == *name);
        // A template with no model (a hint's marker) has nothing to draw.
        if placed.is_some_and(|p| p.model.is_none()) {
            continue;
        }
        let drawn = run.files.placed.iter().any(|p| {
            p.obj.unit == LEVEL
                && p.name == *name
                && p.model.as_ref().is_some_and(|m| {
                    props
                        .level
                        .spawn_models
                        .iter()
                        .any(|&i| &props.level.models[i].path == m)
                        || (SKINNED_SPAWNS.contains(&name.as_str()) && skinned.contains(m))
                })
        });
        let undrawn = ROUTE_UNDRAWN.iter().find(|(n, _)| n == name);
        assert!(
            drawn != undrawn.is_some(),
            "{name}: drawn {drawn}, listed undrawn {undrawn:?}"
        );
    }
}

/// The route from the level's start, AH.8's plan driven on the TV's
/// listing in strict mode (any member the scripts reach unmodelled halts
/// it, AG.165's check past the push blocks), every step from the level's
/// own scripts: the ledge's eight breakables and the walkway's fence broken
/// to his eruptions, the walkway's three coins taken on the drop, the
/// cannon fired with North through its dialogue (the portal in adventure
/// mode from the join) and its ball breaking the rubble wall, the first
/// turtle pushed four times along the bridge, the second into the channel
/// and inactive there, a stepping stone, Setup_001's trigger and
/// Norticus', the six coins down into the pit, the pit's intro cutscene
/// played out and gone, and anchor B, its triggers fired as AH.8's were.
/// Then the fight from there: the four beaten, Norticus' scene ending from
/// his own script (the cutscene system's update in the game systems' tick)
/// with the hero free to walk again, the spike gate lowering once
/// Norticus' camera target is gone, and Norticus walking into the cave 121
/// steps on and going, as AH.10's 20 fights.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn from_the_start_to_anchor_b_and_the_open_gate_on_the_scripts_alone() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (mut run, _, _) = joined(Mode::Strict);
    assert_eq!(
        scripts::portal_mode(&mut run.vm).as_deref(),
        Some(scripts::ADVENTURE_MODE),
        "the join puts the portal in adventure mode"
    );
    let mut d = drive(run, Mode::Strict);
    for l in &d.looks {
        println!(
            "{} at step {}: hero ({:.2}, {:.2}, {:.2}), gold {}",
            l.label, l.step, l.hero.x, l.hero.y, l.hero.z, l.gold
        );
    }
    println!("the route in {:.1?}", started.elapsed());
    assert!(d.stuck.is_empty(), "every plan step done: {:?}", d.stuck);
    assert!(d.stops.is_empty());
    // The ledge route: both eruptions' eight breakables broken (AH.8's
    // first broke five, the rest the second; ours the first six).
    let ledge = d.look("ledge_e2");
    for name in LEDGE {
        assert_eq!(ledge.state(name), 3, "{name} broken");
    }
    // The walkway's fence: the barrel beside it to the first eruption, the
    // fence itself (32 health) by the second, as AH.8's.
    assert_eq!(d.look("fence_e1").state("barrel_b_intact(6)"), 3);
    assert_eq!(d.look("fence_e1").state("Fence Spawner_Template(1)"), 1);
    assert_eq!(d.look("fence_e2").state("Fence Spawner_Template(1)"), 3);
    // Off the walkway's edge on the three spinning coins' line.
    let drop = d.look("drop_after");
    for name in &COINS[..3] {
        assert_eq!(drop.state(name), 3, "{name} taken");
    }
    // The cannon fired with North, its ball through the rubble wall.
    let before = d.look("cannon_before");
    assert_eq!(before.state("Breakable_GateRubble_Wall(1)"), 1);
    assert_eq!(before.portal_mode.as_deref(), Some(scripts::ADVENTURE_MODE));
    assert_eq!(
        d.look("cannon_after").state("Breakable_GateRubble_Wall(1)"),
        3
    );
    // The cannon's pan, asleep until the walk to the strip crosses its
    // range, takes itself off on its first tick with the cannon fired, as
    // AH.8's census has it at the strip (awake 0.015 s, no behaviour).
    assert_eq!(d.look("cannon_after").state("Pan Hint_Show_Cannon"), 2);
    assert_eq!(d.look("strip").state("Pan Hint_Show_Cannon"), 3);
    // The first turtle pushed east four times, 15 ft a push, to 20.46.
    let t1 = d.look("turtle1_pushed");
    let at = t1.at("Push_Block_Template(1)");
    assert!(
        (at.x - 20.46).abs() < 0.05 && (at.z + 36.83).abs() < 0.05,
        "{at:?}"
    );
    assert_eq!(
        t1.behaviour("Push_Block_Template(1)"),
        Some("1. wait for push")
    );
    // The third pushed south-east off its place (AH.8's 150 steps of the
    // stick gave two pushes; ours three, the last into the channel), the
    // second north-east into the channel, dropped and inactive there.
    let t3 = d.look("turtle3_pushed").at("Push_Block_Template(3)");
    assert!(t3.distance(Vec3::new(27.81, 45.90, -25.04)) > 9.0, "{t3:?}");
    let t2 = d.look("turtle2_pushed");
    let at = t2.at("Push_Block_Template(2)");
    assert!(
        at.distance(Vec3::new(34.59, 41.84, -18.62)) < 0.05,
        "the stepping stone: {at:?}"
    );
    assert_eq!(
        t2.behaviour("Push_Block_Template(2)"),
        Some("special - push block inactive")
    );
    // Across it to Setup_001's trigger: the controller on to waiting for
    // the hero; then Norticus' trigger: the far-off Norticus off, the gate
    // and Norticus awake.
    assert_eq!(
        d.look("s001").behaviour("Setup_001_Controller"),
        Some("wait for minion")
    );
    let activate = d.look("activate");
    assert_eq!(activate.state("Norticus_First_Encounter(1)"), 3);
    assert_eq!(
        activate.behaviour("Spike_Gate_Wooden_Template(1)"),
        Some("wait to be opened")
    );
    assert_eq!(activate.behaviour(NORTICUS), Some("wait for approach"));
    // Down the six coins into the pit; the intro played out and gone, the
    // controller at its end.
    let landed = d.look("landed");
    for name in &COINS[3..] {
        assert_eq!(landed.state(name), 3, "{name} taken");
    }
    assert_eq!(landed.state("Eon_4_help_Norticus"), 3, "the intro gone");
    assert_eq!(
        landed.behaviour("Setup_001_Controller"),
        Some("in which you triumph and the script ends")
    );
    // While the intro plays the cutscene system holds the minions (its
    // control wrapper: under designer control, invulnerable), then lets go.
    println!(
        "the scene's steps on the route {}, his minion held {}",
        d.scene_steps, d.held_steps
    );
    assert!(d.scene_steps > 0, "the intro set Cutscene State");
    assert!(d.held_steps + 1 >= d.scene_steps, "held through the intro");
    assert!(!invulnerable(&d.run), "let go once it's over");
    // Anchor B, where AH.8's stood, its triggers fired as AH.8's were bar
    // the lush peak's (fired before AH.8's route began, out of this
    // route's reach); his gold the nine coins' and the breakables' loot.
    let anchor = d.look("anchor");
    let pose = Vec3::from_array(ANCHOR_B_POSE.0);
    assert!(
        anchor.hero.with_y(0.0).distance(pose.with_y(0.0)) < 0.15,
        "{:?}",
        anchor.hero
    );
    assert!(anchor.gold >= COINS.len() as u32, "{}", anchor.gold);
    let fired = d.run.world.fired_triggers(LEVEL);
    for t in ANCHOR_B_FIRED
        .iter()
        .filter(|t| **t != "trg_lvl_213_lush_peak")
    {
        assert!(fired.iter().any(|f| f == t), "{t} fired: {fired:?}");
    }
    // AG.150: each clip the scripts played on the tortoises and the
    // cannon's top along the route is in its model's palette, the one the
    // TV poses it by; the tortoises' pushes played some.
    let models = skinned_models();
    let palettes = level_palettes();
    println!(
        "played on the skinned clones: {:?}; out of another's palette: {:?}",
        d.played.slots, d.played.foreign
    );
    assert!(d.played.foreign.is_empty());
    assert!(d.played.slots.contains_key("2_Push_Block_Art_Sheep"));
    for (name, slots) in &d.played.slots {
        let model = d
            .run
            .files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == *name)
            .and_then(|p| p.model.clone())
            .unwrap_or_else(|| panic!("{name}'s model"));
        assert!(models.iter().any(|(p, _)| *p == model), "{name}: drawn");
        let palette = palettes
            .palettes
            .iter()
            .find(|p| p.owner == model)
            .unwrap_or_else(|| panic!("{name}: {model}'s palette"));
        for &slot in slots {
            assert!(
                palette.clip(slot as u16).is_some(),
                "{name}: slot {slot} isn't a clip of {model}'s palette"
            );
        }
    }
    // The fight from there, to the open gate.
    let f = fight(&mut d.run, Mode::Strict, 900);
    // What the route's scripts cloned of the level's templates, the TV
    // drawing each that draws (the cannon's ball, the rubble wall's
    // pieces, the spike gate's spikes and holes: openspec AG.239) or
    // listing why it doesn't, as `giants_start` holds the start's.
    route_clones_drawn(&d.run);
    // The gate's four holes run their own script (`Gate_Spikes_Hole.ai`,
    // AH.28's listing), which sets each to 60 % of its template at its start.
    let hole = d
        .run
        .files
        .placed
        .iter()
        .find(|p| p.obj.unit == LEVEL && p.name == "Gate_Spikes_Hole")
        .expect("the holes' template");
    let holes: Vec<[f32; 3]> = d
        .run
        .world
        .clones()
        .into_iter()
        .filter_map(|c| d.run.world.actor(c))
        .filter(|a| a.template == Some(hole.obj))
        .map(|a| a.scale)
        .collect();
    println!(
        "the holes' scales {holes:?}, their template's {:?}",
        hole.scale
    );
    assert_eq!(holes.len(), 4);
    for s in &holes {
        assert!(
            s.iter().all(|v| (v - 60.0).abs() < 1e-3),
            "{holes:?} from {:?}",
            hole.scale
        );
    }
    for (k, n, s) in &f.changes {
        println!("F0 + {k}: {n} {s:?}");
    }
    println!("Cutscene State: {:?}", f.scene);
    let last_gone = CHOMPIES
        .iter()
        .map(|n| f.gone(n).unwrap_or_else(|| panic!("{n} beaten")))
        .max()
        .unwrap();
    let go = f.first(NORTICUS, "cutscene go").expect("his scene");
    let advance = f.first(NORTICUS, "advance with player").expect("its end");
    let end = f.first(NORTICUS, "end").expect("into the cave");
    let gone = f.gone(NORTICUS).expect("he goes");
    let lower = f
        .first("Spike_Gate_Wooden_Template(1)", "lower spikes")
        .expect("the gate lowers");
    let target = f
        .gone("Norticus_intro_Camera_Target")
        .expect("his camera target taken off");
    println!(
        "last Chompy gone F0 + {last_gone}; 'cutscene go' F0 + {go}; 'advance with player' {} \
         on; the camera target off {}; 'lower spikes' {} after advancing; 'end' {} after; \
         gone {} after; freed {:?}; load after: {}",
        advance - go,
        target - go,
        lower - advance,
        end - advance,
        gone - end,
        f.freed,
        load()
    );
    // AH.10: 'Cutscene Go' the frame the last is gone.
    assert_eq!(go, last_gone);
    // The scene ends from his own script: the cutscene system's pan holds
    // 3 s (AH.10's 300 frames also wait on the line's voice, which plays
    // nothing here), its end setting `Cutscene State` back to 0.
    let scene_end = f
        .scene
        .iter()
        .find(|(k, cs)| *k > go && *cs == 0)
        .expect("Cutscene State back to 0")
        .0;
    assert_eq!(advance, scene_end, "'advance with player' as it ends");
    assert!((170..=310).contains(&(advance - go)), "{}", advance - go);
    // The gate's open set is the camera target his scene takes off; its
    // 0.5 s check lowers it then (AH.10: 9-15 frames on).
    assert_eq!(target, advance);
    assert!((1..=31).contains(&(lower - advance)), "{}", lower - advance);
    // Through the lowered gate into the cave: 'End' 121 steps on (AH.10's
    // 121 in all 20 fights), his fade 27-30 steps.
    assert_eq!(end - advance, 121);
    assert!((27..=30).contains(&(gone - end)), "{}", gone - end);
    // The scene's end frees the hero: the stick walks him again.
    let (was, now) = f.freed.expect("60 steps past Norticus' going");
    assert!(was.distance(now) > 2.0, "{was:?} -> {now:?}");
    // The members the route and the fight reach (strict: each one asked a
    // value of is registered), against AH.34's ceiling.
    let reached = d.run.vm.reached();
    let asked = reached
        .iter()
        .filter(|m| d.run.vm.uses().contains_key(*m))
        .count();
    let own = reached
        .iter()
        .filter(|m| guac_script::Builtin::of(**m).is_some())
        .count();
    println!(
        "members reached {} ({own} the VM's own), {asked} asked a value",
        reached.len()
    );
    // AH.34's ceiling as AG.28 raised it, which AG.165's count stands on.
    assert!(
        reached.len() <= 325,
        "past AG.28's ceiling: {}",
        reached.len()
    );
    // A dev aid: `AH28_MEMBERS_OUT=<file outside the tree>` writes every
    // member reached by name.
    if let Ok(out) = std::env::var("AH28_MEMBERS_OUT") {
        let mut all: Vec<String> = reached
            .iter()
            .map(|m| d.run.vm.config().members.name(*m))
            .collect();
        all.sort();
        std::fs::write(out, all.join("\n")).unwrap();
    }
}

fn route_dir() -> PathBuf {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR");
    PathBuf::from(root).join("BLUS30968/ah8-route-1792539821/route_b")
}

fn read_json(path: PathBuf) -> serde_json::Value {
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// AH.8's census at a label: each watched placement's brief and its
/// actor's place.
fn theirs(label: &str) -> BTreeMap<String, Seen> {
    let dir = route_dir();
    let census = read_json(dir.join(format!("census-{label}.json")));
    let actors = read_json(dir.join(format!("actors-{label}.json")));
    let at_of = |name: &str| -> Option<[f32; 3]> {
        let a = actors["actors"]
            .as_array()?
            .iter()
            .find(|a| a["name"] == name)?;
        let p = a["position"].as_array()?;
        Some([0, 1, 2].map(|i| p[i].as_f64().unwrap_or(0.0) as f32))
    };
    census["briefs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| {
            let name = b["name"].as_str()?;
            WATCHED.contains(&name).then(|| {
                (
                    name.to_owned(),
                    Seen {
                        state: b["activity_state"].as_u64().unwrap_or(0) as u8,
                        behaviour: b["behaviour"].as_str().map(str::to_ascii_lowercase),
                        at: at_of(name),
                    },
                )
            })
        })
        .collect()
}

/// A development probe: AH.8's route from the spawn in play mode, each
/// look's watched placements beside AH.8's census there (`=` alike, `!`
/// not; an asleep placement's behaviour isn't compared, the census reading
/// none), every script stopped in play, each plan step the driver couldn't
/// finish; with `AH28_FIGHT` the fight past anchor B.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.8's recordings; a probe"]
fn probe_the_route_from_the_start() {
    println!("load before: {}", load());
    let started = Instant::now();
    let (run, stops, found) = joined(Mode::Play);
    let mut d = drive(run, Mode::Play);
    d.stops
        .splice(0..0, stops.into_iter().map(|(k, h)| (k, usize::MAX, h)));
    for l in &d.looks {
        println!(
            "== {} at step {}: hero {} gold {} portal {:?}",
            l.label,
            l.step,
            fmt_at(Some(l.hero.to_array())),
            l.gold,
            l.portal_mode
        );
        let t = theirs(l.label);
        for name in WATCHED {
            let (o, t) = (l.seen.get(name), t.get(name));
            let alike = match (o, t) {
                (Some(o), Some(t)) => {
                    let far = match (o.at, t.at) {
                        (Some(a), Some(b)) => {
                            Vec3::from_array(a).distance(Vec3::from_array(b)) > 0.3
                        }
                        _ => false,
                    };
                    o.state == t.state && (o.state == 2 || o.behaviour == t.behaviour) && !far
                }
                _ => false,
            };
            println!(
                " {} {name}: ours {:?} {:?} {} | AH.8 {:?} {:?} {}",
                if t.is_none() {
                    " "
                } else if alike {
                    "="
                } else {
                    "!"
                },
                o.map(|s| s.state),
                o.and_then(|s| s.behaviour.clone()),
                fmt_at(o.and_then(|s| s.at)),
                t.map(|s| s.state),
                t.and_then(|s| s.behaviour.clone()),
                fmt_at(t.and_then(|s| s.at)),
            );
        }
    }
    if std::env::var("AH28_FIGHT").is_ok() {
        let f = fight(&mut d.run, Mode::Play, 3000);
        for (k, n, s) in &f.changes {
            println!("F0 + {k}: {n} {s:?}");
        }
        println!("Cutscene State: {:?}; freed {:?}", f.scene, f.freed);
    }
    println!("stops in play: {}", d.stops.len());
    for (k, i, h) in &d.stops {
        println!("  step {k} plan step {i}: {h}");
    }
    println!("stuck: {}", d.stuck.len());
    for s in &d.stuck {
        println!("  plan step {} {:?} at {:?}", s.index, s.step, s.at);
    }
    report(&d.run, &found);
    println!("woken off the whitelist: {:?}", d.run.world.woken);
    let mut met: BTreeMap<String, usize> = BTreeMap::new();
    for h in d.run.vm.inert() {
        let Some(a) = d.run.world.actor(h) else {
            continue;
        };
        if !a.active || a.removed {
            continue;
        }
        let script = a
            .obj
            .or(a.template)
            .and_then(|o| d.run.files.placed.iter().find(|p| p.obj == o))
            .and_then(|p| p.script.clone())
            .unwrap_or_default();
        *met.entry(script).or_default() += 1;
    }
    println!("inert and awake at the end, by script: {met:?}");
    println!(
        "end at step {}: hero {:?}; {:.1?}, load after: {}",
        d.run.steps,
        d.run.hero().pivot,
        started.elapsed(),
        load()
    );
}

/// A development probe: AH.8's `input.ron` replayed open loop from the
/// join, each step's stick read against the view the record's camera held
/// that frame (`recording.json`), and where the hero stands against the
/// record at each of the route's reads.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.8's recordings; a probe"]
fn probe_the_recorded_input_open_loop() {
    use guac_oracle::input::{Button as Pressed, InputScript};
    println!("load before: {}", load());
    let dir = route_dir();
    let script = InputScript::from_ron(&std::fs::read_to_string(dir.join("input.ron")).unwrap())
        .expect("AH.8's input");
    let records = read_json(dir.join("recording.json"));
    let records = records.as_array().unwrap();
    let route = read_json(dir.join("route-record.json"));
    let reads: Vec<(u64, String)> = route["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| {
            let step = s["step"].as_str()?;
            let label = step
                .strip_prefix("Read { label: \"")?
                .strip_suffix("\" }")?;
            Some((s["first_flip"].as_u64()?, label.to_owned()))
        })
        .collect();
    let first_flip = records[0]["flip"].as_u64().unwrap();
    let (mut run, _, _) = joined(Mode::Play);
    for r in records.iter().skip(1) {
        let flip = r["flip"].as_u64().unwrap();
        if let Some((_, label)) = reads.iter().find(|(f, _)| *f == flip) {
            let p = &r["hero"]["position"];
            let theirs = Vec3::new(
                p[0].as_f64().unwrap() as f32,
                p[1].as_f64().unwrap() as f32,
                p[2].as_f64().unwrap() as f32,
            );
            let ours = run.hero().pivot;
            println!(
                "{label} (flip {flip}): ours {} theirs {} apart {:.2} m",
                fmt_at(Some(ours.to_array())),
                fmt_at(Some(theirs.to_array())),
                ours.with_y(0.0).distance(theirs.with_y(0.0))
            );
        }
        let state = script
            .state_at(0, flip - first_flip - 1)
            .unwrap_or_default();
        let mut pad = Pad {
            stick: Stick::from_bytes(state.left.x, state.left.y),
            connected: true,
            ..Pad::default()
        };
        for (b, i) in [
            (Pressed::South, 0),
            (Pressed::West, 1),
            (Pressed::East, 2),
            (Pressed::North, 3),
        ] {
            pad.actions[i] = state.buttons.contains(&b);
        }
        run.bench.pads[0] = pad;
        run.bench.view = r["camera"]["rotation"][1].as_f64().unwrap() as f32;
        step(&mut run, Mode::Play);
    }
    println!("load after: {}", load());
}

/// An operand's path as names, for the probe's op listing.
fn operand_path(program: &guac_script::Program, o: Option<guac_script::OperandId>) -> String {
    use guac_script::{Elem, Ins, Operand};
    let Some(o) = o else { return String::new() };
    match &program.operands[o as usize] {
        Operand::Path {
            elems,
            literal,
            other,
            ..
        } => {
            let names: Vec<String> = elems
                .iter()
                .map(|e| match e {
                    Elem::Op(op) => match &program.ops[*op as usize] {
                        Ins::Parameter(p) => program.sym(p.name).to_owned(),
                        Ins::Variable { name, .. } => format!("${}", program.sym(*name)),
                        _ => format!("{:?}", program.op_info[*op as usize].class),
                    },
                    Elem::Named(a, b) => format!("{}.{}", program.sym(*a), program.sym(*b)),
                    other => format!("{other:?}"),
                })
                .collect();
            let other = other.map_or(String::new(), |o| {
                format!(" @{}", operand_path(program, Some(o)))
            });
            format!(
                "[{}{}{other}]",
                names.join("/"),
                literal.map_or(String::new(), |l| format!(" ={l:?}"))
            )
        }
        Operand::Expr { a, op, b } => format!(
            "({} {op:?} {})",
            operand_path(program, *a),
            operand_path(program, *b)
        ),
    }
}

/// A development probe: the ops `AH28_OPS` names (`<script suffix>#<pos>`,
/// comma-separated) as the linked program holds them.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_ops() {
    let (run, _) = run_on_with(Mode::Play, Setup::default());
    let program = run.vm.program_arc();
    for spec in std::env::var("AH28_OPS").unwrap_or_default().split(',') {
        let Some((suffix, pos)) = spec.split_once('#') else {
            continue;
        };
        let pos: usize = pos.parse().unwrap();
        for s in program.scripts.iter().filter(|s| s.name.ends_with(suffix)) {
            let Some(&op) = s.ops.get(pos) else { continue };
            let ins = &program.ops[op as usize];
            let text = match ins {
                guac_script::Ins::World { operands, words } => format!(
                    "World {} {:?}",
                    operands
                        .iter()
                        .map(|(n, o)| format!("{n}={}", operand_path(&program, *o)))
                        .collect::<Vec<_>>()
                        .join(" "),
                    words
                ),
                guac_script::Ins::SetValue { lhs, rhs } => format!(
                    "SetValue {} = {}",
                    operand_path(&program, *lhs),
                    operand_path(&program, *rhs)
                ),
                other => format!("{other:?}"),
            };
            println!(
                "{} #{pos}: {:?} {text}",
                s.name, program.op_info[op as usize].class
            );
        }
    }
}

/// A development probe: the tags the level's file gives the placements
/// `AH28_TAGS` names (comma-separated), each with its operand as a path.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_tags() {
    let (run, _) = run_on_with(Mode::Play, Setup::default());
    let program = run.vm.program_arc();
    for name in std::env::var("AH28_TAGS").unwrap_or_default().split(',') {
        let Some(p) = run.files.placed.iter().find(|p| p.name == name) else {
            println!("{name}: no such placement");
            continue;
        };
        let tags = program.tags.get(&p.obj).cloned().unwrap_or_default();
        println!("{name} ({:?}): {} tags", p.obj, tags.len());
        for t in tags {
            let operand = t.value.map(|o| (o, program.operands[o as usize].clone()));
            println!(
                "  {} {:?} = {} {operand:?}",
                program.sym(t.name),
                t.decl,
                operand_path(&program, t.value)
            );
        }
    }
}

/// A development probe for AH.108: the level's collision triangles AH.8's
/// route stood on, by material. Each record of `recording.json` the
/// oracle read supported (contact support 2) has a ray cast 2 m down from
/// 0.5 m above his pivot through the level's own collision (no props); the
/// triangle it meets and its material are tallied, records and distinct
/// triangles, and where each material first and last stood along the route.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.8's recordings; a probe"]
fn probe_the_routes_surfaces() {
    use guac_motion::seam::PhysicsSeam;
    let (run, _) = run_on_with(Mode::Play, Setup::default());
    let records = read_json(route_dir().join("recording.json"));
    let mut by: BTreeMap<u8, (usize, std::collections::BTreeSet<u32>, u64, u64)> = BTreeMap::new();
    let (mut stood, mut missed) = (0, 0);
    let mut misses = Vec::new();
    for r in records.as_array().unwrap() {
        if r["contacts"]["support"].as_u64() != Some(2) {
            continue;
        }
        stood += 1;
        let p = &r["hero"]["position"];
        let at = Vec3::new(
            p[0].as_f64().unwrap() as f32,
            p[1].as_f64().unwrap() as f32,
            p[2].as_f64().unwrap() as f32,
        );
        let flip = r["flip"].as_u64().unwrap();
        match run
            .bench
            .world
            .ray(at + Vec3::Y * 0.5, Vec3::NEG_Y, 2.0, &[])
            .and_then(|h| Some((h.material?, h.triangle?)))
        {
            Some((m, t)) => {
                let e = by.entry(m).or_insert((0, Default::default(), flip, flip));
                e.0 += 1;
                e.1.insert(t);
                e.3 = flip;
            }
            None => {
                missed += 1;
                misses.push((flip, at));
            }
        }
    }
    println!("{stood} supported records, {missed} with no level triangle under them");
    for (flip, at) in &misses {
        println!(
            "  no triangle under flip {flip} at {}",
            fmt_at(Some(at.to_array()))
        );
    }
    for (m, (n, tris, first, last)) in &by {
        println!(
            "material {m}: {n} records on {} triangles, flips {first}-{last}",
            tris.len()
        );
    }
}
