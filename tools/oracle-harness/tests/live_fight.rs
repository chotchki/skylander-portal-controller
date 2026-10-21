//! Live (openspec AH.4): the fight's state through AH.4's readers
//! ([`oracle_harness::fight`]) at a pause. AF.8's fresh state (or whatever
//! gameplay state `GUAC_ORACLE_SAVESTATE` names) boots to Start Paused's pause
//! (F0), gets `RECONNECT`, and there, read twice (equal):
//!
//! - the hero: his health and full health, his figure's experience, gold,
//!   playtime and level through AH.2's rows (read off the disc), his player and
//!   his next level's threshold;
//! - the difficulty out of the console save, the portal-master experience
//!   multiplier, the element-area flags and the one at the hero;
//! - the level's `EnemyStatsList` as `Macros_UI` holds it, and the level
//!   file's own cells (the enemies' damage and HP cells, the experience
//!   awards) where the level's data section sits, at their disc offsets;
//! - the loot data's running counts;
//! - every trigger placement's wake state (a name with `Trigger`, `trg_`,
//!   `Transition `, `Activate_` or `Pop_` in it), and the first fight's
//!   placements as script instances (the four Chompies, their controller,
//!   Norticus, the level's enemy system): state, behaviour, health and the
//!   alerted flag where their scripts have started.
//!
//! Then Giants' watch list is resolved there and grown with the fight's cells,
//! armed on pad 0's reads, a fresh copy of the figure goes on the portal and an
//! idle script (both pads at rest) runs [`FLIPS`] flips without a pause; every
//! record's fight cells have to decode, and the last record and the readers at
//! the end pause have to agree with F0's (nothing touches him at rest).
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/ah4-readers-<unix time>/`
//! (refused inside any work tree): `run.txt` opening on the identity line, the
//! reads as JSON and the ring raw, since all of it is guest memory. Mac,
//! against `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`,
//! `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` (a
//! gameplay state this build accepts, booted in a home holding its trophy
//! files) from the env or `.env.dev`, and skips when one is unset.
//!
//! `cargo test -p oracle-harness --test live_fight -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::level::Placements;
use guac_ir::{DumpIdentity, Sha256};
use guac_oracle::input::InputScript;
use oracle_harness::actors::{self, ActorFrame};
use oracle_harness::env;
use oracle_harness::fight::{self, FightState, FightWatch, Rows};
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::level;
use oracle_harness::pine::{EmuStatus, PineClient};
use oracle_harness::script::{Instance, Number, Types};
use oracle_harness::snapshot::Snapshot;
use oracle_harness::watch::{GiantsWatch, RawRecord, WatchFile};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(180);
const LEVEL_213: &str = "level/level_213_tutotial.bld";
/// The idle stretch recorded with the fight's cells on the list.
const FLIPS: u64 = 300;
const RING: u32 = 512;
const IDLE: &str = "(steps: [(at: 0, pads: {0: (), 1: ()})])";
/// The level file's cells read in place: the enemies' damage and HP cells
/// `Game - Populate Enemy Stats List` writes (or, for the HP literals, never
/// does), the experience awards and the level's difficulty rating.
const LEVEL_CELLS: [&str; 10] = [
    "Enemy_Chompy_DirectDMG",
    "Enemy_Chompy_DoTDMG",
    "Enemy_Chompy_HP",
    "PandoranGrunt_DMG",
    "PandoranGrunt_DMG_Simple",
    "PandoranGrunt_HP",
    "XP_Value_Near",
    "XP_Value_Far",
    "XP_Value_HeavyNear",
    "Difficulty_Rating_Current_Level",
];
/// The first fight's placements, read as script instances.
const ENCOUNTER: [&str; 8] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "Setup_001_Controller",
    "1.NPC_Norticus_First_Encounter",
    "Norticus_First_Encounter(1)",
    "EnemySystem_",
];
/// The route's triggers AH.8 times, watched per frame.
const ROUTE: [&str; 2] = ["Setup_001_Trigger", "Activate_Norticus and Chompies"];
const HEALTH: [&str; 3] = ["description", "creature info", "hp"];
const ALERTED: [&str; 3] = ["description", "attention parameters", "alerted"];

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

fn write_json<T: serde::Serialize>(out: &Path, name: &str, value: &T) {
    std::fs::write(out.join(name), serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn is_trigger(name: &str) -> bool {
    name.contains("Trigger")
        || name.contains("trigger")
        || ["trg_", "Transition ", "Activate_", "Pop_"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// Level 213's `level.bld` off the dump.
fn level_bld(usrdir: &Path) -> Vec<u8> {
    let bytes = std::fs::read(usrdir.join(LEVEL_213)).unwrap();
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc.find("level.bld").unwrap();
    arc.read(entry).unwrap().into_owned()
}

/// What the disc gives the live reads: the actor placements' offsets by name
/// (the names it lists once), and section 0's named `ValueInfo`s.
struct Disc {
    placements: BTreeMap<String, u32>,
    triggers: Vec<String>,
    cells: BTreeMap<String, u32>,
}

fn disc(usrdir: &Path) -> Disc {
    let data = level_bld(usrdir);
    let igz = Igz::parse(&data).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let mut seen: BTreeMap<String, (usize, u32)> = BTreeMap::new();
    for a in &placements.actors {
        let at = igz.objects()[a.index].at;
        assert_eq!(at.section(), 0);
        let e = seen
            .entry(a.name().unwrap_or("").to_owned())
            .or_insert((0, at.offset()));
        e.0 += 1;
    }
    let unique: BTreeMap<String, u32> = seen
        .into_iter()
        .filter(|(n, (k, _))| *k == 1 && !n.is_empty())
        .map(|(n, (_, off))| (n, off))
        .collect();
    let triggers = unique.keys().filter(|n| is_trigger(n)).cloned().collect();
    let mut cells: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for o in igz.objects_of_type("ValueInfo") {
        if o.at.section() != 0 {
            continue;
        }
        let Some(name) = o
            .field(0x08)
            .and_then(|f| igz.string(f, "_name").ok().flatten())
        else {
            continue;
        };
        cells
            .entry(name.to_owned())
            .or_default()
            .push(o.at.offset());
    }
    let cells = cells
        .into_iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(n, v)| (n, v[0]))
        .collect();
    Disc {
        placements: unique,
        triggers,
        cells,
    }
}

fn num(n: Number) -> String {
    match n {
        Number::Int(v) => format!("{v} (int)"),
        Number::Float(v) => format!("{v} (float, bits {:#010x})", v.to_bits()),
    }
}

fn state_lines(rec: &mut Record, s: &FightState) {
    let h = &s.hero;
    let f = &h.figure;
    rec.line(format!(
        "  hero {} ({:#010x}, behaviour {:?}): health {} of {}, invulnerable {}; player {}, next \
         level at {}",
        h.name.as_deref().unwrap_or("?"),
        h.placement,
        h.behaviour,
        num(h.health),
        num(h.health_max),
        num(h.invulnerable),
        h.player,
        h.exp_target
            .map_or_else(|| "none (the cap)".to_owned(), num)
    ));
    rec.line(format!(
        "  figure (tag data {:#010x}, {} save words): experience {}, gold {}, playtime {} s, \
         level {}; words 0-2 {:08x} {:08x} {:08x}, 28-29 {:08x} {:08x}",
        f.tag_data,
        f.words.len(),
        f.experience,
        f.gold,
        f.playtime_secs,
        f.level,
        f.words[0],
        f.words[1],
        f.words[2],
        f.words.get(28).copied().unwrap_or(0),
        f.words.get(29).copied().unwrap_or(0)
    ));
    rec.line(format!(
        "  difficulty {} (bit {}, {} bits, {}, {} words); accolade xp multiplier {}",
        s.difficulty.value,
        s.difficulty.bit,
        s.difficulty.bits,
        if s.difficulty.header {
            "header"
        } else {
            "save"
        },
        s.difficulty.words,
        num(s.xp_multiplier.value)
    ));
    let m = &s.xp_multiplier;
    rec.line(format!(
        "  the multiplier's accolades: {} of {} done ({}); table {}; by the game's rule {:?}",
        m.completed(),
        m.accolades.len(),
        m.accolades
            .iter()
            .filter(|(_, done)| *done)
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        m.table
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        m.by_rule()
    ));
    rec.line(format!(
        "  area: P1 {}, P2 {}, current {}; at the hero {}",
        num(s.area.p1),
        num(s.area.p2),
        num(s.area.current),
        num(s.area_at_hero)
    ));
    for e in &s.enemy_stats {
        rec.line(format!(
            "  enemy stats: {}: hp {}, xp {}",
            e.kind.as_deref().unwrap_or("(none)"),
            num(e.hp),
            num(e.xp)
        ));
    }
    for a in &s.loot {
        rec.line(format!(
            "  loot {}: index {}, rate {}",
            a.name,
            num(a.index),
            num(a.rate)
        ));
    }
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, reads the fight's state over PINE with AH.4's readers and records an idle stretch with the fight's cells on O5's list, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_fights_state_reads_at_a_pause_and_rides_the_watch_list() {
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

    // ---- the disc ------------------------------------------------------------
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let t = Instant::now();
    let d = disc(&usrdir);
    // The rows don't read the identity; the dump's own hash isn't worth its
    // minutes here.
    let progression = guac_extract::progression::read(
        &usrdir,
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([0; 32]),
        },
    )
    .unwrap();
    let rows = Rows::of(&progression).unwrap();
    let disc_ms = t.elapsed().as_millis();

    // ---- the run -------------------------------------------------------------
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let figure_hash = sha256(&figure);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("ah4-readers-{stamp}")),
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
    rec.line(format!(
        "disc ({disc_ms} ms): {} actor names listed once, {} of them triggers, {} named \
         ValueInfos in section 0; AH.2's rows: experience bits {:?} + {:?}, gold {:?}, playtime \
         {:?}, {} levels to {}",
        d.placements.len(),
        d.triggers.len(),
        d.cells.len(),
        rows.experience,
        rows.spillover,
        rows.gold,
        rows.playtime,
        rows.level_xp.len(),
        rows.level_xp.last().unwrap()
    ));

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
    let mut pine: PineClient = common::pine_connect(&oracle.run_paths().pine_socket);

    // ---- F0: the state, twice ----------------------------------------------------
    let mut frame: ActorFrame = actors::read_frame(&mut pine).unwrap();
    frame.flip = Some(f0);
    frame.write(&out.join("actors-f0.json")).unwrap();
    let hero = frame.hero().unwrap().clone();
    let section = level::section_base(&frame, &d.placements).unwrap();
    let t = Instant::now();
    let s0 = fight::read_paused(&mut pine, hero.placement, &rows).unwrap();
    let read_ms = t.elapsed().as_millis();
    let again = fight::read_paused(&mut pine, hero.placement, &rows).unwrap();
    assert_eq!(s0, again, "two reads at one pause differ");
    write_json(&out, "fight-f0.json", &s0);
    rec.line(format!(
        "F0: {}; section 0 at {:#010x} ({} actors agree); the fight's state read in {read_ms} ms, \
         twice, equal:",
        frame.summary(),
        section.base,
        section.votes
    ));
    state_lines(&mut rec, &s0);

    // The level's own cells, in place.
    let wanted: Vec<(&str, u32)> = LEVEL_CELLS
        .iter()
        .map(|&n| {
            (
                n,
                *d.cells
                    .get(n)
                    .unwrap_or_else(|| panic!("{n} not on the disc")),
            )
        })
        .collect();
    let mut types = Types::default();
    oracle_harness::rng::check_title(&mut pine).unwrap();
    let cells = fight::level_values(&mut pine, &mut types, section.base, &wanted).unwrap();
    let cells_again = fight::level_values(&mut pine, &mut types, section.base, &wanted).unwrap();
    assert_eq!(cells, cells_again, "two reads at one pause differ");
    write_json(&out, "level-cells-f0.json", &cells);
    for c in &cells {
        rec.line(format!(
            "  level cell {} at {:#010x} (disc offset {:#x}): {}",
            c.name,
            c.at,
            c.at - section.base,
            num(c.value)
        ));
    }

    // The triggers and the first fight's placements.
    let at = |name: &str| section.base + d.placements[name];
    let trigger_at: Vec<(&str, u32)> = d.triggers.iter().map(|n| (n.as_str(), at(n))).collect();
    let triggers = fight::triggers(&mut pine, &trigger_at).unwrap();
    write_json(&out, "triggers-f0.json", &triggers);
    let fired: Vec<String> = triggers
        .iter()
        .filter(|t| t.fired == Some(true))
        .map(|t| {
            format!(
                "{} ({} ft, state {}, {:.2} s)",
                t.name, t.range, t.activity_state, t.active_seconds
            )
        })
        .collect();
    let rangeless: Vec<String> = triggers
        .iter()
        .filter(|t| t.fired.is_none())
        .map(|t| {
            format!(
                "{} (state {}, {:.2} s)",
                t.name, t.activity_state, t.active_seconds
            )
        })
        .collect();
    rec.line(format!(
        "  triggers: {} read; {} range triggers fired: {}; {} without a range (their \
         controllers' check decides): {}",
        triggers.len(),
        fired.len(),
        fired.join("; "),
        rangeless.len(),
        rangeless.join("; ")
    ));
    let route: Vec<_> = triggers
        .iter()
        .filter(|t| ROUTE.contains(&t.name.as_str()))
        .cloned()
        .collect();
    assert_eq!(route.len(), ROUTE.len());
    for t in &route {
        rec.line(format!(
            "  the route's {}: {} ft, state {}, started {}, active {:.2} s, fired {:?}",
            t.name, t.range, t.activity_state, t.started, t.active_seconds, t.fired
        ));
        assert_eq!(t.fired, Some(false), "{} fired at the spawn", t.name);
    }
    for name in ENCOUNTER {
        let i = Instance::read(&mut pine, at(name)).unwrap();
        let health = i.number(&mut pine, &mut types, &HEALTH);
        let alerted = i.number(&mut pine, &mut types, &ALERTED);
        let show = |r: anyhow::Result<Option<Number>>| match r {
            Ok(Some(n)) => num(n),
            Ok(None) => "(script not started)".into(),
            Err(e) => format!("(none: {e:#})"),
        };
        rec.line(format!(
            "  {name} ({:#010x}): state {}, range {} ft, started {}, active {:.3} s, behaviour \
             {:?}, {} variables; health {}; alerted {}",
            i.placement,
            i.activity_state,
            i.activation_range,
            i.started,
            i.active_seconds,
            i.behaviour,
            i.variables.as_ref().map_or(0, Vec::len),
            show(health),
            show(alerted)
        ));
    }
    let hero_i = Instance::read(&mut pine, hero.placement).unwrap();
    let hero_alerted = hero_i.number(&mut pine, &mut types, &ALERTED).unwrap();
    rec.line(format!(
        "  the hero's own instance: {} variables, active {:.3} s, alerted {}",
        hero_i.variables.as_ref().map_or(0, Vec::len),
        hero_i.active_seconds,
        hero_alerted.map_or("?".into(), num)
    ));
    assert_eq!(
        pine.status().unwrap(),
        EmuStatus::Paused,
        "the reads ran paused"
    );

    // ---- the box's numbers -----------------------------------------------------
    let cell = |name: &str| cells.iter().find(|c| c.name == name).unwrap().value;
    assert_eq!(
        cell("Enemy_Chompy_DirectDMG"),
        Number::Int(11),
        "medium's bite"
    );
    assert_eq!(cell("PandoranGrunt_DMG"), Number::Int(36), "medium's Grunt");
    assert!(
        matches!(cell("XP_Value_Near"), Number::Float(v) if (v - 128.333).abs() < 1e-3),
        "{:?}",
        cell("XP_Value_Near")
    );
    let chompy = s0
        .enemy_stats
        .iter()
        .find(|e| e.kind.as_deref() == Some("Enemy_Chompy"))
        .expect("a Chompy row");
    assert_eq!(
        chompy.hp,
        Number::Int(1),
        "the Chompy's health as the list holds it"
    );
    assert_eq!(s0.difficulty.value, 1, "medium");
    let by_rule = s0
        .xp_multiplier
        .by_rule()
        .expect("the multiplier's table entry");
    assert!(
        (s0.xp_multiplier.value.as_f64() - by_rule).abs() < 1e-6,
        "the multiplier {:?} against the rule's {by_rule}",
        s0.xp_multiplier.value
    );
    assert_eq!(s0.hero.health, Number::Int(290), "Eruptor at level 1");
    assert_eq!(s0.hero.health_max, Number::Int(290));
    assert_eq!(s0.hero.figure.level, rows.level(s0.hero.figure.experience));
    assert_eq!(
        s0.area_at_hero,
        Number::Int(0),
        "no element area at the spawn"
    );

    // ---- the watch list grown, armed; a fresh figure, idle ------------------------
    let t = Instant::now();
    let watch = GiantsWatch::resolve(&mut pine).unwrap();
    let base_ranges = watch.list.ranges().len();
    let base_bytes = watch.list.bytes();
    let watch = watch
        .with_fight(FightWatch::of(&s0, &rows).unwrap().with_triggers(&route))
        .unwrap();
    let fw = watch.fight.as_ref().unwrap();
    rec.line(format!(
        "the list resolved and grown in {:.0} ms: {} ranges and {} bytes a record before the \
         fight's cells, {} and {} with them ({} cells: {}; the route's triggers {:?})",
        t.elapsed().as_secs_f64() * 1e3,
        base_ranges,
        base_bytes,
        watch.list.ranges().len(),
        watch.list.bytes(),
        fw.cells.len(),
        fw.cells
            .iter()
            .map(|(l, a)| format!("{l} {a:#010x}"))
            .collect::<Vec<_>>()
            .join(", "),
        fw.triggers
    ));
    write_json(&out, "list.json", &watch);
    control.watch_arm(&watch.list, watch.port, RING).unwrap();
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    let loaded = control
        .load_script(&InputScript::from_ron(IDLE).unwrap())
        .unwrap();
    rec.line(format!(
        "armed a ring of {RING} on port {}; LOAD of a fresh figure copy; the idle script, \
         offset 0 on flip {}",
        watch.port, loaded.base
    ));
    let end = control.pause_at(f0 + FLIPS, WAIT).unwrap();
    let s1 = fight::read_paused(&mut pine, hero.placement, &rows).unwrap();
    write_json(&out, "fight-end.json", &s1);
    rec.line(format!("paused at {}: the readers again:", end.frames));
    state_lines(&mut rec, &s1);
    // A stop waits on PINE's thread, which blocks on an open client (AF.8).
    drop(pine);
    let ring = control.watch_read().unwrap();
    let flips = ring.flips();
    let (pads, reads) = control.pad_readback(watch.port, &flips).unwrap();
    let file = WatchFile {
        identity: identity.clone(),
        watch: watch.clone(),
        held: ring.summary.held,
        total: ring.summary.total,
        frames: ring.summary.frames,
        armed: ring.summary.armed,
        records: ring.records.iter().map(RawRecord::of).collect(),
        pads,
        reads,
    };
    file.write(&out.join(WatchFile::FILE)).unwrap();
    let decoded: Vec<_> = ring
        .records
        .iter()
        .map(|r| {
            watch
                .fight(r)
                .unwrap_or_else(|| panic!("flip {}: no fight cells", r.flip))
        })
        .collect();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for f in &decoded {
        *kinds
            .entry(format!(
                "health {:?}, experience {}, gold {}, playtime {}, level {}, area {:?}/{:?}, loot \
                 {:?}, triggers {:?}",
                f.health,
                f.experience,
                f.gold,
                f.playtime_secs,
                f.level,
                f.area_p1,
                f.area_p2,
                f.loot,
                f.triggers
                    .iter()
                    .map(|t| t.activity_state)
                    .collect::<Vec<_>>()
            ))
            .or_default() += 1;
    }
    rec.line(format!(
        "the ring: {} records held of {} (flips {:?}-{:?}), every one's fight cells decoded; \
         distinct values: {}",
        ring.summary.held,
        ring.summary.total,
        flips.first(),
        flips.last(),
        kinds
            .iter()
            .map(|(k, n)| format!("[{n}x] {k}"))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    let last = decoded.last().expect("a record");
    assert_eq!(last.health, s1.hero.health);
    assert_eq!(last.health_max, s1.hero.health_max);
    assert_eq!(
        (last.experience, last.gold, last.level),
        (
            s1.hero.figure.experience,
            s1.hero.figure.gold,
            s1.hero.figure.level
        )
    );
    assert_eq!(last.area_p1, s1.area.p1);
    assert_eq!(Some(&last.loot), s1.loot.first().map(|a| &a.index));
    assert_eq!(s1.hero.health, s0.hero.health, "at rest nothing hurt him");
    assert_eq!(
        (s1.hero.figure.experience, s1.hero.figure.gold),
        (s0.hero.figure.experience, s0.hero.figure.gold)
    );

    rec.line(format!("shutdown: {:?}", oracle.shutdown().unwrap()));
    std::fs::copy(
        live.home.cache_dir().join("RPCS3.log"),
        out.join("RPCS3.log"),
    )
    .unwrap();
    assert_eq!(sha256(&figure), figure_hash, "the figure source changed");
    let changed_dirs = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed_dirs.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed_dirs.join("\n")
    );
    rec.line(format!(
        "end: {}; {}; load average {}; uptime: {}; the figure source unchanged ({}); the dev's \
         RPCS3 dirs, {} entries, unchanged",
        screen_lock(),
        hid_idle(),
        load_average(),
        uptime(),
        &figure_hash[..16],
        before.len()
    ));
}
