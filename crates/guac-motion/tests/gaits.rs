//! The locomotion stand-in against the oracle's own recordings (openspec
//! AG.30 on AG.11). `#[ignore]`d: the recordings are guest memory and stay
//! outside the tree, under `GUAC_CAPTURE_DIR` (process env, else the
//! workspace's `.env.dev`). Run with `cargo test -p guac-motion --test gaits
//! -- --ignored --nocapture`.
//!
//! Each replay starts the stand-in standing at its first record at rest in
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
    /// the one the stand-in's gaits are whole again (recording.json keeps
    /// no clips while the fall plays, so the slide back's start isn't in it).
    falls: usize,
    falls_in: usize,
    gone: BTreeMap<i64, usize>,
    misses: Vec<String>,
}

fn replay(name: &str, frames: &[Record], falls: bool, tally: &mut Tally) {
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
        let mut miss = (mine[0] - rec[0]).abs();
        if rec[1] + rec[2] > 0.01 && mine[1] + mine[2] > 0.01 {
            miss = miss.max((l.run_share() - rec[2] / (rec[1] + rec[2])).abs());
        }
        let kind = if own {
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

/// AG.11's 244 recordings: the stand-in's idle weight and run share against
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
    // On the walk's own speed every record within 1e-3 but AG.11's two
    // known partings: `g38_m049`'s uphill dead stop, whose idle comes back
    // two records late, and the gate's edge-probe hold, where the
    // original's idle and run flip each record (the stand-in follows the
    // walk's fade there, a hypothesis); every rate within 0.1 %.
    let w = &tally.walk;
    assert!(w.records > 19_000, "{}", w.records);
    let known = [
        "ag38-switch-1792509065/a/g38_m049-W",
        "ag38-switch-1792509065/gate/walk-W",
    ];
    let other: Vec<_> = w
        .missed
        .iter()
        .filter(|(s, _)| !known.contains(&s.as_str()))
        .collect();
    assert!(other.is_empty(), "{other:?}");
    assert_eq!(w.rates_within, w.rates);
}
