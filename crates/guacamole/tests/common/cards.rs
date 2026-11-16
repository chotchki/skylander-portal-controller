//! A card game played headless (openspec AS.366): a player on the posted
//! pads, the game's own state read off the board's manager, the opponent's
//! rolls classified, and what the game leaves behind. The game is the
//! original's scripts on the VM (`card_game`'s module docs); nothing here
//! decides a rule, only what a player presses.

use std::collections::{BTreeMap, BTreeSet};

use guac_motion::stick::Stick;
use guac_script::{Generator, PlacementId, Value};
use guacamole::script_engine::Pad;

use super::Run;

/// The board's manager's script name, its clone's placement name.
pub const MANAGER: &str = "CardGameManager";

/// The manager's behaviours a player's pads answer.
pub const PICK_STONE: &str = "player turn - wait for player card selection";
pub const PICK_SQUARE: &str = "player turn - wait for player position selection";
/// The opponent's pick, whose rolls [`Picks`] counts.
pub const OPPONENT_PICK: &str = "opponent turn - pick location";
/// The manager's first behaviour of a game: where a seeded game is seeded.
pub const GAME_START: &str = "delay at start of game";
pub const WON: &str = "won";
pub const LOST: &str = "lost";

/// The running board manager, if a game is on.
pub fn manager(run: &Run) -> Option<PlacementId> {
    let mut pid = 0u32;
    let mut found = None;
    while let Some(h) = run.vm.handle_of(pid) {
        if !run.vm.removed(pid) && run.world.name(h) == MANAGER {
            found = Some(pid);
        }
        pid += 1;
    }
    found
}

/// What the board waits on now (the manager's behaviour), if a game is on.
pub fn board(run: &Run) -> Option<String> {
    manager(run).and_then(|m| run.vm.behaviour(m).map(str::to_owned))
}

/// A packed screen position's two halves (12.4 fixed point), the board's
/// squares' column and row.
pub fn square(v: Value) -> Option<(i32, i32)> {
    match v {
        Value::Screen(b) => Some((
            i32::from((b & 0xffff) as u16 as i16) / 16,
            i32::from((b >> 16) as u16 as i16) / 16,
        )),
        _ => None,
    }
}

fn local(run: &Run, m: PlacementId, name: &str) -> Option<Value> {
    run.vm.local(m, name).map(|v| run.vm.value(v))
}

fn field(run: &Run, v: Value, name: &str) -> Option<Value> {
    run.vm.field(v, name).map(|f| run.vm.value(f))
}

fn set_of(run: &Run, v: Value) -> Vec<Value> {
    run.vm
        .set(v)
        .map(|s| s.iter().map(|e| run.vm.value(*e)).collect())
        .unwrap_or_default()
}

fn number(run: &Run, v: Value, name: &str) -> f32 {
    field(run, v, name).and_then(|x| x.as_f32()).unwrap_or(0.0)
}

/// How a player picks: the naive player loses most games against any
/// opponent, the strong one wins most against a weak one, the weak one
/// loses nearly every game (a run of replays, each a seeded game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// The stone and the square the cursor rests on.
    Naive,
    /// The stone with the most pips, then every free square visited and the
    /// one where the board's own preview flips most (`cards to flip this
    /// move`, the script's count) taken.
    Strong,
    /// The stone with the fewest pips, then the square the preview flips
    /// least.
    Weak,
}

/// A player on pad 0: what it holds each step, by what the board waits on.
#[derive(Debug)]
pub struct Player {
    pub policy: Policy,
    cool: u32,
    seen: BTreeMap<(i32, i32), usize>,
    moves: u32,
    last: String,
    held: u32,
}

impl Player {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            cool: 0,
            seen: BTreeMap::new(),
            moves: 0,
            last: String::new(),
            held: 0,
        }
    }

    /// The pad this step: released between presses, a press or a push of
    /// the stick every 20 steps (the board's stick timer reads one a
    /// quarter second at most).
    pub fn pad(&mut self, run: &Run) -> Pad {
        let mut pad = Pad {
            stick: Stick::new(0.0, 0.0),
            actions: [false; 4],
            connected: true,
        };
        let Some(m) = manager(run) else {
            return pad;
        };
        let b = run.vm.behaviour(m).unwrap_or("").to_owned();
        if b != self.last {
            self.seen.clear();
            self.moves = 0;
            self.last = b.clone();
            self.cool = 10;
        }
        if self.cool > 0 {
            self.cool -= 1;
            return pad;
        }
        // A tutorial's lines and the reports wait for accept.
        if b.contains("tutorial") || b == WON || b == LOST {
            pad.actions[0] = true;
            self.cool = 20;
            return pad;
        }
        match b.as_str() {
            PICK_STONE => {
                let hand = local(run, m, "game data ref")
                    .and_then(|g| field(run, g, "my hand"))
                    .map(|h| set_of(run, h))
                    .unwrap_or_default();
                let pips = |c: &Value| {
                    [
                        "north strength",
                        "east strength",
                        "south strength",
                        "west strength",
                    ]
                    .iter()
                    .map(|f| number(run, *c, f))
                    .sum::<f32>()
                };
                let want = match self.policy {
                    Policy::Naive => None,
                    Policy::Strong => hand
                        .iter()
                        .enumerate()
                        .max_by(|a, b| pips(a.1).total_cmp(&pips(b.1)))
                        .map(|(i, _)| i as i32),
                    Policy::Weak => hand
                        .iter()
                        .enumerate()
                        .min_by(|a, b| pips(a.1).total_cmp(&pips(b.1)))
                        .map(|(i, _)| i as i32),
                };
                let at = local(run, m, "card selection index")
                    .and_then(|v| v.as_i32())
                    .unwrap_or(0);
                match want {
                    Some(w) if w != at && self.moves < 8 => {
                        pad.stick = Stick::new(if at < w { 1.0 } else { -1.0 }, 0.0);
                        self.moves += 1;
                    }
                    _ => pad.actions[0] = true,
                }
                self.cool = 20;
            }
            PICK_SQUARE => {
                let at = local(run, m, "board selection location")
                    .and_then(square)
                    .unwrap_or((0, 0));
                if self.policy == Policy::Naive {
                    pad.actions[0] = true;
                    self.cool = 20;
                    return pad;
                }
                let flips = local(run, m, "cards to flip this move")
                    .map(|v| set_of(run, v).len())
                    .unwrap_or(0);
                self.seen.insert(at, flips);
                let taken: Vec<(i32, i32)> = local(run, m, "game data ref")
                    .and_then(|g| field(run, g, "all objects on board"))
                    .map(|s| set_of(run, s))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|o| field(run, *o, "board location").and_then(square))
                    .collect();
                let free: Vec<(i32, i32)> = (0..3)
                    .flat_map(|y| (0..3).map(move |x| (x, y)))
                    .filter(|q| !taken.contains(q))
                    .collect();
                let unseen = free.iter().find(|q| !self.seen.contains_key(q)).copied();
                let target = match unseen {
                    Some(q) if self.moves < 16 => q,
                    _ => self
                        .seen
                        .iter()
                        .filter(|(q, _)| free.contains(q))
                        .max_by_key(|(q, n)| {
                            let n = if self.policy == Policy::Weak {
                                usize::MAX - **n
                            } else {
                                **n
                            };
                            (n, std::cmp::Reverse(**q))
                        })
                        .map_or(at, |(q, _)| *q),
                };
                let done_looking = unseen.is_none() || self.moves >= 16;
                if at == target {
                    if done_looking {
                        pad.actions[0] = true;
                        self.cool = 20;
                    }
                    return pad;
                }
                let (dx, dy) = (target.0 - at.0, target.1 - at.1);
                pad.stick = match dx {
                    0 => Stick::new(0.0, -(dy.signum() as f32)),
                    _ => Stick::new(dx.signum() as f32, 0.0),
                };
                self.moves += 1;
                self.cool = 20;
            }
            _ => {
                self.held += 1;
            }
        }
        pad
    }
}

/// The opponent's rolls over the picks seen ([`Picks::note`]). Each tick of
/// `opponent turn - pick location` that restarts its delay timer is one
/// attempt: a draw against the call's best chance, and if that misses, one
/// against its second (the script's own rolls, `random 100` each). The
/// first roll's landing sets the chosen move to the list's first *stone*
/// (`best ai moves/first/card`), a structure of another definition than the
/// move the reference holds, which the VM's set reference refuses
/// (guac-script's crate docs, Variables, structures and sets): nothing is
/// chosen and the next tick attempts again. So an attempt that leaves no
/// move chosen is a landed first roll, and a pick's last attempt took the
/// list's second (the second roll landed) or its last.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Picks {
    /// Every attempt, and those whose first roll landed.
    pub attempts: u32,
    pub first_landed: u32,
    /// Of the picks over a full list (three moves): the second taken, the
    /// last taken.
    pub second: u32,
    pub last: u32,
    /// Each pick: its step, its list's length, the taken one's place in it
    /// and its attempts. The replay check compares these.
    pub trace: Vec<(u32, usize, Option<usize>, u32)>,
    was: Option<String>,
    timer: Option<Value>,
    tries: u32,
}

impl Picks {
    /// After a step: an attempt counted, a pick that just ended classified.
    pub fn note(&mut self, run: &Run) {
        let Some(m) = manager(run) else {
            self.was = None;
            return;
        };
        let now = run.vm.behaviour(m).map(str::to_owned);
        let chosen = local(run, m, "chosen ai move").filter(|c| matches!(c, Value::Struct(_)));
        if now.as_deref() == Some(OPPONENT_PICK) {
            let timer = local(run, m, "opponent move delay timer");
            if self.was.as_deref() == Some(OPPONENT_PICK) && timer != self.timer {
                self.attempts += 1;
                self.tries += 1;
                if chosen.is_none() {
                    self.first_landed += 1;
                }
            }
            self.timer = timer;
        }
        if self.was.as_deref() == Some(OPPONENT_PICK) && now.as_deref() != Some(OPPONENT_PICK) {
            let list = local(run, m, "best ai moves")
                .map(|v| set_of(run, v))
                .unwrap_or_default();
            let at = chosen.and_then(|c| list.iter().position(|e| *e == c));
            self.trace.push((run.steps, list.len(), at, self.tries));
            if list.len() == 3 && self.tries > 0 {
                match at {
                    Some(1) => self.second += 1,
                    Some(2) => self.last += 1,
                    _ => {}
                }
            }
            self.tries = 0;
        }
        if now.as_deref() != Some(OPPONENT_PICK) {
            self.timer = None;
        }
        self.was = now;
    }

    /// The first roll's landings, percent of its draws.
    pub fn first(&self) -> f64 {
        100.0 * f64::from(self.first_landed) / f64::from(self.attempts.max(1))
    }

    /// The second roll's landings, percent of the full lists' picks.
    pub fn second(&self) -> f64 {
        100.0 * f64::from(self.second) / f64::from((self.second + self.last).max(1))
    }

    /// The full lists' picks.
    pub fn count(&self) -> u32 {
        self.second + self.last
    }
}

/// Each manager behaviour's longest stay over a game, in steps, and the
/// order they came in.
#[derive(Debug, Default, Clone)]
pub struct Stays {
    pub longest: BTreeMap<String, u32>,
    pub order: Vec<(u32, String)>,
    at: Option<(String, u32)>,
}

impl Stays {
    pub fn note(&mut self, run: &Run) {
        let now = board(run);
        match (&self.at, now) {
            (Some((b, _)), Some(n)) if *b == n => {}
            (was, now) => {
                if let Some((b, from)) = was {
                    let stay = run.steps - from;
                    let e = self.longest.entry(b.clone()).or_default();
                    *e = (*e).max(stay);
                }
                if let Some(n) = &now {
                    self.order.push((run.steps, n.clone()));
                }
                self.at = now.map(|n| (n, run.steps));
            }
        }
    }

    /// The longest stay of a behaviour that isn't the player's own wait.
    pub fn longest_unpressed(&self) -> Option<(&String, u32)> {
        self.longest
            .iter()
            .filter(|(b, _)| !b.starts_with("player turn") && !b.contains("tutorial"))
            .map(|(b, n)| (b, *n))
            .max_by_key(|(_, n)| *n)
    }
}

/// A game's seed: the run's draws (the script's random rolls, the host's
/// MT19937, guac-script's `Generator`) restarted from `seed` as the board's
/// manager starts its game, so a game is the same to the step from the
/// same seed and the same start.
pub fn seed(run: &mut Run, seed: u32) {
    run.generator = Generator::seeded(seed);
}

/// What the board's set-up changes and its teardown should give back, read
/// off the scripts' own state: the game systems' minigame data, pause menu
/// macro and reward macro, the level's atmospheric zone, the music playing,
/// the portal's locks and minigame mode, the hero's own creature state and
/// every running placement outside the card game's streams by behaviour.
pub fn leftovers(run: &Run) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let program = run.vm.program();
    for g in [
        "Game Systems",
        "level control data",
        "Portal Control Data",
        "portal control data",
        "music data",
        "camera control data",
        "System Update Controls",
        "UI Systems",
        "cs control data",
    ] {
        let Some(id) = program.global_named(g) else {
            continue;
        };
        let Some(v) = run.vm.global_peek(id).map(|v| run.vm.value(v)) else {
            continue;
        };
        for (name, value) in run.vm.fields(v) {
            let value = run.vm.value(value);
            if matches!(
                value,
                Value::Int(_)
                    | Value::Float(_)
                    | Value::Object(_)
                    | Value::Null
                    | Value::Entity(_)
                    | Value::Named(..)
            ) {
                out.insert(format!("{g}.{name}"), format!("{value:?}"));
            }
        }
    }
    let streams: BTreeSet<u16> = run.world.card_streams.unit_list().into_iter().collect();
    let mut pid = 0u32;
    while let Some(h) = run.vm.handle_of(pid) {
        let in_streams = run
            .handles
            .iter()
            .any(|(o, hh)| *hh == h && streams.contains(&o.unit));
        if !run.vm.removed(pid) && !in_streams {
            let name = run.world.name(h);
            if !name.is_empty() && name != MANAGER {
                out.insert(
                    format!("placement {name} #{pid}"),
                    run.vm.behaviour(pid).unwrap_or("-").to_owned(),
                );
            }
        }
        pid += 1;
    }
    out
}

/// The keys whose values differ between `a` and `b` (in either only).
pub fn differ(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> Vec<String> {
    a.keys()
        .chain(b.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| format!("{k}: {:?} -> {:?}", a.get(k), b.get(k)))
        .collect()
}

/// The card game's streams' placements still running: those of the board
/// and the hand select by name (a stone's own, STARTUP-only, aside).
pub fn streams_running(run: &Run) -> Vec<String> {
    let units: BTreeSet<u16> = run
        .world
        .card_streams
        .units
        .iter()
        .filter(|(a, _)| guacamole::card_game::STREAMS.contains(a))
        .map(|(_, u)| *u)
        .collect();
    let mut out = Vec::new();
    let mut pid = 0u32;
    while let Some(h) = run.vm.handle_of(pid) {
        let ours = run
            .handles
            .iter()
            .any(|(o, hh)| *hh == h && units.contains(&o.unit))
            || run.world.name(h) == MANAGER;
        if ours && !run.vm.removed(pid) {
            out.push(run.world.name(h));
        }
        pid += 1;
    }
    out
}

/// The hero put back after a game (openspec AS.481): the board loads as the
/// portal's replacement character stream, so the game's start takes his
/// minion off, and its teardown asks for his character again; once that's
/// owed ([`guacamole::card_game::hero_owed`]: no scene holding the
/// players) the harness answers it as the TV's swap answers a figure
/// put back on the portal, his file unloaded and joined again
/// ([`Run::deactivate`], [`Run::join`]). His minion then runs the spawn
/// sequence a join runs (under designer control until its clip ends,
/// [`free`]). Whether he was put back this step.
pub fn hero_back(run: &mut Run) -> bool {
    let off = run.minion.is_some_and(|m| run.vm.removed(m));
    if !guacamole::card_game::hero_owed(&run.vm, &run.world, off) {
        return false;
    }
    let _ = run.deactivate();
    run.join()
        .unwrap_or_else(|h| panic!("his join after the game: {h}"));
    run.world.card_streams.hero_returned();
    true
}

/// Steps until his minion's spawn sequence has let him go (his behaviour
/// past `disabled`: `Portal - Minion Spawn Sequence` holds a joined minion
/// under designer control while his spawn clip plays), at most `cap`; the
/// stops on the way. Whether he's free.
pub fn free(run: &mut Run, cap: u32, stops: &mut Vec<guac_script::Halt>) -> bool {
    for _ in 0..cap {
        if run
            .minion
            .is_some_and(|m| !run.vm.removed(m) && run.vm.behaviour(m) != Some("disabled"))
        {
            return true;
        }
        stops.extend(run.step_play().unwrap());
    }
    false
}

/// A full stick held on each of eight headings in turn, `steps` steps
/// each, from where he stands: each heading's distance walked. A spot is
/// the walk's to leave once one heading leaves it (a stick held into a
/// wall or a pirate walks nothing).
pub fn walk_off(run: &mut Run, steps: u32, stops: &mut Vec<guac_script::Halt>) -> Vec<f32> {
    let mut out = Vec::new();
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let from = run.hero().pivot;
        for _ in 0..steps {
            run.bench.pads[0] = guacamole::script_engine::Pad {
                stick: guac_motion::stick::Stick::new(a.sin(), a.cos()),
                actions: [false; 4],
                connected: true,
            };
            stops.extend(run.step_play().unwrap());
        }
        out.push(run.hero().pivot.distance(from));
    }
    run.bench.pads[0] = guacamole::script_engine::Pad::default();
    out
}

/// The board's two chances as the pirate's call set them, read off its
/// manager (`best move - 1 %`, `best move - 2 %`), once a game is on.
pub fn chances(run: &Run) -> Option<(i32, i32)> {
    let m = manager(run)?;
    let a = local(run, m, "best move - 1 %")?.as_i32()?;
    let b = local(run, m, "best move - 2 %")?.as_i32()?;
    Some((a, b))
}
