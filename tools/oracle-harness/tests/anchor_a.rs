//! AH.51 over AH.9's recordings (openspec, design decision 18 under AH.45's
//! (b)); nothing here boots the oracle.
//!
//! - `named_floors_are_the_groups_floors`: each of AH.9's five groups from
//!   anchor A (`lob`, `erupt`, `lobs`, `erupts`, `coin`, 10 pause-free
//!   replays each) calibrated by AF.12's method with every kind stamped; per
//!   kind the largest of the five is `guac_oracle::tolerance::anchor_a`'s
//!   named floor, so a default that drifts from its measurement fails here.
//! - `replays_pass_and_perturbations_fail`: at `anchor_a::EVENTS` every
//!   replay's leave-one-out passes, and each first press moved 1 or 2 flips
//!   either way (`lob_p1` to `erupt_m2`) fails against its group, on a kind
//!   the game times as well as on the press. A replay whose break dropped no
//!   coin, what decision 18's independent loot roll does to a break in 2 of
//!   5, fails too (the drop count itself is AH.30's totals).
//!
//! The replays are read by name from `GUAC_CAPTURE_DIR` (or its `BLUS30968/`
//! folder): `ah9-main-1792545846/` and `ah9-perturb-1792549070/`. Skips when
//! the key is unset.
//!
//! `cargo test -p oracle-harness --test anchor_a -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use guac_oracle::fight::extract::press_kind;
use guac_oracle::fight::log::{ActorEvent, ActorLogs, LogMismatch};
use guac_oracle::input::Button;
use guac_oracle::tolerance::anchor_a;
use oracle_harness::env;
use oracle_harness::fightlog::{GroupReport, Replay, ReportRules, load};

const MAIN: &str = "ah9-main-1792545846";
const PERTURB: &str = "ah9-perturb-1792549070";

/// AH.9's rules for its lob groups (`ah9/rules/lob-all.ron`): the throw
/// either hand's clip, the ball a lava ball the hero spawns, its burst its
/// leaving, a break a breakable leaving within 3 m of the ball's last place,
/// its coin a loot entry the breakable left.
const LOB_RULES: &str = r#"(
    spawns: [
        (kind: "lava_ball", parents: ["hero"], radius: 4.0, within: 1),
        (kind: "fire_blob_splash", parents: ["hero"], radius: 4.0, within: 1),
        (kind: "eruption", parents: ["hero"], radius: 1.0, within: 1),
        (kind: "debris", parents: ["breakable"], radius: 3.0, within: 2),
        (kind: "coin", parents: ["breakable"], radius: 3.0, within: 3),
        (kind: "coin_art", parents: ["breakable"], radius: 3.0, within: 3),
    ],
    rules: (
        chain: Some((
            attack: South,
            throw: Playing(["Models/Characters/MinionsMonsters/Eruptor/Eruptor_RunAttackLeft.ma", "Models/Characters/MinionsMonsters/Eruptor/Eruptor_RunAttackRight.ma"]),
            ball: "lava_ball",
            burst: Gone,
            breaks: (kinds: ["breakable"], broken: Gone, coin: "coin"),
            reach: 3.0,
            window: 0.2,
        )),
        breaks: Some((kinds: ["breakable"], broken: Gone, coin: "coin")),
        pickups: Some((kinds: ["coin"], worth: "worth", collector: "hero", reach: 3.6576)),
        gains: ["gold"],
    ),
    quantum: 0.005,
    stamps_past_horizon: true,
)"#;

/// AH.9's rules for its eruption groups (`ah9/rules/erupt-all.ron`): the
/// eruption a child of the hero, reaching 7 m.
const ERUPT_RULES: &str = r#"(
    spawns: [
        (kind: "lava_ball", parents: ["hero"], radius: 4.0, within: 1),
        (kind: "fire_blob_splash", parents: ["hero"], radius: 4.0, within: 1),
        (kind: "eruption", parents: ["hero"], radius: 1.0, within: 1),
        (kind: "debris", parents: ["breakable"], radius: 3.0, within: 2),
        (kind: "coin", parents: ["breakable"], radius: 3.0, within: 3),
        (kind: "coin_art", parents: ["breakable"], radius: 3.0, within: 3),
    ],
    rules: (
        chain: Some((
            attack: West,
            throw: Playing(["Models/Characters/MinionsMonsters/Eruptor/Eruptor_Eruption.ma"]),
            ball: "eruption",
            burst: Gone,
            breaks: (kinds: ["breakable"], broken: Gone, coin: "coin"),
            reach: 7.0,
            window: 0.2,
        )),
        breaks: Some((kinds: ["breakable"], broken: Gone, coin: "coin")),
        pickups: Some((kinds: ["coin"], worth: "worth", collector: "hero", reach: 3.6576)),
        gains: ["gold"],
    ),
    quantum: 0.005,
    stamps_past_horizon: true,
)"#;

/// Each group: its script, its rules, its perturbed replays and the frame
/// AH.9 found its generator agreeing through.
const GROUPS: [(&str, &str, &[&str], u64); 5] = [
    (
        "lob",
        LOB_RULES,
        &["lob_p1", "lob_m1", "lob_p2", "lob_m2"],
        0,
    ),
    (
        "erupt",
        ERUPT_RULES,
        &["erupt_p1", "erupt_m1", "erupt_p2", "erupt_m2"],
        0,
    ),
    ("lobs", LOB_RULES, &[], 3),
    ("erupts", ERUPT_RULES, &[], 1),
    ("coin", LOB_RULES, &[], 3),
];

struct Group {
    script: &'static str,
    rules: ReportRules,
    replays: Vec<Replay>,
    perturbed: Vec<Replay>,
    horizon: u64,
}

fn root() -> Option<PathBuf> {
    let Some(dir) = env::path(env::CAPTURE_DIR) else {
        eprintln!("skipping: {} is unset (env or .env.dev)", env::CAPTURE_DIR);
        return None;
    };
    let found = [dir.clone(), dir.join("BLUS30968")]
        .into_iter()
        .find(|d| d.join(MAIN).is_dir());
    assert!(
        found.is_some(),
        "no {MAIN} under {} or its BLUS30968/",
        dir.display()
    );
    found
}

fn groups() -> Option<&'static [Group]> {
    static GROUPS_READ: OnceLock<Option<Vec<Group>>> = OnceLock::new();
    GROUPS_READ
        .get_or_init(|| {
            let root = root()?;
            Some(GROUPS.iter().map(|g| read(&root, g)).collect())
        })
        .as_deref()
}

fn read(
    root: &Path,
    &(script, rules, perturbed, horizon): &(&'static str, &str, &[&str], u64),
) -> Group {
    let rules = ReportRules::from_ron(rules).unwrap();
    let replays = (0..10)
        .map(|k| load(&root.join(MAIN).join(format!("{script}-{k:02}")), &rules).unwrap())
        .collect();
    let perturbed = perturbed
        .iter()
        .map(|p| load(&root.join(PERTURB).join(p), &rules).unwrap())
        .collect();
    Group {
        script,
        rules,
        replays,
        perturbed,
        horizon,
    }
}

/// What anchor A's judge widens `kind` by, a press of any button included.
fn named_floor(kind: &str) -> f64 {
    anchor_a::EVENTS.judge(Some(0)).floor(kind)
}

#[test]
#[ignore = "reads AH.9's oracle replays, outside the repo"]
fn named_floors_are_the_groups_floors() {
    let Some(groups) = groups() else {
        return;
    };
    let mut largest: BTreeMap<String, (f64, &str)> = BTreeMap::new();
    for g in groups {
        let r = GroupReport::build(&g.replays, &[], &g.rules).unwrap();
        println!("== {}\n{}", g.script, r.text());
        assert_eq!(r.horizon.last_agreeing, Some(g.horizon), "{}", g.script);
        assert!(
            r.leave_one_out.iter().all(|j| j.passed),
            "{} fails its own leave-one-out at its own floors",
            g.script
        );
        for (kind, &floor) in &r.floors {
            let slot = largest.entry(kind.clone()).or_insert((0.0, g.script));
            if floor > slot.0 {
                *slot = (floor, g.script);
            }
        }
    }
    println!("largest per kind over the five groups:");
    for (kind, (floor, script)) in &largest {
        println!(
            "  {kind}: {floor:.3} ({script}), named {:.3}",
            named_floor(kind)
        );
    }
    // The named kinds are exactly the ones AH.9 logged; the judge expands
    // the press floor to every button, and AH.9 pressed South and West.
    let presses = [press_kind(Button::South), press_kind(Button::West)];
    for (kind, _) in anchor_a::EVENTS.kinds {
        assert!(largest.contains_key(*kind), "{kind} never logged");
    }
    for p in &presses {
        assert!(largest.contains_key(p), "{p} never logged");
    }
    assert_eq!(largest.len(), anchor_a::EVENTS.kinds.len() + presses.len());
    for (kind, (floor, script)) in &largest {
        assert!(
            (floor - named_floor(kind)).abs() < 1e-12,
            "{kind}: {script} needs {floor}, named {}",
            named_floor(kind)
        );
    }
    let top = largest.values().map(|v| v.0).fold(0.0, f64::max);
    assert!((top - anchor_a::DEFAULT).abs() < 1e-12);
}

/// The kinds a verdict failed, by actor.
fn failed_kinds(j: &oracle_harness::fightlog::Judged) -> BTreeMap<String, Vec<String>> {
    j.verdict
        .actors
        .iter()
        .filter(|(_, v)| !v.passed())
        .map(|(a, v)| {
            let kinds = v
                .mismatches
                .iter()
                .map(|m| match m {
                    LogMismatch::Stamp { kind, .. } => kind.clone(),
                    LogMismatch::Kind { expected, .. } => format!("{expected} (kind)"),
                    LogMismatch::Missing { expected, .. } => format!("{expected} (missing)"),
                    LogMismatch::Extra { found, .. } => format!("{found} (extra)"),
                })
                .collect();
            (a.clone(), kinds)
        })
        .collect()
}

#[test]
#[ignore = "reads AH.9's oracle replays, outside the repo"]
fn replays_pass_and_perturbations_fail() {
    let Some(groups) = groups() else {
        return;
    };
    let named = anchor_a::EVENTS;
    let mut perturbations = 0;
    for g in groups {
        let r = GroupReport::build_at(&g.replays, &g.perturbed, &g.rules, Some(&named)).unwrap();
        println!("== {} at anchor A's named floors\n{}", g.script, r.text());
        assert!(r.named);
        for j in &r.leave_one_out {
            assert!(
                j.passed,
                "{} fails at the named floors: {:?}",
                j.label,
                failed_kinds(j)
            );
        }
        for (c, j) in &r.candidates {
            let failed = failed_kinds(j);
            println!("{}: {failed:?}", c.label);
            assert!(!j.passed, "{} passes", c.label);
            // Not the input alone: a kind the game times fails too.
            let game = failed.values().flatten().any(|k| !k.starts_with("press "));
            assert!(game, "{} fails on its press alone", c.label);
            perturbations += 1;
        }
        // A break that dropped no coin: its coin's log gone.
        if g.script == "lob" {
            for (k, replay) in r.replays.iter().enumerate() {
                let mut logs: BTreeMap<String, Vec<ActorEvent>> = replay.logs.clone().into();
                let coin = "barrel_b_intact(1)>coin#1";
                assert!(logs.remove(coin).is_some(), "{coin} in {}", replay.label);
                let others: Vec<ActorLogs> = r
                    .replays
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i != k)
                    .map(|(_, x)| x.logs.clone())
                    .collect();
                let reference = guac_oracle::fight::log::LogReference::from_runs(&others).unwrap();
                let v = reference
                    .check(&ActorLogs::new(logs).unwrap(), &r.judge)
                    .unwrap();
                assert_eq!(v.failed(), [coin], "{}", replay.label);
            }
        }
    }
    assert_eq!(perturbations, 8);
}
