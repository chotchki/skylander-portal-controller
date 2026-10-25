//! AH.49's disc tests: Eruptor's locomotion picked on the VM, by
//! `global.bld`'s locomotion script (`Macros_Animation.ai`'s every tick),
//! against AG.11's recordings record by record, the check `guac-motion`'s
//! `tests/gaits.rs` holds the stand-in to (stand-in ledger: `locomotion`).
//! Ignored by default; they read `GUAC_GIANTS_DUMP` and the recordings under
//! `GUAC_CAPTURE_DIR` (each from the process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --release --test giants_locomotion -- --ignored --nocapture --test-threads=1`
//!
//! A dev aid, not a check: [`probe_one_replay`] replays one recording
//! (`AH49_REPLAY=<run>/<label>`), printing each record's weights, rates and
//! times beside the record's with `AH49_TRACE=<part of its name>`, the
//! locomotion's ops each record ran with `AH49_OPS=<from>-<to>`, and where
//! op ids sit with `AH49_OPMAP=<id>,...`.
//!
//! Each replay runs the locomotion's tick alone over what each record held:
//! the run joined at the spawn in strict mode (his setup's "Minion - Define
//! Locomotion States" and "Minion - Initialize Locomotions" on the VM), then
//! per record his walk set where his movement script leaves it for the
//! locomotion to read (after Face and Move, before Update Movers: AG.11's
//! speed, replayed the way `gaits.rs` replays it), his move down and his
//! height as the frame before left them, his facing, the frame's slides, the
//! locomotion's tick at the record's step and the sequences on by that step.
//! What it wrote on his clips (their `blend percent`s and rates, and the
//! clips it played) is then the record's own: each of the five clips'
//! weight, against the recorded weight, raw, both of the forms a record
//! holds the gaits in included (`gaits.rs` compares the idle and the run's
//! share because AG.30's locomotion draws one form only).

mod common;
#[path = "../../guac-motion/tests/oracle/mod.rs"]
mod oracle;

use std::collections::BTreeMap;

use common::*;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_motion::tuning::{FOOT, Tuning};
use guac_motion::walk::Walk;
use guac_oracle::recording::Frame as Record;
use guac_script::Mode;
use oracle::{consecutive, field, footing, frames, replay_dirs, stored};

/// AG.11's five boxes, as `gaits.rs` replays them.
const RUNS: [&str; 8] = [
    "ag8-motion-1792423086",
    "ag8-level-1792424606",
    "ag8r2-axes-1792466142",
    "ag9-contacts-1792471160",
    "ag9-contacts-1792472400",
    "ag36-edges-1792474678",
    "ag38-switch-1792509065",
    "ag39-falls-1792502813",
];

/// The runs whose sixth clip is a fall (AG.9's `b` presses add an attack's).
const FALLS: [&str; 2] = ["ag36-edges-1792474678", "ag39-falls-1792502813"];

/// AG.58's 13 set O recordings, every record on the walk's own speed.
const AG58: &str = "ag58-wall-1792597650";

const T: Tuning = Tuning::ERUPTOR;

/// His walk as the record's frame left it for the locomotion, ft/s, and
/// whether it's the walk's own (`gaits.rs`' `speed`, the vector kept): the
/// stored walk of the record before replayed through Face and Move where
/// the watch held it, else the body's velocity at whichever of the record
/// and the one before was the faster.
fn walk(a: &Record, b: &Record) -> (Vec3, bool) {
    let (Some(ma), Some(mb)) = (&a.mover, &b.mover) else {
        let v = |f: &Record| Vec3::from(f.hero.velocity) / FOOT;
        let (va, vb) = (v(a), v(b));
        return (if va.length() > vb.length() { va } else { vb }, false);
    };
    let mut walk = Walk {
        velocity: stored(ma),
        landing: None,
    };
    let held = |m| field(m, "controller inactive") != 0.0;
    if !held(ma)
        && let (Some(fa), Some(view)) = (footing(a), a.camera.map(|c| c.rotation[1]))
    {
        let push = Stick::new(b.stick[0], b.stick[1]).push(view);
        walk.face_and_move(push, &fa, &T, field(ma, "time since last update"));
    }
    if held(mb) {
        walk.velocity.x = 0.0;
        walk.velocity.z = 0.0;
    }
    (walk.velocity, !held(ma) && !held(mb))
}

/// A record's idle weight and the run's share of the gaits, both forms a
/// record holds the gaits in read alike (`gaits.rs`' `held_weights`, which
/// AG.30's locomotion is compared by): applied to the VM's weights too, it
/// gives AG.30's own measure on the same records.
fn held_weights(rec: &[f32]) -> (f32, Option<f32>) {
    let [idle, walk, run] = [rec[0], rec[1], rec[2]];
    if idle == 1.0 && walk > 0.0 && walk < 1.0 && run == 0.0 {
        return (1.0 - walk, Some(0.0));
    }
    if walk > 0.0 && (run - walk).abs() < 1e-4 && (idle + walk - 1.0).abs() < 2e-3 {
        return (idle, Some(0.0));
    }
    let share = (walk + run > 0.01).then(|| run / (walk + run));
    (idle, share)
}

/// The five clips the recordings hold, in their order, and the fall: each
/// one's palette slot by its clip's name.
const CLIPS: [&str; 6] = [
    "Eruptor_Idle.ma",
    "Eruptor_Walk.ma",
    "Eruptor_Run.ma",
    "Eruptor_LeanLeft.ma",
    "Eruptor_LeanRight.ma",
    "Eruptor_Falling.ma",
];

/// A clip as the VM left it on him: weight (percent), rate (percent) and
/// seconds into it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Played {
    weight: f32,
    rate: f32,
    time: f32,
}

/// The run joined, his minion asleep, and his five clips' slots.
struct Loco {
    run: Run,
    slots: [u32; 6],
}

impl Loco {
    fn new() -> Self {
        let (run, _) = joined(Mode::Strict);
        let (h, _) = run
            .files
            .placed
            .iter()
            .find(|p| p.name == "Macros_Animation")
            .map(|p| (run.handles[&p.obj], p))
            .expect("the locomotion's placement");
        assert!(run.vm.placement_of(h).is_some(), "the locomotion runs");
        let mut run = run;
        assert!(run.vm.set_asleep(HERO, true), "his minion");
        let slots = CLIPS.map(|name| {
            run.rig
                .palette
                .entries
                .iter()
                .find(|e| match &e.entry {
                    guac_ir::palette::SlotEntry::Clip { clip, .. } => clip.name == name,
                    _ => false,
                })
                .map(|e| u32::from(e.slot))
                .unwrap_or_else(|| panic!("no {name} in his palette"))
        });
        Self { run, slots }
    }

    /// One frame of `dt` seconds with his walk `walk` (ft/s) in his movement
    /// script's velocity data, where the locomotion reads its speed (the
    /// movement update binds the locomotion's `$use velocity` to its
    /// `velocity length` when he joins as a scripted mover), his move down
    /// `down` (ft/s) on his actor's velocity, where the fall check reads it,
    /// his height over the ground (metres, none over nothing) and his
    /// facing.
    fn frame(&mut self, dt: f32, walk: Vec3, down: f32, height: Option<f32>, facing: f32) {
        let m = self.run.minion.expect("joined");
        let d = self
            .run
            .vm
            .local(m, "description")
            .map(|v| self.run.vm.value(v));
        let data = d
            .and_then(|d| self.run.vm.field(d, "velocity data"))
            .map(|v| self.run.vm.value(v))
            .expect("his velocity data");
        let length = walk.length();
        let (heading, pitch) = if length > 0.0 {
            (
                guacamole::script_engine::heading_of(walk),
                guacamole::script_engine::pitch_of(walk),
            )
        } else {
            (0.0, 0.0)
        };
        for (name, value) in [
            ("velocity length", length),
            ("velocity heading", heading),
            ("velocity pitch", pitch),
            ("vertical difference", walk.y),
            ("horizontal length", walk.x.hypot(walk.z)),
        ] {
            assert!(
                self.run
                    .vm
                    .set_field(data, name, guac_script::Value::Float(value)),
                "{name}"
            );
        }
        let h = self.run.bench.heroes.get_mut(&HERO).unwrap();
        h.walk.velocity = Vec3::new(walk.x, down, walk.z);
        h.footing.height = height;
        h.facing = facing.rem_euclid(360.0);
        // The frame's slides and every placement's tick in the original's
        // order, as `Run::step`'s, at the record's step: the locomotion's,
        // and the UI's after it, whose tick runs the keyframers the
        // locomotion eases his rates by. His minion is asleep, so his
        // movement script leaves the walk set here for the locomotion to
        // read; the engine doesn't move him.
        self.run.vm.set_tick_seconds(dt);
        self.run.world.read_pads([Default::default(); 2]);
        let player_loop = self.run.player_loop.clone();
        let mut stopped = Vec::new();
        self.run
            .with_host(|vm, host| {
                player_loop.frame_with(vm, host, true, &mut stopped, &mut |_, _, _| Ok(()))
            })
            .unwrap_or_else(|h| panic!("{h}"));
        assert!(stopped.is_empty(), "strict mode stops by halting");
        self.run.world.end_step();
        self.run.world.advance(dt, &mut self.run.bench);
    }

    /// Standing still at rest for `n` frames: what each replay starts from.
    fn rest(&mut self, n: u32, facing: f32) {
        for _ in 0..n {
            self.frame(1.0 / 60.0, Vec3::ZERO, 0.0, Some(0.005), facing);
        }
    }

    /// Each of the five clips at the time and rate `r` holds them: a replay
    /// starts from its record's phases (the idle's, and the walk's and the
    /// run's, which play on unweighted at rest and which a set-off reads:
    /// the walk starts where the run's place in its stride is, AG.11's
    /// "in step").
    fn seed(&mut self, r: &Record) {
        let slots = self.slots;
        let world = &mut self.run.world;
        let mut at = Vec::new();
        for (i, q) in world.sequences().iter().enumerate() {
            if q.actor != HERO || q.stopped || q.done {
                continue;
            }
            if let Some((_, slot)) = world.clip_of(q)
                && let Some(c) = slots[..5].iter().position(|&x| x == slot)
            {
                at.push((i, c));
            }
        }
        for (i, c) in at {
            let rec = &r.clips[c];
            let q = &mut world.sequences_mut()[i];
            q.time = (rec.period - rec.remaining).clamp(0.0, q.length);
            q.rate = rec.rate;
        }
    }

    /// Each of [`CLIPS`] as the last sequence of its slot on him left it,
    /// while it plays.
    fn played(&self) -> [Option<Played>; 6] {
        let mut out = [None; 6];
        for s in self.run.world.sequences() {
            if s.actor != HERO || s.stopped || s.done {
                continue;
            }
            let Some((_, slot)) = self.run.world.clip_of(s) else {
                continue;
            };
            if let Some(i) = self.slots.iter().position(|&x| x == slot) {
                out[i] = Some(Played {
                    weight: s.blend,
                    rate: s.rate,
                    time: s.time,
                });
            }
        }
        out
    }
}

/// One kind of record's counts: those whose five weights all fall within
/// 0.1 % of the record's, and the rates of the clips each record weights.
#[derive(Default)]
struct Agreement {
    records: usize,
    /// Every one of the five weights within 1e-3 of the record's percent
    /// (0.1 of 100).
    within: usize,
    worst: f32,
    worst_at: String,
    rates: usize,
    rates_within: usize,
    worst_rate: f32,
    rate_at: String,
    rates_missed: BTreeMap<String, usize>,
    missed: BTreeMap<String, usize>,
    /// `gaits.rs`' measure on the same records: the idle's weight and the
    /// run's share within 1e-3.
    gaits_within: usize,
    gaits_missed: BTreeMap<String, usize>,
    /// The walk's and the run's place in their strides (time over length)
    /// within 1e-3 of a cycle where the record weights them, while he moves.
    phases: usize,
    phases_within: usize,
    worst_phase: f32,
    phase_at: String,
}

impl Agreement {
    fn print(&self, what: &str) {
        eprintln!(
            "{what}: weights {} of {} records within 0.1 % on all five (worst {:.4} %: {}); \
             rates {} of {} within 0.1 % (worst {:.3}: {})",
            self.within,
            self.records,
            self.worst,
            self.worst_at,
            self.rates_within,
            self.rates,
            self.worst_rate,
            self.rate_at
        );
        eprintln!("    past 0.1 % by set: {:?}", self.missed);
        eprintln!("    rates past 0.1 % by set: {:?}", self.rates_missed);
        eprintln!(
            "    gaits.rs' measure (idle and run share within 1e-3): {} of {}, past it by set: {:?}",
            self.gaits_within, self.records, self.gaits_missed
        );
        eprintln!(
            "    the walk's and run's phase within 1e-3 of a cycle: {} of {} (worst {:.4}: {})",
            self.phases_within, self.phases, self.worst_phase, self.phase_at
        );
    }
}

#[derive(Default)]
struct Tally {
    walk: Agreement,
    body: Agreement,
    falls: usize,
    falls_in: usize,
    gone: BTreeMap<i64, usize>,
    misses: Vec<String>,
}

/// One replay from its first record at rest in the idle, after a second at
/// rest that settles what the last one left: each record's weights and
/// rates tallied in `t`, its fall's records in and out against the VM's.
fn replay(
    loco: &mut Loco,
    name: &str,
    frames: &[Record],
    falls: bool,
    count_held: bool,
    t: &mut Tally,
) {
    let at_rest = |f: &Record| f.hero.velocity == [0.0; 3];
    let Some(k0) = (1..frames.len()).find(|&k| {
        at_rest(&frames[k])
            && at_rest(&frames[k - 1])
            && frames[k].clips.len() == 5
            && frames[k].clips[0].weight == 100.0
    }) else {
        return;
    };
    loco.rest(60, frames[k0].hero.rotation[1]);
    loco.seed(&frames[k0]);
    if std::env::var("AH49_TRACE").is_ok_and(|n| name.contains(&n)) {
        for (i, q) in loco.run.world.sequences().iter().enumerate() {
            if q.actor == HERO {
                eprintln!(
                    "seq {i}: slot {:?} len {} time {} rate {} blend {} looping {} stopped {} done {}",
                    loco.run.world.clip_of(q).map(|c| c.1),
                    q.length,
                    q.time,
                    q.rate,
                    q.blend,
                    q.looping,
                    q.stopped,
                    q.done
                );
            }
        }
    }
    let set = name.rsplit_once('-').map_or(name, |(s, _)| s).to_owned();
    let (mut fall_rec, mut fall_mine) = (None, None);
    let (mut gone_rec, mut gone_mine) = (None, None);
    for k in k0 + 1..frames.len() {
        let (a, b) = (&frames[k - 1], &frames[k]);
        if !consecutive(&frames[k - 1..=k]) {
            break;
        }
        let (w, own) = walk(a, b);
        // His move down and his height as the frame before left them
        // (AG.30's reading of the fall check).
        let down = a.hero.velocity[1] / FOOT;
        let height = footing(a).and_then(|f| f.height);
        let was_falling = loco.played()[5].is_some();
        let tracing = std::env::var("AH49_TRACE").is_ok_and(|n| name.contains(&n))
            && std::env::var("AH49_OPS")
                .ok()
                .and_then(|r| {
                    let (a, b) = r.split_once('-')?;
                    Some((a.parse::<usize>().ok()?..=b.parse::<usize>().ok()?).contains(&k))
                })
                .unwrap_or(false);
        if tracing {
            loco.run.vm.trace(Some(2_000_000));
        }
        loco.frame(a.clock.step(), w, down, height, b.hero.rotation[1]);
        if tracing {
            let ops = loco.run.vm.traced();
            let program = loco.run.vm.program_arc();
            let mut runs: Vec<(String, u32, u32)> = Vec::new();
            for (script, pos) in ops {
                let n = &program.scripts[script as usize].name;
                if !(n.ends_with("Macros_Animation.ai") || n.ends_with("Keyframers.ai")) {
                    continue;
                }
                let short = n.rsplit('/').next().unwrap_or(n).to_owned();
                match runs.last_mut() {
                    Some((s, _, to)) if *s == short && pos == *to + 1 => *to = pos,
                    _ => runs.push((short, pos, pos)),
                }
            }
            let text: Vec<String> = runs
                .iter()
                .map(|(s, a, b)| format!("{}:{a}-{b}", &s[..3]))
                .collect();
            eprintln!("ops {k}: {}", text.join(" "));
        }
        let p = loco.played();
        if std::env::var("AH49_TRACE").is_ok_and(|n| name.contains(&n)) {
            eprintln!(
                "{k} dt {:.4} walk {:.4} y {:.3} h {:?} f {:.2} ours {:?} rec {:?}",
                a.clock.step(),
                w.length(),
                down,
                height,
                b.hero.rotation[1],
                p.map(|c| c.map(|c| (c.weight, c.rate, c.time))),
                b.clips
                    .iter()
                    .map(|c| (c.weight, c.rate, c.period - c.remaining))
                    .collect::<Vec<_>>()
            );
        }
        let five = b.clips.len() == 5;
        if !five && fall_rec.is_none() {
            fall_rec = Some(k);
        }
        if fall_rec.is_some() && five && gone_rec.is_none() {
            gone_rec = Some(k);
        }
        if fall_mine.is_none() && p[5].is_some() {
            fall_mine = Some(k);
        }
        if gone_mine.is_none() && was_falling && p[5].is_none() {
            gone_mine = Some(k);
        }
        if !five {
            continue;
        }
        let kind = if own || (count_held && a.mover.is_some() && b.mover.is_some()) {
            &mut t.walk
        } else {
            &mut t.body
        };
        kind.records += 1;
        let mut miss: f32 = 0.0;
        for (mine, theirs) in p.iter().zip(&b.clips).take(5) {
            miss = miss.max((mine.map_or(0.0, |c| c.weight) - theirs.weight).abs());
        }
        if miss <= 0.1 {
            kind.within += 1;
        } else {
            *kind.missed.entry(set.clone()).or_default() += 1;
        }
        if miss > kind.worst {
            kind.worst = miss;
            kind.worst_at = format!(
                "{name} record {k}: {:?} against {:?} at {:.4} ft/s",
                p.map(|c| c.map(|c| c.weight)),
                b.clips.iter().map(|c| c.weight).collect::<Vec<_>>(),
                w.length()
            );
        }
        let theirs: Vec<f32> = b.clips[..3].iter().map(|c| c.weight / 100.0).collect();
        let ours: Vec<f32> = (0..3)
            .map(|i| p[i].map_or(0.0, |c| c.weight) / 100.0)
            .collect();
        let (ti, ts) = held_weights(&theirs);
        let (oi, os) = held_weights(&ours);
        let mut gmiss = (oi - ti).abs();
        if let (Some(ts), Some(os)) = (ts, os) {
            gmiss = gmiss.max((os - ts).abs());
        }
        if gmiss <= 1e-3 {
            kind.gaits_within += 1;
        } else {
            *kind.gaits_missed.entry(set.clone()).or_default() += 1;
        }
        // The walk's and the run's place in their strides.
        if w.length() > 1e-4 {
            for i in [1, 2] {
                if b.clips[i].weight > 0.0
                    && let Some(c) = p[i]
                {
                    let period = b.clips[i].period;
                    let theirs = (period - b.clips[i].remaining) / period;
                    let mine = c.time / period;
                    let d = (mine - theirs).rem_euclid(1.0);
                    let off = d.min(1.0 - d);
                    kind.phases += 1;
                    if off <= 1e-3 {
                        kind.phases_within += 1;
                    }
                    if off > kind.worst_phase {
                        kind.worst_phase = off;
                        kind.phase_at =
                            format!("{name} record {k} clip {i}: {mine} against {theirs}");
                    }
                }
            }
        }
        // The rate of every clip the record weights.
        for (i, (mine, theirs)) in p.iter().zip(&b.clips).take(5).enumerate() {
            if theirs.weight > 0.0 {
                let mine = mine.map_or(f32::NAN, |c| c.rate);
                let off = (mine - theirs.rate).abs();
                kind.rates += 1;
                if off <= 0.1 {
                    kind.rates_within += 1;
                } else {
                    *kind.rates_missed.entry(set.clone()).or_default() += 1;
                }
                // A clip the VM doesn't play (NaN) is the worst.
                if off.is_nan() || off > kind.worst_rate {
                    kind.worst_rate = off;
                    kind.rate_at =
                        format!("{name} record {k} clip {i}: {mine} against {}", theirs.rate);
                }
            }
        }
    }
    if falls && (fall_rec.is_some() || fall_mine.is_some()) {
        t.falls += 1;
        if fall_rec == fall_mine {
            t.falls_in += 1;
        } else {
            t.misses
                .push(format!("{name}: in at {fall_rec:?}, ours {fall_mine:?}"));
        }
        match (gone_rec, gone_mine) {
            (Some(g), Some(m)) => {
                let off = g as i64 - m as i64;
                *t.gone.entry(off).or_default() += 1;
                if off != 0 {
                    t.misses.push(format!("{name}: gone at {g}, ours at {m}"));
                }
            }
            _ => t
                .misses
                .push(format!("{name}: gone at {gone_rec:?}, ours {gone_mine:?}")),
        }
    }
}

/// AG.11's 244 recordings through the VM's picks: the five clips' weights
/// and rates against every record that holds the five, and every fall's
/// clip in and out.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.11's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn the_vms_picks_against_ag11s_recordings() {
    println!("load before: {}", load());
    let started = std::time::Instant::now();
    let mut loco = Loco::new();
    let mut t = Tally::default();
    let mut replays = 0;
    for run in RUNS {
        for (name, dir) in replay_dirs(run) {
            replays += 1;
            replay(
                &mut loco,
                &format!("{run}/{name}"),
                &frames(&dir),
                FALLS.contains(&run),
                false,
                &mut t,
            );
        }
    }
    t.walk.print("the walk's own speed");
    t.body.print("the body's speed");
    eprintln!(
        "{replays} replays in {:.1?}; falls {}: in on the record {}; the record the fall's gone \
         less ours: {:?}",
        started.elapsed(),
        t.falls,
        t.falls_in,
        t.gone
    );
    for m in &t.misses {
        eprintln!("    {m}");
    }
    println!("load after: {}", load());
    assert_eq!(replays, 244);
    // Every fall in and gone on its record (AG.30's brings 47 back on
    // theirs).
    assert_eq!((t.falls, t.falls_in), (48, 48));
    assert_eq!(t.gone.get(&0), Some(&48), "{:?}", t.gone);
    // On the walk's own speed, by `gaits.rs`' measure, every record but
    // AG.11's one known parting, `g38_m049`'s uphill dead stop; on all five
    // weights raw, that and the corridor's lean where his turn a frame sits
    // at the lean's 0.5° (the record's facing doesn't settle which side);
    // every rate but that parting's.
    let w = &t.walk;
    assert!(w.records > 19_000, "{}", w.records);
    let parting = "ag38-switch-1792509065/a/g38_m049-W";
    let corridor = "ag38-switch-1792509065/c/g38_corridor-W";
    let other = |m: &BTreeMap<String, usize>, known: &[&str]| -> Vec<String> {
        m.keys()
            .filter(|s| !known.contains(&s.as_str()))
            .cloned()
            .collect()
    };
    assert!(
        other(&w.gaits_missed, &[parting]).is_empty(),
        "{:?}",
        w.gaits_missed
    );
    assert!(
        other(&w.missed, &[parting, corridor]).is_empty(),
        "{:?}",
        w.missed
    );
    assert!(
        other(&w.rates_missed, &[parting]).is_empty(),
        "{:?}",
        w.rates_missed
    );
}

/// AG.58's 13 recordings through the VM's picks, every record on the walk's
/// own speed, the edge probe's holds included.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and AG.58's recordings under GUAC_CAPTURE_DIR; run on the Mac"]
fn the_vms_picks_against_ag58s_recordings() {
    println!("load before: {}", load());
    let mut loco = Loco::new();
    let mut t = Tally::default();
    let mut replays = 0;
    for (name, dir) in replay_dirs(AG58) {
        replays += 1;
        replay(
            &mut loco,
            &format!("ag58/{name}"),
            &frames(&dir),
            false,
            true,
            &mut t,
        );
    }
    t.walk.print("the walk's own speed");
    t.body.print("the body's speed");
    println!("load after: {}", load());
    assert_eq!(replays, 13);
    let w = &t.walk;
    assert!(w.records > 5_000, "{}", w.records);
    assert_eq!(t.body.records, 0);
    assert_eq!(w.within, w.records, "{:?}", w.missed);
    assert_eq!(w.gaits_within, w.records);
    assert_eq!(w.rates_within, w.rates, "{:?}", w.rates_missed);
}

/// A development probe: one replay (`AH49_REPLAY=<run>/<label>`, a label as
/// `replay_dirs` names it), traced record by record with `AH49_TRACE` and
/// the locomotion's ops over `AH49_OPS=<from>-<to>` records.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and the recordings; a probe"]
fn probe_one_replay() {
    let Ok(which) = std::env::var("AH49_REPLAY") else {
        return;
    };
    let (run, label) = which.split_once('/').expect("<run>/<label>");
    let mut loco = Loco::new();
    let mut t = Tally::default();
    for (name, dir) in replay_dirs(run) {
        if name == label {
            replay(
                &mut loco,
                &format!("{run}/{name}"),
                &frames(&dir),
                true,
                false,
                &mut t,
            );
        }
    }
    t.walk.print("the walk's own speed");
    t.body.print("the body's speed");
    if let Ok(ids) = std::env::var("AH49_OPMAP") {
        let program = loco.run.vm.program_arc();
        for id in ids.split(',').filter_map(|i| i.trim().parse::<u32>().ok()) {
            for s in &program.scripts {
                if let Some(pos) = s.ops.iter().position(|&o| o == id) {
                    eprintln!("op {id}: {} #{pos}", s.name);
                }
            }
        }
    }
}
