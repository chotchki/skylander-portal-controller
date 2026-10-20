//! One extractor per fight metric (openspec AH.6, design decisions 17 and
//! 18), each run unchanged over the oracle's trace and the headless runner's,
//! so the two sides differ only in what they recorded. Which behaviour, clip
//! or variable marks what (the throw, the lunge, a break) is a rule's to say,
//! since AH.9 and AH.10 measure it; the extractors only read cells by name.
//!
//! - [`chain`]: press → throw → ball → burst → break → coin, a link per throw.
//! - [`states`]: each actor's behaviours with their entry stamps.
//! - [`bites`]: a bite is the lunge's hit, so one armour turned away (no
//!   health lost) still counts; its value is the health the target lost.
//! - [`pushes`]: a knockback's length, from speeds over AG.8's [`Motion`].
//! - [`pickups`] and [`gains`]: orbs and coins, collected or not, and each
//!   change to the hero's experience and gold.
//! - [`breaks`]: what broke, in order, and the coins each dropped (the drop
//!   count over a break order).
//! - [`totals`] and [`logs`]: the run's totals (AF.10's shape) and its event
//!   logs, one per actor ([`super::log`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::log::{ActorEvent, ActorLogs};
use super::ticks::{Cue, FightTrace, HERO, Stamp, Track};
use crate::MetricError;
use crate::input::Button;
use crate::motion::{Motion, Sample};
use crate::totals::Totals;

/// An actor by its trace name, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    pub at: Stamp,
}

fn named(trace: &FightTrace, name: &str, tick: usize) -> Named {
    Named {
        name: name.to_string(),
        at: trace.stamp(tick),
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn ground(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// Where an actor was at `tick`, or its last place before it if it's gone.
fn place(track: &Track<'_>, tick: usize) -> Option<[f64; 3]> {
    track
        .at(tick)
        .or_else(|| tick.checked_sub(1).and_then(|t| track.at(t)))
        .map(|a| a.position)
}

/// The ticks `button` goes down at: held there and not the tick before, both
/// ticks having the pad.
pub fn presses(trace: &FightTrace, button: Button) -> Vec<usize> {
    trace
        .ticks()
        .windows(2)
        .enumerate()
        .filter(|(_, w)| match (&w[0].buttons, &w[1].buttons) {
            (Some(before), Some(now)) => now.contains(&button) && !before.contains(&button),
            _ => false,
        })
        .map(|(k, _)| k + 1)
        .collect()
}

/// What breaks and what it drops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakRule {
    /// The kinds that break (barrels, pots, gourds).
    pub kinds: Vec<String>,
    /// What marks one broken: its health at 0, a behaviour, or its leaving.
    pub broken: Cue,
    /// The kind of what it drops; a drop is a child of the broken one.
    pub coin: String,
}

/// A breakable at the tick its `broken` cue was entered, with the coins
/// whose parent it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub breakable: String,
    pub at: Stamp,
    pub tick: usize,
    pub coins: Vec<Named>,
}

/// Every break in the order they happened (by name within a tick), each with
/// the coins it spawned: the drop count over the break order is how many
/// have one ([`drop_count`]).
pub fn breaks(trace: &FightTrace, rule: &BreakRule) -> Vec<Break> {
    let mut out: Vec<Break> = trace
        .tracks()
        .filter(|t| rule.kinds.iter().any(|k| k == t.kind()))
        .filter_map(|t| {
            let tick = *t.entries(&rule.broken).first()?;
            let coins = trace
                .tracks()
                .filter(|c| c.kind() == rule.coin && c.parent() == Some(t.name()))
                .map(|c| named(trace, c.name(), c.first()))
                .collect();
            Some(Break {
                breakable: t.name().to_string(),
                at: trace.stamp(tick),
                tick,
                coins,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| a.breakable.cmp(&b.breakable))
    });
    out
}

pub fn drop_count(breaks: &[Break]) -> usize {
    breaks.iter().filter(|b| !b.coins.is_empty()).count()
}

/// From the attack button to the coin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainRule {
    pub attack: Button,
    /// The hero entering his throw.
    pub throw: Cue,
    /// The projectile's kind, a child of the hero.
    pub ball: String,
    /// The ball bursting ([`Cue::Gone`] when bursting removes it).
    pub burst: Cue,
    pub breaks: BreakRule,
    /// How far from the burst a break counts as the ball's doing, metres.
    pub reach: f64,
    /// How long after the burst, game seconds.
    pub window: f64,
}

/// One throw's chain, as far as it got.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// The last press of the attack button at or before the throw.
    pub press: Option<Stamp>,
    pub throw: Stamp,
    pub ball: Option<Named>,
    pub burst: Option<Stamp>,
    pub broke: Option<Named>,
    pub coins: Vec<Named>,
}

/// A link per throw: the first ball the hero spawned from the throw on (and
/// before the next), its burst, the first break within `window` and `reach`
/// of it that no earlier ball took, and that break's coins.
pub fn chain(trace: &FightTrace, rule: &ChainRule) -> Vec<Link> {
    let Some(hero) = trace.track(HERO) else {
        return Vec::new();
    };
    let throws = hero.entries(&rule.throw);
    let presses = presses(trace, rule.attack);
    let mut balls: Vec<Track<'_>> = trace
        .tracks()
        .filter(|t| t.kind() == rule.ball && t.parent() == Some(HERO) && t.spawned())
        .collect();
    balls.sort_by_key(|b| (b.first(), b.name().to_string()));
    let all_breaks = breaks(trace, &rule.breaks);
    let mut ball_taken = vec![false; balls.len()];
    let mut break_taken = vec![false; all_breaks.len()];
    let mut out = Vec::with_capacity(throws.len());
    for (i, &t) in throws.iter().enumerate() {
        let next = throws.get(i + 1).copied().unwrap_or(usize::MAX);
        let press = presses
            .iter()
            .rev()
            .find(|&&p| p <= t)
            .map(|&p| trace.stamp(p));
        let mut link = Link {
            press,
            throw: trace.stamp(t),
            ball: None,
            burst: None,
            broke: None,
            coins: Vec::new(),
        };
        let ball = (0..balls.len())
            .find(|&k| !ball_taken[k] && balls[k].first() >= t && balls[k].first() < next);
        if let Some(k) = ball {
            ball_taken[k] = true;
            let b = &balls[k];
            link.ball = Some(named(trace, b.name(), b.first()));
            if let Some(&burst) = b.entries(&rule.burst).first() {
                link.burst = Some(trace.stamp(burst));
                let at = place(b, burst);
                let found = (0..all_breaks.len()).find(|&j| {
                    let br = &all_breaks[j];
                    let near = trace
                        .track(&br.breakable)
                        .and_then(|bt| place(&bt, br.tick))
                        .zip(at)
                        .is_some_and(|(p, q)| distance(p, q) <= rule.reach);
                    !break_taken[j]
                        && br.tick >= burst
                        && br.at.time - trace.stamp(burst).time <= rule.window
                        && near
                });
                if let Some(j) = found {
                    break_taken[j] = true;
                    link.broke = Some(Named {
                        name: all_breaks[j].breakable.clone(),
                        at: all_breaks[j].at,
                    });
                    link.coins = all_breaks[j].coins.clone();
                }
            }
        }
        out.push(link);
    }
    out
}

/// A behaviour, stamped at the first tick it was read in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub state: String,
    pub at: Stamp,
}

/// Each behaviour an actor enters, in order: a change from the last one
/// read (an unreadable tick changes nothing), and for one spawned in play
/// its first. One standing from F0 starts in its behaviour there.
pub fn entries(trace: &FightTrace, track: &Track<'_>) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut last: Option<&str> = None;
    for (t, a) in track.states() {
        let Some(now) = a.behaviour.as_deref() else {
            continue;
        };
        let entered = match last {
            None => track.spawned(),
            Some(before) => before != now,
        };
        if entered {
            out.push(Entry {
                state: now.to_string(),
                at: trace.stamp(t),
            });
        }
        last = Some(now);
    }
    out
}

/// [`entries`] for every actor of `kind`, by name (each Chompy's states).
pub fn states(trace: &FightTrace, kind: &str) -> BTreeMap<String, Vec<Entry>> {
    trace
        .tracks()
        .filter(|t| t.kind() == kind)
        .map(|t| (t.name().to_string(), entries(trace, &t)))
        .collect()
}

/// Where in a lunge the hit falls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Hit {
    /// The first tick the attacker's pivot is within this many metres of
    /// the target's across the ground.
    Reach(f64),
    /// The first tick a script variable of the attacker's changes (its own
    /// record of the hit).
    Changes(String),
}

/// What a bite is. The health lost is read across the hit's own tick, so
/// the hit's marker has to land in the tick the target's health drops: one
/// a frame ahead of it reads every bite as turned away (AH.10 picks it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiteRule {
    /// The biters' kind.
    pub attacker: String,
    /// The lunge: a bite is its hit, at most one per lunge.
    pub lunge: Cue,
    /// The bitten, by name.
    pub target: String,
    pub hit: Hit,
    /// The target's health variable.
    pub health: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bite {
    pub attacker: String,
    pub at: Stamp,
    /// The health the target lost at the hit (every bite landing that tick
    /// together); `None` when it wasn't read either side of it.
    pub drop: Option<f64>,
    /// The bites landing on the target at that tick, this one included.
    pub shared: usize,
}

impl Bite {
    /// Armour turned it away: it hit and took nothing.
    pub fn turned_away(&self) -> bool {
        self.drop == Some(0.0)
    }
}

/// Every bite in tick order (by attacker within a tick).
pub fn bites(trace: &FightTrace, rule: &BiteRule) -> Vec<Bite> {
    let Some(target) = trace.track(&rule.target) else {
        return Vec::new();
    };
    let health = |t: usize| target.at(t).and_then(|a| a.vars.get(&rule.health).copied());
    let mut found: Vec<(usize, String)> = Vec::new();
    for attacker in trace.tracks().filter(|t| t.kind() == rule.attacker) {
        for (start, end) in attacker.spans(&rule.lunge) {
            let hit = (start..=end).find(|&t| match &rule.hit {
                Hit::Reach(metres) => attacker
                    .at(t)
                    .zip(target.at(t))
                    .is_some_and(|(a, b)| ground(a.position, b.position) <= *metres),
                Hit::Changes(var) => {
                    let read = |t: usize| attacker.at(t).and_then(|a| a.vars.get(var).copied());
                    t.checked_sub(1)
                        .and_then(read)
                        .zip(read(t))
                        .is_some_and(|(before, now)| before != now)
                }
            });
            if let Some(t) = hit {
                found.push((t, attacker.name().to_string()));
            }
        }
    }
    found.sort();
    found
        .iter()
        .map(|(t, attacker)| Bite {
            attacker: attacker.clone(),
            at: trace.stamp(*t),
            drop: t
                .checked_sub(1)
                .and_then(health)
                .zip(health(*t))
                .map(|(before, now)| before - now),
            shared: found.iter().filter(|(u, _)| u == t).count(),
        })
        .collect()
}

/// A run of steps an actor moves at or over a speed: a knockback, where no
/// walk goes that fast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Push {
    pub actor: String,
    /// The start of its first step.
    pub at: Stamp,
    /// Game seconds, the steps' own.
    pub seconds: f64,
    /// Across the ground, metres.
    pub distance: f64,
}

/// Which actors' pushes count, and the ground speed (m/s) a step needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushRule {
    pub kinds: Vec<String>,
    pub speed: f64,
}

/// The pushes of one actor, read through AG.8's [`Motion`] (a step's speed
/// is its distance over its game time; a step with no game time, the logic
/// stopped, is skipped).
pub fn pushes_of(
    trace: &FightTrace,
    track: &Track<'_>,
    speed: f64,
) -> Result<Vec<Push>, MetricError> {
    let mut ticks = Vec::new();
    let mut samples: Vec<Sample> = Vec::new();
    for (t, a) in track.states() {
        let time = trace.ticks()[t].time;
        if samples.last().is_some_and(|s| s.time >= time) {
            continue;
        }
        ticks.push(t);
        samples.push(Sample {
            time,
            position: a.position,
            heading: a.heading,
            stick: [0.0; 2],
            camera: None,
        });
    }
    if samples.len() < 2 {
        return Ok(Vec::new());
    }
    let motion = Motion::new(samples)?;
    let s = motion.samples();
    let mut out = Vec::new();
    let mut k = 0;
    while k + 1 < s.len() {
        if motion.ground_speed(k) < speed {
            k += 1;
            continue;
        }
        let start = k;
        while k + 1 < s.len() && motion.ground_speed(k) >= speed {
            k += 1;
        }
        out.push(Push {
            actor: track.name().to_string(),
            at: trace.stamp(ticks[start]),
            seconds: s[k].time - s[start].time,
            distance: motion.distance(s[start].time, s[k].time),
        });
    }
    Ok(out)
}

/// Every push of the rule's kinds, by actor then time.
pub fn pushes(trace: &FightTrace, rule: &PushRule) -> Result<Vec<Push>, MetricError> {
    let mut out = Vec::new();
    for t in trace
        .tracks()
        .filter(|t| rule.kinds.iter().any(|k| k == t.kind()))
    {
        out.extend(pushes_of(trace, &t, rule.speed)?);
    }
    Ok(out)
}

/// What gets collected and by whom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickupRule {
    pub kinds: Vec<String>,
    /// The variable holding what one's worth.
    pub worth: String,
    /// The collector, by name.
    pub collector: String,
    /// One gone within this many metres of the collector, at its last tick,
    /// was collected; one gone further off expired.
    pub reach: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pickup {
    pub actor: String,
    pub kind: String,
    /// When it appeared; `None` for one standing from F0 (a placed coin).
    pub spawned: Option<Stamp>,
    /// Its worth as first read.
    pub worth: Option<f64>,
    pub collected: Option<Stamp>,
}

/// Every orb and coin, by name.
pub fn pickups(trace: &FightTrace, rule: &PickupRule) -> Vec<Pickup> {
    let collector = trace.track(&rule.collector);
    trace
        .tracks()
        .filter(|t| rule.kinds.iter().any(|k| k == t.kind()))
        .map(|t| {
            let collected = t.gone().filter(|_| {
                let last = t.last();
                collector
                    .as_ref()
                    .and_then(|c| c.at(last))
                    .zip(t.at(last))
                    .is_some_and(|(c, a)| distance(c.position, a.position) <= rule.reach)
            });
            Pickup {
                actor: t.name().to_string(),
                kind: t.kind().to_string(),
                spawned: t.spawned().then(|| trace.stamp(t.first())),
                worth: t
                    .states()
                    .find_map(|(_, a)| a.vars.get(&rule.worth).copied()),
                collected: collected.map(|g| trace.stamp(g)),
            }
        })
        .collect()
}

/// A variable's change against the tick before it was last read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gain {
    pub var: String,
    pub at: Stamp,
    pub amount: f64,
}

/// Each tick `var` of `actor` differs from the tick before (both read):
/// the experience and gold each pickup adds, as the game holds them.
pub fn gains(trace: &FightTrace, actor: &str, var: &str) -> Vec<Gain> {
    let Some(track) = trace.track(actor) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut last: Option<f64> = None;
    for (t, a) in track.states() {
        let Some(&now) = a.vars.get(var) else {
            continue;
        };
        if let Some(before) = last
            && now != before
        {
            out.push(Gain {
                var: var.to_string(),
                at: trace.stamp(t),
                amount: now - before,
            });
        }
        last = Some(now);
    }
    out
}

/// A kind beaten when its health variable reaches 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beaten {
    pub kind: String,
    pub health: String,
}

impl Beaten {
    fn cue(&self) -> Cue {
        Cue::AtMost {
            var: self.health.clone(),
            value: 0.0,
        }
    }
}

/// Every metric's rule, for [`logs`] and [`totals`]; what's `None` or empty
/// isn't extracted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub chain: Option<ChainRule>,
    pub breaks: Option<BreakRule>,
    pub bites: Option<BiteRule>,
    pub pushes: Option<PushRule>,
    pub pickups: Option<PickupRule>,
    /// The hero's variables whose changes count (experience, gold).
    #[serde(default)]
    pub gains: Vec<String>,
    #[serde(default)]
    pub beaten: Vec<Beaten>,
}

/// One log per actor: its spawn; each behaviour entered (`state <name>`);
/// the hero's presses (`press <Button>`) and throws; a ball's burst; a
/// break; a creature beaten; a biter's bites and a pushed actor's pushes;
/// and its leaving, `pickup` when collected, else `gone`.
pub fn logs(trace: &FightTrace, rules: &Rules) -> Result<ActorLogs, MetricError> {
    let mut logs: BTreeMap<String, Vec<ActorEvent>> = BTreeMap::new();
    let event = |kind: String, at: Stamp| ActorEvent::new(kind, at.time, at.frame);
    let collected: BTreeMap<String, Option<Stamp>> = rules
        .pickups
        .as_ref()
        .map(|r| {
            pickups(trace, r)
                .into_iter()
                .map(|p| (p.actor, p.collected))
                .collect()
        })
        .unwrap_or_default();
    for t in trace.tracks() {
        let log = logs.entry(t.name().to_string()).or_default();
        if t.spawned() {
            log.push(event("spawn".into(), trace.stamp(t.first())));
        }
        for e in entries(trace, &t) {
            log.push(event(format!("state {}", e.state), e.at));
        }
        for b in &rules.beaten {
            if b.kind == t.kind()
                && let Some(&k) = t.entries(&b.cue()).first()
            {
                log.push(event("beaten".into(), trace.stamp(k)));
            }
        }
        if let Some(g) = t.gone() {
            let kind = match collected.get(t.name()) {
                Some(Some(_)) => "pickup",
                _ => "gone",
            };
            log.push(event(kind.into(), trace.stamp(g)));
        }
    }
    let mut push = |name: &str, kind: String, at: Stamp| {
        logs.entry(name.to_string())
            .or_default()
            .push(event(kind, at));
    };
    if let Some(hero) = trace.track(HERO) {
        let ticks = trace.ticks();
        for k in 1..ticks.len() {
            if let (Some(before), Some(now)) = (&ticks[k - 1].buttons, &ticks[k].buttons) {
                for b in now.difference(before) {
                    push(HERO, format!("press {b:?}"), trace.stamp(k));
                }
            }
        }
        if let Some(rule) = &rules.chain {
            for t in hero.entries(&rule.throw) {
                push(HERO, "throw".into(), trace.stamp(t));
            }
            if rule.burst != Cue::Gone {
                for ball in trace.tracks().filter(|b| b.kind() == rule.ball) {
                    if let Some(&t) = ball.entries(&rule.burst).first() {
                        push(ball.name(), "burst".into(), trace.stamp(t));
                    }
                }
            }
        }
    }
    if let Some(rule) = &rules.breaks {
        for b in breaks(trace, rule) {
            push(&b.breakable, "break".into(), b.at);
        }
    }
    if let Some(rule) = &rules.bites {
        for b in bites(trace, rule) {
            push(&b.attacker, "bite".into(), b.at);
        }
    }
    if let Some(rule) = &rules.pushes {
        for p in pushes(trace, rule)? {
            push(&p.actor, "push".into(), p.at);
        }
    }
    for log in logs.values_mut() {
        log.sort_by_key(|e| e.frame);
    }
    ActorLogs::new(logs)
}

/// The run's totals in AF.10's shape: each of `gains`' variables (the
/// hero's last value less his first), bites and bites turned away, pushes,
/// what's collected and its worth per kind, the beaten per kind, breaks and
/// drops. Values are whole; a fraction rounds to the nearest.
pub fn totals(trace: &FightTrace, rules: &Rules) -> Result<Totals, MetricError> {
    let mut out = Totals::new();
    let whole = |v: f64| v.round() as i64;
    if let Some(hero) = trace.track(HERO) {
        for var in &rules.gains {
            let mut read = hero.states().filter_map(|(_, a)| a.vars.get(var).copied());
            if let Some(first) = read.next() {
                let last = read.last().unwrap_or(first);
                out.insert(var.clone(), whole(last - first));
            }
        }
    }
    if let Some(rule) = &rules.bites {
        let b = bites(trace, rule);
        out.insert("bites".into(), b.len() as i64);
        out.insert(
            "bites turned away".into(),
            b.iter().filter(|b| b.turned_away()).count() as i64,
        );
    }
    if let Some(rule) = &rules.pushes {
        out.insert("pushes".into(), pushes(trace, rule)?.len() as i64);
    }
    if let Some(rule) = &rules.pickups {
        let all = pickups(trace, rule);
        for kind in &rule.kinds {
            let got: Vec<&Pickup> = all
                .iter()
                .filter(|p| &p.kind == kind && p.collected.is_some())
                .collect();
            out.insert(format!("collected {kind}"), got.len() as i64);
            out.insert(
                format!("worth {kind}"),
                whole(got.iter().filter_map(|p| p.worth).sum()),
            );
        }
    }
    for b in &rules.beaten {
        let n = trace
            .tracks()
            .filter(|t| t.kind() == b.kind && !t.entries(&b.cue()).is_empty())
            .count();
        out.insert(format!("beaten {}", b.kind), n as i64);
    }
    if let Some(rule) = &rules.breaks {
        let all = breaks(trace, rule);
        out.insert("breaks".into(), all.len() as i64);
        out.insert("drops".into(), drop_count(&all) as i64);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fight::log::{Judge, LogReference};
    use crate::fight::oracle::tests::{NOMINAL, chained, item, record};
    use crate::fight::oracle::{ChainItem, Names, fight_trace};
    use crate::fight::ticks::tests::{actor, ticks};
    use crate::fight::ticks::{Actor, SpawnRule, Tick};
    use crate::recording::{Frame, Recording};

    fn with(mut a: Actor, f: impl FnOnce(&mut Actor)) -> Actor {
        f(&mut a);
        a
    }

    fn hero(x: f64, health: f64) -> Actor {
        with(actor(0, HERO, Some(HERO), x), |a| {
            a.vars.insert("health".into(), health);
        })
    }

    fn chompy(id: u64, n: u32, x: f64, behaviour: &str) -> Actor {
        with(
            actor(id, "chompy", Some(&format!("Enemy_Chompy({n})")), x),
            |a| a.behaviour = Some(behaviour.into()),
        )
    }

    fn bite_rule(hit: Hit) -> BiteRule {
        BiteRule {
            attacker: "chompy".into(),
            lunge: Cue::Behaviour("Lunge".into()),
            target: HERO.into(),
            hit,
            health: "health".into(),
        }
    }

    #[test]
    fn a_turned_away_bite_counts() {
        // Chompy 1 lunges in from 4 m at ticks 2-4 (reaching the hero at 3)
        // and again at 8-9; armour turns the first away (290 stays 290), the
        // second takes 12. Chompy 2 lunges at 8-9 too, 3 m off: never close.
        let health = [
            290.0, 290.0, 290.0, 290.0, 290.0, 290.0, 290.0, 290.0, 278.0, 278.0,
        ];
        let c1 = [4.0, 4.0, 3.0, 1.0, 1.0, 4.0, 4.0, 4.0, 1.0, 1.0];
        let state = |t: usize| match t {
            2..=4 | 8..=9 => "Lunge",
            _ => "Battle",
        };
        let trace = FightTrace::new(
            ticks(
                (0..10)
                    .map(|t| {
                        vec![
                            hero(0.0, health[t]),
                            chompy(1, 1, c1[t], state(t)),
                            chompy(2, 2, -3.0, if t >= 8 { "Lunge" } else { "Battle" }),
                        ]
                    })
                    .collect(),
            ),
            &[],
        )
        .unwrap();
        let found = bites(&trace, &bite_rule(Hit::Reach(1.5)));
        let summary: Vec<(&str, u32, Option<f64>, bool)> = found
            .iter()
            .map(|b| (b.attacker.as_str(), b.at.frame, b.drop, b.turned_away()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Enemy_Chompy(1)", 3, Some(0.0), true),
                ("Enemy_Chompy(1)", 8, Some(12.0), false)
            ]
        );
        // By the biter's own record: a hit counter it bumps at the hit.
        let counted = FightTrace::new(
            trace
                .ticks()
                .iter()
                .map(|t| Tick {
                    actors: t
                        .actors
                        .iter()
                        .map(|a| {
                            with(a.clone(), |a| {
                                if a.id == 1 {
                                    let hits = [0, 0, 0, 1, 1, 1, 1, 1, 2, 2][t.frame as usize];
                                    a.vars.insert("hits".into(), f64::from(hits));
                                }
                            })
                        })
                        .collect(),
                    ..t.clone()
                })
                .collect(),
            &[],
        )
        .unwrap();
        assert_eq!(
            bites(&counted, &bite_rule(Hit::Changes("hits".into()))),
            found
        );
        let rules = Rules {
            bites: Some(bite_rule(Hit::Reach(1.5))),
            ..Rules::default()
        };
        let totals = totals(&trace, &rules).unwrap();
        assert_eq!((totals["bites"], totals["bites turned away"]), (2, 1));
        let logs = logs(&trace, &rules).unwrap();
        let kinds: Vec<&str> = logs
            .log("Enemy_Chompy(1)")
            .iter()
            .map(|e| e.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            ["state Lunge", "bite", "state Battle", "state Lunge", "bite"]
        );
    }

    #[test]
    fn two_bites_in_one_tick_share_the_health_lost() {
        let trace = FightTrace::new(
            ticks(vec![
                vec![
                    hero(0.0, 290.0),
                    chompy(1, 1, 1.0, "Battle"),
                    chompy(2, 2, -1.0, "Battle"),
                ],
                vec![
                    hero(0.0, 266.0),
                    chompy(1, 1, 1.0, "Lunge"),
                    chompy(2, 2, -1.0, "Lunge"),
                ],
            ]),
            &[],
        )
        .unwrap();
        let found = bites(&trace, &bite_rule(Hit::Reach(1.5)));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|b| b.shared == 2 && b.drop == Some(24.0)));
    }

    fn spawn_rules() -> Vec<SpawnRule> {
        vec![
            SpawnRule {
                kind: "lava_ball".into(),
                parents: vec![HERO.into()],
                radius: 2.0,
            },
            SpawnRule {
                kind: "coin".into(),
                parents: vec!["barrel".into()],
                radius: 2.0,
            },
            SpawnRule {
                kind: "orb".into(),
                parents: vec!["chompy".into()],
                radius: 2.0,
            },
        ]
    }

    fn chain_rule() -> ChainRule {
        ChainRule {
            attack: Button::South,
            throw: Cue::Clip("Eruptor_Throw".into()),
            ball: "lava_ball".into(),
            burst: Cue::Gone,
            breaks: BreakRule {
                kinds: vec!["barrel".into()],
                broken: Cue::AtMost {
                    var: "health".into(),
                    value: 0.0,
                },
                coin: "coin".into(),
            },
            reach: 1.5,
            window: 0.1,
        }
    }

    #[test]
    fn the_chain_runs_from_the_press_to_the_coin() {
        // South down at 2, the throw at 4, the ball out at 5 and flying 1 m a
        // tick to burst at 10 by the barrel 6 m off, the barrel broken at 11
        // and a coin out of it at 12. South again at 12, a second throw at 14
        // lobbing a ball that bursts at 20, 5 m short of a second barrel that
        // something else breaks at 20, dropping nothing.
        let ball_x = |t: usize| (t as f64 - 4.0).min(6.0);
        let trace = FightTrace::new(
            (0..22)
                .map(|t| {
                    let mut h = hero(0.0, 290.0);
                    if (4..=6).contains(&t) || (14..=16).contains(&t) {
                        h.clip = Some("Eruptor_Throw".into());
                    }
                    let mut actors = vec![
                        h,
                        with(actor(5, "barrel", Some("barrel_b_intact(1)"), 6.5), |a| {
                            a.vars
                                .insert("health".into(), if t >= 11 { 0.0 } else { 8.0 });
                        }),
                        with(actor(6, "barrel", Some("barrel_b_intact(4)"), 0.5), |a| {
                            a.position[2] = -70.0;
                            a.vars
                                .insert("health".into(), if t >= 20 { 0.0 } else { 8.0 });
                        }),
                    ];
                    if (5..10).contains(&t) {
                        actors.push(actor(40, "lava_ball", None, ball_x(t)));
                    }
                    if (15..20).contains(&t) {
                        actors.push(with(actor(41, "lava_ball", None, 0.5), |a| {
                            a.position[2] = -60.0 - (t as f64 - 14.0);
                        }));
                    }
                    if t >= 12 {
                        actors.push(with(actor(50, "coin", None, 6.4), |a| {
                            a.vars.insert("worth".into(), 1.0);
                        }));
                    }
                    Tick {
                        frame: t as u32,
                        time: t as f64 / 60.0,
                        buttons: Some(if (2..=8).contains(&t) || (12..=13).contains(&t) {
                            BTreeSet::from([Button::South])
                        } else {
                            BTreeSet::new()
                        }),
                        mt: None,
                        actors,
                    }
                })
                .collect(),
            &spawn_rules(),
        )
        .unwrap();
        let links = chain(&trace, &chain_rule());
        assert_eq!(links.len(), 2);
        let first = &links[0];
        assert_eq!(first.press.map(|s| s.frame), Some(2));
        assert_eq!(first.throw.frame, 4);
        assert_eq!(
            first.ball.as_ref().map(|b| (b.name.as_str(), b.at.frame)),
            Some(("hero>lava_ball#1", 5))
        );
        assert_eq!(first.burst.map(|s| s.frame), Some(10));
        assert_eq!(
            first.broke.as_ref().map(|b| (b.name.as_str(), b.at.frame)),
            Some(("barrel_b_intact(1)", 11))
        );
        let coins: Vec<(&str, u32)> = first
            .coins
            .iter()
            .map(|c| (c.name.as_str(), c.at.frame))
            .collect();
        assert_eq!(coins, [("barrel_b_intact(1)>coin#1", 12)]);
        // The second throw's press is the last at or before it, and its ball
        // breaks nothing: the barrel broken then is 5 m off its burst.
        let second = &links[1];
        assert_eq!(second.press.map(|s| s.frame), Some(12));
        assert_eq!(
            second.ball.as_ref().map(|b| b.name.as_str()),
            Some("hero>lava_ball#2")
        );
        assert_eq!(
            (second.burst.map(|s| s.frame), &second.broke),
            (Some(20), &None)
        );
        // The break order and its drop count.
        let all = breaks(&trace, &chain_rule().breaks);
        let order: Vec<(&str, u32, usize)> = all
            .iter()
            .map(|b| (b.breakable.as_str(), b.at.frame, b.coins.len()))
            .collect();
        assert_eq!(
            order,
            [("barrel_b_intact(1)", 11, 1), ("barrel_b_intact(4)", 20, 0)]
        );
        assert_eq!(drop_count(&all), 1);
        let rules = Rules {
            chain: Some(chain_rule()),
            breaks: Some(chain_rule().breaks),
            ..Rules::default()
        };
        let logs = logs(&trace, &rules).unwrap();
        let kinds = |a: &str| -> Vec<String> {
            logs.log(a)
                .iter()
                .map(|e| format!("{} {}", e.frame, e.kind))
                .collect()
        };
        assert_eq!(
            kinds(HERO),
            ["2 press South", "4 throw", "12 press South", "14 throw"]
        );
        assert_eq!(kinds("hero>lava_ball#1"), ["5 spawn", "10 gone"]);
        assert_eq!(kinds("barrel_b_intact(1)"), ["11 break"]);
        assert_eq!(kinds("barrel_b_intact(1)>coin#1"), ["12 spawn"]);
        let totals = totals(&trace, &rules).unwrap();
        assert_eq!((totals["breaks"], totals["drops"]), (2, 1));
    }

    /// The review's case: a barrel that leaves the trace as it breaks, in the
    /// tick the ball bursts, its coin out that same tick. The break is the
    /// ball's (a burst's own tick counts), and the coin is the barrel's,
    /// found where it stood the tick before.
    #[test]
    fn a_barrel_gone_in_the_bursts_own_tick_is_the_balls_and_parents_its_coin() {
        let trace = FightTrace::new(
            (0..10)
                .map(|t| {
                    let mut h = hero(0.0, 290.0);
                    if (1..=2).contains(&t) {
                        h.clip = Some("Eruptor_Throw".into());
                    }
                    let mut actors = vec![h];
                    if t < 6 {
                        actors.push(actor(5, "barrel", Some("barrel_b_intact(1)"), 4.5));
                    }
                    if (2..6).contains(&t) {
                        actors.push(actor(40, "lava_ball", None, t as f64 - 1.0));
                    }
                    if t >= 6 {
                        actors.push(actor(50, "coin", None, 4.4));
                    }
                    Tick {
                        frame: t as u32,
                        time: t as f64 / 60.0,
                        buttons: Some(if t == 1 {
                            BTreeSet::from([Button::South])
                        } else {
                            BTreeSet::new()
                        }),
                        mt: None,
                        actors,
                    }
                })
                .collect(),
            &spawn_rules(),
        )
        .unwrap();
        let mut rule = chain_rule();
        rule.breaks.broken = Cue::Gone;
        let links = chain(&trace, &rule);
        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(
            (
                link.press.map(|s| s.frame),
                link.throw.frame,
                link.burst.map(|s| s.frame)
            ),
            (Some(1), 1, Some(6))
        );
        assert_eq!(
            link.broke.as_ref().map(|b| (b.name.as_str(), b.at.frame)),
            Some(("barrel_b_intact(1)", 6))
        );
        let coins: Vec<&str> = link.coins.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(coins, ["barrel_b_intact(1)>coin#1"]);
        assert_eq!(drop_count(&breaks(&trace, &rule.breaks)), 1);
    }

    #[test]
    fn orbs_and_coins_collected_and_what_they_add() {
        // Chompy 1 beaten at 3, its four orbs out at 4; three fly to the hero
        // (gone 1.25, 1.1 and 0.7 m from him at 8, 9 and 10, each adding 1
        // experience the tick it's gone), the fourth flies off and is gone
        // 9.3 m away at 9.
        let orb = |id: u64, x: f64| {
            with(actor(id, "orb", None, x), |a| {
                a.vars.insert("worth".into(), 1.0);
            })
        };
        let trace = FightTrace::new(
            ticks(
                (0..12)
                    .map(|t| {
                        let mut actors = vec![with(hero(0.0, 290.0), |a| {
                            let xp = [8, 9, 10].iter().filter(|&&g| t >= g).count();
                            a.vars.insert("experience".into(), xp as f64);
                        })];
                        if t < 6 {
                            actors.push(with(chompy(1, 1, 5.0, "Battle"), |a| {
                                a.vars
                                    .insert("health".into(), if t >= 3 { 0.0 } else { 1.0 });
                            }));
                        }
                        // Each orb out at 4 at 5.0-5.3 m, gone at `gone`.
                        for (id, gone, x0, step) in [
                            (60, 8, 5.0, -1.25),
                            (61, 9, 5.1, -1.0),
                            (62, 10, 5.2, -0.9),
                            (63, 9, 5.3, 1.0),
                        ] {
                            if (4..gone).contains(&t) {
                                actors.push(orb(id, x0 + (t as f64 - 4.0) * step));
                            }
                        }
                        actors
                    })
                    .collect(),
            ),
            &spawn_rules(),
        )
        .unwrap();
        let rule = PickupRule {
            kinds: vec!["orb".into()],
            worth: "worth".into(),
            collector: HERO.into(),
            reach: 1.5,
        };
        let got: Vec<(String, Option<u32>, Option<u32>)> = pickups(&trace, &rule)
            .into_iter()
            .map(|p| {
                (
                    p.actor,
                    p.spawned.map(|s| s.frame),
                    p.collected.map(|s| s.frame),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("Enemy_Chompy(1)>orb#1".to_string(), Some(4), Some(8)),
                ("Enemy_Chompy(1)>orb#2".to_string(), Some(4), Some(9)),
                ("Enemy_Chompy(1)>orb#3".to_string(), Some(4), Some(10)),
                ("Enemy_Chompy(1)>orb#4".to_string(), Some(4), None),
            ]
        );
        let xp: Vec<(u32, f64)> = gains(&trace, HERO, "experience")
            .iter()
            .map(|g| (g.at.frame, g.amount))
            .collect();
        assert_eq!(xp, [(8, 1.0), (9, 1.0), (10, 1.0)]);
        let rules = Rules {
            pickups: Some(rule),
            gains: vec!["experience".into()],
            beaten: vec![Beaten {
                kind: "chompy".into(),
                health: "health".into(),
            }],
            ..Rules::default()
        };
        let totals = totals(&trace, &rules).unwrap();
        assert_eq!(
            totals,
            Totals::from([
                ("beaten chompy".into(), 1),
                ("collected orb".into(), 3),
                ("experience".into(), 3),
                ("worth orb".into(), 3)
            ])
        );
        let logs = logs(&trace, &rules).unwrap();
        let kinds = |a: &str| -> Vec<String> {
            logs.log(a)
                .iter()
                .map(|e| format!("{} {}", e.frame, e.kind))
                .collect()
        };
        assert_eq!(kinds("Enemy_Chompy(1)"), ["3 beaten", "6 gone"]);
        assert_eq!(kinds("Enemy_Chompy(1)>orb#1"), ["4 spawn", "8 pickup"]);
        assert_eq!(kinds("Enemy_Chompy(1)>orb#4"), ["4 spawn", "9 gone"]);
    }

    #[test]
    fn states_are_stamped_where_they_change() {
        let unread = |a: Actor| with(a, |a| a.behaviour = None);
        let trace = FightTrace::new(
            ticks(vec![
                vec![chompy(1, 1, 0.0, "Asleep"), unread(chompy(3, 3, 4.0, "x"))],
                vec![chompy(1, 1, 0.0, "Asleep"), unread(chompy(3, 3, 4.0, "x"))],
                vec![
                    unread(chompy(1, 1, 0.0, "x")),
                    chompy(3, 3, 4.0, "Asleep"),
                    chompy(9, 9, 2.0, "Spawned"),
                ],
                vec![
                    chompy(1, 1, 0.0, "Asleep"),
                    chompy(3, 3, 4.0, "Asleep"),
                    chompy(9, 9, 2.0, "Spawned"),
                ],
                vec![
                    chompy(1, 1, 0.0, "Alerted"),
                    chompy(3, 3, 4.0, "Asleep"),
                    chompy(9, 9, 2.0, "Battle"),
                ],
            ]),
            &[],
        )
        .unwrap();
        let s = states(&trace, "chompy");
        let flat = |name: &str| -> Vec<(String, u32)> {
            s[name]
                .iter()
                .map(|e| (e.state.clone(), e.at.frame))
                .collect()
        };
        // Asleep from F0 is no entry, nor is it read again after a gap, nor
        // first read late for one standing from F0.
        assert_eq!(flat("Enemy_Chompy(1)"), [("Alerted".to_string(), 4)]);
        assert_eq!(flat("Enemy_Chompy(3)"), []);
        // One spawned in play enters its first behaviour.
        assert_eq!(
            flat("Enemy_Chompy(9)"),
            [("Spawned".to_string(), 2), ("Battle".to_string(), 4)]
        );
    }

    /// A pushed hero's O5 records: still for three, then 24.4 m/s across
    /// the ground for `steps` record steps (their frame times `deltas`),
    /// then still.
    fn pushed(deltas: &[f32]) -> Vec<Frame> {
        let all: Vec<f32> = [NOMINAL; 3]
            .iter()
            .chain(deltas)
            .chain(&[NOMINAL; 3])
            .copied()
            .collect();
        let mut x = 0.0f64;
        let mut out = Vec::new();
        for (k, &d) in all.iter().enumerate() {
            out.push(record(k as u64, d, x as f32, false));
            if (3..3 + deltas.len()).contains(&k) {
                x += 24.4 * f64::from(out[k].clock.step());
            }
        }
        out
    }

    fn push_trace(frames: &[Frame]) -> FightTrace {
        let recording = Recording::new(frames.to_vec()).unwrap();
        let empty = vec![Vec::<ChainItem>::new(); frames.len()];
        fight_trace(&recording, &chained(frames, empty), &Names::default(), &[]).unwrap()
    }

    #[test]
    fn a_push_lasts_its_game_time_a_hitch_included() {
        let rule = PushRule {
            kinds: vec![HERO.into()],
            speed: 12.0,
        };
        // Nine nominal steps, or six and a hitch held to three: 0.15 s of
        // game time either way, in 9 steps or 7.
        let plain = pushes(&push_trace(&pushed(&[NOMINAL; 9])), &rule).unwrap();
        let mut hitched = [NOMINAL; 7];
        hitched[3] = 0.2;
        let hitch = pushes(&push_trace(&pushed(&hitched)), &rule).unwrap();
        assert_eq!((plain.len(), hitch.len()), (1, 1));
        let n = f64::from(NOMINAL);
        assert!((plain[0].seconds - 9.0 * n).abs() < 1e-12);
        assert!((hitch[0].seconds - (6.0 * n + f64::from(NOMINAL * 3.0))).abs() < 1e-12);
        assert!((plain[0].seconds - hitch[0].seconds).abs() < 1e-8);
        assert!(
            (plain[0].distance - 24.4 * 0.15).abs() < 1e-5,
            "{}",
            plain[0].distance
        );
        assert!((plain[0].distance - hitch[0].distance).abs() < 1e-5);
        assert_eq!((plain[0].at.frame, hitch[0].at.frame), (3, 3));
    }

    /// An oracle replay: Chompy 1 and 2 alerted at the given game frames,
    /// a hitch at record `hitch` when given.
    fn replay(alert: [usize; 2], hitch: Option<usize>) -> FightTrace {
        let frames: Vec<Frame> = (0..12)
            .map(|k| {
                record(
                    k as u64,
                    if Some(k) == hitch { 0.2 } else { NOMINAL },
                    0.0,
                    false,
                )
            })
            .collect();
        let items = (0..12)
            .map(|k| {
                (0..2)
                    .map(|c| {
                        let mut i = item(
                            0x8000_1000 + c as u32 * 0x100,
                            "chompy",
                            None,
                            10.0 + c as f32,
                        );
                        i.placement = Some(format!("Enemy_Chompy({})", c + 1));
                        i.behaviour = Some(if k >= alert[c] { 0x20 } else { 0x10 });
                        i
                    })
                    .collect()
            })
            .collect();
        let names = Names {
            behaviours: BTreeMap::from([(
                "chompy".into(),
                BTreeMap::from([(0x10, "Asleep".into()), (0x20, "Alerted".into())]),
            )]),
            clips: BTreeMap::new(),
        };
        let recording = Recording::new(frames.clone()).unwrap();
        fight_trace(&recording, &chained(&frames, items), &names, &[]).unwrap()
    }

    #[test]
    fn replays_interleaving_differently_on_game_time_pass_and_one_actor_off_fails() {
        // Replay A: Chompy 1 alerted at frame 6 (game time 6 steps), Chompy 2
        // at 7. Replay B hitched at record 2 (three steps in one), so its
        // frame k from 3 on is k + 2 steps in: Chompy 2 at frame 4 (6 steps),
        // Chompy 1 at 5 (7). The order between them swaps and the frames
        // differ by up to 3, while each Chompy is alerted within a step of
        // itself.
        let a = logs(&replay([6, 7], None), &Rules::default()).unwrap();
        let b = logs(&replay([5, 4], Some(2)), &Rules::default()).unwrap();
        let first = |l: &ActorLogs, c: &str| l.log(c)[0].clone();
        let frames = |c: &str| (first(&a, c).frame, first(&b, c).frame);
        assert_eq!(
            (frames("Enemy_Chompy(1)"), frames("Enemy_Chompy(2)")),
            ((6, 5), (7, 4))
        );
        let reference = LogReference::from_runs(&[a.clone(), b]).unwrap();
        let judge = Judge {
            default: 1e-6,
            floors: BTreeMap::new(),
            horizon: None,
            ordered: Default::default(),
        };
        // A headless run at the nominal step, both alerted at frame 6: passes.
        let headless = |c2: usize| {
            let steps: Vec<f64> = (0..12)
                .map(|k| (0..k).fold(0.0, |t, _| t + f64::from(NOMINAL)))
                .collect();
            let alert = [6, c2];
            FightTrace::new(
                (0..12)
                    .map(|k| Tick {
                        frame: k as u32,
                        time: steps[k],
                        buttons: None,
                        mt: None,
                        actors: (0..2)
                            .map(|c| {
                                chompy(
                                    c as u64 + 1,
                                    c as u32 + 1,
                                    10.0 + c as f64,
                                    if k >= alert[c] { "Alerted" } else { "Asleep" },
                                )
                            })
                            .collect(),
                    })
                    .collect(),
                &[],
            )
            .unwrap()
        };
        let ok = logs(&headless(6), &Rules::default()).unwrap();
        let verdict = reference.check(&ok, &judge).unwrap();
        assert!(verdict.passed(), "{verdict:?}");
        // Chompy 2 alerted two steps late: it fails, Chompy 1 doesn't.
        let late = logs(&headless(9), &Rules::default()).unwrap();
        let verdict = reference.check(&late, &judge).unwrap();
        assert_eq!(verdict.failed(), ["Enemy_Chompy(2)"]);
    }
}
