//! Does an observer move the run (openspec AG.1)? AF.11's discriminators,
//! the ones that told its set A (observed at the end only) from its set B
//! (paused every 60 flips) where end positions can't (set A's own end spread
//! is 1.48 m wide and splits at the gate's corner), read off a reference group
//! and a candidate group of the same script at the end flip:
//!
//! - MT draws since F0: a pause added about 19 draws idle, and set B sat at
//!   rank-sum z +3.17 idle and +2.65 walking. Agreeing means |z| < 2;
//! - the camera's resting offset behind the hero in x (the hero's x less the
//!   camera's): 3.05-3.23 m in AF.11's set A, 3.33-3.55 m in its set B, no
//!   overlap. Agreeing means every candidate inside the reference's range;
//! - where the hero of the slide outcome stops: set B's stopped 5-7 cm short.
//!   Agreeing means every candidate that slid stopping inside the slides'
//!   range of the reference, in x and in z;
//! - the gate's two outcomes: when the reference holds both, the candidates
//!   hold both.
//!
//! AF.11 found 15 of its 20 walks slid along the corner and stopped at x
//! 20.52-20.64, z -85.12 to -85.22, and 5 turned free and stopped 1.3-1.7 m
//! further toward the bridge; a stop within [`SLIDE_RADIUS`] of
//! [`SLIDE_STOP`] on the ground is a slide, anything else turned free. The
//! walk is AF.11's ([`super::WALK`]); for another script the outcome rows say
//! nothing.

use std::fmt::Write as _;

use serde::Serialize;

use super::report::rank_sum_z;
use super::{Band, ReplayRecord, Set};

/// The middle of AF.11's slide stops, x and z, metres.
pub const SLIDE_STOP: [f64; 2] = [20.58, -85.17];
/// How far from [`SLIDE_STOP`] a slide's stop sits at most: AF.11's 15 lay
/// within 0.07 m of it, the 5 that turned free 1.3 m and more away.
pub const SLIDE_RADIUS: f64 = 0.5;
/// Draws agree under this rank-sum |z|: about a 5 % chance that two groups of
/// one run's replays land past it, where AF.11's pauses put set B at +2.65
/// walking and +3.17 idle.
pub const Z_AGREE: f64 = 2.0;

/// How a replay left the gate's corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Outcome {
    Slide,
    TurnedFree,
}

impl Outcome {
    /// A slide stops by the corner; one that turned free went on toward the
    /// bridge, well past [`SLIDE_RADIUS`].
    pub fn of(x: f64, z: f64) -> Self {
        if (x - SLIDE_STOP[0]).hypot(z - SLIDE_STOP[1]) <= SLIDE_RADIUS {
            Outcome::Slide
        } else {
            Outcome::TurnedFree
        }
    }
}

/// One discriminator over both groups.
#[derive(Debug, Clone, Serialize)]
pub struct Discriminator {
    pub name: String,
    /// Each replay's value, in label order.
    pub reference: Vec<f64>,
    pub candidate: Vec<f64>,
    pub reference_band: Option<Band>,
    pub candidate_band: Option<Band>,
    /// Candidates inside the reference's [min, max].
    pub inside: usize,
    /// Mann-Whitney's z, the candidates against the reference: positive when
    /// the candidates sit higher, `None` when every value is equal.
    pub z: Option<f64>,
    /// What agreeing means for this one, and whether it held.
    pub rule: String,
    pub agrees: bool,
}

impl Discriminator {
    fn new(name: &str, reference: Vec<f64>, candidate: Vec<f64>, by_rank: bool) -> Self {
        let reference_band = Band::of(&reference);
        let inside = reference_band.map_or(0, |b| {
            candidate
                .iter()
                .filter(|&&v| v >= b.min && v <= b.max)
                .count()
        });
        let z = rank_sum_z(&reference, &candidate);
        let (rule, agrees) = if by_rank {
            (
                format!("rank-sum |z| < {Z_AGREE}"),
                z.is_none_or(|z| z.abs() < Z_AGREE) && !candidate.is_empty(),
            )
        } else {
            (
                "every candidate inside the reference's range".to_owned(),
                reference_band.is_some() && inside == candidate.len(),
            )
        };
        Self {
            name: name.to_owned(),
            candidate_band: Band::of(&candidate),
            reference,
            candidate,
            reference_band,
            inside,
            z,
            rule,
            agrees,
        }
    }
}

/// The gate's outcomes on each side.
#[derive(Debug, Clone, Serialize)]
pub struct Outcomes {
    pub reference: Vec<Outcome>,
    pub candidate: Vec<Outcome>,
    /// The candidates hold both outcomes when the reference does.
    pub agrees: bool,
}

/// A candidate group against a reference group at the end flip.
#[derive(Debug, Clone, Serialize)]
pub struct Discriminators {
    pub script: String,
    pub reference: Set,
    pub candidate: Set,
    pub end: u64,
    pub reference_labels: Vec<String>,
    pub candidate_labels: Vec<String>,
    pub draws: Discriminator,
    pub camera_offset: Discriminator,
    /// The slides' stops, x then z.
    pub slide_stop: [Discriminator; 2],
    pub outcomes: Outcomes,
}

/// What one replay's end says, when it has a hero and a camera there.
struct End {
    label: String,
    draws: Option<f64>,
    hero: [f64; 3],
    camera: [f64; 3],
}

fn end_of(r: &ReplayRecord, end: u64) -> Option<End> {
    let at = r.at(end)?;
    let hero = at.hero.as_ref()?.position.map(f64::from);
    let camera = at.camera.as_ref()?.position.map(f64::from);
    let draws = match (at.rng.drawn, r.at(0).and_then(|o| o.rng.drawn)) {
        (Some(e), Some(s)) => Some(e as f64 - s as f64),
        _ => None,
    };
    Some(End {
        label: r.label.clone(),
        draws,
        hero,
        camera,
    })
}

impl Discriminators {
    /// `reference` and `candidate` read at `end`; replays without a hero or a
    /// camera there are left out (and their labels with them).
    pub fn build(
        script: &str,
        reference: (Set, &[&ReplayRecord]),
        candidate: (Set, &[&ReplayRecord]),
        end: u64,
    ) -> Self {
        let ends = |rs: &[&ReplayRecord]| -> Vec<End> {
            let mut v: Vec<End> = rs.iter().filter_map(|r| end_of(r, end)).collect();
            v.sort_by(|a, b| a.label.cmp(&b.label));
            v
        };
        let (re, ce) = (ends(reference.1), ends(candidate.1));
        let col = |v: &[End], f: &dyn Fn(&End) -> Option<f64>| -> Vec<f64> {
            v.iter().filter_map(f).collect()
        };
        let draws = Discriminator::new(
            "MT draws since F0",
            col(&re, &|e| e.draws),
            col(&ce, &|e| e.draws),
            true,
        );
        let offset = |e: &End| Some(e.hero[0] - e.camera[0]);
        let camera_offset = Discriminator::new(
            "camera's offset behind the hero in x (m)",
            col(&re, &offset),
            col(&ce, &offset),
            false,
        );
        let outcome = |e: &End| Outcome::of(e.hero[0], e.hero[2]);
        let slid = |e: &End, i: usize| (outcome(e) == Outcome::Slide).then_some(e.hero[i]);
        let slide_stop = [0, 2].map(|i| {
            Discriminator::new(
                if i == 0 {
                    "slide's stop x (m)"
                } else {
                    "slide's stop z (m)"
                },
                col(&re, &|e| slid(e, i)),
                col(&ce, &|e| slid(e, i)),
                false,
            )
        });
        let (ro, co): (Vec<Outcome>, Vec<Outcome>) = (
            re.iter().map(outcome).collect(),
            ce.iter().map(outcome).collect(),
        );
        let both = |o: &[Outcome]| o.contains(&Outcome::Slide) && o.contains(&Outcome::TurnedFree);
        let outcomes = Outcomes {
            agrees: !both(&ro) || both(&co),
            reference: ro,
            candidate: co,
        };
        Self {
            script: script.to_owned(),
            reference: reference.0,
            candidate: candidate.0,
            end,
            reference_labels: re.iter().map(|e| e.label.clone()).collect(),
            candidate_labels: ce.iter().map(|e| e.label.clone()).collect(),
            draws,
            camera_offset,
            slide_stop,
            outcomes,
        }
    }

    /// The candidates can't be told from the reference on anything AF.11 told
    /// its sets apart by.
    pub fn agree(&self) -> bool {
        self.draws.agrees
            && self.camera_offset.agrees
            && self.slide_stop.iter().all(|d| d.agrees)
            && self.outcomes.agrees
    }

    pub fn write_text(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "== {}: set {} against set {} at F0 + {}, AF.11's discriminators ({} and {} replays) ==",
            self.script,
            self.candidate,
            self.reference,
            self.end,
            self.candidate_labels.len(),
            self.reference_labels.len()
        );
        let band = |b: &Option<Band>| {
            b.map_or("none".into(), |b| {
                format!("{:.4}..{:.4} median {:.4}", b.min, b.max, b.median)
            })
        };
        for d in [&self.draws, &self.camera_offset]
            .into_iter()
            .chain(self.slide_stop.iter())
        {
            let _ = writeln!(
                out,
                "{}: {} {}; {} {}; {} of {} inside {}'s range; rank-sum z {}; {} -> {}",
                d.name,
                self.reference,
                band(&d.reference_band),
                self.candidate,
                band(&d.candidate_band),
                d.inside,
                d.candidate.len(),
                self.reference,
                d.z.map_or("n/a".into(), |z| format!("{z:+.2}")),
                d.rule,
                if d.agrees { "agrees" } else { "MISSES" }
            );
        }
        let count = |o: &[Outcome], k: Outcome| o.iter().filter(|&&x| x == k).count();
        let _ = writeln!(
            out,
            "gate outcomes: {} {} slid, {} turned free; {} {} slid, {} turned free -> {}",
            self.reference,
            count(&self.outcomes.reference, Outcome::Slide),
            count(&self.outcomes.reference, Outcome::TurnedFree),
            self.candidate,
            count(&self.outcomes.candidate, Outcome::Slide),
            count(&self.outcomes.candidate, Outcome::TurnedFree),
            if self.outcomes.agrees {
                "agrees"
            } else {
                "MISSES"
            }
        );
        let _ = writeln!(
            out,
            "verdict: {}\n",
            if self.agree() {
                "every discriminator agrees"
            } else {
                "a discriminator misses"
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::Counts;
    use crate::divergence::{Host, Observation, Pose, RngObservation};

    fn obs(offset: u64, drawn: u64, hero: [f32; 3], camera: [f32; 3]) -> Observation {
        Observation {
            offset,
            flip: offset + 2,
            game_frame: 6_000,
            frame_delta: 0.0167,
            counts: Counts {
                listed: 170,
                actors: 170,
                skinned: 36,
                players: 1,
            },
            hero: Some(Pose {
                position: hero,
                rotation: [0.0, 105.0, 0.0],
                velocity: Some([0.0; 3]),
            }),
            camera: Some(Pose {
                position: camera,
                rotation: [-23.0, 15.0, 0.0],
                velocity: None,
            }),
            rng: RngObservation {
                instances: 1,
                mti: 0,
                seed: Some(10554),
                drawn: Some(drawn),
                anim_steps: Some(0),
                fmod: Some(0),
                state_sha256: String::new(),
            },
            read_ms: 4.0,
        }
    }

    /// A replay ending at `hero` with the camera `offset` behind it in x and
    /// `draws` drawn since F0.
    fn replay(set: Set, i: u32, hero: [f32; 3], offset: f32, draws: u64) -> ReplayRecord {
        let camera = [hero[0] - offset, hero[1] + 8.9, hero[2] - 13.7];
        ReplayRecord {
            label: format!("walk-{set}-{i:02}"),
            script: "walk".into(),
            set,
            replay: i,
            identity: "oracle".into(),
            state: "s".into(),
            state_sha256: "s".into(),
            figure_sha256: "f".into(),
            before: Host {
                load_average: None,
                uptime: String::new(),
                screen_locked: None,
            },
            after: Host {
                load_average: None,
                uptime: String::new(),
                screen_locked: None,
            },
            boot_secs: 5.0,
            f0: 2,
            base: 3,
            run_secs: 30.0,
            observations: vec![
                obs(0, 116_862, [10.19, 43.38, -93.39], [6.5, 52.3, -107.1]),
                obs(1_800, 116_862 + draws, hero, camera),
            ],
            shot: None,
            shots: vec![],
            watch: None,
            saved: None,
            save_refusals: vec![],
            save_error: None,
            silent: true,
            fatal: vec![],
            wall_secs: 60.0,
            shutdown: "Graceful".into(),
        }
    }

    const SLIDE: [f32; 3] = [20.6, 43.52, -85.2];
    const FREE: [f32; 3] = [21.5, 43.73, -86.6];

    /// AF.11's set A in miniature: 7 slides and 3 turned free.
    fn set_a() -> Vec<ReplayRecord> {
        (0..10)
            .map(|i| {
                let mut hero = if [1, 2, 3].contains(&i) { FREE } else { SLIDE };
                hero[0] += 0.01 * i as f32;
                replay(
                    Set::A,
                    i,
                    hero,
                    3.05 + 0.02 * i as f32,
                    32_000 + 300 * u64::from(i),
                )
            })
            .collect()
    }

    /// A candidate ending like set A's replays: its slides and offsets inside
    /// theirs, its draws interleaved with theirs, two turned free.
    fn like(i: u32) -> ([f32; 3], f32, u64) {
        let mut hero = if [0, 5].contains(&i) { FREE } else { SLIDE };
        hero[0] += 0.009 * i as f32 + 0.003;
        (hero, 3.055 + 0.0175 * i as f32, 32_150 + 300 * u64::from(i))
    }

    fn judge(a: &[ReplayRecord], w: &[ReplayRecord]) -> Discriminators {
        let (a, w): (Vec<&ReplayRecord>, Vec<&ReplayRecord>) =
            (a.iter().collect(), w.iter().collect());
        Discriminators::build("walk", (Set::A, &a), (Set::W, &w), 1_800)
    }

    #[test]
    fn a_stop_by_the_slides_is_a_slide_and_one_toward_the_bridge_turned_free() {
        assert_eq!(Outcome::of(20.52, -85.12), Outcome::Slide);
        assert_eq!(Outcome::of(20.64, -85.22), Outcome::Slide);
        // AF.11's turned-free stops.
        assert_eq!(Outcome::of(21.299, -86.284), Outcome::TurnedFree);
        assert_eq!(Outcome::of(21.551, -86.669), Outcome::TurnedFree);
    }

    #[test]
    fn a_candidate_like_the_reference_agrees_on_every_discriminator() {
        let a = set_a();
        // Interleaved draws, offsets inside A's, slides inside A's, both outcomes.
        let w: Vec<ReplayRecord> = (0..10)
            .map(|i| {
                let (hero, offset, draws) = like(i);
                replay(Set::W, i, hero, offset, draws)
            })
            .collect();
        let d = judge(&a, &w);
        assert!(d.agree(), "{d:#?}");
        assert_eq!(d.camera_offset.inside, 10);
        assert!(d.draws.z.unwrap().abs() < 0.5);
        assert_eq!(d.slide_stop[0].candidate.len(), 8);
        let mut text = String::new();
        d.write_text(&mut text);
        assert!(text.contains("every discriminator agrees"), "{text}");
    }

    #[test]
    fn each_discriminator_misses_on_its_own() {
        let a = set_a();
        let build = |f: &dyn Fn(u32) -> ([f32; 3], f32, u64)| -> Vec<ReplayRecord> {
            (0..10)
                .map(|i| {
                    let (h, o, d) = f(i);
                    replay(Set::W, i, h, o, d)
                })
                .collect()
        };
        // More draws, as a pause every 60 flips adds: z past 2.
        let d = judge(
            &a,
            &build(&|i| {
                let (h, o, d) = like(i);
                (h, o, d + 3_000)
            }),
        );
        assert!(!d.draws.agrees && d.draws.z.unwrap() > 2.0);
        assert!(d.camera_offset.agrees && d.outcomes.agrees);
        // The camera resting further back, as set B's did: one out is a miss.
        let d = judge(
            &a,
            &build(&|i| {
                let (h, o, d) = like(i);
                (h, if i == 4 { 3.4 } else { o }, d)
            }),
        );
        assert!(!d.camera_offset.agrees && d.camera_offset.inside == 9);
        assert!(d.draws.agrees);
        // A slide stopping 6 cm short in z.
        let d = judge(
            &a,
            &build(&|i| {
                let (mut h, o, d) = like(i);
                if i == 7 {
                    h[2] += 0.06;
                }
                (h, o, d)
            }),
        );
        assert!(d.slide_stop[0].agrees && !d.slide_stop[1].agrees);
        // Only slides where the reference has both.
        let d = judge(
            &a,
            &build(&|i| {
                let (_, o, d) = like(i);
                (SLIDE, o, d)
            }),
        );
        assert!(!d.outcomes.agrees && !d.agree());
        // A reference with one outcome asks nothing of the candidates' outcomes.
        let slides: Vec<ReplayRecord> = (0..10)
            .map(|i| replay(Set::A, i, SLIDE, 3.1, 32_000 + u64::from(i)))
            .collect();
        let d = judge(&slides, &build(&|i| (SLIDE, 3.1, 32_000 + u64::from(i))));
        assert!(d.outcomes.agrees);
        // A replay without a camera at the end is left out.
        let mut w = build(&like);
        w[3].observations[1].camera = None;
        assert_eq!(judge(&a, &w).candidate_labels.len(), 9);
    }
    #[test]
    fn the_report_judges_set_w_against_set_a_and_nothing_else() {
        let mut records = set_a();
        records.extend((0..10).map(|i| {
            let (hero, offset, draws) = like(i);
            replay(Set::W, i, hero, offset, draws)
        }));
        let report = crate::divergence::Report::build(&records, None).unwrap();
        assert_eq!(report.observing.len(), 1);
        let o = &report.observing[0];
        assert_eq!((o.reference, o.candidate, o.end), (Set::A, Set::W, 1_800));
        assert!(o.agree());
        assert!(report.text().contains("set W against set A at F0 + 1800"));
        // No set W, no judging.
        let report = crate::divergence::Report::build(&set_a(), None).unwrap();
        assert!(report.observing.is_empty());
    }
}
