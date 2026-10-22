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
use guac_script::members::{COMBAT, CREATURES, FIGURE, HERO as HERO_MEMBERS, INIT_CHAIN, LOOP};
use guac_script::{Handle, MemberId, Mode, Obj, Value};
use guacamole::script_engine::{FEET_PER_METRE, Pad, heading_of};
use guacamole::scripts::{self, Rarity};

/// Anchor A (AH.7, `ah7-anchor-a-1792536443/a2_anchor`): Eruptor still at
/// the foot of the ledge route facing `barrel_b_intact(1)`, his pivot and
/// heading as its `record.json` reads them.
const ANCHOR_A: [f32; 3] = [31.08841, 51.46313, -72.9834];
const ANCHOR_A_FACING: f32 = 253.706;
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
/// The ledge route's three breakables (AG.7's reach: the walkway's way up).
const LEDGE: [&str; 3] = ["barrel_b_intact(1)", "barrel_b_intact(4)", "pottery_1(8)"];

/// A run at anchor A: joined over the dump with the level's props in the
/// seam, his hands at AH.7's launch point and its mirror, set down at
/// anchor A's pose and the loot counts at its read.
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

/// His hands at AH.7's launch point and its mirror, and him set down at
/// anchor A's pose.
fn to_anchor_a(run: &mut Run) {
    let hand = Vec3::new(0.6011, 0.7710, 1.4838);
    let mut nodes = vec![Vec3::ZERO; 9];
    nodes[7] = hand;
    nodes[8] = Vec3::new(-hand.x, hand.y, hand.z);
    run.world.set_nodes(HERO, nodes);
    run.put(Vec3::from_array(ANCHOR_A), ANCHOR_A_FACING);
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
/// 31 steps after the press, AH.9's 33-34 (the lob's aim, AH.60's); our loot
/// steps the step it breaks, AH.9's the frame after (AH.59's tick order).
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
    let ours_after = k - PRESS;
    let theirs: Vec<usize> = ah9.iter().map(|r| r.breaks[0].1 - 1 - r.press).collect();
    println!("the break {ours_after} steps after the press; AH.9's {theirs:?}");
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
    assert!(w.coins.iter().all(|c| c.0 == k && (1..=2).contains(&c.1)));
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// One Eruption from anchor A breaks the uncommon pot, the gourd and the
/// barrel its reach passes, in AH.9's order and within two of its steps
/// from the press (65, 69-70 and 72 over the ten `erupt` replays), each
/// solid out of the seam the step it breaks, three pieces of debris each
/// gone 1.52 s on: the uncommon count's rate of 1 leaves 0.097497 at
/// 0.09749699 and drops a coin worth 3 or 7; the common count steps twice,
/// 0.8242795 to 0.42427945 to 0.024279475 as AH.9's, each dropping a coin
/// worth 1 or 2, the step after its break as AH.9's.
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
        assert!(
            (lo.saturating_sub(2)..=hi + 2).contains(&ours),
            "{name}: {ours} past {lo}..={hi} by more than 2"
        );
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
/// view along his way: the nearest his pivot came across the ground, how
/// high over `target` it stood then (m), and whether the level's own
/// collision was what he stood on there.
fn walk_at(run: &mut Run, target: Vec3, steps: u32) -> (f32, f32, bool) {
    let mut nearest = (f32::MAX, 0.0, false);
    for k in 0..steps {
        run.bench.view = heading_of(target - run.hero().pivot);
        run.bench.pads[0] = pad([false; 4], Stick::new(0.0, 1.0));
        run.step().unwrap_or_else(|h| panic!("step {k}: {h}"));
        let p = run.hero().pivot;
        let across = ((p - target) * Vec3::new(1.0, 0.0, 1.0)).length();
        if std::env::var("AH21_WALK").is_ok() && k % 10 == 0 {
            println!("walk {k}: {p:.3} {across:.3}");
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
    nearest
}

/// The ledge route opens (breakables' Up to the walkway): a lob from
/// anchor A breaks its two barrels; he walks 0.6 s toward `pottery_1(8)`
/// and a second lob, him turned to it, breaks it; the three solids are out
/// of the seam, and walked at where the
/// first barrel lay he stands on the ground there, where with it whole his
/// controller climbed onto it (an oracle check of its own: AH.70). The
/// props the scripts took off are written where `AH21_STATE_OUT` says, for
/// AG.7's reach.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_ledge_routes_three_break_and_he_walks_where_they_stood() {
    println!("load before: {}", load());
    let started = Instant::now();
    let mut whole = at_anchor_a();
    let (_, barrel) = whole.placed(LEDGE[0]);
    let (whole_across, whole_above, on_level) = walk_at(&mut whole, barrel, 180);
    println!(
        "{} whole: he comes within {whole_across:.3} m of where it lies across the ground, {whole_above:.3} m above its pivot, on the level's collision {on_level}",
        LEDGE[0]
    );
    let mut run = at_anchor_a();
    let (_, pot) = run.placed(LEDGE[2]);
    let w = drive(&mut run, 300, |k, run| {
        let to = pot - run.hero().pivot;
        if (100..136).contains(&k) {
            run.bench.view = heading_of(to);
        }
        if k == 199 {
            run.bench.heroes.get_mut(&HERO).unwrap().facing = heading_of(to).rem_euclid(360.0);
        }
        let mut p = press(0, PRESS, k);
        p.actions[0] |= (200..206).contains(&k);
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
    let (across, above, on_level) = walk_at(&mut run, barrel, 180);
    println!(
        "broken: he comes within {across:.3} m of where it lay across the ground, {above:.3} m above its pivot, on the level's collision {on_level}"
    );
    assert!(across < 0.3, "{across}");
    assert!(on_level, "on the ground where it lay");
    assert!(
        whole_above - above > 1.0,
        "lower than on it whole: {above} against {whole_above}"
    );
    println!("{:.2?}; load after: {}", started.elapsed(), load());
}

/// A hero swap keeps the level as he left it (openspec AH.27, the TV's
/// take-off): a lob from anchor A breaks the ledge route's two barrels and
/// steps the common count to 0.024279475; the level read off that run and
/// put back into a fresh one before its first step, as the TV's take-off
/// installs it again, holds both barrels off (out of the VM, asleep, their
/// solids out of the seam, the destructibles system listing neither as a
/// target) and, once the fresh run's loot data is made, the counts where
/// they stood rather than drawn anew; joined again at anchor A, an
/// eruption breaks the pot and the gourd and never the barrel, the
/// uncommon count holding and the common one going on from 0.024279475.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_swap_keeps_the_broken_ones_broken_and_the_counts_where_they_stood() {
    use guacamole::scripts::LevelState;
    println!("load before: {}", load());
    let started = Instant::now();
    let mut run = at_anchor_a();
    // Taken off by the level's start (its definition and the archetype
    // setups take themselves off), which the fresh run runs again.
    let at_join = LevelState::read(&run.vm, &run.world, LEVEL).taken_off;
    let w = drive(&mut run, 200, |k, _| press(0, PRESS, k));
    assert_eq!(
        w.broke().iter().map(|b| b.0).collect::<Vec<_>>(),
        ["barrel_b_intact(1)", "barrel_b_intact(4)"]
    );
    let kept = LevelState::read(&run.vm, &run.world, LEVEL).since(&at_join);
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
    assert_eq!(kept.taken_off, barrels, "play's alone");
    println!(
        "taken off at the join: {:?}",
        at_join
            .iter()
            .map(|o| run.world.name(run.handles[o]))
            .collect::<Vec<_>>()
    );
    let common = |state: &LevelState| {
        state
            .loot
            .iter()
            .find(|(r, _)| *r == Rarity::Common)
            .map(|c| c.1)
    };
    assert_eq!(common(&kept), Some(0.024_279_475));
    println!(
        "kept: {:?} off, counts {:?}",
        kept.taken_off
            .iter()
            .map(|o| run.world.name(run.handles[o]))
            .collect::<Vec<_>>(),
        kept.loot
    );

    // The fresh run, as the TV's take-off installs it again.
    let (mut fresh, _) = run_on_with(Mode::Strict, Setup { props: true });
    kept.put_back(&mut fresh.vm, &mut fresh.world);
    fresh.step().unwrap_or_else(|h| panic!("{h}"));
    assert!(
        kept.set_loot(&mut fresh.vm, &fresh.world),
        "the loot data made"
    );
    let again = LevelState::read(&fresh.vm, &fresh.world, LEVEL);
    assert_eq!(again.loot, kept.loot, "the counts where they stood");
    fresh.step().unwrap_or_else(|h| panic!("{h}"));
    fresh.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        fresh.step().unwrap_or_else(|h| panic!("{h}"));
    }
    to_anchor_a(&mut fresh);
    for b in &barrels {
        assert!(fresh.world.taken_off(*b), "{b:?} off in the fresh run");
        let h = fresh.handles[b];
        assert!(!fresh.world.actor(h).unwrap().active, "{b:?} asleep");
        let p = fresh.vm.placement_of(h);
        assert!(p.is_none_or(|p| fresh.vm.removed(p)), "{b:?} out of the VM");
        assert!(!in_seam(&fresh, b.index), "{b:?}'s solid out of the seam");
    }
    let targets: Vec<String> = destructibles(&fresh)
        .into_iter()
        .filter(|(_, active)| *active)
        .map(|(n, _)| n)
        .collect();
    for n in ["barrel_b_intact(1)", "barrel_b_intact(4)"] {
        assert!(!targets.contains(&n.to_owned()), "{n} a target again");
    }
    let w = drive(&mut fresh, 300, |k, _| press(1, PRESS, k));
    println!(
        "the eruption after the swap: broke {:?}, coins {:?}, counts {:?}",
        w.broke(),
        w.coins,
        w.counts
    );
    // The watch finds the two off from its first step: put back, never
    // broken again.
    let broke: Vec<(&str, u32)> = w.broke();
    assert_eq!(
        broke,
        [
            ("barrel_b_intact(1)", 0),
            ("barrel_b_intact(4)", 0),
            ("pottery_2(5)", PRESS + 66),
            ("gourd_01(10)", PRESS + 71),
        ],
        "the eruption at AH.21's steps, never the barrel again"
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
/// under AH.34's ceiling of 324; the ones past the fight's tables named.
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
    assert!(
        reached.len() <= 324,
        "past AH.34's ceiling: {}",
        reached.len()
    );
    let _ = FEET_PER_METRE;
}
