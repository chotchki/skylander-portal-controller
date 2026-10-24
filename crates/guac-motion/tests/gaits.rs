//! The locomotion against the oracle's own recordings (openspec AG.30 on
//! AG.11; stand-in ledger: `locomotion`). `#[ignore]`d: the recordings are
//! guest memory and stay outside the tree, under `GUAC_CAPTURE_DIR` (process
//! env, else the workspace's `.env.dev`). Run with `cargo test -p guac-motion
//! --test gaits -- --ignored --nocapture`.
//!
//! Each replay starts the locomotion standing at its first record at rest in
//! the idle and steps it a record at a time on what the record holds: the
//! speed, the move down and the height. Records pair by `_curFrame`; a
//! record's advance is the step of the record before it, as AG.11 summed
//! them. The speed is the movement script's walk after Face and Move and the
//! edge probe, before Update Movers, replayed from the stored walk of the
//! record before where the watch held it (AG.38's recordings: the speed the
//! original read, to AG.38's 0.032 ft/s). Elsewhere it's the larger of the
//! body's 3D speed at the record and the one before, AG.11's fit, which
//! parts from the walk into walls, at edges and on landings: those runs are
//! reported, and only the falls asserted on them, since the fall reads the
//! body's own move and height.
//!
//! A record holds the gaits either as the rest of the idle's weight or as
//! their own split of 100: both kinds turn up through a slide (a set-off's
//! records alternate, AG.30's tick). The idle's weight and the run's share of
//! the gaits read the same in both, so they're what's compared.

mod oracle;

use std::collections::BTreeMap;

use guac_motion::locomotion::{Air, Clip, Frame, Gaits, Locomotion, Loops};
use guac_motion::stick::Stick;
use guac_motion::tuning::{FOOT, Tuning};
use guac_motion::walk::Walk;
use guac_oracle::recording::Frame as Record;
use oracle::{consecutive, field, footing, frames, replay_dirs, stored};

/// Eruptor's loops as the recordings hold them (AG.11).
const LOOPS: Loops = Loops {
    idle: 4.0 / 3.0,
    walk: Some(1.0667),
    run: Some(0.6667),
    fall: Some(0.8),
};

const T: Tuning = Tuning::ERUPTOR;

/// AG.11's five boxes.
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

/// The speed the record's frame read, ft/s, and whether it's the walk's
/// own (the watch held the stored walk and the edge probe didn't hold him
/// either side).
fn speed(a: &Record, b: &Record) -> (f32, bool) {
    let (Some(ma), Some(mb)) = (&a.mover, &b.mover) else {
        let s = |f: &Record| {
            let v = f.hero.velocity;
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };
        return (s(a).max(s(b)) / FOOT, false);
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
    (walk.velocity.length(), !held(ma) && !held(mb))
}

/// The edge probe held his controller off at the record's frame (the
/// mover's "controller inactive" where the watch held it).
fn held_at_edge(f: &Record) -> bool {
    f.mover
        .as_ref()
        .is_some_and(|m| field(m, "controller inactive") != 0.0)
}

/// The weights' and the rates' agreement over one kind of record.
#[derive(Default)]
struct Agreement {
    records: usize,
    within: usize,
    worst: f32,
    worst_at: String,
    rates: usize,
    rates_within: usize,
    worst_rate: f32,
    rate_at: String,
    /// Records past 1e-3, by replay set.
    missed: BTreeMap<String, usize>,
}

impl Agreement {
    fn weigh(&mut self, set: &str, miss: f32, at: impl FnOnce() -> String) {
        self.records += 1;
        if miss <= 1e-3 {
            self.within += 1;
        } else {
            *self.missed.entry(set.to_owned()).or_default() += 1;
        }
        if miss > self.worst {
            self.worst = miss;
            self.worst_at = at();
        }
    }

    fn rate(&mut self, miss: f32, at: impl FnOnce() -> String) {
        self.rates += 1;
        if miss <= 0.1 {
            self.rates_within += 1;
        }
        if miss > self.worst_rate {
            self.worst_rate = miss;
            self.rate_at = at();
        }
    }

    fn print(&self, what: &str) {
        eprintln!(
            "{what}: weights {} of {} within 1e-3 (worst {:.4}: {}); rates {} of {} within 0.1 % (worst {:.3}: {})",
            self.within,
            self.records,
            self.worst,
            self.worst_at,
            self.rates_within,
            self.rates,
            self.worst_rate,
            self.rate_at
        );
        eprintln!("    past 1e-3 by set: {:?}", self.missed);
    }
}

/// Every replay's agreement: the walk's own speed, and the body's.
#[derive(Default)]
struct Tally {
    walk: Agreement,
    body: Agreement,
    /// Per fall: in on the record, and the record the fall's gone against
    /// the one the locomotion's gaits are whole again (recording.json keeps
    /// no clips while the fall plays, so the slide back's start isn't in it).
    falls: usize,
    falls_in: usize,
    gone: BTreeMap<i64, usize>,
    misses: Vec<String>,
}

/// A record's idle weight and the run's share of the gaits, where it shows
/// one. Held at an edge with the stick pushed (AG.58's door runs, AG.36's,
/// the gate's), the records flip each frame: one shows the idle at 100
/// beside a walk at the rest of the idle's own weight, the next the idle at
/// its weight and the run at the walk's, its share 0 (the speed read is a
/// slope's share of a push, 0.14 ft/s). Both flips turn up where the probe
/// holds nothing too (AG.9's `walk` coming to rest, the first record of a
/// set-off from the corner's stop in AG.58's `w58_angle`), so every record is
/// read this way: read only on held records, AG.11's walk-speed records
/// within 1e-3 fall from 19,287 to 19,231 and AG.58's from 5,800 to 5,798.
/// Elsewhere a record holds the gaits either as the rest of the idle or as
/// their own split of 100.
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

fn replay(name: &str, frames: &[Record], falls: bool, count_held: bool, tally: &mut Tally) {
    let at_rest = |f: &Record| f.hero.velocity == [0.0; 3];
    let Some(k0) = (1..frames.len()).find(|&k| {
        at_rest(&frames[k])
            && at_rest(&frames[k - 1])
            && frames[k].clips.len() == 5
            && frames[k].clips[0].weight == 100.0
    }) else {
        return;
    };
    let mut l = Locomotion::new(Gaits::ERUPTOR, LOOPS);
    let (mut fall_rec, mut fall_mine) = (None, None);
    let (mut gone_rec, mut whole_mine) = (None, None);
    for k in k0 + 1..frames.len() {
        let (a, b) = (&frames[k - 1], &frames[k]);
        if !consecutive(&frames[k - 1..=k]) {
            break;
        }
        let (s, own) = speed(a, b);
        let frame = Frame {
            speed: s,
            falling: -b.hero.velocity[1] / FOOT,
            height: footing(b).and_then(|f| f.height),
            held: held_at_edge(b),
        };
        let was = *l.air();
        l.step(a.clock.step(), frame);
        let five = b.clips.len() == 5;
        if !five && fall_rec.is_none() {
            fall_rec = Some(k);
        }
        if fall_rec.is_some() && five && gone_rec.is_none() {
            gone_rec = Some(k);
        }
        if fall_mine.is_none() && matches!(l.air(), Air::Falling(_)) {
            fall_mine = Some(k);
        }
        if whole_mine.is_none() && matches!(was, Air::Landing(_)) && *l.air() == Air::Ground {
            whole_mine = Some(k);
        }
        if !five {
            continue;
        }
        let mine = l.weights();
        let rec: Vec<f32> = b.clips[..3].iter().map(|c| c.weight / 100.0).collect();
        let (idle, share) = held_weights(&rec);
        let mut miss = (mine[0] - idle).abs();
        if let Some(share) = share
            && mine[1] + mine[2] > 0.01
        {
            miss = miss.max((l.run_share() - share).abs());
        }
        let kind = if own || (count_held && a.mover.is_some() && b.mover.is_some()) {
            &mut tally.walk
        } else {
            &mut tally.body
        };
        let set = name.rsplit_once('-').map_or(name, |(set, _)| set);
        kind.weigh(set, miss, || {
            format!(
                "{name} record {k}: idle {} share {} against {rec:?} at {s} ft/s",
                mine[0],
                l.run_share()
            )
        });
        // The rate of a gait the record shows, while he moves.
        for (i, clip) in [(1, Clip::Walk), (2, Clip::Run)] {
            if l.moving() && rec[i] > 0.0 && mine[i] > 0.0 {
                let rate = l.play(clip).rate * 100.0;
                kind.rate((rate - b.clips[i].rate).abs(), || {
                    format!(
                        "{name} record {k}: {} {rate} % against {} % at {s} ft/s",
                        clip.name(),
                        b.clips[i].rate
                    )
                });
            }
        }
    }
    if falls && (fall_rec.is_some() || fall_mine.is_some()) {
        tally.falls += 1;
        if fall_rec == fall_mine {
            tally.falls_in += 1;
        } else {
            tally
                .misses
                .push(format!("{name}: in at {fall_rec:?}, ours {fall_mine:?}"));
        }
        if let (Some(g), Some(w)) = (gone_rec, whole_mine) {
            let off = g as i64 - w as i64;
            *tally.gone.entry(off).or_default() += 1;
            if off != 0 {
                tally
                    .misses
                    .push(format!("{name}: gone at {g}, ours whole at {w}"));
            }
        } else {
            tally.misses.push(format!(
                "{name}: gone at {gone_rec:?}, ours whole at {whole_mine:?}"
            ));
        }
    }
}

/// AG.11's 244 recordings: the locomotion's idle weight and run share against
/// the recorded ones on every record that holds its five clips, the gaits'
/// rates where they show, and every fall's in and out.
#[test]
#[ignore]
fn the_stand_in_against_ag11s_recordings() {
    let mut tally = Tally::default();
    let mut replays = 0;
    for run in RUNS {
        for (name, dir) in replay_dirs(run) {
            replays += 1;
            replay(
                &format!("{run}/{name}"),
                &frames(&dir),
                FALLS.contains(&run),
                false,
                &mut tally,
            );
        }
    }
    tally.walk.print("the walk's own speed");
    tally.body.print("the body's speed");
    eprintln!(
        "{replays} replays; falls {}: in on the record {}; the record the fall's gone less the one the stand-in's gaits are whole: {:?}",
        tally.falls, tally.falls_in, tally.gone
    );
    for m in &tally.misses {
        eprintln!("    {m}");
    }
    assert_eq!(replays, 244);
    assert_eq!((tally.falls, tally.falls_in), (48, 48));
    assert_eq!(tally.gone.get(&0), Some(&47), "{:?}", tally.gone);
    // On the walk's own speed every record within 1e-3 but AG.11's one
    // known parting, `g38_m049`'s uphill dead stop, whose idle comes back
    // two records late; every rate within 0.1 %. The gate's edge-probe hold
    // parted too until AG.58: the probe's flicker set the locomotion off while
    // the original's idle kept coming back.
    let w = &tally.walk;
    assert!(w.records > 19_000, "{}", w.records);
    let known = ["ag38-switch-1792509065/a/g38_m049-W"];
    let other: Vec<_> = w
        .missed
        .iter()
        .filter(|(s, _)| !known.contains(&s.as_str()))
        .collect();
    assert!(other.is_empty(), "{other:?}");
    assert_eq!(w.rates_within, w.rates);
}

/// AG.58's 13 set O recordings (`ag58-wall-1792597650`: the wall behind the
/// start held into, run into and slid along at 20° into its corner; the
/// gate's door slid along to its end over the bridge's foot, where the edge
/// probe stops him and flickers with the stick held; let-gos from a 1.25 s
/// and a 9 s run), every record on the walk's own speed, the probe's holds
/// included: the locomotion's idle weight and run share against the
/// original's, both of its flips read (`held_weights`), and the rates.
#[test]
#[ignore]
fn the_stand_in_against_ag58s_recordings() {
    let mut tally = Tally::default();
    let mut replays = 0;
    for (name, dir) in replay_dirs("ag58-wall-1792597650") {
        replays += 1;
        replay(
            &format!("ag58/{name}"),
            &frames(&dir),
            false,
            true,
            &mut tally,
        );
    }
    tally.walk.print("the walk's own speed");
    tally.body.print("the body's speed");
    assert_eq!(replays, 13);
    let w = &tally.walk;
    assert!(w.records > 5_000, "{}", w.records);
    assert_eq!(tally.body.records, 0);
    assert!(w.missed.is_empty(), "{:?}", w.missed);
    assert_eq!(w.rates_within, w.rates);
}
