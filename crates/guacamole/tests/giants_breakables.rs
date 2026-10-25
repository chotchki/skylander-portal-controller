//! AH.21's disc tests: chapter 1's breakables on the VM over level 213 with
//! guacamole's host, headless (the shared run, [`common`]): the level's
//! archetype setups and `global.bld`'s destructibles system run as the
//! original's (the loot system's init listing every breakable, the game
//! systems' update making each one in range a target and shutting a
//! broken one down for its loot, the actor remover taking its debris), the
//! props standing in the seam by AG.25's rule, a broken one's solid gone.
//! Ignored by default; they read `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`) and, for the comparisons, AH.9's recordings under
//! `GUAC_CAPTURE_DIR`:
//!
//! `cargo test -p guacamole --release --test giants_breakables -- --ignored --nocapture --test-threads=1`
//!
//! `AH21_STATE_OUT=<file outside the tree>` has the ledge route's test
//! write the props the scripts took off, by name, as JSON: what AG.7's
//! reach is rerun on.

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::*;
use guac_motion::seam::{PhysicsSeam, Shape, Vec3};
use guac_motion::stick::Stick;
use guac_script::members::{
    CAMERA, COMBAT, CREATURES, FIGURE, HERO as HERO_MEMBERS, INIT_CHAIN, LOOP,
};
use guac_script::{Handle, MemberId, Mode, Obj, Value};
use guacamole::script_engine::{FEET_PER_METRE, Pad, heading_of};
use guacamole::scripts::{self, Rarity};

/// Anchor A (AH.7, `ah7-anchor-a-1792536443/a2_anchor`): Eruptor still at
/// the foot of the ledge route facing `barrel_b_intact(1)`, his pivot and
/// heading as its `record.json` reads them.
const ANCHOR_A: [f32; 3] = [31.08841, 51.46313, -72.9834];
const ANCHOR_A_FACING: f32 = 253.706;
/// His model's tilt and roll there, degrees, as AH.9's records hold his
/// `rotation` (the ground under him; AH.68's to pose on our side).
const ANCHOR_A_TILT: f32 = 8.288_624;
const ANCHOR_A_ROLL: f32 = 6.209_965;
/// Anchor A's loot counts (its `fight-anchor.json`, AH.4's reader).
const ANCHOR_A_LOOT: [(Rarity, f32); 3] = [
    (Rarity::Common, 0.824_279_5),
    (Rarity::Uncommon, 0.097_497),
    (Rarity::Rare, 0.570_767_6),
];
/// AH.9's pause-free replays from anchor A, ten a script.
const AH9_MAIN: &str = "BLUS30968/ah9-main-1792545846";
/// The pad's bits for South (attack 1) and West (attack 2) in a record.
const SOUTH: u64 = 0x40;
const WEST: u64 = 0x80;
/// Where our runs press: the step that reads the press as AH.9's press
/// record does (a record holds the pad its frame reads, the frame after
/// the record before it; our step k reads what it's given).
const PRESS: u32 = 30;
/// His idle's time at AH.9's press record (`lob-00`'s 0.5636 s; 0.560-0.570
/// over the 20 `lob` and `erupt` replays), its clip's period less what
/// remains: the state our step `PRESS - 1` leaves.
const AH9_PRESS_IDLE: f64 = 0.5636;
/// The ledge route's three breakables (AG.7's reach: the walkway's way up).
const LEDGE: [&str; 3] = ["barrel_b_intact(1)", "barrel_b_intact(4)", "pottery_1(8)"];

/// A run at anchor A: joined over the dump with the level's props in the
/// seam, set down at anchor A's pose with his idle where AH.9's replays
/// have it and the loot counts at its read.
fn at_anchor_a() -> Run {
    let (mut run, _) = joined_with(Mode::Strict, Setup { props: true });
    to_anchor_a(&mut run);
    for (rarity, index) in ANCHOR_A_LOOT {
        assert!(scripts::set_loot_index(
            &mut run.vm,
            &run.world,
            rarity,
            index
        ));
    }
    run
}

/// Him set down at anchor A's pose as AH.9's records hold it: his idle
/// timed so the state our step `PRESS - 1` from here leaves holds it as
/// AH.9's press record does, and his posed hands leaned by the ground's
/// tilt and roll there while he stands on it (AH.50: the run poses his
/// hands each step and a lob leaves from one, so both move its burst;
/// upright, the hand stands 0.62 m off the oracle's, AH.68).
fn to_anchor_a(run: &mut Run) {
    run.put(Vec3::from_array(ANCHOR_A), ANCHOR_A_FACING);
    run.idle0 = AH9_PRESS_IDLE - f64::from(run.steps + PRESS) / 60.0;
    run.lean = Some((
        run.hero().pivot,
        lean_of([ANCHOR_A_TILT, ANCHOR_A_FACING, ANCHOR_A_ROLL]),
    ));
}

fn pad(actions: [bool; 4], stick: Stick) -> Pad {
    Pad {
        stick,
        actions,
        connected: true,
    }
}

/// Attack `n` from `at`, held 6 steps as AH.9's scripts hold a button 6
/// flips.
fn press(n: usize, at: u32, k: u32) -> Pad {
    let mut actions = [false; 4];
    actions[n] = (at..at + 6).contains(&k);
    pad(actions, Stick::CENTRED)
}

/// The destructibles system's lists as `global.bld`'s UI holds them: each
/// breakable it lists, by name, and whether it's among the active ones (a
/// target, woken by the system's update), not the inactive ones.
fn destructibles(run: &Run) -> Vec<(String, bool)> {
    let ui = run
        .files
        .placed
        .iter()
        .find(|p| p.script.as_deref().is_some_and(|s| s.ends_with("/UI.ai")))
        .map(|p| run.handles[&p.obj])
        .and_then(|h| run.vm.placement_of(h))
        .expect("the UI");
    let data = run
        .vm
        .local(ui, "destructibles system data")
        .map(|v| run.vm.value(v))
        .expect("the destructibles' data");
    let set = |v: Option<Value>| -> Vec<Value> {
        v.and_then(|v| run.vm.set(run.vm.value(v)).map(<[Value]>::to_vec))
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for d in set(run.vm.field(data, "destructibles")) {
        let d = run.vm.value(d);
        for (field, active) in [("active", true), ("inactive", false)] {
            for a in set(run.vm.field(d, field)) {
                if let Value::Entity(h) = run.vm.value(a) {
                    out.push((run.world.name(h), active));
                }
            }
        }
    }
    out
}

/// A piece of debris: its template's name, the step it was made, where and
/// its speed after that step's frame (ft/s), and the step a remove took it
/// off.
#[derive(Debug, Clone)]
struct Debris {
    name: String,
    born: u32,
    at: Vec3,
    speed: f32,
    gone: Option<u32>,
}

/// What a run's steps left, step by step: each breakable broken (its name,
/// the step, whether its solid was still in the seam after that step, where
/// it stood), each piece of debris, each coin dropped (the step, its
/// worth), each kind's count where it moved (the step, before, after).
#[derive(Debug, Default)]
struct Watch {
    breaks: Vec<(String, u32, bool, Vec3)>,
    debris: Vec<(Handle, Debris)>,
    coins: Vec<(u32, i32)>,
    counts: BTreeMap<&'static str, Vec<(u32, f32, f32)>>,
    seen: HashSet<Handle>,
    last: [f32; 2],
}

const KINDS: [(&str, Rarity); 2] = [("common", Rarity::Common), ("uncommon", Rarity::Uncommon)];

impl Watch {
    fn new(run: &Run) -> Self {
        Self {
            seen: run.world.clones().into_iter().collect(),
            last: KINDS.map(|(_, r)| scripts::loot_index(&run.vm, &run.world, r).unwrap()),
            ..Self::default()
        }
    }

    /// What step `k` left.
    fn after(&mut self, run: &Run, k: u32) {
        let props = run.bench.props.as_ref().unwrap();
        for p in &props.level.props {
            let taken = run.world.taken_off(Obj {
                unit: LEVEL,
                index: p.object,
            });
            if taken && !self.breaks.iter().any(|b| b.0 == p.name) {
                let standing = props.standing.contains(&p.object);
                let at = Vec3::from_array(p.position);
                self.breaks.push((p.name.clone(), k, standing, at));
            }
        }
        for c in run.world.clones() {
            if !self.seen.insert(c) {
                continue;
            }
            let a = run.world.actor(c).unwrap();
            let name = run.world.name(c);
            if name.contains("debris") {
                let speed = a.velocity.length();
                self.debris.push((
                    c,
                    Debris {
                        name,
                        born: k,
                        at: a.position,
                        speed,
                        gone: None,
                    },
                ));
            } else if name.contains("Coin") {
                self.coins.push((k, a.id));
            }
        }
        for (h, d) in &mut self.debris {
            if d.gone.is_none() && run.world.removed(*h) {
                d.gone = Some(k);
            }
        }
        for (i, (label, rarity)) in KINDS.into_iter().enumerate() {
            let now = scripts::loot_index(&run.vm, &run.world, rarity).unwrap();
            if now != self.last[i] {
                self.counts
                    .entry(label)
                    .or_default()
                    .push((k, self.last[i], now));
                self.last[i] = now;
            }
        }
    }

    /// What broke and when, for the order and the steps the tests check.
    fn broke(&self) -> Vec<(&str, u32)> {
        self.breaks
            .iter()
            .map(|(n, k, ..)| (n.as_str(), *k))
            .collect()
    }

    fn count(&self, kind: &str) -> Vec<(u32, f32, f32)> {
        self.counts.get(kind).cloned().unwrap_or_default()
    }
}

/// `steps` steps of `run` on the pads `script` gives (it may poke the run
/// first), and what they broke, made, dropped and counted.
fn drive(run: &mut Run, steps: u32, mut script: impl FnMut(u32, &mut Run) -> Pad) -> Watch {
    let mut w = Watch::new(run);
    for k in 0..steps {
        run.bench.pads[0] = script(k, run);
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        w.after(run, k);
    }
    w
}

/// One of AH.9's replays as its recordings hold it, by record: the press,
/// each breakable's break (the first record it's no longer listed), each
/// piece of debris's first and last record listed, each dropped coin's
/// first record and worth, each kind's count where it moved.
#[derive(Debug)]
struct Replay {
    label: String,
    press: usize,
    breaks: Vec<(String, usize)>,
    debris: Vec<(usize, usize)>,
    coins: Vec<(usize, i32)>,
    counts: BTreeMap<&'static str, Vec<(usize, f32, f32)>>,
}

fn capture_dir() -> PathBuf {
    PathBuf::from(
        env_or_dotenv("GUAC_CAPTURE_DIR")
            .expect("set GUAC_CAPTURE_DIR (env or .env.dev) to the captures"),
    )
}

/// A chained item as AH.9's reader keys it: its kind, its placement's name
/// (none for a spawned one), its address and serial.
type ItemKey = (String, Option<String>, u64, u64);

fn replay(dir: &Path, bit: u64) -> Replay {
    let json = |n: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(n)).unwrap()).unwrap()
    };
    let num = |v: &serde_json::Value| -> f32 {
        v.as_object()
            .and_then(|o| o.values().next())
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32
    };
    let rec = json("recording.json");
    let rec = rec.as_array().unwrap();
    let press = rec
        .iter()
        .position(|r| r["pad"]["digital2"].as_u64().is_some_and(|d| d & bit != 0))
        .expect("a press");
    let chain = json("chain-records.json");
    let chain = chain.as_array().unwrap();
    let mut life: HashMap<ItemKey, (usize, usize, i32)> = HashMap::new();
    for (k, r) in chain.iter().enumerate() {
        for it in r["items"].as_array().unwrap() {
            let key = (
                it["kind"].as_str().unwrap().to_owned(),
                it["placement"].as_str().map(str::to_owned),
                it["address"].as_u64().unwrap(),
                it["serial"].as_u64().unwrap_or(0),
            );
            let worth = it["vars"]["worth"].as_f64().unwrap_or(-1.0).round() as i32;
            let e = life.entry(key).or_insert((k, k, worth));
            e.1 = k;
        }
    }
    let n = chain.len();
    let mut breaks: Vec<(String, usize)> = life
        .iter()
        .filter(|(k, v)| k.0 == "breakable" && v.1 < n - 1)
        .map(|(k, v)| (k.1.clone().unwrap(), v.1 + 1))
        .collect();
    breaks.sort_by_key(|b| (b.1, b.0.clone()));
    let mut debris: Vec<(usize, usize)> = life
        .iter()
        .filter(|(k, _)| k.0 == "debris")
        .map(|(_, v)| (v.0, v.1))
        .collect();
    debris.sort();
    let mut coins: Vec<(usize, i32)> = life
        .iter()
        .filter(|(k, _)| k.0 == "coin" && k.1.is_none())
        .map(|(_, v)| (v.0, v.2))
        .collect();
    coins.sort();
    let frames = json("route-frames.json");
    let frames = frames.as_array().unwrap();
    let mut counts: BTreeMap<&'static str, Vec<(usize, f32, f32)>> = BTreeMap::new();
    for (label, field) in [("common", "loot"), ("uncommon", "loot_uncommon")] {
        for k in 1..frames.len() {
            let (a, b) = (num(&frames[k - 1][field]), num(&frames[k][field]));
            if a != b {
                counts.entry(label).or_default().push((k, a, b));
            }
        }
    }
    Replay {
        label: dir.file_name().unwrap().to_string_lossy().into_owned(),
        press,
        breaks,
        debris,
        coins,
        counts,
    }
}

/// AH.9's ten replays of `script` (`lob`, `erupt`), pressing `bit`.
fn replays(script: &str, bit: u64) -> Vec<Replay> {
    let root = capture_dir().join(AH9_MAIN);
    (0..10)
        .map(|i| replay(&root.join(format!("{script}-{i:02}")), bit))
        .collect()
}

/// Each broken thing's debris made the step it broke where it stood, three
/// a piece of its kind, flung off at 57-75 ft/s after their first frame
/// (the scripts' 60 plus a roll to 15 at 45° up, a frame of gravity on;
/// AH.9's 57-73 over their first frames) and gone 91 or 92 steps on (1.52
/// s, AH.9's 91-92 frames, each life one of AH.9's).
fn check_debris(w: &Watch, ah9: &[Replay]) {
    let lives: HashSet<usize> = ah9
        .iter()
        .flat_map(|r| r.debris.iter().map(|(a, b)| b - a + 1))
        .collect();
    println!("AH.9's debris listed for {lives:?} records");
    assert_eq!(w.debris.len(), 3 * w.breaks.len(), "{:?}", w.debris);
    for (name, k, _, at) in &w.breaks {
        let made: Vec<&Debris> = w
            .debris
            .iter()
            .map(|(_, d)| d)
            .filter(|d| d.born == *k && (d.at - *at).length() < 0.5)
            .collect();
        assert_eq!(made.len(), 3, "{name}: {made:?}");
        for d in made {
            assert!(same_kind(name, &d.name), "{name}: {d:?}");
            assert!((57.0..=75.01).contains(&d.speed), "{d:?}");
            let gone = d.gone.expect("gone");
            let life = gone - d.born;
            println!(
                "  {name}: {} at {:.1} ft/s, gone {life} steps on",
                d.name, d.speed
            );
            assert!((91..=92).contains(&life), "{d:?}");
            assert!(lives.contains(&(life as usize)), "{life} not AH.9's");
        }
    }
}

/// Debris of the kind a breakable leaves: a barrel's, a pot's, a gourd's.
fn same_kind(broken: &str, debris: &str) -> bool {
    let kind = |n: &str| {
        if n.starts_with("barrel") {
            "barrel"
        } else if n.starts_with("pottery") {
            "pottery"
        } else if n.starts_with("gourd") {
            "gourd"
        } else {
            "?"
        }
    };
    kind(broken) == kind(debris)
}

/// One Lava Lob from anchor A breaks the two barrels at the foot of the
/// ledge route the same step, as every one of AH.9's ten `lob` replays:
/// nothing else breaks; each solid is out of the seam that step; each
/// leaves three pieces of debris, gone 1.52 s on; the common count steps
/// twice by 0.6 from anchor A's 0.8242795 to AH.9's 0.024279475 and both
/// drop a coin worth 1 or 2; the destructibles system's lists hold every
/// other breakable in range of him active, the rest asleep. Our break comes
/// within AH.9's 33-34 steps after the press (his hand posed off his model,
/// AH.50, in anchor A's recorded lean; the lob aimed as AH.60's), the
/// burst's look at `barrel_b_intact(4)` passing the barrels' own solids as
/// the original's ray cast does. The count steps and the coins come the step
/// after the break, AH.9's the record after: the lob's hit lands in the
/// projectile manager's slice, after the game systems' update has run that
/// step (AH.59's order), so the destructibles system shuts the two down for
/// their loot the next; their debris gone 92 steps on, AH.9's 92 (`lob-04`'s
/// 91, its ball a frame shorter).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.9's recordings; run on the Mac"]
fn a_lob_breaks_the_ledge_routes_two_barrels_as_ah9s() {
    println!("load before: {}", load());
    let started = Instant::now();
    let ah9 = replays("lob", SOUTH);
    let mut run = at_anchor_a();
    let w = drive(&mut run, 260, |k, _| press(0, PRESS, k));
    println!(
        "ours: broke {:?}, coins {:?}, counts {:?}",
        w.broke(),
        w.coins,
        w.counts
    );
    for r in &ah9 {
        println!(
            "{}: press {}, broke {:?}, coins {:?}, counts {:?}",
            r.label, r.press, r.breaks, r.coins, r.counts
        );
        let names: Vec<&str> = r.breaks.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(names, ["barrel_b_intact(1)", "barrel_b_intact(4)"]);
        assert_eq!(r.breaks[0].1, r.breaks[1].1, "the same record");
        assert_eq!(r.coins.len(), 2);
    }
    let broke = w.broke();
    assert_eq!(
        broke.iter().map(|b| b.0).collect::<Vec<_>>(),
        ["barrel_b_intact(1)", "barrel_b_intact(4)"]
    );
    assert_eq!(broke[0].1, broke[1].1, "the same step");
    let k = broke[0].1;
    assert!(
        w.breaks.iter().all(|b| !b.2),
        "the solids gone at once: {:?}",
        w.breaks
    );
    // Each solid's out of the seam: a probe where it stood meets none; the
    // barrel the lob passed over stands.
    let props = run.bench.props.as_ref().unwrap();
    for p in props.level.props.iter() {
        let gone = broke.iter().any(|b| b.0 == p.name);
        if gone || p.name == "barrel_b_intact(3)" {
            assert_eq!(!gone, in_seam(&run, p.object), "{}", p.name);
        }
    }
    let ours_after = (k - PRESS) as usize;
    let theirs: Vec<usize> = ah9.iter().map(|r| r.breaks[0].1 - 1 - r.press).collect();
    println!("the break {ours_after} steps after the press; AH.9's {theirs:?}");
    let (lo, hi) = (*theirs.iter().min().unwrap(), *theirs.iter().max().unwrap());
    assert!(
        (lo..=hi).contains(&ours_after),
        "{ours_after} past {lo}..={hi}"
    );
    check_debris(&w, &ah9);
    let common = w.count("common");
    assert_eq!(common.len(), 1, "{common:?}");
    assert_eq!(
        (common[0].1, common[0].2),
        (0.824_279_5, 0.024_279_475),
        "two steps of 0.6"
    );
    for r in &ah9 {
        assert_eq!(
            r.counts["common"]
                .iter()
                .map(|c| (c.1, c.2))
                .collect::<Vec<_>>(),
            [(0.824_279_5, 0.024_279_475)],
            "{}",
            r.label
        );
    }
    assert!(w.count("uncommon").is_empty());
    // The system's lists by the activity rule: every breakable it lists
    // within its range of him (AG.7's) a target, the rest asleep with no
    // ground (the system shuts one down that way, a hypothesis), the two
    // broken in neither.
    let listed = destructibles(&run);
    let props = &run.bench.props.as_ref().unwrap().level;
    let hero = run.hero().pivot;
    let mut awake = 0;
    for (name, active) in &listed {
        let p = props.props.iter().find(|p| &p.name == name).expect(name);
        assert_eq!(*active, p.stands(hero), "{name}");
        awake += usize::from(*active);
        assert!(!broke.iter().any(|b| b.0 == name), "{name} broken");
    }
    println!("the destructibles listed: {}, {awake} active", listed.len());
    assert!(awake > 10 && listed.len() > awake);
    assert_eq!(w.coins.len(), 2, "{:?}", w.coins);
    assert!(
        w.coins
            .iter()
            .all(|c| c.0 == k + 1 && (1..=2).contains(&c.1)),
        "the step after the break: {:?}",
        w.coins
    );
    assert_eq!(common[0].0, k + 1, "the count the step after");
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// One Eruption from anchor A breaks the uncommon pot, the gourd and the
/// barrel its reach passes, in AH.9's order and on its steps from the press
/// (65, 69-70 and 72 over the ten `erupt` replays; a step late before
/// AH.54's lava ticked the step it spawns), each solid out of the seam the
/// step it breaks, three pieces of debris each gone 1.52 s on: the uncommon
/// count's rate of 1 leaves 0.097497 at 0.09749699 and drops a coin worth 3
/// or 7; the common count steps twice, 0.8242795 to 0.42427945 to
/// 0.024279475 as AH.9's, each dropping a coin worth 1 or 2, the step after
/// its break as AH.9's.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AH.9's recordings; run on the Mac"]
fn an_eruption_breaks_the_pot_the_gourd_and_the_barrel_as_ah9s() {
    println!("load before: {}", load());
    let started = Instant::now();
    let ah9 = replays("erupt", WEST);
    let mut run = at_anchor_a();
    let w = drive(&mut run, 300, |k, _| press(1, PRESS, k));
    println!(
        "ours: broke {:?}, coins {:?}, counts {:?}",
        w.broke(),
        w.coins,
        w.counts
    );
    let order = ["pottery_2(5)", "gourd_01(10)", "barrel_b_intact(1)"];
    let broke = w.broke();
    assert_eq!(broke.iter().map(|b| b.0).collect::<Vec<_>>(), order);
    assert!(w.breaks.iter().all(|b| !b.2), "{:?}", w.breaks);
    // Steps from the press to the step it's taken off: ours, and AH.9's
    // from its press record to the record before its break (the record
    // holds the state after its frame).
    for (i, name) in order.iter().enumerate() {
        let ours = (broke[i].1 - PRESS) as usize;
        let theirs: Vec<usize> = ah9
            .iter()
            .map(|r| {
                let b = r.breaks.iter().find(|b| b.0 == *name).expect(name);
                b.1 - 1 - r.press
            })
            .collect();
        println!("{name}: ours {ours} steps after the press, AH.9's {theirs:?}");
        let (lo, hi) = (*theirs.iter().min().unwrap(), *theirs.iter().max().unwrap());
        assert!((lo..=hi).contains(&ours), "{name}: {ours} past {lo}..={hi}");
    }
    check_debris(&w, &ah9);
    let unc = w.count("uncommon");
    assert_eq!(
        unc.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>(),
        [(0.097_497, 0.097_496_99)]
    );
    let common = w.count("common");
    assert_eq!(
        common.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>(),
        [(0.824_279_5, 0.424_279_45), (0.424_279_45, 0.024_279_475)]
    );
    for r in &ah9 {
        assert_eq!(r.counts["uncommon"][0].2, 0.097_496_99, "{}", r.label);
        assert_eq!(
            r.counts["common"]
                .iter()
                .map(|c| (c.1, c.2))
                .collect::<Vec<_>>(),
            [(0.824_279_5, 0.424_279_45), (0.424_279_45, 0.024_279_475)],
            "{}",
            r.label
        );
    }
    // A coin each, the step after its break as AH.9's (the record after).
    assert_eq!(w.coins.len(), 3, "{:?}", w.coins);
    for ((name, k, ..), (at, worth)) in w.breaks.iter().zip(&w.coins) {
        assert_eq!(*at, k + 1, "{name}");
        let worths: &[i32] = if name.starts_with("pottery_2") {
            &[3, 7]
        } else {
            &[1, 2]
        };
        assert!(worths.contains(worth), "{name}: {worth}");
    }
    for r in &ah9 {
        for (name, at) in &r.breaks {
            assert!(
                r.coins.iter().any(|c| c.0 == at + 1),
                "{}: {name}'s coin the record after",
                r.label
            );
        }
    }
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// Whether the prop `object`'s solid is in the run's seam: a small probe
/// at its own middle meets it.
fn in_seam(run: &Run, object: u32) -> bool {
    let props = run.bench.props.as_ref().unwrap();
    let p = props
        .level
        .props
        .iter()
        .find(|p| p.object == object)
        .unwrap();
    let Some(body) = &p.body else {
        return false;
    };
    run.bench
        .world
        .overlaps(&Shape::Sphere { radius: 0.05 }, body.pose, &[])
        .contains(&body.id)
}

/// He walks at `target` for `steps` steps, the stick full ahead with the
/// view along his way, re-aimed at it every step, so once there it turns
/// his walk round every step he crosses it, which holds him on the slope
/// there as the original's does (AG.90): the nearest his pivot came across
/// the ground, how high over `target` it stood then (m), whether the
/// level's own collision was what he stood on there, and the highest he
/// stood over the ground finder's point once within 0.1 m of it.
fn walk_at(run: &mut Run, target: Vec3, steps: u32) -> (f32, f32, bool, f32) {
    let mut nearest = (f32::MAX, 0.0, false);
    let mut highest: Option<f32> = None;
    for k in 0..steps {
        let to = target - run.hero().pivot;
        run.bench.view = heading_of(to);
        run.bench.pads[0] = pad([false; 4], Stick::new(0.0, 1.0));
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let p = run.hero().pivot;
        let across = ((p - target) * Vec3::new(1.0, 0.0, 1.0)).length();
        if std::env::var("AH21_WALK").is_ok() && k % 10 == 0 {
            println!("walk {k}: {p:.3} {across:.3}");
        }
        if across < 0.1 || highest.is_some() {
            let over = run.hero().footing.height.unwrap_or(f32::INFINITY);
            highest = Some(highest.map_or(over, |h| h.max(over)));
        }
        if across < nearest.0 {
            let below = run
                .bench
                .world
                .ray(p + Vec3::Y * 0.5, -Vec3::Y, 1.0, &[HERO_BODY]);
            let on_level = below.is_some_and(|h| {
                h.body.0 == guacamole::level_scene::LEVEL_BODIES && (h.point.y - p.y).abs() < 0.1
            });
            nearest = (across, p.y - target.y, on_level);
        }
    }
    (nearest.0, nearest.1, nearest.2, highest.unwrap_or(f32::NAN))
}

/// The ledge route opens (breakables' Up to the walkway): a lob from
/// anchor A breaks its two barrels; he walks 0.6 s toward `pottery_1(8)`
/// and, him turned to it, lobs twice: the second lob, from his right hand
/// (2.3 m up since AH.50 poses it), meets the level over the pot and
/// breaks nothing, the third, from his left, breaks it; the three solids
/// are out of the seam, and walked at where the first barrel lay, the stick
/// re-aimed there every step, he holds the ground there (AG.90), where with
/// it whole it stops him short of it on the ground (his controller climbed
/// onto it before AG.90; the oracle's check is AH.70's). The props the
/// scripts took off are
/// written where `AH21_STATE_OUT` says, for AG.7's reach.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_ledge_routes_three_break_and_he_walks_where_they_stood() {
    println!("load before: {}", load());
    let started = Instant::now();
    let mut whole = at_anchor_a();
    let (_, barrel) = whole.placed(LEDGE[0]);
    let (whole_across, whole_above, whole_on_level, _) = walk_at(&mut whole, barrel, 180);
    println!(
        "{} whole: he comes within {whole_across:.3} m of where it lies across the ground, {whole_above:.3} m above its pivot, on the level's collision {whole_on_level}",
        LEDGE[0]
    );
    // Whole, it stops him on the ground short of it. Before AG.90 he climbed
    // onto it (0.956 m over its pivot, AH.70): pressed against it off the
    // ground, his scripts never saw him off it and his walk's pitch carried
    // him up its side. Whether the original's stops him against it is
    // AH.70's oracle walk.
    assert!(whole_across > 0.5, "stopped short of it: {whole_across}");
    assert!(whole_on_level, "whole, on the ground short of it");
    let mut run = at_anchor_a();
    let (_, pot) = run.placed(LEDGE[2]);
    let w = drive(&mut run, 380, |k, run| {
        let to = pot - run.hero().pivot;
        if (100..136).contains(&k) {
            run.bench.view = heading_of(to);
        }
        if k == 199 || k == 259 {
            run.bench.heroes.get_mut(&HERO).unwrap().facing = heading_of(to).rem_euclid(360.0);
        }
        let mut p = press(0, PRESS, k);
        p.actions[0] |= (200..206).contains(&k) || (260..266).contains(&k);
        if (100..136).contains(&k) {
            p.stick = Stick::new(0.0, 1.0);
        }
        p
    });
    println!(
        "broke {:?}; he stood {:.2} m from the pot at its lob",
        w.broke(),
        (pot - run.hero().pivot).length()
    );
    let pot_at = w.breaks.iter().find(|b| b.0 == LEDGE[2]).map(|b| b.1);
    assert!(
        pot_at.is_some_and(|k| k >= 260),
        "the pot to the third lob: {pot_at:?}"
    );
    for name in LEDGE {
        let b = w.breaks.iter().find(|b| b.0 == name).expect(name);
        assert!(!b.2, "{name}'s solid gone at once");
    }
    let props = run.bench.props.as_ref().unwrap();
    for p in props
        .level
        .props
        .iter()
        .filter(|p| LEDGE.contains(&p.name.as_str()))
    {
        assert!(!in_seam(&run, p.object), "{} still in the seam", p.name);
    }
    if let Ok(out) = std::env::var("AH21_STATE_OUT") {
        let taken: Vec<&str> = props
            .level
            .props
            .iter()
            .filter(|p| {
                run.world.taken_off(Obj {
                    unit: LEVEL,
                    index: p.object,
                })
            })
            .map(|p| p.name.as_str())
            .collect();
        std::fs::write(&out, serde_json::to_string_pretty(&taken).unwrap()).unwrap();
        println!("wrote {taken:?} to {out}");
    }
    let (across, above, on_level, highest) = walk_at(&mut run, barrel, 180);
    println!(
        "broken: he comes within {across:.3} m of where it lay across the ground, {above:.3} m above its pivot, on the level's collision {on_level}; re-aimed there, {highest:.4} m over the ground at the most"
    );
    assert!(across < 0.3, "{across}");
    assert!(on_level, "on the ground where it lay");
    // The stick re-aimed at it every step turns his walk round every step
    // he crosses it: he holds the slope there, off it a frame at a time and
    // never past 0.1 m, inside the 0.4 ft (12.2 cm) where a frame off the
    // ground puts 10 ft/s down into his walk (AG.90: the original's first
    // such frame 4.7-5.2 cm up on AG.90's line, our walk here 6.3 cm on its
    // own; without the fix he climbed 1 m).
    assert!(
        highest < 0.1,
        "re-aimed there he holds the ground: {highest}"
    );

    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// A hero swap keeps the level as he left it (openspec AH.105, the
/// deactivate path; breakables' Swapped mid-smash): a lob from anchor A
/// breaks the ledge route's two barrels and steps the common count to
/// 0.024279475; his figure taken off the portal (the deactivate path, the
/// run going on) and put back on (the join, then his spawn's 120 steps),
/// both barrels stay off (out of the VM, their solids out of the seam, the
/// destructibles system listing neither as a target) and the counts stand
/// where they stood; at anchor A again, an eruption breaks the pot and the
/// gourd and never a barrel, the uncommon count holding and the common one
/// going on from 0.024279475.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_swap_keeps_the_broken_ones_broken_and_the_counts_where_they_stood() {
    println!("load before: {}", load());
    let started = Instant::now();
    let mut run = at_anchor_a();
    let w = drive(&mut run, 200, |k, _| press(0, PRESS, k));
    assert_eq!(
        w.broke().iter().map(|b| b.0).collect::<Vec<_>>(),
        ["barrel_b_intact(1)", "barrel_b_intact(4)"]
    );
    let barrels: Vec<Obj> = ["barrel_b_intact(1)", "barrel_b_intact(4)"]
        .iter()
        .map(|n| {
            run.files
                .placed
                .iter()
                .find(|p| p.name == *n && p.obj.unit == LEVEL)
                .unwrap()
                .obj
        })
        .collect();
    let counts = |run: &Run| {
        [Rarity::Common, Rarity::Uncommon, Rarity::Rare]
            .map(|r| scripts::loot_index(&run.vm, &run.world, r))
    };
    let before = counts(&run);
    assert_eq!(before[0], Some(0.024_279_475));
    // Off the portal and on again where he stands.
    let values = run.figure_values().expect("his figure's words");
    let gone = run.deactivate().unwrap_or_else(|h| panic!("{h}"));
    println!(
        "taken off: his minion {}, {} of his file's placements, clones {:?}",
        gone.minion,
        gone.placements.len(),
        gone.clones
            .iter()
            .map(|&h| run.world.name(h))
            .collect::<Vec<_>>()
    );
    run.join_with(values).unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    to_anchor_a(&mut run);
    assert_eq!(counts(&run), before, "the counts where they stood");
    println!("counts across the swap: {before:?}");
    for b in &barrels {
        assert!(run.world.taken_off(*b), "{b:?} still off");
        let p = run.vm.placement_of(run.handles[b]);
        assert!(p.is_none_or(|p| run.vm.removed(p)), "{b:?} out of the VM");
        assert!(!in_seam(&run, b.index), "{b:?}'s solid out of the seam");
    }
    let targets: Vec<String> = destructibles(&run)
        .into_iter()
        .filter(|(_, active)| *active)
        .map(|(n, _)| n)
        .collect();
    for n in ["barrel_b_intact(1)", "barrel_b_intact(4)"] {
        assert!(!targets.contains(&n.to_owned()), "{n} a target again");
    }
    let w = drive(&mut run, 300, |k, _| press(1, PRESS, k));
    println!(
        "the eruption after the swap: broke {:?}, coins {:?}, counts {:?}",
        w.broke(),
        w.coins,
        w.counts
    );
    // The watch finds the two off from its first step: never broken
    // again.
    let broke: Vec<(&str, u32)> = w.broke();
    assert_eq!(
        broke,
        [
            ("barrel_b_intact(1)", 0),
            ("barrel_b_intact(4)", 0),
            ("pottery_2(5)", PRESS + 65),
            ("gourd_01(10)", PRESS + 70),
        ],
        "the eruption at AH.9's steps (AH.54), never the barrel again"
    );
    assert_eq!(w.coins.len(), 1, "the pot's; the gourd's count past 0.6");
    assert_eq!(
        w.count("uncommon")
            .iter()
            .map(|c| (c.1, c.2))
            .collect::<Vec<_>>(),
        [(0.097_497, 0.097_496_99)]
    );
    let c = w.count("common");
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!(c[0].1, 0.024_279_475, "on from where it stood");
    assert!((c[0].2 - 0.624_279_5).abs() < 1e-5, "{c:?}");
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// Five breaks in a row drop three coins (breakables' Five in a row): the
/// disc's own `Combat - Hurt Creature` with a Lava Lob's numbers on five of
/// the clearing's common breakables in turn, 30 steps apart, from anchor
/// A's count: the count steps by 0.6 at each, wrapping past 1, and a coin
/// drops at the first, second and fourth (the count at or under 0.6), each
/// worth 1 or 2.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn five_breaks_in_a_row_drop_three_coins() {
    println!("load before: {}", load());
    let mut run = at_anchor_a();
    let m = run.minion.unwrap();
    let owner = {
        let d = run
            .vm
            .local(m, "description")
            .map(|v| run.vm.value(v))
            .unwrap();
        run.vm
            .field(d, "creature info")
            .map(|v| run.vm.value(v))
            .unwrap()
    };
    let five = [
        "barrel_b_intact(1)",
        "barrel_b_intact(4)",
        "barrel_b_intact(3)",
        "gourd_01(10)",
        "pottery_1(8)",
    ];
    let targets: Vec<Handle> = five.iter().map(|n| run.placed(n).0).collect();
    let mut w = Watch::new(&run);
    for k in 0..200u32 {
        if k % 30 == 10 && ((k / 30) as usize) < five.len() {
            let target = targets[(k / 30) as usize];
            let args = [
                ("damage", Value::Float(8.0)),
                ("target", Value::Entity(target)),
                ("knockback strength", Value::Int(25)),
                ("hurt owner info", owner),
                ("hurt location actor", Value::Entity(HERO)),
            ];
            run.with_host(|vm, host| {
                vm.call_with(Some(m), "Combat - Hurt Creature", &args, &[], host)
            })
            .unwrap_or_else(|h| panic!("{h}"));
        }
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        w.after(&run, k);
    }
    println!(
        "broke {:?}; coins {:?}; counts {:?}",
        w.broke(),
        w.coins,
        w.counts
    );
    assert_eq!(w.broke().iter().map(|b| b.0).collect::<Vec<_>>(), five);
    let common = w.count("common");
    assert_eq!(common.len(), 5, "{common:?}");
    let dropped: Vec<bool> = common.iter().map(|c| c.2 <= 0.6 + 1e-4).collect();
    assert_eq!(dropped, [true, true, false, true, false]);
    assert_eq!(w.coins.len(), 3, "{:?}", w.coins);
    // Each hurt lands between steps; the game systems' update shuts the
    // broken one down for its loot in the step after it, the step the
    // watch sees it gone.
    let breaks = w.broke();
    for (i, &d) in dropped.iter().enumerate() {
        let k = breaks[i].1;
        assert_eq!(w.coins.iter().any(|c| c.0 == k), d, "{}", breaks[i].0);
    }
    assert!(w.coins.iter().all(|c| (1..=2).contains(&c.1)));
    println!("load after: {}", load());
}

/// The coins' worth (breakables' The coins' worth): the disc's own `Loot -
/// Get Loot` for a common barrel called in the game systems' context until
/// 300 coins have dropped, on the session's generator: three of every five
/// calls drop, and between 176 and 224 of the 300 are worth 1, the rest 2
/// (two in three, mean and three standard deviations).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn three_hundred_common_coins_are_two_in_three_worth_1() {
    println!("load before: {}", load());
    let started = Instant::now();
    let mut run = at_anchor_a();
    let (barrel, _) = run.placed("barrel_b_intact(9)");
    let gs = run
        .files
        .placed
        .iter()
        .find(|p| {
            p.script
                .as_deref()
                .is_some_and(|s| s.ends_with("GameSystems.ai"))
        })
        .map(|p| run.handles[&p.obj])
        .and_then(|h| run.vm.placement_of(h))
        .expect("the game systems");
    let mut seen: HashSet<Handle> = run.world.clones().into_iter().collect();
    let mut worths: Vec<i32> = Vec::new();
    let mut calls = 0;
    while worths.len() < 300 {
        calls += 1;
        run.with_host(|vm, host| {
            vm.call_with(
                Some(gs),
                "Loot - Get Loot",
                &[
                    ("LootProducerRef", Value::Entity(barrel)),
                    ("ActorToSpawnLootAt", Value::Entity(barrel)),
                ],
                &[],
                host,
            )
        })
        .unwrap_or_else(|h| panic!("{h}"));
        for c in run.world.clones() {
            if seen.insert(c) && run.world.name(c).contains("Coin") {
                worths.push(run.world.actor(c).unwrap().id);
            }
        }
        assert!(calls < 1000);
    }
    let ones = worths.iter().filter(|&&w| w == 1).count();
    println!(
        "{calls} calls, {} coins: {ones} worth 1, {} worth 2; {:.2?}",
        worths.len(),
        worths.iter().filter(|&&w| w == 2).count(),
        started.elapsed()
    );
    assert!(worths.iter().all(|w| (1..=2).contains(w)));
    assert!((176..=224).contains(&ones), "{ones}");
    assert!((498..=502).contains(&calls), "three in five: {calls}");
    println!("load after: {}", load());
}

/// What only a Giant moves stays put (breakables' Eruptor at the stone
/// wall): two Lava Lobs and an Eruption at the stone wall by the spawn
/// 4.6 m off, and a lob and an eruption at the Titan boulder over
/// `Emerald(2)` 3.9 m off (AH.9's `tour_s`), leave both whole, their solids
/// standing.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_stone_wall_and_a_titan_boulder_stand_under_his_attacks() {
    println!("load before: {}", load());
    for (name, offset, facing, lobs) in [
        (
            "Breakable_Stone_Wall_Castle_206(1)",
            Vec3::new(0.0, 0.0, -4.6),
            0.0,
            2,
        ),
        (
            "Titan_CarriableObject_Boulder_v3_(4)",
            Vec3::new(3.9, 0.0, 0.0),
            270.0,
            1,
        ),
    ] {
        let (mut run, _) = joined_with(Mode::Strict, Setup { props: true });
        let (h, at) = run.placed(name);
        run.put(at + offset, facing);
        let w = drive(&mut run, 420, |k, _| {
            let mut p = pad([false; 4], Stick::CENTRED);
            p.actions[0] = (0..lobs).any(|i| (30 + 60 * i..36 + 60 * i).contains(&k));
            p.actions[1] = (150..156).contains(&k);
            p
        });
        let props = run.bench.props.as_ref().unwrap();
        let object = props
            .level
            .props
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .object;
        println!(
            "{name}: broke {:?}, standing {}, its placement taken off {}",
            w.broke(),
            props.standing.contains(&object),
            run.world.removed(h)
        );
        assert!(!run.world.removed(h), "{name}");
        assert!(props.standing.contains(&object), "{name}'s solid");
        assert!(w.breaks.iter().all(|b| b.0 != name));
    }
    println!("load after: {}", load());
}

/// The members the breakables reach (design decision 2's count): a lob and
/// an eruption from anchor A and the five hurts, every member registered,
/// under AH.34's ceiling of 324 and AG.28's one more; the ones past the
/// fight's tables named.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_members_the_breakables_reach_stay_under_the_ceiling() {
    println!("load before: {}", load());
    let mut run = at_anchor_a();
    let (_, pot) = run.placed(LEDGE[2]);
    drive(&mut run, 400, |k, run| {
        if k == 149 {
            let to = pot - run.hero().pivot;
            run.bench.heroes.get_mut(&HERO).unwrap().facing = heading_of(to).rem_euclid(360.0);
        }
        let mut p = press(0, PRESS, k);
        p.actions[0] |= (150..156).contains(&k);
        p.actions[1] = (220..226).contains(&k);
        p
    });
    let name = |m: &MemberId| run.vm.config().members.name(*m);
    let registered = |m: &MemberId| {
        INIT_CHAIN
            .iter()
            .chain(HERO_MEMBERS)
            .chain(CREATURES)
            .chain(COMBAT)
            .chain(LOOP)
            .chain(FIGURE)
            .chain(CAMERA)
            .any(|r| MemberId::named(r.namespace, r.name) == *m)
            || guac_script::Builtin::of(*m).is_some()
    };
    let reached = run.vm.reached();
    let asked = reached
        .iter()
        .filter(|m| run.vm.uses().contains_key(*m))
        .count();
    let mut past: Vec<String> = reached
        .iter()
        .filter(|m| run.vm.uses().contains_key(*m) && !registered(m))
        .map(name)
        .collect();
    past.sort();
    let mut all: Vec<String> = reached.iter().map(name).collect();
    all.sort();
    println!(
        "{} steps; members reached {} ({} the VM's own), {asked} asked a value; past the tables: {past:?}",
        run.steps,
        reached.len(),
        reached
            .iter()
            .filter(|m| guac_script::Builtin::of(**m).is_some())
            .count(),
    );
    if let Ok(out) = std::env::var("AH21_MEMBERS_OUT") {
        std::fs::write(out, all.join("\n")).unwrap();
    }
    println!("load after: {}", load());
    assert!(past.is_empty(), "{past:?}");
    // AG.28's camera volumes and activator raise AH.34's ceiling by one.
    assert!(
        reached.len() <= 325,
        "past AH.34's ceiling: {}",
        reached.len()
    );
    let _ = FEET_PER_METRE;
}

/// AG.90's recordings (2026-10-23, the O6 build from anchor A, O5's watch):
/// `rock_near` (3) walks at a point on the ledge route's 17° slope 2 m short
/// of `barrel_b_intact(1)`, `rock_lob` (2) lobs, breaking it and `(4)`, and
/// walks at where it stood; then each turns the stick round every frame for
/// 150 frames (a stick re-aimed at a point he crosses), and lets go.
const AG90_RUNS: [&str; 2] = ["ag90-rock-1792773586", "ag90-rock-1792773746"];

/// Each AG.90 recording by its run and label, its records as JSON.
fn ag90_recordings() -> Vec<(String, Vec<serde_json::Value>)> {
    let mut out = Vec::new();
    for run in AG90_RUNS {
        let dir = capture_dir().join("BLUS30968").join(run);
        let mut labels: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("rock_"))
            .collect();
        labels.sort();
        for label in labels {
            let text = std::fs::read_to_string(dir.join(&label).join("recording.json")).unwrap();
            let records: serde_json::Value = serde_json::from_str(&text).unwrap();
            out.push((
                format!("{run}/{label}"),
                records.as_array().unwrap().clone(),
            ));
        }
    }
    out
}

fn json_vec3(v: &serde_json::Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap() as f32;
    Vec3::new(f(0), f(1), f(2))
}

/// A record's pad as our step reads it: its left stick's bytes as the
/// game's floats (128 centred, y 0 forward) and South (attack 1).
fn record_pad(r: &serde_json::Value) -> Pad {
    let byte = |b: &serde_json::Value| b.as_u64().unwrap() as u8;
    let left = &r["pad"]["left"];
    let stick = Stick::from_bytes(byte(&left["x"]), byte(&left["y"]));
    let south = r["pad"]["digital2"].as_u64().unwrap() & SOUTH != 0;
    pad([south, false, false, false], stick)
}

/// What a step left, ours or a record's: the pivot, its height over the
/// ground finder's point, whether the support check held him and the
/// heading of the walk the movement script keeps (none at rest).
#[derive(Debug, Clone, Copy)]
struct Stood {
    pivot: Vec3,
    height: f32,
    supported: bool,
    walk: Option<f32>,
    /// The walk's pitch, degrees.
    pitch: f32,
}

fn theirs(r: &serde_json::Value) -> Stood {
    let pivot = json_vec3(&r["hero"]["position"]);
    let ground = json_vec3(&r["contacts"]["ground"]["location"]);
    let m = &r["mover"];
    Stood {
        pivot,
        height: pivot.y - ground.y,
        supported: r["contacts"]["support"].as_u64() == Some(2),
        walk: (m["length"].as_f64().unwrap() > 0.0).then(|| m["heading"].as_f64().unwrap() as f32),
        pitch: m["pitch"].as_f64().unwrap() as f32,
    }
}

fn ours(run: &Run) -> Stood {
    let h = run.hero();
    let w = h.walk.velocity;
    Stood {
        pivot: h.pivot,
        height: h.footing.height.unwrap_or(f32::NAN),
        supported: h.footing.supported,
        walk: (w.x.hypot(w.z) > 0.0).then(|| heading_of(w)),
        pitch: guacamole::script_engine::pitch_of(w),
    }
}

/// A recording replayed from anchor A: step k reads record k's pad against
/// the view record k's camera holds and is set beside record k + 1 (O5
/// samples a frame's state before its pad read, so the frame reading record
/// k's pad leaves record k + 1, AH.59). The records whose game stick turned
/// round from the record before's are the rock's; at the record before its
/// first, he's set to the recording's state there ([`seed_from`]), so what
/// the rock is compared on is its own and not the approach's drift (our
/// steps are 1/60 s, the oracle's its own frame times).
fn rock_replay(
    records: &[serde_json::Value],
) -> (Vec<(Stood, Stood)>, std::ops::RangeInclusive<usize>) {
    let stick = |r: &serde_json::Value| r["stick"][0].as_f64().unwrap();
    let rock: Vec<usize> = (1..records.len())
        .filter(|&i| stick(&records[i]) * stick(&records[i - 1]) < 0.0)
        .collect();
    let seed = rock[0] - 1;
    let mut run = at_anchor_a();
    let mut steps = Vec::new();
    for k in 0..records.len() - 1 {
        if k == seed {
            seed_from(&mut run, &records[k]);
        }
        run.bench.view = records[k]["camera"]["rotation"][1].as_f64().unwrap() as f32;
        run.bench.pads[0] = record_pad(&records[k]);
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        steps.push((ours(&run), theirs(&records[k + 1])));
    }
    // As steps: record i is step i - 1's.
    (steps, rock[0] - 1..=rock[rock.len() - 1] - 1)
}

/// He's set to a record's state: his pivot, facing and lean, his footing
/// as its contacts read it (the push tilting to his settled lean, else the
/// ground finder's, AG.38), and the walk his movement script keeps, both
/// where the engine reads it and in the script's own velocity data.
fn seed_from(run: &mut Run, r: &serde_json::Value) {
    use guac_motion::ground::{LEAN_SETTLED, angle};
    let pivot = json_vec3(&r["hero"]["position"]);
    let rotation = r["hero"]["rotation"].as_array().unwrap();
    let rotation = [0, 1, 2].map(|i| rotation[i].as_f64().unwrap() as f32);
    let ground = &r["contacts"]["ground"];
    let normal = json_vec3(&ground["normal"]);
    let m = &r["mover"];
    let f = |k: &str| m[k].as_f64().unwrap() as f32;
    let (length, heading, pitch) = (f("length"), f("heading"), f("pitch"));
    let walk = guacamole::script_engine::direction(heading, pitch) * length;
    let hero = run.bench.heroes.get_mut(&HERO).unwrap();
    hero.pivot = pivot;
    hero.facing = rotation[1].rem_euclid(360.0);
    hero.up = guac_motion::solid::euler(rotation) * Vec3::Y;
    hero.footing = guac_motion::walk::Footing {
        supported: r["contacts"]["support"].as_u64() == Some(2),
        height: Some(pivot.y - json_vec3(&ground["location"]).y),
        slope: if angle(hero.up, normal) <= LEAN_SETTLED {
            hero.up
        } else {
            normal
        },
        ground: Some(normal),
    };
    hero.walk.velocity = walk;
    let m = run.minion.unwrap();
    let d = run
        .vm
        .local(m, "description")
        .map(|v| run.vm.value(v))
        .unwrap();
    let data = run
        .vm
        .field(d, "velocity data")
        .map(|v| run.vm.value(v))
        .unwrap();
    for (name, value) in [
        ("velocity length", length),
        ("velocity heading", heading),
        ("velocity pitch", pitch),
        ("vertical difference", f("vertical")),
        ("horizontal length", f("horizontal")),
    ] {
        assert!(run.vm.set_field(data, name, Value::Float(value)), "{name}");
    }
}

/// hero-movement's A walk turned round every frame (openspec AG.90): a stick
/// turned round every frame on the ledge route's 17° slope, as one re-aimed
/// at a point he crosses asks, turns his walk round every frame and he holds
/// the ground as the original's Eruptor does, never climbing. Each of AG.90's
/// five recordings replayed from anchor A on its own pads and views, his
/// walk the movement script's on the VM, set to the recording's state at
/// the record before the stick first turns round and set beside it step by
/// step over the 148 steps it turns round on: his walk turns round every
/// step, along theirs within 0.05°; its pitch over the rock spans theirs to
/// 1° at either end (the push tilted to the slope along the stick, AG.38);
/// he leaves the ground a frame at a time, never two running, as many times
/// as theirs to 3 (which frame is a knife's edge: a walk 0.6° shallower than
/// the slope leaves it, and the oracle's own replays part there); he stands
/// over the ground finder's point within 1 cm of theirs at every step; his
/// pivot falls over the rock, within 0.1 m of theirs (theirs 0.149-0.184 m,
/// ours 3-6 cm more: the slow uphill step after each frame off the ground
/// climbs 2 mm less, AG.16's landing-frame hypothesis (19)). Without AG.90's
/// fix his scripts never saw him off the ground and he climbed 1 m over the
/// rock.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.90's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn a_walk_turned_round_every_frame_holds_the_slope() {
    println!("load before: {}", load());
    let started = Instant::now();
    let recordings = ag90_recordings();
    assert_eq!(recordings.len(), 5, "AG.90's five");
    let angle = |a: f32, b: f32| (a - b + 180.0).rem_euclid(360.0) - 180.0;
    for (label, records) in &recordings {
        let (steps, rock) = rock_replay(records);
        let (a, b) = (*rock.start(), *rock.end());
        let rock = &steps[rock];
        let mut heading = 0f32;
        let mut height = 0f32;
        let mut reversed = 0;
        for (k, (ours, theirs)) in rock.iter().enumerate() {
            let (o, t) = (ours.walk.unwrap(), theirs.walk.unwrap());
            heading = heading.max(angle(o, t).abs());
            height = height.max((ours.height - theirs.height).abs());
            if k > 0 {
                let before = rock[k - 1].0.walk.unwrap();
                reversed += usize::from((angle(o, before).abs() - 180.0).abs() < 5.0);
            }
        }
        let of = |pick: fn(&(Stood, Stood)) -> Stood| {
            let pitch = rock.iter().map(|s| pick(s).pitch);
            let low = pitch.clone().fold(f32::MAX, f32::min);
            let high = pitch.fold(f32::MIN, f32::max);
            let off: Vec<usize> = (0..rock.len())
                .filter(|&k| !pick(&rock[k]).supported)
                .collect();
            let running = off.windows(2).filter(|w| w[1] == w[0] + 1).count();
            let fall = pick(&rock[rock.len() - 1]).pivot.y - pick(&rock[0]).pivot.y;
            (low, high, off.len(), running, fall)
        };
        let (ours, theirs) = (of(|s| s.0), of(|s| s.1));
        println!(
            "{label}: steps {a}-{b}; the walk turned round {reversed} of {}, along theirs within {heading:.4}°; its pitch {:.2}..{:.2}°, theirs {:.2}..{:.2}°; off the ground {} times ({} running), theirs {} ({}); over the ground within {height:.4} m of theirs; the pivot {:+.4} m over the rock, theirs {:+.4}",
            rock.len() - 1,
            ours.0,
            ours.1,
            theirs.0,
            theirs.1,
            ours.2,
            ours.3,
            theirs.2,
            theirs.3,
            ours.4,
            theirs.4,
        );
        assert_eq!(reversed, rock.len() - 1, "{label}: turned round every step");
        assert!(heading < 0.05, "{label}: {heading}");
        assert!(
            (ours.0 - theirs.0).abs() < 1.0 && (ours.1 - theirs.1).abs() < 1.0,
            "{label}: pitch {ours:?} against {theirs:?}"
        );
        assert_eq!((ours.3, theirs.3), (0, 0), "{label}: a frame at a time");
        assert!(
            ours.2.abs_diff(theirs.2) <= 3,
            "{label}: {} against {}",
            ours.2,
            theirs.2
        );
        assert!(height < 0.01, "{label}: {height}");
        assert!(
            ours.4 < 0.0 && (ours.4 - theirs.4).abs() < 0.1,
            "{label}: {} against {}",
            ours.4,
            theirs.4
        );
    }
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// Chotchki's evening run (AG.75, the TV's log at 03:17:28Z): where the
/// TV's walk had left Eruptor up the ledge route when his figure came off,
/// his pivot and heading as the log reads them.
const EVENING: [f32; 3] = [-12.598_333, 54.720_524, -54.114_693];
const EVENING_FACING: f32 = 52.166_492;

/// AG.81: a hero put down where the last one stood (the TV's replacement,
/// `Hero::stand` at the last pivot) stands there. On anchor A's slope and
/// at chotchki's evening spot, his first 30 steps with his scripts driving
/// and the stick centred are each on the ground, his pivot within 1 cm of
/// where he was put (on anchor A's slope the first step lifts his upright
/// shape 5.4 mm out of the ground the last one stood on leaned), riding
/// 1.5 cm or less over the ground finder's point, his locomotion on the
/// ground with no weight on his fall. A hero dropped there fails it (the
/// original drops its replacement from 0.41 m up: AG.100's).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_hero_put_down_where_the_last_stood_stands_there() {
    use guac_motion::hero::Hero;
    use guac_motion::locomotion::{Frame, Gaits, Locomotion, Loops};
    use guac_motion::tuning::{Tuning, eruptor_solid};
    let started = Instant::now();
    for (label, at, facing) in [
        ("anchor A", ANCHOR_A, ANCHOR_A_FACING),
        ("evening", EVENING, EVENING_FACING),
    ] {
        let (mut run, _) = joined_with(Mode::Strict, Setup { props: true });
        let at = Vec3::from_array(at);
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, facing);
        hero.stand(&run.bench.world, &[HERO_BODY]);
        assert_eq!(hero.pivot, at, "{label}: stood, not moved");
        assert!(hero.footing.supported, "{label}: stood on the ground");
        run.bench.heroes.insert(HERO, hero);
        run.bench.pads = [pad([false; 4], Stick::CENTRED), Pad::default()];
        // His locomotion as the TV builds it from his palette's loops.
        let mut locomotion = Locomotion::new(
            Gaits::ERUPTOR,
            Loops {
                idle: 4.0 / 3.0,
                walk: Some(1.066_666_7),
                run: Some(0.666_666_7),
                fall: Some(0.8),
            },
        );
        let (mut highest, mut farthest) = (0.0f32, 0.0f32);
        for k in 0..30 {
            run.step().unwrap_or_else(|h| panic!("{label}: {h}"));
            let h = run.hero();
            locomotion.step(1.0 / 60.0, Frame::of(h));
            let height = h.footing.height.unwrap_or(f32::NAN);
            let moved = h.pivot.distance(at);
            if k < 6 || std::env::var_os("AG81_STEPS").is_some() {
                println!(
                    "{label} {k}: {} by {}, {height:.4} m up, supported {}, walk {}",
                    h.pivot,
                    h.pivot - at,
                    h.footing.supported,
                    h.walk.velocity
                );
            }
            assert!(
                h.footing.supported,
                "{label}: step {k} off the ground at {}",
                h.pivot
            );
            assert!(
                height > 0.0 && height <= 0.015,
                "{label}: step {k} {height} m over the ground"
            );
            assert!(moved < 0.01, "{label}: step {k} {moved} m from {at}");
            assert_eq!(locomotion.air().name(), "ground", "{label}: step {k}");
            assert_eq!(locomotion.weights()[3], 0.0, "{label}: step {k}: his fall");
            highest = highest.max(height);
            farthest = farthest.max(moved);
        }
        println!(
            "{label}: at most {highest:.4} m over the ground, {farthest:.4} m from where he was put"
        );
    }
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// Free in the air as guac-motion's replays read a record: unsupported 0.4
/// ft or more over the ground finder's point, the first plane his manifold
/// holds 2 cm or more off.
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

/// He's set to a record of AG.39's (no mover read there): his pivot, facing
/// and lean, his footing as its contacts read it, and his walk, where the
/// engine reads it and in the script's own velocity data, as what moved his
/// body into it (in the air the walk moves him by itself).
fn seed_in_the_air(run: &mut Run, r: &serde_json::Value) {
    use guac_motion::ground::{LEAN_SETTLED, angle};
    let pivot = json_vec3(&r["hero"]["position"]);
    let rotation = r["hero"]["rotation"].as_array().unwrap();
    let rotation = [0, 1, 2].map(|i| rotation[i].as_f64().unwrap() as f32);
    let ground = &r["contacts"]["ground"];
    let normal = json_vec3(&ground["normal"]);
    let walk = json_vec3(&r["hero"]["velocity"]) * FEET_PER_METRE;
    let length = walk.length();
    let pitch = (walk.y / length.max(1e-6))
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees();
    let hero = run.bench.heroes.get_mut(&HERO).unwrap();
    hero.pivot = pivot;
    hero.facing = rotation[1].rem_euclid(360.0);
    hero.up = guac_motion::solid::euler(rotation) * Vec3::Y;
    hero.footing = guac_motion::walk::Footing {
        supported: r["contacts"]["support"].as_u64() == Some(2),
        height: Some(pivot.y - json_vec3(&ground["location"]).y),
        slope: if angle(hero.up, normal) <= LEAN_SETTLED {
            hero.up
        } else {
            normal
        },
        ground: Some(normal),
    };
    hero.walk.velocity = walk;
    let m = run.minion.unwrap();
    let d = run
        .vm
        .local(m, "description")
        .map(|v| run.vm.value(v))
        .unwrap();
    let data = run
        .vm
        .field(d, "velocity data")
        .map(|v| run.vm.value(v))
        .unwrap();
    for (name, value) in [
        ("velocity length", length),
        ("velocity heading", heading_of(walk)),
        ("velocity pitch", pitch),
        ("vertical difference", walk.y),
        ("horizontal length", walk.x.hypot(walk.z)),
    ] {
        assert!(run.vm.set_field(data, name, Value::Float(value)), "{name}");
    }
}

/// AG.16's falls with his scripts driving (the TV's hero since AH.24): each
/// of AG.39's 40 falls off the ledge route and through the air, set to its
/// recording's state at the second record of each run of free ones and fed
/// each record's pad and view at our 1/60 s, lands within a record of
/// theirs in 38, as AG.16's controller does in `replays.rs` with no scripts
/// (stand-in ledger: `walker`; the same two parted: `g39_w` W-00 two early,
/// `g39_w_rel60` W-00 caught on the cliff). Before
/// AG.90 his scripts read his separation off where he joined, so above the
/// spawn's height none of these frames was off the ground for them, no
/// gravity pulled his walk down and 32 never landed (7 within a record).
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.39's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn ag39s_falls_land_as_theirs_with_his_scripts_driving() {
    println!("load before: {}", load());
    let started = Instant::now();
    let root = capture_dir().join("BLUS30968/ag39-falls-1792502813");
    let (mut falls, mut near, mut never) = (0, 0, 0);
    let mut groups: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    groups.sort();
    for group in groups {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&group)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("recording.json").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let text = std::fs::read_to_string(dir.join("recording.json")).unwrap();
            let records: serde_json::Value = serde_json::from_str(&text).unwrap();
            let records = records.as_array().unwrap();
            let label = dir.strip_prefix(&root).unwrap().display().to_string();
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
                let end = theirs.map_or(records.len() - 1, |l| (l + 3).min(records.len() - 1));
                let mut run = at_anchor_a();
                seed_in_the_air(&mut run, &records[k]);
                let mut ours = None;
                for (i, record) in records.iter().enumerate().take(end).skip(k) {
                    run.bench.view = record["camera"]["rotation"][1].as_f64().unwrap() as f32;
                    run.bench.pads[0] = record_pad(record);
                    run.step()
                        .unwrap_or_else(|h| panic!("{label} step {i}: {h}"));
                    if run.hero().footing.supported {
                        ours = Some(i + 1);
                        break;
                    }
                }
                println!("{label} from record {k}: lands at ours {ours:?}, theirs {theirs:?}");
                falls += 1;
                near +=
                    usize::from(matches!((ours, theirs), (Some(a), Some(b)) if a.abs_diff(b) <= 1));
                never += usize::from(ours.is_none());
                k = end.max(k + 1);
            }
        }
    }
    println!(
        "{falls} falls, {near} landing within a record of theirs, {never} not landing by theirs + 3; {:.2?}; load after: {}",
        started.elapsed(),
        load()
    );
    assert_eq!(falls, 40, "AG.39's 40");
    assert!(near >= 38, "{near} of {falls} within a record of theirs");
    assert!(never <= 1, "{never} never landing");
}
