//! Live (openspec AH.5): O6's chains ride O5's records through a walk without
//! a pause. AF.8's fresh state (or whatever gameplay state
//! `GUAC_ORACLE_SAVESTATE` names) boots to Start Paused's pause (F0), gets
//! `RECONNECT` and a fresh copy of the figure; there Giants' watch list (the
//! fight's cells and the generator's words on it) and Giants' three chains are
//! resolved through PINE, the chains armed and checked against O6's walk run
//! by the harness over the same memory, then the watch. AF.11's walk
//! ([`oracle_harness::divergence::WALK`]) runs to F0 + 600 with no pause. The
//! actor chain keeps the fight's kinds (the hero, the Chompies, Norticus, the
//! breakables in reach, Eruptor's projectiles) and the clearing's two leaf
//! templates' clones, which spawn and die through the walk: the live case of
//! a list growing and shrinking, a clone's placement, a slot taken again and
//! a dying actor's body.
//!
//! Checked: the ring holds every game frame once (AG.35's check) with a block
//! for each chain in every record, every list read, nothing dropped past a
//! chain's items, every kept actor's body readable in every record it's
//! listed; the generator's words in every record, counted into draws since
//! F0 from a peek there; the last record's kept actors are AF.9's reader's
//! actors at the end pause filtered by the same names, corpus for corpus, and
//! O6's last blocks equal the harness's walk at the end pause; the decoded
//! records make `guac_oracle`'s fight trace, keyed by actor. Measured and
//! recorded: how many actors of each kind came and went, the slots taken
//! again, the bodies' agreement with AF.9's at the end.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/ah5-chain-<unix time>/`
//! (refused inside any work tree): `run.txt` opening on the identity line, the
//! list and the chains, the ring (`watch.json`, `chain.json`, `chain.bin`),
//! the decoded recording and chain records and the actor frames, since all of
//! it is guest memory. Mac, against `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`;
//! reads `GUAC_GIANTS_DUMP`, `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and
//! `GUAC_ORACLE_SAVESTATE` from the env or `.env.dev`, and skips when one is
//! unset.
//!
//! `cargo test -p oracle-harness --test live_chain -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_ir::{DumpIdentity, Sha256};
use guac_oracle::fight::draws::draws_since;
use guac_oracle::fight::oracle::fight_trace;
use guac_oracle::fight::ticks::HERO;
use guac_oracle::input::InputScript;
use oracle_harness::actors::{self, ActorFrame};
use oracle_harness::chains::{self, ACTORS, GiantsChains, actor_range};
use oracle_harness::divergence::{RECORDING_FILE, WALK};
use oracle_harness::env;
use oracle_harness::fight::{self, FightWatch, Rows};
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::rng::Peek;
use oracle_harness::snapshot::Snapshot;
use oracle_harness::verbs::{ChainBlock, name_hash};
use oracle_harness::watch::{GiantsWatch, RawRecord, RingCheck, WatchFile};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(180);
/// AF.11's walk with its stop and rest (AG.35's `live_watch` length).
const FLIPS: u64 = 600;
/// The ring: the walk with room to spare.
const RING: u32 = 1_024;
/// The clearing's two leaf templates, whose clones the spawners keep making.
const LEAVES: [&str; 2] = ["leaves Blowing in air ", " leaves blowing along ground "];

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn write_json(dir: &Path, name: &str, value: &impl serde::Serialize) {
    std::fs::write(dir.join(name), serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// What two blocks have to agree on even when a body moved between them.
fn kept(b: &ChainBlock) -> Vec<(u32, u32)> {
    b.items.iter().map(|i| (i.address, i.value)).collect()
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, arms O6's chains on Giants' actors, orbs and loot with O5's watch and records a 600-flip walk without a pause, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_chains_ride_a_walk_every_game_frame_without_a_pause() {
    let (Some(figure), Some(state), Some(capture_root), Some(dump)) = (
        env::path(FIGURE),
        env::path(STATE),
        env::path(env::CAPTURE_DIR),
        env::path(env::GIANTS_DUMP),
    ) else {
        eprintln!(
            "skip: set {FIGURE}, {STATE} (a gameplay state), {} (outside the repo) and {}",
            env::CAPTURE_DIR,
            env::GIANTS_DUMP
        );
        return;
    };
    let Some(live) = common::live() else { return };
    let progression = guac_extract::progression::read(
        &dump.join("PS3_GAME/USRDIR"),
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([0; 32]),
        },
    )
    .unwrap();
    let rows = Rows::of(&progression).unwrap();
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let figure_hash = sha256(&figure);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("ah5-chain-{stamp}")),
    )
    .unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut rec = Record(std::fs::File::create(out.join("run.txt")).unwrap());
    let identity = Identity::of(&live.exe, &live.home.config_dir())
        .unwrap()
        .line();
    rec.line(&identity);
    rec.line(format!(
        "start: {}; {}; load average {}; uptime: {}",
        screen_lock(),
        hid_idle(),
        load_average(),
        uptime()
    ));

    // ---- F0: the state's pause, a fresh figure, the list and the chains -------
    let launched = Instant::now();
    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            savestate: Some(state.clone()),
            borderless: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    let held = oracle
        .wait_paused(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    let f0 = held.frames;
    rec.line(format!(
        "--savestate {} ({}) boot: Start Paused at frame {f0} after {:.1} s",
        state.file_name().unwrap().to_string_lossy(),
        &sha256(&state)[..16],
        launched.elapsed().as_secs_f64()
    ));
    let control = oracle.control();
    control.driver().reconnect().unwrap();
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    let mut pine = common::pine_connect(&oracle.run_paths().pine_socket);
    let mut at_f0: ActorFrame = actors::read_frame(&mut pine).unwrap();
    at_f0.flip = Some(f0);
    at_f0.write(&out.join("actors-f0.json")).unwrap();
    let peek = Peek::read(&mut pine).unwrap();
    write_json(&out, "peek-f0.json", &peek);
    let t = Instant::now();
    let watch = GiantsWatch::resolve(&mut pine).unwrap();
    let (o5_ranges, o5_bytes) = (watch.list.ranges().len(), watch.list.bytes());
    let s0 = fight::read_paused(&mut pine, watch.hero_placement, &rows).unwrap();
    let watch = watch
        .with_fight(FightWatch::of(&s0, &rows).unwrap())
        .unwrap();
    let mut kinds = chains::fight_kinds();
    kinds.push((
        "leaf".into(),
        LEAVES.iter().map(|n| (*n).to_owned()).collect(),
    ));
    let giants = GiantsChains::resolve(&mut pine, &watch, &kinds).unwrap();
    let resolve_ms = t.elapsed().as_secs_f64() * 1e3;
    write_json(&out, "list.json", &watch);
    write_json(&out, "chains.json", &giants);
    rec.line(format!(
        "the list and the chains resolved in {resolve_ms:.0} ms: O5's list {o5_ranges} ranges \
         ({o5_bytes} bytes) without the fight's cells, {} ranges and {} bytes with them and the \
         generator's words (at {:#010x}); hero {} (corpus {:#010x}); the actor list through \
         updater {}, `combat control data` {:#010x}, `Macros_UI` {:#010x}; {} actor names in {} \
         kinds",
        watch.list.ranges().len(),
        watch.list.bytes(),
        watch.mt.unwrap(),
        watch.hero_name.as_deref().unwrap_or("?"),
        watch.hero_corpus,
        giants.updater_index,
        giants.combat_control,
        giants.macros_ui,
        giants.actors.len(),
        kinds.len() + 1
    ));
    for (slot, spec) in &giants.chains.slots {
        rec.line(format!(
            "  chain {slot}: {} items of {} bytes, {} values, {} ranges, a {}-byte block; {}",
            spec.items,
            spec.item_bytes(),
            spec.values.len(),
            spec.ranges.len(),
            spec.bytes(),
            oracle_harness::verbs::Chain::Arm {
                slot: *slot,
                spec: spec.clone()
            }
            .encode()
            .trim_end()
            .len()
        ));
    }
    // Armed, each chain's answer against the harness's own walk at F0.
    let armed = control.chains_arm(&giants.chains).unwrap();
    let mut at_arm = BTreeMap::new();
    for a in &armed {
        let mine = chains::evaluate(&giants.chains.slots[&a.slot], &mut pine).unwrap();
        rec.line(format!(
            "  CHAIN {}: listed {} kept {}; the harness's walk at F0: status {}, listed {}, \
             unread {}, matched {}, kept {}",
            a.slot,
            a.listed,
            a.kept,
            mine.status,
            mine.listed,
            mine.unread,
            mine.matched,
            mine.items.len()
        ));
        assert_eq!(
            (a.listed, a.kept),
            (mine.listed, mine.items.len() as u32),
            "chain {} at F0",
            a.slot
        );
        at_arm.insert(a.slot, mine);
    }
    let f0_kinds: BTreeMap<String, usize> =
        at_arm[&ACTORS]
            .items
            .iter()
            .fold(BTreeMap::new(), |mut m, i| {
                *m.entry(giants.kind(i.value).unwrap().to_owned())
                    .or_default() += 1;
                m
            });
    rec.line(format!("  the actor chain at F0 by kind: {f0_kinds:?}"));
    control
        .watch_arm_chained(&watch.list, &giants.chains, watch.port, RING)
        .unwrap();
    let script = InputScript::from_ron(WALK).unwrap();
    let loaded = control.load_script(&script).unwrap();
    rec.line(format!(
        "armed a ring of {RING} records of {} + {} bytes at every read of port {}; AF.11's walk \
         script, offset 0 on flip {}",
        watch.list.bytes(),
        giants.chains.bytes(),
        watch.port,
        loaded.base
    ));

    // ---- the walk, no pause until its end -------------------------------------
    let t = Instant::now();
    let end = control.pause_at(f0 + FLIPS, WAIT).unwrap();
    let ran = t.elapsed();
    let mut at_end: ActorFrame = actors::read_frame(&mut pine).unwrap();
    at_end.flip = Some(end.frames);
    at_end.write(&out.join("actors-end.json")).unwrap();
    let mut walked = BTreeMap::new();
    for (&slot, spec) in &giants.chains.slots {
        walked.insert(slot, chains::evaluate(spec, &mut pine).unwrap());
    }
    let t = Instant::now();
    let ring = control.watch_read().unwrap();
    let (own, run, decoded) = giants
        .write_outputs(&watch, &ring.records, &mut pine, &identity, &out)
        .unwrap();
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    // A stop waits on PINE's thread, which blocks on an open client (AF.8).
    drop(pine);
    let flips = ring.flips();
    let (pads, reads) = control.pad_readback(watch.port, &flips).unwrap();
    let file = WatchFile {
        identity: identity.clone(),
        watch: watch.clone(),
        held: ring.summary.held,
        total: ring.summary.total,
        frames: ring.summary.frames,
        armed: ring.summary.armed,
        records: own.iter().map(RawRecord::of).collect(),
        pads,
        reads,
    };
    file.write(&out.join(WatchFile::FILE)).unwrap();
    let recording = file.recording().unwrap();
    std::fs::write(
        out.join(RECORDING_FILE),
        serde_json::to_string(&recording).unwrap(),
    )
    .unwrap();
    let check = RingCheck::of(&recording, &own, &file.reads);
    rec.line(format!(
        "paused at {} after {:.2} s running; the ring read, split and named in {read_ms:.0} ms \
         ({:.0} ms of it naming): {} held of {} taken (armed at {}), {} bytes a record; \
         {check:?}",
        end.frames,
        ran.as_secs_f64(),
        run.names_ms,
        ring.summary.held,
        ring.summary.total,
        ring.summary.armed,
        ring.summary.bytes
    ));
    rec.line(format!("chains: {}", serde_json::to_string(&run).unwrap()));

    // ---- every game frame once, a block for each chain in each ---------------
    assert!(check.holds(), "{check:?}");
    assert_eq!(check.last_flip, Some(f0 + FLIPS - 1), "{check:?}");
    assert_eq!(check.reads_mismatch, Some(0), "{check:?}");
    assert_eq!(decoded.len(), own.len());
    for (slot, s) in &run.slots {
        assert_eq!(s.broken, 0, "chain {slot}'s list broke");
        assert_eq!(
            s.dropped, 0,
            "chain {slot} dropped items past its {}",
            giants.chains.slots[slot].items
        );
        assert_eq!(
            s.bodies_missing, 0,
            "chain {slot} kept a body that didn't read"
        );
    }
    let blocks: Vec<BTreeMap<u8, ChainBlock>> = ring
        .records
        .iter()
        .map(|r| giants.chains.split(&watch.list, r).unwrap().1)
        .collect();

    // ---- the actors that came and went ---------------------------------------
    let mut seen: BTreeMap<String, BTreeSet<(u32, Option<u32>)>> = BTreeMap::new();
    let mut listed_runs: BTreeMap<(u32, Option<u32>), (usize, usize)> = BTreeMap::new();
    for (k, r) in decoded.iter().enumerate() {
        for i in &r.items {
            seen.entry(i.kind.clone())
                .or_default()
                .insert((i.address, i.serial));
            if i.kind == "leaf" {
                listed_runs
                    .entry((i.address, i.serial))
                    .and_modify(|e| e.1 = k)
                    .or_insert((k, k));
            }
        }
    }
    let leaves: Vec<_> = listed_runs.values().collect();
    let came = leaves.iter().filter(|(a, _)| *a > 0).count();
    let went = leaves
        .iter()
        .filter(|(_, b)| *b + 1 < decoded.len())
        .count();
    let reused = seen
        .get("leaf")
        .map(|s| {
            let addresses: BTreeSet<u32> = s.iter().map(|(a, _)| *a).collect();
            s.len() - addresses.len()
        })
        .unwrap_or(0);
    rec.line(format!(
        "actors by kind over the walk: {:?}; leaves: {} listed in all, {came} came after F0, \
         {went} left before the end, {reused} slots taken again by another leaf (a serial \
         bumped)",
        seen.iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect::<BTreeMap<_, _>>(),
        leaves.len()
    ));
    // A leaf listed is read whole to its last listing: no body missing (above),
    // and its placement none, a clone's.
    assert!(
        decoded
            .iter()
            .flat_map(|r| &r.items)
            .filter(|i| i.kind == "leaf")
            .all(|i| i.placement.is_none() && i.serial.is_some() && i.body.is_some())
    );
    // The hero's item carries his health, experience and gold every record.
    for r in &decoded {
        let hero = r
            .items
            .iter()
            .find(|i| i.kind == HERO)
            .expect("the hero's item");
        for var in ["health", "experience", "gold"] {
            assert!(
                hero.vars.contains_key(var),
                "{var} at frame {}",
                r.game_frame
            );
        }
    }
    let hero0 = decoded[0].items.iter().find(|i| i.kind == HERO).unwrap();
    rec.line(format!(
        "the hero's item at the first record: {:?}, behaviour {:?}",
        hero0.vars, hero0.behaviour
    ));

    // ---- the generator's draws since F0 ---------------------------------------
    let words: Vec<(u32, guac_oracle::fight::draws::MtWords)> = decoded
        .iter()
        .map(|r| (r.game_frame, r.mt.expect("the generator's words")))
        .collect();
    let draws = draws_since(&peek.mt, &words, 100_000).unwrap();
    let per: Vec<u64> = draws.windows(2).map(|w| w[1].1 - w[0].1).collect();
    rec.line(format!(
        "the generator: {} draws from F0's peek to the last record ({} frames), {}-{} a frame",
        draws.last().unwrap().1,
        draws.len(),
        per.iter().min().unwrap(),
        per.iter().max().unwrap()
    ));

    // ---- the last record against the end pause ---------------------------------
    let last = blocks.last().unwrap();
    for (slot, mine) in &walked {
        let o6 = &last[slot];
        rec.line(format!(
            "chain {slot} at the end: O6's last block listed {} unread {} matched {} kept {}; the \
             harness's walk at the end pause listed {} unread {} matched {} kept {}; same items: \
             {}, same bytes: {}",
            o6.listed,
            o6.unread,
            o6.matched,
            o6.items.len(),
            mine.listed,
            mine.unread,
            mine.matched,
            mine.items.len(),
            kept(o6) == kept(mine),
            o6.items == mine.items
        ));
    }
    // AF.9's reader at the end pause, filtered by the same names.
    let names: BTreeMap<u32, &str> = giants
        .actors
        .iter()
        .map(|(k, n)| (name_hash(n), k.as_str()))
        .collect();
    let af9: BTreeSet<u32> = at_end
        .actors
        .iter()
        .filter(|a| {
            a.name
                .as_deref()
                .is_some_and(|n| names.contains_key(&name_hash(n)))
        })
        .map(|a| a.corpus)
        .collect();
    let o6_last: BTreeSet<u32> = last[&ACTORS].items.iter().map(|i| i.address).collect();
    // The last record is taken at the frame's pad read and the pause comes
    // after the frame's update: an actor that moves has moved on by then.
    let mut worst: BTreeMap<&str, f64> = BTreeMap::new();
    for i in &last[&ACTORS].items {
        let Some(a) = at_end.actors.iter().find(|a| a.corpus == i.address) else {
            continue;
        };
        let m = i.range(actor_range::MATRIX).unwrap();
        let w = worst.entry(giants.kind(i.value).unwrap()).or_default();
        for k in 0..3 {
            let v = f32::from_bits(u32::from_be_bytes(m[4 * k..4 * k + 4].try_into().unwrap()));
            *w = w.max((f64::from(v) - f64::from(a.position[k])).abs());
        }
    }
    rec.line(format!(
        "the last record's {} kept actors against AF.9's reader at the end pause filtered by the \
         same names ({} of {} actors): {}; the most a position moved between, by kind: {}",
        o6_last.len(),
        af9.len(),
        at_end.actors.len(),
        if o6_last == af9 {
            "the same corpora".to_owned()
        } else {
            format!(
                "O6 alone {:?}, AF.9 alone {:?}",
                o6_last.difference(&af9).collect::<Vec<_>>(),
                af9.difference(&o6_last).collect::<Vec<_>>()
            )
        },
        worst
            .iter()
            .map(|(k, v)| format!("{k} {v:.3e} m"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    assert_eq!(
        o6_last, af9,
        "the last record's actors against AF.9's at the end pause"
    );
    assert_eq!(
        kept(&last[&ACTORS]),
        kept(&walked[&ACTORS]),
        "O6's last actor block against the harness's walk at the end pause"
    );

    // ---- the fight trace, keyed by actor ---------------------------------------
    let names = giants.names(&blocks, &file_resolved(&out));
    let trace = fight_trace(&recording, &decoded, &names, &[]).unwrap();
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for t in trace.tracks() {
        *by_kind.entry(t.kind()).or_default() += 1;
    }
    rec.line(format!(
        "fight_trace: {} ticks, {} tracks by kind {by_kind:?}; behaviours named per kind {:?}",
        trace.ticks().len(),
        trace.tracks().count(),
        names
            .behaviours
            .iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect::<BTreeMap<_, _>>()
    ));
    assert_eq!(trace.ticks().len(), decoded.len());

    // ---- shutdown and the dev's dirs --------------------------------------------
    let shutdown = oracle.shutdown().unwrap();
    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    rec.line(format!(
        "shutdown {shutdown:?}; the figure {} {}; the dev's RPCS3 dirs {} ({} entries before); \
         end: {}; {}; load average {}; uptime: {}",
        &figure_hash[..16],
        if sha256(&figure) == figure_hash {
            "unchanged"
        } else {
            "CHANGED"
        },
        if changed.is_empty() {
            "unchanged".to_owned()
        } else {
            format!("CHANGED: {}", changed.join("; "))
        },
        before.len(),
        screen_lock(),
        hid_idle(),
        load_average(),
        uptime()
    ));
    assert_eq!(sha256(&figure), figure_hash, "the figure source changed");
    assert!(changed.is_empty(), "the dev's RPCS3 dirs changed");
}

/// The names `write_outputs` resolved, read back from its file.
fn file_resolved(dir: &Path) -> chains::Resolved {
    let file: chains::ChainFile =
        serde_json::from_str(&std::fs::read_to_string(dir.join(chains::CHAIN_FILE)).unwrap())
            .unwrap();
    file.resolved
}
