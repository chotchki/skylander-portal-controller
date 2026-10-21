//! The controller stepped from a recording's own state on its own sticks,
//! views and steps, over level 213's collision read off the dump (openspec
//! AG.16 on AG.9, AG.10, AG.36, AG.38 and AG.39). `#[ignore]`d: the
//! recordings are guest memory under `GUAC_CAPTURE_DIR` and the level comes
//! from `GUAC_GIANTS_DUMP` (each from the process env, else `.env.dev`). Run
//! with `cargo test -p guac-motion --release --test replays -- --ignored
//! --nocapture --test-threads 1`.
//!
//! Unlike `recordings.rs`, which replays one law a record at a time from the
//! record before, these run the whole controller free: from a recording's
//! first record, or from the record before an event (a let-go, the gate's
//! swing, the first record of a fall) so that what's measured is the law's
//! and not the drift a run picks up on the way there. The world holds the
//! level's mesh and the gate's door; nothing else in reach is solid in these
//! recordings' manifolds but a Titan boulder, whose live solid scripts
//! rewrite (AG.7), touched in 4 records of one replay. AG.18 replays the
//! same recordings through both sides of the seam with its declared
//! tolerances; this is the parry side's account of where each one parts.

mod oracle;

use guac_motion::hero::{Hero, Input};
use guac_motion::parry::ParryWorld;
use guac_motion::proxy::Proxy;
use guac_motion::seam::Vec3;
use guac_motion::solid::euler;
use guac_motion::stick::Stick;
use guac_motion::tuning::{FOOT, Tuning};
use guac_motion::walk::Walk;
use guac_oracle::recording::Frame;
use oracle::*;

/// The controller's state after record `k`'s frame: his pivot, the move
/// over the frame's step (m/s, as `moved` measures theirs) and his walk
/// (ft/s, as the script stores it).
#[derive(Debug, Clone)]
struct Step {
    k: usize,
    pivot: Vec3,
    /// His up axis, his shape's centre 0.9144 m along it from the pivot.
    up: Vec3,
    velocity: Vec3,
    walk: Vec3,
    supported: bool,
    /// The gate's door within 1 cm of his shape.
    door: bool,
}

/// The hero as record `s` holds him: his pivot, facing and lean, his walk
/// (the stored one where the watch held it, else what moved his body into
/// the record, which is the walk off the ground, AG.10, and laid along it
/// on it), his footing, and the steps of the two records before.
fn seeded(frames: &[Frame], s: usize) -> Hero {
    let f = &frames[s];
    let mut hero = Hero::new(
        eruptor_solid(),
        Tuning::ERUPTOR,
        Vec3::from(f.hero.position),
        f.hero.rotation[1],
    );
    hero.up = euler(f.hero.rotation) * Vec3::Y;
    hero.walk = Walk {
        velocity: f.mover.as_ref().map_or(moved(frames, s) / FOOT, stored),
        landing: None,
    };
    hero.footing = footing(f).unwrap();
    hero.velocity = Vec3::from(f.hero.velocity);
    hero.held_off = f
        .mover
        .as_ref()
        .is_some_and(|m| field(m, "controller inactive") != 0.0);
    hero.set_steps(frames[s - 1].clock.step(), frames[s - 2].clock.step());
    hero
}

/// The controller from record `s` through record `until`, stopping at a gap
/// in the game's frame count.
fn run(world: &ParryWorld, frames: &[Frame], s: usize, until: usize) -> Vec<Step> {
    let mut hero = seeded(frames, s);
    let mut out = Vec::new();
    for k in s + 1..=until.min(frames.len() - 1) {
        let (a, b) = (&frames[k - 1], &frames[k]);
        if b.game_frame != a.game_frame.wrapping_add(1) {
            break;
        }
        let view = a.camera.map_or(0.0, |c| c.rotation[1]);
        let input = Input::stick(Stick::new(b.stick[0], b.stick[1]), view);
        hero.step(world, &input, a.clock.step(), &[]);
        let proxy = Proxy {
            seam: world,
            shape: hero.solid().grown(0.0),
            centre: hero.solid().centre,
            rotation: hero.solid().rotation,
            exclude: &[],
            surfaces: guac_motion::proxy::Surfaces::NONE,
        };
        let door = proxy
            .planes(hero.pivot, hero.up, 0.01)
            .iter()
            .any(|p| p.body == DOOR);
        out.push(Step {
            k,
            pivot: hero.pivot,
            up: hero.up,
            velocity: hero.velocity,
            walk: hero.walk.velocity,
            supported: hero.footing.supported,
            door,
        });
    }
    out
}

/// What theirs did over record `k`: its displacement over the step that
/// moved it there, m/s.
fn moved(frames: &[Frame], k: usize) -> Vec3 {
    let (a, b) = (&frames[k - 1], &frames[k]);
    (Vec3::from(b.hero.position) - Vec3::from(a.hero.position)) / a.clock.step()
}

/// The gate's door is the manifold's own entry at a record.
fn their_door(f: &Frame) -> bool {
    const DOOR_COLLIDER: u32 = 815_119_376;
    f.contacts
        .as_ref()
        .and_then(|c| c.planes.first())
        .is_some_and(|p| p.collider == DOOR_COLLIDER)
}

/// Records a run spent within 1 mm and 1 cm of the recorded pivot, the
/// first past each, the records whose move over the step is within 0.01
/// m/s of theirs, and (where the watch held the walk) whose walk is within
/// AG.38's 0.032 ft/s.
#[derive(Debug, Default, Clone)]
struct Held {
    steps: usize,
    within_mm: usize,
    within_cm: usize,
    first_off_mm: Option<usize>,
    first_off_cm: Option<usize>,
    worst: f32,
    moved_ok: usize,
    walks: usize,
    walk_ok: usize,
}

impl Held {
    fn of(frames: &[Frame], steps: &[Step]) -> Held {
        let mut h = Held {
            steps: steps.len(),
            ..Held::default()
        };
        for s in steps {
            let b = &frames[s.k];
            let dp = (s.pivot - Vec3::from(b.hero.position)).length();
            h.worst = h.worst.max(dp);
            if dp <= 1e-3 {
                h.within_mm += 1;
            } else if h.first_off_mm.is_none() {
                h.first_off_mm = Some(s.k);
            }
            if dp <= 1e-2 {
                h.within_cm += 1;
            } else if h.first_off_cm.is_none() {
                h.first_off_cm = Some(s.k);
            }
            h.moved_ok += usize::from((s.velocity - moved(frames, s.k)).length() <= 0.01);
            if let Some(m) = &b.mover {
                h.walks += 1;
                h.walk_ok += usize::from((s.walk - stored(m)).length() <= 0.032);
            }
        }
        h
    }

    fn add(&mut self, h: &Held) {
        self.steps += h.steps;
        self.within_mm += h.within_mm;
        self.within_cm += h.within_cm;
        self.moved_ok += h.moved_ok;
        self.walks += h.walks;
        self.walk_ok += h.walk_ok;
        self.worst = self.worst.max(h.worst);
    }

    fn line(&self, name: &str) -> String {
        format!(
            "{name}: {} records, pivot within 1 mm {} (first past {:?}) and 1 cm {} (first past {:?}, worst {:.3} m), moved within 0.01 m/s {}, walk within 0.032 ft/s {} of {}",
            self.steps,
            self.within_mm,
            self.first_off_mm,
            self.within_cm,
            self.first_off_cm,
            self.worst,
            self.moved_ok,
            self.walk_ok,
            self.walks
        )
    }
}

fn level() -> ParryWorld {
    ParryWorld::new(&level_213()).unwrap()
}

/// The first record from `from` on where the stick reads `stick` after not
/// doing so (the game's own floats).
fn first_reading(frames: &[Frame], from: usize, stick: [f32; 2]) -> Option<usize> {
    (from.max(1)..frames.len()).find(|&k| frames[k].stick == stick && frames[k - 1].stick != stick)
}

const AG38: &str = "ag38-switch-1792509065";

/// AG.38's 38 replays run free from their first record: each holds within
/// 1 mm until its first frame off the ground (a crest's, the corridor
/// foot's, the gate's) and its walk comes back after it.
#[test]
#[ignore]
fn ag38_free_from_the_start() {
    let world = level();
    let mut total = Held::default();
    let mut earliest = usize::MAX;
    for (name, frames) in recordings(AG38) {
        let steps = run(&world, &frames, 2, frames.len());
        let h = Held::of(&frames, &steps);
        eprintln!("{}", h.line(&name));
        earliest = earliest.min(h.first_off_mm.unwrap_or(usize::MAX));
        total.add(&h);
    }
    eprintln!("{}", total.line("all"));
    assert!(earliest >= 91, "{earliest}");
    assert!(
        total.within_mm >= 3_700 && total.walk_ok >= 15_800,
        "{total:?}"
    );
}

/// The first record from `from` on where he lands from a frame off the
/// ground within 0.4 ft of it: the crest's landing.
fn crest_landing(frames: &[Frame], from: usize) -> Option<usize> {
    (from.max(1)..frames.len()).find(|&k| {
        let (a, b) = (footing(&frames[k - 1]), footing(&frames[k]));
        a.is_some_and(|a| !a.supported && !a.high()) && b.is_some_and(|b| b.supported)
    })
}

/// The dip's edge at six strengths (AG.38's `a/g38_m*`, 2 replays each), run
/// free from the start: the two records after the crest's landing he walks
/// at the original's speed to 1e-3 m/s, his walk its length to 0.005 ft/s
/// (AG.38's 3.809, 4.415, 5.006-5.007, 5.826-5.827, 6.553 and 6.553 m/s).
/// The landing's own record is the proxy's (a hypothesis, reported).
#[test]
#[ignore]
fn ag38_the_crests_at_six_strengths() {
    let world = level();
    let mut seen = 0;
    for (name, frames) in recordings(AG38) {
        if !name.starts_with("a/g38_m") {
            continue;
        }
        let steps = run(&world, &frames, 2, frames.len());
        let at = |k: usize| steps.iter().find(|s| s.k == k).unwrap();
        let land = crest_landing(&frames, 70).unwrap();
        for k in land + 1..=land + 2 {
            let (ours, theirs) = (at(k).velocity.length(), moved(&frames, k).length());
            let walk = stored(frames[k].mover.as_ref().unwrap()).length();
            assert!((ours - theirs).abs() <= 1e-3, "{name} {k}: {ours} {theirs}");
            assert!((at(k).walk.length() - walk).abs() <= 5e-3, "{name} {k}");
        }
        eprintln!(
            "{name}: from {:.4} m/s, landing at record {land} ours {:.4} theirs {:.4}, then ours {:.4} theirs {:.4} m/s",
            moved(&frames, land - 3).length(),
            at(land).velocity.length(),
            moved(&frames, land).length(),
            at(land + 1).velocity.length(),
            moved(&frames, land + 1).length(),
        );
        seen += 1;
    }
    assert_eq!(seen, 12);
}

/// A let-go's slide: the records it moved, how far, its last moving
/// record's speed and the game time it took.
#[derive(Debug, Clone, Copy)]
struct Slide {
    frames: usize,
    metres: f32,
    last: f32,
    seconds: f32,
}

/// The slide in `speeds` (record, pivot, speed), from `from`.
fn slide(frames: &[Frame], speeds: &[(usize, Vec3, f32)], from: Vec3) -> Slide {
    let moving: Vec<&(usize, Vec3, f32)> = speeds.iter().take_while(|s| s.2 > 1e-4).collect();
    let end = moving.last().map_or(from, |s| s.1);
    Slide {
        frames: moving.len(),
        metres: (end - from).length(),
        last: moving.last().map_or(0.0, |s| s.2),
        seconds: moving.iter().map(|s| frames[s.0 - 1].clock.step()).sum(),
    }
}

/// The nine slope let-gos (AG.38's `a/g38_t*`, up, down and across t2673
/// at 5.34°, t2871 at 8.53° and t2979 at 17.3°, 2 replays each), each from
/// the record before the stick reads centred: the same number of records
/// moving, the last one's speed to 1e-3 m/s and the slide's length to 1 cm,
/// inside AG.38's bands (stopped dead from 1.323-2.543 m/s walking down,
/// 0.011-0.519 across, 1.208-2.732 up; 0.814-1.049 m in 0.232-0.269 s).
#[test]
#[ignore]
fn ag38_the_nine_slope_let_gos() {
    let world = level();
    let mut seen = 0;
    for (name, frames) in recordings(AG38) {
        if !name.starts_with("a/g38_t") {
            continue;
        }
        let g = first_reading(&frames, 100, [0.0, 0.0]).unwrap();
        let steps = run(&world, &frames, g - 1, g + 60);
        let from = Vec3::from(frames[g - 1].hero.position);
        let ours: Vec<(usize, Vec3, f32)> = steps
            .iter()
            .map(|s| (s.k, s.pivot, s.velocity.length()))
            .collect();
        let theirs: Vec<(usize, Vec3, f32)> = (g..=g + 60)
            .map(|k| {
                let p = Vec3::from(frames[k].hero.position);
                (k, p, moved(&frames, k).length())
            })
            .collect();
        let (o, t) = (slide(&frames, &ours, from), slide(&frames, &theirs, from));
        eprintln!("{name} let go at record {g}: ours {o:?}; theirs {t:?}");
        assert_eq!(o.frames, t.frames, "{name}");
        assert!((o.last - t.last).abs() <= 1e-3, "{name}");
        assert!((o.metres - t.metres).abs() <= 0.01, "{name}");
        let band = if name.contains("_down") {
            1.323..=2.543
        } else if name.contains("_across") {
            0.011..=0.519
        } else {
            1.208..=2.733
        };
        assert!(band.contains(&o.last), "{name}: {}", o.last);
        assert!((0.814..=1.049).contains(&o.metres), "{name}");
        assert!(
            (0.232..=0.269).contains(&o.seconds),
            "{name}: {}",
            o.seconds
        );
        seen += 1;
    }
    assert_eq!(seen, 18);
}

/// The gate's split (AG.38's `gate`, AF.11's walk, 6 replays): the stick
/// swings 90° at full on the ground falling away ahead of the gate. Free
/// from the start, his walk sits at most 1.2e-5 ft/s over 21.5 at the swing,
/// under the switch, and he turns free in all six (the creep that tips the
/// original's past it is hypothesis 1). From the record before the swing,
/// with the original's crept walk, he steers into the door in the three
/// whose walk sat 1.26e-4-1.49e-4 over (W-00, -01, -04) and turns free in
/// two of the other three; W-05's, 1.03e-4 over, steers a frame and turns
/// free in the original as rounding took it back under 1e-4.
#[test]
#[ignore]
fn ag38_the_gates_split() {
    let world = level();
    let mut agree = Vec::new();
    for (name, frames) in recordings(AG38) {
        if !name.starts_with("gate/") {
            continue;
        }
        let swing = first_reading(&frames, 100, [-1.0, 0.0]).unwrap();
        let theirs = (swing..frames.len()).any(|k| their_door(&frames[k]));
        let free = run(&world, &frames, 2, frames.len());
        let free_door = free.iter().any(|s| s.door);
        let free_over = free
            .iter()
            .find(|s| s.k == swing - 1)
            .unwrap()
            .walk
            .length()
            - 21.5;
        let seeded = run(&world, &frames, swing - 1, swing + 60);
        let seeded_door = seeded.iter().any(|s| s.door);
        let over = stored(frames[swing - 1].mover.as_ref().unwrap()).length() - 21.5;
        eprintln!(
            "{name} swing at record {swing}: theirs into the door {theirs}, walk {over:+.3e} ft/s over; free run into it {free_door} ({free_over:+.3e} over); from the record before {seeded_door}; {}",
            Held::of(&frames, &seeded).line("from there")
        );
        assert!(!free_door && free_over < 1e-4, "{name}");
        if seeded_door == theirs {
            agree.push(name.clone());
        } else {
            assert_eq!(name, "gate/walk-W-05");
        }
    }
    assert_eq!(agree.len(), 5, "{agree:?}");
}

const AG39: &str = "ag39-falls-1792502813";

/// Free in the air: not supported, 0.4 ft or more up, and his manifold's
/// own closest point 2 cm or more off (AG.10's choice).
fn free(f: &Frame) -> bool {
    let Some(c) = &f.contacts else { return false };
    footing(f).is_some_and(|ft| ft.high()) && c.planes.first().is_none_or(|p| p.distance >= 0.02)
}

/// AG.39's falls off the ledge route (54 replays): from the second record of
/// each run of free ones (the frame into it moved him by his walk alone, so
/// the walk is known), his move over each record theirs stays free (gravity
/// 136.21 ft/s², the air's steer and its 50 ft/s² slowing) and the record he
/// lands at.
#[test]
#[ignore]
fn ag39_falls_from_their_second_free_record() {
    let world = level();
    let (mut falls, mut held, mut landed, mut free_records, mut free_ok) = (0, 0, 0, 0, 0);
    let mut parted = Vec::new();
    for (name, frames) in recordings(AG39) {
        let mut k = 3;
        while k < frames.len() - 1 {
            if !(free(&frames[k]) && free(&frames[k - 1]) && !free(&frames[k - 2])) {
                k += 1;
                continue;
            }
            let land =
                (k..frames.len()).find(|&j| footing(&frames[j]).is_some_and(|f| f.supported));
            let until = land.map_or(frames.len() - 1, |l| (l + 3).min(frames.len() - 1));
            let steps = run(&world, &frames, k, until);
            let mut worst = 0.0f32;
            for s in &steps {
                if free(&frames[s.k]) && free(&frames[s.k - 1]) {
                    let off = (s.velocity - moved(&frames, s.k)).length();
                    worst = worst.max(off);
                    free_records += 1;
                    free_ok += usize::from(off <= 1.1e-3);
                }
            }
            let ours = steps.iter().find(|s| s.supported).map(|s| s.k);
            let lands_with = ours.zip(land).is_some_and(|(o, t)| o.abs_diff(t) <= 1);
            eprintln!(
                "{name} from record {k}: worst {worst:.2e} m/s while free; lands at ours {ours:?} theirs {land:?}"
            );
            falls += 1;
            held += usize::from(worst <= 1.1e-3);
            landed += usize::from(lands_with);
            if worst > 1.1e-3 || !lands_with {
                parted.push(format!("{name} from {k}"));
            }
            k = until.max(k + 1);
        }
    }
    eprintln!(
        "{falls} falls, {held} within 1.1e-3 m/s while free and {landed} landing within a record of theirs; {free_ok} of {free_records} free records within 1.1e-3 m/s; parted: {parted:#?}"
    );
    assert!(
        falls >= 40 && held >= 31 && landed >= 38,
        "{falls} {held} {landed}"
    );
    assert!(free_ok >= 194, "{free_ok} of {free_records}");
}

/// Eruptor's shape: a 0.8644 m sphere, 0.9144 m with the level mesh's own
/// 0.05 m, centred 0.9144 m up his up axis.
const REACH: f32 = 0.9144;

/// AG.48: AG.39's `air/g39_air_rel` (let go) and `g39_air_rev` (reversed)
/// replays, 2 each, from their second free record: each slides round the
/// open edge under the ledge route (triangle 2920's upper edge, which no
/// other triangle has) and lands within a record of theirs, never hanging
/// in the air (every frame before it lands moves him over 1 mm) and never
/// behind a face of the level, where the mesh's one-sided contacts give
/// nothing; theirs too.
#[test]
#[ignore]
fn ag48_round_the_open_edge_under_the_ledge_route() {
    let bodies = level_213();
    let world = ParryWorld::new(&bodies).unwrap();
    let mesh = mesh_of(&bodies);
    let mut seen = 0;
    for (name, frames) in recordings(AG39) {
        if !(name.starts_with("air/g39_air_rel-") || name.starts_with("air/g39_air_rev-")) {
            continue;
        }
        let k = (3..frames.len())
            .find(|&k| free(&frames[k]) && free(&frames[k - 1]) && !free(&frames[k - 2]))
            .unwrap();
        let land = (k..frames.len())
            .find(|&j| footing(&frames[j]).is_some_and(|f| f.supported))
            .unwrap();
        let steps = run(&world, &frames, k, land + 5);
        let ours = steps.iter().find(|s| s.supported).map(|s| s.k);
        let mut last = Vec3::from(frames[k].hero.position);
        let mut edge = 0;
        for s in steps.iter().take_while(|s| !s.supported) {
            assert!(
                (s.pivot - last).length() > 1e-3,
                "{name}: hung at record {} at {}",
                s.k,
                s.pivot
            );
            last = s.pivot;
            let centre = s.pivot + s.up * REACH;
            assert_eq!(behind_a_face(&mesh, centre, REACH), None, "{name} {}", s.k);
            let corners = mesh.corners(2920).unwrap();
            edge += usize::from(
                (centre - nearest_on_triangle(centre, corners)).length() < REACH + 0.02,
            );
        }
        for f in &frames[k..=land] {
            let up = euler(f.hero.rotation) * Vec3::Y;
            let centre = Vec3::from(f.hero.position) + up * REACH;
            assert_eq!(behind_a_face(&mesh, centre, REACH), None, "theirs: {name}");
        }
        let at = steps.iter().find(|s| s.k == land).unwrap();
        eprintln!(
            "{name} from record {k}: within 2 cm of the overhang for {edge} records; lands at ours {ours:?} theirs {land}, {:.4} m off theirs there",
            (at.pivot - Vec3::from(frames[land].hero.position)).length()
        );
        assert!(edge >= 3, "{name}: never met the overhang's edge");
        assert!(
            ours.is_some_and(|o| o.abs_diff(land) <= 1),
            "{name}: lands at {ours:?}, theirs {land}"
        );
        seen += 1;
    }
    assert_eq!(seen, 4);
}

/// AG.48: let go at the ledge route's west end (AG.39's `g39_w_rel30` W-00
/// and `g39_w_rel60` W-00 and W-01), theirs stop on the overhang's open edge
/// below it, held up by it at 41.6-45.0° below them, and stay. Seeded where
/// theirs first rests there, ours stays held 120 records, supported at the
/// end and within 5 cm of where it started (theirs creep 0-2.2 cm down the
/// edge in that time).
#[test]
#[ignore]
fn ag48_held_on_the_open_edge() {
    let world = level();
    let mut seen = 0;
    for (name, frames) in recordings(AG39) {
        if !matches!(
            name.as_str(),
            "a/g39_w_rel30-W-00" | "a/g39_w_rel60-W-00" | "a/g39_w_rel60-W-01"
        ) {
            continue;
        }
        // Their first record at rest on the edge with two at rest before it.
        let s = (190..frames.len() - 1)
            .find(|&j| {
                (j - 2..=j).all(|i| moved(&frames, i).length() < 1e-4)
                    && footing(&frames[j]).is_some_and(|f| f.supported)
            })
            .unwrap();
        let steps = run(&world, &frames, s, s + 120);
        let from = Vec3::from(frames[s].hero.position);
        let end = steps.last().unwrap();
        let drift = (end.pivot - from).length();
        eprintln!(
            "{name} from record {s}: ours supported {} at record {}, {drift:.4} m from where it started; theirs {:.4} m",
            end.supported,
            end.k,
            (Vec3::from(frames[end.k].hero.position) - from).length()
        );
        assert!(end.supported && drift < 0.05, "{name}: {end:?}");
        seen += 1;
    }
    assert_eq!(seen, 3);
}

/// AG.39's walks off the ledge route (the 48 of 54 that are 0.4 ft up after
/// their third record), free from the start across the edge, which the falls
/// above, seeded in the air, never cross: about the record he's first 0.4 ft
/// up, his pivot rises over 1 cm between two records only where theirs does
/// (the snap sets him down, never up off the crest: clear of the cliff
/// face's plane it lifted him 2.3-5.7 m in 29 of them, AG.16's review), and
/// the record he's next supported.
#[test]
#[ignore]
fn ag39_off_the_edge_free_from_the_start() {
    let world = level();
    let (mut n, mut same_land, mut rose, mut theirs_rose) = (0, 0, Vec::new(), 0);
    for (name, frames) in recordings(AG39) {
        let Some(off) = (3..frames.len()).find(|&k| footing(&frames[k]).is_some_and(|f| f.high()))
        else {
            continue;
        };
        let steps = run(&world, &frames, 2, frames.len());
        let their_land =
            (off..frames.len()).find(|&k| footing(&frames[k]).is_some_and(|f| f.supported));
        let our_land = steps.iter().find(|s| s.k > off && s.supported).map(|s| s.k);
        let (mut ours, mut theirs) = (0.0f32, 0.0f32);
        for w in steps.windows(2) {
            if w[1].k + 8 >= off && w[1].k <= off + 4 {
                ours = ours.max(w[1].pivot.y - w[0].pivot.y);
                let (a, b) = (&frames[w[0].k], &frames[w[1].k]);
                theirs = theirs.max(b.hero.position[1] - a.hero.position[1]);
            }
        }
        eprintln!(
            "{name}: up at record {off}; rises about it ours {ours:.4} m, theirs {theirs:.4} m; next supported ours {our_land:?}, theirs {their_land:?}"
        );
        n += 1;
        theirs_rose += usize::from(theirs > 0.01);
        if ours > 0.01 && theirs <= 0.01 {
            rose.push(name.clone());
        }
        same_land += usize::from(
            our_land
                .zip(their_land)
                .is_some_and(|(a, b)| a.abs_diff(b) <= 1),
        );
    }
    eprintln!(
        "{n} walks off the edge: theirs rising over 1 cm about it in {theirs_rose}, ours where theirs doesn't in {}; next supported within a record of theirs in {same_land}",
        rose.len()
    );
    assert!(n >= 48, "{n}");
    assert!(rose.is_empty(), "{rose:?}");
    assert!(same_land >= 19, "{same_land}");
}

/// The records where a run comes to rest: over 1 mm/s the record before,
/// not over it at the record.
fn stops(speeds: &[(usize, f32)]) -> Vec<usize> {
    speeds
        .windows(2)
        .filter(|w| w[0].1 > 1e-3 && w[1].1 <= 1e-3)
        .map(|w| w[1].0)
        .collect()
}

const AG36: &str = "ag36-edges-1792474678";
const AG9: [&str; 2] = ["ag9-contacts-1792471160", "ag9-contacts-1792472400"];

/// AG.36's edges and AG.9's walls, door, mounds and corridor (55 and 38
/// replays), run free from the start: each recording's first rest (an
/// edge's stop dead in AG.36's, the positioning leg's in AG.9's) against
/// theirs, and over AG.9's wall slides the move along the wall against
/// theirs.
#[test]
#[ignore]
fn ag36_and_ag9_free_from_the_start() {
    let world = level();
    let (mut rests, mut same_record, mut within_8cm) = (0, 0, 0);
    let mut slides = Vec::new();
    for run_name in [AG36, AG9[0], AG9[1]] {
        let mut total = Held::default();
        for (name, frames) in recordings(run_name) {
            let steps = run(&world, &frames, 2, frames.len());
            let h = Held::of(&frames, &steps);
            total.add(&h);
            let ours = stops(
                &steps
                    .iter()
                    .map(|s| (s.k, s.velocity.length()))
                    .collect::<Vec<_>>(),
            );
            let theirs = stops(
                &steps
                    .iter()
                    .map(|s| (s.k, moved(&frames, s.k).length()))
                    .collect::<Vec<_>>(),
            );
            let rest = ours.first().zip(theirs.first()).map(|(&o, &t)| {
                let at = steps.iter().find(|s| s.k == o).unwrap().pivot;
                (o, t, (at - Vec3::from(frames[t].hero.position)).length())
            });
            if let Some((o, t, apart)) = rest {
                rests += 1;
                same_record += usize::from(o == t);
                within_8cm += usize::from(apart <= 0.08);
            }
            // Along a wall: their manifold's own entry a wall within 6 mm
            // over two records running, theirs moving over 1 m/s.
            for s in &steps {
                let wall = |f: &Frame| {
                    f.contacts
                        .as_ref()
                        .and_then(|c| c.planes.first())
                        .is_some_and(|p| p.normal[1] < 0.7 && p.distance < 0.006)
                };
                let theirs = moved(&frames, s.k).length();
                if name.contains("wall")
                    && wall(&frames[s.k])
                    && wall(&frames[s.k - 1])
                    && theirs > 1.0
                {
                    slides.push((s.velocity.length() - theirs).abs());
                }
            }
            eprintln!("{}; first rest ours/theirs/apart {rest:?}", h.line(&name));
        }
        eprintln!("{}", total.line(run_name));
    }
    slides.sort_by(f32::total_cmp);
    let q = |f: f64| slides[((slides.len() - 1) as f64 * f) as usize];
    eprintln!(
        "{rests} rests, {same_record} at theirs' record, {within_8cm} within 8 cm; {} wall records, ours' move off theirs median {:.4} p90 {:.4} max {:.4} m/s",
        slides.len(),
        q(0.5),
        q(0.9),
        q(1.0)
    );
    assert!(
        same_record >= 80 && within_8cm >= 80,
        "{rests} {same_record} {within_8cm}"
    );
}
