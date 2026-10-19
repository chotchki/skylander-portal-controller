//! Live (openspec AF.8): Giants' random number generators through PINE at
//! `PAUSE_AT` pauses, in a fresh gameplay savestate, with nobody at the Mac.
//! AF.5's gameplay state boots to Start Paused's pause, gets `RECONNECT` and a
//! fresh copy of the figure, runs on until the figure's arrival has played out
//! and then takes a fresh state with `SAVESTATE`. At the reboot's pause:
//!
//! - the census: the class meta's live count, `tfbApplication::_random`, every
//!   word in the PPU's areas equal to the class's vtable and what points at
//!   each instance, against the static census (one instance, owned by the
//!   static);
//! - two peeks at one pause are equal; the MT's `sgenrand` origin is the power-on
//!   seed AF.5's state had (no `LOAD`, `SAVESTATE` or reboot reseeds it);
//! - a seed, then a peek, reads `sgenrand(seed)` with `_mti` 624 and the
//!   animation LCG untouched; putting the first peek back reads it again;
//! - then 600 flips under a frame-locked idle script (both pads at rest), a
//!   `PAUSE_AT` every 60 flips, each checkpoint's peek recorded with the numbers
//!   the MT drew and the animation LCG's steps since the last, and the draws
//!   since seeding counted two ways (stepping forward from the first peek,
//!   untwisting back to the seed) that have to agree;
//! - a peek while running is refused.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/af8-rng-<unix time>/`
//! (refused inside any work tree), `run.txt` opening on the identity line; the
//! peeks are written beside it as JSON, since they're guest memory. Mac, against
//! `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`,
//! `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` (a
//! gameplay state this build accepts, booted in a home holding its trophy
//! files, as `live_pad` needs) from the env or `.env.dev`, and skips when one is
//! unset.
//!
//! `cargo test -p oracle-harness --test live_rng -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_oracle::input::InputScript;
use guac_oracle::rng::{Mt19937, Origin};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::PineClient;
use oracle_harness::rng::{self, Hit, MT_GLOBAL, Peek};
use oracle_harness::scan;
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(120);
/// Where the fresh state is taken: by then the figure's arrival from the
/// `LOAD` has played out and Eruptor stands in the clearing (`live_pad`).
const SETTLE: u64 = 700;
const EVERY: u64 = 60;
const CHECKPOINTS: u64 = 10;
/// The seed for the seed check: `sgenrand`'s own default in the 1998 code.
const SEED: u32 = 4357;
const IDLE: &str = "(steps: [(at: 0, pads: {0: (), 1: ()})])";

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn peek_at(pine: &mut PineClient, flip: u64) -> (Peek, Duration) {
    let t = Instant::now();
    let mut peek = Peek::read(pine).unwrap();
    peek.flip = Some(flip);
    (peek, t.elapsed())
}

fn origin(peek: &Peek) -> Origin {
    peek.origin(rng::ORIGIN_TWISTS)
        .unwrap_or_else(|| panic!("no sgenrand seeding behind {}", peek.summary()))
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, takes a fresh one and reads and writes Giants' RNG over PINE, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_rng_census_peek_seed_and_distance_hold_in_a_fresh_gameplay_state() {
    let (Some(figure), Some(state), Some(capture_root)) = (
        env::path(FIGURE),
        env::path(STATE),
        env::path(env::CAPTURE_DIR),
    ) else {
        eprintln!(
            "skip: set {FIGURE}, {STATE} (a gameplay state) and {} (outside the repo)",
            env::CAPTURE_DIR
        );
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let figure_hash = sha256(&figure);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("af8-rng-{stamp}")),
    )
    .unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut rec = Record(std::fs::File::create(out.join("run.txt")).unwrap());
    rec.line(
        Identity::of(&live.exe, &live.home.config_dir())
            .unwrap()
            .line(),
    );
    rec.line(format!(
        "start: {}; {}; load average {}",
        screen_lock(),
        hid_idle(),
        load_average()
    ));

    // ---- AF.5's gameplay state, booted, run on, and saved fresh -------------
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
    rec.line(format!(
        "--savestate {} ({}) boot: Start Paused at frame {} after {:.1} s",
        state.file_name().unwrap().to_string_lossy(),
        &sha256(&state)[..16],
        held.frames,
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
    rng::check_title(&mut pine).unwrap();
    let (booted, _) = peek_at(&mut pine, held.frames);
    let booted_origin = origin(&booted);
    rec.line(format!(
        "AF.5's state at its pause, flip {}: {}; seeded {}, {} drawn since",
        held.frames,
        booted.summary(),
        booted_origin.seed,
        booted_origin.draws
    ));
    control.pause_at(SETTLE, WAIT).unwrap();
    let (settled, _) = peek_at(&mut pine, SETTLE);
    rec.line(format!(
        "flip {SETTLE} (a fresh figure's arrival played out): {}; {:?} since flip {}",
        settled.summary(),
        booted.apart(&settled, rng::MAX_TWISTS),
        held.frames
    ));
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));
    // RPCS3's PINE thread blocks reading its one client, and the stop a
    // savestate makes waits on that thread: a held connection hangs it.
    drop(pine);
    let saved = control.savestate(&out, WAIT).unwrap();
    control.driver().reconnect().unwrap();
    rec.line(format!(
        "SAVESTATE at frame {}: {} ({} bytes, {}), rebooted to {} frame {}; RECONNECT",
        saved.saved_at.frames,
        saved.path.file_name().unwrap().to_string_lossy(),
        std::fs::metadata(&saved.path).unwrap().len(),
        &sha256(&saved.path)[..16],
        saved.rebooted.status,
        saved.rebooted.frames
    ));
    let f0 = saved.rebooted.frames;

    // ---- the census, at the fresh state's pause ------------------------------
    // The reboot restarts PINE's server with the emulator.
    let mut pine = common::pine_connect(&oracle.run_paths().pine_socket);
    rng::check_title(&mut pine).unwrap();
    let t = Instant::now();
    let census = rng::census(&mut pine, &scan::PPU_AREAS).unwrap();
    rec.line(format!(
        "census in {:.1} s: meta {:#010x} counts {} live; tfbApplication::_random ({MT_GLOBAL:#010x}) -> {:#010x}",
        t.elapsed().as_secs_f64(),
        census.peek.meta,
        census.peek.instances,
        census.peek.instance
    ));
    for (what, found) in ["vtable", "owner"].iter().zip(&census.searches) {
        rec.line(format!(
            "  {what} search: {} MB in {} mapped ranges ({}), {} unreadable 4 KiB pages, {} PINE messages",
            found.bytes >> 20,
            found.mapped.len(),
            found
                .mapped
                .iter()
                .map(|r| format!("{:#x}-{:#x}", r.start, r.end))
                .collect::<Vec<_>>()
                .join(" "),
            found.unreadable.len(),
            found.messages
        ));
    }
    for hit in &census.hits {
        let line = match hit {
            Hit::Instance { at, refcount, mti } => format!(
                "  instance {at:#010x}: refcount {refcount}, mti {mti}, pointed at from {:x?}",
                census.owners[at]
            ),
            Hit::MetaField { at } => format!("  {at:#010x}: the meta's own vtable field"),
            Hit::Other { at } => format!("  {at:#010x}: the vtable word, not an object"),
        };
        rec.line(line);
    }
    let instances: Vec<u32> = census
        .hits
        .iter()
        .filter_map(|h| match h {
            Hit::Instance { at, .. } => Some(*at),
            _ => None,
        })
        .collect();
    assert_eq!(census.peek.instances, 1, "the meta counts other than one");
    assert_eq!(
        instances,
        [census.peek.instance],
        "the vtable search found other instances"
    );
    assert!(
        census.owners[&census.peek.instance].contains(&MT_GLOBAL),
        "tfbApplication::_random isn't among the instance's owners"
    );

    // ---- two peeks, the origin, a seed and the peek put back -----------------
    let (a, took) = peek_at(&mut pine, f0);
    let (b, _) = peek_at(&mut pine, f0);
    assert_eq!(a, b, "two peeks at one pause differ");
    assert_eq!(
        a.mt, census.peek.mt,
        "the census's peek and this one differ"
    );
    let a_origin = origin(&a);
    rec.line(format!(
        "two peeks at frame {f0} equal ({} ms each): {}; seeded {}, {} drawn since; \
         {:?} from flip {SETTLE}'s peek before the save",
        took.as_millis(),
        a.summary(),
        a_origin.seed,
        a_origin.draws,
        settled.apart(&a, rng::MAX_TWISTS)
    ));
    assert_eq!(
        a_origin.seed, booted_origin.seed,
        "a LOAD, SAVESTATE or reboot reseeded the MT"
    );
    a.write(&out.join(format!("peek-{f0:05}.json"))).unwrap();

    rng::seed(&mut pine, SEED).unwrap();
    let (seeded, _) = peek_at(&mut pine, f0);
    assert_eq!(seeded.mt, Mt19937::sgenrand(SEED));
    assert_eq!(seeded.mt.mti(), 624);
    assert_eq!(seeded.anim, a.anim, "seeding moved the animation LCG");
    assert_eq!(
        seeded.origin(0),
        Some(Origin {
            seed: SEED,
            draws: 0
        })
    );
    rng::restore(&mut pine, &a).unwrap();
    let (restored, _) = peek_at(&mut pine, f0);
    assert_eq!(restored, a, "the peek put back reads differently");
    let still = control.state().unwrap();
    assert_eq!((still.status.as_str(), still.frames), ("paused", f0));
    rec.line(format!(
        "seed {SEED}, then a peek: sgenrand({SEED}), mti 624, the animation LCG untouched; \
         the first peek put back and read again; STATE still paused at frame {f0}"
    ));

    // ---- 600 idle flips, a peek every 60 ---------------------------------------
    let loaded = control
        .load_script(&InputScript::from_ron(IDLE).unwrap())
        .unwrap();
    rec.line(format!(
        "idle script (both pads at rest), offset 0 on flip {}",
        loaded.base
    ));
    let mut prev = a.clone();
    let mut per_checkpoint = Vec::new();
    for k in 1..=CHECKPOINTS {
        let flip = f0 + EVERY * k;
        let paused = control.pause_at(flip, WAIT).unwrap();
        let (peek, _) = peek_at(&mut pine, paused.frames);
        let step = prev.apart(&peek, rng::MAX_TWISTS);
        let total = a.apart(&peek, rng::MAX_TWISTS);
        let o = origin(&peek);
        let drawn = step
            .mt
            .unwrap_or_else(|| panic!("flip {flip}: the MT isn't within reach of the last peek"));
        assert!(drawn >= 0, "flip {flip}: the MT went back {drawn}");
        assert_eq!(o.seed, a_origin.seed, "flip {flip}: reseeded");
        assert_eq!(
            Some(o.draws as i64 - a_origin.draws as i64),
            total.mt,
            "flip {flip}: stepping forward and untwisting back disagree"
        );
        assert_eq!(peek.instances, 1);
        assert_eq!(peek.instance, a.instance);
        rec.line(format!(
            "flip {flip}: {drawn} drawn since flip {} ({} since {f0}, {} since seeding), \
             animation LCG {:?} steps, FMOD word {:?}; mti {}",
            prev.flip.unwrap(),
            total.mt.unwrap(),
            o.draws,
            step.anim,
            peek.fmod,
            peek.mt.mti()
        ));
        peek.write(&out.join(format!("peek-{flip:05}.json")))
            .unwrap();
        per_checkpoint.push(drawn);
        prev = peek;
    }
    rec.line(format!(
        "draws per {EVERY} flips: {per_checkpoint:?}; {} over {} flips",
        per_checkpoint.iter().sum::<i64>(),
        EVERY * CHECKPOINTS
    ));
    let end_shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "shot at flip {}: {} [{}, {}]",
        end_shot.flip,
        end_shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));

    // ---- running: refused ------------------------------------------------------
    control.resume().unwrap();
    let err = format!("{:#}", Peek::read(&mut pine).unwrap_err());
    assert!(err.contains("Running"), "{err}");
    rec.line(format!("a peek while running: {err}"));
    drop(pine);

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
        "end: {}; {}; load average {}; the figure source unchanged ({}); the dev's RPCS3 dirs, \
         {} entries, unchanged",
        screen_lock(),
        hid_idle(),
        load_average(),
        &figure_hash[..16],
        before.len()
    ));
}
